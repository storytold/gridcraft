//! File › Options › General › User name (#1449): the field sets the user name through
//! `options.userName`, and an emptied field keeps the saved name.

use egui::{Event, Key, Modifiers, accesskit::Role};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, gridcraft_ui_egui::SheetApp>;

fn field_text(h: &Harness, label: &str) -> Option<String> {
    h.get_by_role_and_label(Role::TextInput, label).value()
}

/// Clicks the field, clears it, types `text` one keystroke per frame and ends with `end`.
fn retype(h: &mut Harness, label: &str, text: &str, end: Key) {
    let rect = h.get_by_role_and_label(Role::TextInput, label).rect();
    h.input_mut().events.push(Event::PointerMoved(rect.center()));
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    h.key_press(Key::Backspace);
    h.run_steps(1);
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(2);
    }
    h.key_press(end);
    h.run_steps(3);
}

#[test]
fn typing_a_user_name_sets_it_and_an_emptied_field_keeps_the_saved_one() {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);
    let name = |h: &Harness| h.state().session.prefs.user_name.clone();
    assert_eq!(field_text(&h, "User name").as_deref(), Some("GridCraft User"));
    let revision = h.state().session.active().unwrap().revision;

    retype(&mut h, "User name", "  Ann Lee ", Key::Enter);
    assert_eq!(name(&h), "Ann Lee", "Enter commits the name through the command");
    assert_eq!(field_text(&h, "User name").as_deref(), Some("Ann Lee"), "the field shows the name as kept");
    assert!(h.state().message.is_none());
    assert_eq!(h.state().session.active().unwrap().revision, revision, "the open workbook doesn't change");

    // An emptied field, or one of spaces, sends nothing and shows the saved name again.
    for blank in ["", "   "] {
        retype(&mut h, "User name", blank, Key::Enter);
        assert_eq!(name(&h), "Ann Lee", "{blank:?}");
        assert_eq!(field_text(&h, "User name").as_deref(), Some("Ann Lee"), "{blank:?}");
        assert!(h.state().message.is_none(), "{blank:?} raises no message");
    }
    // Esc drops what was typed.
    retype(&mut h, "User name", "Zed", Key::Escape);
    assert_eq!(name(&h), "Ann Lee");
    assert_eq!(field_text(&h, "User name").as_deref(), Some("Ann Lee"));

    // New notes are signed with it.
    h.state_mut().run("review.newNote", json!({"cell": "A1", "text": "x"})).unwrap();
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.wb.active().unwrap().comments.get(&CellRef::new(0, 0)).map(|c| c.author.as_str()), Some("Ann Lee"));

    // In German the field is "Benutzername".
    h.state_mut().run("app.language.set", json!({"language": Language::De.code()})).unwrap();
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);
    retype(&mut h, "Benutzername", "Anna Lee", Key::Enter);
    assert_eq!(name(&h), "Anna Lee");
    h.check_accessibility();
}
