//! Space on a checkbox cell toggles it and nothing else (#1951).

use egui::{Event, Key, Modifiers, PointerButton};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::core::Value;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// A space, as a keyboard reports it: the key press, the text it produces, then the release.
fn space(h: &mut Harness) {
    let k = |pressed| Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed, repeat: false, modifiers: Modifiers::NONE };
    h.input_mut().events.push(k(true));
    h.input_mut().events.push(Event::Text(" ".into()));
    h.input_mut().events.push(k(false));
    h.run_steps(2);
}

fn press(h: &mut Harness, key: Key) {
    let k = |pressed| Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: Modifiers::NONE };
    h.input_mut().events.push(k(true));
    h.input_mut().events.push(k(false));
    h.run_steps(2);
}

fn value(h: &Harness, a1: &str) -> Option<Value> {
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().cell(CellRef::parse(a1).unwrap()).map(|c| c.value.clone())
}

#[test]
fn space_toggles_a_checkbox_cell_without_editing_it() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "B2", "input": "FALSE"})).unwrap();
    s.execute("insert.checkbox", json!({"range": "B2"})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    let mut h = harness(s);
    assert_eq!(value(&h, "B2"), Some(Value::Bool(false)));

    // The key press toggles the box; the text it also produces must not start editing, or Enter
    // commits that space over the checkbox and leaves the cell blank.
    space(&mut h);
    assert_eq!(value(&h, "B2"), Some(Value::Bool(true)));
    assert!(h.state().editor.is_none(), "no editor opened");
    press(&mut h, Key::Enter);
    assert_eq!(value(&h, "B2"), Some(Value::Bool(true)), "Enter moved on and left the checkbox");

    // And again, back to unchecked. Reached without clicking the box, since a click on the box
    // toggles it in its own right (below): an ordinary cell, then across to it.
    click_cell(&mut h, "D2");
    press(&mut h, Key::ArrowLeft);
    press(&mut h, Key::ArrowLeft);
    assert_eq!(active(&h), "B2", "the arrows walked back to the checkbox");
    space(&mut h);
    assert_eq!(value(&h, "B2"), Some(Value::Bool(false)));
    assert!(h.state().editor.is_none());

    // A click on the box toggles it too, which is what the pointer path does with a checkbox
    // cell and the reason the walk above avoids it.
    click_cell(&mut h, "B2");
    assert_eq!(value(&h, "B2"), Some(Value::Bool(true)), "the click toggled the box");
    assert!(h.state().editor.is_none(), "no editor opened");

    // A space still types on an ordinary cell.
    click_cell(&mut h, "C3");
    space(&mut h);
    assert!(h.state().editor.is_some(), "an ordinary cell starts editing");
    press(&mut h, Key::Escape);
}

/// Click a cell, as a person selects one: the grid's own geometry gives where it is, so the
/// click goes through hit-testing and whatever the grid does with focus, rather than the
/// selection being set behind the app's back once it is running (#1960 review).
fn click_cell(h: &mut Harness, cell: &str) {
    let pos = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center()
    };
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
    // The click has to have landed: a selection that did not move would leave each assertion
    // after it testing the cell before it instead.
    assert_eq!(active(h), cell, "the click selected {cell}");
}

/// The active cell, in A1 form.
fn active(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.active.to_string()
}
