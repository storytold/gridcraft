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

#[test]
fn german_samples_are_german_workbooks_without_errors() {
    use crate::lang::Lang;
    for (name, _) in crate::sample::SAMPLES {
        let de = crate::sample::build_in(name, Lang::German).unwrap_or_else(|| panic!("German sample {name}"));
        let en = crate::sample::build(name).unwrap_or_else(|| panic!("sample {name}"));
        assert_eq!(de.sheets[0].cells.len(), en.sheets[0].cells.len(), "{name}: same layout in both languages");
        for sh in &de.sheets {
            for (at, _) in sh.cells.iter() {
                let value = sh.value(at);
                assert!(!matches!(value, Value::Error(_)), "{name} {}: {value:?}", at.a1());
            }
        }
        assert_ne!(crate::sample::title_in(name, Lang::German), crate::sample::title(name));
    }
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales", "language": "de"})).unwrap();
    assert_eq!(s.doc().unwrap().title, "Quartalsumsätze");
    assert_eq!(s.doc().unwrap().wb.sheets[0].name, "Umsatz");
    // The structured reference follows the German table and column names.
    assert_eq!(v(&s, "C14"), Value::Text("Online".into()));
    assert_eq!(v(&s, "B5"), Value::Text("Nord".into()));
}

#[test]
fn new_workbooks_are_named_in_the_session_language() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().title, "Book1");
    assert_eq!(s.doc().unwrap().wb.sheets[0].name, "Sheet1");
    s.lang = crate::lang::Lang::German;
    s.execute("file.new", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().title, "Mappe1");
    assert_eq!(s.doc().unwrap().wb.sheets[0].name, "Tabelle1");
    s.execute("home.insertSheet", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.sheet_index("Tabelle2").is_some());
    // An explicit language wins over the session's.
    s.execute("file.new", json!({"language": "en"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets[0].name, "Sheet1");
    assert_eq!(s.lang, crate::lang::Lang::German, "the session keeps its language");
}

#[test]
fn german_input_and_display() {
    use gridcraft_core::Locale;
    let mut s = s();
    let de = |s: &mut Session, cell: &str, input: &str| s.execute("cell.set", json!({"cell": cell, "input": input, "locale": "de"})).unwrap();
    de(&mut s, "A1", "1.234,5");
    de(&mut s, "A2", "2,5");
    de(&mut s, "A3", "=SUMME(A1;A2;0,5)");
    de(&mut s, "A4", "10.10.2026");
    de(&mut s, "A5", "12,5 %");
    de(&mut s, "A6", "WAHR");
    de(&mut s, "A7", "=WENN(A1>1000;\"groß\";\"klein\")");
    de(&mut s, "A8", "=SVERWEIS(2,5;A2:A3;1;FALSCH)");
    assert_eq!(v(&s, "A1"), Value::Number(1234.5));
    assert_eq!(v(&s, "A3"), Value::Number(1237.5));
    assert_eq!(v(&s, "A6"), Value::Bool(true));
    assert_eq!(v(&s, "A7"), Value::text("groß"));
    assert_eq!(v(&s, "A8"), Value::Number(2.5));
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let at = |a: &str| CellRef::parse(a).unwrap();
    // Stored as en-US, shown in German.
    assert_eq!(sh.cell(at("A3")).unwrap().input_text(), "=SUM(A1,A2,0.5)");
    let text = |a: &str| crate::display::cell_text_in(&d.wb, sh, at(a), Locale::De);
    let input = |a: &str| crate::display::input_text_in(&d.wb, sh.cell(at(a)).unwrap(), Locale::De);
    assert_eq!(text("A1"), "1.234,50", "typing thousands separators applies #,##0.00, as in Excel");
    assert_eq!(text("A3"), "1237,5");
    assert_eq!(text("A4"), "10.10.2026");
    assert_eq!(text("A5"), "12,50%");
    assert_eq!(text("A6"), "WAHR");
    assert_eq!(crate::display::cell_text(&d.wb, sh, at("A6")), "TRUE", "en-US callers are unchanged");
    assert_eq!(input("A1"), "1234,5");
    assert_eq!(input("A3"), "=SUMME(A1;A2;0,5)");
    assert_eq!(input("A4"), "10.10.2026");
    assert_eq!(input("A5"), "12,5%");
    assert_eq!(input("A6"), "WAHR");
    // Without a locale the same text is en-US input.
    s.execute("cell.set", json!({"cell": "B1", "input": "2,5"})).unwrap();
    assert_ne!(v(&s, "B1"), Value::Number(2.5));
    s.execute("cell.set", json!({"cell": "B2", "input": "=SUM(1.5,1)"})).unwrap();
    assert_eq!(v(&s, "B2"), Value::Number(2.5));
    // range.fill takes the locale too; relative references still adjust.
    s.execute("selection.set", json!({"range": "C1:C2"})).unwrap();
    s.execute("range.fill", json!({"input": "=A1*0,5", "locale": "de"})).unwrap();
    assert_eq!(v(&s, "C2"), Value::Number(1.25));
}

#[test]
fn german_number_formats_are_in_euros() {
    let mut s = s();
    let code = |s: &Session| {
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        d.wb.styles.get(sh.style_id(d.selection.active)).num_fmt.as_str().to_string()
    };
    s.execute("cell.set", json!({"cell": "A1", "input": "1234.5"})).unwrap();
    s.execute("home.numberFormat", json!({"format": "Currency", "locale": "de"})).unwrap();
    assert_eq!(code(&s), "#,##0.00 \"€\"");
    let text = |s: &Session| {
        let d = s.doc().unwrap();
        crate::display::cell_text_in(&d.wb, d.wb.active().unwrap(), CellRef::new(0, 0), gridcraft_core::Locale::De)
    };
    assert_eq!(text(&s), "1.234,50 €");
    s.execute("home.accounting", json!({"locale": "de"})).unwrap();
    assert!(code(&s).contains('€'));
    assert_eq!(text(&s).trim(), "1.234,50 €");
    s.execute("home.numberFormat", json!({"format": "Currency"})).unwrap();
    assert_eq!(code(&s), "\"$\"#,##0.00", "scripts keep the en-US formats");
    s.execute("home.accounting", json!({})).unwrap();
    assert!(code(&s).contains('$'));
}

#[test]
fn german_validation_input() {
    let mut s = s();
    s.execute("data.validation", json!({"range": "A1", "type": "decimal", "operator": "between", "formula1": "1", "formula2": "2"})).unwrap();
    let d = s.doc().unwrap();
    let at = CellRef::new(0, 0);
    let si = d.wb.active_sheet;
    assert!(crate::cmd::data::check_validation_in(&d.wb, si, at, "1,5", gridcraft_core::Locale::De).is_none());
    assert!(crate::cmd::data::check_validation_in(&d.wb, si, at, "2,5", gridcraft_core::Locale::De).is_some());
    assert!(crate::cmd::data::check_validation(&d.wb, si, at, "1.5").is_none());
}
