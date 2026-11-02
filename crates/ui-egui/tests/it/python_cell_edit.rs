//! Editing a Python cell's content (F2, the formula bar) edits its code, in Python mode; typing
//! over it replaces it, as typing over a formula does.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("python.set", json!({"cell": "A1", "code": "1 + 1", "output": "object"})).unwrap();
    session.execute("selection.set", json!({"range": "A1"})).unwrap();
    crate::common::harness_with(crate::common::builder().with_size(egui::vec2(1200.0, 800.0)), crate::common::app(session), 4)
}

fn text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn key_with(h: &mut Harness, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
    }
    h.run_steps(2);
}

fn key(h: &mut Harness, key: Key) {
    key_with(h, key, Modifiers::NONE);
}

/// Ctrl+Enter (Cmd+Enter on macOS): commits Python mode, where Enter starts a new line.
fn commit(h: &mut Harness) {
    key_with(h, Key::Enter, Modifiers::COMMAND);
}

/// A1's code and return type (true: a Python object), or `None` when it isn't a Python cell.
fn python(h: &Harness) -> Option<(String, bool)> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    sh.cell(CellRef::parse("A1").unwrap()).and_then(|c| c.py.as_deref()).map(|p| (p.code.clone(), p.object))
}

fn select_a1(h: &mut Harness) {
    h.state_mut().session.execute("selection.set", json!({"range": "A1"})).unwrap();
    h.run_steps(2);
}

#[test]
fn f2_edits_a_python_cells_code_and_typing_replaces_it() {
    let mut h = harness();
    let undo_depth = |h: &Harness| h.state().session.active().unwrap().undo.len();
    let before = undo_depth(&h);

    // F2 and a commit without a change writes nothing.
    key(&mut h, Key::F2);
    assert_eq!(h.state().editor.as_ref().map(|e| (e.text.as_str(), e.python)), Some(("1 + 1", true)));
    commit(&mut h);
    assert!(h.state().editor.is_none());
    assert_eq!(undo_depth(&h), before, "an unchanged edit is no undo step");
    assert_eq!(python(&h), Some(("1 + 1".into(), true)));

    // A change is new code for the same Python cell, which keeps its return type.
    select_a1(&mut h);
    key(&mut h, Key::F2);
    text(&mut h, " + 2");
    commit(&mut h);
    assert!(h.state().editor.is_none(), "{:?}", h.state().message);
    assert_eq!(python(&h), Some(("1 + 1 + 2".into(), true)), "the cell stays a Python cell with the new code");

    // Typing over the cell replaces it.
    select_a1(&mut h);
    text(&mut h, "5");
    key(&mut h, Key::Enter);
    assert_eq!(python(&h), None);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("A1").unwrap()), Value::Number(5.0));
}

#[test]
fn deleting_all_the_code_clears_the_cell() {
    let mut h = harness();
    let a1 = |h: &Harness| h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("A1").unwrap());
    // Code of spaces is still code, for the same Python cell.
    key(&mut h, Key::F2);
    h.state_mut().editor.as_mut().unwrap().text = " ".into();
    assert!(h.state_mut().commit_edit(0, 0, false, false));
    assert_eq!(python(&h), Some((" ".into(), true)));
    // No code at all clears the cell, as an empty entry clears any other cell.
    select_a1(&mut h);
    key(&mut h, Key::F2);
    h.state_mut().editor.as_mut().unwrap().text.clear();
    assert!(h.state_mut().commit_edit(0, 0, false, false));
    assert!(h.state().editor.is_none());
    assert_eq!(python(&h), None, "the cell is no longer a Python cell");
    assert_eq!(a1(&h), Value::Empty);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(python(&h), Some((" ".into(), true)), "one undo step brings the code back");
}

#[test]
fn deleting_all_the_code_clears_a_python_cell_with_no_cached_value() {
    let mut h = harness();
    // A Python cell from a file that cached no value (the value is empty, the code is not).
    {
        let d = h.state_mut().session.doc_mut().unwrap();
        let mut wb = (*d.wb).clone();
        let i = wb.active_sheet();
        let sh = wb.sheet_mut(i).unwrap();
        let at = CellRef::parse("A1").unwrap();
        let mut cell = sh.cell(at).cloned().unwrap();
        cell.value = Value::Empty;
        sh.set_cell(at, cell);
        d.wb = std::sync::Arc::new(wb);
    }
    h.run_steps(2);
    key(&mut h, Key::F2);
    h.state_mut().editor.as_mut().unwrap().text.clear();
    assert!(h.state_mut().commit_edit(0, 0, false, false));
    assert!(h.state().editor.is_none());
    assert_eq!(python(&h), None, "the code is removed, not kept as an unchanged empty cell");
}

#[test]
fn a_code_edit_is_never_completed_from_the_column() {
    let mut h = harness();
    // A2 holds text that starts with A1's code: typing at the end of the code would offer it.
    h.state_mut().session.execute("cell.set", json!({"cell": "A2", "input": "1 + 1 and more"})).unwrap();
    select_a1(&mut h);
    key(&mut h, Key::F2);
    text(&mut h, " ");
    assert_eq!(h.state().editor.as_ref().and_then(|e| e.completion.clone()), None, "no completion while code is edited");
    commit(&mut h);
    assert_eq!(python(&h), Some(("1 + 1 ".into(), true)), "the typed code is kept");
}
