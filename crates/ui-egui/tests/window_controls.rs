//! GNOME on Wayland: the title bar carries the window buttons until File ▸ Options ▸ Use system
//! title bar gives the window back its frame, at once and without a restart. The choice is saved;
//! desktops that draw the frame themselves don't offer it.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::window_controls::{Button, Layout};
use gridcraft_ui_egui::{SheetApp, UiState};
use serde_json::json;

fn harness(controls: Option<Layout>) -> Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.window_controls = controls;
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn decorations(h: &Harness<'static, SheetApp>) -> Option<bool> {
    h.output().viewport_output.values().flat_map(|v| v.commands.iter()).find_map(|c| match c {
        egui::ViewportCommand::Decorations(on) => Some(*on),
        _ => None,
    })
}

#[test]
fn use_system_title_bar_swaps_the_header_bar_buttons_for_the_window_frame() {
    let mut h = harness(Some(Layout::default()));
    // The window opened undecorated, so the first frames ask for nothing.
    assert_eq!(decorations(&h), None);
    for label in ["Minimize", "Maximize", "Close"] {
        assert!(h.query_by_role_and_label(Role::Button, label).is_some(), "the title bar has {label}");
    }
    h.state_mut().run("ui.backstage", json!({"page": "options"})).unwrap();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Use system title bar").click();
    let mut asked = None;
    for _ in 0..4 {
        h.step();
        asked = asked.or(decorations(&h));
    }
    assert!(h.state().ui.system_title_bar);
    assert_eq!(asked, Some(true), "the window gets its frame back");
    assert!(h.query_by_role_and_label(Role::Button, "Minimize").is_none(), "and the title bar loses its buttons");
    // Saved with the other interface preferences.
    let saved = serde_json::to_string(&h.state().ui).unwrap();
    assert!(saved.contains(r#""systemTitleBar":true"#), "{saved}");
    let restored: UiState = serde_json::from_str(&saved).unwrap();
    assert!(restored.system_title_bar);
    // The command turns it back off (and toggles without a parameter).
    assert_eq!(h.state_mut().run("view.systemTitleBar", json!({"on": false})).unwrap(), json!({"on": false, "appDrawsFrame": true}));
    let mut asked = None;
    for _ in 0..4 {
        h.step();
        asked = asked.or(decorations(&h));
    }
    assert_eq!(asked, Some(false), "the frame goes again");
    assert!(h.query_by_role_and_label(Role::Button, "Minimize").is_some());
}

/// GNOME's `button-layout` decides which buttons appear: Fedora's default shows only close.
#[test]
fn the_desktop_button_layout_decides_the_buttons() {
    let h = harness(Some(Layout { left: vec![], right: vec![Button::Close] }));
    assert!(h.query_by_role_and_label(Role::Button, "Close").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Minimize").is_none());
    assert!(h.query_by_role_and_label(Role::Button, "Maximize").is_none());
}

/// Where the desktop draws the frame there are no buttons and nothing to choose.
#[test]
fn desktops_that_draw_the_frame_are_left_alone() {
    let mut h = harness(None);
    assert!(h.query_by_role_and_label(Role::Button, "Minimize").is_none());
    h.state_mut().run("ui.backstage", json!({"page": "options"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Use system title bar").is_none());
    assert_eq!(decorations(&h), None);
    assert_eq!(h.state_mut().run("view.systemTitleBar", json!({"on": true})).unwrap(), json!({"on": true, "appDrawsFrame": false}));
    h.run_steps(2);
    assert_eq!(decorations(&h), None);
}
