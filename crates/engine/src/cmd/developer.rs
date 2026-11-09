//! Reserved Developer-tab ids: the tab's Excel entries whose features are not built yet (#2960).
//!
//! They are registered so a programmatic caller gets a defined [`EngineError::NotImplemented`]
//! (`NOT_IMPLEMENTED: …`) instead of "unknown command", and so `engine.commands` reports them
//! with `status: "not_implemented"` and `enabled: false`. They run nothing, are not in any menu,
//! and [`crate::catalog::parity`] counts them as missing. [`crate::catalog::RESERVED`] is the
//! list; the catalog rows supply each label.

use serde_json::Value as Json;

use super::{CommandSpec, EngineError, NOT_IMPLEMENTED_REASON, Result, Session, Status, always};
use crate::catalog;

pub fn specs() -> Vec<CommandSpec> {
    catalog::RESERVED
        .iter()
        .map(|&id| CommandSpec {
            id,
            label: catalog::label(id).unwrap_or(id),
            // Drawn by the Developer tab itself; no other menu lists an unimplemented entry.
            menu: &[],
            shortcut: None,
            shortcut_mac: None,
            params: "{} → NOT_IMPLEMENTED (no parameters are accepted yet)",
            enabled: always,
            run: reserved,
            journal: false,
            undoable: false,
            status: Status::NotImplemented,
        })
        .collect()
}

fn reserved(_: &mut Session, _: &Json) -> Result<Json> {
    // `Session::execute` refuses a `NotImplemented` spec before it calls this, with the command's
    // id in the message; this body keeps the error defined for a caller that reaches it directly.
    Err(EngineError::NotImplemented(format!("this command {NOT_IMPLEMENTED_REASON}")))
}
