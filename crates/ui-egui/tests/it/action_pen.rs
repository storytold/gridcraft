//! Draw › Action Pen: the model panel stays reachable, and a tap is a stroke.
//!
//! The Action Pen button only toggles the tool when the model is ready, so a separate control has
//! to open model management (Download, Import…, Remove) without touching the tool. A tap is a
//! one-point stroke: the dot of an "i" or a decimal point never becomes a drag.

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    crate::support::blank_harness(4)
}

fn centre_of(h: &Harness, a1: &str) -> egui::Pos2 {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    let geo = h.state().grid.geo.clone().expect("grid geometry");
    geo.screen_cell_rect(sh, CellRef::parse(a1).unwrap()).center()
}

#[test]
fn the_model_control_opens_management_without_toggling_the_tool() {
    let mut h = harness();
    h.state_mut().ui.ribbon_tab = "Draw".into();
    h.run_steps(2);
    h.get_by_label("Model…").click();
    h.run_steps(1);
    assert!(h.state().grid.ink_panel, "the Model… control opens the model panel");
    assert_ne!(h.state().session.draw_tool, "actionPen", "and does not select the tool");
}

#[test]
fn an_action_pen_tap_is_a_one_point_stroke() {
    let mut session = crate::support::blank_session();
    session.ink = Some(std::sync::Arc::new(gridcraft_ink::FixedRecognizer::new("5")));
    session.execute("draw.actionPen", json!({"on": true})).unwrap();
    let mut h = crate::support::stepped_harness(session);
    let p = centre_of(&h, "C3");
    for event in [
        Event::PointerMoved(p),
        Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ] {
        h.input_mut().events.push(event);
        h.run_steps(1);
    }
    h.run_steps(2);
    let at = CellRef::parse("C3").unwrap();
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sh.value(at), Value::Number(5.0), "the tap is one stroke and becomes the cell's text");
}
