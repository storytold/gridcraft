//! PivotTables: reading the source list, grouping and aggregating, laying out the report and
//! writing it onto the sheet as static, styled cells.
//!
//! Behaviour follows Excel's documented PivotTable model (rows/columns/values/filters areas,
//! Compact/Outline/Tabular layouts, subtotals, grand totals, show-values-as); the code and the
//! look (our own palette) are original.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

pub use gridcraft_core::quote_sheet;
use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{
    BorderLine, BorderStyle, Cell, Chart, Color, Fill, HAlign, NumFmt, PivotDataCell, PivotDateGroup, PivotField, PivotFilter, PivotFunc,
    PivotItemRef, PivotLayout, PivotLookup, PivotShowAs, PivotSort, PivotTable, PivotValue, Series, Style, StyleId, Workbook,
};

use crate::pivot_calc::CalcSet;

mod charts;
mod compute;
mod items;
mod output;

use charts::rebind_charts;
pub use charts::{chart_link, chart_series, clear_output, links_to, parse_chart_link};
use compute::compute_from;
pub use compute::{NOT_A_VALUE_CELL, detail_cells, detail_records};
pub use items::{distinct_labels, func_caption, item_label};
#[cfg(test)]
use output::data_cols_exceed_sheet;
pub use output::{CellKind, OutCell, Output, RowKind};
use output::{cell_style, look};

/// Largest source block read (cells).
pub const MAX_SOURCE_CELLS: u64 = 10_000_000;
/// Largest report written (cells).
pub const MAX_OUTPUT_CELLS: u64 = 2_000_000;
/// Most fields in the Rows plus Columns areas.
pub const MAX_AXIS_FIELDS: usize = 32;
/// Most fields in the Values area.
pub const MAX_VALUE_FIELDS: usize = 256;
/// Most report value cells a report may have (every one is indexed for GETPIVOTDATA; bigger
/// reports are rejected).
pub const MAX_INDEXED_CELLS: usize = 1_000_000;

// ---------------------------------------------------------------- source

/// The source list: header names, per-column number formats and the records.
#[derive(Clone, Debug)]
pub struct Source {
    pub sheet: usize,
    pub range: RangeRef,
    pub headers: Vec<String>,
    pub formats: Vec<String>,
    /// Every non-blank value in the column is a number.
    pub numeric: Vec<bool>,
    /// Numeric with a date number format.
    pub dates: Vec<bool>,
    pub rows: Vec<Vec<Value>>,
}

impl Source {
    pub fn col(&self, name: &str) -> Option<usize> {
        self.headers.iter().position(|h| h.eq_ignore_ascii_case(name))
    }
}

/// Absolute reference text for a range on a sheet: `Sheet1!$A$1:$E$20`.
pub fn range_text(sheet_name: &str, r: RangeRef) -> String {
    let abs = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row as u64 + 1);
    format!("{}!{}:{}", quote_sheet(sheet_name), abs(r.start), abs(r.end))
}

/// Resolves source text (range, table name or defined name) to (sheet, range including header).
pub fn resolve_source(wb: &Workbook, text: &str, default_sheet: usize) -> Result<(usize, RangeRef), String> {
    gridcraft_model::pivot_source::resolve(wb, text, default_sheet)
}

/// Reads the source list.
pub fn read_source(wb: &Workbook, text: &str, default_sheet: usize) -> Result<Source, String> {
    let (si, r) = resolve_source(wb, text, default_sheet)?;
    let sh = wb.sheet(si).ok_or("The reference isn't valid.")?;
    if r.count() > MAX_SOURCE_CELLS {
        return Err("The source range is too large for a PivotTable.".into());
    }
    let headers = gridcraft_model::pivot_source::headers(wb, si, r)?;
    let formats: Vec<String> = (r.start.col..=r.end.col).map(|c| gridcraft_model::pivot_source::column_format(wb, si, r, c)).collect();
    let w = headers.len();
    let mut rows = Vec::new();
    let mut numeric = vec![true; w];
    let mut any = vec![false; w];
    if r.end.row > r.start.row {
        for row in r.start.row + 1..=r.end.row {
            let rec: Vec<Value> = (r.start.col..=r.end.col).map(|c| sh.value(CellRef::new(row, c)).scalar()).collect();
            if rec.iter().all(Value::is_empty) {
                continue;
            }
            for (i, v) in rec.iter().enumerate() {
                if !v.is_empty() {
                    if let Some(a) = any.get_mut(i) {
                        *a = true;
                    }
                    if !v.is_number()
                        && let Some(n) = numeric.get_mut(i)
                    {
                        *n = false;
                    }
                }
            }
            rows.push(rec);
        }
    }
    for (n, a) in numeric.iter_mut().zip(any.iter()) {
        *n = *n && *a;
    }
    let dates = numeric.iter().zip(formats.iter()).map(|(n, f)| *n && crate::display::parsed(f).is_date()).collect();
    Ok(Source { sheet: si, range: r, headers, formats, numeric, dates, rows })
}

/// Computes a PivotTable report from the workbook.
pub fn compute(wb: &Workbook, pt: &PivotTable, pivot_sheet: usize) -> Result<Output, String> {
    let (pt, src) = effective_source(wb, pt, pivot_sheet)?;
    compute_from(wb, &pt, &src, None)
}

/// The definition a PivotTable is computed from: one saved by an older GridCraft with its fields
/// under their columns' current names (see [`older_field_names`]), as the report and the field
/// list show them; anything else as it is. The renamed names aren't stored here.
pub fn effective<'a>(wb: &Workbook, pt: &'a PivotTable, pivot_sheet: usize) -> std::borrow::Cow<'a, PivotTable> {
    match older_field_names(wb, pt, pivot_sheet) {
        Some(older) if older.shown.is_none() && !older.names.is_empty() => {
            let mut copy = pt.clone();
            rename_fields(&mut copy, &older.names);
            std::borrow::Cow::Owned(copy)
        }
        _ => std::borrow::Cow::Borrowed(pt),
    }
}

/// [`effective`] with the source it is computed from. An older PivotTable whose fields can't
/// take their columns' current names safely (see [`older_field_names`]) is computed as older
/// GridCraft computed it: its source's headers read as the sheet showed them then.
pub fn effective_source<'a>(wb: &Workbook, pt: &'a PivotTable, pivot_sheet: usize) -> Result<(std::borrow::Cow<'a, PivotTable>, Source), String> {
    let mut src = read_source(wb, &pt.source, pivot_sheet)?;
    if let Some(shown) = older_field_names(wb, pt, pivot_sheet).and_then(|o| o.shown)
        && shown.len() == src.headers.len()
    {
        src.headers = shown;
        return Ok((std::borrow::Cow::Borrowed(pt), src));
    }
    Ok((effective(wb, pt, pivot_sheet), src))
}

/// How the fields of a PivotTable saved by an older GridCraft find their columns.
struct OlderNames {
    /// Saved name (lower case) → its column's current name, for the names that change.
    names: HashMap<String, String>,
    /// Whether the columns are certain, so the PivotTable can keep the current names for good.
    settled: bool,
    /// Whether every field found a column (certain or not).
    placed: bool,
    /// The headers as the sheet showed them when the PivotTable was saved, for one whose fields
    /// can't take their columns' current names safely: it is computed with these instead, as
    /// older GridCraft computed it, and never settles.
    shown: Option<Vec<String>>,
}

/// The fields of a PivotTable saved by an older GridCraft, each with its column's current name;
/// `None` for a PivotTable that already names its fields by header value, or whose source can't
/// be read.
///
/// GridCraft read a header as the cell showed it: its formula under Show Formulas (`="Region"`),
/// a long General number narrowed to the cell width. Now a header is its value through its number
/// format. Which way the sheet showed its headers when the PivotTable was saved isn't stored, so
/// both are tried, whatever Show Formulas is now; the decision is made for the whole PivotTable,
/// so two fields never land on one column. Calculated fields aren't columns and take no part.
/// - Exactly one way places every saved field (or both place them on the same columns, the way
///   the sheet's Show Formulas setting now picks): each field moves to its column's current name,
///   even one whose old name is also some current header (a text header `Region` beside a
///   formula header `="Region"` is now `Region2`).
/// - Both place every field, on different columns: the saved names can't tell, so the fields
///   resolve as they did before, by the sheet's Show Formulas setting now, and nothing is settled.
/// - Neither does: when every name is a current header they stay; otherwise nothing changes.
///
/// When moving the fields would change what a name means (a calculated field's name is now also
/// a header, or a calculated field's formula names a column the current headers put elsewhere),
/// nothing moves: the PivotTable stays an older one and is computed with the headers as they
/// were shown (see [`effective_source`]), so each saved field and formula reads its old column.
fn older_field_names(wb: &Workbook, pt: &PivotTable, pivot_sheet: usize) -> Option<OlderNames> {
    use gridcraft_model::pivot_source::{headers, unique_names};
    if pt.names_from_values {
        return None;
    }
    let (si, r) = resolve_source(wb, &pt.source, pivot_sheet).ok()?;
    let sh = wb.sheet(si)?;
    let current = headers(wb, si, r).ok()?;
    let calculated = |n: &str| pt.calculated_fields.iter().any(|c| c.name.eq_ignore_ascii_case(n));
    let saved: Vec<&String> = pt
        .rows
        .iter()
        .chain(&pt.columns)
        .map(|f| &f.source_col)
        .chain(pt.values.iter().map(|v| &v.source_col).filter(|n| !calculated(n)))
        .chain(pt.filters.iter().map(|f| &f.source_col))
        .collect();
    let shown = |formulas: bool| {
        unique_names((r.start.col..=r.end.col).map(|c| crate::display::cell_text_with(wb, sh, CellRef::new(r.start.row, c), formulas))).ok()
    };
    // Each saved field's column, when the list places them all.
    let columns = |list: &[String]| -> Option<Vec<usize>> { saved.iter().map(|n| list.iter().position(|h| h.eq_ignore_ascii_case(n))).collect() };
    let lists = [shown(true), shown(false)];
    let placed: Vec<Option<(Vec<usize>, &Vec<String>)>> = lists.iter().map(|l| l.as_ref().and_then(|l| columns(l).map(|c| (c, l)))).collect();
    let pick = |i: usize| placed.get(i).cloned().flatten();
    let ((cols, list), settled) = match (pick(0), pick(1)) {
        // Both place every field: by the sheet's Show Formulas setting now. On the same columns the
        // fields are settled, but the headers still differ, so calculated formulas read this list.
        (Some(a), Some(b)) => {
            let same = a.0 == b.0;
            (if sh.show_formulas { a } else { b }, same)
        }
        (Some(a), _) | (None, Some(a)) => (a, true),
        (None, None) => {
            let settled = columns(&current).is_some() && !current.iter().any(|h| calculated(h));
            return Some(OlderNames { names: HashMap::new(), settled, placed: settled, shown: None });
        }
    };
    // Names would no longer mean the columns they meant: a calculated field's name is now a
    // header (`="Margin"` reads `Margin`), or a calculated field's formula names a column that
    // the current headers put elsewhere (`Amount` was B beside `="Amount"` in A; now A). Then the
    // PivotTable is computed with the headers as they were shown, as older GridCraft did.
    let calc = crate::pivot_calc::CalcSet::new(pt, list);
    let moved_ref = (0..pt.calculated_fields.len()).flat_map(|i| calc.source_refs(i)).any(|n| {
        let at = |l: &[String]| l.iter().position(|h| h.eq_ignore_ascii_case(&n));
        at(list) != at(&current)
    });
    if moved_ref || current.iter().any(|h| calculated(h)) {
        return Some(OlderNames { names: HashMap::new(), settled: false, placed: false, shown: Some(list.clone()) });
    }
    let mut names = HashMap::new();
    for (n, c) in saved.iter().zip(cols) {
        let h = current.get(c)?;
        if !h.eq_ignore_ascii_case(n) {
            names.insert(n.to_ascii_lowercase(), h.clone());
        }
    }
    Some(OlderNames { names, settled, placed: true, shown: None })
}

/// Renames the fields named in `names` (calculated fields keep theirs).
fn rename_fields(pt: &mut PivotTable, names: &HashMap<String, String>) {
    let calculated: HashSet<String> = pt.calculated_fields.iter().map(|c| c.name.to_ascii_lowercase()).collect();
    let rename = |n: &mut String| {
        if let Some(h) = names.get(&n.to_ascii_lowercase()) {
            *n = h.clone();
        }
    };
    pt.rows.iter_mut().chain(pt.columns.iter_mut()).for_each(|f| rename(&mut f.source_col));
    pt.values.iter_mut().filter(|v| !calculated.contains(&v.source_col.to_ascii_lowercase())).for_each(|v| rename(&mut v.source_col));
    pt.filters.iter_mut().for_each(|f| rename(&mut f.source_col));
}

/// Moves the fields of a PivotTable saved by an older GridCraft to their columns' current names
/// for good, when [`older_field_names`] is certain of the columns; with `pin`, also when it isn't
/// (an edit made now pins the columns the fields show now). Whether the PivotTable changed.
pub fn settle_field_names(wb: &Workbook, pt: &mut PivotTable, pivot_sheet: usize, pin: bool) -> bool {
    let Some(older) = older_field_names(wb, pt, pivot_sheet) else { return false };
    if !older.settled && !(pin && older.placed) {
        return false;
    }
    rename_fields(pt, &older.names);
    pt.names_from_values = true;
    true
}

/// Recomputes a pivot and writes its report. With `replace` false, refuses when the target area
/// holds other data. Returns the written area.
pub fn refresh(wb: &mut Workbook, sheet: usize, idx: usize, replace: bool) -> Result<RangeRef, String> {
    let mut pt = wb.sheet(sheet).and_then(|s| s.pivots.get(idx)).cloned().ok_or("There's no such PivotTable.")?;
    // A field saved under a header text older GridCraft read keeps its place: it takes its
    // column's current name, which the refreshed PivotTable keeps.
    let migrated = settle_field_names(wb, &mut pt, sheet, false);
    let out = compute(wb, &pt, sheet)?;
    let h = out.height().max(1);
    let w = out.width.max(1);
    let k = out.body_offset;
    let origin_row = if pt.anchor.row >= k { pt.anchor.row - k } else { pt.anchor.row };
    let origin = CellRef::new(origin_row, pt.anchor.col);
    let end_row = origin.row as u64 + h as u64 - 1;
    let end_col = origin.col as u64 + w as u64 - 1;
    if end_row >= MAX_ROWS as u64 || end_col >= MAX_COLS as u64 {
        return Err("The PivotTable report will not fit on the sheet. Choose a different location.".into());
    }
    let area = RangeRef::new(origin, CellRef::new(end_row as u32, end_col as u32));
    let sh = wb.sheet(sheet).ok_or("There's no such sheet.")?;
    for (j, other) in sh.pivots.iter().enumerate() {
        if j != idx && other.last_range.is_some_and(|r| r.intersects(&area)) {
            return Err("A PivotTable report cannot overlap another PivotTable report.".into());
        }
    }
    if sh.is_protected() {
        return Err("The cell or chart you're trying to change is on a protected sheet.".into());
    }
    if let Ok((ssi, sr)) = resolve_source(wb, &pt.source, sheet)
        && ssi == sheet
        && sr.intersects(&area)
    {
        return Err("The PivotTable report can't overlap its source data. Choose a different location.".into());
    }
    if sh.tables.iter().any(|t| t.range.intersects(&area)) {
        return Err("A PivotTable report cannot overlap a table.".into());
    }
    if !replace {
        for (c, cell) in sh.cells.iter_range(area) {
            if cell.has_content() && !pt.last_range.is_some_and(|r| r.contains(c)) {
                return Err(format!(
                    "There's already data in {}!{}. Do you want to replace it? (Run again with replace: true to overwrite.)",
                    quote_sheet(&sh.name),
                    c.a1()
                ));
            }
        }
        if let Some(c) = sh.spill.keys().find(|c| area.contains(**c) && !pt.last_range.is_some_and(|r| r.contains(**c))) {
            return Err(format!(
                "There's already data in {}!{}. Do you want to replace it? (Run again with replace: true to overwrite.)",
                quote_sheet(&sh.name),
                c.a1()
            ));
        }
    }
    // Styles first (the style table is workbook-wide).
    let lk = look(&pt.style);
    let header_last = out.row_kinds.iter().rposition(|k| *k == RowKind::Header);
    let mut cache: HashMap<Style, StyleId> = HashMap::new();
    let mut cells: Vec<(CellRef, Cell)> = Vec::new();
    for (ri, row) in out.grid.iter().enumerate() {
        let rk = out.row_kinds.get(ri).copied().unwrap_or(RowKind::Blank);
        if rk == RowKind::Blank {
            continue;
        }
        for (ci, oc) in row.iter().enumerate() {
            if rk == RowKind::Filter && oc.value.is_empty() {
                continue;
            }
            if rk == RowKind::Placeholder && oc.value.is_empty() {
                continue;
            }
            let flag = match rk {
                RowKind::Header => Some(ri) == header_last,
                RowKind::Filter => ci == 0,
                _ => false,
            };
            let st = cell_style(&lk, rk, oc, flag);
            let id = match cache.get(&st) {
                Some(id) => *id,
                None => {
                    let id = wb.styles.intern(st.clone());
                    cache.insert(st, id);
                    id
                }
            };
            if oc.value.is_empty() && id == StyleId::default() {
                continue;
            }
            cells.push((
                CellRef::new(origin.row + ri as u32, origin.col + ci as u32),
                Cell { value: oc.value.clone(), formula: None, style: id, py: None },
            ));
        }
    }
    // Column widths that fit the labels.
    let mut widths: HashMap<u32, f32> = HashMap::new();
    for (c, cell) in &cells {
        if cell.value.is_empty() {
            continue;
        }
        let st = wb.styles.get(cell.style);
        let text = crate::display::format(&cell.value, st.num_fmt.as_str(), wb).text;
        let wpt = crate::cmd::format::approx_text_width(&text, st.font.size, st.font.bold) + st.align.indent as f32 * 9.0 + 14.0;
        let e = widths.entry(c.col).or_insert(0.0);
        *e = e.max(wpt.min(320.0));
    }
    let shm = wb.sheet_mut(sheet).ok_or("There's no such sheet.")?;
    if let Some(old) = pt.last_range {
        shm.take_cells(old);
    }
    shm.take_cells(area);
    for (c, cell) in cells {
        shm.set_cell(c, cell);
    }
    for (col, wpt) in widths {
        if wpt > shm.col_width(col) && !shm.is_col_hidden(col) {
            shm.cols.entry(col).or_default().size = Some(wpt);
        }
    }
    if let Some(p) = shm.pivots.get_mut(idx) {
        p.last_range = Some(area);
        if migrated {
            p.rows = pt.rows.clone();
            p.columns = pt.columns.clone();
            p.values = pt.values.clone();
            p.filters = pt.filters.clone();
            p.names_from_values = true;
        }
    }
    let sheet_name = shm.name.clone();
    rebind_charts(wb, &sheet_name, &pt.name, origin, &out);
    // The index of the new report; its lookup hash is built on the first GETPIVOTDATA.
    if let Some(p) = wb.sheet_mut(sheet).and_then(|s| s.pivots.get_mut(idx)) {
        let index = PivotLookup::default();
        index.set_cells(out.data);
        p.data_index = index;
    }
    Ok(area)
}

/// The value cells of a PivotTable's report, for GETPIVOTDATA when a refresh did not set the
/// index (a workbook just opened from XLSX). The positions must describe the report as it is
/// stored on the sheet, not as the current source would render it: the source is recomputed only
/// to learn the layout, and the result is accepted when every label and header cell of that
/// layout equals the stored cell at the same place. A source changed since the last refresh gives
/// a different layout, so there is no trustworthy index and the lookup fails until a refresh.
pub fn index_cells(wb: &Workbook, sheet: usize, pt: &PivotTable) -> Result<Vec<PivotDataCell>, String> {
    const STALE: &str = "The stored report no longer matches the source. Refresh the PivotTable.";
    let report = pt.last_range.ok_or("The PivotTable has no report.")?;
    let out = compute(wb, pt, sheet)?;
    let sh = wb.sheet(sheet).ok_or("There's no such sheet.")?;
    let rows = u64::from(report.end.row) - u64::from(report.start.row) + 1;
    let cols = u64::from(report.end.col) - u64::from(report.start.col) + 1;
    if rows != u64::from(out.height().max(1)) || cols != u64::from(out.width.max(1)) {
        return Err(STALE.into());
    }
    let labels = [CellKind::Header, CellKind::RowLabel, CellKind::ColLabel, CellKind::Filter];
    for (r, row) in out.grid.iter().enumerate() {
        for (c, oc) in row.iter().enumerate() {
            if labels.contains(&oc.kind) && sh.value(CellRef::new(report.start.row + r as u32, report.start.col + c as u32)) != oc.value {
                return Err(STALE.into());
            }
        }
    }
    Ok(out.data)
}

#[cfg(test)]
mod fit_tests {
    use super::data_cols_exceed_sheet;

    #[test]
    fn column_budget_counts_value_fields_and_grand_total() {
        assert!(!data_cols_exceed_sheet(64, true, 128));
        assert!(data_cols_exceed_sheet(64, true, 256));
        assert!(!data_cols_exceed_sheet(64, false, 256));
        assert!(!data_cols_exceed_sheet(16_384, false, 0));
        assert!(data_cols_exceed_sheet(16_385, false, 0));
    }
}
