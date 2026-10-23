//! Format Cells › Fill: creating, cancelling and replacing a gradient through real controls.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Color, Fill, PatternType};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.run("range.setValues", json!({"range": "A1", "values": [[1]]})).unwrap();
    session.run("selection.set", json!({"range": "A1"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn fill(h: &egui_kittest::Harness<'static, SheetApp>, cell: &str) -> Fill {
    let doc = h.state().session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    doc.wb.styles.get(sheet.style_id(CellRef::parse(cell).unwrap())).fill.clone()
}

#[test]
fn gradient_can_be_created_cancelled_and_replaced_by_solid() {
    let mut h = harness();
    assert_eq!(fill(&h, "A1"), Fill::default());

    // Cancel keeps the original fill.
    h.state_mut().open_dialog("formatCells", json!({"tab": "Fill"}));
    h.run_steps(6);
    h.get_by_label("Gradient").click();
    h.run_steps(6);
    h.get_by_label(" Cancel ").click();
    h.run_steps(6);
    assert_eq!(fill(&h, "A1"), Fill::default());

    // OK applies a two-stop linear gradient.
    h.state_mut().open_dialog("formatCells", json!({"tab": "Fill"}));
    h.run_steps(6);
    h.get_by_label("Gradient").click();
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    let applied = fill(&h, "A1");
    let gradient = applied.gradient.as_ref().expect("gradient fill");
    assert!(gradient.is_linear());
    assert_eq!(gradient.stops.len(), 2);

    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(fill(&h, "A1"), Fill::default());

    // Switching the same cell to Solid clears the gradient rather than leaving both active.
    h.state_mut().open_dialog("formatCells", json!({"tab": "Fill"}));
    h.run_steps(6);
    h.get_by_label("Gradient").click();
    h.run_steps(6);
    h.get_by_label("Solid").click();
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    let solid = fill(&h, "A1");
    assert_eq!(solid.pattern, PatternType::Solid);
    assert!(solid.gradient.is_none());
}

fn drag(h: &mut egui_kittest::Harness<'static, SheetApp>, from: Pos2, to: Pos2) {
    let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(from), press(from, true)]);
    h.step();
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    h.input_mut().events.push(press(to, false));
    h.run_steps(3);
}

fn stop_slider(n: &egui_kittest::Node<'_>) -> bool {
    let r = n.rect();
    r.left() > 400.0 && r.top() > 340.0 && r.top() < 500.0
}

fn focused_stop(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<usize> {
    h.get_all_by_role(Role::Slider).filter(|n| stop_slider(n)).position(|n| n.accesskit_node().is_focused())
}

fn tab_until(h: &mut egui_kittest::Harness<'static, SheetApp>, want: usize) {
    for _ in 0..400 {
        if focused_stop(h) == Some(want) {
            return;
        }
        h.key_press(Key::Tab);
        h.step();
    }
    panic!("could not focus gradient stop {want}");
}

fn shift_tab_until(h: &mut egui_kittest::Harness<'static, SheetApp>, want: usize) {
    for _ in 0..20 {
        if focused_stop(h) == Some(want) {
            return;
        }
        h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
        h.step();
    }
    panic!("could not focus gradient stop {want} backwards");
}

#[test]
fn dragging_a_stop_past_its_neighbour_keeps_editing_that_stop() {
    let mut h = harness();
    h.state_mut().open_dialog("formatCells", json!({"tab": "Fill"}));
    h.run_steps(6);
    h.get_by_label("Gradient").click();
    h.run_steps(6);

    let stops: Vec<_> = h.get_all_by_role(Role::Slider).filter(|n| stop_slider(n)).collect();
    assert_eq!(stops.len(), 2, "two gradient stop sliders");
    let (first_rect, second_rect) = (stops[0].rect(), stops[1].rect());

    // Move the second stop to roughly the middle.
    drag(
        &mut h,
        pos2(second_rect.right() - 5.0, second_rect.center().y),
        pos2(second_rect.left() + second_rect.width() * 0.5, second_rect.center().y),
    );
    // Drag the first stop from 0% past the second to roughly 80%.
    drag(&mut h, pos2(first_rect.left() + 5.0, first_rect.center().y), pos2(first_rect.left() + first_rect.width() * 0.8, first_rect.center().y));

    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    let applied = fill(&h, "A1");
    let gradient = applied.gradient.expect("gradient fill");
    let first = gradient.stops.iter().find(|s| s.color == Color::Theme(4, 0)).expect("start stop");
    let second = gradient.stops.iter().find(|s| s.color == Color::Theme(0, 0)).expect("end stop");
    assert!(first.position > second.position, "the dragged stop kept its identity: {gradient:?}");
    assert!(first.position > 0.6, "first stop should be near 80%: {gradient:?}");
    assert!((0.3..0.7).contains(&second.position), "second stop should be near 50%: {gradient:?}");
}

#[test]
fn keyboard_editing_a_stop_past_its_neighbour_keeps_editing_that_stop() {
    let mut h = harness();
    h.state_mut().open_dialog("formatCells", json!({"tab": "Fill"}));
    h.run_steps(6);
    h.get_by_label("Gradient").click();
    h.run_steps(6);

    // Focus the second stop with Tab and move it to roughly 50%.
    tab_until(&mut h, 1);
    for _ in 0..50 {
        h.key_press(Key::ArrowLeft);
        h.step();
    }

    // Move focus back to the first stop and drag its value past the second.
    shift_tab_until(&mut h, 0);
    for _ in 0..80 {
        h.key_press(Key::ArrowRight);
        h.step();
    }
    h.get_by_label("   OK   ").click();
    h.run_steps(6);

    let applied = fill(&h, "A1");
    let gradient = applied.gradient.expect("gradient fill");
    let first = gradient.stops.iter().find(|s| s.color == Color::Theme(4, 0)).expect("start stop");
    let second = gradient.stops.iter().find(|s| s.color == Color::Theme(0, 0)).expect("end stop");
    assert!(first.position > second.position, "the keyboard-edited stop kept its identity: {gradient:?}");
    assert!(first.position > 0.6, "first stop should be near 80%: {gradient:?}");
    assert!((0.3..0.7).contains(&second.position), "second stop should be near 50%: {gradient:?}");
}
