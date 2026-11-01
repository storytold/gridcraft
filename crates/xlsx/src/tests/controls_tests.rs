//! Form controls: the three parts written for each control agree, they read back, and files with
//! only VML or with odd values give valid controls.

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, AnchorMode, Comment, ControlAction, ControlKind, FormControl, MacroInput, Sheet, Workbook};

use super::minimal;
use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

fn anchor(cell: &str, width: f32, height: f32) -> Anchor {
    Anchor { cell: at(cell), dx: 4.0, dy: 3.0, width, height, mode: AnchorMode::MoveOnly }
}

fn note(author: &str, text: &str) -> Comment {
    Comment {
        author: author.into(),
        text: text.into(),
        replies: vec![],
        threaded: false,
        resolved: false,
        visible: false,
        dates: vec![],
        note_box: None,
    }
}

fn book() -> Workbook {
    let mut wb = Workbook::default();
    wb.sheets.clear();
    let mut sh = Sheet::new("Data");
    sh.comments.insert(at("A1"), note("Me", "a note"));
    sh.controls.push(FormControl {
        id: 1,
        kind: ControlKind::Slider,
        anchor: anchor("C3", 150.0, 20.0),
        link: Some("$B$1".into()),
        min: 5,
        max: 500,
        step: 5,
        page: 50,
        value: 35,
        name: "Rate".into(),
        ..Default::default()
    });
    sh.controls.push(FormControl {
        id: 2,
        kind: ControlKind::Spin,
        anchor: Anchor { mode: AnchorMode::Absolute, ..anchor("F3", 20.0, 40.0) },
        link: Some("'Other Sheet'!$A$2".into()),
        max: 30_000,
        step: 100,
        value: 30_000,
        print: false,
        ..Default::default()
    });
    sh.controls.push(FormControl {
        id: 3,
        kind: ControlKind::Button,
        anchor: Anchor { mode: AnchorMode::MoveAndSize, ..anchor("C8", 100.0, 28.0) },
        text: "Run <it> & go".into(),
        action: Some(ControlAction::Macro { name: "Fill Down".into(), input: MacroInput::Selection, output: None }),
        locked: false,
        ..Default::default()
    });
    sh.controls.push(FormControl { id: 4, kind: ControlKind::Slider, anchor: anchor("H3", 20.0, 120.0), horizontal: false, ..Default::default() });
    wb.sheets.push(sh.into());
    wb.sheets.push(Sheet::new("Other Sheet").into());
    wb
}

fn text(pkg: &mut Package<'_>, part: &str) -> String {
    String::from_utf8(pkg.read(part).unwrap().unwrap_or_else(|| panic!("missing {part}"))).unwrap()
}

#[test]
fn the_three_parts_agree_and_read_back() {
    let wb = book();
    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let sheet = text(&mut pkg, "xl/worksheets/sheet1.xml");
    let vml = text(&mut pkg, "xl/drawings/vmlDrawing1.vml");
    let types = text(&mut pkg, "[Content_Types].xml");
    assert!(sheet.contains("<mc:Choice xmlns:x14=") && sheet.contains("<controls>"), "{sheet}");
    // Each control's shape id is a VML shape of the same kind; the note keeps its own id.
    let ids: Vec<String> = sheet.split("<control shapeId=\"").skip(1).map(|s| s.split('"').next().unwrap().to_string()).collect();
    assert_eq!(ids.len(), 8, "a Choice and a Fallback per control");
    for (i, kind) in ["Scroll", "Spin", "Button", "Scroll"].iter().enumerate() {
        let id = &ids[i * 2];
        let shape = vml.split(&format!("id=\"_x0000_s{id}\"")).nth(1).unwrap_or_else(|| panic!("no shape {id}"));
        assert!(shape.starts_with(" type=\"#_x0000_t201\""), "{shape}");
        assert!(shape.split("</v:shape>").next().unwrap().contains(&format!("ObjectType=\"{kind}\"")), "{id}");
    }
    assert!(vml.contains("id=\"_x0000_s1025\"") && vml.contains("ObjectType=\"Note\""), "the note is shape 1025");
    assert!(!ids.contains(&"1025".to_string()));
    for n in 1..=4 {
        assert!(types.contains(&format!("/xl/ctrlProps/ctrlProp{n}.xml")), "content type {n}");
    }
    let prop = text(&mut pkg, "xl/ctrlProps/ctrlProp1.xml");
    assert!(
        prop.contains("objectType=\"Scroll\"") && prop.contains("fmlaLink=\"$B$1\"") && prop.contains("page=\"50\"") && prop.contains("horiz=\"1\""),
        "{prop}"
    );
    assert!(sheet.contains("macro=\"[0]!Fill Down\""));

    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let got = &back.sheet(0).unwrap().controls;
    assert_eq!(got.len(), 4);
    for (a, b) in wb.sheet(0).unwrap().controls.iter().zip(got) {
        let same = |x: &FormControl| {
            (x.kind, x.link.clone(), x.min, x.max, x.step, x.value, x.horizontal, x.print, x.locked, x.action.clone(), x.anchor.cell, x.anchor.mode)
        };
        assert_eq!(same(a), same(b));
        assert!(
            (a.anchor.width - b.anchor.width).abs() < 1.0
                && (a.anchor.height - b.anchor.height).abs() < 1.0
                && (a.anchor.dx - b.anchor.dx).abs() < 1.0
        );
        if a.kind == ControlKind::Slider {
            assert_eq!(a.page, b.page);
        }
    }
    assert_eq!(got[0].name, "Rate");
    assert_eq!(got[2].text, "Run <it> & go");
    assert_eq!(back.sheet(0).unwrap().comments.len(), 1, "notes still read");
}

#[test]
fn many_shapes_take_several_id_blocks() {
    let mut wb = Workbook::default();
    wb.sheets.clear();
    let mut sh = Sheet::new("Data");
    for i in 0..1100u32 {
        sh.controls.push(FormControl { id: i + 1, kind: ControlKind::Spin, anchor: anchor("C3", 20.0, 30.0), ..Default::default() });
    }
    wb.sheets.push(sh.into());
    wb.sheets.push(Sheet::new("Next").into());
    if let Some(s) = wb.sheet_mut(1) {
        s.comments.insert(at("B2"), note("x", "y"));
    }
    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    assert!(text(&mut pkg, "xl/drawings/vmlDrawing1.vml").contains("data=\"1,2\""));
    let second = text(&mut pkg, "xl/drawings/vmlDrawing2.vml");
    assert!(second.contains("data=\"3\"") && second.contains("_x0000_s3073"), "the next sheet's ids don't collide");
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().controls.len(), 1100);
}

const VML_ONLY: &str = r##"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel">
<v:shape id="_x0000_s1025" type="#_x0000_t201"><x:ClientData ObjectType="Scroll"><x:Anchor>2, 0, 3, 0, 5, 0, 4, 0</x:Anchor><x:FmlaLink>$B$1</x:FmlaLink><x:Val>50000</x:Val><x:Min>-5</x:Min><x:Max>99999</x:Max><x:Inc>0</x:Inc><x:Page>-3</x:Page><x:Horiz/></x:ClientData></v:shape>
<v:shape id="_x0000_s1026" type="#_x0000_t201"><x:ClientData ObjectType="Spin"><x:Anchor>1, 0, 1, 0, 1, 20, 3, 0</x:Anchor><x:Val>7</x:Val><x:Min>9</x:Min><x:Max>9</x:Max></x:ClientData></v:shape>
<v:shape id="_x0000_s1027" o:button="t"><v:textbox><div>Go</div></v:textbox><x:ClientData ObjectType="Button"><x:Anchor>1, 0, 6, 0, 3, 0, 7, 0</x:Anchor><x:FmlaMacro>[0]!Tidy</x:FmlaMacro></x:ClientData></v:shape>
<v:shape id="_x0000_s1028"><x:ClientData ObjectType="Drop"><x:Anchor>1, 0, 1, 0, 2, 0, 2, 0</x:Anchor></x:ClientData></v:shape>
<v:shape id="_x0000_s1029"><x:ClientData ObjectType="Scroll"><x:Anchor>junk</x:Anchor></x:ClientData></v:shape>
</xml>"##;

#[test]
fn vml_only_controls_are_read_and_clamped() {
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><legacyDrawing r:id="rId9"/>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/drawings/vmlDrawing1.vml", VML_ONLY)],
        "",
        "",
    );
    let (wb, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.contains("form controls are not supported")), "{:?}", report.warnings);
    let c = &wb.sheet(0).unwrap().controls;
    assert_eq!(c.len(), 3, "the drop-down and the shape without an anchor are skipped");
    assert_eq!((c[0].kind, c[0].min, c[0].max, c[0].step, c[0].value, c[0].horizontal), (ControlKind::Slider, 0, 30_000, 1, 30_000, true));
    assert!(c[0].page >= c[0].step && c[0].check_numbers().is_ok());
    assert_eq!((c[1].min, c[1].max), (9, 10), "a range with no width gets one");
    assert!(c[1].check_numbers().is_ok());
    assert_eq!(c[2].text, "Go");
    assert_eq!(c[2].action, Some(ControlAction::Macro { name: "Tidy".into(), input: MacroInput::Selection, output: None }));
}

/// Shapes with a named `id` and their number in `o:spid`, as some producers write them.
const VML_SPID: &str = r##"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel">
<v:shape id="Button 1" o:spid="_x0000_s1025" o:button="t"><v:textbox><div>Go</div></v:textbox><x:ClientData ObjectType="Button"><x:Anchor>1, 0, 6, 0, 3, 0, 7, 0</x:Anchor><x:FmlaMacro>[0]!Tidy</x:FmlaMacro></x:ClientData></v:shape>
<v:shape id="Spinner 2" o:spid="_x0000_s1026"><x:ClientData ObjectType="Spin"><x:Anchor>1, 0, 1, 0, 1, 20, 3, 0</x:Anchor><x:Val>7</x:Val><x:Max>9</x:Max></x:ClientData></v:shape>
</xml>"##;

#[test]
fn vml_shapes_are_also_found_by_their_spid() {
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/></Relationships>"#;
    // The button is listed in <controls> without a properties part; the spin button is only in
    // the VML.
    let bytes = minimal(
        r#"<sheetData/><legacyDrawing r:id="rId9"/><controls><control shapeId="1025" name="Go Button"/></controls>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/drawings/vmlDrawing1.vml", VML_SPID)],
        "",
        "",
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let c = &wb.sheet(0).unwrap().controls;
    assert_eq!(c.len(), 2, "{c:?}");
    assert_eq!((c[0].kind, c[0].name.as_str(), c[0].text.as_str()), (ControlKind::Button, "Go Button", "Go"));
    assert_eq!(c[0].action, Some(ControlAction::Macro { name: "Tidy".into(), input: MacroInput::Selection, output: None }));
    assert_eq!((c[1].kind, c[1].value, c[1].max), (ControlKind::Spin, 7, 9));
    // Saved again, each is written once and nothing of them is kept twice.
    let out = write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&out).unwrap();
    assert_eq!(back.sheet(0).unwrap().controls.len(), 2);
}

#[test]
fn the_properties_part_wins_over_the_vml() {
    let mut wb = book();
    if let Some(s) = wb.sheet_mut(0) {
        s.comments.clear();
        s.controls.truncate(1);
    }
    let bytes = write_xlsx(&wb).unwrap();
    // Rewrite the VML's numbers: the reader still takes the properties part's.
    let mut pkg = Package::open(&bytes).unwrap();
    let mut parts = Vec::new();
    for name in pkg.names() {
        let mut data = pkg.read(&name).unwrap().unwrap();
        if name.ends_with(".vml") {
            data = String::from_utf8(data).unwrap().replace("<x:Max>500</x:Max>", "<x:Max>9</x:Max>").into_bytes();
        }
        parts.push((name, data));
    }
    let zipped = super::make_zip(&parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect::<Vec<_>>());
    let (back, _) = read_xlsx(&zipped).unwrap();
    assert_eq!(back.sheet(0).unwrap().controls[0].max, 500);
}

/// A hostile sheet: more controls than a sheet keeps, properties parts that are not XML or hold
/// numbers no control can take, shape ids that are not numbers, and oversized names, captions and
/// links. Reading must not panic, must keep at most the cap, and every control it keeps must pass
/// the model's rules.
#[test]
fn hostile_form_controls_are_capped_and_repaired() {
    let n = 13_000;
    let mut controls = String::from("<controls>");
    let anchor = r#"<controlPr><anchor><from><xdr:col>1</xdr:col><xdr:colOff>-99999999999</xdr:colOff><xdr:row>99999999999</xdr:row><xdr:rowOff>0</xdr:rowOff></from><to><xdr:col>-4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></to></anchor></controlPr>"#;
    for i in 0..n {
        let rid = match i % 10 {
            0 => "rIdB",
            1 => "rIdC",
            _ => "rIdA",
        };
        let _ = std::fmt::Write::write_fmt(
            &mut controls,
            format_args!(r#"<control shapeId="{}" r:id="{rid}" name="{}">{anchor}</control>"#, 2000 + i, "n".repeat(400)),
        );
    }
    controls
        .push_str(r#"<control shapeId="99999999999999999999999" r:id="rIdA"/><control shapeId="x" r:id="rIdA"/><control r:id="rIdA"/></controls>"#);
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdA" Type="http://schemas.microsoft.com/office/2006/relationships/ctrlProp" Target="../ctrlProps/a.xml"/><Relationship Id="rIdB" Type="http://schemas.microsoft.com/office/2006/relationships/ctrlProp" Target="../ctrlProps/b.xml"/><Relationship Id="rIdC" Type="http://schemas.microsoft.com/office/2006/relationships/ctrlProp" Target="../ctrlProps/missing.xml"/></Relationships>"#;
    let link = format!("$B${}", "9".repeat(500));
    let prop_a = format!(
        r#"<?xml version="1.0"?><formControlPr xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" objectType="Scroll" fmlaLink="{link}" min="1e400" max="NaN" inc="-inf" page="18446744073709551616" val="-7"/>"#
    );
    let body = format!(r#"<sheetData/>{controls}"#)
        .replace("<controls>", r#"<controls xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing">"#);
    let bytes = minimal(
        &body,
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/ctrlProps/a.xml", &prop_a), ("xl/ctrlProps/b.xml", "\u{0}\u{1}not xml <<<")],
        "",
        "",
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let c = &wb.sheet(0).unwrap().controls;
    assert_eq!(c.len(), 10_000, "the cap of one sheet");
    for x in c {
        assert_eq!(x.kind, ControlKind::Slider);
        assert!(x.check_numbers().is_ok(), "{x:?}");
        assert!(x.value >= x.min && x.value <= x.max, "{x:?}");
        assert!(x.name.chars().count() <= 255 && x.link.is_none(), "{x:?}");
        assert!(x.anchor.width >= 1.0 && x.anchor.height >= 1.0 && x.anchor.cell.row < gridcraft_core::MAX_ROWS, "{x:?}");
    }
    // Written back and read again, the kept controls stay valid.
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheet(0).unwrap().controls.len(), c.len());
}

/// Each listed control finds its VML shape through an index, so a sheet of many controls reads in
/// linear time (it used to scan every shape for every control).
#[test]
#[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
fn many_listed_controls_read_in_linear_time() {
    let mut wb = Workbook::default();
    if let Some(sh) = wb.sheet_mut(0) {
        for i in 0..10_000u32 {
            let a = anchor(&format!("B{}", i + 1), 60.0, 18.0);
            sh.controls.push(FormControl { id: i + 1, kind: ControlKind::Spin, anchor: a, ..Default::default() });
        }
    }
    let bytes = write_xlsx(&wb).unwrap();
    let t0 = std::time::Instant::now();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let took = t0.elapsed();
    assert_eq!(back.sheet(0).unwrap().controls.len(), 10_000);
    // About 0.4 s here; scanning every shape for every control took over 8 s.
    assert!(took < std::time::Duration::from_secs(3), "{took:?}");
}

/// The sheet relates each control properties part with the relationship type MS-XLSX defines and
/// Excel writes (in the officeDocument namespace); Excel finds the part by that type.
#[test]
fn the_properties_parts_use_the_ctrl_prop_relationship_type() {
    let bytes = write_xlsx(&book()).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let rels = text(&mut pkg, "xl/worksheets/_rels/sheet1.xml.rels");
    let types: Vec<&str> = rels.split("Type=\"").skip(1).filter_map(|r| r.split('"').next()).filter(|t| t.ends_with("/ctrlProp")).collect();
    assert_eq!(types.len(), 4, "{rels}");
    assert!(types.iter().all(|t| *t == "http://schemas.openxmlformats.org/officeDocument/2006/relationships/ctrlProp"), "{types:?}");
}

/// A sheet with a header and footer drawing and a drawing for its controls: the controls come
/// from the drawing `<legacyDrawing r:id>` names, whichever relationship is listed first.
#[test]
fn controls_come_from_the_legacy_drawing_not_the_header_and_footer_drawing() {
    let hf = r##"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office"><v:shape id="LH" o:spid="_x0000_s2049" type="#_x0000_t75"><v:imagedata o:title="logo"/></v:shape></xml>"##;
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawingHF1.vml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><legacyDrawing r:id="rId9"/><legacyDrawingHF r:id="rId1"/>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/drawings/vmlDrawingHF1.vml", hf), ("xl/drawings/vmlDrawing1.vml", VML_ONLY)],
        "",
        "",
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let c = &wb.sheet(0).unwrap().controls;
    assert_eq!(c.len(), 3, "{c:?}");
    assert_eq!(c[2].text, "Go");
}

/// Every listed control that names the same properties part shares one read of it. Each read is
/// charged to the package's 1 GB budget, so 10,000 reads of a 200 KB part would spend it, drop the
/// controls after that, and fail the parts read after the controls (here the sheet's notes). A
/// hostile file can list that many controls against one part.
#[test]
fn controls_naming_one_properties_part_read_it_once() {
    let n = 13_000;
    let mut controls = String::from(r#"<controls xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing">"#);
    for i in 0..n {
        let _ = std::fmt::Write::write_fmt(
            &mut controls,
            format_args!(
                r#"<control shapeId="{}" r:id="rIdA" name="c{i}"><controlPr><anchor><from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{i}</xdr:row><xdr:rowOff>0</xdr:rowOff></from><to><xdr:col>2</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>0</xdr:rowOff></to></anchor></controlPr></control>"#,
                2000 + i,
                i + 1
            ),
        );
    }
    controls.push_str("</controls>");
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdA" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/ctrlProp" Target="../ctrlProps/a.xml"/><Relationship Id="rIdN" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments1.xml"/></Relationships>"#;
    // 200 KB of attribute whitespace: read once per control, 10,000 reads would be 2 GB.
    let prop_a = format!(
        r#"<?xml version="1.0"?><formControlPr xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" objectType="Scroll" fmlaLink="$B$1"{}min="0" max="200" inc="5" page="20" val="45"/>"#,
        " ".repeat(200 * 1024)
    );
    let notes = r#"<?xml version="1.0"?><comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>Ann</author></authors><commentList><comment ref="A1" authorId="0"><text><t>read after the controls</t></text></comment></commentList></comments>"#;
    let bytes = minimal(
        &format!("<sheetData/>{controls}"),
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/ctrlProps/a.xml", &prop_a), ("xl/comments1.xml", notes)],
        "",
        "",
    );
    let (wb, report) = read_xlsx(&bytes).unwrap();
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.controls.len(), 10_000, "the cap of one sheet, every control read from the one part");
    assert!(sh.controls.iter().all(|c| {
        c.kind == ControlKind::Slider && c.link.as_deref() == Some("$B$1") && (c.min, c.max, c.step, c.page, c.value) == (0, 200, 5, 20, 45)
    }));
    assert_eq!(sh.comments.len(), 1, "a part read after the controls still reads: {:?}", report.warnings);
}
