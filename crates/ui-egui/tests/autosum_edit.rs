//! AutoSum from the ribbon or keyboard on one cell proposes its formula in the editor, with the
//! range ready to be replaced by pointing or typing, as in Excel (#1834).

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::{Mode, Session};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// 1, 2, 3 in A1:A3 and `selected` selected.
fn harness(selected: &str) -> Harness {
    harness_at("A1", selected)
}

/// 1, 2, 3 down from `first` and `selected` selected.
fn harness_at(first: &str, selected: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": first, "values": [[1], [2], [3]]})).unwrap();
    session.execute("selection.set", json!({"range": selected})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// The AutoSum button: a menu-style run.
fn auto_sum(h: &mut Harness) {
    h.state_mut().run_or_alert("formulas.autoSum", json!({}));
    h.run_steps(2);
}

fn editing(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

fn input(h: &Harness, cell: &str) -> String {
    h.state().session.active().unwrap().wb.active().unwrap().input_text(CellRef::parse(cell).unwrap())
}

fn cell_center(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center()
}

fn pointer(h: &mut Harness, cell: &str, pressed: bool) {
    let pos = cell_center(h, cell);
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    h.run_steps(2);
}

fn enter(h: &mut Harness) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
}

#[test]
fn autosum_on_one_cell_proposes_its_range_in_the_editor() {
    let mut h = harness("A4");
    auto_sum(&mut h);
    assert_eq!(editing(&h).as_deref(), Some("=SUM(A1:A3)"));
    assert_eq!(h.state().session.mode, Mode::Point);
    assert_eq!(input(&h, "A4"), "", "nothing is entered before Enter");
    enter(&mut h);
    assert_eq!(input(&h, "A4"), "=SUM(A1:A3)");
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("A4").unwrap()), Value::Number(6.0));

    // A `range` that isn't text is no range, for the engine and here alike.
    let mut h = harness("A4");
    h.state_mut().run_or_alert("formulas.autoSum", json!({"range": null}));
    h.run_steps(2);
    assert_eq!(editing(&h).as_deref(), Some("=SUM(A1:A3)"));
}

#[test]
fn the_autosum_menu_functions_propose_theirs_the_same_way() {
    // Home › AutoSum ▾ Average (and Formulas › AutoSum ▾ once it has the menu, #658).
    let mut h = harness("A4");
    h.state_mut().run_or_alert("formulas.autoSum", json!({"function": "AVERAGE"}));
    h.run_steps(2);
    assert_eq!(editing(&h).as_deref(), Some("=AVERAGE(A1:A3)"));
    pointer(&mut h, "A2", true);
    pointer(&mut h, "A2", false);
    assert_eq!(editing(&h).as_deref(), Some("=AVERAGE(A2)"));
}

#[test]
fn a_click_or_drag_replaces_the_proposed_range() {
    let mut h = harness("A4");
    auto_sum(&mut h);
    pointer(&mut h, "A2", true);
    pointer(&mut h, "A2", false);
    assert_eq!(editing(&h).as_deref(), Some("=SUM(A2)"));

    let mut h = harness("A4");
    auto_sum(&mut h);
    pointer(&mut h, "A1", true);
    let to = cell_center(&h, "A2");
    for pos in [to, to + egui::vec2(1.0, 0.0)] {
        h.input_mut().events.push(Event::PointerMoved(pos));
        h.step();
    }
    pointer(&mut h, "A2", false);
    assert_eq!(editing(&h).as_deref(), Some("=SUM(A1:A2)"));
    enter(&mut h);
    assert_eq!(input(&h, "A4"), "=SUM(A1:A2)");
}

#[test]
fn an_arrow_key_points_from_the_proposed_range() {
    // Observed in Excel for the web: ↑ after AutoSum moves from the range's first cell.
    let mut h = harness_at("A2", "A5");
    auto_sum(&mut h);
    assert_eq!(editing(&h).as_deref(), Some("=SUM(A2:A4)"));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key: Key::ArrowUp, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
    assert_eq!(editing(&h).as_deref(), Some("=SUM(A1)"));
}

#[test]
fn typing_replaces_the_proposed_range() {
    let mut h = harness("A4");
    auto_sum(&mut h);
    h.input_mut().events.push(Event::Text("A2:A3".into()));
    h.run_steps(2);
    assert_eq!(editing(&h).as_deref(), Some("=SUM(A2:A3)"));
}

#[test]
fn with_nothing_to_sum_the_caret_waits_between_the_parentheses() {
    let mut h = harness("C10");
    auto_sum(&mut h);
    assert_eq!(editing(&h).as_deref(), Some("=SUM()"));
    pointer(&mut h, "A1", true);
    pointer(&mut h, "A1", false);
    assert_eq!(editing(&h).as_deref(), Some("=SUM(A1)"));
}

#[test]
fn a_block_and_programmatic_calls_enter_the_totals_at_once() {
    let mut h = harness("A1:A4");
    auto_sum(&mut h);
    assert!(h.state().editor.is_none());
    assert_eq!(input(&h, "A4"), "=SUM(A1:A3)");

    // The control channel, CLI and MCP run commands with `run`: no editor.
    let mut h = harness("A4");
    h.state_mut().run("formulas.autoSum", json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().editor.is_none());
    assert_eq!(input(&h, "A4"), "=SUM(A1:A3)");
}
