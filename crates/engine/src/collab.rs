//! The seam a collaboration layer plugs into.
//!
//! The engine knows that something may want to hear about a command it just
//! ran. It deliberately knows nothing about what that something is. A relay, a
//! log file and a test double are all the same shape from here.
//!
//! This is the only thing the engine gains for shared editing. Ordering,
//! locking, transport and credentials all live outside it.

use serde_json::Value as Json;

/// Receives commands that originated on this machine.
///
/// Called after a journaled command has succeeded, so an implementation sees
/// exactly the `(id, params)` pairs that changed the workbook. Commands that
/// only move the view or open a dialog are not journaled and never arrive
/// here.
///
/// A command applied through [`crate::Session::apply_remote`] is not reported,
/// which is what stops two peers echoing one edit back and forth forever.
pub trait CollabSink: Send + Sync {
    /// Forward one local command. Must not block: it runs inside the command.
    fn on_local(&self, id: &str, params: &Json);
}
