//! Workbook and sheet extensions (`<extLst>`) kept on save. Packages are built in code.

use std::io::Read;

use super::{CT, ROOT_RELS, make_zip};
use crate::{read_xlsx, write_xlsx};

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const NS_X15: &str = "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main";
const NS_MX: &str = "http://schemas.microsoft.com/office/mac/excel/2008/main";

const CALC: &str = r#"<ext uri="{B58B0392-4F1F-4190-BB64-5DF3571DCE5F}" xmlns:xcalcf="http://schemas.microsoft.com/office/spreadsheetml/2018/calcfeatures"><xcalcf:calcFeatures><xcalcf:feature name="microsoft.com:RD"/></xcalcf:calcFeatures></ext>"#;
const X14_WB: &str = r#"<ext uri="{79F54976-1DA5-4618-B147-4CDE4B953A38}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:workbookPr defaultImageDpi="220"/></ext>"#;
const X15_WB: &str = r#"<ext uri="{140A7094-0E35-4892-8432-C4D2E57EDEB5}" xmlns:x15="http://schemas.microsoft.com/office/spreadsheetml/2010/11/main"><x15:workbookPr chartTrackingRefBase="1"/></ext>"#;
const XLWCV: &str = r#"<ext uri="{D14903EA-33C4-47F7-8F05-3474C54BE107}" xmlns:xlwcv="http://schemas.microsoft.com/office/spreadsheetml/2024/workbookCompatibilityVersion"><xlwcv:version setVersion="1"/></ext>"#;
const ARCH_ID: &str = r#"<ext uri="{7523E5D3-25F3-A5E0-1632-64F254C22452}" xmlns:mx="http://schemas.microsoft.com/office/mac/excel/2008/main"><mx:ArchID Flags="2"/></ext>"#;
const PLV: &str = r#"<ext uri="{64002731-A6B0-56B0-2670-7721B7C09600}" xmlns:mx="http://schemas.microsoft.com/office/mac/excel/2008/main"><mx:PLV Mode="1" OnePage="0" WScale="0"/></ext>"#;

/// A workbook whose `<extLst>` holds the kept extensions, interleaved with ones that point at
/// other content (the data model, Google Sheets custom data); sheet "A" has a sparkline, Mac page
/// layout settings and a web extension reference.
fn package() -> Vec<u8> {
    let workbook = format!(
        r#"<?xml version="1.0"?><workbook xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><fileVersion appName="xl" lastEdited="7" lowestEdited="7" rupBuild="27328"/><workbookPr/><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets><calcPr calcId="191029"/><extLst>{X14_WB}<ext uri="{{FCE2AD5D-F65C-4FA6-A056-5C36A1767C68}}" xmlns:x15="{NS_X15}"><x15:dataModel><x15:modelTables><x15:modelTable id="T" name="T" connection="C"/></x15:modelTables></x15:dataModel></ext>{X15_WB}{CALC}<ext uri="GOOGLESHEETSCUSTOMDATAVERSION2"><go:sheetsCustomData xmlns:go="http://customooxmlschemas.google.com/" r:id="rId9" roundtripDataChecksum="x"/></ext>{XLWCV}{ARCH_ID}</extLst></workbook>"#
    );
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row></sheetData><extLst><ext uri="{{05C60535-1F16-4fd2-B633-F4F36F0B64E0}}" xmlns:x14="{NS_X14}"><x14:sparklineGroups xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:sparklineGroup><x14:colorSeries rgb="FF376092"/><x14:sparklines><x14:sparkline><xm:f>A!A1:B1</xm:f><xm:sqref>C1</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup></x14:sparklineGroups></ext><ext uri="{{64002731-A6B0-56B0-2670-7721B7C09600}}" xmlns:mx="{NS_MX}"><mx:PLV Mode="1" OnePage="0" WScale="0"/></ext><ext uri="{{F7C9EE02-42E1-4005-9D12-6889AFFD525C}}" xmlns:x15="{NS_X15}"><x15:webExtensions><x15:webExtension appRef="{{00000000-0000-0000-0000-000000000000}}"/></x15:webExtensions></ext></extLst></worksheet>"#
    );
    let rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{NS_R}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
    );
    make_zip(&[
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ])
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

/// The `<extLst>…</extLst>` of a part.
fn ext_lst(xml: &str) -> &str {
    let a = xml.find("<extLst>").unwrap_or_else(|| panic!("no extLst: {xml}"));
    let b = xml.find("</extLst>").unwrap();
    &xml[a..b + "</extLst>".len()]
}

#[test]
fn self_contained_extensions_survive_saves_in_source_order() {
    let (wb, report) = read_xlsx(&package()).unwrap();
    let w = report.warnings.iter().find(|w| w.starts_with("the workbook:")).unwrap_or_else(|| panic!("{:?}", report.warnings));
    assert!(
        w.contains("2 Excel extensions") && w.contains("{FCE2AD5D-F65C-4FA6-A056-5C36A1767C68}") && w.contains("GOOGLESHEETSCUSTOMDATAVERSION2"),
        "{w}"
    );
    let s = report.warnings.iter().find(|w| w.starts_with("sheet \"A\":")).unwrap_or_else(|| panic!("{:?}", report.warnings));
    assert!(s.contains("1 Excel extension ") && s.contains("{F7C9EE02-42E1-4005-9D12-6889AFFD525C}"), "{s}");
    // Twice: what was kept on the first save is read back and kept again.
    let first = write_xlsx(&wb).unwrap();
    let (again, report) = read_xlsx(&first).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("Excel extension")), "{:?}", report.warnings);
    for saved in [first, write_xlsx(&again).unwrap()] {
        let book = part(&saved, "xl/workbook.xml");
        assert_eq!(ext_lst(&book), format!("<extLst>{X14_WB}{X15_WB}{CALC}{XLWCV}{ARCH_ID}</extLst>"));
        assert!(book.ends_with("</extLst></workbook>"), "{book}");
        let sheet = part(&saved, "xl/worksheets/sheet1.xml");
        let list = ext_lst(&sheet);
        // The sparkline written by the sheet writer once, then the kept page layout settings once.
        assert_eq!(list.matches("{05C60535-1F16-4fd2-B633-F4F36F0B64E0}").count(), 1, "{list}");
        assert_eq!(list.matches("{64002731-A6B0-56B0-2670-7721B7C09600}").count(), 1, "{list}");
        assert!(list.ends_with(&format!("{PLV}</extLst>")), "{list}");
        assert!(!list.contains("webExtension"), "{list}");
        crate::xml::parse(sheet.as_bytes()).unwrap();
        crate::xml::parse(book.as_bytes()).unwrap();
    }
}

#[test]
fn a_sheet_with_only_kept_extensions_gets_an_ext_list() {
    let sheet = format!(r#"<?xml version="1.0"?><worksheet xmlns="{NS_MAIN}"><sheetData/><extLst>{PLV}</extLst></worksheet>"#);
    let rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{NS_R}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<?xml version="1.0"?><workbook xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let bytes = make_zip(&[
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ]);
    let (wb, report) = read_xlsx(&bytes).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("Excel extension")), "{:?}", report.warnings);
    let saved = write_xlsx(&wb).unwrap();
    assert!(part(&saved, "xl/worksheets/sheet1.xml").ends_with(&format!("<extLst>{PLV}</extLst></worksheet>")));
    // No extension on the workbook: none written.
    assert!(!part(&saved, "xl/workbook.xml").contains("extLst"));
}

#[test]
fn a_modelled_extension_with_an_unmodelled_child_is_reported_dropped() {
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="{NS_MAIN}"><sheetData/><extLst><ext uri="{{05C60535-1F16-4fd2-B633-F4F36F0B64E0}}" xmlns:x14="{NS_X14}"><x14:sparklineGroups/><x14:unknownThing/></ext></extLst></worksheet>"#
    );
    let rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{NS_R}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"<?xml version="1.0"?><workbook xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let bytes = make_zip(&[
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ]);
    let (_, report) = read_xlsx(&bytes).unwrap();
    let w = report.warnings.iter().find(|w| w.starts_with("sheet \"A\":")).unwrap_or_else(|| panic!("{:?}", report.warnings));
    assert!(w.contains("{05C60535-1F16-4fd2-B633-F4F36F0B64E0}"), "{w}");
}

#[test]
fn a_new_workbook_has_no_ext_lists() {
    let saved = write_xlsx(&gridcraft_model::Workbook::new()).unwrap();
    assert!(!part(&saved, "xl/workbook.xml").contains("extLst"));
    assert!(!part(&saved, "xl/worksheets/sheet1.xml").contains("extLst"));
}
