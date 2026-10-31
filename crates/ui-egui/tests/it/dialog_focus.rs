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

    for (id, expected) in [
        ("home.bold", "Cmd+B"),
        ("data.filter", "Ctrl+Shift+L"),
        ("edit.flashFill", "Ctrl+E"),
        ("edit.pasteSpecial", "Ctrl+Cmd+V"),
        ("selection.row", "Shift+Space"),
        ("selection.column", "Ctrl+Space"),
        ("selection.currentRegion", "Ctrl+Shift+8"),
        ("home.alignLeft", "Cmd+L"),
        ("home.alignCenter", "Cmd+E"),
        ("data.group", "Alt+Shift+Right"),
        ("data.ungroup", "Alt+Shift+Left"),
    ] {
        let command = commands.iter().find(|c| c.id == id).unwrap();
        assert_eq!(command.shortcut, Some(expected), "{id}");
    }
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
