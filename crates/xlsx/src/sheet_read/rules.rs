use super::*;

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
        "min" | "autoMin" => CfValueKind::Min,
        "max" | "autoMax" => CfValueKind::Max,
        "percent" => n.map(CfValueKind::Percent).unwrap_or_else(|| CfValueKind::Formula(from_file(val))),
        "percentile" => n.map(CfValueKind::Percentile).unwrap_or_else(|| CfValueKind::Formula(from_file(val))),
        "formula" => CfValueKind::Formula(from_file(val)),
        _ => n.map(CfValueKind::Number).unwrap_or_else(|| CfValueKind::Formula(from_file(val))),
    }
}

/// `cfvo` elements: plain children, or x14 ones with the value in an `xm:f` child.
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
        let ext_id =
            r.child("extLst").and_then(|l| l.kids("ext").find_map(|x| x.child("id"))).map(|i| i.text.trim().to_string()).filter(|i| !i.is_empty());
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
                }
            }
            "iconSet" => {
                let Some(is) = r.child("iconSet") else { continue };
                CfRule::IconSet {
                    set: is.attr("iconSet").unwrap_or("3TrafficLights1").to_string(),
                    thresholds: cfvos(is),
                    reverse: is.flag("reverse", false),
                    show_value: is.flag("showValue", true),
                }
            }
            other => {
                cx.warn(format!("conditional format type \"{other}\" is not supported"));
                continue;
            }
        };
        if let Some(id) = ext_id {
            cx.cf_ext_ids.push((id, sheet.cond_formats.len()));
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
