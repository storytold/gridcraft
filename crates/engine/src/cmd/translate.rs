//! Review › Language › Translate: fully local cell translation.
//!
//! The synchronous command path never runs inference. `translate.start` snapshots the selection
//! (cell text and kind, not the workbook), starts a native worker thread and returns a job id;
//! the pane, the control channel, the CLI and MCP poll `translate.status`. `translate.run` is the
//! blocking form for headless callers. `translate.insert` is the only command that writes cells:
//! one undo step, text values only, formula cells untouched, all-or-nothing.
//!
//! Nothing here logs or formats cell text into an error. Model files are untrusted input: their
//! manifest paths are checked, their size and SHA-256 verified, and nothing is written until the
//! whole set is staged and verified.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use gridcraft_core::CellRef;
use gridcraft_core::Value;
use gridcraft_model::{Cell, Sheet, Workbook};
use gridcraft_translate::batch::DEFAULT_LIMITS;
use gridcraft_translate::manifest::{self, ModelFile, ModelManifest};
use gridcraft_translate::pipeline::{CellInput, CellResult, CellStatus, Failure, Options, Progress, Report, Stats};
use gridcraft_translate::{Cancel, LangTag, SkipReason, ValueKind, skip_reason};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cmd, edit, has_doc, str_param, target_range, target_sheet};
#[cfg(not(target_arch = "wasm32"))]
use gridcraft_translate::CancelReason;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query
            "review.translate",
            "Translate",
            ["Review", "Language"],
            None,
            "{on?: bool, from?, to?} → {on, from, to, model, ready, hint?} (toggles the Translate pane)",
            has_doc,
            review_translate
        ),
        cmd!(
            query
            "translate.start",
            "Translate Selection",
            ["Review", "Language"],
            None,
            "{range?, sheet?, from?: \"auto\"|tag (default: remembered), to?: tag, maxCells?} → {job, cells, skipped}",
            has_doc,
            start
        ),
        cmd!(
            query
            "translate.status",
            "Translation Status",
            ["Review", "Language"],
            None,
            "{job} → {state, progress, segments?, skipped?, stats?, error?}",
            no_doc_needed,
            status
        ),
        cmd!(query "translate.cancel", "Cancel Translation", ["Review", "Language"], None, "{job} → {cancelled}", no_doc_needed, cancel),
        cmd!(
            query
            "translate.run",
            "Translate (blocking)",
            ["Review", "Language"],
            None,
            "{range?, sheet?, from?: \"auto\"|tag, to?: tag, maxCells?, timeoutMs?} → {segments, stats}",
            has_doc,
            run
        ),
        cmd!(
            "translate.insert",
            "Insert Translation",
            ["Review", "Language"],
            None,
            "{range?, job?, values?, mode?: \"replace\"|\"emptyOnly\"} → {cells, skipped}",
            has_doc,
            insert
        ),
        cmd!(
            query
            "translate.copy",
            "Copy Translation",
            ["Review", "Language"],
            None,
            "{job, cells?: [A1…]} → {chars, cells}",
            no_doc_needed,
            copy
        ),
        cmd!(query "translate.models", "Translation Models", ["Review", "Language"], None, "{} → {models, active, modelsDir, accelerators}", no_doc_needed, models),
        cmd!(
            query
            "translate.modelImport",
            "Import Translation Model",
            ["Review", "Language"],
            None,
            "{path, id?, consent: true} → model record",
            no_doc_needed,
            model_import
        ),
        cmd!(
            query
            "translate.modelDownload",
            "Download Translation Model",
            ["Review", "Language"],
            None,
            "{id, consent: true} → model record",
            no_doc_needed,
            model_download
        ),
        cmd!(query "translate.modelRemove", "Remove Translation Model", ["Review", "Language"], None, "{id} → {removed}", no_doc_needed, model_remove),
    ]
}

fn no_doc_needed(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

// ---------------------------------------------------------------- job state

/// Where a translation job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Done,
    Cancelled,
    Failed,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Done => "done",
            JobState::Cancelled => "cancelled",
            JobState::Failed => "failed",
        }
    }
    pub fn is_terminal(self) -> bool {
        matches!(self, JobState::Done | JobState::Cancelled | JobState::Failed)
    }
}

/// One cell in a job's snapshot, before inference.
#[derive(Clone, Debug)]
pub struct SnapCell {
    pub sheet: usize,
    /// The sheet's identity at snapshot time. Indices shift when sheets are moved or deleted, so
    /// `translate.insert` resolves this instead of trusting `sheet`.
    pub sheet_uid: u64,
    pub sheet_name: String,
    pub at: CellRef,
    pub text: String,
}

/// A selected cell that the skip rules left out, with its reason.
#[derive(Clone, Debug)]
pub struct SkippedCell {
    pub sheet_name: String,
    pub at: CellRef,
    pub reason: SkipReason,
}

/// Job state shared with its worker. Cell text lives here only in memory, for the length of the
/// session; nothing writes it to disk or a log.
pub struct JobData {
    pub state: JobState,
    pub progress: Progress,
    pub report: Option<Report>,
    pub cells: Vec<SnapCell>,
    pub skipped: Vec<SkippedCell>,
    pub error: Option<String>,
    pub cancel: Cancel,
    pub from: String,
    pub to: String,
    pub model: String,
    pub stats: Stats,
    pub elapsed_ms: u64,
    pub finished: bool,
}

/// A job's shared slot.
#[derive(Clone)]
pub struct JobHandle {
    pub id: u64,
    pub data: Arc<(Mutex<JobData>, Condvar)>,
}

fn lock_job(data: &Arc<(Mutex<JobData>, Condvar)>) -> MutexGuard<'_, JobData> {
    match data.0.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn jobs(s: &Session) -> MutexGuard<'_, std::collections::HashMap<u64, JobHandle>> {
    match s.translate_jobs.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn job_of(s: &Session, p: &Json) -> Result<JobHandle> {
    let id = p.get("job").and_then(Json::as_u64).ok_or_else(|| bad("translate", "missing `job` (a number from translate.start)"))?;
    jobs(s).get(&id).cloned().ok_or_else(|| EngineError::Other(format!("there is no translation job {id}")))
}

// ---------------------------------------------------------------- snapshot

struct Snapshot {
    from: Option<LangTag>,
    to: LangTag,
    cells: Vec<SnapCell>,
    skipped: Vec<SkippedCell>,
}

fn target_languages(s: &Session, p: &Json) -> Result<(Option<LangTag>, LangTag, String, String)> {
    let from_text = str_param(p, "from").map(str::to_string).unwrap_or_else(|| default_from(s));
    let from = LangTag::parse_auto(&from_text).map_err(|e| bad("translate", format!("`{from_text}` is not a source language: {e}")))?;
    let to_text = str_param(p, "to").map(str::to_string).unwrap_or_else(|| s.translate_to.clone());
    if to_text.trim().is_empty() {
        return Err(EngineError::Other("choose a target language (`to`) for translation".into()));
    }
    let to = LangTag::parse(&to_text).map_err(|e| bad("translate", format!("`{to_text}` is not a target language: {e}")))?;
    Ok((from, to, from_text, to_text))
}

fn default_from(s: &Session) -> String {
    if s.translate_from.trim().is_empty() { "auto".to_string() } else { s.translate_from.clone() }
}

fn max_cells_param(p: &Json) -> Result<usize> {
    match p.get("maxCells").and_then(Json::as_u64) {
        None => Ok(DEFAULT_LIMITS.max_cells),
        Some(0) => Err(bad("translate", "`maxCells` must be at least 1")),
        Some(n) => Ok(usize::try_from(n.min(DEFAULT_LIMITS.max_cells as u64)).unwrap_or(DEFAULT_LIMITS.max_cells)),
    }
}

fn snapshot(s: &Session, p: &Json) -> Result<Snapshot> {
    let sheet = target_sheet(s, p)?;
    let range = target_range(s, p)?;
    let cap = max_cells_param(p)?;
    if range.count() > cap as u64 {
        return Err(EngineError::Other(format!(
            "the selection has {} cells; the translation limit is {cap}. Select fewer cells or pass a smaller `range`.",
            range.count()
        )));
    }
    let (from, to, _, _) = target_languages(s, p)?;
    let d = s.doc()?;
    let Some(sh) = d.wb.sheet(sheet) else { return Err(bad("translate", "no such sheet")) };
    let limits = gridcraft_translate::Limits { max_cells: cap, ..DEFAULT_LIMITS };
    let mut cells = Vec::new();
    let mut skipped = Vec::new();
    for at in range.iter() {
        let cell = sh.cell(at);
        let kind = classify(&d.wb, sh, at, cell);
        let text = match cell.map(|c| &c.value) {
            Some(Value::Text(t)) => t.to_string(),
            _ => String::new(),
        };
        match skip_reason(kind, &text, &limits) {
            Some(reason) => skipped.push(SkippedCell { sheet_name: sh.name.clone(), at, reason }),
            None => cells.push(SnapCell { sheet, sheet_uid: sh.uid.get(), sheet_name: sh.name.clone(), at, text }),
        }
    }
    Ok(Snapshot { from, to, cells, skipped })
}

/// A cell's kind, with date number formats reported as dates rather than numbers.
fn classify(wb: &Workbook, sh: &Sheet, at: CellRef, cell: Option<&Cell>) -> ValueKind {
    let Some(c) = cell else { return ValueKind::Empty };
    if c.formula.is_some() || c.py.is_some() {
        return ValueKind::Formula;
    }
    match &c.value {
        Value::Empty => ValueKind::Empty,
        Value::Text(_) => ValueKind::Text,
        Value::Bool(_) => ValueKind::Boolean,
        Value::Error(_) => ValueKind::Error,
        Value::Array(_) => ValueKind::Error,
        Value::Number(_) => {
            let code = wb.styles.get(sh.style_id(at)).num_fmt.as_str();
            if gridcraft_numfmt::NumberFormat::parse(code).is_date() { ValueKind::Date } else { ValueKind::Number }
        }
    }
}

// ---------------------------------------------------------------- start / run

fn start(s: &mut Session, p: &Json) -> Result<Json> {
    let snap = snapshot(s, p)?;
    let backend = backend(s)?;
    let timeout = 1_800_000u64;
    let (handle, cells, skipped) = spawn_job(s, snap, backend, timeout)?;
    Ok(json!({"job": handle.id, "cells": cells, "skipped": skipped}))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    // The desktop control channel and the pane run commands on the UI frame: a blocking
    // `translate.run` there would freeze the window for up to `timeoutMs` (30 minutes by
    // default), and the channel's own reply timeout cannot stop the started job. Those
    // callers already have the non-blocking path: `translate.start` plus `translate.status`.
    // Headless callers (CLI `run --cmd`, MCP, scripts) go through `execute_headless`, which
    // sets no such guard. The flag is host-set, like `link_policy`: no command can widen it.
    if s.block_translate_run_on_ui {
        return Err(EngineError::Other(
            "translate.run blocks the UI thread; from the desktop (pane, control channel, MCP connected to the app) use translate.start and poll translate.status instead. translate.run is for headless callers only (gridcraft-cli, headless MCP)".into(),
        ));
    }
    let snap = snapshot(s, p)?;
    let backend = backend(s)?;
    let timeout = timeout_param(p)?;
    let (handle, _, _) = spawn_job(s, snap, backend, timeout)?;
    wait_for(&handle);
    let data = lock_job(&handle.data);
    match data.state {
        JobState::Done => {
            let mut out = report_json(&data);
            if let Some(object) = out.as_object_mut() {
                object.insert("job".into(), json!(handle.id));
            }
            Ok(out)
        }
        JobState::Cancelled => Err(EngineError::Other("the translation was cancelled; nothing was written".into())),
        JobState::Failed => {
            Err(EngineError::Other(format!("{}; nothing was written", data.error.clone().unwrap_or_else(|| "the translation failed".into()))))
        }
        _ => Err(EngineError::Other("the translation did not finish; nothing was written".into())),
    }
}

fn backend(s: &Session) -> Result<Arc<dyn gridcraft_translate::Backend>> {
    s.translate_backend.clone().ok_or_else(|| EngineError::Other(no_model_message(s)))
}

fn no_model_message(s: &Session) -> String {
    let dir = models_dir_display(s);
    format!(
        "no translation model is installed ({} or {}). Open Review > Language > Translate and download one (or place the model files in {dir}), then try again",
        manifest::MANIFESTS.first().map(|m| m.id).unwrap_or("qwen3-1.7b"),
        manifest::MANIFESTS.get(1).map(|m| m.id).unwrap_or("qwen3-0.6b")
    )
}

fn models_dir_display(s: &Session) -> String {
    if s.translate_models_dir.trim().is_empty() { "the model directory (set GRIDCRAFT_MODELS)".to_string() } else { s.translate_models_dir.clone() }
}

fn timeout_param(p: &Json) -> Result<u64> {
    match p.get("timeoutMs").and_then(Json::as_u64) {
        None => Ok(1_800_000),
        Some(0) => Err(bad("translate.run", "`timeoutMs` must be at least 1")),
        Some(ms) => Ok(ms.min(3_600_000)),
    }
}

fn spawn_job(s: &mut Session, snap: Snapshot, backend: Arc<dyn gridcraft_translate::Backend>, timeout_ms: u64) -> Result<(JobHandle, usize, usize)> {
    if cfg!(target_arch = "wasm32") {
        let _ = timeout_ms;
        return Err(EngineError::Other("translation runs on the desktop build; the web build does not run a local model yet".into()));
    }
    let id = s.next_translate_job;
    s.next_translate_job = s.next_translate_job.saturating_add(1);
    let model = backend.model_id().to_string();
    let cells = snap.cells.len();
    let skipped = snap.skipped.len();
    let from = snap.from.as_ref().map_or_else(|| "auto".to_string(), |t| t.canonical());
    let to = snap.to.canonical();
    let inputs: Vec<CellInput> = snap.cells.iter().enumerate().map(|(i, c)| CellInput { index: i, text: c.text.clone() }).collect();
    let options = Options { from: snap.from, to: snap.to, limits: DEFAULT_LIMITS, detect_threshold: 0.5 };
    #[cfg(target_arch = "wasm32")]
    let _ = (&inputs, &options);
    let cancel = Cancel::new();
    let data = Arc::new((
        Mutex::new(JobData {
            state: JobState::Queued,
            progress: Progress::default(),
            report: None,
            cells: snap.cells,
            skipped: snap.skipped,
            error: None,
            cancel: cancel.clone(),
            from,
            to,
            model,
            stats: Stats::default(),
            elapsed_ms: 0,
            finished: false,
        }),
        Condvar::new(),
    ));
    let handle = JobHandle { id, data: data.clone() };
    {
        let mut registry = jobs(s);
        registry.insert(id, handle.clone());
        // Finished jobs are kept for the pane to read; bound how many.
        if registry.len() > 16 {
            let mut finished: Vec<u64> = registry.iter().filter(|(_, h)| lock_job(&h.data).state.is_terminal()).map(|(id, _)| *id).collect();
            finished.sort_unstable();
            for old in finished {
                if registry.len() <= 16 {
                    break;
                }
                registry.remove(&old);
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let worker_data = data.clone();
        let worker_cancel = cancel.clone();
        let spawned = std::thread::Builder::new().name(format!("gridcraft-translate-{id}")).spawn(move || {
            work(backend, inputs, options, worker_cancel, worker_data);
        });
        if let Err(e) = spawned {
            drop(jobs(s).remove(&id));
            return Err(EngineError::Other(format!("could not start the translation worker: {e}")));
        }
        arm_timeout(data, timeout_ms);
    }
    Ok((handle, cells, skipped))
}

// Worker threads never exist on wasm; these cfg-gated functions are the only clock users.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::disallowed_methods)]
fn work(
    backend: Arc<dyn gridcraft_translate::Backend>,
    inputs: Vec<CellInput>,
    options: Options,
    cancel: Cancel,
    data: Arc<(Mutex<JobData>, Condvar)>,
) {
    let started = std::time::Instant::now();
    {
        let mut guard = lock_job(&data);
        guard.state = JobState::Running;
    }
    let outcome = gridcraft_translate::run(backend.as_ref(), &inputs, &options, &cancel, |progress| {
        let mut guard = lock_job(&data);
        guard.progress = progress;
    });
    let mut guard = lock_job(&data);
    guard.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match outcome {
        Ok(report) => {
            guard.stats = report.stats;
            guard.report = Some(report);
            guard.state = if cancel.is_cancelled() { cancelled_state(&cancel) } else { JobState::Done };
            if guard.state == JobState::Failed {
                guard.error = Some("timed out".to_string());
            }
        }
        Err(gridcraft_translate::TranslateError::Cancelled) => {
            guard.state = JobState::Cancelled;
            guard.error = Some("the translation was cancelled".into());
        }
        Err(gridcraft_translate::TranslateError::Timeout) => {
            guard.state = JobState::Failed;
            guard.error = Some("the translation timed out".into());
        }
        Err(e) => {
            guard.state = JobState::Failed;
            guard.error = Some(e.to_string());
        }
    }
    if cancel.is_cancelled() && guard.state == JobState::Done {
        guard.state = cancelled_state(&cancel);
    }
    guard.finished = true;
    data.1.notify_all();
}

#[cfg(not(target_arch = "wasm32"))]
fn cancelled_state(cancel: &Cancel) -> JobState {
    match cancel.reason() {
        Some(CancelReason::Timeout) => JobState::Failed,
        _ => JobState::Cancelled,
    }
}

/// Cancels a job with the timeout reason unless it finished first.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::disallowed_methods)]
fn arm_timeout(data: Arc<(Mutex<JobData>, Condvar)>, timeout_ms: u64) {
    let spawned = std::thread::Builder::new().name("gridcraft-translate-timeout".into()).spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(1));
        let mut guard = lock_job(&data);
        loop {
            if guard.finished {
                return;
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                let cancel = guard.cancel.clone();
                drop(guard);
                cancel.cancel_with(CancelReason::Timeout);
                return;
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next, _) = match data.1.wait_timeout(guard, remaining) {
                Ok(pair) => pair,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard = next;
        }
    });
    let _ = spawned;
}

/// There are no worker threads on wasm (start/run refuse first), so this is never reached.
#[cfg(target_arch = "wasm32")]
fn wait_for(_: &JobHandle) {}

#[cfg(not(target_arch = "wasm32"))]
fn wait_for(handle: &JobHandle) {
    let mut guard = lock_job(&handle.data);
    while !guard.state.is_terminal() {
        let waited = handle.data.1.wait_timeout(guard, std::time::Duration::from_secs(30));
        guard = match waited {
            Ok((guard, _)) => guard,
            Err(poisoned) => poisoned.into_inner().0,
        };
    }
}

// ---------------------------------------------------------------- status / cancel / copy

fn status(s: &mut Session, p: &Json) -> Result<Json> {
    let handle = job_of(s, p)?;
    let data = lock_job(&handle.data);
    let mut out = json!({
        "job": handle.id,
        "state": data.state.as_str(),
        "progress": {
            "done": data.progress.done_batches,
            "total": data.progress.total_batches,
            "segmentsDone": data.progress.done_segments,
            "segmentsTotal": data.progress.total_segments,
        },
        "model": data.model.clone(),
        "from": data.from.clone(),
        "to": data.to.clone(),
        "elapsedMs": data.elapsed_ms,
    });
    if let Some(error) = &data.error {
        out["error"] = json!(error);
    }
    if data.state == JobState::Done {
        let report = report_json(&data);
        for key in ["segments", "stats"] {
            if let Some(value) = report.get(key) {
                out[key] = value.clone();
            }
        }
    }
    out["skipped"] = skipped_json(&data.skipped);
    Ok(out)
}

fn report_json(data: &JobData) -> Json {
    let report = data.report.as_ref();
    let segments = report.map(|report| Json::Array(report.cells.iter().zip(&data.cells).map(|(result, cell)| cell_json(cell, result)).collect()));
    // Cells the skip rules removed before inference are part of what the selection asked for,
    // so the totals add them back.
    let mut stats = report.map(|r| r.stats).unwrap_or(data.stats);
    stats.skipped = stats.skipped.saturating_add(data.skipped.len());
    stats.cells = stats.translated.saturating_add(stats.skipped).saturating_add(stats.failed);
    json!({
        "segments": segments.unwrap_or(Json::Null),
        "stats": stats_json(stats),
        "model": data.model.clone(),
        "from": data.from.clone(),
        "to": data.to.clone(),
        "elapsedMs": data.elapsed_ms,
    })
}

fn cell_json(cell: &SnapCell, result: &CellResult) -> Json {
    let mut out = json!({
        "sheet": cell.sheet_name,
        "cell": cell.at.a1(),
        "status": result.status.kind(),
        "text": result.text,
    });
    match &result.status {
        CellStatus::Skipped(reason) => out["reason"] = json!(reason.as_str()),
        CellStatus::Failed(failure) => {
            out["failure"] = json!(failure.code());
            match failure {
                Failure::LowConfidence { detected, confidence } => {
                    out["detected"] = detected.as_ref().map_or(Json::Null, |tag| json!(tag.canonical()));
                    out["confidence"] = json!(confidence);
                }
                Failure::Backend(message) => out["detail"] = json!(message),
                _ => {}
            }
        }
        CellStatus::Translated { from, confidence } => {
            out["from"] = json!(from.canonical());
            out["confidence"] = json!(confidence);
        }
    }
    out
}

fn stats_json(stats: Stats) -> Json {
    json!({
        "cells": stats.cells,
        "translated": stats.translated,
        "skipped": stats.skipped,
        "failed": stats.failed,
        "chars": stats.chars,
        "segments": stats.segments,
        "batches": stats.batches,
    })
}

fn skipped_json(skipped: &[SkippedCell]) -> Json {
    Json::Array(skipped.iter().map(|s| json!({"sheet": s.sheet_name, "cell": s.at.a1(), "reason": s.reason.as_str()})).collect())
}

fn cancel(s: &mut Session, p: &Json) -> Result<Json> {
    let handle = job_of(s, p)?;
    let data = lock_job(&handle.data);
    if data.state.is_terminal() {
        return Ok(json!({"cancelled": false}));
    }
    data.cancel.cancel();
    Ok(json!({"cancelled": true}))
}

fn copy(s: &mut Session, p: &Json) -> Result<Json> {
    let handle = job_of(s, p)?;
    let data = lock_job(&handle.data);
    if data.state != JobState::Done {
        return Err(EngineError::Other(format!("translation job {} is {}; there is nothing to copy", handle.id, data.state.as_str())));
    }
    let Some(report) = &data.report else { return Err(EngineError::Other("the translation produced no results".into())) };
    let filter: Option<Vec<String>> = match p.get("cells") {
        None => None,
        Some(Json::Array(items)) => Some(items.iter().filter_map(Json::as_str).map(|s| s.to_string()).collect()),
        Some(_) => return Err(bad("translate.copy", "`cells` must be an array of cell references")),
    };
    let mut parts = Vec::new();
    for (result, cell) in report.cells.iter().zip(&data.cells) {
        if result.status.kind() != "translated" {
            continue;
        }
        if let Some(filter) = &filter
            && !filter.iter().any(|f| f.eq_ignore_ascii_case(&cell.at.a1()))
        {
            continue;
        }
        if let Some(text) = &result.text {
            parts.push(text.clone());
        }
    }
    let text = parts.join("\n");
    let chars = text.chars().count();
    let count = parts.len();
    s.translate_clipboard = if text.is_empty() { None } else { Some(text) };
    Ok(json!({"chars": chars, "cells": count}))
}

// ---------------------------------------------------------------- insert

fn insert(s: &mut Session, p: &Json) -> Result<Json> {
    let mode = match str_param(p, "mode").unwrap_or("replace") {
        "replace" => InsertMode::Replace,
        "emptyOnly" | "empty-only" | "fillEmpty" => InsertMode::EmptyOnly,
        other => return Err(bad("translate.insert", format!("`mode` is `{other}`; use `replace` or `emptyOnly`"))),
    };
    let (targets, mut skipped) = if p.get("values").is_some() { values_targets(s, p, mode)? } else { job_targets(s, p, mode)? };
    if targets.is_empty() {
        return Ok(json!({"cells": 0, "skipped": skipped}));
    }
    // Validate everything before changing anything: a bad sheet or cell is an error, not a
    // half-written selection. Skips (formulas, changed cells, empty-only) are not errors.
    let d = s.doc()?;
    if targets.iter().any(|target| d.wb.sheet(target.sheet).is_none()) {
        return Err(bad("translate.insert", "no such sheet"));
    }
    let count = targets.len();
    edit(s, |cx| {
        for target in &targets {
            let Some(sh) = cx.wb.sheet(target.sheet) else { continue };
            let style = sh.style_id(target.at);
            let value = if target.text.is_empty() {
                None
            } else {
                Some(Cell { value: Value::Text(target.text.as_str().into()), formula: None, style, py: None })
            };
            let sh = cx.sheet_mut(target.sheet)?;
            match value {
                Some(cell) => sh.set_cell(target.at, cell),
                None => {
                    sh.remove_cell(target.at);
                }
            }
            crate::cmd::data::grow_autofilter(sh, target.at);
            cx.touch(target.sheet, target.at);
        }
        Ok(())
    })?;
    // The same cell written twice in `values` is a caller mistake; report it instead of
    // pretending one write won.
    let _ = &mut skipped;
    Ok(json!({"cells": count, "skipped": skipped}))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InsertMode {
    Replace,
    EmptyOnly,
}

struct InsertTarget {
    sheet: usize,
    at: CellRef,
    text: String,
}

fn job_targets(s: &Session, p: &Json, mode: InsertMode) -> Result<(Vec<InsertTarget>, Vec<Json>)> {
    let handle = job_of(s, p)?;
    let data = lock_job(&handle.data);
    if data.state != JobState::Done {
        return Err(EngineError::Other(format!("translation job {} is {}; there is nothing to insert", handle.id, data.state.as_str())));
    }
    let Some(report) = &data.report else { return Err(EngineError::Other("the translation produced no results".into())) };
    let filter = if p.get("range").is_some() {
        let sheet = target_sheet(s, p)?;
        Some((sheet, target_range(s, p)?))
    } else {
        None
    };
    let d = s.doc()?;
    let mut targets = Vec::new();
    let mut skipped = Vec::new();
    for (result, cell) in report.cells.iter().zip(&data.cells) {
        let Some(text) = result.text.clone().filter(|_| result.status.kind() == "translated") else { continue };
        // Sheets can move or be deleted between the snapshot and the insert, and a stale index
        // would write into whatever sheet now holds that position. Resolve the recorded identity
        // instead, and drop the result when the sheet is gone.
        let Some(sheet) = d.wb.sheets.iter().position(|sh| sh.uid.get() == cell.sheet_uid) else { continue };
        let Some(sh) = d.wb.sheet(sheet) else { continue };
        if let Some((filter_sheet, range)) = filter
            && (filter_sheet != sheet || !range.contains(cell.at))
        {
            continue;
        }
        if sh.cell(cell.at).is_some_and(|c| c.formula.is_some() || c.py.is_some()) {
            skipped.push(json!({"sheet": cell.sheet_name, "cell": cell.at.a1(), "reason": "formula"}));
            continue;
        }
        if mode == InsertMode::EmptyOnly && !is_empty(sh.value(cell.at)) {
            skipped.push(json!({"sheet": cell.sheet_name, "cell": cell.at.a1(), "reason": "nonEmpty"}));
            continue;
        }
        // Do not overwrite an edit made after the snapshot.
        let current = match sh.value(cell.at) {
            Value::Text(t) => t.to_string(),
            _ => String::new(),
        };
        if current != cell.text {
            skipped.push(json!({"sheet": cell.sheet_name, "cell": cell.at.a1(), "reason": "changed"}));
            continue;
        }
        targets.push(InsertTarget { sheet, at: cell.at, text });
    }
    Ok((targets, skipped))
}

fn is_empty(value: Value) -> bool {
    matches!(&value, Value::Empty) || matches!(&value, Value::Text(t) if t.trim().is_empty())
}

fn values_targets(s: &Session, p: &Json, mode: InsertMode) -> Result<(Vec<InsertTarget>, Vec<Json>)> {
    let values = p.get("values").and_then(Json::as_array).ok_or_else(|| bad("translate.insert", "`values` must be an array"))?;
    let sheet = target_sheet(s, p)?;
    let start = target_range(s, p)?.start;
    let d = s.doc()?;
    let mut targets = Vec::new();
    let mut skipped = Vec::new();
    for (row_index, row) in values.iter().enumerate() {
        let items: Vec<&Json> = match row {
            Json::Array(items) => items.iter().collect(),
            other => vec![other],
        };
        for (col_index, item) in items.iter().enumerate() {
            let (target_sheet, at, text) = match item {
                Json::Object(object) => {
                    let text = object.get("text").and_then(Json::as_str).map(str::to_string).unwrap_or_default();
                    let cell_text = object.get("cell").and_then(Json::as_str);
                    let (sh, at) = match cell_text {
                        Some(reference) => parse_cell_reference(&d.wb, reference, sheet)?,
                        None => {
                            let at = start
                                .offset(i64::try_from(row_index).unwrap_or(0), i64::try_from(col_index).unwrap_or(0))
                                .ok_or_else(|| bad("translate.insert", "`values` runs past the end of the sheet"))?;
                            (sheet, at)
                        }
                    };
                    (sh, at, text)
                }
                Json::String(text) => {
                    let at = start
                        .offset(i64::try_from(row_index).unwrap_or(0), i64::try_from(col_index).unwrap_or(0))
                        .ok_or_else(|| bad("translate.insert", "`values` runs past the end of the sheet"))?;
                    (sheet, at, text.clone())
                }
                _ => return Err(bad("translate.insert", "each `values` entry must be text or {cell, text}")),
            };
            let Some(sh) = d.wb.sheet(target_sheet) else {
                skipped.push(json!({"sheet": "", "cell": at.a1(), "reason": "noSheet"}));
                continue;
            };
            if sh.cell(at).is_some_and(|c| c.formula.is_some() || c.py.is_some()) {
                skipped.push(json!({"sheet": sh.name, "cell": at.a1(), "reason": "formula"}));
                continue;
            }
            if mode == InsertMode::EmptyOnly && !is_empty(sh.value(at)) {
                skipped.push(json!({"sheet": sh.name, "cell": at.a1(), "reason": "nonEmpty"}));
                continue;
            }
            targets.push(InsertTarget { sheet: target_sheet, at, text });
        }
    }
    // A repeated cell: keep the first write, report the rest, so "all-or-nothing" stays true.
    let mut seen: Vec<(usize, CellRef)> = Vec::new();
    targets.retain(|target| {
        if seen.contains(&(target.sheet, target.at)) {
            skipped.push(json!({"sheet": "", "cell": target.at.a1(), "reason": "duplicate"}));
            false
        } else {
            seen.push((target.sheet, target.at));
            true
        }
    });
    Ok((targets, skipped))
}

fn parse_cell_reference(wb: &Workbook, reference: &str, default_sheet: usize) -> Result<(usize, CellRef)> {
    let (sheet_name, body) = gridcraft_core::split_sheet(reference);
    let sheet = match sheet_name {
        Some(name) => wb.sheet_index(&name).ok_or_else(|| bad("translate.insert", format!("no sheet named `{name}`")))?,
        None => default_sheet,
    };
    let at = CellRef::parse(body).ok_or_else(|| bad("translate.insert", format!("`{reference}` is not a cell reference")))?;
    Ok((sheet, at))
}

// ---------------------------------------------------------------- models

fn models(s: &mut Session, _: &Json) -> Result<Json> {
    let mut list = Vec::new();
    for m in manifest::MANIFESTS {
        list.push(model_record(s, m));
    }
    Ok(json!({
        "models": list,
        "active": s.translate_backend.as_ref().map(|b| b.model_id().to_string()),
        "modelsDir": s.translate_models_dir.clone(),
        "accelerators": gridcraft_translate::accelerator::available().iter().map(|a| a.as_str()).collect::<Vec<_>>(),
        "note": if cfg!(target_arch = "wasm32") { "models are not available in the web build" } else { "" },
    }))
}

fn model_record(s: &Session, m: &ModelManifest) -> Json {
    let (installed, verified, dir) = model_state(s, m);
    let active = s.translate_backend.as_ref().is_some_and(|b| b.model_id() == m.id);
    let state = if active {
        "ready"
    } else if !installed {
        if dir.as_ref().is_some_and(|d| d.exists()) { "incomplete" } else { "missing" }
    } else if verified {
        "ready"
    } else {
        "unverified"
    };
    json!({
        "id": m.id,
        "name": m.name,
        "source": m.source,
        "revision": m.revision,
        "license": m.license,
        "licenseUrl": m.license_url,
        "licenseNote": m.license_note,
        "bytes": manifest::total_bytes(m),
        "minRamMb": m.min_ram_mb,
        "runtime": m.runtime,
        "pairs": m.pairs.iter().map(|p| json!({"from": p.from, "to": p.to})).collect::<Vec<_>>(),
        "files": m.files.iter().map(|f| json!({"path": f.path, "bytes": f.bytes, "sha256": f.sha256})).collect::<Vec<_>>(),
        "installed": installed,
        "verified": verified,
        "state": state,
        "path": dir.map(|d| d.display().to_string()),
    })
}

fn model_state(s: &Session, m: &ModelManifest) -> (bool, bool, Option<PathBuf>) {
    if s.translate_models_dir.trim().is_empty() {
        return (false, false, None);
    }
    let dir = Path::new(&s.translate_models_dir).join(m.id);
    let installed = model_files_present(&dir, m);
    let verified = installed && sidecar_matches(&dir, m);
    (installed, verified, Some(dir))
}

fn model_files_present(dir: &Path, m: &ModelManifest) -> bool {
    if !manifest::valid_id(m.id) || !manifest::valid_relative_path(m.id) {
        return false;
    }
    m.files.iter().all(|f| {
        manifest::valid_relative_path(f.path) && std::fs::metadata(dir.join(f.path)).is_ok_and(|meta| meta.is_file() && meta.len() == f.bytes)
    })
}

fn sidecar_matches(dir: &Path, m: &ModelManifest) -> bool {
    let Ok(bytes) = std::fs::read(dir.join("manifest.json")) else { return false };
    let Ok(json) = serde_json::from_slice::<Json>(&bytes) else { return false };
    json.get("id").and_then(Json::as_str) == Some(m.id) && json.get("revision").and_then(Json::as_str) == Some(m.revision)
}

fn model_import(s: &mut Session, p: &Json) -> Result<Json> {
    require_consent("translate.modelImport", p)?;
    let path = str_param(p, "path").ok_or_else(|| bad("translate.modelImport", "missing `path` (a folder holding the model files)"))?;
    if path.contains('\0') {
        return Err(bad("translate.modelImport", "the path is not valid"));
    }
    let source = Path::new(path);
    if !source.is_dir() {
        return Err(EngineError::Other(format!("`{path}` is not a folder")));
    }
    let id = match str_param(p, "id") {
        Some(id) => id.to_string(),
        None => source.file_name().and_then(|n| n.to_str()).map(str::to_string).unwrap_or_default(),
    };
    let m = manifest::find(&id).ok_or_else(|| EngineError::Other(format!("`{id}` is not a known translation model; known ids: {}", known_ids())))?;
    models_dir(s)?;
    if cfg!(target_arch = "wasm32") {
        return Err(EngineError::Other("importing a model from a folder needs the desktop build".into()));
    }
    install_from_folder(s, m, source).map_err(EngineError::Other)?;
    refresh_installed_backend(s, m);
    Ok(model_record(s, m))
}

fn model_download(s: &mut Session, p: &Json) -> Result<Json> {
    require_consent("translate.modelDownload", p)?;
    let id = str_param(p, "id").ok_or_else(|| bad("translate.modelDownload", "missing `id`"))?;
    let m = manifest::find(id).ok_or_else(|| EngineError::Other(format!("`{id}` is not a known translation model; known ids: {}", known_ids())))?;
    let _ = models_dir(s)?;
    if cfg!(target_arch = "wasm32") {
        return Err(EngineError::Other("downloading a model needs the desktop build; place the model files in the model directory instead".into()));
    }
    let fetcher = s.translate_fetcher.clone().ok_or_else(|| {
        EngineError::Other(
            "this build has no download transport. Place the model files in the model directory by hand, or use the desktop app".into(),
        )
    })?;
    install_from_download(s, m, fetcher.as_ref()).map_err(EngineError::Other)?;
    refresh_installed_backend(s, m);
    Ok(model_record(s, m))
}

/// The `Session::execute` path never loads model weights: the pane keeps its own download
/// session, and headless callers (CLI, MCP, the control channel) inject the backend at startup.
/// After either install command succeeds on the session that ran it, that session can use the
/// model right away: the host's opener (installed at startup next to the model root) reopens
/// the just-installed `m.id`, and the refreshed `translate_backend` replaces the old one.
/// Without it a model-less session keeps reporting no-model after its own import or download.
#[cfg(not(target_arch = "wasm32"))]
fn refresh_installed_backend(s: &mut Session, m: &ModelManifest) {
    if let Some(reopened) = s.translate_backend_opener.as_ref().and_then(|open| open(s, m.id))
        && reopened.as_ref().is_some_and(|b| b.model_id() == m.id)
    {
        s.translate_backend = reopened;
    }
}

#[cfg(target_arch = "wasm32")]
fn refresh_installed_backend(_s: &mut Session, _m: &ModelManifest) {}

fn model_remove(s: &mut Session, p: &Json) -> Result<Json> {
    let id = str_param(p, "id").ok_or_else(|| bad("translate.modelRemove", "missing `id`"))?;
    let m = manifest::find(id).ok_or_else(|| EngineError::Other(format!("`{id}` is not a known translation model; known ids: {}", known_ids())))?;
    let root = models_dir(s)?;
    if cfg!(target_arch = "wasm32") {
        return Err(EngineError::Other("removing a model needs the desktop build".into()));
    }
    let dir = root.join(m.id);
    let removed = match std::fs::remove_dir_all(&dir) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(EngineError::Other(format!("could not remove `{}`: {e}", dir.display()))),
    };
    if s.translate_backend.as_ref().is_some_and(|b| b.model_id() == m.id) {
        s.translate_backend = None;
    }
    Ok(json!({"removed": removed}))
}

fn require_consent(cmd: &str, p: &Json) -> Result<()> {
    match bool_param(p, "consent") {
        Some(true) => Ok(()),
        _ => Err(EngineError::Other(format!(
            "{cmd} needs explicit consent: the model is a large download whose source, licence and SHA-256 are shown first; call it again with `consent: true`"
        ))),
    }
}

fn known_ids() -> String {
    manifest::MANIFESTS.iter().map(|m| m.id).collect::<Vec<_>>().join(", ")
}

fn models_dir(s: &Session) -> Result<PathBuf> {
    if s.translate_models_dir.trim().is_empty() {
        return Err(EngineError::Other("there is no model directory. Set GRIDCRAFT_MODELS, or start the desktop app, which resolves one".into()));
    }
    Ok(PathBuf::from(&s.translate_models_dir))
}

fn install_from_folder(s: &Session, m: &ModelManifest, source: &Path) -> std::result::Result<(), String> {
    for file in m.files {
        if !manifest::valid_relative_path(file.path) {
            return Err(format!("the manifest path `{}` is unsafe", file.path));
        }
        let path = source.join(file.path);
        verify_file(&path, file, file.path)?;
    }
    let root = models_dir_raw(s)?;
    let staging = stage_dir(&root, m.id)?;
    let result = (|| {
        for file in m.files {
            let from = source.join(file.path);
            let to = staging.join(file.path);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            }
            std::fs::copy(&from, &to).map_err(|e| format!("could not copy `{}`: {e}", file.path))?;
            verify_file(&to, file, file.path)?;
        }
        write_sidecar(&staging, m)?;
        finalize(&staging, &root.join(m.id))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn install_from_download(s: &Session, m: &ModelManifest, fetcher: &dyn gridcraft_translate::ModelFetcher) -> std::result::Result<(), String> {
    let root = models_dir_raw(s)?;
    let staging = stage_dir(&root, m.id)?;
    let result = (|| {
        for file in m.files {
            if !manifest::valid_relative_path(file.path) {
                return Err(format!("the manifest path `{}` is unsafe", file.path));
            }
            let url = file.url;
            let to = staging.join(file.path);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            }
            // The manifest's own size is the download bound: the fetcher refuses a larger body
            // before it fills the disk, and `verify_file` then checks it exactly.
            fetcher.fetch(url, &to, file.bytes).map_err(|e| format!("downloading `{}` failed: {e}", file.path))?;
            verify_file(&to, file, file.path)?;
        }
        write_sidecar(&staging, m)?;
        finalize(&staging, &root.join(m.id))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn models_dir_raw(s: &Session) -> std::result::Result<PathBuf, String> {
    if s.translate_models_dir.trim().is_empty() {
        return Err("there is no model directory. Set GRIDCRAFT_MODELS, or start the desktop app, which resolves one".into());
    }
    Ok(PathBuf::from(&s.translate_models_dir))
}

fn stage_dir(root: &Path, id: &str) -> std::result::Result<PathBuf, String> {
    if !manifest::valid_id(id) {
        return Err(format!("`{id}` is not a valid model id"));
    }
    std::fs::create_dir_all(root).map_err(|e| format!("could not create the model directory: {e}"))?;
    let staging = root.join(format!(".{id}.staging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("could not create a staging folder: {e}"))?;
    Ok(staging)
}

fn finalize(staging: &Path, target: &Path) -> std::result::Result<(), String> {
    if !manifest::valid_relative_path(target.file_name().and_then(|n| n.to_str()).unwrap_or_default()) {
        return Err("the model directory name is unsafe".into());
    }
    let backup = target.with_file_name(format!("{}.old-{}", target.file_name().and_then(|n| n.to_str()).unwrap_or("model"), std::process::id()));
    let had_old = target.exists();
    if had_old {
        std::fs::rename(target, &backup).map_err(|e| format!("could not move the old model aside: {e}"))?;
    }
    match std::fs::rename(staging, target) {
        Ok(()) => {
            if had_old {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(e) => {
            if had_old {
                let _ = std::fs::rename(&backup, target);
            }
            Err(format!("could not install the model: {e}"))
        }
    }
}

fn write_sidecar(dir: &Path, m: &ModelManifest) -> std::result::Result<(), String> {
    let sidecar = json!({
        "id": m.id,
        "revision": m.revision,
        "source": m.source,
        "license": m.license,
        "licenseUrl": m.license_url,
        "files": m.files.iter().map(|f| json!({"path": f.path, "bytes": f.bytes, "sha256": f.sha256})).collect::<Vec<_>>(),
    });
    let bytes = serde_json::to_vec_pretty(&sidecar).map_err(|e| format!("could not write the model record: {e}"))?;
    std::fs::write(dir.join("manifest.json"), bytes).map_err(|e| format!("could not write the model record: {e}"))
}

fn verify_file(path: &Path, file: &ModelFile, relative: &str) -> std::result::Result<(), String> {
    if !manifest::valid_relative_path(relative) {
        return Err(format!("the manifest path `{relative}` is unsafe"));
    }
    let metadata = std::fs::metadata(path).map_err(|_| format!("`{relative}` is missing"))?;
    if metadata.len() != file.bytes {
        return Err(format!("`{relative}` has {} bytes; the manifest says {}", metadata.len(), file.bytes));
    }
    let digest = sha256_file(path)?;
    if !digest.eq_ignore_ascii_case(file.sha256) {
        return Err(format!("`{relative}` does not match its SHA-256"));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> std::result::Result<String, String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| format!("could not read `{}`: {e}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| format!("could not read `{}`: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

// ---------------------------------------------------------------- review.translate

fn review_translate(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(from) = str_param(p, "from") {
        LangTag::parse_auto(from).map_err(|e| bad("review.translate", format!("`{from}` is not a source language: {e}")))?;
        s.translate_from = from.to_string();
    }
    if let Some(to) = str_param(p, "to") {
        let tag = LangTag::parse(to).map_err(|e| bad("review.translate", format!("`{to}` is not a target language: {e}")))?;
        s.translate_to = tag.canonical();
    }
    s.translate_pane = bool_param(p, "on").unwrap_or(!s.translate_pane);
    // One task pane at a time: showing Translate closes the Comments pane.
    if s.translate_pane {
        s.comments_pane = false;
    }
    let active = s.translate_backend.as_ref().map(|b| b.model_id().to_string());
    let mut out = json!({
        "on": s.translate_pane,
        "from": default_from(s),
        "to": s.translate_to.clone(),
        "model": active,
        "ready": active.is_some(),
    });
    if active.is_none() {
        out["hint"] = json!(no_model_message(s));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_translate::FixedBackend;

    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "sales"})).expect("sample");
        s.translate_to = "es".into();
        s.translate_from = "en".into();
        s.translate_backend = Some(Arc::new(FixedBackend::new()));
        s
    }

    #[test]
    fn review_translate_toggles_the_pane() {
        let mut s = session();
        let out = s.execute("review.translate", json!({})).expect("toggle");
        assert_eq!(out["on"], json!(true));
        let out = s.execute("review.translate", json!({"on": false})).expect("toggle");
        assert_eq!(out["on"], json!(false));
    }

    #[test]
    fn the_task_panes_are_mutually_exclusive() {
        let mut s = session();
        s.execute("review.showComments", json!({"on": true})).expect("comments");
        assert!(s.comments_pane);
        s.execute("review.translate", json!({"on": true})).expect("translate");
        assert!(s.translate_pane);
        assert!(!s.comments_pane, "opening Translate closes Comments");
        s.execute("review.showComments", json!({"on": true})).expect("comments");
        assert!(s.comments_pane);
        assert!(!s.translate_pane, "opening Comments closes Translate");
    }

    #[test]
    fn no_model_is_an_actionable_error_and_writes_nothing() {
        let mut s = session();
        s.translate_backend = None;
        let before = s.doc().expect("doc").wb.clone();
        let err = s.execute("translate.run", json!({"range": "A1:B4", "from": "en", "to": "es"})).unwrap_err().to_string();
        assert!(err.contains("no translation model"), "{err}");
        assert!(Arc::ptr_eq(&before, &s.doc().expect("doc").wb));
    }

    #[test]
    fn a_successful_install_refreshes_the_session_backend_through_the_opener() {
        // The failing case this guards: a model-less session installs a model and still gets
        // the no-model error because nothing refreshed `translate_backend` on the session
        // that ran the install. The opener stands in for the host's `installed_backend`;
        // the id gate is covered by `install_refresh_ignores_a_backend_for_another_model`.
        struct Installed;
        impl gridcraft_translate::Backend for Installed {
            fn model_id(&self) -> &str {
                "qwen3-1.7b"
            }
            fn supports(&self, _: &LangTag, _: &LangTag) -> bool {
                true
            }
            fn translate(
                &self,
                req: &gridcraft_translate::BatchRequest<'_>,
                _: &gridcraft_translate::Cancel,
            ) -> std::result::Result<Vec<String>, gridcraft_translate::BackendError> {
                Ok(req.texts.iter().map(|_| String::new()).collect())
            }
        }
        let mut s = session();
        s.translate_backend = None;
        let opened: Arc<dyn gridcraft_translate::Backend> = Arc::new(Installed);
        let probe = Arc::clone(&opened);
        s.translate_backend_opener = Some(Arc::new(move |_: &Session, _: &str| Some(Some(Arc::clone(&probe)))));
        let m = manifest::find("qwen3-1.7b").expect("manifest");
        refresh_installed_backend(&mut s, m);
        assert!(s.translate_backend.as_ref().is_some_and(|b| b.model_id() == opened.model_id()));
    }

    #[test]
    fn install_refresh_ignores_a_backend_for_another_model() {
        // A session mid-install for one model must not pick up a stale backend for another id:
        // without the model-id check the pane would show "ready" for the wrong model.
        struct Other;
        impl gridcraft_translate::Backend for Other {
            fn model_id(&self) -> &str {
                "qwen3-0.6b"
            }
            fn supports(&self, _: &LangTag, _: &LangTag) -> bool {
                false
            }
            fn translate(
                &self,
                req: &gridcraft_translate::BatchRequest<'_>,
                _: &gridcraft_translate::Cancel,
            ) -> std::result::Result<Vec<String>, gridcraft_translate::BackendError> {
                Ok(req.texts.iter().map(|_| String::new()).collect())
            }
        }
        let mut s = session();
        s.translate_backend = None;
        let other: Arc<dyn gridcraft_translate::Backend> = Arc::new(Other);
        s.translate_backend_opener = Some(Arc::new(move |_: &Session, _: &str| Some(Some(Arc::clone(&other)))));
        let m = manifest::find("qwen3-1.7b").expect("manifest");
        refresh_installed_backend(&mut s, m);
        assert!(s.translate_backend.is_none());
    }

    #[test]
    fn install_without_an_opener_changes_no_backend() {
        // The wasm build installs no opener: a successful install there leaves the session
        // exactly as it was, and translation still reports no-model.
        let mut s = session();
        s.translate_backend = None;
        assert!(s.translate_backend_opener.is_none());
        let m = manifest::find("qwen3-1.7b").expect("manifest");
        refresh_installed_backend(&mut s, m);
        assert!(s.translate_backend.is_none());
    }

    #[test]
    fn translate_run_is_refused_on_ui_sessions_but_start_is_not() {
        // The failing case this guards: a desktop control-channel request runs `translate.run`
        // on the UI frame and freezes the window for up to `timeoutMs`. The host sets
        // `block_translate_run_on_ui`; the non-blocking path keeps working there, and
        // headless sessions (flag unset) keep the blocking convenience.
        let mut ui = session();
        ui.block_translate_run_on_ui = true;
        let err = ui.execute("translate.run", json!({"range": "A1", "from": "en", "to": "es"})).unwrap_err().to_string();
        assert!(err.contains("blocks the UI thread"), "{err}");
        assert!(err.contains("translate.start"), "{err}");
        // `translate.start` snapshots and returns a job without blocking.
        ui.execute("cell.set", json!({"cell": "A1", "input": "Hello"})).expect("cell");
        let started = ui.execute("translate.start", json!({"range": "A1", "from": "en", "to": "es"})).expect("start");
        assert!(started["job"].as_u64().is_some(), "{started}");
        // A headless caller with no guard keeps `translate.run`.
        let mut headless = session();
        assert!(!headless.block_translate_run_on_ui);
        headless.execute("cell.set", json!({"cell": "A1", "input": "Hello"})).expect("cell");
        let report = headless.execute("translate.run", json!({"range": "A1", "from": "en", "to": "es"})).expect("run");
        assert_eq!(report["stats"]["translated"], 1, "{report}");
    }
}
