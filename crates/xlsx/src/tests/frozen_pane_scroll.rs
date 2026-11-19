//! Where a sheet with frozen panes opens scrolled to. A pane frozen one way only scrolls with
//! the sheet the other way: Excel 365 takes that coordinate from the sheet view's own
//! `topLeftCell`, and the frozen way's from the pane's.

use std::io::{Read, Write};
use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_model::{Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

/// A one-sheet workbook whose sheet view is `view` (the `sheetView` element, pane included).
fn workbook_with_view(view: &str) -> Vec<u8> {
    let parts = [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_string(),
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string(),
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_string(),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_string(),
        ),
        (
            "xl/worksheets/sheet1.xml",
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews>{view}</sheetViews><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#
            ),
        ),
    ];
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in parts {
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(data.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn opened_at(view: &str) -> (Option<(u32, u32)>, CellRef) {
    let (wb, _) = read_xlsx(&workbook_with_view(view)).unwrap();
    let sh = &wb.sheets[0];
    (sh.freeze, sh.view_top_left)
}

/// The three cases measured in Excel (`ActiveWindow.Panes(n).ScrollRow/ScrollColumn`).
#[test]
fn a_pane_frozen_one_way_scrolls_the_other_way_with_the_sheet_view() {
    // Frozen rows: column Q from the sheet view, row 20 from the pane.
    let rows = r#"<sheetView tabSelected="1" topLeftCell="Q1" workbookViewId="0"><pane ySplit="1" topLeftCell="A20" activePane="bottomLeft" state="frozen"/></sheetView>"#;
    assert_eq!(opened_at(rows), (Some((1, 0)), CellRef::new(19, 16)));
    // Frozen columns: row 30 from the sheet view, column F from the pane.
    let cols = r#"<sheetView tabSelected="1" topLeftCell="A30" workbookViewId="0"><pane xSplit="1" topLeftCell="F1" activePane="topRight" state="frozen"/></sheetView>"#;
    assert_eq!(opened_at(cols), (Some((0, 1)), CellRef::new(29, 5)));
    // Both: the pane's cell.
    let both = r#"<sheetView tabSelected="1" topLeftCell="Q30" workbookViewId="0"><pane xSplit="1" ySplit="1" topLeftCell="F20" activePane="bottomRight" state="frozen"/></sheetView>"#;
    assert_eq!(opened_at(both), (Some((1, 1)), CellRef::new(19, 5)));
    // No sheet-view cell: the free way starts at the sheet's first column.
    let plain =
        r#"<sheetView tabSelected="1" workbookViewId="0"><pane ySplit="1" topLeftCell="C20" activePane="bottomLeft" state="frozen"/></sheetView>"#;
    assert_eq!(opened_at(plain), (Some((1, 0)), CellRef::new(19, 0)));
}

/// What GridCraft saves opens in Excel where it was: the free way goes in the sheet view's
/// `topLeftCell`, and it reads back the same.
#[test]
fn a_scrolled_frozen_sheet_round_trips() {
    for (freeze, top_left, sheet_view) in [
        ((1, 0), CellRef::new(19, 16), Some("Q1")),
        ((0, 1), CellRef::new(29, 5), Some("A30")),
        ((1, 1), CellRef::new(19, 5), None),
        ((1, 0), CellRef::new(19, 0), None),
    ] {
        let mut sh = Sheet::new("S");
        sh.freeze = Some(freeze);
        sh.view_top_left = top_left;
        let wb = Workbook { sheets: vec![Arc::new(sh)], ..Workbook::default() };
        let bytes = write_xlsx(&wb).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut xml = String::new();
        z.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut xml).unwrap();
        let view = xml.split("<sheetView ").nth(1).and_then(|v| v.split('>').next()).unwrap().to_string();
        match sheet_view {
            Some(cell) => assert!(view.contains(&format!("topLeftCell=\"{cell}\"")), "{freeze:?}: {view}"),
            None => assert!(!view.contains("topLeftCell"), "{freeze:?}: {view}"),
        }
        let (back, _) = read_xlsx(&bytes).unwrap();
        assert_eq!((back.sheets[0].freeze, back.sheets[0].view_top_left), (Some(freeze), top_left));
    }
}
