//! Rasterize the actual print PDF rather than maintaining a second, incomplete sheet painter.
use std::sync::Arc;

pub fn render(bytes: Arc<Vec<u8>>, index: usize) -> Result<egui::ColorImage, String> {
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
        let pixmap = hayro::render(page, &hayro::RenderCache::new(), &Default::default(), &settings);
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
    fn invalid_preview_returns_an_error() {
        assert!(render(Arc::new(b"not a PDF".to_vec()), 0).is_err());
    }
}
