//! Customize the Ribbon and the Quick Access Toolbar: right-click menus, the toolbar's … menu, the
//! Customize dialog and the agents' commands.

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1500.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn click_at(h: &mut Harness, pos: Pos2, button: PointerButton) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos, button, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

fn qat(h: &Harness) -> Vec<String> {
    h.state().ui.ribbon.qat.clone()
}

fn a1(h: &Harness) -> gridcraft_engine::model::Style {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::new(0, 0))).clone()
}

fn painted(h: &Harness, text: &str) -> bool {
    h.output().shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == text))
}

#[test]
fn a_ribbon_button_goes_on_the_quick_access_toolbar_from_its_right_click_menu() {
    let mut h = harness();
    // Home › Wrap Text: the button sits just before its label.
    let label = h.get_by_role_and_label(Role::Label, "Wrap Text").rect();
    click_at(&mut h, pos2(label.left() - 14.0, label.center().y), PointerButton::Secondary);
    h.get_by_label("Add to Quick Access Toolbar").click();
    h.run_steps(3);
    assert_eq!(qat(&h), vec!["file.save", "edit.undo", "edit.redo", "home.wrapText"]);
    // The toolbar's button runs the command, as the ribbon's does.
    assert!(!a1(&h).align.wrap);
    h.get_by_role_and_label(Role::Button, "Wrap Text").click();
    h.run_steps(3);
    assert!(a1(&h).align.wrap, "the toolbar button wrapped A1");
    // On the ribbon button again, the menu offers to take it off; on the toolbar too.
    click_at(&mut h, pos2(label.left() - 14.0, label.center().y), PointerButton::Secondary);
    assert!(h.query_by_label("Remove from Quick Access Toolbar").is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    let button = h.get_by_role_and_label(Role::Button, "Wrap Text").rect();
    click_at(&mut h, button.center(), PointerButton::Secondary);
    h.get_by_label("Remove from Quick Access Toolbar").click();
    h.run_steps(3);
    assert_eq!(qat(&h), vec!["file.save", "edit.undo", "edit.redo"]);
    // Right-click on the ribbon's empty space: customise either.
    let card_right = pos2(1480.0, label.center().y);
    click_at(&mut h, card_right, PointerButton::Secondary);
    h.get_by_label("Customize the Ribbon…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("customizeRibbon"));
}

#[test]
fn the_toolbar_menu_ticks_commands_on_and_off() {
    let mut h = harness();
    h.get_by_role_and_label(Role::Button, "Customize Quick Access Toolbar").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::CheckBox, "Sort A to Z").click();
    h.run_steps(3);
    assert!(qat(&h).contains(&"data.sortAscending".to_string()));
    // Each pick closes the menu, as Excel's does.
    h.get_by_role_and_label(Role::Button, "Customize Quick Access Toolbar").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::CheckBox, "Undo").click();
    h.run_steps(3);
    assert!(!qat(&h).contains(&"edit.undo".to_string()), "Undo ticked off");
    assert!(h.query_by_role_and_label(Role::Button, "Sort A to Z").is_some(), "on the toolbar now");
}

#[test]
fn agents_hide_add_and_reorder_tabs_without_a_dialog() {
    let mut h = harness();
    let r = h
        .state_mut()
        .run(
            "ui.customizeRibbon",
            json!({"layout": {
                "order": ["Data", "Home"],
                "hidden": ["Draw"],
                "tabs": [{"key": "custom1", "label": "My Tools"}],
                "groups": [{"tab": "custom1", "label": "Mine", "commands": ["home.bold", "data.sortAscending", "no.such.command"]}]
            }}),
        )
        .unwrap();
    assert!(h.state().dialog.is_none(), "a programmatic call opens no dialog");
    let tabs: Vec<&str> = r["tabs"].as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert_eq!(tabs.first(), Some(&"Data"));
    assert!(!tabs.contains(&"Draw"));
    assert_eq!(tabs.last(), Some(&"custom1"));
    assert_eq!(r["layout"]["groups"][0]["commands"], json!(["home.bold", "data.sortAscending"]), "unknown commands are left out");
    assert_eq!(r["layout"]["qat"], json!(["file.save", "edit.undo", "edit.redo"]), "a layout without qat keeps the toolbar");
    h.run_steps(3);
    assert!(!painted(&h, "Draw"));
    assert!(painted(&h, "My Tools"));
    // The user's tab shows their group; its buttons run their commands.
    h.state_mut().run("ui.ribbonTab", json!({"tab": "custom1"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_role_and_label(Role::Button, "Sort A to Z").is_some());
    h.get_by_role_and_label(Role::Button, "Bold").click();
    h.run_steps(3);
    assert!(a1(&h).font.bold);
    // Hiding the tab that is showing moves to the first tab shown.
    h.state_mut().run("ui.customizeRibbon", json!({"layout": {"hidden": ["custom1"], "tabs": [{"key": "custom1", "label": "My Tools"}]}})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
    // Hostile input is an error or left out, never a crash.
    assert!(h.state_mut().run("ui.customizeRibbon", json!({"layout": 5})).is_err());
    let kept = h.state().ui.ribbon.clone();
    let e = h.state_mut().run("ui.customizeRibbon", json!({"layout": {"order": "Data"}})).unwrap_err();
    assert!(e.contains("malformed"), "{e}");
    assert_eq!(h.state().ui.ribbon, kept, "a malformed layout leaves the ribbon as it was");
    assert!(h.state_mut().run("ui.quickAccessToolbar", json!({"add": "no.such.command"})).is_err());
    let e = h.state_mut().run("ui.quickAccessToolbar", json!({"add": "view.zoom"})).unwrap_err();
    assert!(e.contains("needs a value"), "{e}");
    assert!(h.state_mut().run("ui.quickAccessToolbar", json!({"commands": ["file.save", 7, null]})).is_ok());
    let r = h.state_mut().run("ui.quickAccessToolbar", json!({"add": "home.bold"})).unwrap();
    assert_eq!(r["commands"], json!(["file.save", "home.bold"]));
    assert_eq!(h.state_mut().run("ui.quickAccessToolbar", json!({})).unwrap(), r, "{{}} reads it");
    let r = h.state_mut().run("ui.customizeRibbon", json!({"reset": true})).unwrap();
    assert_eq!(r["layout"]["qat"], json!(["file.save", "edit.undo", "edit.redo"]));
    assert_eq!(r["tabs"].as_array().map(Vec::len), Some(gridcraft_ui_egui::ribbon::TABS.len()));
}

#[test]
fn the_customize_dialog_fills_the_toolbar_and_a_new_tab() {
    let mut h = harness();
    h.state_mut().open_dialog("customizeRibbon", json!({"page": "qat"}));
    h.run_steps(3);
    // Filter the list as a person would, then pick from it.
    h.state_mut().dialog.as_mut().unwrap().search = "calculate now".into();
    h.run_steps(2);
    h.get_by_label("Calculate Now").click();
    h.run_steps(2);
    h.get_by_label("Add >>").click();
    h.run_steps(3);
    assert_eq!(qat(&h).last().map(String::as_str), Some("formulas.calculateNow"));
    // The Ribbon page: a new tab gets the command in its group.
    h.get_by_label("Ribbon").click();
    h.run_steps(2);
    h.get_by_label("New Tab").click();
    h.run_steps(3);
    h.get_by_label("Add >>").click();
    h.run_steps(3);
    let layout = &h.state().ui.ribbon;
    assert_eq!(layout.tabs.len(), 1);
    assert_eq!(layout.groups.len(), 1);
    assert_eq!(layout.groups[0].tab, layout.tabs[0].key);
    assert_eq!(layout.groups[0].commands, vec!["formulas.calculateNow".to_string()]);
    // Unticking a built-in tab hides it.
    assert!(h.state().ui.ribbon.visible_tabs().contains(&"Review".to_string()));
    h.get_by_label("Show Review").click();
    h.run_steps(3);
    assert!(!h.state().ui.ribbon.visible_tabs().contains(&"Review".to_string()));
    h.get_by_label("Close").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
}

#[test]
fn a_tab_or_group_is_renamed_as_typed_spaces_and_all() {
    let mut h = harness();
    h.state_mut().open_dialog("customizeRibbon", json!({}));
    h.run_steps(3);
    h.get_by_label("New Tab").click();
    h.run_steps(3);
    let value = |h: &Harness, rect: egui::Rect| h.get_all_by_role(Role::TextInput).find(|n| n.rect() == rect).and_then(|n| n.value());
    let label = |h: &Harness| {
        let l = &h.state().ui.ribbon;
        (l.tabs[0].label.clone(), l.groups[0].label.clone())
    };
    // The new tab or its group is selected, with its name in the field.
    let field = h.get_all_by_role(Role::TextInput).find(|n| matches!(n.value().as_deref(), Some("New Tab" | "New Group"))).unwrap();
    let (rect, was) = (field.rect(), field.value().unwrap());
    let renamed = |h: &Harness| if was == "New Tab" { label(h).0 } else { label(h).1 };
    h.input_mut().events.push(Event::PointerMoved(rect.center()));
    h.run_steps(1);
    h.get_all_by_role(Role::TextInput).find(|n| n.rect() == rect).unwrap().click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    h.key_press(egui::Key::Backspace);
    h.run_steps(2);
    // An empty field stays empty while the user types; the label keeps its old name.
    assert_eq!(value(&h, rect).as_deref(), Some(""));
    assert_eq!(renamed(&h), was);
    // One keystroke per frame, as a person types: each space must survive the next frame.
    for c in "My Tools ".chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(2);
    }
    assert_eq!(value(&h, rect).as_deref(), Some("My Tools "));
    assert_eq!(renamed(&h), "My Tools");
    // Leaving the field shows the label, trimmed.
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(value(&h, rect).as_deref(), Some("My Tools"));
    assert_eq!(renamed(&h), "My Tools");
}
