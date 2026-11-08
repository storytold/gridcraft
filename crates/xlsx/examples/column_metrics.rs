//! Owned column/font and fixed-picture probes for black-box spreadsheet comparisons.
//!
//! `cargo run -p gridcraft-xlsx --example column_metrics -- plan/excel/column-metrics`
//!
//! Writes original packages with specified raw column widths, plus saves made by this
//! checkout. No vendor templates, images, fonts or issue attachments are used. Reference-app
//! saves and screenshots stay outside version control. See `docs/column-metrics-probe.md`
//! for the native comparison protocol and baseline observations.

use std::io::{Cursor, Write};
use std::path::Path;

const NS: &str = "http://schemas.openxmlformats.org/";
const WIDTHS: [f64; 8] = [1.0, 2.5, 5.0, 8.43, 10.0, 13.7, 20.0, 30.0];
const CASES: [(&str, u16); 4] = [("Calibri", 11), ("Calibri", 12), ("Calibri", 14), ("Arial", 16)];

fn crc(bytes: &[u8]) -> u32 {
    let mut c = u32::MAX;
    for b in bytes {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(c & 1));
        }
    }
    !c
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut checked = kind.to_vec();
    checked.extend_from_slice(data);
    out.extend_from_slice(&crc(&checked).to_be_bytes());
}

/// A contributor-original one-pixel blue PNG, generated rather than stored as a fixture asset.
fn picture() -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
    // A zlib stream containing one uncompressed DEFLATE block (filter byte + RGB).
    let raw = [0u8, 20, 70, 160];
    let (mut a, mut b) = (1u32, 0u32);
    for byte in raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    let mut stream = vec![0x78, 0x01, 0x01, 4, 0, 0xfb, 0xff];
    stream.extend_from_slice(&raw);
    stream.extend_from_slice(&((b << 16) | a).to_be_bytes());
    chunk(&mut png, b"IDAT", &stream);
    chunk(&mut png, b"IEND", &[]);
    png
}

fn pic(id: u32, name: &str) -> String {
    format!(
        "<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{id}\" name=\"{name}\"/><xdr:cNvPicPr/></xdr:nvPicPr>\
         <xdr:blipFill><a:blip r:embed=\"rId1\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill>\
         <xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"952500\" cy=\"476250\"/></a:xfrm>\
         <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic>"
    )
}

fn original(font: &str, size: u16) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut put = |path: &str, data: &[u8]| -> Result<(), Box<dyn std::error::Error>> {
        zip.start_file(path, options)?;
        zip.write_all(data)?;
        Ok(())
    };
    put("[Content_Types].xml", format!(
        "<Types xmlns=\"{NS}package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/>\
         <Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
         <Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>\
         <Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
         <Override PartName=\"/xl/drawings/drawing1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\"/></Types>"
    ).as_bytes())?;
    put("_rels/.rels", format!("<Relationships xmlns=\"{NS}package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{NS}officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>").as_bytes())?;
    put("xl/workbook.xml", format!("<workbook xmlns=\"{NS}spreadsheetml/2006/main\" xmlns:r=\"{NS}officeDocument/2006/relationships\"><bookViews><workbookView/></bookViews><sheets><sheet name=\"{font}-{size}\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>").as_bytes())?;
    put("xl/_rels/workbook.xml.rels", format!("<Relationships xmlns=\"{NS}package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{NS}officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"{NS}officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>").as_bytes())?;
    put("xl/styles.xml", format!(
        "<styleSheet xmlns=\"{NS}spreadsheetml/2006/main\"><fonts count=\"1\"><font><sz val=\"{size}\"/><name val=\"{font}\"/></font></fonts>\
         <fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>\
         <borders count=\"1\"><border/></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
         <cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"3\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/></cellXfs>\
         <cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>"
    ).as_bytes())?;
    let cols: String =
        WIDTHS.iter().enumerate().map(|(i, w)| format!("<col min=\"{}\" max=\"{}\" width=\"{w}\" customWidth=\"1\"/>", i + 1, i + 1)).collect();
    let labels: String =
        WIDTHS.iter().enumerate().map(|(i, w)| format!("<c r=\"{}1\" t=\"inlineStr\"><is><t>{w}</t></is></c>", char::from(b'A' + i as u8))).collect();
    let values: String = (b'A'..=b'I').map(|col| format!("<c r=\"{}2\" s=\"1\"><v>2870000</v></c>", char::from(col))).collect();
    put("xl/worksheets/sheet1.xml", format!(
        "<worksheet xmlns=\"{NS}spreadsheetml/2006/main\" xmlns:r=\"{NS}officeDocument/2006/relationships\"><dimension ref=\"A1:I20\"/>\
         <sheetViews><sheetView workbookViewId=\"0\" zoomScale=\"100\"/></sheetViews><sheetFormatPr baseColWidth=\"8\" defaultRowHeight=\"15\"/>\
         <cols>{cols}</cols><sheetData><row r=\"1\">{labels}<c r=\"I1\" t=\"inlineStr\"><is><t>Default</t></is></c></row><row r=\"2\">{values}</row></sheetData><drawing r:id=\"rId1\"/></worksheet>"
    ).as_bytes())?;
    put("xl/worksheets/_rels/sheet1.xml.rels", format!("<Relationships xmlns=\"{NS}package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{NS}officeDocument/2006/relationships/drawing\" Target=\"../drawings/drawing1.xml\"/></Relationships>").as_bytes())?;
    put("xl/drawings/drawing1.xml", format!(
        "<xdr:wsDr xmlns:xdr=\"{NS}drawingml/2006/spreadsheetDrawing\" xmlns:a=\"{NS}drawingml/2006/main\" xmlns:r=\"{NS}officeDocument/2006/relationships\">\
         <xdr:oneCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>4</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>\
         <xdr:ext cx=\"952500\" cy=\"476250\"/>{}<xdr:clientData/></xdr:oneCellAnchor>\
         <xdr:absoluteAnchor><xdr:pos x=\"381000\" y=\"1905000\"/><xdr:ext cx=\"952500\" cy=\"476250\"/>{}<xdr:clientData/></xdr:absoluteAnchor></xdr:wsDr>",
        pic(1, "MoveOnly-100x50px"), pic(2, "Absolute-100x50px")
    ).as_bytes())?;
    put("xl/drawings/_rels/drawing1.xml.rels", format!("<Relationships xmlns=\"{NS}package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{NS}officeDocument/2006/relationships/image\" Target=\"../media/probe.png\"/></Relationships>").as_bytes())?;
    put("xl/media/probe.png", &picture())?;
    Ok(zip.finish()?.into_inner())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "plan/excel/column-metrics".to_string());
    let dir = Path::new(&arg);
    std::fs::create_dir_all(dir)?;
    println!("Raw column widths A:H: {WIDTHS:?}; I uses baseColWidth=8.");
    println!("Both pictures have physical extents 952500 × 476250 EMU (75 × 37.5 pt).");
    for (font, size) in CASES {
        let original = original(font, size)?;
        let stem = format!("{}-{size}", font.to_ascii_lowercase());
        let path = dir.join(format!("{stem}-original.xlsx"));
        std::fs::write(&path, &original)?;
        let (wb, _) = gridcraft_xlsx::read_xlsx(&original)?;
        let saved = dir.join(format!("{stem}-saved.xlsx"));
        std::fs::write(&saved, gridcraft_xlsx::write_xlsx(&wb)?)?;
        println!("{} → {}", path.display(), saved.display());
    }
    Ok(())
}
