//! File › Options › Display server, on Linux and the BSDs (#3140): Automatic, Wayland or X11
//! (XWayland), kept in `ui.json` for the desktop app to read before its window opens.

use gridcraft_engine::Session;
use gridcraft_ui_egui::control::{self, Outcome};
use gridcraft_ui_egui::{DisplayServer, SheetApp, UiState};
use serde_json::{Value, json};

/// No workbook open: the setting doesn't need one.
fn app() -> SheetApp {
    crate::common::app(Session::new())
}

/// The JSON a control call answered with; `Null` for a screenshot request.
fn reply(outcome: Outcome) -> Value {
    match outcome {
        Outcome::Done(v) => v,
        _ => Value::Null,
    }
}

/// `options.displayServer` reads and sets the choice, refuses unknown values, and the choice
/// survives a save and load of `ui.json`.
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
    for junk in [
        "",
        r#""displayServer": "mir""#,
        r#""displayServer": 7"#,
        r#""displayServer": null"#,
        r#""displayServer": true"#,
        r#""displayServer": ["wayland"]"#,
        r#""displayServer": {"value": "x11"}"#,
    ] {
        let old = if junk.is_empty() { r#"{"dark": true}"#.to_string() } else { format!(r#"{{"dark": true, {junk}}}"#) };
        let ui: UiState = serde_json::from_str(&old).unwrap();
        assert!(ui.dark, "{old}");
        assert_eq!(ui.display_server, DisplayServer::Auto, "{old}");
    }
    // A hand-edited value reads in any case and with spaces around it.
    for (written, server) in
        [("WAYLAND", DisplayServer::Wayland), (" wayland ", DisplayServer::Wayland), ("X11", DisplayServer::X11), (" Auto", DisplayServer::Auto)]
    {
        let ui: UiState = serde_json::from_value(json!({"dark": true, "displayServer": written})).unwrap();
        assert!(ui.dark, "{written:?}");
        assert_eq!(ui.display_server, server, "{written:?}");
    }
}

/// The command is listed in `engine.commands`, runs through `engine.execute` and shows in
/// `ui.inspect`.
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
    // An unknown value is an error over the channel too and leaves the setting alone; a call
    // without a value reads it.
    let refused = reply(control::handle(&mut a, &ctx, "engine.execute", &json!({"command": "options.displayServer", "params": {"value": "mir"}})));
    assert!(refused["error"].as_str().is_some_and(|e| e.contains("unknown display server")), "{refused}");
    assert_eq!(a.ui.display_server, DisplayServer::Wayland);
    let read = reply(control::handle(&mut a, &ctx, "engine.execute", &json!({"command": "options.displayServer"})));
    assert_eq!(read, json!({"ok": true, "result": {"displayServer": "wayland"}}));
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
    assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Display server").value().as_deref(), Some("Automatic"));
    for (choice, server) in [("Wayland", DisplayServer::Wayland), ("X11 (XWayland)", DisplayServer::X11)] {
        h.get_by_role_and_label(Role::ComboBox, "Display server").click();
        h.run_steps(3);
        h.get_by_label(choice).click();
        h.run_steps(3);
        assert_eq!(h.state().ui.display_server, server, "{choice}");
        assert_eq!(h.get_by_role_and_label(Role::ComboBox, "Display server").value().as_deref(), Some(choice));
    }
    // The page says why to pick one, that an unavailable choice falls back, and when it applies.
    assert!(h.query_by_label("Wayland sizes the window for scaled (HiDPI) displays; X11 (XWayland) keeps file drag-and-drop.").is_some());
    assert!(h.query_by_label("A choice that isn't available falls back to the other. Applies the next time GridCraft starts.").is_some());
}
