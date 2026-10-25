//! The auto-growing formula bar, the 5-line in-cell editor and the code editor widget.

use egui::{Event, Key, Modifiers, Rect, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::formula_bar::{BarLayout, MAX_LINES, one_line};
use gridcraft_ui_egui::widgets::{Lang, code_editor};
use gridcraft_ui_egui::{SheetApp, theme};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    settle(&mut h);
    h
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

/// Long enough for the 120 ms height ease to finish.
fn settle(h: &mut Harness) {
    h.run_steps(16);
}

fn key(key: Key, modifiers: Modifiers) -> [Event; 2] {
    [
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn alt_enter(h: &mut Harness) {
    h.input_mut().events.extend(key(Key::Enter, Modifiers::ALT));
    h.run_steps(2);
}

fn layout(h: &Harness) -> BarLayout {
    BarLayout::get(&h.ctx)
}

fn row_height(h: &Harness) -> f32 {
    h.ctx.fonts_mut(|f| f.row_height(&theme::ui_font(13.0)))
}

fn same(a: Rect, b: Rect) -> bool {
    (a.min - b.min).length() < 0.5 && (a.max - b.max).length() < 0.5
}

#[test]
fn bar_grows_to_five_lines_then_scrolls_with_name_box_pinned() {
    let mut h = harness(blank());
    // The one-line height is 30 px unless the font's rows need more (Windows' do).
    let rh = row_height(&h);
    let base = one_line(rh);
    let one = layout(&h);
    assert!((one.height - base).abs() < 0.5, "one line is {base} px, got {}", one.height);
    assert_eq!(one.lines, 1);
    h.state_mut().begin_edit(None, true);
    h.run_steps(2);
    type_text(&mut h, "line 1");
    for n in 2..=8 {
        alt_enter(&mut h);
        type_text(&mut h, &format!("line {n}"));
        settle(&mut h);
        let l = layout(&h);
        assert_eq!(l.lines, n, "the editor holds {n} lines");
        let expect = base + (n.min(MAX_LINES) - 1) as f32 * rh;
        assert!((l.height - expect).abs() < 0.5, "{n} lines: height {} expected {expect}", l.height);
        assert!(same(l.name_box, one.name_box), "the Name Box stays on the first line ({n} lines)");
        assert!(same(l.buttons, one.buttons), "the buttons stay on the first line ({n} lines)");
        let caret = l.caret.expect("a caret while editing");
        assert!(l.field.contains_rect(caret.shrink(0.5)), "the caret stays visible at {n} lines: {caret:?} in {:?}", l.field);
    }
    assert_eq!(layout(&h).visible_lines, MAX_LINES);
    // Enter commits the multi-line text; viewing the cell keeps the bar at five lines.
    h.input_mut().events.extend(key(Key::Enter, Modifiers::NONE));
    settle(&mut h);
    let text = (1..=8).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
    let d = h.state().session.active().unwrap();
    assert_eq!(d.wb.active().unwrap().value(CellRef::parse("A1").unwrap()), Value::from(text.as_str()));
    h.state_mut().session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    settle(&mut h);
    let viewing = layout(&h);
    assert_eq!(viewing.lines, 8);
    assert!((viewing.height - (base + 4.0 * rh)).abs() < 0.5);
    // An empty cell shrinks it back to one line.
    h.state_mut().session.execute("selection.set", json!({"cell": "B1"})).unwrap();
    settle(&mut h);
    assert!((layout(&h).height - base).abs() < 0.5);
}

#[test]
fn enter_that_closes_a_message_box_adds_no_line_to_the_entry() {
    // A message box over the editor (a data validation alert) takes Enter to close. The bar's
    // field is multi-line, and a field of its own would add a line on that Enter; the editor
    // handles Enter itself, so the field does nothing with it.
    let mut h = harness(blank());
    h.state_mut().begin_edit(None, true);
    h.run_steps(2);
    type_text(&mut h, "abc");
    h.state_mut().message = Some(("Stop".into(), "Whole numbers only.".into()));
    h.run_steps(2);
    h.input_mut().events.extend(key(Key::Enter, Modifiers::NONE));
    h.run_steps(2);
    assert!(h.state().message.is_none(), "Enter closed the message box");
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("abc"), "and added nothing to the entry");
    assert_eq!(layout(&h).lines, 1);
}

#[test]
fn chevron_pins_the_bar_at_five_lines() {
    let mut h = harness(blank());
    h.state_mut().ui.formula_bar_expanded = true;
    settle(&mut h);
    let rh = row_height(&h);
    let base = one_line(rh);
    assert!((layout(&h).height - (base + 4.0 * rh)).abs() < 0.5);
    h.state_mut().ui.formula_bar_expanded = false;
    settle(&mut h);
    assert!((layout(&h).height - base).abs() < 0.5);
}

#[test]
fn height_eases_instead_of_jumping() {
    let mut h = harness(blank());
    h.state_mut().ui.formula_bar_expanded = true;
    h.run_steps(2);
    let mid = layout(&h).height;
    let rh = row_height(&h);
    let base = one_line(rh);
    assert!(mid > base + 0.5 && mid < base + 4.0 * rh - 0.5, "mid-ease height {mid}");
    settle(&mut h);
    assert!((layout(&h).height - (base + 4.0 * rh)).abs() < 0.5);
}

fn in_cell_height(lines: usize) -> f32 {
    let mut s = blank();
    s.execute("selection.set", json!({"cell": "B3"})).unwrap();
    let mut h = harness(s);
    let text = (1..=lines).map(|n| format!("row {n}")).collect::<Vec<_>>().join("\n");
    h.state_mut().begin_edit(Some(text.clone()), false);
    settle(&mut h);
    assert_eq!(h.state().editor.as_ref().unwrap().text, text, "the text is kept whole");
    let rect: Rect = h.ctx.data(|d| d.get_temp(egui::Id::new("gridcraft.cell_editor.rect"))).expect("the in-cell editor painted");
    rect.height()
}

#[test]
fn in_cell_editor_grows_with_every_line() {
    // The formula bar stops at five lines and scrolls; the in-cell editor shows every line, as
    // the grid's own tests ask (`an_unwrapped_multiline_edit_shows_every_line`).
    let (four, five, nine) = (in_cell_height(4), in_cell_height(5), in_cell_height(9));
    assert!(five > four + 1.0, "grows with the lines ({four} then {five})");
    assert!(nine > five + 1.0, "and past five ({five} then {nine})");
}

#[test]
fn code_editor_indents_and_keeps_indentation() {
    struct State {
        text: String,
        focused: bool,
    }
    let mut h = egui_kittest::Harness::builder().with_size(vec2(400.0, 300.0)).build_ui_state(
        |ui, s: &mut State| {
            if ui.ctx().cumulative_pass_nr() == 0 {
                // egui's own fonts stand in for the system ones under the theme's family names.
                let mut fonts = egui::FontDefinitions::default();
                let mono = fonts.families.get(&egui::FontFamily::Monospace).cloned().unwrap_or_default();
                fonts.families.insert(egui::FontFamily::Name(theme::MONO.into()), mono);
                ui.ctx().set_fonts(fonts);
                return;
            }
            let id = egui::Id::new("code");
            if !s.focused {
                ui.memory_mut(|m| m.request_focus(id));
                s.focused = true;
            }
            code_editor(ui, id, &mut s.text, Lang::Python, 5);
        },
        State { text: String::new(), focused: false },
    );
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("if x:".into()));
    h.run_steps(1);
    h.input_mut().events.extend(key(Key::Enter, Modifiers::NONE));
    h.run_steps(1);
    h.input_mut().events.push(Event::Text("y = 1".into()));
    h.run_steps(1);
    h.input_mut().events.extend(key(Key::Enter, Modifiers::NONE));
    h.run_steps(1);
    h.input_mut().events.extend(key(Key::Tab, Modifiers::NONE));
    h.run_steps(1);
    h.input_mut().events.push(Event::Text("z".into()));
    h.run_steps(1);
    assert_eq!(h.state().text, "if x:\n    y = 1\n        z");
}
