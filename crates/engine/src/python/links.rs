//! Floating pictures linked to the Python cell that gives them.
//!
//! A linked picture shows the current picture of its source cell, which it names by a reference that
//! follows the cell as any reference does. It exists only while the source can give it a picture. It
//! goes when the cell stops being a Python cell or is removed ([`prune`], which runs inside the
//! command that did it, so that command's undo step holds it), and when a run of the cell finishes
//! with a result that isn't a picture ([`after_result`]).
//!
//! Undo restores whole workbooks, and a result makes no undo step, so the snapshot of the edit that
//! caused a run holds the old code, the old result and the picture together. Only a run with no edit
//! behind it has no step to belong to, and makes one when it takes a picture away.

use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_core::addr::{col_to_letters, quote_sheet, unquote_sheet};
use gridcraft_model::{Image, Workbook};

use crate::DocState;

/// The reference a linked picture holds for a cell.
pub(crate) fn reference(sheet_name: &str, at: CellRef) -> String {
    format!("{}!${}${}", quote_sheet(sheet_name), col_to_letters(at.col), at.row.saturating_add(1))
}

/// The sheet and cell a reference names. `None` for one that names nothing here, such as `#REF!`.
pub(crate) fn source_of(wb: &Workbook, reference: &str) -> Option<(usize, CellRef)> {
    let (sheet, cell) = reference.rsplit_once('!')?;
    let at = CellRef::parse(&cell.replace('$', ""))?;
    Some((wb.sheet_index(&unquote_sheet(sheet))?, at))
}

fn names_a_python_cell(wb: &Workbook, reference: &str) -> bool {
    source_of(wb, reference).is_some_and(|(sheet, at)| wb.sheet(sheet).and_then(|s| s.cell(at)).is_some_and(|c| c.py.is_some()))
}

/// Every linked picture whose source is gone. Costs nothing on a workbook with no linked pictures.
pub(crate) fn prune(d: &mut DocState) -> bool {
    let gone: Vec<(usize, u32)> =
        d.wb.sheets
            .iter()
            .enumerate()
            .flat_map(|(i, sh)| {
                sh.images.iter().filter(|o| o.python_source.as_deref().is_some_and(|r| !names_a_python_cell(&d.wb, r))).map(move |o| (i, o.id))
            })
            .collect();
    remove(d, &gone)
}

fn remove(d: &mut DocState, pictures: &[(usize, u32)]) -> bool {
    if pictures.is_empty() {
        return false;
    }
    let done = crate::cmd::commit(d, |cx| {
        for (sheet, id) in pictures {
            cx.sheet_mut(*sheet)?.images.retain(|o| o.id != *id);
        }
        Ok(())
    });
    if let Err(e) = done {
        log::warn!("Python cells: {e}");
    }
    d.revision = d.revision.saturating_add(1);
    true
}

/// The pictures linked to the cell `at` on `sheet`, on any sheet.
fn linked_to(wb: &Workbook, sheet: usize, at: CellRef) -> Vec<(usize, u32)> {
    wb.sheets
        .iter()
        .enumerate()
        .flat_map(|(i, sh)| {
            sh.images.iter().filter(|o| o.python_source.as_deref().and_then(|r| source_of(wb, r)) == Some((sheet, at))).map(move |o| (i, o.id))
        })
        .collect()
}

/// A run of the cell finished and its result has been written. With a picture, every linked picture
/// shows it. With a result that isn't one, they go. An error leaves them, marked ([`problem`]): a
/// typing error doesn't destroy a picture someone placed. `before` is the workbook before the result,
/// and `edit_behind` whether an edit caused this run, whose undo step already holds them.
pub(super) fn after_result(d: &mut DocState, sheet: usize, at: CellRef, before: &Arc<Workbook>, edit_behind: bool) -> bool {
    let linked = linked_to(&d.wb, sheet, at);
    if linked.is_empty() {
        return false;
    }
    let Some(py) = d.wb.sheet(sheet).and_then(|s| s.cell(at)).and_then(|c| c.py.clone()) else { return false };
    if let Some(image) = py.image() {
        return show(d, &linked, image);
    }
    if py.output.is_none() || py.failed.is_some() {
        return false;
    }
    let removed = remove(d, &linked);
    if removed && !edit_behind {
        let selection = d.selection.clone();
        crate::push_history(&mut d.undo, "Run Python Cell".to_string(), settled(before, sheet, at), &d.wb, selection);
        d.redo.clear();
    }
    removed
}

/// `before` with the cell no longer waiting for the run that was under way then: what Undo returns to
/// is the earlier result, and nothing runs to replace it.
fn settled(before: &Arc<Workbook>, sheet: usize, at: CellRef) -> Arc<Workbook> {
    let mut wb = before.clone();
    if let Some(cell) = Arc::make_mut(&mut wb).sheet_mut(sheet).and_then(|s| s.cells.get_mut(at))
        && let Some(py) = cell.py.as_mut()
    {
        let py = Arc::make_mut(py);
        py.busy = false;
        if let Some(value) = py.result_value() {
            cell.value = value;
        }
    }
    wb
}

fn show(d: &mut DocState, linked: &[(usize, u32)], image: &gridcraft_model::PyImage) -> bool {
    let data = Arc::new(image.picture.data.clone());
    let done = crate::cmd::commit(d, |cx| {
        for (sheet, id) in linked {
            let Some(o) = cx.sheet_mut(*sheet)?.images.iter_mut().find(|o| o.id == *id) else { continue };
            take_image(o, image, &data);
        }
        Ok(())
    });
    if let Err(e) = done {
        log::warn!("Python cells: {e}");
    }
    d.revision = d.revision.saturating_add(1);
    true
}

fn take_image(o: &mut Image, image: &gridcraft_model::PyImage, data: &Arc<Vec<u8>>) {
    if *o.data != **data {
        o.data = data.clone();
        o.mime.clone_from(&image.picture.mime);
        o.rendered = None;
        // A figure that can be drawn again keeps the shape it was given. Any other picture keeps its
        // width, and its height follows its proportions.
        if image.floor.is_none() && image.width > 0 {
            o.anchor.height = o.anchor.width * image.height as f32 / image.width as f32;
        }
    }
}

/// The code of the Python cell a reference names.
pub(crate) fn code_at(wb: &Workbook, reference: &str) -> Option<String> {
    let (sheet, at) = source_of(wb, reference)?;
    Some(wb.sheet(sheet)?.cell(at)?.py.as_deref()?.code.clone())
}

/// A picture from the object clipboard as it arrives in `wb`, where `reference` says its cell is now.
/// While the cell can give it a picture it stays linked and shows what the cell shows now, which may
/// have changed since the copy; a run under way or one that failed leaves it as it is, as for a
/// picture on a sheet. With no such cell left there is nothing to link to, and the picture that was
/// asked for arrives without a link, as one from a file does: the pictures linked to a cell go when
/// it does, and this one wasn't there to go.
pub(crate) fn as_pasted(wb: &Workbook, picture: &mut Image, reference: Option<&str>) {
    let source = reference.and_then(|r| source_of(wb, r)).and_then(|(sheet, at)| wb.sheet(sheet)?.cell(at)?.py.clone());
    let (Some(py), Some(reference)) = (source, reference) else {
        picture.python_source = None;
        return;
    };
    picture.python_source = Some(reference.to_string());
    if let Some(image) = py.image() {
        take_image(picture, image, &Arc::new(image.picture.data.clone()));
    } else if py.output.is_some() && py.failed.is_none() {
        picture.python_source = None;
    }
}

/// A figure to draw again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DueRender {
    pub for_what: RenderFor,
    /// The cell's place in the worker.
    pub place: [u32; 3],
    /// The size to draw it at, in the units of [`gridcraft_model::PyImage::width`].
    pub size: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RenderFor {
    /// A linked picture: its sheet and object id.
    Picture(usize, u32),
    /// A cell that draws its picture inside it.
    Cell(usize, CellRef),
}

pub(super) struct RenderFlight {
    pub id: u64,
    pub due: DueRender,
}

/// How many of a figure's own pixels one unit of an anchor stands for. A plot placed over the cells is
/// at most 640 wide ([`new_picture`]), and its text keeps the size that gives it, however it is resized.
fn anchor_scale(image: &gridcraft_model::PyImage) -> f32 {
    (image.width as f32 / 640.0).max(1.0)
}

/// The size a figure should be drawn at to show at `shown` (anchor units, times `scale`): never below
/// its floor, where the picture is scaled instead, and never past what a picture can be.
fn draw_size(shown: [f32; 2], scale: f32, floor: [u32; 2]) -> [u32; 2] {
    let side = |n: f32, least: u32| {
        let wanted = if n.is_finite() { (n * scale).round() } else { 0.0 };
        (wanted.clamp(1.0, gridcraft_python::protocol::MAX_PICTURE_SIDE as f32) as u32).max(least)
    };
    [side(shown[0], floor[0]), side(shown[1], floor[1])]
}

fn differs(a: [u32; 2], b: [u32; 2]) -> bool {
    a[0].abs_diff(b[0]) > 2 || a[1].abs_diff(b[1]) > 2
}

/// The next figure that isn't drawn at the size it is shown at, if any. `cells` are the Python cells
/// the worker has run, with the place it knows each by: it has no figure of any other.
pub(super) fn next_render(wb: &Workbook, cells: &[(usize, CellRef, [u32; 3])]) -> Option<DueRender> {
    let place = |sheet: usize, at: CellRef| cells.iter().find(|(s, c, _)| *s == sheet && *c == at).map(|(_, _, place)| *place);
    for (sheet, sh) in wb.sheets.iter().enumerate() {
        for picture in sh.images.iter().filter(|o| o.python_source.is_some()) {
            let Some((from, at)) = picture.python_source.as_deref().and_then(|r| source_of(wb, r)) else { continue };
            let Some(image) = wb.sheet(from).and_then(|s| s.cell(at)).and_then(|c| c.py.as_deref()).and_then(|py| py.image()) else { continue };
            let Some(floor) = image.floor else { continue };
            let size = draw_size([picture.anchor.width, picture.anchor.height], anchor_scale(image), floor);
            if differs(size, picture.rendered.unwrap_or([image.width, image.height]))
                && let Some(place) = place(from, at)
            {
                return Some(DueRender { for_what: RenderFor::Picture(sheet, picture.id), place, size });
            }
        }
    }
    // A cell that draws its picture inside it fills the cell, at or above the floor.
    for (sheet, at, place) in cells {
        let Some(sh) = wb.sheet(*sheet) else { continue };
        let Some(py) = sh.cell(*at).and_then(|c| c.py.as_deref()) else { continue };
        let Some(image) = py.image().filter(|_| !py.shows_object()) else { continue };
        let Some(floor) = image.floor else { continue };
        let area = sh.merge_at(*at).unwrap_or(gridcraft_core::RangeRef::new(*at, *at));
        // A merged area a file claims can be huge: the sides stop adding up at what a picture can be.
        let limit = gridcraft_python::protocol::MAX_PICTURE_SIDE as f32;
        let width = (area.start.col..=area.end.col).try_fold(0.0f32, |sum, c| (sum < limit).then(|| sum + sh.col_width(c))).unwrap_or(limit);
        let height = (area.start.row..=area.end.row).try_fold(0.0f32, |sum, r| (sum < limit).then(|| sum + sh.row_height(r))).unwrap_or(limit);
        let room = [width.min(limit).round() as u32, height.min(limit).round() as u32];
        // Below the floor it stays the natural picture, fitted to the cell, as Excel does.
        if room[0] < floor[0] || room[1] < floor[1] {
            continue;
        }
        let drawn = image.fitted.as_ref().map_or([image.width, image.height], |(_, size)| *size);
        if differs(room, drawn) {
            return Some(DueRender { for_what: RenderFor::Cell(*sheet, *at), place: *place, size: room });
        }
    }
    None
}

/// A figure was drawn again, or couldn't be. Either way it counts as done at this size, so a failure
/// isn't tried over and over: the picture stays as it is until the next run of its cell. No undo step:
/// what shows is derived from the anchor and the result.
pub(super) fn after_render(d: &mut DocState, due: &DueRender, outcome: gridcraft_python::Outcome) -> bool {
    let picture = match outcome {
        gridcraft_python::Outcome::Ok { output: gridcraft_python::Output::Object { picture: Some(p), .. }, .. } => Some(*p),
        _ => None,
    };
    let done = crate::cmd::commit(d, |cx| {
        match due.for_what {
            RenderFor::Picture(sheet, id) => {
                let Some(o) = cx.sheet_mut(sheet)?.images.iter_mut().find(|o| o.id == id) else { return Ok(()) };
                if let Some(p) = picture {
                    o.data = Arc::new(p.bytes);
                    o.mime = p.mime.mime().to_string();
                }
                o.rendered = Some(due.size);
            }
            RenderFor::Cell(sheet, at) => {
                let Some(py) = cx.sheet_mut(sheet)?.cells.get_mut(at).and_then(|c| c.py.as_mut()) else { return Ok(()) };
                let Some(image) = Arc::make_mut(py).output.as_mut().and_then(|o| o.image.as_mut()) else { return Ok(()) };
                let fitted = match picture {
                    Some(p) => {
                        Arc::new(gridcraft_model::CellPicture { data: p.bytes, mime: p.mime.mime().to_string(), alt: image.picture.alt.clone() })
                    }
                    None => image.picture.clone(),
                };
                image.fitted = Some((fitted, due.size));
            }
        }
        Ok(())
    });
    if let Err(e) = done {
        log::warn!("Python cells: {e}");
    }
    d.revision = d.revision.saturating_add(1);
    true
}

/// A new picture over the cells, showing `at`'s image and linked to it.
pub(crate) fn new_picture(wb: &Workbook, sheet: usize, at: CellRef, image: &gridcraft_model::PyImage, id: u32, place: CellRef) -> Option<Image> {
    let name = &wb.sheet(sheet)?.name;
    // As large as a picture placed by hand, on a sheet: at most 640 across.
    let scale = (640.0 / image.width.max(1) as f32).min(1.0);
    Some(Image {
        id,
        anchor: gridcraft_model::Anchor {
            cell: place,
            dx: 0.0,
            dy: 0.0,
            width: image.width as f32 * scale,
            height: image.height as f32 * scale,
            mode: gridcraft_model::AnchorMode::MoveOnly,
        },
        data: Arc::new(image.picture.data.clone()),
        mime: image.picture.mime.clone(),
        alt: format!("Python result of {}", at.a1()),
        python_source: Some(reference(name, at)),
        rendered: None,
    })
}

/// What is wrong with a linked picture's source, when its last run left it with no picture: the cell
/// and its error. The picture keeps its last image meanwhile.
pub fn problem(wb: &Workbook, picture: &Image) -> Option<(CellRef, gridcraft_core::CellError)> {
    let (sheet, at) = source_of(wb, picture.python_source.as_deref()?)?;
    let error = wb.sheet(sheet)?.cell(at)?.py.as_deref()?.failed?;
    Some((at, error))
}
