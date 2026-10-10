//! PivotTable commands (Insert › PivotTable, PivotTable Analyze, PivotTable Design, Data ›
//! Refresh All). Every change re-renders the report as one undo step.
//!
//! `pivot?` parameters name a PivotTable (name or numeric id); by default it is the pivot whose
//! report contains the active cell, else the last pivot on the active sheet.

use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::*;
use crate::pivot as pv;
use crate::selection::current_region;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.pivotTable",
            "PivotTable",
            ["Insert", "Tables"],
            None,
            "{source?: \"Sheet1!$A$1:$E$200\" | table name (default: the table or current region of the selection), destination?: \"new\" (default: new sheet before the current one, report at A3) | \"Sheet1!H3\", name?, rows?: [field | {field, group?: years|quarters|months|days}], columns?: [..], values?: [field | {field, func?, name?}], filters?: [field], replace?: bool} → {id, name, sheet, anchor, fields}",
            has_doc,
            insert_pivot
        ),
        cmd!(
            "insert.recommendedPivotTables",
            "Recommended PivotTables",
            ["Insert", "Tables"],
            None,
            "{source?, apply?: index, destination?} → suggestions [{index, title, rows, columns, values}]; with apply, creates that one",
            has_doc,
            recommended
        ),
        cmd!(
            "table.summarizePivot",
            "Summarize with PivotTable",
            ["Table Design", "Tools"],
            None,
            "{table?, destination?} creates a PivotTable from the table",
            has_doc,
            summarize_table
        ),
        cmd!(query "pivot.fields", "PivotTable Fields", [], None, "{pivot?} → source fields (name, numeric, date, samples, areas) and the current layout", has_doc, fields),
        cmd!(query "pivot.fieldList", "Field List", ["PivotTable Analyze", "Show"], None, "{pivot?} (same as pivot.fields)", has_doc, fields),
        cmd!(query "pivot.list", "PivotTables", [], None, "{} → every PivotTable in the workbook", has_doc, list),
        cmd!(
            "pivot.addField",
            "Add Field",
            [],
            None,
            "{pivot?, field, area: rows|columns|values|filters, func?: sum|count|average|max|min|product|countNumbers|stdDev|stdDevP|var|varP, name?, position?: index, replace?}",
            has_doc,
            add_field
        ),
        cmd!(
            "pivot.removeField",
            "Remove Field",
            [],
            None,
            "{pivot?, field (or value caption), area?: rows|columns|values|filters, replace?}",
            has_doc,
            remove_field
        ),
        cmd!(
            "pivot.moveField",
            "Move Field",
            [],
            None,
            "{pivot?, field, from: rows|columns|values|filters, to: rows|columns|values|filters, position?: index, replace?}",
            has_doc,
            move_field
        ),
        cmd!(
            "pivot.valueSettings",
            "Value Field Settings",
            ["PivotTable Analyze", "Active Field"],
            None,
            "{pivot?, field: caption or source field | index: n, func?, name?, showAs?: normal|percentOfGrandTotal|percentOfColumnTotal|percentOfRowTotal|runningTotal|rank, numberFormat?: \"#,##0\" | null, replace?}",
            has_doc,
            value_settings
        ),
        cmd!("pivot.filter", "Filter Items", [], None, "{pivot?, field, selected: [\"East\", ..] | null (all), replace?}", has_doc, filter),
        cmd!("pivot.sort", "Sort Field", [], None, "{pivot?, field, order: asc|desc|none, replace?}", has_doc, sort),
        cmd!(
            "pivot.group",
            "Group Field",
            ["PivotTable Analyze", "Group"],
            None,
            "{pivot?, field, by: years|quarters|months|days|none, replace?}",
            has_doc,
            group
        ),
        cmd!(
            "pivot.collapse",
            "Expand/Collapse Field",
            ["PivotTable Analyze", "Active Field"],
            None,
            "{pivot?, field, items?: [labels] (default: all), collapse?: true, replace?}",
            has_doc,
            collapse
        ),
        cmd!(
            "pivot.layout",
            "PivotTable Layout",
            ["PivotTable Design", "Layout"],
            None,
            "{pivot?, layout?: compact|outline|tabular, grandTotalsRows?: bool, grandTotalsCols?: bool, subtotals?: bool|top|bottom|none, style?: \"PivotStyleLight16\", showHeaders?: bool, replace?}",
            has_doc,
            layout
        ),
        cmd!(
            "pivot.grandTotals",
            "Grand Totals",
            ["PivotTable Design", "Layout"],
            None,
            "{pivot?, mode?: off|on|rows|columns, rows?: bool, cols?: bool}",
            has_doc,
            grand_totals
        ),
        cmd!("pivot.subtotals", "Subtotals", ["PivotTable Design", "Layout"], None, "{pivot?, mode: none|top|bottom}", has_doc, |s, p| {
            let mode = str_param(p, "mode").unwrap_or("top");
            layout(s, &with(p, "subtotals", json!(mode)))
        }),
        cmd!(
            "pivot.reportLayout",
            "Report Layout",
            ["PivotTable Design", "Layout"],
            None,
            "{pivot?, layout: compact|outline|tabular}",
            has_doc,
            layout
        ),
        cmd!(
            "pivot.style",
            "PivotTable Styles",
            ["PivotTable Design", "PivotTable Styles"],
            None,
            "{pivot?, style: \"PivotStyleMedium9\"}",
            has_doc,
            layout
        ),
        cmd!("pivot.refresh", "Refresh", ["PivotTable Analyze", "Data"], Some("Alt+F5"), "{pivot?, replace?}", has_doc, refresh),
        cmd!(
            "data.refreshAll",
            "Refresh All",
            ["Data", "Queries & Connections"],
            Some("Cmd+Alt+F5"),
            "{replace?} refreshes every PivotTable → {refreshed, errors}",
            has_doc,
            refresh_all
        ),
        cmd!(
            "pivot.changeSource",
            "Change Data Source",
            ["PivotTable Analyze", "Data"],
            None,
            "{pivot?, source: \"Sheet1!$A$1:$E$300\" | table name, replace?} (fields missing from the new source are removed)",
            has_doc,
            change_source
        ),
        cmd!(
            "pivot.delete",
            "Delete PivotTable",
            ["PivotTable Analyze", "Actions"],
            None,
            "{pivot?} clears the report and removes the PivotTable",
            has_doc,
            delete
        ),
    ]
}

fn with(p: &Json, k: &str, v: Json) -> Json {
    let mut m = p.as_object().cloned().unwrap_or_default();
    m.insert(k.into(), v);
    Json::Object(m)
}

fn other(msg: impl Into<String>) -> EngineError {
    EngineError::Other(msg.into())
}

// ---------------------------------------------------------------- lookup

/// (sheet, index) of the pivot a call targets.
fn find_pivot(s: &Session, p: &Json) -> Result<(usize, usize)> {
    let d = s.doc()?;
    let active = d.wb.active_sheet;
    if let Some(v) = p.get("pivot").filter(|v| !v.is_null()) {
        let mut order: Vec<usize> = vec![active];
        order.extend((0..d.wb.sheets.len()).filter(|i| *i != active));
        for si in order {
            let Some(sh) = d.wb.sheet(si) else { continue };
            let hit = sh.pivots.iter().position(|pt| match v {
                Json::String(n) => pt.name.eq_ignore_ascii_case(n),
                Json::Number(n) => n.as_u64() == Some(pt.id as u64),
                _ => false,
            });
            if let Some(i) = hit {
                return Ok((si, i));
            }
        }
        return Err(bad("pivot", format!("no PivotTable `{}`", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))));
    }
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let at = d.selection.active;
    if let Some(i) = sh.pivots.iter().position(|pt| pt.last_range.is_some_and(|r| r.contains(at))) {
        return Ok((active, i));
    }
    if !sh.pivots.is_empty() {
        return Ok((active, sh.pivots.len() - 1));
    }
    Err(EngineError::Disabled("pivot".into(), "select a cell in a PivotTable (or pass `pivot`)".into()))
}

fn area_param(cmd: &str, v: Option<&str>) -> Result<&'static str> {
    match v.map(|x| x.to_ascii_lowercase()) {
        Some(a) if a == "rows" || a == "row" => Ok("rows"),
        Some(a) if a == "columns" || a == "column" || a == "cols" => Ok("columns"),
        Some(a) if a == "values" || a == "value" || a == "data" => Ok("values"),
        Some(a) if a == "filters" || a == "filter" => Ok("filters"),
        Some(a) => Err(bad(cmd, format!("unknown area `{a}` (rows|columns|values|filters)"))),
        None => Err(bad(cmd, "missing `area` (rows|columns|values|filters)")),
    }
}

fn func_param(v: Option<&str>) -> Option<PivotFunc> {
    Some(match v?.to_ascii_lowercase().as_str() {
        "sum" => PivotFunc::Sum,
        "count" | "counta" => PivotFunc::Count,
        "average" | "avg" | "mean" => PivotFunc::Average,
        "max" => PivotFunc::Max,
        "min" => PivotFunc::Min,
        "product" => PivotFunc::Product,
        "countnumbers" | "countnums" | "count numbers" => PivotFunc::CountNumbers,
        "stddev" => PivotFunc::StdDev,
        "stddevp" => PivotFunc::StdDevP,
        "var" => PivotFunc::Var,
        "varp" => PivotFunc::VarP,
        _ => return None,
    })
}

fn func_name(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "sum",
        PivotFunc::Count => "count",
        PivotFunc::Average => "average",
        PivotFunc::Max => "max",
        PivotFunc::Min => "min",
        PivotFunc::Product => "product",
        PivotFunc::CountNumbers => "countNumbers",
        PivotFunc::StdDev => "stdDev",
        PivotFunc::StdDevP => "stdDevP",
        PivotFunc::Var => "var",
        PivotFunc::VarP => "varP",
    }
}

fn show_as_param(v: &str) -> Option<PivotShowAs> {
    Some(match v.to_ascii_lowercase().as_str() {
        "normal" | "none" => PivotShowAs::Normal,
        "percentofgrandtotal" => PivotShowAs::PercentOfGrandTotal,
        "percentofcolumntotal" => PivotShowAs::PercentOfColumnTotal,
        "percentofrowtotal" => PivotShowAs::PercentOfRowTotal,
        "runningtotal" => PivotShowAs::RunningTotal,
        "rank" => PivotShowAs::Rank,
        _ => return None,
    })
}

fn show_as_name(v: PivotShowAs) -> &'static str {
    match v {
        PivotShowAs::Normal => "normal",
        PivotShowAs::PercentOfGrandTotal => "percentOfGrandTotal",
        PivotShowAs::PercentOfColumnTotal => "percentOfColumnTotal",
        PivotShowAs::PercentOfRowTotal => "percentOfRowTotal",
        PivotShowAs::RunningTotal => "runningTotal",
        PivotShowAs::Rank => "rank",
    }
}

fn layout_name(l: PivotLayout) -> &'static str {
    match l {
        PivotLayout::Compact => "compact",
        PivotLayout::Outline => "outline",
        PivotLayout::Tabular => "tabular",
    }
}

fn group_name(g: PivotDateGroup) -> &'static str {
    match g {
        PivotDateGroup::None => "none",
        PivotDateGroup::Years => "years",
        PivotDateGroup::Quarters => "quarters",
        PivotDateGroup::Months => "months",
        PivotDateGroup::Days => "days",
    }
}

fn sort_name(o: PivotSort) -> &'static str {
    match o {
        PivotSort::Asc => "asc",
        PivotSort::Desc => "desc",
        PivotSort::None => "none",
    }
}

// ---------------------------------------------------------------- layout editing (pure)

fn in_axes(pt: &PivotTable, field: &str) -> bool {
    pt.rows.iter().chain(pt.columns.iter()).any(|f| f.source_col.eq_ignore_ascii_case(field))
}

/// The canonical source field name for `field`.
fn canonical(cmd: &str, src: &pv::Source, field: &str) -> Result<String> {
    src.col(field).and_then(|c| src.headers.get(c).cloned()).ok_or_else(|| bad(cmd, format!("no field `{field}` in the source data")))
}

fn unique_value_name(pt: &PivotTable, base: &str, except: Option<usize>) -> String {
    let taken = |n: &str| pt.values.iter().enumerate().any(|(i, v)| Some(i) != except && v.name.eq_ignore_ascii_case(n));
    if !taken(base) {
        return base.to_string();
    }
    (2..10_000).map(|k| format!("{base}{k}")).find(|n| !taken(n)).unwrap_or_else(|| base.to_string())
}

fn auto_name(f: PivotFunc, field: &str) -> String {
    format!("{} of {field}", pv::func_caption(f))
}

/// Adds `field` to an area (moving it out of the other axis areas).
fn add_to_area(
    cmd: &str,
    pt: &mut PivotTable,
    src: &pv::Source,
    field: &str,
    area: &str,
    func: Option<PivotFunc>,
    name: Option<&str>,
    position: Option<usize>,
) -> Result<()> {
    let field = canonical(cmd, src, field)?;
    match area {
        "rows" | "columns" => {
            if pt.rows.len() + pt.columns.len() >= pv::MAX_AXIS_FIELDS && !in_axes(pt, &field) {
                return Err(other(format!("A PivotTable can have at most {} row and column fields.", pv::MAX_AXIS_FIELDS)));
            }
            let existing = pt
                .rows
                .iter()
                .chain(pt.columns.iter())
                .find(|f| f.source_col.eq_ignore_ascii_case(&field))
                .cloned()
                .unwrap_or_else(|| PivotField { source_col: field.clone(), ..Default::default() });
            pt.rows.retain(|f| !f.source_col.eq_ignore_ascii_case(&field));
            pt.columns.retain(|f| !f.source_col.eq_ignore_ascii_case(&field));
            // A report filter that selects nothing in particular goes away; a selection stays
            // as the field's hidden items.
            pt.filters.retain(|f| !(f.source_col.eq_ignore_ascii_case(&field) && f.selected.is_none()));
            let list = if area == "rows" { &mut pt.rows } else { &mut pt.columns };
            let at = position.unwrap_or(list.len()).min(list.len());
            list.insert(at, existing);
        }
        "values" => {
            if pt.values.len() >= pv::MAX_VALUE_FIELDS {
                return Err(other(format!("A PivotTable can have at most {} value fields.", pv::MAX_VALUE_FIELDS)));
            }
            let numeric = src.col(&field).and_then(|c| src.numeric.get(c).copied()).unwrap_or(false);
            let f = func.unwrap_or(if numeric { PivotFunc::Sum } else { PivotFunc::Count });
            let base = match name {
                Some(n) if !n.trim().is_empty() => {
                    if src.col(n).is_some() {
                        return Err(other("PivotTable field name already exists."));
                    }
                    n.to_string()
                }
                _ => auto_name(f, &field),
            };
            let nm = unique_value_name(pt, &base, None);
            let v = PivotValue { source_col: field, func: f, name: nm, show_as: PivotShowAs::Normal, number_format: None };
            let at = position.unwrap_or(pt.values.len()).min(pt.values.len());
            pt.values.insert(at, v);
        }
        _ => {
            pt.rows.retain(|f| !f.source_col.eq_ignore_ascii_case(&field));
            pt.columns.retain(|f| !f.source_col.eq_ignore_ascii_case(&field));
            let existing = pt.filters.iter().position(|f| f.source_col.eq_ignore_ascii_case(&field));
            let entry = match existing {
                Some(i) => pt.filters.remove(i),
                None => PivotFilter { source_col: field, selected: None },
            };
            let at = position.unwrap_or(pt.filters.len()).min(pt.filters.len());
            pt.filters.insert(at, entry);
        }
    }
    Ok(())
}

/// Removes `field` from an area; returns the source field it referred to.
fn remove_from_area(cmd: &str, pt: &mut PivotTable, field: &str, area: &str) -> Result<String> {
    match area {
        "rows" | "columns" => {
            let list = if area == "rows" { &mut pt.rows } else { &mut pt.columns };
            let i = list
                .iter()
                .position(|f| f.source_col.eq_ignore_ascii_case(field))
                .ok_or_else(|| bad(cmd, format!("`{field}` isn't in the {area} area")))?;
            let f = list.remove(i);
            pt.filters.retain(|x| !x.source_col.eq_ignore_ascii_case(&f.source_col));
            Ok(f.source_col)
        }
        "values" => {
            let i = pt
                .values
                .iter()
                .position(|v| v.name.eq_ignore_ascii_case(field))
                .or_else(|| pt.values.iter().position(|v| v.source_col.eq_ignore_ascii_case(field)))
                .ok_or_else(|| bad(cmd, format!("`{field}` isn't in the values area")))?;
            Ok(pt.values.remove(i).source_col)
        }
        _ => {
            let i = pt
                .filters
                .iter()
                .position(|f| f.source_col.eq_ignore_ascii_case(field) && !in_axes(pt, &f.source_col))
                .ok_or_else(|| bad(cmd, format!("`{field}` isn't in the filters area")))?;
            Ok(pt.filters.remove(i).source_col)
        }
    }
}

// ---------------------------------------------------------------- shared update

/// Applies `f` to the targeted pivot and re-renders it, as one undo step.
fn update(s: &mut Session, p: &Json, f: impl FnOnce(&mut PivotTable, &pv::Source, &Workbook) -> Result<Json>) -> Result<Json> {
    let (si, pi) = find_pivot(s, p)?;
    let replace = bool_param(p, "replace").unwrap_or(false);
    edit(s, |cx| {
        let pt = cx.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).cloned().ok_or_else(|| other("There's no such PivotTable."))?;
        let src = pv::read_source(&cx.wb, &pt.source, si).map_err(other)?;
        let mut new = pt.clone();
        let mut out = f(&mut new, &src, &cx.wb)?;
        if let Some(slot) = cx.sheet_mut(si)?.pivots.get_mut(pi) {
            *slot = new;
        }
        let area = pv::refresh(&mut cx.wb, si, pi, replace).map_err(other)?;
        cx.structural = true;
        if out.is_null() {
            out = json!({});
        }
        if let Some(m) = out.as_object_mut() {
            m.insert("range".into(), json!(area.a1()));
        }
        Ok(out)
    })
}

// ---------------------------------------------------------------- insert

/// The default source: the table at the selection, else the selected block / current region.
fn default_source(s: &Session) -> Result<String> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    if let Some(t) = sh.table_at(d.selection.active) {
        return Ok(t.name.clone());
    }
    let sel = d.selection.current();
    let r = if sel.is_single() {
        current_region(sh, d.selection.active)
    } else {
        let used = sh.used_range().unwrap_or(sel);
        sel.intersection(&used).unwrap_or(sel)
    };
    if r.is_single() && sh.value(r.start).is_empty() {
        return Err(other("The reference isn't valid. Select a cell in a range with labeled columns, or pass `source`."));
    }
    Ok(pv::range_text(&sh.name, r))
}

fn source_param(s: &Session, p: &Json) -> Result<String> {
    match str_param(p, "source").or_else(|| str_param(p, "range")) {
        Some(t) if !t.trim().is_empty() => {
            // Bare ranges are on the active sheet; store them with the sheet name.
            let d = s.doc()?;
            let t = t.trim().trim_start_matches('=');
            if !t.contains('!')
                && RangeRef::parse(t).is_some()
                && let Some(sh) = d.wb.active()
                && let Some(r) = RangeRef::parse(t)
            {
                return Ok(pv::range_text(&sh.name, r));
            }
            Ok(t.to_string())
        }
        _ => default_source(s),
    }
}

fn next_pivot_name(wb: &Workbook) -> String {
    (1..)
        .map(|n| format!("PivotTable{n}"))
        .find(|n| !wb.sheets.iter().any(|s| s.pivots.iter().any(|p| p.name.eq_ignore_ascii_case(n))))
        .unwrap_or_default()
}

fn next_pivot_id(wb: &Workbook) -> u32 {
    let m = wb.sheets.iter().flat_map(|s| s.pivots.iter().map(|p| p.id)).max().unwrap_or(0);
    wb.next_object_id().max(m.saturating_add(1))
}

fn field_list(v: Option<&Json>) -> Vec<Json> {
    match v {
        Some(Json::Array(a)) => a.iter().take(512).cloned().collect(),
        Some(Json::String(s)) => vec![Json::String(s.clone())],
        _ => vec![],
    }
}

fn insert_pivot(s: &mut Session, p: &Json) -> Result<Json> {
    if s.doc()?.wb.protected_structure && matches!(str_param(p, "destination"), None | Some("new")) {
        return Err(other("The workbook is protected and cannot be changed."));
    }
    let source = source_param(s, p)?;
    let active = s.doc()?.wb.active_sheet;
    // Validate the source now (header names, size).
    let src = pv::read_source(&s.doc()?.wb, &source, active).map_err(other)?;
    let name = match str_param(p, "name") {
        Some(n) if !n.trim().is_empty() => {
            let n = n.trim().to_string();
            if s.doc()?.wb.sheets.iter().any(|sh| sh.pivots.iter().any(|x| x.name.eq_ignore_ascii_case(&n))) {
                return Err(other("A PivotTable with that name already exists."));
            }
            n
        }
        _ => next_pivot_name(&s.doc()?.wb),
    };
    let dest = str_param(p, "destination").or_else(|| str_param(p, "location")).unwrap_or("new").trim().to_string();
    // Existing sheet destination: (sheet index, anchor).
    let target: Option<(usize, CellRef)> = if dest.eq_ignore_ascii_case("new") || dest.is_empty() {
        None
    } else {
        let d = s.doc()?;
        let (sh, body) = super::split_sheet(&dest);
        let si = match sh {
            Some(n) => d.wb.sheet_index(&n).ok_or_else(|| bad("insert.pivotTable", format!("no sheet named `{n}`")))?,
            None => active,
        };
        let at = RangeRef::parse(body).map(|r| r.start).ok_or_else(|| bad("insert.pivotTable", format!("not a cell: `{dest}`")))?;
        Some((si, at))
    };
    let replace = bool_param(p, "replace").unwrap_or(false);
    let rows = field_list(p.get("rows"));
    let columns = field_list(p.get("columns"));
    let values = field_list(p.get("values"));
    let filters = field_list(p.get("filters"));
    let mut new_sheet_at: Option<usize> = None;
    let res = edit(s, |cx| {
        let id = next_pivot_id(&cx.wb);
        let (si, anchor) = match target {
            Some(t) => t,
            None => {
                let at = cx.wb.active_sheet.min(cx.wb.sheets.len());
                let nm = cx.wb.next_sheet_name();
                cx.wb.sheets.insert(at, Arc::new(Sheet::new(nm)));
                for n in cx.wb.names.iter_mut() {
                    if let Some(sc) = n.scope.as_mut()
                        && *sc >= at
                    {
                        *sc += 1;
                    }
                }
                new_sheet_at = Some(at);
                (at, CellRef::new(2, 0))
            }
        };
        let mut pt = PivotTable { id, name: name.clone(), source: source.clone(), anchor, ..Default::default() };
        let add = |pt: &mut PivotTable, list: &[Json], area: &str| -> Result<()> {
            for f in list {
                match f {
                    Json::String(n) => add_to_area("insert.pivotTable", pt, &src, n, area, None, None, None)?,
                    Json::Object(_) => {
                        let n = str_param(f, "field").ok_or_else(|| bad("insert.pivotTable", "a field object needs `field`"))?;
                        add_to_area("insert.pivotTable", pt, &src, n, area, func_param(str_param(f, "func")), str_param(f, "name"), None)?;
                        let by = group_param(str_param(f, "group"));
                        if let Some(by) = by.filter(|b| *b != PivotDateGroup::None)
                            && matches!(area, "rows" | "columns")
                            && src.col(n).and_then(|c| src.numeric.get(c).copied()).unwrap_or(false)
                            && let Some(af) = pt.rows.iter_mut().chain(pt.columns.iter_mut()).find(|x| x.source_col.eq_ignore_ascii_case(n))
                        {
                            af.date_group = by;
                        }
                    }
                    _ => return Err(bad("insert.pivotTable", "fields are names or {field, func?, name?}")),
                }
            }
            Ok(())
        };
        add(&mut pt, &rows, "rows")?;
        add(&mut pt, &columns, "columns")?;
        add(&mut pt, &values, "values")?;
        add(&mut pt, &filters, "filters")?;
        let sh = cx.sheet_mut(si)?;
        sh.pivots.push(pt);
        let pi = sh.pivots.len() - 1;
        let area = pv::refresh(&mut cx.wb, si, pi, replace).map_err(other)?;
        cx.wb.active_sheet = si;
        *cx.sel = crate::Selection::at(anchor);
        cx.structural = true;
        let sheet_name = cx.wb.sheet(si).map(|s| s.name.clone()).unwrap_or_default();
        Ok(json!({
            "id": id,
            "name": name,
            "sheet": sheet_name,
            "anchor": anchor.a1(),
            "range": area.a1(),
            "source": source,
            "fields": src.headers,
        }))
    })?;
    if let Some(at) = new_sheet_at
        && let Ok(d) = s.doc_mut()
    {
        d.sheet_selections.insert(at.min(d.sheet_selections.len()), crate::Selection::default());
    }
    Ok(res)
}

fn summarize_table(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let name = match str_param(p, "table") {
        Some(n) => d.wb.table(n).and_then(|(si, ti)| d.wb.sheet(si).and_then(|sh| sh.tables.get(ti))).map(|t| t.name.clone()),
        None => d.wb.active().and_then(|sh| sh.table_at(d.selection.active)).map(|t| t.name.clone()),
    }
    .ok_or_else(|| EngineError::Disabled("table.summarizePivot".into(), "select a cell in a table".into()))?;
    let mut q = with(p, "source", json!(name));
    if let Some(m) = q.as_object_mut() {
        m.remove("table");
    }
    insert_pivot(s, &q)
}

// ---------------------------------------------------------------- queries

fn pivot_json(wb: &Workbook, si: usize, pt: &PivotTable) -> Json {
    json!({
        "id": pt.id,
        "name": pt.name,
        "sheet": wb.sheet(si).map(|s| s.name.clone()).unwrap_or_default(),
        "anchor": pt.anchor.a1(),
        "range": pt.last_range.map(|r| r.a1()),
        "source": pt.source,
        "rows": pt.rows.iter().map(|f| json!({"field": f.source_col, "sort": sort_name(f.sort), "group": group_name(f.date_group), "collapsed": f.collapsed_items})).collect::<Vec<_>>(),
        "columns": pt.columns.iter().map(|f| json!({"field": f.source_col, "sort": sort_name(f.sort), "group": group_name(f.date_group), "collapsed": f.collapsed_items})).collect::<Vec<_>>(),
        "values": pt.values.iter().map(|v| json!({"field": v.source_col, "func": func_name(v.func), "name": v.name, "showAs": show_as_name(v.show_as), "numberFormat": v.number_format})).collect::<Vec<_>>(),
        "filters": pt.filters.iter().map(|f| json!({"field": f.source_col, "selected": f.selected, "report": !in_axes(pt, &f.source_col)})).collect::<Vec<_>>(),
        "layout": layout_name(pt.layout),
        "grandTotalsRows": pt.grand_totals_rows,
        "grandTotalsCols": pt.grand_totals_cols,
        "subtotals": if !pt.subtotals { "none" } else if pt.subtotals_top { "top" } else { "bottom" },
        "style": pt.style,
        "showHeaders": pt.show_headers,
    })
}

fn fields(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, pi) = find_pivot(s, p)?;
    let d = s.doc()?;
    let pt = d.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).ok_or_else(|| other("There's no such PivotTable."))?;
    let mut out = pivot_json(&d.wb, si, pt);
    let src = pv::read_source(&d.wb, &pt.source, si);
    let list: Vec<Json> = match &src {
        Ok(src) => src
            .headers
            .iter()
            .enumerate()
            .map(|(c, h)| {
                let mut areas = vec![];
                if pt.rows.iter().any(|f| f.source_col.eq_ignore_ascii_case(h)) {
                    areas.push("rows");
                }
                if pt.columns.iter().any(|f| f.source_col.eq_ignore_ascii_case(h)) {
                    areas.push("columns");
                }
                if pt.values.iter().any(|f| f.source_col.eq_ignore_ascii_case(h)) {
                    areas.push("values");
                }
                if pt.filters.iter().any(|f| f.source_col.eq_ignore_ascii_case(h)) && !in_axes(pt, h) {
                    areas.push("filters");
                }
                json!({
                    "name": h,
                    "numeric": src.numeric.get(c).copied().unwrap_or(false),
                    "date": src.dates.get(c).copied().unwrap_or(false),
                    "samples": pv::distinct_labels(&d.wb, src, c, PivotDateGroup::None, 5),
                    "areas": areas,
                })
            })
            .collect(),
        Err(_) => vec![],
    };
    if let Some(m) = out.as_object_mut() {
        m.insert("fields".into(), Json::Array(list));
        if let Ok(src) = &src {
            m.insert("records".into(), json!(src.rows.len()));
        }
        if let Err(e) = src {
            m.insert("error".into(), json!(e));
        }
    }
    Ok(out)
}

fn list(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let mut v = vec![];
    for (si, sh) in d.wb.sheets.iter().enumerate() {
        for pt in &sh.pivots {
            v.push(pivot_json(&d.wb, si, pt));
        }
    }
    Ok(Json::Array(v))
}

// ---------------------------------------------------------------- field commands

fn position_param(p: &Json) -> Option<usize> {
    u32_param(p, "position").map(|v| v as usize)
}

fn add_field(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.addField";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let area = area_param(C, str_param(p, "area"))?;
    let func = match str_param(p, "func") {
        Some(f) => Some(func_param(Some(f)).ok_or_else(|| bad(C, format!("unknown function `{f}`")))?),
        None => None,
    };
    let name = str_param(p, "name").map(str::to_string);
    let pos = position_param(p);
    update(s, p, |pt, src, _| {
        add_to_area(C, pt, src, &field, area, func, name.as_deref(), pos)?;
        Ok(json!({"field": canonical(C, src, &field)?, "area": area}))
    })
}

fn remove_field(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.removeField";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let area = match str_param(p, "area") {
        Some(a) => Some(area_param(C, Some(a))?),
        None => None,
    };
    update(s, p, |pt, _, _| {
        match area {
            Some(a) => {
                remove_from_area(C, pt, &field, a)?;
            }
            None => {
                let mut any = false;
                for a in ["rows", "columns", "values", "filters"] {
                    any |= remove_from_area(C, pt, &field, a).is_ok();
                }
                if !any {
                    return Err(bad(C, format!("`{field}` isn't in the PivotTable")));
                }
            }
        }
        Ok(json!({}))
    })
}

fn move_field(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.moveField";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let from = area_param(C, str_param(p, "from"))?;
    let to = area_param(C, str_param(p, "to"))?;
    let pos = position_param(p);
    update(s, p, |pt, src, _| {
        if from == to {
            // Reorder within the area.
            match from {
                "rows" | "columns" => {
                    let list = if from == "rows" { &mut pt.rows } else { &mut pt.columns };
                    let i = list
                        .iter()
                        .position(|f| f.source_col.eq_ignore_ascii_case(&field))
                        .ok_or_else(|| bad(C, format!("`{field}` isn't in the {from} area")))?;
                    let f = list.remove(i);
                    let at = pos.unwrap_or(list.len()).min(list.len());
                    list.insert(at, f);
                }
                "values" => {
                    let i = pt
                        .values
                        .iter()
                        .position(|v| v.name.eq_ignore_ascii_case(&field))
                        .or_else(|| pt.values.iter().position(|v| v.source_col.eq_ignore_ascii_case(&field)))
                        .ok_or_else(|| bad(C, format!("`{field}` isn't in the values area")))?;
                    let v = pt.values.remove(i);
                    let at = pos.unwrap_or(pt.values.len()).min(pt.values.len());
                    pt.values.insert(at, v);
                }
                _ => {
                    let i = pt
                        .filters
                        .iter()
                        .position(|f| f.source_col.eq_ignore_ascii_case(&field))
                        .ok_or_else(|| bad(C, format!("`{field}` isn't in the filters area")))?;
                    let f = pt.filters.remove(i);
                    let at = pos.unwrap_or(pt.filters.len()).min(pt.filters.len());
                    pt.filters.insert(at, f);
                }
            }
            return Ok(json!({}));
        }
        // Keep axis settings (sort, grouping) when moving between rows and columns.
        let kept = pt.rows.iter().chain(pt.columns.iter()).find(|f| f.source_col.eq_ignore_ascii_case(&field)).cloned();
        let hidden = pt.filters.iter().find(|f| f.source_col.eq_ignore_ascii_case(&field)).cloned();
        let src_field = remove_from_area(C, pt, &field, from)?;
        add_to_area(C, pt, src, &src_field, to, None, None, pos)?;
        if matches!(to, "rows" | "columns") {
            let list = if to == "rows" { &mut pt.rows } else { &mut pt.columns };
            if let (Some(k), Some(slot)) = (kept, list.iter_mut().find(|f| f.source_col.eq_ignore_ascii_case(&src_field))) {
                *slot = k;
            }
            if let Some(h) = hidden.filter(|h| h.selected.is_some())
                && !pt.filters.iter().any(|f| f.source_col.eq_ignore_ascii_case(&src_field))
            {
                pt.filters.push(h);
            }
        }
        Ok(json!({"field": src_field, "to": to}))
    })
}

fn value_settings(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.valueSettings";
    let field = str_param(p, "field").map(str::to_string);
    let index = u32_param(p, "index").map(|v| v as usize);
    let func = match str_param(p, "func") {
        Some(f) => Some(func_param(Some(f)).ok_or_else(|| bad(C, format!("unknown function `{f}`")))?),
        None => None,
    };
    let show = match str_param(p, "showAs") {
        Some(v) => Some(show_as_param(v).ok_or_else(|| bad(C, format!("unknown showAs `{v}`")))?),
        None => None,
    };
    let name = str_param(p, "name").map(str::to_string);
    let fmt = p.get("numberFormat").cloned();
    update(s, p, |pt, src, _| {
        let i = match (index, &field) {
            (Some(i), _) => i,
            (None, Some(f)) => pt
                .values
                .iter()
                .position(|v| v.name.eq_ignore_ascii_case(f))
                .or_else(|| pt.values.iter().position(|v| v.source_col.eq_ignore_ascii_case(f)))
                .ok_or_else(|| bad(C, format!("`{f}` isn't in the values area")))?,
            (None, None) => {
                if pt.values.len() == 1 {
                    0
                } else {
                    return Err(bad(C, "pass `field` or `index`"));
                }
            }
        };
        let cur = pt.values.get(i).cloned().ok_or_else(|| bad(C, "no value field at that index"))?;
        let mut v = cur.clone();
        if let Some(f) = func {
            v.func = f;
            // An automatic caption follows the function.
            let auto = auto_name(cur.func, &cur.source_col);
            if cur.name.eq_ignore_ascii_case(&auto) || cur.name.strip_prefix(auto.as_str()).is_some_and(|r| r.chars().all(|c| c.is_ascii_digit())) {
                v.name = unique_value_name(pt, &auto_name(f, &cur.source_col), Some(i));
            }
        }
        if let Some(n) = &name {
            let n = n.trim();
            if n.is_empty() {
                return Err(other("The name isn't valid."));
            }
            if src.col(n).is_some() {
                return Err(other("PivotTable field name already exists."));
            }
            if pt.values.iter().enumerate().any(|(j, x)| j != i && x.name.eq_ignore_ascii_case(n)) {
                return Err(other("PivotTable field name already exists."));
            }
            v.name = n.to_string();
        }
        if let Some(sa) = show {
            v.show_as = sa;
        }
        match &fmt {
            Some(Json::String(f)) if !f.is_empty() && f.len() <= 255 => v.number_format = Some(f.clone()),
            Some(Json::Null) => v.number_format = None,
            Some(Json::String(f)) if f.is_empty() => v.number_format = None,
            Some(_) => return Err(bad(C, "`numberFormat` is a format code or null")),
            None => {}
        }
        let name = v.name.clone();
        if let Some(slot) = pt.values.get_mut(i) {
            *slot = v;
        }
        Ok(json!({"name": name}))
    })
}

fn filter(s: &mut Session, p: &Json) -> Result<Json> {
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
                if in_axes(pt, &field) {
                    pt.filters.remove(i);
                } else if let Some(f) = pt.filters.get_mut(i) {
                    f.selected = None;
                }
            }
            (Some(i), Some(sel)) => {
                if let Some(f) = pt.filters.get_mut(i) {
                    f.selected = Some(sel);
                }
            }
            (None, Some(sel)) => pt.filters.push(PivotFilter { source_col: field.clone(), selected: Some(sel) }),
            (None, None) => {}
        }
        Ok(json!({"field": field}))
    })
}

fn axis_field<'a>(cmd: &str, pt: &'a mut PivotTable, field: &str) -> Result<&'a mut PivotField> {
    pt.rows
        .iter_mut()
        .chain(pt.columns.iter_mut())
        .find(|f| f.source_col.eq_ignore_ascii_case(field))
        .ok_or_else(|| bad(cmd, format!("`{field}` isn't in the rows or columns area")))
}

fn sort(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.sort";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.to_string();
    let order = match str_param(p, "order").unwrap_or("asc").to_ascii_lowercase().as_str() {
        "asc" | "ascending" | "a-z" => PivotSort::Asc,
        "desc" | "descending" | "z-a" => PivotSort::Desc,
        "none" | "manual" | "source" => PivotSort::None,
        o => return Err(bad(C, format!("unknown order `{o}` (asc|desc|none)"))),
    };
    update(s, p, |pt, _, _| {
        axis_field(C, pt, &field)?.sort = order;
        Ok(json!({}))
    })
}

fn group_param(v: Option<&str>) -> Option<PivotDateGroup> {
    Some(match v?.to_ascii_lowercase().as_str() {
        "years" | "year" => PivotDateGroup::Years,
        "quarters" | "quarter" => PivotDateGroup::Quarters,
        "months" | "month" => PivotDateGroup::Months,
        "days" | "day" => PivotDateGroup::Days,
        "none" | "ungroup" => PivotDateGroup::None,
        _ => return None,
    })
}

fn group(s: &mut Session, p: &Json) -> Result<Json> {
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

fn collapse(s: &mut Session, p: &Json) -> Result<Json> {
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

fn layout(s: &mut Session, p: &Json) -> Result<Json> {
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

fn grand_totals(s: &mut Session, p: &Json) -> Result<Json> {
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

fn refresh(s: &mut Session, p: &Json) -> Result<Json> {
    update(s, p, |_, src, _| Ok(json!({"records": src.rows.len()})))
}

fn refresh_all(s: &mut Session, p: &Json) -> Result<Json> {
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

fn change_source(s: &mut Session, p: &Json) -> Result<Json> {
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
        pt.rows.retain(|f| keep(&f.source_col, &mut dropped));
        pt.columns.retain(|f| keep(&f.source_col, &mut dropped));
        pt.values.retain(|f| keep(&f.source_col, &mut dropped));
        pt.filters.retain(|f| keep(&f.source_col, &mut dropped));
        Ok(json!({"source": source, "dropped": dropped}))
    })
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, pi) = find_pivot(s, p)?;
    edit(s, |cx| {
        pv::clear_output(&mut cx.wb, si, pi);
        let sh = cx.sheet_mut(si)?;
        if pi < sh.pivots.len() {
            sh.pivots.remove(pi);
        }
        cx.structural = true;
        Ok(json!({}))
    })
}

// ---------------------------------------------------------------- recommended

fn suggestions(wb: &Workbook, src: &pv::Source) -> Vec<Json> {
    let n = src.headers.len();
    let h = |i: usize| src.headers.get(i).cloned().unwrap_or_default();
    let distinct = |i: usize| pv::distinct_labels(wb, src, i, PivotDateGroup::None, 1001).len();
    let records = src.rows.len().max(1);
    let is_num = |i: usize| src.numeric.get(i).copied().unwrap_or(false);
    let is_date = |i: usize| src.dates.get(i).copied().unwrap_or(false);
    // Category columns: text with repeats.
    let cats: Vec<usize> = (0..n)
        .filter(|&i| {
            !is_num(i) && {
                let d = distinct(i);
                d >= 1 && (d < records || records <= 2) && d <= 1000
            }
        })
        .collect();
    let nums: Vec<usize> = (0..n).filter(|&i| is_num(i) && !is_date(i)).collect();
    let dates: Vec<usize> = (0..n).filter(|&i| is_date(i)).collect();
    let mut out = vec![];
    let mut push = |title: String, rows: Vec<Json>, cols: Vec<Json>, vals: Vec<Json>| {
        if out.len() < 5 {
            let idx = out.len();
            out.push(json!({"index": idx, "title": title, "rows": rows, "columns": cols, "values": vals}));
        }
    };
    if let (Some(&c), Some(&v)) = (cats.first(), nums.first()) {
        push(format!("Sum of {} by {}", h(v), h(c)), vec![json!(h(c))], vec![], vec![json!({"field": h(v), "func": "sum"})]);
    }
    if let (Some(&c), Some(&c2), Some(&v)) = (cats.first(), cats.get(1), nums.first()) {
        push(
            format!("Sum of {} by {} and {}", h(v), h(c), h(c2)),
            vec![json!(h(c))],
            vec![json!(h(c2))],
            vec![json!({"field": h(v), "func": "sum"})],
        );
    }
    if let (Some(&d), Some(&v)) = (dates.first(), nums.first()) {
        push(
            format!("Sum of {} by {} (quarters)", h(v), h(d)),
            vec![json!({"field": h(d), "group": "quarters"})],
            vec![],
            vec![json!({"field": h(v), "func": "sum"})],
        );
    }
    if let (Some(&c), Some(&v), Some(&v2)) = (cats.first(), nums.first(), nums.get(1)) {
        push(
            format!("Sum of {} and {} by {}", h(v), h(v2), h(c)),
            vec![json!(h(c))],
            vec![],
            vec![json!({"field": h(v), "func": "sum"}), json!({"field": h(v2), "func": "sum"})],
        );
    }
    if let Some(&c) = cats.first() {
        push(format!("Count of {} by {}", h(c), h(c)), vec![json!(h(c))], vec![], vec![json!({"field": h(c), "func": "count"})]);
    }
    if let (Some(&c), Some(&v)) = (cats.get(1), nums.first()) {
        push(format!("Average of {} by {}", h(v), h(c)), vec![json!(h(c))], vec![], vec![json!({"field": h(v), "func": "average"})]);
    }
    if cats.is_empty()
        && dates.is_empty()
        && let Some(&v) = nums.first()
    {
        push(format!("Sum of {}", h(v)), vec![], vec![], vec![json!({"field": h(v), "func": "sum"})]);
    }
    out
}

fn recommended(s: &mut Session, p: &Json) -> Result<Json> {
    let source = source_param(s, p)?;
    let active = s.doc()?.wb.active_sheet;
    let src = pv::read_source(&s.doc()?.wb, &source, active).map_err(other)?;
    let list = suggestions(&s.doc()?.wb, &src);
    let Some(apply) = p.get("apply").and_then(Json::as_u64) else {
        return Ok(json!({"source": source, "suggestions": list}));
    };
    let pick = list.get(apply as usize).cloned().ok_or_else(|| bad("insert.recommendedPivotTables", "no suggestion with that index"))?;
    let mut q = json!({
        "source": source,
        "rows": pick.get("rows").cloned().unwrap_or(json!([])),
        "columns": pick.get("columns").cloned().unwrap_or(json!([])),
        "values": pick.get("values").cloned().unwrap_or(json!([])),
    });
    if let (Some(m), Some(d)) = (q.as_object_mut(), p.get("destination")) {
        m.insert("destination".into(), d.clone());
    }
    if let (Some(m), Some(r)) = (q.as_object_mut(), p.get("replace")) {
        m.insert("replace".into(), r.clone());
    }
    let mut res = insert_pivot(s, &q)?;
    if let Some(m) = res.as_object_mut() {
        m.insert("suggestion".into(), pick);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellRef, Value};
    use serde_json::json;

    use crate::Session;

    /// Region, Product, Date, Units, Sales — 12 records over 2026.
    fn sales() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([
            ["Region", "Product", "Date", "Units", "Sales"],
            ["East", "Apples", "1/15/2026", 10, 100],
            ["West", "Apples", "2/10/2026", 5, 50],
            ["East", "Pears", "3/3/2026", 8, 120],
            ["North", "Pears", "4/20/2026", 2, 30],
            ["West", "Plums", "5/5/2026", 7, 70],
            ["East", "Plums", "6/30/2026", 4, 60],
            ["North", "Apples", "7/4/2026", 9, 90],
            ["West", "Pears", "8/8/2026", 3, 45],
            ["East", "Apples", "9/9/2026", 6, 60],
            ["North", "Plums", "10/10/2026", 1, 15],
            ["West", "Apples", "11/11/2026", 12, 120],
            ["East", "Pears", "12/12/2026", 5, 75]
        ]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
        s
    }
    const TOTAL: f64 = 835.0;

    fn val(s: &Session, a: &str) -> Value {
        s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a).unwrap())
    }
    fn text(s: &Session, a: &str) -> String {
        val(s, a).display()
    }
    fn pt(s: &Session) -> gridcraft_model::PivotTable {
        s.doc().unwrap().wb.active().unwrap().pivots.last().unwrap().clone()
    }
    /// Finds the row (0-based) whose first report column holds `label`.
    fn find_row(s: &Session, label: &str) -> Option<u32> {
        let p = pt(s);
        let r = p.last_range.unwrap();
        (r.start.row..=r.end.row).find(|row| val(s, &CellRef::new(*row, r.start.col).a1()).display() == label)
    }

    #[test]
    fn insert_creates_placeholder_on_new_sheet() {
        let mut s = sales();
        let r = s.execute("insert.pivotTable", json!({})).unwrap();
        assert_eq!(r["name"], "PivotTable1");
        assert_eq!(r["anchor"], "A3");
        assert_eq!(r["fields"], json!(["Region", "Product", "Date", "Units", "Sales"]));
        let d = s.doc().unwrap();
        assert_eq!(d.wb.sheets.len(), 2);
        assert_eq!(d.wb.active_sheet, 0, "new sheet goes before the current one");
        assert_eq!(d.wb.active().unwrap().name, "Sheet2");
        assert!(text(&s, "A3").starts_with("PivotTable1"));
        assert_eq!(pt(&s).source, "Sheet1!$A$1:$E$13");
        // One undo step removes it.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    }

    #[test]
    fn region_by_product_sum_of_sales() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({})).unwrap();
        s.execute("pivot.addField", json!({"field": "region", "area": "rows"})).unwrap();
        s.execute("pivot.addField", json!({"field": "Product", "area": "columns"})).unwrap();
        let r = s.execute("pivot.addField", json!({"field": "Sales", "area": "values"})).unwrap();
        assert_eq!(r["range"], "A3:E8");
        assert_eq!(text(&s, "A3"), "Sum of Sales");
        assert_eq!(text(&s, "B3"), "Column Labels");
        assert_eq!(text(&s, "A4"), "Row Labels");
        assert_eq!(text(&s, "B4"), "Apples");
        assert_eq!(text(&s, "C4"), "Pears");
        assert_eq!(text(&s, "D4"), "Plums");
        assert_eq!(text(&s, "E4"), "Grand Total");
        assert_eq!(text(&s, "A5"), "East");
        assert_eq!(text(&s, "A6"), "North");
        assert_eq!(text(&s, "A7"), "West");
        assert_eq!(text(&s, "A8"), "Grand Total");
        assert_eq!(val(&s, "B5"), Value::Number(160.0));
        assert_eq!(val(&s, "C5"), Value::Number(195.0));
        assert_eq!(val(&s, "E5"), Value::Number(415.0));
        assert_eq!(val(&s, "E8"), Value::Number(TOTAL));
        // Column grand totals add up too.
        let sum: f64 = ["B8", "C8", "D8"].iter().map(|a| val(&s, a).as_f64().unwrap()).sum();
        assert_eq!(sum, TOTAL);
        let rows: f64 = ["E5", "E6", "E7"].iter().map(|a| val(&s, a).as_f64().unwrap()).sum();
        assert_eq!(rows, TOTAL);
        // Styles: header bold, grand total bold with a top border.
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        assert!(d.wb.styles.get(sh.style_id(CellRef::parse("A4").unwrap())).font.bold);
        let g = d.wb.styles.get(sh.style_id(CellRef::parse("B8").unwrap()));
        assert!(g.font.bold && !g.borders.top.is_none());
        // Listing.
        let l = s.execute("pivot.list", json!({})).unwrap();
        assert_eq!(l[0]["values"][0]["name"], "Sum of Sales");
    }

    #[test]
    fn multiple_value_fields_make_a_values_dimension() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales", {"field": "Units", "func": "average"}]})).unwrap();
        assert_eq!(text(&s, "A3"), "Row Labels");
        assert_eq!(text(&s, "B3"), "Sum of Sales");
        assert_eq!(text(&s, "C3"), "Average of Units");
        assert_eq!(text(&s, "A7"), "Grand Total");
        assert_eq!(val(&s, "B7"), Value::Number(TOTAL));
        assert_eq!(val(&s, "C7"), Value::Number(72.0 / 12.0));
        // With a column field the values become the innermost column level.
        s.execute("pivot.addField", json!({"field": "Product", "area": "columns"})).unwrap();
        assert_eq!(text(&s, "B4"), "Apples");
        assert_eq!(text(&s, "B5"), "Sum of Sales");
        assert_eq!(text(&s, "C5"), "Average of Units");
        assert_eq!(text(&s, "D4"), "Pears");
        assert_eq!(text(&s, "H4"), "Total Sum of Sales");
        assert_eq!(text(&s, "I4"), "Total Average of Units");
        let last = pt(&s).last_range.unwrap();
        assert_eq!(val(&s, &CellRef::new(last.end.row, 7).a1()), Value::Number(TOTAL));
    }

    #[test]
    fn date_grouping_by_quarter_and_month() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Date"], "values": ["Sales"]})).unwrap();
        // Ungrouped: one row per date, shown with the source date format.
        assert_eq!(pt(&s).last_range.unwrap().height(), 14);
        s.execute("pivot.group", json!({"field": "Date", "by": "quarters"})).unwrap();
        assert_eq!(text(&s, "A4"), "Qtr1");
        assert_eq!(text(&s, "A7"), "Qtr4");
        assert_eq!(val(&s, "B4"), Value::Number(270.0));
        assert_eq!(val(&s, "B5"), Value::Number(160.0));
        assert_eq!(val(&s, "B8"), Value::Number(TOTAL));
        s.execute("pivot.group", json!({"field": "Date", "by": "months"})).unwrap();
        assert_eq!(text(&s, "A4"), "Jan");
        assert_eq!(text(&s, "A15"), "Dec");
        s.execute("pivot.group", json!({"field": "Date", "by": "years"})).unwrap();
        assert_eq!(text(&s, "A4"), "2026");
        assert!(s.execute("pivot.group", json!({"field": "Region", "by": "years"})).is_err());
    }

    #[test]
    fn filters_and_report_filter_rows() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
        // Report filter above the body; the body stays at A3.
        assert_eq!(text(&s, "A1"), "Region");
        assert_eq!(text(&s, "B1"), "(All)");
        assert_eq!(text(&s, "A3"), "Row Labels");
        s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
        assert_eq!(text(&s, "B1"), "East");
        let gt = find_row(&s, "Grand Total").unwrap();
        assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(415.0));
        s.execute("pivot.filter", json!({"field": "Region", "selected": ["East", "west"]})).unwrap();
        assert_eq!(text(&s, "B1"), "(Multiple Items)");
        s.execute("pivot.filter", json!({"field": "Region", "selected": null})).unwrap();
        let gt = find_row(&s, "Grand Total").unwrap();
        assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(TOTAL));
        // Hidden items on a row field.
        s.execute("pivot.filter", json!({"field": "Product", "selected": ["Apples"]})).unwrap();
        assert_eq!(text(&s, "A4"), "Apples");
        assert_eq!(text(&s, "A5"), "Grand Total");
        assert_eq!(val(&s, "B5"), Value::Number(420.0));
        assert!(s.execute("pivot.filter", json!({"field": "Product", "selected": []})).is_err());
    }

    #[test]
    fn show_values_as() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
        s.execute("pivot.valueSettings", json!({"field": "Sum of Sales", "showAs": "percentOfGrandTotal"})).unwrap();
        let east = val(&s, "B4").as_f64().unwrap();
        assert!((east - 415.0 / TOTAL).abs() < 1e-12);
        assert_eq!(val(&s, "B7"), Value::Number(1.0));
        assert_eq!(text(&s, "B7"), "1");
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        assert_eq!(d.wb.styles.get(sh.style_id(CellRef::parse("B4").unwrap())).num_fmt.as_str(), "0.00%");
        s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "runningTotal"})).unwrap();
        assert_eq!(val(&s, "B4"), Value::Number(415.0));
        assert_eq!(val(&s, "B5"), Value::Number(415.0 + 135.0));
        assert_eq!(val(&s, "B6"), Value::Number(TOTAL));
        s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "rank"})).unwrap();
        assert_eq!(val(&s, "B4"), Value::Number(1.0));
        assert_eq!(val(&s, "B5"), Value::Number(3.0));
        assert_eq!(val(&s, "B6"), Value::Number(2.0));
        s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "normal", "func": "max", "numberFormat": "#,##0"})).unwrap();
        assert_eq!(text(&s, "B3"), "Max of Sales");
        assert_eq!(val(&s, "B4"), Value::Number(120.0));
        assert!(s.execute("pivot.valueSettings", json!({"index": 0, "name": "Sales"})).is_err());
        s.execute("pivot.valueSettings", json!({"index": 0, "name": "Biggest sale"})).unwrap();
        assert_eq!(text(&s, "B3"), "Biggest sale");
    }

    #[test]
    fn compact_vs_tabular_shapes() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Sales"]})).unwrap();
        // Compact: one label column; parents carry subtotals at the top; children indented.
        let r = pt(&s).last_range.unwrap();
        assert_eq!(r.width(), 2);
        // header + 3 regions + 9 (region, product) pairs present + grand total
        assert_eq!(r.height(), 1 + 3 + 9 + 1);
        assert_eq!(text(&s, "A4"), "East");
        assert_eq!(val(&s, "B4"), Value::Number(415.0));
        assert_eq!(text(&s, "A5"), "Apples");
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        assert_eq!(d.wb.styles.get(sh.style_id(CellRef::parse("A5").unwrap())).align.indent, 1);
        assert!(d.wb.styles.get(sh.style_id(CellRef::parse("A4").unwrap())).font.bold);
        // Outline: one column per field.
        s.execute("pivot.layout", json!({"layout": "outline"})).unwrap();
        let r = pt(&s).last_range.unwrap();
        assert_eq!(r.width(), 3);
        assert_eq!(text(&s, "A3"), "Region");
        assert_eq!(text(&s, "B3"), "Product");
        assert_eq!(text(&s, "A4"), "East");
        assert_eq!(text(&s, "B5"), "Apples");
        // Tabular: first child on its parent's row; "East Total" rows below each group.
        s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
        let r = pt(&s).last_range.unwrap();
        assert_eq!(r.width(), 3);
        assert_eq!(r.height(), 1 + 9 + 3 + 1);
        assert_eq!(text(&s, "A4"), "East");
        assert_eq!(text(&s, "B4"), "Apples");
        assert_eq!(text(&s, "A5"), "");
        assert_eq!(text(&s, "B5"), "Pears");
        assert_eq!(text(&s, "A7"), "East Total");
        assert_eq!(val(&s, "C7"), Value::Number(415.0));
        // No subtotals, no grand totals.
        s.execute("pivot.layout", json!({"subtotals": false, "grandTotalsCols": false})).unwrap();
        assert_eq!(pt(&s).last_range.unwrap().height(), 1 + 9);
        // Collapse East in compact layout: its children disappear.
        s.execute("pivot.layout", json!({"layout": "compact", "subtotals": "top"})).unwrap();
        s.execute("pivot.collapse", json!({"field": "Region", "items": ["East"]})).unwrap();
        assert_eq!(text(&s, "A4"), "East");
        assert_eq!(text(&s, "A5"), "North");
        s.execute("pivot.collapse", json!({"field": "Region", "collapse": false})).unwrap();
        assert_eq!(text(&s, "A5"), "Apples");
    }

    #[test]
    fn refresh_after_source_edits() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
        s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
        s.execute("cell.set", json!({"cell": "E2", "input": "1100"})).unwrap();
        // Refresh All picks the change up.
        let r = s.execute("data.refreshAll", json!({})).unwrap();
        assert_eq!(r["refreshed"], 1);
        let d = s.doc().unwrap();
        let si = d.wb.sheet_index("Sheet2").unwrap();
        let sh = d.wb.sheet(si).unwrap();
        assert_eq!(sh.value(CellRef::parse("B4").unwrap()), Value::Number(1415.0));
        assert_eq!(sh.value(CellRef::parse("B7").unwrap()), Value::Number(TOTAL + 1000.0));
        // pivot.refresh by name; change source to fewer rows.
        s.execute("pivot.changeSource", json!({"pivot": "PivotTable1", "source": "Sheet1!A1:E3"})).unwrap();
        let d = s.doc().unwrap();
        let sh = d.wb.sheet(si).unwrap();
        assert_eq!(sh.value(CellRef::parse("A6").unwrap()).display(), "Grand Total");
        assert_eq!(sh.value(CellRef::parse("B6").unwrap()), Value::Number(1150.0));
        // Old rows were cleared.
        assert_eq!(sh.value(CellRef::parse("A7").unwrap()), Value::Empty);
        s.execute("pivot.refresh", json!({"pivot": "PivotTable1"})).unwrap();
    }

    #[test]
    fn replace_protection() {
        let mut s = sales();
        s.execute("cell.set", json!({"cell": "H5", "input": "keep me"})).unwrap();
        let e = s.execute("insert.pivotTable", json!({"destination": "Sheet1!H3", "rows": ["Region"], "values": ["Sales"]})).unwrap_err().to_string();
        assert!(e.contains("There's already data in Sheet1!H5"), "{e}");
        assert_eq!(s.doc().unwrap().wb.active().unwrap().pivots.len(), 0);
        // Growing into data is refused as well.
        s.execute("insert.pivotTable", json!({"destination": "Sheet1!H3"})).unwrap();
        assert!(s.execute("pivot.addField", json!({"field": "Region", "area": "rows"})).is_err());
        s.execute("pivot.addField", json!({"field": "Region", "area": "rows", "replace": true})).unwrap();
        assert_eq!(text(&s, "H5"), "North");
        // A second pivot can't overlap the first.
        let e =
            s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "destination": "Sheet1!H6", "replace": true})).unwrap_err().to_string();
        assert!(e.contains("overlap"), "{e}");
        // Nor its own source data.
        let e =
            s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "destination": "Sheet1!B2", "replace": true})).unwrap_err().to_string();
        assert!(e.contains("source data"), "{e}");
        // Delete clears the report.
        s.execute("pivot.delete", json!({})).unwrap();
        assert_eq!(val(&s, "H5"), Value::Empty);
        assert!(s.doc().unwrap().wb.active().unwrap().pivots.is_empty());
    }

    #[test]
    fn fields_move_remove_sort() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
        let f = s.execute("pivot.fields", json!({})).unwrap();
        assert_eq!(f["fields"][0]["name"], "Region");
        assert_eq!(f["fields"][0]["samples"], json!(["East", "North", "West"]));
        assert_eq!(f["fields"][0]["areas"], json!(["rows"]));
        assert_eq!(f["fields"][4]["numeric"], true);
        assert_eq!(f["fields"][2]["date"], true);
        s.execute("pivot.sort", json!({"field": "Region", "order": "desc"})).unwrap();
        assert_eq!(text(&s, "A4"), "West");
        s.execute("pivot.moveField", json!({"field": "Region", "from": "rows", "to": "columns"})).unwrap();
        assert_eq!(text(&s, "B4"), "West");
        s.execute("pivot.removeField", json!({"field": "Sum of Sales", "area": "values"})).unwrap();
        s.execute("pivot.addField", json!({"field": "Region", "area": "values"})).unwrap();
        assert_eq!(pt(&s).values[0].name, "Count of Region");
        s.execute("pivot.removeField", json!({"field": "Region"})).unwrap();
        assert!(pt(&s).columns.is_empty() && pt(&s).values.is_empty());
        assert!(text(&s, "A3").contains("choose fields"));
        assert!(s.execute("pivot.addField", json!({"field": "Nope", "area": "rows"})).is_err());
        assert!(s.execute("pivot.addField", json!({"field": "Region", "area": "sideways"})).is_err());
    }

    #[test]
    fn recommended_and_summarize() {
        let mut s = sales();
        let r = s.execute("insert.recommendedPivotTables", json!({})).unwrap();
        let list = r["suggestions"].as_array().unwrap();
        assert!((3..=5).contains(&list.len()), "{list:?}");
        assert_eq!(list[0]["title"], "Sum of Units by Region");
        let r = s.execute("insert.recommendedPivotTables", json!({"apply": 2})).unwrap();
        assert!(r["suggestion"]["title"].as_str().unwrap().contains("quarters"));
        assert_eq!(text(&s, "A4"), "Qtr1");
        // From a table.
        s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:E13"})).unwrap();
        s.execute("selection.set", json!({"range": "B2"})).unwrap();
        let r = s.execute("table.summarizePivot", json!({"rows": ["Product"], "values": ["Units"]})).unwrap();
        assert_eq!(pt(&s).source, "Table1");
        assert_eq!(r["name"], "PivotTable2");
        let gt = find_row(&s, "Grand Total").unwrap();
        assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(72.0));
    }

    #[test]
    fn repeated_headers_get_numbered_names() {
        let mut s = Session::new();
        s.new_workbook();
        let mut header = vec![json!("a"), json!("A"), json!("a"), json!("a2")];
        // Thousands of equal headers (a row of check boxes, say) used to take cubic time; this would
        // hang rather than fail a wall-clock check, which is flaky on a loaded machine.
        header.extend((0..3000).map(|_| json!("x")));
        let data: Vec<_> = (0..header.len()).map(|i| json!(i)).collect();
        let end = CellRef::new(1, header.len() as u32 - 1).a1();
        s.execute("range.setValues", json!({"range": "A1", "values": [header, data]})).unwrap();
        let src = crate::pivot::read_source(&s.doc().unwrap().wb, &format!("Sheet1!A1:{end}"), 0).unwrap();
        assert_eq!(&src.headers[..4], ["a", "A2", "a3", "a22"]);
        assert_eq!(src.headers[4], "x");
        assert_eq!(src.headers.last().map(String::as_str), Some("x3000"));
        // Table column names follow the same rule.
        s.execute("insert.table", json!({"range": format!("A1:{end}"), "header": true})).unwrap();
        let text = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["text"].clone();
        assert_eq!(text(&mut s, "B1"), "A2");
        let last = CellRef::new(0, 3003).a1();
        assert_eq!(text(&mut s, &last), "x3000");
    }

    #[test]
    fn hostile_params_never_panic() {
        let mut s = sales();
        let cmds = [
            "insert.pivotTable",
            "insert.recommendedPivotTables",
            "table.summarizePivot",
            "pivot.fields",
            "pivot.fieldList",
            "pivot.list",
            "pivot.addField",
            "pivot.removeField",
            "pivot.moveField",
            "pivot.valueSettings",
            "pivot.filter",
            "pivot.sort",
            "pivot.group",
            "pivot.collapse",
            "pivot.layout",
            "pivot.grandTotals",
            "pivot.subtotals",
            "pivot.reportLayout",
            "pivot.style",
            "pivot.refresh",
            "data.refreshAll",
            "pivot.changeSource",
            "pivot.delete",
        ];
        let hostile = [
            json!({}),
            json!([1, 2]),
            json!({"pivot": 99999, "field": 5, "area": null}),
            json!({"source": "ZZZ999999999", "destination": "Nope!A1"}),
            json!({"source": "A:XFD", "destination": "Sheet1!XFD1048576"}),
            json!({"source": "''!A1", "destination": "!"}),
            json!({"field": "Sales", "area": "values", "func": "bogus", "position": 4294967295u64}),
            json!({"field": "Region", "selected": [null, 1, {"x": 1}], "index": 18446744073709551615u64, "showAs": "rank"}),
            json!({"apply": 99, "layout": "weird", "subtotals": 7, "style": "x".repeat(500)}),
            json!({"field": "Date", "by": "decades", "items": "nope", "order": "sideways"}),
        ];
        for c in cmds {
            for h in &hostile {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let spec = crate::find_command(c).unwrap();
                    let _ = (spec.run)(&mut s, h);
                }));
                assert!(r.is_ok(), "{c} panicked on {h}");
            }
        }
        // And on a real pivot.
        s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "rows": ["Region"], "values": ["Sales"]})).unwrap();
        for c in cmds {
            for h in &hostile {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let spec = crate::find_command(c).unwrap();
                    let _ = (spec.run)(&mut s, h);
                }));
                assert!(r.is_ok(), "{c} panicked on {h}");
            }
        }
    }

    #[test]
    fn empty_header_is_refused() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Region", ""], ["East", 1]]})).unwrap();
        let e = s.execute("insert.pivotTable", json!({"source": "A1:B2"})).unwrap_err().to_string();
        assert!(e.contains("field name is not valid"), "{e}");
        assert!(s.execute("insert.pivotTable", json!({"source": "NoSuchTable"})).is_err());
    }
}
