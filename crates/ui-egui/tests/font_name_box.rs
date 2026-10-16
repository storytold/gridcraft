//! Home ▸ Font is a box you type a font name into, with the fonts behind its arrow (#933, the Enter
//! bug reported on #932).

use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, pos2};
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
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
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

/// Alt, H, F, F focuses the font box, then `text` is typed (not confirmed).
fn type_font(h: &mut Harness, text: &str) {
    key(h, Key::F10);
    // Each letter is released: F, F unreleased reads as one held key.
    for k in [Key::H, Key::F, Key::F] {
        h.input_mut().events.extend([
            Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
            Event::Text(k.name().to_ascii_lowercase()),
            Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE },
        ]);
        h.run_steps(2);
    }
    h.run_steps(3);
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn font(h: &Harness, cell: &str) -> String {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).font.name.clone()
}

fn box_shows(h: &Harness, text: &str) -> bool {
    h.get_all_by_role(Role::TextInput).any(|n| n.value().as_deref() == Some(text))
}

#[test]
fn typed_name_and_enter_apply_it_to_the_selection() {
    // Aptos and Aptos Narrow both contain "Aptos": Enter applied Aptos Narrow, the row the stale
    // highlight (Calibri's index) landed on in the filtered list.
    for (typed, want) in [("Aptos", "Aptos"), ("Georgia", "Georgia"), ("arial", "Arial"), ("Ari", "Arial")] {
        let mut h = harness();
        type_font(&mut h, typed);
        key(&mut h, Key::Enter);
        h.run_steps(3);
        assert!(h.state().message.is_none(), "typing {typed} must not raise an error");
        for cell in ["A1", "B2"] {
            assert_eq!(font(&h, cell), want, "{typed} + Enter sets {cell} to {want}");
        }
        let d = h.state().session.active().unwrap();
        assert_eq!(d.selection.active, CellRef::parse("A1").unwrap(), "the Enter is the box's, the active cell stays put");
        assert_eq!(d.wb.active().unwrap().value(CellRef::parse("A1").unwrap()), Value::Empty, "the name goes to the box, not the cell");
        assert!(box_shows(&h, want), "the box shows the applied font");
    }
}

#[test]
fn a_name_outside_the_list_is_applied_as_typed() {
    let mut h = harness();
    type_font(&mut h, "Comic Sans MS");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(font(&h, "A1"), "Comic Sans MS");
    assert_eq!(font(&h, "B2"), "Comic Sans MS");

    // Only the start of a name completes it: "Neue" is not Helvetica Neue.
    let mut h = harness();
    type_font(&mut h, "Neue");
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(font(&h, "A1"), "Neue");
}

#[test]
fn clicking_away_or_escape_leaves_the_font_alone() {
    let mut h = harness();
    h.get_all_by_role(Role::TextInput).find(|n| n.value().as_deref() == Some("Calibri")).expect("the font box").click();
    h.run_steps(3);
    h.input_mut().events.push(Event::Text("Georgia".into()));
    h.run_steps(2);
    assert!(box_shows(&h, "Georgia"), "Georgia is typed into the font box");
    let on_grid = pos2(600.0, 500.0);
    h.input_mut().events.extend([
        Event::PointerMoved(on_grid),
        Event::PointerButton { pos: on_grid, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: on_grid, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(4);
    let d = h.state().session.active().unwrap();
    let clicked = d.selection.active;
    assert_ne!(clicked, CellRef::parse("A1").unwrap(), "the click landed on the grid");
    assert_eq!(d.wb.styles.get(d.wb.active().unwrap().style_id(clicked)).font.name, "Calibri", "clicking away applies nothing");
    assert_eq!(font(&h, "A1"), "Calibri");
    assert!(box_shows(&h, "Calibri"), "the box shows the cell's font again");

    let mut h = harness();
    type_font(&mut h, "Georgia");
    key(&mut h, Key::Escape);
    h.run_steps(3);
    assert_eq!(font(&h, "A1"), "Calibri");
    assert!(box_shows(&h, "Calibri"), "Escape reverts the box");
}

#[test]
fn the_arrow_opens_the_list_of_fonts() {
    let mut h = harness();
    h.get_by_role_and_label(Role::Button, "Font").click();
    h.run_steps(3);
    assert!(h.query_by_label("Type to filter…").is_none(), "the list has no filter field, the box is where you type");
    h.get_by_role_and_label(Role::Button, "Consolas").click();
    h.run_steps(3);
    assert_eq!(font(&h, "B2"), "Consolas", "clicking a font applies it");

    let mut h = harness();
    h.get_by_role_and_label(Role::Button, "Font").click();
    h.run_steps(3);
    key(&mut h, Key::ArrowDown);
    key(&mut h, Key::Enter);
    h.run_steps(3);
    assert_eq!(font(&h, "A1"), "Cambria", "Down from Calibri then Enter picks Cambria");
}
