//! The Zoom dialog stores the Magnification the user picks.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn zoom(h: &egui_kittest::Harness<'static, SheetApp>) -> u16 {
    h.state().session.active().unwrap().wb.active().unwrap().zoom
}

fn control(h: &mut egui_kittest::Harness<'static, SheetApp>, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

#[test]
fn choosing_a_magnification_and_pressing_ok_sets_that_zoom() {
    let mut h = harness();
    h.state_mut().run("view.zoom", json!({"percent": 160})).unwrap();
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(6);
    h.get_by_label("50%").click();
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert_eq!(zoom(&h), 50);
}

#[test]
fn choosing_a_magnification_and_confirming_by_command_sets_that_zoom() {
    let mut h = harness();
    h.state_mut().run("view.zoom", json!({"percent": 180})).unwrap();
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(6);
    h.get_by_label("75%").click();
    h.run_steps(6);
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(zoom(&h), 75);
}

#[test]
fn setting_the_magnification_over_the_control_channel_sets_that_zoom() {
    let mut h = harness();
    h.state_mut().run("view.zoom", json!({"percent": 180})).unwrap();
    control(&mut h, "ui.dialog", json!({"name": "zoom"}));
    control(&mut h, "ui.dialog.set", json!({"field": "preset", "value": "25"}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(zoom(&h), 25);
}

#[test]
fn a_custom_percentage_after_a_magnification_wins_and_an_unedited_ok_keeps_the_zoom() {
    let mut h = harness();
    control(&mut h, "ui.dialog", json!({"name": "zoom"}));
    control(&mut h, "ui.dialog.set", json!({"field": "preset", "value": "25"}));
    control(&mut h, "ui.dialog.set", json!({"field": "percent", "value": 225}));
    control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(zoom(&h), 225);
    control(&mut h, "ui.dialog", json!({"name": "zoom"}));
    control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(zoom(&h), 225);
}

#[test]
fn the_current_zoom_is_preselected_as_its_magnification_or_as_custom() {
    let mut h = harness();
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(2);
    let d = h.state().dialog.clone().unwrap();
    assert_eq!(d.values["preset"], json!("100"));
    assert_eq!(d.values["percent"], json!(100));
    control(&mut h, "ui.dialog.cancel", json!({}));
    h.state_mut().run("view.zoom", json!({"percent": 130})).unwrap();
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(2);
    let d = h.state().dialog.clone().unwrap();
    assert_eq!(d.values["preset"], json!("custom"));
    assert_eq!(d.values["percent"], json!(130));
    // An unedited OK keeps the zoom.
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(zoom(&h), 130);
}

#[test]
fn a_typed_percentage_selects_custom_and_sets_that_zoom() {
    let mut h = harness();
    control(&mut h, "ui.dialog", json!({"name": "zoom"}));
    control(&mut h, "ui.dialog.set", json!({"field": "preset", "value": "fit"}));
    control(&mut h, "ui.dialog.set", json!({"field": "percent", "value": 150}));
    assert_eq!(h.state().dialog.as_ref().unwrap().values["preset"], json!("custom"));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(zoom(&h), 150);
}

#[test]
fn fit_selection_zooms_so_the_selected_cells_fill_the_grid() {
    let mut h = harness();
    h.state_mut().run("selection.set", json!({"range": "A1:J40"})).unwrap();
    h.run_steps(2);
    let view = h.state().grid.cells_rect.unwrap();
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(6);
    h.get_by_label("Fit selection").click();
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    // The same answer the ribbon's Zoom to Selection gives for this grid.
    let mut expected = Session::new();
    expected.new_workbook();
    expected.execute("selection.set", json!({"range": "A1:J40"})).unwrap();
    let r = expected.execute("view.zoomToSelection", json!({"viewWidth": view.width(), "viewHeight": view.height()})).unwrap();
    assert_eq!(json!(zoom(&h)), r["zoom"]);
    assert!(zoom(&h) > 10 && zoom(&h) < 100, "{}", zoom(&h));
    // Fit selection over the control channel, before any frame is drawn, uses the grid size captured on opening.
    h.state_mut().run("selection.set", json!({"range": "A1"})).unwrap();
    control(&mut h, "ui.dialog", json!({"name": "zoom"}));
    control(&mut h, "ui.dialog.set", json!({"field": "preset", "value": "fit"}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(zoom(&h), 400);
}
