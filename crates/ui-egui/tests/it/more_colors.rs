//! The colour menus' More Colors… and Recent Colors, through the real egui input path.

use egui::{Event, Key, Modifiers, os::OperatingSystem};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = crate::common::builder().with_os(OperatingSystem::Nix).with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

fn press(h: &mut Harness, key: Key, modifiers: Modifiers) {
    let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
    h.input_mut().events.extend([ev(true), ev(false)]);
    h.run_steps(2);
}

/// H, H opens the Fill Color palette.
fn open_fill_palette(h: &mut Harness) {
    for k in [Key::F10, Key::H, Key::H] {
        press(h, k, Modifiers::NONE);
    }
    h.run_steps(3);
}

fn type_hex(h: &mut Harness, hex: &str) {
    h.get_by_label("Hex").click();
    h.run_steps(2);
    press(h, Key::A, Modifiers::COMMAND);
    h.input_mut().events.push(Event::Text(hex.to_string()));
    h.run_steps(3);
}

#[test]
fn more_colors_applies_the_typed_colour_and_remembers_it() {
    let mut h = harness();
    open_fill_palette(&mut h);
    assert!(h.query_by_label("Recent Colors").is_none(), "nothing is recent before a custom colour is used");
    h.get_by_label("More Colors…").click();
    h.run_steps(3);
    type_hex(&mut h, "#12AB34");
    h.get_by_label("  OK  ").click();
    h.run_steps(4);
    assert_eq!(h.state().grid.last_fill, "#12AB34");
    assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == "home.fillColor").count(), 1);
    assert!(h.query_by_label("Theme Colors").is_none(), "the palette closes after OK");
    // The next time any colour menu opens, the colour is the first Recent Color.
    open_fill_palette(&mut h);
    h.get_by_label("Recent Colors");
    assert!(h.query_by_label("Hex").is_none(), "More Colors… starts closed");
}

#[test]
/// A short hex value applies no fill, and Cancel applies nothing and remembers no color.
fn a_colour_that_is_not_hex_does_not_apply_and_cancel_applies_nothing() {
    let mut h = harness();
    open_fill_palette(&mut h);
    h.get_by_label("More Colors…").click();
    h.run_steps(3);
    // Enter in the Hex box does not apply the palette's highlighted swatch or close the menu.
    type_hex(&mut h, "#12AB");
    press(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().session.journal.iter().all(|(id, _)| id != "home.fillColor"));
    h.get_by_label("Hex");
    // The formula bar's icon-only Cancel also carries that tooltip name (role Unknown); the
    // palette's own Cancel is the button here.
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").click();
    h.run_steps(3);
    assert!(h.state().session.journal.iter().all(|(id, _)| id != "home.fillColor"));
    assert!(h.query_by_label("Hex").is_none());
    assert!(h.query_by_label("Recent Colors").is_none(), "Cancel remembers nothing");
}

#[test]
fn the_font_colour_menu_has_them_too_and_shares_the_recent_colours() {
    let mut h = harness();
    open_fill_palette(&mut h);
    h.get_by_label("More Colors…").click();
    h.run_steps(3);
    type_hex(&mut h, "#00FF7F");
    h.get_by_label("  OK  ").click();
    h.run_steps(4);
    for k in [Key::F10, Key::H, Key::F, Key::C] {
        press(&mut h, k, Modifiers::NONE);
    }
    h.run_steps(3);
    h.get_by_label("Automatic");
    h.get_by_label("Recent Colors");
    h.get_by_label("More Colors…");
}
