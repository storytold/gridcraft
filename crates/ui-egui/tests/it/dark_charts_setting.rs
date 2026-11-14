//! The dark charts and dark pictures settings (`view.darkCharts`, `view.darkPictures`): off by default, a toggle, a preference, independent
//! of the dark sheet and of Display theme, and it inverts what a chart paints.

use egui::{Color32, Pos2, Rect, vec2};
use gridcraft_chart::{Prim, Rgba};
use gridcraft_engine::Session;
use gridcraft_ui_egui::chartview::paint_prims_dark;
use gridcraft_ui_egui::theme;
use gridcraft_ui_egui::{SheetApp, UiState};
use serde_json::json;

/// An app with an empty workbook, run for a few frames.
fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(Session::new()),
    );
    h.run_steps(4);
    h
}

#[test]
fn off_by_default() {
    assert!(!UiState::default().dark_charts);
    let h = harness();
    assert!(!h.state().ui.dark_charts);
    assert!(!theme::dark_charts(&h.ctx));
}

#[test]
fn the_command_sets_toggles_and_accepts_value() {
    let mut h = harness();
    assert_eq!(h.state_mut().run("view.darkCharts", json!({})), Ok(json!({"on": true})));
    assert_eq!(h.state_mut().run("view.darkCharts", json!({})), Ok(json!({"on": false})));
    assert_eq!(h.state_mut().run("view.darkCharts", json!({"on": true})), Ok(json!({"on": true})));
    assert_eq!(h.state_mut().run("view.darkCharts", json!({"value": false})), Ok(json!({"on": false})));
}

#[test]
fn it_is_independent_of_the_dark_sheet_and_the_display_theme_and_not_a_document_change() {
    let mut h = harness();
    let dirty = h.state().session.active().map(|d| d.is_dirty());
    h.state_mut().run("view.darkCharts", json!({"on": true})).unwrap();
    h.run_steps(2);
    assert!(theme::dark_charts(&h.ctx));
    assert!(!theme::dark_sheet(&h.ctx), "the sheet stays light");
    h.state_mut().run("view.theme", json!({"mode": "light"})).unwrap();
    h.state_mut().run("view.darkSheet", json!({"on": true})).unwrap();
    h.state_mut().run("view.darkSheet", json!({"on": false})).unwrap();
    h.run_steps(2);
    assert!(theme::dark_charts(&h.ctx), "neither the theme nor the sheet setting touch it");
    assert_eq!(h.state().session.active().map(|d| d.is_dirty()), dirty);
}

/// `view.darkPictures` toggles on its own, leaves the other settings alone and is saved.
#[test]
fn dark_pictures_command_toggles_is_independent_and_persists() {
    assert!(!UiState::default().dark_pictures);
    let mut h = harness();
    assert!(!theme::dark_pictures(&h.ctx));
    assert_eq!(h.state_mut().run("view.darkPictures", json!({})), Ok(json!({"on": true})));
    assert_eq!(h.state_mut().run("view.darkPictures", json!({})), Ok(json!({"on": false})));
    assert_eq!(h.state_mut().run("view.darkPictures", json!({"value": true})), Ok(json!({"on": true})));
    h.run_steps(2);
    assert!(theme::dark_pictures(&h.ctx));
    assert!(!theme::dark_charts(&h.ctx) && !theme::dark_sheet(&h.ctx), "only pictures");
    let state = UiState { dark_pictures: true, ..UiState::default() };
    let saved = serde_json::to_string(&state).unwrap();
    assert!(serde_json::from_str::<UiState>(&saved).unwrap().dark_pictures);
    assert!(!serde_json::from_str::<UiState>("{}").unwrap().dark_pictures);
}

#[test]
fn the_preference_survives_a_restart_and_older_settings_read_as_off() {
    let state = UiState { dark_charts: true, ..UiState::default() };
    let saved = serde_json::to_string(&state).unwrap();
    assert!(serde_json::from_str::<UiState>(&saved).unwrap().dark_charts);
    assert!(!serde_json::from_str::<UiState>("{}").unwrap().dark_charts);
}

/// Paints one white rectangle and one grey text through `paint_prims_dark`; returns the shapes' colours.
fn painted(dark: bool) -> (Color32, Color32) {
    let ctx = egui::Context::default();
    let white: Rgba = [255, 255, 255, 255];
    let grey: Rgba = [0x59, 0x59, 0x59, 255];
    let prims = vec![
        Prim::Rect { x: 0.0, y: 0.0, w: 50.0, h: 20.0, fill: Some(white), stroke: None, radius: 0.0 },
        Prim::Text {
            x: 5.0,
            y: 5.0,
            text: "Q1".into(),
            size: 10.0,
            color: grey,
            bold: false,
            align: gridcraft_chart::HAlign::Left,
            valign: gridcraft_chart::VAlign::Top,
            rotation: 0.0,
        },
    ];
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0))), ..Default::default() }, |ui| {
        let p = ui.painter().clone();
        paint_prims_dark(&p, Pos2::ZERO, &prims, 1.0, dark);
    });
    out.textures_delta.clear();
    let mut fill = Color32::TRANSPARENT;
    let mut text = Color32::TRANSPARENT;
    for clipped in &out.shapes {
        match &clipped.shape {
            egui::Shape::Rect(r) => fill = r.fill,
            egui::Shape::Text(t) => text = t.fallback_color,
            _ => {}
        }
    }
    (fill, text)
}

#[test]
fn dark_charts_map_white_to_the_sheet_grey_and_keep_text_legible() {
    let (light_fill, light_text) = painted(false);
    assert_eq!(light_fill, Color32::WHITE);
    assert_eq!(light_text, Color32::from_rgb(0x59, 0x59, 0x59));
    let (fill, text) = painted(true);
    assert_eq!(fill, Color32::from_rgb(theme::DARK_PAPER, theme::DARK_PAPER, theme::DARK_PAPER));
    assert!(theme::contrast(text, fill) >= theme::INK_TEXT_MIN, "text {text:?} on {fill:?}");
}
