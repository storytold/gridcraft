//! Typing into a PivotTable's report: a header cell renames what it names, and the other cells
//! can't be typed over.

use gridcraft_core::CellRef;

use super::fields::value_settings;
use super::layout::layout;
use super::{EngineError, Json, Result, Session, json, other, pv};

/// Why a typed item, total or field label is refused.
const RENAME_IN_CELL: &str = "Renaming PivotTable items and fields in their cells isn't supported yet.";

/// `cell.set` on cell `at` of PivotTable `pi`'s report on sheet `si`. A value field's caption
/// ("Sum of Sales") takes `input` as the field's custom name, and the Compact layout's Row Labels
/// and Column Labels cells take it as their caption, through the commands the PivotTable Fields
/// pane uses, so the checks and the one undo step are theirs. Typing what a header already says
/// changes nothing.
pub(crate) fn type_into_report(s: &mut Session, si: usize, pi: usize, at: CellRef, input: &str) -> Result<Json> {
    let d = s.doc()?;
    let pt = d.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).ok_or_else(|| other("There's no such PivotTable."))?;
    let area = pt.last_range.ok_or_else(|| other("The PivotTable has no report."))?;
    let cell = pv::report_cell(&d.wb, pt, si, at.row - area.start.row, at.col - area.start.col).map_err(other)?;
    let target = json!({"pivot": pt.id});
    let mut q = target.as_object().cloned().unwrap_or_default();
    let unchanged = match cell {
        pv::ReportCell::Header(pv::HeaderRole::Value(vi)) => {
            q.insert("index".into(), json!(vi));
            q.insert("name".into(), json!(input));
            pt.values.get(vi).is_some_and(|v| v.name == input)
        }
        pv::ReportCell::Header(pv::HeaderRole::RowCaption) => {
            q.insert("rowHeaderCaption".into(), json!(input));
            pt.row_header_caption.as_deref().unwrap_or("Row Labels") == input
        }
        pv::ReportCell::Header(pv::HeaderRole::ColCaption) => {
            q.insert("colHeaderCaption".into(), json!(input));
            pt.col_header_caption.as_deref().unwrap_or("Column Labels") == input
        }
        pv::ReportCell::Label => return Err(other(RENAME_IN_CELL)),
        pv::ReportCell::Other => return Err(EngineError::Other(crate::cmd::REPORT_CELLS.into())),
    };
    if !unchanged {
        let q = Json::Object(q);
        match cell {
            pv::ReportCell::Header(pv::HeaderRole::Value(_)) => value_settings(s, &q)?,
            _ => layout(s, &q)?,
        };
    }
    Ok(json!({"cell": at.a1(), "value": input}))
}
