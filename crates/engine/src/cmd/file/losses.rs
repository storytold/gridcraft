//! What saving the workbook would lose: the notice an interactive save asks about first, and the
//! `warnings` of a programmatic one.

use serde_json::{Value as Json, json};

use super::{Result, Session, str_param};
use crate::io::FileKind;

/// Sheet names a warning spells out before it only counts the rest.
const MAX_NAMES: usize = 10;

/// What `file.save` or `file.saveAs` with these params would lose, one message each; empty when
/// the format keeps everything, and when the save would ask for a file first. CSV and HTML are
/// copies that never become the workbook's file, so only the .xlsx family is asked about.
pub fn save_losses(s: &Session, p: &Json) -> Vec<String> {
    let Some(d) = s.active() else { return vec![] };
    let Some(path) = str_param(p, "path").or(d.path.as_deref()) else { return vec![] };
    if super::save_kind(d, path) == Some(FileKind::Xlsx) { xlsx_losses(&d.wb) } else { vec![] }
}

fn xlsx_losses(wb: &gridcraft_model::Workbook) -> Vec<String> {
    let names: Vec<&str> = wb.sheets.iter().filter(|sh| sh.print.excluded).map(|sh| sh.name.as_str()).collect();
    if names.is_empty() {
        return vec![];
    }
    let mut list = names.iter().take(MAX_NAMES).copied().collect::<Vec<_>>().join(", ");
    if let Some(more) = names.len().checked_sub(MAX_NAMES).filter(|n| *n > 0) {
        list.push_str(&format!(" (and {more} more)"));
    }
    vec![format!(
        "An .xlsx file can't keep that a sheet is left out of whole-workbook printing. Once the file is opened again, these sheets print with the others: {list}. A GridCraft workbook (.json) keeps it."
    )]
}

/// Runs a save and adds what it lost to the result as `warnings`. A save with no loss, and one
/// that only asked for a file, return what they did before.
pub(super) fn reported(s: &mut Session, p: &Json, save: fn(&mut Session, &Json) -> Result<Json>) -> Result<Json> {
    let losses = save_losses(s, p);
    with_warnings(save(s, p)?, losses)
}

/// [`reported`] for `file.saveBytes`, whose format is named instead of a path.
pub(super) fn reported_bytes(s: &mut Session, p: &Json, save: fn(&mut Session, &Json) -> Result<Json>) -> Result<Json> {
    let name = format!("Book.{}", str_param(p, "format").unwrap_or("xlsx"));
    let losses = if FileKind::from_path(&name) == Some(FileKind::Xlsx) { xlsx_losses(&s.doc()?.wb) } else { vec![] };
    with_warnings(save(s, p)?, losses)
}

fn with_warnings(mut r: Json, losses: Vec<String>) -> Result<Json> {
    if let Some(o) = r.as_object_mut().filter(|_| !losses.is_empty()) {
        o.insert("warnings".into(), json!(losses));
    }
    Ok(r)
}
