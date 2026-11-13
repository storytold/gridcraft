//! New Formatting Rule ▸ formula: clicking or dragging on the grid writes the absolute reference
//! into the formula box at its caret, as in Excel, and leaves the selection the rule applies to alone.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.insertSheet", json!({})).unwrap();
    session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    session.execute("selection.set", json!({"range": "F8:G9"})).unwrap();
    let mut h = crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h.state_mut().open_dialog("cfQuick", json!({"type": "expression", "title": "New Formatting Rule"}));
    h.run_steps(6);
    h
}

fn formula(h: &Harness) -> String {
    h.state().dialog.as_ref().and_then(|d| d.values.get("formula")).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn selection(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.current().a1()
}

fn type_text(h: &mut Harness, text: &str) {
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(1);
    }
}

fn cell_center(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let c = gridcraft_engine::core::CellRef::parse(a1).unwrap();
    pos2(geo.x(sheet, c.col) + sheet.col_width(c.col) * geo.z * 0.5, geo.y(sheet, c.row) + sheet.row_height(c.row) * geo.z * 0.5)
}

fn click(h: &mut Harness, a1: &str) {
    let pos = cell_center(h, a1);
    for pressed in [true, false] {
        h.input_mut()
            .events
            .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        h.step();
    }
    h.run_steps(3);
}

fn drag(h: &mut Harness, from: &str, to: &str) {
    let (a, b) = (cell_center(h, from), cell_center(h, to));
    h.input_mut()
        .events
        .extend([Event::PointerMoved(a), Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    h.step();
    for i in 1..=6 {
        let p = a + (b - a) * (i as f32 / 6.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
    }
    h.input_mut().events.push(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
    h.run_steps(3);
}

#[test]
fn clicking_a_cell_after_equals_inserts_its_absolute_reference() {
    let mut h = harness();
    type_text(&mut h, "=");
    assert_eq!(formula(&h), "=", "the formula box had the keyboard");
    click(&mut h, "B2");
    assert_eq!(formula(&h), "=$B$2");
    assert_eq!(selection(&h), "F8:G9", "the range the rule applies to is still selected");
}

#[test]
fn clicking_a_cell_in_an_empty_box_starts_the_formula() {
    let mut h = harness();
    click(&mut h, "C3");
    assert_eq!(formula(&h), "=$C$3");
    assert_eq!(selection(&h), "F8:G9");
}

#[test]
fn a_second_click_replaces_the_reference_just_inserted() {
    let mut h = harness();
    type_text(&mut h, "=");
    click(&mut h, "B2");
    click(&mut h, "D5");
    assert_eq!(formula(&h), "=$D$5");
}

#[test]
fn dragging_inserts_a_range() {
    let mut h = harness();
    type_text(&mut h, "=");
    drag(&mut h, "B2", "C4");
    assert_eq!(formula(&h), "=$B$2:$C$4");
    assert_eq!(selection(&h), "F8:G9");
}

#[test]
fn the_reference_goes_at_the_caret_and_typing_continues_after_it() {
    let mut h = harness();
    type_text(&mut h, "=SUM()");
    h.key_press(Key::ArrowLeft);
    h.run_steps(2);
    click(&mut h, "B2");
    assert_eq!(formula(&h), "=SUM($B$2)");
    type_text(&mut h, "+1");
    assert_eq!(formula(&h), "=SUM($B$2+1)", "the box has the keyboard back, its caret after the reference");
}

#[test]
fn a_click_after_an_operand_changes_nothing() {
    let mut h = harness();
    type_text(&mut h, "=A1>5");
    click(&mut h, "B2");
    assert_eq!(formula(&h), "=A1>5");
    assert_eq!(selection(&h), "F8:G9");
}

#[test]
fn ok_applies_the_picked_formula_to_the_selection() {
    let mut h = harness();
    type_text(&mut h, "=");
    click(&mut h, "B2");
    type_text(&mut h, ">0");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert!(h.state().dialog.is_none(), "Enter confirmed the rule");
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    let cf = sheet.cond_formats.last().expect("a rule was added");
    assert_eq!(cf.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>(), ["F8:G9"]);
    assert!(format!("{:?}", cf.rule).contains("$B$2>0"), "rule: {:?}", cf.rule);
}

#[test]
fn other_dialogs_leave_grid_clicks_alone() {
    let mut h = harness();
    h.state_mut().dialog = None;
    h.state_mut().open_dialog("cfQuick", json!({"type": "containsText", "title": "Text that Contains"}));
    h.run_steps(6);
    click(&mut h, "B2");
    assert_eq!(selection(&h), "B2", "a box that is not a formula does not take references");
}

#[test]
fn pointing_on_another_sheet_keeps_the_rule_on_the_sheet_it_was_opened_for() {
    let mut h = harness();
    type_text(&mut h, "=");
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    h.run_steps(3);
    click(&mut h, "A1");
    assert_eq!(formula(&h), "=Sheet2!$A$1");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert!(h.state().dialog.is_none(), "Enter confirmed the rule");
    let wb = &h.state().session.active().unwrap().wb;
    assert!(wb.sheet(1).unwrap().cond_formats.is_empty(), "nothing lands on the sheet pointed at");
    let cf = wb.sheet(0).unwrap().cond_formats.last().expect("the rule is on the sheet the dialog was opened on");
    assert_eq!(cf.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>(), ["F8:G9"]);
}

#[test]
fn the_rule_follows_its_sheet_when_sheets_move_while_the_dialog_is_open() {
    let mut h = harness();
    type_text(&mut h, "=");
    click(&mut h, "B2");
    h.state_mut().session.execute("sheet.move", json!({"sheet": 0, "to": 1})).unwrap();
    h.run_steps(3);
    let names = |h: &Harness| h.state().session.active().unwrap().wb.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&h), ["Sheet2", "Sheet1"], "Sheet1 now sits at index 1");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert!(h.state().dialog.is_none(), "Enter confirmed the rule");
    let wb = &h.state().session.active().unwrap().wb;
    let sheet1 = wb.sheets.iter().find(|s| s.name == "Sheet1").unwrap();
    let sheet2 = wb.sheets.iter().find(|s| s.name == "Sheet2").unwrap();
    assert!(sheet2.cond_formats.is_empty(), "nothing lands on the sheet that took index 0");
    let cf = sheet1.cond_formats.last().expect("the rule stays on Sheet1");
    assert_eq!(cf.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>(), ["F8:G9"]);
}
