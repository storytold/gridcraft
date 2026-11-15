//! A sheet's gridline colour: `sheetView defaultGridColor="0" colorId="…"`, the palette index + 7
//! (#3056). The cases are the ones measured in Excel.

use std::io::Read;
use std::sync::Arc;

use gridcraft_model::{Sheet, Workbook};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

/// The XML of a part of a written package.
fn part(bytes: &[u8], name: &str) -> String {
    let mut xml = String::new();
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap().by_name(name).unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// The `<sheetView …>` start tag of a written sheet.
fn sheet_view(xml: &str) -> &str {
    let start = xml.find("<sheetView ").unwrap();
    &xml[start..start + xml[start..].find('>').unwrap()]
}

fn open(view_attrs: &str) -> Workbook {
    let body = format!(r#"<sheetViews><sheetView {view_attrs} workbookViewId="0"/></sheetViews><sheetData/>"#);
    read_xlsx(&minimal(&body, &[], "", "")).unwrap().0
}

#[test]
fn hand_written_sheet_views_open_as_excel_opens_them() {
    for (attrs, want) in [
        (r#"tabSelected="1" defaultGridColor="0" colorId="10""#, Some(3)),
        (r#"defaultGridColor="0" colorId="12""#, Some(5)),
        (r#"defaultGridColor="false" colorId="17""#, Some(10)),
        (r#"defaultGridColor="0" colorId="8""#, Some(1)),
        (r#"defaultGridColor="0" colorId="63""#, Some(56)),
        // Without `defaultGridColor="0"` the colour is the default one, whatever `colorId` says.
        (r#"colorId="10""#, None),
        (r#"defaultGridColor="1" colorId="10""#, None),
        // No colour, or 64 (the default, the window text colour).
        (r#"defaultGridColor="0""#, None),
        (r#"defaultGridColor="0" colorId="64""#, None),
        // Excel refuses these files; here the values are ignored and the file opens.
        (r#"defaultGridColor="0" colorId="200""#, None),
        (r#"defaultGridColor="0" colorId="7""#, None),
        (r#"defaultGridColor="0" colorId="0""#, None),
        (r#"defaultGridColor="0" colorId="-3""#, None),
        (r#"defaultGridColor="0" colorId="4294967296""#, None),
        (r#"defaultGridColor="0" colorId="red""#, None),
    ] {
        let wb = open(attrs);
        assert_eq!(wb.sheet(0).unwrap().gridline_color, want, "{attrs}");
        // Saved as Excel saves it: both attributes for a colour, neither for Automatic.
        let bytes = write_xlsx(&wb).unwrap();
        let view = sheet_view(&part(&bytes, "xl/worksheets/sheet1.xml")).to_string();
        match want {
            Some(index) => assert!(view.contains(&format!(r#" defaultGridColor="0" colorId="{}""#, index + 7)), "{attrs}: {view}"),
            None => assert!(!view.contains("defaultGridColor") && !view.contains("colorId"), "{attrs}: {view}"),
        }
        let back = read_xlsx(&bytes).unwrap().0;
        assert_eq!(back.sheet(0).unwrap().gridline_color, want, "{attrs} after a save");
    }
}

#[test]
fn the_colour_stays_while_the_gridlines_are_hidden() {
    // Excel: `showGridLines="0" defaultGridColor="0" colorId="17"`; turned on again, ColorIndex 10.
    let wb = open(r#"showGridLines="0" defaultGridColor="0" colorId="17""#);
    let sheet = wb.sheet(0).unwrap();
    assert_eq!((sheet.show_gridlines, sheet.gridline_color), (false, Some(10)));
    let bytes = write_xlsx(&wb).unwrap();
    let view = sheet_view(&part(&bytes, "xl/worksheets/sheet1.xml")).to_string();
    assert!(view.contains(r#"showGridLines="0""#) && view.contains(r#"defaultGridColor="0" colorId="17""#), "{view}");
    let back = read_xlsx(&bytes).unwrap().0;
    let sheet = back.sheet(0).unwrap();
    assert_eq!((sheet.show_gridlines, sheet.gridline_color), (false, Some(10)));
}

#[test]
fn each_sheet_keeps_its_own_colour() {
    // Excel with Sheet1 red: Sheet1 `defaultGridColor="0" colorId="10"`, Sheet2 `<sheetView workbookViewId="0"/>`.
    let mut wb = Workbook::new();
    let mut first = Sheet::new("Sheet1");
    first.gridline_color = Some(3);
    wb.sheets[0] = Arc::new(first);
    wb.sheets.push(Arc::new(Sheet::new("Sheet2")));
    let bytes = write_xlsx(&wb).unwrap();
    assert_eq!(
        sheet_view(&part(&bytes, "xl/worksheets/sheet1.xml")),
        r#"<sheetView tabSelected="1" defaultGridColor="0" colorId="10" workbookViewId="0""#
    );
    assert_eq!(sheet_view(&part(&bytes, "xl/worksheets/sheet2.xml")), r#"<sheetView workbookViewId="0""#);
    let back = read_xlsx(&bytes).unwrap().0;
    assert_eq!(back.sheets.iter().map(|s| s.gridline_color).collect::<Vec<_>>(), [Some(3), None]);
}

#[test]
fn an_index_outside_the_palette_saves_as_automatic() {
    // Only a hand-edited JSON workbook holds one; the file gets no colour rather than a bad one.
    let mut wb = Workbook::new();
    let mut sheet = Sheet::new("Sheet1");
    sheet.gridline_color = Some(200);
    wb.sheets[0] = Arc::new(sheet);
    let bytes = write_xlsx(&wb).unwrap();
    let view = sheet_view(&part(&bytes, "xl/worksheets/sheet1.xml")).to_string();
    assert!(!view.contains("colorId") && !view.contains("defaultGridColor"), "{view}");
}
