//! Automatic calculated columns: entering one formula fills the table's data column.
//! Reference: Microsoft Support, "Use calculated columns in an Excel table".
use gridcraft_engine::{
    Session,
    core::{CellRef, Value},
};
use serde_json::json;

fn table() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "values":[
            ["Qty", "Price", "Total"], [2, 10, null], [3, 20, null], [4, 30, null], [5, 40, null]
        ]}),
    )
    .unwrap();
    s.execute("insert.table", json!({"range":"A1:C5", "header":true, "name":"Sales"})).unwrap();
    s
}

fn value(s: &Session, address: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(address).unwrap())
}

fn assert_totals(s: &Session) {
    for (address, expected) in [("C2", 20.0), ("C3", 60.0), ("C4", 120.0), ("C5", 200.0)] {
        assert_eq!(value(s, address), Value::Number(expected), "{address}");
    }
    assert_eq!(value(s, "C1"), Value::text("Total"), "the header must remain intact");
    assert_eq!(value(s, "C6"), Value::Empty, "the fill must stop at the table boundary");
    assert!(
        s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.is_some(),
        "the calculated-column formula must be recorded for XLSX persistence"
    );
}

#[test]
fn structured_formula_entered_in_the_middle_fills_above_and_below() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=[@Qty]*[@Price]"})).unwrap();
    assert_totals(&s);
}

#[test]
fn relative_references_follow_each_destination_row() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).unwrap();
    assert_totals(&s);
}

#[test]
fn calculated_column_creation_is_one_undoable_action() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=[@Qty]*[@Price]"})).unwrap();
    assert_totals(&s);
    s.execute("edit.undo", json!({})).unwrap();
    for address in ["C2", "C3", "C4", "C5"] {
        assert_eq!(value(&s, address), Value::Empty, "undo must restore {address}");
    }
    assert!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.is_none());
    s.execute("edit.redo", json!({})).unwrap();
    assert_totals(&s);
}

#[test]
fn existing_data_is_not_silently_overwritten() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C4", "input":999})).unwrap();
    s.execute("cell.set", json!({"cell":"C3", "input":"=[@Qty]*[@Price]"})).unwrap();
    assert_eq!(value(&s, "C4"), Value::Number(999.0));
    assert_eq!(value(&s, "C3"), Value::Number(60.0));
    assert_eq!(value(&s, "C2"), Value::Empty);
    assert_eq!(value(&s, "C5"), Value::Empty);
    assert!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.is_none());
}

#[test]
fn bulk_writes_do_not_turn_into_automatic_column_fills() {
    let mut s = table();
    s.execute("range.setValues", json!({"range":"C3", "values":[["=A3*B3"]]})).unwrap();
    assert_eq!(value(&s, "C3"), Value::Number(60.0));
    assert_eq!(value(&s, "C2"), Value::Empty);
    assert_eq!(value(&s, "C5"), Value::Empty);
}

#[test]
fn automatic_fill_can_be_disabled_without_disabling_formula_calculation() {
    let mut s = table();
    assert_eq!(s.execute("options.fillTableFormulas", json!({"on":false})).unwrap()["on"], false);
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).unwrap();
    assert_eq!(value(&s, "C3"), Value::Number(60.0));
    assert_eq!(value(&s, "C2"), Value::Empty);
    s.execute("cell.set", json!({"cell":"C3", "input":""})).unwrap();
    assert_eq!(s.execute("options.fillTableFormulas", json!({})).unwrap()["on"], true);
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).unwrap();
    assert_totals(&s);
}

#[test]
fn absolute_references_and_destination_formats_are_preserved() {
    let mut s = table();
    s.execute("home.numberFormat", json!({"range":"C4", "format":"0.00"})).unwrap();
    let style = s.doc().unwrap().wb.active().unwrap().style_id(CellRef::parse("C4").unwrap());
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*$B$2"})).unwrap();
    for (address, expected) in [("C2", 20.0), ("C3", 30.0), ("C4", 40.0), ("C5", 50.0)] {
        assert_eq!(value(&s, address), Value::Number(expected), "{address}");
    }
    assert_eq!(s.doc().unwrap().wb.active().unwrap().style_id(CellRef::parse("C4").unwrap()), style);
}

#[test]
fn metadata_is_anchored_to_the_first_data_row_and_survives_xlsx() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C4", "input":"=A4*B4"})).unwrap();
    let wb = &s.doc().unwrap().wb;
    assert_eq!(wb.active().unwrap().tables[0].columns[2].formula.as_deref(), Some("A2*B2"));
    let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
    let (back, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sh = back.active().unwrap();
    assert_eq!(sh.tables[0].columns[2].formula.as_deref(), Some("A2*B2"));
    for address in ["C2", "C3", "C4", "C5"] {
        assert!(sh.cell(CellRef::parse(address).unwrap()).unwrap().formula.is_some(), "{address}");
    }
}

#[test]
fn structured_formulas_share_the_parsed_expression_across_rows() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=[@Qty]*[@Price]"})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    let first = sh.cell(CellRef::parse("C2").unwrap()).unwrap().formula.as_ref().unwrap();
    let last = sh.cell(CellRef::parse("C5").unwrap()).unwrap().formula.as_ref().unwrap();
    assert!(std::sync::Arc::ptr_eq(first, last), "table formulas must not be cloned and reparsed for every row");
}

#[test]
fn old_preferences_default_to_automatic_fill_and_new_preferences_round_trip() {
    let old: gridcraft_engine::Prefs = serde_json::from_str("{}").unwrap();
    assert!(old.fill_table_formulas);
    let mut s = Session::new();
    s.execute("options.fillTableFormulas", json!({"on":false})).unwrap();
    let text = serde_json::to_string(&s.prefs).unwrap();
    let back: gridcraft_engine::Prefs = serde_json::from_str(&text).unwrap();
    assert!(!back.fill_table_formulas);
}

#[test]
fn totals_are_not_part_of_the_automatic_fill() {
    let mut s = table();
    s.execute("table.totalRow", json!({"table":"Sales", "on":true})).unwrap();
    let totals = s.doc().unwrap().wb.active().unwrap().input_text(CellRef::parse("C6").unwrap());
    s.execute("cell.set", json!({"cell":"C3", "input":"=[@Qty]*[@Price]"})).unwrap();
    assert_eq!(value(&s, "C5"), Value::Number(200.0));
    assert_eq!(s.doc().unwrap().wb.active().unwrap().input_text(CellRef::parse("C6").unwrap()), totals);
    assert_eq!(value(&s, "C6"), Value::Number(400.0));
}

#[test]
fn protected_destination_rolls_back_the_entire_fill() {
    let mut s = table();
    s.execute("home.lockCell", json!({"range":"C3", "on":false})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    assert!(s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).is_err());
    for address in ["C2", "C3", "C4", "C5"] {
        assert_eq!(value(&s, address), Value::Empty, "failed fill must restore {address}");
    }
    assert!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.is_none());
}

#[test]
fn editing_a_consistent_calculated_column_updates_all_rows_and_undoes_together() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).unwrap();
    s.execute("cell.set", json!({"cell":"C4", "input":"=A4*B4*2"})).unwrap();
    for (address, expected) in [("C2", 40.0), ("C3", 120.0), ("C4", 240.0), ("C5", 400.0)] {
        assert_eq!(value(&s, address), Value::Number(expected), "{address}");
    }
    assert_eq!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.as_deref(), Some("A2*B2*2"));
    s.execute("edit.undo", json!({})).unwrap();
    assert_totals(&s);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.as_deref(), Some("A2*B2"));
}

#[test]
fn constants_blanks_and_formula_exceptions_prevent_column_wide_updates() {
    for exception in [json!(999), json!(""), json!("=123")] {
        let mut s = table();
        s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).unwrap();
        // A bulk write deliberately makes a single-cell exception, rather than editing the
        // master formula of the consistent calculated column.
        s.execute("range.setValues", json!({"range":"C4", "values":[[exception]]})).unwrap();
        let before = value(&s, "C4");
        s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3*2"})).unwrap();
        assert_eq!(value(&s, "C4"), before);
        assert_eq!(value(&s, "C2"), Value::Number(20.0));
        assert_eq!(value(&s, "C3"), Value::Number(120.0));
        assert_eq!(value(&s, "C5"), Value::Number(200.0));
        assert_eq!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.as_deref(), Some("A2*B2"));
    }
}

#[test]
fn editing_the_exception_itself_does_not_replace_the_master() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=[@Qty]*[@Price]"})).unwrap();
    s.execute("cell.set", json!({"cell":"C4", "input":999})).unwrap();
    s.execute("cell.set", json!({"cell":"C4", "input":"=123"})).unwrap();
    assert_eq!(value(&s, "C2"), Value::Number(20.0));
    assert_eq!(value(&s, "C3"), Value::Number(60.0));
    assert_eq!(value(&s, "C4"), Value::Number(123.0));
    assert_eq!(value(&s, "C5"), Value::Number(200.0));
}

#[test]
fn disabling_fill_also_disables_propagation_of_edits() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).unwrap();
    s.execute("options.fillTableFormulas", json!({"on":false})).unwrap();
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3*2"})).unwrap();
    assert_eq!(value(&s, "C2"), Value::Number(20.0));
    assert_eq!(value(&s, "C3"), Value::Number(120.0));
}

#[test]
fn changes_to_source_data_recalculate_the_filled_formulas() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=[@Qty]*[@Price]"})).unwrap();
    s.execute("cell.set", json!({"cell":"A4", "input":10})).unwrap();
    assert_eq!(value(&s, "C4"), Value::Number(300.0));
    assert_eq!(value(&s, "C3"), Value::Number(60.0));
}

#[test]
fn ordinary_formulas_and_single_cell_arrays_do_not_fill_outside_their_scope() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"E3", "input":"=A3*B3"})).unwrap();
    assert_eq!(value(&s, "E2"), Value::Empty);
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3", "array":true})).unwrap();
    assert_eq!(value(&s, "C3"), Value::Number(60.0));
    assert_eq!(value(&s, "C2"), Value::Empty);
    assert!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.is_none());
}

#[test]
fn updates_after_xlsx_round_trip_still_use_the_saved_master() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).unwrap();
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (back, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    s.doc_mut().unwrap().wb = std::sync::Arc::new(back);
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3*2"})).unwrap();
    assert_eq!(value(&s, "C2"), Value::Number(40.0));
    assert_eq!(value(&s, "C5"), Value::Number(400.0));
}

#[test]
fn ten_thousand_row_structured_formulas_reuse_one_allocation() {
    let mut s = Session::new();
    s.new_workbook();
    let mut values = vec![json!(["Qty", "Price", "Total"])];
    values.extend((0..10_000).map(|_| json!([2, 10, null])));
    s.execute("range.setValues", json!({"range":"A1", "values":values})).unwrap();
    s.execute("insert.table", json!({"range":"A1:C10001", "header":true, "name":"Sales"})).unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::disallowed_methods)] // Native timing only: the web has no Instant clock.
    let started = std::time::Instant::now();
    s.execute("cell.set", json!({"cell":"C5000", "input":"=[@Qty]*[@Price]"})).unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("10,000-row calculated column: {:?}", started.elapsed());
    assert_eq!(value(&s, "C10001"), Value::Number(20.0));
    let sh = s.doc().unwrap().wb.active().unwrap();
    let first = sh.cell(CellRef::parse("C2").unwrap()).unwrap().formula.as_ref().unwrap();
    let last = sh.cell(CellRef::parse("C10001").unwrap()).unwrap().formula.as_ref().unwrap();
    assert!(std::sync::Arc::ptr_eq(first, last));
}

#[test]
fn excessive_generated_formula_text_is_rejected_without_partial_edits() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range":"A1", "values":[["Input","Output"]]})).unwrap();
    s.execute("insert.table", json!({"range":"A1:B10000", "header":true, "name":"Large"})).unwrap();
    let input = format!("=A2&\"{}\"", "x".repeat(8000));
    assert!(s.execute("cell.set", json!({"cell":"B2", "input":input})).is_err());
    assert_eq!(value(&s, "B2"), Value::Empty);
    assert!(s.doc().unwrap().wb.active().unwrap().tables[0].columns[1].formula.is_none());
}

#[test]
fn column_header_and_table_renames_keep_the_master_usable() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=Sales[@Qty]*Sales[@Price]"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1", "input":"Count"})).unwrap();
    s.execute("table.rename", json!({"table":"Sales", "name":"Orders"})).unwrap();
    let master = s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.as_ref().unwrap();
    assert!(master.contains("Orders") && master.contains("Count"));
    assert!(!master.contains("Sales") && !master.contains("Qty"));
    s.execute("cell.set", json!({"cell":"C3", "input":"=Orders[@Count]*Orders[@Price]*2"})).unwrap();
    assert_eq!(value(&s, "C2"), Value::Number(40.0));
    assert_eq!(value(&s, "C5"), Value::Number(400.0));
}

#[test]
fn structural_edits_adjust_the_master_alongside_cell_formulas() {
    let mut s = table();
    s.execute("cell.set", json!({"cell":"C3", "input":"=A3*B3"})).unwrap();
    s.execute("home.insertRows", json!({"rows":"1:1"})).unwrap();
    let master = s.doc().unwrap().wb.active().unwrap().tables[0].columns[2].formula.as_deref();
    assert_eq!(master, Some("A3*B3"));
    s.execute("cell.set", json!({"cell":"C4", "input":"=A4*B4*2"})).unwrap();
    assert_eq!(value(&s, "C3"), Value::Number(40.0));
    assert_eq!(value(&s, "C6"), Value::Number(400.0));
}

#[test]
fn hidden_rows_are_part_of_a_calculated_column() {
    let mut s = table();
    s.execute("home.hideRows", json!({"rows":"4:4"})).unwrap();
    s.execute("cell.set", json!({"cell":"C3", "input":"=[@Qty]*[@Price]"})).unwrap();
    assert_totals(&s);
    assert!(s.doc().unwrap().wb.active().unwrap().is_row_hidden(3));
}
