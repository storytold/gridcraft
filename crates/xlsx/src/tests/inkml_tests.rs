//! Native Office ink: an `xdr:contentPart` in `mc:AlternateContent` pointing at an InkML part,
//! with the picture fallback Excel requires. The package is built in code: Excel-produced files
//! are never committed.

use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, AnchorMode, Color, InkEffect, InkStyle, InkTool, Shape, ShapeKind, Sheet, Workbook};

use crate::tests::make_zip;
use crate::{read_xlsx, write_xlsx};

const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const XDR14: &str = "http://schemas.microsoft.com/office/excel/2010/spreadsheetDrawing";

const GUIDE: &str = "{11111111-2222-4333-8444-555555555555}";

fn content_types(ink: bool, png: bool) -> String {
    let mut s = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#.to_string();
    if png {
        s.push_str(r#"<Default Extension="png" ContentType="image/png"/>"#);
    }
    if ink {
        s.push_str(r#"<Override PartName="/xl/ink/ink1.xml" ContentType="application/inkml+xml"/>"#);
    }
    s.push_str("</Types>");
    s
}

fn drawing() -> String {
    format!(
        r#"<?xml version="1.0"?><xdr:wsDr xmlns:xdr="{XDR}" xmlns:a="{A}" xmlns:r="{NS_REL}"><xdr:twoCellAnchor editAs="oneCell"><xdr:from><xdr:col>3</xdr:col><xdr:colOff>76200</xdr:colOff><xdr:row>5</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:colOff>228600</xdr:colOff><xdr:row>7</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><mc:AlternateContent xmlns:mc="{MC}"><mc:Choice xmlns:xdr14="{XDR14}" xmlns:aink="http://schemas.microsoft.com/office/drawing/2016/ink" Requires="xdr14 aink"><xdr:contentPart r:id="rId1"><xdr14:nvContentPartPr><xdr14:cNvPr id="3" name="Ink 2"><a:extLst><a:ext uri="{{FF2B5EF4-FFF2-40B4-BE49-F238E27FC236}}"><a16:creationId xmlns:a16="http://schemas.microsoft.com/office/drawing/2014/main" id="{GUIDE}"/></a:ext></a:extLst></xdr14:cNvPr><xdr14:cNvContentPartPr/></xdr14:nvContentPartPr><xdr14:nvPr macro=""/><xdr14:xfrm><a:off x="1905000" y="952500"/><a:ext cx="762000" cy="381000"/></xdr14:xfrm></xdr:contentPart></mc:Choice><mc:Fallback><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="3" name="Ink 2"/><xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="762000" cy="381000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic></mc:Fallback></mc:AlternateContent><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#
    )
}

fn inkml(trace: &str, f: bool, props: &str) -> String {
    let mut channels = String::from(
        r#"<inkml:channel name="X" type="integer" min="-2.14748E9" max="2.14748E9" units="cm"/><inkml:channel name="Y" type="integer" min="-2.14748E9" max="2.14748E9" units="cm"/>"#,
    );
    let mut cp = String::from(
        r#"<inkml:channelProperty channel="X" name="resolution" value="1000" units="1/cm"/><inkml:channelProperty channel="Y" name="resolution" value="1000" units="1/cm"/>"#,
    );
    if f {
        channels.push_str(r#"<inkml:channel name="F" type="integer" max="32767" units="dev"/>"#);
        cp.push_str(r#"<inkml:channelProperty channel="F" name="resolution" value="0" units="1/dev"/>"#);
    }
    format!(
        r##"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><inkml:ink xmlns:inkml="http://www.w3.org/2003/InkML"><inkml:definitions><inkml:context xml:id="ctx0"><inkml:inkSource xml:id="inkSrc0"><inkml:traceFormat>{channels}</inkml:traceFormat><inkml:channelProperties>{cp}</inkml:channelProperties></inkml:inkSource><inkml:timestamp xml:id="ts0" timeString="2026-11-08T02:47:38.696"/></inkml:context><inkml:brush xml:id="br0">{props}</inkml:brush></inkml:definitions><inkml:trace contextRef="#ctx0" brushRef="#br0">{trace}</inkml:trace></inkml:ink>"##
    )
}

fn tiny_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 1, 1);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[0, 0, 0, 0]).unwrap();
    }
    out
}

/// A minimal one-sheet package with an Excel-shaped ink drawing.
fn package(trace: &str, f: bool, props: &str, ink: bool, declared: bool, media: bool) -> Vec<u8> {
    let wb = r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let wb_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="{NS_REL}"><sheetData/><drawing r:id="rId1"/></worksheet>"#
    );
    let sheet_rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{NS_REL}/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#
    );
    let drels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{NS_REL}/customXml" Target="../ink/ink1.xml"/><Relationship Id="rId2" Type="{NS_REL}/image" Target="../media/image1.png"/></Relationships>"#
    );
    let root = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let ct = content_types(declared, media);
    let drawing = drawing();
    let ink_part = inkml(trace, f, props);
    let png = tiny_png();
    let mut parts: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", root.as_bytes()),
        ("xl/workbook.xml", wb.as_bytes()),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels.as_bytes()),
        ("xl/drawings/drawing1.xml", drawing.as_bytes()),
        ("xl/drawings/_rels/drawing1.xml.rels", drels.as_bytes()),
    ];
    if ink {
        parts.push(("xl/ink/ink1.xml", ink_part.as_bytes()));
    }
    if media {
        parts.push(("xl/media/image1.png", &png));
    }
    make_zip(&parts)
}

const PEN_PROPS: &str = r##"<inkml:brushProperty name="width" value="0.05" units="cm"/><inkml:brushProperty name="height" value="0.05" units="cm"/><inkml:brushProperty name="color" value="#1F5FC9"/>"##;

fn read(bytes: &[u8]) -> (Workbook, crate::ReadReport) {
    read_xlsx(bytes).unwrap()
}

fn ink_shape(wb: &Workbook) -> &Shape {
    let shapes = &wb.sheets[0].shapes;
    assert_eq!(shapes.len(), 1, "{shapes:?}");
    &shapes[0]
}

fn points(shape: &Shape) -> Vec<[f32; 2]> {
    shape.text.split(';').filter_map(|p| p.split_once(',').and_then(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?]))).collect()
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}

#[test]
fn excel_content_part_is_read_as_editable_ink_with_its_brush() {
    let (wb, _) = read(&package("!0 !0,'1000 '1000,\"0 \"-2000,0 0", false, PEN_PROPS, true, true, true));
    let s = ink_shape(&wb);
    assert_eq!(s.kind, ShapeKind::Ink);
    assert!(wb.sheets[0].images.is_empty(), "the fallback picture is not a separate object");
    let ink = s.ink.as_ref().expect("editable ink");
    assert_eq!(ink.tool, InkTool::Pen);
    assert_eq!(ink.effect, None);
    close(ink.width_cm, 0.05);
    close(ink.height_cm, 0.05);
    assert_eq!(ink.key.as_deref(), Some(GUIDE));
    assert_eq!(s.line, Color::Rgb(0x1F5FC9));
    // The xfrm (200 px, 100 px, 80 × 40 px) gives the anchor: column D (192 px) + 8 px, row 6.
    assert_eq!(s.anchor.cell.a1(), "D6");
    assert_eq!(s.anchor.mode, AnchorMode::MoveOnly);
    close(s.anchor.dx, 8.0);
    close(s.anchor.dy, 0.0);
    close(s.anchor.width, 80.0);
    close(s.anchor.height, 40.0);
}

#[test]
fn all_trace_prefixes_and_the_wildcard_decode() {
    // X: 0, 1000, 2000, 3000 (thousandths of a cm) and Y: 0, 1000, 0, -1000.
    let (wb, _) = read(&package("!0 !0,'1000 '1000,\"0 \"-2000,0 0", false, PEN_PROPS, true, true, true));
    let points = points(ink_shape(&wb));
    let expected = [[0.0, 0.0], [37.795_276, 37.795_276], [75.590_55, 0.0], [113.385_83, -37.795_276]];
    assert_eq!(points.len(), expected.len());
    for (got, want) in points.iter().zip(&expected) {
        close(got[0], want[0] as f32);
        close(got[1], want[1] as f32);
    }
}

#[test]
fn pressure_and_effects_survive_a_read_and_a_gridcraft_save() {
    let trace = "!0 !0 !1000,'1000 '1000 '1000,\"0 \"-2000 0";
    let props = concat!(
        r##"<inkml:brushProperty name="width" value="0.35" units="cm"/><inkml:brushProperty name="height" value="0.35" units="cm"/><inkml:brushProperty name="color" value="#FF00AA"/>"##,
        r#"<inkml:brushProperty name="inkEffects" value="rosegold"/><inkml:brushProperty name="anchorX" value="-1303.86621"/><inkml:brushProperty name="anchorY" value="-1016"/><inkml:brushProperty name="scaleFactor" value="0.5"/>"#
    );
    let (wb, _) = read(&package(trace, true, props, true, true, true));
    let original = ink_shape(&wb).clone();
    let ink = original.ink.as_ref().unwrap();
    assert_eq!(ink.effect, Some(InkEffect::RoseGold));
    // F's last prefix is `'` (no `"` in the fixture for F), so the third value repeats the first difference.
    assert_eq!(ink.pressure, vec![1000, 2000, 2000]);
    close(ink.anchor[0], -1_303.866_2);
    close(ink.anchor[1], -1016.0);
    close(ink.scale, 0.5);

    // Reopen what GridCraft writes: the brush, pressure and key come back unchanged.
    let (back, _) = read(&write_xlsx(&wb).unwrap());
    let again = ink_shape(&back).clone();
    assert_eq!(again.ink, original.ink);
    assert_eq!(again.line, original.line);
    assert!(back.sheets[0].images.is_empty());
}

#[test]
fn every_tool_and_effect_round_trips_through_gridcraft() {
    let tools = [InkTool::Pen, InkTool::Pencil, InkTool::Highlighter, InkTool::FountainPen, InkTool::BrushPen];
    let effects = [
        None,
        Some(InkEffect::Rainbow),
        Some(InkEffect::Galaxy),
        Some(InkEffect::Lava),
        Some(InkEffect::Ocean),
        Some(InkEffect::RoseGold),
        Some(InkEffect::Gold),
        Some(InkEffect::Silver),
        Some(InkEffect::Bronze),
    ];
    let mut shapes = Vec::new();
    let mut id = 1;
    for tool in tools {
        for effect in effects {
            let key = format!("{{00000000-0000-4000-8000-{id:012X}}}");
            shapes.push(Shape {
                id,
                kind: ShapeKind::Ink,
                anchor: Anchor { cell: CellRef::parse("B2").unwrap(), dx: 2.0, dy: 3.0, width: 90.0, height: 45.0, mode: AnchorMode::Absolute },
                fill: Color::Rgb(2 << 16),
                line: Color::Rgb(0xA12BCD),
                text: "0,0;40,20;80,4".into(),
                format: Default::default(),
                math: Default::default(),
                ink: Some(InkStyle {
                    tool,
                    width_cm: 0.1,
                    height_cm: if tool == InkTool::Pencil { 0.6 } else { 0.1 },
                    effect,
                    anchor: [-1_303.866_2, -1016.0],
                    scale: 0.5,
                    pressure: vec![100, 2000, 32767],
                    order: id,
                    texture: None,
                    texture_scale: 1.0,
                    key: Some(key),
                }),
            });
            id += 1;
        }
    }
    let sheet = Sheet { shapes, ..Sheet::new("Ink") };
    let wb = Workbook { sheets: vec![Arc::new(sheet)], ..Default::default() };
    let (back, _) = read(&write_xlsx(&wb).unwrap());
    let got = &back.sheets[0].shapes;
    assert_eq!(got.len(), 45, "images={}", back.sheets[0].images.len());
    for (got, want) in got.iter().zip(&wb.sheets[0].shapes) {
        assert_eq!(got.kind, ShapeKind::Ink);
        assert_eq!(got.ink, want.ink, "shape {}", want.id);
        assert_eq!(got.line, want.line);
        close(got.anchor.width, want.anchor.width);
        close(got.anchor.height, want.anchor.height);
        close(got.anchor.dx, want.anchor.dx);
        close(got.anchor.dy, want.anchor.dy);
    }
    assert!(back.sheets[0].images.is_empty());
}

#[test]
fn a_second_save_is_byte_stable_for_ink_strokes() {
    let (wb, _) = read(&package("!0 !0,'1000 '1000,\"0 \"-2000", false, PEN_PROPS, true, true, true));
    let first = write_xlsx(&wb).unwrap();
    let (again, report) = read(&first);
    let second = write_xlsx(&again).unwrap();
    let names = |b: &[u8]| -> Vec<String> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(b.to_vec())).unwrap();
        (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect()
    };
    assert_eq!(names(&first), names(&second), "{:?}", report.warnings);
    let part = |b: &[u8], name: &str| -> Vec<u8> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(b.to_vec())).unwrap();
        let mut f = z.by_name(name).unwrap();
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut out).unwrap();
        out
    };
    for name in names(&first) {
        let (a, b) = (part(&first, &name), part(&second, &name));
        assert_eq!(String::from_utf8_lossy(&a), String::from_utf8_lossy(&b), "part {name}");
        assert_eq!(a, b, "part {name} (bytes)");
    }
    assert_eq!(first, second);
}

#[test]
fn malformed_or_oversized_ink_is_refused_without_a_panic() {
    let huge = format!("!0 !0,{} 0", "9".repeat(30));
    let mut many = String::from("!0 !0");
    for i in 1..=20_001 {
        many.push_str(&format!(",{i} {i}"));
    }
    let cases = [
        ("too few channels", "!0", false),
        ("too many channels", "!0 !0 0", false),
        ("not a number", "!x !0", false),
        ("over the coordinate cap", huge.as_str(), false),
        ("over the point cap", many.as_str(), false),
        ("empty", "", false),
    ];
    for (name, trace, f) in cases {
        let (wb, _) = read(&package(trace, f, PEN_PROPS, true, true, true));
        assert!(wb.sheets[0].shapes.is_empty(), "{name}: {:?}", wb.sheets[0].shapes);
        assert_eq!(wb.sheets[0].images.len(), 1, "{name}: the fallback picture is used");
    }
    // Hostile brush values are bounded, not fatal.
    let (wb, _) = read(&package(
        "!0 !0,'1000 '1000",
        false,
        r#"<inkml:brushProperty name="width" value="1e40" units="cm"/><inkml:brushProperty name="color" value="not-a-colour"/>"#,
        true,
        true,
        true,
    ));
    let ink = ink_shape(&wb).ink.as_ref().unwrap();
    assert!(ink.width_cm <= 100.0);
    assert_eq!(ink.tool, InkTool::Pen);
}

#[test]
fn a_missing_or_undeclared_ink_part_falls_back_to_the_picture() {
    for (name, ink, declared) in [("part missing", false, true), ("no content type", true, false)] {
        let (wb, report) = read(&package("!0 !0,'1000 '1000", false, PEN_PROPS, ink, declared, true));
        assert!(wb.sheets[0].shapes.is_empty(), "{name}");
        assert_eq!(wb.sheets[0].images.len(), 1, "{name}: the picture is used");
        assert!(report.warnings.iter().any(|w| w.contains("ink") || w.contains("picture")), "{name}: {:?}", report.warnings);
    }
}

#[test]
fn legacy_custgeom_ink_still_round_trips_without_an_ink_style() {
    let sheet = Sheet {
        shapes: vec![Shape {
            id: 7,
            kind: ShapeKind::Ink,
            anchor: Anchor { cell: CellRef::parse("C3").unwrap(), dx: 4.0, dy: 6.0, width: 100.0, height: 50.0, mode: AnchorMode::Absolute },
            fill: Color::Rgb(3 << 16),
            line: Color::Rgb(0xA12BCD),
            text: "0,0;40,20;90,10".into(),
            format: Default::default(),
            math: Default::default(),
            ink: None,
        }],
        ..Sheet::new("Legacy")
    };
    let wb = Workbook { sheets: vec![Arc::new(sheet)], ..Default::default() };
    let (back, _) = read(&write_xlsx(&wb).unwrap());
    let s = &back.sheets[0].shapes[0];
    assert_eq!(s.kind, ShapeKind::Ink);
    assert!(s.ink.is_none());
    assert_eq!(s.line, Color::Rgb(0xA12BCD));
    assert_eq!(s.text, "0,0;40,20;90,10");
}

#[test]
fn a_fountain_pen_without_pressure_stays_a_fountain_pen_and_keeps_an_empty_pressure() {
    let sheet = Sheet {
        shapes: vec![Shape {
            id: 9,
            kind: ShapeKind::Ink,
            anchor: Anchor { cell: CellRef::parse("B2").unwrap(), dx: 0.0, dy: 0.0, width: 60.0, height: 30.0, mode: AnchorMode::Absolute },
            fill: Color::Rgb(2 << 16),
            line: Color::Rgb(0x1F5FC9),
            text: "0,0;30,20".into(),
            format: Default::default(),
            math: Default::default(),
            ink: Some(InkStyle { tool: InkTool::FountainPen, key: Some("{AAAAAAAA-1111-4111-8111-111111111111}".into()), ..InkStyle::default() }),
        }],
        ..Sheet::new("Pen")
    };
    let wb = Workbook { sheets: vec![Arc::new(sheet)], ..Default::default() };
    let (back, _) = read(&write_xlsx(&wb).unwrap());
    let ink = back.sheets[0].shapes[0].ink.as_ref().expect("editable ink");
    assert_eq!(ink.tool, InkTool::FountainPen);
    assert!(ink.pressure.is_empty(), "no F channel was recorded, so none comes back: {:?}", ink.pressure);
    assert_eq!(ink.key.as_deref(), Some("{AAAAAAAA-1111-4111-8111-111111111111}"));
}
