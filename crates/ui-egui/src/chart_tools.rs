//! Chart-specific menus, image export and host clipboard bridge.
use crate::{SheetApp, chartview::ChartElement};
use gridcraft_engine::model::Chart;
use serde_json::{Value as Json, json};
use std::hash::{Hash, Hasher};

pub(crate) struct ChartClipboard {
    snapshot: Chart,
    document: u64,
    fingerprint: u64,
}

fn fingerprint(image: &egui::ColorImage) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    image.size.hash(&mut h);
    image.pixels.hash(&mut h);
    h.finish()
}

impl SheetApp {
    pub(crate) fn selected_chart_snapshot(&self) -> Option<Chart> {
        let id = self.selected_chart?;
        self.session.active()?.wb.active()?.charts.iter().find(|c| c.id == id).cloned()
    }

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
                    self.selected_chart = None;
                }
            }
            Err(e) => self.alert(e),
        }
    }

    /// Only reuse an editable snapshot if the OS clipboard still owns that exact image,
    /// and the source workbook is still current. Otherwise paste the actual image.
    pub(crate) fn paste_chart_image(&mut self) -> bool {
        let Some(image) = self.services.paste_image.as_mut().and_then(|read| read()) else { return false };
        let doc = self.session.active().map(|d| d.uid);
        if let Some(copy) = &self.chart_clipboard
            && doc == Some(copy.document)
            && fingerprint(&image) == copy.fingerprint
        {
            let snapshot = copy.snapshot.clone();
            self.run_or_alert("chart.paste", json!({"snapshot":snapshot}));
        } else {
            self.chart_clipboard = None;
            let mut bytes = Vec::new();
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_srgba_unmultiplied()).collect();
            let img = image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, rgba);
            let Some(img) = img else { return false };
            match image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png) {
                Ok(()) => self.run_or_alert(
                    "insert.picture",
                    json!({"base64":gridcraft_engine::io::base64_encode(&bytes),"mime":"image/png",
                    "width":image.size[0] as f32/2.0,"height":image.size[1] as f32/2.0}),
                ),
                Err(e) => self.alert(e.to_string()),
            }
        }
        true
    }

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

/// The same element hierarchy is used by the ribbon and chart context menu.
pub(crate) fn element_menu(app: &mut SheetApp, ui: &mut egui::Ui, id: u32) {
    let Some(chart) = app.selected_chart_snapshot().filter(|c| c.id == id) else { return };
    let lang = app.ui.language;
    if ui.button(lang.tr("Chart Title…")).clicked() {
        app.open_dialog("chartTitle", json!({"chart":id}));
        ui.close();
    }
    ui.menu_button(lang.tr("Axis Titles"), |ui| {
        for (label, key, element) in
            [("Primary Horizontal", "xTitle", ChartElement::XAxisTitle), ("Primary Vertical", "yTitle", ChartElement::YAxisTitle)]
        {
            let on = if key == "xTitle" { chart.x_title.is_some() } else { chart.y_title.is_some() };
            if ui.selectable_label(on, lang.tr(label)).clicked() {
                let value = if on { Json::Null } else { json!(if key == "xTitle" { "Horizontal Axis" } else { "Vertical Axis" }) };
                let mut p = json!({"chart":id});
                p[key] = value;
                app.run_or_alert("chart.set", p);
                crate::chartview::select_element(ui.ctx(), id, element);
                ui.close();
            }
        }
    });
    if ui.selectable_label(chart.data_labels, lang.tr("Data Labels")).clicked() {
        app.run_or_alert("chart.set", json!({"chart":id,"dataLabels":!chart.data_labels}));
        ui.close();
    }
    ui.menu_button(lang.tr("Gridlines"), |ui| {
        if ui.selectable_label(chart.gridlines, lang.tr("Primary Horizontal")).clicked() {
            app.run_or_alert("chart.set", json!({"chart":id,"gridlines":!chart.gridlines}));
            ui.close();
        }
        if ui.selectable_label(chart.format.vertical_gridlines, lang.tr("Primary Vertical")).clicked() {
            let mut f = chart.format.clone();
            f.vertical_gridlines = !f.vertical_gridlines;
            app.run_or_alert("chart.set", json!({"chart":id,"format":f}));
            ui.close();
        }
    });
    ui.menu_button(lang.tr("Legend"),|ui| {
        for (label,pos) in [("None","none"),("Bottom","bottom"),("Top","top"),("Left","left"),("Right","right")] {
            if ui.button(lang.tr(label)).clicked() {app.run_or_alert("chart.set",json!({"chart":id,"legend":pos}));ui.close();}
        }
        if ui.selectable_label(chart.format.legend_overlay,lang.tr("Overlay Legend")).clicked() {
            let mut f=chart.format.clone();f.legend_overlay = !f.legend_overlay;
            app.run_or_alert("chart.set",json!({"chart":id,"legend":if chart.legend==gridcraft_engine::model::LegendPos::None {"right"} else {
                match chart.legend {gridcraft_engine::model::LegendPos::Top=>"top",gridcraft_engine::model::LegendPos::Left=>"left",gridcraft_engine::model::LegendPos::Bottom=>"bottom",_=>"right"}
            },"format":f}));ui.close();
        }
    });
    if gridcraft_chart::supports_trendline(chart.kind) && ui.button(lang.tr("Trendline…")).clicked() {
        crate::chartview::select_element(ui.ctx(), id, ChartElement::TrendlineText);
        app.grid.pane = Some("formatChart".into());
        ui.close();
    }
}

pub(crate) fn context_menu(app: &mut SheetApp, ui: &mut egui::Ui, id: u32) -> bool {
    let lang = app.ui.language;
    let mut close = false;
    for (label, action) in [("Cut", 0), ("Copy", 1), ("Paste", 2), ("Duplicate Chart", 3), ("Copy as Picture", 4), ("Save as Picture…", 5)] {
        if ui.button(lang.tr(label)).clicked() {
            close = true;
            match action {
                0 => app.copy_chart(ui.ctx(), true, false),
                1 => app.copy_chart(ui.ctx(), false, false),
                2 => app.paste_from_clipboard(),
                3 => app.run_or_alert("chart.duplicate", json!({"chart":id})),
                4 => app.copy_chart(ui.ctx(), false, true),
                _ => app.open_dialog("chartExport", json!({"chart":id})),
            }
        }
    }
    if ui.button(lang.tr("Save as Chart Template…")).clicked() {
        app.open_dialog("saveChartTemplate", json!({"chart":id}));
        close = true;
    }
    let chart_kind = app.selected_chart_snapshot().filter(|chart| chart.id == id).map(|chart| chart.kind);
    let templates: Vec<_> = app
        .ui
        .chart_templates
        .iter()
        .enumerate()
        .filter(|(_, template)| Some(template.kind) == chart_kind)
        .map(|(i, t)| (i, t.name.clone(), t.use_for_new))
        .collect();
    if !templates.is_empty() {
        ui.menu_button(lang.tr("Chart Templates"), |ui| {
            for (index, name, use_for_new) in templates {
                if ui.button(format!("{}: {}", lang.tr("Apply"), name)).clicked() {
                    apply_template(app, id, index);
                    close = true;
                    ui.close();
                }
                let mut enabled = use_for_new;
                if ui.checkbox(&mut enabled, format!("{}: {}", lang.tr("Use for new charts"), name)).changed() {
                    set_default_template(app, index, enabled);
                }
            }
        });
    }
    ui.separator();
    if ui.button(lang.tr("Select Data…")).clicked() {
        app.open_dialog("selectData", json!({"chart":id}));
        close = true;
    }
    ui.menu_button(lang.tr("Change Chart Type"), |ui| {
        for (label, kind) in [
            ("Column", "column"),
            ("Bar", "bar"),
            ("Line", "line"),
            ("Scatter", "scatter"),
            ("Area", "area"),
            ("Pie", "pie"),
            ("Doughnut", "doughnut"),
        ] {
            if ui.button(lang.tr(label)).clicked() {
                app.run_or_alert("chart.set", json!({"chart":id,"type":kind}));
                close = true;
                ui.close();
            }
        }
    });
    ui.menu_button(lang.tr("Add Chart Element"), |ui| element_menu(app, ui, id));
    ui.separator();
    if ui.button(lang.tr("Format Selection…")).clicked() {
        app.grid.pane = Some("formatChart".into());
        close = true;
    }
    if ui.button(lang.tr("Delete Chart")).clicked() {
        app.run_or_alert("chart.delete", json!({"chart":id}));
        app.selected_chart = None;
        close = true;
    }
    close
}

pub(crate) fn save_template(app: &mut SheetApp, chart_id: u32, name: &str) -> Result<(), String> {
    let chart =
        app.selected_chart_snapshot().filter(|chart| chart.id == chart_id).ok_or_else(|| "The selected chart is no longer available.".to_string())?;
    let name = name.trim();
    if name.is_empty() || name.len() > 80 {
        return Err("Enter a template name between 1 and 80 characters.".into());
    }
    let kind = chart.kind;
    for template in &mut app.ui.chart_templates {
        if template.kind == kind {
            template.use_for_new = false;
        }
    }
    app.ui.chart_templates.retain(|template| !(template.kind == kind && template.name.eq_ignore_ascii_case(name)));
    app.ui.chart_templates.push(crate::ChartTemplate {
        name: name.to_string(),
        kind,
        style: chart.style,
        legend: chart.legend,
        data_labels: chart.data_labels,
        gridlines: chart.gridlines,
        format: chart.format,
        use_for_new: true,
    });
    Ok(())
}

pub(crate) fn set_default_template(app: &mut SheetApp, index: usize, enabled: bool) {
    let Some(kind) = app.ui.chart_templates.get(index).map(|template| template.kind) else { return };
    for (i, template) in app.ui.chart_templates.iter_mut().enumerate() {
        if template.kind == kind {
            template.use_for_new = enabled && i == index;
        }
    }
}

pub(crate) fn apply_template(app: &mut SheetApp, chart_id: u32, index: usize) {
    let Some(template) = app
        .ui
        .chart_templates
        .get(index)
        .filter(|template| app.selected_chart_snapshot().is_some_and(|chart| chart.id == chart_id && chart.kind == template.kind))
        .cloned()
    else {
        return;
    };
    app.run_or_alert(
        "chart.set",
        json!({
            "chart": chart_id,
            "style": template.style,
            "legend": match template.legend {
                gridcraft_engine::model::LegendPos::None => "none",
                gridcraft_engine::model::LegendPos::Top => "top",
                gridcraft_engine::model::LegendPos::Left => "left",
                gridcraft_engine::model::LegendPos::Bottom => "bottom",
                gridcraft_engine::model::LegendPos::Right => "right",
            },
            "dataLabels": template.data_labels,
            "gridlines": template.gridlines,
            "format": template.format,
        }),
    );
}
