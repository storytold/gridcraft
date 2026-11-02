//! Goal Seek: clicking or dragging on the grid writes the cell's absolute reference into the Set
//! cell or By changing cell box, whichever has the keyboard, without a leading `=`.

use egui::{Event, Modifiers, PointerButton, Pos2, accesskit::Role, pos2, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"range": "B2"})).unwrap();
    let mut h = crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h.state_mut().open_dialog("goalSeek", json!({}));
    h.run_steps(6);
    h
}

fn value(h: &Harness, key: &str) -> String {
    h.state().dialog.as_ref().and_then(|d| d.values.get(key)).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn clear(h: &mut Harness, key: &str) {
    if let Some(d) = h.state_mut().dialog.as_mut() {
        d.values.insert(key.into(), json!(""));
    }
    h.run_steps(2);
}

fn press(h: &mut Harness, pos: Pos2) {
    for pressed in [true, false] {
        h.input_mut()
            .events
            .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        h.step();
    }
    h.run_steps(3);
}

/// Clicks the text box on the row of the dialog label `label`, giving it the keyboard.
fn focus_box(h: &mut Harness, label: &str) {
    let label = h.get_by_label(label).rect();
    let on_row = |r: &egui::Rect| r.left() > label.right() && r.top() <= label.center().y && r.bottom() >= label.center().y;
    let field = h.get_all_by_role(Role::TextInput).map(|n| n.rect()).find(on_row).unwrap();
    press(h, field.center());
}

fn cell_center(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let c = gridcraft_engine::core::CellRef::parse(a1).unwrap();
    pos2(geo.x(sheet, c.col) + sheet.col_width(c.col) * geo.z * 0.5, geo.y(sheet, c.row) + sheet.row_height(c.row) * geo.z * 0.5)
}

fn click(h: &mut Harness, a1: &str) {
    let pos = cell_center(h, a1);
    press(h, pos);
}

fn drag(h: &mut Harness, from: &str, to: &str) {
    let (a, b) = (cell_center(h, from), cell_center(h, to));
    h.input_mut()
        .events
        .extend([Event::PointerMoved(a), Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    h.step();
    for i in 1..=6 {
        let p = a + (b - a) * (i as f32 / 6.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.step();
    }
    h.input_mut().events.push(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.step();
    h.run_steps(3);
}

#[test]
fn the_set_cell_box_takes_a_cell_clicked_on_the_grid() {
    let mut h = harness();
    assert_eq!(value(&h, "set"), "B2", "the box starts with the active cell");
    clear(&mut h, "set");
    focus_box(&mut h, "Set cell:");
    click(&mut h, "D5");
    assert_eq!(value(&h, "set"), "$D$5");
    assert_eq!(value(&h, "changing"), "");
}

#[test]
fn dragging_into_the_set_cell_box_writes_a_range() {
    let mut h = harness();
    clear(&mut h, "set");
    focus_box(&mut h, "Set cell:");
    drag(&mut h, "D5", "E6");
    assert_eq!(value(&h, "set"), "$D$5:$E$6");
}

#[test]
fn the_by_changing_cell_box_takes_a_click_once_it_has_the_keyboard() {
    let mut h = harness();
    focus_box(&mut h, "By changing cell:");
    click(&mut h, "D5");
    assert_eq!(value(&h, "changing"), "$D$5");
    assert_eq!(value(&h, "set"), "B2", "the Set cell box is left alone");

    // Back to the Set cell box: the next click goes there, not into By changing cell.
    clear(&mut h, "set");
    focus_box(&mut h, "Set cell:");
    click(&mut h, "E6");
    assert_eq!(value(&h, "set"), "$E$6");
    assert_eq!(value(&h, "changing"), "$D$5");
}
