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

fn values(h: &egui_kittest::Harness<'static, SheetApp>) -> Json {
    json!(h.state().dialog.as_ref().unwrap().values)
}

#[test]
fn the_dialog_opens_on_the_current_zoom_with_whole_numbers() {
    let mut h = harness();
    // At a magnification in the list, that choice is on.
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(6);
    assert_eq!((values(&h)["preset"].clone(), values(&h)["percent"].clone()), (json!("100"), json!(100)));
    h.get_by_label("75%");
    h.get_by_label("Fit selection");
    assert!(h.query_by_label("100.0").is_none(), "no float formatting");
    // At any other zoom, Custom is on and holds the zoom.
    h.state_mut().dialog = None;
    h.state_mut().run("view.zoom", json!({"percent": 160})).unwrap();
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(6);
    assert_eq!((values(&h)["preset"].clone(), values(&h)["percent"].clone()), (json!("custom"), json!(160)));
}

#[test]
fn typing_in_the_custom_box_chooses_custom() {
    let mut h = harness();
    control(&mut h, "ui.dialog", json!({"name": "zoom"}));
    control(&mut h, "ui.dialog.set", json!({"field": "preset", "value": "75"}));
    assert_eq!(values(&h)["preset"], json!("75"));
    control(&mut h, "ui.dialog.set", json!({"field": "percent", "value": 130}));
    assert_eq!(values(&h)["preset"], json!("custom"));
    control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(zoom(&h), 130);
}

#[test]
fn fit_selection_zooms_so_the_selection_fills_the_window() {
    let mut h = harness();
    h.state_mut().run("edit.goTo", json!({"reference": "A1:F40"})).unwrap();
    h.run_steps(6);
    // The grid area the dialog is opened over: what Fit selection fits the selection into.
    let viewport = h.state().grid.cells_rect.expect("the grid has been drawn");
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(6);
    assert_eq!(values(&h)["viewWidth"], json!(viewport.width()));
    assert_eq!(values(&h)["viewHeight"], json!(viewport.height()));
    h.get_by_label("Fit selection").click();
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    // The same zoom as the ribbon's Zoom to Selection gives for the same grid area.
    let fitted = zoom(&h);
    assert!(fitted != 100 && (10..=400).contains(&fitted), "{fitted}");
    h.state_mut().run("view.zoom", json!({"percent": 100})).unwrap();
    // The same area as the dialog fitted into: after a zoom the header and grid geometry differ
    // slightly, so the area drawn now is not the one the dialog saw.
    h.state_mut().run("view.zoomToSelection", json!({"viewWidth": viewport.width(), "viewHeight": viewport.height()})).unwrap();
    assert_eq!(zoom(&h), fitted);
}

#[test]
fn the_dialog_fits_its_content() {
    let mut h = harness();
    h.state_mut().open_dialog("zoom", json!({}));
    h.run_steps(6);
    // The Zoom title to the buttons: eight choices and one button row, no empty band between them.
    let (top, bottom) = (h.get_by_label("Zoom").rect().min.y, h.get_by_label("   OK   ").rect().max.y);
    let (custom, ok) = (h.get_by_label("Custom:").rect(), h.get_by_label("   OK   ").rect());
    assert!(ok.min.y - custom.max.y < 30.0, "{} points between Custom and the buttons", ok.min.y - custom.max.y);
    assert!(bottom - top < 270.0, "the dialog is {} points tall", bottom - top);
}
