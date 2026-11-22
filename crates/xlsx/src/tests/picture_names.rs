//! A picture keeps the name its file gave it through a save, so macros, links and other tools that
//! refer to it by name still find it; a picture without a name gets `Picture N` (#4544).

use std::io::Read;

use super::minimal;
use crate::{read_xlsx, write_xlsx};

fn pic(id: u32, name: Option<&str>, rid: &str, row: u32) -> String {
    let name = name.map(|n| format!(r#" name="{n}""#)).unwrap_or_default();
    format!(
        r#"<xdr:oneCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:ext cx="952500" cy="952500"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="{id}"{name}/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="{rid}"/></xdr:blipFill><xdr:spPr/></xdr:pic><xdr:clientData/></xdr:oneCellAnchor>"#
    )
}

/// A workbook whose one sheet has three pictures: two named in the file, one with no name.
fn workbook_with_named_pictures() -> Vec<u8> {
    let drawing = format!(
        r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{}{}{}</xdr:wsDr>"#,
        pic(2, Some("Logo"), "rId1", 1),
        pic(5, Some("Cash &amp; Flow"), "rId2", 8),
        pic(7, None, "rId3", 15),
    );
    let image = |n: u32| {
        format!(
            r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image{n}.png"/>"#
        )
    };
    let drels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}{}{}</Relationships>"#,
        image(1),
        image(2),
        image(3)
    );
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><drawing r:id="rId1"/>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", srels), ("xl/drawings/drawing1.xml", &drawing), ("xl/drawings/_rels/drawing1.xml.rels", &drels)],
        "",
        "",
    );
    // Add the binary pictures by rebuilding the package with them.
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = vec![];
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut d = vec![];
        f.read_to_end(&mut d).unwrap();
        parts.push((f.name().to_string(), d));
    }
    for n in 1..=3u8 {
        parts.push((format!("xl/media/image{n}.png"), [b"\x89PNG\r\n\x1a\nfake".as_slice(), &[n]].concat()));
    }
    let refs: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    super::make_zip(&refs)
}

/// The `name` of each picture in a saved drawing part, in order (as written, XML-escaped).
fn picture_names(xlsx: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(xlsx.to_vec())).unwrap();
    let mut xml = String::new();
    z.by_name("xl/drawings/drawing1.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml.split("<xdr:pic>")
        .skip(1)
        .map(|p| {
            let props = p.split("<xdr:cNvPr ").nth(1).and_then(|c| c.split('>').next()).unwrap_or_default();
            props.split(" name=\"").nth(1).and_then(|n| n.split('"').next()).unwrap_or_default().to_string()
        })
        .collect()
}

/// A workbook whose one sheet has two pictures: the first is named `Picture 2` in the file
/// (the fallback name the second, unnamed picture would get from its position), the second
/// has no name.
fn workbook_with_a_picture_named_picture_2() -> Vec<u8> {
    let drawing = format!(
        r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{}{}</xdr:wsDr>"#,
        pic(2, Some("Picture 2"), "rId1", 1),
        pic(5, None, "rId2", 8),
    );
    let image = |n: u32| {
        format!(
            r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image{n}.png"/>"#
        )
    };
    let drels =
        format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}{}</Relationships>"#, image(1), image(2));
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><drawing r:id="rId1"/>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", srels), ("xl/drawings/drawing1.xml", &drawing), ("xl/drawings/_rels/drawing1.xml.rels", &drels)],
        "",
        "",
    );
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = vec![];
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut d = vec![];
        f.read_to_end(&mut d).unwrap();
        parts.push((f.name().to_string(), d));
    }
    for n in 1..=2u8 {
        parts.push((format!("xl/media/image{n}.png"), [b"\x89PNG\r\n\x1a\nfake".as_slice(), &[n]].concat()));
    }
    let refs: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    super::make_zip(&refs)
}

#[test]
fn a_fallback_name_does_not_collide_with_a_preserved_name() {
    let (wb, _) = read_xlsx(&workbook_with_a_picture_named_picture_2()).unwrap();
    assert_eq!(wb.sheet(0).unwrap().images.len(), 2);
    let saved = write_xlsx(&wb).unwrap();
    // The preserved name stays, and the unnamed picture gets a different fallback name.
    assert_eq!(picture_names(&saved), ["Picture 2", "Picture 3"]);
}

#[test]
fn a_save_keeps_each_pictures_name() {
    let (wb, _) = read_xlsx(&workbook_with_named_pictures()).unwrap();
    assert_eq!(wb.sheet(0).unwrap().images.len(), 3);
    let saved = write_xlsx(&wb).unwrap();
    assert_eq!(picture_names(&saved), ["Logo", "Cash &amp; Flow", "Picture 3"]);
    // A second open and save changes nothing.
    let (again, _) = read_xlsx(&saved).unwrap();
    assert_eq!(picture_names(&write_xlsx(&again).unwrap()), ["Logo", "Cash &amp; Flow", "Picture 3"]);
}
