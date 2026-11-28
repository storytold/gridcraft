//! What saving the workbook would lose: the notice an interactive save asks about first, and the
//! `warnings` of a programmatic one.

use serde_json::{Value as Json, json};

use super::{Result, Session, str_param};
use crate::io::FileKind;

/// Sheet names a warning spells out before it only counts the rest.
const MAX_NAMES: usize = 10;

/// What a save would lose. Equal when the same sheets lose something, whatever they are called or
/// wherever they are, so agreeing to a loss is agreeing to exactly these sheets; the messages name
/// only some of them.
#[derive(Clone, Debug, Default)]
pub struct SaveLosses {
    /// The sheets left out of whole-workbook printing: their identity and name, in tab order.
    excluded: Vec<(u64, String)>,
}

impl PartialEq for SaveLosses {
    fn eq(&self, other: &SaveLosses) -> bool {
        let sheets = |l: &SaveLosses| l.excluded.iter().map(|(uid, _)| *uid).collect::<std::collections::BTreeSet<_>>();
        sheets(self) == sheets(other)
    }
}

impl SaveLosses {
    pub fn is_empty(&self) -> bool {
        self.excluded.is_empty()
    }

    /// What to tell the user, one message for each kind of loss.
    pub fn messages(&self) -> Vec<String> {
        if self.is_empty() {
            return vec![];
        }
        let mut list = self.excluded.iter().take(MAX_NAMES).map(|(_, name)| name.as_str()).collect::<Vec<_>>().join(", ");
        if let Some(more) = self.excluded.len().checked_sub(MAX_NAMES).filter(|n| *n > 0) {
            list.push_str(&format!(" (and {more} more)"));
        }
        vec![format!(
            "An .xlsx file can't keep that a sheet is left out of whole-workbook printing. Once the file is opened again, these sheets print with the others: {list}. A GridCraft workbook (.json) keeps it."
        )]
    }
}

/// What the save command `id` (`file.save`, `file.saveAs`) with these params would lose; nothing
/// when the format keeps everything, and when the command would ask for a file first (Save As
/// with no path always does, so nothing is known about what it writes). CSV and HTML are copies
/// that never become the workbook's file, so only the .xlsx family is asked about.
pub fn save_losses(s: &Session, id: &str, p: &Json) -> SaveLosses {
    let Some(d) = s.active() else { return SaveLosses::default() };
    let own = d.path.as_deref().filter(|_| id != "file.saveAs");
    let Some(path) = str_param(p, "path").or(own) else { return SaveLosses::default() };
    if super::save_kind(d, path) == Some(FileKind::Xlsx) { xlsx_losses(&d.wb) } else { SaveLosses::default() }
}

/// What encoding the active workbook as .xlsx (a download) would lose.
pub fn xlsx_save_losses(s: &Session) -> SaveLosses {
    s.active().map(|d| xlsx_losses(&d.wb)).unwrap_or_default()
}

fn xlsx_losses(wb: &gridcraft_model::Workbook) -> SaveLosses {
    SaveLosses { excluded: wb.sheets.iter().filter(|sh| sh.print.excluded).map(|sh| (sh.uid.get(), sh.name.clone())).collect() }
}

/// Runs a save and adds what it lost to the result as `warnings`. A save with no loss, and one
/// that only asked for a file, return what they did before.
pub(super) fn reported(s: &mut Session, id: &str, p: &Json, save: fn(&mut Session, &Json) -> Result<Json>) -> Result<Json> {
    let losses = save_losses(s, id, p);
    with_warnings(save(s, p)?, losses)
}

/// [`reported`] for `file.saveBytes`, whose format is named instead of a path.
pub(super) fn reported_bytes(s: &mut Session, p: &Json, save: fn(&mut Session, &Json) -> Result<Json>) -> Result<Json> {
    let name = format!("Book.{}", str_param(p, "format").unwrap_or("xlsx"));
    let losses = if FileKind::from_path(&name) == Some(FileKind::Xlsx) { xlsx_losses(&s.doc()?.wb) } else { SaveLosses::default() };
    with_warnings(save(s, p)?, losses)
}

fn with_warnings(mut r: Json, losses: SaveLosses) -> Result<Json> {
    if let Some(o) = r.as_object_mut().filter(|_| !losses.is_empty()) {
        o.insert("warnings".into(), json!(losses.messages()));
    }
    Ok(r)
}
