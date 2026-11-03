//! Typing into a locked cell of a protected sheet warns before an editor opens, and the warning
//! leaves Undo, Save and typing in unlocked cells working (#2945).

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const PROTECTED: &str = "The cell or chart you're trying to change is on a protected sheet.";

/// A protected sheet whose C2 is unlocked and holds "keep"; A1 is locked and active.
fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "C2", "input": "keep"})).unwrap();
    s.execute("home.formatCells", json!({"range": "C2", "style": {"protection": {"locked": false}}})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    crate::support::harness(s, 4)
}

fn key(h: &mut Harness, k: Key) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// Types text the way a keyboard does (into whatever has focus).
fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn value(h: &Harness, a1: &str) -> Value {
    let at = CellRef::parse(a1).unwrap();
    h.state().session.active().and_then(|d| d.wb.sheet(0)).map(|sh| sh.value(at)).unwrap_or_default()
}

#[test]
fn typing_in_a_locked_cell_warns_without_a_stuck_draft() {
    let mut h = harness();
    let undo = h.state().session.active().unwrap().undo.len();

    type_text(&mut h, "locked");

    assert!(h.state().editor.is_none(), "no editor opened on the locked cell");
    assert_eq!(h.state().message.as_ref().map(|(_, m)| m.as_str()), Some(PROTECTED));
    assert_eq!(h.state().session.active().unwrap().undo.len(), undo, "the refused edit added no undo step");
    assert_eq!(value(&h, "A1"), Value::Empty);
    // No draft exists, so Save is not blocked while the warning is up.
    assert!(h.state_mut().run("file.saveBytes", json!({"format": "xlsx"})).is_ok());

    // Enter dismisses the warning and the keyboard goes back to the grid.
    key(&mut h, Key::Enter);
    assert!(h.state().message.is_none(), "Enter dismisses the warning");

    // An unlocked cell on the same sheet still edits, undoes and saves.
    h.state_mut().run("selection.set", json!({"cell": "C2"})).unwrap();
    h.run_steps(1);
    type_text(&mut h, "typed");
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("typed"));
    key(&mut h, Key::Enter);
    assert_eq!(value(&h, "C2"), Value::text("typed"));
    assert!(h.state_mut().run("edit.undo", json!({})).is_ok());
    assert_eq!(value(&h, "C2"), Value::text("keep"), "Undo of the unlocked cell's edit still works");
    assert!(h.state_mut().run("file.saveBytes", json!({"format": "xlsx"})).is_ok());
}
