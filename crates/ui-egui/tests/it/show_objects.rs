//! Ctrl+6 hides and shows every object (Options › Advanced › "For objects, show"), through real input.

use egui::{Color32, Event, Key, Modifiers, PointerButton, epaint::Shape, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::ShowObjects;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const FILL: Color32 = Color32::from_rgb(0x12, 0x34, 0x56);
/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

/// A rectangle at D3, 144 × 96 points, filled with a colour nothing else on screen uses.
fn fixture() -> (Harness, u32) {
    let mut session = Session::new();
    session.new_workbook();
    let shape = session.execute("insert.shape", json!({"kind": "rectangle", "at": "D3", "fill": "123456"})).unwrap();
    let id = shape["shape"].as_u64().unwrap() as u32;
    session.execute("selection.set", json!({"range": "A1"})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    (h, id)
}

fn ctrl_6(h: &mut Harness) {
    h.input_mut().events.push(Event::Key { key: Key::Num6, physical_key: Some(Key::Num6), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key: Key::Num6, physical_key: Some(Key::Num6), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(2);
}

fn show_objects(h: &Harness) -> ShowObjects {
    h.state().session.active().unwrap().wb.show_objects
}

/// Whether the last frame painted the rectangle's fill.
fn painted(h: &Harness) -> bool {
    h.output().shapes.iter().any(|c| matches!(&c.shape, Shape::Rect(r) if r.fill == FILL))
}

/// Inside the rectangle, in the cell E4 (D3 is 64 points wide and 20 high at 100%).
fn inside(h: &Harness) -> egui::Pos2 {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sh, CellRef::parse("E4").unwrap()).center()
}

fn click(h: &mut Harness, pos: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(2);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
}

fn active_cell(h: &Harness) -> CellRef {
    h.state().session.active().unwrap().selection.active
}

#[test]
fn ctrl_6_hides_objects_and_a_second_ctrl_6_shows_them() {
    let (mut h, _) = fixture();
    assert_eq!(show_objects(&h), ShowObjects::All);
    assert!(painted(&h), "the rectangle is drawn to begin with");
    ctrl_6(&mut h);
    assert_eq!(show_objects(&h), ShowObjects::None);
    assert!(!painted(&h), "hidden objects are not drawn");
    ctrl_6(&mut h);
    assert_eq!(show_objects(&h), ShowObjects::All);
    assert!(painted(&h), "a second Ctrl+6 draws them again");
}

#[test]
fn hidden_objects_do_not_take_clicks() {
    let (mut h, id) = fixture();
    let at = inside(&h);
    click(&mut h, at);
    assert_eq!(h.state().selected_chart, Some(id), "the click lands on the rectangle while it shows");
    assert_eq!(active_cell(&h), CellRef::parse("A1").unwrap());
    ctrl_6(&mut h);
    assert_eq!(h.state().selected_chart, None, "hiding objects drops the selection on one");
    click(&mut h, at);
    assert_eq!(h.state().selected_chart, None);
    assert_eq!(active_cell(&h), CellRef::parse("E4").unwrap(), "the click selects the cell under the hidden rectangle");
}
