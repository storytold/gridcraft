//! Sheet names XLSX cannot hold are refused on export instead of breaking formulas on reopen.

use std::sync::Arc;

use gridcraft_model::{Sheet, Workbook};

use crate::{IoError, write_xlsx};

fn book(names: &[&str]) -> Workbook {
    let mut wb = Workbook::new();
    wb.sheets = names.iter().map(|n| Arc::new(Sheet::new(*n))).collect();
    wb
}

#[test]
fn unrepresentable_sheet_names_are_refused() {
    let long_a = format!("{}X", "A".repeat(31));
    let long_b = format!("{}Y", "A".repeat(31));
    for names in [vec!["Data", "a/b"], vec![long_a.as_str(), long_b.as_str()], vec!["Data", "data"], vec!["Data", ""]] {
        assert!(matches!(write_xlsx(&book(&names)), Err(IoError::Format(_))), "{names:?}");
    }
    assert!(write_xlsx(&book(&["Data", "Other"])).is_ok());
}

/// Excel refuses a sheet name over 31 UTF-16 units, so 16 emoji (32 units) are refused on save
/// while 15 emoji and a letter (31 units) are written (#4454).
#[test]
fn sheet_name_length_is_counted_in_utf16_units() {
    let emoji = "😀".repeat(16);
    assert!(matches!(write_xlsx(&book(&[emoji.as_str()])), Err(IoError::Format(_))));
    let fits = format!("{}a", "😀".repeat(15));
    let (read, _) = crate::read_xlsx(&write_xlsx(&book(&[fits.as_str()])).unwrap()).unwrap();
    assert_eq!(read.sheets[0].name, fits);
    // A file that already has a longer name opens with one Excel can hold.
    let mut names = crate::read::SheetNames::default();
    assert_eq!(names.unique(&emoji, 0), "😀".repeat(15));
    assert_eq!(names.unique(&emoji, 1), format!("{} (2)", "😀".repeat(12)));
}

/// An XLSX whose sheet name is longer than Excel allows (as earlier GridCraft versions wrote)
/// opens with a shorter name and says so (#4454).
#[test]
fn an_xlsx_sheet_name_over_31_units_is_cut_with_a_warning() {
    use crate::tests::{CT, ROOT_RELS, make_zip};
    let ns = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    let wb =
        format!(r#"<?xml version="1.0"?><workbook {ns}><sheets><sheet name="{}" sheetId="1" r:id="rId1"/></sheets></workbook>"#, "😀".repeat(16));
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let sheet = format!(r#"<?xml version="1.0"?><worksheet {ns}><sheetData/></worksheet>"#);
    let bytes = make_zip(&[
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", wb.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ]);
    let (read, report) = crate::read_xlsx(&bytes).unwrap();
    assert_eq!(read.sheets[0].name, "😀".repeat(15));
    assert!(report.warnings.iter().any(|w| w.contains("renamed")), "{:?}", report.warnings);
    assert!(write_xlsx(&read).is_ok());
}

/// A saved sheet keeps its `sheetId`; a copied or new sheet gets one no other sheet asks for.
#[test]
fn sheet_ids_survive_a_save() {
    let mut wb = book(&["A", "B", "C", "D"]);
    for (sheet, id) in wb.sheets.iter_mut().zip([3, 7, 0, 3]) {
        Arc::make_mut(sheet).file_id = id;
    }
    let (read, _) = crate::read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let ids: Vec<u32> = read.sheets.iter().map(|s| s.file_id).collect();
    assert_eq!(ids, [3, 7, 1, 2]);
    let (again, _) = crate::read_xlsx(&write_xlsx(&read).unwrap()).unwrap();
    assert_eq!(again.sheets.iter().map(|s| s.file_id).collect::<Vec<_>>(), ids);
}

/// Repairing repeated names takes one set lookup per candidate, so thousands of sheets sharing
/// a name are renamed without rescanning the workbook for every suffix tried.
#[test]
fn repeated_sheet_names_are_repaired_without_rescans() {
    let mut names = crate::read::SheetNames::default();
    assert_eq!(names.unique("Data", 0), "Data");
    assert_eq!(names.unique("data", 1), "data (2)");
    assert_eq!(names.unique("", 2), "Sheet3");
    assert_eq!(names.unique("Data (3)", 3), "Data (3)");
    for i in 4..100_000 {
        assert_eq!(names.unique("Data", i), format!("Data ({i})"));
    }
}
