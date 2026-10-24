//! Merged-cell edits must read and write the visible, top-left cell.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(input: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.mergeCells", json!({"range": "A1:B2"})).unwrap();
    session.execute("cell.set", json!({"cell": "A1", "input": input})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn cell_center(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center()
}

fn click_cell(h: &mut Harness, cell: &str) {
    let pos = cell_center(h, cell);
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

fn text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn key(h: &mut Harness, key: Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
}

fn value(h: &Harness, cell: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

#[test]
fn typing_into_a_covered_cell_remains_visible_after_clicking_away() {
    let mut h = harness("");
    click_cell(&mut h, "B2");
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "B2");
    text(&mut h, "hello");
    click_cell(&mut h, "D4");

    assert!(h.state().editor.is_none());
    assert_eq!(value(&h, "A1"), Value::from("hello"), "the cell painted for the merge must contain the committed text");
    assert_eq!(value(&h, "B2"), Value::Empty, "editing a merge must not hide text in a covered cell");
    click_cell(&mut h, "B2");
    assert!(h.query_by_label("hello").is_some(), "the formula bar must show the merged content when a covered cell is selected");
}

#[test]
fn f2_and_double_click_edit_the_existing_merged_content() {
    let mut h = harness("before");
    click_cell(&mut h, "B2");
    key(&mut h, Key::F2);
    let editor = h.state().editor.as_ref().unwrap();
    assert_eq!(editor.cell.a1(), "A1");
    assert_eq!(editor.text, "before");
    text(&mut h, "!");
    key(&mut h, Key::Enter);
    assert_eq!(value(&h, "A1"), Value::from("before!"));

    click_cell(&mut h, "D4");
    click_cell(&mut h, "B2");
    click_cell(&mut h, "B2");
    let editor = h.state().editor.as_ref().unwrap();
    assert_eq!(editor.cell.a1(), "A1");
    assert_eq!(editor.text, "before!");
    text(&mut h, " again");
    click_cell(&mut h, "D4");
    assert_eq!(value(&h, "A1"), Value::from("before! again"));
    assert_eq!(value(&h, "B2"), Value::Empty);
}

#[test]
fn formula_bar_reads_and_edits_the_merge_anchor_from_a_covered_cell() {
    let mut h = harness("formula bar text");
    click_cell(&mut h, "B2");
    h.get_by_label("formula bar text").click();
    h.run_steps(2);

    let editor = h.state().editor.as_ref().unwrap();
    assert!(editor.from_formula_bar);
    assert_eq!(editor.cell.a1(), "A1");
    assert_eq!(editor.text, "formula bar text");
    text(&mut h, " edited");
    key(&mut h, Key::Enter);
    assert_eq!(value(&h, "A1"), Value::from("formula bar text edited"));
    assert_eq!(value(&h, "B2"), Value::Empty);
    click_cell(&mut h, "D4");
    click_cell(&mut h, "B2");
    assert!(h.query_by_label("formula bar text edited").is_some());
}
