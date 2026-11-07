//! The translation pipeline: snapshot text in, per-cell results out, with no workbook access.
//!
//! Order matters and is deliberate:
//! 1. skip/limit rules and source detection per cell;
//! 2. protect numbers, dates, URLs, e-mails and placeholders;
//! 3. segment the masked text by line and sentence;
//! 4. de-duplicate and batch by token budget, grouped by source language;
//! 5. infer with the injected backend;
//! 6. restore protection markers and verify them;
//! 7. join the pieces back into one string per cell.
//!
//! Cancellation and timeouts abort the whole run: the caller inserts only a completed report.

use std::collections::BTreeMap;

use crate::backend::{Backend, BatchRequest, Cancel};
use crate::batch::{BatchPlan, Limits, plan};
use crate::detect::detect;
use crate::protect::{ProtectError, Protected, Token, protect, restore_piece};
use crate::segment::{SegmentKind, segment};
use crate::skip::SkipReason;
use crate::tag::LangTag;
use private::{CellPlan, Job, PlannedCell};

/// One cell of text to translate, in selection order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellInput {
    pub index: usize,
    pub text: String,
}

/// Per-cell outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum CellStatus {
    Translated { from: LangTag, confidence: Option<f32> },
    Skipped(SkipReason),
    Failed(Failure),
}

impl CellStatus {
    /// Stable JSON kind: `translated`, `skipped` or `failed`.
    pub fn kind(&self) -> &'static str {
        match self {
            CellStatus::Translated { .. } => "translated",
            CellStatus::Skipped(_) => "skipped",
            CellStatus::Failed(_) => "failed",
        }
    }
}

/// A per-cell failure. Messages never include the cell's text.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    LowConfidence { detected: Option<LangTag>, confidence: f32 },
    UnsupportedPair,
    MalformedPlaceholder,
    PlaceholderLost,
    TooLong,
    EmptyOutput,
    Backend(String),
    Cancelled,
    Timeout,
}

impl Failure {
    /// Stable JSON code for the failure.
    pub fn code(&self) -> &'static str {
        match self {
            Failure::LowConfidence { .. } => "lowConfidence",
            Failure::UnsupportedPair => "unsupportedPair",
            Failure::MalformedPlaceholder => "malformedPlaceholder",
            Failure::PlaceholderLost => "placeholderLost",
            Failure::TooLong => "tooLong",
            Failure::EmptyOutput => "emptyOutput",
            Failure::Backend(_) => "backend",
            Failure::Cancelled => "cancelled",
            Failure::Timeout => "timeout",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellResult {
    pub index: usize,
    pub status: CellStatus,
    pub text: Option<String>,
}

/// What one request asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// `None` means auto-detect.
    pub from: Option<LangTag>,
    pub to: LangTag,
    pub limits: Limits,
    /// Below this the auto detector asks for a source language instead of guessing.
    pub detect_threshold: f32,
}

impl Options {
    pub fn new(to: LangTag) -> Options {
        Options { from: None, to, limits: crate::batch::DEFAULT_LIMITS, detect_threshold: 0.5 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub cells: usize,
    pub translated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub chars: usize,
    pub segments: usize,
    pub batches: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub cells: Vec<CellResult>,
    pub stats: Stats,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub done_batches: usize,
    pub total_batches: usize,
    pub done_segments: usize,
    pub total_segments: usize,
}

/// Runs the pipeline. `progress` is called between model calls; `cancel` is checked before every
/// batch and honoured by the backend between sentences and tokens.
pub fn run<F: FnMut(Progress)>(
    backend: &dyn Backend,
    cells: &[CellInput],
    opts: &Options,
    cancel: &Cancel,
    mut progress: F,
) -> Result<Report, crate::error::TranslateError> {
    private::validate(cells, opts)?;
    let mut stats = Stats { cells: cells.len(), chars: cells.iter().map(|c| c.text.chars().count()).sum(), ..Stats::default() };
    let mut results: Vec<Option<CellResult>> = vec![None; cells.len()];
    let mut planned: Vec<Option<PlannedCell>> = (0..cells.len()).map(|_| None).collect();
    let mut jobs: Vec<Job> = Vec::new();
    for (slot, cell) in cells.iter().enumerate() {
        match private::plan_cell(cell, opts, backend) {
            CellPlan::Skipped(reason) => {
                stats.skipped += 1;
                results[slot] = Some(CellResult { index: cell.index, status: CellStatus::Skipped(reason), text: None });
            }
            CellPlan::Failed(failure) => {
                stats.failed += 1;
                results[slot] = Some(CellResult { index: cell.index, status: CellStatus::Failed(failure), text: None });
            }
            CellPlan::Planned(planned_cell) => {
                stats.segments += planned_cell.text_segments();
                for (seg_index, text) in planned_cell.text_pieces() {
                    jobs.push(Job { slot, segment: seg_index, from: planned_cell.from.clone(), text });
                }
                planned[slot] = Some(planned_cell);
            }
        }
    }
    // Group jobs by source language; one batch call is always one direction.
    let mut groups: BTreeMap<String, (LangTag, Vec<usize>)> = BTreeMap::new();
    for (job_index, job) in jobs.iter().enumerate() {
        groups.entry(job.from.canonical()).or_insert_with(|| (job.from.clone(), Vec::new())).1.push(job_index);
    }
    let mut total_batches = 0usize;
    for (_, indexes) in groups.values() {
        let texts: Vec<String> = indexes.iter().map(|i| jobs[*i].text.clone()).collect();
        let group_limits = Limits { max_cells: texts.len().max(1), ..opts.limits };
        total_batches += plan(&texts, &group_limits)?.batches.len();
    }
    let mut done_batches = 0usize;
    let mut done_segments = 0usize;
    for (from, indexes) in groups.values() {
        let texts: Vec<String> = indexes.iter().map(|i| jobs[*i].text.clone()).collect();
        let group_limits = Limits { max_cells: texts.len().max(1), ..opts.limits };
        let plan = plan(&texts, &group_limits)?;
        let translations = match private::translate_group(backend, from, &opts.to, &plan, cancel)? {
            Ok(translations) => translations,
            Err(failure) => {
                for &job_index in indexes {
                    let slot = jobs[job_index].slot;
                    if let Some(p) = planned[slot].as_mut() {
                        p.failed = Some(failure.clone());
                    }
                }
                continue;
            }
        };
        for (job_pos, &job_index) in indexes.iter().enumerate() {
            let translated = translations.get(plan.mapping[job_pos]).cloned().unwrap_or_default();
            let job = &jobs[job_index];
            if let Some(p) = planned[job.slot].as_mut() {
                p.segments[job.segment].translated = Some(translated);
            }
        }
        done_segments += indexes.len();
        done_batches += plan.batches.len();
        progress(Progress { done_batches, total_batches, done_segments, total_segments: jobs.len() });
    }
    stats.batches = total_batches;
    for (slot, p) in planned.into_iter().enumerate() {
        let Some(p) = p else { continue };
        let index = cells[slot].index;
        match private::finish_cell(p) {
            Ok(text) => {
                stats.translated += 1;
                let (from, confidence) = private::status_language(&cells[slot], opts)?;
                results[slot] = Some(CellResult { index, status: CellStatus::Translated { from, confidence }, text: Some(text) });
            }
            Err(failure) => {
                stats.failed += 1;
                results[slot] = Some(CellResult { index, status: CellStatus::Failed(failure), text: None });
            }
        }
    }
    let cells_out: Vec<CellResult> = results
        .into_iter()
        .enumerate()
        .map(|(slot, r)| r.unwrap_or_else(|| CellResult { index: cells[slot].index, status: CellStatus::Failed(Failure::EmptyOutput), text: None }))
        .collect();
    Ok(Report { cells: cells_out, stats })
}

mod private {
    use super::{
        Backend, BatchPlan, BatchRequest, Cancel, CellInput, Failure, LangTag, Options, ProtectError, Protected, SegmentKind, SkipReason, Token,
        detect, protect, restore_piece, segment,
    };

    pub struct PlannedCell {
        pub from: LangTag,
        pub tokens: Vec<Token>,
        pub segments: Vec<PlannedSegment>,
        pub failed: Option<Failure>,
    }

    pub struct PlannedSegment {
        pub literal: Option<String>,
        pub masked_core: String,
        pub source_piece: String,
        pub suffix: String,
        pub translated: Option<String>,
    }

    impl PlannedCell {
        pub fn text_segments(&self) -> usize {
            self.segments.iter().filter(|s| s.literal.is_none()).count()
        }
        pub fn text_pieces(&self) -> Vec<(usize, String)> {
            self.segments.iter().enumerate().filter(|(_, s)| s.literal.is_none()).map(|(i, s)| (i, s.masked_core.clone())).collect()
        }
    }

    pub struct Job {
        pub slot: usize,
        pub segment: usize,
        pub from: LangTag,
        pub text: String,
    }

    pub enum CellPlan {
        Planned(PlannedCell),
        Skipped(SkipReason),
        Failed(Failure),
    }

    pub fn validate(cells: &[CellInput], opts: &Options) -> Result<(), crate::error::TranslateError> {
        if cells.len() > opts.limits.max_cells {
            return Err(crate::error::TranslateError::TooManyCells { count: cells.len(), limit: opts.limits.max_cells });
        }
        let mut chars = 0usize;
        for (index, cell) in cells.iter().enumerate() {
            let n = cell.text.chars().count();
            if n > opts.limits.max_cell_chars {
                return Err(crate::error::TranslateError::CellTooLong { index, count: n, limit: opts.limits.max_cell_chars });
            }
            chars = chars.saturating_add(n);
            if chars > opts.limits.max_chars {
                return Err(crate::error::TranslateError::TooManyChars { count: chars, limit: opts.limits.max_chars });
            }
        }
        Ok(())
    }

    pub fn plan_cell(cell: &CellInput, opts: &Options, backend: &dyn Backend) -> CellPlan {
        if cell.text.trim().is_empty() {
            return CellPlan::Skipped(SkipReason::Empty);
        }
        if cell.text.chars().count() > opts.limits.max_cell_chars {
            return CellPlan::Skipped(SkipReason::TooLong);
        }
        let (from, _confidence) = match &opts.from {
            Some(from) => (from.clone(), None),
            None => {
                let d = detect(&cell.text);
                if d.language.language() == "und" || d.confidence < opts.detect_threshold {
                    return CellPlan::Failed(Failure::LowConfidence { detected: Some(d.language), confidence: d.confidence });
                }
                (d.language, Some(d.confidence))
            }
        };
        if !backend.supports(&from, &opts.to) {
            return CellPlan::Failed(Failure::UnsupportedPair);
        }
        let Protected { masked, tokens } = match protect(&cell.text) {
            Ok(p) => p,
            Err(ProtectError::MalformedPlaceholder { .. }) => return CellPlan::Failed(Failure::MalformedPlaceholder),
            Err(_) => return CellPlan::Failed(Failure::MalformedPlaceholder),
        };
        let mut segments = Vec::new();
        for s in segment(&masked) {
            match s.kind {
                SegmentKind::Text => {
                    let core = s.text.trim_end();
                    let suffix = &s.text[core.len()..];
                    if core.is_empty() {
                        segments.push(PlannedSegment {
                            literal: Some(s.text.to_string()),
                            masked_core: String::new(),
                            source_piece: String::new(),
                            suffix: String::new(),
                            translated: None,
                        });
                    } else {
                        segments.push(PlannedSegment {
                            literal: None,
                            masked_core: core.to_string(),
                            source_piece: s.text.to_string(),
                            suffix: suffix.to_string(),
                            translated: None,
                        });
                    }
                }
                SegmentKind::Whitespace | SegmentKind::Newline => segments.push(PlannedSegment {
                    literal: Some(s.text.to_string()),
                    masked_core: String::new(),
                    source_piece: String::new(),
                    suffix: String::new(),
                    translated: None,
                }),
            }
        }
        if !segments.iter().any(|s| s.literal.is_none()) {
            return CellPlan::Skipped(SkipReason::Empty);
        }
        CellPlan::Planned(PlannedCell { from, tokens, segments, failed: None })
    }

    /// `Err(failure)` marks every cell in the group failed (an unsupported pair). A model or
    /// cancellation error is returned to abort the whole run.
    pub fn translate_group(
        backend: &dyn Backend,
        from: &LangTag,
        to: &LangTag,
        plan: &BatchPlan,
        cancel: &Cancel,
    ) -> Result<Result<Vec<String>, Failure>, crate::error::TranslateError> {
        let mut out = vec![String::new(); plan.unique.len()];
        for batch in &plan.batches {
            cancel.check()?;
            let texts: Vec<String> = batch.iter().map(|&i| plan.unique[i].clone()).collect();
            match backend.translate(&BatchRequest { from, to, texts: &texts }, cancel) {
                Ok(translated) => {
                    if translated.len() != texts.len() {
                        return Err(crate::error::TranslateError::Backend(format!(
                            "the model returned {} results for {} sentences",
                            translated.len(),
                            texts.len()
                        )));
                    }
                    for (&index, text) in batch.iter().zip(translated) {
                        out[index] = text;
                    }
                }
                Err(crate::backend::BackendError::UnsupportedPair { .. }) => return Ok(Err(Failure::UnsupportedPair)),
                Err(crate::backend::BackendError::Cancelled) => return Err(crate::error::TranslateError::Cancelled),
                Err(crate::backend::BackendError::Timeout) => return Err(crate::error::TranslateError::Timeout),
                Err(e) => return Err(crate::error::TranslateError::Backend(e.to_string())),
            }
        }
        Ok(Ok(out))
    }

    pub fn finish_cell(cell: PlannedCell) -> Result<String, Failure> {
        if let Some(failure) = &cell.failed {
            return Err(failure.clone());
        }
        let mut out = String::new();
        for segment in &cell.segments {
            if let Some(literal) = &segment.literal {
                out.push_str(literal);
                continue;
            }
            let Some(translated) = &segment.translated else { return Err(Failure::EmptyOutput) };
            let restored = match restore_piece(translated, &segment.source_piece, &cell.tokens) {
                Ok(text) => text,
                Err(_) => return Err(Failure::PlaceholderLost),
            };
            if restored.trim().is_empty() {
                return Err(Failure::EmptyOutput);
            }
            out.push_str(&restored);
            out.push_str(&segment.suffix);
        }
        if out.trim().is_empty() {
            return Err(Failure::EmptyOutput);
        }
        Ok(out)
    }

    pub fn status_language(cell: &CellInput, opts: &Options) -> Result<(LangTag, Option<f32>), crate::error::TranslateError> {
        match &opts.from {
            Some(from) => Ok((from.clone(), None)),
            None => {
                let d = detect(&cell.text);
                Ok((d.language, Some(d.confidence)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CellInput, CellStatus, Failure, Options, Report, run};
    use crate::backend::{Cancel, CancelReason};
    use crate::tag::LangTag;
    use crate::test_backend::FixedBackend;

    fn lang(code: &str) -> LangTag {
        LangTag::parse(code).expect("tag")
    }

    fn run_cells(texts: &[&str], from: Option<&str>) -> Report {
        let cells: Vec<CellInput> = texts.iter().enumerate().map(|(i, t)| CellInput { index: i, text: (*t).to_string() }).collect();
        let mut opts = Options::new(lang("es"));
        opts.from = from.map(lang);
        run(&FixedBackend, &cells, &opts, &Cancel::new(), |_| {}).expect("run")
    }

    #[test]
    fn translates_cells_and_preserves_numbers_and_placeholders() {
        let report = run_cells(&["Hello, world! The total is 42 dollars.", "Order {0} ships 2026-12-01."], Some("en"));
        assert_eq!(report.stats.translated, 2);
        assert_eq!(report.cells[0].text.as_deref(), Some("Hola, mundo! El total es 42 dólares."));
        let second = report.cells[1].text.as_deref().unwrap_or_default();
        assert!(second.contains("42") || second.contains("2026-12-01"), "{second}");
        assert!(second.contains("{0}"), "{second}");
    }

    #[test]
    fn translates_each_sentence_and_keeps_newlines() {
        let report = run_cells(&["Hello. World.\nHello."], Some("en"));
        assert_eq!(report.cells[0].text.as_deref(), Some("Hola. Mundo.\nHola."));
    }

    #[test]
    fn deduplicates_repeated_text() {
        let report = run_cells(&["Hello", "Hello", "World"], Some("en"));
        assert_eq!(report.stats.translated, 3);
        assert_eq!(report.cells[0].text.as_deref(), Some("Hola"));
        assert_eq!(report.cells[2].text.as_deref(), Some("Mundo"));
    }

    #[test]
    fn auto_detection_refuses_an_ambiguous_short_cell() {
        let report = run_cells(&["No."], None);
        assert_eq!(report.cells[0].status.kind(), "failed");
        assert!(matches!(report.cells[0].status, CellStatus::Failed(Failure::LowConfidence { .. })));
    }

    #[test]
    fn auto_detection_works_for_a_clear_sentence() {
        let report = run_cells(&["The total is 42 dollars and the report is ready."], None);
        assert_eq!(report.cells[0].status.kind(), "translated");
    }

    #[test]
    fn unsupported_pair_fails_the_cell_without_writing() {
        let cells = vec![CellInput { index: 0, text: "Hello".into() }];
        let mut opts = Options::new(lang("de"));
        opts.from = Some(lang("en"));
        let report = run(&FixedBackend, &cells, &opts, &Cancel::new(), |_| {}).expect("run");
        assert!(matches!(report.cells[0].status, CellStatus::Failed(Failure::UnsupportedPair)));
        assert!(report.cells[0].text.is_none());
    }

    #[test]
    fn malformed_placeholder_fails_the_cell() {
        let report = run_cells(&["Hello {name"], Some("en"));
        assert!(matches!(report.cells[0].status, CellStatus::Failed(Failure::MalformedPlaceholder)));
    }

    #[test]
    fn cancellation_aborts_the_run() {
        let cells = vec![CellInput { index: 0, text: "Hello".into() }];
        let cancel = Cancel::new();
        cancel.cancel_with(CancelReason::Timeout);
        let opts = Options { from: Some(lang("en")), ..Options::new(lang("es")) };
        assert!(matches!(run(&FixedBackend, &cells, &opts, &cancel, |_| {}), Err(crate::error::TranslateError::Timeout)));
    }

    #[test]
    fn limits_are_checked_before_any_batch() {
        let cells = vec![CellInput { index: 0, text: "Hello".into() }];
        let mut opts = Options { from: Some(lang("en")), ..Options::new(lang("es")) };
        opts.limits.max_chars = 2;
        assert!(matches!(run(&FixedBackend, &cells, &opts, &Cancel::new(), |_| {}), Err(crate::error::TranslateError::TooManyChars { .. })));
    }
}
