//! Rasterize the actual print PDF rather than maintaining a second, incomplete sheet painter.
use hayro::hayro_interpret::{
    InterpreterSettings,
    font::{FontData, FontQuery},
    hayro_cmap::CidFamily,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn render(bytes: Arc<Vec<u8>>, index: usize) -> Result<egui::ColorImage, String> {
    let chinese = crate::theme::chinese_preview_font().map(|face| (Arc::new(face.font) as FontData, face.index));
    render_with_font(bytes, index, chinese)
}

fn render_with_font(bytes: Arc<Vec<u8>>, index: usize, chinese: Option<(FontData, u32)>) -> Result<egui::ColorImage, String> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("The print preview PDF is too large.".into());
    }
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let pdf = hayro::hayro_syntax::Pdf::new(bytes).map_err(|e| format!("Could not read the print preview: {e:?}"))?;
        let page = pdf.pages().get(index).ok_or("The preview page does not exist.")?;
        let (width, height) = page.render_dimensions();
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err("The preview page size is invalid.".into());
        }
        let scale = (1000.0 / width.max(height)).min(2.0);
        let settings = hayro::RenderSettings { x_scale: scale, y_scale: scale, ..Default::default() };
        let missing_font = Arc::new(AtomicBool::new(false));
        let missing = missing_font.clone();
        let defaults = InterpreterSettings::default();
        let standard = defaults.font_resolver.clone();
        let interpreter = InterpreterSettings {
            font_resolver: Arc::new(move |query| {
                // #1912 exports Chinese with a non-embedded GB1 CID font. Resolve it
                // with the UI's installed face instead of silently losing the glyphs.
                if matches!(query, FontQuery::Fallback(f) if f.character_collection.as_ref().is_some_and(|c| c.family == CidFamily::AdobeGB1)) {
                    if chinese.is_none() {
                        missing.store(true, Ordering::Relaxed);
                    }
                    return chinese.clone();
                }
                standard(query)
            }),
            ..defaults
        };
        let pixmap = hayro::render(page, &hayro::RenderCache::new(), &interpreter, &settings);
        if missing_font.load(Ordering::Relaxed) {
            return Err("A Chinese font is needed to display this print preview. The PDF can still be exported.".into());
        }
        Ok(egui::ColorImage::from_rgba_premultiplied([usize::from(pixmap.width()), usize::from(pixmap.height())], pixmap.data_as_u8_slice()))
    }))
    .map_err(|_| "Could not render the print preview.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;
    use serde_json::json;

    #[test]
    fn preview_renders_the_exported_cell_fill_and_chart() {
        let mut session = Session::new();
        session.new_workbook();
        for (cell, input) in [("A1", "Product"), ("B1", "Amount"), ("A2", "One"), ("B2", "10"), ("A3", "Two"), ("B3", "20")] {
            session.run("cell.set", json!({"cell":cell,"input":input})).unwrap();
        }
        session.run("home.fillColor", json!({"range":"A1:B1","color":"#FF0000"})).unwrap();
        session.run("insert.chart", json!({"range":"A1:B3","type":"column"})).unwrap();
        let pdf = session.run("file.print", json!({"sheets":"active","fitToPage":true})).unwrap();
        let bytes = gridcraft_engine::io::base64_decode(pdf["base64"].as_str().unwrap()).unwrap();
        let image = render(bytes.into(), 0).unwrap();
        assert!(image.pixels.iter().any(|p| p.r() > 200 && p.g() < 50 && p.b() < 50), "cell fill must appear");
        assert!(image.pixels.iter().filter(|p| p.b() > 100 && p.b() > p.r().saturating_add(30)).count() > 100, "chart bars must appear");
    }

    #[test]
    fn preview_keeps_upstream_cjk_export_text_visible() {
        let mut session = Session::new();
        session.new_workbook();
        session.run("cell.set", json!({"cell":"A1","input":"中文"})).unwrap();
        session.run("home.fontColor", json!({"range":"A1","color":"#FF0000"})).unwrap();
        let data = session.run("file.print", json!({"sheets":"active"})).unwrap();
        let bytes = gridcraft_engine::io::base64_decode(data["base64"].as_str().unwrap()).unwrap();
        // Prefer the pinned OFL build input so this test does not depend on a system font.
        let chinese = std::env::var_os("CRAFT_FONTS_DIR")
            .map(|dir| {
                let path = std::path::PathBuf::from(dir).join("fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf");
                (Arc::new(std::fs::read(path).unwrap()) as FontData, 0)
            })
            .or_else(|| crate::theme::chinese_preview_font().map(|f| (Arc::new(f.font) as FontData, f.index)));
        let Some(chinese) = chinese else {
            eprintln!("CJK preview regression skipped: no craft-fonts input or installed Chinese face");
            return;
        };
        let bytes = Arc::new(bytes);
        assert!(render_with_font(bytes.clone(), 0, None).unwrap_err().contains("Chinese font"));
        let image = render_with_font(bytes, 0, Some(chinese)).unwrap();
        assert!(
            image.pixels.iter().filter(|p| p.r() > 150 && p.g() < 100 && p.b() < 100).count() > 20,
            "the Chinese text from upstream #1912 must not disappear in the preview"
        );
    }

    #[test]
    fn invalid_preview_returns_an_error() {
        assert!(render(Arc::new(b"not a PDF".to_vec()), 0).is_err());
    }
}
