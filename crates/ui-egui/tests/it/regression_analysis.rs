//! Regression range picking and document ownership through actual UI input and control calls.
use egui::{Event, Modifiers, PointerButton, accesskit::Role, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::{Session, core::CellRef};
use gridcraft_ui_egui::{
    SheetApp,
    control::{Outcome, handle},
};
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"name":"Y's data"})).unwrap();
    s.execute("home.insertSheet", json!({"name":"X's data"})).unwrap();
    s.execute("sheet.activate", json!({"sheet":"Y's data"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 1000.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(6);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool":"regression"}));
    h.run_steps(6);
    h
}

fn control(h: &mut Harness, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => panic!("unexpected screenshot"),
    }
}

fn drag(h: &mut Harness, start: CellRef, end: CellRef) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = app.grid.geo.as_ref().unwrap();
    let (start, end) = (geo.mirror_rect(geo.cell_rect(sheet, start)).center(), geo.mirror_rect(geo.cell_rect(sheet, end)).center());
    h.input_mut().events.extend([
        Event::PointerMoved(start),
        Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerMoved(end));
    h.run_steps(3);
    h.input_mut().events.push(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

#[test]
fn regression_picker_tracks_a_drag_after_switching_worksheets_and_finishes() {
    let mut h = harness();
    h.get_all_by_role_and_label(Role::Button, "Select…").next().unwrap().click();
    h.run_steps(3);
    h.state_mut().run("sheet.activate", json!({"sheet":"X's data"})).unwrap();
    h.run_steps(3);
    drag(&mut h, CellRef::new(0, 0), CellRef::new(3, 0));
    assert_eq!(h.state().dialog.as_ref().unwrap().values["inputY"], "'X''s data'!A1:A4");
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "A1:A4");
    h.get_by_role_and_label(Role::Button, "Finish").click();
    h.run_steps(3);
    assert!(h.get_all_by_role_and_label(Role::Button, "Select…").count() >= 3);
    // Leaving picker mode restores ordinary worksheet selection, not the previous range field.
    drag(&mut h, CellRef::new(0, 0), CellRef::new(2, 0));
    assert_eq!(h.state().dialog.as_ref().unwrap().values["inputY"], "'X''s data'!A1:A4");
}

#[test]
fn regression_output_picker_uses_one_cell_on_the_selected_sheet() {
    let mut h = harness();
    h.get_all_by_role_and_label(Role::Button, "Select…").nth(2).unwrap().click();
    h.run_steps(3);
    h.state_mut().run("sheet.activate", json!({"sheet":"X's data"})).unwrap();
    h.run_steps(3);
    drag(&mut h, CellRef::new(1, 1), CellRef::new(4, 3));
    let values = &h.state().dialog.as_ref().unwrap().values;
    assert_eq!(values["output"], "'X''s data'!B2");
    assert_eq!(values["outputTo"], "range");
    assert_eq!(values["inputY"], "'Y''s data'!A1");
}

#[test]
fn regression_control_and_ok_refuse_a_different_workbook() {
    let mut h = harness();
    h.state_mut().session.new_workbook();
    let before = serde_json::to_value(&h.state().session.active().unwrap().wb).unwrap();
    let result = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(result["ok"], false);
    assert!(h.state().dialog.is_some());
    assert_eq!(serde_json::to_value(&h.state().session.active().unwrap().wb).unwrap(), before);
    assert_eq!(control(&mut h, "ui.dialog.set", json!({"field":"inputY","value":"B1:B3"}))["ok"], false);
    h.run_steps(4);
    assert!(h.query_by_role_and_label(Role::Button, "OK").is_none());
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
}
