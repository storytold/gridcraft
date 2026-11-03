//! Route Home font commands to the selected chart text role.

use super::{Json, SheetApp, json};

impl SheetApp {
    fn chart_font_target(&self) -> Option<(u32, &'static str)> {
        use crate::chartview::ChartElement;
        let id = self.selected_chart()?;
        self.session.active()?.wb.active()?.charts.iter().find(|c| c.id == id)?;
        let element = self.chart_context.as_ref().map(|ctx| crate::chartview::selected_element(ctx, id)).unwrap_or_default();
        Some((
            id,
            match element {
                ChartElement::Title => "title",
                ChartElement::XAxis => "x_axis",
                ChartElement::YAxis => "y_axis",
                ChartElement::XAxisTitle => "x_axis_title",
                ChartElement::YAxisTitle => "y_axis_title",
                ChartElement::Legend => "legend",
                ChartElement::Series(_) | ChartElement::DataLabels => "data_labels",
                ChartElement::TrendlineText => "trendline",
                _ => "chart",
            },
        ))
    }

    /// The rendered default size for a chart text role, shared with Home's size controls.
    fn chart_font_default_size(role: &str) -> f32 {
        match role {
            "title" => 14.0,
            "x_axis_title" | "y_axis_title" => 10.0,
            _ => 9.0,
        }
    }

    pub(crate) fn selected_chart_font(&self) -> Option<gridcraft_engine::model::ChartTextFormat> {
        let (id, role) = self.chart_font_target()?;
        let doc = self.session.active()?;
        let mut style = doc.wb.active()?.charts.iter().find(|c| c.id == id)?.format.text_style(role);
        style.size.get_or_insert(Self::chart_font_default_size(role));
        style.name.get_or_insert(doc.wb.theme.minor_font.clone());
        Some(style)
    }

    pub(super) fn format_chart_font(&mut self, command: &str, p: &Json) -> Option<Result<Json, String>> {
        if !matches!(
            command,
            "home.fontName"
                | "home.fontSize"
                | "home.fontColor"
                | "home.bold"
                | "home.italic"
                | "home.underline"
                | "home.strikethrough"
                | "home.increaseFontSize"
                | "home.decreaseFontSize"
        ) {
            return None;
        }
        if ["range", "cell", "sheet"].iter().any(|key| p.get(*key).is_some_and(|value| !value.is_null())) {
            return None;
        }
        let (id, role) = self.chart_font_target()?;
        let chart = self.session.active()?.wb.active()?.charts.iter().find(|c| c.id == id)?;
        let mut format = chart.format.clone();
        let current = format.text_style(role);
        let style = format.text.entry(role.into()).or_default();
        match command {
            "home.fontName" => style.name = p["name"].as_str().map(str::to_string),
            "home.fontSize" => {
                let Some(n) = p["size"].as_f64() else {
                    return Some(Err("Missing chart font size".into()));
                };
                if !n.is_finite() || n <= 0.0 {
                    return Some(Err("Chart font size must be a finite positive number".into()));
                }
                style.size = Some(n.clamp(1.0, 144.0) as f32);
            }
            "home.fontColor" => {
                let color = p["color"].as_str().unwrap_or("auto");
                style.color = if matches!(color, "auto" | "none") {
                    None
                } else {
                    let Some(rgb) = gridcraft_engine::model::Color::from_hex(color).and_then(|c| c.resolve(&Default::default())) else {
                        return Some(Err("Invalid chart font color".into()));
                    };
                    Some([rgb[0], rgb[1], rgb[2], 255])
                };
            }
            "home.bold" => style.bold = Some(p["on"].as_bool().unwrap_or(!current.bold.unwrap_or(false))),
            "home.italic" => style.italic = Some(p["on"].as_bool().unwrap_or(!current.italic.unwrap_or(false))),
            "home.increaseFontSize" | "home.decreaseFontSize" => {
                style.size = Some(
                    (current.size.unwrap_or(Self::chart_font_default_size(role)) + if command == "home.increaseFontSize" { 1.0 } else { -1.0 })
                        .clamp(1.0, 144.0),
                )
            }
            _ => return Some(Err("This text decoration is not supported for charts yet.".into())),
        }
        Some(self.run_command("chart.set", json!({"chart":id,"format":format})))
    }
}
