//! Original-package fixtures: a surviving drawing must not mask a different object's loss.

#![allow(clippy::unwrap_used)]

use std::io::Write as _;
use std::sync::Arc;

use gridcraft_engine::Session;
use gridcraft_engine::model::{Anchor, Image};

use super::*;

fn unpack(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..archive.len())
        .map(|i| {
            let mut part = archive.by_index(i).unwrap();
            let mut bytes = Vec::new();
            part.read_to_end(&mut bytes).unwrap();
            (part.name().to_string(), bytes)
        })
        .collect()
}

fn pack(parts: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        archive.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(&bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

fn pictures(per_sheet: &[usize]) -> Vec<u8> {
    let mut wb = Workbook::new();
    wb.sheets.clear();
    let mut id = 1;
    for (index, count) in per_sheet.iter().enumerate() {
        let mut sheet = Sheet::new(format!("Page {}", index + 1));
        for picture in 0..*count {
            sheet.images.push(Image {
                id,
                anchor: Anchor { cell: CellRef::new(picture as u32 * 3, 1), width: 24.0, height: 24.0, ..Default::default() },
                // Package-level fixture: image bytes are opaque to the XLSX reader/checker.
                data: b"\x89PNG\r\n\x1a\n0000IHDRfake".to_vec(),
                mime: "image/png".into(),
                alt: format!("Picture {id}"),
            });
            id += 1;
        }
        wb.sheets.push(Arc::new(sheet));
    }
    gridcraft_engine::io::save_bytes(&wb, "pictures.xlsx").unwrap()
}

fn group_object(bytes: &[u8], drawing: &str, tag: &str, index: usize) -> Vec<u8> {
    let mut parts = unpack(bytes);
    let xml = String::from_utf8(parts[drawing].clone()).unwrap();
    let start_tag = format!("<xdr:{tag}");
    let end_tag = format!("</xdr:{tag}>");
    let start = xml.match_indices(&start_tag).nth(index).unwrap().0;
    let end = start + xml[start..].find(&end_tag).unwrap() + end_tag.len();
    let object = &xml[start..end];
    let group = format!(
        r#"<xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="900" name="Grouped fixture"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr><xdr:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="228600" cy="228600"/><a:chOff x="0" y="0"/><a:chExt cx="228600" cy="228600"/></a:xfrm></xdr:grpSpPr>{object}</xdr:grpSp>"#
    );
    let mut replaced = xml;
    replaced.replace_range(start..end, &group);
    parts.insert(drawing.into(), replaced.into_bytes());
    pack(parts)
}

fn drawing_diffs(report: &FileReport) -> usize {
    report.round_trip.categories.iter().find(|c| c.name == "drawings").map_or(0, |c| c.diffs)
}

fn assert_preserved_or_reported(bytes: &[u8], expected: usize, imported: usize) {
    assert!(imported == expected || imported + 1 == expected, "unexpected fixture object count: {imported} of {expected}");
    let report = check_bytes("drawings.xlsx", bytes, &Options::default());
    assert!(report.error.is_none(), "{:?}", report.error);
    assert!(report.round_trip.error.is_none(), "{:?}", report.round_trip.error);
    if imported == expected {
        assert_eq!(drawing_diffs(&report), 0, "preserved objects were reported lost: {report:?}");
        assert!(report.is_clean(), "{report:?}");
    } else {
        assert!(drawing_diffs(&report) > 0, "drawing loss was missed: {report:?}");
        assert!(!report.is_clean(), "drawing loss must affect the check result");
    }
}

#[test]
fn grouped_picture_is_preserved_or_its_loss_is_reported() {
    let source = group_object(&pictures(&[2]), "xl/drawings/drawing1.xml", "pic", 1);
    let (imported, _) = gridcraft_engine::io::open_bytes("source.xlsx", &source).unwrap();
    assert_preserved_or_reported(&source, 2, imported.sheet(0).unwrap().images.len());
}

fn charts(count: usize) -> Vec<u8> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range":"A1", "values":[["Label","Value"],["A",10],["B",20]]})).unwrap();
    for _ in 0..count {
        session.execute("insert.chart", json!({"range":"A1:B3", "type":"column"})).unwrap();
    }
    gridcraft_engine::io::save_bytes(&session.doc().unwrap().wb, "charts.xlsx").unwrap()
}

#[test]
fn grouped_chart_is_preserved_or_its_loss_is_reported() {
    let source = group_object(&charts(2), "xl/drawings/drawing1.xml", "graphicFrame", 1);
    let (imported, _) = gridcraft_engine::io::open_bytes("source.xlsx", &source).unwrap();
    assert_preserved_or_reported(&source, 2, imported.sheet(0).unwrap().charts.len());
}

#[test]
fn picture_on_another_sheet_does_not_mask_a_lost_drawing() {
    let source = group_object(&pictures(&[1, 1]), "xl/drawings/drawing2.xml", "pic", 0);
    let (imported, _) = gridcraft_engine::io::open_bytes("source.xlsx", &source).unwrap();
    assert_eq!(imported.sheet(0).unwrap().images.len(), 1);
    assert_preserved_or_reported(&source, 1, imported.sheet(1).unwrap().images.len());
}

#[test]
fn surviving_chart_does_not_mask_partial_chart_loss() {
    let category = drawings::compare(&charts(2), &charts(1), 20);
    assert_eq!(category.diffs, 1);
    assert!(category.examples[0].contains("charts 2 -> 1"), "{category:?}");
}

#[test]
fn surviving_sheet_does_not_mask_another_sheets_picture_loss() {
    let category = drawings::compare(&pictures(&[1, 1]), &pictures(&[1, 0]), 20);
    assert_eq!(category.diffs, 1);
    assert!(category.examples[0].contains("Page 2") && category.examples[0].contains("pictures 1 -> 0"), "{category:?}");
}

#[test]
fn repeated_pictures_sharing_one_media_relationship_are_clean() {
    let mut parts = unpack(&pictures(&[2]));
    let drawing = "xl/drawings/drawing1.xml";
    let xml = String::from_utf8(parts[drawing].clone()).unwrap();
    let ids: Vec<_> = xml.split("r:embed=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    let shared = xml.replace(&format!("r:embed=\"{}\"", ids[1]), &format!("r:embed=\"{}\"", ids[0]));
    parts.insert(drawing.into(), shared.into_bytes());
    let source = pack(parts);
    let (imported, _) = gridcraft_engine::io::open_bytes("source.xlsx", &source).unwrap();
    assert_eq!(imported.sheet(0).unwrap().images.len(), 2);
    let report = check_bytes("shared.xlsx", &source, &Options::default());
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(drawing_diffs(&report), 0);
}

#[test]
fn renamed_parts_and_relationship_ids_do_not_report_drawing_loss() {
    let parts = unpack(&pictures(&[2, 1]))
        .into_iter()
        .map(|(name, bytes)| {
            let name = name.replace("drawing1", "renamedArtwork");
            let bytes = if name.ends_with(".xml") || name.ends_with(".rels") {
                String::from_utf8(bytes).unwrap().replace("drawing1", "renamedArtwork").replace("rId", "renamedRel").into_bytes()
            } else {
                bytes
            };
            (name, bytes)
        })
        .collect();
    let source = pack(parts);
    let report = check_bytes("renamed.xlsx", &source, &Options::default());
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(drawing_diffs(&report), 0);
}

#[test]
fn native_chartex_choice_and_shape_fallback_count_as_one_chart() {
    for kind in ["waterfall", "funnel"] {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("range.setValues", json!({"range":"A1", "values":[["Label","Value"],["A",10],["B",20]]})).unwrap();
        session.execute("insert.chart", json!({"range":"A1:B3", "type":kind})).unwrap();
        let source = gridcraft_engine::io::save_bytes(&session.doc().unwrap().wb, "chartex.xlsx").unwrap();
        let drawing = String::from_utf8(unpack(&source)["xl/drawings/drawing1.xml"].clone()).unwrap();
        assert!(drawing.contains("mc:Choice") && drawing.contains("mc:Fallback"));
        let report = check_bytes("chartex.xlsx", &source, &Options::default());
        assert!(report.is_clean(), "{kind}: {report:?}");
        assert_eq!(drawing_diffs(&report), 0);
    }
}

#[test]
fn nested_groups_count_leaf_pictures_once() {
    let source = group_object(&pictures(&[2]), "xl/drawings/drawing1.xml", "pic", 1);
    let source = group_object(&source, "xl/drawings/drawing1.xml", "pic", 1);
    let compared = drawings::compare(&source, &pictures(&[1]), 20);
    assert_eq!(compared.diffs, 1);
    assert!(compared.examples[0].contains("pictures 2 -> 1"), "{compared:?}");
}

fn change_drawing(bytes: &[u8], change: impl FnOnce(String) -> String) -> Vec<u8> {
    let mut parts = unpack(bytes);
    let path = "xl/drawings/drawing1.xml";
    let changed = change(String::from_utf8(parts[path].clone()).unwrap());
    parts.insert(path.into(), changed.into_bytes());
    pack(parts)
}

#[test]
fn malformed_or_excessively_nested_drawing_reports_incomplete_inspection() {
    let good = pictures(&[1]);
    let malformed = change_drawing(&good, |xml| xml.replace("</xdr:wsDr>", ""));
    let deep = change_drawing(&good, |xml| {
        assert!(xml.contains("<xdr:pic>"), "the depth fixture must wrap a picture");
        xml.replace("<xdr:pic>", &format!("{}<xdr:pic>", "<xdr:grpSp>".repeat(200)))
            .replace("</xdr:pic>", &format!("</xdr:pic>{}", "</xdr:grpSp>".repeat(200)))
    });
    for source in [malformed, deep] {
        let compared = drawings::compare(&source, &good, 20);
        assert!(compared.diffs > 0, "inspection failure cannot be a clean result");
        assert!(compared.examples.iter().any(|e| e.contains("inspection incomplete")), "{compared:?}");
    }
}

#[test]
fn foreign_namespace_picture_is_not_treated_as_a_standard_picture() {
    let good = pictures(&[1]);
    let source = change_drawing(&good, |xml| {
        xml.replace("<xdr:wsDr ", "<xdr:wsDr xmlns:foreign=\"urn:test:foreign-drawing\" ").replace("xdr:pic>", "foreign:pic>")
    });
    let compared = drawings::compare(&source, &good, 20);
    assert!(compared.diffs > 0);
    assert!(compared.examples.iter().any(|e| e.contains("inspection incomplete")), "{compared:?}");
}

#[test]
fn capped_examples_retain_total_drawing_difference_count() {
    let category = drawings::compare(&pictures(&[2, 2, 2]), &pictures(&[1, 1, 1]), 1);
    assert_eq!(category.diffs, 3);
    assert_eq!(category.examples.len(), 1);
}

#[test]
fn ordinary_large_worksheet_does_not_exhaust_drawing_inspection_budget() {
    let mut parts = unpack(&pictures(&[1]));
    let path = "xl/worksheets/sheet1.xml";
    let mut xml = String::from_utf8(parts[path].clone()).unwrap();
    let start = xml.find("<sheetData>").unwrap();
    let end = xml[start..].find("</sheetData>").unwrap() + start + "</sheetData>".len();
    let mut cells = String::from("<sheetData>");
    for row in 1..=100_001 {
        cells.push_str(&format!("<row r=\"{row}\"><c r=\"A{row}\"><v>{row}</v></c></row>"));
    }
    cells.push_str("</sheetData>");
    xml.replace_range(start..end, &cells);
    parts.insert(path.into(), xml.into_bytes());
    let source = pack(parts);
    // The drawing did not change. Cell volume is unrelated to the drawing node budget.
    let compared = drawings::compare(&source, &source, 20);
    assert_eq!(compared.diffs, 0, "{compared:?}");
}

#[test]
fn csv_starting_with_zip_signature_letters_is_not_an_xlsx_inspection_error() {
    let compared = drawings::compare(b"PK,value\nitem,42\n", &pictures(&[0]), 20);
    assert_eq!(compared.diffs, 0, "{compared:?}");
}

#[test]
fn chartsheet_does_not_prevent_comparing_ordinary_worksheets() {
    let mut parts = unpack(&pictures(&[1, 0]));
    let rels = "xl/_rels/workbook.xml.rels";
    let xml = String::from_utf8(parts[rels].clone()).unwrap();
    let sheet2 = xml.split("<Relationship ").find(|r| r.contains("worksheets/sheet2.xml")).unwrap();
    let original = format!("<Relationship {}", sheet2.split("/>").next().unwrap());
    let changed = original.replace("relationships/worksheet", "relationships/chartsheet");
    parts.insert(rels.into(), xml.replace(&original, &changed).into_bytes());
    let types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    let sheet_type = types.split("<Override ").find(|r| r.contains("/xl/worksheets/sheet2.xml")).unwrap();
    let original = format!("<Override {}", sheet_type.split("/>").next().unwrap());
    let changed = original.replace("spreadsheetml.worksheet+xml", "spreadsheetml.chartsheet+xml");
    parts.insert("[Content_Types].xml".into(), types.replace(&original, &changed).into_bytes());
    parts.insert("xl/worksheets/sheet2.xml".into(), br#"<chartsheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"/></sheetViews></chartsheet>"#.to_vec());
    let source = pack(parts);
    let compared = drawings::compare(&source, &source, 20);
    assert_eq!(compared.diffs, 0, "{compared:?}");
}
