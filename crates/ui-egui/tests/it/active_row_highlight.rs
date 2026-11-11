//! "Highlight the active cell's row": a global preference, off by default, that tints the whole
//! row of the active cell and leaves the workbook alone.

use egui::{Shape, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::theme::Tokens;
use gridcraft_ui_egui::{SheetApp, UiState, grid::Geo, grid::active_row_band};
use serde_json::json;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn book(active: &str) -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"range": active})).unwrap();
    session
}

/// The rectangles painted with the highlight's own colour.
fn tinted(h: &egui_kittest::Harness<'_, SheetApp>) -> Vec<egui::Rect> {
    let tint = Tokens::get(&h.ctx).active_row;
    h.output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(r) if r.fill == tint => Some(r.rect),
            _ => None,
        })
        .collect()
}

#[test]
fn it_is_off_by_default_and_paints_nothing() {
    assert!(!UiState::default().highlight_active_row);
    let h = harness(book("C5"));
    assert!(tinted(&h).is_empty());
}

#[test]
fn turned_on_it_tints_the_active_row_across_the_columns() {
    let mut h = harness(book("C5"));
    assert_eq!(h.state_mut().run("options.highlightActiveRow", json!({"on": true})), Ok(json!({"on": true})));
    h.run_steps(2);
    let bands = tinted(&h);
    assert_eq!(bands.len(), 1, "{bands:?}");
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let cell = geo.cell_rect(sheet, CellRef::parse("C5").unwrap());
    assert_eq!((bands[0].top(), bands[0].bottom()), (cell.top(), cell.bottom()));
    assert!(bands[0].left() <= geo.cells.left() + 1.0, "starts at the first column: {:?}", bands[0]);
    assert!(bands[0].right() >= geo.cells.right() - 1.0, "reaches the last visible column: {:?}", bands[0]);
}

#[test]
fn the_band_follows_the_active_cell_and_only_the_row() {
    let mut h = harness(book("C5"));
    h.state_mut().run("options.highlightActiveRow", json!({"on": true})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"range": "E9"})).unwrap();
    h.run_steps(2);
    let bands = tinted(&h);
    assert_eq!(bands.len(), 1, "a row, and no column band: {bands:?}");
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    assert_eq!(bands[0].top(), geo.cell_rect(sheet, CellRef::parse("E9").unwrap()).top());
}

#[test]
fn it_applies_to_every_sheet_and_workbook() {
    let mut h = harness(book("C5"));
    h.state_mut().run("options.highlightActiveRow", json!({"on": true})).unwrap();
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    h.run_steps(2);
    assert_eq!(tinted(&h).len(), 1, "another sheet");
    h.state_mut().session.new_workbook();
    h.run_steps(2);
    assert_eq!(tinted(&h).len(), 1, "another workbook");
}

#[test]
fn turning_it_off_clears_the_band_and_the_toggle_leaves_the_workbook_alone() {
    let mut h = harness(book("C5"));
    let dirty = h.state().session.active().unwrap().is_dirty();
    assert_eq!(h.state_mut().run("options.highlightActiveRow", json!({})), Ok(json!({"on": true})), "no parameters: it toggles");
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().is_dirty(), dirty);
    assert_eq!(h.state_mut().run("options.highlightActiveRow", json!({})), Ok(json!({"on": false})));
    h.run_steps(2);
    assert!(tinted(&h).is_empty());
    assert_eq!(h.state().session.active().unwrap().is_dirty(), dirty);
}

#[test]
fn the_preference_survives_a_restart_and_older_settings_read_as_off() {
    let state = UiState { highlight_active_row: true, ..UiState::default() };
    let saved = serde_json::to_string(&state).unwrap();
    assert!(serde_json::from_str::<UiState>(&saved).unwrap().highlight_active_row);
    assert!(!serde_json::from_str::<UiState>("{}").unwrap().highlight_active_row);
}

#[test]
fn a_hidden_row_or_a_row_outside_the_quadrant_has_no_band() {
    let mut session = book("A1");
    session.execute("home.rowHeight", json!({"rows": "5:5", "height": 0})).unwrap();
    let h = harness(session);
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    assert_eq!(active_row_band(&geo, sheet, 4, (0, 20), (0, 5)), None, "hidden");
    assert_eq!(active_row_band(&geo, sheet, 30, (0, 20), (0, 5)), None, "outside");
    assert!(active_row_band(&geo, sheet, 6, (0, 20), (0, 5)).is_some());
}

#[test]
fn in_page_layout_the_band_shows_on_a_click_to_add_data_page_too() {
    let mut h = harness(book("C5"));
    h.state_mut().run("options.highlightActiveRow", json!({"on": true})).unwrap();
    h.state_mut().session.execute("view.pageLayout", json!({})).unwrap();
    h.run_steps(3);
    let bands = tinted(&h);
    // Each page paints its own part of the row; the faded page paints it again over the fade.
    let repeated = bands.iter().filter(|b| bands.iter().filter(|o| o == b).count() == 2).count();
    assert_eq!(repeated, 2, "{bands:?}");
    assert!(bands.iter().all(|b| b.top() == bands[0].top()), "one row: {bands:?}");
}

#[test]
fn with_frozen_panes_the_band_crosses_the_frozen_columns() {
    let mut h = harness(book("C5"));
    h.state_mut().session.execute("selection.set", json!({"range": "C3"})).unwrap();
    h.state_mut().session.execute("view.freezePanes", json!({})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"range": "E7"})).unwrap();
    h.state_mut().run("options.highlightActiveRow", json!({"on": true})).unwrap();
    h.run_steps(2);
    let bands = tinted(&h);
    assert_eq!(bands.len(), 2, "the frozen columns' pane and the scrolling one: {bands:?}");
    assert_eq!(bands[0].top(), bands[1].top());
}
