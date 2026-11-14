//! Original Numbers packages assembled in Rust (see `crate::testing`), not files from Apple's software.

use gridcraft_core::{CellError, CellRef, Value};

use crate::testing::{NumbersCell as N, numbers_document, numbers_package, snappy_literals};
use crate::{Format, IoError, read_numbers, read_xlsx, sniff, write_xlsx};

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

fn at(wb: &gridcraft_model::Workbook, sheet: usize, row: u32, col: u32) -> Value {
    wb.sheets[sheet].value(CellRef::new(row, col))
}

#[test]
fn numbers_values_of_every_kind_are_imported() {
    let bytes = numbers_document(&[(
        "Budget",
        vec![(
            "Table 1",
            vec![
                vec![N::Text("Item"), N::Rich("Rich note"), N::Number(-12.5)],
                vec![N::Number(1234.0), N::Empty, N::Number(0.1)],
                vec![N::Bool(true), N::Date(86_400.0 * 365.0), N::Duration(5400.0)],
                vec![N::Date(43_200.0), N::Error, N::Text("ΕΥΠ ünï")],
            ],
        )],
    )]);
    assert_eq!(sniff(&bytes), Format::Numbers);
    let (wb, report) = read_numbers(&bytes).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("Numbers data-only import")));
    assert_eq!(wb.sheets.len(), 1);
    assert_eq!(wb.sheets[0].name, "Budget");
    assert_eq!(at(&wb, 0, 0, 0), text("Item"));
    assert_eq!(at(&wb, 0, 0, 1), text("Rich note"));
    assert_eq!(at(&wb, 0, 0, 2), Value::Number(-12.5));
    assert_eq!(at(&wb, 0, 1, 0), Value::Number(1234.0));
    assert_eq!(at(&wb, 0, 1, 1), Value::Empty);
    assert_eq!(at(&wb, 0, 1, 2), Value::Number(0.1));
    assert_eq!(at(&wb, 0, 2, 0), Value::Bool(true));
    // 2001-01-01 is serial 36892 and a duration is a fraction of a day.
    assert_eq!(at(&wb, 0, 2, 1), Value::Number(36_892.0 + 365.0));
    assert_eq!(at(&wb, 0, 2, 2), Value::Number(5400.0 / 86_400.0));
    assert_eq!(at(&wb, 0, 3, 0), Value::Number(36_892.5));
    assert_eq!(at(&wb, 0, 3, 1), Value::Error(CellError::Value));
    assert_eq!(at(&wb, 0, 3, 2), text("ΕΥΠ ünï"));
}

#[test]
fn numbers_dates_get_date_formats_and_survive_xlsx_round_trip() {
    let bytes = numbers_document(&[("S", vec![("T", vec![vec![N::Date(86_400.0 * 10.0), N::Date(43_200.0), N::Duration(3600.0)]])])]);
    let (wb, _) = read_numbers(&bytes).unwrap();
    let code = |col| {
        let sheet = &wb.sheets[0];
        wb.styles.get(sheet.style_id(CellRef::new(0, col))).num_fmt.as_str().to_string()
    };
    assert_eq!(code(0), "yyyy-mm-dd");
    assert_eq!(code(1), "yyyy-mm-dd hh:mm:ss");
    assert_eq!(code(2), "[h]:mm:ss");
    let saved = write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&saved).unwrap();
    assert_eq!(at(&back, 0, 0, 0), Value::Number(36_902.0));
}

#[test]
fn numbers_tables_become_sheets_with_unique_names() {
    let bytes = numbers_document(&[
        ("Plan", vec![("Costs", vec![vec![N::Number(1.0)]]), ("Dates", vec![vec![N::Number(2.0)]])]),
        ("Plan", vec![("Only", vec![vec![N::Number(3.0)]])]),
        ("Empty sheet", vec![]),
    ]);
    let (wb, report) = read_numbers(&bytes).unwrap();
    let names: Vec<&str> = wb.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Plan - Costs", "Plan - Dates", "Plan"]);
    assert_eq!(at(&wb, 2, 0, 0), Value::Number(3.0));
    assert!(report.warnings.iter().any(|w| w.contains("without tables")));
}

#[test]
fn numbers_rows_past_one_tile_land_on_the_right_rows() {
    let rows: Vec<Vec<N>> = (0..600).map(|i| vec![N::Number(f64::from(i))]).collect();
    let (wb, _) = read_numbers(&numbers_document(&[("Big", vec![("T", rows)])])).unwrap();
    for row in [0, 255, 256, 511, 512, 599] {
        assert_eq!(at(&wb, 0, row, 0), Value::Number(f64::from(row)));
    }
}

#[test]
fn numbers_import_is_not_confused_with_other_zip_formats() {
    let xlsx = write_xlsx(&gridcraft_model::Workbook::new()).unwrap();
    assert_eq!(sniff(&xlsx), Format::Xlsx);
    assert!(read_numbers(&xlsx).is_err());
}

#[test]
fn malformed_numbers_packages_are_errors_not_panics() {
    assert!(read_numbers(b"not a zip").is_err());
    let doc = |bytes: &[u8]| numbers_package(&[("Index/Document.iwa", bytes)]);
    let block = snappy_literals(&[1, 2, 3]);
    let frame = |kind: u8, block: &[u8]| [&[kind, block.len() as u8, 0, 0][..], block].concat();
    for bad in [
        vec![],
        vec![1, 0, 0, 0],
        vec![0, 9, 0, 0, 1],
        frame(0, &block),
        frame(0, &[0xFF; 12]),
        frame(0, &snappy_literals(&[0xFF; 40])),
        // A copy that reaches before the start of the output.
        frame(0, &[4, 0b0000_0101, 9]),
        // A declared length far over the limit.
        frame(0, &[0xFF, 0xFF, 0xFF, 0xFF, 0x0F, 0]),
    ] {
        assert!(read_numbers(&doc(&bad)).is_err(), "{bad:?}");
    }
    // A package without the document index is not a Numbers document.
    assert!(matches!(read_numbers(&numbers_package(&[("other.txt", b"x")])), Err(IoError::Zip(_) | IoError::Format(_))));
}
