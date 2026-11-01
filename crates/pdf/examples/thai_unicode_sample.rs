use gridcraft_pdf::{FontCatalog, PdfDoc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).unwrap_or_else(|| "target/thai-pdf-sample.pdf".to_string());
    let catalog = FontCatalog::system();
    let font = catalog.face_for("Arial", false, false, "น้ำ กุ้ง ผู้ใช้ เก้าอี้ ยอดรวม บาท").map_err(std::io::Error::other)?;
    let mut doc = PdfDoc::new();
    doc.info.title = "GridCraft Thai PDF regression sample".to_string();
    let page = doc.add_page(595.0, 842.0);
    let font_id = page.register_unicode_font(font);
    for (i, text) in ["ภาษาไทย", "น้ำ", "กุ้ง", "ผู้ใช้", "เก้าอี้", "ป ป ญ ฐ", "ยอดรวม 1,234.50 บาท", "Thai mixed Latin and ภาษาไทย"].into_iter().enumerate()
    {
        page.text_unicode(40.0, 60.0 + i as f32 * 54.0, 18.0, font_id, [0, 0, 0], text)?;
    }
    std::fs::write(output, doc.finish())?;
    Ok(())
}
