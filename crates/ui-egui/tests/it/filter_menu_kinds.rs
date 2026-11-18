//! The filter menu follows the column's kind: Excel's sort labels and Number / Text / Date
//! Filters submenu, whose items open Custom AutoFilter or Top 10 or filter straight away.

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Name (text), Qty (numbers) and When (dates relative to today, so the periods hold any day).
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "range.setValues",
            json!({"range": "A1", "values": [["Name", "Qty", "When"], ["Red", 1, "=TODAY()"], ["Blue", 5, "=TODAY()-400"], ["Green", 9, "=TODAY()+1"]]}),
        )
        .unwrap();
    session.execute("home.numberFormat", json!({"range": "C2:C4", "code": "yyyy-mm-dd"})).unwrap();
    session.execute("data.filter", json!({"range": "A1:C4"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn open_filter(h: &mut Harness, header: &str) {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let pos = grid::filter_button_rect(&geo, sh, CellRef::parse(header).unwrap()).center();
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert!(h.state().grid.filter_menu.is_some(), "the filter menu opens on {header}");
}

fn hidden(h: &Harness) -> Vec<u32> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    (1..=3).filter(|r| sh.is_row_hidden(*r)).collect()
}

fn choose(h: &mut Harness, submenu: &str, item: &str) {
    h.get_by_label(submenu).click();
    h.run_steps(3);
    h.get_by_label(item).click();
    h.run_steps(4);
}

#[test]
fn sort_labels_and_filters_submenu_follow_the_column_kind() {
    for (header, sort, filters) in
        [("A1", "Sort A to Z", "Text Filters"), ("B1", "Sort Smallest to Largest", "Number Filters"), ("C1", "Sort Oldest to Newest", "Date Filters")]
    {
        let mut h = harness();
        open_filter(&mut h, header);
        assert!(h.query_by_label_contains(sort).is_some(), "{header}: {sort}");
        assert!(h.query_by_label(filters).is_some(), "{header}: {filters}");
        assert!(h.query_by_label("OK").is_some(), "{header}: OK");
    }
}

#[test]
fn greater_than_opens_custom_autofilter_and_filters_the_column() {
    let mut h = harness();
    open_filter(&mut h, "B1");
    choose(&mut h, "Number Filters", "Greater Than…");
    assert!(h.state().grid.filter_menu.is_none(), "the menu closes for the dialog");
    let d = h.state().dialog.as_ref().expect("Custom AutoFilter opens");
    assert_eq!((d.name.as_str(), d.values["op"].as_str()), ("customAutoFilter", Some(">")));
    confirm(&mut h, "value", json!("4"));
    assert!(h.state().dialog.is_none());
    assert_eq!(hidden(&h), vec![1], "only Qty 1 is not greater than 4");

    // The column is filtered now, so Clear Filter From “Qty” is on and clears it.
    open_filter(&mut h, "B1");
    h.get_by_label("Clear Filter From “Qty”").click();
    h.run_steps(3);
    assert!(hidden(&h).is_empty());
}

#[test]
fn top_10_keeps_the_largest_items() {
    let mut h = harness();
    open_filter(&mut h, "B1");
    choose(&mut h, "Number Filters", "Top 10…");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("top10AutoFilter"));
    confirm(&mut h, "count", json!(1));
    assert_eq!(hidden(&h), vec![1, 2], "only the largest (9) stays");
}

#[test]
fn a_date_period_filters_without_a_dialog() {
    let mut h = harness();
    open_filter(&mut h, "C1");
    choose(&mut h, "Date Filters", "Today");
    assert!(h.state().dialog.is_none());
    assert_eq!(hidden(&h), vec![2, 3], "only today's row stays");
}

#[test]
fn text_filters_offer_begins_with() {
    let mut h = harness();
    open_filter(&mut h, "A1");
    choose(&mut h, "Text Filters", "Begins With…");
    let d = h.state().dialog.as_ref().expect("Custom AutoFilter opens");
    assert_eq!(d.values["op"].as_str(), Some("beginsWith"));
    confirm(&mut h, "value", json!("gr"));
    assert_eq!(hidden(&h), vec![1, 2], "only Green begins with gr");
}

/// Fills one field and presses OK, through the control channel as a host program would.
fn confirm(h: &mut Harness, field: &str, value: serde_json::Value) {
    let ctx = h.ctx.clone();
    for (method, params) in [("ui.dialog.set", json!({"field": field, "value": value})), ("ui.dialog.confirm", json!({}))] {
        match gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, method, &params) {
            gridcraft_ui_egui::control::Outcome::Done(r) => assert_eq!(r["ok"], json!(true), "{method}: {r}"),
            gridcraft_ui_egui::control::Outcome::Screenshot { .. } => panic!("{method}"),
        }
    }
    h.run_steps(4);
}

#[test]
fn filter_by_color_choices_are_named_by_their_colour() {
    let mut h = harness();
    h.state_mut().run("home.fillColor", json!({"range": "A3", "color": "#FF0000"})).unwrap();
    h.run_steps(2);
    open_filter(&mut h, "A1");
    // Each swatch has a name a screen reader reads (and the same text on hover).
    choose(&mut h, "Filter by Color", "Cell Color #FF0000");
    assert_eq!(hidden(&h), vec![1, 3], "only Blue (red fill) stays");
    open_filter(&mut h, "A1");
    h.get_by_label("Filter by Color").click();
    h.run_steps(3);
    assert!(h.query_by_label("No Fill").is_some(), "the no-fill choice keeps its text");
}
