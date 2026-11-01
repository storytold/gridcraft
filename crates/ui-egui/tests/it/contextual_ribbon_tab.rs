//! A contextual ribbon tab (Table Design, Chart Design) only exists while its object is selected.
//! The tab remembered in the saved UI state, or left behind when the selection moves away, must
//! not leave the ribbon blank: it falls back to Home.

use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    crate::support::harness(session, 4)
}

fn session_with_table() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["a", 1], ["b", 2]]})).unwrap();
    session.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
    session.execute("selection.set", json!({"range": "A2"})).unwrap();
    session
}

/// Sets a saved ribbon tab on a fresh app and builds the stepped harness, for unavailable-tab cases.
fn app_restored_on(tab: &str, session: Session) -> Harness {
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = tab.into();
    crate::support::app_harness(app, 4)
}

#[test]
fn a_saved_table_design_tab_without_a_table_opens_on_home() {
    let mut session = Session::new();
    session.new_workbook();
    let h = app_restored_on("Table Design", session);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn a_saved_chart_design_tab_without_a_chart_opens_on_home() {
    let mut session = Session::new();
    session.new_workbook();
    let h = app_restored_on("Chart Design", session);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn leaving_the_table_while_on_table_design_returns_to_home() {
    let mut h = harness(session_with_table());
    h.state_mut().ui.ribbon_tab = "Table Design".into();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Table Design", "the tab stays while the cell is in the table");
    h.state_mut().session.execute("selection.set", json!({"range": "D10"})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn startup_opens_on_home_not_the_saved_tab() {
    let saved = gridcraft_ui_egui::UiState { ribbon_tab: "Table Design".into(), dark: true, ..Default::default() };
    let json = serde_json::to_string(&saved).unwrap();
    assert!(json.contains(r#""ribbonTab":"Table Design""#), "{json}");
    let restored = serde_json::from_str::<gridcraft_ui_egui::UiState>(&json).unwrap().at_startup();
    assert_eq!(restored.ribbon_tab, "Home", "the saved tab is not restored");
    assert!(restored.dark, "the other preferences are");
}

#[test]
fn an_ordinary_tab_stays_put_in_a_session() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness(session);
    h.state_mut().ui.ribbon_tab = "Data".into();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, "Data");
}
