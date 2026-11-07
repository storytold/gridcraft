//! Excel 2003 menu access keys that open a ribbon control rather than run a command (#941).

use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn press(h: &mut egui_kittest::Harness<'static, SheetApp>, key: Key) {
    let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([ev(true), Event::Text(key.name().to_ascii_lowercase()), ev(false)]);
    h.run_steps(2);
}

#[test]
fn alt_i_s_opens_the_symbol_gallery_and_leaves_the_cell_alone() {
    // Insert > Symbol picks a symbol; running insert.symbol with no symbol set the active cell
    // to its own text again, re-reading it (a Python cell lost its code, text could turn into a
    // number). From the Home tab the keys show the Insert tab's gallery instead.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "'007"})).unwrap();
    let mut h = crate::common::os_harness(s, OperatingSystem::Nix);
    h.run_steps(4);
    for key in [Key::F10, Key::I, Key::S] {
        press(&mut h, key);
    }
    h.run_steps(4);
    assert!(!h.state().session.journal.iter().any(|(id, _)| id == "insert.symbol"), "nothing is inserted until a symbol is picked");
    assert_eq!(h.state().ui.ribbon_tab, "Insert");
    assert!(h.query_by_label("€").is_some(), "the Symbol gallery is open");
}

#[test]
fn alt_i_s_expands_a_collapsed_ribbon_on_the_insert_tab() {
    // With the Insert tab already chosen but the ribbon collapsed, the gallery's control isn't
    // drawn: the keys expand the ribbon first.
    let mut s = Session::new();
    s.new_workbook();
    let mut h = crate::common::os_harness(s, OperatingSystem::Nix);
    h.state_mut().run("ui.ribbonTab", json!({"tab": "Insert"})).unwrap();
    h.state_mut().ui.ribbon_collapsed = true;
    h.run_steps(4);
    for key in [Key::F10, Key::I, Key::S] {
        press(&mut h, key);
    }
    h.run_steps(4);
    assert!(!h.state().ui.ribbon_collapsed);
    assert!(h.query_by_label("€").is_some(), "the Symbol gallery is open");
    assert!(!h.state().session.journal.iter().any(|(id, _)| id == "insert.symbol"));
}

#[test]
fn a_waiting_key_tip_request_stays_with_its_window() {
    // A request waiting for the frame that draws its control belongs to the window whose keys
    // made it: another window drawn first must not take it.
    let mut s = Session::new();
    s.new_workbook();
    let mut h = crate::common::os_harness(s, OperatingSystem::Nix);
    h.run_steps(2);
    let first = h.state().session.active_window().unwrap();
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    let second = h.state().session.active_window().unwrap();
    h.state_mut().show_window(second);
    h.state_mut().ui.keytip_invoke_next = Some("insert.symbol");
    h.state_mut().show_window(first);
    assert_eq!(h.state().ui.keytip_invoke_next, None, "the first window doesn't see the second window's request");
    h.state_mut().show_window(second);
    assert_eq!(h.state().ui.keytip_invoke_next, Some("insert.symbol"));
}

#[test]
fn a_hidden_window_drops_its_waiting_key_tip_request() {
    // A hidden window isn't drawn, so its waiting request would fire when it was shown again,
    // long after the keys: hiding the window drops it.
    let mut s = Session::new();
    s.new_workbook();
    let mut h = crate::common::os_harness(s, OperatingSystem::Nix);
    h.run_steps(2);
    let first = h.state().session.active_window().unwrap();
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    let second = h.state().session.active_window().unwrap();
    h.state_mut().show_window(second);
    h.state_mut().ui.keytip_invoke_next = Some("insert.symbol");
    h.state_mut().run("view.hideWindow", json!({"window": second})).unwrap();
    h.state_mut().show_window(first);
    h.state_mut().run("view.unhideWindow", json!({"window": second})).unwrap();
    h.state_mut().show_window(second);
    assert_eq!(h.state().ui.keytip_invoke_next, None);
}

#[test]
fn a_window_hidden_from_another_window_drops_its_parked_request() {
    // Hidden while another window is in front (its request already parked), the window still
    // loses the request, so showing it again doesn't open the control.
    let mut s = Session::new();
    s.new_workbook();
    let mut h = crate::common::os_harness(s, OperatingSystem::Nix);
    h.run_steps(2);
    let first = h.state().session.active_window().unwrap();
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    let second = h.state().session.active_window().unwrap();
    h.state_mut().show_window(second);
    h.state_mut().ui.keytip_invoke_next = Some("insert.symbol");
    h.state_mut().enter_window(first);
    h.state_mut().run("view.hideWindow", json!({"window": second})).unwrap();
    h.state_mut().show_window(first);
    h.state_mut().run("view.unhideWindow", json!({"window": second})).unwrap();
    h.state_mut().show_window(second);
    assert_eq!(h.state().ui.keytip_invoke_next, None);
}
