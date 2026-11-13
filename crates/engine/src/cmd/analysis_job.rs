//! Incremental, transport-independent analysis lifecycle; no report is written until finish.
use super::monte_carlo::Job;
use super::{CommandSpec, Json, Result, Session, always, bad, cmd, has_doc};
use serde_json::json;

/// Registers the same job lifecycle for native UI, WASM, CLI and MCP callers.
pub fn specs() -> Vec<CommandSpec> {
    let mut publication = cmd!(
        "analysis.finish",
        "Monte Carlo",
        ["Data", "Analysis"],
        None,
        "{job: id}. Publishes a ready job as one undoable report in its source workbook. Early finish does not discard the job.",
        has_doc,
        finish
    );
    publication.journal = false; // Replay records the canonical simulation, not a job id.
    vec![
        cmd!(query "analysis.start", "Start analysis", ["Data", "Analysis"], None,
            "{command?: data.monteCarlo, params: {inputs, outputs, ...}, autoAdvance?: false}. Creates one isolated job; source remains unchanged. No nested undo groups or action recording.", has_doc, start),
        cmd!(query "analysis.status", "Analysis progress", ["Data", "Analysis"], None,
            "{job?: id}. Returns progress/work limits; no workbook mutation. Without a job returns idle/budget information.", always, status),
        cmd!(query "analysis.step", "Advance analysis", ["Data", "Analysis"], None,
            "{job: id, trials?: 512 (1–2048)}. Advances a bounded cooperative batch; never publishes a report.", has_doc, step),
        cmd!(query "analysis.cancel", "Cancel analysis", ["Data", "Analysis"], None,
            "{job: id}. Discards pending work without a report, source edits or undo entry. A stale id never cancels a later job.", has_doc, cancel),
        publication,
    ]
}

/// Requires an explicit matching identity before a caller can advance, cancel or publish a job.
fn matching<'a>(s: &'a Session, p: &Json, cmd: &str) -> Result<&'a Job> {
    let id = p.get("job").and_then(Json::as_u64).filter(|id| *id > 0).ok_or_else(|| bad(cmd, "give a positive job id"))?;
    s.analysis_job.as_ref().filter(|job| job.id == id).ok_or_else(|| bad(cmd, "job is absent or stale"))
}

/// Validates an isolated job before retaining it; a session permits only one pending job.
fn start(s: &mut Session, p: &Json) -> Result<Json> {
    if s.analysis_job.is_some() {
        return Err(bad("analysis.start", "finish or cancel the current analysis first"));
    }
    let uid = s.doc()?.uid;
    if s.depth > 1 || s.in_group(uid) || s.recorder.is_some() || s.recording.is_some() {
        return Err(bad("analysis.start", "start incremental analysis outside nested scripts, undo groups and action recording"));
    }
    if p.get("command").is_some_and(|v| v.as_str() != Some("data.monteCarlo")) {
        return Err(bad("analysis.start", "the incremental command currently supports data.monteCarlo"));
    }
    let params = p.get("params").filter(|v| v.is_object()).ok_or_else(|| bad("analysis.start", "give a params object"))?;
    let auto = match p.get("autoAdvance") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| bad("analysis.start", "autoAdvance must be boolean"))?,
    };
    let id = s.next_analysis_job.checked_add(1).ok_or_else(|| bad("analysis.start", "job identity exhausted"))?;
    let mut job = Job::prepare(s, params)?;
    job.id = id;
    job.auto_advance = auto;
    let progress = job.status();
    s.next_analysis_job = id;
    s.analysis_job = Some(job);
    Ok(progress)
}

/// Reads progress without advancing trials or committing any workbook/history changes.
fn status(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("job").is_some() {
        return Ok(matching(s, p, "analysis.status")?.status());
    }
    if let Some(job) = &s.analysis_job {
        return Ok(job.status());
    }
    let remaining = s.active().map(|d| d.analysis_budget.remaining()).transpose()?;
    Ok(
        json!({"state":"idle","sharedWorkRemaining":remaining,"maxRunWork":crate::workload::MAX_RUN_WORK,"sharedWindowWork":crate::workload::MAX_WINDOW_WORK,"windowSeconds":60}),
    )
}

/// Advances a portable batch; terminal errors drop only the isolated job, never source cells.
fn step(s: &mut Session, p: &Json) -> Result<Json> {
    matching(s, p, "analysis.step")?;
    let trials = match p.get("trials") {
        None => 512,
        Some(v) => v.as_u64().filter(|n| (1..=2048).contains(n)).ok_or_else(|| bad("analysis.step", "trials must be between 1 and 2048"))? as usize,
    };
    let mut job = s.analysis_job.take().ok_or_else(|| bad("analysis.step", "job is absent"))?;
    job.check_source(s)?;
    job.advance(trials)?;
    let progress = job.status();
    s.analysis_job = Some(job);
    Ok(progress)
}

/// Cancels a pending identity before publication; no journal or undo entry is added.
fn cancel(s: &mut Session, p: &Json) -> Result<Json> {
    let progress = matching(s, p, "analysis.cancel")?.status();
    let job = s.analysis_job.take().ok_or_else(|| bad("analysis.cancel", "job is absent"))?;
    if !job.cancellation().cancel() {
        return Err(bad("analysis.cancel", "publication already started; cancellation is too late"));
    }
    Ok(json!({"job":job.id,"state":"cancelled","completed":progress["completed"],"sourceUnchanged":true}))
}

/// Publishes only a complete, unchanged-source job. Session::execute routes this command to
/// the source document before taking its undo snapshot, preserving normal history semantics.
fn finish(s: &mut Session, p: &Json) -> Result<Json> {
    let job = matching(s, p, "analysis.finish")?;
    if !job.ready() {
        return Err(bad("analysis.finish", "job is not ready; advance or cancel it"));
    }
    if let Err(error) = job.check_source(s) {
        drop(s.analysis_job.take());
        return Err(error);
    }
    let job = s.analysis_job.take().ok_or_else(|| bad("analysis.finish", "job is absent"))?;
    let config = job.configuration();
    let result = job.publish(s)?;
    s.push_journal("data.monteCarlo", super::journal_params(&config));
    Ok(result)
}

impl Session {
    /// Whether a cooperative analysis owns pending work, without exposing its mutable state.
    pub fn analysis_active(&self) -> bool {
        self.analysis_job.is_some()
    }

    /// A portable progress snapshot for front ends; querying it never advances or publishes work.
    pub fn analysis_progress(&self) -> Option<Json> {
        self.analysis_job.as_ref().map(Job::status)
    }

    /// A thread-safe cancellation signal for embedding hosts, including during a long trial.
    pub fn analysis_cancellation(&self) -> Option<crate::workload::Cancellation> {
        self.analysis_job.as_ref().map(Job::cancellation)
    }

    /// UI dialogs may run cooperatively unless an existing recording/group requires synchronous
    /// command semantics. Programmatic data.monteCarlo calls retain their synchronous contract.
    pub fn can_background_analysis(&self) -> bool {
        self.active().is_some_and(|d| !self.in_group(d.uid)) && self.recorder.is_none() && self.recording.is_none() && !self.analysis_active()
    }
}

#[cfg(test)]
mod tests;
