//! A list validation's input message must not cover the dropdown it describes (#505).

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const MESSAGE: &str = "Pick a colour from the list";

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("data.validation", json!({"range": "B2", "type": "list", "formula1": "\"Red,Green,Blue\"", "inputMessage": MESSAGE})).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn message_shown(h: &Harness) -> bool {
    h.query_by_label(MESSAGE).is_some()
}

#[test]
fn input_message_hides_while_the_list_is_open() {
    let mut h = harness();
    assert!(message_shown(&h), "the selected cell's input message is shown");

    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let pos = grid::validation_arrow(sh, &geo, CellRef::parse("B2").unwrap()).unwrap().center();
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);

    assert!(h.state().grid.list_picker.is_some(), "the dropdown arrow opens the list");
    assert!(h.query_by_label("Green").is_some(), "the list shows its items");
    assert!(!message_shown(&h), "the input message must not cover the open list");

    h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(h.state().grid.list_picker.is_none(), "Escape closes the list");
    assert!(message_shown(&h), "the input message comes back once the list is closed");
}
