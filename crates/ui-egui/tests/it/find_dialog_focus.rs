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

#[test]
fn a_refused_replacement_shows_its_error_and_keeps_the_match_selected() {
    use egui_kittest::kittest::Queryable as _;
    use gridcraft_engine::core::CellRef;

    let mut h = harness();
    let app = h.state_mut();
    app.session.execute("range.setValues", json!({"range": "A1:A2", "values": [["pear"], ["pear"]]})).unwrap();
    app.session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    app.session.execute("review.protectSheet", json!({})).unwrap();
    app.open_dialog("find", json!({"replace": true}));
    let dialog = app.dialog.as_mut().unwrap();
    dialog.values.insert("what".into(), json!("pear"));
    dialog.values.insert("with".into(), json!("fig"));
    h.run_steps(4);
    // The tab and action have the same label; the action is the last one.
    h.get_all_by_label("Replace").last().expect("Replace action").click();
    h.run_steps(4);
    assert!(h.state().message.as_ref().is_some_and(|(title, message)| title == "Find and Replace" && message.contains("protected sheet")));
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.selection.active, CellRef::new(0, 0), "a refused replacement must not find the next match");
    assert_eq!(doc.wb.active().unwrap().input_text(CellRef::new(0, 0)), "pear");
    assert_eq!(h.state().dialog.as_ref().unwrap().name, "find");
}
