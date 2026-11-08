//! File › Options › Linux display server (#3140): Automatic, Wayland or X11 (XWayland), kept in
//! `ui.json` for the desktop app to read before its window opens.

use gridcraft_engine::Session;
use gridcraft_ui_egui::control::{self, Outcome};
use gridcraft_ui_egui::{DisplayServer, SheetApp, UiState};
use serde_json::{Value, json};

/// No workbook open: the setting doesn't need one.
fn app() -> SheetApp {
    crate::common::app(Session::new())
}

fn reply(outcome: Outcome) -> Value {
    match outcome {
        Outcome::Done(v) => v,
        _ => Value::Null,
    }
}

#[test]
fn the_display_server_is_a_setting_that_survives_a_restart() {
    let mut a = app();
    assert_eq!(a.run("options.displayServer", json!({})), Ok(json!({"displayServer": "auto"})), "Automatic by default; no value reads it");
    assert_eq!(a.run("options.displayServer", json!({"value": " X11 "})), Ok(json!({"displayServer": "x11"})));
    for junk in [json!({"value": "mir"}), json!({"value": 1}), json!({"value": null}), json!({"value": ["wayland"]})] {
        assert!(a.run("options.displayServer", junk.clone()).is_err(), "{junk}");
    }
    assert_eq!(a.ui.display_server, DisplayServer::X11, "a refused value keeps the setting");
    assert!(a.dialog.is_none() && a.message.is_none(), "programmatic calls open nothing");
    let saved = serde_json::to_string(&a.ui).unwrap();
    assert!(saved.contains(r#""displayServer":"x11""#), "{saved}");
    assert_eq!(serde_json::from_str::<UiState>(&saved).unwrap().display_server, DisplayServer::X11);
    // A ui.json from before the setting, or with a value it doesn't know, keeps the other
    // preferences and reads as Automatic.
    for old in [r#"{"dark": true}"#, r#"{"dark": true, "displayServer": "mir"}"#, r#"{"dark": true, "displayServer": 7}"#] {
        let ui: UiState = serde_json::from_str(old).unwrap();
        assert!(ui.dark, "{old}");
        assert_eq!(ui.display_server, DisplayServer::Auto, "{old}");
    }
}

#[test]
fn agents_find_run_and_read_the_setting_over_the_control_channel() {
    let mut a = app();
    let ctx = egui::Context::default();
    let found = reply(control::handle(&mut a, &ctx, "engine.commands", &json!({"search": "display server"})));
    let entry = found["result"].as_array().and_then(|all| all.iter().find(|c| c["id"] == "options.displayServer")).cloned().unwrap();
    assert_eq!(entry["ui"], json!(true));
    let ran = reply(control::handle(&mut a, &ctx, "engine.execute", &json!({"command": "options.displayServer", "params": {"value": "wayland"}})));
    assert_eq!(ran, json!({"ok": true, "result": {"displayServer": "wayland"}}));
    assert_eq!(reply(control::handle(&mut a, &ctx, "ui.inspect", &json!({})))["result"]["ui"]["displayServer"], json!("wayland"));
}

/// File › Options offers the choice on Linux and the BSDs, also with no workbook open.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn file_options_offers_the_display_server() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    let mut h = crate::common::harness_with(crate::common::builder().with_size(egui::vec2(1200.0, 1400.0)), app(), 4);
    h.state_mut().run("ui.backstage", json!({"page": "options"})).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ComboBox, "Display server").click();
    h.run_steps(3);
    h.get_by_label("Wayland").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.display_server, DisplayServer::Wayland);
    assert!(h.query_by_label("Applies the next time GridCraft starts.").is_some());
}
