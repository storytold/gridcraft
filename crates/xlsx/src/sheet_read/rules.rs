use super::{
    CfOperator, CfRule, CfValueKind, CondFormat, Ctx, El, ErrorStyle, MAX_COND_CELLS, RangeRef, Sheet, Validation, ValidationKind, from_file,
    parse_sqref, read_color,
};
use crate::styles::read_dxf;
use gridcraft_model::IconChoice;

fn cf_op(s: Option<&str>) -> CfOperator {
    match s.unwrap_or("between") {
        "notBetween" => CfOperator::NotBetween,
        "equal" => CfOperator::Equal,
        "notEqual" => CfOperator::NotEqual,
        "greaterThan" => CfOperator::Greater,
        "lessThan" => CfOperator::Less,
        "greaterThanOrEqual" => CfOperator::GreaterOrEqual,
        "lessThanOrEqual" => CfOperator::LessOrEqual,
        _ => CfOperator::Between,
    }
}

fn cfvo(e: &El) -> CfValueKind {
    let val = e.attr("val").unwrap_or("");
    let n = val.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    match e.attr("type").unwrap_or("num") {
        // The classic section's own bound names are Lowest/Highest (`min`/`max`); a classic
        // `autoMin`/`autoMax` is not in the schema, but a writer that puts one there means
        // Automatic, so it reads as that (and saves with a proper x14 rule).
        "autoMin" => CfValueKind::AutoMin,
        "autoMax" => CfValueKind::AutoMax,
        "min" => CfValueKind::Min,
        "max" => CfValueKind::Max,
        "percent" => n.map(CfValueKind::Percent).unwrap_or_else(|| CfValueKind::Formula(from_file(val))),
        "percentile" => n.map(CfValueKind::Percentile).unwrap_or_else(|| CfValueKind::Formula(from_file(val))),
        "formula" => CfValueKind::Formula(from_file(val)),
        _ => n.map(CfValueKind::Number).unwrap_or_else(|| CfValueKind::Formula(from_file(val))),
    }
}

fn cfvos(e: &El) -> Vec<CfValueKind> {
    e.kids("cfvo")
        .map(|c| {
            if c.attr("val").is_none()
                && let Some(f) = c.child("f")
            {
                let mut c2 = c.clone();
                c2.attrs.push(("val".into(), f.text.clone()));
                return cfvo(&c2);
            }
            cfvo(c)
        })
        .collect()
}

pub(super) fn read_cf(cx: &mut Ctx<'_>, e: &El, sheet: &mut Sheet) {
    let ranges = parse_sqref(e.attr("sqref"));
    if ranges.is_empty() {
        return;
    }
    for r in e.kids("cfRule") {
        let style = Box::new(r.attr_u32("dxfId").and_then(|i| cx.dxfs.get(i as usize)).cloned().unwrap_or_default());
        let formulas: Vec<String> = r.kids("formula").map(|f| from_file(f.text.trim())).collect();
        let f0 = formulas.first().cloned().unwrap_or_default();
        let text = r.attr("text").unwrap_or("").to_string();
        // The x14 id that links a data bar to its Excel 2010 extension (solid fill lives there).
        // The x14 id that links a rule to its Excel 2010 extension: the id ext sits inside the
        // rule's `dataBar`/`iconSet`/`colorScale` element (Excel's shape) or as a direct child
        // (PR #610's), and is capped against hostile lengths.
        let ext_id = capped_id(&x14_rule_id(r));
        let rule = match r.attr("type").unwrap_or("") {
            "cellIs" => CfRule::CellIs { op: cf_op(r.attr("operator")), a: f0, b: formulas.get(1).cloned(), style },
            "expression" => CfRule::Expression { formula: f0, style },
            "containsText" => CfRule::ContainsText { text, style },
            "notContainsText" => CfRule::NotContainsText { text, style },
            "beginsWith" => CfRule::BeginsWith { text, style },
            "endsWith" => CfRule::EndsWith { text, style },
            "containsBlanks" => CfRule::Blanks { style },
            "notContainsBlanks" => CfRule::NoBlanks { style },
            "containsErrors" => CfRule::Errors { style },
            "notContainsErrors" => CfRule::NoErrors { style },
            "duplicateValues" => CfRule::Duplicate { style },
            "uniqueValues" => CfRule::Unique { style },
            "top10" => {
                CfRule::Top10 { bottom: r.flag("bottom", false), percent: r.flag("percent", false), rank: r.attr_u32("rank").unwrap_or(10), style }
            }
            "aboveAverage" => CfRule::AboveAverage {
                below: !r.flag("aboveAverage", true),
                equal: r.flag("equalAverage", false),
                std_dev: r.attr_u32("stdDev").unwrap_or(0).min(3) as u8,
                style,
            },
            "timePeriod" => CfRule::TimePeriod { period: r.attr("timePeriod").unwrap_or("today").to_string(), style },
            "colorScale" => {
                let Some(cs) = r.child("colorScale") else { continue };
                let colors = cs.kids("color").map(|c| read_color(c, &cx.palette));
                CfRule::ColorScale { stops: cfvos(cs).into_iter().zip(colors).collect() }
            }
            "dataBar" => {
                let Some(db) = r.child("dataBar") else { continue };
                let v = cfvos(db);
                CfRule::DataBar {
                    min: v.first().cloned().unwrap_or(CfValueKind::Min),
                    max: v.get(1).cloned().unwrap_or(CfValueKind::Max),
                    color: db.child("color").map(|c| read_color(c, &cx.palette)).unwrap_or_default(),
                    gradient: true,
                    show_value: db.flag("showValue", true),
                    border: None,
                    negative_fill: None,
                    negative_border: None,
                    axis: None,
                    limits: None,
                }
            }
            "iconSet" => {
                let Some(is) = r.child("iconSet") else { continue };
                CfRule::IconSet {
                    set: is.attr("iconSet").unwrap_or("3TrafficLights1").to_string(),
                    thresholds: cfvos(is),
                    reverse: is.flag("reverse", false),
                    show_value: is.flag("showValue", true),
                    icons: None,
                }
            }
            other => {
                cx.warn(format!("conditional format type \"{other}\" is not supported"));
                continue;
            }
        };
        if !ext_id.is_empty() {
            cx.cf_ext_ids.push((ext_id, sheet.cond_formats.len()));
        }
        let rules = sheet.cond_formats.len();
        let area = ranges.iter().map(RangeRef::count).sum::<u64>().saturating_mul(1 + rules as u64);
        if area > MAX_COND_CELLS {
            cx.warn(format!("conditional formats past {MAX_COND_CELLS} kept range cells were dropped"));
            break;
        }
        sheet.cond_formats.push(CondFormat {
            ranges: ranges.clone(),
            rule,
            priority: r.attr_u32("priority").unwrap_or(0),
            stop_if_true: r.flag("stopIfTrue", false),
        });
    }
}

/// A `dataValidation` (main namespace, or x14 with `xm:f` formulas and an `xm:sqref` child).
pub(super) fn read_validation(v: &El) -> Option<Validation> {
    let sq = v.attr("sqref").map(str::to_string).or_else(|| v.child("sqref").map(|s| s.text.clone()));
    let ranges = parse_sqref(sq.as_deref());
    if ranges.is_empty() {
        return None;
    }
    let formula = |n: &str| -> Option<String> {
        let f = v.child(n)?;
        let t = f.child("f").map(|x| x.text.as_str()).unwrap_or(f.text.as_str()).trim();
        if t.is_empty() { None } else { Some(t.to_string()) }
    };
    let kind = match v.attr("type").unwrap_or("none") {
        "whole" => ValidationKind::Whole,
        "decimal" => ValidationKind::Decimal,
        "list" => ValidationKind::List,
        "date" => ValidationKind::Date,
        "time" => ValidationKind::Time,
        "textLength" => ValidationKind::TextLength,
        "custom" => ValidationKind::Custom,
        _ => ValidationKind::Any,
    };
    let mut f1 = formula("formula1").unwrap_or_default();
    if kind == ValidationKind::List && !f1.is_empty() && !f1.starts_with('"') {
        f1 = format!("={}", from_file(&f1));
    } else if !f1.starts_with('"') && !f1.is_empty() {
        f1 = from_file(&f1);
    }
    Some(Validation {
        ranges,
        kind,
        op: cf_op(v.attr("operator")),
        f1,
        f2: formula("formula2").map(|f| from_file(&f)),
        allow_blank: v.flag("allowBlank", false),
        // `showDropDown="1"` *hides* the in-cell list arrow.
        in_cell_dropdown: !v.flag("showDropDown", false),
        input_title: v.attr("promptTitle").unwrap_or("").to_string(),
        input_message: v.attr("prompt").unwrap_or("").to_string(),
        show_input: v.flag("showInputMessage", false),
        error_title: v.attr("errorTitle").unwrap_or("").to_string(),
        error_message: v.attr("error").unwrap_or("").to_string(),
        error_style: match v.attr("errorStyle") {
            Some("warning") => ErrorStyle::Warning,
            Some("information") => ErrorStyle::Information,
            _ => ErrorStyle::Stop,
        },
        show_error: v.flag("showErrorMessage", false),
    })
}

/// The `x14:id` linking a classic conditional-format rule to its x14 extension: the id ext
/// inside the rule's `dataBar`/`iconSet`/`colorScale` element, or (PR #610's shape) the rule's
/// own extLst. Empty when the rule carries none.
fn x14_rule_id(r: &El) -> String {
    ["dataBar", "iconSet", "colorScale"]
        .iter()
        .find_map(|n| r.child(n).and_then(|e| e.child("extLst")).and_then(ext_lst_id))
        .or_else(|| r.child("extLst").and_then(ext_lst_id))
        .unwrap_or_default()
}

/// x14 conditional-format extensions (Excel 2010): each `x14:cfRule` either refines the classic
/// rule linked by its GUID (data bars, icon sets) or — with no classic counterpart — stands on
/// its own (x14-only icon sets like `5Boxes`, or expression rules with an inline dxf). Anything
/// unmatched or unmodelled warns once, honestly.
pub(super) fn read_x14_conditional_formats(cx: &mut Ctx<'_>, c: &El, sheet: &mut Sheet) {
    let mut skipped = false;
    for cf in c.kids("conditionalFormatting") {
        for r in cf.kids("cfRule") {
            let id = r.attr("id").unwrap_or("");
            let Some(index) = cx.cf_ext_ids.iter().find(|(sid, _)| sid == id).map(|(_, i)| *i) else {
                // No classic rule carries this id: an x14-only rule over its `xm:sqref`.
                skipped |= read_x14_only_rule(cx, cf, r, sheet);
                continue;
            };
            let Some(rule) = sheet.cond_formats.get_mut(index).map(|f| &mut f.rule) else {
                skipped = true;
                continue;
            };
            if let Some(db) = r.child("dataBar") {
                skipped |= merge_x14_data_bar(cx, db, rule);
            } else if let Some(is) = r.child("iconSet") {
                skipped |= merge_x14_icon_set(is, rule);
            } else {
                skipped = true;
            }
        }
    }
    if skipped {
        cx.warn("some Excel 2010 conditional format extensions were skipped");
    }
}

/// An x14 rule with no classic counterpart (an x14-only icon set like `5Boxes`, or a standalone
/// expression rule): a complete conditional format over the `xm:sqref` of its
/// `x14:conditionalFormatting` parent, carrying the x14 rule's priority. Returns whether it had
/// to be skipped.
fn read_x14_only_rule(cx: &Ctx<'_>, cf: &El, r: &El, sheet: &mut Sheet) -> bool {
    // The rule is dropped when any sqref token won't parse: importing the rest would quietly
    // change what the format covers.
    let sq = cf.child("sqref").map(|s| s.text.as_str()).unwrap_or("");
    let ranges = parse_sqref(Some(sq));
    if ranges.is_empty() || ranges.len() != sq.split_whitespace().count() {
        return true;
    }
    if let Some(is) = r.child("iconSet") {
        let skipped = unmodelled_icon_set(is);
        let mut thresholds = Vec::new();
        for v in is.kids("cfvo") {
            // A threshold we can't model would leave a hole the icons shift into.
            let Some(kind) = x14_cfvo(v) else { return true };
            thresholds.push(kind);
        }
        let (icons, truncated) = x14_icons(is);
        sheet.cond_formats.push(CondFormat {
            ranges,
            rule: CfRule::IconSet {
                set: is.attr("iconSet").unwrap_or("3TrafficLights1").to_string(),
                thresholds,
                reverse: is.flag("reverse", false),
                show_value: is.flag("showValue", true),
                icons,
            },
            priority: r.attr_u32("priority").unwrap_or(0),
            stop_if_true: false,
        });
        return skipped || truncated;
    }
    if r.attr("type") == Some("expression") {
        return read_x14_only_expression(cx, r, ranges, sheet);
    }
    true
}

/// An x14-only expression rule: an ordinary model expression rule — the formula from `xm:f`
/// through the usual file conversion, the style from the inline `<x14:dxf>`, whose child shape
/// matches a styles.xml dxf. A missing dxf is a legal style-less rule; a missing or empty
/// formula drops the rule. Anything outside the modelled shape warns. Returns whether the rule
/// Merges an x14 data bar into the classic rule it refines. Returns whether any part of the
/// extension had to be skipped; a rule that isn't a data bar is skipped wholesale.
fn merge_x14_data_bar(cx: &Ctx<'_>, db: &El, rule: &mut CfRule) -> bool {
    let CfRule::DataBar { min, max, color, gradient, border, negative_fill, negative_border, axis, limits, .. } = rule else {
        return true;
    };
    let mut skipped = false;
    for (name, _) in &db.attrs {
        // `direction` is checked separately below: only a non-default direction is unmodelled.
        if name == "direction" {
            continue;
        }
        if !matches!(
            name.as_str(),
            "minLength"
                | "maxLength"
                | "showValue"
                | "border"
                | "gradient"
                | "negativeBarColorSameAsPositive"
                | "negativeBarBorderColorSameAsPositive"
                | "axisPosition"
        ) {
            skipped = true;
        }
    }
    // Bar direction has no modelled home: an rtl bar would save back as ltr, so a direction
    // other than Excel's schema default ("context") warns instead of silently flipping it.
    if db.attr("direction").is_some_and(|d| d != "context") {
        skipped = true;
    }
    for child in &db.children {
        if !matches!(child.name.as_str(), "cfvo" | "fillColor" | "borderColor" | "negativeFillColor" | "negativeBorderColor" | "axisColor") {
            skipped = true;
        }
    }
    *gradient = db.flag("gradient", true);
    let (lo, hi) = (db.attr_u32("minLength"), db.attr_u32("maxLength"));
    *limits = match (lo, hi) {
        (None, None) => None,
        (lo, hi) => Some((lo.unwrap_or(10).min(100) as u8, hi.unwrap_or(90).min(100) as u8)),
    };
    // MS-XLSX §2.6.30: the colour children are flat color-attribute elements (`rgb`/`theme`/
    // `tint`/`auto` directly, like the classic `<color>`); `fillColor` overrides the classic
    // bar colour. No negative-fill element means Excel's default red — the model's `None` —
    // unless a same-as-positive flag pins it to the positive colour.
    let flat = |name: &str| db.child(name).map(|e| read_color(e, &cx.palette));
    if let Some(c) = flat("fillColor") {
        *color = c;
    }
    *border = flat("borderColor");
    *negative_fill = flat("negativeFillColor");
    *negative_border = flat("negativeBorderColor");
    *axis = flat("axisColor");
    if db.flag("negativeBarColorSameAsPositive", false) {
        *negative_fill = Some(*color);
    }
    if db.attr("axisPosition") == Some("none") {
        *axis = None;
    }
    let cfvos: Vec<&El> = db.kids("cfvo").collect();
    if cfvos.len() > 2 {
        skipped = true;
    }
    for (slot, v) in [min, max].into_iter().zip(cfvos) {
        if let Some(kind) = x14_cfvo(v) {
            *slot = kind;
        } else {
            // Unusable bound: keep the classic one and say so.
            skipped = true;
        }
    }
    skipped
}

/// One x14 `cfvo`, its value in an `xm:f` child (`autoMin`/`autoMax`/`min`/`max` carry none).
/// `autoMin`/`autoMax` are the data bar's Automatic bounds, apart from Lowest/Highest Value
/// (`min`/`max`) as in Excel (#1159). `None` when the type or its value can't be used.
fn x14_cfvo(e: &El) -> Option<CfValueKind> {
    let t = e.attr("type").unwrap_or("num");
    if t == "autoMin" {
        return Some(CfValueKind::AutoMin);
    }
    if t == "autoMax" {
        return Some(CfValueKind::AutoMax);
    }
    if t == "min" {
        return Some(CfValueKind::Min);
    }
    if t == "max" {
        return Some(CfValueKind::Max);
    }
    let f = e.child("f").map(|f| f.text.trim()).filter(|f| !f.is_empty())?;
    let n = || f.parse::<f64>().ok().filter(|v| v.is_finite());
    match t {
        "num" => n().map(CfValueKind::Number),
        "percent" => n().map(CfValueKind::Percent),
        "percentile" => n().map(CfValueKind::Percentile),
        "formula" => Some(CfValueKind::Formula(from_file(f))),
        _ => None,
    }
}

/// Merges an x14 icon set into the classic rule it refines: restated cfvo thresholds and custom
/// per-position icons (`x14:cfIcon`). Returns whether any part of the extension had to be
/// skipped; a rule that isn't an icon set is skipped wholesale.
fn merge_x14_icon_set(is: &El, rule: &mut CfRule) -> bool {
    let CfRule::IconSet { set, thresholds, reverse, show_value, icons, .. } = rule else {
        return true;
    };
    let mut skipped = unmodelled_icon_set(is);
    if let Some(name) = is.attr("iconSet") {
        *set = name.to_string();
    }
    *reverse = is.flag("reverse", *reverse);
    *show_value = is.flag("showValue", *show_value);
    let cfvos: Vec<&El> = is.kids("cfvo").collect();
    if cfvos.len() > thresholds.len() {
        skipped = true;
    }
    for (slot, v) in thresholds.iter_mut().zip(cfvos) {
        if let Some(kind) = x14_cfvo(v) {
            *slot = kind;
        } else {
            // Unusable bound: keep the classic one and say so.
            skipped = true;
        }
    }
    let (list, truncated) = x14_icons(is);
    *icons = list;
    skipped || truncated
}

/// The custom per-position icon choices (`x14:cfIcon`) of an x14 icon set: `None` when it has
/// none. The list is capped at 5 (no icon set has more) so a hostile file can't balloon it;
/// truncation and unusable entries are reported so the reader can warn.
fn x14_icons(is: &El) -> (Option<Vec<IconChoice>>, bool) {
    let mut skipped = false;
    let mut icons: Vec<IconChoice> = Vec::new();
    for e in is.kids("cfIcon") {
        if icons.len() == 5 {
            skipped = true;
            break;
        }
        match (e.attr("iconSet"), e.attr_u32("iconId").and_then(|i| u8::try_from(i).ok())) {
            (Some(set), Some(id)) => icons.push(IconChoice { set: set.to_string(), id }),
            _ => skipped = true,
        }
    }
    (if icons.is_empty() { None } else { Some(icons) }, skipped)
}

/// Merges an x14 data bar into the classic rule it refines. Returns whether any part of the
/// Whether an `x14:iconSet` element carries anything outside the modelled attributes/children.
fn unmodelled_icon_set(is: &El) -> bool {
    is.attrs.iter().any(|(n, _)| !matches!(n.as_str(), "iconSet" | "custom" | "reverse" | "showValue"))
        || is.children.iter().any(|c| !matches!(c.name.as_str(), "cfvo" | "cfIcon"))
}

/// The custom per-position icon choices (`x14:cfIcon`) of an x14 icon set: `None` when it has
/// none. The list is capped at 5 (no icon set has more) so a hostile file can't balloon it;
/// A stored x14 id, capped at 256 bytes on a char boundary so a hostile file can't balloon it.
fn capped_id(id: &str) -> String {
    let mut end = id.len().min(256);
    while !id.is_char_boundary(end) {
        end -= 1;
    }
    id.get(..end).unwrap_or_default().to_string()
}

fn ext_lst_id(e: &El) -> Option<String> {
    let id = e.kids("ext").find_map(|x| x.child("id"))?.text.trim().to_string();
    (!id.is_empty()).then_some(id)
}

fn read_x14_only_expression(cx: &Ctx<'_>, r: &El, ranges: Vec<RangeRef>, sheet: &mut Sheet) -> bool {
    // Known attributes only: `type`, `priority` and the `id` (unused here — an x14-only rule has
    // no classic counterpart to link).
    let mut skipped = r.attrs.iter().any(|(n, _)| !matches!(n.as_str(), "type" | "priority" | "id"));
    // No formula, no rule: the condition is all an expression rule is.
    let Some(formula) = r.child("f").map(|f| f.text.trim()).filter(|f| !f.is_empty()) else {
        return true;
    };
    let style = r.child("dxf").map(|d| read_dxf(d, &cx.palette, &cx.num_fmts)).unwrap_or_default();
    skipped |= r.children.iter().any(|c| !matches!(c.name.as_str(), "f" | "dxf"));
    sheet.cond_formats.push(CondFormat {
        ranges,
        rule: CfRule::Expression { formula: from_file(formula), style: Box::new(style) },
        priority: r.attr_u32("priority").unwrap_or(0),
        stop_if_true: false,
    });
    skipped
}
