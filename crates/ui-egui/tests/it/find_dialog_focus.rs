//! Opening Find and Replace puts the cursor in "Find what", so the user can type at once.

use egui::{Event, Key, Modifiers};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

fn harness() -> Harness {
    crate::support::blank_harness(6)
}

fn type_after_shortcut(k: Key) -> Option<serde_json::Value> {
    let mut h = harness();
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(6);
    h.input_mut().events.push(Event::Text("abc".into()));
    h.run_steps(6);
    let d = h.state().dialog.as_ref().expect("find dialog is open");
    assert_eq!(d.name, "find");
    d.values.get("what").cloned()
}

#[test]
fn typing_right_after_ctrl_f_fills_find_what() {
    assert_eq!(type_after_shortcut(Key::F), Some(json!("abc")));
}

#[test]
fn typing_right_after_ctrl_h_fills_find_what() {
    assert_eq!(type_after_shortcut(Key::H), Some(json!("abc")));
}

/// With a range selected, Find All in the dialog lists only the matches inside it, as in Excel
/// (#3224); with one cell selected it searches the whole sheet.
#[test]
fn find_all_with_a_range_selected_searches_only_the_range() {
    use egui_kittest::kittest::Queryable;
    for (range, expected) in [("B1:B3", 2), ("C5", 4)] {
        let mut h = harness();
        let app = h.state_mut();
        app.session.execute("range.setValues", json!({"range": "A1:B3", "values": [["pear", "pear"], ["fig", "fig"], ["pear", "pear"]]})).unwrap();
        app.session.execute("selection.set", json!({"range": range})).unwrap();
        h.run_steps(2);
        h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: true, repeat: false, modifiers: CTRL });
        h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: false, repeat: false, modifiers: CTRL });
        h.run_steps(6);
        h.input_mut().events.push(Event::Text("pear".into()));
        h.run_steps(4);
        h.get_by_label("Find All").click();
        h.run_steps(4);
        let d = h.state().dialog.as_ref().expect("find dialog is open");
        let count = d.result.as_ref().map(|r| r["count"].clone());
        assert_eq!(count, Some(json!(expected)), "{range} selected");
    }
}

/// Ctrl+A in the Find All list selects every found cell on the focused result's sheet, each as
/// its own area, and leaves out the results on other sheets, as in Excel (#4874). The rows show
/// which are selected, and the count is in a live region.
#[test]
fn ctrl_a_in_find_all_results_selects_the_found_cells_on_that_sheet() {
    use egui::accesskit::{Live, Toggled};
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut h = harness();
    let app = h.state_mut();
    app.session.execute("range.setValues", json!({"range": "A1:A2", "values": [["pear"], ["fig"]]})).unwrap();
    app.session.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    app.session.execute("range.setValues", json!({"range": "B2:D4", "values": [["pear", "", ""], ["", "fig", ""], ["", "", "pear"]]})).unwrap();
    app.session.execute("selection.set", json!({"sheet": 0})).unwrap();
    h.run_steps(2);
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(6);
    h.input_mut().events.push(Event::Text("pear".into()));
    h.run_steps(4);
    h.get_by_label("Within workbook").click();
    h.run_steps(2);
    h.get_by_label("Find All").click();
    h.run_steps(4);
    let found = h.state().dialog.as_ref().and_then(|d| d.result.clone()).expect("Find All listed results");
    let rows = found["results"].as_array().cloned().unwrap_or_default();
    let label = |r: &serde_json::Value| {
        format!("{}  {}  {}", r["sheet"].as_str().unwrap_or(""), r["cell"].as_str().unwrap_or(""), r["value"].as_str().unwrap_or(""))
    };
    let (other, first): (Vec<_>, Vec<_>) = rows.iter().partition(|r| r["sheet"] == "Other");
    assert!(other.len() > 1 && !first.is_empty(), "{found}");
    h.get_by_label(&label(other[0])).click();
    h.run_steps(2);
    assert!(h.get_by_label(&label(other[0])).is_focused(), "a click leaves focus on the row");
    h.input_mut().events.push(Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(4);
    let doc = h.state().session.active().expect("a workbook");
    assert_eq!(doc.wb.active().map(|sh| sh.name.as_str()), Some("Other"), "the focused result's sheet");
    let areas: Vec<String> = doc.selection.ranges.iter().map(|r| r.a1()).collect();
    let expected: Vec<&str> = other.iter().filter_map(|r| r["cell"].as_str()).collect();
    assert_eq!(areas, expected, "every result on that sheet, each its own area");
    let shown_selected = |h: &Harness, r: &serde_json::Value| h.get_by_label(&label(r)).accesskit_node().toggled() == Some(Toggled::True);
    for r in &other {
        assert!(shown_selected(&h, r), "{} shows as selected", label(r));
    }
    for r in &first {
        assert!(!shown_selected(&h, r), "{} is on another sheet", label(r));
    }
    let status = h.get_by_label_contains("cell(s) selected");
    assert!(status.accesskit_node().value().is_some_and(|l| l.contains(&other.len().to_string())), "the count of selected cells");
    assert_eq!(status.accesskit_node().live(), Live::Polite, "the count is announced");
}

/// In the Find All list, Enter on a focused result goes to that result rather than running Find
/// Next, and Ctrl+click can't take out the last selected result, so the list and the grid agree
/// on what is selected (#4874).
#[test]
fn enter_and_ctrl_click_on_a_find_all_result_keep_the_list_and_grid_in_step() {
    use egui::accesskit::Toggled;
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut h = harness();
    let app = h.state_mut();
    app.session.execute("range.setValues", json!({"range": "A1:A3", "values": [["pear"], ["pear"], ["pear"]]})).unwrap();
    h.run_steps(2);
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(6);
    h.input_mut().events.push(Event::Text("pear".into()));
    h.run_steps(4);
    h.get_by_label("Find All").click();
    h.run_steps(4);
    let row = h.get_by_label_contains("  A2  ");
    let name = row.accesskit_node().label().map(|l| l.to_string()).unwrap_or_default();
    row.click();
    h.run_steps(2);
    let active = |h: &Harness| h.state().session.active().map(|d| d.selection.active.a1());
    assert_eq!(active(&h).as_deref(), Some("A2"));
    h.input_mut().events.push(Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.input_mut().events.push(Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(4);
    assert_eq!(active(&h).as_deref(), Some("A2"), "Enter on the result stays on it instead of finding the next hit");
    h.get_by_label(&name).click_modifiers(CTRL);
    h.run_steps(4);
    assert_eq!(h.get_by_label(&name).accesskit_node().toggled(), Some(Toggled::True), "the only selected result stays selected");
    let sel = h.state().session.active().map(|d| d.selection.a1());
    assert_eq!(sel.as_deref(), Some("A2"), "and the grid still selects its cell");
}
