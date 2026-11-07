//! Developer tab: Excel's group layout, its real Macros command, and its disabled stubs.

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::ribbon::RibbonOverflow;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const IN_DEVELOPMENT: &str = "In development — tracked in #2960";

/// Every Developer entry except Macros, in Excel's observed tab order.
const STUBS: [&str; 17] = [
    "Visual Basic",
    "Record Macro",
    "Use Relative References",
    "Macro Security",
    "Add-ins",
    "Excel Add-ins",
    "COM Add-ins",
    "Insert",
    "Design Mode",
    "View Code",
    "Run Dialog",
    "Source",
    "Map Properties",
    "Expansion Packs",
    "Refresh Data",
    "Import",
    "Export",
];

fn developer_at(width: f32) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = "Developer".into();
    crate::common::harness_with(crate::common::builder().with_size(vec2(width, 700.0)), app, 4)
}

fn click_at(h: &mut Harness, at: egui::Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

#[test]
fn the_developer_tab_renders_every_entry_and_fits_the_card() {
    let h = developer_at(1440.0);
    // `Insert` is also a tab name, so entries are looked up by role as well as label.
    assert!(h.query_by_role_and_label(Role::Button, "Macros").is_some(), "Macros is missing");
    for label in STUBS {
        assert!(h.query_by_role_and_label(Role::Button, label).is_some(), "{label} is missing from the Developer tab");
    }
    let o = RibbonOverflow::get(&h.ctx);
    assert!(o.visible_height > 0.0);
    assert!(o.content_height <= o.visible_height + 0.5, "Developer: {} px of groups in {} px", o.content_height, o.visible_height);
}

/// Excel's groups run Code, Add-ins, Controls, XML, with the small buttons stacked beside the big
/// ones; the card is not wide enough at 1440 px to reflow that, so the order is stable.
#[test]
fn the_developer_groups_follow_excels_layout() {
    let h = developer_at(1440.0);
    let x = |label: &str| h.get_by_role_and_label(Role::Button, label).rect().left();
    let y = |label: &str| h.get_by_role_and_label(Role::Button, label).rect().top();
    // Code: Visual Basic, Macros, then the Record Macro column.
    assert!(x("Visual Basic") < x("Macros"), "Visual Basic comes before Macros");
    assert!(x("Macros") < x("Record Macro"), "the Record Macro column follows the big buttons");
    assert!(y("Record Macro") < y("Use Relative References") && y("Use Relative References") < y("Macro Security"));
    // Add-ins: three big buttons.
    assert!(x("Macro Security") < x("Add-ins"), "the Add-ins group follows Code");
    assert!(x("Add-ins") < x("Excel Add-ins") && x("Excel Add-ins") < x("COM Add-ins"));
    // Controls: Insert and Design Mode, then Properties / View Code / Run Dialog stacked.
    assert!(x("COM Add-ins") < x("Insert"), "the Controls group follows Add-ins");
    assert!(x("Design Mode") < x("Properties"), "the small Controls column follows the big buttons");
    assert!(y("Properties") < y("View Code") && y("View Code") < y("Run Dialog"));
    // XML: Source, then two stacked columns.
    assert!(x("Run Dialog") < x("Source"), "the XML group follows Controls");
    assert!(x("Source") < x("Map Properties") && x("Map Properties") < x("Import"));
    assert!(y("Map Properties") < y("Expansion Packs") && y("Expansion Packs") < y("Refresh Data"));
    assert!(y("Import") < y("Export"));
}

#[test]
fn developer_stubs_are_disabled_and_never_run_a_command() {
    let mut h = developer_at(1440.0);
    for label in STUBS {
        assert!(h.get_by_role_and_label(Role::Button, label).accesskit_node().is_disabled(), "{label} is a stub and must be disabled");
    }
    let before = h.state().dialog.as_ref().map(|d| d.name.clone());
    for label in ["Visual Basic", "Record Macro", "Add-ins", "Insert", "Source", "Import"] {
        let at = h.get_by_role_and_label(Role::Button, label).rect().center();
        click_at(&mut h, at);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.name.clone()), before, "clicking {label} must not open anything");
    }
    assert!(!h.state().session.is_recording(), "a stub must not start recording");
}

#[test]
fn developer_stub_tooltips_name_the_issue() {
    let mut h = developer_at(1440.0);
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    for label in ["Visual Basic", "Macro Security", "COM Add-ins", "Map Properties"] {
        let at = h.get_by_role_and_label(Role::Button, label).rect().center();
        h.input_mut().events.push(Event::PointerMoved(at));
        h.run_steps(6);
        assert!(h.query_all_by_value(IN_DEVELOPMENT).next().is_some(), "{label}'s tooltip must name #2960");
    }
}

#[test]
fn developer_macros_opens_the_macros_dialog() {
    let mut h = developer_at(1440.0);
    assert!(!h.get_by_role_and_label(Role::Button, "Macros").accesskit_node().is_disabled(), "Macros is implemented");
    h.get_by_role_and_label(Role::Button, "Macros").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("macros"));
}
