//! What is wrong in the code being edited, for one editor: the static checks of the text (`lint`)
//! and the cell's last run. Checks run once per edit, whatever the frame rate, and every span comes
//! from the current text: a run's line is only used while the text is what ran, and marks are in
//! chars, so one left from before this frame's edit can't split a character.

use std::ops::Range;
use std::sync::Arc;

use gridcraft_engine::core::CellRef;
use gridcraft_engine::python::{Diagnostic, Finding, Kind, Severity, lint};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::code_editor::{Mark, MarkLevel};
use crate::i18n::Language;

/// One thing to say about the code, for the problems strip and `ui.inspect`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: &'static Kind,
    pub level: MarkLevel,
    /// The 1-based line, when it still refers to the text.
    pub line: Option<u32>,
    /// Byte offsets into the text, on char boundaries.
    pub span: Option<Range<usize>>,
    /// Where the caret goes for this entry (a char index).
    pub char_at: Option<usize>,
    pub detail: String,
}

/// The checks of the text in one editor.
#[derive(Default)]
pub struct CodeChecks {
    /// What the findings are for.
    text: String,
    generation: u64,
    language: Option<Language>,
    run: Option<Diagnostic>,
    run_is_current: bool,
    findings: Vec<Finding>,
    pub marks: Vec<Mark>,
    pub entries: Vec<Entry>,
    /// How many times the code was linted, for tests that it isn't every frame.
    lints: u64,
}

fn level(severity: Severity) -> MarkLevel {
    match severity {
        Severity::Blocked | Severity::Error => MarkLevel::Error,
        Severity::Warning => MarkLevel::Warning,
        Severity::Info => MarkLevel::Info,
    }
}

/// `line` of `text` (1-based), without its indentation or line break: a byte range on char
/// boundaries, or `None` when the text has no such line or nothing on it.
pub fn line_span(text: &str, line: u32) -> Option<Range<usize>> {
    let mut start = 0;
    for (index, piece) in text.split_inclusive('\n').enumerate() {
        if u32::try_from(index).ok()?.saturating_add(1) == line {
            let trimmed = piece.trim_end();
            let body = trimmed.trim_start();
            return (!body.is_empty()).then(|| {
                let at = start + (trimmed.len() - body.len());
                at..at + body.len()
            });
        }
        start += piece.len();
    }
    None
}

/// The char index of a byte offset, or the end of the text for one past it.
fn char_of(text: &str, byte: usize) -> usize {
    text.get(..byte).map_or_else(|| text.chars().count(), |s| s.chars().count())
}

impl CodeChecks {
    /// Brings the checks up to date with `text`: lints only when it or the package list changed.
    /// `installed` is `None` while the list isn't known. `run` is what the cell's last run said, and
    /// `run_is_current` whether the text is the code that ran.
    pub fn update(
        &mut self,
        text: &str,
        installed: Option<&[String]>,
        generation: u64,
        language: Language,
        run: Option<&Diagnostic>,
        run_is_current: bool,
    ) {
        let relint = self.language.is_none() || self.text != text || self.generation != generation;
        if relint {
            self.findings = lint(text, installed);
            self.lints = self.lints.saturating_add(1);
            self.text.clear();
            self.text.push_str(text);
            self.generation = generation;
        }
        if !relint && self.language == Some(language) && self.run.as_ref() == run && self.run_is_current == run_is_current {
            return;
        }
        self.language = Some(language);
        self.run = run.cloned();
        self.run_is_current = run_is_current;
        self.rebuild(language);
    }

    fn rebuild(&mut self, language: Language) {
        let text = self.text.as_str();
        let tip = |kind: &Kind, detail: &str| {
            let message = language.tr(kind.message);
            if detail.is_empty() { message.to_string() } else { format!("{message}\n{detail}") }
        };
        let mut marks = Vec::with_capacity(self.findings.len() + 1);
        let mut entries = Vec::with_capacity(self.findings.len() + 1);
        if let Some(d) = &self.run {
            // A run's line means the line of the code that ran.
            let line = d.line.filter(|_| self.run_is_current);
            let span = line.and_then(|line| line_span(text, line));
            let chars = span.as_ref().map(|s| char_of(text, s.start)..char_of(text, s.end));
            if let (Some(line), Some(chars)) = (line, chars.clone()) {
                marks.push(Mark { chars, line, level: level(d.kind.severity), tip: tip(d.kind, &d.detail) });
            }
            let char_at = chars.map(|c| c.start);
            entries.push(Entry { kind: d.kind, level: level(d.kind.severity), line, span, char_at, detail: d.detail.clone() });
        }
        for f in &self.findings {
            let chars = char_of(text, f.start)..char_of(text, f.end);
            let entry_level = level(f.kind.severity);
            marks.push(Mark { chars: chars.clone(), line: f.line, level: entry_level, tip: tip(f.kind, &f.detail) });
            // The last run already said this about this line: the underline stays, the list says it once.
            let said = self.run.as_ref().filter(|_| self.run_is_current).is_some_and(|d| d.line == Some(f.line) && restates(d.kind.id, f.kind.id));
            if said {
                continue;
            }
            entries.push(Entry {
                kind: f.kind,
                level: entry_level,
                line: Some(f.line),
                span: Some(f.start..f.end),
                char_at: Some(chars.start),
                detail: f.detail.clone(),
            });
        }
        self.marks = marks;
        self.entries = entries;
    }

    /// What `ui.inspect` reports.
    pub fn json(&self) -> Json {
        Json::Array(
            self.entries
                .iter()
                .map(|e| {
                    json!({
                        "kind": e.kind.id,
                        "severity": e.kind.severity.id(),
                        "message": e.kind.message,
                        "detail": e.detail,
                        "line": e.line,
                        "start": e.span.as_ref().map(|s| s.start),
                        "end": e.span.as_ref().map(|s| s.end),
                    })
                })
                .collect(),
        )
    }

    /// How many times the code was linted.
    #[doc(hidden)]
    pub fn lints(&self) -> u64 {
        self.lints
    }
}

/// The names of the packages this Python has, or `None` while the list isn't known.
fn installed(app: &mut SheetApp) -> (Option<Arc<Vec<String>>>, u64) {
    let generation = app.session.python_packages_generation();
    if app.python.installed.as_ref().is_none_or(|(g, _)| *g != generation) {
        let packages = &app.session.python_report().packages;
        let names = (!packages.is_empty()).then(|| Arc::new(packages.iter().map(|(name, _)| name.clone()).collect()));
        app.python.installed = Some((generation, names));
    }
    (app.python.installed.as_ref().and_then(|(_, names)| names.clone()), generation)
}

/// What the cell's last run said, and whether `text` is the code that ran.
fn run_of(app: &SheetApp, doc: u64, sheet: Option<usize>, cell: CellRef, text: &str) -> (Option<Diagnostic>, bool) {
    let Some(d) = app.session.active().filter(|d| d.uid == doc) else { return (None, false) };
    let Some(sheet) = sheet else { return (None, false) };
    let current = d.wb.sheet(sheet).and_then(|sh| sh.cell(cell)).and_then(|c| c.py.as_deref()).is_some_and(|py| py.code == text);
    // The line of a failure of the initialization code is in that code, not this.
    let run = app
        .session
        .python_diagnostic(sheet, cell)
        .map(|x| if x.kind.id == "init.failed" { Diagnostic { line: None, ..x.clone() } } else { x.clone() });
    (run, current)
}

/// Brings `checks` up to date with code being edited for `cell` of workbook `doc`.
pub(crate) fn update(app: &mut SheetApp, checks: &mut CodeChecks, text: &str, doc: u64, sheet: Option<usize>, cell: CellRef) {
    let (names, generation) = installed(app);
    let (run, current) = run_of(app, doc, sheet, cell, text);
    checks.update(text, names.as_ref().map(|n| n.as_slice()), generation, app.ui.language, run.as_ref(), current);
}

/// Brings `checks` up to date with initialization code being edited for workbook `doc`: its checks, and
/// its failure, whose line is only used while the text is the code that failed.
pub(crate) fn update_initialization(app: &mut SheetApp, checks: &mut CodeChecks, text: &str, doc: u64) {
    let (names, generation) = installed(app);
    let d = app.session.active().filter(|d| d.uid == doc);
    let current = d.is_some_and(|d| d.wb.python_env.as_ref().is_some_and(|e| e.code == text));
    let run = d.and_then(|_| app.session.python_init_failure()).cloned();
    checks.update(text, names.as_ref().map(|n| n.as_slice()), generation, app.ui.language, run.as_ref(), current);
}

/// Whether a check says what a run's error of kind `run` already says.
fn restates(run: &str, check: &str) -> bool {
    matches!(
        (run, check),
        ("module.missing", "lint.importMissing")
            | ("denied.file", "lint.fileAccess" | "lint.savefig")
            | ("denied.network" | "denied.process", "lint.importBlocked")
            | ("xl.notLiteral", "lint.xlNotLiteral")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_span_stays_on_char_boundaries_and_leaves_out_the_indent() {
        let text = "é = 1\n    naïve = 2  \n\nlast";
        assert_eq!(line_span(text, 1).map(|r| &text[r]), Some("é = 1"));
        assert_eq!(line_span(text, 2).map(|r| &text[r]), Some("naïve = 2"));
        assert_eq!(line_span(text, 3), None, "nothing on it");
        assert_eq!(line_span(text, 4).map(|r| &text[r]), Some("last"));
        assert_eq!(line_span(text, 5), None, "no such line");
        assert_eq!(line_span("", 1), None);
        assert_eq!(line_span("x", 0), None);
    }

    #[test]
    fn linting_happens_once_per_text_and_marks_follow_the_text() {
        let mut checks = CodeChecks::default();
        let installed = ["pandas".to_string()];
        let code = "import socket\n";
        for _ in 0..5 {
            checks.update(code, Some(&installed), 1, Language::En, None, false);
        }
        assert_eq!(checks.lints(), 1, "frames don't lint");
        assert_eq!(checks.marks[0].chars, 7..13);
        // Editing above it with a multi-byte character moves the bytes and the chars differently.
        let edited = "é = 1\nimport socket\n";
        checks.update(edited, Some(&installed), 1, Language::En, None, false);
        assert_eq!(checks.lints(), 2);
        assert_eq!(checks.marks[0].line, 2);
        assert_eq!(checks.marks[0].chars, 13..19);
        let span = checks.entries[0].span.clone().unwrap();
        assert_eq!(&edited[span], "socket");
        // A new package list or nothing else makes it lint again.
        checks.update(edited, Some(&installed), 2, Language::En, None, false);
        assert_eq!(checks.lints(), 3);
        checks.update(edited, Some(&installed), 2, Language::Ja, None, false);
        assert_eq!(checks.lints(), 3, "a language only rewords the tips");
    }

    #[test]
    fn a_run_s_line_is_only_used_while_the_text_is_what_ran() {
        let d = Diagnostic::new("python.exception", "ValueError: boom", Some(2));
        let mut checks = CodeChecks::default();
        checks.update("x = 1\nfail()\n", None, 1, Language::En, Some(&d), true);
        assert_eq!(checks.marks.len(), 1);
        assert_eq!((checks.marks[0].line, checks.marks[0].chars.clone()), (2, 6..12));
        assert_eq!(checks.entries[0].line, Some(2));
        // The text changed under it: the entry stays, without a place in the code.
        checks.update("é\nx = 1\nfail()\n", None, 1, Language::En, Some(&d), false);
        assert!(checks.marks.is_empty());
        assert_eq!((checks.entries[0].line, checks.entries[0].char_at), (None, None));
        // A line past the end isn't a panic.
        let far = Diagnostic::new("python.exception", "", Some(90));
        checks.update("x\n", None, 1, Language::En, Some(&far), true);
        assert!(checks.marks.is_empty());
    }

    #[test]
    fn no_list_reports_nothing_missing() {
        let mut checks = CodeChecks::default();
        checks.update("import nope\n", None, 1, Language::En, None, false);
        assert!(checks.entries.is_empty());
        checks.update("import nope\n", Some(&["pandas".to_string()]), 2, Language::En, None, false);
        assert_eq!(checks.entries[0].kind.id, "lint.importMissing");
    }

    #[test]
    fn a_check_that_repeats_the_last_run_is_listed_once() {
        let mut checks = CodeChecks::default();
        let run = Diagnostic::new("module.missing", "No module named 'x'", Some(1));
        let installed: Vec<String> = Vec::new();
        checks.update("import x", Some(&installed), 1, Language::En, Some(&run), true);
        let ids: Vec<&str> = checks.entries.iter().map(|e| e.kind.id).collect();
        assert_eq!(ids, ["module.missing"]);
        assert_eq!(checks.marks.len(), 2, "the underline stays");
        // Code changed since the run: the run's line says nothing about this line.
        checks.update("import x", Some(&installed), 1, Language::En, Some(&run), false);
        assert_eq!(checks.entries.len(), 2);
    }
}
