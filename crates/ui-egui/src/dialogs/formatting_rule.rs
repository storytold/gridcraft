use super::show::{ok_cancel, track_pick};
use super::*;

/// The six rule types in New Formatting Rule, with the rule `type` each starts from.
const CF_RULE_KINDS: [(&str, &str); 6] = [
    ("colorScale", "Format all cells based on their values"),
    ("cellIs", "Format only cells that contain"),
    ("top10", "Format only top or bottom ranked values"),
    ("aboveAverage", "Format only values that are above or below average"),
    ("duplicate", "Format only unique or duplicate values"),
    ("expression", "Use a formula to determine which cells to format"),
];

const CF_PRESETS: [(&str, &str); 6] = [
    ("lightRedFillDarkRedText", "Light Red Fill with Dark Red Text"),
    ("yellowFillDarkYellowText", "Yellow Fill with Dark Yellow Text"),
    ("greenFillDarkGreenText", "Green Fill with Dark Green Text"),
    ("lightRedFill", "Light Red Fill"),
    ("redText", "Red Text"),
    ("redBorder", "Red Border"),
];

const CF_OPERATORS: [(&str, &str); 8] = [
    ("between", "CF|between"),
    ("notBetween", "CF|not between"),
    ("equal", "CF|equal to"),
    ("notEqual", "CF|not equal to"),
    ("greater", "CF|greater than"),
    ("less", "CF|less than"),
    ("greaterOrEqual", "CF|greater than or equal to"),
    ("lessOrEqual", "CF|less than or equal to"),
];

const CF_PERIODS: [(&str, &str); 10] = [
    ("yesterday", "Yesterday"),
    ("today", "Today"),
    ("tomorrow", "Tomorrow"),
    ("last7Days", "In the last 7 days"),
    ("lastWeek", "Last week"),
    ("thisWeek", "This week"),
    ("nextWeek", "Next week"),
    ("lastMonth", "Last month"),
    ("thisMonth", "This month"),
    ("nextMonth", "Next month"),
];

const CF_ICON_SETS: [(&str, &str); 6] = [
    ("3Arrows", "3 Arrows"),
    ("3TrafficLights1", "3 Traffic Lights"),
    ("3Symbols", "3 Symbols"),
    ("4Rating", "4 Ratings"),
    ("5Arrows", "5 Arrows"),
    ("5Quarters", "5 Quarters"),
];

/// Which of [`CF_RULE_KINDS`] a rule `type` belongs to.
fn cf_kind_of(ty: &str) -> usize {
    match ty {
        "colorScale" | "dataBar" | "iconSet" => 0,
        "top10" => 2,
        "aboveAverage" => 3,
        "duplicate" | "unique" => 4,
        "expression" => 5,
        _ => 1,
    }
}

fn cf_str(rule: &Map<String, Json>, key: &str) -> String {
    match rule.get(key) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Null) | None => String::new(),
        Some(v) => v.to_string(),
    }
}

fn cf_flag(rule: &Map<String, Json>, key: &str) -> bool {
    rule.get(key).and_then(Json::as_bool).unwrap_or(false)
}

fn hex_color(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    let v = u32::from_str_radix(h.get(..6)?, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Sets a rule's plain format overrides to a Highlight Cells preset.
pub(super) fn cf_apply_preset(rule: &mut Map<String, Json>, preset: &str, theme: &gridcraft_engine::model::Theme) {
    use gridcraft_engine::model::{BorderStyle, PatternType, Underline};
    let st = gridcraft_engine::cmd::insert::preset_style(preset);
    let hex = |c: gridcraft_engine::model::Color| c.hex(theme);
    rule.insert("fill".into(), json!(if st.fill.pattern == PatternType::None { None } else { hex(st.fill.fg) }.unwrap_or_else(|| "none".into())));
    rule.insert("fontColor".into(), json!(hex(st.font.color).unwrap_or_else(|| "auto".into())));
    let border = (st.borders.left.style != BorderStyle::None).then(|| hex(st.borders.left.color)).flatten();
    rule.insert("border".into(), json!(border.unwrap_or_else(|| "none".into())));
    rule.insert("bold".into(), json!(st.font.bold));
    rule.insert("italic".into(), json!(st.font.italic));
    rule.insert("underline".into(), json!(st.font.underline != Underline::None));
    rule.insert("strike".into(), json!(st.font.strike));
    rule.remove("preset");
    rule.remove("style");
}

/// A combo box over `(value, label)` pairs for `rule[key]`; true when the user picked one.
fn cf_combo(ui: &mut egui::Ui, id: &str, rule: &mut Map<String, Json>, key: &str, options: &[(&str, &str)], width: f32) -> bool {
    let lang = crate::i18n::current(ui.ctx());
    let current = cf_str(rule, key);
    let label = options.iter().find(|(v, _)| *v == current).or(options.first()).map(|(_, l)| lang.tr(l)).unwrap_or_default();
    let mut picked = None;
    egui::ComboBox::from_id_salt(("cfRule", id)).width(width).selected_text(label).show_ui(ui, |ui| {
        for (v, l) in options {
            if ui.selectable_label(*v == current, lang.tr(l)).clicked() {
                picked = Some(*v);
            }
        }
    });
    if let Some(v) = picked {
        rule.insert(key.into(), json!(v));
    }
    picked.is_some()
}

fn cf_text(ui: &mut egui::Ui, rule: &mut Map<String, Json>, key: &str, width: f32) {
    let mut text = cf_str(rule, key);
    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(width)).changed() {
        rule.insert(key.into(), json!(text));
    }
}

/// A swatch and a menu with our colour palette; returns the picked hex or `none`.
fn cf_color_button(ui: &mut egui::Ui, label: &str, current: Option<Color32>, none_label: &str, colors: &[u32; 12]) -> Option<String> {
    let lang = crate::i18n::current(ui.ctx());
    let mut picked = None;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        let border = ui.visuals().widgets.inactive.fg_stroke.color;
        match current {
            Some(c) => ui.painter().rect_filled(r, 2.0, c),
            None => ui.painter().line_segment([r.left_bottom(), r.right_top()], Stroke::new(1.0, border)),
        };
        ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, border), StrokeKind::Inside);
        ui.menu_button(lang.tr(label), |ui| {
            if let Some(c) = crate::widgets::color_palette(ui, colors, lang.tr(none_label)) {
                picked = Some(c);
                ui.close();
            }
        });
    });
    picked
}

/// Fill, font and border of a rule's format, as a cell would show them.
fn cf_preview(ui: &mut egui::Ui, rule: &Map<String, Json>, size: egui::Vec2) {
    let lang = crate::i18n::current(ui.ctx());
    let fill = hex_color(&cf_str(rule, "fill"));
    let font = hex_color(&cf_str(rule, "fontColor"));
    let border = hex_color(&cf_str(rule, "border"));
    let plain = fill.is_none() && font.is_none() && border.is_none() && !["bold", "italic", "underline", "strike"].iter().any(|k| cf_flag(rule, k));
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 0.0, fill.unwrap_or(Color32::WHITE));
    p.rect_stroke(r, 0.0, Stroke::new(if border.is_some() { 1.5 } else { 1.0 }, border.unwrap_or(Color32::from_gray(190))), StrokeKind::Inside);
    if plain {
        p.text(r.center(), egui::Align2::CENTER_CENTER, lang.tr("No Format Set"), egui::FontId::proportional(12.0), Color32::from_gray(90));
        return;
    }
    let mut text = egui::RichText::new("AaBbCcYyZz").color(font.unwrap_or(Color32::BLACK)).size(12.5);
    if cf_flag(rule, "bold") {
        text = text.strong();
    }
    if cf_flag(rule, "italic") {
        text = text.italics();
    }
    if cf_flag(rule, "underline") {
        text = text.underline();
    }
    if cf_flag(rule, "strike") {
        text = text.strikethrough();
    }
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), r.width(), egui::FontSelection::Default);
    let pos = r.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, Color32::BLACK);
}

/// The cell format a rule applies: preset, colours, font styles and a preview.
fn cf_format_editor(ui: &mut egui::Ui, rule: &mut Map<String, Json>, theme: &gridcraft_engine::model::Theme) {
    let lang = crate::i18n::current(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(lang.tr("Preview:"));
        cf_preview(ui, rule, vec2(200.0, 26.0));
        let mut preset = Map::new();
        preset.insert("p".into(), json!("custom"));
        let mut options = vec![("custom", "Custom Format")];
        options.extend(CF_PRESETS);
        if cf_combo(ui, "preset", &mut preset, "p", &options, 220.0) {
            let p = cf_str(&preset, "p");
            if p != "custom" {
                cf_apply_preset(rule, &p, theme);
            }
        }
    });
    ui.horizontal(|ui| {
        let colors = theme.colors;
        if let Some(c) = cf_color_button(ui, "CF|Fill", hex_color(&cf_str(rule, "fill")), "No Color", &colors) {
            rule.insert("fill".into(), json!(c));
        }
        if let Some(c) = cf_color_button(ui, "Font Color", hex_color(&cf_str(rule, "fontColor")), "Automatic", &colors) {
            rule.insert("fontColor".into(), json!(if c == "none" { "auto".into() } else { c }));
        }
        if let Some(c) = cf_color_button(ui, "CF|Border", hex_color(&cf_str(rule, "border")), "No Border", &colors) {
            rule.insert("border".into(), json!(c));
        }
        for (key, label, tip) in [("bold", "B", "Bold"), ("italic", "I", "Italic"), ("underline", "U", "Underline"), ("strike", "S", "Strikethrough")]
        {
            let mut text = egui::RichText::new(label);
            text = match key {
                "bold" => text.strong(),
                "italic" => text.italics(),
                "underline" => text.underline(),
                _ => text.strikethrough(),
            };
            let on = cf_flag(rule, key);
            if ui.add(egui::Button::selectable(on, text).min_size(vec2(22.0, 20.0))).on_hover_text(lang.tr(tip)).clicked() {
                rule.insert(key.into(), json!(!on));
            }
        }
        if ui.button(lang.tr("Clear")).clicked() {
            for (k, v) in [("fill", json!("none")), ("fontColor", json!("auto")), ("border", json!("none"))] {
                rule.insert(k.into(), v);
            }
            for k in ["bold", "italic", "underline", "strike"] {
                rule.insert(k.into(), json!(false));
            }
            rule.remove("style");
        }
    });
}

/// One column of a colour scale or data bar: the threshold type, its value and colour.
fn cf_threshold(ui: &mut egui::Ui, rule: &mut Map<String, Json>, key: &str, label: &str, value_width: f32) {
    let lang = crate::i18n::current(ui.ctx());
    let ends: &[(&str, &str)] = match key {
        "min" => &[("min", "Lowest Value")],
        "max" => &[("max", "Highest Value")],
        _ => &[],
    };
    let mut options: Vec<(&str, &str)> = ends.to_vec();
    options.extend([("num", "Number"), ("percent", "Percent"), ("formula", "Formula"), ("percentile", "Percentile")]);
    let mut t = rule.get(key).and_then(Json::as_object).cloned().unwrap_or_default();
    if !t.contains_key("type") {
        let default = if key == "mid" { "percentile" } else { key };
        t.insert("type".into(), json!(default));
        if key == "mid" {
            t.insert("value".into(), json!("50"));
        }
    }
    ui.vertical(|ui| {
        ui.strong(lang.tr(label));
        cf_combo(ui, &format!("{key}Type"), &mut t, "type", &options, value_width);
        if !matches!(cf_str(&t, "type").as_str(), "min" | "max") {
            cf_text(ui, &mut t, "value", value_width);
        }
    });
    rule.insert(key.into(), Json::Object(t));
}

/// Rule type 1: colour scales, data bars and icon sets.
fn cf_values_editor(ui: &mut egui::Ui, rule: &mut Map<String, Json>, theme: &gridcraft_engine::model::Theme) {
    let lang = crate::i18n::current(ui.ctx());
    let ty = cf_str(rule, "type");
    let colors = rule.get("colors").and_then(Json::as_array).map_or(0, Vec::len);
    let mut style = Map::new();
    style.insert("s".into(), json!(if ty == "colorScale" { if colors == 2 { "2color" } else { "3color" } } else { ty.as_str() }));
    ui.horizontal(|ui| {
        ui.label(lang.tr("Format Style:"));
        let options = [("2color", "2-Color Scale"), ("3color", "3-Color Scale"), ("dataBar", "Data Bar"), ("iconSet", "Icon Sets")];
        if cf_combo(ui, "formatStyle", &mut style, "s", &options, 160.0) {
            for k in ["colors", "min", "mid", "max", "color", "set", "thresholds"] {
                rule.remove(k);
            }
            match cf_str(&style, "s").as_str() {
                "2color" => {
                    rule.insert("type".into(), json!("colorScale"));
                    rule.insert("colors".into(), json!(["#F8696B", "#63BE7B"]));
                }
                "3color" => {
                    rule.insert("type".into(), json!("colorScale"));
                    rule.insert("colors".into(), json!(["#F8696B", "#FFEB84", "#63BE7B"]));
                }
                "dataBar" => {
                    rule.insert("type".into(), json!("dataBar"));
                    rule.insert("color".into(), json!("#638EC6"));
                }
                _ => {
                    rule.insert("type".into(), json!("iconSet"));
                    rule.insert("set".into(), json!("3TrafficLights1"));
                }
            }
        }
    });
    ui.add_space(6.0);
    let palette = theme.colors;
    match cf_str(rule, "type").as_str() {
        "colorScale" => {
            let mut cols: Vec<String> = rule
                .get("colors")
                .and_then(Json::as_array)
                .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect())
                .unwrap_or_else(|| ["#F8696B", "#FFEB84", "#63BE7B"].map(String::from).to_vec());
            let keys: &[(&str, &str)] = if cols.len() == 2 {
                &[("min", "Minimum"), ("max", "Maximum")]
            } else {
                &[("min", "Minimum"), ("mid", "Midpoint"), ("max", "Maximum")]
            };
            ui.horizontal(|ui| {
                for (i, (key, label)) in keys.iter().enumerate() {
                    ui.vertical(|ui| {
                        cf_threshold(ui, rule, key, label, 140.0);
                        if let Some(slot) = cols.get_mut(i)
                            && let Some(c) = cf_color_button(ui, "Color", hex_color(slot), "Automatic", &palette)
                            && c != "none"
                        {
                            *slot = c;
                        }
                    });
                }
            });
            // The scale itself, as the cells will grade.
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(420.0), 14.0), Sense::hover());
            let stops: Vec<Color32> = cols.iter().filter_map(|c| hex_color(c)).collect();
            let n = 40;
            for k in 0..n {
                let t = k as f32 / (n - 1) as f32 * (stops.len().max(2) - 1) as f32;
                let i = (t.floor() as usize).min(stops.len().saturating_sub(2));
                let (a, b) = (stops.get(i).copied().unwrap_or(Color32::WHITE), stops.get(i + 1).copied().unwrap_or(Color32::WHITE));
                let f = t - i as f32;
                let c = Color32::from_rgb(
                    (a.r() as f32 + (b.r() as f32 - a.r() as f32) * f) as u8,
                    (a.g() as f32 + (b.g() as f32 - a.g() as f32) * f) as u8,
                    (a.b() as f32 + (b.b() as f32 - a.b() as f32) * f) as u8,
                );
                let w = r.width() / n as f32;
                ui.painter().rect_filled(egui::Rect::from_min_size(r.min + vec2(w * k as f32, 0.0), vec2(w + 0.5, r.height())), 0.0, c);
            }
            rule.insert("colors".into(), json!(cols));
        }
        "dataBar" => {
            ui.horizontal(|ui| {
                cf_threshold(ui, rule, "min", "Minimum", 140.0);
                cf_threshold(ui, rule, "max", "Maximum", 140.0);
            });
            ui.horizontal(|ui| {
                if let Some(c) = cf_color_button(ui, "Bar Color", hex_color(&cf_str(rule, "color")), "Automatic", &palette)
                    && c != "none"
                {
                    rule.insert("color".into(), json!(c));
                }
                let mut gradient = rule.get("gradient").and_then(Json::as_bool).unwrap_or(true);
                if ui.checkbox(&mut gradient, lang.tr("Gradient Fill")).changed() {
                    rule.insert("gradient".into(), json!(gradient));
                }
                let mut only = !rule.get("showValue").and_then(Json::as_bool).unwrap_or(true);
                if ui.checkbox(&mut only, lang.tr("Show Bar Only")).changed() {
                    rule.insert("showValue".into(), json!(!only));
                }
            });
        }
        _ => {
            ui.horizontal(|ui| {
                ui.label(lang.tr("Icon Style:"));
                if cf_combo(ui, "iconSet", rule, "set", &CF_ICON_SETS, 160.0) {
                    // Another set has another number of icons: back to the default thresholds.
                    rule.remove("thresholds");
                }
                let set = cf_str(rule, "set");
                let n = gridcraft_engine::cf::icon_count(&set);
                for i in 0..n {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    crate::grid::paint_cf_icon(ui.painter(), r.shrink(2.0), &set, i);
                }
            });
            ui.horizontal(|ui| {
                let mut reverse = cf_flag(rule, "reverse");
                if ui.checkbox(&mut reverse, lang.tr("Reverse Icon Order")).changed() {
                    rule.insert("reverse".into(), json!(reverse));
                }
                let mut only = !rule.get("showValue").and_then(Json::as_bool).unwrap_or(true);
                if ui.checkbox(&mut only, lang.tr("Show Icon Only")).changed() {
                    rule.insert("showValue".into(), json!(!only));
                }
            });
        }
    }
}

/// Rule type 2: cell value, specific text, dates, blanks and errors.
fn cf_contains_editor(ui: &mut egui::Ui, rule: &mut Map<String, Json>) {
    let lang = crate::i18n::current(ui.ctx());
    let ty = cf_str(rule, "type");
    let group = match ty.as_str() {
        "containsText" | "notContainsText" | "beginsWith" | "endsWith" => "text",
        "timePeriod" => "date",
        "blanks" | "noBlanks" | "errors" | "noErrors" => ty.as_str(),
        _ => "cellIs",
    };
    ui.label(lang.tr("Format only cells with:"));
    ui.horizontal(|ui| {
        let mut g = Map::new();
        g.insert("g".into(), json!(group));
        let groups = [
            ("cellIs", "Cell Value"),
            ("text", "Specific Text"),
            ("date", "Dates Occurring"),
            ("blanks", "Blanks"),
            ("noBlanks", "No Blanks"),
            ("errors", "Errors"),
            ("noErrors", "No Errors"),
        ];
        if cf_combo(ui, "contains", &mut g, "g", &groups, 130.0) {
            let t = match cf_str(&g, "g").as_str() {
                "text" => "containsText",
                "date" => "timePeriod",
                "cellIs" => "cellIs",
                other => other,
            }
            .to_string();
            rule.insert("type".into(), json!(t));
        }
        match cf_str(rule, "type").as_str() {
            "cellIs" => {
                if !rule.contains_key("operator") {
                    rule.insert("operator".into(), json!("between"));
                }
                cf_combo(ui, "operator", rule, "operator", &CF_OPERATORS, 170.0);
                cf_text(ui, rule, "value", 90.0);
                if matches!(cf_str(rule, "operator").as_str(), "between" | "notBetween") {
                    ui.label(lang.tr("and"));
                    cf_text(ui, rule, "value2", 90.0);
                }
            }
            "containsText" | "notContainsText" | "beginsWith" | "endsWith" => {
                let ops = [
                    ("containsText", "containing"),
                    ("notContainsText", "not containing"),
                    ("beginsWith", "beginning with"),
                    ("endsWith", "ending with"),
                ];
                cf_combo(ui, "textOp", rule, "type", &ops, 140.0);
                cf_text(ui, rule, "text", 140.0);
            }
            "timePeriod" => {
                if !rule.contains_key("period") {
                    rule.insert("period".into(), json!("yesterday"));
                }
                cf_combo(ui, "period", rule, "period", &CF_PERIODS, 160.0);
            }
            _ => {}
        }
    });
    if cf_str(rule, "type") == "cellIs" {
        ui.label(egui::RichText::new(lang.tr("Type a number, text, or a formula such as =$B$1.")).small().weak());
    }
}

/// New Formatting Rule (or Edit Formatting Rule from Manage Rules):
/// the rule type, then the rule's description and format.
pub(super) fn formatting_rule(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = app.ui.language;
    let theme = app.session.active().map(|doc| doc.wb.theme.clone()).unwrap_or_default();
    let mut rule = d.values.get("rule").and_then(Json::as_object).cloned().unwrap_or_default();
    let kind = cf_kind_of(&cf_str(&rule, "type"));
    ui.label(lang.tr("Select a Rule Type:"));
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, (start, label)) in CF_RULE_KINDS.iter().enumerate() {
            if ui.add(egui::Button::selectable(i == kind, format!("►  {}", lang.tr(label)))).clicked() && i != kind {
                rule.insert("type".into(), json!(start));
                match *start {
                    "colorScale" => {
                        rule.insert("colors".into(), json!(["#F8696B", "#FFEB84", "#63BE7B"]));
                    }
                    "top10" => {
                        rule.insert("rank".into(), json!(10));
                    }
                    _ => {}
                }
            }
        }
    });
    ui.add_space(6.0);
    ui.label(lang.tr("Edit the Rule Description:"));
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match cf_kind_of(&cf_str(&rule, "type")) {
            0 => cf_values_editor(ui, &mut rule, &theme),
            1 => cf_contains_editor(ui, &mut rule),
            2 => {
                ui.label(lang.tr("Format values that rank in the:"));
                ui.horizontal(|ui| {
                    let mut m = Map::new();
                    m.insert("b".into(), json!(if cf_flag(&rule, "bottom") { "bottom" } else { "top" }));
                    if cf_combo(ui, "topBottom", &mut m, "b", &[("top", "CF|Top"), ("bottom", "CF|Bottom")], 90.0) {
                        rule.insert("bottom".into(), json!(cf_str(&m, "b") == "bottom"));
                    }
                    let mut rank = rule.get("rank").and_then(Json::as_u64).unwrap_or(10);
                    if ui.add(egui::DragValue::new(&mut rank).range(1..=1000)).changed() {
                        rule.insert("rank".into(), json!(rank));
                    }
                    let mut percent = cf_flag(&rule, "percent");
                    if ui.checkbox(&mut percent, lang.tr("% of the selected range")).changed() {
                        rule.insert("percent".into(), json!(percent));
                    }
                });
            }
            3 => {
                ui.label(lang.tr("Format values that are:"));
                let current = match (cf_flag(&rule, "below"), cf_flag(&rule, "equal"), rule.get("stdDev").and_then(Json::as_u64).unwrap_or(0)) {
                    (b, _, n @ 1..) => format!("{}{n}", if b { "sdBelow" } else { "sdAbove" }),
                    (false, false, _) => "above".into(),
                    (true, false, _) => "below".into(),
                    (false, true, _) => "equalAbove".into(),
                    (true, true, _) => "equalBelow".into(),
                };
                let mut m = Map::new();
                m.insert("a".into(), json!(current));
                let options = [
                    ("above", "CF|above"),
                    ("below", "CF|below"),
                    ("equalAbove", "equal or above"),
                    ("equalBelow", "equal or below"),
                    ("sdAbove1", "1 std dev above"),
                    ("sdBelow1", "1 std dev below"),
                    ("sdAbove2", "2 std dev above"),
                    ("sdBelow2", "2 std dev below"),
                    ("sdAbove3", "3 std dev above"),
                    ("sdBelow3", "3 std dev below"),
                ];
                ui.horizontal(|ui| {
                    if cf_combo(ui, "average", &mut m, "a", &options, 150.0) {
                        let a = cf_str(&m, "a");
                        let sd = a.strip_prefix("sdAbove").or_else(|| a.strip_prefix("sdBelow")).and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
                        rule.insert("below".into(), json!(a.contains("elow")));
                        rule.insert("equal".into(), json!(a.starts_with("equal")));
                        rule.insert("stdDev".into(), json!(sd));
                    }
                    ui.label(lang.tr("the average for the selected range"));
                });
            }
            4 => {
                ui.label(lang.tr("Format all:"));
                ui.horizontal(|ui| {
                    cf_combo(ui, "uniqueDup", &mut rule, "type", &[("duplicate", "CF|duplicate"), ("unique", "CF|unique")], 110.0);
                    ui.label(lang.tr("values in the selected range"));
                });
            }
            _ => {
                ui.label(lang.tr("Format values where this formula is true:"));
                let mut f = cf_str(&rule, "formula");
                if f.is_empty() {
                    f.push('=');
                }
                let r = ui.add(egui::TextEdit::singleline(&mut f).desired_width(ui.available_width()).font(egui::TextStyle::Monospace));
                if r.changed() {
                    rule.insert("formula".into(), json!(f));
                }
                // A click or drag on the grid writes a reference at the caret (`Dialog::pick_reference`).
                if let Some(pick) = d.pick.as_mut() {
                    track_pick(pick, &r, r.changed());
                }
                let top_left = d.values.get("range").and_then(Json::as_str).and_then(|r| r.split([':', ',']).next()).map(str::to_string);
                let top_left = top_left.or_else(|| app.session.active().map(|doc| doc.selection.current().start.a1())).unwrap_or_else(|| "A1".into());
                ui.label(
                    egui::RichText::new(format!(
                        "{} {top_left}. {}",
                        lang.tr("The formula is written for the top-left cell of the range,"),
                        lang.tr("Example: =$B2>100 highlights each row whose column B is over 100.")
                    ))
                    .small()
                    .weak(),
                );
            }
        }
        if kind != 0 && cf_kind_of(&cf_str(&rule, "type")) != 0 {
            ui.add_space(8.0);
            ui.separator();
            cf_format_editor(ui, &mut rule, &theme);
        }
    });
    // No rule without something to compare against.
    let missing = match cf_str(&rule, "type").as_str() {
        "cellIs" => {
            cf_str(&rule, "value").trim().is_empty()
                || (matches!(cf_str(&rule, "operator").as_str(), "between" | "notBetween") && cf_str(&rule, "value2").trim().is_empty())
        }
        "expression" => cf_str(&rule, "formula").trim().trim_start_matches('=').trim().is_empty(),
        "containsText" | "notContainsText" | "beginsWith" | "endsWith" => cf_str(&rule, "text").is_empty(),
        _ => false,
    };
    d.values.insert("rule".into(), Json::Object(rule));
    ok_cancel(ui, confirm, open);
    if *confirm && missing {
        *confirm = false;
        app.message = Some((d.title.clone(), lang.tr("Enter a value, text or formula for the rule.").into()));
    }
}
