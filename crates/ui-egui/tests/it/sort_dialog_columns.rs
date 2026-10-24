//! #1906: the Sort dialog seeds "Sort by" from the active column instead of a
//! free-text box stuck on "A", and the column list offers the region's columns.

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// Header row + three rows; the user's cursor sits in column B (Revenue).
fn sales() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Month", "Revenue"], ["Mar", 30], ["Jan", 10], ["Feb", 20]]})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    s
}

fn col(h: &Harness) -> String {
    h.state().dialog.as_ref().unwrap().values["levels"][0]["column"].as_str().unwrap_or("<missing>").to_string()
}

#[test]
fn sort_dialog_seeds_the_active_column() {
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    assert_eq!(col(&h), "B", "Sort by starts on the column the cursor is in, not A");
}

#[test]
fn add_level_picks_a_column_no_level_uses_yet() {
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    // Level 1 seeded B; clicking Add Level must not default level 2 to B too (#1906 review).
    h.run_steps(6);
    h.get_by_label("      Add Level").click();
    h.run_steps(6);
    let cols: Vec<String> = h.state().dialog.as_ref().unwrap().values["levels"]
        .as_array()
        .map(|a| a.iter().filter_map(|l| l["column"].as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    assert_eq!(cols.len(), 2, "two levels after Add Level");
    assert_ne!(cols[0], cols[1], "the second level defaults to a different column: {cols:?}");
}

#[test]
fn add_level_does_not_add_a_column_outside_the_selected_range() {
    let mut s = sales();
    s.execute("selection.set", json!({"range": "A1:A4", "active": "A2"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.get_by_label("      Add Level").click();
    h.run_steps(4);
    let levels = h.state().dialog.as_ref().unwrap().values["levels"].as_array().unwrap();
    assert_eq!(levels.len(), 1, "no additional level should use a column outside A");
    assert_eq!(levels[0]["column"], "A");
}

#[test]
fn sort_dialog_restores_all_selected_areas_and_the_active_cell() {
    let mut s = sales();
    s.execute("selection.set", json!({"range": "A1:A4,B1:B4", "active": "B2"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Key {
        key: Key::Escape,
        physical_key: Some(Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    let selection = &h.state().session.active().unwrap().selection;
    assert_eq!(selection.a1(), "A1:A4,B1:B4");
    assert_eq!(selection.active.a1(), "B2");
}

#[test]
fn sorting_by_the_seeded_column_orders_the_rows() {
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    // Accept the dialog: rows must come back ordered by Revenue (10, 20, 30).
    h.input_mut().events.push(egui::Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    let nums: Vec<i64> = (1..4).map(|r| sh.value(gridcraft_engine::core::CellRef::new(r, 1))).filter_map(|v| v.as_f64().map(|n| n as i64)).collect();
    assert_eq!(nums, vec![10, 20, 30], "sort ran on the seeded column B");
}
