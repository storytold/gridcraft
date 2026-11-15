//! Local chart appearance templates and per-kind defaults.
use crate::SheetApp;
use serde_json::json;

/// Saves visual styling without data or trendlines. Capture base series colors in the existing
/// format overrides, since palettes and the series-color picker store those outside `ChartFormat`.
pub(crate) fn save_template(app: &mut SheetApp, chart_id: u32, name: &str) -> Result<(), String> {
    let chart =
        app.selected_chart_snapshot().filter(|chart| chart.id == chart_id).ok_or_else(|| "The selected chart is no longer available.".to_string())?;
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err("Enter a template name between 1 and 80 characters.".into());
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
        format,
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
