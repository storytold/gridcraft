//! Interface size preference (#230): a persistent zoom for the whole window, clamped.

use egui::{Event, Key, Modifiers, os::OperatingSystem};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, UiState};
use serde_json::json;

#[test]
fn interface_size_applies_persists_and_clamps() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(2);
    assert_eq!(h.ctx.zoom_factor(), 1.0);

    h.state_mut().run("view.interfaceScale", json!({"percent": 125})).unwrap();
    h.run_steps(2);
    assert_eq!(h.ctx.zoom_factor(), 1.25);
    let saved: UiState = serde_json::from_str(&serde_json::to_string(&h.state().ui).unwrap()).unwrap();
    assert_eq!(saved.interface_scale, 125);

    // Hostile and missing values: clamped or refused, never applied raw.
    assert_eq!(h.state_mut().run("view.interfaceScale", json!({"percent": 1e12})).unwrap()["percent"], 200);
    assert_eq!(h.state_mut().run("view.interfaceScale", json!({"percent": -5})).unwrap()["percent"], 75);
    assert!(h.state_mut().run("view.interfaceScale", json!({})).is_err(), "no value is an error, not a dialog");
    assert!(h.state_mut().run("view.interfaceScale", json!({"percent": "huge"})).is_err());
    h.state_mut().run("view.interfaceScale", json!({"percent": "auto"})).unwrap();
    h.run_steps(2);
    assert_eq!(h.ctx.zoom_factor(), 1.0);

    // A malformed or out-of-range ui.json keeps the other preferences.
    let ui: UiState = serde_json::from_str(r#"{"dark": true, "interfaceScale": "big"}"#).unwrap();
    assert!(ui.dark);
    assert_eq!(ui.interface_scale, 100);
    let ui: UiState = serde_json::from_str(r#"{"interfaceScale": 900}"#).unwrap();
    assert_eq!(ui.interface_scale, 200);
}

type Harness = egui_kittest::Harness<'static, SheetApp>;
const CTRL: Modifiers = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
const CMD: Modifiers = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };

fn small_harness(os: OperatingSystem, ui_state: UiState, render: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "Synthetic recovery test"})).unwrap();
    session.execute("view.zoom", json!({"percent": 125})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    app.ui = ui_state;
    let builder = Harness::builder().with_os(os).with_size(egui::vec2(640.0, 420.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn oversized(os: OperatingSystem) -> Harness {
    let mut h = small_harness(os, UiState::default(), false);
    h.state_mut().run("view.interfaceScale", json!({"percent": 200})).unwrap();
    h.run_steps(3);
    h
}

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers });
    }
    h.run_steps(3);
}

fn reset(h: &mut Harness, command: Modifiers) {
    key(h, Key::F8, Modifiers { shift: true, ..command });
    assert_eq!(h.state().ui.interface_scale, 100);
    assert_eq!(h.ctx.zoom_factor(), 1.0);
}

#[test]
fn keyboard_recovery_at_200_percent_keeps_workbook_and_sheet_zoom() {
    for (os, command) in [(OperatingSystem::Mac, CMD), (OperatingSystem::Windows, CTRL), (OperatingSystem::Nix, CTRL)] {
        let mut h = oversized(os);
        assert_eq!(h.ctx.zoom_factor(), 2.0);
        let before = h.state().session.active().unwrap().wb.clone();
        reset(&mut h, command);
        assert_eq!(h.state().session.active().unwrap().wb, before);
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().zoom, 125);
        assert!(!h.ctx.options(|o| o.zoom_with_keyboard));
        assert!(h.state().editor.is_none());
    }
}

#[test]
fn recovery_preserves_cell_and_formula_bar_edits_and_their_shortcuts() {
    for from_formula_bar in [false, true] {
        let mut h = oversized(OperatingSystem::Mac);
        h.state_mut().run("cell.set", json!({"cell": "A1", "input": "=A2+12"})).unwrap();
        // Edit mode keeps arrows in the editor; typing/Enter mode commits on ArrowLeft.
        h.state_mut().begin_edit(None, from_formula_bar);
        h.run_steps(3);
        h.input_mut().events.push(Event::Text("3".into()));
        h.run_steps(2);
        key(&mut h, Key::ArrowLeft, Modifiers::NONE);
        let before = h.state().editor.clone().unwrap();
        assert_eq!(before.text, "=A2+123");
        let workbook = h.state().session.active().unwrap().wb.clone();
        reset(&mut h, CMD);
        let after = h.state().editor.as_ref().unwrap();
        assert_eq!(
            (after.text.as_str(), after.caret, after.cell, after.from_formula_bar),
            (before.text.as_str(), before.caret, before.cell, before.from_formula_bar)
        );
        assert_eq!(h.state().session.active().unwrap().wb, workbook);
        // Editing still owns ordinary keys after recovery: Escape cancels without a commit.
        key(&mut h, Key::Escape, Modifiers::NONE);
        assert!(h.state().editor.is_none());
        assert_eq!(h.state().session.active().unwrap().wb, workbook);
    }
}

#[test]
fn recovery_preserves_a_text_box_draft() {
    let mut h = oversized(OperatingSystem::Mac);
    let id = h.state_mut().run("insert.textBox", json!({"at": "A1", "width": 180, "height": 60, "text": "Saved text"})).unwrap()["shape"]
        .as_u64()
        .unwrap() as u32;
    assert!(h.state_mut().begin_text_box_edit(id));
    h.state_mut().text_box_editor.as_mut().unwrap().text = "Unfinished draft".into();
    h.run_steps(3);
    let workbook = h.state().session.active().unwrap().wb.clone();
    reset(&mut h, CMD);
    let editor = h.state().text_box_editor.as_ref().unwrap();
    assert_eq!((editor.id, editor.text.as_str()), (id, "Unfinished draft"));
    assert_eq!(h.state().session.active().unwrap().wb, workbook);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().text_box_editor.is_none());
    assert_eq!(h.state().session.active().unwrap().wb, workbook);
}

#[test]
fn recovery_is_available_in_dialogs_messages_and_backstage() {
    for mode in ["dialog", "message", "backstage"] {
        let mut h = oversized(OperatingSystem::Mac);
        match mode {
            "dialog" => h.state_mut().open_dialog("commandSearch", json!({})),
            "message" => h.state_mut().message = Some(("Synthetic message".into(), "Keep this open".into())),
            "backstage" => {
                h.state_mut().run("ui.backstage", json!({"page": "options"})).unwrap();
            }
            _ => unreachable!(),
        }
        h.run_steps(3);
        if mode == "dialog" {
            h.input_mut().events.push(Event::Text("search text".into()));
            h.run_steps(2);
            assert!(h.ctx.text_edit_focused());
            assert_eq!(h.state().dialog.as_ref().unwrap().search, "search text");
        }
        let before = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
        reset(&mut h, CMD);
        let after = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
        for field in ["dialog", "message", "backstage"] {
            assert_eq!(after[field], before[field], "{mode}: {field}");
        }
        if mode == "dialog" {
            assert_eq!(h.state().dialog.as_ref().unwrap().search, "search text");
        }
    }
}

#[test]
fn recovery_preserves_a_filter_search_and_its_enter_action() {
    let mut h = oversized(OperatingSystem::Mac);
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["Fruit"], ["Apple"], ["Banana"], ["Cherry"]]})).unwrap();
    h.state_mut().run("data.filter", json!({"range": "A1:A4"})).unwrap();
    h.run_steps(3);
    key(&mut h, Key::ArrowDown, Modifiers::ALT);
    assert!(h.state().grid.filter_menu.is_some());
    h.input_mut().events.push(Event::Text("an".into()));
    h.run_steps(2);
    let search = egui::Id::new(("filter_search", 0_u32));
    assert_eq!(h.ctx.data_mut(|d| d.get_temp::<String>(search)).as_deref(), Some("an"));
    let workbook = h.state().session.active().unwrap().wb.clone();
    reset(&mut h, CMD);
    assert!(h.state().grid.filter_menu.is_some());
    assert_eq!(h.ctx.data_mut(|d| d.get_temp::<String>(search)).as_deref(), Some("an"));
    assert_eq!(h.state().session.active().unwrap().wb, workbook);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().zoom, 125);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().grid.filter_menu.is_none());
    let document = h.state().session.active().unwrap();
    assert_eq!(document.selection.active.a1(), "A1");
    let sheet = document.wb.active().unwrap();
    assert_eq!((1..=3).map(|row| sheet.is_row_hidden(row)).collect::<Vec<_>>(), [true, false, true]);
}

#[test]
fn recovery_cancels_keytips_and_leaves_other_shortcuts_available() {
    for (os, command) in [(OperatingSystem::Mac, CMD), (OperatingSystem::Windows, CTRL)] {
        let mut h = oversized(os);
        key(&mut h, Key::F10, Modifiers::NONE);
        key(&mut h, Key::W, Modifiers::NONE);
        assert_eq!(h.state().keytips.prefix(), Some("W"));
        reset(&mut h, command);
        assert_eq!(h.state().keytips.prefix(), None);
        assert!(h.state().editor.is_none());
        key(&mut h, Key::Num0, command);
        assert!(h.state().session.active().unwrap().wb.active().unwrap().is_col_hidden(0));
        key(&mut h, Key::Num0, Modifiers { shift: true, ..command });
        assert!(!h.state().session.active().unwrap().wb.active().unwrap().is_col_hidden(0));
        key(&mut h, Key::Equals, Modifiers { alt: true, ..command });
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().zoom, 135);
        assert_eq!(h.state().ui.interface_scale, 100);
        key(&mut h, Key::F1, command);
        assert!(h.state().ui.ribbon_collapsed);
        key(&mut h, Key::F10, Modifiers::NONE);
        key(&mut h, Key::H, Modifiers::NONE);
        assert_eq!(h.state().keytips.prefix(), Some("H"));
        assert_eq!(h.state().ui.ribbon_tab, "Home");
    }
}

#[test]
fn recovery_closes_a_keytip_opened_popup() {
    let mut h = oversized(OperatingSystem::Mac);
    for k in [Key::F10, Key::H, Key::V] {
        key(&mut h, k, Modifiers::NONE);
    }
    assert_eq!(h.state().keytips.prefix(), Some("HV"));
    assert!(egui::Popup::is_any_open(&h.ctx));
    reset(&mut h, CMD);
    assert_eq!(h.state().keytips.prefix(), None);
    assert!(!egui::Popup::is_any_open(&h.ctx));
}

#[test]
fn recovery_requires_exact_platform_modifiers_a_fresh_press_and_focus() {
    for (os, command, wrong_command) in [(OperatingSystem::Mac, CMD, CTRL), (OperatingSystem::Windows, CTRL, CMD)] {
        let mut h = oversized(os);
        for modifiers in [
            Modifiers::NONE,
            Modifiers::SHIFT,
            command,
            Modifiers { shift: true, alt: true, ..command },
            Modifiers { shift: true, ..wrong_command },
            Modifiers { shift: true, ctrl: true, mac_cmd: true, command: true, ..Modifiers::NONE },
        ] {
            key(&mut h, Key::F8, modifiers);
            assert_eq!(h.state().ui.interface_scale, 200, "{os:?}: {modifiers:?}");
        }
        let modifiers = Modifiers { shift: true, ..command };
        // egui derives repeat from keys_down, not the incoming Event::Key.repeat flag.
        h.input_mut().events.push(Event::Key { key: Key::F8, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(1);
        h.input_mut().events.push(Event::Key { key: Key::F8, physical_key: None, pressed: true, repeat: true, modifiers });
        h.run_steps(2);
        assert_eq!(h.state().ui.interface_scale, 200);
        h.input_mut().events.push(Event::Key { key: Key::F8, physical_key: None, pressed: false, repeat: false, modifiers });
        h.run_steps(1);
        h.input_mut().focused = false;
        key(&mut h, Key::F8, modifiers);
        assert_eq!(h.state().ui.interface_scale, 200);
        h.input_mut().focused = true;
        reset(&mut h, command);
    }
}

#[test]
fn keyboard_reset_survives_preference_reload() {
    let mut h = oversized(OperatingSystem::Mac);
    reset(&mut h, CMD);
    let persisted = serde_json::to_vec_pretty(&h.state().ui).unwrap();
    drop(h);
    let restored = small_harness(OperatingSystem::Mac, serde_json::from_slice(&persisted).unwrap(), false);
    assert_eq!(restored.state().ui.interface_scale, 100);
    assert_eq!(restored.ctx.zoom_factor(), 1.0);
}

/// Opt-in visual evidence uses only a synthetic workbook and a scratch preference file.
fn save_frame(h: &mut Harness, path: &std::path::Path) {
    // A font rebind intentionally skips one paint. Finish pending repaints before capture.
    h.ctx.request_repaint();
    h.run();
    let image = h.render().unwrap();
    let background = *image.get_pixel(0, 0);
    let painted = image.pixels().filter(|pixel| **pixel != background).count();
    assert!(painted > (image.width() * image.height() / 4) as usize, "screenshot must contain painted UI");
    image.save(path).unwrap();
}

#[test]
#[ignore = "requires a GPU; set GRIDCRAFT_SCALE_EVIDENCE to a scratch output directory"]
fn interface_recovery_screenshots_and_preference_file_reload() {
    let dir = std::path::PathBuf::from(std::env::var_os("GRIDCRAFT_SCALE_EVIDENCE").expect("scratch output directory"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = small_harness(OperatingSystem::Mac, UiState::default(), true);
    h.state_mut().run("view.interfaceScale", json!({"percent": 200})).unwrap();
    h.run_steps(3);
    assert_eq!(h.ctx.zoom_factor(), 2.0);
    save_frame(&mut h, &dir.join("interface-200.png"));
    reset(&mut h, CMD);
    save_frame(&mut h, &dir.join("interface-recovered.png"));
    std::fs::write(dir.join("ui.json"), serde_json::to_vec_pretty(&h.state().ui).unwrap()).unwrap();
    drop(h);
    // Restart the test executable after dropping the renderer. The child gets only the scratch
    // preferences, not this process's egui context or in-memory SheetApp state.
    let marker = dir.join("interface-restart-process.json");
    if marker.try_exists().unwrap() {
        std::fs::remove_file(&marker).unwrap();
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "interface_recovery_reload_in_new_process", "--ignored", "--nocapture", "--test-threads=1"])
        .env("GRIDCRAFT_SCALE_EVIDENCE", &dir)
        .env("GRIDCRAFT_SCALE_PARENT_PID", std::process::id().to_string())
        .output()
        .unwrap();
    std::fs::write(dir.join("restart-child.stdout.log"), &output.stdout).unwrap();
    std::fs::write(dir.join("restart-child.stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "restart child failed: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence: serde_json::Value = serde_json::from_slice(&std::fs::read(marker).unwrap()).unwrap();
    assert_eq!(evidence["parentPid"], std::process::id());
    assert_ne!(evidence["childPid"], std::process::id());
    assert_eq!(evidence["interfaceScale"], 100);
    assert_eq!(evidence["zoomFactor"], 1.0);
}

/// Invoked only by the visual test above, in a separate executable process.
#[test]
#[ignore = "restart child for interface_recovery_screenshots_and_preference_file_reload"]
fn interface_recovery_reload_in_new_process() {
    // Running all ignored tests must not execute this helper outside its parent scenario.
    let Ok(parent_pid) = std::env::var("GRIDCRAFT_SCALE_PARENT_PID") else { return };
    let parent_pid: u32 = parent_pid.parse().unwrap();
    let dir = std::path::PathBuf::from(std::env::var_os("GRIDCRAFT_SCALE_EVIDENCE").expect("scratch output directory"));
    assert_ne!(std::process::id(), parent_pid);
    let restored: UiState = serde_json::from_slice(&std::fs::read(dir.join("ui.json")).unwrap()).unwrap();
    let mut h = small_harness(OperatingSystem::Mac, restored, true);
    assert_eq!(h.state().ui.interface_scale, 100);
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    save_frame(&mut h, &dir.join("interface-restarted.png"));
    std::fs::write(
        dir.join("interface-restart-process.json"),
        serde_json::to_vec_pretty(&json!({
            "parentPid": parent_pid,
            "childPid": std::process::id(),
            "interfaceScale": h.state().ui.interface_scale,
            "zoomFactor": h.ctx.zoom_factor(),
        }))
        .unwrap(),
    )
    .unwrap();
}
