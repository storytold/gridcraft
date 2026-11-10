//! Which commands may travel, and how they conflict (ADR-0004).
//!
//! The allowlist is a property of the server, not of the client: a command outside it is refused
//! with `forbidden` before the engine sees it. Two classes:
//!
//! - [`Class::Merge`] — the edit does not shift addresses; the server orders it and every client
//!   receives the authoritative delta.
//! - [`Class::Structural`] — an insert, a delete, a sheet move, a table edit or a sort that
//!   changes the shape of the sheet: it rewrites formulas and object anchors across the workbook
//!   and runs a full recalculation (`crates/engine/src/cmd/sheet.rs`, `crates/engine/src/cmd/data/sort.rs`),
//!   so it is exclusive and requires `base_version == head`.

/// The conflict class of an operation (ADR-0004).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Merge,
    Structural,
}

/// Never replicated: files, application chrome, printing, code, and the two commands that read a
/// local history (undo/redo are per-user operations the room turns into new commands).
const DENIED_PREFIXES: &[&str] = &["file.", "app.", "ui.", "view.", "print.", "macro.", "automate.", "python.", "mcp."];

/// Individual refusals that are not covered by a denied prefix.
const DENIED_IDS: &[&str] =
    &["edit.undo", "edit.redo", "data.getData", "data.fromTextCsv", "link.open", "file.print", "ui.printSubmit", "ui.printSettings"];

/// Commands that change the shape of a sheet or the set of sheets. A sort moves cells, their
/// styles, row heights and object anchors, and the engine rewrites formulas and runs a full
/// recalculation for it exactly as it does for a row insert (`cx.structural`), so a merge edit
/// that was accepted against the old addresses may land on another row: it is structural here too.
const STRUCTURAL_PREFIXES: &[&str] = &["home.insert", "home.delete", "sheet.move", "sheet.rename", "table.", "data.sort"];

/// Structural commands that no prefix of theirs covers. `data.subtotal` writes total rows below
/// each group, which shifts every row under it, and `data.removeDuplicates` compacts the kept rows
/// upwards: a merge edit accepted against the old addresses lands on another row, and an undo of a
/// subtotal leaves its outline behind. Classification happens before the operation runs, so the
/// room still requires `base_version == head` for them (CodeRabbit 5846575558, `ops.rs:32`).
const STRUCTURAL_IDS: &[&str] = &["data.subtotal", "data.removeDuplicates"];

/// Groups allowed to travel in v1. Anything outside them is refused even if it looks harmless:
/// a new command group has to be reviewed before it replicates.
const ALLOWED_PREFIXES: &[&str] = &[
    "cell.",
    "range.",
    "edit.",
    "home.",
    "data.",
    "formulas.",
    "review.",
    "table.",
    "sheet.",
    "insert.",
    "object.",
    "arrange.",
    "chart.",
    "pivot.",
    "slicer.",
    "control.",
    "draw.",
    "stash.",
];

/// The class of `id`, or `None` when the command may not travel in a session.
pub fn classify(id: &str) -> Option<Class> {
    if DENIED_IDS.contains(&id) || DENIED_PREFIXES.iter().any(|p| id.starts_with(p)) {
        return None;
    }
    if !ALLOWED_PREFIXES.iter().any(|p| id.starts_with(p)) {
        return None;
    }
    if STRUCTURAL_IDS.contains(&id) || STRUCTURAL_PREFIXES.iter().any(|p| id.starts_with(p)) {
        return Some(Class::Structural);
    }
    Some(Class::Merge)
}
