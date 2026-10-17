//! Code-generated geometry/font/picture fixture for Linux equal-zoom comparison (#1185).
//! cargo run --release -p gridcraft-ui-egui --example linux_scale_repro -- scale.xlsx
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Anchor, Image};
use serde_json::json;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: linux_scale_repro out.xlsx")?;
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range":"A1","values":[["SYNTHETIC LINUX SCALE"],["Compare at identical zoom"],["Hg0123456789"],["12 pt DejaVu Sans"],["中文尺寸測試 012345"],["12 pt Noto Sans CJK TC"]]}))?;
    s.execute("home.fontName", json!({"range":"A1:A4","name":"DejaVu Sans"}))?;
    s.execute("home.fontName", json!({"range":"A5:A6","name":"Noto Sans CJK TC"}))?;
    s.execute("home.fontSize", json!({"range":"A1:A6","size":12}))?;
    s.execute("home.columnWidth", json!({"cols":"A:L","width":96}))?;
    s.execute("home.rowHeight", json!({"rows":"1:30","height":24}))?;
    s.execute("home.fillColor", json!({"range":"A8:D8","color":"#20B0D0"}))?;
    s.execute("view.zoom", json!({"percent":100}))?;
    let png = image::RgbImage::from_fn(320, 160, |_, _| image::Rgb([208, 32, 224]));
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(png).write_to(&mut encoded, image::ImageFormat::Png)?;
    let wb = Arc::make_mut(&mut s.active_mut().ok_or("no document")?.wb);
    Arc::make_mut(wb.sheets.first_mut().ok_or("no sheet")?).images.push(Image {
        id: 1,
        anchor: Anchor { cell: CellRef::new(9, 1), width: 320.0, height: 160.0, ..Default::default() },
        data: encoded.into_inner(),
        mime: "image/png".into(),
        alt: "Original synthetic 320 by 160 pixel rectangle".into(),
    });
    let bytes = gridcraft_xlsx::write_xlsx(wb)?;
    // The regular writer emits a two-cell anchor. For this measurement fixture use the
    // supported standard absolute anchor, so font substitutions changing column widths
    // cannot stretch the picture. All XML/pixels still originate in this example/our writer.
    let mut input = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut part = input.by_index(i)?;
        let name = part.name().to_string();
        let mut data = Vec::new();
        part.read_to_end(&mut data)?;
        if name == "xl/drawings/drawing1.xml" {
            let drawing = String::from_utf8(data)?;
            let start = drawing.find("<xdr:pic>").ok_or("missing generated picture")?;
            let end = drawing.find("</xdr:pic>").ok_or("missing generated picture end")? + "</xdr:pic>".len();
            let picture = drawing.get(start..end).ok_or("invalid generated picture span")?;
            data = format!(
                "<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><xdr:absoluteAnchor><xdr:pos x=\"914400\" y=\"2057400\"/><xdr:ext cx=\"3048000\" cy=\"1524000\"/>{picture}<xdr:clientData/></xdr:absoluteAnchor></xdr:wsDr>"
            ).into_bytes();
        }
        output.start_file(name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated))?;
        output.write_all(&data)?;
    }
    std::fs::write(out, output.finish()?.into_inner())?;
    Ok(())
}
