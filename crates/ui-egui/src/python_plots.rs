//! Pictures from Python cells in the interface: the Python Output menu in the formula bar, the
//! picture on a cell's card, and what a linked floating picture shows when it is selected or its cell
//! has a problem. The commands behind them live in the engine: `python.output`, `python.image`,
//! `python.saveImage`, `python.displayOverCells` and `python.createReference`.

use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Sense, Vec2, pos2, vec2};
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::{ObjKind, ObjRef, PyCode, PyImage, Sheet, VAlign};
use serde_json::json;

use crate::SheetApp;
use crate::grid::Geo;
use crate::icons::Icon;
use crate::theme::{self, Tokens};

pub const OUTPUT_MENU: &str = "Python Output";
pub const DISPLAY_OVER_CELLS: &str = "Display Plot over Cells";
pub const CREATE_REFERENCE: &str = "Create Reference";
pub const COPY_IMAGE: &str = "Copy Image";
pub const SAVE_IMAGE: &str = "Save Image\u{2026}";
pub const IMAGE_SAVED: &str = "The image is saved.";
pub const IMAGE_COPIED: &str = "The image is on the clipboard.";

/// The card's width before anyone drags its corner, and the limits of the drag.
const CARD_WIDTH: f32 = 320.0;
const CARD_MIN_WIDTH: f32 = 200.0;

/// The side of the double square for each unit of its cell's text size. Its drawing takes 13/16 of
/// the side, so it is a little taller than the text and fills a row of the default height.
const MARK_PER_TEXT_SIZE: f32 = 1.25;

/// How far the word in a cell may be scaled to match the cell's typeface, so that a very narrow or a
/// very wide cell font does not make it too small or too large.
const WORD_SCALE: (f32, f32) = (0.9, 1.1);

/// The gap between a picture's card and its cell.
const CARD_GAP: f32 = 2.0;
/// What a card's frame adds to its width: its margin on both sides.
const CARD_FRAME: f32 = 16.0;

/// Whether a Python cell says `Image`: its result is a picture, kept as a Python object. An error or
/// `#BUSY!` in the cell is not that word.
pub fn says_image(py: &PyCode, value: &Value) -> bool {
    py.image().is_some() && py.shows_object() && matches!(value, Value::Text(_))
}

/// The double square of a cell that says `Image`, in place of its ready PY tag, in the cell `rect`
/// (logical coordinates) from `left`: it opens the card as the tag does. It follows the size of the
/// cell's text, `points`, and is never taller than the cell. `None` when the cell is too small to
/// hold it.
pub fn mark_rect(rect: Rect, left: f32, points: f32, z: f32, v: VAlign) -> Option<Rect> {
    // Clear of the cell's edges, where a selected cell has its border. At the default size this
    // centres it in a row of the default height.
    let inset = 0.8 * z;
    let side = (points * 96.0 / 72.0 * z * MARK_PER_TEXT_SIZE).min(rect.height() - 2.0 * inset);
    let top = match v {
        VAlign::Top => rect.top() + inset,
        VAlign::Bottom => rect.bottom() - inset - side,
        _ => rect.center().y - side / 2.0,
    };
    (side >= 6.0 && rect.right() - left >= side + 12.0 * z).then(|| Rect::from_min_size(pos2(left + z, top), vec2(side, side)))
}

/// The font of the word `Image` in a cell: the ribbon's typeface and the cell's bold, at the size that
/// gives `word` the width it has as any other text in that cell. `size` and `cell` are that text's size
/// and typeface. The same number is not the same size across typefaces: on macOS the ribbon's letters
/// are about 7% narrower than the cell's, and on a display of scale 1 they can round to a pixel shorter.
pub fn word_font(p: &Painter, word: &str, size: f32, bold: bool, cell: FontFamily) -> FontId {
    let ribbon = |size| if bold { theme::ui_bold(size) } else { theme::ui_font(size) };
    let width = |font| p.layout_no_wrap(word.to_string(), font, Color32::PLACEHOLDER).size().x;
    ribbon(size * word_scale(width(FontId::new(size, cell)), width(ribbon(size))))
}

/// The scale of the ribbon's typeface for a word as wide as it is in the cell's.
fn word_scale(in_cell: f32, in_ribbon: f32) -> f32 {
    if in_ribbon > 0.0 { (in_cell / in_ribbon).clamp(WORD_SCALE.0, WORD_SCALE.1) } else { 1.0 }
}

/// Where a picture's card goes: beside its cell, level with the cell's top, so the cell stays in view
/// whatever the picture's height. It takes the side the sheet reads towards, or the other side when
/// it fits only there. `width` is the card's content width.
pub(crate) fn card_place(cell: Rect, width: f32, window: Rect, rtl: bool) -> (Pos2, Align2) {
    let wanted = width + CARD_FRAME + CARD_GAP;
    let fits_after = cell.right() + wanted <= window.right();
    let fits_before = cell.left() - wanted >= window.left();
    let after = if rtl { fits_after && !fits_before } else { fits_after || !fits_before };
    if after { (cell.right_top() + vec2(CARD_GAP, 0.0), Align2::LEFT_TOP) } else { (cell.left_top() - vec2(CARD_GAP, 0.0), Align2::RIGHT_TOP) }
}

/// The Python Output menu, between the Enter button and the PY badge: the return type of the active
/// Python cell. Greyed on a draft, which has no return type yet. Returns its rect.
pub fn output_menu(app: &mut SheetApp, ui: &mut egui::Ui) -> Rect {
    let cell = crate::python_card::active_python_cell(app);
    let lang = app.ui.language;
    let label = lang.tr(OUTPUT_MENU).to_string();
    let t = Tokens::get(ui.ctx());
    let sense = if cell.is_some() { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 24.0), sense);
    let color = if cell.is_some() { t.text } else { t.text_dim };
    if resp.hovered() && cell.is_some() {
        ui.painter().rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let icon = Rect::from_min_size(rect.min + vec2(1.0, 4.0), vec2(16.0, 16.0));
    crate::icons::paint(ui.painter(), icon, Icon::PythonOutput, color);
    let arrow = Rect::from_min_size(rect.min + vec2(18.0, 7.0), vec2(11.0, 11.0));
    crate::icons::paint(ui.painter(), arrow, Icon::Chevron, color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, cell.is_some(), &label));
    let resp = resp.on_hover_text(format!("{label} (Ctrl+Alt+Shift+M)"));
    let Some((_, sheet, at, py)) = cell else { return rect };
    egui::Popup::menu(&resp).show(|ui| {
        ui.label(egui::RichText::new(&label).font(theme::ui_bold(12.5)).color(t.text));
        ui.add_space(2.0);
        let shown = py.shows_object();
        for (name, object) in [(crate::python_card::PYTHON_OBJECT, true), (crate::python_card::EXCEL_VALUE, false)] {
            if ui.radio(shown == object, lang.tr(name)).clicked() {
                if shown != object || !py.output_chosen {
                    app.run_or_alert("python.output", json!({"sheet": sheet, "cell": at.a1(), "mode": if object { "object" } else { "value" }}));
                }
                ui.close();
            }
        }
    });
    rect
}

/// The picture on a card, with what Excel's card shows under it: the word, and the size.
pub(crate) fn card_picture(ui: &mut egui::Ui, image: &PyImage, screen: Vec2, dragged: bool, lang: crate::i18n::Language, t: &Tokens) {
    let width = ui.available_width().max(1.0);
    let proportion = image.height.max(1) as f32 / image.width.max(1) as f32;
    // Room for the rest of the card, and for the cell above it until someone asks for more.
    let height = (width * proportion).min((screen.y * if dragged { 0.75 } else { 0.4 }).max(60.0));
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    crate::cell_pictures::paint(ui.painter(), rect, &image.picture);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &image.picture.alt));
    ui.add_space(6.0);
    ui.label(egui::RichText::new(lang.tr("Image")).font(theme::ui_font(18.0)).color(t.text));
    ui.add_space(2.0);
    ui.label(egui::RichText::new(lang.tr("size")).font(theme::ui_font(11.5)).color(t.text_dim));
    ui.label(egui::RichText::new(format!("{}, {}", image.width, image.height)).font(theme::ui_font(12.5)).color(t.text));
}

/// How wide the card of a picture is.
pub(crate) fn card_width(app: &SheetApp, screen: Vec2) -> f32 {
    app.python.card_image_width.unwrap_or(CARD_WIDTH).clamp(CARD_MIN_WIDTH, (screen.x - 32.0).max(CARD_MIN_WIDTH))
}

/// The corner of a picture's card that resizes it. `frame` is the card's outer rect.
pub(crate) fn card_grip(app: &mut SheetApp, ui: &egui::Ui, frame: Rect, width: f32) {
    let t = Tokens::get(ui.ctx());
    let grip = Rect::from_min_max(frame.right_bottom() - vec2(16.0, 16.0), frame.right_bottom());
    let resp = ui.interact(grip, ui.scope_id().with("python_card_grip"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeNwSe);
    let label = app.ui.language.tr("Resize").to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    if resp.dragged() {
        app.python.card_image_width = Some((width + resp.drag_delta().x).max(CARD_MIN_WIDTH));
    }
    let stroke = egui::Stroke::new(1.0, if resp.hovered() || resp.dragged() { t.text } else { t.text_dim });
    let corner = frame.right_bottom() - vec2(3.0, 3.0);
    for n in [4.0, 7.0, 10.0] {
        ui.painter().line_segment([corner - vec2(n, 0.0), corner - vec2(0.0, n)], stroke);
    }
}

/// Copy Image: the picture's pixels on the system clipboard, for other applications.
pub(crate) fn copy_image(app: &mut SheetApp, sheet: usize, at: CellRef) {
    let picture = app
        .session
        .active()
        .and_then(|d| d.wb.sheet(sheet))
        .and_then(|sh| sh.cell(at))
        .and_then(|c| c.py.as_deref())
        .and_then(|py| py.image())
        .map(|i| i.picture.clone());
    let pixels = picture.as_ref().and_then(|p| crate::cell_pictures::rgba(p));
    let done = match (&pixels, app.services.copy_image.as_mut()) {
        (Some(pixels), Some(copy)) => copy(pixels),
        (None, _) => Err("The image can't be read.".to_string()),
        (_, None) => Err("This window can't put a picture on the clipboard.".to_string()),
    };
    match done {
        Ok(()) => {
            // These pixels are for other applications. They end any copy of a floating picture, and
            // a paste of them is a paste of a plain picture, even when the two look the same. No
            // floating object goes between workbooks, so another workbook still refuses them.
            app.forget_picture_copy();
            if let Some(pixels) = &pixels {
                app.note_copied_image(pixels);
            }
            app.toast = Some((IMAGE_COPIED.into(), crate::now_ms()));
        }
        Err(e) => app.message = Some(("GridCraft".into(), crate::clean_error(&e))),
    }
}

/// Save Image: asks where, as `python.saveImage` itself never does. The native file picker on the
/// desktop, a download on the web. Cancelling the picker does nothing.
pub(crate) fn save_image(app: &mut SheetApp, sheet: usize, at: CellRef) {
    let title = app.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
    let stem = std::path::Path::new(&title).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book1").to_string();
    let picture = app
        .session
        .active()
        .and_then(|d| d.wb.sheet(sheet))
        .and_then(|sh| sh.cell(at))
        .and_then(|c| c.py.as_deref())
        .and_then(|py| py.image())
        .map(|i| i.picture.clone());
    let Some(picture) = picture else { return };
    let ending = if picture.mime == "image/jpeg" { "jpg" } else { "png" };
    let name = format!("{stem}-{}.{ending}", at.a1());
    let done = if let Some(pick) = &app.services.pick_save {
        let Some(path) = pick(&name) else { return };
        app.run("python.saveImage", json!({"sheet": sheet, "cell": at.a1(), "path": path})).map(|_| ())
    } else if let Some(download) = &app.services.download {
        download(&name, &picture.data);
        Ok(())
    } else {
        Err("Exporting needs a file picker, which this window doesn't have.".to_string())
    };
    match done {
        Ok(()) => app.toast = Some((IMAGE_SAVED.into(), crate::now_ms())),
        Err(e) => app.message = Some(("GridCraft".into(), crate::clean_error(&e))),
    }
}

/// Display Plot over Cells for the active Python cell, when it gave a picture. Ctrl+Alt+Shift+C does
/// nothing on another cell.
pub fn display_over_cells(app: &mut SheetApp) {
    if let Some((_, sheet, at, py)) = crate::python_card::active_python_cell(app)
        && py.image().is_some()
    {
        app.run_or_alert("python.displayOverCells", json!({"sheet": sheet, "cell": at.a1()}));
    }
}

/// What goes on a linked picture: a mark when its cell has a problem, and Create Reference beside the
/// selected one.
pub fn picture_chrome(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet) {
    let Some(d) = app.session.active() else { return };
    if sh.images.iter().all(|o| o.python_source.is_none()) || crate::chartview::objects_hidden(&d.wb) {
        return;
    }
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());
    for picture in sh.images.iter().filter(|o| o.python_source.is_some()) {
        let Some((cell, error)) = gridcraft_engine::python::links::problem(&d.wb, picture) else { continue };
        let Some(r) = crate::chartview::object_rect(geo, sh, ObjRef::new(ObjKind::Image, picture.id)) else { continue };
        let r = geo.mirror_rect(r);
        let mark = Rect::from_min_size(r.min + vec2(4.0, 4.0), vec2(16.0, 16.0));
        if !mark.intersects(geo.mirror_rect(geo.cells)) {
            continue;
        }
        ui.painter().circle_filled(mark.center(), 8.0, t.badge_py);
        let text = ui.painter().layout_no_wrap("!".into(), theme::ui_bold(11.0), t.on_badge);
        ui.painter().galley(mark.center() - text.size() / 2.0, text, t.on_badge);
        let said = format!("{}: {} {}", cell.a1(), error.as_str(), lang.tr("This picture shows the last result that cell gave."));
        let resp = ui.interact(mark, ui.scope_id().with(("python_picture_mark", picture.id)), Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &said));
        resp.on_hover_text(said.clone());
    }
    let selected = app.selected_object.filter(|o| o.kind == ObjKind::Image);
    let Some(picture) = selected.and_then(|o| sh.images.iter().find(|i| i.id == o.id)).filter(|i| i.python_source.is_some()) else { return };
    let Some(r) = crate::chartview::object_rect(geo, sh, ObjRef::new(ObjKind::Image, picture.id)) else { return };
    let r = geo.mirror_rect(r);
    let view = geo.mirror_rect(geo.cells);
    let size = vec2(26.0, 26.0);
    let beside = if geo.rtl { pos2(r.left() - size.x - 8.0, r.top()) } else { pos2(r.right() + 8.0, r.top()) };
    let place = Rect::from_min_size(beside, size);
    // Beside the picture while there is room, else inside its corner.
    let place = if view.contains_rect(place) { place } else { Rect::from_min_size(pos2(r.right() - size.x - 4.0, r.top() + 4.0), size) };
    if !view.contains_rect(place) {
        return;
    }
    let id = picture.id;
    ui.painter().rect_filled(place, 5.0, ui.visuals().window_fill());
    ui.painter().rect_stroke(place, 5.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Inside);
    let clicked = ui
        .scope_builder(egui::UiBuilder::new().max_rect(place), |ui| {
            crate::widgets::icon_button(ui, Icon::Link, t.text, lang.tr(CREATE_REFERENCE), size).clicked()
        })
        .inner;
    if clicked {
        app.run_or_alert("python.createReference", json!({"id": id}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(1000.0, 700.0) };

    fn cell(left: f32) -> Rect {
        Rect::from_min_size(pos2(left, 300.0), vec2(64.0, 20.0))
    }

    #[test]
    fn a_pictures_card_goes_beside_its_cell_on_the_side_that_has_room() {
        let (at, pivot) = card_place(cell(100.0), 320.0, WINDOW, false);
        assert_eq!((at, pivot), (pos2(166.0, 300.0), Align2::LEFT_TOP), "after the cell, level with its top");
        let (at, pivot) = card_place(cell(800.0), 320.0, WINDOW, false);
        assert_eq!((at, pivot), (pos2(798.0, 300.0), Align2::RIGHT_TOP), "no room after it, so before it");
        let (at, pivot) = card_place(cell(800.0), 320.0, WINDOW, true);
        assert_eq!((at, pivot), (pos2(798.0, 300.0), Align2::RIGHT_TOP), "a right-to-left sheet reads towards the left");
        let (at, pivot) = card_place(cell(100.0), 320.0, WINDOW, true);
        assert_eq!((at, pivot), (pos2(166.0, 300.0), Align2::LEFT_TOP), "no room before it, so after it");
    }

    #[test]
    fn a_card_that_fits_on_neither_side_keeps_the_reading_side() {
        let narrow = Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 700.0));
        assert_eq!(card_place(cell(100.0), 320.0, narrow, false).1, Align2::LEFT_TOP);
        assert_eq!(card_place(cell(100.0), 320.0, narrow, true).1, Align2::RIGHT_TOP);
    }

    #[test]
    fn the_word_takes_the_width_it_has_in_the_cells_typeface_within_limits() {
        assert_eq!(word_scale(30.0, 30.0), 1.0, "the same width needs no scale");
        assert_eq!(word_scale(33.0, 30.0), 1.1, "a narrower ribbon grows");
        assert_eq!(word_scale(28.5, 30.0), 0.95, "a wider ribbon shrinks");
        assert_eq!(word_scale(90.0, 30.0), WORD_SCALE.1, "a very wide cell font");
        assert_eq!(word_scale(3.0, 30.0), WORD_SCALE.0, "a very narrow cell font");
        assert_eq!(word_scale(30.0, 0.0), 1.0, "nothing to measure");
        assert_eq!(word_scale(0.0, 0.0), 1.0);
    }

    #[test]
    fn the_mark_fills_a_row_of_the_default_height_and_follows_the_text() {
        let row = Rect::from_min_size(pos2(10.0, 20.0), vec2(64.0, 20.0));
        let mark = mark_rect(row, row.left(), 11.0, 1.0, VAlign::Bottom).unwrap();
        assert_eq!(mark.width(), mark.height(), "square");
        assert!(mark.height() > 0.9 * row.height() && row.contains_rect(mark), "{mark:?} fills {row:?}");
        assert_eq!(mark.left(), row.left() + 1.0);

        // It grows with the zoom and the cell's font size, and stays inside the cell.
        let zoomed_row = Rect::from_min_size(row.min, row.size() * 2.0);
        let zoomed = mark_rect(zoomed_row, zoomed_row.left(), 11.0, 2.0, VAlign::Bottom).unwrap();
        assert_eq!(zoomed.height(), 2.0 * mark.height());
        assert!(zoomed_row.contains_rect(zoomed), "{zoomed:?} in {zoomed_row:?}");
        let too_short = mark_rect(row, row.left(), 22.0, 1.0, VAlign::Bottom).unwrap();
        assert!(row.contains_rect(too_short), "a row shorter than the text holds it: {too_short:?} in {row:?}");
        let tall = Rect::from_min_size(row.min, vec2(200.0, 60.0));
        let large = mark_rect(tall, tall.left(), 22.0, 1.0, VAlign::Bottom).unwrap();
        assert_eq!(large.height(), 2.0 * mark.height());
        assert!(tall.contains_rect(large));
    }

    #[test]
    fn the_mark_stands_where_the_cells_text_is_aligned() {
        let tall = Rect::from_min_size(pos2(10.0, 20.0), vec2(64.0, 60.0));
        let at = |v| mark_rect(tall, tall.left(), 11.0, 1.0, v).unwrap();
        assert_eq!(at(VAlign::Top).top(), tall.top() + 0.8);
        assert_eq!(at(VAlign::Center).center().y, tall.center().y);
        assert_eq!(at(VAlign::Bottom).bottom(), tall.bottom() - 0.8);
    }

    #[test]
    fn a_cell_too_small_for_the_mark_has_none() {
        let narrow = Rect::from_min_size(pos2(10.0, 20.0), vec2(20.0, 20.0));
        assert!(mark_rect(narrow, narrow.left(), 11.0, 1.0, VAlign::Bottom).is_none());
        let short = Rect::from_min_size(pos2(10.0, 20.0), vec2(64.0, 5.0));
        assert!(mark_rect(short, short.left(), 11.0, 1.0, VAlign::Bottom).is_none());
    }
}
