//! Contributor-original drawing packages, without vendor assets or network access.
use std::io::{Cursor, Read};

use super::{make_zip, minimal};
use crate::{read_xlsx, write_xlsx};

/// Build an original drawing package with linked and unlinked pictures and shapes.
fn fixture() -> Vec<u8> {
    let mut objects = String::new();
    for (id, picture, linked) in [(2, true, true), (3, false, true), (4, true, false), (5, false, false)] {
        let click = if !linked {
            String::new()
        } else if picture {
            r#"<a:hlinkClick r:id="url" tooltip="Visit &amp; inspect"/>"#.into()
        } else {
            r#"<a:hlinkClick r:id="jump" tooltip="Jump &amp; back" action="custom:jump"/>"#.into()
        };
        let props = format!(r#"<xdr:cNvPr id="{id}" name="Original {id}">{click}</xdr:cNvPr>"#);
        let object = if picture {
            format!(
                r#"<xdr:pic><xdr:nvPicPr>{props}<xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="image"/></xdr:blipFill><xdr:spPr/></xdr:pic>"#
            )
        } else {
            format!(r#"<xdr:sp><xdr:nvSpPr>{props}<xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:prstGeom prst="rect"/></xdr:spPr></xdr:sp>"#)
        };
        objects.push_str(&format!(r#"<xdr:oneCellAnchor><xdr:from><xdr:col>{id}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="952500" cy="476250"/>{object}<xdr:clientData/></xdr:oneCellAnchor>"#));
    }
    let drawing = format!(
        r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{objects}</xdr:wsDr>"#
    );
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="image" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/original.png"/><Relationship Id="url" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.invalid/?a=1&amp;b=&quot;x&quot;" TargetMode="External"/><Relationship Id="jump" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="../worksheets/sheet1.xml#B2"/></Relationships>"#;
    let sheet_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="drawing" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/original.xml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><drawing r:id="drawing"/>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels), ("xl/drawings/original.xml", &drawing), ("xl/drawings/_rels/original.xml.rels", rels)],
        "",
        "",
    );
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        parts.push((f.name().to_string(), data));
    }
    // The reader preserves these original bytes; the test does not decode or render the picture.
    parts.push(("xl/media/original.png".into(), b"\x89PNG\r\n\x1a\noriginal-test-payload".to_vec()));
    make_zip(&parts.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>())
}
/// Parse one asserted XML part from a generated package.
fn xml(bytes: &[u8], part: &str) -> crate::xml::El {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut data = Vec::new();
    z.by_name(part).unwrap().read_to_end(&mut data).unwrap();
    crate::xml::parse(&data).unwrap()
}
/// Both click relationships and their metadata survive two saves while unlinked objects stay unlinked.
#[test]
fn picture_and_shape_click_links_survive_two_saves() {
    let mut bytes = fixture();
    for _ in 0..2 {
        let (wb, _) = read_xlsx(&bytes).unwrap();
        assert_eq!(wb.sheet(0).unwrap().images.len(), 2);
        assert_eq!(wb.sheet(0).unwrap().shapes.len(), 2);
        bytes = write_xlsx(&wb).unwrap();
        let drawing = xml(&bytes, "xl/drawings/drawing1.xml");
        let rels = xml(&bytes, "xl/drawings/_rels/drawing1.xml.rels");
        let links: Vec<_> = drawing
            .children
            .iter()
            .flat_map(|anchor| anchor.children.iter())
            .filter_map(|obj| obj.child("nvPicPr").or_else(|| obj.child("nvSpPr")))
            .filter_map(|nv| nv.child("cNvPr"))
            .filter_map(|nv| nv.child("hlinkClick"))
            .collect();
        assert_eq!(links.len(), 2, "both links must survive while the no-link controls remain unlinked");
        for click in links {
            let rel = rels.kids("Relationship").find(|r| r.attr("Id") == click.attr("id")).unwrap();
            assert!(rel.attr("Type").unwrap().ends_with("/hyperlink"));
            if click.attr("tooltip") == Some("Visit & inspect") {
                assert_eq!(rel.attr("Target"), Some("https://example.invalid/?a=1&b=\"x\""));
                assert_eq!(rel.attr("TargetMode"), Some("External"));
                assert_eq!(click.attr("action"), None);
            } else {
                assert_eq!(click.attr("tooltip"), Some("Jump & back"));
                assert_eq!(click.attr("action"), Some("custom:jump"));
                assert_eq!(rel.attr("Target"), Some("../worksheets/sheet1.xml#B2"));
                assert_eq!(rel.attr("TargetMode"), None);
            }
        }
    }
}

/// Rewrite one synthetic package part without changing the other entries.
fn replace_part(bytes: Vec<u8>, part: &str, from: &str, to: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut parts = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        if name == part {
            data = String::from_utf8(data).unwrap().replace(from, to).into_bytes();
        }
        parts.push((name, data));
    }
    make_zip(&parts.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>())
}

/// Missing or mismatched relationships cannot manufacture click links.
#[test]
fn missing_and_wrong_kind_relationships_do_not_create_click_links() {
    let missing = replace_part(fixture(), "xl/drawings/original.xml", "r:id=\"url\"", "r:id=\"missing\"");
    let (wb, _) = read_xlsx(&missing).unwrap();
    assert!(wb.sheet(0).unwrap().images.iter().all(|i| i.hyperlink.is_none()));
    assert!(wb.sheet(0).unwrap().shapes[0].hyperlink.is_some());
    let wrong = replace_part(fixture(), "xl/drawings/_rels/original.xml.rels", "/relationships/hyperlink", "/relationships/image");
    let (wb, _) = read_xlsx(&wrong).unwrap();
    let sheet = wb.sheet(0).unwrap();
    assert!(sheet.images.iter().all(|i| i.hyperlink.is_none()));
    assert!(sheet.shapes.iter().all(|s| s.hyperlink.is_none()));
    let saved = write_xlsx(&wb).unwrap();
    let rels = xml(&saved, "xl/drawings/_rels/drawing1.xml.rels");
    assert!(rels.kids("Relationship").all(|r| !r.attr("Type").unwrap().ends_with("/hyperlink")));
}

/// JSON preserves click metadata and older object records still deserialize without links.
#[test]
fn drawing_links_survive_json_and_old_objects_default_to_no_link() {
    let (wb, _) = read_xlsx(&fixture()).unwrap();
    let sheet = wb.sheet(0).unwrap();
    let image = &sheet.images[0];
    let shape = &sheet.shapes[0];
    let mut image_json = serde_json::to_value(image).unwrap();
    let mut shape_json = serde_json::to_value(shape).unwrap();
    assert_eq!(serde_json::from_value::<gridcraft_model::Image>(image_json.clone()).unwrap().hyperlink, image.hyperlink);
    assert_eq!(serde_json::from_value::<gridcraft_model::Shape>(shape_json.clone()).unwrap().hyperlink, shape.hyperlink);
    image_json.as_object_mut().unwrap().remove("hyperlink");
    shape_json.as_object_mut().unwrap().remove("hyperlink");
    assert!(serde_json::from_value::<gridcraft_model::Image>(image_json).unwrap().hyperlink.is_none());
    assert!(serde_json::from_value::<gridcraft_model::Shape>(shape_json).unwrap().hyperlink.is_none());
    assert!(serde_json::to_value(&sheet.images[1]).unwrap().get("hyperlink").is_none());
    assert!(serde_json::to_value(&sheet.shapes[1]).unwrap().get("hyperlink").is_none());
}

/// Equation and ink shape serialization retains whole-object click metadata.
#[test]
fn equation_and_ink_shapes_keep_their_click_metadata() {
    use gridcraft_model::{Color, ShapeKind};
    for kind in [ShapeKind::Equation, ShapeKind::Ink] {
        let (mut wb, _) = read_xlsx(&fixture()).unwrap();
        let shape = &mut wb.sheet_mut(0).unwrap().shapes[0];
        shape.kind = kind;
        shape.text = if kind == ShapeKind::Equation { "x^2" } else { "0,0;50,40;90,10" }.into();
        shape.fill = Color::Rgb(3 << 16);
        shape.line = Color::Rgb(0xA12BCD);
        let expected = shape.hyperlink.clone();
        for _ in 0..2 {
            let bytes = write_xlsx(&wb).unwrap();
            wb = read_xlsx(&bytes).unwrap().0;
            let shape = &wb.sheet(0).unwrap().shapes[0];
            assert_eq!(shape.kind, kind);
            assert_eq!(shape.hyperlink, expected);
        }
    }
}

/// Internal links follow imported worksheet identities after part relocation and sheet reordering.
#[test]
fn relocated_part_links_map_to_the_written_worksheet_after_reordering() {
    let mut z = zip::ZipArchive::new(Cursor::new(fixture())).unwrap();
    let mut parts = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        let written_name = match name.as_str() {
            "xl/worksheets/sheet1.xml" => "xl/worksheets/unusual.xml",
            "xl/worksheets/_rels/sheet1.xml.rels" => "xl/worksheets/_rels/unusual.xml.rels",
            "xl/drawings/original.xml" => "xl/nested/drawings/original.xml",
            "xl/drawings/_rels/original.xml.rels" => "xl/nested/drawings/_rels/original.xml.rels",
            _ => &name,
        };
        if name.ends_with(".rels") {
            let mut text = String::from_utf8(data).unwrap();
            if name == "xl/_rels/workbook.xml.rels" {
                text = text.replace("worksheets/sheet1.xml", "worksheets/unusual.xml");
            } else if name == "xl/worksheets/_rels/sheet1.xml.rels" {
                text = text.replace("../drawings/original.xml", "../nested/drawings/original.xml");
            } else if name == "xl/drawings/_rels/original.xml.rels" {
                text = text.replace("../media/", "../../media/").replace("../worksheets/sheet1.xml", "../../worksheets/unusual.xml");
            }
            data = text.into_bytes();
        }
        parts.push((written_name.to_string(), data));
    }
    let bytes = make_zip(&parts.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>());
    let (mut wb, report) = read_xlsx(&bytes).unwrap();
    assert_eq!(report.warnings, vec!["no styles part".to_string()]);
    assert_eq!(wb.sheet(0).unwrap().shapes[0].hyperlink.as_ref().unwrap().internal_sheet.as_deref(), Some("Data"));
    wb.sheets.insert(0, std::sync::Arc::new(gridcraft_model::Sheet::new("Other")));
    // JSON must retain the logical destination without depending on skipped runtime ids.
    wb = serde_json::from_value(serde_json::to_value(&wb).unwrap()).unwrap();
    for _ in 0..2 {
        let bytes = write_xlsx(&wb).unwrap();
        let rels = xml(&bytes, "xl/drawings/_rels/drawing1.xml.rels");
        assert!(rels.kids("Relationship").any(|r| r.attr("Target") == Some("../worksheets/sheet2.xml#B2")));
        xml(&bytes, "xl/worksheets/sheet2.xml");
        wb = read_xlsx(&bytes).unwrap().0;
        assert_eq!(wb.sheet(1).unwrap().shapes[0].hyperlink.as_ref().unwrap().internal_sheet.as_deref(), Some("Data"));
    }
}

/// External and fragment destinations remain intact while unsupported package destinations warn.
#[test]
fn external_sheet_locations_are_unchanged_and_other_package_parts_warn() {
    let bytes = replace_part(
        fixture(),
        "xl/drawings/_rels/original.xml.rels",
        "Target=\"../worksheets/sheet1.xml#B2\"",
        "Target=\"#'Data'!B2\" TargetMode=\"External\"",
    );
    let (mut wb, _) = read_xlsx(&bytes).unwrap();
    for _ in 0..2 {
        let link = wb.sheet(0).unwrap().shapes[0].hyperlink.as_ref().unwrap();
        assert_eq!(link.target, "#'Data'!B2");
        assert!(link.external);
        assert!(link.internal_sheet.is_none());
        wb = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap().0;
    }
    let bytes = replace_part(fixture(), "xl/drawings/_rels/original.xml.rels", "../worksheets/sheet1.xml#B2", "../media/original.png#B2");
    let (wb, report) = read_xlsx(&bytes).unwrap();
    assert!(wb.sheet(0).unwrap().shapes[0].hyperlink.is_none());
    assert!(report.warnings.iter().any(|w| w.contains("drawing click link to an unsupported package part")));
}

/// Connector shapes read their click metadata from connector nonvisual properties.
#[test]
fn connector_shape_click_links_use_their_nonvisual_properties() {
    let bytes = replace_part(fixture(), "xl/drawings/original.xml", "xdr:nvSpPr", "xdr:nvCxnSpPr");
    let bytes = replace_part(bytes, "xl/drawings/original.xml", "xdr:cNvSpPr", "xdr:cNvCxnSpPr");
    let bytes = replace_part(bytes, "xl/drawings/original.xml", "xdr:sp>", "xdr:cxnSp>");
    let (wb, _) = read_xlsx(&bytes).unwrap();
    assert!(wb.sheet(0).unwrap().shapes[0].hyperlink.is_some());
    let wb = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap().0;
    assert_eq!(wb.sheet(0).unwrap().shapes[0].hyperlink.as_ref().unwrap().tooltip.as_deref(), Some("Jump & back"));
}

/// Skipped worksheets and duplicate relationship IDs do not misidentify the surviving destination.
#[test]
fn internal_link_names_match_skipped_sheets_and_duplicate_relationship_ids() {
    let bytes = replace_part(
        fixture(),
        "xl/workbook.xml",
        "<sheets>",
        r#"<sheets><sheet name="Data" sheetId="2" r:id="missing"/><sheet name="Data" sheetId="3" r:id="chart"/><sheet name="Data" sheetId="4" r:id="duplicate"/>"#,
    );
    let bytes = replace_part(
        bytes,
        "xl/_rels/workbook.xml.rels",
        "</Relationships>",
        r#"<Relationship Id="chart" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet" Target="chartsheets/absent.xml"/><Relationship Id="duplicate" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet" Target="chartsheets/absent2.xml"/><Relationship Id="duplicate" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/absent.xml"/></Relationships>"#,
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(wb.sheets.len(), 1);
    assert_eq!(wb.sheet(0).unwrap().name, "Data");
    assert_eq!(wb.sheet(0).unwrap().shapes[0].hyperlink.as_ref().unwrap().internal_sheet.as_deref(), Some("Data"));
    let reopened = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap().0;
    assert_eq!(reopened.sheet(0).unwrap().shapes[0].hyperlink.as_ref().unwrap().internal_sheet.as_deref(), Some("Data"));
}
