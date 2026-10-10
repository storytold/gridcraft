//! Ribbon access keys through the complete egui input and UI path.

use egui::{Event, Key, Modifiers, PointerButton, os::OperatingSystem};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::HAlign;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    harness_on(session, OperatingSystem::Nix)
}

fn harness_on(session: Session, os: OperatingSystem) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_os(os).with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

fn events(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(2);
}

fn key(h: &mut Harness, key: Key) {
    events(h, [key_event(key, true, Modifiers::NONE), key_event(key, false, Modifiers::NONE)]);
}

fn letter(h: &mut Harness, key: Key, text_first: bool) {
    let text = Event::Text(key.name().to_ascii_lowercase());
    let pressed = key_event(key, true, Modifiers::NONE);
    let pair = if text_first { [text, pressed] } else { [pressed, text] };
    events(h, pair.into_iter().chain([key_event(key, false, Modifiers::NONE)]));
}

fn sequence(h: &mut Harness, letters: &[Key], text_first: bool) {
    key(h, Key::F10);
    for &k in letters {
        letter(h, k, text_first);
    }
}

fn value(h: &Harness, cell: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

fn alignment(h: &Harness) -> HAlign {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse("A1").unwrap())).align.h
}

fn copied_row() -> Session {
    let mut s = blank();
    s.execute("cell.set", json!({"cell": "A1", "input": "=20+22"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "99"})).unwrap();
    s.execute("edit.copy", json!({"range": "A1:B1"})).unwrap();
    s.execute("selection.set", json!({"cell": "D4"})).unwrap();
    s
}

#[test]
fn alignment_sequences_consume_key_and_text_events_in_either_order() {
    for text_first in [false, true] {
        let mut s = blank();
        s.execute("cell.set", json!({"cell": "A1", "input": "keep me"})).unwrap();
        let mut h = harness(s);
        h.state_mut().ui.ribbon_tab = "Insert".into();
        for (k, expected) in [(Key::R, HAlign::Right), (Key::L, HAlign::Left), (Key::C, HAlign::Center)] {
            sequence(&mut h, &[Key::H, Key::A, k], text_first);
            assert_eq!(h.state().ui.ribbon_tab, "Home");
            assert_eq!(alignment(&h), expected);
            assert_eq!(value(&h, "A1"), Value::from("keep me"));
            assert!(h.state().editor.is_none(), "access-key letters must not begin a cell edit");
            assert_eq!(h.state().keytips.prefix(), None);
        }
        // Consuming the terminal access key must not swallow the next ordinary keystroke.
        letter(&mut h, Key::X, text_first);
        key(&mut h, Key::Enter);
        assert_eq!(value(&h, "A1"), Value::from("x"));
    }
}

#[test]
fn paste_special_options_wait_for_enter_and_transpose_values() {
    let mut h = harness(copied_row());
    sequence(&mut h, &[Key::H, Key::V, Key::S], true);
    assert_eq!(h.state().dialog.as_ref().unwrap().name(), "pasteSpecial");
    for (k, what) in [(Key::T, "formats"), (Key::V, "values"), (Key::F, "formulas"), (Key::C, "comments")] {
        letter(&mut h, k, true);
        assert_eq!(h.state().dialog.as_ref().unwrap().values.get("what"), Some(&json!(what)));
        assert_eq!(value(&h, "D4"), Value::Empty, "choosing a paste option must not paste yet");
    }
    letter(&mut h, Key::V, true);
    letter(&mut h, Key::E, true);
    assert_eq!(h.state().dialog.as_ref().unwrap().values.get("transpose"), Some(&json!(true)));
    for modifiers in [Modifiers::CTRL, Modifiers::COMMAND] {
        events(
            &mut h,
            [
                Event::ModifiersChanged(modifiers),
                key_event(Key::T, true, modifiers),
                key_event(Key::T, false, modifiers),
                key_event(Key::E, true, modifiers),
                key_event(Key::E, false, modifiers),
                Event::ModifiersChanged(Modifiers::NONE),
            ],
        );
        let dialog = h.state().dialog.as_ref().unwrap();
        assert_eq!(dialog.values.get("what"), Some(&json!("values")), "Ctrl/Cmd must not select a paste mnemonic");
        assert_eq!(dialog.values.get("transpose"), Some(&json!(true)), "Ctrl/Cmd must not toggle transpose");
    }
    // Non-macOS Alt+E selects the dialog mnemonic without activating ribbon access keys.
    events(
        &mut h,
        [
            Event::ModifiersChanged(Modifiers::ALT),
            Event::Text("e".into()),
            key_event(Key::E, true, Modifiers::ALT),
            key_event(Key::E, false, Modifiers::ALT),
            Event::ModifiersChanged(Modifiers::NONE),
        ],
    );
    assert_eq!(h.state().dialog.as_ref().unwrap().values.get("transpose"), Some(&json!(false)));
    assert_eq!(h.state().keytips.prefix(), None);
    letter(&mut h, Key::E, false);
    key(&mut h, Key::Enter);
    assert!(h.state().dialog.is_none());
    assert!(h.state().editor.is_none());
    assert_eq!(value(&h, "D4"), Value::Number(42.0));
    assert_eq!(value(&h, "D5"), Value::Number(99.0));
    assert_eq!(value(&h, "E4"), Value::Empty);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert!(sh.cell(CellRef::parse("D4").unwrap()).unwrap().formula.is_none());
}

#[test]
fn keyboard_opened_paste_menu_accepts_a_mouse_click_after_keytips_cancel() {
    let mut h = harness(copied_row());
    sequence(&mut h, &[Key::H, Key::V], false);
    assert_eq!(h.state().keytips.prefix(), Some("HV"));
    assert!(h.state().dialog.is_none());

    // This sends actual pointer events at the accessible menu item's rectangle.
    // The press cancels keytips, but must not relocate or close the menu before the click.
    h.get_by_label("Paste Special…").click();
    h.run_steps(3);

    assert_eq!(h.state().keytips.prefix(), None);
    assert_eq!(h.state().dialog.as_ref().unwrap().name(), "pasteSpecial");
    assert_eq!(value(&h, "D4"), Value::Empty);
    assert!(h.state().editor.is_none());
}

#[test]
fn legacy_paste_special_keeps_formulas_when_requested() {
    let mut h = harness(copied_row());
    sequence(&mut h, &[Key::E, Key::S], false);
    assert_eq!(h.state().dialog.as_ref().unwrap().name(), "pasteSpecial");
    letter(&mut h, Key::F, false);
    assert_eq!(value(&h, "D4"), Value::Empty);
    key(&mut h, Key::Enter);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sh.cell(CellRef::parse("D4").unwrap()).unwrap().formula.as_ref().unwrap().text, "20+22");
    assert_eq!(value(&h, "D4"), Value::Number(42.0));
    assert_eq!(value(&h, "E4"), Value::Number(99.0));
    assert!(h.state().editor.is_none());
    key(&mut h, Key::ArrowDown);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "D5");
}

#[test]
fn ribbon_and_legacy_autofit_sequences_resize_the_selected_column() {
    for route in [&[Key::H, Key::O, Key::I][..], &[Key::O, Key::C, Key::A][..]] {
        let mut s = blank();
        s.execute("cell.set", json!({"cell": "A1", "input": "A long heading that needs a wider column"})).unwrap();
        s.execute("home.columnWidth", json!({"cols": "A:A", "width": 12})).unwrap();
        let mut h = harness(s);
        let before = h.state().session.active().unwrap().wb.active().unwrap().col_width(0);
        sequence(&mut h, route, false);
        let after = h.state().session.active().unwrap().wb.active().unwrap().col_width(0);
        assert!(after > before, "autofit must widen the narrow column");
        assert_eq!(value(&h, "A1"), Value::from("A long heading that needs a wider column"));
        assert!(h.state().editor.is_none());
        assert_eq!(h.state().keytips.prefix(), None);
        letter(&mut h, Key::X, false);
        key(&mut h, Key::Enter);
        assert_eq!(value(&h, "A1"), Value::from("x"));
    }
}

#[test]
fn legacy_prefix_switches_to_home_once_not_every_frame() {
    let mut h = harness(blank());
    h.state_mut().ui.ribbon_tab = "Insert".into();
    sequence(&mut h, &[Key::E], false);
    assert_eq!(h.state().keytips.prefix(), Some("E"));
    assert_eq!(h.state().ui.ribbon_tab, "Home");
    // While the prefix stays the same, later frames must not keep forcing the Home tab.
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(3);
    assert_eq!(h.state().keytips.prefix(), Some("E"));
    assert_eq!(h.state().ui.ribbon_tab, "Insert");
}

#[test]
fn escape_backs_out_without_clearing_copy_and_f10_toggles_off() {
    let mut h = harness(copied_row());
    sequence(&mut h, &[Key::H, Key::A], false);
    assert_eq!(h.state().keytips.prefix(), Some("HA"));
    for expected in [Some("H"), Some(""), None] {
        key(&mut h, Key::Escape);
        assert_eq!(h.state().keytips.prefix(), expected);
        assert_eq!(h.state().session.clipboard.as_ref().unwrap().range.a1(), "A1:B1");
    }
    key(&mut h, Key::F10);
    assert_eq!(h.state().keytips.prefix(), Some(""));
    key(&mut h, Key::F10);
    assert_eq!(h.state().keytips.prefix(), None);
    assert!(h.state().session.clipboard.is_some());

    // A bare left Alt is the desktop entry point on non-macOS systems.
    events(&mut h, [Event::ModifiersChanged(Modifiers::ALT), key_event(Key::AltLeft, true, Modifiers::ALT)]);
    events(&mut h, [Event::ModifiersChanged(Modifiers::NONE), key_event(Key::AltLeft, false, Modifiers::NONE)]);
    assert_eq!(h.state().keytips.prefix(), Some(""));
}

#[test]
fn mouse_press_and_window_focus_loss_cancel_keytips() {
    let mut h = harness(blank());
    sequence(&mut h, &[Key::H], false);
    let pos = h.state().grid.cells_rect.unwrap().center();
    events(
        &mut h,
        [
            Event::PointerMoved(pos),
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ],
    );
    assert_eq!(h.state().keytips.prefix(), None);
    key(&mut h, Key::F10);
    assert_eq!(h.state().keytips.prefix(), Some(""));
    h.input_mut().focused = false;
    events(&mut h, [Event::WindowFocused(false)]);
    assert_eq!(h.state().keytips.prefix(), None);
    assert!(h.state().editor.is_none());
}

#[test]
fn held_keys_do_not_toggle_twice_or_leak_repeated_letters() {
    let mut h = harness(blank());
    // egui derives repeats from held-key state, so send presses across frames before release.
    events(&mut h, [key_event(Key::F10, true, Modifiers::NONE)]);
    events(&mut h, [key_event(Key::F10, true, Modifiers::NONE)]);
    assert_eq!(h.state().keytips.prefix(), Some(""));
    events(&mut h, [key_event(Key::F10, false, Modifiers::NONE)]);
    events(&mut h, [key_event(Key::H, true, Modifiers::NONE), Event::Text("h".into())]);
    events(&mut h, [key_event(Key::H, true, Modifiers::NONE), Event::Text("h".into())]);
    assert_eq!(h.state().keytips.prefix(), Some("H"));
    events(&mut h, [key_event(Key::H, false, Modifiers::NONE)]);
    letter(&mut h, Key::A, false);
    events(
        &mut h,
        [
            key_event(Key::R, true, Modifiers::NONE),
            Event::Text("r".into()),
            key_event(Key::R, true, Modifiers::NONE),
            Event::Text("r".into()),
            key_event(Key::R, false, Modifiers::NONE),
        ],
    );
    assert_eq!(alignment(&h), HAlign::Right);
    assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == "home.alignRight").count(), 1);
    assert!(h.state().editor.is_none());
    assert_eq!(value(&h, "A1"), Value::Empty);
}

#[test]
fn ordinary_typing_editor_shortcuts_and_altgr_remain_text_input() {
    let mut h = harness(blank());
    events(&mut h, [Event::Text("hello".into())]);
    key(&mut h, Key::F10);
    assert_eq!(h.state().keytips.prefix(), None, "F10 must not interrupt the cell editor");
    assert_eq!(h.state().editor.as_ref().unwrap().text, "hello");
    events(&mut h, [Event::Text("!".into())]);
    key(&mut h, Key::Enter);
    assert_eq!(value(&h, "A1"), Value::from("hello!"));

    let mut h = harness(blank());
    let altgr = Modifiers { alt: true, ctrl: true, ..Modifiers::NONE };
    events(&mut h, [Event::ModifiersChanged(altgr), key_event(Key::AltRight, true, altgr), key_event(Key::Q, true, altgr), Event::Text("@".into())]);
    assert_eq!(h.state().keytips.prefix(), None);
    assert_eq!(h.state().editor.as_ref().unwrap().text, "@");

    let mut h = harness_on(blank(), OperatingSystem::Mac);
    events(&mut h, [Event::ModifiersChanged(Modifiers::ALT), key_event(Key::AltLeft, true, Modifiers::ALT)]);
    events(&mut h, [Event::ModifiersChanged(Modifiers::NONE), key_event(Key::AltLeft, false, Modifiers::NONE), Event::Text("é".into())]);
    assert_eq!(h.state().keytips.prefix(), None, "macOS Option must remain available for text composition");
    assert_eq!(h.state().editor.as_ref().unwrap().text, "é");
}
