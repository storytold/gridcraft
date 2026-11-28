//! Clicking an item in a cell's validation list picker — the arrow, the popup, and the row
//! click itself — recalculates a formula that tests the cell (#3570).
//!
//! The unit test in `grid/tests.rs` enters each pick with the same `cell.set` call the picker
//! makes, so it cannot catch a picker whose rows stop applying the item they show. This test
//! drives the picker the way a user does, forward and back through the list.

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A1 is a Yes/No dropdown and B1 is `=IF(A1="Yes","shown","hidden")`.
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("data.validation", json!({"range": "A1", "type": "list", "formula1": "\"Yes,No\""})).unwrap();
    session.execute("cell.set", json!({"cell": "B1", "input": "=IF(A1=\"Yes\",\"shown\",\"hidden\")"})).unwrap();
    session.execute("selection.set", json!({"range": "A1"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn click_at(h: &mut Harness, pos: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// Opens A1's picker by clicking its validation arrow, then clicks the row showing `item`.
fn pick(h: &mut Harness, item: &str) {
    let (sh, geo) = {
        let app = h.state();
        let sh = app.session.active().unwrap().wb.active().unwrap().clone();
        let geo = grid::Geo::new(&sh, app.grid.rect.unwrap(), app.view().scroll);
        (sh, geo)
    };
    let arrow = grid::validation_arrow(&sh, &geo, CellRef::new(0, 0)).unwrap().center();
    click_at(h, arrow);
    assert!(h.state().grid.list_picker.is_some(), "the arrow opens the picker");
    h.get_by_label(item).click();
    h.run_steps(2);
    assert!(h.state().grid.list_picker.is_none(), "picking {item} closes the picker");
}

fn value(h: &Harness, cell: CellRef) -> Value {
    h.state().session.active().unwrap().wb.sheet(0).unwrap().value(cell)
}

#[test]
fn clicking_picker_items_recalculates_an_if_that_tests_the_cell() {
    let mut h = harness();
    let a1 = CellRef::new(0, 0);
    let b1 = CellRef::new(0, 1);
    assert_eq!(value(&h, b1), Value::Text("hidden".into()), "no pick yet");
    for (item, outcome) in [("Yes", "shown"), ("No", "hidden"), ("Yes", "shown")] {
        pick(&mut h, item);
        assert_eq!(value(&h, a1), Value::Text(item.into()), "A1 after picking {item}");
        assert_eq!(value(&h, b1), Value::Text(outcome.into()), "B1 after picking {item}");
        assert!(h.state().message.is_none(), "picking {item} was refused: {:?}", h.state().message);
    }
}
