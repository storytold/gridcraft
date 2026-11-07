//! Ignore Error: the sheet's `ignoredErrors` are read and written back. The element bodies below
//! are what Excel saved after `Range.Errors(n).Ignore = True` on synthetic workbooks.

use std::io::Read;

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{error_rule, is_error_ignored};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

/// One record per rule, as Excel wrote it for B1 (evalError), C1 (text date), E3 (formula), A6
/// (number as text), D1 (formulaRange), E1 (unlockedFormula), F10 (emptyCellReference) and G1
/// (listDataValidation).
const EXCEL_EACH_RULE: &str = r#"<sheetData/><ignoredErrors><ignoredError sqref="B1" evalError="1"/><ignoredError sqref="C1" twoDigitTextYear="1"/><ignoredError sqref="E3" formula="1"/><ignoredError sqref="A6" numberStoredAsText="1"/><ignoredError sqref="D1" formulaRange="1"/><ignoredError sqref="E1" unlockedFormula="1"/><ignoredError sqref="F10" emptyCellReference="1"/><ignoredError sqref="G1" listDataValidation="1"/></ignoredErrors>"#;

/// Cells ignored in separate calls come back as one multi-area sqref; turning Ignore off on A1
/// leaves a record with no rule on it.
const EXCEL_MERGED: &str =
    r#"<sheetData/><ignoredErrors><ignoredError sqref="A2:A3 C1:D1" numberStoredAsText="1"/><ignoredError sqref="A1"/></ignoredErrors>"#;

fn cell(a1: &str) -> CellRef {
    CellRef::parse(a1).unwrap()
}

fn written_sheet(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut s).unwrap();
    s
}

#[test]
fn each_rule_excel_saved_is_read_and_written_back() {
    let (wb, _) = read_xlsx(&minimal(EXCEL_EACH_RULE, &[], "", "")).unwrap();
    let sh = &wb.sheets[0];
    for (a1, rule) in [
        ("B1", error_rule::EVAL_ERROR),
        ("C1", error_rule::TWO_DIGIT_TEXT_YEAR),
        ("E3", error_rule::FORMULA),
        ("A6", error_rule::NUMBER_STORED_AS_TEXT),
        ("D1", error_rule::FORMULA_RANGE),
        ("E1", error_rule::UNLOCKED_FORMULA),
        ("F10", error_rule::EMPTY_CELL_REFERENCE),
        ("G1", error_rule::LIST_DATA_VALIDATION),
    ] {
        assert!(is_error_ignored(&sh.ignored_errors, cell(a1), rule), "{a1}");
        // Only that rule, and only that cell.
        assert!(!is_error_ignored(&sh.ignored_errors, cell(a1), error_rule::ALL & !rule), "{a1}");
    }
    assert!(!is_error_ignored(&sh.ignored_errors, cell("B2"), error_rule::ALL));

    let bytes = write_xlsx(&wb).unwrap();
    let xml = written_sheet(&bytes);
    for expected in [
        r#"<ignoredError sqref="B1" evalError="1"/>"#,
        r#"<ignoredError sqref="C1" twoDigitTextYear="1"/>"#,
        r#"<ignoredError sqref="E3" formula="1"/>"#,
        r#"<ignoredError sqref="A6" numberStoredAsText="1"/>"#,
        r#"<ignoredError sqref="D1" formulaRange="1"/>"#,
        r#"<ignoredError sqref="E1" unlockedFormula="1"/>"#,
        r#"<ignoredError sqref="F10" emptyCellReference="1"/>"#,
        r#"<ignoredError sqref="G1" listDataValidation="1"/>"#,
    ] {
        assert!(xml.contains(expected), "{expected} in {xml}");
    }
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheets[0].ignored_errors, wb.sheets[0].ignored_errors);
}

#[test]
fn a_multi_area_sqref_is_kept_and_a_record_with_no_rule_ignores_nothing() {
    let (wb, _) = read_xlsx(&minimal(EXCEL_MERGED, &[], "", "")).unwrap();
    let sh = &wb.sheets[0];
    let rule = error_rule::NUMBER_STORED_AS_TEXT;
    for a1 in ["A2", "A3", "C1", "D1"] {
        assert!(is_error_ignored(&sh.ignored_errors, cell(a1), rule), "{a1}");
    }
    for a1 in ["A1", "A4", "C2", "E1"] {
        assert!(!is_error_ignored(&sh.ignored_errors, cell(a1), rule), "{a1}");
    }
    assert_eq!(sh.ignored_errors.len(), 1);
    assert_eq!(sh.ignored_errors[0].ranges, [RangeRef::parse("A2:A3").unwrap(), RangeRef::parse("C1:D1").unwrap()]);
    let xml = written_sheet(&write_xlsx(&wb).unwrap());
    assert!(xml.contains(r#"<ignoredError sqref="A2:A3 C1:D1" numberStoredAsText="1"/>"#), "{xml}");
    assert!(!xml.contains(r#"<ignoredError sqref="A1""#), "{xml}");
}

#[test]
fn a_sheet_without_ignored_errors_writes_no_element() {
    let (wb, _) = read_xlsx(&minimal("<sheetData/>", &[], "", "")).unwrap();
    assert!(!written_sheet(&write_xlsx(&wb).unwrap()).contains("ignoredErrors"));
}
