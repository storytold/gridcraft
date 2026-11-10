//! The dark sheet setting (`view.darkSheet`): off by default, a toggle, a preference, screen only.

use egui::vec2;
use gridcraft_engine::Session;
use gridcraft_ui_egui::theme::{self, Tokens};
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
fn off_by_default_and_the_sheet_stays_light() {
    assert!(!UiState::default().dark_sheet);
    let h = harness();
    assert!(!h.state().ui.dark_sheet);
    assert!(!theme::dark_sheet(&h.ctx));
}

#[test]
fn the_command_sets_toggles_and_accepts_the_wordcraft_spelling() {
    let mut h = harness();
    assert_eq!(h.state_mut().run("view.darkSheet", json!({})), Ok(json!({"on": true})));
    assert_eq!(h.state_mut().run("view.darkSheet", json!({})), Ok(json!({"on": false})));
    assert_eq!(h.state_mut().run("view.darkSheet", json!({"on": true})), Ok(json!({"on": true})));
    assert_eq!(h.state_mut().run("view.darkSheet", json!({"value": false})), Ok(json!({"on": false})));
}

#[test]
fn the_flag_reaches_the_tokens_and_is_independent_of_the_display_theme() {
    let mut h = harness();
    h.state_mut().run("view.darkSheet", json!({"on": true})).unwrap();
    h.run_steps(2);
    assert!(theme::dark_sheet(&h.ctx));
    assert_ne!(Tokens::get(&h.ctx).grid_bg, Tokens::get(&h.ctx).with_dark_sheet(false).grid_bg);
    h.state_mut().run("view.theme", json!({"mode": "light"})).unwrap();
    h.run_steps(2);
    assert!(theme::dark_sheet(&h.ctx), "the Display theme does not touch it");
}

#[test]
fn the_preference_survives_a_restart_and_older_settings_read_as_off() {
    let state = UiState { dark_sheet: true, ..UiState::default() };
    let saved = serde_json::to_string(&state).unwrap();
    assert!(serde_json::from_str::<UiState>(&saved).unwrap().dark_sheet);
    assert!(!serde_json::from_str::<UiState>("{}").unwrap().dark_sheet);
}

#[test]
fn it_is_not_a_document_change() {
    let mut h = harness();
    let dirty = h.state().session.active().map(|d| d.is_dirty());
    h.state_mut().run("view.darkSheet", json!({"on": true})).unwrap();
    assert_eq!(h.state().session.active().map(|d| d.is_dirty()), dirty);
}

/// The pixel at the centre of a cell and on its right gridline, or `None` where there is no
/// GPU to render with.
fn cell_and_gridline_pixels(h: &mut egui_kittest::Harness<'static, SheetApp>, a1: &str) -> Option<([u8; 3], [u8; 3])> {
    let image = h.render().ok()?;
    let sh = h.state().session.active()?.wb.active()?;
    let r = h.state().grid.geo.as_ref()?.screen_cell_rect(sh, gridcraft_engine::core::CellRef::parse(a1)?);
    let ppp = h.ctx.pixels_per_point();
    let px = |x: f32, y: f32| {
        let p = image.get_pixel((x * ppp) as u32, (y * ppp) as u32);
        [p[0], p[1], p[2]]
    };
    // The line is drawn at `round(x) - 0.5`, i.e. on the pixel left of the cell's right edge.
    Some((px(r.center().x, r.center().y), px(r.right().round() - 0.5, r.center().y)))
}

#[test]
fn the_grid_paints_the_sheet_tokens_light_or_dark() {
    let mut h = harness();
    // Cells far from the selection (A1), so only the background and the gridlines show.
    let Some((cell, line)) = cell_and_gridline_pixels(&mut h, "F12") else { return };
    assert_eq!((cell, line), ([255; 3], [0xE1; 3]), "light by default");
    h.state_mut().run("view.darkSheet", json!({"on": true})).unwrap();
    h.run_steps(3);
    let (cell, line) = cell_and_gridline_pixels(&mut h, "F12").unwrap();
    assert_eq!((cell, line), ([0x33; 3], [0x4B; 3]), "the dark sheet paints grid_bg and gridline");
    h.state_mut().run("view.gridlines", json!({"on": false})).unwrap();
    h.run_steps(3);
    let (cell, line) = cell_and_gridline_pixels(&mut h, "F12").unwrap();
    assert_eq!((cell, line), ([0x33; 3], [0x33; 3]), "the gridlines toggle still hides the lines");
    h.state_mut().run("view.darkSheet", json!({"on": false})).unwrap();
    h.run_steps(3);
    let (cell, line) = cell_and_gridline_pixels(&mut h, "F12").unwrap();
    assert_eq!((cell, line), ([255; 3], [255; 3]));
}

/// The pixels inside cell `a1`, or `None` where there is no GPU to render with.
fn cell_pixels(h: &mut egui_kittest::Harness<'static, SheetApp>, a1: &str) -> Option<Vec<[u8; 3]>> {
    let image = h.render().ok()?;
    let sh = h.state().session.active()?.wb.active()?;
    let r = h.state().grid.geo.as_ref()?.screen_cell_rect(sh, gridcraft_engine::core::CellRef::parse(a1)?).shrink(3.0);
    let ppp = h.ctx.pixels_per_point();
    let (x0, x1, y0, y1) = ((r.left() * ppp) as u32, (r.right() * ppp) as u32, (r.top() * ppp) as u32, (r.bottom() * ppp) as u32);
    Some((y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| image.get_pixel(x, y)).map(|p| [p[0], p[1], p[2]]).collect())
}

/// On the dark sheet the painted pixels show light automatic text and fills mapped by the rule.
#[test]
fn cell_text_and_fills_follow_the_colour_rule_on_the_dark_sheet() {
    let mut h = harness();
    let app = h.state_mut();
    app.run("cell.set", json!({"cell": "D8", "input": "WWWWWW"})).unwrap();
    app.run("home.fillColor", json!({"range": "F8", "color": "#DDEBF7"})).unwrap();
    app.run("selection.set", json!({"cell": "A1"})).unwrap();
    h.run_steps(3);
    let Some(text) = cell_pixels(&mut h, "D8") else { return };
    let fill = cell_pixels(&mut h, "F8").unwrap();
    // Light sheet: black text on white, the fill as authored.
    assert!(text.iter().any(|p| p.iter().all(|&v| v < 60)), "black glyphs on the light sheet");
    assert!(fill.iter().all(|p| *p == [0xDD, 0xEB, 0xF7]));
    h.state_mut().run("view.darkSheet", json!({"on": true})).unwrap();
    h.run_steps(3);
    let text = cell_pixels(&mut h, "D8").unwrap();
    let fill = cell_pixels(&mut h, "F8").unwrap();
    // Dark sheet: automatic text is light on #333333, the pale fill becomes the deep #39444E.
    assert!(text.iter().any(|p| p.iter().all(|&v| v > 200)), "light glyphs on the dark sheet");
    assert!(text.iter().all(|p| p.iter().all(|&v| v >= 0x33)), "nothing darker than the sheet");
    assert!(fill.iter().all(|p| *p == [0x39, 0x44, 0x4E]), "{:?}", fill.first());
}
