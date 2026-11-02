//! Form dialogs open ready to type, as in Excel: Alt+H, O, W then a number and Enter sets the
//! column width without clicking into the field first.

use egui::{Event, Key, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(6);
    h
}

fn type_and_enter(h: &mut egui_kittest::Harness<'static, SheetApp>, text: &str) {
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.key_press(Key::Enter);
    h.run_steps(6);
}

#[test]
fn column_width_opens_with_the_width_ready_to_type() {
    let mut h = harness();
    h.state_mut().open_dialog("columnWidth", json!({}));
    h.run_steps(6);
    type_and_enter(&mut h, "12");
    assert!(h.state().dialog.is_none(), "Enter confirmed the dialog");
    let chars = {
        let d = h.state().session.active().unwrap();
        let sheet = d.wb.active().unwrap();
        gridcraft_engine::cmd::format::points_to_chars(sheet.col_width(CellRef::parse("C3").unwrap().col) as f64)
    };
    assert!((chars - 12.0).abs() < 0.2, "column C is 12 characters wide, got {chars}");
}

#[test]
fn row_height_opens_with_the_height_ready_to_type() {
    let mut h = harness();
    h.state_mut().open_dialog("rowHeight", json!({}));
    h.run_steps(6);
    let shown = h.state().dialog.as_ref().and_then(|d| d.values.get("points")).and_then(|v| v.as_f64());
    assert!(shown.is_some_and(|v| (v - 15.0).abs() < 0.01), "a default row opens at 15 points, got {shown:?}");
    type_and_enter(&mut h, "30");
    assert!(h.state().dialog.is_none(), "Enter confirmed the dialog");
    let height = {
        let d = h.state().session.active().unwrap();
        d.wb.active().unwrap().row_height(CellRef::parse("C3").unwrap().row)
    };
    assert!((height as f64 - 40.0).abs() < 0.6, "row 3 is 30 points (40 pixels) high, got {height}");
}

#[test]
fn command_search_arrow_and_page_keys_move_the_selection_and_keep_the_search_focused() {
    let mut h = harness();
    h.state_mut().open_dialog("commandSearch", json!({}));
    h.run_steps(6);
    h.input_mut().events.push(Event::Text("a".into()));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.list_index), Some(0));
    for _ in 0..2 {
        h.key_press(Key::ArrowDown);
        h.run_steps(2);
    }
    assert_eq!(h.state().dialog.as_ref().map(|d| d.list_index), Some(2), "two ArrowDown presses");
    h.key_press(Key::PageDown);
    h.run_steps(2);
    let after_page = h.state().dialog.as_ref().map(|d| d.list_index).unwrap_or(0);
    assert!(after_page > 2, "PageDown moved past the third row, got {after_page}");
    assert!(h.ctx.memory(|m| m.focused()).is_some(), "the search field kept focus");
}

/// The palette displays the engine metadata unchanged on either runtime OS.
#[test]
fn command_search_shows_engine_shortcuts() {
    use egui::os::OperatingSystem;

    for os in [OperatingSystem::Windows, OperatingSystem::Mac] {
        let mut h = harness();
        h.ctx.set_os(os);
        h.run_steps(2);

        h.state_mut().open_dialog("commandSearch", json!({}));
        h.run_steps(6);

        for (query, expected) in [
            ("home.bold", if os == OperatingSystem::Mac { "Bold  (Cmd+B)" } else { "Bold  (Ctrl+B)" }),
            ("data.filter", "Filter  (Ctrl+Shift+L)"),
            ("edit.copy", if os == OperatingSystem::Mac { "Copy  (Cmd+C)" } else { "Copy  (Ctrl+C)" }),
            ("edit.paste", if os == OperatingSystem::Mac { "Paste  (Cmd+V)" } else { "Paste  (Ctrl+V)" }),
            ("home.italic", if os == OperatingSystem::Mac { "Italic  (Cmd+I)" } else { "Italic  (Ctrl+I)" }),
            ("home.underline", if os == OperatingSystem::Mac { "Underline  (Cmd+U)" } else { "Underline  (Ctrl+U)" }),
            ("edit.fillDown", if os == OperatingSystem::Mac { "Fill Down  (Cmd+D)" } else { "Fill Down  (Ctrl+D)" }),
            ("edit.flashFill", "Flash Fill  (Ctrl+E)"),
            ("home.alignLeft", if os == OperatingSystem::Mac { "Align Left  (Cmd+L)" } else { "Align Left    Home" }),
            ("home.alignCenter", if os == OperatingSystem::Mac { "Center  (Cmd+E)" } else { "Center    Home" }),
            ("selection.row", "Select Entire Row  (Shift+Space)"),
            ("selection.column", "Select Entire Column  (Ctrl+Space)"),
            ("data.group", if os == OperatingSystem::Mac { "Group  (Cmd+Shift+K)" } else { "Group  (Alt+Shift+Right)" }),
            ("data.ungroup", if os == OperatingSystem::Mac { "Ungroup  (Cmd+Shift+J)" } else { "Ungroup  (Alt+Shift+Left)" }),
        ] {
            h.state_mut().dialog.as_mut().unwrap().search = query.into();
            h.run_steps(3);
            h.get_by_label_contains(expected);
        }
    }
}

/// Paste Special uses the engine's shortcut metadata when enabled.
#[test]
fn command_search_shows_paste_special_metadata() {
    use egui::os::OperatingSystem;

    for os in [OperatingSystem::Windows, OperatingSystem::Mac] {
        let mut h = harness();
        h.ctx.set_os(os);
        h.run_steps(2);

        h.state_mut().session.execute("cell.set", json!({"cell": "C3", "input": "copied"})).unwrap();

        h.state_mut().session.execute("edit.copy", json!({"range": "C3"})).unwrap();

        h.state_mut().open_dialog("commandSearch", json!({}));
        h.run_steps(6);

        h.state_mut().dialog.as_mut().unwrap().search = "edit.pasteSpecial".into();
        h.run_steps(3);

        h.get_by_label_contains(if os == OperatingSystem::Mac { "Paste Special…  (Ctrl+Cmd+V)" } else { "Paste Special…  (Ctrl+Alt+V)" });
    }
}

/// The engine registry is the authoritative shortcut source.
#[test]
fn command_palette_shortcuts_come_from_engine_metadata() {
    let session = gridcraft_engine::Session::new();
    let commands = session.commands();

    for (id, mac, other) in [
        ("home.bold", Some("Cmd+B"), Some("Ctrl+B")),
        ("edit.selectAll", Some("Cmd+A"), Some("Ctrl+A")),
        ("edit.undo", Some("Cmd+Z"), Some("Ctrl+Z")),
        ("edit.redo", Some("Cmd+Y"), Some("Ctrl+Y")),
        ("edit.copy", Some("Cmd+C"), Some("Ctrl+C")),
        ("edit.cut", Some("Cmd+X"), Some("Ctrl+X")),
        ("edit.paste", Some("Cmd+V"), Some("Ctrl+V")),
        ("edit.fillDown", Some("Cmd+D"), Some("Ctrl+D")),
        ("edit.fillRight", Some("Cmd+R"), Some("Ctrl+R")),
        ("home.italic", Some("Cmd+I"), Some("Ctrl+I")),
        ("home.underline", Some("Cmd+U"), Some("Ctrl+U")),
        ("edit.pasteSpecial", Some("Ctrl+Cmd+V"), Some("Ctrl+Alt+V")),
        ("home.alignLeft", Some("Cmd+L"), None),
        ("home.alignCenter", Some("Cmd+E"), None),
        ("data.group", Some("Cmd+Shift+K"), Some("Alt+Shift+Right")),
        ("data.ungroup", Some("Cmd+Shift+J"), Some("Alt+Shift+Left")),
        ("edit.flashFill", Some("Ctrl+E"), Some("Ctrl+E")),
        ("data.filter", Some("Ctrl+Shift+L"), Some("Ctrl+Shift+L")),
        ("selection.row", Some("Shift+Space"), Some("Shift+Space")),
        ("selection.column", Some("Ctrl+Space"), Some("Ctrl+Space")),
    ] {
        let command = commands.iter().find(|c| c.id == id).unwrap();
        assert_eq!(command.shortcut_for_platform(true), mac, "{id}: macOS");
        assert_eq!(command.shortcut_for_platform(false), other, "{id}: Windows/Linux");
    }

    let command = commands.iter().find(|c| c.id == "home.alignRight").unwrap();
    assert_eq!(command.shortcut_for_platform(true), None);
    assert_eq!(command.shortcut_for_platform(false), None);
}
/// Current Region displays the shortcut declared in engine metadata.
#[test]
fn command_search_shows_current_region_shortcut() {
    use egui::os::OperatingSystem;

    for (os, expected) in
        [(OperatingSystem::Windows, "Select Current Region  (Ctrl+Shift+8)"), (OperatingSystem::Mac, "Select Current Region  (Ctrl+Shift+8)")]
    {
        let mut h = harness();
        h.ctx.set_os(os);
        h.run_steps(2);

        h.state_mut().open_dialog("commandSearch", json!({}));
        h.run_steps(6);

        h.state_mut().dialog.as_mut().unwrap().search = "selection.currentRegion".into();

        h.run_steps(3);
        h.get_by_label_contains(expected);
    }
}

/// Disabled commands are absent; commands without shortcuts show no invented keys.
#[test]
fn command_search_hides_disabled_and_handles_missing_shortcuts() {
    use egui::os::OperatingSystem;

    for os in [OperatingSystem::Windows, OperatingSystem::Mac] {
        let mut h = harness();
        h.ctx.set_os(os);
        h.run_steps(2);

        // Undo is disabled in a fresh workbook with no undo history.
        let undo = h.state().session.commands().into_iter().find(|c| c.id == "edit.undo").unwrap();

        assert!(!undo.enabled, "Undo should initially be disabled");

        h.state_mut().open_dialog("commandSearch", json!({}));
        h.run_steps(6);

        h.state_mut().dialog.as_mut().unwrap().search = "edit.undo".into();
        h.run_steps(3);

        assert!(
            h.query_by_label("Undo  (Cmd+Z)    Edit").is_none() && h.query_by_label("Undo  (Ctrl+Z)    Edit").is_none(),
            "Disabled Undo must not appear in the palette"
        );

        // Align Right has no shortcut on either platform.
        h.state_mut().dialog.as_mut().unwrap().search = "home.alignRight".into();
        h.run_steps(3);

        let align = h.state().session.commands().into_iter().find(|c| c.id == "home.alignRight").unwrap();

        assert_eq!(align.shortcut_for_platform(os == OperatingSystem::Mac), None);

        h.get_by_label_contains("Align Right    Home");
    }
}
