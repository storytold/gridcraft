use super::{
    CUSTOM_SUBTOTALS, CellRef, Ctx, DateSystem, EXT_X14_FIELD, EXT_X14_TABLE, EXT_XPDL_TABLE, El, HashMap, IoError, MAX_FIELDS, PivotCalcField,
    PivotDateGroup, PivotField, PivotFilter, PivotFunc, PivotLayout, PivotScopeItem, PivotShowAs, PivotSort, PivotTable, PivotValue, RangeRef,
    VALUES_FIELD, abs_range, func_caption, func_from, number_to_text, quote_sheet, same, serial_from_iso_ymd,
};

/// A cache field as read.
#[derive(Clone, Debug, Default)]
struct FieldIn {
    name: String,
    database: bool,
    /// Shared item captions.
    items: Vec<String>,
    /// (base field, grouping, group item captions); grouping `None` = unsupported grouping.
    group: Option<(Option<usize>, Option<PivotDateGroup>, Vec<String>)>,
    /// A calculated field's formula.
    formula: Option<String>,
}

impl FieldIn {
    fn labels(&self) -> &[String] {
        match &self.group {
            Some((_, _, items)) if !items.is_empty() => items,
            _ => &self.items,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct CacheIn {
    source: String,
    fields: Vec<FieldIn>,
}

/// Workbook-level pivot cache lookup, filled from `<pivotCaches>`; caches are parsed once.
#[derive(Default)]
pub struct CachesIn {
    by_id: HashMap<u32, String>,
    parsed: HashMap<String, Option<CacheIn>>,
    next_id: u32,
}

impl CachesIn {
    pub fn new(wb_xml: &El, wb_rels: &[crate::package::Rel]) -> CachesIn {
        let mut by_id = HashMap::new();
        if let Some(pc) = wb_xml.child("pivotCaches") {
            for c in pc.kids("pivotCache") {
                if let (Some(id), Some(rid)) = (c.attr_u32("cacheId"), c.attr("id"))
                    && let Some(r) = wb_rels.iter().find(|r| r.id == rid)
                {
                    by_id.insert(id, r.target.clone());
                }
            }
        }
        CachesIn { by_id, parsed: HashMap::new(), next_id: 1 }
    }
}

fn item_label_el(e: &El, sys: DateSystem) -> String {
    let v = e.attr("v").unwrap_or("");
    match e.name.as_str() {
        "m" => "(blank)".into(),
        "n" => v.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(number_to_text).unwrap_or_else(|| v.to_string()),
        "b" => if matches!(v.trim(), "1" | "true") { "TRUE" } else { "FALSE" }.into(),
        // Cache dates count real days from the epoch, as Excel writes them (see `iso`).
        "d" => crate::sheet_read::parse_iso_datetime_with(v, |y, m, d| serial_from_iso_ymd(sys, y, m, d))
            .map(number_to_text)
            .unwrap_or_else(|| v.to_string()),
        _ => v.to_string(),
    }
}

fn read_cache(cx: &mut Ctx<'_>, part: &str, sheet_name: &str) -> Option<CacheIn> {
    let x = cx.optional_xml(part).ok().flatten()?;
    if x.name != "pivotCacheDefinition" {
        cx.warn(format!("{part} is not a pivot cache definition; its PivotTable was kept as plain cells"));
        return None;
    }
    let src = x.child("cacheSource");
    let ty = src.and_then(|s| s.attr("type")).unwrap_or("worksheet");
    if ty != "worksheet" {
        cx.warn(format!("PivotTables with a {ty} data source are not supported; kept as plain cells"));
        return None;
    }
    let ws = src.and_then(|s| s.child("worksheetSource"));
    let source = match ws {
        Some(w) if w.attr("name").is_some_and(|n| !n.trim().is_empty()) => w.attr("name").unwrap_or("").trim().to_string(),
        Some(w) => {
            if w.attr("id").is_some() {
                cx.warn("a PivotTable's source is in another workbook; its source reference may not resolve");
            }
            let r = w.attr("ref").map(|r| r.replace('$', "")).and_then(|r| RangeRef::parse(&r));
            let Some(r) = r else {
                cx.warn("a PivotTable's source reference is missing or invalid; kept as plain cells");
                return None;
            };
            let sheet = w.attr("sheet").filter(|s| !s.is_empty()).unwrap_or(sheet_name);
            format!("{}!{}", quote_sheet(sheet), abs_range(&r))
        }
        None => {
            cx.warn("a PivotTable's cache has no worksheet source; kept as plain cells");
            return None;
        }
    };
    let sys = cx.date_system;
    let mut fields = Vec::new();
    if let Some(cf) = x.child("cacheFields") {
        for f in cf.kids("cacheField").take(MAX_FIELDS) {
            let items = f.child("sharedItems").map(|si| si.children.iter().map(|e| item_label_el(e, sys)).collect()).unwrap_or_default();
            let group = f.child("fieldGroup").map(|g| {
                let base = g.attr_u32("base").map(|b| b as usize);
                let by = match g.child("rangePr").map(|r| r.attr("groupBy").unwrap_or("range")) {
                    Some("years") => Some(PivotDateGroup::Years),
                    Some("quarters") => Some(PivotDateGroup::Quarters),
                    Some("months") => Some(PivotDateGroup::Months),
                    Some("days") => Some(PivotDateGroup::Days),
                    _ => None,
                };
                let gi = g.child("groupItems").map(|gi| gi.children.iter().map(|e| item_label_el(e, sys)).collect()).unwrap_or_default();
                (base, by, gi)
            });
            fields.push(FieldIn {
                name: f.attr("name").unwrap_or("").to_string(),
                database: f.flag("databaseField", true),
                items,
                group,
                formula: f.attr("formula").map(str::to_string),
            });
        }
    }
    if fields.is_empty() {
        cx.warn("a PivotTable's cache has no fields; kept as plain cells");
        return None;
    }
    Some(CacheIn { source, fields })
}

/// Reads the PivotTables related to a worksheet part.
pub fn read_pivots(cx: &mut Ctx<'_>, caches: &mut CachesIn, sheet_part: &str, sheet_name: &str) -> Result<Vec<PivotTable>, IoError> {
    let rels = cx.pkg.rels(sheet_part)?;
    let mut out = Vec::new();
    for rel in rels.iter().filter(|r| r.kind == "pivotTable" && !r.external) {
        let Some(x) = cx.optional_xml(&rel.target)? else { continue };
        if x.name != "pivotTableDefinition" {
            cx.warn(format!("{} is not a PivotTable definition; skipped", rel.target));
            continue;
        }
        // The cache: the table's own relationship, else the workbook's cache id.
        let trels = cx.pkg.rels(&rel.target)?;
        let cache_part = trels
            .iter()
            .find(|r| r.kind == "pivotCacheDefinition" && !r.external)
            .map(|r| r.target.clone())
            .or_else(|| x.attr_u32("cacheId").and_then(|id| caches.by_id.get(&id).cloned()));
        let Some(cache_part) = cache_part else {
            cx.warn(format!("PivotTable in {} has no cache; kept as plain cells", rel.target));
            continue;
        };
        let cache = match caches.parsed.get(&cache_part) {
            Some(c) => c.clone(),
            None => {
                let c = read_cache(cx, &cache_part, sheet_name);
                caches.parsed.insert(cache_part.clone(), c.clone());
                c
            }
        };
        let Some(cache) = cache else { continue };
        if let Some(pt) = read_table(cx, &x, &cache, caches.next_id) {
            caches.next_id = caches.next_id.saturating_add(1);
            out.push(pt);
        }
    }
    Ok(out)
}

/// A pivot field's data items (not subtotal or grand-total items).
fn items_of(e: &El) -> Vec<&El> {
    e.child("items").map(|it| it.kids("item").filter(|i| i.attr("t").is_none_or(|t| t == "data")).collect()).unwrap_or_default()
}

/// Whether a `<pivotField>` shows subtotals: custom subtotal functions (Field Settings ›
/// Subtotals › Custom, `sumSubtotal` and the like, ECMA-376 §18.10.1.69) turn them on as
/// well as `defaultSubtotal`.
fn field_subtotals(e: &El) -> bool {
    has_custom_subtotals(e) || e.flag("defaultSubtotal", true)
}

fn has_custom_subtotals(e: &El) -> bool {
    CUSTOM_SUBTOTALS.iter().any(|a| e.flag(a, false))
}

/// The x14 element of an `<extLst><ext uri>` (e.g. `x14:pivotField`), if present.
fn x14_ext<'a>(e: &'a El, uri: &str) -> Option<&'a El> {
    let ext = e.child("extLst")?.kids("ext").find(|x| x.attr("uri").is_some_and(|u| u.eq_ignore_ascii_case(uri)))?;
    ext.children.first()
}

/// A pivot field's sort by value, from its `<autoSortScope>`: the data field (dataFields
/// position) the scope's Values reference (`4294967294`) names, and the other references as
/// (cache field, index into that field's `<items>`). `Err(())` for a scope GridCraft can't
/// represent (a reference naming no item or several).
type AutoSort = Result<(usize, Vec<(usize, usize)>), ()>;

fn auto_sort_scope(e: &El) -> Option<AutoSort> {
    let refs = e.path(&["autoSortScope", "pivotArea", "references"])?;
    let r = refs.kids("reference").find(|r| r.attr("field") == Some(VALUES_FIELD))?;
    let data = r.child("x").and_then(|x| x.attr_u32("v"))? as usize;
    let mut scope = Vec::new();
    for r in refs.kids("reference").filter(|r| r.attr("field") != Some(VALUES_FIELD)) {
        let xs: Vec<&El> = r.kids("x").collect();
        match (r.attr_u32("field"), xs.as_slice()) {
            (Some(f), [x]) if !r.flag("byPosition", false) && !r.flag("relative", false) => match x.attr_u32("v") {
                Some(v) => scope.push((f as usize, v as usize)),
                None => return Some(Err(())),
            },
            _ => return Some(Err(())),
        }
    }
    Some(Ok((data, scope)))
}

/// (source column name, date grouping) of a cache field used on an axis.
fn field_ref(cx: &mut Ctx<'_>, cache: &CacheIn, i: usize) -> Option<(String, PivotDateGroup)> {
    let f = cache.fields.get(i)?;
    if f.formula.is_some() {
        cx.warn("calculated PivotTable fields are not supported");
        return None;
    }
    match &f.group {
        Some((base, by, _)) => {
            let by = match by {
                Some(b) => *b,
                None => {
                    cx.warn(format!("PivotTable grouping of \"{}\" is not supported; the field is ungrouped", f.name));
                    PivotDateGroup::None
                }
            };
            let base_name = if f.database { f.name.clone() } else { base.and_then(|b| cache.fields.get(b))?.name.clone() };
            Some((base_name, by))
        }
        None if !f.database => {
            cx.warn(format!("PivotTable field \"{}\" (grouped items) is not supported", f.name));
            None
        }
        None => Some((f.name.clone(), PivotDateGroup::None)),
    }
}

fn read_table(cx: &mut Ctx<'_>, x: &El, cache: &CacheIn, id: u32) -> Option<PivotTable> {
    let mut pt = PivotTable { id, ..PivotTable::default() };
    if let Some(n) = x.attr("name").filter(|n| !n.trim().is_empty()) {
        pt.name = n.to_string();
    } else {
        pt.name = format!("PivotTable{id}");
    }
    pt.source = cache.source.clone();
    let Some(loc) = x.child("location").and_then(|l| l.attr("ref")).and_then(|r| RangeRef::parse(&r.replace('$', ""))) else {
        cx.warn(format!("PivotTable \"{}\" has no valid location; kept as plain cells", pt.name));
        return None;
    };
    let pfs: Vec<&El> = x.child("pivotFields").map(|p| p.kids("pivotField").collect()).unwrap_or_default();
    let pf = |i: usize| pfs.get(i).copied();
    let idx_list = |name: &str, attr: &str| -> Vec<i64> {
        x.child(name).map(|e| e.children.iter().filter_map(|f| f.attr_i64(attr)).collect()).unwrap_or_default()
    };
    let row_x = idx_list("rowFields", "x");
    let col_x = idx_list("colFields", "x");
    let pages: Vec<&El> = x.child("pageFields").map(|p| p.kids("pageField").collect()).unwrap_or_default();

    // Layout from the first axis field (pivotField defaults: compact and outline on).
    let first_axis = row_x.iter().chain(col_x.iter()).find(|i| **i >= 0).and_then(|i| pf(*i as usize));
    let (compact, outline) = match first_axis {
        Some(f) => (f.flag("compact", true), f.flag("outline", true)),
        None => (x.flag("compact", true), x.flag("outline", false)),
    };
    pt.layout = if compact {
        PivotLayout::Compact
    } else if outline {
        PivotLayout::Outline
    } else {
        PivotLayout::Tabular
    };
    if let Some(f) = first_axis {
        pt.subtotals = x14_ext(x, EXT_XPDL_TABLE)
            .and_then(|e| e.attr("EnabledSubtotalsDefault"))
            .map_or_else(|| field_subtotals(f), |v| v == "1" || v.eq_ignore_ascii_case("true"));
        pt.subtotals_top = f.flag("subtotalTop", true);
    }
    pt.grand_totals_rows = x.flag("colGrandTotals", true);
    pt.grand_totals_cols = x.flag("rowGrandTotals", true);
    pt.show_headers = x.flag("showHeaders", true);
    pt.enable_show_details = x.flag("enableDrill", true);
    pt.style = x.child("pivotTableStyleInfo").and_then(|s| s.attr("name")).unwrap_or("").to_string();
    let caption = |a: &str| x.attr(a).filter(|c| !c.is_empty()).map(str::to_string);
    pt.row_header_caption = caption("rowHeaderCaption");
    pt.col_header_caption = caption("colHeaderCaption");
    pt.data_caption = caption("dataCaption").filter(|c| c != "Values");
    pt.grand_total_caption = caption("grandTotalCaption");
    pt.empty_caption = caption("missingCaption").filter(|_| x.flag("showMissing", true));
    pt.show_values_row = !x14_ext(x, EXT_X14_TABLE).is_some_and(|e| e.flag("hideValuesRow", false));
    pt.classic_layout = x.flag("gridDropZones", false);
    if x.child("calculatedItems").is_some() || x.path(&["filters", "filter"]).is_some() {
        cx.warn(format!("PivotTable \"{}\": calculated items and label/value filters are not supported", pt.name));
    }

    let mut filters: Vec<PivotFilter> = Vec::new();

    // Per axis field (in rows-then-columns order): its cache field and its sort by value.
    let mut sort_by: Vec<Option<AutoSort>> = Vec::new();
    let mut axis_ix: Vec<usize> = Vec::new();
    let table_subtotals = pt.subtotals;
    // (fields, position of Σ Values (`-2`) among them)
    let mut axis = |cx: &mut Ctx<'_>, list: &[i64], filters: &mut Vec<PivotFilter>| -> (Vec<PivotField>, Option<usize>) {
        let mut v = Vec::new();
        let mut values_at = None;
        for &i in list {
            if i == -2 {
                values_at = Some(v.len());
                continue;
            }
            if i < 0 {
                continue;
            }
            let i = i as usize;
            let Some((name, group)) = field_ref(cx, cache, i) else { continue };
            let labels = cache.fields.get(i).map(|f| f.labels().to_vec()).unwrap_or_default();
            let mut f = PivotField { source_col: name.clone(), date_group: group, sort: PivotSort::None, ..PivotField::default() };
            let mut by = None;
            if let Some(e) = pf(i) {
                f.sort = match e.attr("sortType") {
                    Some("ascending") => PivotSort::Asc,
                    Some("descending") => PivotSort::Desc,
                    _ => PivotSort::None,
                };
                by = auto_sort_scope(e);
                f.caption = e.attr("name").filter(|n| !n.is_empty()).map(str::to_string);
                let custom = has_custom_subtotals(e);
                if custom {
                    cx.warn(format!("PivotTable field \"{name}\": custom subtotal functions are not supported; it shows the automatic subtotal"));
                }
                let own = field_subtotals(e);
                f.subtotal = (own != table_subtotals).then_some(own);
                f.repeat_labels = x14_ext(e, EXT_X14_FIELD).is_some_and(|x| x.flag("fillDownLabels", false));
                if group == PivotDateGroup::None {
                    f.number_format = e
                        .attr_u32("numFmtId")
                        .filter(|id| *id != 0)
                        .and_then(|id| crate::tables::builtin_format(id).map(str::to_string).or_else(|| cx.num_fmts.get(&id).cloned()));
                }
                let items = items_of(e);
                let lab = |it: &El| it.attr_u32("x").and_then(|k| labels.get(k as usize)).cloned();
                f.collapsed_items = items.iter().filter(|it| !it.flag("sd", true)).filter_map(|it| lab(it)).collect();
                f.item_captions = items.iter().filter_map(|it| Some((lab(it)?, it.attr("n").filter(|n| !n.is_empty())?.to_string()))).collect();
                // Not auto-sorted: the `<items>` list fixes the order, even when it happens to be
                // cache order (new items follow it). The engine shows date groups in date order,
                // so they keep a list only when it was reordered.
                let xs: Vec<u32> = items.iter().filter_map(|it| it.attr_u32("x")).collect();
                let reordered = xs.windows(2).any(|w| w[0] > w[1]);
                if f.sort == PivotSort::None && (group == PivotDateGroup::None || reordered) {
                    f.item_order = items.iter().filter_map(|it| lab(it)).collect();
                }
                if items.iter().any(|it| it.flag("h", false)) {
                    let shown: Vec<String> = items.iter().filter(|it| !it.flag("h", false)).filter_map(|it| lab(it)).collect();
                    if !filters.iter().any(|ff| same(&ff.source_col, &name)) {
                        filters.push(PivotFilter { source_col: name.clone(), selected: Some(shown) });
                    }
                }
            }
            sort_by.push(by);
            axis_ix.push(i);
            v.push(f);
        }
        (v, values_at)
    };
    let mut page_filters = Vec::new();
    for p in &pages {
        let Some(i) = p.attr_i64("fld").filter(|i| *i >= 0).map(|i| i as usize) else { continue };
        let Some((name, _)) = field_ref(cx, cache, i) else { continue };
        let labels = cache.fields.get(i).map(|f| f.labels().to_vec()).unwrap_or_default();
        let selected = if let Some(k) = p.attr_u32("item") {
            labels.get(k as usize).map(|l| vec![l.clone()])
        } else {
            pf(i).and_then(|e| {
                let items = items_of(e);
                items.iter().any(|it| it.flag("h", false)).then(|| {
                    items
                        .iter()
                        .filter(|it| !it.flag("h", false))
                        .filter_map(|it| it.attr_u32("x").and_then(|k| labels.get(k as usize)).cloned())
                        .collect()
                })
            })
        };
        page_filters.push(PivotFilter { source_col: name, selected });
    }
    let mut axis_filters = Vec::new();
    let (rows, rows_values) = axis(cx, &row_x, &mut axis_filters);
    let (columns, cols_values) = axis(cx, &col_x, &mut axis_filters);
    pt.rows = rows;
    pt.columns = columns;
    // Σ Values: where its `-2` entry is, else `dataOnRows` and `dataPosition`; after the last
    // field by default. In Columns it always comes last, as the column layout puts it there.
    pt.values_on_rows = rows_values.is_some() || (cols_values.is_none() && x.flag("dataOnRows", false));
    pt.values_position =
        if pt.values_on_rows { rows_values.or_else(|| x.attr_u32("dataPosition").map(|p| p as usize)).filter(|p| *p < pt.rows.len()) } else { None };
    filters.extend(page_filters);
    filters.extend(axis_filters);
    pt.filters = filters;

    // Values.
    // dataFields position -> value index.
    let mut data_value: Vec<Option<usize>> = Vec::new();
    if let Some(df) = x.child("dataFields") {
        for d in df.kids("dataField") {
            data_value.push(None);
            let Some(i) = d.attr_u32("fld").map(|i| i as usize) else { continue };
            let Some(f) = cache.fields.get(i) else { continue };
            let func = match d.attr("subtotal") {
                None => PivotFunc::Sum,
                Some(s) => func_from(s).unwrap_or_else(|| {
                    cx.warn(format!("PivotTable function {s} is not supported; using Sum"));
                    PivotFunc::Sum
                }),
            };
            let ext = d
                .child("extLst")
                .into_iter()
                .flat_map(|l| l.kids("ext"))
                .filter_map(|e| e.child("dataField"))
                .find_map(|e| e.attr("pivotShowAs").map(str::to_string));
            let show_as = match (ext.as_deref(), d.attr("showDataAs")) {
                (Some("rankDescending"), _) => PivotShowAs::Rank,
                (Some(other), _) => {
                    cx.warn(format!("PivotTable show-values-as {other} is not supported"));
                    PivotShowAs::Normal
                }
                (None, None | Some("normal")) => PivotShowAs::Normal,
                (None, Some("percentOfTotal")) => PivotShowAs::PercentOfGrandTotal,
                (None, Some("percentOfCol")) => PivotShowAs::PercentOfColumnTotal,
                (None, Some("percentOfRow")) => PivotShowAs::PercentOfRowTotal,
                (None, Some("runTotal")) => PivotShowAs::RunningTotal,
                (None, Some(other)) => {
                    cx.warn(format!("PivotTable show-values-as {other} is not supported"));
                    PivotShowAs::Normal
                }
            };
            let number_format = d
                .attr_u32("numFmtId")
                .filter(|id| *id != 0)
                .and_then(|id| crate::tables::builtin_format(id).map(str::to_string).or_else(|| cx.num_fmts.get(&id).cloned()));
            let name = d.attr("name").map(str::to_string).unwrap_or_else(|| format!("{} of {}", func_caption(func), f.name));
            if let Some(last) = data_value.last_mut() {
                *last = Some(pt.values.len());
            }
            pt.values.push(PivotValue { source_col: f.name.clone(), func, name, show_as, number_format });
        }
    }
    // Sort scopes: each reference must name an item of a field on the other axis.
    let n_rows = pt.rows.len();
    let keys: Vec<(String, PivotDateGroup)> = pt.rows.iter().chain(pt.columns.iter()).map(|f| (f.source_col.clone(), f.date_group)).collect();
    let item_label = |j: usize, x: usize| -> Option<String> {
        let k = items_of(pf(j)?).get(x)?.attr_u32("x")?;
        cache.fields.get(j)?.labels().get(k as usize).cloned()
    };
    let mut sorts: Vec<Option<(usize, Vec<PivotScopeItem>)>> = Vec::with_capacity(sort_by.len());
    for (p, by) in sort_by.into_iter().enumerate() {
        let others = if p < n_rows { n_rows..keys.len() } else { 0..n_rows };
        let resolved = by.map(|by| {
            let (d, refs) = by?;
            let scope = refs
                .into_iter()
                .map(|(j, x)| {
                    let k = others.clone().find(|k| axis_ix.get(*k) == Some(&j)).ok_or(())?;
                    let (source_col, date_group) = keys.get(k).cloned().ok_or(())?;
                    Ok(PivotScopeItem { source_col, date_group, item: item_label(j, x).ok_or(())? })
                })
                .collect::<Result<Vec<_>, ()>>()?;
            Ok((d, scope))
        });
        if matches!(resolved, Some(Err(()))) {
            cx.warn(format!(
                "PivotTable \"{}\": a sort by value of \"{}\" has a scope that is not supported; it sorts by the label",
                pt.name,
                keys.get(p).map(|k| k.0.as_str()).unwrap_or("")
            ));
        }
        sorts.push(resolved.and_then(Result::ok));
    }
    let values = &pt.values;
    for (f, by) in pt.rows.iter_mut().chain(pt.columns.iter_mut()).zip(sorts) {
        let Some((d, scope)) = by else { continue };
        f.sort_by = data_value.get(d).copied().flatten().and_then(|v| values.get(v)).map(|v| v.name.clone());
        if f.sort_by.is_some() {
            f.sort_scope = scope;
        }
    }

    // Calculated fields of the cache (each PivotTable on it can use them).
    for f in &cache.fields {
        if let Some(formula) = &f.formula
            && !f.name.trim().is_empty()
            && !pt.calculated_fields.iter().any(|c| same(&c.name, &f.name))
        {
            let formula = formula.trim();
            let formula = if formula.starts_with('=') { formula.to_string() } else { format!("={formula}") };
            pt.calculated_fields.push(PivotCalcField { name: f.name.clone(), formula });
        }
    }

    pt.anchor = loc.start;
    let above = if pages.is_empty() { 0 } else { pages.len() as u32 + 1 };
    pt.last_range = Some(RangeRef::new(CellRef::new(loc.start.row.saturating_sub(above), loc.start.col), loc.end));
    Some(pt)
}
