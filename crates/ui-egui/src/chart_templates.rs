//! Local chart appearance presets, independent of the element editor and image exporter.

use crate::SheetApp;
use gridcraft_engine::model::{Chart, ChartFormat, ChartKind, LegendPos};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

const MAX_TEMPLATES: usize = 256;

/// A presentation snapshot stored with UI preferences, without source ranges or chart identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartTemplate {
    /// User-facing name, unique ignoring case within a chart kind.
    pub name: String,
    /// Applying to another kind is refused rather than reinterpreting its settings.
    pub kind: ChartKind,
    /// Native chart style and legend placement accompany the detailed format overrides.
    pub style: u32,
    pub legend: LegendPos,
    pub data_labels: bool,
    pub gridlines: bool,
    pub format: ChartFormat,
    /// Older saved templates omit these fields and keep the destination's geometry defaults.
    #[serde(default)]
    pub hole_size: Option<u8>,
    #[serde(default)]
    pub first_slice_angle: Option<u16>,
    /// At most one saved template per kind is used by subsequent UI insertions.
    #[serde(default)]
    pub use_for_new: bool,
}

/// Drops malformed entries individually so they cannot discard unrelated saved UI preferences.
pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<ChartTemplate>, D::Error> {
    let value = Json::deserialize(d)?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX_TEMPLATES)
        .filter_map(|v| serde_json::from_value::<ChartTemplate>(v.clone()).ok())
        .filter(|t| !t.name.trim().is_empty() && t.name.chars().take(81).count() <= 80 && (1..=16).contains(&t.style))
        .collect())
}

/// Resolves the addressed chart in the active worksheet without requiring UI selection.
pub(crate) fn snapshot(app: &SheetApp, id: u32) -> Option<Chart> {
    app.session.active()?.wb.active()?.charts.iter().find(|chart| chart.id == id).cloned()
}

/// Saves formatting and explicit series colors without changing its source chart or workbook.
pub(crate) fn save_template(app: &mut SheetApp, chart_id: u32, name: &str) -> Result<(), String> {
    let chart = snapshot(app, chart_id).ok_or("The chart is no longer available.")?;
    let name = name.trim();
    if name.is_empty() || name.chars().take(81).count() > 80 {
        return Err("Enter a template name between 1 and 80 characters.".into());
    }
    let existing = app.ui.chart_templates.iter().position(|t| t.kind == chart.kind && t.name.to_lowercase() == name.to_lowercase());
    if existing.is_none() && app.ui.chart_templates.len() >= MAX_TEMPLATES {
        return Err("Delete a chart template before saving another one (maximum 256).".into());
    }
    let theme = app.session.doc().map_err(|error| error.to_string())?.wb.theme.clone();
    let mut format = chart.format;
    for (index, series) in chart.series.iter().take(256).enumerate() {
        if let Some([r, g, b]) = series.color.and_then(|color| color.resolve(&theme)) {
            let style = format.series.entry(index).or_default();
            let color = [r, g, b, 255];
            style.fill_color.get_or_insert(color);
            style.line_color.get_or_insert(color);
            style.marker_fill.get_or_insert(color);
        }
    }
    for template in &mut app.ui.chart_templates {
        if template.kind == chart.kind {
            template.use_for_new = false;
        }
    }
    let template = ChartTemplate {
        name: name.to_string(),
        kind: chart.kind,
        style: chart.style,
        legend: chart.legend,
        data_labels: chart.data_labels,
        gridlines: chart.gridlines,
        format,
        hole_size: Some(chart.hole_size),
        first_slice_angle: Some(chart.first_slice_angle),
        use_for_new: true,
    };
    if let Some(index) = existing {
        if let Some(saved) = app.ui.chart_templates.get_mut(index) {
            *saved = template;
        }
    } else {
        app.ui.chart_templates.push(template);
    }
    Ok(())
}

/// Selects one default for the template's kind, or disables that kind's default.
fn set_default(app: &mut SheetApp, index: usize, enabled: bool) -> Result<(), String> {
    let kind = app.ui.chart_templates.get(index).ok_or("No such chart template.")?.kind;
    for (i, template) in app.ui.chart_templates.iter_mut().enumerate() {
        if template.kind == kind {
            template.use_for_new = enabled && i == index;
        }
    }
    Ok(())
}

impl ChartTemplate {
    /// Command parameters contain appearance only; data, series names and object identity stay put.
    fn params(&self) -> Json {
        let legend = match self.legend {
            LegendPos::None => "none",
            LegendPos::Top => "top",
            LegendPos::Left => "left",
            LegendPos::Bottom => "bottom",
            LegendPos::Right => "right",
        };
        let mut params = json!({"style":self.style,"legend":legend,"dataLabels":self.data_labels,"gridlines":self.gridlines,"format":self.format});
        if let Some(hole) = self.hole_size {
            params["holeSize"] = json!(hole);
        }
        if let Some(angle) = self.first_slice_angle {
            params["firstSliceAngle"] = json!(angle);
        }
        params
    }
}

/// Expands saved UI defaults into one engine insertion so undo and journal replay agree.
pub(crate) fn insert(app: &mut SheetApp, params: Json) -> Result<Json, String> {
    let template = if app.ui.chart_templates.iter().any(|t| t.use_for_new) {
        gridcraft_engine::cmd::insert::build_chart(&app.session, &params)
            .ok()
            .and_then(|(_, chart)| app.ui.chart_templates.iter().find(|t| t.use_for_new && t.kind == chart.kind).cloned())
    } else {
        None
    };
    let Some(template) = template else { return app.session.run("insert.chart", params) };
    let mut resolved = params.clone();
    if !resolved.is_object() {
        resolved = json!({});
    }
    if let (Some(map), Json::Object(defaults)) = (resolved.as_object_mut(), template.params()) {
        for (key, value) in defaults {
            map.entry(key).or_insert(value);
        }
    }
    match app.session.run("insert.chart", resolved) {
        Err(error) => {
            // The engine validates before mutation. A bad saved preference reports the error,
            // then falls back to one ordinary insertion, preserving capped undo history.
            app.message = Some(("Chart Template".into(), crate::clean_error(&error)));
            app.session.run("insert.chart", params)
        }
        result => result,
    }
}

/// UI-only commands for local preferences; applying a preset uses the engine's undoable chart.set.
pub(crate) fn command(app: &mut SheetApp, id: &str, params: &Json) -> Option<Result<Json, String>> {
    if !matches!(id, "chart.saveTemplate" | "chart.applyTemplate" | "chart.defaultTemplate" | "chart.deleteTemplate") {
        return None;
    }
    Some(run(app, id, params))
}

/// Resolves explicit object/template identifiers before updating preferences or the workbook.
fn run(app: &mut SheetApp, id: &str, params: &Json) -> Result<Json, String> {
    if id == "chart.saveTemplate" {
        if params.get("target").is_some_and(|target| *target != json!(app.view_key())) {
            return Err("Return to the original worksheet to save this chart template.".into());
        }
        let chart = params.get("chart").and_then(Json::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or("Specify a chart id.")?;
        let name = params.get("name").and_then(Json::as_str).ok_or("Specify a template name.")?;
        save_template(app, chart, name)?;
        return Ok(json!({"templates":app.ui.chart_templates.len()}));
    }
    let index = params.get("template").and_then(Json::as_u64).and_then(|v| usize::try_from(v).ok()).ok_or("Specify a template index.")?;
    let template = app.ui.chart_templates.get(index).ok_or("No such chart template.")?.clone();
    match id {
        "chart.deleteTemplate" => {
            app.ui.chart_templates.remove(index);
            Ok(Json::Null)
        }
        "chart.defaultTemplate" => {
            set_default(app, index, params.get("on").and_then(Json::as_bool).unwrap_or(true))?;
            Ok(Json::Null)
        }
        _ => {
            let chart = params.get("chart").and_then(Json::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or("Specify a chart id.")?;
            if snapshot(app, chart).is_none_or(|chart| chart.kind != template.kind) {
                return Err("Choose a chart of the same type as this template.".into());
            }
            let mut appearance = template.params();
            appearance["chart"] = json!(chart);
            app.run("chart.set", appearance)
        }
    }
}

/// Context-menu entry points that work without the separate chart element editor.
pub(crate) fn menu(app: &mut SheetApp, ui: &mut egui::Ui, id: u32) -> bool {
    let lang = crate::i18n::current(ui.ctx());
    let mut close = false;
    if ui.button(lang.tr("Save as Chart Template…")).clicked() {
        app.open_dialog("saveChartTemplate", json!({"chart":id}));
        close = true;
    }
    let kind = snapshot(app, id).map(|chart| chart.kind);
    let templates: Vec<_> =
        app.ui.chart_templates.iter().enumerate().filter(|(_, t)| Some(t.kind) == kind).map(|(i, t)| (i, t.name.clone(), t.use_for_new)).collect();
    if !templates.is_empty() {
        ui.menu_button(lang.tr("Chart Templates"), |ui| {
            ui.set_max_width(320.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (index, name, default) in templates {
                    let deleted = ui
                        .push_id(index, |ui| {
                            ui.add(egui::Label::new(&name).truncate()).on_hover_text(&name);
                            if ui.button(lang.tr("Apply")).clicked() {
                                app.run_or_alert("chart.applyTemplate", json!({"chart":id,"template":index}));
                                close = true;
                                ui.close();
                            }
                            let mut enabled = default;
                            if ui.checkbox(&mut enabled, lang.tr("Use for new charts")).changed() {
                                app.run_or_alert("chart.defaultTemplate", json!({"template":index,"on":enabled}));
                            }
                            let deleted = ui.button(lang.tr("Delete Template")).clicked();
                            if deleted {
                                app.run_or_alert("chart.deleteTemplate", json!({"template":index}));
                                close = true;
                                ui.close();
                            }
                            deleted
                        })
                        .inner;
                    if deleted {
                        break;
                    }
                    ui.separator();
                }
            });
        });
    }
    ui.separator();
    close
}
