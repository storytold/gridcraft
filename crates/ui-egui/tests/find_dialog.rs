//! Find and Replace: Ctrl+F puts the caret in "Find what", so the search can be typed at once.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["North"], ["South"], ["East"]]})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(3);
}

fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn what(h: &Harness) -> String {
    let d = h.state().dialog.as_ref().expect("find dialog open");
    d.values.get("what").and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn active(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.active.a1()
}

#[test]
fn ctrl_f_focuses_find_what_and_enter_finds_next() {
    let mut h = harness();
    key(&mut h, Key::F, CTRL);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("find"));
    type_text(&mut h, "East");
    assert_eq!(what(&h), "East", "typing goes straight into Find what");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(active(&h), "A3");
    // The caret stays in the field: more typing still edits the search.
    type_text(&mut h, "x");
    assert_eq!(what(&h), "Eastx");
}

#[test]
fn reopening_selects_the_last_search_so_typing_replaces_it() {
    let mut h = harness();
    key(&mut h, Key::F, CTRL);
    type_text(&mut h, "South");
    key(&mut h, Key::Enter, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    key(&mut h, Key::H, CTRL);
    let d = h.state().dialog.as_ref().expect("reopened");
    assert_eq!(d.tab, "Replace");
    assert_eq!(what(&h), "South", "the last search comes back");
    type_text(&mut h, "North");
    assert_eq!(what(&h), "North", "the old text was selected, so typing replaced it");
}
