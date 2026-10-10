//! Array evaluation must survive export even when a formula returns one value.

use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Cell, DefinedName, Formula, Sheet, Table, TableColumn, TotalsFn, Workbook};

use crate::package::Package;
use crate::xml::El;
use crate::{read_xlsx, write_xlsx};

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

fn range(s: &str) -> RangeRef {
    RangeRef::parse(s).unwrap()
}

fn formula(text: &str, value: f64) -> Cell {
    let mut cell = Cell::formula(Formula::new(text));
    cell.value = Value::Number(value);
    cell
}

fn cell<'a>(sheet: &'a El, address: &str) -> &'a El {
    sheet.child("sheetData").unwrap().kids("row").flat_map(|r| r.kids("c")).find(|c| c.attr("r") == Some(address)).unwrap()
}

fn assert_scalar(sheet: &El, address: &str) {
    let c = cell(sheet, address);
    assert_eq!(c.attr("cm"), None, "scalar formula must not have dynamic metadata at {address}");
    let f = c.child("f").unwrap();
    assert_eq!(f.attr("t"), None);
    assert_eq!(f.attr("ref"), None);
}

fn assert_dynamic(sheet: &El, address: &str, reference: &str) {
    let c = cell(sheet, address);
    assert_eq!(c.attr("cm"), Some("1"), "array evaluation metadata at {address}");
    let f = c.child("f").unwrap();
    assert_eq!(f.attr("t"), Some("array"));
    assert_eq!(f.attr("ref"), Some(reference));
}

#[test]
fn nonspilling_formulas_preserve_array_evaluation_and_metadata() {
    let mut wb = Workbook::new();
    let data = wb.sheet_mut(0).unwrap();
    data.name = "Data".into();
    for (address, value) in [("A1", 1.0), ("A2", 4.0), ("A3", 9.0), ("B1", 2.0), ("B2", 3.0), ("B3", 4.0)] {
        data.cells.set(at(address), Cell::value(Value::Number(value)));
    }
    let cases = [("B107", "SUM(Data!A1:A3*Data!B1:B3)", 50.0), ("B108", "SUM(SQRT(Data!A1:A3))", 6.0), ("B109", "SUM(ABS(Data!A1:A3))", 14.0)];
    let mut results = Sheet::new("Results");
    for (address, text, value) in cases {
        results.cells.set(at(address), formula(text, value));
    }
    results.cells.set(at("C107"), Cell::value(Value::text("neighbor")));
    assert!(results.spill_ranges.is_empty());
    wb.sheets.push(Arc::new(results));

    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet2.xml").unwrap().unwrap();
    for (address, text, _) in cases {
        assert_dynamic(&sheet, address, address);
        assert_eq!(cell(&sheet, address).child("f").unwrap().text, text);
    }
    assert_eq!(cell(&sheet, "C107").attr("cm"), None);

    let metadata = pkg.read_xml("xl/metadata.xml").unwrap().unwrap();
    let kind = metadata.path(&["metadataTypes", "metadataType"]).unwrap();
    assert_eq!(kind.attr("name"), Some("XLDAPR"));
    assert_eq!(kind.attr("cellMeta"), Some("1"));
    let dynamic = metadata.path(&["futureMetadata", "bk", "extLst", "ext", "dynamicArrayProperties"]).unwrap();
    assert_eq!(dynamic.attr("fDynamic"), Some("1"));
    assert_eq!(dynamic.attr("fCollapsed"), Some("0"));
    let record = metadata.path(&["cellMetadata", "bk", "rc"]).unwrap();
    assert_eq!((record.attr("t"), record.attr("v")), (Some("1"), Some("0")));
    let types = pkg.read_xml("[Content_Types].xml").unwrap().unwrap();
    assert!(types.kids("Override").any(|e| e.attr("PartName") == Some("/xl/metadata.xml")
        && e.attr("ContentType") == Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml")));
    assert!(pkg.rels("xl/workbook.xml").unwrap().iter().any(|r| r.kind == "sheetMetadata" && r.target == "xl/metadata.xml"));

    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let results = back.sheet(1).unwrap();
    for (address, text, value) in cases {
        let c = results.cell(at(address)).unwrap();
        let f = c.formula.as_ref().unwrap();
        assert_eq!(f.text, text);
        assert_eq!(f.array, None, "dynamic formula must not become legacy CSE");
        assert_eq!(c.value, Value::Number(value));
    }
    assert_eq!(results.value(at("C107")), Value::text("neighbor"));
}

#[test]
fn dynamic_spills_and_legacy_cse_keep_distinct_ranges() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    let mut legacy = Formula::new("ROW(A1:A2)");
    legacy.array = Some(range("A1:A2"));
    sh.cells.set(at("A1"), Cell::formula(legacy));
    sh.cells.set(at("A2"), Cell::value(Value::Number(2.0)));
    sh.cells.set(at("B1"), formula("SEQUENCE(3)", 1.0));
    sh.spill_ranges.insert(at("B1"), range("B1:B3"));
    sh.spill.insert(at("B2"), Value::Number(2.0));
    sh.spill.insert(at("B3"), Value::Number(3.0));
    sh.cells.set(at("C1"), formula("SUM(B1:B3)", 6.0));

    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    assert_eq!(cell(&sheet, "A1").attr("cm"), None);
    let legacy = cell(&sheet, "A1").child("f").unwrap();
    assert_eq!((legacy.attr("t"), legacy.attr("ref")), (Some("array"), Some("A1:A2")));
    assert_dynamic(&sheet, "B1", "B1:B3");
    assert_scalar(&sheet, "C1");
    assert_eq!(cell(&sheet, "B2").child("v").unwrap().text, "2");
    assert_eq!(cell(&sheet, "B2").attr("cm"), None);

    let (back, _) = read_xlsx(&bytes).unwrap();
    let sh = back.sheet(0).unwrap();
    assert_eq!(sh.cell(at("A1")).unwrap().formula.as_ref().unwrap().array, Some(range("A1:A2")));
    // A legacy array's cached values are dropped on read: the formula fills its range on recalc (#156).
    assert!(sh.cell(at("A2")).is_none());
    assert_eq!(sh.cell(at("B1")).unwrap().formula.as_ref().unwrap().array, None);
    assert!(sh.cell(at("B2")).is_none());
    assert!(sh.cell(at("B3")).is_none());
    assert_eq!(sh.value(at("C1")), Value::Number(6.0));
}

#[test]
fn scalar_formulas_constants_and_legacy_cse_do_not_require_dynamic_metadata() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.name = "Data".into();
    sh.cells.set(at("A1"), Cell::value(Value::Number(3.0)));
    for (address, text, value) in [("C1", "1+2", 3.0), ("C2", "Data!A1", 3.0), ("C3", "SUM(A1:A3)", 3.0), ("C4", "A1*2", 6.0)] {
        sh.cells.set(at(address), formula(text, value));
    }
    for legacy in [false, true] {
        if legacy {
            let mut f = Formula::new("SUM(A1:A2)");
            f.array = Some(range("B1"));
            wb.sheet_mut(0).unwrap().cells.set(at("B1"), Cell::formula(f));
        }
        let bytes = write_xlsx(&wb).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        assert!(!pkg.has("xl/metadata.xml"));
        assert!(!pkg.rels("xl/workbook.xml").unwrap().iter().any(|r| r.kind == "sheetMetadata"));
        let types = pkg.read_xml("[Content_Types].xml").unwrap().unwrap();
        assert!(!types.kids("Override").any(|e| e.attr("PartName") == Some("/xl/metadata.xml")));
        let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
        assert_eq!(cell(&sheet, "A1").attr("cm"), None);
        for address in ["C1", "C2", "C3", "C4"] {
            assert_scalar(&sheet, address);
        }
    }
}

#[test]
fn calculated_table_column_keeps_formula_and_cell_value() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.cells.set(at("A2"), Cell::value(Value::Number(3.0)));
    sh.cells.set(at("B2"), formula("A2*2", 6.0));
    sh.tables.push(Table {
        id: 1,
        name: "Data".into(),
        range: range("A1:B2"),
        header_row: true,
        totals_row: false,
        columns: vec![
            TableColumn { name: "Input".into(), totals: TotalsFn::None, totals_label: None, formula: None },
            TableColumn { name: "Output".into(), totals: TotalsFn::None, totals_label: None, formula: Some("A2*2".into()) },
        ],
        style: "TableStyleLight1".into(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
    });
    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    assert_scalar(&sheet, "B2");
    assert_eq!(cell(&sheet, "B1").attr("cm"), None, "table headers are text");
    let table = pkg.read_xml("xl/tables/table1.xml").unwrap().unwrap();
    let column = table.child("tableColumns").unwrap().kids("tableColumn").nth(1).unwrap();
    assert_eq!(column.child("calculatedColumnFormula").unwrap().text, "A2*2");
    let (back, _) = read_xlsx(&bytes).unwrap();
    let sh = back.sheet(0).unwrap();
    assert_eq!(sh.tables[0].columns[1].formula.as_deref(), Some("A2*2"));
    assert_eq!(sh.cell(at("B2")).unwrap().formula.as_ref().unwrap().array, None);
    assert_eq!(sh.value(at("B2")), Value::Number(6.0));
}

/// Compare both cell and formula metadata, collecting all boundaries in the group on failure.
fn check_formula_metadata(mut wb: Workbook, sheet_index: usize, cases: &[(&str, bool)]) {
    for (i, (text, _)) in cases.iter().enumerate() {
        let f = Formula::new(text);
        assert!(f.expr().is_some(), "test formula must parse: {text}");
        wb.sheet_mut(sheet_index).unwrap().cells.set(CellRef::new(i as u32 + 100, 5), Cell::formula(f));
    }
    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let sheet = pkg.read_xml(&format!("xl/worksheets/sheet{}.xml", sheet_index + 1)).unwrap().unwrap();
    let mut mismatches = vec![];
    for (i, (text, dynamic)) in cases.iter().enumerate() {
        let address = CellRef::new(i as u32 + 100, 5).to_string();
        let c = cell(&sheet, &address);
        let f = c.child("f").unwrap();
        let expected = if *dynamic { (Some("1"), Some("array"), Some(address.as_str())) } else { (None, None, None) };
        let actual = (c.attr("cm"), f.attr("t"), f.attr("ref"));
        if actual != expected {
            mismatches.push(format!("{text}: expected {expected:?}, got {actual:?}"));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn implicit_intersection_metadata_respects_function_argument_context() {
    check_formula_metadata(
        Workbook::new(),
        0,
        &[
            ("SQRT(A1:A3)", true),
            ("ABS(A1:A3)", true),
            ("SUM(A1:A3*B1:B3)", true),
            ("SUM(SQRT(A1:A3))", true),
            ("SUM(A1:A3)", false),
            ("SUMPRODUCT(A1:A3*B1:B3)", false),
            ("SUMPRODUCT(SQRT(A1:A3))", false),
            ("SQRT(@A1:A3)", false),
            ("@A1:A3*2", false),
            ("@(A1:A3*2)", true),
            ("SQRT(INDIRECT(\"A1\"))", false),
            ("SUM(INDIRECT(\"A1:A3\"))", false),
            ("SQRT(INDIRECT(\"A1:A3\"))", true),
            ("SUM(INDIRECT(A1))", false),
            ("SQRT(INDIRECT(A1))", true),
            ("INDEX(A1:B3,1,1)", false),
            ("INDEX(A1:A3,1)", false),
            ("SUM(INDEX(A1:B3,0,1))", false),
            ("SQRT(INDEX(A1:B3,0,1))", true),
            ("SUM(ROW(A1:A3))", true),
            ("SUMPRODUCT(ROW(A1:A3))", false),
            ("OFFSET(A1,0,0,1,1)*2", false),
            ("SUM(OFFSET(A1,0,0,3,1)*2)", true),
            ("SUM(OFFSET(A1,0,0,3,1))", false),
            ("LET(x,A1:A3,SUM(x))", false),
            ("LET(x,A1:A3,SUM(x*2))", true),
            ("XLOOKUP(1,A1:A3,B1:B3)", false),
            ("XLOOKUP(1,A1:A3,B1:C3)", true),
        ],
    );
}

#[test]
fn implicit_intersection_metadata_resolves_names_in_formula_sheet_scope() {
    let mut wb = Workbook::new();
    wb.sheets.push(Arc::new(Sheet::new("Results")));
    for (name, scope, text) in [
        ("Rate", None, "2"),
        ("Inputs", None, "Sheet1!$A$1:$A$3"),
        ("Single", None, "Sheet1!$A$1"),
        ("Alias", None, "Inputs"),
        ("Scoped", None, "Sheet1!$A$1:$A$3"),
        ("Scoped", Some(0), "2"),
        ("LocalInputs", Some(1), "Results!$A$1:$A$3"),
    ] {
        wb.names.push(DefinedName { name: name.into(), scope, formula: text.into(), comment: String::new(), hidden: false });
    }
    check_formula_metadata(
        wb.clone(),
        0,
        &[
            ("Rate", false),
            ("SQRT(Rate)", false),
            ("Single*2", false),
            ("Inputs", true),
            ("SUM(Inputs)", false),
            ("SUM(Inputs*2)", true),
            ("SUM(SQRT(Alias))", true),
            ("SQRT(Scoped)", false),
        ],
    );
    check_formula_metadata(wb, 1, &[("SQRT(Scoped)", true), ("SUM(LocalInputs)", false), ("LocalInputs*2", true)]);
}

#[test]
fn cyclic_names_preserve_array_evaluation_without_unbounded_export_recursion() {
    let mut wb = Workbook::new();
    for (name, formula) in [("First", "Second"), ("Second", "First")] {
        wb.names.push(DefinedName { name: name.into(), scope: None, formula: formula.into(), comment: String::new(), hidden: false });
    }
    check_formula_metadata(wb, 0, &[("First", true), ("SUM(First)", true)]);
}

#[test]
fn implicit_intersection_metadata_distinguishes_table_rows_and_columns() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.tables.push(Table {
        id: 1,
        name: "Data".into(),
        range: range("A1:B3"),
        header_row: true,
        totals_row: false,
        columns: vec![
            TableColumn { name: "Input".into(), totals: TotalsFn::None, totals_label: None, formula: None },
            TableColumn { name: "Output".into(), totals: TotalsFn::None, totals_label: None, formula: Some("[@Input]*2".into()) },
        ],
        style: "TableStyleLight1".into(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
    });
    sh.cells.set(at("A2"), Cell::value(Value::Number(3.0)));
    sh.cells.set(at("B2"), formula("[@Input]*2", 6.0));
    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    assert_scalar(&sheet, "B2");
    check_formula_metadata(
        wb,
        0,
        &[("Data[@Input]*2", false), ("SUM(Data[Input])", false), ("SUM(Data[Input]*2)", true), ("SQRT(Data[Input])", true)],
    );
}
