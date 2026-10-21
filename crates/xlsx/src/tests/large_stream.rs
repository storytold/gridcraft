//! A synthetic workbook whose shared-string part inflates past 100 MiB, plus the streaming
//! tree and the declared-size cap. Nothing here is an Excel-authored file.

use std::io::{Cursor, Write};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use super::make_zip;
use crate::IoError;
use crate::package::{MAX_PART, MAX_STREAM_PART, Package};
use crate::read_xlsx;
use crate::xml::{self, El};

const BODY: usize = 6 * 1024 * 1024;
const STRINGS: u32 = 17;

fn opts() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn content_types() -> &'static str {
    r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/></Types>"#
}

fn write_part(z: &mut zip::ZipWriter<Cursor<Vec<u8>>>, name: &str, bytes: &[u8]) {
    z.start_file(name, opts()).unwrap();
    z.write_all(bytes).unwrap();
}

/// Shared strings that decompress to at least 100 MiB, plus a small sheet with two of those
/// strings and two formulas.
fn large_package() -> (Vec<u8>, u64) {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_part(&mut z, "[Content_Types].xml", content_types().as_bytes());
    write_part(
        &mut z,
        "_rels/.rels",
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
    );
    write_part(
        &mut z,
        "xl/workbook.xml",
        br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    write_part(
        &mut z,
        "xl/_rels/workbook.xml.rels",
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#,
    );
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>{last}</v></c><c r="C1"><f>1+2</f><v>3</v></c><c r="D1"><v>21</v></c><c r="E1"><f>D1*2</f><v>42</v></c></row></sheetData></worksheet>"#,
        last = STRINGS - 1
    );
    write_part(&mut z, "xl/worksheets/sheet1.xml", sheet.as_bytes());

    z.start_file("xl/sharedStrings.xml", opts()).unwrap();
    let header = format!(
        r#"<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{STRINGS}" uniqueCount="{STRINGS}">"#
    );
    z.write_all(header.as_bytes()).unwrap();
    let mut inflated = header.len() as u64;
    let chunk = vec![b'x'; 1024 * 1024];
    for i in 0..STRINGS {
        let open = format!("<si><t>{i:04}");
        z.write_all(open.as_bytes()).unwrap();
        inflated += open.len() as u64;
        for _ in 0..(BODY / chunk.len()) {
            z.write_all(&chunk).unwrap();
            inflated += chunk.len() as u64;
        }
        let close = b"</t></si>";
        z.write_all(close).unwrap();
        inflated += close.len() as u64;
    }
    let footer = b"</sst>";
    z.write_all(footer).unwrap();
    inflated += footer.len() as u64;
    let bytes = z.finish().unwrap().into_inner();
    (bytes, inflated)
}

fn assert_no_named(el: &El, name: &str) {
    assert_ne!(el.name, name);
    for child in &el.children {
        assert_no_named(child, name);
    }
}

#[test]
fn hundred_mib_shared_strings_stream_into_sampled_cells() {
    let (bytes, inflated) = large_package();
    assert!(inflated >= 100 * 1024 * 1024, "inflated {inflated}");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
    let declared = archive.by_name("xl/sharedStrings.xml").unwrap().size();
    assert!(declared >= 100 * 1024 * 1024, "declared {declared}");
    drop(archive);

    let (wb, rep) = read_xlsx(&bytes).unwrap();
    assert!(!rep.warnings.iter().any(|w| w.contains("damaged") || w.contains("too large") || w.contains("dropped")), "{:?}", rep.warnings);
    let sh = wb.sheet(0).unwrap();
    let a1 = sh.value(gridcraft_core::CellRef::parse("A1").unwrap());
    let b1 = sh.value(gridcraft_core::CellRef::parse("B1").unwrap());
    let (gridcraft_core::Value::Text(a), gridcraft_core::Value::Text(b)) = (a1, b1) else {
        panic!("shared strings were not text");
    };
    let expect_len = 4 + BODY;
    assert_eq!(a.len(), expect_len);
    assert!(a.starts_with("0000"));
    assert!(a.ends_with('x'));
    assert_eq!(b.len(), expect_len);
    assert!(b.starts_with(&format!("{:04}", STRINGS - 1)));
    let c1 = sh.cell(gridcraft_core::CellRef::parse("C1").unwrap()).unwrap();
    assert_eq!(c1.formula.as_ref().map(|f| f.text.as_str()), Some(gridcraft_model::Formula::new("=1+2").text.as_str()));
    assert_eq!(sh.value(gridcraft_core::CellRef::parse("C1").unwrap()), gridcraft_core::Value::Number(3.0));
    let e1 = sh.cell(gridcraft_core::CellRef::parse("E1").unwrap()).unwrap();
    assert_eq!(e1.formula.as_ref().map(|f| f.text.as_str()), Some(gridcraft_model::Formula::new("=D1*2").text.as_str()));
    assert_eq!(sh.value(gridcraft_core::CellRef::parse("E1").unwrap()), gridcraft_core::Value::Number(42.0));
    assert_eq!(sh.value(gridcraft_core::CellRef::parse("D1").unwrap()), gridcraft_core::Value::Number(21.0));
}

#[test]
fn streamed_rows_are_absent_from_the_returned_tree() {
    let sheet = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row></sheetData>"#;
    let bytes = make_zip(&[
        ("[Content_Types].xml", content_types().as_bytes()),
        (
            "_rels/.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            format!(
                r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{sheet}</worksheet>"#
            )
            .as_bytes(),
        ),
    ]);
    let mut pkg = Package::open(&bytes).unwrap();
    let mut seen = 0u32;
    let root = pkg
        .parse_xml_streaming("xl/worksheets/sheet1.xml", &["row"], &mut |row| {
            assert_eq!(row.name, "row");
            seen += 1;
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(seen, 2);
    assert_no_named(&root, "row");

    let mut strings = 0u32;
    let sst = xml::parse_streaming(br#"<sst><si><t>one</t></si><si><t>two</t></si><si><t>three</t></si></sst>"#, &["si"], &mut |si| {
        assert_eq!(si.name, "si");
        strings += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(strings, 3);
    assert_no_named(&sst, "si");
}

/// Patches the central-directory uncompressed size of `name` so the reader sees a part past its cap
/// without storing that many bytes.
fn declare_uncompressed(bytes: &mut [u8], name: &str, size: u32) {
    let needle = name.as_bytes();
    let mut from = 0;
    let mut patched = 0;
    while let Some(rel) = bytes[from..].windows(needle.len()).position(|w| w == needle) {
        let at = from + rel;
        if at >= 46 && bytes[at - 46..at - 42] == [0x50, 0x4b, 0x01, 0x02] {
            let field = at - 22;
            bytes[field..field + 4].copy_from_slice(&size.to_le_bytes());
            patched += 1;
        }
        from = at + needle.len();
    }
    assert_eq!(patched, 1, "central directory entry for {name}");
}

#[test]
fn declared_stream_over_the_cap_is_an_error() {
    let sst = br#"<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1"><si><t>Hi</t></si></sst>"#;
    let mut bytes = make_zip(&[
        ("[Content_Types].xml", content_types().as_bytes()),
        (
            "_rels/.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            br#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData></worksheet>"#,
        ),
        ("xl/sharedStrings.xml", sst),
    ]);
    let over = u32::try_from(MAX_STREAM_PART.saturating_add(1)).unwrap_or(u32::MAX);
    declare_uncompressed(&mut bytes, "xl/sharedStrings.xml", over);
    let err = read_xlsx(&bytes).unwrap_err();
    assert!(matches!(err, IoError::TooLarge(_)), "{err}");
}

fn workbook_rels(extra: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>{extra}</Relationships>"#
    )
    .into_bytes()
}

fn sheet_with_styled_sample() -> &'static [u8] {
    br#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" s="1"><v>21</v></c><c r="B1" s="1"><f>A1*2</f><v>42</v></c></row></sheetData></worksheet>"#
}

/// Styles part whose one text node inflates to at least 100 MiB. The reader keeps styles, so this
/// is under the 512 MB buffered cap and must open. The pad is one node, not a tree of elements.
fn large_styles_package() -> (Vec<u8>, u64) {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_part(&mut z, "[Content_Types].xml", content_types().as_bytes());
    write_part(
        &mut z,
        "_rels/.rels",
        br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
    );
    write_part(
        &mut z,
        "xl/workbook.xml",
        br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    write_part(
        &mut z,
        "xl/_rels/workbook.xml.rels",
        &workbook_rels(
            r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
        ),
    );
    write_part(&mut z, "xl/worksheets/sheet1.xml", sheet_with_styled_sample());

    z.start_file("xl/styles.xml", opts()).unwrap();
    let head = br#"<?xml version="1.0"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="1"><fill><patternFill patternType="none"/></fill></fills><borders count="1"><border/></borders><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" applyFont="1"/></cellXfs><pad>"#;
    z.write_all(head).unwrap();
    let mut inflated = head.len() as u64;
    let chunk = vec![b'x'; 1024 * 1024];
    for _ in 0..100 {
        z.write_all(&chunk).unwrap();
        inflated += chunk.len() as u64;
    }
    let tail = b"</pad></styleSheet>";
    z.write_all(tail).unwrap();
    inflated += tail.len() as u64;
    let bytes = z.finish().unwrap().into_inner();
    (bytes, inflated)
}

#[test]
fn hundred_mib_styles_part_opens_with_sampled_cells() {
    let (bytes, inflated) = large_styles_package();
    assert!(inflated >= 100 * 1024 * 1024, "inflated {inflated}");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
    let declared = archive.by_name("xl/styles.xml").unwrap().size();
    assert!(declared >= 100 * 1024 * 1024, "declared {declared}");
    drop(archive);

    let (wb, rep) = read_xlsx(&bytes).unwrap();
    assert!(!rep.warnings.iter().any(|w| w.contains("damaged") || w.contains("too large") || w.contains("dropped")), "{:?}", rep.warnings);
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(gridcraft_core::CellRef::parse("A1").unwrap()), gridcraft_core::Value::Number(21.0));
    assert_eq!(sh.value(gridcraft_core::CellRef::parse("B1").unwrap()), gridcraft_core::Value::Number(42.0));
    let b1 = sh.cell(gridcraft_core::CellRef::parse("B1").unwrap()).unwrap();
    assert_eq!(b1.formula.as_ref().map(|f| f.text.as_str()), Some("A1*2"));
    assert!(wb.styles.get(sh.style_id(gridcraft_core::CellRef::parse("A1").unwrap())).font.bold);
}

#[test]
fn declared_styles_over_the_cap_is_an_error() {
    let styles = br#"<?xml version="1.0"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="1"><xf numFmtId="0"/></cellXfs></styleSheet>"#;
    let mut bytes = make_zip(&[
        ("[Content_Types].xml", content_types().as_bytes()),
        (
            "_rels/.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            &workbook_rels(
                r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
            ),
        ),
        ("xl/worksheets/sheet1.xml", sheet_with_styled_sample()),
        ("xl/styles.xml", styles),
    ]);
    let over = u32::try_from(MAX_PART.saturating_add(1)).unwrap_or(u32::MAX);
    declare_uncompressed(&mut bytes, "xl/styles.xml", over);
    let err = read_xlsx(&bytes).unwrap_err();
    assert!(matches!(err, IoError::TooLarge(_)), "{err}");
}
