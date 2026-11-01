//! The formula bar (Name Box, ✕ ✓ fx, formula field) and the shared cell editor widget.

use egui::{FontId, Key, Rect, Sense, Stroke, StrokeKind, TextEdit, pos2, vec2};
use serde_json::json;

use crate::SheetApp;
use crate::editor::formula_refs;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Lines the formula bar (and the in-cell editor) grows to before it scrolls.
pub const MAX_LINES: usize = 5;
/// The bar's height at one line.
const ONE_LINE: f32 = 30.0;
/// What the bar adds around the field's rows: the panel's margins (3 + 3), the field's (3 + 3)
/// and two pixels for the rounding to the pixel grid.
const PAD: f32 = 14.0;

/// The bar's height for one line of a font whose rows are `row_h` tall: Excel's 30 px, unless
/// the rows need more (the Windows UI font's row is 17.28 px). Every line counted must fit in
/// the field, whatever the font; `n` lines take `one_line(row_h) + (n - 1) * row_h`.
pub fn one_line(row_h: f32) -> f32 {
    ONE_LINE.max(row_h + PAD)
}
/// How long the bar takes to grow or shrink to a new line count.
const EASE_SECONDS: f32 = 0.12;

/// Where the formula bar put its parts last frame (for tests).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarLayout {
    pub name_box: Rect,
    /// ✕ ✓ fx.
    pub buttons: Rect,
    pub field: Rect,
    pub height: f32,
    /// Laid-out lines of the content (wrapped to the field), and how many show without scrolling.
    pub lines: usize,
    pub visible_lines: usize,
    /// The caret while editing in the bar.
    pub caret: Option<Rect>,
}

impl Default for BarLayout {
    fn default() -> Self {
        BarLayout { name_box: Rect::NOTHING, buttons: Rect::NOTHING, field: Rect::NOTHING, height: 0.0, lines: 1, visible_lines: 1, caret: None }
    }
}

impl BarLayout {
    /// Returns the stable widget id used for the formula-bar layout state.
    pub fn id() -> egui::Id {
        egui::Id::unique("formula_bar.layout")
    }

    /// Last frame's layout.
    pub fn get(ctx: &egui::Context) -> BarLayout {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }
}

/// What the editor widget laid out: its line count and caret.
#[derive(Clone, Copy, Debug, Default)]
pub struct EditorInfo {
    pub lines: usize,
    pub caret: Option<Rect>,
}

/// Draws the formula bar: line-count-driven height, Name Box, cancel/enter/Fx buttons, the editable field, muted read-only spill text and the expansion chevron.
pub fn show(app: &mut SheetApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let font = theme::ui_font(13.0);
    let row_h = ui.ctx().fonts_mut(|f| f.row_height(&font));
    let last = BarLayout::get(ui.ctx());
    // Height follows the content's line count (last frame's), 1 to MAX_LINES; the chevron pins it
    // at MAX_LINES. It eases only when that target changes.
    let lines = if app.ui.formula_bar_expanded { MAX_LINES } else { last.lines.clamp(1, MAX_LINES) };
    let target = one_line(row_h) + (lines - 1) as f32 * row_h;
    let h = ui.ctx().animate_value_with_time(egui::Id::unique("formula_bar.height"), target, EASE_SECONDS);
    let mut layout = BarLayout { height: h, ..Default::default() };
    egui::Panel::top("formula_bar")
        .exact_size(h)
        .frame(egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin { left: 8, right: 8, top: 3, bottom: 3 }))
        .show(ui, |ui| {
            // Top-aligned: the Name Box and buttons stay on the first line as the field grows.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                layout.name_box = name_box(app, ui, &t);
                ui.add_space(4.0);
                let editing = app.editor.is_some();
                let c = if editing { t.text } else { t.text_disabled };
                let cancel = crate::widgets::icon_button(ui, Icon::Close, c, "Cancel", vec2(22.0, 24.0));
                if cancel.clicked() && editing {
                    app.cancel_edit();
                }
                if crate::widgets::icon_button(ui, Icon::Check, c, "Enter", vec2(22.0, 24.0)).clicked() && editing {
                    app.commit_edit(0, 0, false, false);
                }
                let fx = crate::widgets::icon_button(ui, Icon::Fx, t.text, "Insert Function", vec2(26.0, 24.0));
                if fx.clicked() {
                    app.open_dialog("insertFunction", json!({}));
                }
                layout.buttons = cancel.rect.union(fx.rect);
                ui.add_space(4.0);
                // The formula field fills the rest, as tall as the bar.
                let avail = ui.available_width() - 24.0;
                let field = Rect::from_min_size(ui.cursor().min, vec2(avail, ui.available_height()));
                ui.painter().rect_filled(field, 4.0, t.input_bg);
                ui.painter().rect_stroke(field, 4.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
                let inner = field.shrink2(vec2(6.0, 3.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
                let id = egui::Id::unique("gridcraft.formula_bar");
                // A spill child shows its anchor's formula muted and read-only.
                let read_only = active_spill_anchor(app).is_some();
                let info = if app.editor.is_some() {
                    editor_widget(app, &mut child, id, font.clone(), true, inner.width(), inner.height())
                } else {
                    // Show the active cell's content; clicking starts editing in the bar.
                    let text = active_input(app);
                    let color = if read_only { t.text_disabled } else { t.text };
                    let galley = child.painter().layout(text, font.clone(), color, inner.width());
                    let lines = galley.rows.len();
                    // Each cell's view starts at its top: the offset is kept per cell, not per bar.
                    let shown = app.session.active().map(|d| (d.uid, d.wb.active_sheet(), d.selection.active.row, d.selection.active.col));
                    egui::ScrollArea::vertical().id_salt(id.with(("view", shown))).max_height(inner.height()).auto_shrink([false, true]).show(
                        &mut child,
                        |ui| {
                            if ui.add(egui::Label::new(galley).sense(Sense::click())).clicked() && !read_only {
                                app.begin_edit(None, true);
                            }
                        },
                    );
                    if !read_only && ui.interact(field, id.with("click"), Sense::click()).clicked() {
                        app.begin_edit(None, true);
                    }
                    EditorInfo { lines, caret: None }
                };
                layout.field = field;
                layout.lines = info.lines.max(1);
                layout.visible_lines = ((inner.height() / row_h).floor() as usize).max(1);
                layout.caret = info.caret;
                ui.allocate_space(vec2(avail, field.height()));
                let chevron = if app.ui.formula_bar_expanded { Icon::ChevronUp } else { Icon::Chevron };
                if crate::widgets::icon_button(ui, chevron, t.text_dim, "Expand formula bar", vec2(20.0, 24.0)).clicked() {
                    app.ui.formula_bar_expanded = !app.ui.formula_bar_expanded;
                }
            });
        });
    ui.ctx().data_mut(|d| d.insert_temp(BarLayout::id(), layout));
}

/// Resolve ownership on every frame; never cache a spill range in presentation state.
pub(crate) fn active_spill_anchor(app: &SheetApp) -> Option<gridcraft_engine::core::CellRef> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let at = sh.merge_at(d.selection.active).map(|m| m.start).unwrap_or(d.selection.active);
    sh.spill_anchor(at)
}

fn active_input(app: &SheetApp) -> String {
    let Some(d) = app.session.active() else { return String::new() };
    let Some(sh) = d.wb.active() else { return String::new() };
    let a = sh.merge_at(d.selection.active).map(|m| m.start).unwrap_or(d.selection.active);
    if let Some(picture) = sh.cell_pictures.get(&a) {
        return if picture.alt.is_empty() { "Picture in cell".into() } else { format!("Picture: {}", picture.alt) };
    }
    if let Some(anchor) = sh.spill_anchor(a) {
        if gridcraft_engine::display::formula_hidden(&d.wb, sh, anchor) {
            return String::new();
        }
        return sh
            .cell(anchor)
            .map(|c| crate::formula_locale::display(&c.input_text(), app.ui.language.formula_locale(), &d.wb, d.wb.active_sheet()))
            .unwrap_or_default();
    }
    if let Some(c) = sh.cell(a) {
        if c.formula.is_some() && d.wb.styles.get(c.style).protection.hidden && sh.is_protected() {
            return String::new();
        }
        return if let Some(f) = &c.formula {
            let shown = crate::formula_locale::display(&c.input_text(), app.ui.language.formula_locale(), &d.wb, d.wb.active_sheet());
            // A legacy array formula shows in braces (`{=A1:A2*2}`), as `Cell::bar_text` does.
            if f.array.is_some() { format!("{{{shown}}}") } else { shown }
        } else {
            gridcraft_engine::display::edit_text(&d.wb, sh, a)
        };
    }
    String::new()
}

/// Draws the Name Box and its name dropdown; Enter jumps to a reference or known name (or defines a new name) and is consumed so the grid does not move down.
fn name_box(app: &mut SheetApp, ui: &mut egui::Ui, t: &Tokens) -> Rect {
    let id = egui::Id::unique("gridcraft.name_box");
    let shown = app.name_box_text();
    let mut text = app.name_box.clone().unwrap_or(shown.clone());
    let rect = Rect::from_min_size(ui.cursor().min, vec2(118.0, 24.0));
    ui.painter().rect_filled(rect, 4.0, t.input_bg);
    ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(rect.min + vec2(6.0, 3.0), vec2(96.0, 18.0))));
    // The box has no visible label of its own: screen readers and the strict accessibility check
    // need a name (egui 0.37 makes unnamed text inputs a test failure).
    let accessible_name = app.ui.language.tr("Name Box").to_string();
    let resp = child
        .add(TextEdit::singleline(&mut text).id(id).frame(egui::Frame::NONE).font(theme::ui_font(13.0)).desired_width(96.0))
        .accessible_name(accessible_name);
    if resp.gained_focus() {
        app.name_box = Some(shown.clone());
    }
    if resp.changed() {
        app.name_box = Some(text.clone());
    }
    if resp.lost_focus() {
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            // The Enter that commits the Name Box is used up here. Left in the frame's input, the
            // grid (drawn after the formula bar, and focused again now the box let go) would read
            // it as its own Enter and move the active cell one row down from the jump target (#247).
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { key: Key::Enter, .. })));
            let target = text.trim().to_string();
            if !target.is_empty() && target != shown {
                // A new name for the selection, or a place to go.
                let is_ref = gridcraft_engine::core::RangeRef::parse(target.split('!').next_back().unwrap_or("")).is_some();
                let known = app.session.active().is_some_and(|d| d.wb.name(&target, d.wb.active_sheet()).is_some() || d.wb.table(&target).is_some());
                let r = if is_ref || known {
                    app.run("edit.goTo", json!({"reference": target}))
                } else {
                    app.run("formulas.defineName", json!({"name": target}))
                };
                if let Err(e) = r {
                    app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                }
                app.grid.ensure_visible = true;
            }
        }
        app.name_box = None;
    }
    ui.allocate_space(vec2(118.0, 24.0));
    // Dropdown of names.
    let names: Vec<String> =
        app.session.active().map(|d| d.wb.names.iter().filter(|n| !n.hidden).map(|n| n.name.clone()).collect()).unwrap_or_default();
    let mb = Rect::from_min_size(pos2(rect.right() - 16.0, rect.top() + 4.0), vec2(14.0, 16.0));
    icons::paint(ui.painter(), mb, Icon::Chevron, t.text_dim);
    let r = ui.interact(mb, id.with("drop"), Sense::click());
    egui::Popup::menu(&r).show(|ui| {
        if names.is_empty() {
            ui.label(egui::RichText::new("No names defined").italics());
        }
        for n in names {
            if ui.button(&n).clicked() {
                let _ = app.run("edit.goTo", json!({"reference": n}));
                app.grid.ensure_visible = true;
            }
        }
    });
    rect
}

/// Backspace and Delete take back the AutoComplete proposal (Excel's selected rest), not a typed character.
fn dismiss_completion(i: &mut egui::InputState, completion: &mut Option<String>) {
    if completion.is_some() && (i.consume_key(egui::Modifiers::NONE, Key::Backspace) || i.consume_key(egui::Modifiers::NONE, Key::Delete)) {
        *completion = None;
    }
}

/// Column AutoComplete proposes only after typing: deleting characters (Backspace, Delete) brings no proposal back, as in Excel.
fn entry_shrank(chars_before: usize, text: &str) -> bool {
    text.chars().count() < chars_before
}

/// The editor's text field, the same in the cell and in the formula bar. Enter is the editor's
/// own key (it commits, and Alt+Enter breaks the line), so the field itself does nothing with it:
/// a message box over the editor takes Enter to close, and the field must not add a line then.
fn show_edit<'t>(
    edit: TextEdit<'t>,
    ui: &mut egui::Ui,
    id: egui::Id,
    font: FontId,
    width: f32,
    layouter: &'t mut dyn FnMut(&egui::Ui, &dyn egui::TextBuffer, f32) -> std::sync::Arc<egui::Galley>,
) -> egui::text_edit::TextEditOutput {
    edit.id(id)
        .frame(egui::Frame::NONE)
        .font(font)
        .desired_width(width)
        .desired_rows(1)
        .layouter(layouter)
        .lock_focus(true)
        .return_key(None)
        .event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
        .show(ui)
}

/// The text editor used in the cell and in the formula bar (they share `app.editor`). It shows
/// `max_height` of text and scrolls beyond that, keeping the caret in view.
pub fn editor_widget(app: &mut SheetApp, ui: &mut egui::Ui, id: egui::Id, font: FontId, multiline: bool, width: f32, max_height: f32) -> EditorInfo {
    let Some(mut ed) = app.editor.take() else { return EditorInfo::default() };
    let mine = ed.from_formula_bar == (id == egui::Id::unique("gridcraft.formula_bar"));
    // Keys the editor handles itself (before the TextEdit sees them), only where it has focus.
    let focused = ui.ctx().memory(|m| m.focused());
    let has_focus = focused == Some(id) || (mine && (ed.request_focus || focused.is_none()));
    let EditorKeyOutcome { action, arrow, cancel } = editor_shortcuts(app, ui, id, &mut ed, mine, has_focus);
    // The text widget.
    let refs = formula_refs(&ed.text);
    let t = Tokens::get(ui.ctx());
    let on_sheet = id == egui::Id::unique("gridcraft.cell_editor");
    let cell_text_color = if on_sheet { t.cell_text } else { t.text };
    let font2 = font.clone();
    let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let s = buf.as_str();
        let mut job = egui::text::LayoutJob::default();
        let mut pos = 0usize;
        let mut spans: Vec<(usize, usize, usize)> = refs.iter().filter(|r| r.1 <= s.len()).map(|r| (r.0, r.1, r.4)).collect();
        spans.sort();
        for (a, b, idx) in spans {
            if a < pos || !s.is_char_boundary(a) || !s.is_char_boundary(b) {
                continue;
            }
            job.append(s.get(pos..a).unwrap_or(""), 0.0, egui::TextFormat::simple(font2.clone(), cell_text_color));
            job.append(s.get(a..b).unwrap_or(""), 0.0, egui::TextFormat::simple(font2.clone(), t.ref_color(idx, on_sheet)));
            pos = b;
        }
        job.append(s.get(pos..).unwrap_or(""), 0.0, egui::TextFormat::simple(font2.clone(), cell_text_color));
        job.wrap.max_width = if multiline { wrap_width } else { f32::INFINITY };
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let chars_before = ed.text.chars().count();
    // A dialog opened over the edit (Paste Name) takes the keyboard until it closes, and the
    // editor takes it back afterwards.
    let dialog_open = app.dialog.is_some();
    if dialog_open && mine {
        ed.request_focus = true;
        ed.keep_selection = true;
    }
    let synced = mine && !dialog_open && (ed.request_focus || ed.sync_caret);
    let edit = if multiline || ed.text.contains('\n') { TextEdit::multiline(&mut ed.text) } else { TextEdit::singleline(&mut ed.text) };
    let edit = edit.interactive(!dialog_open);
    let caret_index = ed.caret;
    // The in-cell editor grows with its text and never scrolls, so the field keeps its whole width
    // for the text (a scroll area would take the bar's width off it and wrap the text narrower
    // than the cell does). The formula bar shows its lines and scrolls past them.
    let mut output = if on_sheet {
        show_edit(edit, ui, id, font, width, &mut layouter)
    } else {
        egui::ScrollArea::vertical()
            .id_salt(id.with("scroll"))
            .max_height(max_height.max(1.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let output = show_edit(edit, ui, id, font, width, &mut layouter);
                // A caret moved by our own keys (Alt+Enter, F4, point mode) isn't the TextEdit's
                // doing, so it doesn't scroll to it itself.
                if synced {
                    let caret = output.galley.pos_from_cursor(egui::text::CCursor::new(caret_index)).translate(output.galley_pos.to_vec2());
                    ui.scroll_to_rect(caret.expand(2.0), None);
                }
                output
            })
            .inner
    };
    // Both editors are text inputs without a visible label next to them: name the one being shown.
    let accessible_name = (if on_sheet { app.ui.language.tr("Cell Editor") } else { app.ui.language.tr("Formula Bar") }).to_string();
    let resp = output.response.response.clone().accessible_name(accessible_name);
    if synced {
        if ed.request_focus {
            resp.request_focus();
        }
        ed.request_focus = false;
        // Closing a dialog that changed nothing (Cancel) gives the selection back as it was.
        let range = if ed.keep_selection && !ed.sync_caret {
            egui::text::CCursorRange::two(egui::text::CCursor::new(ed.anchor), egui::text::CCursor::new(ed.caret))
        } else {
            ed.anchor = ed.caret;
            egui::text::CCursorRange::one(egui::text::CCursor::new(ed.caret))
        };
        ed.sync_caret = false;
        ed.keep_selection = false;
        output.state.cursor.set_char_range(Some(range));
        output.state.store(ui.ctx(), id);
    } else if resp.clicked() || resp.gained_focus() {
        // Clicking into the other editor moves editing there.
        ed.from_formula_bar = id == egui::Id::unique("gridcraft.formula_bar");
        ed.enter_mode = false;
        app.session.mode = gridcraft_engine::Mode::Edit;
    }
    if !synced
        && !dialog_open
        && let Some(r) = output.cursor_range
    {
        ed.caret = r.primary.index.into();
        ed.anchor = r.secondary.index.into();
    }
    if resp.changed() {
        ed.completion = None;
        if app.ui.autocomplete
            && !ed.is_formula()
            && !entry_shrank(chars_before, &ed.text)
            && ed.caret == ed.text.chars().count()
            && let Some(sh) = app.session.active().and_then(|d| d.wb.sheet(ed.sheet))
        {
            ed.completion = crate::editor::column_completion(sh, ed.cell, &ed.text);
        }
        ed.point = None;
        ed.point_anchor = None;
        // Typing ends the pointing: the next arrow starts again from the edited cell, as in Excel.
        ed.point_cell = None;
        let names: Vec<String> = function_names()
            .iter()
            .map(|name| {
                app.session.active().map(|d| crate::formula_locale::completion_name(name, ed.locale, &d.wb, ed.sheet)).unwrap_or(name).to_string()
            })
            .collect();
        ed.update_autocomplete(&names);
        app.session.mode = if ed.can_point() {
            gridcraft_engine::Mode::Point
        } else if ed.enter_mode {
            gridcraft_engine::Mode::Enter
        } else {
            gridcraft_engine::Mode::Edit
        };
    }
    show_editor_suggestions(ui, EditorSuggestion { app, id, resp: &resp, ed: &mut ed, galley: &output.galley, tokens: &t, mine });
    let lines = output.galley.rows.len().max(1);
    let caret = Some(output.galley.pos_from_cursor(egui::text::CCursor::new(ed.caret)).translate(output.galley_pos.to_vec2()));
    app.editor = Some(ed);
    if cancel {
        app.cancel_edit();
    } else if let Some((dr, dc, array, fill)) = action {
        if arrow {
            app.commit_edit_arrow(dr, dc);
        } else {
            app.commit_edit(dr, dc, array, fill);
        }
    }
    EditorInfo { lines, caret }
}

type EditorAction = (i64, i64, bool, bool);

struct EditorKeyOutcome {
    action: Option<EditorAction>,
    arrow: bool,
    cancel: bool,
}

fn editor_shortcuts(
    app: &mut SheetApp,
    ui: &mut egui::Ui,
    id: egui::Id,
    ed: &mut crate::editor::EditState,
    mine: bool,
    has_focus: bool,
) -> EditorKeyOutcome {
    let mut outcome = EditorKeyOutcome { action: None, arrow: false, cancel: false };
    // A message or dialog owns Enter and Escape until it closes.
    if !has_focus || !mine || app.message.is_some() || app.dialog.is_some() {
        return outcome;
    }
    let (action, cancel) = editor_confirm_keys(ui, ed);
    outcome.action = action;
    outcome.cancel = cancel;
    let (action, arrow) = editor_arrow_keys(app, ui, ed, outcome.action);
    outcome.action = action;
    outcome.arrow = arrow;
    editor_other_shortcuts(app, ui, id, ed);
    outcome
}

fn editor_confirm_keys(ui: &mut egui::Ui, ed: &mut crate::editor::EditState) -> (Option<EditorAction>, bool) {
    let (enter, tab, esc, alt_enter, cmd_enter, ctrl_shift_enter, shift) = ui.input_mut(|i| {
        let m = i.modifiers;
        let alt_enter = i.consume_key(egui::Modifiers::ALT, Key::Enter) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::ALT, Key::Enter);
        let ctrl_shift_enter = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::Enter);
        let cmd_enter = i.consume_key(egui::Modifiers::COMMAND, Key::Enter);
        let enter = i.consume_key(egui::Modifiers::NONE, Key::Enter) || i.consume_key(egui::Modifiers::SHIFT, Key::Enter);
        let tab = i.consume_key(egui::Modifiers::NONE, Key::Tab) || i.consume_key(egui::Modifiers::SHIFT, Key::Tab);
        let esc = i.consume_key(egui::Modifiers::NONE, Key::Escape);
        dismiss_completion(i, &mut ed.completion);
        (enter, tab, esc, alt_enter, cmd_enter, ctrl_shift_enter, m.shift)
    });
    if alt_enter {
        let b = ed.text.char_indices().nth(ed.caret).map(|(i, _)| i).unwrap_or(ed.text.len());
        ed.text.insert(b, '\n');
        ed.caret += 1;
        ed.caret_to_end = false;
        ed.sync_caret = true;
    }
    let mut action = if !ed.autocomplete.is_empty() && tab {
        ed.accept_autocomplete();
        None
    } else if enter {
        Some((if shift { -1 } else { 1 }, 0, false, false))
    } else if tab {
        Some((0, if shift { -1 } else { 1 }, false, false))
    } else {
        None
    };
    if cmd_enter {
        action = Some((0, 0, false, true));
    }
    if ctrl_shift_enter {
        action = Some((0, 0, true, false));
    }
    let cancel = if !esc {
        false
    } else if !ed.autocomplete.is_empty() {
        ed.autocomplete.clear();
        false
    } else if ed.completion.is_some() {
        ed.completion = None;
        false
    } else {
        true
    };
    (action, cancel)
}

fn editor_arrow_keys(
    app: &mut SheetApp,
    ui: &mut egui::Ui,
    ed: &mut crate::editor::EditState,
    mut action: Option<EditorAction>,
) -> (Option<EditorAction>, bool) {
    // The modifiers come with the key event: an arrow pressed and released within one frame (a
    // control client's or an automation tool's keystroke) has already lost them from `i.modifiers`.
    let arrow = ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key { key: key @ (Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight), pressed: true, modifiers, .. } => {
                Some((*key, *modifiers))
            }
            _ => None,
        })
    });
    let (up, down, left, right) = (
        arrow.is_some_and(|(k, _)| k == Key::ArrowUp),
        arrow.is_some_and(|(k, _)| k == Key::ArrowDown),
        arrow.is_some_and(|(k, _)| k == Key::ArrowLeft),
        arrow.is_some_and(|(k, _)| k == Key::ArrowRight),
    );
    let (shift, jump) = arrow.map_or((false, false), |(_, m)| (m.shift, m.command));
    if (up || down) && !ed.autocomplete.is_empty() {
        ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
        });
        let n = ed.autocomplete.len();
        ed.ac_index = if down { (ed.ac_index + 1) % n } else { (ed.ac_index + n - 1) % n };
        return (action, false);
    }
    if !ed.enter_mode || (!up && !down && !left && !right) || ed.from_formula_bar {
        return (action, false);
    }
    let (dr, dc) = if up {
        (-1, 0)
    } else if down {
        (1, 0)
    } else if left {
        (0, -1)
    } else {
        (0, 1)
    };
    let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|s| s.right_to_left);
    let dc = if rtl { -dc } else { dc };
    if ed.can_point() {
        // Ctrl+arrow (⌘arrow) jumps the pointed cell to the edge of its data, Ctrl+Shift+arrow
        // extends the pointed range there, as the same keys do on the selection.
        ui.input_mut(|i| {
            for key in [Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight] {
                for modifiers in
                    [egui::Modifiers::NONE, egui::Modifiers::SHIFT, egui::Modifiers::COMMAND, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT]
                {
                    i.consume_key(modifiers, key);
                }
            }
        });
        crate::grid::point_move(app, ed, dr, dc, shift, jump);
        (action, false)
    } else {
        action = Some((dr, dc, false, false));
        (action, true)
    }
}

fn editor_other_shortcuts(app: &mut SheetApp, ui: &mut egui::Ui, id: egui::Id, ed: &mut crate::editor::EditState) {
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F4)) {
        cycle_anchor(ed);
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F3)) {
        app.open_dialog("pasteName", json!({"shortcut": true}));
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::A)) {
        let (text, caret) = (ed.text.clone(), ed.caret);
        if ed.insert_argument_names(app.ui.language)
            && let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id)
        {
            // Record the pre-shortcut text so one Undo removes the inserted argument names.
            let mut undoer = state.undoer();
            let cursor = state.cursor.char_range().unwrap_or(egui::text::CCursorRange::one(egui::text::CCursor::new(caret)));
            undoer.add_undo(&(cursor, text));
            state.set_undoer(undoer);
            state.store(ui.ctx(), id);
        }
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::U)) {
        app.run_or_alert("view.expandFormulaBar", json!({}));
    }
}

fn cycle_anchor(ed: &mut crate::editor::EditState) {
    let caret_b = ed.text.char_indices().nth(ed.caret).map(|(i, _)| i).unwrap_or(ed.text.len());
    for (a, b, ..) in formula_refs(&ed.text) {
        if a <= caret_b && caret_b <= b {
            let Some(r) = ed.text.get(a..b) else { return };
            let cycled: String = r
                .split(':')
                .map(|part| {
                    let p = part.split('!').next_back().unwrap_or(part);
                    let prefix = &part[..part.len() - p.len()];
                    let bare = p.replace('$', "");
                    let letters: String = bare.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
                    let digits = &bare[letters.len()..];
                    let state = (p.starts_with('$'), p[letters.len().min(p.len())..].contains('$') || p.trim_start_matches('$').contains('$'));
                    let next = match state {
                        (false, false) => format!("${letters}${digits}"),
                        (true, true) => format!("{letters}${digits}"),
                        (false, true) => format!("${letters}{digits}"),
                        (true, false) => format!("{letters}{digits}"),
                    };
                    format!("{prefix}{next}")
                })
                .collect::<Vec<_>>()
                .join(":");
            let (Some(x), Some(y)) = (ed.text.get(..a), ed.text.get(b..)) else { return };
            ed.text = format!("{x}{cycled}{y}");
            ed.caret = ed.text.get(..a + cycled.len()).map(|s| s.chars().count()).unwrap_or(ed.caret);
            ed.caret_to_end = false;
            ed.request_focus = true;
            ed.sync_caret = true;
            return;
        }
    }
}

fn function_names() -> Vec<String> {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| gridcraft_engine::cmd::formulas::function_list().iter().filter_map(|f| f["name"].as_str().map(str::to_string)).collect())
        .clone()
}

fn function_info(name: &str) -> Option<serde_json::Value> {
    static LIST: std::sync::OnceLock<Vec<serde_json::Value>> = std::sync::OnceLock::new();
    LIST.get_or_init(gridcraft_engine::cmd::formulas::function_list)
        .iter()
        .find(|f| f["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .cloned()
}

pub fn function_signature(name: &str) -> Option<String> {
    function_info(name).and_then(|f| f["signature"].as_str().map(str::to_string))
}

pub fn function_description(name: &str) -> Option<String> {
    function_info(name).and_then(|f| f["description"].as_str().map(str::to_string))
}

#[cfg(test)]
mod spill_tests {
    use super::*;
    use gridcraft_engine::Session;

    fn app() -> SheetApp {
        let mut s = Session::new();
        s.new_workbook();
        SheetApp::new(s, Default::default())
    }

    fn set(app: &mut SheetApp, cell: &str, input: &str) {
        app.run("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }

    fn select(app: &mut SheetApp, cell: &str) {
        app.cancel_edit();
        app.run("selection.set", json!({"cell": cell})).unwrap();
    }

    fn check(app: &mut SheetApp, cell: &str, text: &str, read_only: bool) {
        select(app, cell);
        assert_eq!(active_input(app), text);
        assert_eq!(active_spill_anchor(app).is_some(), read_only);
        app.begin_edit(None, true);
        assert_eq!(app.editor.is_none(), read_only);
        app.cancel_edit();
        if read_only {
            app.begin_edit(Some("=SUM(1,2)".into()), true);
            assert!(app.editor.is_none());
        }
    }

    #[test]
    fn selection_restores_editing_and_keeps_spill_formulas_only_at_anchor() {
        let mut app = app();
        set(&mut app, "A1", "=SEQUENCE(5,1,10,10)");
        set(&mut app, "D1", "=SUM(1,2)");
        set(&mut app, "D2", "hello");
        for cell in ["A2", "A3", "A4", "A5"] {
            check(&mut app, cell, "=SEQUENCE(5,1,10,10)", true);
        }
        check(&mut app, "A1", "=SEQUENCE(5,1,10,10)", false);
        check(&mut app, "D1", "=SUM(1,2)", false);
        check(&mut app, "D2", "hello", false);
        check(&mut app, "E1", "", false);
        let sh = app.session.active().unwrap().wb.active().unwrap();
        assert!(sh.cell(gridcraft_engine::core::CellRef::parse("A2").unwrap()).and_then(|c| c.formula.as_ref()).is_none());
        // Intentional grid replacement retains the existing entry behavior.
        select(&mut app, "A2");
        app.begin_edit(Some("replacement".into()), false);
        assert!(app.editor.is_some());
    }

    #[test]
    fn spill_ownership_follows_recalculation_and_obstructions() {
        let mut app = app();
        set(&mut app, "D1", "2");
        set(&mut app, "A1", "=SEQUENCE(D1,2)");
        check(&mut app, "B2", "=SEQUENCE(D1,2)", true);
        set(&mut app, "D1", "4");
        check(&mut app, "B4", "=SEQUENCE(D1,2)", true);
        set(&mut app, "D1", "1");
        check(&mut app, "B4", "", false);
        check(&mut app, "B1", "=SEQUENCE(D1,2)", true);
        set(&mut app, "A1", "");
        check(&mut app, "B1", "", false);
        set(&mut app, "B2", "blocked");
        set(&mut app, "A1", "=SEQUENCE(3,2)");
        check(&mut app, "A2", "", false);
        check(&mut app, "B2", "blocked", false);
        set(&mut app, "B2", "");
        check(&mut app, "B2", "=SEQUENCE(3,2)", true);
    }

    #[test]
    fn formatted_children_and_other_dynamic_array_functions() {
        let mut app = app();
        app.run("home.bold", json!({"range": "A2"})).unwrap();
        set(&mut app, "D1", "3");
        set(&mut app, "D2", "1");
        set(&mut app, "D3", "3");
        for formula in ["=SORT(D1:D3)", "=UNIQUE(D1:D3)", "=FILTER(D1:D3,D1:D3>0)"] {
            set(&mut app, "A1", formula);
            check(&mut app, "A2", formula, true);
        }
        app.run("home.insertSheet", json!({"name": "Other"})).unwrap();
        set(&mut app, "A1", "=SORT(Sheet1!D1:D3)");
        check(&mut app, "A2", "=SORT(Sheet1!D1:D3)", true);
        app.run("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
        check(&mut app, "A2", "=FILTER(D1:D3,D1:D3>0)", true);
    }
    #[test]
    /// A disabled spill formula paints disabled and does not edit; clicking an active formula starts formula-bar editing in both light and dark themes, with accessibility checks.
    fn formula_bar_click_and_theme_presentation() {
        use egui_kittest::kittest::Queryable as _;
        for dark in [false, true] {
            let mut app = app();
            set(&mut app, "A1", "=SEQUENCE(3)");
            select(&mut app, "A2");
            let mut fonts_ready = false;
            let mut h = egui_kittest::Harness::builder().with_size(vec2(900.0, 100.0)).build_ui_state(
                move |ui, app: &mut SheetApp| {
                    if !fonts_ready {
                        ui.ctx().set_fonts(theme::font_definitions());
                        fonts_ready = true;
                        return;
                    }
                    ui.ctx().set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                    show(app, ui);
                },
                app,
            );
            h.run_steps(3);
            // `run_steps` does not enforce the 0.37 strict policy; run it explicitly so the
            // formula bar's accessible names are actually tested.
            h.check_accessibility();
            let disabled = Tokens::get(&h.ctx).text_disabled;
            assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t)
                if t.galley.job.text == "=SEQUENCE(3)" && t.galley.job.sections.iter().all(|section| section.format.color == disabled))));
            h.get_by_label("=SEQUENCE(3)").click();
            h.run_steps(3);
            assert!(h.state().editor.is_none());
            select(h.state_mut(), "A1");
            h.run_steps(3);
            h.get_by_label("=SEQUENCE(3)").click();
            h.run_steps(3);
            h.check_accessibility();
            assert!(h.state().editor.as_ref().is_some_and(|e| e.from_formula_bar));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key) -> egui::RawInput {
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        raw
    }

    #[test]
    fn backspace_and_delete_dismiss_completion() {
        let ctx = egui::Context::default();
        for key in [Key::Backspace, Key::Delete] {
            let mut completion = Some("SomeText".to_string());
            let mut out = ctx.run_ui(press(key), |ui| ui.input_mut(|i| dismiss_completion(i, &mut completion)));
            out.textures_delta.clear(); // nothing consumes the atlas in a headless test
            assert_eq!(completion, None, "{key:?} must drop the proposal");
        }
        let mut completion = Some("SomeText".to_string());
        let mut out = ctx.run_ui(press(Key::A), |ui| ui.input_mut(|i| dismiss_completion(i, &mut completion)));
        out.textures_delta.clear();
        assert!(completion.is_some());
    }

    #[test]
    fn only_typing_proposes_completion() {
        assert!(!entry_shrank(4, "SomeT"));
        // Backspacing "Some" to "Som" must not bring "SomeText" back.
        assert!(entry_shrank(4, "Som"));
        assert!(!entry_shrank(4, "Same"));
    }

    #[test]
    fn one_line_fits_a_row_of_any_font() {
        // macOS and Linux UI fonts: rows of 15 to 16 px keep Excel's 30 px bar.
        assert_eq!(one_line(15.6), 30.0);
        assert_eq!(one_line(16.0), 30.0);
        // The Windows UI font's 17.28 px rows need more: the panel's and the field's margins take
        // 12 px, and rounding to the pixel grid up to another one, so 30 px held only four rows
        // of five (a tester's finding). Every counted line must fit in what is left.
        for row_h in [17.0_f32, 17.28, 18.0, 20.0] {
            for lines in 1..=MAX_LINES {
                let bar = one_line(row_h) + (lines - 1) as f32 * row_h;
                let inner = bar - 13.0;
                assert!((inner / row_h).floor() as usize >= lines, "{lines} lines of {row_h} px in a {bar} px bar");
            }
        }
    }
}

struct EditorSuggestion<'a> {
    app: &'a SheetApp,
    id: egui::Id,
    resp: &'a egui::Response,
    ed: &'a mut crate::editor::EditState,
    galley: &'a egui::Galley,
    tokens: &'a Tokens,
    mine: bool,
}

fn show_editor_suggestions(ui: &mut egui::Ui, suggestion: EditorSuggestion<'_>) {
    let EditorSuggestion { app, id, resp, ed, galley, tokens: t, mine } = suggestion;
    // Column AutoComplete: the rest of the suggested entry, shown selected after the caret.
    if let Some(full) = &ed.completion
        && let Some(rest) = crate::editor::completion_rest(full, &ed.text)
    {
        let pos = resp.rect.left_top() + egui::vec2(galley.rect.width(), 0.0);
        let g = ui.painter().layout_no_wrap(
            rest.to_string(),
            galley.job.sections.first().map(|s| s.format.font_id.clone()).unwrap_or_default(),
            t.on_accent,
        );
        let r = egui::Rect::from_min_size(pos, g.size());
        ui.painter().rect_filled(r, 0.0, t.accent);
        ui.painter().galley(pos, g, t.on_accent);
    }
    // Autocomplete list and argument hint under the editor. Decide visibility from the area the
    // editor was allotted, not from its response: egui 0.37 gives every text field a
    // `spacing.interact_size.y` floor, so a one-line cell editor's response can reach below a short
    // cell and touch the visible quadrant even while the cell itself is hidden under a frozen row.
    if mine && ui.max_rect().intersect(ui.clip_rect()).is_positive() && (!ed.autocomplete.is_empty() || ed.current_function().is_some()) {
        let below = resp.rect.left_bottom() + vec2(0.0, 4.0);
        let language = app.ui.language;
        // Areas have their own clip rect: keep cell helpers inside the worksheet.
        let bounds = if ed.from_formula_bar { ui.ctx().content_rect() } else { ui.clip_rect() };
        egui::Area::new(id.with("ac")).fixed_pos(below).constrain_to(bounds).order(egui::Order::Tooltip).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(4.0).show(ui, |ui| {
                if !ed.autocomplete.is_empty() {
                    let mut pick = None;
                    for (i, n) in ed.autocomplete.iter().enumerate() {
                        let sel = i == ed.ac_index;
                        let r = ui.add(
                            egui::Button::selectable(sel, egui::RichText::new(format!("ƒ  {n}")).font(theme::ui_font(12.5)))
                                .min_size(vec2(220.0, 20.0)),
                        );
                        if r.clicked() {
                            pick = Some(i);
                        }
                        if sel && let Some(desc) = crate::formula_locale::description(n, ed.locale, language) {
                            r.on_hover_text(desc);
                        }
                    }
                    if let Some(desc) = ed.autocomplete.get(ed.ac_index).and_then(|n| crate::formula_locale::description(n, ed.locale, language)) {
                        ui.separator();
                        ui.add(egui::Label::new(egui::RichText::new(desc).small()).wrap());
                    }
                    if let Some(i) = pick {
                        ed.ac_index = i;
                        ed.accept_autocomplete();
                        ed.request_focus = true;
                    }
                } else if let Some((name, arg)) = ed.current_function()
                    && let Some(sig) = crate::formula_locale::signature(&name, ed.locale, language)
                {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (head, rest) = sig.split_once('(').unwrap_or((&sig, ""));
                        ui.label(egui::RichText::new(format!("{head}(")).font(theme::ui_font(12.0)));
                        let inner = rest.trim_end_matches(')');
                        let parts: Vec<&str> = inner.split(&format!("{} ", ed.locale.list_separator())).collect();
                        for (i, part) in parts.iter().enumerate() {
                            let is_cur = i == arg.min(parts.len().saturating_sub(1)) || (part.contains("...") && arg >= i);
                            let txt = egui::RichText::new(*part).font(if is_cur { theme::ui_bold(12.0) } else { theme::ui_font(12.0) });
                            ui.label(txt);
                            if i + 1 < parts.len() {
                                ui.label(egui::RichText::new(format!("{} ", ed.locale.list_separator())).font(theme::ui_font(12.0)));
                            }
                        }
                        ui.label(egui::RichText::new(")").font(theme::ui_font(12.0)));
                    });
                    // The description too, so values an argument takes (DATEDIF's units, #363) show
                    // while it's typed; wrapped to the hint's width.
                    if let Some(desc) = crate::formula_locale::description(&name, ed.locale, language) {
                        ui.set_max_width(ui.min_rect().width().max(220.0));
                        ui.separator();
                        ui.add(egui::Label::new(egui::RichText::new(desc).small()).wrap());
                    }
                }
            });
        });
    }
}
