//! Manual page breaks keep their extent: a break Excel limits to the print area (`brk max`, and
//! `min` when the area doesn't start at the sheet's edge) is written back as it was read, and a
//! break across the whole sheet stays one.

use super::minimal;
use crate::{read_xlsx, write_xlsx};

/// The first sheet's XML as written.
fn sheet_xml(bytes: &[u8]) -> String {
    String::from_utf8(crate::package::Package::open(bytes).unwrap().read("xl/worksheets/sheet1.xml").unwrap().unwrap()).unwrap()
}

/// The `<tag …>…</tag>` element of a sheet's XML, if written.
fn element<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let start = xml.find(&format!("<{tag}"))?;
    let end = xml[start..].find(&format!("</{tag}>")).unwrap() + tag.len() + 3;
    Some(&xml[start..start + end])
}

/// Reads `breaks` as the sheet's page breaks, saves, and returns the `rowBreaks` and `colBreaks`
/// written, with the workbook read back from the saved file.
fn round_trip(breaks: &str) -> (Option<String>, Option<String>, gridcraft_model::Workbook) {
    let (wb, _) = read_xlsx(&minimal(&format!("<sheetData/>{breaks}"), &[], "", "")).unwrap();
    let bytes = write_xlsx(&wb).unwrap();
    let xml = sheet_xml(&bytes);
    let (back, _) = read_xlsx(&bytes).unwrap();
    (element(&xml, "rowBreaks").map(str::to_string), element(&xml, "colBreaks").map(str::to_string), back)
}

#[test]
fn a_break_limited_to_the_print_area_stays_limited() {
    // The issue's two files: print area A1:C140 with a break before row 15, and A1:S10 with a
    // break before column E, as Excel writes them.
    let (rows, cols, back) = round_trip(
        r#"<rowBreaks count="1" manualBreakCount="1"><brk id="14" max="2" man="1"/></rowBreaks><colBreaks count="1" manualBreakCount="1"><brk id="4" max="9" man="1"/></colBreaks>"#,
    );
    assert_eq!(rows.as_deref(), Some(r#"<rowBreaks count="1" manualBreakCount="1"><brk id="14" max="2" man="1"/></rowBreaks>"#));
    assert_eq!(cols.as_deref(), Some(r#"<colBreaks count="1" manualBreakCount="1"><brk id="4" max="9" man="1"/></colBreaks>"#));
    let p = &back.sheet(0).unwrap().print;
    assert_eq!((p.row_breaks.clone(), p.row_break_extents.get(&14).copied()), (vec![14], Some((0, 2))));
    assert_eq!((p.col_breaks.clone(), p.col_break_extents.get(&4).copied()), (vec![4], Some((0, 9))));

    // A print area away from the sheet's edge: `min` is kept, and each break keeps its own.
    let (rows, _, _) = round_trip(
        r#"<rowBreaks count="2" manualBreakCount="2"><brk id="14" min="1" max="3" man="1"/><brk id="30" max="16383" man="1"/></rowBreaks>"#,
    );
    assert_eq!(
        rows.as_deref(),
        Some(r#"<rowBreaks count="2" manualBreakCount="2"><brk id="14" min="1" max="3" man="1"/><brk id="30" max="16383" man="1"/></rowBreaks>"#)
    );
}

#[test]
fn a_break_across_the_whole_sheet_stays_whole() {
    // Written across the sheet, without `max`, with `max="0"`, past the sheet, and backwards.
    for brk in [r#"max="16383""#, "", r#"max="0""#, r#"max="99999""#, r#"min="5" max="2""#] {
        let (rows, _, back) = round_trip(&format!(r#"<rowBreaks count="1" manualBreakCount="1"><brk id="14" {brk} man="1"/></rowBreaks>"#));
        assert_eq!(rows.as_deref(), Some(r#"<rowBreaks count="1" manualBreakCount="1"><brk id="14" max="16383" man="1"/></rowBreaks>"#), "{brk}");
        assert!(back.sheet(0).unwrap().print.row_break_extents.is_empty(), "{brk}");
    }
    let (_, cols, _) = round_trip(r#"<colBreaks count="1" manualBreakCount="1"><brk id="4" max="1048575" man="1"/></colBreaks>"#);
    assert_eq!(cols.as_deref(), Some(r#"<colBreaks count="1" manualBreakCount="1"><brk id="4" max="1048575" man="1"/></colBreaks>"#));
}
