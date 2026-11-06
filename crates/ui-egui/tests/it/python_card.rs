//! Python cells on the sheet: the PY tag, the card (a cached result, or the reason a cell shows
//! #BLOCKED!), its place on right-to-left sheets, the return-type switch, and Convert to Normal
//! Cell.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::PyOutput;
use gridcraft_ui_egui::python_card::{self, CONVERT, Card, MAX_PREVIEW_CHARS};
use serde_json::json;

use crate::common::Harness;

/// What the catalog says about a cell while Python is off, as it is in these tests.
const OFF: &str = "Python is off. Turn it on in Settings to run Python cells.";

/// B2 holds new code (#BLOCKED!); C4 a cached DataFrame shown as a Python object, with a preview
/// far longer than the card shows; E2 a cached list shown as values.
fn session(rtl: bool) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("python.set", json!({"cell": "B2", "code": "x = 1"})).unwrap();
    s.execute("python.set", json!({"cell": "C4", "code": "df", "output": "object"})).unwrap();
    s.execute("python.set", json!({"cell": "E2", "code": "[1, 2, 3]"})).unwrap();
    let long = (0..2_000).map(|i| format!("row {i}")).collect::<Vec<_>>().join("\n");
    cache(
        &mut s,
        "C4",
        PyOutput { value: Value::text("DataFrame"), type_name: "DataFrame".into(), preview: long, convertible: false, ..Default::default() },
    );
    let list = gridcraft_engine::core::Array::new(3, 1, vec![1.0.into(), 2.0.into(), 3.0.into()]).map(Value::from).unwrap();
    cache(&mut s, "E2", PyOutput { value: list, type_name: "list".into(), preview: "[1, 2, 3]".into(), convertible: true, ..Default::default() });
    // Lay the cached results out, as opening a file does.
    s.execute("formulas.calculateNow", json!({})).unwrap();
    if rtl {
        s.execute("view.rightToLeft", json!({"on": true})).unwrap();
    }
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    s
}

fn cache(s: &mut Session, cell: &str, output: PyOutput) {
    let at = CellRef::parse(cell).unwrap();
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    let c = wb.sheet_mut(0).unwrap().cells.get_mut(at).unwrap();
    c.value = output.value.clone();
    std::sync::Arc::make_mut(c.py.as_mut().unwrap()).output = Some(output);
}

fn harness(session: Session) -> Harness {
    crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 800.0)), crate::common::app(session), 4)
}

fn press(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
    h.run_steps(3);
}

fn select(h: &mut Harness, cell: &str) {
    h.state_mut().session.execute("selection.set", json!({"cell": cell})).unwrap();
    h.run_steps(2);
}

fn click(h: &mut Harness, at: Pos2) {
    for event in [
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ] {
        h.input_mut().events.push(event);
        h.run_steps(1);
    }
    h.run_steps(2);
}

/// A cell's area and its PY tag on screen.
fn on_screen(h: &Harness, cell: &str) -> (Rect, Option<Rect>) {
    let app = h.state();
    let d = app.session.active().unwrap();
    let sh = d.wb.active().unwrap();
    let geo = app.grid.geo.as_ref().unwrap();
    let c = CellRef::parse(cell).unwrap();
    (geo.screen_cell_rect(sh, c), python_card::tag_at(geo, &d.wb, sh, c).map(|t| geo.mirror_rect(t)))
}

fn card(h: &Harness) -> Option<CellRef> {
    h.state().grid.py_card.map(|c: Card| c.cell)
}

fn card_rect(h: &Harness) -> Option<Rect> {
    h.ctx.memory(|m| m.area_rect(egui::Id::unique("python_card")))
}

/// The painted "PY" labels: the tags on the sheet (and the badge, when one shows).
fn tag_labels(h: &Harness) -> Vec<Rect> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "PY" => Some(Rect::from_min_size(t.pos, t.galley.size())),
            _ => None,
        })
        .collect()
}

#[test]
fn python_cells_carry_a_py_tag_and_a_click_on_it_opens_the_card() {
    let mut h = harness(session(false));
    let tags = tag_labels(&h);
    assert_eq!(tags.len(), 3, "one tag per Python cell, and no badge on A1: {tags:?}");
    for cell in ["B2", "C4", "E2"] {
        let (area, tag) = on_screen(&h, cell);
        let tag = tag.expect("a tag");
        assert!(area.contains_rect(tag), "{cell}'s tag is in its cell");
        assert!(tag.center().x < area.center().x, "at its start");
        assert!(tags.iter().any(|t| tag.contains(t.center())), "{cell}'s tag is painted where it is hit");
    }
    // A click on the tag selects the cell and opens its card.
    let tag = on_screen(&h, "B2").1.unwrap();
    click(&mut h, tag.center());
    assert_eq!(card(&h), Some(CellRef::parse("B2").unwrap()));
    assert_eq!(h.state().session.active().map(|d| d.selection.active), Some(CellRef::new(1, 1)));
    // New code has no result: the card says why the cell shows #BLOCKED!, with what to do.
    assert!(h.query_by_label(OFF).is_some(), "the card explains #BLOCKED!");
    assert!(h.query_by_label("Open Settings").is_some());
    assert!(h.query_by_label("#BLOCKED!").is_some());
    assert!(h.query_by_label("Excel Value").is_some(), "and names the return type");
    h.check_accessibility();
    // Esc closes it; a click elsewhere in the cell only selects.
    press(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(card(&h), None);
    let (area, _) = on_screen(&h, "B2");
    click(&mut h, area.right_center() - vec2(4.0, 0.0));
    assert_eq!(card(&h), None);
}

#[test]
fn ctrl_shift_f5_opens_the_card_of_a_cached_result_with_its_preview_capped() {
    let mut h = harness(session(false));
    // Not on an ordinary cell.
    press(&mut h, Key::F5, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(card(&h), None);
    assert!(h.state().message.is_none());
    select(&mut h, "C4");
    press(&mut h, Key::F5, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(card(&h), Some(CellRef::parse("C4").unwrap()));
    assert!(h.query_by_label("DataFrame").is_some(), "the result's Python type");
    assert!(h.query_by_label("Python Object").is_some());
    let d = h.state().session.active().unwrap();
    let text = python_card::card_text(&d.wb, d.wb.active().unwrap(), CellRef::parse("C4").unwrap()).unwrap();
    assert!(text.body.starts_with("row 0\nrow 1"));
    assert!(text.body.chars().count() <= MAX_PREVIEW_CHARS + 1 && text.body.ends_with('…'), "a huge preview is cut");
    assert!(h.query_by_label(&text.body).is_some(), "the preview shows");
    // A spilled value's card is its anchor's.
    press(&mut h, Key::Escape, Modifiers::NONE);
    select(&mut h, "E3");
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("E3").unwrap()), Value::Number(2.0));
    press(&mut h, Key::F5, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(card(&h), Some(CellRef::parse("E2").unwrap()));
    // The preview, and the code in the formula bar, which here read the same.
    assert_eq!(h.query_all_by_label("[1, 2, 3]").count(), 2);
    // Editing closes it.
    press(&mut h, Key::F2, Modifiers::NONE);
    assert_eq!(card(&h), None);
}

#[test]
fn the_card_and_the_tag_are_mirrored_on_a_right_to_left_sheet() {
    let mut ltr = harness(session(false));
    let mut rtl = harness(session(true));
    for h in [&mut ltr, &mut rtl] {
        select(h, "C4");
        h.state_mut().run("ui.pythonCard", json!({})).unwrap();
        h.run_steps(3);
    }
    let (cell, tag) = on_screen(&ltr, "C4");
    let card = card_rect(&ltr).expect("the card is shown");
    assert!((card.left() - cell.left()).abs() < 1.5, "left to right: the card starts at the cell's left edge ({card:?}, {cell:?})");
    assert!(card.top() >= cell.bottom() - 0.5, "under the cell");
    assert!(tag.unwrap().center().x < cell.center().x);
    let (cell, tag) = on_screen(&rtl, "C4");
    let card = card_rect(&rtl).expect("the card is shown");
    assert!((card.right() - cell.right()).abs() < 1.5, "right to left: the card ends at the cell's right edge ({card:?}, {cell:?})");
    assert!(card.top() >= cell.bottom() - 0.5);
    let tag = tag.unwrap();
    assert!(tag.center().x > cell.center().x, "the tag sits at the cell's start, on the right");
    assert!(tag_labels(&rtl).iter().any(|t| tag.contains(t.center())), "and is painted there");
    // The click on the mirrored tag opens it too.
    rtl.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    rtl.run_steps(2);
    click(&mut rtl, tag.center());
    assert_eq!(rtl.state().grid.py_card.map(|c| c.cell), Some(CellRef::parse("C4").unwrap()));
}

fn output(h: &mut Harness, cell: &str) -> String {
    h.state_mut().session.execute("cell.get", json!({"cell": cell})).unwrap()["python"]["output"].as_str().unwrap_or_default().to_string()
}

#[test]
fn ctrl_alt_shift_m_and_the_cell_menu_switch_the_return_type() {
    let mut h = harness(session(false));
    let ctrl_alt_shift = Modifiers { alt: true, ctrl: true, shift: true, mac_cmd: false, command: true };
    // On an ordinary cell the shortcut does nothing (no error box).
    press(&mut h, Key::M, ctrl_alt_shift);
    assert!(h.state().message.is_none());
    select(&mut h, "E2");
    press(&mut h, Key::M, ctrl_alt_shift);
    assert_eq!(output(&mut h, "E2"), "object");
    let v = h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("E2").unwrap());
    // The cached list was the Excel value's result: the new return type has none until the code
    // runs, as with python.set.
    assert_eq!(v, Value::Error(gridcraft_engine::core::CellError::Blocked), "a result of the other return type goes");
    press(&mut h, Key::M, ctrl_alt_shift);
    assert_eq!(output(&mut h, "E2"), "value");
    // The cell menu (Shift+F10) has both return types; picking one switches to it.
    press(&mut h, Key::F10, Modifiers::SHIFT);
    assert!(h.state().grid.context_menu.is_some());
    h.get_by_label("Python Object").click();
    h.run_steps(3);
    assert_eq!(output(&mut h, "E2"), "object");
    assert!(h.state().grid.context_menu.is_none(), "the menu closes");
    // Not on an ordinary cell.
    select(&mut h, "A1");
    press(&mut h, Key::F10, Modifiers::SHIFT);
    assert!(h.query_by_label("Excel Value").is_none());
}

#[test]
fn ui_python_card_opens_only_on_python_cells_and_never_a_dialog() {
    let mut h = harness(session(false));
    assert!(h.state_mut().run("ui.pythonCard", json!({})).is_err(), "A1 isn't a Python cell");
    select(&mut h, "B2");
    let r = h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    assert_eq!(r["cell"], "B2");
    h.run_steps(2);
    assert!(card_rect(&h).is_some());
    assert!(h.state().dialog.is_none() && h.state().message.is_none());
    assert_eq!(h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap()["open"], false);
    h.run_steps(2);
    assert_eq!(card(&h), None);
    // A Python cell scrolled out of view closes its card.
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"cell": "A500"})).unwrap();
    h.state_mut().grid.ensure_visible = true;
    h.run_steps(4);
    assert_eq!(card(&h), None);
}

#[test]
fn a_python_cell_that_shows_an_error_gets_the_warning_button_which_says_why() {
    let mut h = harness(session(false));
    select(&mut h, "B2");
    // egui waits for the pointer to settle before opening a tooltip: remove the delay.
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    h.get_by_label("Error checking options").hover();
    h.run_steps(6);
    assert!(h.query_by_value(OFF).is_some() || h.query_by_label(OFF).is_some(), "hovering the button says why the cell shows #BLOCKED!");
    h.get_by_label("Error checking options").click();
    h.run_steps(3);
    assert!(h.query_by_label("#BLOCKED!").is_some(), "the menu names the error");
    h.get_by_label("Show Card").click();
    h.run_steps(3);
    assert_eq!(card(&h), Some(CellRef::parse("B2").unwrap()));
}

#[test]
fn python_cells_with_a_result_and_other_cells_get_no_warning_button() {
    let mut h = harness(session(false));
    for cell in ["C4", "E2", "A1"] {
        select(&mut h, cell);
        assert!(h.query_by_label("Error checking options").is_none(), "{cell}");
    }
}

#[test]
fn the_warning_button_says_python_did_not_run_only_for_blocked() {
    // A file can cache another Python error, such as #PYTHON! from Excel: the button names it
    // but doesn't say this version didn't run the code.
    let mut s = session(false);
    s.execute("python.set", json!({"cell": "G2", "code": "1 / 0"})).unwrap();
    let error = Value::Error(gridcraft_engine::core::CellError::Python);
    cache(
        &mut s,
        "G2",
        PyOutput { value: error, type_name: "ZeroDivisionError".into(), preview: String::new(), convertible: true, ..Default::default() },
    );
    s.execute("formulas.calculateNow", json!({})).unwrap();
    let mut h = harness(s);
    select(&mut h, "G2");
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    h.get_by_label("Error checking options").hover();
    h.run_steps(6);
    assert!(h.query_by_value(OFF).is_some() || h.query_by_label(OFF).is_some(), "the hover says what the catalog says");
    h.get_by_label("Error checking options").click();
    h.run_steps(3);
    assert!(h.query_by_label("#PYTHON!").is_some(), "the menu names the error");
    assert!(h.query_by_label("Show Card").is_some(), "the menu still opens the card");
}

fn is_python(h: &mut Harness, cell: &str) -> bool {
    !h.state_mut().session.execute("cell.get", json!({"cell": cell})).unwrap()["python"].is_null()
}

fn value(h: &Harness, cell: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

#[test]
fn the_cell_menu_converts_a_python_cell_to_a_normal_cell() {
    let mut h = harness(session(false));
    // On a spilled value the menu acts on its anchor, E2, whose list spills over E2:E4.
    select(&mut h, "E3");
    press(&mut h, Key::F10, Modifiers::SHIFT);
    h.get_by_label(CONVERT).click();
    h.run_steps(3);
    assert!(h.state().grid.context_menu.is_none(), "the menu closes");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    assert!(!is_python(&mut h, "E2"));
    for (c, n) in [("E2", 1.0), ("E3", 2.0), ("E4", 3.0)] {
        assert_eq!(value(&h, c), Value::Number(n), "{c} keeps what it showed");
    }
    // An ordinary cell's menu has no such item.
    press(&mut h, Key::F10, Modifiers::SHIFT);
    assert!(h.query_by_label(CONVERT).is_none());
    press(&mut h, Key::Escape, Modifiers::NONE);
    // A locked Python cell on a protected sheet has it, turned off.
    h.state_mut().session.execute("review.protectSheet", json!({})).unwrap();
    select(&mut h, "C4");
    press(&mut h, Key::F10, Modifiers::SHIFT);
    assert!(h.get_by_label(CONVERT).accesskit_node().is_disabled());
    h.get_by_label(CONVERT).click();
    h.run_steps(3);
    assert!(is_python(&mut h, "C4"));
}

#[test]
fn the_card_converts_its_python_cell_to_a_normal_cell() {
    let mut h = harness(session(false));
    select(&mut h, "C4");
    press(&mut h, Key::F5, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(card(&h), Some(CellRef::parse("C4").unwrap()));
    h.get_by_label(CONVERT).click();
    h.run_steps(3);
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    assert!(!is_python(&mut h, "C4"));
    assert_eq!(value(&h, "C4"), Value::text("DataFrame"), "the cell keeps what it showed");
    assert_eq!(card(&h), None, "the card closes with its Python cell");
    // One undo step brings the Python cell back, with its result.
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert!(is_python(&mut h, "C4"));
    assert_eq!(value(&h, "C4"), Value::text("DataFrame"));
}

/// The card belongs to its cell: moving the selection away closes it, whichever way it moves.
#[test]
fn moving_the_selection_away_closes_the_card() {
    let mut h = harness(session(false));
    select(&mut h, "B2");
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(2);
    assert!(card(&h).is_some());
    select(&mut h, "A1");
    h.run_steps(2);
    assert!(card(&h).is_none());
}
