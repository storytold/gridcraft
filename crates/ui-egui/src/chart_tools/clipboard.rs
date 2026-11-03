//! Editable chart snapshots and the host image clipboard bridge.
use crate::SheetApp;
use gridcraft_engine::model::Chart;
use serde_json::{Value as Json, json};
use std::hash::{Hash, Hasher};

pub(crate) struct ChartClipboard {
    pub(super) snapshot: Chart,
    pub(super) document: u64,
    pub(super) fingerprint: u64,
}

pub(super) fn fingerprint(image: &egui::ColorImage) -> u64 {
    fingerprint_rgba(image.size, image.pixels.iter().flat_map(|pixel| pixel.to_srgba_unmultiplied()))
}

fn fingerprint_rgba(size: [usize; 2], pixels: impl Iterator<Item = u8>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    size.hash(&mut h);
    for channel in pixels {
        channel.hash(&mut h);
    }
    h.finish()
}

impl SheetApp {
    pub(crate) fn copy_chart(&mut self, ctx: &egui::Context, cut: bool, as_picture: bool) {
        let Some(chart) = self.selected_chart_snapshot() else { return };
        let document = self.session.active().map(|d| d.uid).unwrap_or(0);
        let image = (|| -> Result<egui::ColorImage, String> {
            let result = self.run("chart.export", json!({"chart":chart.id,"format":"png","scale":2}))?;
            let bytes = result.get("base64").and_then(Json::as_str).and_then(gridcraft_engine::io::base64_decode).ok_or("Missing chart image")?;
            let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgba8();
            Ok(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
        })();
        match image {
            Ok(image) => {
                self.chart_clipboard = (!as_picture).then(|| ChartClipboard { snapshot: chart.clone(), document, fingerprint: fingerprint(&image) });
                self.cut_text = None;
                self.clipboard_text_at_copy = None;
                ctx.copy_image(image);
                if cut {
                    self.run_or_alert("chart.delete", json!({"chart":chart.id}));
                    self.selected_object = None;
                }
            }
            Err(e) => self.alert(e),
        }
    }

    /// Only reuse an editable snapshot if the OS clipboard still owns that exact image,
    /// and the source workbook is still current. Otherwise paste the actual image.
    pub(crate) fn paste_chart_image(&mut self) -> bool {
        let Some(image) = self.services.paste_image.as_mut().and_then(|read| read()) else { return false };
        let (width, height) = image.dimensions();
        if let Err(e) = crate::app::check_picture_size(width, height) {
            self.alert(e);
            return true;
        }
        let doc = self.session.active().map(|d| d.uid);
        let fingerprint = fingerprint_rgba([width as usize, height as usize], image.as_raw().iter().copied());
        if let Some(copy) = &self.chart_clipboard
            && doc == Some(copy.document)
            && fingerprint == copy.fingerprint
        {
            let snapshot = copy.snapshot.clone();
            self.run_or_alert("chart.paste", json!({"snapshot":snapshot}));
        } else {
            self.chart_clipboard = None;
            let mut bytes = Vec::new();
            match image::DynamicImage::ImageRgba8(image).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png) {
                Ok(()) => self.run_or_alert("insert.picture", json!({"base64":gridcraft_engine::io::base64_encode(&bytes),"mime":"image/png"})),
                Err(e) => self.alert(e.to_string()),
            }
        }
        true
    }
}
