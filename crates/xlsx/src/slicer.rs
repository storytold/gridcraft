//! Table slicers: writing and reading GridCraft's on-sheet slicer widgets as genuine Excel
//! *table* slicers.
//!
//! A table slicer is spread across several coordinated parts ([MS-XLSX] 2.1.26, and the
//! x15 extensions in [MS-XLSX] 2.5.*):
//! - a **slicer cache** part per slicer (`xl/slicerCaches/slicerCacheN.xml`), whose
//!   `x15:tableSlicerCache` binds it to a table id and a 1-based column, referenced from the
//!   workbook rels and from the workbook `extLst` (`x15:slicerCaches`);
//! - a **slicer** part per sheet (`xl/slicers/slicerN.xml`) listing each `<slicer>`, referenced
//!   from the worksheet rels and from the worksheet `extLst` (`x15:slicerList`);
//! - a **drawing anchor** (`sle:slicer`, written by [`crate::drawing`]) that positions the widget.
//!
//! The *selection* of a table slicer is carried by the table column's filter, not by the slicer
//! parts, so [`crate::sheet_write`] mirrors each slicer's `selected` into the table's `<autoFilter>`
//! and the reader rebuilds `Slicer::selected` from it.
//!
//! Only element/attribute names and the fixed extension GUIDs come from the public schema; no Excel
//! style artwork is copied — the `style` attribute names a built-in style that Excel defines itself.

use crate::xml::esc_attr;

/// Relationship type of a slicer part (from the worksheet).
pub const REL_SLICER: &str = "http://schemas.microsoft.com/office/2007/relationships/slicer";
/// Relationship type of a slicer-cache part (from the workbook).
pub const REL_SLICER_CACHE: &str = "http://schemas.microsoft.com/office/2007/relationships/slicerCache";
/// Content type of a slicer part.
pub const CT_SLICER: &str = "application/vnd.ms-excel.slicer+xml";
/// Content type of a slicer-cache part.
pub const CT_SLICER_CACHE: &str = "application/vnd.ms-excel.slicerCache+xml";

/// `x14` main namespace (slicer part/elements).
const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
/// `x15` namespace (table slicer extensions).
const NS_X15: &str = "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main";
/// Spreadsheet main namespace.
const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
/// Relationships namespace.
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Markup-compatibility namespace.
const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// `extLst` uri of the `x15:tableSlicerCache` inside a slicer-cache part.
const EXT_TABLE_SLICER_CACHE: &str = "{2F2917AC-EB37-4324-AD4E-5DD8C200BD13}";
/// `extLst` uri of the worksheet's table `x15:slicerList`.
pub const EXT_WORKSHEET_SLICERS: &str = "{3A4CF648-6AED-40f4-86FF-DC5316D8B592}";
/// `extLst` uri of the workbook's table `x15:slicerCaches`.
pub const EXT_WORKBOOK_SLICER_CACHES: &str = "{46BE6895-7355-4a93-B00E-2C351335B9C9}";

/// One slicer to emit into a sheet's slicer part.
pub struct SlicerOut {
    /// Slicer name: a defined-name-like identifier (no spaces), unique in the workbook.
    pub name: String,
    /// Name of the slicer cache this slicer draws from.
    pub cache: String,
    /// Caption shown in the slicer header (free text).
    pub caption: String,
    /// Number of button columns.
    pub columns: u32,
}

/// Metadata for one workbook-level slicer-cache part, collected while sheets are written.
pub struct CacheOut {
    /// Cache name (referenced by the slicer's `cache` attribute).
    pub name: String,
    /// Source column header name.
    pub source: String,
    /// Global table id this slicer filters.
    pub table_id: u32,
    /// 1-based column index within the table.
    pub column: u32,
}

/// A slicer name derived from a table and column, e.g. `Slicer_Sales_Region`. Keeps only
/// identifier characters so it is a valid slicer/cache name; `id` disambiguates collisions.
pub fn slicer_name(table: &str, column: &str, id: u32) -> String {
    let keep = |s: &str| s.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect::<String>();
    let base = format!("Slicer_{}_{}", keep(table), keep(column));
    let base = base.trim_matches('_');
    if base.is_empty() { format!("Slicer_{id}") } else { format!("{base}_{id}") }
}

/// The slicer-cache part for one table slicer. The root uses the x14 slicer namespace exactly as
/// Excel writes it; the table binding lives in the `x15:tableSlicerCache` extension.
pub fn cache_part(c: &CacheOut) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<slicerCacheDefinition xmlns=\"{NS_X14}\" xmlns:mc=\"{NS_MC}\" mc:Ignorable=\"x15\" \
xmlns:x=\"{NS_MAIN}\" xmlns:x15=\"{NS_X15}\" name=\"{name}\" sourceName=\"{source}\">\
<extLst><ext uri=\"{EXT_TABLE_SLICER_CACHE}\"><x15:tableSlicerCache tableId=\"{tid}\" column=\"{col}\"/></ext></extLst>\
</slicerCacheDefinition>",
        name = esc_attr(&c.name),
        source = esc_attr(&c.source),
        tid = c.table_id,
        col = c.column,
    )
}

/// The slicer part listing all of one sheet's slicers.
pub fn slicers_part(slicers: &[SlicerOut]) -> String {
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<slicers xmlns=\"{NS_X14}\" xmlns:r=\"{NS_R}\" xmlns:mc=\"{NS_MC}\" xmlns:x=\"{NS_MAIN}\">"
    );
    for sl in slicers {
        // Attributes and order as Excel writes them: a `rowHeight`, then `columnCount` only when it
        // differs from the default of 1 (so a single-column slicer matches Excel byte-shape). Style
        // and `showCaption` are left at their defaults — no Excel style artwork is emitted.
        let column_count = if sl.columns > 1 { format!(" columnCount=\"{}\"", sl.columns) } else { String::new() };
        s.push_str(&format!(
            "<slicer name=\"{name}\" cache=\"{cache}\" caption=\"{caption}\" rowHeight=\"241300\"{column_count}/>",
            name = esc_attr(&sl.name),
            cache = esc_attr(&sl.cache),
            caption = esc_attr(&sl.caption),
        ));
    }
    s.push_str("</slicers>");
    s
}

/// The worksheet `<ext>` (inside `<extLst>`) that lists the sheet's table slicers by rel id.
pub fn worksheet_ext(rid: &str) -> String {
    format!(
        "<ext uri=\"{EXT_WORKSHEET_SLICERS}\" xmlns:x15=\"{NS_X15}\">\
<x15:slicerList><x15:slicer r:id=\"{rid}\"/></x15:slicerList></ext>",
        rid = esc_attr(rid),
    )
}

/// The workbook `<ext>` (inside `<extLst>`) that lists the table slicer caches by rel id.
pub fn workbook_ext(rids: &[String]) -> String {
    let mut s = format!("<ext uri=\"{EXT_WORKBOOK_SLICER_CACHES}\" xmlns:x15=\"{NS_X15}\"><x15:slicerCaches>");
    for rid in rids {
        s.push_str(&format!("<x15:slicerCache r:id=\"{}\"/>", esc_attr(rid)));
    }
    s.push_str("</x15:slicerCaches></ext>");
    s
}

// ---------------------------------------------------------------- reading

use crate::IoError;
use crate::package::Rel;
use crate::read::Ctx;
use crate::xml::El;

/// One `<slicer>` parsed from a slicer part.
pub struct SlicerIn {
    pub name: String,
    pub cache: String,
    pub caption: String,
    pub columns: u32,
}

/// Reads the table slicer caches reachable from the workbook rels into `cx.slicer_caches`
/// (cache name → file table id, 1-based column). Pivot slicer caches (no `tableSlicerCache`)
/// are ignored. Problems are warnings, never errors.
pub fn read_caches(cx: &mut Ctx<'_>, wb_rels: &[Rel]) -> Result<(), IoError> {
    let targets: Vec<String> = wb_rels.iter().filter(|r| r.kind == "slicerCache").map(|r| r.target.clone()).collect();
    for target in targets {
        let Some(x) = cx.optional_xml(&target)? else { continue };
        let Some(name) = x.attr("name").map(str::to_string) else { continue };
        let tsc = x.child("extLst").and_then(|l| l.kids("ext").find_map(|e| e.child("tableSlicerCache")));
        if let Some(tsc) = tsc {
            let table_id = tsc.attr_u32("tableId").unwrap_or(0);
            let column = tsc.attr_u32("column").unwrap_or(0);
            if column > 0 {
                cx.slicer_caches.insert(name, (table_id, column));
            }
        }
    }
    Ok(())
}

/// Reads the `<slicer>` entries of a slicer part.
pub fn read_slicers_part(x: &El) -> Vec<SlicerIn> {
    x.kids("slicer")
        .map(|sl| SlicerIn {
            name: sl.attr("name").unwrap_or("").to_string(),
            cache: sl.attr("cache").unwrap_or("").to_string(),
            caption: sl.attr("caption").map(str::to_string).unwrap_or_default(),
            columns: sl.attr_u32("columnCount").unwrap_or(1).max(1),
        })
        .collect()
}
