//! Page Setup › Sheet › Page order: `<pageSetup pageOrder>` is read and written back. As in
//! Excel's files, only "over, then down" is written; "down, then over" is the schema default.

use gridcraft_model::{PageOrder, Workbook};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

/// The first sheet's XML as written.
fn sheet_xml(bytes: &[u8]) -> String {
    String::from_utf8(crate::package::Package::open(bytes).unwrap().read("xl/worksheets/sheet1.xml").unwrap().unwrap()).unwrap()
}

/// The `<pageSetup …/>` element of a sheet's XML.
fn page_setup(xml: &str) -> &str {
    let start = xml.find("<pageSetup").unwrap_or_else(|| panic!("no pageSetup: {xml}"));
    let len = xml[start..].find("/>").unwrap() + 2;
    &xml[start..start + len]
}

#[test]
fn over_then_down_is_read_and_written_back() {
    // The attributes Excel writes for a sheet set to Over, then down.
    let body = r#"<sheetData/><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup paperSize="9" pageOrder="overThenDown" orientation="portrait"/>"#;
    let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().print.page_order, PageOrder::OverThenDown);
    let bytes = write_xlsx(&wb).unwrap();
    let xml = sheet_xml(&bytes);
    let ps = page_setup(&xml);
    assert_eq!(ps, r#"<pageSetup paperSize="9" pageOrder="overThenDown" orientation="portrait"/>"#);
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().print.page_order, PageOrder::OverThenDown);
    // CT_PageSetup's attribute order: Fit To before the page order, the page order before the
    // orientation.
    let mut wb = back;
    wb.sheet_mut(0).unwrap().print.fit_to = Some((1, 2));
    let xml = sheet_xml(&write_xlsx(&wb).unwrap());
    assert!(page_setup(&xml).contains(r#"fitToHeight="2" pageOrder="overThenDown" orientation="portrait""#), "{xml}");
}

#[test]
fn down_then_over_writes_no_page_order() {
    // Written out, absent, unknown (read as the default) and a sheet without page setup at all.
    for body in [
        r#"<sheetData/><pageSetup paperSize="9" pageOrder="downThenOver" orientation="portrait"/>"#,
        r#"<sheetData/><pageSetup paperSize="9" orientation="portrait"/>"#,
        r#"<sheetData/><pageSetup pageOrder="sideways"/>"#,
        "<sheetData/>",
    ] {
        let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
        assert_eq!(wb.sheet(0).unwrap().print.page_order, PageOrder::DownThenOver, "{body}");
        let xml = sheet_xml(&write_xlsx(&wb).unwrap());
        assert!(!xml.contains("pageOrder"), "{body}: {xml}");
        assert!(xml.matches("<pageSetup").count() <= 1, "{xml}");
    }
    // A new workbook, and a sheet set back to Down, then over.
    let mut wb = Workbook::new();
    assert!(!sheet_xml(&write_xlsx(&wb).unwrap()).contains("pageOrder"));
    wb.sheet_mut(0).unwrap().print.page_order = PageOrder::OverThenDown;
    assert!(sheet_xml(&write_xlsx(&wb).unwrap()).contains(r#"pageOrder="overThenDown""#));
    wb.sheet_mut(0).unwrap().print.page_order = PageOrder::DownThenOver;
    assert!(!sheet_xml(&write_xlsx(&wb).unwrap()).contains("pageOrder"));
}
