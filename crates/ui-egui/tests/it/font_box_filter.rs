//! The Home ▸ Font list grows back when a filter that matched nothing is cleared, and its filter
//! field stays where it is (egui#5225).

use egui::{Event, Key, Modifiers, accesskit::Role};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

fn key(h: &mut Harness, key: Key) {
    h.input_mut().events.extend([
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(3);
}

fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.to_string()));
    h.run_steps(3);
}

fn text_inputs(h: &Harness) -> Vec<egui::Rect> {
    h.get_all_by_role(Role::TextInput).map(|n| n.rect()).collect()
}

/// The bottom of the Font list's ninth row, if that row is on screen.
fn ninth_row_bottom(h: &Harness) -> Option<f32> {
    h.query_by_label("Garamond").map(|n| n.rect().bottom())
}

#[test]
fn font_list_grows_back_after_a_filter_is_cleared() {
    let mut h = harness();
    let before = text_inputs(&h);
    let font = h.state().session.active().unwrap().wb.active().unwrap().style_id(gridcraft_engine::core::CellRef::parse("A1").unwrap());
    let name = h.state().session.active().unwrap().wb.styles.get(font).font.name.clone();
    h.get_by_label(&name).click();
    h.run_steps(4);
    let open: Vec<_> = text_inputs(&h).into_iter().filter(|r| !before.contains(r)).collect();
    assert_eq!(open.len(), 1, "the open list has one filter field");
    let field = open[0];
    let full = ninth_row_bottom(&h).expect("the full list shows its ninth row");

    type_text(&mut h, "zzz");
    assert!(ninth_row_bottom(&h).is_none(), "a filter that matches nothing leaves no rows");
    assert!(text_inputs(&h).contains(&field), "the filter field stays put while the list is empty");

    for _ in 0..3 {
        key(&mut h, Key::Backspace);
    }
    assert_eq!(ninth_row_bottom(&h), Some(full), "the cleared filter shows the full list again");
    assert!(text_inputs(&h).contains(&field), "the filter field stays put after the list regrows");
}
