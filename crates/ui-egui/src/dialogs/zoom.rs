use super::show::ok_cancel;
use super::*;

/// Zoom's Magnification presets, as `preset` values and labels, in Excel's order.
const ZOOM_PRESETS: [(&str, &str); 5] = [("200", "200%"), ("100", "100%"), ("75", "75%"), ("50", "50%"), ("25", "25%")];

/// The `preset` that shows a zoom percentage: its Magnification entry, or Custom.
pub(super) fn zoom_preset(percent: f64) -> &'static str {
    ZOOM_PRESETS.iter().map(|(val, _)| *val).find(|val| val.parse::<f64>().ok() == Some(percent)).unwrap_or("custom")
}

/// View › Zoom, laid out as Excel's: one Magnification column of 200% to 25%, Fit selection and
/// Custom with its percentage box. The current zoom is pre-selected; typing a percentage
/// selects Custom; Fit selection zooms so the selected cells fill the grid.
pub(super) fn zoom(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let cur = d.values.get("preset").and_then(Json::as_str).unwrap_or("custom").to_string();
    ui.label(egui::RichText::new(lang.tr("Magnification")).strong());
    for (val, text) in ZOOM_PRESETS {
        if ui.radio(cur == val, text).clicked() {
            d.set_value("preset", json!(val));
        }
    }
    if ui.radio(cur == "fit", lang.tr("Fit selection")).clicked() {
        d.set_value("preset", json!("fit"));
    }
    ui.horizontal(|ui| {
        if ui.radio(cur == "custom", format!("{}:", lang.tr("Custom"))).clicked() {
            d.set_value("preset", json!("custom"));
        }
        // Whole percentages, as Excel shows them; the engine clamps to 10..400 as well.
        let mut percent = d.values.get("percent").and_then(Json::as_f64).unwrap_or(100.0).round() as i64;
        if ui.add(egui::DragValue::new(&mut percent).range(10..=400).speed(1.0)).changed() {
            d.set_value("percent", json!(percent));
        }
        ui.label("%");
    });
    ok_cancel(ui, confirm, open);
}
