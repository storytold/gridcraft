//! Copy, cut and paste of floating objects. The object waits in [`Session::object_clipboard`], and
//! only the workbook it came from can paste it. Only pictures are copied for now.

use std::sync::atomic::{AtomicU64, Ordering};

use gridcraft_model::{Image, ObjKind, Sheet, Workbook};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, PROTECTED, Result, Session, bad, cell_param, cmd, edit, has_doc, str_param, target_sheet, u32_param};
use crate::python::links;

/// `object.paste` in any workbook but the one the object came from. No floating object crosses
/// workbooks, whatever its kind.
pub const OTHER_WORKBOOK: &str = "A floating object can't be pasted into another workbook.";

/// Tells one copy from another in a workbook's [`gridcraft_model::Workbook::held_link`].
static NEXT_COPY: AtomicU64 = AtomicU64::new(1);

/// What is on the object clipboard.
#[derive(Clone, Debug)]
pub enum CopiedObject {
    Image(Image),
}

/// A floating object copied or cut.
#[derive(Clone, Debug)]
pub struct ObjectClipboard {
    /// The workbook it came from.
    pub doc_uid: u64,
    pub object: CopiedObject,
}

/// Where the Python cell of a copied picture was when it was copied. The first edit after that takes
/// the reference into the workbook, where it follows the cell ([`commit`](super::commit),
/// [`gridcraft_model::Workbook::held_link`]): a copy itself changes nothing in the workbook.
#[derive(Clone, Debug)]
pub(crate) struct HeldLink {
    copy: u64,
    reference: String,
    code: String,
}

impl HeldLink {
    /// Whether the reference can be trusted in a workbook that has none of its own yet: that is the
    /// workbook as copied, or an earlier one Undo returned to, where the same text may name another
    /// cell. It holds while the cell it names has the code it had.
    fn holds_in(&self, wb: &Workbook) -> bool {
        links::code_at(wb, &self.reference).is_some_and(|code| code == self.code)
    }

    /// Gives a workbook about to be edited the reference to follow, unless it has it already.
    pub(crate) fn seed(&self, wb: &mut Workbook) {
        if wb.held_link.as_ref().is_none_or(|(copy, _)| *copy != self.copy) && self.holds_in(wb) {
            wb.held_link = Some((self.copy, self.reference.clone()));
        }
    }

    /// Where the cell is in `wb`, when that is known.
    fn now_in(&self, wb: &Workbook) -> Option<String> {
        match &wb.held_link {
            Some((copy, reference)) if *copy == self.copy => Some(reference.clone()),
            _ => self.holds_in(wb).then(|| self.reference.clone()),
        }
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "object.copy",
            "Copy Object",
            [],
            None,
            "{kind?: image, id, sheet?} → {image}: puts the picture on the object clipboard and changes nothing in the workbook. Only pictures can be copied for now",
            has_doc,
            copy
        ),
        cmd!(
            "object.cut",
            "Cut Object",
            [],
            None,
            "{kind?: image, id, sheet?} → {image}: puts the picture on the object clipboard and removes it, as one undoable step",
            has_doc,
            cut
        ),
        cmd!(
            "object.paste",
            "Paste Object",
            [],
            None,
            "{at?: \"C3\"} → {image, linked}: adds a new picture at `at` or the active cell of the active sheet, as one undoable step, and can be repeated. It stays linked to the Python cell it came from. An error in any workbook but the one it was copied from",
            has_doc,
            paste
        ),
    ]
}

/// The picture a `{kind?, id, sheet?}` command names, and its sheet.
fn picture_param(s: &Session, p: &Json, command: &str) -> Result<(usize, Image)> {
    let kind = str_param(p, "kind").unwrap_or("image");
    match ObjKind::parse(kind) {
        Some(ObjKind::Image) => {}
        Some(_) => return Err(EngineError::Other("Only pictures can be copied for now.".into())),
        None => return Err(bad(command, "kind must be chart, image, shape, slicer, equation or control")),
    }
    let id = u32_param(p, "id").ok_or_else(|| bad(command, "missing `id`"))?;
    let sheet = target_sheet(s, p)?;
    let image = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.images.iter().find(|o| o.id == id));
    Ok((sheet, image.ok_or_else(|| bad(command, "no such picture"))?.clone()))
}

/// Sheet protection locks drawing objects, as for `shape.setText`.
pub(super) fn refuse_when_protected(s: &Session, sheet: usize) -> Result<()> {
    if s.doc()?.wb.sheet(sheet).is_some_and(Sheet::is_protected) {
        return Err(EngineError::Other(PROTECTED.into()));
    }
    Ok(())
}

/// One clipboard at a time: this replaces cells copied before it, and an object copied before it.
fn hold(s: &mut Session, image: Image) -> Result<()> {
    s.forget_object_clipboard();
    s.clipboard = None;
    let copy = NEXT_COPY.fetch_add(1, Ordering::Relaxed);
    let d = s.doc_mut()?;
    d.held_link = image.python_source.as_deref().and_then(|r| Some(HeldLink { copy, reference: r.to_string(), code: links::code_at(&d.wb, r)? }));
    let doc_uid = d.uid;
    s.object_clipboard = Some(ObjectClipboard { doc_uid, object: CopiedObject::Image(image) });
    Ok(())
}

fn copy(s: &mut Session, p: &Json) -> Result<Json> {
    let (_, image) = picture_param(s, p, "object.copy")?;
    let id = image.id;
    hold(s, image)?;
    Ok(json!({"image": id}))
}

fn cut(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, image) = picture_param(s, p, "object.cut")?;
    refuse_when_protected(s, sheet)?;
    let id = image.id;
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.images.retain(|o| o.id != id);
        Ok(())
    })?;
    hold(s, image)?;
    Ok(json!({"image": id}))
}

fn paste(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(clip) = s.object_clipboard.clone() else { return Err(EngineError::Other("Nothing to paste.".into())) };
    let d = s.doc()?;
    if clip.doc_uid != d.uid {
        return Err(EngineError::Other(OTHER_WORKBOOK.into()));
    }
    let sheet = d.wb.active_sheet();
    refuse_when_protected(s, sheet)?;
    let at = cell_param(s, p, "at")?.unwrap_or(d.selection.active);
    let id = d.wb.next_object_id();
    match &clip.object {
        CopiedObject::Image(image) => {
            let mut picture = image.clone();
            picture.id = id;
            picture.anchor.cell = at;
            picture.anchor.dx = 0.0;
            picture.anchor.dy = 0.0;
            let reference = d.held_link.as_ref().and_then(|held| held.now_in(&d.wb));
            links::as_pasted(&d.wb, &mut picture, reference.as_deref());
            let linked = picture.python_source.is_some();
            edit(s, |cx| {
                cx.sheet_mut(sheet)?.images.push(picture);
                Ok(json!({"image": id, "linked": linked}))
            })
        }
    }
}
