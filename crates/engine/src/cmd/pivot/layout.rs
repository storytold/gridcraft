use std::collections::{HashMap, HashSet};

use super::create::source_param;
use super::fields::{canonical, in_axes, shift_values, update};
use super::params::{group_param, layout_name};
use super::{
    Chart, Json, PivotDateGroup, PivotField, PivotFilter, PivotLayout, PivotSort, PivotTable, PivotTopOf, PivotValueFilter, PivotValueRule, Result,
    Session, bad, bool_param, edit, find_pivot, json, other, pv, str_param,
};

/// `pivot.filter`: the items a field shows (`null`: all). On a row or column field it keeps any
/// value filter; a filter left hiding nothing goes away.
pub(super) fn filter(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.filter";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let selected: Option<Vec<String>> = match p.get("selected") {
        None | Some(Json::Null) => None,
        Some(Json::Array(a)) => {
            if a.len() > 100_000 {
                return Err(bad(C, "too many items"));
            }
            let v: Vec<String> = a
                .iter()
                .map(|x| match x {
                    Json::String(s) => s.clone(),
                    Json::Null => "(blank)".into(),
                    other => other.to_string(),
                })
                .collect();
            if v.is_empty() {
                return Err(other("You must select at least one item."));
            }
            Some(v)
        }
        Some(Json::String(s)) => Some(vec![s.clone()]),
        Some(_) => return Err(bad(C, "`selected` is a list of item labels or null")),
    };
    update(s, p, |pt, src, _| {
        let field = canonical(C, src, &field)?;
        let i = pt.filters.iter().position(|f| f.source_col.eq_ignore_ascii_case(&field));
        match (i, selected) {
            (Some(i), None) => {
                if let Some(f) = pt.filters.get_mut(i) {
                    f.selected = None;
                }
                // An axis field's filter that hides nothing goes; a report filter stays, showing all.
                if in_axes(pt, &field) && pt.filters.get(i).is_some_and(PivotFilter::is_inactive) {
                    pt.filters.remove(i);
                }
            }
            (Some(i), Some(sel)) => {
                if let Some(f) = pt.filters.get_mut(i) {
                    f.selected = Some(sel);
                }
            }
            (None, Some(sel)) => pt.filters.push(PivotFilter { source_col: field.clone(), selected: Some(sel), value: None }),
            (None, None) => {}
        }
        Ok(json!({"field": field}))
    })
}

/// Most items a Top/Bottom value filter keeps by count.
const MAX_TOP_ITEMS: f64 = 1_048_576.0;

/// The `rule` of a `pivot.valueFilter` call (`None`: clear the field's value filter).
fn value_rule_param(p: &Json) -> Result<Option<PivotValueRule>> {
    const C: &str = "pivot.valueFilter";
    let number = |key: &str| -> Result<f64> {
        p.get(key).and_then(Json::as_f64).filter(|n| n.is_finite()).ok_or_else(|| bad(C, format!("`{key}` is a number")))
    };
    let rule = str_param(p, "rule").ok_or_else(|| bad(C, "missing `rule`"))?.to_ascii_lowercase();
    let ranked = |top: bool| -> Result<PivotValueRule> {
        let of = match str_param(p, "of").unwrap_or("items").to_ascii_lowercase().as_str() {
            "items" | "item" => PivotTopOf::Items,
            "percent" | "%" => PivotTopOf::Percent,
            "sum" => PivotTopOf::Sum,
            o => return Err(bad(C, format!("unknown `of` `{o}` (items|percent|sum)"))),
        };
        let count = p.get("count").map_or(Ok(10.0), |_| number("count"))?;
        let valid = match of {
            PivotTopOf::Items => (1.0..=MAX_TOP_ITEMS).contains(&count) && count.fract() == 0.0,
            PivotTopOf::Percent => count > 0.0 && count <= 100.0,
            PivotTopOf::Sum => true,
        };
        if !valid {
            return Err(bad(C, "`count` is a whole number of items from 1, or a percentage above 0 up to 100"));
        }
        Ok(if top { PivotValueRule::Top { count, of } } else { PivotValueRule::Bottom { count, of } })
    };
    Ok(Some(match rule.as_str() {
        "none" | "clear" => return Ok(None),
        "top" => ranked(true)?,
        "bottom" => ranked(false)?,
        "equal" | "equals" => PivotValueRule::Equal(number("value")?),
        "notequal" => PivotValueRule::NotEqual(number("value")?),
        "greater" | "greaterthan" => PivotValueRule::Greater(number("value")?),
        "greaterorequal" | "greaterthanorequal" => PivotValueRule::GreaterOrEqual(number("value")?),
        "less" | "lessthan" => PivotValueRule::Less(number("value")?),
        "lessorequal" | "lessthanorequal" => PivotValueRule::LessOrEqual(number("value")?),
        "between" => PivotValueRule::Between(number("value")?, number("value2")?),
        "notbetween" => PivotValueRule::NotBetween(number("value")?, number("value2")?),
        r => {
            return Err(bad(
                C,
                format!("unknown rule `{r}` (top|bottom|equal|notEqual|greater|greaterOrEqual|less|lessOrEqual|between|notBetween|none)"),
            ));
        }
    }))
}

/// `pivot.valueFilter`: sets or clears a row or column field's value filter, measured by a value
/// field of the report (the first by default).
pub(super) fn value_filter(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.valueFilter";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let rule = value_rule_param(p)?;
    let by = str_param(p, "by").map(str::to_string);
    update(s, p, |pt, _, _| {
        let source_col = axis_field(C, pt, &field)?.source_col.clone();
        let value = match rule {
            Some(rule) => {
                let by = match &by {
                    Some(name) => value_field_name(C, pt, name)?,
                    None => pt.values.first().map(|v| v.name.clone()).ok_or_else(|| bad(C, "the PivotTable has no value field to filter by"))?,
                };
                Some(PivotValueFilter { by, rule })
            }
            None => None,
        };
        match pt.filters.iter_mut().find(|f| f.source_col.eq_ignore_ascii_case(&source_col)) {
            Some(f) => f.value = value,
            None if value.is_some() => pt.filters.push(PivotFilter { source_col: source_col.clone(), selected: None, value }),
            None => {}
        }
        pt.filters.retain(|f| !(f.source_col.eq_ignore_ascii_case(&source_col) && f.is_inactive()));
        let value = pt.filters.iter().find(|f| f.source_col.eq_ignore_ascii_case(&source_col)).and_then(|f| f.value.as_ref());
        Ok(json!({"field": source_col, "valueFilter": value.map(value_filter_json)}))
    })
}

/// A value filter as `pivot.fields` and `pivot.valueFilter` report it (the parameters that set it).
pub(super) fn value_filter_json(vf: &PivotValueFilter) -> Json {
    let of = |of: PivotTopOf| match of {
        PivotTopOf::Items => "items",
        PivotTopOf::Percent => "percent",
        PivotTopOf::Sum => "sum",
    };
    let (rule, rest) = match vf.rule {
        PivotValueRule::Top { count, of: o } => ("top", json!({"count": count, "of": of(o)})),
        PivotValueRule::Bottom { count, of: o } => ("bottom", json!({"count": count, "of": of(o)})),
        PivotValueRule::Equal(v) => ("equal", json!({"value": v})),
        PivotValueRule::NotEqual(v) => ("notEqual", json!({"value": v})),
        PivotValueRule::Greater(v) => ("greater", json!({"value": v})),
        PivotValueRule::GreaterOrEqual(v) => ("greaterOrEqual", json!({"value": v})),
        PivotValueRule::Less(v) => ("less", json!({"value": v})),
        PivotValueRule::LessOrEqual(v) => ("lessOrEqual", json!({"value": v})),
        PivotValueRule::Between(a, b) => ("between", json!({"value": a, "value2": b})),
        PivotValueRule::NotBetween(a, b) => ("notBetween", json!({"value": a, "value2": b})),
    };
    let mut out = json!({"by": vf.by, "rule": rule});
    if let (Some(m), Some(r)) = (out.as_object_mut(), rest.as_object()) {
        m.extend(r.clone());
    }
    out
}

fn axis_field<'a>(cmd: &str, pt: &'a mut PivotTable, field: &str) -> Result<&'a mut PivotField> {
    pt.rows
        .iter_mut()
        .chain(pt.columns.iter_mut())
        .find(|f| f.source_col.eq_ignore_ascii_case(field))
        .ok_or_else(|| bad(cmd, format!("`{field}` isn't in the rows or columns area")))
}

/// What `pivot.sort` orders a field's items by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortOrder {
    Ascending,
    Descending,
    Source,
    /// The `items` given, then the rest ascending.
    Custom,
}

/// `pivot.sort`: orders a row or column field's items by label, by a value field's totals, in source
/// order, or in a custom order.
pub(super) fn sort(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.sort";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let order = match str_param(p, "order").unwrap_or("asc").to_ascii_lowercase().as_str() {
        "asc" | "ascending" | "a-z" => SortOrder::Ascending,
        "desc" | "descending" | "z-a" => SortOrder::Descending,
        // "manual" was source order before custom orders existed; callers may still send it.
        "none" | "source" | "manual" => SortOrder::Source,
        "custom" => SortOrder::Custom,
        o => return Err(bad(C, format!("unknown order `{o}` (asc|desc|none|custom)"))),
    };
    let by = str_param(p, "by").map(str::to_string);
    if by.is_some() && !matches!(order, SortOrder::Ascending | SortOrder::Descending) {
        return Err(bad(C, "`by` sorts ascending or descending"));
    }
    let items: Vec<String> = match (order, p.get("items")) {
        (SortOrder::Custom, Some(Json::Array(list))) if list.len() > MAX_CUSTOM_ITEMS => {
            return Err(bad(C, format!("at most {MAX_CUSTOM_ITEMS} `items`")));
        }
        (SortOrder::Custom, Some(Json::Array(list))) if !list.is_empty() => {
            list.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad(C, "`items` are item labels"))).collect::<Result<_>>()?
        }
        (SortOrder::Custom, _) => return Err(bad(C, "a custom order needs `items`: the item labels in order")),
        (_, Some(_)) => return Err(bad(C, "`items` go with order: custom")),
        (_, None) => vec![],
    };
    update(s, p, |pt, src, wb| {
        let sort_by = by.as_deref().map(|name| value_field_name(C, pt, name)).transpose()?;
        let custom = if order == SortOrder::Custom { item_labels(C, pt, src, wb, &field, &items)? } else { vec![] };
        let f = axis_field(C, pt, &field)?;
        f.sort = match order {
            SortOrder::Ascending => PivotSort::Asc,
            SortOrder::Descending => PivotSort::Desc,
            SortOrder::Source | SortOrder::Custom => PivotSort::None,
        };
        f.sort_by = sort_by;
        f.custom_order = custom;
        Ok(json!({"field": f.source_col, "sortBy": f.sort_by, "customOrder": f.custom_order}))
    })
}

/// The caption of the value field `name` (a caption or the source field of one).
fn value_field_name(cmd: &str, pt: &PivotTable, name: &str) -> Result<String> {
    let name = name.trim();
    pt.values
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(name))
        .or_else(|| pt.values.iter().find(|v| v.source_col.eq_ignore_ascii_case(name)))
        .map(|v| v.name.clone())
        .ok_or_else(|| bad(cmd, format!("`{name}` isn't in the values area")))
}

/// Most labels a custom order lists.
const MAX_CUSTOM_ITEMS: usize = 100_000;

/// `labels` as the axis field `field` shows them (each an item of it), without repeats. One pass
/// over the field's items and one over the labels, so a long order on a large field stays fast.
fn item_labels(cmd: &str, pt: &PivotTable, src: &pv::Source, wb: &gridcraft_model::Workbook, field: &str, labels: &[String]) -> Result<Vec<String>> {
    let axis = pt.rows.iter().chain(pt.columns.iter()).find(|f| f.source_col.eq_ignore_ascii_case(field));
    let axis = axis.ok_or_else(|| bad(cmd, format!("`{field}` isn't in the rows or columns area")))?;
    let column = src.col(&axis.source_col).ok_or_else(|| bad(cmd, format!("no field `{field}` in the source data")))?;
    let mut known: HashMap<String, String> = HashMap::new();
    for label in pv::distinct_labels(wb, src, column, axis.date_group, usize::MAX) {
        known.entry(label.to_ascii_lowercase()).or_insert(label);
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for label in labels {
        // The exact label first (a source label can end in a space), then without padding.
        let exact = label.to_ascii_lowercase();
        let key = if known.contains_key(&exact) { exact } else { label.trim().to_ascii_lowercase() };
        let canonical = known.get(&key).ok_or_else(|| bad(cmd, format!("`{label}` isn't an item of `{field}`")))?;
        if seen.insert(key) {
            out.push(canonical.clone());
        }
    }
    Ok(out)
}

pub(super) fn group(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.group";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let raw = str_param(p, "by").unwrap_or("months");
    let by = group_param(Some(raw)).ok_or_else(|| bad(C, format!("unknown grouping `{raw}` (years|quarters|months|days|none)")))?;
    update(s, p, |pt, src, _| {
        if by != PivotDateGroup::None {
            let c = src.col(&field).ok_or_else(|| bad(C, format!("no field `{field}` in the source data")))?;
            if !src.numeric.get(c).copied().unwrap_or(false) {
                return Err(other("Cannot group that selection."));
            }
        }
        let f = axis_field(C, pt, &field)?;
        f.date_group = by;
        f.collapsed_items.clear();
        Ok(json!({}))
    })
}

pub(super) fn collapse(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.collapse";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let on = bool_param(p, "collapse").unwrap_or(true);
    let items: Option<Vec<String>> =
        p.get("items").and_then(Json::as_array).map(|a| a.iter().take(100_000).filter_map(|x| x.as_str().map(str::to_string)).collect());
    update(s, p, |pt, src, wb| {
        let all = match &items {
            Some(v) => v.clone(),
            None => {
                let f = pt.rows.iter().chain(pt.columns.iter()).find(|f| f.source_col.eq_ignore_ascii_case(&field));
                let g = f.map(|f| f.date_group).unwrap_or_default();
                match src.col(&field) {
                    Some(c) => pv::distinct_labels(wb, src, c, g, 100_000),
                    None => vec![],
                }
            }
        };
        let f = axis_field(C, pt, &field)?;
        if on {
            for it in all {
                if !f.collapsed_items.iter().any(|x| x.eq_ignore_ascii_case(&it)) {
                    f.collapsed_items.push(it);
                }
            }
        } else if items.is_none() {
            f.collapsed_items.clear();
        } else {
            f.collapsed_items.retain(|x| !all.iter().any(|a| a.eq_ignore_ascii_case(x)));
        }
        Ok(json!({"collapsed": f.collapsed_items}))
    })
}

pub(super) fn layout(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.layout";
    let lay = match str_param(p, "layout").map(|x| x.to_ascii_lowercase()) {
        Some(l) if l == "compact" => Some(PivotLayout::Compact),
        Some(l) if l == "outline" => Some(PivotLayout::Outline),
        Some(l) if l == "tabular" => Some(PivotLayout::Tabular),
        Some(l) => return Err(bad(C, format!("unknown layout `{l}` (compact|outline|tabular)"))),
        None => None,
    };
    let subtotals: Option<(bool, Option<bool>)> = match p.get("subtotals") {
        Some(Json::Bool(b)) => Some((*b, None)),
        Some(Json::String(m)) => match m.to_ascii_lowercase().as_str() {
            "top" => Some((true, Some(true))),
            "bottom" => Some((true, Some(false))),
            "none" | "off" => Some((false, None)),
            o => return Err(bad(C, format!("unknown subtotals `{o}` (top|bottom|none)"))),
        },
        _ => None,
    };
    let style = str_param(p, "style").map(str::to_string);
    if style.as_ref().is_some_and(|s| s.len() > 64) {
        return Err(bad(C, "style name too long"));
    }
    let gr = bool_param(p, "grandTotalsRows");
    let gc = bool_param(p, "grandTotalsCols");
    let hd = bool_param(p, "showHeaders");
    update(s, p, |pt, _, _| {
        if let Some(l) = lay {
            pt.layout = l;
            if l == PivotLayout::Tabular {
                pt.subtotals_top = false;
            }
        }
        if let Some((on, top)) = subtotals {
            pt.subtotals = on;
            if let Some(t) = top {
                pt.subtotals_top = t;
            }
        }
        if let Some(st) = &style {
            pt.style = st.clone();
        }
        if let Some(v) = gr {
            pt.grand_totals_rows = v;
        }
        if let Some(v) = gc {
            pt.grand_totals_cols = v;
        }
        if let Some(v) = hd {
            pt.show_headers = v;
        }
        Ok(json!({"layout": layout_name(pt.layout)}))
    })
}

pub(super) fn grand_totals(s: &mut Session, p: &Json) -> Result<Json> {
    let (r, c) = match str_param(p, "mode").map(|m| m.to_ascii_lowercase()) {
        Some(m) if m == "off" => (Some(false), Some(false)),
        Some(m) if m == "on" => (Some(true), Some(true)),
        Some(m) if m == "rows" => (Some(true), Some(false)),
        Some(m) if m == "columns" || m == "cols" => (Some(false), Some(true)),
        Some(m) => return Err(bad("pivot.grandTotals", format!("unknown mode `{m}` (off|on|rows|columns)"))),
        None => (bool_param(p, "rows"), bool_param(p, "cols")),
    };
    let mut q = p.as_object().cloned().unwrap_or_default();
    if let Some(v) = r {
        q.insert("grandTotalsRows".into(), json!(v));
    }
    if let Some(v) = c {
        q.insert("grandTotalsCols".into(), json!(v));
    }
    layout(s, &Json::Object(q))
}

pub(super) fn refresh(s: &mut Session, p: &Json) -> Result<Json> {
    update(s, p, |_, src, _| Ok(json!({"records": src.rows.len()})))
}

pub(super) fn refresh_all(s: &mut Session, p: &Json) -> Result<Json> {
    let replace = bool_param(p, "replace").unwrap_or(false);
    edit(s, |cx| {
        let mut n = 0;
        let mut errors = vec![];
        for si in 0..cx.wb.sheets.len() {
            let count = cx.wb.sheet(si).map(|sh| sh.pivots.len()).unwrap_or(0);
            for pi in 0..count {
                let name = cx.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).map(|p| p.name.clone()).unwrap_or_default();
                match pv::refresh(&mut cx.wb, si, pi, replace) {
                    Ok(_) => n += 1,
                    Err(e) => errors.push(json!({"pivot": name, "error": e})),
                }
            }
        }
        cx.structural = n > 0;
        Ok(json!({"refreshed": n, "errors": errors}))
    })
}

pub(super) fn change_source(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.changeSource";
    if str_param(p, "source").or_else(|| str_param(p, "range")).is_none() {
        return Err(bad(C, "missing `source`"));
    }
    let source = source_param(s, p)?;
    let (si, _) = find_pivot(s, p)?;
    let src = pv::read_source(&s.doc()?.wb, &source, si).map_err(other)?;
    update(s, p, move |pt, _, _| {
        pt.source = source.clone();
        let mut dropped: Vec<String> = vec![];
        let keep = |name: &str, dropped: &mut Vec<String>| {
            let ok = src.col(name).is_some();
            if !ok {
                dropped.push(name.to_string());
            }
            ok
        };
        // Axis fields leave one by one so Σ Values keeps its place among the rest.
        for rows in [true, false] {
            let mut i = 0;
            loop {
                let list = if rows { &mut pt.rows } else { &mut pt.columns };
                let Some(f) = list.get(i) else { break };
                if keep(&f.source_col, &mut dropped) {
                    i += 1;
                } else {
                    list.remove(i);
                    shift_values(pt, rows, i, false);
                }
            }
        }
        pt.values.retain(|f| keep(&f.source_col, &mut dropped));
        pt.filters.retain(|f| keep(&f.source_col, &mut dropped));
        Ok(json!({"source": source, "dropped": dropped}))
    })
}

pub(super) fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, pi) = find_pivot(s, p)?;
    edit(s, |cx| {
        // Its PivotCharts become ordinary charts.
        let link = cx.wb.sheet(si).and_then(|sh| sh.pivots.get(pi).map(|pt| (sh.name.clone(), pt.name.clone())));
        if let Some((sheet, name)) = link {
            for i in 0..cx.wb.sheets.len() {
                let linked = |c: &Chart| c.pivot.as_deref().is_some_and(|l| pv::links_to(l, &sheet, &name));
                if cx.wb.sheet(i).is_some_and(|sh| sh.charts.iter().any(linked)) {
                    for c in cx.sheet_mut(i)?.charts.iter_mut().filter(|c| linked(c)) {
                        c.pivot = None;
                    }
                }
            }
        }
        pv::clear_output(&mut cx.wb, si, pi);
        let sh = cx.sheet_mut(si)?;
        if pi < sh.pivots.len() {
            sh.pivots.remove(pi);
        }
        cx.structural = true;
        Ok(json!({}))
    })
}
