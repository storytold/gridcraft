//! A shared workbook (ECMA-376 Part 1, §18.11 shared workbook revisions) keeps its change
//! history, its list of users and the users' own views (§18.2.4, §18.3.1.25) on save (#4684).
//! The package is built in code.

use gridcraft_core::{CellRef, Value};

use super::make_zip;
use super::passthrough_tests::{Unpacked, attr_of, rels, sheet_part};
use crate::{read_xlsx, write_xlsx};

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml";
const VIEW: &str = "{6A3F0C2E-1B4D-4E5F-9A7B-2C8D0E1F3A5B}";
const PRINTER: &[u8] = &[7, 0, 1, 2, 3, 250, 251, 9];

fn headers() -> String {
    format!(
        r#"<?xml version="1.0"?><headers xmlns="{NS_MAIN}" xmlns:r="{NS_R}" guid="{{0D9E8F7A-6B5C-4D3E-2F1A-0B9C8D7E6F5A}}" lastGuid="{{0D9E8F7A-6B5C-4D3E-2F1A-0B9C8D7E6F5A}}" shared="1" diskRevisions="1" revisionId="1" version="2"><header guid="{{0D9E8F7A-6B5C-4D3E-2F1A-0B9C8D7E6F5A}}" dateTime="2026-11-20T10:00:00" maxSheetId="3" userName="Ana" r:id="rId1"><sheetIdMap count="2"><sheetId val="1"/><sheetId val="2"/></sheetIdMap></header></headers>"#
    )
}

fn log() -> String {
    format!(
        r#"<?xml version="1.0"?><revisions xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><rcc rId="1" sId="1" numFmtId="200"><nc r="A3" t="n"><v>3</v></nc></rcc></revisions>"#
    )
}

fn users() -> String {
    format!(
        r#"<?xml version="1.0"?><users xmlns="{NS_MAIN}" count="1"><userInfo guid="{{1F2E3D4C-5B6A-4978-8695-A4B3C2D1E0F9}}" name="Ana" id="1" dateTime="2026-11-20T10:00:00"/></users>"#
    )
}

/// Number format 200 is used only by the revision log, differential format 1 (a red fill) only by
/// the view's colour filter.
const STYLES: &str = r#"<?xml version="1.0"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="200" formatCode="0.000&quot; kg&quot;"/></numFmts><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="1"><xf xfId="0"/></cellXfs><dxfs count="2"><dxf><font><b/></font></dxf><dxf><fill><patternFill patternType="solid"><bgColor rgb="FFFF0000"/></patternFill></fill></dxf></dxfs></styleSheet>"#;

/// Sheet "Data" has a view with a filter and its own printer settings, sheet "Other" one
/// without (nothing else on it would make the reader look for kept elements).
fn shared_package() -> Vec<u8> {
    let types = format!(
        r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="bin" ContentType="{CT}.printerSettings"/><Override PartName="/xl/styles.xml" ContentType="{CT}.styles+xml"/><Override PartName="/xl/workbook.xml" ContentType="{CT}.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="{CT}.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="{CT}.worksheet+xml"/><Override PartName="/xl/revisions/revisionHeaders.xml" ContentType="{CT}.revisionHeaders+xml"/><Override PartName="/xl/revisions/revisionLog1.xml" ContentType="{CT}.revisionLog+xml"/><Override PartName="/xl/revisions/userNames.xml" ContentType="{CT}.userNames+xml"/></Types>"#
    );
    let workbook = format!(
        r#"<?xml version="1.0"?><workbook xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><bookViews><workbookView/></bookViews><sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="Other" sheetId="2" r:id="rId2"/></sheets><definedNames><definedName name="Z_6A3F0C2E_1B4D_4E5F_9A7B_2C8D0E1F3A5B_.wvu.FilterData" localSheetId="0" hidden="1">Data!$A$1:$A$3</definedName></definedNames><calcPr calcId="191029"/><customWorkbookViews><customWorkbookView name="Ana - Personal View" guid="{VIEW}" mergeInterval="0" personalView="1" windowWidth="800" windowHeight="600" activeSheetId="1"/></customWorkbookViews></workbook>"#
    );
    let view = format!(r#"<customSheetView guid="{VIEW}" filter="1" showAutoFilter="1"><selection activeCell="A1" sqref="A1"/>"#);
    let sheet1 = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row></sheetData><customSheetViews>{view}<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup orientation="landscape" r:id="rId1"/><autoFilter ref="A1:A3"><filterColumn colId="0"><colorFilter dxfId="1" cellColor="1"/></filterColumn></autoFilter></customSheetView></customSheetViews><mergeCells count="1"><mergeCell ref="B1:C1"/></mergeCells><dataValidations count="1"><dataValidation type="whole" sqref="A1:A3"><formula1>0</formula1></dataValidation></dataValidations><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#
    );
    let sheet2 = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="{NS_MAIN}"><sheetData><row r="1"><c r="A1"><v>9</v></c></row></sheetData><sheetProtection sheet="1"/><customSheetViews>{view}</customSheetView></customSheetViews></worksheet>"#
    );
    let root_rels = rels(&[("rId1", "officeDocument", "xl/workbook.xml")]);
    let wb_rels = rels(&[
        ("rId1", "worksheet", "worksheets/sheet1.xml"),
        ("rId2", "worksheet", "worksheets/sheet2.xml"),
        ("rId3", "revisionHeaders", "revisions/revisionHeaders.xml"),
        ("rId4", "usernames", "revisions/userNames.xml"),
        ("rId5", "styles", "styles.xml"),
    ]);
    let sheet1_rels = rels(&[("rId1", "printerSettings", "../printerSettings/printerSettings1.bin")]);
    let headers_rels = rels(&[("rId1", "revisionLog", "revisionLog1.xml")]);
    let (headers, log, users) = (headers(), log(), users());
    make_zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes()),
        ("xl/styles.xml", STYLES.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet1_rels.as_bytes()),
        ("xl/printerSettings/printerSettings1.bin", PRINTER),
        ("xl/revisions/revisionHeaders.xml", headers.as_bytes()),
        ("xl/revisions/_rels/revisionHeaders.xml.rels", headers_rels.as_bytes()),
        ("xl/revisions/revisionLog1.xml", log.as_bytes()),
        ("xl/revisions/userNames.xml", users.as_bytes()),
    ])
}

fn pos(x: &str, tag: &str) -> usize {
    x.find(tag).unwrap_or_else(|| panic!("no {tag} in {x}"))
}

#[test]
fn a_shared_workbook_keeps_its_change_history_and_users_on_save() {
    let (mut wb, rep) = read_xlsx(&shared_package()).unwrap();
    assert!(rep.warnings.iter().any(|w| w.contains("shared workbook") && w.contains("not recorded")), "{:?}", rep.warnings);
    wb.sheet_mut(0).unwrap().set_value(CellRef::new(2, 0), Value::Number(3.0));
    let out = Unpacked::new(&write_xlsx(&wb).unwrap());
    out.assert_valid();
    let wb_rels = out.rels("xl/workbook.xml");
    let target = |kind: &str| wb_rels.iter().find(|r| r.1.ends_with(&format!("/{kind}"))).map(|r| r.2.clone()).unwrap_or_else(|| panic!("no {kind}"));
    let (headers, users) = (target("revisionHeaders"), target("usernames"));
    assert_eq!(out.text(&headers), self::headers());
    assert_eq!(out.text(&users), self::users());
    let logs = out.rels(&headers);
    let log = logs.iter().find(|r| r.1.ends_with("/revisionLog")).map(|r| out.text(&r.2)).unwrap_or_else(|| panic!("{logs:?}"));
    assert!(log.contains("<nc r=\"A3\" t=\"n\"><v>3</v></nc>"), "{log}");
    // The number format only the history uses still names the same code in the written styles.
    let id = attr_of(&log, "rcc", "numFmtId");
    let styles = out.text("xl/styles.xml");
    let at = styles.find(&format!("numFmtId=\"{id}\" formatCode=")).unwrap_or_else(|| panic!("no numFmt {id} in {styles}"));
    assert!(styles[at..].starts_with(&format!("numFmtId=\"{id}\" formatCode=\"0.000&quot; kg&quot;\"")), "{styles}");
}

#[test]
fn custom_views_and_their_printer_settings_survive_a_save() {
    let (mut wb, rep) = read_xlsx(&shared_package()).unwrap();
    assert!(!rep.warnings.iter().any(|w| w.contains("dropped")), "{:?}", rep.warnings);
    wb.sheet_mut(0).unwrap().set_value(CellRef::new(2, 0), Value::Number(3.0));
    let out = Unpacked::new(&write_xlsx(&wb).unwrap());
    out.assert_valid();
    // The workbook's views come after <calcPr> (CT_Workbook).
    let wx = out.text("xl/workbook.xml");
    assert!(pos(&wx, "<calcPr") < pos(&wx, "<customWorkbookViews"), "{wx}");
    assert!(wx.contains(VIEW), "{wx}");
    // Each sheet's views come after <sheetData> and before <mergeCells> (CT_Worksheet).
    let data = sheet_part(&out, "Data");
    let sx = out.text(&data);
    assert!(pos(&sx, "</sheetData>") < pos(&sx, "<customSheetViews"), "{sx}");
    assert!(pos(&sx, "</customSheetViews>") < pos(&sx, "<mergeCells"), "{sx}");
    let views = &sx[pos(&sx, "<customSheetViews")..pos(&sx, "</customSheetViews>")];
    assert!(views.contains(VIEW), "{views}");
    // The colour filter's format is the red fill in the written styles.
    let dxf: usize = attr_of(views, "colorFilter", "dxfId").parse().unwrap();
    let styles = out.text("xl/styles.xml");
    let dxfs = &styles[pos(&styles, "<dxfs")..pos(&styles, "</dxfs>")];
    let red = dxfs.split("<dxf>").nth(dxf + 1).unwrap_or_else(|| panic!("no dxf {dxf} in {dxfs}"));
    assert!(red.contains("FFFF0000"), "dxf {dxf}: {red}");
    assert!(pos(&sx, "</customSheetViews>") < pos(&sx, "<dataValidations"), "{sx}");
    // The view's printer settings: its <pageSetup> names them through the sheet's new rels.
    let (ty, part) = out.target(&data, &attr_of(views, "pageSetup", "r:id"));
    assert!(ty.ends_with("/printerSettings"));
    assert_eq!(out.parts.get(&part).map(Vec::as_slice), Some(PRINTER));
    let ox = out.text(&sheet_part(&out, "Other"));
    assert!(pos(&ox, "<sheetProtection") < pos(&ox, "<customSheetViews"), "{ox}");
    assert!(ox.contains(VIEW), "{ox}");
    // Saved again, the views are still there.
    let (again, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let twice = Unpacked::new(&write_xlsx(&again).unwrap());
    assert!(twice.text(&sheet_part(&twice, "Data")).contains(VIEW));
    assert!(twice.text("xl/workbook.xml").contains("<customWorkbookViews"));
}
