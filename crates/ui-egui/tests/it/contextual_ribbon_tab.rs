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

#[test]
fn a_second_window_does_not_reset_the_first_windows_table_design_tab() {
    use egui::{Event, PointerButton, Shape};

    let mut h = harness(session_with_table());
    let first = h.state().session.active_window().unwrap();
    // A second window on the same workbook, with its active cell outside the table.
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    let second = h.state().session.active_window().unwrap();
    h.state_mut().run("selection.set", json!({"cell": "D10"})).unwrap();
    h.state_mut().run("window.activate", json!({"window": first})).unwrap();
    h.run_steps(2);
    let tab_center = h
        .output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == "Table Design" => Some(t.pos + t.galley.size() / 2.0),
            _ => None,
        })
        .expect("the Table Design tab is painted while a cell is in the table");
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton {
            pos: tab_center,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Table Design");
    // DocWindows draws every window each frame. The second window's cell is outside the table,
    // so its frame falls back to Home; the first window's Table Design tab must survive it.
    h.state_mut().show_window(second);
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home", "the second window shows Home for its own selection");
    h.state_mut().enter_window(first);
    h.run_steps(2);
    assert_eq!(h.state().session.active_window(), Some(first));
    assert_eq!(h.state().ui.ribbon_tab, "Table Design", "the first window's tab survives the second window's frame");
    let d = h.state().session.active().unwrap();
    assert_eq!(d.selection.active.a1(), "A2", "the first window still selects a cell in the table");
    assert!(d.wb.active().unwrap().table_at(d.selection.active).is_some(), "the table is still there");
}
