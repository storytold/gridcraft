//! Point mode on the headers: while a formula waits for a reference, a header click inserts a
//! whole-column or whole-row reference, driven through real egui pointer events.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"range": "F8"})).unwrap();
    let mut h = crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h.state_mut().begin_edit(Some("=SUM(".into()), false);
    h.run_steps(2);
    h
}

fn header_center(h: &egui_kittest::Harness<'_, SheetApp>, rows: bool, index: u32) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    if rows {
        pos2(geo.rect.left() + geo.header_w * 0.5, geo.y(sheet, index) + sheet.row_height(index) * geo.z * 0.5)
    } else {
        pos2(geo.x(sheet, index) + sheet.col_width(index) * geo.z * 0.5, geo.rect.top() + geo.header_h * 0.5)
    }
}

fn click(h: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2) {
    click_with_modifiers(h, pos, Modifiers::NONE);
}

fn click_with_modifiers(h: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        h.input_mut().events.extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers }]);
        h.step();
    }
    h.run_steps(2);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
}

#[test]
fn column_header_click_inserts_a_column_reference() {
    let mut h = harness();
    let at = header_center(&h, false, 2);
    click(&mut h, at);
    let app = h.state();
    assert_eq!(app.editor.as_ref().map(|e| e.text.as_str()), Some("=SUM(C:C"));
    assert_eq!(app.session.active().unwrap().selection.current().a1(), "F8");
    let text = app.editor.as_ref().map_or("", |ed| ed.text.as_str());
    assert_eq!(gridcraft_ui_egui::editor::formula_refs(text).first().map(|r| r.2.a1()).as_deref(), Some("C:C"));
}

#[test]
fn row_header_click_inserts_a_row_reference() {
    let mut h = harness();
    let at = header_center(&h, true, 3);
    click(&mut h, at);
    let app = h.state();
    assert_eq!(app.editor.as_ref().map(|e| e.text.as_str()), Some("=SUM(4:4"));
    assert_eq!(app.session.active().unwrap().selection.current().a1(), "F8");
    let text = app.editor.as_ref().map_or("", |ed| ed.text.as_str());
    assert_eq!(gridcraft_ui_egui::editor::formula_refs(text).first().map(|r| r.2.a1()).as_deref(), Some("4:4"));
}

#[test]
fn a_second_header_click_replaces_the_pointed_reference() {
    let mut h = harness();
    let first = header_center(&h, false, 2);
    click(&mut h, first);
    let second = header_center(&h, false, 4);
    click(&mut h, second);
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("=SUM(E:E"));
}

#[test]
fn shift_selects_a_contiguous_column_range_while_editing_a_formula() {
    let mut h = harness();
    let first = header_center(&h, false, 2);
    click(&mut h, first);
    let second = header_center(&h, false, 4);
    click_with_modifiers(&mut h, second, Modifiers::SHIFT);
    let app = h.state();
    let text = app.editor.as_ref().map_or("", |ed| ed.text.as_str());
    assert_eq!(text, "=SUM(C:E");
    assert_eq!(gridcraft_ui_egui::editor::formula_refs(text).iter().map(|r| r.2.a1()).collect::<Vec<_>>(), ["C:E"]);
}

#[test]
fn command_selects_discontiguous_columns_while_editing_a_formula() {
    let mut h = harness();
    let first = header_center(&h, false, 2);
    click(&mut h, first);
    let second = header_center(&h, false, 4);
    click_with_modifiers(&mut h, second, Modifiers::COMMAND);
    let app = h.state();
    let text = app.editor.as_ref().map_or("", |ed| ed.text.as_str());
    assert_eq!(text, "=SUM(C:C,E:E");
    assert_eq!(gridcraft_ui_egui::editor::formula_refs(text).iter().map(|r| r.2.a1()).collect::<Vec<_>>(), ["C:C", "E:E"]);
}

#[test]
fn shift_selects_a_contiguous_row_range_while_editing_a_formula() {
    let mut h = harness();
    let first = header_center(&h, true, 3);
    click(&mut h, first);
    let second = header_center(&h, true, 5);
    click_with_modifiers(&mut h, second, Modifiers::SHIFT);
    let app = h.state();
    let text = app.editor.as_ref().map_or("", |ed| ed.text.as_str());
    assert_eq!(text, "=SUM(4:6");
    assert_eq!(gridcraft_ui_egui::editor::formula_refs(text).iter().map(|r| r.2.a1()).collect::<Vec<_>>(), ["4:6"]);
}

#[test]
fn command_selects_discontiguous_rows_while_editing_a_formula() {
    let mut h = harness();
    let first = header_center(&h, true, 3);
    click(&mut h, first);
    let second = header_center(&h, true, 5);
    click_with_modifiers(&mut h, second, Modifiers::COMMAND);
    let app = h.state();
    let text = app.editor.as_ref().map_or("", |ed| ed.text.as_str());
    assert_eq!(text, "=SUM(4:4,6:6");
    assert_eq!(gridcraft_ui_egui::editor::formula_refs(text).iter().map(|r| r.2.a1()).collect::<Vec<_>>(), ["4:4", "6:6"]);
}

#[test]
fn shift_does_not_extend_a_column_reference_across_sheets() {
    let mut h = harness();
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Other sheet"})).unwrap();
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    h.run_steps(2);
    let first = header_center(&h, false, 2);
    click(&mut h, first);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Other sheet"})).unwrap();
    h.run_steps(2);
    let second = header_center(&h, false, 4);
    click_with_modifiers(&mut h, second, Modifiers::SHIFT);
    let text = h.state().editor.as_ref().map_or("", |ed| ed.text.as_str());
    assert_eq!(text, "=SUM('Other sheet'!E:E");
    assert_eq!(
        gridcraft_ui_egui::editor::formula_refs(text).iter().map(|r| (r.2.a1(), r.3.clone())).collect::<Vec<_>>(),
        [("E:E".into(), Some("Other sheet".into())),]
    );
}

#[test]
fn shift_does_not_extend_a_row_reference_across_sheets() {
    let mut h = harness();
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Other sheet"})).unwrap();
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    h.run_steps(2);
    let first = header_center(&h, true, 3);
    click(&mut h, first);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Other sheet"})).unwrap();
    h.run_steps(2);
    let second = header_center(&h, true, 5);
    click_with_modifiers(&mut h, second, Modifiers::SHIFT);
    let text = h.state().editor.as_ref().map_or("", |ed| ed.text.as_str());
    assert_eq!(text, "=SUM('Other sheet'!6:6");
    assert_eq!(
        gridcraft_ui_egui::editor::formula_refs(text).iter().map(|r| (r.2.a1(), r.3.clone())).collect::<Vec<_>>(),
        [("6:6".into(), Some("Other sheet".into())),]
    );
}
