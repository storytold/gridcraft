//! Host save-picker/download bridge for chart image export.
use crate::SheetApp;
use serde_json::{Value as Json, json};

impl SheetApp {
    /// Returns false when a native save picker is canceled, keeping the export dialog open.
    pub(crate) fn save_chart_image(&mut self, mut params: Json) -> Result<bool, String> {
        let format = params.get("format").and_then(Json::as_str).unwrap_or("png").to_string();
        if let Some(pick) = &self.services.pick_save {
            let Some(path) = pick(&format!("Chart.{format}")) else { return Ok(false) };
            params["path"] = json!(std::path::Path::new(&path).with_extension(&format).to_string_lossy());
        } else if self.services.download.is_none() {
            return Err("Image export needs a save picker or download service".into());
        }
        let result = self.run("chart.export", params)?;
        if result.get("path").is_none()
            && let Some(download) = &self.services.download
            && let Some(bytes) = result.get("base64").and_then(Json::as_str).and_then(gridcraft_engine::io::base64_decode)
        {
            download(&format!("Chart.{format}"), &bytes);
        }
        Ok(true)
    }
}
