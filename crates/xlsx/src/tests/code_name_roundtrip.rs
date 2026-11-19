//! `<sheetPr codeName="…">` round-trips. This is the VBA project's name for a sheet's
//! code-behind module, independent of the sheet's display name - losing it (as `code_name`'s
//! absence used to mean, silently) can leave a workbook's VBA project unable to bind to its
//! sheets even though `vbaProject.bin` itself is untouched.

use std::io::Read;
use std::sync::Arc;

use gridcraft_model::{Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

fn book_with_code_name(code_name: Option<&str>) -> Workbook {
    let mut wb = Workbook::new();
    let mut sheet = Sheet::new("Основное");
    sheet.code_name = code_name.map(str::to_string);
    wb.sheets = vec![Arc::new(sheet)];
    wb
}

/// The common real-world case: a sheet whose only `sheetPr`-worthy property is its codeName
/// (default tab color, default outline summary settings, not set to fit to page) - before this
/// fix, `<sheetPr>` was only ever written when one of the *other* properties needed it, so this
/// exact shape silently dropped the codeName on every save.
#[test]
fn code_name_alone_survives_a_save_even_with_no_other_sheet_pr() {
    let wb = book_with_code_name(Some("Лист1"));
    let (read, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(read.sheets[0].code_name.as_deref(), Some("Лист1"));
}

/// codeName coexists with the other `sheetPr` children once something else also needs the
/// element written.
#[test]
fn code_name_survives_alongside_other_sheet_pr_children() {
    let wb = book_with_code_name(Some("Лист1"));
    let mut wb = wb;
    Arc::make_mut(&mut wb.sheets[0]).summary_right = false;
    let (read, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(read.sheets[0].code_name.as_deref(), Some("Лист1"));
    assert!(!read.sheets[0].summary_right);
}

/// No codeName in, no `codeName` attribute out, and `<sheetPr>` itself is still skipped when
/// nothing else needs it either (no regression on the common plain-sheet case).
#[test]
fn no_code_name_writes_no_sheet_pr() {
    let wb = book_with_code_name(None);
    let bytes = write_xlsx(&wb).unwrap();
    let mut sheet_xml = String::new();
    zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap().by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut sheet_xml).unwrap();
    assert!(!sheet_xml.contains("sheetPr"), "{sheet_xml}");
    let (read, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(read.sheets[0].code_name, None);
}

/// A codeName with XML-significant characters is escaped on write and decoded back exactly -
/// real codeNames are plain identifiers, but nothing here should assume that.
#[test]
fn code_name_with_special_characters_round_trips() {
    let wb = book_with_code_name(Some("A\"B&C<D>E"));
    let (read, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(read.sheets[0].code_name.as_deref(), Some("A\"B&C<D>E"));
}
