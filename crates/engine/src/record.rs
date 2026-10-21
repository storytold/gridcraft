//! The recording buffer: the commands a person (or an agent) ran while recording, each with the
//! document, sheet and selection it ran in.
//!
//! Only top-level calls are kept. A command that runs other commands (Custom Views › Show, a
//! script) is one step, so playing the recording back does not run the inner commands twice. The
//! buffer is separate from the journal, so trimming the journal never loses recorded steps, and
//! secret parameters are left out as they are in the journal ([`crate::cmd::journal_params`]).

use serde::Serialize;
use serde_json::Value as Json;

use crate::Selection;

/// The most steps one recording keeps; later ones are counted in [`Recorder::dropped`].
pub const RECORD_LIMIT: usize = 100_000;

/// One recorded command and where it ran.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedStep {
    /// Uid of the document that was active when the command ran (0 when none was open).
    pub doc: u64,
    /// That document's active sheet (index and name) just before the command ran.
    pub sheet: usize,
    pub sheet_name: String,
    /// That document's selection just before the command ran.
    pub selection: Selection,
    /// The command id and its parameters, without secrets.
    pub id: String,
    pub params: Json,
}

/// The steps recorded since recording started.
#[derive(Clone, Debug, Default)]
pub struct Recorder {
    pub steps: Vec<RecordedStep>,
    /// Steps not kept because the recording reached [`RECORD_LIMIT`].
    pub dropped: usize,
}

impl Recorder {
    /// Adds a step, or counts it as dropped once the buffer is full.
    pub fn push(&mut self, step: RecordedStep) {
        if self.steps.len() < RECORD_LIMIT {
            self.steps.push(step);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}
