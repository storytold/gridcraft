//! Ctrl+[ / Ctrl+] select precedent and dependent cells (#1865).

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    crate::support::harness(session, 4)
}

fn active(h: &Harness) -> String {
    let d = h.state().session.active().unwrap();
    d.selection.active.a1()
}

fn ranges(h: &Harness) -> Vec<String> {
    let d = h.state().session.active().unwrap();
    d.selection.ranges.iter().map(|r| r.a1()).collect()
}

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

/// A1 and B1 feed C1; C1 feeds D1.
fn chain() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2, 3]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=A1+B1"})).unwrap();
    s.execute("cell.set", json!({"cell": "D1", "input": "=C1*2"})).unwrap();
    s
}

#[test]
fn ctrl_bracket_selects_precedents_and_dependents() {
    let mut h = harness(chain());
    h.state_mut().session.execute("selection.set", json!({"range": "C1"})).unwrap();
    key(&mut h, Key::OpenBracket, Modifiers::CTRL);
    let mut r = ranges(&h);
    r.sort();
    assert_eq!(r, vec!["A1".to_string(), "B1".to_string()], "Ctrl+[ selects the precedent cells");
    h.state_mut().session.execute("selection.set", json!({"range": "C1"})).unwrap();
    key(&mut h, Key::CloseBracket, Modifiers::CTRL);
    assert_eq!(active(&h), "D1", "Ctrl+] lands on the dependent cell");
}

#[test]
fn ctrl_shift_bracket_walks_every_level() {
    let mut h = harness(chain());
    h.state_mut().session.execute("selection.set", json!({"range": "D1"})).unwrap();
    key(&mut h, Key::OpenBracket, Modifiers::CTRL | Modifiers::SHIFT);
    let r = ranges(&h);
    assert!(
        r.contains(&"A1".to_string()) && r.contains(&"B1".to_string()) && r.contains(&"C1".to_string()),
        "Ctrl+Shift+[ walks back through C1 to A1:B1, got {r:?}"
    );
}

#[test]
fn transitive_walk_reaches_formulas_late_in_a_wide_area() {
    // =SUM(A1:Z1) has a wide precedent area; a formula stored late in it must still be
    // walked, and blank positions must not be selected (#1865 review).
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "Z1", "input": "5"})).unwrap();
    s.execute("cell.set", json!({"cell": "AB1", "input": "=SUM(A1:Z1)"})).unwrap();
    s.execute("cell.set", json!({"cell": "AD1", "input": "=AB1*2"})).unwrap();
    let mut h = harness(s);
    h.state_mut().session.execute("selection.set", json!({"range": "AD1"})).unwrap();
    key(&mut h, Key::OpenBracket, Modifiers::CTRL | Modifiers::SHIFT);
    let r = ranges(&h);
    assert!(r.contains(&"Z1".to_string()), "the formula late in the area is reached, got {r:?}");
    assert_eq!(r.len(), 2, "only the stored cells (AB1, Z1) are selected, no blanks, got {r:?}");
}

#[test]
fn no_precedents_leaves_selection_untouched() {
    let mut h = harness(chain());
    h.state_mut().session.execute("selection.set", json!({"range": "A1"})).unwrap();
    key(&mut h, Key::OpenBracket, Modifiers::CTRL);
    assert_eq!(active(&h), "A1", "a plain entry cell has no precedents to select");
}

#[test]
fn cross_sheet_precedents_jump_to_the_other_sheet() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.insertSheet", json!({"name": "Src"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "Src!B2", "input": "42"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=Src!B2*2"})).unwrap();
    // Back on the first sheet, Ctrl+[ from A1 jumps to Src!B2.
    let d = s.active().unwrap();
    let first = d.wb.active_sheet();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    let mut h = harness(s);
    key(&mut h, Key::OpenBracket, Modifiers::CTRL);
    let d = h.state().session.active().unwrap();
    assert_ne!(d.wb.active_sheet(), first, "selection jumped to the precedent's sheet");
    assert_eq!(d.selection.active, CellRef { row: 1, col: 1 }, "B2 on the Src sheet");
}
