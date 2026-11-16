//! #4389 and #4390: a selected chart outlines its data in the sheet (series names red, category
//! labels purple, values blue), and dragging a corner of the values outline changes what the
//! chart plots, through real pointer input.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Shape, vec2};
use gridcraft_engine::core::RangeRef;
use gridcraft_ui_egui::theme::Tokens;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, gridcraft_ui_egui::SheetApp>;

/// 10..50 down A1:A5 and 5s below them, charted from A1:A5, which stays the cell selection as
/// after Insert › Chart, so the fill handle sits on the outline's bottom-right corner.
fn fixture() -> (Harness, u32) {
    let mut session = crate::support::blank_session();
    let values: Vec<_> = [10, 20, 30, 40, 50, 5, 5, 5].iter().map(|v| json!([v])).collect();
    session.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
    session.execute("selection.set", json!({"range": "A1:A5"})).unwrap();
    let chart = session.execute("insert.chart", json!({"type": "column", "at": "D3", "width": 300, "height": 200})).unwrap();
    let id = chart["chart"].as_u64().unwrap() as u32;
    let mut h = crate::support::stepped_harness(session);
    h.state_mut().select_object_id(Some(id));
    h.run_steps(2);
    (h, id)
}

fn values(h: &Harness) -> String {
    h.state().session.doc().unwrap().wb.active().unwrap().charts[0].series[0].values.clone()
}

/// On-screen rect of `range`.
fn rect(h: &Harness, range: &str) -> egui::Rect {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    app.grid.geo.as_ref().unwrap().range_rect(sh, RangeRef::parse(range).unwrap())
}

fn hover(h: &mut Harness, at: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
}

fn button(h: &mut Harness, at: Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// Presses at `from` and moves to `to` in small steps, leaving the button down.
fn press_and_move(h: &mut Harness, from: Pos2, to: Pos2) {
    hover(h, from);
    button(h, from, true);
    for i in 1..=8 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 8.0)));
        h.run_steps(1);
    }
}

#[test]
fn dragging_the_values_corner_extends_the_chart_data() {
    let (mut h, id) = fixture();
    assert_eq!(values(&h), "Sheet1!$A$1:$A$5");
    let corner = rect(&h, "A1:A5").right_bottom();
    hover(&mut h, corner);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeNwSe, "the corner shows a resize cursor, not the fill cross");
    let target = rect(&h, "A8").center();
    press_and_move(&mut h, corner, target);
    // The outline previews the new block while the button is held; the chart is unchanged.
    let blue = Tokens::get(&h.ctx).ref_color(0, true);
    let preview = rect(&h, "A1:A8");
    let previewed = h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Rect(r) if r.rect == preview && r.stroke.color == blue));
    assert!(previewed, "the outline follows the pointer");
    assert_eq!(values(&h), "Sheet1!$A$1:$A$5");
    button(&mut h, target, false);
    assert_eq!(values(&h), "Sheet1!$A$1:$A$8", "the chart plots the dragged block");
    assert_eq!(h.state().selected_chart(), Some(id), "the chart stays selected");
    let a6 = h.state().session.doc().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse("A6").unwrap());
    assert_eq!(a6, gridcraft_engine::core::Value::number(5.0), "the press did not drag the fill handle");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(values(&h), "Sheet1!$A$1:$A$5", "one undo step puts the block back");
}

#[test]
fn dragging_the_top_corner_down_shrinks_the_chart_data() {
    let (mut h, _) = fixture();
    let corner = rect(&h, "A1:A5").left_top() + vec2(1.0, 1.0);
    let target = rect(&h, "A3").center();
    press_and_move(&mut h, corner, target);
    button(&mut h, target, false);
    assert_eq!(values(&h), "Sheet1!$A$3:$A$5");
}

#[test]
fn escape_cancels_a_values_drag() {
    let (mut h, _) = fixture();
    let corner = rect(&h, "A1:A5").right_bottom();
    let target = rect(&h, "A8").center();
    press_and_move(&mut h, corner, target);
    h.key_press(Key::Escape);
    h.run_steps(2);
    button(&mut h, target, false);
    assert_eq!(values(&h), "Sheet1!$A$1:$A$5");
}

#[test]
fn a_selected_chart_outlines_names_categories_and_values_in_their_colours() {
    let mut session = crate::support::blank_session();
    session
        .execute("range.setValues", json!({"range": "A1", "values": [["", "North", "South"], ["Q1", 10, 15], ["Q2", 20, 25], ["Q3", 30, 35]]}))
        .unwrap();
    session.execute("selection.set", json!({"range": "A1:C4"})).unwrap();
    let chart = session.execute("insert.chart", json!({"type": "column", "at": "F2", "width": 300, "height": 200})).unwrap();
    let mut h = crate::support::stepped_harness(session);
    h.state_mut().select_object_id(Some(chart["chart"].as_u64().unwrap() as u32));
    h.run_steps(2);
    let t = Tokens::get(&h.ctx);
    let stroked = |h: &Harness, range: &str, color: egui::Color32| {
        let want = rect(h, range);
        h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Rect(r) if r.rect == want && r.stroke.color == color && r.stroke.width == 1.5))
    };
    assert!(stroked(&h, "B1:C1", t.ref_color(1, true)), "series names in red");
    assert!(stroked(&h, "A2:A4", t.ref_color(2, true)), "category labels in purple");
    assert!(stroked(&h, "B2:C4", t.ref_color(0, true)), "the values as one blue block");
    assert!(!stroked(&h, "B2:B4", t.ref_color(0, true)), "not one outline per series");
}
