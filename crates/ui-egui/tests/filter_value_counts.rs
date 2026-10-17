//! AutoFilter counts stay separate from the checkbox labels and align at the list's right edge.

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

#[test]
fn filter_menu_shows_counts_for_values_and_blanks() {
    let mut session = Session::new();
    session.new_workbook();
    let mut values = vec![vec![json!("Payment")]];
    values.extend((0..12).map(|_| vec![json!("Card")]));
    values.extend(vec![vec![json!(null)]; 3]);
    values.push(vec![json!("A very long payment method that must leave room for its count")]);
    session.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
    session.execute("data.filter", json!({"range": "A1:A17"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key {
            key: Key::ArrowDown,
            physical_key: Some(Key::ArrowDown),
            pressed,
            repeat: false,
            modifiers: Modifiers::ALT,
        });
    }
    h.run_steps(4);
    let card = h.get_by_label("Card").rect();
    let blanks = h.get_by_label("(Blanks)").rect();
    let twelve = h.get_by_label("12").rect();
    let three = h.get_by_label("3").rect();
    let long = h.get_by_label("A very long payment method that must leave room for its count").rect();
    let one = h.get_by_label("1").rect();
    assert!((twelve.center().y - card.center().y).abs() < 1.0, "Card: {card:?}, count: {twelve:?}");
    assert!((three.center().y - blanks.center().y).abs() < 1.0, "Blanks: {blanks:?}, count: {three:?}");
    assert!(one.left() > long.right(), "long labels leave room for the count");
    assert!((one.right() - twelve.right()).abs() < 1.0);
    assert!((twelve.right() - three.right()).abs() < 1.0);
    h.input_mut().events.push(Event::Text("Card".into()));
    h.run_steps(2);
    assert!(h.query_by_label("(Blanks)").is_none());
    assert!(h.query_by_label("3").is_none());
    assert!(h.query_by_label("12").is_some(), "search keeps the full count of a matching value");
}
