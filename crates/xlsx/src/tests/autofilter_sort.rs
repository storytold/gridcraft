//! #4292: a worksheet AutoFilter's saved sort (`autoFilter/sortState`) is read and written back,
//! so Excel's Data › Reapply sorts by it again after a GridCraft save.

use super::minimal;
use crate::{read_xlsx, write_xlsx};

const DATA: &str = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Name</t></is></c><c r="B1" t="inlineStr"><is><t>Amount</t></is></c></row><row r="2"><c r="B2"><v>2</v></c></row><row r="3"><c r="B3"><v>5</v></c></row><row r="4"><c r="B4"><v>3</v></c></row></sheetData>"#;

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

/// The `<autoFilter>` written after reading `filter`, and the one written after reading that back.
fn written(filter: &str) -> (String, String) {
    let (wb, _) = read_xlsx(&minimal(&format!("{DATA}{filter}"), &[], "", "")).unwrap();
    let bytes = write_xlsx(&wb).unwrap();
    let first = autofilter(&sheet_xml(&bytes)).to_string();
    let (back, _) = read_xlsx(&bytes).unwrap();
    (first, autofilter(&sheet_xml(&write_xlsx(&back).unwrap())).to_string())
}

#[test]
fn a_saved_sort_survives_a_save() {
    for filter in [
        // The issue's file: Amount sorted largest to smallest from its dropdown.
        r#"<autoFilter ref="A1:B4"><sortState ref="A2:B4"><sortCondition descending="1" ref="B2:B4"/></sortState></autoFilter>"#,
        // Two keys, case-sensitive, ascending first.
        r#"<autoFilter ref="A1:B4"><sortState caseSensitive="1" ref="A2:B4"><sortCondition ref="A2:A4"/><sortCondition descending="1" ref="B2:B4"/></sortState></autoFilter>"#,
        // After a criterion and a hidden arrow, in CT_AutoFilter's order.
        r#"<autoFilter ref="A1:B4"><filterColumn colId="1"><filters><filter val="5"/></filters></filterColumn><filterColumn colId="0" hiddenButton="1"/><sortState ref="A2:B4"><sortCondition descending="1" ref="B2:B4"/></sortState></autoFilter>"#,
    ] {
        let (first, again) = written(filter);
        assert_eq!(first, filter);
        assert_eq!(again, filter);
    }
}

#[test]
fn a_sort_by_colour_icon_or_list_is_not_half_kept() {
    // A colour or icon key, a custom list, a sort across columns, and no key at all: the whole
    // state is left out rather than kept as a different sort.
    for state in [
        r#"<sortState ref="A2:B4"><sortCondition sortBy="cellColor" dxfId="0" ref="B2:B4"/></sortState>"#,
        r#"<sortState ref="A2:B4"><sortCondition ref="A2:A4"/><sortCondition sortBy="icon" iconSet="3Arrows" ref="B2:B4"/></sortState>"#,
        r#"<sortState ref="A2:B4"><sortCondition customList="Low,High" ref="B2:B4"/></sortState>"#,
        r#"<sortState columnSort="1" ref="A2:B4"><sortCondition ref="A2:B2"/></sortState>"#,
        r#"<sortState ref="A2:B4"/>"#,
    ] {
        let (first, _) = written(&format!(r#"<autoFilter ref="A1:B4">{state}</autoFilter>"#));
        assert_eq!(first, r#"<autoFilter ref="A1:B4"/>"#, "{state}");
    }
    // A sortBy of "value" is the default, written out.
    let (first, _) =
        written(r#"<autoFilter ref="A1:B4"><sortState ref="A2:B4"><sortCondition sortBy="value" ref="B2:B4"/></sortState></autoFilter>"#);
    assert_eq!(first, r#"<autoFilter ref="A1:B4"><sortState ref="A2:B4"><sortCondition ref="B2:B4"/></sortState></autoFilter>"#);
}
