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

fn cell_value(h: &Harness, a1: &str) -> String {
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    sheet.value(gridcraft_engine::core::CellRef::parse(a1).unwrap()).display()
}

fn find_what(h: &Harness) -> Option<serde_json::Value> {
    h.state().dialog.as_ref().filter(|d| d.name == "find").and_then(|d| d.values.get("what").cloned())
}

fn type_keys(h: &mut Harness, text: &str) {
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(1);
    }
}

fn press(h: &mut Harness, key: Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn click_at(h: &mut Harness, pos: egui::Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.extend([
            Event::PointerMoved(pos),
            Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE },
        ]);
        h.step();
    }
    h.run_steps(3);
}

fn cell_center(h: &Harness, a1: &str) -> egui::Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = gridcraft_ui_egui::grid::Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, gridcraft_engine::core::CellRef::parse(a1).unwrap()).center()
}

/// #4587: Find and Replace is modeless, as in Excel. With it open, a click on the sheet gives the
/// grid the keyboard: typing edits the clicked cell and Enter commits it, and the dialog stays
/// open with its search. With the cursor back in "Find what", typing goes there, not to a cell.
#[test]
fn the_sheet_takes_typing_while_find_is_open() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: Key::F, physical_key: Some(Key::F), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(6);
    type_keys(&mut h, "pear");
    assert_eq!(find_what(&h), Some(json!("pear")));

    let b2 = cell_center(&h, "B2");
    click_at(&mut h, b2);
    type_keys(&mut h, "42");
    press(&mut h, Key::Enter);
    assert_eq!(cell_value(&h, "B2"), "42", "the clicked cell took the typing and Enter");
    assert_eq!(find_what(&h), Some(json!("pear")), "the dialog stays open, its search unchanged");
    // Enter on the sheet moves down; it isn't also the dialog's Find Next (no "not found" box).
    let before = h.state().session.active().unwrap().selection.active;
    press(&mut h, Key::Enter);
    assert!(h.state().session.active().unwrap().selection.active.row > before.row);
    assert!(h.state().message.is_none(), "Find Next didn't run");

    let field = h.get_by_label("Find what:").rect().center();
    click_at(&mut h, field);
    type_keys(&mut h, "s");
    assert_eq!(find_what(&h), Some(json!("pears")), "typing in Find what goes there");
    assert_eq!(cell_value(&h, "B3"), "", "and edits no cell");
    assert!(h.state().editor.is_none(), "nor opens one");
}
