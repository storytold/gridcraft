//! Worksheet table references must use Excel's structured-reference spelling on disk.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Cell, Formula, Table, TableColumn, TotalsFn, Workbook};

use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

fn at(text: &str) -> CellRef {
    CellRef::parse(text).unwrap()
}

fn workbook() -> Workbook {
    let mut wb = Workbook::new();
    let sheet = wb.sheet_mut(0).unwrap();
    for (address, value) in [("A1", Value::text("Price")), ("A2", Value::Number(5.0)), ("A3", Value::Number(7.0))] {
        sheet.cells.set(at(address), Cell::value(value));
    }
    sheet.tables.push(Table {
        id: 1,
        name: "Prices".into(),
        range: RangeRef::parse("A1:A3").unwrap(),
        header_row: true,
        totals_row: false,
        columns: vec![TableColumn { name: "Price".into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None }],
        style: String::new(),
        banded_rows: false,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: false,
        filter: vec![],
        table_type: None,
        connection_id: None,
    });
    wb
}

#[test]
fn a_bare_table_worksheet_reference_exports_as_explicit_data_reference() {
    let mut wb = workbook();
    for (address, text) in [("D1", "SUM(Prices)"), ("D2", "SUM(Prices[])")] {
        let mut cell = Cell::formula(Formula::new(text));
        cell.value = Value::Number(12.0);
        wb.sheet_mut(0).unwrap().cells.set(at(address), cell);
    }
    let bytes = write_xlsx(&wb).unwrap();
    let mut package = Package::open(&bytes).unwrap();
    let sheet = package.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    for address in ["D1", "D2"] {
        let cell = sheet.child("sheetData").unwrap().kids("row").flat_map(|row| row.kids("c")).find(|cell| cell.attr("r") == Some(address)).unwrap();
        assert_eq!(cell.child("f").unwrap().text, "SUM(Prices[])", "Excel spelling at {address}");
        assert_eq!(cell.child("v").unwrap().text, "12");
    }
    assert_eq!(
        wb.sheet(0).unwrap().cells.get(at("D1")).unwrap().formula.as_ref().unwrap().text,
        "SUM(Prices)",
        "export must not rewrite the user's model formula"
    );
    let (reopened, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(reopened.sheet(0).unwrap().cells.get(at("D1")).unwrap().formula.as_ref().unwrap().text, "SUM(Prices[])");
}

fn put_formula(wb: &mut Workbook, sheet: usize, address: &str, text: &str) {
    wb.sheet_mut(sheet).unwrap().cells.set(at(address), Cell::formula(Formula::new(text)));
}

fn file_formula(wb: &Workbook, sheet: usize, address: &str) -> String {
    let bytes = write_xlsx(wb).unwrap();
    let mut package = Package::open(&bytes).unwrap();
    let sheet = package.read_xml(&format!("xl/worksheets/sheet{}.xml", sheet + 1)).unwrap().unwrap();
    sheet
        .child("sheetData")
        .unwrap()
        .kids("row")
        .flat_map(|row| row.kids("c"))
        .find(|cell| cell.attr("r") == Some(address))
        .unwrap()
        .child("f")
        .unwrap()
        .text
        .clone()
}

#[test]
fn table_export_respects_let_and_lambda_binding_boundaries() {
    let cases = [
        ("SUM(pRiCeS)", "SUM(Prices[])"),
        ("LET(Prices,3,Prices+1)", "_xlfn.LET(_xlpm.Prices,3,_xlpm.Prices+1)"),
        ("LET(Prices,SUM(Prices),Prices)", "_xlfn.LET(_xlpm.Prices,SUM(Prices[]),_xlpm.Prices)"),
        ("LET(x,SUM(Prices),Prices,4,x+Prices)", "_xlfn.LET(_xlpm.x,SUM(Prices[]),_xlpm.Prices,4,_xlpm.x+_xlpm.Prices)"),
        ("LET(Prices,3,Prices)+SUM(Prices)", "_xlfn.LET(_xlpm.Prices,3,_xlpm.Prices)+SUM(Prices[])"),
        ("LET(Prices,3,LET(x,Prices,x)+SUM(Prices))", "_xlfn.LET(_xlpm.Prices,3,_xlfn.LET(_xlpm.x,_xlpm.Prices,_xlpm.x)+SUM(_xlpm.Prices))"),
        ("LAMBDA(Prices,Prices+SUM(Prices))(3)", "_xlfn.LAMBDA(_xlpm.Prices,_xlpm.Prices+SUM(_xlpm.Prices))(3)"),
        ("LAMBDA(x,SUM(Prices)+x)(3)", "_xlfn.LAMBDA(_xlpm.x,SUM(Prices[])+_xlpm.x)(3)"),
        ("LET(Prices,LAMBDA(x,x+1),Prices(3))", "_xlfn.LET(_xlpm.Prices,_xlfn.LAMBDA(_xlpm.x,_xlpm.x+1),_xlpm.PRICES(3))"),
        ("LET(Prices,3,SUM(Sheet1!Prices)+Prices)", "_xlfn.LET(_xlpm.Prices,3,SUM(Prices[])+_xlpm.Prices)"),
        (r#""Prices"&SUM(Prices)"#, r#""Prices"&SUM(Prices[])"#),
        ("SUM(UnknownName)", "SUM(UnknownName)"),
    ];
    for (input, expected) in cases {
        let mut wb = workbook();
        put_formula(&mut wb, 0, "D1", input);
        assert_eq!(file_formula(&wb, 0, "D1"), expected, "scope in {input}");
    }
}

#[test]
fn visible_defined_names_keep_precedence_over_table_names() {
    let mut wb = workbook();
    wb.names.push(gridcraft_model::DefinedName { name: "Prices".into(), scope: None, formula: "42".into(), comment: String::new(), hidden: false });
    for (input, expected) in [
        ("SUM(Prices)", "SUM(Prices)"),
        ("SUM(Prices[])", "SUM(Prices[])"),
        ("LET(Prices,SUM(Prices),Prices)", "_xlfn.LET(_xlpm.Prices,SUM(Prices),_xlpm.Prices)"),
    ] {
        put_formula(&mut wb, 0, "D1", input);
        assert_eq!(file_formula(&wb, 0, "D1"), expected);
    }
}

#[test]
fn table_export_uses_the_qualified_name_scope_and_preserves_unknown_sheets() {
    let mut wb = workbook();
    wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Scoped")));
    wb.names.push(gridcraft_model::DefinedName {
        name: "Prices".into(),
        scope: Some(1),
        formula: "42".into(),
        comment: String::new(),
        hidden: false,
    });
    for (sheet, input, expected) in [
        (0, "SUM(Prices)", "SUM(Prices[])"),
        (0, "SUM(Scoped!Prices)", "SUM(Scoped!Prices)"),
        (1, "SUM(Prices)", "SUM(Prices)"),
        (1, "SUM(Sheet1!Prices)", "SUM(Prices[])"),
        (0, "SUM(Missing!Prices)", "SUM(Missing!Prices)"),
    ] {
        put_formula(&mut wb, sheet, "D1", input);
        assert_eq!(file_formula(&wb, sheet, "D1"), expected, "sheet {sheet}: {input}");
    }
}

#[test]
fn unicode_local_binding_is_not_captured_by_a_matching_table() {
    let mut wb = workbook();
    wb.sheet_mut(0).unwrap().tables[0].name = "δ".into();
    put_formula(&mut wb, 0, "D1", "LET(Δ,3,δ)");
    assert_eq!(file_formula(&wb, 0, "D1"), "_xlfn.LET(_xlpm.Δ,3,_xlpm.δ)");
}

#[test]
fn a_long_table_formula_exports_without_recursive_chain_growth() {
    let input = vec!["SUM(Prices)"; 400].join("+");
    let expected = vec!["SUM(Prices[])"; 400].join("+");
    let mut wb = workbook();
    put_formula(&mut wb, 0, "D1", &input);
    assert_eq!(file_formula(&wb, 0, "D1"), expected);
    assert_eq!(wb.sheet(0).unwrap().cells.get(at("D1")).unwrap().formula.as_ref().unwrap().text, input);
}

#[test]
fn empty_table_names_keep_their_existing_error_semantics() {
    for totals_row in [false, true] {
        let mut wb = workbook();
        let table = &mut wb.sheet_mut(0).unwrap().tables[0];
        table.range = RangeRef::parse(if totals_row { "A1:A2" } else { "A1:A1" }).unwrap();
        table.totals_row = totals_row;
        assert!(table.data_range().is_none());
        put_formula(&mut wb, 0, "D1", "SUM(Prices)");
        // Calculation does not resolve a bare name to a table without a data body;
        // forcing [] here would change its Name error into a structured-reference error.
        assert_eq!(file_formula(&wb, 0, "D1"), "SUM(Prices)");
    }
}
