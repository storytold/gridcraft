//! AutoFilter › a column's hidden dropdown arrow (`filterColumn hiddenButton`, Excel's
//! `VisibleDropDown:=False`) is read and written back, with or without a criterion on it.

use gridcraft_model::FilterCriterion;

use super::minimal;
use crate::{read_xlsx, write_xlsx};

const DATA: &str = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Name</t></is></c><c r="B1" t="inlineStr"><is><t>Amount</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>x</t></is></c><c r="B2"><v>1</v></c></row></sheetData>"#;

/// The first sheet's XML as written.
fn sheet_xml(bytes: &[u8]) -> String {
    String::from_utf8(crate::package::Package::open(bytes).unwrap().read("xl/worksheets/sheet1.xml").unwrap().unwrap()).unwrap()
}

/// The `<autoFilter …>` element of a sheet's XML.
fn autofilter(xml: &str) -> &str {
    let start = xml.find("<autoFilter").unwrap_or_else(|| panic!("no autoFilter: {xml}"));
    let rest = &xml[start..];
    let len = match (rest.find("/>"), rest.find('>')) {
        (Some(a), Some(b)) if a + 1 == b => a + 2,
        _ => rest.find("</autoFilter>").unwrap() + "</autoFilter>".len(),
    };
    &rest[..len]
}

/// Reads `filter` as the sheet's AutoFilter, saves, and returns the hidden arrows read, the
/// `<autoFilter>` written, and the hidden arrows read back from the saved file.
fn round_trip(filter: &str) -> (Vec<u32>, String, Vec<u32>) {
    let (wb, _) = read_xlsx(&minimal(&format!("{DATA}{filter}"), &[], "", "")).unwrap();
    let read = wb.sheet(0).unwrap().autofilter.as_ref().unwrap().hidden_buttons.clone();
    let bytes = write_xlsx(&wb).unwrap();
    let written = autofilter(&sheet_xml(&bytes)).to_string();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let again = back.sheet(0).unwrap().autofilter.as_ref().unwrap().hidden_buttons.clone();
    (read, written, again)
}

#[test]
fn a_hidden_filter_arrow_survives_a_save() {
    // The issue's file: the first header's arrow hidden, no criterion anywhere.
    let (read, written, again) = round_trip(r#"<autoFilter ref="A1:B2"><filterColumn colId="0" hiddenButton="1"/></autoFilter>"#);
    assert_eq!(read, vec![0]);
    assert_eq!(written, r#"<autoFilter ref="A1:B2"><filterColumn colId="0" hiddenButton="1"/></autoFilter>"#);
    assert_eq!(again, vec![0]);

    // Both arrows hidden, listed out of order: read in column order.
    let (read, written, _) =
        round_trip(r#"<autoFilter ref="A1:B2"><filterColumn colId="1" hiddenButton="1"/><filterColumn colId="0" hiddenButton="1"/></autoFilter>"#);
    assert_eq!(read, vec![0, 1]);
    assert_eq!(
        written,
        r#"<autoFilter ref="A1:B2"><filterColumn colId="0" hiddenButton="1"/><filterColumn colId="1" hiddenButton="1"/></autoFilter>"#
    );
}

#[test]
fn a_hidden_arrow_on_a_filtered_column_is_written_once() {
    let (wb, _) = read_xlsx(&minimal(
        &format!(
            r#"{DATA}<autoFilter ref="A1:B2"><filterColumn colId="0" hiddenButton="1"><filters><filter val="x"/></filters></filterColumn></autoFilter>"#
        ),
        &[],
        "",
        "",
    ))
    .unwrap();
    let af = wb.sheet(0).unwrap().autofilter.clone().unwrap();
    assert_eq!(af.hidden_buttons, vec![0]);
    assert_eq!(af.criteria, vec![(0, FilterCriterion::Values { values: vec!["x".into()], blanks: false })]);
    let xml = sheet_xml(&write_xlsx(&wb).unwrap());
    assert_eq!(
        autofilter(&xml),
        r#"<autoFilter ref="A1:B2"><filterColumn colId="0" hiddenButton="1"><filters><filter val="x"/></filters></filterColumn></autoFilter>"#
    );
}

#[test]
fn shown_arrows_write_no_filter_column() {
    // Written as shown, absent, and a column past the filter's range (which has no arrow to hide).
    for filter in [
        r#"<autoFilter ref="A1:B2"><filterColumn colId="0" hiddenButton="0"/></autoFilter>"#,
        r#"<autoFilter ref="A1:B2"/>"#,
        r#"<autoFilter ref="A1:B2"><filterColumn colId="2" hiddenButton="1"/></autoFilter>"#,
    ] {
        let (read, written, _) = round_trip(filter);
        assert!(read.is_empty(), "{filter}: {read:?}");
        assert_eq!(written, r#"<autoFilter ref="A1:B2"/>"#, "{filter}");
    }
}
