use gridcraft_core::{CellRef, Value};
use serde_json::json;

use crate::Session;

fn s() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn v(s: &Session, a: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a).unwrap())
}

#[test]
fn enter_values_and_formulas() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "10"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "32"})).unwrap();
    s.execute("cell.set", json!({"cell": "A3", "input": "=sum(a1:a2"})).unwrap();
    assert_eq!(v(&s, "A3"), Value::Number(42.0));
    assert!(s.execute("cell.set", json!({"cell": "A4", "input": "=1+*2"})).is_err());
    s.execute("cell.set", json!({"cell": "B1", "input": "12%"})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(crate::display::cell_text(&d.wb, sh, CellRef::parse("B1").unwrap()), "12%");
}

#[test]
fn undo_redo() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(1.0));
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0));
    assert!(s.doc().unwrap().is_dirty());
}

#[test]
fn copy_paste_shifts_formulas() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=A1+B1"})).unwrap();
    s.execute("edit.copy", json!({"range": "C1"})).unwrap();
    s.execute("selection.set", json!({"range": "C2"})).unwrap();
    s.execute("edit.paste", json!({})).unwrap();
    assert_eq!(v(&s, "C2"), Value::Number(7.0));
    s.execute("edit.cut", json!({"range": "A1"})).unwrap();
    s.execute("selection.set", json!({"range": "E5"})).unwrap();
    s.execute("edit.paste", json!({})).unwrap();
    assert_eq!(v(&s, "E5"), Value::Number(1.0));
    let f = s.execute("cell.get", json!({"cell": "C1"})).unwrap();
    assert_eq!(f["formula"], "=E5+B1");
}

#[test]
fn insert_delete_rows_adjust() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=SUM(A1:A3)"})).unwrap();
    s.execute("home.insertRows", json!({"rows": "2:2"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "B1"})).unwrap()["formula"], "=SUM(A1:A4)");
    s.execute("cell.set", json!({"cell": "A2", "input": "10"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Number(16.0));
    s.execute("home.deleteRows", json!({"rows": "1:1"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Empty);
    assert_eq!(v(&s, "A1"), Value::Number(10.0));
}

#[test]
fn fill_series() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [3]]})).unwrap();
    s.execute("edit.autoFill", json!({"source": "A1:A2", "target": "A1:A6"})).unwrap();
    assert_eq!(v(&s, "A6"), Value::Number(11.0));
    s.execute("cell.set", json!({"cell": "B1", "input": "Mon"})).unwrap();
    s.execute("edit.autoFill", json!({"source": "B1", "target": "B1:B3"})).unwrap();
    assert_eq!(v(&s, "B3"), Value::from("Wed"));
    s.execute("cell.set", json!({"cell": "C1", "input": "Item 9"})).unwrap();
    s.execute("edit.autoFill", json!({"source": "C1", "target": "C1:C2"})).unwrap();
    assert_eq!(v(&s, "C2"), Value::from("Item 10"));
}

#[test]
fn sort_and_filter() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Score"], ["b", 2], ["c", 3], ["a", 1]]})).unwrap();
    s.execute("selection.set", json!({"cell": "B2"})).unwrap();
    s.execute("data.sortDescending", json!({})).unwrap();
    assert_eq!(v(&s, "A2"), Value::from("c"));
    assert_eq!(v(&s, "A1"), Value::from("Name"));
    s.execute("data.filter", json!({})).unwrap();
    let r = s.execute("data.filterBy", json!({"column": "B", "custom": {"op": ">", "value": "1"}})).unwrap();
    assert_eq!(r["hiddenRows"], 1);
}

#[test]
fn formatting_and_styles() {
    let mut s = s();
    s.execute("selection.set", json!({"range": "A1:B2"})).unwrap();
    s.execute("home.bold", json!({})).unwrap();
    s.execute("home.fillColor", json!({"color": "#FFFF00"})).unwrap();
    s.execute("home.mergeCenter", json!({})).unwrap();
    let c = s.execute("cell.get", json!({"cell": "B2"})).unwrap();
    assert_eq!(c["style"]["font"]["bold"], true);
    assert_eq!(c["merge"], "A1:B2");
    s.execute("home.cellStyle", json!({"range": "D1", "name": "Good"})).unwrap();
}

#[test]
fn tables_and_structured_refs() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Qty"], ["x", 2], ["y", 5]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3"})).unwrap();
    s.execute("cell.set", json!({"cell": "D1", "input": "=SUM(Table1[Qty])"})).unwrap();
    assert_eq!(v(&s, "D1"), Value::Number(7.0));
    s.execute("table.totalRow", json!({"on": true, "table": "Table1"})).unwrap();
    assert_eq!(v(&s, "B4"), Value::Number(7.0));
}

#[test]
fn sheets() {
    let mut s = s();
    s.execute("home.insertSheet", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "5"})).unwrap();
    s.execute("sheet.rename", json!({"name": "Data"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=Data!A1*2"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(10.0));
    s.execute("sheet.rename", json!({"sheet": "Data", "name": "My Data"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["formula"], "='My Data'!A1*2");
}

#[test]
fn xlsx_roundtrip_through_engine() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a", 1], ["b", 2]]})).unwrap();
    s.execute("cell.set", json!({"cell": "B3", "input": "=SUM(B1:B2)"})).unwrap();
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let b64 = r["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "x.xlsx", "base64": b64})).unwrap();
    assert_eq!(v(&s, "B3"), Value::Number(3.0));
}

#[test]
fn samples_build() {
    for (name, _) in crate::sample::SAMPLES {
        let wb = crate::sample::build(name).unwrap_or_else(|| panic!("sample {name}"));
        assert!(!wb.sheets[0].cells.is_empty());
    }
}

#[test]
fn replacing_a_spilling_formula_clears_its_spill() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=SEQUENCE(3)"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=SUM(A1:A3)"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    assert_eq!((v(&s, "A2"), v(&s, "A3"), v(&s, "B1")), (Value::Empty, Value::Empty, Value::Number(2.0)));
    s.execute("cell.set", json!({"cell": "A1", "input": "=SEQUENCE(3)"})).unwrap();
    s.execute("edit.clearContents", json!({"range": "A1"})).unwrap();
    assert_eq!((v(&s, "A2"), v(&s, "B1")), (Value::Empty, Value::Number(0.0)));
}

#[test]
fn range_ending_in_index_recalculates() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "B6", "input": "=SUM(C3:INDEX(B9:C11,2,2))"})).unwrap();
    s.execute("cell.set", json!({"cell": "C5", "input": "2"})).unwrap();
    assert_eq!(v(&s, "B6"), Value::Number(2.0));
}

#[test]
fn structural_edits_move_spills_with_their_formula() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A5", "input": "=SEQUENCE(3)"})).unwrap();
    s.execute("home.deleteRows", json!({"rows": "1:1"})).unwrap();
    let col = |s: &Session| (4..=7).map(|r| v(s, &format!("A{r}"))).collect::<Vec<_>>();
    assert_eq!(col(&s), [Value::Number(1.0), Value::Number(2.0), Value::Number(3.0), Value::Empty]);
    s.execute("home.deleteRows", json!({"rows": "4:4"})).unwrap();
    assert_eq!(col(&s), [Value::Empty, Value::Empty, Value::Empty, Value::Empty]);
}

#[test]
fn clearing_a_distant_blocker_unblocks_the_spill() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=SEQUENCE(100)"})).unwrap();
    s.execute("cell.set", json!({"cell": "A90", "input": "x"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Error(gridcraft_core::CellError::Spill));
    s.execute("edit.clearContents", json!({"range": "A90"})).unwrap();
    assert_eq!((v(&s, "A1"), v(&s, "A100")), (Value::Number(1.0), Value::Number(100.0)));
}

#[test]
fn a_spill_that_goes_away_unblocks_another() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "F8", "input": "=SEQUENCE(5)"})).unwrap();
    s.execute("cell.set", json!({"cell": "E10", "input": "=SEQUENCE(1,3)"})).unwrap();
    assert_eq!(v(&s, "E10"), Value::Error(gridcraft_core::CellError::Spill));
    // Blocking F8's array frees F10 for E10's.
    s.execute("cell.set", json!({"cell": "F9", "input": "0"})).unwrap();
    assert_eq!(v(&s, "F8"), Value::Error(gridcraft_core::CellError::Spill));
    assert_eq!((v(&s, "E10"), v(&s, "G10")), (Value::Number(1.0), Value::Number(3.0)));
}

#[test]
fn an_array_reading_another_arrays_spill_is_updated() {
    // G5 is evaluated before D11 in the same pass, so it first reads D11's old spill.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    s.execute("cell.set", json!({"cell": "D11", "input": "=SEQUENCE(1,3,A1)"})).unwrap();
    s.execute("cell.set", json!({"cell": "G5", "input": "=UNIQUE(VSTACK(F9:F12,A1))"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "10"})).unwrap();
    assert_eq!((v(&s, "G6"), v(&s, "G7")), (Value::Number(12.0), Value::Number(10.0)));
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!((v(&s, "G6"), v(&s, "G7")), (Value::Number(12.0), Value::Number(10.0)));
}

#[test]
fn indirect_reads_a_spill_laid_out_in_the_same_pass() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "F5", "input": "=SEQUENCE(2,2,7)"})).unwrap();
    s.execute("home.insertRows", json!({"rows": "3:3"})).unwrap();
    s.execute("cell.set", json!({"cell": "E5", "input": "=INDIRECT(\"F7\")"})).unwrap();
    assert_eq!(v(&s, "E5"), Value::Number(9.0));
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "E5"), Value::Number(9.0));
}

#[test]
fn every_command_survives_empty_params() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    for spec in crate::command_specs() {
        if matches!(spec.id, "file.close" | "file.open" | "file.save" | "file.saveAs" | "file.exportCsv" | "file.exportHtml" | "insert.picture") {
            continue;
        }
        let _ = s.execute(spec.id, json!({}));
        let _ = s.execute(spec.id, json!([1, 2]));
        let _ = s.execute(spec.id, json!({"range": "ZZZ999999999", "cell": "", "sheet": 99}));
    }
}

#[test]
fn parity_counts() {
    let (done, total) = crate::catalog::parity();
    assert!(total > 250);
    assert!(done > 150, "parity {done}/{total}");
}

#[test]
fn ink_strokes_and_ink_to_shape() {
    let mut s = s();
    // A rough closed box.
    let pts: Vec<[f64; 2]> = vec![[100.0, 100.0], [200.0, 102.0], [201.0, 180.0], [99.0, 181.0], [101.0, 104.0]];
    s.execute("draw.stroke", json!({"points": pts})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap().clone();
    assert_eq!(sh.shapes.len(), 1);
    assert_eq!(sh.shapes[0].kind, gridcraft_model::ShapeKind::Ink);
    let r = s.execute("draw.inkToShape", json!({})).unwrap();
    assert_eq!(r["kind"], "Rectangle");
    // An open stroke becomes a line.
    s.execute("draw.stroke", json!({"points": [[0.0, 0.0], [50.0, 10.0], [120.0, 30.0]]})).unwrap();
    assert_eq!(s.execute("draw.inkToShape", json!({})).unwrap()["kind"], "Line");
    assert!(s.execute("draw.stroke", json!({"points": [[1.0, 1.0]]})).is_err());
}
