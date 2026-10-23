//! Insert › Text split button: the body inserts a text box and the arrow opens the drop-down.

use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::model::SheetViewMode;
use gridcraft_ui_egui::SheetApp;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_os(OperatingSystem::Nix).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(6);
    h
}

fn journal_has(h: &egui_kittest::Harness<'static, SheetApp>, id: &str) -> bool {
    h.state().session.journal.iter().any(|(command, _)| command == id)
}

#[test]
fn body_inserts_a_text_box_instead_of_opening_the_menu() {
    let mut h = harness();
    h.get_by_label("Text").click();
    h.run_steps(6);
    assert!(journal_has(&h, "insert.textBox"), "the body runs insert.textBox");
    assert!(h.query_by_label("Header & Footer").is_none(), "the body does not open the drop-down");
}

#[test]
fn arrow_opens_the_menu_and_both_items_work() {
    let mut h = harness();

    h.get_by_label("Text options").click();
    h.run_steps(6);
    assert!(h.query_by_label("Text Box").is_some(), "the arrow opens the drop-down");
    assert!(h.query_by_label("Header & Footer").is_some());
    h.get_by_label("Text Box").click();
    h.run_steps(6);
    assert!(journal_has(&h, "insert.textBox"));

    h.get_by_label("Text options").click();
    h.run_steps(6);
    h.get_by_label("Header & Footer").click();
    h.run_steps(6);
    let view = h.state().session.active().unwrap().wb.active().unwrap().view;
    assert_eq!(view, SheetViewMode::PageLayout, "Header & Footer starts editing the page header");
    let edit = h.state().grid.hf_edit.as_ref().expect("header editing is active");
    assert!(!edit.footer, "the centre header is edited, not the footer");
    assert_eq!(edit.section, 1, "the centre section is edited");
}

fn key_event(key: Key, pressed: bool) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }
}

fn key(h: &mut egui_kittest::Harness<'static, SheetApp>, key: Key) {
    h.input_mut().events.extend([key_event(key, true), key_event(key, false)]);
    h.run_steps(2);
}

fn letter(h: &mut egui_kittest::Harness<'static, SheetApp>, key: Key) {
    let text = Event::Text(key.name().to_ascii_lowercase());
    h.input_mut().events.extend([key_event(key, true), text, key_event(key, false)]);
    h.run_steps(2);
}

#[test]
fn nh1_starts_editing_the_page_header() {
    let mut h = harness();
    key(&mut h, Key::F10);
    for ch in "NH1".chars() {
        letter(&mut h, Key::from_name(&ch.to_string()).expect("sequence letters map to keys"));
    }
    h.run_steps(6);
    let view = h.state().session.active().unwrap().wb.active().unwrap().view;
    assert_eq!(view, SheetViewMode::PageLayout, "NH1 starts editing the page header");
    assert!(h.state().grid.hf_edit.is_some(), "NH1 begins an in-place header edit");
    assert!(h.state().dialog.is_none(), "NH1 no longer opens the Page Setup dialog");
}
