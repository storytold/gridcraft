//! The in-cell editor keeps the text where the cell shows it: its alignment, in a merge too (#948).

use egui::epaint::Shape;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(commands: &[(&str, serde_json::Value)]) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    for (id, params) in commands {
        session.execute(id, params.clone()).unwrap();
    }
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

fn range_rect(h: &Harness, range: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.range_rect(sheet, RangeRef::parse(range).unwrap())
}

fn click(h: &mut Harness, pos: Pos2) {
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

fn key(h: &mut Harness, key: Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(2);
}

fn collect_text(shape: &Shape, text: &str, inside: Rect, out: &mut Vec<Rect>) {
    match shape {
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect_text(s, text, inside, out)),
        Shape::Text(t) if t.galley.text() == text => {
            let r = t.galley.rect.translate(t.pos.to_vec2());
            if inside.contains(r.center()) {
                out.push(r);
            }
        }
        _ => {}
    }
}

/// The on-screen box of the one `text` painted inside `inside` (the formula bar shows it too).
fn text_rect(h: &Harness, text: &str, inside: Rect) -> Rect {
    let mut found = Vec::new();
    for s in &h.output().shapes {
        collect_text(&s.shape, text, inside, &mut found);
    }
    assert_eq!(found.len(), 1, "expected one {text:?} inside {inside:?}, found {found:?}");
    found[0]
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 3.0
}

#[test]
fn editing_a_centred_merge_keeps_the_text_centred_both_ways() {
    let mut h = harness(&[
        ("home.mergeCells", json!({"range": "B2:D5"})),
        ("home.alignCenter", json!({"range": "B2:D5"})),
        ("home.alignMiddle", json!({"range": "B2:D5"})),
        ("cell.set", json!({"cell": "B2", "input": "hello"})),
    ]);
    let merge = range_rect(&h, "B2:D5");
    let shown = text_rect(&h, "hello", merge);
    assert!(near(shown.center().x, merge.center().x) && near(shown.center().y, merge.center().y), "painted {shown:?} in {merge:?}");

    click(&mut h, merge.center());
    key(&mut h, Key::F2);
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(2);
    assert_eq!(h.state().editor.as_ref().unwrap().text, "hellox");
    let editing = text_rect(&h, "hellox", merge);
    assert!(near(editing.center().x, shown.center().x), "editor text {editing:?} is not centred like {shown:?}");
    assert!(near(editing.center().y, shown.center().y), "editor text {editing:?} is not in the middle like {shown:?}");
}

#[test]
fn editing_a_number_in_a_tall_wide_cell_keeps_it_bottom_right() {
    let mut h = harness(&[
        ("home.columnWidth", json!({"cols": "B:B", "chars": 30})),
        ("home.rowHeight", json!({"rows": "2:2", "height": 60})),
        ("cell.set", json!({"cell": "B2", "input": "42"})),
    ]);
    let cell = range_rect(&h, "B2:B2");
    let shown = text_rect(&h, "42", cell);
    assert!(near(shown.right(), cell.right() - 2.5) && shown.bottom() > cell.center().y, "painted {shown:?} in {cell:?}");

    click(&mut h, cell.center());
    key(&mut h, Key::F2);
    h.input_mut().events.push(Event::Text("0".into()));
    h.run_steps(2);
    assert!(h.state().editor.as_ref().is_some_and(|e| e.cell == CellRef::parse("B2").unwrap() && e.text == "420"));
    let editing = text_rect(&h, "420", cell);
    assert!(near(editing.right(), shown.right()), "a General number edits right-aligned like {shown:?}, got {editing:?}");
    assert!(near(editing.bottom(), shown.bottom()), "the default bottom alignment holds while editing: {editing:?} vs {shown:?}");
}
