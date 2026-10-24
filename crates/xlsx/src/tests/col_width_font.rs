//! Column widths count in the Normal font's maximum digit width: a Calibri 12 workbook's widths
//! are 8 px characters, Calibri 11's are 7 px.

use std::io::Read;

use gridcraft_model::Workbook;

use super::minimal;
use crate::{read_xlsx, write_xlsx};

const RELS: &str =
    r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;

fn styles(font: &str, size: &str) -> String {
    format!(
        r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="{size}"/><name val="{font}"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs></styleSheet>"#
    )
}

/// A one-sheet book in `font`/`size` whose column B is 13 characters wide.
fn book(font: &str, size: &str, format_pr: &str) -> Workbook {
    let body = format!(r#"{format_pr}<cols><col min="2" max="2" width="13" customWidth="1"/></cols><sheetData/>"#);
    read_xlsx(&minimal(&body, &[("xl/styles.xml", &styles(font, size))], RELS, "")).unwrap().0
}

fn col_b(wb: &Workbook) -> Option<f32> {
    wb.sheet(0).unwrap().cols.get(&1).and_then(|c| c.size)
}

fn sheet_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut s).unwrap();
    s
}

#[test]
fn calibri_11_counts_seven_px_digits() {
    let wb = book("Calibri", "11", "");
    assert_eq!(wb.digit_width(), 7.0);
    assert_eq!(col_b(&wb), Some(91.0));
    assert_eq!(wb.sheet(0).unwrap().default_col_width, 64.0);
}

#[test]
fn calibri_12_counts_eight_px_digits() {
    let wb = book("Calibri", "12", "");
    assert_eq!(wb.digit_width(), 8.0);
    assert_eq!(col_b(&wb), Some(104.0));
    // Eight digits plus padding, rounded up to 8 px.
    assert_eq!(wb.sheet(0).unwrap().default_col_width, 72.0);
    // A Mac-saved sheet's base width of 10 characters.
    let wb = book("Calibri", "12", r#"<sheetFormatPr baseColWidth="10" defaultRowHeight="16"/>"#);
    assert_eq!(wb.sheet(0).unwrap().default_col_width, 88.0);
}

#[test]
fn other_normal_fonts() {
    // Arial 10 has 7 px digits like Calibri 11: the 64 px default column.
    let wb = book("Arial", "10", "");
    assert_eq!(wb.digit_width(), 7.0);
    assert_eq!(wb.sheet(0).unwrap().default_col_width, 64.0);
    // Arial 16: 12 px digits.
    let wb = book("Arial", "16", "");
    assert_eq!(wb.digit_width(), 12.0);
    assert_eq!(col_b(&wb), Some(156.0));
    // An unknown font counts like Calibri; a hostile size falls back to 11 pt.
    assert_eq!(book("No Such Font", "11", "").digit_width(), 7.0);
    assert_eq!(book("Calibri", "NaN", "").digit_width(), 7.0);
    assert_eq!(book("Calibri", "1e30", "").digit_width(), book("Calibri", "409", "").digit_width());
}

#[test]
fn widths_round_trip_in_the_files_units() {
    for (font, size) in [("Calibri", "11"), ("Calibri", "12"), ("Arial", "16")] {
        let wb = book(font, size, "");
        let bytes = write_xlsx(&wb).unwrap();
        let xml = sheet_xml(&bytes);
        // The default column of the font is implied, not written out.
        assert!(!xml.contains("defaultColWidth"), "{font} {size}: {xml}");
        let (back, _) = read_xlsx(&bytes).unwrap();
        assert_eq!(back.digit_width(), wb.digit_width(), "{font} {size}");
        assert_eq!(col_b(&back), col_b(&wb), "{font} {size}");
        assert_eq!(back.sheet(0).unwrap().default_col_width, wb.sheet(0).unwrap().default_col_width, "{font} {size}");
    }
}
