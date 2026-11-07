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

/// A localized draft keeps both its original A1 references and the pointed range visible.
#[test]
fn localized_formula_points_to_and_highlights_a_column_range() {
    let mut h = harness();
    h.state_mut().run("app.language.set", json!({"code":"es"})).unwrap();
    h.state_mut().begin_edit(Some("=SUMA(Ventas[[#Datos];[Importe]];A1;1,5;".into()), false);
    h.run_steps(2);
    let first = header_center(&h, false, 2);
    click(&mut h, first);
    let second = header_center(&h, false, 4);
    click_with_modifiers(&mut h, second, Modifiers::SHIFT);
    let draft = h.state().editor.as_ref().unwrap();
    assert_eq!(draft.text, "=SUMA(Ventas[[#Datos];[Importe]];A1;1,5;C:E");
    let refs = gridcraft_ui_egui::editor::formula_refs_in(&draft.text, draft.locale);
    assert_eq!(refs.iter().map(|r| &draft.text[r.0..r.1]).collect::<Vec<_>>(), ["A1", "C:E"]);
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

fn press_and_drag(h: &mut egui_kittest::Harness<'_, SheetApp>, from: Pos2, to: Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut()
        .events
        .extend([Event::PointerMoved(from), Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers }]);
    h.step();
    drag_to(h, to);
}

fn drag_to(h: &mut egui_kittest::Harness<'_, SheetApp>, to: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(to));
    h.step();
}

fn release(h: &mut egui_kittest::Harness<'_, SheetApp>, at: Pos2) {
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
}

fn text<'a>(h: &'a egui_kittest::Harness<'_, SheetApp>) -> &'a str {
    h.state().editor.as_ref().map_or("", |ed| ed.text.as_str())
}

#[test]
fn dragging_over_column_headers_extends_the_reference() {
    let mut h = harness();
    let (c, e, a, d) = (header_center(&h, false, 2), header_center(&h, false, 4), header_center(&h, false, 0), header_center(&h, false, 3));
    press_and_drag(&mut h, c, e, Modifiers::NONE);
    assert_eq!(text(&h), "=SUM(C:E");
    // Back past the press: the reference follows the pointer instead of keeping E.
    drag_to(&mut h, a);
    assert_eq!(text(&h), "=SUM(A:C");
    // The release position counts even without a movement frame before it.
    release(&mut h, d);
    assert_eq!(text(&h), "=SUM(C:D");
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "F8");
}

#[test]
fn dragging_over_row_headers_extends_the_reference() {
    let mut h = harness();
    let (r3, r6, r2) = (header_center(&h, true, 2), header_center(&h, true, 5), header_center(&h, true, 1));
    press_and_drag(&mut h, r3, r6, Modifiers::NONE);
    assert_eq!(text(&h), "=SUM(3:6");
    release(&mut h, r2);
    assert_eq!(text(&h), "=SUM(2:3");
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "F8");
}

#[test]
fn header_drag_on_another_sheet_keeps_the_qualifier() {
    let mut h = harness();
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Other sheet"})).unwrap();
    h.run_steps(2);
    let (b, d) = (header_center(&h, false, 1), header_center(&h, false, 3));
    press_and_drag(&mut h, b, d, Modifiers::NONE);
    assert_eq!(text(&h), "=SUM('Other sheet'!B:D");
}

#[test]
fn header_drag_with_command_keeps_the_earlier_argument() {
    let mut h = harness();
    let (a, e, f) = (header_center(&h, false, 0), header_center(&h, false, 4), header_center(&h, false, 5));
    click(&mut h, a);
    press_and_drag(&mut h, e, f, Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE });
    assert_eq!(text(&h), "=SUM(A:A,E:F");
}

#[test]
fn header_drag_with_shift_keeps_the_far_end_of_the_reference() {
    let mut h = harness();
    let (c, e, d) = (header_center(&h, false, 2), header_center(&h, false, 4), header_center(&h, false, 3));
    click(&mut h, c);
    press_and_drag(&mut h, e, d, Modifiers { shift: true, ..Modifiers::NONE });
    assert_eq!(text(&h), "=SUM(C:D");
}

#[test]
fn clicks_on_headers_replace_the_reference_after_a_drag_too() {
    let mut h = harness();
    let (c, e, g) = (header_center(&h, false, 2), header_center(&h, false, 4), header_center(&h, false, 6));
    press_and_drag(&mut h, c, e, Modifiers::NONE);
    release(&mut h, e);
    assert_eq!(text(&h), "=SUM(C:E");
    click(&mut h, g);
    assert_eq!(text(&h), "=SUM(G:G");
}

fn shift_arrow(h: &mut egui_kittest::Harness<'_, SheetApp>, key: egui::Key) {
    let modifiers = Modifiers { shift: true, ..Modifiers::NONE };
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.step();
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        h.step();
    }
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

#[test]
fn shift_arrow_after_a_header_drag_moves_the_dragged_end() {
    let mut h = harness();
    let (c, e) = (header_center(&h, false, 2), header_center(&h, false, 4));
    press_and_drag(&mut h, c, e, Modifiers::NONE);
    release(&mut h, e);
    assert_eq!(text(&h), "=SUM(C:E");
    shift_arrow(&mut h, egui::Key::ArrowRight);
    assert_eq!(text(&h), "=SUM(C:F");

    // Dragged leftwards, the dragged end is the first column.
    let mut h = harness();
    let (e, c) = (header_center(&h, false, 4), header_center(&h, false, 2));
    press_and_drag(&mut h, e, c, Modifiers::NONE);
    release(&mut h, c);
    assert_eq!(text(&h), "=SUM(C:E");
    shift_arrow(&mut h, egui::Key::ArrowLeft);
    assert_eq!(text(&h), "=SUM(B:E");

    let mut h = harness();
    let (r1, r3) = (header_center(&h, true, 0), header_center(&h, true, 2));
    press_and_drag(&mut h, r1, r3, Modifiers::NONE);
    release(&mut h, r3);
    assert_eq!(text(&h), "=SUM(1:3");
    shift_arrow(&mut h, egui::Key::ArrowDown);
    assert_eq!(text(&h), "=SUM(1:4");
}

#[test]
fn shift_arrow_pointing_on_another_sheet_keeps_the_qualifier() {
    let mut h = harness();
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Other sheet"})).unwrap();
    h.run_steps(2);
    let (b, d) = (header_center(&h, false, 1), header_center(&h, false, 3));
    press_and_drag(&mut h, b, d, Modifiers::NONE);
    release(&mut h, d);
    assert_eq!(text(&h), "=SUM('Other sheet'!B:D");
    let app = h.state_mut();
    let mut ed = app.editor.take().unwrap();
    gridcraft_ui_egui::grid::point_move(app, &mut ed, 0, 1, true, false);
    app.editor = Some(ed);
    assert_eq!(text(&h), "=SUM('Other sheet'!B:E");
}
