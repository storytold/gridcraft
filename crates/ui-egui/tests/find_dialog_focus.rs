//! Opening Find and Replace puts the cursor in "Find what", so the user can type at once.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
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
