use super::{
    Anchor, BorderLine, BorderStyle, Borders, CellRef, CfOperator, CfRule, CfValueKind, Color, CondFormat, EngineError, Fill, Json, RangeRef, Result,
    Session, Shape, ShapeKind, Style, Underline, Workbook, anchor_mode_param, bad, bool_param, cell_param, edit, f64_param, json, ok, split_sheet,
    str_param, target_ranges, target_sheet, u32_param,
};
use crate::cf::icon_count;
use gridcraft_model::{PatternType, Theme};

// ---------------------------------------------------------------- conditional formatting

/// The format of a Highlight Cells preset (`lightRedFillDarkRedText`, `redBorder`…).
pub fn preset_style(name: &str) -> Style {
    let mut s = Style::default();
    match name {
        "redText" => s.font.color = Color::rgb(0x9C, 0x00, 0x06),
        "yellowFill" | "yellowFillDarkYellowText" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xEB, 0x9C));
            s.font.color = Color::rgb(0x9C, 0x57, 0x00);
        }
        "greenFill" | "greenFillDarkGreenText" => {
            s.fill = Fill::solid(Color::rgb(0xC6, 0xEF, 0xCE));
            s.font.color = Color::rgb(0x00, 0x61, 0x00);
        }
        "lightRedFill" => s.fill = Fill::solid(Color::rgb(0xFF, 0xC7, 0xCE)),
        "redBorder" => {
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0xC0, 0x00, 0x00) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        _ => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xC7, 0xCE));
            s.font.color = Color::rgb(0x9C, 0x00, 0x06);
        }
    }
    s
}

/// Whether `v` is one formula string literal: `"…"` with every inner quote doubled.
fn is_string_literal(v: &str) -> bool {
    v.len() >= 2 && v.starts_with('"') && v.ends_with('"') && v.get(1..v.len() - 1).is_some_and(|inner| !inner.replace("\"\"", "").contains('"'))
}

/// A conditional format's `style` as the model's `Style` JSON (`font`, `fill`, `borders`, `align`,
/// `num_fmt`, `protection`, merged over the default by the caller), where a colour may also be
/// written as `format_range` takes it (`"#RRGGBB"`, `{theme, tint}`, `"auto"`) and `fill` may be a
/// colour alone (a solid fill) or `"none"` (no fill). An unknown key is an error rather than
/// dropped: dropped, it left a rule with an empty format that matches and shows nothing.
fn cf_style_json(st: &Json) -> Result<Json> {
    const KEYS: [&str; 6] = ["font", "fill", "borders", "align", "num_fmt", "protection"];
    let err = |m: String| bad("home.conditionalFormat", m);
    let Some(obj) = st.as_object() else { return Err(err("`style` must be an object, e.g. {\"fill\": \"#FFC7CE\"}".into())) };
    let mut st = obj.clone();
    if let Some(k) = st.keys().find(|k| !KEYS.contains(&k.as_str())) {
        let hint = if k == "preset" { "; a preset goes in `rule.preset`" } else { "" };
        return Err(err(format!("unknown key `{k}` in `style` (known: {}){hint}", KEYS.join(", "))));
    }
    // A colour written the way the other formatting commands take it, as the model stores it.
    let color = |v: &mut Json, what: &str| -> Result<()> {
        if v.is_string() || v.get("theme").is_some() {
            let c = crate::cmd::format::color_param(Some(v)).ok_or_else(|| err(format!("invalid colour {v} for `{what}`")))?;
            *v = serde_json::to_value(c).map_err(|e| EngineError::Other(e.to_string()))?;
        }
        Ok(())
    };
    if let Some(f) = st.get_mut("font").and_then(Json::as_object_mut)
        && let Some(c) = f.get_mut("color")
    {
        color(c, "font.color")?;
    }
    if let Some(fill) = st.get_mut("fill") {
        // `"none"` (or `"auto"`) is no fill, as `home.fillColor` reads it, not a solid automatic one.
        if fill.is_string() && crate::cmd::format::color_param(Some(fill)) == Some(Color::Auto) {
            *fill = serde_json::to_value(Fill::default()).map_err(|e| EngineError::Other(e.to_string()))?;
        } else if fill.is_string() || fill.get("theme").is_some() {
            *fill = json!({"pattern": "Solid", "fg": fill.clone()});
        }
        if let Some(f) = fill.as_object_mut() {
            for k in ["fg", "bg"] {
                if let Some(c) = f.get_mut(k) {
                    color(c, &format!("fill.{k}"))?;
                }
            }
            // A colour with no pattern is a solid fill, not an invisible one.
            if f.contains_key("fg") && !f.contains_key("pattern") {
                f.insert("pattern".into(), json!("Solid"));
            }
        }
    }
    if let Some(b) = st.get_mut("borders").and_then(Json::as_object_mut) {
        for (side, line) in b.iter_mut() {
            if let Some(c) = line.get_mut("color") {
                color(c, &format!("borders.{side}.color"))?;
            }
        }
    }
    Ok(Json::Object(st))
}

/// #1999: a formula that doesn't parse must never reach the workbook — Excel refuses to open
/// files carrying it, even in repair mode. Validated here so the API and the New Rule dialog
/// (which dispatches this command) share one gate, the way Excel's object model refuses.
fn check_cf_formula(cmd: &str, what: &str, formula: &str) -> Result<()> {
    // Strip exactly one leading '=', the way the XLSX writer does: a value like '===A1'
    // only parses after several strips and would still be saved unparseable. (#1999 review)
    if gridcraft_formula::parse(formula.strip_prefix('=').unwrap_or(formula)).is_err() {
        return Err(bad(cmd, format!("invalid {what} formula `{formula}` — it does not parse; Excel would reject the saved file")));
    }
    Ok(())
}

fn check_cf_value(what: &str, v: &gridcraft_model::features::CfValueKind) -> Result<()> {
    if let gridcraft_model::features::CfValueKind::Formula(f) = v {
        check_cf_formula("home.conditionalFormat", what, f)?;
    }
    Ok(())
}

fn check_cf_rule(rule: &CfRule) -> Result<()> {
    const CMD: &str = "home.conditionalFormat";
    match rule {
        CfRule::CellIs { a, b, .. } => {
            check_cf_formula(CMD, "comparison value", a)?;
            if let Some(b) = b {
                check_cf_formula(CMD, "second comparison value", b)?;
            }
        }
        CfRule::Expression { formula, .. } => check_cf_formula(CMD, "rule", formula)?,
        CfRule::DataBar { min, max, .. } => {
            check_cf_value("data bar minimum", min)?;
            check_cf_value("data bar maximum", max)?;
        }
        CfRule::ColorScale { stops } => {
            for (kind, _) in stops {
                check_cf_value("color scale threshold", kind)?;
            }
        }
        CfRule::IconSet { set, thresholds, .. } => {
            // The evaluator reads one threshold per icon, or one fewer (no lower bound); any other
            // count would be stored and then ignored for the defaults.
            let count = icon_count(set);
            if !thresholds.is_empty() && thresholds.len() != count && thresholds.len() + 1 != count {
                return Err(bad(CMD, format!("icon set `{set}` takes {count} thresholds, or {}", count - 1)));
            }
            for kind in thresholds {
                check_cf_value("icon set threshold", kind)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn add_cf(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    let sheet = target_sheet(s, p)?;
    let Some(rule) = p.get("rule") else {
        s.ui_requests.push(crate::UiRequest::Dialog("newFormattingRule".into(), json!({})));
        return ok();
    };
    let rule = parse_rule(rule, &s.doc()?.wb.theme)?;
    let stop_if_true = bool_param(p, "stopIfTrue").unwrap_or(false);
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        // New rules take precedence over older ones.
        sh.cond_formats.insert(0, CondFormat { ranges: ranges.clone(), rule, priority: 1, stop_if_true });
        renumber_cf(&mut sh.cond_formats);
        Ok(json!({"rules": sh.cond_formats.len()}))
    })
}

/// The plain format overrides that describe `st`, as [`rule_params`] lists them. They are
/// coarser than the style (one border colour on four thin sides, a solid fill, a single
/// underline), so [`rule_style`] applies only those that differ from these.
fn style_overrides(st: &Style, theme: &Theme) -> serde_json::Map<String, Json> {
    let hex = |c: &Color| c.hex(theme);
    let line = [st.borders.left, st.borders.right, st.borders.top, st.borders.bottom].into_iter().find(|l| l.style != BorderStyle::None);
    let mut m = serde_json::Map::new();
    m.insert("fill".into(), if st.fill.pattern == PatternType::None { json!("none") } else { hex(&st.fill.fg).map_or(Json::Null, Json::from) });
    m.insert("fontColor".into(), hex(&st.font.color).map_or(json!("auto"), Json::from));
    m.insert("border".into(), line.map_or(json!("none"), |l| json!(hex(&l.color).unwrap_or_else(|| "#000000".into()))));
    m.insert("bold".into(), json!(st.font.bold));
    m.insert("italic".into(), json!(st.font.italic));
    m.insert("underline".into(), json!(st.font.underline != Underline::None));
    m.insert("strike".into(), json!(st.font.strike));
    m
}

/// The style a rule's params give: `style` (a partial style) or a `preset`, then the plain
/// overrides `fill`, `fontColor`, `border` (colours, `none` clears), `bold`, `italic`,
/// `underline` and `strike`. An override equal to what the base style already shows is left
/// out, so a rule sent back unchanged keeps its partial borders, patterns and underline kind.
fn rule_style(rule: &Json, theme: &Theme) -> Result<Box<Style>> {
    let preset = rule.get("preset").and_then(Json::as_str);
    let mut st = match rule.get("style") {
        Some(st) if !st.is_null() => {
            // A listed rule's `style` carries `dxf_stated` (the parts a file states at their
            // default value): it is kept as is, not checked as a format key.
            let mut st = st.clone();
            let stated = st.as_object_mut().and_then(|o| o.remove("dxf_stated"));
            let st = cf_style_json(&st)?;
            let base = serde_json::to_value(Style::default()).map_err(|e| EngineError::Other(e.to_string()))?;
            let mut style: Style =
                serde_json::from_value(crate::cmd::format::merge_json(base, &st)).map_err(|e| bad("home.conditionalFormat", e.to_string()))?;
            if let Some(d) = stated {
                style.dxf_stated = serde_json::from_value(d).map_err(|e| bad("home.conditionalFormat", e.to_string()))?;
            }
            style
        }
        // Plain overrides alone start from an empty format, not from the default preset.
        _ if preset.is_none() && ["fill", "fontColor", "border", "bold", "italic", "underline", "strike"].iter().any(|k| rule.get(*k).is_some()) => {
            Style::default()
        }
        _ => preset_style(preset.unwrap_or("lightRedFillDarkRedText")),
    };
    // A malformed override is an error rather than no change; only a missing or null one is.
    for k in ["fill", "fontColor", "border"] {
        if let Some(v) = rule.get(k).filter(|v| !v.is_null())
            && crate::cmd::format::color_param(Some(v)).is_none()
        {
            return Err(bad("home.conditionalFormat", format!("invalid colour {v} for `{k}`")));
        }
    }
    for k in ["bold", "italic", "underline", "strike"] {
        if let Some(v) = rule.get(k).filter(|v| !v.is_null() && !v.is_boolean()) {
            return Err(bad("home.conditionalFormat", format!("`{k}` must be true or false, not {v}")));
        }
    }
    let shown = style_overrides(&st, theme);
    let changed = |k: &str| rule.get(k).filter(|v| shown.get(k) != Some(*v));
    // Colours compare by the colour they give, so `#ffc7ce`, `#FFC7CE` and a theme slot of the
    // same shade all count as unchanged.
    let changed_color = |k: &str| {
        let c = crate::cmd::format::color_param(rule.get(k))?;
        let base = crate::cmd::format::color_param(shown.get(k));
        (base.map(|b| b.hex(theme)) != Some(c.hex(theme))).then_some(c)
    };
    if let Some(c) = changed_color("fill") {
        st.fill = if c == Color::Auto { Fill::default() } else { Fill::solid(c) };
    }
    if let Some(c) = changed_color("fontColor") {
        st.font.color = c;
    }
    if let Some(c) = changed_color("border") {
        st.borders = if c == Color::Auto {
            Borders::default()
        } else {
            let l = BorderLine { style: BorderStyle::Thin, color: c };
            Borders { left: l, right: l, top: l, bottom: l, ..Default::default() }
        };
    }
    let flag = |k: &str| changed(k).and_then(Json::as_bool);
    if let Some(b) = flag("bold") {
        st.font.bold = b;
    }
    if let Some(b) = flag("italic") {
        st.font.italic = b;
    }
    if let Some(b) = flag("underline") {
        st.font.underline = if b { Underline::Single } else { Underline::None };
    }
    if let Some(b) = flag("strike") {
        st.font.strike = b;
    }
    Ok(Box::new(st))
}

/// A threshold from `{type: min|max|num|percent|percentile|formula, value}`.
fn cf_value_kind(v: Option<&Json>, default: CfValueKind) -> CfValueKind {
    match v {
        Some(Json::Object(o)) => {
            let val = o
                .get("value")
                .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
                // `inf` and `NaN` parse but can't be listed or saved: they read as the default.
                .filter(|v: &f64| v.is_finite())
                .unwrap_or(0.0);
            match o.get("type").and_then(Json::as_str).unwrap_or("num") {
                "min" => CfValueKind::Min,
                "max" => CfValueKind::Max,
                "percent" => CfValueKind::Percent(val),
                "percentile" => CfValueKind::Percentile(val),
                // Kept as typed: the check and the file writer each take off one leading `=` (#1999).
                "formula" => CfValueKind::Formula(o.get("value").map(crate::cmd::edit::json_to_input).unwrap_or_default()),
                _ => CfValueKind::Number(val),
            }
        }
        _ => default,
    }
}

fn cf_value_json(k: &CfValueKind) -> Json {
    match k {
        CfValueKind::Min => json!({"type": "min"}),
        CfValueKind::Max => json!({"type": "max"}),
        CfValueKind::Number(v) => json!({"type": "num", "value": v}),
        CfValueKind::Percent(v) => json!({"type": "percent", "value": v}),
        CfValueKind::Percentile(v) => json!({"type": "percentile", "value": v}),
        CfValueKind::Formula(f) => json!({"type": "formula", "value": if f.starts_with('=') { f.clone() } else { format!("={f}") }}),
    }
}

/// A rule from its `home.conditionalFormat` params.
fn parse_rule(rule: &Json, theme: &Theme) -> Result<CfRule> {
    let g = |k: &str| rule.get(k).and_then(Json::as_str).map(str::to_string);
    let flag = |k: &str| rule.get(k).and_then(Json::as_bool);
    let kind = g("type").unwrap_or_else(|| "cellIs".into());
    let style = || rule_style(rule, theme);
    let rule = match kind.as_str() {
        "cellIs" => {
            // Excel's file and VBA names (`lessThan`, …) are accepted too.
            let op = match g("operator").as_deref().unwrap_or("greater") {
                "between" => CfOperator::Between,
                "notBetween" => CfOperator::NotBetween,
                "equal" => CfOperator::Equal,
                "notEqual" => CfOperator::NotEqual,
                "less" | "lessThan" => CfOperator::Less,
                "greaterOrEqual" | "greaterThanOrEqual" => CfOperator::GreaterOrEqual,
                "lessOrEqual" | "lessThanOrEqual" => CfOperator::LessOrEqual,
                "greater" | "greaterThan" => CfOperator::Greater,
                other => return Err(bad("home.conditionalFormat", format!("unknown operator `{other}`"))),
            };
            let a = rule.get("value").map(crate::cmd::edit::json_to_input).unwrap_or_default();
            let b = rule.get("value2").map(crate::cmd::edit::json_to_input);
            let quote = |v: String| {
                // A number, a formula, or text already written as a string literal (`"Closed"`, as
                // in the file and in openpyxl's `CellIsRule`) is kept; other text is quoted.
                if gridcraft_core::parse::parse_number_text(&v).is_some() || v.starts_with('=') {
                    v.trim_start_matches('=').to_string()
                } else if is_string_literal(&v) {
                    v
                } else {
                    format!("\"{}\"", v.replace('"', "\"\""))
                }
            };
            CfRule::CellIs { op, a: quote(a), b: b.map(quote), style: style()? }
        }
        "expression" => CfRule::Expression { formula: g("formula").unwrap_or_default().trim_start_matches('=').to_string(), style: style()? },
        "containsText" => CfRule::ContainsText { text: g("text").unwrap_or_default(), style: style()? },
        "notContainsText" => CfRule::NotContainsText { text: g("text").unwrap_or_default(), style: style()? },
        "beginsWith" => CfRule::BeginsWith { text: g("text").unwrap_or_default(), style: style()? },
        "endsWith" => CfRule::EndsWith { text: g("text").unwrap_or_default(), style: style()? },
        "blanks" => CfRule::Blanks { style: style()? },
        "noBlanks" => CfRule::NoBlanks { style: style()? },
        "errors" => CfRule::Errors { style: style()? },
        "noErrors" => CfRule::NoErrors { style: style()? },
        "duplicate" => CfRule::Duplicate { style: style()? },
        "unique" => CfRule::Unique { style: style()? },
        "top10" => CfRule::Top10 {
            bottom: flag("bottom").unwrap_or(false),
            percent: flag("percent").unwrap_or(false),
            rank: rule.get("rank").and_then(Json::as_u64).unwrap_or(10).clamp(1, 1000) as u32,
            style: style()?,
        },
        "aboveAverage" => CfRule::AboveAverage {
            below: flag("below").unwrap_or(false),
            equal: flag("equal").unwrap_or(false),
            std_dev: rule.get("stdDev").and_then(Json::as_u64).unwrap_or(0).min(3) as u8,
            style: style()?,
        },
        "timePeriod" => CfRule::TimePeriod { period: g("period").unwrap_or_else(|| "today".into()), style: style()? },
        "colorScale" => {
            let cols: Vec<Color> = rule
                .get("colors")
                .and_then(Json::as_array)
                .map(|a| a.iter().filter_map(|c| crate::cmd::format::color_param(Some(c))).collect())
                .unwrap_or_else(|| vec![Color::rgb(0xF8, 0x69, 0x6B), Color::rgb(0xFF, 0xEB, 0x84), Color::rgb(0x63, 0xBE, 0x7B)]);
            let min = cf_value_kind(rule.get("min"), CfValueKind::Min);
            let max = cf_value_kind(rule.get("max"), CfValueKind::Max);
            let stops = match cols.len() {
                2 => vec![(min, cols[0]), (max, cols[1])],
                _ => vec![
                    (min, cols.first().copied().unwrap_or_default()),
                    (cf_value_kind(rule.get("mid"), CfValueKind::Percentile(50.0)), cols.get(1).copied().unwrap_or_default()),
                    (max, cols.get(2).copied().unwrap_or_default()),
                ],
            };
            CfRule::ColorScale { stops }
        }
        "dataBar" => CfRule::DataBar {
            min: cf_value_kind(rule.get("min"), CfValueKind::Min),
            max: cf_value_kind(rule.get("max"), CfValueKind::Max),
            color: crate::cmd::format::color_param(rule.get("color")).unwrap_or(Color::rgb(0x63, 0x8E, 0xC6)),
            gradient: flag("gradient").unwrap_or(true),
            show_value: flag("showValue").unwrap_or(true),
        },
        "iconSet" => {
            let set = g("set").unwrap_or_else(|| "3TrafficLights1".into());
            let thresholds: Vec<CfValueKind> = match rule.get("thresholds") {
                Some(t) if !t.is_null() => {
                    serde_json::from_value(t.clone()).map_err(|e| bad("home.conditionalFormat", format!("`thresholds`: {e}")))?
                }
                _ => vec![],
            };
            CfRule::IconSet { set, thresholds, reverse: flag("reverse").unwrap_or(false), show_value: flag("showValue").unwrap_or(true) }
        }
        other => return Err(bad("home.conditionalFormat", format!("unknown rule type `{other}`"))),
    };
    check_cf_rule(&rule)?;
    Ok(rule)
}

/// A rule as `home.conditionalFormat` params, the inverse of [`parse_rule`]: Manage Rules edits
/// a rule through these. The format is given as plain overrides, plus the whole `style` so that
/// parts the overrides don't cover (number format, partial borders) survive an edit.
fn rule_params(rule: &CfRule, theme: &Theme) -> Json {
    let hex = |c: &Color| c.hex(theme).map(Json::from).unwrap_or(Json::Null);
    let unformula = |f: &str| {
        if is_string_literal(f) {
            let t = f.get(1..f.len() - 1).unwrap_or_default().replace("\"\"", "\"");
            // Text that would read back as a number, a formula or a literal stays quoted, as
            // `parse_rule` keeps a string literal as it is.
            if gridcraft_core::parse::parse_number_text(&t).is_some() || t.starts_with('=') || is_string_literal(&t) { f.to_string() } else { t }
        } else if gridcraft_core::parse::parse_number_text(f).is_some() {
            f.to_string()
        } else {
            format!("={f}")
        }
    };
    let (mut v, style) = match rule {
        CfRule::CellIs { op, a, b, style } => {
            let op = match op {
                CfOperator::Between => "between",
                CfOperator::NotBetween => "notBetween",
                CfOperator::Equal => "equal",
                CfOperator::NotEqual => "notEqual",
                CfOperator::Greater => "greater",
                CfOperator::Less => "less",
                CfOperator::GreaterOrEqual => "greaterOrEqual",
                CfOperator::LessOrEqual => "lessOrEqual",
            };
            let mut v = json!({"type": "cellIs", "operator": op, "value": unformula(a)});
            if let Some(b) = b {
                v["value2"] = json!(unformula(b));
            }
            (v, Some(style))
        }
        CfRule::Expression { formula, style } => (json!({"type": "expression", "formula": format!("={formula}")}), Some(style)),
        CfRule::ContainsText { text, style } => (json!({"type": "containsText", "text": text}), Some(style)),
        CfRule::NotContainsText { text, style } => (json!({"type": "notContainsText", "text": text}), Some(style)),
        CfRule::BeginsWith { text, style } => (json!({"type": "beginsWith", "text": text}), Some(style)),
        CfRule::EndsWith { text, style } => (json!({"type": "endsWith", "text": text}), Some(style)),
        CfRule::Blanks { style } => (json!({"type": "blanks"}), Some(style)),
        CfRule::NoBlanks { style } => (json!({"type": "noBlanks"}), Some(style)),
        CfRule::Errors { style } => (json!({"type": "errors"}), Some(style)),
        CfRule::NoErrors { style } => (json!({"type": "noErrors"}), Some(style)),
        CfRule::Duplicate { style } => (json!({"type": "duplicate"}), Some(style)),
        CfRule::Unique { style } => (json!({"type": "unique"}), Some(style)),
        CfRule::Top10 { bottom, percent, rank, style } => (json!({"type": "top10", "bottom": bottom, "percent": percent, "rank": rank}), Some(style)),
        CfRule::AboveAverage { below, equal, std_dev, style } => {
            (json!({"type": "aboveAverage", "below": below, "equal": equal, "stdDev": std_dev}), Some(style))
        }
        CfRule::TimePeriod { period, style } => (json!({"type": "timePeriod", "period": period}), Some(style)),
        CfRule::ColorScale { stops } => {
            let mut v = json!({"type": "colorScale", "colors": stops.iter().map(|(_, c)| hex(c)).collect::<Vec<_>>()});
            let keys: &[&str] = if stops.len() == 2 { &["min", "max"] } else { &["min", "mid", "max"] };
            for (k, (kind, _)) in keys.iter().zip(stops) {
                v[*k] = cf_value_json(kind);
            }
            (v, None)
        }
        CfRule::DataBar { min, max, color, gradient, show_value } => (
            json!({"type": "dataBar", "color": hex(color), "gradient": gradient, "showValue": show_value, "min": cf_value_json(min), "max": cf_value_json(max)}),
            None,
        ),
        CfRule::IconSet { set, thresholds, reverse, show_value } => {
            (json!({"type": "iconSet", "set": set, "thresholds": thresholds, "reverse": reverse, "showValue": show_value}), None)
        }
    };
    if let Some(st) = style {
        for (k, x) in style_overrides(st, theme) {
            v[k.as_str()] = x;
        }
        v["style"] = serde_json::to_value(st).unwrap_or(Json::Null);
    }
    v
}

/// `=$A$1:$B$9,$D$2` for a rule's ranges, as Manage Rules shows them.
fn applies_to(ranges: &[RangeRef]) -> String {
    let a = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row as u64 + 1);
    let parts: Vec<String> = ranges
        .iter()
        .map(|r| {
            if r.is_single() {
                a(r.start)
            } else if r.is_full_cols() || r.is_full_rows() {
                r.a1()
            } else {
                format!("{}:{}", a(r.start), a(r.end))
            }
        })
        .collect();
    format!("={}", parts.join(","))
}

/// The ranges of an `appliesTo` like `=$A$1:$B$9,$D$2` on `sheet`. A `Sheet!` prefix must name
/// that sheet: a rule's ranges live on its own sheet, so another sheet's range is refused rather
/// than read as the same address here.
fn parse_applies_to(text: &str, wb: &Workbook, sheet: usize) -> Result<Vec<RangeRef>> {
    let body = text.trim().trim_start_matches('=');
    let mut out = Vec::new();
    for part in body.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let (name, part) = split_sheet(part);
        if let Some(name) = name
            && wb.sheet_index(&name) != Some(sheet)
        {
            return Err(bad("home.manageRules", format!("`appliesTo` names another sheet: `{name}`")));
        }
        out.push(RangeRef::parse(part).ok_or_else(|| bad("home.manageRules", format!("not a range: `{part}`")))?);
    }
    if out.is_empty() {
        return Err(bad("home.manageRules", "`appliesTo` names no range"));
    }
    Ok(out)
}

fn renumber_cf(rules: &mut [CondFormat]) {
    for (k, cf) in rules.iter_mut().enumerate() {
        cf.priority = k as u32 + 1;
    }
}

pub(super) fn clear_cf(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let whole = bool_param(p, "sheet").unwrap_or(false);
    let ranges = target_ranges(s, p)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if whole {
            sh.cond_formats.clear();
        } else {
            for cf in sh.cond_formats.iter_mut() {
                cf.ranges.retain(|r| !ranges.iter().any(|x| x.intersects(r)));
            }
            sh.cond_formats.retain(|c| !c.ranges.is_empty());
        }
        Ok(Json::Null)
    })
}

pub(super) fn manage_cf(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    if let Some(list) = p.get("set") {
        // The whole list at once, highest priority first (Manage Rules › Cancel puts it back).
        // A listed `rule` restores exactly; `params` go through the same parsing as a new rule.
        let theme = s.doc()?.wb.theme.clone();
        let mut rules = Vec::new();
        for (i, x) in list.as_array().ok_or_else(|| bad("home.manageRules", "`set` is a list of rules"))?.iter().enumerate() {
            let ranges = parse_applies_to(x.get("appliesTo").and_then(Json::as_str).unwrap_or(""), &s.doc()?.wb, sheet)?;
            let rule = match (x.get("rule"), x.get("params")) {
                (Some(r), _) => match serde_json::from_value::<CfRule>(r.clone()) {
                    Ok(rule) => {
                        check_cf_rule(&rule)?;
                        rule
                    }
                    // Params written as `rule` (they name a `type`) still parse; a serialized rule
                    // that doesn't deserialize is refused rather than read as a default rule.
                    Err(_) if r.get("type").is_some_and(Json::is_string) => parse_rule(r, &theme)?,
                    Err(e) => return Err(bad("home.manageRules", format!("`set` entry {i}: {e}"))),
                },
                (None, Some(p)) => parse_rule(p, &theme)?,
                (None, None) => return Err(bad("home.manageRules", format!("`set` entry {i} has no `rule` or `params`"))),
            };
            let stop_if_true = bool_param(x, "stopIfTrue").unwrap_or(false);
            rules.push(CondFormat { ranges, rule, priority: i as u32 + 1, stop_if_true });
        }
        return edit(s, |cx| {
            cx.sheet_mut(sheet)?.cond_formats = rules;
            Ok(Json::Null)
        });
    }
    if ["update", "duplicate", "delete", "moveUp", "moveDown"].iter().all(|k| p.get(*k).is_none()) {
        let d = s.doc()?;
        // Current Selection lists the rules that touch the selection (`selection`, else the
        // active sheet's); the default is the whole sheet.
        let only = match (str_param(p, "show"), str_param(p, "selection")) {
            (Some("selection"), Some(t)) => Some(parse_applies_to(t, &d.wb, sheet)?),
            (Some("selection"), None) if sheet == d.wb.active_sheet() => Some(d.selection.ranges.clone()),
            _ => None,
        };
        let list: Vec<Json> =
            d.wb.sheet(sheet)
                .map(|sh| {
                    sh.cond_formats
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| only.as_ref().is_none_or(|sel| c.ranges.iter().any(|r| sel.iter().any(|x| x.intersects(r)))))
                        .map(|(i, c)| {
                            json!({
                                "index": i,
                                "ranges": c.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>(),
                                "appliesTo": applies_to(&c.ranges),
                                "stopIfTrue": c.stop_if_true,
                                "params": rule_params(&c.rule, &d.wb.theme),
                                "rule": c.rule,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
        let total = d.wb.sheet(sheet).map_or(0, |sh| sh.cond_formats.len());
        return Ok(json!({"rules": list, "total": total}));
    }
    let update = u32_param(p, "update").map(|v| v as usize);
    let rule = match (update, p.get("rule")) {
        (Some(_), Some(r)) => Some(parse_rule(r, &s.doc()?.wb.theme)?),
        _ => None,
    };
    let ranges = match (update, str_param(p, "appliesTo")) {
        (Some(_), Some(t)) => Some(parse_applies_to(t, &s.doc()?.wb, sheet)?),
        _ => None,
    };
    let stop = bool_param(p, "stopIfTrue");
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(cf) = update.and_then(|i| sh.cond_formats.get_mut(i)) {
            if let Some(r) = rule {
                cf.rule = r;
            }
            if let Some(r) = ranges {
                cf.ranges = r;
            }
            if let Some(b) = stop {
                cf.stop_if_true = b;
            }
        }
        // The copy goes right below the rule it copies.
        if let Some(i) = u32_param(p, "duplicate").map(|v| v as usize)
            && let Some(cf) = sh.cond_formats.get(i).cloned()
        {
            sh.cond_formats.insert(i + 1, cf);
        }
        // The count is taken again after each step: a delete shortens the list.
        if let Some(i) = u32_param(p, "delete").map(|v| v as usize)
            && i < sh.cond_formats.len()
        {
            sh.cond_formats.remove(i);
        }
        if let Some(i) = u32_param(p, "moveUp").map(|v| v as usize)
            && i > 0
            && i < sh.cond_formats.len()
        {
            sh.cond_formats.swap(i, i - 1);
        }
        if let Some(i) = u32_param(p, "moveDown").map(|v| v as usize)
            && i + 1 < sh.cond_formats.len()
        {
            sh.cond_formats.swap(i, i + 1);
        }
        renumber_cf(&mut sh.cond_formats);
        Ok(Json::Null)
    })
}

pub(super) fn insert_icon(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").unwrap_or("check").to_string();
    let size = f64_param(p, "size").unwrap_or(48.0).clamp(8.0, 1000.0) as f32;
    let d = s.doc()?;
    let id = d.wb.next_object_id();
    let at = cell_param(s, p, "at")?.unwrap_or(d.selection.active);
    let color = crate::cmd::format::color_param(p.get("color")).unwrap_or(Color::Theme(1, 0));
    let shape = Shape {
        id,
        kind: ShapeKind::Icon,
        anchor: Anchor { cell: at, dx: 4.0, dy: 4.0, width: size, height: size, mode: anchor_mode_param(p).unwrap_or_default() },
        fill: color,
        line: Color::Auto,
        text: name,
        format: Default::default(),
    };
    let sheet = d.wb.active_sheet();
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
}
