use super::{
    BASE_ITEM_NEXT, CellRef, Ctx, DateSystem, El, HashMap, IoError, MAX_FIELDS, PivotBaseItem, PivotCalcField, PivotDateGroup, PivotField,
    PivotFilter, PivotFunc, PivotLayout, PivotShowAs, PivotSort, PivotTable, PivotValue, PivotValueFilter, PivotValueRule, RangeRef, abs_range,
    func_caption, func_from, number_to_text, quote_sheet, same, show_as_from_xml, value_filter_from_xml,
};

// ================================================================ reading

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
        "d" => crate::sheet_read::parse_iso_datetime(v, sys).map(number_to_text).unwrap_or_else(|| v.to_string()),
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
/// The data field a pivot field's items are sorted by (`autoSortScope` referring to the data
/// field, `-2`, written unsigned or signed).
fn auto_sort_data_field(e: &El) -> Option<usize> {
    let references = e.path(&["autoSortScope", "pivotArea", "references"])?;
    let data = references.kids("reference").find(|r| matches!(r.attr("field"), Some("4294967294" | "-2")))?;
    data.child("x")?.attr_u32("v").map(|v| v as usize)
}

/// Most labels kept from a manual item order.
const MAX_CUSTOM_ORDER: usize = 100_000;

/// A field's manual item order: its `<item>`s' labels when they aren't in the cache's order (then
/// the report shows the cache's order anyway).
fn custom_order(items: &[&El], labels: &[String]) -> Vec<String> {
    let xs: Vec<u32> = items.iter().filter_map(|it| it.attr_u32("x")).take(MAX_CUSTOM_ORDER).collect();
    if xs.windows(2).all(|w| w.first() < w.get(1)) {
        return vec![];
    }
    xs.iter().filter_map(|x| labels.get(*x as usize).cloned()).collect()
}

fn items_of(e: &El) -> Vec<&El> {
    e.child("items").map(|it| it.kids("item").filter(|i| i.attr("t").is_none_or(|t| t == "data")).collect()).unwrap_or_default()
}

/// The source column a cache field stands for (a date group field's base field), without the
/// warnings of [`field_ref`].
fn axis_source_name(cache: &CacheIn, i: usize) -> Option<String> {
    let f = cache.fields.get(i)?;
    match &f.group {
        Some((Some(base), _, _)) if !f.database => cache.fields.get(*base).map(|b| b.name.clone()),
        _ => Some(f.name.clone()),
    }
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

/// Reads one PivotTable definition into the model, against its cache; `None` (with a warning)
/// when the table can't be used, such as one without a valid location.
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
        pt.subtotals = f.flag("defaultSubtotal", true);
        pt.subtotals_top = f.flag("subtotalTop", true);
    }
    pt.grand_totals_rows = x.flag("colGrandTotals", true);
    pt.grand_totals_cols = x.flag("rowGrandTotals", true);
    pt.show_headers = x.flag("showHeaders", true);
    pt.enable_show_details = x.flag("enableDrill", true);
    pt.style = x.child("pivotTableStyleInfo").and_then(|s| s.attr("name")).unwrap_or("").to_string();
    if x.child("calculatedItems").is_some() {
        cx.warn(format!("PivotTable \"{}\": calculated items are not supported", pt.name));
    }
    // (cache field, data field, rule) of each value filter; other filter kinds are dropped.
    let mut value_filters: Vec<(usize, usize, PivotValueRule)> = Vec::new();
    for f in x.child("filters").into_iter().flat_map(|fs| fs.kids("filter")) {
        let rule = value_filter_from_xml(f);
        match (rule, f.attr_u32("fld")) {
            (Some(rule), Some(fld)) => value_filters.push((fld as usize, f.attr_u32("iMeasureFld").unwrap_or(0) as usize, rule)),
            _ => cx.warn(format!("PivotTable \"{}\": {} filters are not supported", pt.name, f.attr("type").unwrap_or("label"))),
        }
    }

    let mut filters: Vec<PivotFilter> = Vec::new();

    // (fields, position of Σ Values (`-2`) among them)
    // (index in its axis, data field index) of each field sorted by a value field.
    let axis =
        |cx: &mut Ctx<'_>, list: &[i64], filters: &mut Vec<PivotFilter>, sort_refs: &mut Vec<(usize, usize)>| -> (Vec<PivotField>, Option<usize>) {
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
                let mut f = PivotField { source_col: name.clone(), date_group: group, ..PivotField::default() };
                if let Some(e) = pf(i) {
                    f.sort = match e.attr("sortType") {
                        Some("ascending") => PivotSort::Asc,
                        Some("descending") => PivotSort::Desc,
                        _ => PivotSort::None,
                    };
                    if f.sort != PivotSort::None
                        && let Some(k) = auto_sort_data_field(e)
                    {
                        sort_refs.push((v.len(), k));
                    }
                    let items = items_of(e);
                    let lab = |it: &El| it.attr_u32("x").and_then(|k| labels.get(k as usize)).cloned();
                    if f.sort == PivotSort::None {
                        f.custom_order = custom_order(&items, &labels);
                    }
                    f.collapsed_items = items.iter().filter(|it| !it.flag("sd", true)).filter_map(|it| lab(it)).collect();
                    if items.iter().any(|it| it.flag("h", false)) {
                        let shown: Vec<String> = items.iter().filter(|it| !it.flag("h", false)).filter_map(|it| lab(it)).collect();
                        if !filters.iter().any(|ff| same(&ff.source_col, &name)) {
                            filters.push(PivotFilter { source_col: name.clone(), selected: Some(shown), value: None });
                        }
                    }
                }
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
        page_filters.push(PivotFilter { source_col: name, selected, value: None });
    }
    let mut axis_filters = Vec::new();
    let (mut row_sorts, mut col_sorts) = (Vec::new(), Vec::new());
    let (rows, rows_values) = axis(cx, &row_x, &mut axis_filters, &mut row_sorts);
    let (columns, cols_values) = axis(cx, &col_x, &mut axis_filters, &mut col_sorts);
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
    let mut data_field_names: Vec<(usize, String)> = Vec::new();
    if let Some(df) = x.child("dataFields") {
        for (k, d) in df.kids("dataField").enumerate() {
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
            let show_as = show_as_from_xml(d.attr("showDataAs"), ext.as_deref()).unwrap_or_else(|other| {
                cx.warn(format!("PivotTable show-values-as {other} is not supported"));
                PivotShowAs::Normal
            });
            // The base field is one of the report's row or column fields (a date-grouped one is
            // saved as its group field); with anything else the default base field applies, and
            // an item index of the file's field means nothing.
            let base = d
                .attr_u32("baseField")
                .map(|i| i as usize)
                .filter(|_| show_as.uses_base_field())
                .and_then(|i| Some((i, axis_source_name(cache, i)?)))
                .filter(|(_, name)| pt.rows.iter().chain(pt.columns.iter()).any(|f| same(&f.source_col, name)));
            let base_item = match (&base, d.attr_u32("baseItem")) {
                _ if !show_as.uses_base_item() => PivotBaseItem::default(),
                (_, Some(BASE_ITEM_NEXT)) => PivotBaseItem::Next,
                (Some((i, _)), Some(k)) => {
                    cache.fields.get(*i).and_then(|f| f.labels().get(k as usize)).cloned().map_or(PivotBaseItem::Previous, PivotBaseItem::Named)
                }
                _ => PivotBaseItem::Previous,
            };
            // A file always names a base field; the default one (the first row field, else the
            // first column field) reads back as unset, so the report keeps following its first field.
            // A named base item keeps its field: the label belongs to that field, wherever it moves.
            let default_base = pt.rows.first().or(pt.columns.first()).map(|f| f.source_col.as_str());
            let named = matches!(base_item, PivotBaseItem::Named(_));
            let base_field = base.map(|(_, name)| name).filter(|name| named || !default_base.is_some_and(|d| same(d, name)));
            let number_format = d
                .attr_u32("numFmtId")
                .filter(|id| *id != 0)
                .and_then(|id| crate::tables::builtin_format(id).map(str::to_string).or_else(|| cx.num_fmts.get(&id).cloned()));
            let name = d.attr("name").map(str::to_string).unwrap_or_else(|| format!("{} of {}", func_caption(func), f.name));
            pt.values.push(PivotValue { source_col: f.name.clone(), func, name, show_as, base_field, base_item, number_format });
            data_field_names.push((k, pt.values.last().map(|v| v.name.clone()).unwrap_or_default()));
        }
    }

    // Value filters, on the row or column field their cache field stands for.
    for (fld, measure, rule) in value_filters {
        let (Some(name), Some(by)) = (axis_source_name(cache, fld), data_field_names.iter().find(|(j, _)| *j == measure).map(|(_, n)| n.clone()))
        else {
            continue;
        };
        if !pt.rows.iter().chain(pt.columns.iter()).any(|f| same(&f.source_col, &name)) {
            continue;
        }
        let value = Some(PivotValueFilter { by, rule });
        match pt.filters.iter_mut().find(|f| same(&f.source_col, &name)) {
            Some(f) => f.value = value,
            None => pt.filters.push(PivotFilter { source_col: name, selected: None, value }),
        }
    }

    // Fields sorted by a value field name it by its position among the data fields.
    let name_of = |k: usize| data_field_names.iter().find(|(j, _)| *j == k).map(|(_, n)| n.clone());
    for (fields, sorts) in [(&mut pt.rows, &row_sorts), (&mut pt.columns, &col_sorts)] {
        for (position, k) in sorts {
            if let Some(f) = fields.get_mut(*position) {
                f.sort_by = name_of(*k);
            }
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
