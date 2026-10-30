//! Workbook Statistics, Accessibility, Error Checking and Evaluate Formula draw their reports as
//! labelled rows that go through the interface language, not as the engine's raw JSON.

use egui_kittest::kittest::{By, NodeT, Queryable};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn open(h: &mut Harness, name: &str) {
    h.state_mut().open_dialog(name, json!({}));
    h.run_steps(4);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some(name));
}

/// No text node shows a JSON key or brace, as the old monospace dump did.
fn no_raw_json(h: &Harness, keys: &[&str]) {
    for n in h.query_all(By::new()) {
        let a = n.accesskit_node();
        let Some(s) = a.label().or_else(|| a.value()) else { continue };
        assert!(!s.contains("\":"), "raw JSON on screen: {s:?}");
        for k in keys {
            assert!(!s.contains(k), "JSON key {k} on screen: {s:?}");
        }
    }
}

#[test]
fn workbook_statistics_lists_labelled_counts() {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
    h.state_mut().run("cell.set", json!({"cell": "C3", "input": "=SUM(A1:B2)"})).unwrap();
    open(&mut h, "statistics");
    assert!(h.query_by_label("Current sheet: Sheet1").is_some());
    assert!(h.query_by_label("End of sheet").is_some());
    assert!(h.query_by_label("C3").is_some(), "end of sheet");
    assert!(h.query_by_label("Workbook:").is_some());
    assert!(h.query_by_label("Names").is_some());
    no_raw_json(&h, &["cellsWithData", "endOfSheet"]);
}

#[test]
fn accessibility_lists_issues_or_says_there_are_none() {
    let mut h = crate::support::blank_harness(4);
    open(&mut h, "accessibility");
    assert!(h.query_by_label("Default sheet name").is_some());
    assert!(h.query_by_label("Issue").is_some());
    no_raw_json(&h, &["issue"]);

    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("sheet.rename", json!({"sheet": 0, "name": "Sales"})).unwrap();
    open(&mut h, "accessibility");
    assert!(h.query_by_label("No accessibility issues found.").is_some());
}

#[test]
fn error_checking_lists_error_cells_or_says_there_are_none() {
    let mut h = crate::support::blank_harness(4);
    open(&mut h, "errorChecking");
    assert!(h.query_by_label("No errors found.").is_some());

    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "=1/0"})).unwrap();
    open(&mut h, "errorChecking");
    assert!(h.query_by_label("Sheet1!B2").is_some());
    assert!(h.query_by_label("#DIV/0!").is_some());
    assert!(h.query_by_label("=1/0").is_some());
    no_raw_json(&h, &["circular", "errors"]);
}

#[test]
fn evaluate_formula_shows_the_result_or_why_there_is_none() {
    let mut h = crate::support::blank_harness(4);
    open(&mut h, "evaluateFormula");
    assert!(h.query_by_label("The cell has no formula.").is_some(), "the reason, not null");

    let mut h = crate::support::blank_harness(4);
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "=1+2"})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "A1"})).unwrap();
    h.run_steps(2);
    // A1 itself already shows 3: the dialog adds the step's value and the result.
    let before = h.query_all_by_label("3").count();
    open(&mut h, "evaluateFormula");
    assert!(h.query_by_label("Result").is_some());
    assert!(h.query_all_by_label("3").count() > before, "the result 3, not 3.0");
    no_raw_json(&h, &["steps", "expression"]);
}

#[test]
fn a_long_error_report_pages_through_every_row() {
    let mut h = crate::support::blank_harness(4);
    // Far below the visible grid, so every #DIV/0! on screen is a report row.
    for r in 1000..1250 {
        h.state_mut().run("cell.set", json!({"cell": format!("A{r}"), "input": "=1/0"})).unwrap();
    }
    open(&mut h, "errorChecking");
    assert!(h.query_by_label("Rows 1–200 of 250").is_some());
    assert_eq!(h.query_all_by_label("#DIV/0!").count(), 200);
    assert!(h.query_by_label("Sheet1!A1249").is_none());
    h.get_by_label("Next").click();
    h.run_steps(4);
    assert!(h.query_by_label("Rows 201–250 of 250").is_some());
    assert_eq!(h.query_all_by_label("#DIV/0!").count(), 50);
    assert!(h.query_by_label("Sheet1!A1249").is_some(), "the last finding is reachable");
    h.get_by_label("Previous").click();
    h.run_steps(4);
    assert!(h.query_by_label("Sheet1!A1000").is_some());
}

#[test]
fn a_long_formula_wraps_inside_its_column() {
    let mut h = crate::support::blank_harness(4);
    let formula = format!("=LEN(\"{}\")/0", "x".repeat(300));
    h.state_mut().run("cell.set", json!({"cell": "A1000", "input": formula})).unwrap();
    open(&mut h, "errorChecking");
    let width = h.get_by_label(&formula).rect().width();
    assert!(width <= 281.0, "the formula column is {width} wide");
}

#[test]
fn without_a_workbook_the_reports_keep_the_error_instead_of_finding_nothing() {
    // The window always keeps a workbook; an embedding host or the control channel may not.
    let mut app = crate::common::app(gridcraft_engine::Session::new());
    assert!(app.session.active().is_none());
    for name in ["statistics", "accessibility", "errorChecking"] {
        app.open_dialog(name, json!({}));
        let result = app.dialog.take().and_then(|d| d.result).unwrap_or_default();
        assert!(result.get("error").is_some(), "{name}: {result}");
    }
}
