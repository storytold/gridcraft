//! Python cells on the sheet: the PY tag at the start of every Python cell, and the card that a
//! click on the tag (or Ctrl+Shift+F5) opens, with the cell's cached result (its Python type and a
//! preview) or the reason it shows `#BLOCKED!`.

use egui::{Align2, FontId, Painter, Rect, pos2, vec2};
use gridcraft_engine::core::{CellError, CellRef, Value};
use gridcraft_engine::model::{PyCode, Sheet, VAlign, Workbook};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::grid::Geo;
use crate::theme::{self, Tokens};

/// The most characters of a type name the card shows.
pub const MAX_TITLE_CHARS: usize = 80;
/// The most characters and lines of a preview the card shows: a cached `repr` can be as long as a
/// file makes it, and the card is laid out every frame.
pub const MAX_PREVIEW_CHARS: usize = 600;
pub const MAX_PREVIEW_LINES: usize = 12;

/// What the card says of a cell this build can't calculate.
pub const NOT_RUN: &str = "This version of GridCraft stores Python code but does not run it.";

/// The labels of a Python cell's return type, as Excel names them.
pub const PYTHON_OBJECT: &str = "Python Object";
pub const EXCEL_VALUE: &str = "Excel Value";

/// The open card: the workbook, sheet and Python cell it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub uid: u64,
    pub sheet: usize,
    pub cell: CellRef,
}

/// Where a Python cell's PY tag sits in its cell `rect` (logical coordinates), from `left`, level
/// with text aligned `v`. `None` when the cell is too small to hold it.
pub fn tag_rect(rect: Rect, left: f32, z: f32, v: VAlign) -> Option<Rect> {
    let h = (12.0 * z).min(rect.height() - 4.0);
    let w = 18.0 * z;
    let top = match v {
        VAlign::Top => rect.top() + 2.0 * z,
        VAlign::Bottom => rect.bottom() - h - 2.5 * z,
        _ => rect.center().y - h / 2.0,
    };
    (h >= 6.0 && rect.right() - left >= w + 12.0 * z).then(|| Rect::from_min_size(pos2(left + 2.0 * z, top), vec2(w, h)))
}

/// Paints a PY tag. The sheet stays white in both themes, so the tag's colours follow it.
pub fn paint_tag(p: &Painter, tag: Rect, t: &Tokens) {
    p.rect_filled(tag, tag.height() * 0.2, t.sheet_py_tag);
    let font = FontId::new((tag.height() * 0.72).max(4.0), theme::ui_bold(8.0).family);
    p.text(tag.center(), Align2::CENTER_CENTER, "PY", font, t.on_sheet_py_tag);
}

/// The Python cell's area on the sheet: the cell, or the merged area it starts.
fn area(geo: &Geo, sh: &Sheet, c: CellRef) -> Rect {
    match sh.merge_at(c) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, c),
    }
}

/// The PY tag of the Python cell at `c` (logical coordinates), when `c` holds Python code.
pub fn tag_at(geo: &Geo, wb: &Workbook, sh: &Sheet, c: CellRef) -> Option<Rect> {
    let c = sh.merge_at(c).map(|m| m.start).unwrap_or(c);
    let cell = sh.cell(c).filter(|x| x.py.is_some())?;
    let rect = area(geo, sh, c);
    tag_rect(rect, rect.left(), geo.z, wb.styles.get(cell.style).align.v)
}

/// The Python cell the active cell shows: itself, or the one whose result spills over it.
pub fn active_python_cell(app: &SheetApp) -> Option<(u64, usize, CellRef, std::sync::Arc<PyCode>)> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let a = sh.merge_at(d.selection.active).map(|m| m.start).unwrap_or(d.selection.active);
    let at = sh.spill_anchor(a).unwrap_or(a);
    sh.cell(at).and_then(|c| c.py.clone()).map(|py| (d.uid, d.wb.active_sheet(), at, py))
}

/// At most `chars` characters and `lines` lines of `s`, with an ellipsis when it was cut. The work
/// is bounded by the limits, not by `s`.
fn capped(s: &str, chars: usize, lines: usize) -> String {
    let mut out = String::new();
    let mut taken_lines = 1;
    for (i, ch) in s.chars().enumerate() {
        if i >= chars || (ch == '\n' && taken_lines >= lines) {
            out.push('…');
            return out;
        }
        if ch == '\n' {
            taken_lines += 1;
        }
        out.push(ch);
    }
    out
}

/// What the card shows for a Python cell.
#[derive(Clone, Debug, PartialEq)]
pub struct CardText {
    /// The result's Python type, or what the cell shows when there is no result.
    pub title: String,
    /// The return type, as Excel labels it (untranslated).
    pub output: &'static str,
    /// A preview of the result, or why there is none.
    pub body: String,
}

/// The card's text for the Python cell at `c`, capped ([`MAX_TITLE_CHARS`], [`MAX_PREVIEW_CHARS`]
/// and [`MAX_PREVIEW_LINES`]). `None` when `c` holds no Python code.
pub fn card_text(wb: &Workbook, sh: &Sheet, c: CellRef) -> Option<CardText> {
    let py = sh.cell(c)?.py.as_deref()?;
    let value = sh.value(c);
    let output = if py.object { PYTHON_OBJECT } else { EXCEL_VALUE };
    let shown = || capped(&gridcraft_engine::display::cell_text(wb, sh, c), MAX_PREVIEW_CHARS, MAX_PREVIEW_LINES);
    let (title, body) = match (&py.output, &value) {
        (Some(o), _) if !o.type_name.is_empty() => {
            let body = if o.preview.is_empty() { shown() } else { capped(&o.preview, MAX_PREVIEW_CHARS, MAX_PREVIEW_LINES) };
            (capped(&o.type_name, MAX_TITLE_CHARS, 1), body)
        }
        (_, Value::Error(CellError::Blocked)) => (CellError::Blocked.as_str().to_string(), NOT_RUN.to_string()),
        _ => (capped(&gridcraft_engine::display::cell_text(wb, sh, c), MAX_TITLE_CHARS, 1), String::new()),
    };
    Some(CardText { title, output, body })
}

/// Opens the card of the active cell's Python cell (`ui.pythonCard`, Ctrl+Shift+F5).
pub fn open(app: &mut SheetApp) -> Result<Json, String> {
    let Some((uid, sheet, cell, _)) = active_python_cell(app) else {
        return Err("The active cell isn't a Python cell.".into());
    };
    if app.editor.is_some() {
        return Err("The card opens when no cell is being edited.".into());
    }
    app.grid.py_card = Some(Card { uid, sheet, cell });
    app.grid.ensure_visible = true;
    Ok(json!({"open": true, "cell": cell.a1()}))
}

/// `ui.pythonCard {on?}`: opens the active Python cell's card (the default), or closes it.
pub fn command(app: &mut SheetApp, p: &Json) -> Result<Json, String> {
    if p.get("on").and_then(Json::as_bool) == Some(false) {
        app.grid.py_card = None;
        return Ok(json!({"open": false}));
    }
    open(app)
}

/// Draws the open card under its cell (beside it on the screen's side the cell starts on, so it is
/// mirrored on a right-to-left sheet), and closes it when its cell is no longer a Python cell in
/// view, an edit starts, or the user clicks elsewhere or presses Esc.
pub fn show(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet, was_open: bool) {
    let Some(card) = app.grid.py_card else { return };
    let here = app.session.active().map(|d| (d.uid, d.wb.active_sheet()));
    let text = app.session.active().filter(|_| here == Some((card.uid, card.sheet))).and_then(|d| card_text(&d.wb, sh, card.cell));
    let screen = geo.mirror_rect(area(geo, sh, card.cell));
    let view = geo.mirror_rect(geo.cells);
    let Some(text) = text.filter(|_| app.editor.is_none() && view.intersects(screen)) else {
        app.grid.py_card = None;
        return;
    };
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let (at, pivot) = if geo.rtl { (screen.right_bottom(), Align2::RIGHT_TOP) } else { (screen.left_bottom(), Align2::LEFT_TOP) };
    let mut close = false;
    let area = egui::Area::new(egui::Id::unique("python_card"))
        .order(egui::Order::Foreground)
        .pivot(pivot)
        .fixed_pos(at + vec2(0.0, 2.0))
        .constrain_to(ui.ctx().content_rect());
    let resp = area.show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(8.0).show(ui, |ui| {
            ui.set_width(360.0);
            ui.horizontal(|ui| {
                let (tag, _) = ui.allocate_exact_size(vec2(22.0, 15.0), egui::Sense::hover());
                paint_tag(ui.painter(), tag, &t);
                ui.label(egui::RichText::new(&text.title).font(theme::ui_bold(13.0)).color(t.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::widgets::icon_button(ui, crate::icons::Icon::Close, t.text_dim, lang.tr("Close"), vec2(18.0, 18.0)).clicked() {
                        close = true;
                    }
                });
            });
            ui.label(egui::RichText::new(lang.tr(text.output)).font(theme::ui_font(11.5)).color(t.text_dim));
            if !text.body.is_empty() {
                ui.add_space(4.0);
                if text.body == NOT_RUN {
                    ui.add(egui::Label::new(egui::RichText::new(lang.tr(NOT_RUN)).font(theme::ui_font(12.5)).color(t.text)).wrap());
                } else {
                    // A repr keeps its lines (a table's columns line up): it scrolls rather than wraps.
                    egui::ScrollArea::both().max_height(220.0).min_scrolled_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&text.body).font(crate::code_editor::font()).color(t.text)).extend());
                    });
                }
            }
        });
    });
    if close || popup_dismissed(&resp.response, was_open) {
        app.grid.py_card = None;
    }
}

/// Esc, the window losing focus, or a click outside a card that was already open.
fn popup_dismissed(response: &egui::Response, was_open: bool) -> bool {
    response.ctx.input(|i| !i.focused || i.key_pressed(egui::Key::Escape)) || (was_open && response.clicked_elsewhere())
}

/// The cell menu's return-type items on a Python cell: "Python Object" and "Excel Value", the
/// current one marked. Returns whether one was picked (the menu then closes).
pub fn menu_items(app: &mut SheetApp, ui: &mut egui::Ui) -> bool {
    let Some((_, sheet, cell, py)) = active_python_cell(app) else { return false };
    let lang = app.ui.language;
    let mut picked = None;
    for (label, object) in [(PYTHON_OBJECT, true), (EXCEL_VALUE, false)] {
        if ui.radio(py.object == object, lang.tr(label)).clicked() {
            picked = Some(object);
        }
    }
    ui.separator();
    let Some(object) = picked else { return false };
    if object != py.object {
        app.run_or_alert("python.output", json!({"sheet": sheet, "cell": cell.a1(), "mode": if object { "object" } else { "value" }}));
    }
    true
}

/// Ctrl+Alt+Shift+M: switches the active Python cell's return type. Nothing on other cells.
pub fn switch_output(app: &mut SheetApp) {
    if let Some((_, sheet, cell, _)) = active_python_cell(app) {
        app.run_or_alert("python.output", json!({"sheet": sheet, "cell": cell.a1()}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_text_stops_at_its_limits() {
        assert_eq!(capped("abc", 5, 2), "abc");
        assert_eq!(capped("abcdef", 3, 2), "abc…");
        assert_eq!(capped("a\nb\nc", 50, 2), "a\nb…");
        assert_eq!(capped("é字x", 2, 1), "é字…");
        assert_eq!(capped("", 0, 0), "");
    }

    #[test]
    fn the_tag_needs_room_and_follows_the_vertical_alignment() {
        let cell = Rect::from_min_size(pos2(100.0, 50.0), vec2(64.0, 20.0));
        let tag = tag_rect(cell, cell.left(), 1.0, VAlign::Bottom).unwrap();
        assert!(cell.contains_rect(tag));
        assert!((tag.left() - 102.0).abs() < 0.01);
        assert!(tag_rect(cell, cell.left(), 1.0, VAlign::Top).unwrap().top() < tag.top());
        assert!(tag_rect(Rect::from_min_size(cell.min, vec2(20.0, 20.0)), 100.0, 1.0, VAlign::Bottom).is_none(), "too narrow");
        assert!(tag_rect(Rect::from_min_size(cell.min, vec2(64.0, 8.0)), 100.0, 1.0, VAlign::Bottom).is_none(), "too short");
    }
}
