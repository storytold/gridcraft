//! Quick Analysis: the floating button past a numeric selection, its card, and when it hides.

use egui::{Event, Key, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::quick_analysis;
use serde_json::json;

fn harness(range: &str) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Q1", "Q2"], ["a", 1, 4], ["b", 2, 5], ["c", 3, 6]]})).unwrap();
    session.execute("selection.set", json!({"range": range})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(6);
    h
}

fn button(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<egui::Rect> {
    quick_analysis::button_rect(&h.ctx)
}

#[test]
fn the_button_shows_past_a_numeric_block_and_hides_for_a_single_cell() {
    let mut h = harness("B2:C4");
    let b = button(&h).expect("a block of numbers offers Quick Analysis");
    let corner = {
        let app = h.state();
        let geo = app.grid.geo.as_ref().unwrap();
        let sh = app.session.active().unwrap().wb.active().unwrap();
        geo.range_rect(sh, gridcraft_engine::core::RangeRef::parse("B2:C4").unwrap()).right_bottom()
    };
    assert!(b.left() > corner.x && b.top() > corner.y, "the button sits past the corner: {b:?} vs {corner:?}");
    assert!(b.left() - corner.x < 12.0 && b.top() - corner.y < 12.0, "and close to it: {b:?} vs {corner:?}");

    h.state_mut().session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    h.run_steps(3);
    assert!(button(&h).is_none(), "a single cell has no Quick Analysis");
}

#[test]
fn text_only_selections_and_editing_hide_the_button() {
    let mut h = harness("A1:A4");
    assert!(button(&h).is_none(), "a block without numbers has no Quick Analysis");
    h.state_mut().session.execute("selection.set", json!({"range": "B2:C4"})).unwrap();
    h.run_steps(3);
    assert!(button(&h).is_some());
    h.state_mut().begin_edit(Some("7".into()), false);
    h.run_steps(3);
    assert!(button(&h).is_none(), "editing a cell hides it");
}

#[test]
fn dragging_a_selection_hides_the_button() {
    let mut h = harness("B2:C4");
    let start = button(&h).expect("shown before the drag").center() + vec2(-120.0, -60.0);
    h.input_mut().events.push(Event::PointerMoved(start));
    h.input_mut().events.push(Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerMoved(start + vec2(40.0, 40.0)));
    h.run_steps(2);
    assert!(button(&h).is_none(), "hidden while the pointer drags on the grid");
    h.input_mut().events.push(Event::PointerButton {
        pos: start + vec2(40.0, 40.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    h.run_steps(3);
}

#[test]
fn the_card_runs_a_tile_for_the_selection_and_escape_closes_it() {
    let mut h = harness("B2:C4");
    h.get_by_label("Quick Analysis").click();
    h.run_steps(3);
    assert!(quick_analysis::is_open(&h.ctx), "the button opens the card");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!quick_analysis::is_open(&h.ctx), "Escape closes it");

    h.get_by_label("Quick Analysis").click();
    h.run_steps(3);
    h.get_by_label("Data Bars").click();
    h.run_steps(3);
    let rules = h.state().session.active().unwrap().wb.active().unwrap().cond_formats.len();
    assert_eq!(rules, 1, "Data Bars added a conditional format");
    assert!(!quick_analysis::is_open(&h.ctx), "running a tile closes the card");

    h.get_by_label("Quick Analysis").click();
    h.run_steps(3);
    h.get_by_label("Totals").click();
    h.run_steps(2);
    h.get_by_label("Sum").click();
    h.run_steps(3);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap().clone();
    let total = sheet.value(gridcraft_engine::core::CellRef::parse("B5").unwrap());
    assert_eq!(total, gridcraft_engine::core::Value::Number(6.0), "Sum wrote a total under column B");
}

#[test]
fn running_total_adds_the_columns_up_to_each() {
    let h = harness("B2:C4");
    let range = gridcraft_engine::core::RangeRef::parse("B2:C4").unwrap();
    let (at, values) = quick_analysis::running_total(h.state(), range).expect("room below");
    assert_eq!(at, "B5");
    assert_eq!(values, json!([["=SUM($B$2:B4)", "=SUM($B$2:C4)"]]));
}

#[test]
fn a_whole_column_running_total_goes_just_past_the_data() {
    let h = harness("B:C");
    let range = gridcraft_engine::core::RangeRef::parse("B:C").unwrap();
    let (at, values) = quick_analysis::running_total(h.state(), range).expect("room below");
    assert_eq!(at, "B5", "not the sheet's last row");
    assert_eq!(values, json!([["=SUM($B$1:B4)", "=SUM($B$1:C4)"]]));
}

#[test]
fn a_click_elsewhere_closes_the_card() {
    let mut h = harness("B2:C4");
    h.get_by_label("Quick Analysis").click();
    h.run_steps(3);
    assert!(quick_analysis::is_open(&h.ctx));
    let far = egui::pos2(1100.0, 700.0);
    h.input_mut().events.push(Event::PointerMoved(far));
    h.input_mut().events.push(Event::PointerButton { pos: far, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.run_steps(1);
    h.input_mut().events.push(Event::PointerButton { pos: far, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
    assert!(!quick_analysis::is_open(&h.ctx), "a click outside closes it");
}
