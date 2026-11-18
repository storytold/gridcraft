//! Custom command shortcuts: they dispatch ahead of the built-in keymap, show in command search
//! and on menu hover, and are edited from File › Options › Hotkeys.

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::HAlign;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::backstage::Page;
use gridcraft_ui_egui::hotkeys;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

fn harness() -> Harness {
    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    session.execute("cell.set", serde_json::json!({"cell": "A1", "input": "1"})).unwrap();
    session.execute("cell.set", serde_json::json!({"cell": "B1", "input": "2"})).unwrap();
    session.execute("selection.set", serde_json::json!({"cell": "A1"})).unwrap();
    crate::support::harness(session, 4)
}

fn key(h: &mut Harness, k: Key, modifiers: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers });
    h.run_steps(2);
}

fn style(h: &Harness, a1: &str) -> gridcraft_engine::model::Style {
    let doc = h.state().session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    doc.wb.styles.get(sheet.style_id(CellRef::parse(a1).unwrap())).clone()
}

#[test]
fn an_override_runs_before_the_builtin_and_clear_restores_it() {
    let mut h = harness();
    hotkeys::assign(&mut h.state_mut().ui.command_hotkeys, "home.bold", "F6", false).unwrap();
    hotkeys::assign(&mut h.state_mut().ui.command_hotkeys, "home.alignRight", "Cmd+B", false).unwrap();

    key(&mut h, Key::B, CTRL);
    assert_eq!(style(&h, "A1").align.h, HAlign::Right);
    assert!(!style(&h, "A1").font.bold, "Ctrl+B no longer bolds once Bold has been moved");

    key(&mut h, Key::F6, Modifiers::NONE);
    assert!(style(&h, "A1").font.bold, "F6 runs Bold");

    let refused = hotkeys::clear(&mut h.state_mut().ui.command_hotkeys, "home.bold", false).unwrap_err();
    assert!(refused.contains("Align Right"), "{refused}");
    hotkeys::clear(&mut h.state_mut().ui.command_hotkeys, "home.alignRight", false).unwrap();
    hotkeys::clear(&mut h.state_mut().ui.command_hotkeys, "home.bold", false).unwrap();

    h.state_mut().session.execute("selection.set", serde_json::json!({"cell": "B1"})).unwrap();
    key(&mut h, Key::B, CTRL);
    assert!(style(&h, "B1").font.bold, "clearing the override restores Ctrl+B");
    assert_eq!(style(&h, "B1").align.h, HAlign::General);
}

#[test]
fn command_search_and_the_cell_menu_show_the_current_shortcut() {
    let mut h = harness();
    hotkeys::assign(&mut h.state_mut().ui.command_hotkeys, "edit.copy", "F8", false).unwrap();
    h.state_mut().open_dialog("commandSearch", serde_json::json!({}));
    h.run_steps(4);
    h.state_mut().dialog.as_mut().unwrap().search = "edit.copy".into();
    h.run_steps(3);
    h.get_by_label_contains("Copy  (F8)");

    h.state_mut().dialog = None;
    h.run_steps(2);
    let pos = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = gridcraft_ui_egui::grid::Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        geo.cell_rect(sheet, CellRef::parse("A1").unwrap()).center()
    };
    h.input_mut().events.push(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    h.ctx.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Copy").hover();
    h.run_steps(6);
    assert!(h.query_by_value("Copy (F8)").is_some(), "the cell menu tooltip shows the override");
}

#[test]
fn options_hotkeys_page_assigns_a_recorded_chord() {
    let mut h = harness();
    h.state_mut().open_backstage(Page::Options);
    h.run_steps(4);
    h.get_by_label("Hotkeys").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Search commands").click();
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("alignRight".into()));
    h.run_steps(3);
    h.get_by_label_contains("Align Right").click();
    h.run_steps(2);
    h.get_by_label("Record").click();
    h.run_steps(2);
    press(&mut h, Key::F6, Modifiers::NONE);
    h.get_by_label_contains("Recorded: F6");
    h.get_by_label("Assign").click();
    h.run_steps(2);
    assert_eq!(h.state().ui.command_hotkeys.get("home.alignRight").map(String::as_str), Some("F6"));

    // A taken chord, a macro chord, a key tip and F10 stay visible refusals.
    h.get_by_label("Record").click();
    h.run_steps(2);
    press(&mut h, Key::B, CTRL);
    h.get_by_label_contains("Recorded: Ctrl+B");
    h.get_by_label("Assign").click();
    h.run_steps(2);
    h.get_by_label_contains("already used by Bold");
    assert_eq!(h.state().ui.command_hotkeys.get("home.alignRight").map(String::as_str), Some("F6"));

    h.get_by_label("Record").click();
    h.run_steps(2);
    press(&mut h, Key::B, Modifiers { shift: true, ..CTRL });
    h.get_by_label_contains("reserved for macros");

    h.get_by_label("Record").click();
    h.run_steps(2);
    press(&mut h, Key::B, Modifiers { alt: true, ctrl: false, shift: false, mac_cmd: false, command: false });
    h.get_by_label_contains("reserved for key tips");

    h.get_by_label("Record").click();
    h.run_steps(2);
    press(&mut h, Key::F10, Modifiers::NONE);
    h.get_by_label_contains("F10 is reserved for key tips");
}

fn press(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut().events.push(Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers });
    h.run_steps(3);
    // Release before the next chord. A key left down is still held when Record is clicked again,
    // so the next press of the same letter would be a repeat and would not record.
    h.input_mut().events.push(Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers });
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}
