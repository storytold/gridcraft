//! PivotTable slicers (the Excel 2010 `x14` extension, [MS-XLSX] §2.3): a drawing frame
//! (`sle:slicer`, in a markup-compatibility block with a fallback shape) places the slicer, a
//! slicer part (`xl/slicers/slicerN.xml`, listed by the sheet's `x14:slicerList`) holds its
//! caption and layout, and a slicer cache (`xl/slicerCaches/slicerCacheN.xml`, listed by the
//! workbook's `x14:slicerCaches`) names the PivotTables, the field and the selected items.
//!
//! Reading: frames and slicer parts are collected while sheets are read, then [`resolve`] joins
//! them with the slicer caches once the PivotTables exist. The selection lives in the model as the
//! PivotTables' filters on the field (read from their hidden items); a cache's `s` flags only fill
//! in a PivotTable that has no filter on the field. Table and OLAP slicers, and slicers on a
//! coarser date level that has its own cache field, are skipped with a warning.
//!
//! Writing: [`SlicerWriter`] plans the slicers before the sheets are written, so the PivotTable
//! writer can give their fields shared items and their caches an id; the sheets write the frames
//! and slicer parts and [`SlicerWriter::finish`] the caches.

use std::fmt::Write as _;

use gridcraft_model::{
    Anchor, PivotDateGroup, PivotFilter, PivotSort, PivotTable, Sheet, Slicer, SlicerCrossFilter, SlicerPivot, SlicerSource, Workbook,
};

use crate::package::Rel;
use crate::read::Ctx;
use crate::tables::EMU_PER_PX;
use crate::write::{Out, Rels, XML_DECL};
use crate::xml::{El, esc_attr};

pub const CT_SLICER: &str = "application/vnd.ms-excel.slicer+xml";
pub const CT_SLICER_CACHE: &str = "application/vnd.ms-excel.slicerCache+xml";
const REL_SLICER: &str = "http://schemas.microsoft.com/office/2007/relationships/slicer";
const REL_SLICER_CACHE: &str = "http://schemas.microsoft.com/office/2007/relationships/slicerCache";
const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// `a:graphicData` uri (and `sle` namespace) of a slicer frame.
pub const FRAME_URI: &str = "http://schemas.microsoft.com/office/drawing/2010/slicer";
const EXT_SHEET_SLICERS: &str = "{A8765BA9-456A-4dab-B4F3-ACF838C121DE}";
const EXT_WB_SLICER_CACHES: &str = "{BBE1A952-AA13-448e-AADC-164F8A28A991}";
/// Excel's default button height (EMU), written when the model has none.
const DEFAULT_ROW_HEIGHT: u32 = 241_300;
/// Most items read from one slicer cache.
const MAX_ITEMS: usize = 1_048_576;
/// Most slicers read from one workbook.
const MAX_SLICERS: usize = 4096;

// ---------------------------------------------------------------- reading

/// Slicer pieces found while reading sheets.
#[derive(Default)]
pub struct SlicersIn {
    /// (sheet name, slicer name, anchor) of the `sle:slicer` drawing frames.
    frames: Vec<(String, String, Anchor)>,
    /// (sheet name, slicer) of the slicers the sheets list.
    parts: Vec<(String, PartIn)>,
}

/// One `<slicer>` of a slicer part.
struct PartIn {
    name: String,
    cache: String,
    caption: String,
    columns: u32,
    style: String,
    row_height: Option<f32>,
}

/// A slicer cache definition.
struct CacheDef {
    name: String,
    source: String,
    /// (`tabId`, PivotTable name).
    pivots: Vec<(u32, String)>,
    /// `None`: not a PivotTable (tabular) cache.
    tabular: Option<Tabular>,
}

struct Tabular {
    cache_id: Option<u32>,
    sort: PivotSort,
    cross: SlicerCrossFilter,
    show_missing: bool,
    /// (shared item index, selected).
    items: Vec<(u32, bool)>,
}

/// Records a drawing frame's slicer; returns false when `gd` is not a slicer frame.
pub fn read_frame(cx: &mut Ctx<'_>, sheet: &str, gd: &El, anchor: Anchor) -> bool {
    if gd.attr("uri") != Some(FRAME_URI) {
        return false;
    }
    let Some(name) = gd.child("slicer").and_then(|s| s.attr("name")) else { return false };
    if cx.slicers.frames.len() < MAX_SLICERS {
        cx.slicers.frames.push((sheet.to_string(), name.to_string(), anchor));
    }
    true
}

/// Reads the slicer parts a worksheet's `extLst` lists (`x14:slicerList`).
pub fn read_sheet_ext(cx: &mut Ctx<'_>, ext_lst: &El, rels: &[Rel], sheet: &str) {
    for list in ext_lst.kids("ext").flat_map(|e| e.kids("slicerList")) {
        for sl in list.kids("slicer") {
            let Some(rel) = sl.attr("id").and_then(|id| rels.iter().find(|r| r.id == id && !r.external)) else { continue };
            let target = rel.target.clone();
            let Ok(Some(x)) = cx.optional_xml(&target) else { continue };
            for s in x.kids("slicer") {
                if cx.slicers.parts.len() >= MAX_SLICERS {
                    return;
                }
                let part = PartIn {
                    name: s.attr("name").unwrap_or("").to_string(),
                    cache: s.attr("cache").unwrap_or("").to_string(),
                    caption: s.attr("caption").unwrap_or("").to_string(),
                    columns: s.attr_u32("columnCount").unwrap_or(1).clamp(1, 20_000),
                    style: s.attr("style").unwrap_or("").to_string(),
                    row_height: s.attr_u32("rowHeight").filter(|h| *h > 0).map(|h| (h as f64 / EMU_PER_PX) as f32),
                };
                cx.slicers.parts.push((sheet.to_string(), part));
            }
        }
    }
}

fn read_cache_def(x: &El) -> Option<CacheDef> {
    if x.name != "slicerCacheDefinition" {
        return None;
    }
    let pivots = x
        .child("pivotTables")
        .map(|p| p.kids("pivotTable").filter_map(|t| Some((t.attr_u32("tabId")?, t.attr("name")?.to_string()))).collect())
        .unwrap_or_default();
    let tabular = x.path(&["data", "tabular"]).map(|t| Tabular {
        cache_id: t.attr_u32("pivotCacheId"),
        sort: if t.attr("sortOrder") == Some("descending") { PivotSort::Desc } else { PivotSort::Asc },
        cross: match t.attr("crossFilter") {
            Some("none") => SlicerCrossFilter::None,
            Some("showItemsWithNoData") => SlicerCrossFilter::ShowItemsWithNoData,
            _ => SlicerCrossFilter::ShowItemsWithDataAtTop,
        },
        show_missing: t.flag("showMissing", true),
        items: t
            .child("items")
            .map(|i| i.kids("i").filter_map(|e| Some((e.attr_u32("x")?, e.flag("s", false)))).take(MAX_ITEMS).collect())
            .unwrap_or_default(),
    });
    Some(CacheDef { name: x.attr("name").unwrap_or("").to_string(), source: x.attr("sourceName").unwrap_or("").to_string(), pivots, tabular })
}

/// Joins the collected frames and slicer parts with the workbook's slicer caches and adds the
/// PivotTable slicers to their sheets. `sheet_els` are the workbook's `<sheet>` elements and
/// `file_to_model` maps each to a model sheet.
pub fn resolve(
    cx: &mut Ctx<'_>,
    wb_xml: &El,
    wb_rels: &[Rel],
    sheet_els: &[El],
    file_to_model: &[Option<usize>],
    caches: &crate::pivot::CachesIn,
    wb: &mut Workbook,
) {
    let found = std::mem::take(&mut cx.slicers);
    if found.frames.is_empty() && found.parts.is_empty() {
        return;
    }
    let mut defs: Vec<CacheDef> = Vec::new();
    let lists = wb_xml.child("extLst").map(|l| l.kids("ext").flat_map(|e| e.kids("slicerCaches")).collect::<Vec<_>>()).unwrap_or_default();
    for c in lists.iter().flat_map(|l| l.kids("slicerCache")) {
        let Some(rel) = c.attr("id").and_then(|id| wb_rels.iter().find(|r| r.id == id && !r.external)) else { continue };
        let target = rel.target.clone();
        if let Ok(Some(x)) = cx.optional_xml(&target)
            && let Some(d) = read_cache_def(&x)
            && defs.len() < MAX_SLICERS
        {
            defs.push(d);
        }
    }
    // `tabId` is the `sheetId` of the PivotTable's sheet.
    let sheet_of_tab = |tab: u32| -> Option<usize> {
        let i = sheet_els.iter().position(|s| s.attr_u32("sheetId") == Some(tab))?;
        file_to_model.get(i).copied().flatten()
    };
    let mut used = vec![false; found.frames.len()];
    for (sheet_name, part) in found.parts {
        let Some(si) = wb.sheet_index(&sheet_name) else { continue };
        // The part's frame is accounted for even when the slicer is skipped below with its own
        // warning.
        let fi = found.frames.iter().position(|(s, n, _)| s.eq_ignore_ascii_case(&sheet_name) && n.eq_ignore_ascii_case(&part.name));
        if let Some(u) = fi.and_then(|i| used.get_mut(i)) {
            *u = true;
        }
        let Some(def) = defs.iter().find(|d| d.name.eq_ignore_ascii_case(&part.cache)) else {
            cx.warn(format!("slicer \"{}\" has no slicer cache and was skipped", part.name));
            continue;
        };
        let Some(tab) = &def.tabular else {
            cx.warn(format!("slicer \"{}\" filters a table or an external data source, which is not supported; skipped", part.name));
            continue;
        };
        // The model's slicer filters a source column; a cache field that isn't one (a coarser date
        // level Excel keeps in its own field, such as "Years" over "Date") can't be filtered or
        // saved.
        if let Some(other) = tab.cache_id.and_then(|id| caches.slicer_non_source(id, &def.source)) {
            cx.warn(match other {
                crate::pivot::NonSourceField::DateLevel { base } => {
                    format!(
                        "slicer \"{}\" filters \"{}\", a coarser date level of \"{base}\", which is not supported; skipped",
                        part.name, def.source
                    )
                }
                crate::pivot::NonSourceField::Other => {
                    format!("slicer \"{}\" filters \"{}\", which is not a column of its PivotTable's source; skipped", part.name, def.source)
                }
            });
            continue;
        }
        // The connected PivotTables that were read: (sheet, index).
        let targets: Vec<(usize, usize)> = def
            .pivots
            .iter()
            .filter_map(|(t, name)| {
                let psi = sheet_of_tab(*t)?;
                let pi = wb.sheet(psi)?.pivots.iter().position(|p| p.name.eq_ignore_ascii_case(name))?;
                Some((psi, pi))
            })
            .collect();
        if targets.is_empty() || def.source.trim().is_empty() {
            cx.warn(format!("slicer \"{}\": its PivotTable is missing or could not be read; skipped", part.name));
            continue;
        }
        let Some(&(_, _, anchor)) = fi.and_then(|i| found.frames.get(i)) else {
            cx.warn(format!("slicer \"{}\" has no drawing frame and was skipped", part.name));
            continue;
        };
        // The cache field's items and date grouping. A PivotTable without a filter on the field
        // takes the cache's selection; with everything selected it still keeps the field's
        // grouping (the cache field's, ECMA-376 §18.10.1.33) when no Rows/Columns field has it.
        let field = tab.cache_id.and_then(|id| caches.slicer_labels(id, &def.source));
        let group = field.map(|(_, g)| g).unwrap_or_default();
        let selected: Option<Vec<String>> = field.and_then(|(labels, _)| {
            let sel: Vec<String> = tab.items.iter().filter(|(_, s)| *s).filter_map(|(x, _)| labels.get(*x as usize).cloned()).collect();
            (tab.items.iter().any(|(_, s)| !s) && !sel.is_empty()).then_some(sel)
        });
        for &(psi, pi) in &targets {
            let Some(pt) = wb.sheet_mut(psi).and_then(|sh| sh.pivots.get_mut(pi)) else { continue };
            if pt.filters.iter().any(|f| f.source_col.eq_ignore_ascii_case(&def.source)) {
                continue;
            }
            match &selected {
                Some(sel) => pt.set_filter(&def.source, Some(sel.clone()), group),
                None if group != PivotDateGroup::None && pt.field_group(&def.source) != group => {
                    pt.filters.push(PivotFilter { source_col: def.source.clone(), selected: None, hidden: true, date_group: group });
                    pt.normalize_filter_groups();
                }
                None => {}
            }
        }
        let pivots = targets
            .iter()
            .filter_map(|&(psi, pi)| {
                let sh = wb.sheet(psi)?;
                Some(SlicerPivot { sheet: sh.name.clone(), name: sh.pivots.get(pi)?.name.clone() })
            })
            .collect();
        let id = cx.next_object_id();
        let slicer = Slicer {
            id,
            source: SlicerSource::Pivot {
                pivots,
                field: def.source.clone(),
                sort: tab.sort,
                cross_filter: tab.cross,
                show_missing: tab.show_missing,
            },
            caption: part.caption,
            anchor,
            columns: part.columns,
            name: part.name,
            style: part.style,
            row_height: part.row_height,
        };
        if let Some(sh) = wb.sheet_mut(si) {
            sh.slicers.push(slicer);
        }
    }
    for ((_, name, _), u) in found.frames.iter().zip(used) {
        if !u {
            cx.warn(format!("slicer \"{name}\" was skipped (only PivotTable slicers are supported)"));
        }
    }
}

// ---------------------------------------------------------------- writing

/// A slicer to write.
struct Planned {
    sheet: usize,
    /// Index in the sheet's slicers.
    slicer: usize,
    /// Unique slicer name.
    name: String,
    /// Unique slicer cache name (also a workbook defined name).
    cache: String,
    field: String,
    /// The connected PivotTables written (sheet, index), which share one pivot cache.
    pivots: Vec<(usize, usize)>,
}

/// A slicer on the sheet being written: its index in the sheet's slicers, name and cache name.
pub struct SheetSlicer {
    pub slicer: usize,
    pub name: String,
    pub cache: String,
}

/// The PivotTable slicers of a workbook being written.
#[derive(Default)]
pub struct SlicerWriter {
    planned: Vec<Planned>,
    /// PivotTables (sheet, index) written with one pivot cache: those the same slicers connect.
    groups: Vec<Vec<(usize, usize)>>,
    parts: u32,
}

impl SlicerWriter {
    /// Plans the PivotTable slicers whose PivotTables can be written. A slicer's PivotTables must
    /// share a pivot cache; where they can't (different sources or date grouping), the slicer
    /// keeps only its first PivotTable.
    pub fn new(wb: &Workbook) -> SlicerWriter {
        let mut planned: Vec<Planned> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut caches: Vec<String> = Vec::new();
        for (si, sheet) in wb.sheets.iter().enumerate() {
            for (i, sl) in sheet.slicers.iter().enumerate() {
                let SlicerSource::Pivot { pivots, field, .. } = &sl.source else { continue };
                let mut targets: Vec<(usize, usize)> = Vec::new();
                for p in pivots {
                    let Some(psi) = wb.sheet_index(&p.sheet) else { continue };
                    let Some(sh) = wb.sheet(psi) else { continue };
                    let Some(pi) = sh.pivots.iter().position(|t| t.name.eq_ignore_ascii_case(&p.name)) else { continue };
                    let ok = sh.pivots.get(pi).is_some_and(|pt| crate::pivot::slicer_field_ok(wb, pt, psi, field));
                    if ok && !targets.contains(&(psi, pi)) {
                        targets.push((psi, pi));
                    }
                }
                if targets.is_empty() {
                    continue;
                }
                let base = [sl.name.trim(), sl.caption.trim(), field.trim()].into_iter().find(|n| !n.is_empty()).unwrap_or("Slicer");
                let name = unique(base, " ", &mut names);
                let clean: String = field.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
                let cache = unique(&format!("Slicer_{clean}"), "", &mut caches);
                planned.push(Planned { sheet: si, slicer: i, name, cache, field: field.clone(), pivots: targets });
            }
        }
        let mut groups = merge_groups(&planned);
        let mut split = false;
        for g in groups.iter().filter(|g| g.len() > 1) {
            if !crate::pivot::can_share(wb, g, &group_fields(&planned, g)) {
                for p in planned.iter_mut().filter(|p| p.pivots.iter().any(|x| g.contains(x))) {
                    p.pivots.truncate(1);
                }
                split = true;
            }
        }
        if split {
            groups = merge_groups(&planned);
        }
        SlicerWriter { planned, groups, parts: 0 }
    }

    /// The PivotTables written with the same pivot cache as PivotTable `pi` on sheet `si`
    /// (itself included), when slicers connect it.
    pub fn cache_group(&self, si: usize, pi: usize) -> Option<Vec<(usize, usize)>> {
        self.groups.iter().find(|g| g.contains(&(si, pi))).cloned()
    }

    /// Fields that slicers filter in the pivot cache of PivotTable `pi` on sheet `si`.
    pub fn fields_for(&self, si: usize, pi: usize) -> Vec<String> {
        self.groups.iter().find(|g| g.contains(&(si, pi))).map(|g| group_fields(&self.planned, g)).unwrap_or_default()
    }

    /// The slicers to write on sheet `si`.
    pub fn on_sheet(&self, si: usize) -> Vec<SheetSlicer> {
        self.planned
            .iter()
            .filter(|p| p.sheet == si)
            .map(|p| SheetSlicer { slicer: p.slicer, name: p.name.clone(), cache: p.cache.clone() })
            .collect()
    }

    /// Whether `name` is a slicer cache name (written as a defined name by the writer).
    pub fn is_cache_name(&self, name: &str) -> bool {
        self.planned.iter().any(|p| p.cache.eq_ignore_ascii_case(name))
    }

    /// `<definedName>` elements Excel keeps for slicer caches.
    pub fn defined_names(&self) -> String {
        self.planned.iter().map(|p| format!("<definedName name=\"{}\">#N/A</definedName>", esc_attr(&p.cache))).collect()
    }

    /// Writes a sheet's slicer part and relationship; returns the sheet's `x14:slicerList`
    /// extension (empty when the sheet has none).
    pub fn sheet_part(sheet: &Sheet, list: &[SheetSlicer], out: &mut Out, rels: &mut Rels) -> String {
        if list.is_empty() {
            return String::new();
        }
        let mut x = format!("{XML_DECL}<slicers xmlns=\"{NS_X14}\" xmlns:mc=\"{NS_MC}\" mc:Ignorable=\"x\" xmlns:x=\"{}\">", crate::write::NS_MAIN);
        let mut any = false;
        for p in list {
            let Some(sl) = sheet.slicers.get(p.slicer) else { continue };
            any = true;
            let _ = write!(x, "<slicer name=\"{}\" cache=\"{}\"", esc_attr(&p.name), esc_attr(&p.cache));
            if !sl.caption.is_empty() {
                let _ = write!(x, " caption=\"{}\"", esc_attr(&sl.caption));
            }
            if sl.columns > 1 {
                let _ = write!(x, " columnCount=\"{}\"", sl.columns.min(20_000));
            }
            if let Some(st) = builtin_style(&sl.style) {
                let _ = write!(x, " style=\"{st}\"");
            }
            let h = sl.row_height.filter(|h| h.is_finite() && *h > 0.0).map(|h| (h as f64 * EMU_PER_PX).round().min(u32::MAX as f64) as u32);
            let _ = write!(x, " rowHeight=\"{}\"/>", h.unwrap_or(DEFAULT_ROW_HEIGHT));
        }
        if !any {
            return String::new();
        }
        x.push_str("</slicers>");
        out.slicers.parts += 1;
        let n = out.slicers.parts;
        out.part(&format!("xl/slicers/slicer{n}.xml"), Some(CT_SLICER), x.into_bytes());
        let rid = rels.add_uri(REL_SLICER, &format!("../slicers/slicer{n}.xml"));
        format!("<ext uri=\"{EXT_SHEET_SLICERS}\" xmlns:x14=\"{NS_X14}\"><x14:slicerList><x14:slicer r:id=\"{rid}\"/></x14:slicerList></ext>")
    }

    /// Writes the slicer caches; returns the workbook `extLst`'s `ext` listing them (empty when
    /// there are none). `sheet_ids` is the `sheetId` written for each model sheet, which a cache's
    /// `tabId` names.
    pub fn finish(self, wb: &Workbook, sheet_ids: &[usize], pivots: &crate::pivot::PivotWriter, out: &mut Out, wb_rels: &mut Rels) -> String {
        let mut list = String::new();
        for (k, p) in self.planned.iter().enumerate() {
            // The connected PivotTables that were written, with their (shared) cache's data.
            let written: Vec<((usize, usize), &PivotTable, &crate::pivot::PivotSlicerInfo)> =
                p.pivots.iter().filter_map(|&(psi, pi)| Some(((psi, pi), wb.sheet(psi)?.pivots.get(pi)?, pivots.slicer_info(psi, pi)?))).collect();
            let Some(&(_, pt, info)) = written.first() else { continue };
            let Some(sl) = wb.sheet(p.sheet).and_then(|s| s.slicers.get(p.slicer)) else { continue };
            let SlicerSource::Pivot { sort, cross_filter, show_missing, .. } = &sl.source else { continue };
            let field = info.fields.iter().find(|f| f.name.eq_ignore_ascii_case(&p.field));
            let labels: &[String] = field.map(|f| f.labels.as_slice()).unwrap_or(&[]);
            // The selection at the base cache field's level (the items written below).
            let level = pt.field_group(&p.field);
            let sel = pt
                .filters
                .iter()
                .find(|f| f.source_col.eq_ignore_ascii_case(&p.field) && pt.filter_group(f) == level)
                .and_then(|f| f.selected.as_deref());
            let mut on: Vec<bool> = labels.iter().map(|l| sel.is_none_or(|s| s.iter().any(|x| x.eq_ignore_ascii_case(l)))).collect();
            if !on.contains(&true) {
                on.iter_mut().for_each(|o| *o = true);
            }
            // Date groups are in date order already; other items sort by value.
            let mut order: Vec<usize> = (0..labels.len()).collect();
            match (sort, field.is_some_and(|f| f.grouped)) {
                (PivotSort::Asc, false) => order.sort_by(|a, b| cmp_label(&labels[*a], &labels[*b])),
                (PivotSort::Desc, false) => order.sort_by(|a, b| cmp_label(&labels[*b], &labels[*a])),
                (PivotSort::Desc, true) => order.reverse(),
                (PivotSort::Asc | PivotSort::None, true) | (PivotSort::None, false) => {}
            }
            let mut x = format!(
                "{XML_DECL}<slicerCacheDefinition xmlns=\"{NS_X14}\" xmlns:mc=\"{NS_MC}\" mc:Ignorable=\"x\" xmlns:x=\"{}\" name=\"{}\" sourceName=\"{}\"><pivotTables>",
                crate::write::NS_MAIN,
                esc_attr(&p.cache),
                esc_attr(&p.field),
            );
            for &((psi, pi), t, i) in &written {
                if i.cache_id != info.cache_id {
                    continue;
                }
                let Some(tab) = sheet_ids.get(psi) else { continue };
                let pname = if t.name.trim().is_empty() { format!("PivotTable{}", pi + 1) } else { t.name.clone() };
                let _ = write!(x, "<pivotTable tabId=\"{tab}\" name=\"{}\"/>", esc_attr(&pname));
            }
            let _ = write!(x, "</pivotTables><data><tabular pivotCacheId=\"{}\"", info.cache_id);
            if *sort == PivotSort::Desc {
                x.push_str(" sortOrder=\"descending\"");
            }
            if !show_missing {
                x.push_str(" showMissing=\"0\"");
            }
            match cross_filter {
                SlicerCrossFilter::None => x.push_str(" crossFilter=\"none\""),
                SlicerCrossFilter::ShowItemsWithNoData => x.push_str(" crossFilter=\"showItemsWithNoData\""),
                SlicerCrossFilter::ShowItemsWithDataAtTop => {}
            }
            let _ = write!(x, "><items count=\"{}\">", order.len());
            for i in order {
                let _ = write!(x, "<i x=\"{i}\"{}/>", if on[i] { " s=\"1\"" } else { "" });
            }
            x.push_str("</items></tabular></data></slicerCacheDefinition>");
            let part = format!("slicerCaches/slicerCache{}.xml", k + 1);
            out.part(&format!("xl/{part}"), Some(CT_SLICER_CACHE), x.into_bytes());
            let rid = wb_rels.add_uri(REL_SLICER_CACHE, &part);
            let _ = write!(list, "<x14:slicerCache r:id=\"{rid}\"/>");
        }
        if list.is_empty() {
            return String::new();
        }
        format!("<ext uri=\"{EXT_WB_SLICER_CACHES}\" xmlns:x14=\"{NS_X14}\"><x14:slicerCaches>{list}</x14:slicerCaches></ext>")
    }
}

/// The PivotTables (sheet, index) that must share a pivot cache: each slicer's, merged where
/// slicers have a PivotTable in common.
fn merge_groups(planned: &[Planned]) -> Vec<Vec<(usize, usize)>> {
    let mut groups: Vec<Vec<(usize, usize)>> = Vec::new();
    for p in planned {
        let mut g = p.pivots.clone();
        let (overlap, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut groups).into_iter().partition(|o| o.iter().any(|x| g.contains(x)));
        groups = rest;
        for x in overlap.into_iter().flatten() {
            if !g.contains(&x) {
                g.push(x);
            }
        }
        groups.push(g);
    }
    groups
}

/// The fields that the slicers on the PivotTables `group` filter.
fn group_fields(planned: &[Planned], group: &[(usize, usize)]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for p in planned.iter().filter(|p| p.pivots.iter().any(|x| group.contains(x))) {
        if !v.iter().any(|f| f.eq_ignore_ascii_case(&p.field)) {
            v.push(p.field.clone());
        }
    }
    v
}

/// `base`, or `base{sep}2`, `base{sep}3`… if taken (case-insensitively); records the result.
fn unique(base: &str, sep: &str, taken: &mut Vec<String>) -> String {
    let mut n = base.to_string();
    let mut i = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&n)) {
        n = format!("{base}{sep}{i}");
        i += 1;
    }
    taken.push(n.clone());
    n
}

/// Ascending item order: numbers (dates are serials) by value, before text by case-insensitive
/// text.
fn cmp_label(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<f64>().ok().filter(|n| n.is_finite()), b.parse::<f64>().ok().filter(|n| n.is_finite())) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// A built-in slicer style name to write: the style itself, or the built-in a custom copy is named
/// after (`SlicerStyleDark1 2`). Custom style definitions aren't written, so other names are
/// dropped and Excel uses its default.
fn builtin_style(style: &str) -> Option<&str> {
    let base = style.split_whitespace().next()?;
    let ok =
        ["SlicerStyleLight", "SlicerStyleDark"].iter().any(|p| base.strip_prefix(p).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6")))
            || matches!(base, "SlicerStyleOther1" | "SlicerStyleOther2");
    ok.then_some(base)
}

/// The drawing XML of a slicer frame, with a fallback shape for readers without slicers.
pub fn frame_xml(id: usize, name: &str, xfrm: &str, a_xfrm: &str) -> String {
    format!(
        "<mc:AlternateContent xmlns:mc=\"{NS_MC}\"><mc:Choice xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\" Requires=\"a14\"><xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{id}\" name=\"{n}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>{xfrm}<a:graphic><a:graphicData uri=\"{FRAME_URI}\"><sle:slicer xmlns:sle=\"{FRAME_URI}\" name=\"{n}\"/></a:graphicData></a:graphic></xdr:graphicFrame></mc:Choice><mc:Fallback><xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"0\" name=\"\"/><xdr:cNvSpPr><a:spLocks noTextEdit=\"1\"/></xdr:cNvSpPr></xdr:nvSpPr><xdr:spPr>{a_xfrm}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr vertOverflow=\"clip\" horzOverflow=\"clip\"/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\" sz=\"1100\"/><a:t>This is a PivotTable slicer. Slicers need Excel 2010 or later.</a:t></a:r></a:p></xdr:txBody></xdr:sp></mc:Fallback></mc:AlternateContent>",
        n = esc_attr(name)
    )
}
