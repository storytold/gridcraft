//! The note editor and the cell menu's note item (#1561).

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, pos2, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
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

fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn press_enter(h: &mut Harness, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers });
    }
    h.run_steps(2);
}

fn note_at(h: &Harness, cell: &str) -> Option<String> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    sh.comments.get(&CellRef::parse(cell).unwrap()).map(|c| c.text.clone())
}

#[test]
fn enter_in_the_note_editor_adds_a_line_and_keeps_it_open() {
    let mut h = harness();
    h.state_mut().run("selection.set", json!({"cell": "B2"})).unwrap();
    h.state_mut().open_dialog("comment", json!({"threaded": false}));
    h.run_steps(3);
    h.get_by_role(Role::MultilineTextInput).click();
    h.run_steps(2);

    type_text(&mut h, "first");
    press_enter(&mut h, Modifiers::NONE);
    assert!(h.state().dialog.is_some(), "Enter must not close the note editor");
    type_text(&mut h, "second");
    press_enter(&mut h, Modifiers::SHIFT);
    assert!(h.state().dialog.is_some(), "Shift+Enter must not close the note editor");
    type_text(&mut h, "third");
    assert_eq!(note_at(&h, "B2"), None, "nothing is saved while the editor is open");

    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(note_at(&h, "B2").as_deref(), Some("first\nsecond\nthird"));
}

fn menu_has(h: &Harness, label: &str) -> bool {
    h.query_by_label(label).is_some()
}

fn open_cell_menu(h: &mut Harness, cell: &str) {
    h.state_mut().run("selection.set", json!({"cell": cell})).unwrap();
    h.state_mut().grid.context_menu = Some(pos2(400.0, 300.0));
    h.run_steps(3);
}

#[test]
fn the_cell_menu_offers_edit_note_on_a_cell_that_has_one() {
    let mut h = harness();
    h.state_mut().run("review.newNote", json!({"cell": "B2", "text": "existing"})).unwrap();

    open_cell_menu(&mut h, "C3");
    assert!(menu_has(&h, "New Note") && !menu_has(&h, "Edit Note"), "a cell without a note offers New Note");
    h.state_mut().grid.context_menu = None;
    h.run_steps(2);

    open_cell_menu(&mut h, "B2");
    assert!(menu_has(&h, "Edit Note") && !menu_has(&h, "New Note"), "a cell with a note offers Edit Note");
    h.get_by_label("Edit Note").click();
    h.run_steps(3);
    let d = h.state().dialog.as_ref().expect("Edit Note opens the note editor");
    assert_eq!(d.title, "Edit Note");
    assert_eq!(d.values.get("text"), Some(&json!("existing")));
}
