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

#[test]
fn page_margins_are_written_as_their_decimals() {
    // #1609 item 7: the f32 margins were widened to f64, so 0.3 came out as 0.30000001192092896.
    let body = r#"<sheetData/><pageMargins left="0.3" right="0.25" top="1.1" bottom="0.75" header="0.3" footer="0.3"/>"#;
    let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    let xml = sheet_xml(&write_xlsx(&wb).unwrap());
    assert!(xml.contains(r#"<pageMargins left="0.3" right="0.25" top="1.1" bottom="0.75" header="0.3" footer="0.3"/>"#), "{xml}");
    // The default sheet writes Excel's default margins exactly.
    let xml = sheet_xml(&write_xlsx(&Workbook::new()).unwrap());
    assert!(xml.contains(r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>"#), "{xml}");
}

#[test]
fn first_page_number_is_read_and_written_back() {
    // #4293: Excel writes Page Setup's First page number 7 as these two attributes; both were lost.
    let body = r#"<sheetData/><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup paperSize="9" firstPageNumber="7" orientation="portrait" useFirstPageNumber="1"/>"#;
    let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().print.first_page_number, Some(7));
    let bytes = write_xlsx(&wb).unwrap();
    let xml = sheet_xml(&bytes);
    assert_eq!(page_setup(&xml), r#"<pageSetup paperSize="9" firstPageNumber="7" orientation="portrait" useFirstPageNumber="1"/>"#);
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().print.first_page_number, Some(7));
    // In the schema's order with the other attributes: after the scale, before Fit To.
    let mut wb = back;
    wb.sheet_mut(0).unwrap().print.scale = 80;
    wb.sheet_mut(0).unwrap().print.fit_to = Some((2, 3));
    let xml = sheet_xml(&write_xlsx(&wb).unwrap());
    assert!(page_setup(&xml).contains(r#"scale="80" firstPageNumber="7" fitToWidth="2""#), "{xml}");
    // Automatic: no number, a number Excel keeps without using it, and `useFirstPageNumber="0"`.
    for body in [
        r#"<sheetData/><pageSetup paperSize="9" orientation="portrait"/>"#,
        r#"<sheetData/><pageSetup paperSize="9" firstPageNumber="5" orientation="portrait"/>"#,
        r#"<sheetData/><pageSetup paperSize="9" firstPageNumber="5" orientation="portrait" useFirstPageNumber="0"/>"#,
        "<sheetData/>",
    ] {
        let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
        assert_eq!(wb.sheet(0).unwrap().print.first_page_number, None, "{body}");
        let xml = sheet_xml(&write_xlsx(&wb).unwrap());
        assert!(!xml.contains("irstPageNumber"), "{body}: {xml}");
    }
    // 1 is a number of its own (Excel's field shows 1, not Automatic) and is kept too.
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().print.first_page_number = Some(1);
    let xml = sheet_xml(&write_xlsx(&wb).unwrap());
    assert!(xml.contains(r#"firstPageNumber="1""#) && xml.contains(r#"useFirstPageNumber="1""#), "{xml}");
}

#[test]
fn scale_and_default_fit_to_do_not_inject_fit_to_width_or_height() {
    // Explicit scale without fitToPage must not emit fitToWidth / fitToHeight or fitToPage.
    let body_scale = r#"<sheetFormatPr defaultRowHeight="12.75"/><sheetData/><pageSetup scale="50" orientation="landscape"/>"#;
    let (wb, _) = read_xlsx(&minimal(body_scale, &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().print.scale, 50);
    assert_eq!(wb.sheet(0).unwrap().print.fit_to, None);
    let xml = sheet_xml(&write_xlsx(&wb).unwrap());
    let ps = page_setup(&xml);
    assert!(ps.contains(r#"scale="50""#), "{ps}");
    assert!(!ps.contains("fitToWidth") && !ps.contains("fitToHeight"), "{ps}");
    assert!(!xml.contains("fitToPage"), "{xml}");
    assert!(!xml.contains(r#"customHeight="1""#), "non-custom defaultRowHeight must not inject customHeight: {xml}");

    // fitToPage="1" with schema-default 1x1 (omitted fitToWidth/fitToHeight) and explicit scale
    // preserves fitToPage and scale without injecting fitToWidth="1" fitToHeight="1".
    let body_fit = r#"<sheetPr><pageSetUpPr fitToPage="1"/></sheetPr><sheetFormatPr defaultRowHeight="21.95" customHeight="1"/><sheetData/><pageSetup scale="48" orientation="portrait"/>"#;
    let (wb, _) = read_xlsx(&minimal(body_fit, &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().print.scale, 48);
    assert_eq!(wb.sheet(0).unwrap().print.fit_to, Some((1, 1)));
    let bytes = write_xlsx(&wb).unwrap();
    let xml = sheet_xml(&bytes);
    let ps = page_setup(&xml);
    assert!(xml.contains(r#"<pageSetUpPr fitToPage="1"/>"#), "{xml}");
    assert!(ps.contains(r#"scale="48""#), "{ps}");
    assert!(!ps.contains("fitToWidth") && !ps.contains("fitToHeight"), "{ps}");
    assert!(xml.contains(r#"defaultRowHeight="21.95" customHeight="1""#), "{xml}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().print.scale, 48);
    assert_eq!(back.sheet(0).unwrap().print.fit_to, Some((1, 1)));
}

#[test]
fn default_row_height_custom_survives_json_and_programmatic_sheets() {
    // (a) Source XLSX without customHeight="1" keeps customHeight omitted even across JSON serde.
    let body_plain = r#"<sheetFormatPr defaultRowHeight="12.75"/><sheetData/>"#;
    let (wb_plain, _) = read_xlsx(&minimal(body_plain, &[], "", "")).unwrap();
    assert_eq!(wb_plain.sheet(0).unwrap().default_row_height_custom, Some(false));
    let json_plain = serde_json::to_string(&wb_plain).unwrap();
    let back_plain: Workbook = serde_json::from_str(&json_plain).unwrap();
    assert_eq!(back_plain.sheet(0).unwrap().default_row_height_custom, Some(false));
    let xml_plain = sheet_xml(&write_xlsx(&back_plain).unwrap());
    assert!(!xml_plain.contains(r#"customHeight="1""#), "plain sheet must omit customHeight after JSON round-trip: {xml_plain}");

    // Explicit customHeight="1" also survives JSON serde round-trip.
    let body_custom = r#"<sheetFormatPr defaultRowHeight="21.95" customHeight="1"/><sheetData/>"#;
    let (wb_custom, _) = read_xlsx(&minimal(body_custom, &[], "", "")).unwrap();
    assert_eq!(wb_custom.sheet(0).unwrap().default_row_height_custom, Some(true));
    let back_custom: Workbook = serde_json::from_str(&serde_json::to_string(&wb_custom).unwrap()).unwrap();
    let xml_custom = sheet_xml(&write_xlsx(&back_custom).unwrap());
    assert!(xml_custom.contains(r#"defaultRowHeight="21.95" customHeight="1""#), "{xml_custom}");

    // (b) Programmatic sheet with default_row_height_custom == None omits customHeight at default
    // 15pt (20px) and emits customHeight="1" for non-15pt heights (including after JSON serde).
    let mut prog = Workbook::new();
    assert_eq!(prog.sheet(0).unwrap().default_row_height_custom, None);
    let prog_json = serde_json::to_string(&prog).unwrap();
    assert!(!prog_json.contains("default_row_height_custom"), "None must be omitted from JSON: {prog_json}");
    let xml_default = sheet_xml(&write_xlsx(&prog).unwrap());
    assert!(!xml_default.contains(r#"customHeight="1""#), "default 15pt programmatic sheet must omit customHeight: {xml_default}");

    prog.sheet_mut(0).unwrap().default_row_height = 24.0;
    let prog_back: Workbook = serde_json::from_str(&serde_json::to_string(&prog).unwrap()).unwrap();
    assert_eq!(prog_back.sheet(0).unwrap().default_row_height_custom, None);
    let xml_tall = sheet_xml(&write_xlsx(&prog_back).unwrap());
    assert!(xml_tall.contains(r#"defaultRowHeight="18" customHeight="1""#), "non-15pt programmatic sheet must emit customHeight: {xml_tall}");
}
