//! Python cells on the sheet: the PY tag at the start of every Python cell, which shows the cell's
//! state in its shape as well as its colour, and the card that a click on the tag (or Ctrl+Shift+F5)
//! opens: the cell's result (what names it and a preview, or a table with what can be done with it),
//! what the catalog says about it with one action, and Convert to Normal Cell (`python.clear`) on the
//! card and the cell menu.

use std::sync::Arc;

use egui::{Align2, FontId, Painter, Rect, Stroke, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellError, CellRef, Value};
use gridcraft_engine::model::{PyCode, PyTable, Sheet, VAlign, Workbook};
use gridcraft_engine::python::{Diagnostic, Severity};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::grid::Geo;
use crate::theme::{self, Tokens};

mod table;

/// The most characters of a type name the card shows.
pub const MAX_TITLE_CHARS: usize = 80;
/// The most characters and lines of a preview the card shows: a cached `repr` can be as long as a
/// file makes it, and the card is laid out every frame.
pub const MAX_PREVIEW_CHARS: usize = 600;
pub const MAX_PREVIEW_LINES: usize = 12;

/// The labels of a Python cell's return type, as Excel names them.
pub const PYTHON_OBJECT: &str = "Python Object";
pub const EXCEL_VALUE: &str = "Excel Value";

/// The label of `python.clear`, on the card and the cell menu.
pub const CONVERT: &str = "Convert to Normal Cell";

/// The card's actions on a result.
pub const COPY_CSV: &str = "Copy as CSV";
pub const COPY_MARKDOWN: &str = "Copy as Markdown";
pub const SAVE_CSV: &str = "Save as .csv…";
pub const SPILL_HERE: &str = "Spill Here";
pub const SPILL_LINES: &str = "Spill One Line per Row";
/// What the card says about a table a file couldn't keep.
pub const RUN_TO_SEE: &str = "The table isn't kept in the file. Run the cell to see it.";
/// What the card says about a table with nothing in it.
pub const NO_COLUMNS: &str = "This table is empty.";
pub const NO_ROWS: &str = "This table has no rows.";

/// The widest the card is with a table in it, and without.
const TABLE_CARD_WIDTH: f32 = 560.0;
const CARD_WIDTH: f32 = 360.0;

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

/// What a PY tag says about its cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagState {
    /// Its result is up to date.
    Ready,
    /// It is running or about to.
    Running,
    /// It keeps a value it didn't just work out: stale, stopped, or Python isn't on.
    NotRunning,
    Error,
    /// Python can't run here at all.
    Blocked,
}

/// The state of a Python cell. A few lookups, so it can be asked for every cell in view.
pub fn tag_state(session: &Session, sheet: usize, cell: CellRef, py: &PyCode, value: &Value) -> TagState {
    if py.busy {
        return TagState::Running;
    }
    match session.python_diagnostic(sheet, cell).map(|d| d.kind) {
        Some(k) if k.id == "state.busy" => TagState::Running,
        Some(k) if k.id.starts_with("off.") || matches!(k.id, "state.stale" | "state.stopped") => TagState::NotRunning,
        Some(k) if k.severity == Severity::Blocked => TagState::Blocked,
        Some(k) if k.severity == Severity::Error => TagState::Error,
        _ if py.stale || matches!(value, Value::Error(CellError::Blocked)) => TagState::NotRunning,
        _ if matches!(value, Value::Error(CellError::Python | CellError::Timeout | CellError::Connect)) => TagState::Error,
        _ => TagState::Ready,
    }
}

/// Paints a PY tag. The sheet stays white in both themes, so the tag's colours follow it. Each state
/// has a shape of its own: plain and filled when ready, hollow when not running, and with a mark
/// beside a smaller PY when it runs (an open ring), failed (an exclamation mark) or is blocked (a
/// lock). On a right-to-left sheet the shapes are mirrored with the rest of the cell.
pub fn paint_tag(p: &Painter, tag: Rect, t: &Tokens, state: TagState) {
    let h = tag.height();
    let corner = h * 0.2;
    let (fill, ink) = match state {
        TagState::Ready | TagState::Running => (t.sheet_py_tag, t.on_sheet_py_tag),
        TagState::NotRunning => (t.on_sheet_py_tag, t.sheet_py_tag),
        TagState::Error => (t.sheet_py_tag_error, t.on_sheet_py_tag),
        TagState::Blocked => (t.sheet_py_tag_blocked, t.on_sheet_py_tag),
    };
    p.rect_filled(tag, corner, fill);
    if state == TagState::NotRunning {
        p.rect_stroke(tag, corner, Stroke::new((h * 0.1).max(0.9), t.sheet_py_tag), egui::StrokeKind::Inside);
    }
    let family = theme::ui_bold(8.0).family;
    if matches!(state, TagState::Ready | TagState::NotRunning) {
        p.text(tag.center(), Align2::CENTER_CENTER, "PY", FontId::new((h * 0.72).max(4.0), family), ink);
        return;
    }
    let text_at = pos2(tag.left() + tag.width() * 0.33, tag.center().y);
    p.text(text_at, Align2::CENTER_CENTER, "PY", FontId::new((h * 0.6).max(4.0), family), ink);
    let mark = pos2(tag.right() - tag.width() * 0.18, tag.center().y);
    paint_tag_mark(p, mark, h, state, ink);
}

/// The mark of a tag that has one, centred on `c`.
fn paint_tag_mark(p: &Painter, c: egui::Pos2, h: f32, state: TagState, ink: egui::Color32) {
    let stroke = Stroke::new((h * 0.11).max(0.9), ink);
    let arc = |c: egui::Pos2, r: f32, from: f32, to: f32| -> Vec<egui::Pos2> {
        (0..=8).map(|i| from + (to - from) * i as f32 / 8.0).map(|a| c + vec2(r * a.cos(), r * a.sin())).collect()
    };
    match state {
        TagState::Running => {
            p.add(egui::epaint::PathShape::line(arc(c, h * 0.22, -1.2, 3.7), stroke));
        }
        TagState::Error => {
            p.line_segment([c + vec2(0.0, -h * 0.3), c + vec2(0.0, h * 0.08)], stroke);
            p.circle_filled(c + vec2(0.0, h * 0.27), (h * 0.075).max(0.7), ink);
        }
        TagState::Blocked => {
            let body = Rect::from_center_size(c + vec2(0.0, h * 0.13), vec2(h * 0.42, h * 0.3));
            p.rect_filled(body, h * 0.04, ink);
            p.add(egui::epaint::PathShape::line(arc(c + vec2(0.0, -h * 0.03), h * 0.13, std::f32::consts::PI, std::f32::consts::TAU), stroke));
        }
        TagState::Ready | TagState::NotRunning => {}
    }
}

/// The Python cell's area on the sheet: the cell, or the merged area it starts.
fn area(geo: &Geo, sh: &Sheet, c: CellRef) -> Rect {
    match sh.merge_at(c) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, c),
    }
}

/// What opens the card of the Python cell at `c` (logical coordinates), when `c` holds Python code:
/// its PY tag, or the double square that stands there when the cell says `Image` and is ready. The
/// area is the one that is painted.
pub fn tag_at(session: &Session, geo: &Geo, wb: &Workbook, sh: &Sheet, c: CellRef) -> Option<Rect> {
    let c = sh.merge_at(c).map(|m| m.start).unwrap_or(c);
    let cell = sh.cell(c)?;
    let py = cell.py.as_deref()?;
    let rect = area(geo, sh, c);
    let style = wb.styles.get(cell.style);
    if tag_state(session, wb.active_sheet(), c, py, &cell.value) == TagState::Ready && crate::python_plots::says_image(py, &cell.value) {
        crate::python_plots::mark_rect(rect, rect.left(), style.font.size, geo.z, style.align.v)
    } else {
        tag_rect(rect, rect.left(), geo.z, style.align.v)
    }
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
    /// A preview of the result, empty when there is none.
    pub body: String,
    /// A Python Object's table, when it has one: the card shows this instead of the preview.
    pub table: Option<Arc<PyTable>>,
    /// The result is a Python Object with something to spill.
    pub spillable: bool,
    /// The table this cell's object had can't be here: the file didn't keep it.
    pub run_to_see: bool,
    /// A text result with line breaks: whether it spills one line per row.
    pub spill_lines: Option<bool>,
    /// The picture the result is, when it is one: the card shows this instead of the preview.
    pub image: Option<gridcraft_engine::model::PyImage>,
}

/// Whether what a Python object's cell shows names a kind of object that can have a table, with or
/// without its size: all a file leaves of it.
fn names_a_table(shown: &str) -> bool {
    let is_size = |s: &str| !s.is_empty() && s.split('x').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let name = shown.rsplit_once(' ').filter(|(_, size)| is_size(size)).map_or(shown, |(name, _)| name);
    matches!(name, "DataFrame" | "Series" | "ndarray" | "list" | "tuple")
}

/// The card's text for the Python cell at `c`, capped ([`MAX_TITLE_CHARS`], [`MAX_PREVIEW_CHARS`]
/// and [`MAX_PREVIEW_LINES`]). `None` when `c` holds no Python code.
pub fn card_text(wb: &Workbook, sh: &Sheet, c: CellRef) -> Option<CardText> {
    let py = sh.cell(c)?.py.as_deref()?;
    let value = sh.value(c);
    let output = if py.shows_object() { PYTHON_OBJECT } else { EXCEL_VALUE };
    let shown = || capped(&gridcraft_engine::display::cell_text(wb, sh, c), MAX_PREVIEW_CHARS, MAX_PREVIEW_LINES);
    let (title, body) = match (&py.output, &value) {
        (Some(o), _) if !o.type_name.is_empty() => {
            let body = if o.preview.is_empty() { shown() } else { capped(&o.preview, MAX_PREVIEW_CHARS, MAX_PREVIEW_LINES) };
            // A Python object's cell says what it is, with its size.
            let name = match &o.value {
                Value::Text(label) if !o.convertible => label.as_ref(),
                _ => o.type_name.as_str(),
            };
            (capped(name, MAX_TITLE_CHARS, 1), body)
        }
        _ => (capped(&gridcraft_engine::display::cell_text(wb, sh, c), MAX_TITLE_CHARS, 1), String::new()),
    };
    let table = py.output.as_ref().and_then(|o| o.table.clone()).filter(|_| py.object);
    let names_one = matches!(&value, Value::Text(t) if names_a_table(t));
    let from_a_file = py.output.as_ref().is_none_or(|o| !o.from_run);
    let spill_lines =
        py.output.as_ref().filter(|o| o.convertible && matches!(&o.value, Value::Text(t) if t.contains(['\n', '\r']))).map(|_| py.spill_lines);
    Some(CardText {
        title,
        output,
        body,
        spillable: py.object && (table.is_some() || names_one),
        run_to_see: py.object && table.is_none() && names_one && from_a_file,
        spill_lines,
        table,
        image: py.image().cloned(),
    })
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
/// mirrored on a right-to-left sheet; a picture's card beside the cell), and closes it when its cell is no longer a Python cell in
/// view, the selection leaves the cell, an edit starts, or the user clicks elsewhere or presses Esc.
pub fn show(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet, was_open: bool) {
    let Some(card) = app.grid.py_card else { return };
    if active_python_cell(app).is_none_or(|(_, _, at, _)| at != card.cell) {
        app.grid.py_card = None;
        return;
    }
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
    let refusal = gridcraft_engine::cmd::python::clear_refusal(&app.session, card.sheet, card.cell);
    let said = app.session.python_diagnostic(card.sheet, card.cell).cloned();
    let state = app.session.active().and_then(|d| d.wb.sheet(card.sheet)).and_then(|sh| {
        let cell = sh.cell(card.cell)?;
        cell.py.as_deref().map(|py| tag_state(&app.session, card.sheet, card.cell, py, &cell.value))
    });
    let printed = said.as_ref().filter(|d| d.kind.severity == Severity::Error).and_then(|_| last_printed(app, sh, card.cell));
    let locked = app.session.locked_cell_message(card.sheet, card.cell);
    let can_save = app.services.pick_save.is_some() || app.services.download.is_some();
    let screen_size = ui.ctx().content_rect().size();
    let wanted = if text.table.as_ref().is_some_and(|t| !t.is_empty()) { TABLE_CARD_WIDTH } else { CARD_WIDTH };
    let picture_width = text.image.as_ref().map(|_| crate::python_plots::card_width(app, screen_size));
    let width = picture_width.unwrap_or_else(|| wanted.min(screen_size.x - 32.0).max(200.0));
    let can_copy_image = app.services.copy_image.is_some();
    let dragged = app.python.card_image_width.is_some();
    let mut action = None;
    let mut convert = false;
    // A picture's card is tall, so it goes beside the cell. Another card goes under it.
    let (at, pivot) = match picture_width {
        Some(w) => crate::python_plots::card_place(screen, w, ui.ctx().content_rect(), geo.rtl),
        None if geo.rtl => (screen.right_bottom() + vec2(0.0, 2.0), Align2::RIGHT_TOP),
        None => (screen.left_bottom() + vec2(0.0, 2.0), Align2::LEFT_TOP),
    };
    let mut close = false;
    let mut toggle_output = false;
    let output_open = app.python.card_output == Some(card);
    let area = egui::Area::new(egui::Id::unique("python_card"))
        .order(egui::Order::Foreground)
        .pivot(pivot)
        .fixed_pos(at)
        .constrain_to(ui.ctx().content_rect());
    let resp = area.show(ui.ctx(), |ui| {
        let framed = egui::Frame::popup(ui.style()).inner_margin(8.0).show(ui, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                let (tag, _) = ui.allocate_exact_size(vec2(22.0, 15.0), egui::Sense::hover());
                paint_tag(ui.painter(), tag, &t, state.unwrap_or(TagState::Ready));
                if text.image.is_none() {
                    ui.label(egui::RichText::new(&text.title).font(theme::ui_bold(13.0)).color(t.text));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::widgets::icon_button(ui, crate::icons::Icon::Close, t.text_dim, lang.tr("Close"), vec2(18.0, 18.0)).clicked() {
                        close = true;
                    }
                });
            });
            // A picture's card has the return type as a choice, further down.
            if text.image.is_none() {
                ui.label(egui::RichText::new(lang.tr(text.output)).font(theme::ui_font(11.5)).color(t.text_dim));
            }
            if let Some(d) = &said {
                ui.add_space(4.0);
                close |= says(app, ui, d, (card.sheet, card.cell), &t);
                if let Some(run) = printed.as_deref() {
                    if ui.button(lang.tr(if output_open { "Hide Printed Output" } else { "Show Printed Output" })).clicked() {
                        toggle_output = true;
                    }
                    if output_open {
                        egui::ScrollArea::both().id_salt("python_card_output").max_height(140.0).auto_shrink([false, true]).show(ui, |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(run).font(crate::code_editor::font()).color(t.text)).extend());
                        });
                    }
                }
            }
            if text.run_to_see {
                ui.add_space(4.0);
                ui.add(egui::Label::new(egui::RichText::new(lang.tr(RUN_TO_SEE)).font(theme::ui_font(12.5)).color(t.text)).wrap());
                // What the catalog says comes first: Python may not be able to run yet.
                if said.is_none() && ui.button(lang.tr("Run Again")).clicked() {
                    action = Some(Action::Run);
                }
            }
            if let Some(image) = &text.image {
                ui.add_space(4.0);
                crate::python_plots::card_picture(ui, image, screen_size, dragged, lang, &t);
            } else if let Some(table) = &text.table {
                ui.add_space(4.0);
                show_table(ui, table, geo.rtl, screen_size.y, &t, lang);
            } else if !text.body.is_empty() {
                ui.add_space(4.0);
                // A repr keeps its lines (a table's columns line up): it scrolls rather than wraps.
                egui::ScrollArea::both().max_height(220.0).min_scrolled_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&text.body).font(crate::code_editor::font()).color(t.text)).extend());
                });
            }
            if let Some(on) = text.spill_lines {
                ui.add_space(4.0);
                let mut spill = on;
                let check = ui.add_enabled(locked.is_none(), egui::Checkbox::new(&mut spill, lang.tr(SPILL_LINES)));
                if check.changed() {
                    action = Some(Action::SpillLines(spill));
                }
            }
            ui.add_space(4.0);
            action = result_buttons(ui, &text, locked.is_none(), can_save, lang).or(action);
            if text.image.is_some() {
                action = picture_buttons(ui, &text, locked.is_none(), (can_copy_image, can_save), lang).or(action);
            }
            ui.add_space(6.0);
            convert = convert_button(ui, lang, refusal.as_deref(), true, vec2(0.0, 22.0));
        });
        if let Some(w) = picture_width {
            crate::python_plots::card_grip(app, ui, framed.response.rect, w);
        }
    });
    if toggle_output {
        app.python.card_output = if output_open { None } else { Some(card) };
    }
    if convert {
        app.run_or_alert("python.clear", json!({"sheet": card.sheet, "cell": card.cell.a1()}));
    }
    let spilled = action == Some(Action::SpillHere);
    if let Some(action) = action {
        act(app, ui.ctx(), card, action);
    }
    if convert || spilled || close || popup_dismissed(&resp.response, was_open) {
        app.grid.py_card = None;
        app.python.card_output = None;
    }
}

/// What the card's buttons on a result ask for, done once the card is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Copy(&'static str),
    Save,
    SpillHere,
    SpillLines(bool),
    Run,
    CopyImage,
    SaveImage,
    DisplayOverCells,
    Output(bool),
}

/// The table, what it has left out, or that it holds nothing.
fn show_table(ui: &mut egui::Ui, table: &Arc<PyTable>, rtl: bool, screen_height: f32, t: &Tokens, lang: crate::i18n::Language) {
    let dim = |ui: &mut egui::Ui, text: &str| {
        ui.add(egui::Label::new(egui::RichText::new(text).font(theme::ui_font(12.0)).color(t.text_dim)).wrap());
    };
    if table.is_empty() {
        dim(ui, lang.tr(NO_COLUMNS));
        return;
    }
    table::grid(ui, table, t, rtl, (screen_height * 0.45).clamp(90.0, 260.0));
    if table.rows == 0 {
        dim(ui, lang.tr(NO_ROWS));
    }
    if let Some(note) = table::cut_note(lang, table) {
        ui.add_space(4.0);
        egui::Frame::new().fill(t.notice_warn_bg).stroke(Stroke::new(1.0, t.notice_warn_border)).corner_radius(3.0).inner_margin(6.0).show(
            ui,
            |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(egui::RichText::new(note).font(theme::ui_font(12.0)).color(t.text)).wrap());
            },
        );
    }
}

/// Copy, Save and Spill Here, for a result that has them. Returns the one clicked.
fn result_buttons(ui: &mut egui::Ui, text: &CardText, can_edit: bool, can_save: bool, lang: crate::i18n::Language) -> Option<Action> {
    let has_cells = text.table.as_ref().is_some_and(|t| !t.is_empty());
    if !has_cells && !text.spillable {
        return None;
    }
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        if has_cells {
            if ui.button(lang.tr(COPY_CSV)).clicked() {
                action = Some(Action::Copy("csv"));
            }
            if ui.button(lang.tr(COPY_MARKDOWN)).clicked() {
                action = Some(Action::Copy("markdown"));
            }
            if ui.add_enabled(can_save, egui::Button::new(lang.tr(SAVE_CSV))).clicked() {
                action = Some(Action::Save);
            }
        }
        if text.spillable && ui.add_enabled(can_edit, egui::Button::new(lang.tr(SPILL_HERE))).clicked() {
            action = Some(Action::SpillHere);
        }
    });
    action
}

/// What the card of a picture offers: its return type, and taking the picture somewhere.
fn picture_buttons(
    ui: &mut egui::Ui,
    text: &CardText,
    can_edit: bool,
    (can_copy, can_save): (bool, bool),
    lang: crate::i18n::Language,
) -> Option<Action> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        for (label, object) in [(PYTHON_OBJECT, true), (EXCEL_VALUE, false)] {
            if ui.add_enabled(can_edit, egui::RadioButton::new(text.output == label, lang.tr(label))).clicked() {
                action = Some(Action::Output(object));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(can_copy, egui::Button::new(lang.tr(crate::python_plots::COPY_IMAGE))).clicked() {
            action = Some(Action::CopyImage);
        }
        if ui.add_enabled(can_save, egui::Button::new(lang.tr(crate::python_plots::SAVE_IMAGE))).clicked() {
            action = Some(Action::SaveImage);
        }
        if ui.button(lang.tr(crate::python_plots::DISPLAY_OVER_CELLS)).clicked() {
            action = Some(Action::DisplayOverCells);
        }
    });
    action
}

fn act(app: &mut SheetApp, ctx: &egui::Context, card: Card, action: Action) {
    let (sheet, cell) = (card.sheet, card.cell.a1());
    match action {
        Action::CopyImage => crate::python_plots::copy_image(app, card.sheet, card.cell),
        Action::SaveImage => crate::python_plots::save_image(app, card.sheet, card.cell),
        Action::DisplayOverCells => app.run_or_alert("python.displayOverCells", json!({"sheet": sheet, "cell": cell})),
        Action::Output(object) => {
            app.run_or_alert("python.output", json!({"sheet": sheet, "cell": cell, "mode": if object { "object" } else { "value" }}))
        }
        Action::Copy(format) => copy_table(app, ctx, card, format),
        Action::Save => save_table(app, json!({"sheet": sheet, "cell": cell})),
        Action::SpillHere => app.run_or_alert("python.output", json!({"sheet": sheet, "cell": cell, "mode": "value"})),
        Action::SpillLines(on) => {
            let lines = if on { "rows" } else { "cell" };
            app.run_or_alert("python.output", json!({"sheet": sheet, "cell": cell, "lines": lines}));
        }
        Action::Run => app.run_or_alert("python.run", json!({"sheet": sheet, "cell": cell})),
    }
}

fn alert(app: &mut SheetApp, error: &str) {
    app.message = Some(("GridCraft".into(), crate::clean_error(error)));
}

/// Puts the table on the clipboard. It is the bounded preview, and the person hears when that isn't
/// all of it.
fn copy_table(app: &mut SheetApp, ctx: &egui::Context, card: Card, format: &str) {
    match app.run("python.table", json!({"sheet": card.sheet, "cell": card.cell.a1(), "format": format})) {
        Ok(table) => {
            ctx.copy_text(table["text"].as_str().unwrap_or_default().to_string());
            let said = if table["cut"] == true { COPIED_CUT } else { "Copied to the clipboard." };
            app.toast = Some((said.into(), crate::now_ms()));
        }
        Err(e) => alert(app, &e),
    }
}

/// What the person hears after a copy or a save of a table that isn't all there.
pub const COPIED_CUT: &str = "Copied only what is shown: the rest of the table isn't in the copy.";
pub const SAVED_CUT: &str = "Saved only what is shown: the rest of the table isn't in the file.";
pub const SAVED: &str = "The table is saved.";

/// Asks where to save the table of the cell `at` names (`{sheet, cell}`), and saves it:
/// `python.saveCsv` itself never asks. The native file picker on the desktop, a download on the web.
/// Cancelling the picker does nothing.
fn save_table(app: &mut SheetApp, at: Json) {
    let title = app.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
    let stem = std::path::Path::new(&title).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book1").to_string();
    let cell: String = at["cell"].as_str().unwrap_or_default().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    let name = format!("{stem}-{cell}.csv");
    let with = |key: &str, value: &str| {
        let mut params = at.clone();
        params[key] = json!(value);
        params
    };
    let done = if let Some(pick) = &app.services.pick_save {
        let Some(path) = pick(&name) else { return };
        app.run("python.saveCsv", with("path", &path))
    } else if app.services.download.is_some() {
        app.run("python.table", with("format", "csv")).inspect(|table| {
            if let Some(download) = &app.services.download {
                download(&name, table["text"].as_str().unwrap_or_default().as_bytes());
            }
        })
    } else {
        Err("Exporting needs a file picker, which this window doesn't have.".to_string())
    };
    match done {
        Ok(saved) => app.toast = Some((if saved["cut"] == true { SAVED_CUT } else { SAVED }.into(), crate::now_ms())),
        Err(e) => alert(app, &e),
    }
}

/// `python.saveCsv` from a menu, with no path: it asks where, for the cell the parameters name or the
/// active Python cell. Programmatic callers use `run`, which gets an error instead.
pub(crate) fn save_with_picker(app: &mut SheetApp, params: &Json) {
    let named = params.get("cell").is_some_and(Json::is_string);
    match active_python_cell(app) {
        _ if named => {
            let mut at = json!({"cell": params["cell"]});
            if let Some(sheet) = params.get("sheet") {
                at["sheet"] = sheet.clone();
            }
            save_table(app, at);
        }
        Some((_, sheet, cell, _)) => save_table(app, json!({"sheet": sheet, "cell": cell.a1()})),
        None => alert(app, "The active cell isn't a Python cell."),
    }
}

/// What the catalog says about the cell: its message, the detail as it came with the line when
/// there is one, and the kind's action. Returns whether the action means the card is done.
fn says(app: &mut SheetApp, ui: &mut egui::Ui, d: &Diagnostic, cell: (usize, CellRef), t: &Tokens) -> bool {
    let lang = app.ui.language;
    ui.add(egui::Label::new(egui::RichText::new(lang.tr(d.kind.message)).font(theme::ui_font(12.5)).color(t.text)).wrap());
    if let Some(line) = d.line {
        let text = lang.tr("Line {line}").replace("{line}", &line.to_string());
        ui.label(egui::RichText::new(text).font(theme::ui_font(11.5)).color(t.text_dim));
    }
    if !d.detail.is_empty() {
        let detail = capped(&d.detail, MAX_PREVIEW_CHARS, MAX_PREVIEW_LINES);
        ui.add(egui::Label::new(egui::RichText::new(detail).font(crate::code_editor::font()).color(t.text_dim)).wrap());
    }
    ui.add_space(2.0);
    crate::python_ui::action_button(app, ui, d, Some(cell))
}

/// What the cell's last run printed, when it printed something.
fn last_printed(app: &SheetApp, sh: &Sheet, cell: CellRef) -> Option<String> {
    let run = app.session.python_run_log().filter(|r| r.cell == cell && r.sheet == sh.name).last()?;
    (!run.stdout.is_empty()).then(|| capped(&run.stdout, MAX_PREVIEW_CHARS * 8, MAX_PREVIEW_LINES * 8))
}

/// Esc, the window losing focus, or a click outside a card that was already open.
fn popup_dismissed(response: &egui::Response, was_open: bool) -> bool {
    response.ctx.input(|i| !i.focused || i.key_pressed(egui::Key::Escape)) || (was_open && response.clicked_elsewhere())
}

/// Convert to Normal Cell: enabled where `python.clear` would convert the cell, else disabled
/// with the reason (`refusal`) on hover. Returns whether it was clicked.
fn convert_button(ui: &mut egui::Ui, lang: crate::i18n::Language, refusal: Option<&str>, framed: bool, min: egui::Vec2) -> bool {
    let button = egui::Button::new(lang.tr(CONVERT)).frame_when_inactive(framed).min_size(min);
    let r = ui.add_enabled(refusal.is_none(), button);
    match refusal {
        Some(why) => r.on_disabled_hover_text(lang.tr(why)).clicked(),
        None => r.clicked(),
    }
}

/// The cell menu's items on a Python cell: "Edit in Python Editor", its return type ("Python
/// Object" and "Excel Value", the current one marked) and Convert to Normal Cell. Returns whether
/// one was picked (the menu then closes).
pub fn menu_items(app: &mut SheetApp, ui: &mut egui::Ui) -> bool {
    let Some((_, sheet, cell, py)) = active_python_cell(app) else { return false };
    let lang = app.ui.language;
    if crate::python_window::menu_offers(app) && ui.button(lang.tr(crate::python_window::EDIT_IN_EDITOR)).clicked() {
        crate::python_window::open_from_menu(app);
        return true;
    }
    let mut picked = None;
    for (label, object) in [(PYTHON_OBJECT, true), (EXCEL_VALUE, false)] {
        if ui.radio(py.shows_object() == object, lang.tr(label)).clicked() {
            picked = Some(object);
        }
    }
    if py.image().is_some() && ui.button(lang.tr(crate::python_plots::DISPLAY_OVER_CELLS)).clicked() {
        crate::python_plots::display_over_cells(app);
        return true;
    }
    let refusal = gridcraft_engine::cmd::python::clear_refusal(&app.session, sheet, cell);
    let convert = convert_button(ui, lang, refusal.as_deref(), false, vec2(190.0, 20.0));
    ui.separator();
    if convert {
        app.run_or_alert("python.clear", json!({"sheet": sheet, "cell": cell.a1()}));
        return true;
    }
    let Some(object) = picked else { return false };
    if object != py.shows_object() || !py.output_chosen {
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
    fn a_label_names_a_table_with_or_without_its_size() {
        for yes in ["DataFrame 6x4", "DataFrame", "Series 12", "list 12", "tuple 3", "ndarray 2x3", "ndarray 2x3x4", "ndarray"] {
            assert!(names_a_table(yes), "{yes}");
        }
        for no in ["dict 3", "function", "set 2", "DataFrame 6x", "DataFrame x4", "DataFrame 6y4", "hello 12", "Data Frame 6x4", "", " ", "list  12"]
        {
            assert!(!names_a_table(no), "{no}");
        }
    }

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
