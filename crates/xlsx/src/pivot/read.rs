use super::{
    CUSTOM_SUBTOTALS, CellRef, Ctx, DateSystem, EXT_GC_DATA_FIELD, EXT_X14_FIELD, EXT_X14_TABLE, EXT_XPDL_TABLE, El, HashMap, IoError, MAX_FIELDS,
    PivotCalcField, PivotDateGroup, PivotField, PivotFilter, PivotFunc, PivotLayout, PivotScopeItem, PivotShowAs, PivotSort, PivotTable, PivotValue,
    RangeRef, VALUES_FIELD, abs_range, func_caption, func_from, number_to_text, quote_sheet, serial_from_iso_ymd,
};
use gridcraft_core::{CellError, Value};
use gridcraft_model::{PivotDataCell, PivotItemRef, pivot_key};
use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
struct FieldGroupIn {
    base: Option<usize>,
    by: Option<PivotDateGroup>,
    items: Vec<String>,
    values: Vec<Value>,
}
/// A cache field as read.
#[derive(Clone, Debug, Default)]
struct FieldIn {
    name: String,
    database: bool,
    /// Optional `cacheField/@numFmtId` (0 or absent = General).
    num_fmt_id: Option<u32>,
    /// True when `<fieldGroup>` has `<rangePr .../>` (numeric or date/time range grouping).
    has_range_pr: bool,
    /// Shared item captions.
    items: Vec<String>,
    /// Shared item typed values, parallel to `items`.
    item_values: Vec<Value>,
    /// Field grouping (`by == None` means unsupported grouping).
    group: Option<FieldGroupIn>,
    /// A calculated field's formula.
    formula: Option<String>,
}

impl FieldIn {
    fn labels(&self) -> &[String] {
        match &self.group {
            Some(g) if !g.items.is_empty() => &g.items,
            _ => &self.items,
        }
    }

    fn values(&self) -> &[Value] {
        match &self.group {
            Some(g) if !g.items.is_empty() => &g.values,
            _ => &self.item_values,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct CacheIn {
    source: String,
    fields: Vec<FieldIn>,
    /// Set when the PivotTables on this cache are kept as they are, not modelled.
    kept: Option<KeptSource>,
}

/// Where the data of a PivotTable GridCraft keeps unchanged (doesn't model) comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeptSource {
    /// A range or name in another workbook.
    OtherWorkbook,
    /// A data connection of the workbook (`xl/connections.xml`) other than its Data Model.
    Connection,
    /// The workbook's Data Model (Power Pivot), through its model connection.
    DataModel,
}

impl KeptSource {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            KeptSource::OtherWorkbook => "another workbook",
            KeptSource::Connection => "a data connection",
            KeptSource::DataModel => "the workbook's Data Model",
        }
    }
}

/// A PivotTable GridCraft doesn't model (its data is in another workbook, a data connection or
/// the Data Model), kept unchanged for saving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalPivot {
    /// The id of the sheet's relationship to the PivotTable part.
    pub rel_id: String,
    /// The pivot cache definition part it uses.
    pub cache_part: String,
    pub source: KeptSource,
}

/// The PivotTables of a sheet: those modelled, and those kept as they are.
#[derive(Debug, Default)]
pub struct SheetPivots {
    pub tables: Vec<PivotTable>,
    pub external: Vec<ExternalPivot>,
}

/// Workbook-level pivot cache lookup, filled from `<pivotCaches>`; caches are parsed once.
#[derive(Default)]
pub struct CachesIn {
    by_id: HashMap<u32, String>,
    parsed: HashMap<String, Option<CacheIn>>,
    next_id: u32,
    /// The workbook's data connections (`connectionId` → whether it is the Data Model's).
    connections: HashMap<u32, bool>,
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
        CachesIn { by_id, parsed: HashMap::new(), next_id: 1, connections: HashMap::new() }
    }

    /// Reads the ids of the workbook's data connections (`xl/connections.xml`), which caches on a
    /// connection or the Data Model name by `connectionId`.
    pub fn read_connections(&mut self, cx: &mut Ctx<'_>, wb_rels: &[crate::package::Rel]) {
        let Some(r) = wb_rels.iter().find(|r| r.kind == "connections" && !r.external) else { return };
        let Ok(Some(x)) = cx.optional_xml(&r.target) else { return };
        for c in x.kids("connection").take(MAX_CONNECTIONS) {
            let Some(id) = c.attr_u32("id") else { continue };
            // The model connection: `<x15:connection model="1">` in its extension list (Excel
            // also gives it `<dbPr command="Model">`).
            let x15_model = c.child("extLst").is_some_and(|l| l.kids("ext").filter_map(|e| e.child("connection")).any(|x| x.flag("model", false)));
            let dbpr_model = c.attr("type") == Some("5") && c.child("dbPr").and_then(|d| d.attr("command")) == Some("Model");
            self.connections.insert(id, x15_model || dbpr_model);
        }
    }
}

/// Most data connections read from `xl/connections.xml`.
const MAX_CONNECTIONS: usize = 100_000;

fn item_label_val_el(e: &El, sys: DateSystem) -> (String, Value) {
    let v = e.attr("v").unwrap_or("");
    match e.name.as_str() {
        "m" => ("(blank)".into(), Value::Empty),
        "n" => match v.trim().parse::<f64>().ok().filter(|n| n.is_finite()) {
            Some(n) => (number_to_text(n), Value::Number(n)),
            None => (v.to_string(), Value::text(v)),
        },
        "b" => {
            let b = matches!(v.trim(), "1" | "true");
            ((if b { "TRUE" } else { "FALSE" }).into(), Value::Bool(b))
        }
        // Cache dates count real days from the epoch, as Excel writes them (see `iso`).
        "d" => match crate::sheet_read::parse_iso_datetime_with(v, |y, m, d| serial_from_iso_ymd(sys, y, m, d)) {
            Some(s) => (number_to_text(s), Value::Number(s)),
            None => (v.to_string(), Value::text(v)),
        },
        "e" => {
            let val = CellError::parse(v).map(Value::Error).unwrap_or_else(|| Value::text(v));
            (v.to_string(), val)
        }
        _ => (v.to_string(), Value::text(v)),
    }
}

fn read_cache(cx: &mut Ctx<'_>, part: &str, sheet_name: &str, connections: &HashMap<u32, bool>) -> Option<CacheIn> {
    // A cache that can't be read loses its PivotTable: say so instead of dropping it silently.
    // An oversized cache is refused before it inflates, so warning and opening the rest is safe.
    let x = match cx.optional_xml(part) {
        Ok(Some(x)) => x,
        Ok(None) => {
            cx.warn(format!("a PivotTable's cache {part} is missing or could not be read; kept as plain cells"));
            return None;
        }
        Err(IoError::TooLarge(m)) => {
            cx.warn(format!("a PivotTable's cache is too large to read ({m}); kept as plain cells"));
            return None;
        }
        Err(e) => {
            cx.warn(format!("a PivotTable's cache could not be read ({e}); kept as plain cells"));
            return None;
        }
    };
    if x.name != "pivotCacheDefinition" {
        cx.warn(format!("{part} is not a pivot cache definition; its PivotTable was kept as plain cells"));
        return None;
    }
    let src = x.child("cacheSource");
    let ty = src.and_then(|s| s.attr("type")).unwrap_or("worksheet");
    // A data connection or the Data Model (OLAP): kept as it is when the workbook has that
    // connection.
    if ty == "external" {
        let model = src.and_then(|s| s.attr_u32("connectionId")).and_then(|id| connections.get(&id).copied());
        let Some(model) = model else {
            cx.warn("a PivotTable's data connection is not in the workbook; kept as plain cells");
            return None;
        };
        let kept = if model { KeptSource::DataModel } else { KeptSource::Connection };
        return Some(CacheIn { source: String::new(), fields: vec![], kept: Some(kept) });
    }
    if ty != "worksheet" {
        cx.warn(format!("PivotTables with a data source of type \"{ty}\" are not supported; kept as plain cells"));
        return None;
    }
    let ws = src.and_then(|s| s.child("worksheetSource"));
    // A source in another workbook: `r:id` names the cache's link to that workbook's path.
    if let Some(rid) = ws.and_then(|w| w.attr("id")) {
        let linked = cx.pkg.rels(part).ok().is_some_and(|rels| rels.iter().any(|r| r.id == rid && r.external && r.kind == "externalLinkPath"));
        if !linked {
            cx.warn("a PivotTable's source is in another workbook that its cache doesn't name; kept as plain cells");
            return None;
        }
        return Some(CacheIn { source: String::new(), fields: vec![], kept: Some(KeptSource::OtherWorkbook) });
    }
    let source = match ws {
        Some(w) if w.attr("name").is_some_and(|n| !n.trim().is_empty()) => w.attr("name").unwrap_or("").trim().to_string(),
        Some(w) => {
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
    let mut truncated = false;
    if let Some(cf) = x.child("cacheFields") {
        for f in capped(cf.kids("cacheField"), &mut truncated) {
            let (items, item_values): (Vec<String>, Vec<Value>) =
                f.child("sharedItems").map(|si| si.children.iter().map(|e| item_label_val_el(e, sys)).unzip()).unwrap_or_default();
            let mut has_range_pr = false;
            let group = f.child("fieldGroup").map(|g| {
                let base = g.attr_u32("base").map(|b| b as usize);
                let range_pr = g.child("rangePr");
                has_range_pr = range_pr.is_some();
                let by = match range_pr.map(|r| r.attr("groupBy").unwrap_or("range")) {
                    Some("years") => Some(PivotDateGroup::Years),
                    Some("quarters") => Some(PivotDateGroup::Quarters),
                    Some("months") => Some(PivotDateGroup::Months),
                    Some("days") => Some(PivotDateGroup::Days),
                    _ => None,
                };
                let (items, values): (Vec<String>, Vec<Value>) =
                    g.child("groupItems").map(|gi| gi.children.iter().map(|e| item_label_val_el(e, sys)).unzip()).unwrap_or_default();
                FieldGroupIn { base, by, items, values }
            });
            fields.push(FieldIn {
                name: f.attr("name").unwrap_or("").to_string(),
                database: f.flag("databaseField", true),
                num_fmt_id: f.attr_u32("numFmtId"),
                has_range_pr,
                items,
                item_values,
                group,
                formula: f.attr("formula").map(str::to_string),
            });
        }
    }
    if truncated {
        cx.warn(format!("{part}: cache fields past {MAX_FIELDS} were dropped; its PivotTables may be incomplete"));
    }
    if fields.is_empty() {
        cx.warn("a PivotTable's cache has no fields; kept as plain cells");
        return None;
    }
    Some(CacheIn { source, fields, kept: None })
}

/// Reads the PivotTables related to a worksheet part. Those whose source is in another workbook,
/// a data connection or the Data Model are listed in [`SheetPivots::external`] (with a warning),
/// not modelled.
pub fn read_pivots(cx: &mut Ctx<'_>, caches: &mut CachesIn, sheet_part: &str, sheet_name: &str) -> Result<SheetPivots, IoError> {
    let rels = cx.pkg.rels(sheet_part)?;
    let mut out = SheetPivots::default();
    let mut seen_targets: HashSet<String> = HashSet::new();
    for rel in rels.iter().filter(|r| r.kind == "pivotTable" && !r.external) {
        if !seen_targets.insert(rel.target.to_ascii_lowercase()) {
            continue;
        }
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
                let c = read_cache(cx, &cache_part, sheet_name, &caches.connections);
                caches.parsed.insert(cache_part.clone(), c.clone());
                c
            }
        };
        let Some(cache) = cache else { continue };
        if let Some(source) = cache.kept {
            let name = x.attr("name").unwrap_or("");
            let from = source.describe();
            cx.warn(format!(
                "PivotTable \"{name}\" on sheet \"{sheet_name}\" gets its data from {from}: it is kept for saving as it is, but can't be edited or refreshed here"
            ));
            out.external.push(ExternalPivot { rel_id: rel.id.clone(), cache_part, source });
            continue;
        }
        if let Some(pt) = read_table(cx, &x, &cache, caches.next_id) {
            caches.next_id = caches.next_id.saturating_add(1);
            out.tables.push(pt);
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
        Some(g) => {
            let by = match g.by {
                Some(b) => b,
                None => {
                    cx.warn(format!("PivotTable grouping of \"{}\" is not supported; the field is ungrouped", f.name));
                    PivotDateGroup::None
                }
            };
            let base_name = if f.database { f.name.clone() } else { g.base.and_then(|b| cache.fields.get(b))?.name.clone() };
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
    // Field lists are capped, and a field is used once (Excel puts a field on one axis, once):
    // each use walks the field's items.
    // A list cut at the cap is reported, so an incomplete PivotTable can be told apart.
    let mut truncated = false;
    let pfs: Vec<&El> = x.child("pivotFields").map(|p| capped(p.kids("pivotField"), &mut truncated)).unwrap_or_default();
    let pf = |i: usize| pfs.get(i).copied();
    let mut used_fields = std::collections::HashSet::new();
    let mut idx_list = |name: &str, attr: &str| -> Vec<i64> {
        x.child(name)
            .map(|e| capped(e.children.iter().filter_map(|f| f.attr_i64(attr)).filter(|&i| i < 0 || used_fields.insert(i)), &mut truncated))
            .unwrap_or_default()
    };
    let row_x = idx_list("rowFields", "x");
    let col_x = idx_list("colFields", "x");
    let mut page_fields = std::collections::HashSet::new();
    let pages: Vec<&El> = x
        .child("pageFields")
        .map(|p| capped(p.kids("pageField").filter(|f| f.attr_i64("fld").is_none_or(|i| page_fields.insert(i))), &mut truncated))
        .unwrap_or_default();

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
    // Lower-cased names of the fields already filtered on an axis, the first of which wins (as
    // `same` compares them): scanning the filters for each field would be quadratic.
    let mut axis_filtered: std::collections::HashSet<String> = std::collections::HashSet::new();
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
            let labels: &[String] = cache.fields.get(i).map(|f| f.labels()).unwrap_or(&[]);
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
                    f.number_format = e.attr_u32("numFmtId").and_then(|id| resolve_num_fmt_id(id, &cx.num_fmts)).map(str::to_string);
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
                if items.iter().any(|it| it.flag("h", false)) && axis_filtered.insert(name.to_lowercase()) {
                    let shown: Vec<String> = items.iter().filter(|it| !it.flag("h", false)).filter_map(|it| lab(it)).collect();
                    filters.push(PivotFilter { source_col: name.clone(), selected: Some(shown) });
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
        let labels: &[String] = cache.fields.get(i).map(|f| f.labels()).unwrap_or(&[]);
        let selected = page_field_selected(p, pf(i), labels);
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
        truncated |= df.kids("dataField").nth(MAX_FIELDS).is_some();
        for d in df.kids("dataField").take(MAX_FIELDS) {
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
            let exts = || d.child("extLst").into_iter().flat_map(|l| l.kids("ext"));
            // GridCraft's marker for a Distinct Count written as Count; ignored when the subtotal
            // was changed to something else since.
            let distinct = exts()
                .filter(|e| e.attr("uri") == Some(EXT_GC_DATA_FIELD))
                .filter_map(|e| e.child("summary"))
                .any(|e| e.attr("func") == Some("distinctCount"));
            let func = if distinct && func == PivotFunc::Count { PivotFunc::DistinctCount } else { func };
            let ext = exts().filter_map(|e| e.child("dataField")).find_map(|e| e.attr("pivotShowAs").map(str::to_string));
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
            let number_format = d.attr_u32("numFmtId").and_then(|id| resolve_num_fmt_id(id, &cx.num_fmts)).map(str::to_string);
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

    // Calculated fields of the cache (each PivotTable on it can use them); the first of a
    // lower-cased name wins, looked up in a set rather than by scanning the fields kept so far.
    let mut calc_names: std::collections::HashSet<String> = pt.calculated_fields.iter().map(|c| c.name.to_lowercase()).collect();
    for f in &cache.fields {
        if let Some(formula) = &f.formula
            && !f.name.trim().is_empty()
            && calc_names.insert(f.name.to_lowercase())
        {
            let formula = formula.trim();
            let formula = if formula.starts_with('=') { formula.to_string() } else { format!("={formula}") };
            pt.calculated_fields.push(PivotCalcField { name: f.name.clone(), formula });
        }
    }

    if truncated {
        cx.warn(format!("PivotTable \"{}\": field lists past {MAX_FIELDS} entries were dropped; the table is incomplete", pt.name));
    }
    pt.anchor = loc.start;
    let loc_el = x.child("location");
    let page_rows = loc_el.and_then(|l| l.attr_u32("rowPageCount")).unwrap_or(pages.len() as u32);
    let above = if page_rows == 0 { 0 } else { page_rows.saturating_add(1) };
    let report_start_row = loc.start.row.saturating_sub(above);
    pt.last_range = Some(RangeRef::new(CellRef::new(report_start_row, loc.start.col), loc.end));
    if let Some(cells) = build_ooxml_data_index(cx, &pt.name, x, loc, loc_el, report_start_row, &row_x, &col_x, &pfs, cache, &pt.values, &data_value)
    {
        pt.data_index.set_cells(cells);
    }
    Some(pt)
}

/// Resolves a `<pageField>`'s selected item(s). MS-OE376 §2.1.806: `pageField/@item` indexes
/// `<pivotField><items>`, whose `@x` indexes the cache field's items (falling back to direct
/// cache indexing only when the `<pivotField>` carries no `<items>`).
fn page_field_selected(p: &El, pf: Option<&El>, labels: &[String]) -> Option<Vec<String>> {
    if let Some(k) = p.attr_u32("item") {
        let resolved = match pf.and_then(|e| e.child("items")) {
            Some(its) => its.kids("item").nth(k as usize).and_then(|it| it.attr_u32("x")).and_then(|sx| labels.get(sx as usize)),
            None => labels.get(k as usize),
        };
        return resolved.map(|l| vec![l.clone()]);
    }
    pf.and_then(|e| {
        let items = items_of(e);
        items.iter().any(|it| it.flag("h", false)).then(|| {
            items.iter().filter(|it| !it.flag("h", false)).filter_map(|it| it.attr_u32("x").and_then(|k| labels.get(k as usize)).cloned()).collect()
        })
    })
}

/// Resolves a non-zero OOXML `numFmtId` against the workbook's custom `<numFmts>` table and
/// built-in format table (`styles.rs` precedence: custom overrides built-in).
fn resolve_num_fmt_id(id: u32, num_fmts: &HashMap<u32, String>) -> Option<&str> {
    if id == 0 {
        return None;
    }
    num_fmts.get(&id).map(String::as_str).or_else(|| crate::tables::builtin_format(id))
}

/// Cap on cells indexed (and total row × col iterations) from OOXML `<rowItems>`/`<colItems>`
/// (matches `engine::pivot::MAX_INDEXED_CELLS`).
const MAX_INDEXED_CELLS: usize = 1_000_000;
/// Cap on combined real axis fields (`rowFields` + `colFields` with `x >= 0`, excluding the `-2`
/// Values placeholder, matching `engine::pivot::MAX_AXIS_FIELDS`) when decoding OOXML axis items.
const MAX_AXIS_FIELDS: usize = 32;
/// Workbook-wide `64 MiB` retained byte budget (`Ctx::pivot_index_retained_bytes`) for kept
/// `PivotDataIndex` cells & items across all PivotTables (rolled back when a table bails out).
const MAX_RETAINED_INDEX_BYTES: usize = 64 * 1024 * 1024;
/// Per-table `128 MiB` transient work byte budget for a single PivotTable's OOXML `data_index`
/// build (freed and reset per table so a tall or failed pivot does not consume other tables'
/// per-table work budgets (unless the workbook-wide iteration or work limits are reached)).
const MAX_TABLE_WORK_BYTES: usize = 128 * 1024 * 1024;
/// Workbook-wide `2 GiB` cumulative transient work byte ceiling
/// (`Ctx::pivot_index_workbook_work_bytes`) across all PivotTables' OOXML `data_index` builds
/// (never rolled back on failed tables), bounding transient allocation churn across many sheets.
const MAX_WORKBOOK_WORK_BYTES: usize = 2 * 1024 * 1024 * 1024;
/// Workbook-wide cap on cumulative decoded axis `<i>` entries and row × col cell-loop iterations
/// (`Ctx::pivot_index_iterations`) across all PivotTables' OOXML `data_index` builds, bounding
/// CPU even across many oversized tables.
const MAX_WORKBOOK_INDEX_ITERATIONS: usize = 16_000_000;
/// OOXML sentinel item index (`0x0010_0100`, MS-OI29500) written for missing/special pivot items
/// instead of a real cache item index.
const OOXML_SENTINEL_ITEM: usize = 1_048_832;

enum IndexBail {
    /// Missing, unsupported, or malformed OOXML layout; silently fall back to the lazy index.
    Unsupported,
    /// Exceeded a retained/work byte budget or cell/iteration cap; emit a warning and fall back.
    Budget(&'static str),
}

fn charge_work(table_work_bytes: &mut usize, workbook_work_bytes: &mut usize, add: usize) -> Result<(), IndexBail> {
    let next_table = match table_work_bytes.checked_add(add) {
        Some(next) if next <= MAX_TABLE_WORK_BYTES => next,
        _ => return Err(IndexBail::Budget("table work budget exceeded")),
    };
    let next_wb = match workbook_work_bytes.checked_add(add) {
        Some(next) if next <= MAX_WORKBOOK_WORK_BYTES => next,
        _ => {
            *workbook_work_bytes = MAX_WORKBOOK_WORK_BYTES;
            return Err(IndexBail::Budget("workbook work limit reached"));
        }
    };
    *table_work_bytes = next_table;
    *workbook_work_bytes = next_wb;
    Ok(())
}

fn charge_retained(retained_before: usize, table_retained: &mut usize, add: usize) -> bool {
    match retained_before.checked_add(*table_retained).and_then(|cur| cur.checked_add(add)) {
        Some(next) if next <= MAX_RETAINED_INDEX_BYTES => {
            *table_retained += add;
            true
        }
        _ => false,
    }
}

fn charge_workbook_iteration(workbook_iterations: &mut usize) -> Result<(), IndexBail> {
    *workbook_iterations = workbook_iterations.saturating_add(1);
    if *workbook_iterations > MAX_WORKBOOK_INDEX_ITERATIONS {
        return Err(IndexBail::Budget("workbook iteration limit reached"));
    }
    Ok(())
}

fn charge_iteration(table_iterations: &mut usize, workbook_iterations: &mut usize) -> Result<(), IndexBail> {
    *table_iterations += 1;
    *workbook_iterations = workbook_iterations.saturating_add(1);
    if *table_iterations > MAX_INDEXED_CELLS {
        return Err(IndexBail::Budget("cell iteration limit reached"));
    }
    if *workbook_iterations > MAX_WORKBOOK_INDEX_ITERATIONS {
        return Err(IndexBail::Budget("workbook iteration limit reached"));
    }
    Ok(())
}

/// Parses an optional unsigned integer XML attribute strictly: absent -> `Ok(None)`,
/// valid non-negative integer -> `Ok(Some(v))`, malformed/negative -> `Err(())`.
fn strict_opt_u32(el: &El, k: &str) -> Result<Option<u32>, ()> {
    match el.attr(k) {
        None => Ok(None),
        Some(raw) => raw.trim().parse::<u32>().map(Some).map_err(|_| ()),
    }
}
/// One decoded row or column entry from `<rowItems>` / `<colItems>`.
struct AxisEntry {
    /// `(cache_field_index, PivotItemRef)` for each non-Values field on this axis.
    items: Vec<(usize, PivotItemRef)>,
    /// Data field index (into `<dataFields>`) when `Values` (`-2`) is on this axis or `@i` is set.
    data_field_idx: Option<usize>,
    /// True for `t="grand"`, subtotal `t` (`default`, `sum`, ...), or trailing subtotal `<item>`.
    is_subtotal_or_grand: bool,
    /// True when this entry stepped into the `-2` (Values) axis field.
    has_values_field: bool,
    /// Raw `(field_idx, item_idx)` path (including `-2`) before stripping subtotal markers.
    raw_path: Vec<(i64, usize)>,
}

fn is_subtotal_item_type(t: &str) -> bool {
    matches!(t, "default" | "sum" | "countA" | "avg" | "max" | "min" | "product" | "count" | "stdDev" | "stdDevP" | "var" | "varP")
}

/// Decodes `<rowItems>` or `<colItems>` (ECMA-376 §18.10.1.84 / §18.10.1.17) into per-row/column
/// axis item paths. `None` entries in the returned `Vec` correspond to `t="blank"` spacer lines
/// or non-subtotal item-header lines. Returns `None` if any axis item is malformed or cannot be
/// resolved, so callers fall back instead of producing truncated keys.
fn decode_axis_items(
    cx: &mut Ctx<'_>,
    table_work_bytes: &mut usize,
    axis_el: &El,
    field_indices: &[i64],
    pfs: &[&El],
    cache: &CacheIn,
    limit: usize,
) -> Result<Vec<Option<AxisEntry>>, IndexBail> {
    if field_indices.iter().filter(|&&f| f >= 0).count() > MAX_AXIS_FIELDS
        || field_indices.iter().filter(|&&f| f == -2).count() > 1
        || field_indices
            .iter()
            .any(|&f| (f < 0 && f != -2) || (f >= 0 && ((f as usize) == OOXML_SENTINEL_ITEM || (f as usize) >= cache.fields.len())))
    {
        return Err(IndexBail::Unsupported);
    }
    let pf_items: Vec<Option<Vec<&El>>> = pfs.iter().map(|pf| pf.child("items").map(|its| its.kids("item").collect())).collect();
    let mut out = Vec::new();
    let mut prev: Vec<(i64, usize)> = Vec::new();
    for iel in axis_el.kids("i") {
        if out.len() >= limit {
            return Err(IndexBail::Unsupported);
        }
        charge_workbook_iteration(&mut cx.pivot_index_iterations)?;
        if out.len() >= MAX_INDEXED_CELLS {
            return Err(IndexBail::Budget("axis item limit reached"));
        }
        let t = iel.attr("t").unwrap_or("data");
        if t == "blank" {
            charge_work(table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, std::mem::size_of::<Option<AxisEntry>>())?;
            out.push(None);
            continue;
        }
        let r = strict_opt_u32(iel, "r").map_err(|()| IndexBail::Unsupported)?.unwrap_or(0) as usize;
        let mut df_idx = strict_opt_u32(iel, "i").map_err(|()| IndexBail::Unsupported)?.map(|v| v as usize);
        if df_idx == Some(OOXML_SENTINEL_ITEM) {
            return Err(IndexBail::Unsupported);
        }
        let mut xs = Vec::new();
        for x in iel.kids("x") {
            if xs.len() > MAX_AXIS_FIELDS {
                return Err(IndexBail::Unsupported);
            }
            let xv = strict_opt_u32(x, "v").map_err(|()| IndexBail::Unsupported)?.unwrap_or(0) as usize;
            if xv == OOXML_SENTINEL_ITEM {
                return Err(IndexBail::Unsupported);
            }
            xs.push(xv);
        }
        if t == "grand" {
            charge_work(table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, std::mem::size_of::<Option<AxisEntry>>())?;
            let mut has_values_field = false;
            for (j, &xv) in xs.iter().enumerate() {
                let pos = r.checked_add(j).ok_or(IndexBail::Unsupported)?;
                if field_indices.get(pos).copied() == Some(-2) {
                    if df_idx.is_none() {
                        df_idx = Some(xv);
                    }
                    has_values_field = true;
                }
            }
            out.push(Some(AxisEntry {
                items: Vec::new(),
                data_field_idx: df_idx,
                is_subtotal_or_grand: true,
                has_values_field,
                raw_path: Vec::new(),
            }));
            prev.clear();
            continue;
        }
        if r > prev.len() {
            return Err(IndexBail::Unsupported);
        }
        let mut cur: Vec<(i64, usize)> = prev.iter().take(r).copied().collect();
        for (j, &xv) in xs.iter().enumerate() {
            let pos = r.checked_add(j).ok_or(IndexBail::Unsupported)?;
            if let Some(&fidx) = field_indices.get(pos) {
                cur.push((fidx, xv));
            } else if !field_indices.is_empty() {
                return Err(IndexBail::Unsupported);
            }
        }
        let path_bytes =
            cur.len().saturating_mul(std::mem::size_of::<(i64, usize)>()).saturating_mul(2).saturating_add(std::mem::size_of::<Option<AxisEntry>>());
        charge_work(table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, path_bytes)?;
        prev = cur.clone();
        let mut is_subtotal_or_grand = is_subtotal_item_type(t);
        let mut has_values_field = false;
        let mut items = Vec::with_capacity(cur.len());
        for &(fidx, pitem_idx) in &cur {
            if fidx == -2 {
                df_idx = Some(pitem_idx);
                has_values_field = true;
                continue;
            }
            if fidx < 0 {
                continue;
            }
            let fi = fidx as usize;
            let cf = cache.fields.get(fi).ok_or(IndexBail::Unsupported)?;
            let pitems = pf_items.get(fi).ok_or(IndexBail::Unsupported)?.as_deref().ok_or(IndexBail::Unsupported)?;
            if pitem_idx == OOXML_SENTINEL_ITEM {
                return Err(IndexBail::Unsupported);
            }
            let pit = pitems.get(pitem_idx).ok_or(IndexBail::Unsupported)?;
            if let Some(it_t) = pit.attr("t").filter(|&it_t| it_t != "data") {
                if is_subtotal_item_type(it_t) || it_t == "grand" {
                    // Trailing subtotal/total marker item (e.g. `t="default"`): omitted from item path.
                    is_subtotal_or_grand = true;
                    continue;
                }
                return Err(IndexBail::Unsupported);
            }
            let sx = strict_opt_u32(pit, "x").map_err(|()| IndexBail::Unsupported)?.ok_or(IndexBail::Unsupported)? as usize;
            if sx == OOXML_SENTINEL_ITEM {
                return Err(IndexBail::Unsupported);
            }
            let raw_label = cf.labels().get(sx).ok_or(IndexBail::Unsupported)?;
            let item_header_bytes = std::mem::size_of::<(usize, PivotItemRef)>().saturating_add(cf.name.len());
            charge_work(table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, item_header_bytes)?;
            let value = cf.values().get(sx).cloned().unwrap_or_else(|| Value::text(raw_label.as_str()));
            // Format numeric (`<n>`) and date (`<d>`) cache items using the field's display number
            // format (`pivotField/@numFmtId`, falling back to `cacheField/@numFmtId`), matching
            // `engine::pivot::items::make_item`, while keeping raw `Value::Number(serial)` on `value`.
            let label = if let (Value::Number(_), Some(fmt_fn)) = (&value, cx.format_value) {
                let fmt_code = pfs
                    .get(fi)
                    .and_then(|pf| pf.attr_u32("numFmtId"))
                    .and_then(|id| resolve_num_fmt_id(id, &cx.num_fmts))
                    .or_else(|| cf.num_fmt_id.and_then(|id| resolve_num_fmt_id(id, &cx.num_fmts)))
                    .unwrap_or("General");
                let formatted = fmt_fn(&value, fmt_code, cx.date_system);
                charge_work(table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, formatted.len())?;
                formatted
            } else {
                charge_work(table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, raw_label.len())?;
                raw_label.clone()
            };
            // Use the canonical cache/source field name and formatted cache item label (matching
            // `compute.rs::item_refs`), not `<pivotField name>` or `<item n>` custom captions, so
            // GETPIVOTDATA works identically before and after refresh.
            items.push((fi, PivotItemRef { field: cf.name.clone(), label, value }));
        }
        out.push(Some(AxisEntry { items, data_field_idx: df_idx, is_subtotal_or_grand, has_values_field, raw_path: cur }));
    }

    drop_blank_item_headers(&mut out, field_indices, pfs);
    Ok(out)
}

/// Filter out non-subtotal item-header rows/columns (compact/outline layout headers when
/// subtotals are at the bottom or disabled, or outer item headers before stepping into `-2`
/// Values on the same axis) so blank header lines never shadow real data/subtotal rows.
fn drop_blank_item_headers(out: &mut [Option<AxisEntry>], field_indices: &[i64], pfs: &[&El]) {
    let axis_has_values = field_indices.contains(&-2);
    let mut drop_header = vec![false; out.len()];
    for idx in 0..out.len() {
        let Some(cur) = out[idx].as_ref() else { continue };
        if cur.is_subtotal_or_grand {
            continue;
        }
        let next = out[idx + 1..].iter().flatten().next();
        let is_item_header = next.is_some_and(|n| {
            (cur.raw_path.len() < n.raw_path.len() && n.raw_path.starts_with(&cur.raw_path))
                || (axis_has_values
                    && !cur.has_values_field
                    && n.items.len() >= cur.items.len()
                    && n.items[..cur.items.len()] == cur.items[..]
                    && (n.has_values_field || n.is_subtotal_or_grand))
        });
        if !is_item_header {
            continue;
        }
        if axis_has_values && !cur.has_values_field {
            drop_header[idx] = true;
            continue;
        }
        let last_fidx = cur.raw_path.last().map(|(f, _)| *f);
        match last_fidx {
            Some(fi) if fi >= 0 => {
                let pf = pfs.get(fi as usize).copied();
                let subtotals_on = pf.is_none_or(field_subtotals);
                let subtotal_top = pf.is_none_or(|e| e.attr_bool("subtotalTop").unwrap_or(true));
                if !subtotals_on || !subtotal_top {
                    drop_header[idx] = true;
                }
            }
            _ => {
                // `-2` Values header (when Values precedes another axis field) never holds a top subtotal.
                drop_header[idx] = true;
            }
        }
    }
    for (slot, drop) in out.iter_mut().zip(drop_header) {
        if drop {
            *slot = None;
        }
    }
}

/// Builds the `GETPIVOTDATA` lookup index directly from the OOXML `<rowItems>` and `<colItems>`
/// describing the stored PivotTable report. Returns `None` when the XML carries no stored layout
/// or cannot be decoded cleanly, letting the lazy engine indexer run.
#[allow(clippy::too_many_arguments)]
fn build_ooxml_data_index(
    cx: &mut Ctx<'_>,
    pt_name: &str,
    x: &El,
    loc: RangeRef,
    loc_el: Option<&El>,
    report_start_row: u32,
    row_x: &[i64],
    col_x: &[i64],
    pfs: &[&El],
    cache: &CacheIn,
    pt_values: &[PivotValue],
    data_value: &[Option<usize>],
) -> Option<Vec<PivotDataCell>> {
    let retained_before = cx.pivot_index_retained_bytes;
    let mut table_work_bytes: usize = 0;
    match try_build_ooxml_data_index(
        cx,
        retained_before,
        &mut table_work_bytes,
        x,
        loc,
        loc_el,
        report_start_row,
        row_x,
        col_x,
        pfs,
        cache,
        pt_values,
        data_value,
    ) {
        Ok((cells, table_retained)) => {
            cx.pivot_index_retained_bytes = retained_before.saturating_add(table_retained);
            Some(cells)
        }
        Err(IndexBail::Budget(reason)) => {
            cx.pivot_index_retained_bytes = retained_before;
            cx.warn(format!("PivotTable \"{pt_name}\": OOXML GETPIVOTDATA index skipped ({reason}); later lookups fall back to the lazy index"));
            None
        }
        Err(IndexBail::Unsupported) => {
            cx.pivot_index_retained_bytes = retained_before;
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn try_build_ooxml_data_index(
    cx: &mut Ctx<'_>,
    retained_before: usize,
    table_work_bytes: &mut usize,
    x: &El,
    loc: RangeRef,
    loc_el: Option<&El>,
    report_start_row: u32,
    row_x: &[i64],
    col_x: &[i64],
    pfs: &[&El],
    cache: &CacheIn,
    pt_values: &[PivotValue],
    data_value: &[Option<usize>],
) -> Result<(Vec<PivotDataCell>, usize), IndexBail> {
    if pt_values.is_empty() || row_x.iter().chain(col_x).filter(|&&f| f >= 0).count() > MAX_AXIS_FIELDS {
        return Err(IndexBail::Unsupported);
    }
    let row_el = x.child("rowItems");
    let col_el = x.child("colItems");
    if row_el.is_none() && col_el.is_none() {
        return Err(IndexBail::Unsupported);
    }
    if (row_el.is_none() && !row_x.is_empty()) || (col_el.is_none() && !col_x.is_empty()) {
        return Err(IndexBail::Unsupported);
    }
    if cx.pivot_index_iterations >= MAX_WORKBOOK_INDEX_ITERATIONS {
        return Err(IndexBail::Budget("workbook iteration limit reached"));
    }
    if cx.pivot_index_workbook_work_bytes >= MAX_WORKBOOK_WORK_BYTES {
        return Err(IndexBail::Budget("workbook work limit reached"));
    }
    let first_data_row = loc_el.and_then(|l| l.attr_u32("firstDataRow")).unwrap_or(1);
    let first_data_col = loc_el.and_then(|l| l.attr_u32("firstDataCol")).unwrap_or(1);
    let body_rows = loc.end.row.saturating_sub(loc.start.row).saturating_add(1) as usize;
    let body_cols = loc.end.col.saturating_sub(loc.start.col).saturating_add(1) as usize;
    let row_entries = match row_el {
        Some(el) => decode_axis_items(cx, table_work_bytes, el, row_x, pfs, cache, body_rows)?,
        None if !row_x.is_empty() => return Err(IndexBail::Unsupported),
        None => vec![Some(AxisEntry {
            items: Vec::new(),
            data_field_idx: None,
            is_subtotal_or_grand: false,
            has_values_field: false,
            raw_path: Vec::new(),
        })],
    };
    let col_entries = match col_el {
        Some(el) => decode_axis_items(cx, table_work_bytes, el, col_x, pfs, cache, body_cols)?,
        None if !col_x.is_empty() => return Err(IndexBail::Unsupported),
        None => vec![Some(AxisEntry {
            items: Vec::new(),
            data_field_idx: None,
            is_subtotal_or_grand: false,
            has_values_field: false,
            raw_path: Vec::new(),
        })],
    };
    if row_entries.is_empty() || col_entries.is_empty() {
        return Err(IndexBail::Unsupported);
    }
    let data_row_base = loc.start.row.saturating_sub(report_start_row).saturating_add(first_data_row);
    let max_row_off = loc.end.row.saturating_sub(report_start_row);
    let max_col_off = loc.end.col.saturating_sub(loc.start.col);
    let mut cells: Vec<PivotDataCell> = Vec::new();
    let mut table_retained: usize = 0;
    // Maps lookup key -> (index in `cells`, precedence) so explicit subtotal or `-2` value rows
    // win over any earlier header row that produced the same key.
    let mut seen: HashMap<String, (usize, u8)> = HashMap::new();
    let mut record = |cell: PivotDataCell,
                      prio: u8,
                      cell_bytes: usize,
                      work_bytes: &mut usize,
                      workbook_work_bytes: &mut usize,
                      table_retained: &mut usize|
     -> Result<(), IndexBail> {
        let key_items: Vec<(&str, &str, bool)> =
            cell.items.iter().map(|it| (it.field.as_str(), it.label.as_str(), matches!(it.value, Value::Bool(_)))).collect();
        let key = pivot_key(&cell.data_field, &key_items);
        let key_bytes = std::mem::size_of::<(String, (usize, u8))>().saturating_add(key.len());
        charge_work(work_bytes, workbook_work_bytes, key_bytes)?;
        if let Some((old_idx, old_prio)) = seen.get_mut(&key)
            && cells[*old_idx].items == cell.items
        {
            if prio > *old_prio {
                cells[*old_idx] = cell;
                *old_prio = prio;
            }
            return Ok(());
        }
        if cells.len() >= MAX_INDEXED_CELLS {
            return Err(IndexBail::Budget("indexed cell limit reached"));
        }
        if !charge_retained(retained_before, table_retained, cell_bytes) {
            return Err(IndexBail::Budget("workbook retained budget exceeded"));
        }
        let idx = cells.len();
        cells.push(cell);
        seen.insert(key, (idx, prio));
        Ok(())
    };
    let mut iterations: usize = 0;
    for (ri, r_opt) in row_entries.iter().enumerate() {
        let Some(re) = r_opt else {
            charge_iteration(&mut iterations, &mut cx.pivot_index_iterations)?;
            continue;
        };
        let row_off = data_row_base.checked_add(ri as u32).ok_or(IndexBail::Unsupported)?;
        if row_off > max_row_off {
            return Err(IndexBail::Unsupported);
        }
        for (ci, c_opt) in col_entries.iter().enumerate() {
            charge_iteration(&mut iterations, &mut cx.pivot_index_iterations)?;
            let Some(ce) = c_opt else { continue };
            let col_off = first_data_col.checked_add(ci as u32).ok_or(IndexBail::Unsupported)?;
            if col_off > max_col_off {
                return Err(IndexBail::Unsupported);
            }
            let raw_df = re.data_field_idx.or(ce.data_field_idx).unwrap_or(0);
            let Some(slot) = data_value.get(raw_df) else {
                return Err(IndexBail::Unsupported);
            };
            let Some(vi) = *slot else { continue };
            let Some(vdef) = pt_values.get(vi) else {
                return Err(IndexBail::Unsupported);
            };
            let full_item_count = re.items.len().saturating_add(ce.items.len());
            let full_bytes: usize = std::mem::size_of::<PivotDataCell>()
                .saturating_add(full_item_count.saturating_mul(std::mem::size_of::<PivotItemRef>()))
                .saturating_add(vdef.name.len())
                .saturating_add(vdef.source_col.len())
                .saturating_add(re.items.iter().chain(&ce.items).map(|(_, it)| it.field.len().saturating_add(it.label.len())).sum::<usize>());
            if full_bytes > MAX_RETAINED_INDEX_BYTES {
                return Err(IndexBail::Budget("workbook retained budget exceeded"));
            }
            charge_work(table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, full_bytes)?;
            let full_items: Vec<PivotItemRef> = re.items.iter().chain(&ce.items).map(|(_, it)| it.clone()).collect();
            let prio = 1 + (u8::from(re.is_subtotal_or_grand || ce.is_subtotal_or_grand) << 1) + u8::from(re.has_values_field || ce.has_values_field);
            let full_cell =
                PivotDataCell { data_field: vdef.name.clone(), source_col: vdef.source_col.clone(), items: full_items, row: row_off, col: col_off };
            record(full_cell, prio, full_bytes, table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, &mut table_retained)?;
            // If a manual/non-date parent group field (`databaseField="0"`, `fieldGroup/@base = b`,
            // no `rangePr`, and `base != fi`) appears alongside its base field `b`, Excel allows
            // `GETPIVOTDATA` to omit the parent group because each base item uniquely identifies it.
            let is_manual_parent = |fi: usize| {
                cache.fields.get(fi).is_some_and(|cf| {
                    !cf.database
                        && !cf.has_range_pr
                        && cf.group.as_ref().is_some_and(|g| {
                            g.by.is_none()
                                && g.base.is_some_and(|base| base != fi && re.items.iter().chain(&ce.items).any(|(other, _)| *other == base))
                        })
                })
            };
            let has_manual_parent = re.items.iter().chain(&ce.items).any(|(fi, _)| is_manual_parent(*fi));
            if has_manual_parent {
                let mut stripped_item_count = 0usize;
                let mut stripped_str_bytes = 0usize;
                for (fi, it) in re.items.iter().chain(&ce.items) {
                    if !is_manual_parent(*fi) {
                        stripped_item_count += 1;
                        stripped_str_bytes = stripped_str_bytes.saturating_add(it.field.len()).saturating_add(it.label.len());
                    }
                }
                let stripped_bytes: usize = std::mem::size_of::<PivotDataCell>()
                    .saturating_add(stripped_item_count.saturating_mul(std::mem::size_of::<PivotItemRef>()))
                    .saturating_add(vdef.name.len())
                    .saturating_add(vdef.source_col.len())
                    .saturating_add(stripped_str_bytes);
                if stripped_bytes > MAX_RETAINED_INDEX_BYTES {
                    return Err(IndexBail::Budget("workbook retained budget exceeded"));
                }
                charge_work(table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, stripped_bytes)?;
                let stripped: Vec<PivotItemRef> =
                    re.items.iter().chain(&ce.items).filter(|(fi, _)| !is_manual_parent(*fi)).map(|(_, it)| it.clone()).collect();
                let stripped_cell =
                    PivotDataCell { data_field: vdef.name.clone(), source_col: vdef.source_col.clone(), items: stripped, row: row_off, col: col_off };
                record(stripped_cell, 0, stripped_bytes, table_work_bytes, &mut cx.pivot_index_workbook_work_bytes, &mut table_retained)?;
            }
        }
    }
    if cells.is_empty() {
        return Err(IndexBail::Unsupported);
    }
    Ok((cells, table_retained))
}

/// The first [`MAX_FIELDS`] items; sets `truncated` when there were more.
fn capped<T>(items: impl Iterator<Item = T>, truncated: &mut bool) -> Vec<T> {
    let mut v: Vec<T> = items.take(MAX_FIELDS.saturating_add(1)).collect();
    if v.len() > MAX_FIELDS {
        v.truncate(MAX_FIELDS);
        *truncated = true;
    }
    v
}
