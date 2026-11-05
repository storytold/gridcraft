use super::*;

fn cf_op_name(op: CfOperator) -> &'static str {
    match op {
        CfOperator::Between => "between",
        CfOperator::NotBetween => "notBetween",
        CfOperator::Equal => "equal",
        CfOperator::NotEqual => "notEqual",
        CfOperator::Greater => "greaterThan",
        CfOperator::Less => "lessThan",
        CfOperator::GreaterOrEqual => "greaterThanOrEqual",
        CfOperator::LessOrEqual => "lessThanOrEqual",
    }
}

fn cfvo_xml(v: &CfValueKind) -> String {
    match v {
        // The classic section cannot tell Automatic from Lowest/Highest (both are `min`/`max`;
        // only the x14 extension can — see `x14_cfvo_xml`), which is why a rule with an
        // Automatic bound always writes one.
        CfValueKind::AutoMin => "<cfvo type=\"min\"/>".into(),
        CfValueKind::AutoMax => "<cfvo type=\"max\"/>".into(),
        CfValueKind::Min => "<cfvo type=\"min\"/>".into(),
        CfValueKind::Max => "<cfvo type=\"max\"/>".into(),
        CfValueKind::Number(n) => format!("<cfvo type=\"num\" val=\"{}\"/>", num(*n)),
        CfValueKind::Percent(n) => format!("<cfvo type=\"percent\" val=\"{}\"/>", num(*n)),
        CfValueKind::Percentile(n) => format!("<cfvo type=\"percentile\" val=\"{}\"/>", num(*n)),
        CfValueKind::Formula(f) => format!("<cfvo type=\"formula\" val=\"{}\"/>", esc_attr(&text_to_file(f))),
    }
}
fn quote(t: &str) -> String {
    format!("\"{}\"", t.replace('"', "\"\""))
}

/// The x14 (Excel 2010) extension namespace, and the one its formula/sqref values live in.
pub(super) const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
pub(super) const NS_XM: &str = "http://schemas.microsoft.com/office/excel/2006/main";
/// Worksheet `ext` uri of the conditional-format extensions. Its block is written before the
/// sparkline ext that shares the worksheet's `extLst`.
pub(super) const URI_X14_CF: &str = "{78C0D931-6437-407d-A8EE-F0AAD7539E65}";
/// `ext` uri linking a classic `dataBar`/`iconSet` rule to its x14 refinement (the `x14:id`).
const URI_X14_ID: &str = "{B025F937-C7B1-47D3-B67F-A62EFF666E3E}";
/// Worksheet `ext` uri of sparklines.
pub(super) const URI_SPARKLINES: &str = "{05C60535-1F16-4fd2-B633-F4F36F0B64E0}";

/// Per-rule x14 GUID, derived from the rule's priority — unique per sheet because priorities
/// are renumbered 1..n — as `{00000000-0000-0000-0000-PPPPPPPPPPPP}` (PR #610's scheme, credit
/// @MarekSpodBiedry).
fn x14_guid(priority: u32) -> String {
    format!("{{00000000-0000-0000-0000-{priority:012}}}")
}

/// The id ext linking a classic rule element to its x14 refinement, placed inside the rule
/// element (`<dataBar>`/`<iconSet>`) — Excel's shape, which Excel 2007 quietly ignores.
fn id_ext(priority: u32) -> String {
    format!("<extLst><ext xmlns:x14=\"{NS_X14}\" uri=\"{URI_X14_ID}\"><x14:id>{}</x14:id></ext></extLst>", x14_guid(priority))
}

/// One x14 `cfvo`: the value is an `xm:f` child, never a `val` attribute; the bound types that
/// carry no value are `autoMin`/`autoMax` (a data bar's Automatic bounds) and `min`/`max`
/// (Lowest/Highest Value) — the distinction the classic section cannot make (#1159).
fn x14_cfvo_xml(v: &CfValueKind) -> String {
    match v {
        CfValueKind::AutoMin => "<x14:cfvo type=\"autoMin\"/>".into(),
        CfValueKind::AutoMax => "<x14:cfvo type=\"autoMax\"/>".into(),
        CfValueKind::Min => "<x14:cfvo type=\"min\"/>".into(),
        CfValueKind::Max => "<x14:cfvo type=\"max\"/>".into(),
        CfValueKind::Number(n) => format!("<x14:cfvo type=\"num\"><xm:f>{}</xm:f></x14:cfvo>", num(*n)),
        CfValueKind::Percent(n) => format!("<x14:cfvo type=\"percent\"><xm:f>{}</xm:f></x14:cfvo>", num(*n)),
        CfValueKind::Percentile(n) => format!("<x14:cfvo type=\"percentile\"><xm:f>{}</xm:f></x14:cfvo>", num(*n)),
        CfValueKind::Formula(f) => format!("<x14:cfvo type=\"formula\"><xm:f>{}</xm:f></x14:cfvo>", esc(&text_to_file(f))),
    }
}

/// The icon sets Excel 2010 added in x14 only (`3Stars`, `3Triangles`, `5Boxes`): a rule using
/// one is written as an x14-only rule with no classic counterpart, because the classic
/// `iconSet` attribute has no value for them.
fn x14_only_icon_set(set: &str) -> bool {
    matches!(set, "3Stars" | "3Triangles" | "5Boxes")
}

/// The cfvos of an icon-set rule: one per icon (`CT_IconSet` wants at least two), the first
/// being the lower bound — the stored thresholds when complete, the lower bound prepended when
/// one short, Excel's defaults otherwise.
fn icon_cfvos(set: &str, thresholds: &[CfValueKind]) -> Vec<CfValueKind> {
    let defaults = default_icon_thresholds(set);
    match thresholds.len() {
        n if n == defaults.len() => thresholds.to_vec(),
        n if n + 1 == defaults.len() => defaults.iter().take(1).cloned().chain(thresholds.iter().cloned()).collect(),
        _ => defaults,
    }
}

/// The `x14:cfRule` body of an icon-set extension: the set restated with its thresholds as
/// `xm:f` cfvos plus the custom per-position icon choices. Identical whether the rule refines
/// a classic rule or stands alone; only the classic counterpart differs.
fn x14_icon_set_xml(set: &str, thresholds: &[CfValueKind], reverse: bool, show_value: bool, icons: &[IconChoice], priority: u32) -> String {
    let mut b =
        format!("<x14:cfRule type=\"iconSet\" id=\"{}\" priority=\"{priority}\"><x14:iconSet iconSet=\"{}\"", x14_guid(priority), esc_attr(set));
    if !icons.is_empty() {
        b.push_str(" custom=\"1\"");
    }
    if reverse {
        b.push_str(" reverse=\"1\"");
    }
    if !show_value {
        b.push_str(" showValue=\"0\"");
    }
    b.push('>');
    for t in &icon_cfvos(set, thresholds) {
        b.push_str(&x14_cfvo_xml(t));
    }
    for ic in icons {
        let _ = write!(b, "<x14:cfIcon iconSet=\"{}\" iconId=\"{}\"/>", esc_attr(&ic.set), ic.id);
    }
    b.push_str("</x14:iconSet></x14:cfRule>");
    b
}

/// A conditional format's markup split by destination: the classic rule (`None` when the rule
/// is x14-only — its icon set has no classic value), and the body of its `x14:cfRule` (`None`
/// when nothing the model carries needs an extension; a plain gradient bar must not grow one).
pub(super) struct CfXml {
    pub(super) classic: Option<String>,
    pub(super) x14: Option<String>,
}

pub(super) fn cf_rule_xml(cf: &CondFormat, priority: u32, out: &mut Out) -> CfXml {
    // Generated formulas refer to the top-left cell of all the rule's areas, the cell Excel
    // reads relative references from.
    let tl = RangeRef::top_left(&cf.ranges).map(|c| c.a1()).unwrap_or_else(|| "A1".into());
    let stop = if cf.stop_if_true { " stopIfTrue=\"1\"" } else { "" };
    let mut dxf = |st: &Style| dxf_id(&mut out.dxfs, st);
    let f = |x: &str| format!("<formula>{}</formula>", esc(&text_to_file(x)));
    let simple = |ty: &str, st: &Style, extra: &str, body: String, dxf: &mut dyn FnMut(&Style) -> u32| CfXml {
        classic: Some(format!("<cfRule type=\"{ty}\" dxfId=\"{}\" priority=\"{priority}\"{stop}{extra}>{body}</cfRule>", dxf(st))),
        x14: None,
    };
    match &cf.rule {
        CfRule::CellIs { op, a, b, style } => {
            let mut body = f(a);
            if let Some(b) = b {
                body.push_str(&f(b));
            }
            simple("cellIs", style, &format!(" operator=\"{}\"", cf_op_name(*op)), body, &mut dxf)
        }
        CfRule::Expression { formula, style } => simple("expression", style, "", f(formula), &mut dxf),
        CfRule::ContainsText { text, style } => simple(
            "containsText",
            style,
            &format!(" operator=\"containsText\" text=\"{}\"", esc_attr(text)),
            f(&format!("NOT(ISERROR(SEARCH({},{tl})))", quote(text))),
            &mut dxf,
        ),
        CfRule::NotContainsText { text, style } => simple(
            "notContainsText",
            style,
            &format!(" operator=\"notContains\" text=\"{}\"", esc_attr(text)),
            f(&format!("ISERROR(SEARCH({},{tl}))", quote(text))),
            &mut dxf,
        ),
        CfRule::BeginsWith { text, style } => simple(
            "beginsWith",
            style,
            &format!(" operator=\"beginsWith\" text=\"{}\"", esc_attr(text)),
            f(&format!("LEFT({tl},LEN({0}))={0}", quote(text))),
            &mut dxf,
        ),
        CfRule::EndsWith { text, style } => simple(
            "endsWith",
            style,
            &format!(" operator=\"endsWith\" text=\"{}\"", esc_attr(text)),
            f(&format!("RIGHT({tl},LEN({0}))={0}", quote(text))),
            &mut dxf,
        ),
        CfRule::Blanks { style } => simple("containsBlanks", style, "", f(&format!("LEN(TRIM({tl}))=0")), &mut dxf),
        CfRule::NoBlanks { style } => simple("notContainsBlanks", style, "", f(&format!("LEN(TRIM({tl}))>0")), &mut dxf),
        CfRule::Errors { style } => simple("containsErrors", style, "", f(&format!("ISERROR({tl})")), &mut dxf),
        CfRule::NoErrors { style } => simple("notContainsErrors", style, "", f(&format!("NOT(ISERROR({tl}))")), &mut dxf),
        CfRule::Duplicate { style } => simple("duplicateValues", style, "", String::new(), &mut dxf),
        CfRule::Unique { style } => simple("uniqueValues", style, "", String::new(), &mut dxf),
        CfRule::Top10 { bottom, percent, rank, style } => {
            let extra = format!("{}{} rank=\"{}\"", if *percent { " percent=\"1\"" } else { "" }, if *bottom { " bottom=\"1\"" } else { "" }, rank);
            simple("top10", style, &extra, String::new(), &mut dxf)
        }
        CfRule::AboveAverage { below, equal, std_dev, style } => {
            let mut extra = String::new();
            if *below {
                extra.push_str(" aboveAverage=\"0\"");
            }
            if *equal {
                extra.push_str(" equalAverage=\"1\"");
            }
            if *std_dev > 0 {
                let _ = write!(extra, " stdDev=\"{std_dev}\"");
            }
            simple("aboveAverage", style, &extra, String::new(), &mut dxf)
        }
        CfRule::TimePeriod { period, style } => {
            let d = format!("FLOOR({tl},1)");
            let body = match period.as_str() {
                "yesterday" => format!("{d}=TODAY()-1"),
                "tomorrow" => format!("{d}=TODAY()+1"),
                "last7Days" => format!("AND(TODAY()-{d}<=6,{d}<=TODAY())"),
                "thisMonth" => format!("AND(MONTH({tl})=MONTH(TODAY()),YEAR({tl})=YEAR(TODAY()))"),
                "lastMonth" => format!("AND(MONTH({tl})=MONTH(EDATE(TODAY(),0-1)),YEAR({tl})=YEAR(EDATE(TODAY(),0-1)))"),
                "nextMonth" => format!("AND(MONTH({tl})=MONTH(EDATE(TODAY(),0+1)),YEAR({tl})=YEAR(EDATE(TODAY(),0+1)))"),
                "thisWeek" => format!("AND(TODAY()-ROUNDDOWN({tl},0)<=WEEKDAY(TODAY())-1,ROUNDDOWN({tl},0)-TODAY()<=7-WEEKDAY(TODAY()))"),
                "lastWeek" => format!("AND(TODAY()-ROUNDDOWN({tl},0)>=(WEEKDAY(TODAY())),TODAY()-ROUNDDOWN({tl},0)<(WEEKDAY(TODAY())+7))"),
                "nextWeek" => format!("AND(ROUNDDOWN({tl},0)-TODAY()>(7-WEEKDAY(TODAY())),ROUNDDOWN({tl},0)-TODAY()<(15-WEEKDAY(TODAY())))"),
                _ => format!("{d}=TODAY()"),
            };
            simple("timePeriod", style, &format!(" timePeriod=\"{}\"", esc_attr(period)), f(&body), &mut dxf)
        }
        CfRule::ColorScale { stops } => {
            let mut b = String::from("<colorScale>");
            for (v, _) in stops {
                b.push_str(&cfvo_xml(v));
            }
            for (_, c) in stops {
                let _ = write!(b, "<color {}/>", color_attrs(c));
            }
            b.push_str("</colorScale>");
            CfXml { classic: Some(format!("<cfRule type=\"colorScale\" priority=\"{priority}\"{stop}>{b}</cfRule>")), x14: None }
        }
        CfRule::DataBar { min, max, color, gradient, show_value, border, negative_fill, negative_border, axis, limits } => {
            // Solid fill, border, negative colours, axis, length limits and Automatic bounds
            // (which the classic section cannot express — `min`/`max` there are Lowest/Highest)
            // exist only in x14; a plain gradient bar is fully classic and must not grow an
            // extension.
            let extended = !*gradient
                || border.is_some()
                || negative_fill.is_some()
                || negative_border.is_some()
                || axis.is_some()
                || limits.is_some()
                || matches!(min, CfValueKind::AutoMin | CfValueKind::AutoMax)
                || matches!(max, CfValueKind::AutoMin | CfValueKind::AutoMax);
            let col = if *color == Color::Auto { "rgb=\"FF638EC6\"".to_string() } else { color_attrs(color) };
            let x14 = extended.then(|| {
                let mut a = String::new();
                if let Some((lo, hi)) = limits {
                    let _ = write!(a, " minLength=\"{lo}\" maxLength=\"{hi}\"");
                }
                // The classic side writes `showValue="0"` when values are hidden; the x14 side
                // must mirror it or an Excel that merges the refinement over the classic rule
                // would bring the values back.
                if !*show_value {
                    a.push_str(" showValue=\"0\"");
                }
                if !*gradient {
                    a.push_str(" gradient=\"0\"");
                }
                // MS-XLSX MUST-exist-iff: each same-as-positive flag is "0" exactly when its
                // colour element exists; a missing pair is Excel's default (negative fill red,
                // negative border same as positive), which is what the model's `None` means.
                // `negativeBorderColor` additionally requires `border="1"` — a negative border
                // without a border is meaningless — so a negative border colour forces the
                // border on with an automatic colour. One-way normalisation: the schema cannot
                // express `border: None, negative_border: Some`, which reads back with
                // `border: Some(_)`.
                let border_color = (*border).or_else(|| negative_border.is_some().then_some(Color::Auto));
                if border_color.is_some() {
                    a.push_str(" border=\"1\"");
                }
                if negative_fill.is_some() {
                    a.push_str(" negativeBarColorSameAsPositive=\"0\"");
                }
                if negative_border.is_some() {
                    a.push_str(" negativeBarBorderColorSameAsPositive=\"0\"");
                }
                if axis.is_none() {
                    a.push_str(" axisPosition=\"none\"");
                }
                let mut b = format!("<x14:cfRule type=\"dataBar\" id=\"{}\" priority=\"{priority}\"><x14:dataBar{a}>", x14_guid(priority));
                b.push_str(&x14_cfvo_xml(min));
                b.push_str(&x14_cfvo_xml(max));
                // Schema sequence: cfvo ×2, then the colour elements — flat color-attribute
                // elements (MS-XLSX §2.6.30), fillColor mirroring the classic colour (it MUST
                // exist whenever the rule carries a priority, which ours always do).
                let _ = write!(b, "<x14:fillColor {col}/>");
                for (name, c) in [
                    ("borderColor", border_color.as_ref()),
                    ("negativeFillColor", negative_fill.as_ref()),
                    ("negativeBorderColor", negative_border.as_ref()),
                    ("axisColor", axis.as_ref()),
                ] {
                    if let Some(c) = c {
                        let _ = write!(b, "<x14:{name} {}/>", color_attrs(c));
                    }
                }
                b.push_str("</x14:dataBar></x14:cfRule>");
                b
            });
            let id = extended.then(|| id_ext(priority));
            CfXml {
                classic: Some(format!(
                    "<cfRule type=\"dataBar\" priority=\"{priority}\"{stop}><dataBar{}>{}{}<color {col}/>{}</dataBar></cfRule>",
                    if *show_value { "" } else { " showValue=\"0\"" },
                    cfvo_xml(min),
                    cfvo_xml(max),
                    id.as_deref().unwrap_or("")
                )),
                x14,
            }
        }
        CfRule::IconSet { set, thresholds, reverse, show_value, icons } => {
            let custom = icons.as_deref().is_some_and(|v| !v.is_empty());
            let x14_only = x14_only_icon_set(set);
            let mut b = format!("<iconSet iconSet=\"{}\"", esc_attr(set));
            if *reverse {
                b.push_str(" reverse=\"1\"");
            }
            if !*show_value {
                b.push_str(" showValue=\"0\"");
            }
            b.push('>');
            for t in &icon_cfvos(set, thresholds) {
                b.push_str(&cfvo_xml(t));
            }
            if custom {
                b.push_str(&id_ext(priority));
            }
            b.push_str("</iconSet>");
            CfXml {
                classic: (!x14_only).then(|| format!("<cfRule type=\"iconSet\" priority=\"{priority}\"{stop}>{b}</cfRule>")),
                x14: (custom || x14_only)
                    .then(|| x14_icon_set_xml(set, thresholds, *reverse, *show_value, icons.as_deref().unwrap_or_default(), priority)),
            }
        }
    }
}

fn validation_formula(f: &str) -> String {
    let f = f.trim();
    if f.starts_with('"') {
        return esc(f);
    }
    esc(&text_to_file(f.strip_prefix('=').unwrap_or(f)))
}

pub(super) fn validation_xml(v: &Validation) -> String {
    let ty = match v.kind {
        ValidationKind::Any => "none",
        ValidationKind::Whole => "whole",
        ValidationKind::Decimal => "decimal",
        ValidationKind::List => "list",
        ValidationKind::Date => "date",
        ValidationKind::Time => "time",
        ValidationKind::TextLength => "textLength",
        ValidationKind::Custom => "custom",
    };
    let mut s = format!("<dataValidation type=\"{ty}\"");
    match v.error_style {
        ErrorStyle::Stop => {}
        ErrorStyle::Warning => s.push_str(" errorStyle=\"warning\""),
        ErrorStyle::Information => s.push_str(" errorStyle=\"information\""),
    }
    if v.op != CfOperator::Between && !matches!(v.kind, ValidationKind::List | ValidationKind::Custom | ValidationKind::Any) {
        let _ = write!(s, " operator=\"{}\"", cf_op_name(v.op));
    }
    if v.allow_blank {
        s.push_str(" allowBlank=\"1\"");
    }
    if !v.in_cell_dropdown {
        s.push_str(" showDropDown=\"1\"");
    }
    if v.show_input {
        s.push_str(" showInputMessage=\"1\"");
    }
    if v.show_error {
        s.push_str(" showErrorMessage=\"1\"");
    }
    for (a, t) in [("errorTitle", &v.error_title), ("error", &v.error_message), ("promptTitle", &v.input_title), ("prompt", &v.input_message)] {
        if !t.is_empty() {
            let _ = write!(s, " {a}=\"{}\"", esc_attr(t));
        }
    }
    let _ = write!(s, " sqref=\"{}\">", sqref(&v.ranges));
    if !v.f1.is_empty() {
        let _ = write!(s, "<formula1>{}</formula1>", validation_formula(&v.f1));
    }
    if let Some(f2) = v.f2.as_ref().filter(|f| !f.is_empty()) {
        let _ = write!(s, "<formula2>{}</formula2>", validation_formula(f2));
    }
    s.push_str("</dataValidation>");
    s
}
