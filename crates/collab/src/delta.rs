//! The authoritative cell delta (ADR-0001: the client applies values, it does not recompute).
//!
//! The engine names every cell a command changed, including the formulas it recalculated because
//! of the edit ([`gridcraft_engine::Session::take_changed`]). This module turns that list into
//! per-sheet messages a client can apply without running the command: address, value, formula
//! text and interned style id.

use gridcraft_calc::Key;
use gridcraft_model::Workbook;

use crate::SheetKeys;
use crate::protocol::{CellChange, SheetDelta};

/// The changed cells of `changed`, grouped by the session key of their sheet. A cell that no
/// longer exists is reported with an empty value: the client clears it.
pub fn delta_for(workbook: &Workbook, keys: &SheetKeys, changed: &[Key]) -> Vec<SheetDelta> {
    let mut out: Vec<SheetDelta> = Vec::new();
    for (index, at) in changed {
        let Some(sheet) = workbook.sheet(*index) else { continue };
        let Some(key) = keys.key_for_index(workbook, *index) else { continue };
        let sheet_key = key.to_string();
        let slot = match out.iter().position(|d| d.sheet == sheet_key) {
            Some(i) => i,
            None => {
                out.push(SheetDelta { sheet: sheet_key, cells: Vec::new(), objects_deleted: Vec::new(), objects_created: Vec::new() });
                out.len() - 1
            }
        };
        let cell = sheet.cell(*at);
        // `Sheet::value` reads spilled dynamic-array results too: a spilled cell has no `Cell`
        // of its own but is part of what the user sees.
        let value = serde_json::to_value(sheet.value(*at)).unwrap_or(serde_json::Value::Null);
        let formula = cell.and_then(|c| c.formula.as_ref()).map(|f| f.text.clone());
        let py = cell.and_then(|c| c.py.as_ref()).map(|p| p.code.clone());
        let (array, dynamic) = cell.and_then(|c| c.formula.as_ref()).map_or((None, false), |f| (f.array, f.dynamic));
        let style = Some(sheet.style_id(*at).0);
        out[slot].cells.push(CellChange { a1: at.a1(), value, formula, style, py, array, dynamic });
    }
    out
}
