//! Pictures from Python cells in the interface: the Python Output menu in the formula bar, the
//! picture on a cell's card, and what a linked floating picture shows when it is selected or its cell
//! has a problem. The commands behind them live in the engine: `python.output`, `python.image`,
//! `python.saveImage`, `python.displayOverCells` and `python.createReference`.

use egui::{Rect, Sense, Vec2, pos2, vec2};
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{ObjKind, ObjRef, PyImage, Sheet};
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
