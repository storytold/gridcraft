//! Home ▸ Font Size is a box you type a size into, with the standard sizes behind its arrow (#932).

use egui::{Event, Key, Modifiers, accesskit::Role};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("selection.set", json!({"range": "A1:B2"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

fn key(h: &mut Harness, key: Key) {
    h.input_mut().events.extend([
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

/// Alt, H, F, S focuses the size box, then `text` is typed and confirmed with Enter.
fn type_size(h: &mut Harness, text: &str) {
    key(h, Key::F10);
    for k in [Key::H, Key::F, Key::S] {
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.input_mut().events.push(Event::Text(k.name().to_ascii_lowercase()));
        h.run_steps(2);
    }
    h.run_steps(3);
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(1);
    }
    key(h, Key::Enter);
    h.run_steps(3);
}

fn size(h: &Harness, cell: &str) -> f32 {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).font.size
}

#[test]
fn typed_size_is_applied_exactly_to_the_selection() {
    for (typed, want) in [("6", 6.0), ("2", 2.0), ("10.5", 10.5)] {
        let mut h = harness();
        type_size(&mut h, typed);
        assert!(h.state().message.is_none(), "typing {typed} must not raise an error");
        for cell in ["A1", "B2"] {
            assert_eq!(size(&h, cell), want, "typing {typed} + Enter sets {cell} to {want}");
        }
        assert_eq!(
            h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("A1").unwrap()),
            Value::Empty,
            "the digits go to the box, not the cell"
        );
    }
}

#[test]
fn out_of_range_or_not_a_number_changes_nothing() {
    let mut h = harness();
    type_size(&mut h, "500");
    let (_, text) = h.state().message.clone().expect("500 is refused with a message");
    assert!(text.contains("between 1 and 409"), "{text}");
    assert_eq!(size(&h, "A1"), 11.0);

    let mut h = harness();
    type_size(&mut h, "abc");
    assert!(h.state().message.is_none());
    assert_eq!(size(&h, "A1"), 11.0, "text that is not a number reverts");
}

#[test]
fn escape_cancels_the_typed_size() {
    let mut h = harness();
    key(&mut h, Key::F10);
    for k in [Key::H, Key::F, Key::S] {
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.input_mut().events.push(Event::Text(k.name().to_ascii_lowercase()));
        h.run_steps(2);
    }
    h.run_steps(3);
    h.input_mut().events.push(Event::Text("6".into()));
    h.run_steps(1);
    key(&mut h, Key::Escape);
    h.run_steps(3);
    assert_eq!(size(&h, "A1"), 11.0);
    assert!(h.get_all_by_role(Role::TextInput).any(|n| n.value().as_deref() == Some("11")), "the box shows the cell's size again");
}

#[test]
/// The size list has no filter; clicking 16 applies it, and Down then Enter picks 12.
fn the_arrow_opens_the_plain_list_of_sizes() {
    let mut h = harness();
    h.get_by_role_and_label(Role::Button, "Font Size").click();
    h.run_steps(3);
    assert!(h.query_by_label("Type to filter…").is_none(), "the size list has no filter field");
    // The grid's row headers carry the same numbers as labels: pick the list's button.
    h.get_by_role_and_label(Role::Button, "16").click();
    h.run_steps(3);
    assert_eq!(size(&h, "B2"), 16.0, "clicking a size applies it");

    let mut h = harness();
    h.get_by_role_and_label(Role::Button, "Font Size").click();
    h.run_steps(3);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(size(&h, "A1"), 12.0, "Down from 11 then Enter picks 12");
}
