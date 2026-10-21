//! Arabic word navigation must not panic or delete unrelated cell text.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Shape, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::{
    Session,
    core::{CellRef, Value},
};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn editing(text: &str, formula_bar: bool) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    if formula_bar {
        h.get_by_label(text).click();
    } else {
        h.key_press(Key::F2);
    }
    h.run_steps(2);
    assert_eq!(h.state().editor.as_ref().map(|e| e.from_formula_bar), Some(formula_bar));
    h.key_press(Key::End);
    h.run_steps(2);
    h
}

/// The in-cell editor's laid-out text and its screen position: the lowest painted copy, below the formula bar.
fn cell_editor_galley(h: &egui_kittest::Harness<'static, SheetApp>, text: &str) -> Option<(Pos2, std::sync::Arc<egui::Galley>)> {
    fn collect(shape: &Shape, text: &str, out: &mut Vec<(Pos2, std::sync::Arc<egui::Galley>)>) {
        match shape {
            Shape::Text(s) if s.galley.text() == text => out.push((s.pos, s.galley.clone())),
            Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text, out)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &h.output().shapes {
        collect(&clipped.shape, text, &mut found);
    }
    found.into_iter().max_by(|a, b| a.0.y.total_cmp(&b.0.y))
}

fn cell_editor_text_right_edge(h: &egui_kittest::Harness<'static, SheetApp>, text: &str) -> Option<Pos2> {
    cell_editor_galley(h, text).map(|(pos, galley)| pos + vec2(galley.rect.max.x - 1.0, galley.rect.center().y))
}

#[test]
fn caret_after_typing_arabic_is_drawn_at_its_left_end() {
    let mut h = editing("", false);
    h.input_mut().events.push(Event::Text("بسم الله".into()));
    h.run_steps(2);
    let state = egui::text_edit::TextEditState::load(&h.ctx, egui::Id::new("gridcraft.cell_editor")).unwrap();
    let caret = state.cursor.char_range().unwrap().primary;
    assert_eq!(caret.index.0, "بسم الله".chars().count());
    let (_, galley) = cell_editor_galley(&h, "بسم الله").unwrap();
    let x = galley.pos_from_cursor(caret).min.x;
    assert!(x <= galley.rect.min.x + 1.0, "caret at x={x}, text spans {:?}", galley.rect.x_range());
}

#[test]
fn previous_word_in_arabic_cell_does_not_panic() {
    let mut h = editing("Aurora بب", false);
    h.key_press_modifiers(Modifiers::CTRL, Key::ArrowLeft);
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Text("x".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    // End is the logical end at the Arabic run's left edge. Ctrl+Left follows visual
    // word order across the Latin prefix, preserving all original characters.
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::text("xAurora بب"));
}

#[test]
fn delete_previous_arabic_word_keeps_the_latin_prefix() {
    let mut h = editing("Aurora بب", false);
    h.key_press_modifiers(Modifiers::CTRL, Key::Backspace);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::text("Aurora "));
}

#[test]
fn delete_previous_arabic_word_in_formula_bar_keeps_the_prefix() {
    let mut h = editing("Aurora بب", true);
    h.key_press_modifiers(Modifiers::CTRL, Key::Backspace);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::text("Aurora "));
}

#[test]
fn previous_visual_word_crosses_arabic_from_its_right_edge() {
    let mut h = editing("Aurora بب", false);
    let pos = cell_editor_text_right_edge(&h, "Aurora بب").unwrap();
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
    let state = egui::text_edit::TextEditState::load(&h.ctx, egui::Id::new("gridcraft.cell_editor")).unwrap();
    let caret = state.cursor.char_range().map(|r| (r.primary.index.0, r.primary.prefer_next_row));
    assert_eq!(caret, Some((7, true)), "the click lands on the Arabic run's right edge");
    h.key_press_modifiers(Modifiers::CTRL, Key::ArrowLeft);
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Text("x".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::text("Aurora ببx"));
}
