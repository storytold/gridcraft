//! Pointing in Python mode: while the code waits for a value, clicking, dragging or arrowing on the
//! sheet inserts an `xl("…")` reference, as Excel does; each step replaces the reference the
//! pointing session inserted, and typing ends the session. Driven through real egui events.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use serde_json::json;

use crate::common::Harness;

fn harness(session: Session) -> Harness {
    crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 800.0)), crate::common::app(session), 4)
}

/// A blank workbook in Python mode on A1, with `code` typed.
fn python(code: &str) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    python_in(s, code)
}

fn python_in(session: Session, code: &str) -> Harness {
    let mut h = harness(session);
    send(&mut h, press(Key::P, Modifiers { alt: true, ctrl: true, shift: true, mac_cmd: false, command: true }));
    assert!(h.state().editor.as_ref().is_some_and(|e| e.python), "Python mode is on");
    if !code.is_empty() {
        send(&mut h, [Event::Text(code.into())]);
    }
    h
}

fn press(key: Key, modifiers: Modifiers) -> [Event; 2] {
    [
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

fn send(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(3);
}

fn code(h: &Harness) -> String {
    h.state().editor.as_ref().map(|e| e.text.clone()).unwrap_or_default()
}

fn cell_center(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    app.grid.geo.as_ref().unwrap().screen_cell_rect(sh, CellRef::parse(a1).unwrap()).center()
}

fn click(h: &mut Harness, a1: &str, modifiers: Modifiers) {
    let at = cell_center(h, a1);
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        h.input_mut().events.extend([Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers }]);
        h.step();
    }
    h.run_steps(2);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(1);
}

fn drag(h: &mut Harness, from: &str, to: &str) {
    let (a, b) = (cell_center(h, from), cell_center(h, to));
    h.input_mut()
        .events
        .extend([Event::PointerMoved(a), Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    h.step();
    for i in 1..=6 {
        h.input_mut().events.push(Event::PointerMoved(a + (b - a) * (i as f32 / 6.0)));
        h.step();
    }
    h.input_mut().events.push(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
    h.run_steps(3);
}

fn selection(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.current().a1()
}

#[test]
fn a_click_inserts_an_xl_reference_and_a_second_click_replaces_it() {
    let mut h = python("df = ");
    click(&mut h, "B2", Modifiers::NONE);
    assert_eq!(code(&h), r#"df = xl("B2")"#);
    assert_eq!(selection(&h), "A1", "the selection stays on the edited cell");
    assert!(h.state().editor.as_ref().is_some_and(|e| e.python), "the edit goes on in Python mode");
    click(&mut h, "D5", Modifiers::NONE);
    assert_eq!(code(&h), r#"df = xl("D5")"#, "the session's reference is replaced, not stacked");
    click(&mut h, "B3", Modifiers::SHIFT);
    assert_eq!(code(&h), r#"df = xl("B3:D5")"#, "Shift+click extends it to a range");
    click(&mut h, "F1", Modifiers::COMMAND);
    assert_eq!(code(&h), r#"df = xl("B3:D5"), xl("F1")"#, "Ctrl+click adds another reference");
}

#[test]
fn a_drag_inserts_a_range() {
    let mut h = python("df = ");
    drag(&mut h, "B2", "C4");
    assert_eq!(code(&h), r#"df = xl("B2:C4")"#);
    assert_eq!(selection(&h), "A1");
}

#[test]
fn arrows_move_the_pointed_cell_and_shift_arrows_extend_it() {
    let mut h = python("x = ");
    click(&mut h, "B2", Modifiers::NONE);
    send(&mut h, press(Key::ArrowDown, Modifiers::NONE));
    assert_eq!(code(&h), r#"x = xl("B3")"#);
    send(&mut h, press(Key::ArrowRight, Modifiers::NONE));
    assert_eq!(code(&h), r#"x = xl("C3")"#);
    send(&mut h, press(Key::ArrowDown, Modifiers::SHIFT));
    send(&mut h, press(Key::ArrowRight, Modifiers::SHIFT));
    assert_eq!(code(&h), r#"x = xl("C3:D4")"#);
    send(&mut h, press(Key::ArrowUp, Modifiers::SHIFT));
    assert_eq!(code(&h), r#"x = xl("C3:D3")"#);
}

#[test]
fn typing_ends_the_pointing_and_a_new_click_starts_a_second_reference() {
    let mut h = python("f(");
    click(&mut h, "B2", Modifiers::NONE);
    send(&mut h, [Event::Text(", ".into())]);
    assert_eq!(code(&h), r#"f(xl("B2"), "#);
    assert_eq!(h.state().editor.as_ref().and_then(|e| e.point), None, "typing ended the session");
    click(&mut h, "C3", Modifiers::NONE);
    assert_eq!(code(&h), r#"f(xl("B2"), xl("C3")"#);
    // Typed text after a reference: an arrow edits the code again rather than pointing.
    send(&mut h, [Event::Text(")".into())]);
    send(&mut h, press(Key::ArrowLeft, Modifiers::NONE));
    assert_eq!(code(&h), r#"f(xl("B2"), xl("C3"))"#);
    assert_eq!(h.state().editor.as_ref().map(|e| e.caret), Some(r#"f(xl("B2"), xl("C3")"#.len()), "the arrow moved the caret");
}

#[test]
fn moving_the_caret_ends_the_pointing() {
    let mut h = python("x = ");
    click(&mut h, "B2", Modifiers::NONE);
    send(&mut h, press(Key::Home, Modifiers::NONE));
    assert_eq!(h.state().editor.as_ref().and_then(|e| e.point), None);
    // At the start of the line a click inserts a new reference there.
    click(&mut h, "C3", Modifiers::NONE);
    assert_eq!(code(&h), r#"xl("C3")x = xl("B2")"#);
}

#[test]
fn no_pointing_after_an_identifier_and_a_click_commits_the_code() {
    let mut h = python("abc");
    click(&mut h, "B2", Modifiers::NONE);
    assert!(h.state().editor.is_none(), "the click committed the code, as before");
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.cell(CellRef::new(0, 0)).and_then(|c| c.py.as_ref()).map(|p| p.code.as_str()), Some("abc"));
    assert_eq!(selection(&h), "B2");
}

#[test]
fn no_pointing_inside_a_string() {
    let mut h = python("s = 'B ");
    click(&mut h, "B2", Modifiers::NONE);
    assert!(h.state().editor.is_none(), "the click committed the code, with no reference in the string");
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.cell(CellRef::new(0, 0)).and_then(|c| c.py.as_ref()).map(|p| p.code.as_str()), Some("s = 'B "));
}

#[test]
fn whole_columns_and_rows_from_the_headers() {
    let mut h = python("x = ");
    let header = |h: &Harness, rows: bool, index: u32| {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = gridcraft_ui_egui::grid::Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        if rows {
            egui::pos2(geo.rect.left() + geo.header_w * 0.5, geo.y(sheet, index) + sheet.row_height(index) * geo.z * 0.5)
        } else {
            egui::pos2(geo.x(sheet, index) + sheet.col_width(index) * geo.z * 0.5, geo.rect.top() + geo.header_h * 0.5)
        }
    };
    let click_at = |h: &mut Harness, at: Pos2, modifiers: Modifiers| {
        h.input_mut().events.push(Event::ModifiersChanged(modifiers));
        for pressed in [true, false] {
            h.input_mut()
                .events
                .extend([Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers }]);
            h.step();
        }
        h.run_steps(2);
        h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
        h.run_steps(1);
    };
    let at = header(&h, false, 0);
    click_at(&mut h, at, Modifiers::NONE);
    assert_eq!(code(&h), r#"x = xl("A:A")"#);
    let at = header(&h, false, 2);
    click_at(&mut h, at, Modifiers::SHIFT);
    assert_eq!(code(&h), r#"x = xl("A:C")"#, "Shift extends the pointed columns");
    let at = header(&h, true, 0);
    click_at(&mut h, at, Modifiers::NONE);
    assert_eq!(code(&h), r#"x = xl("1:1")"#);
    let at = header(&h, true, 2);
    click_at(&mut h, at, Modifiers::SHIFT);
    assert_eq!(code(&h), r#"x = xl("1:3")"#);
}

/// A workbook whose second sheet is `name`, in Python mode on Sheet1!A1 with `code` typed, then
/// showing that sheet as a click on its tab would.
fn on_other_sheet(name: &str, code: &str) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.insertSheet", json!({"name": name})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    let mut h = python_in(s, code);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    h.run_steps(3);
    h
}

#[test]
fn another_sheet_is_named_as_formulas_name_it() {
    let mut h = on_other_sheet("Sheet2", "x = ");
    click(&mut h, "B2", Modifiers::NONE);
    assert_eq!(code(&h), r#"x = xl("Sheet2!B2")"#);
    send(&mut h, press(Key::ArrowDown, Modifiers::NONE));
    assert_eq!(code(&h), r#"x = xl("Sheet2!B3")"#, "an arrow keeps the sheet");
    send(&mut h, press(Key::ArrowRight, Modifiers::SHIFT));
    assert_eq!(code(&h), r#"x = xl("Sheet2!B3:C3")"#);
    drag(&mut h, "A1", "B2");
    assert_eq!(code(&h), r#"x = xl("Sheet2!A1:B2")"#);
}

#[test]
fn a_sheet_name_with_a_space_or_an_apostrophe_is_quoted() {
    let mut h = on_other_sheet("My Sheet", "x = ");
    click(&mut h, "B2", Modifiers::NONE);
    assert_eq!(code(&h), r#"x = xl("'My Sheet'!B2")"#);
    let mut h = on_other_sheet("Bob's", "x = ");
    drag(&mut h, "B2", "C4");
    assert_eq!(code(&h), r#"x = xl("'Bob''s'!B2:C4")"#);
}

#[test]
fn a_double_quote_in_a_sheet_name_is_escaped_in_the_python_string() {
    let mut h = on_other_sheet(r#"Say "hi""#, "x = ");
    click(&mut h, "B2", Modifiers::NONE);
    assert_eq!(code(&h), r#"x = xl("'Say \"hi\"'!B2")"#);
    send(&mut h, press(Key::ArrowDown, Modifiers::SHIFT));
    assert_eq!(code(&h), r#"x = xl("'Say \"hi\"'!B2:B3")"#, "the escaped reference is read back to extend it");
}

#[test]
fn a_tab_click_while_pointing_keeps_the_code_open() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.insertSheet", json!({"name": "Inputs"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    let mut h = python_in(s, "x = ");
    let tab = h.output().shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.job.text == "Inputs" => Some(t.visual_bounding_rect().center()),
        _ => None,
    });
    let at = tab.expect("the tab is painted");
    for pressed in [true, false] {
        h.input_mut()
            .events
            .extend([Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        h.step();
    }
    h.run_steps(3);
    assert_eq!(h.state().session.active().unwrap().wb.active_sheet(), 1);
    assert!(h.state().editor.as_ref().is_some_and(|e| e.python), "the draft stays open on the other sheet");
    click(&mut h, "A1", Modifiers::NONE);
    assert_eq!(code(&h), r#"x = xl("Inputs!A1")"#);
}
