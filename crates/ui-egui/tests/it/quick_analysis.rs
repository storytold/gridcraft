//! Selecting a range of cells shows the Quick Analysis button at its bottom-right corner; its
//! pop-out applies one-click formatting, charts, totals and tables to the selection.

use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(selection: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4], [5, 6]]})).unwrap();
    session.execute("selection.set", json!({"range": selection})).unwrap();
    crate::support::stepped_harness(session)
}

fn rules(h: &Harness) -> usize {
    h.state().session.active().unwrap().wb.active().unwrap().cond_formats.len()
}

#[test]
/// A single cell has no Quick Analysis button; a range of cells has one.
fn the_button_shows_for_a_range_only() {
    let h = harness("A1");
    assert!(h.query_by_label("Quick Analysis").is_none());
    let h = harness("A1:B3");
    assert!(h.query_by_label("Quick Analysis").is_some());
}

#[test]
/// Choosing Data Bars in the pop-out adds a conditional format over the selection.
fn the_formatting_tab_applies_data_bars() {
    let mut h = harness("A1:B3");
    h.get_by_label("Quick Analysis").click();
    h.run_steps(4);
    h.get_by_label("Data Bars").click();
    h.run_steps(4);
    assert_eq!(rules(&h), 1);
}

#[test]
/// The Totals tab writes the chosen total under each column of the selection.
fn the_totals_tab_sums_the_columns() {
    let mut h = harness("A1:B3");
    h.get_by_label("Quick Analysis").click();
    h.run_steps(4);
    h.get_by_label("Totals").click();
    h.run_steps(4);
    h.get_by_label("Sum").click();
    h.run_steps(4);
    let d = h.state().session.active().unwrap();
    let a4 = d.wb.active().unwrap().cell(gridcraft_engine::core::CellRef::parse("A4").unwrap());
    assert!(a4.is_some_and(|c| c.formula.is_some()), "A4 should hold a SUM formula");
}
