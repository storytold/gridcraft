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
    let src = read_source(wb, &pt.source, pivot_sheet)?;
    compute_from(wb, pt, &src, None)
}

/// Brings the PivotTables an older GridCraft saved to how fields are named now, once, as a
/// workbook is opened (after its formulas have values). Every reader of a PivotTable (the report,
/// edits, the field list, Show Details, GETPIVOTDATA, file writers) then sees one set of names.
/// A PivotTable is left for a later open only when its source can't be read now.
pub fn migrate_older_names(wb: &mut Workbook) {
    let mut updates: Vec<(usize, usize, PivotTable)> = Vec::new();
    for (si, sh) in wb.sheets.iter().enumerate() {
        for (pi, pt) in sh.pivots.iter().enumerate() {
            if !pt.names_from_values
                && let Some(new) = migrated(wb, pt, si)
            {
                updates.push((si, pi, new));
            }
        }
    }
    for (si, pi, new) in updates {
        if let Some(slot) = wb.sheet_mut(si).and_then(|sh| sh.pivots.get_mut(pi)) {
            *slot = new;
        }
    }
}

/// A PivotTable saved by an older GridCraft, with its fields named by header value; `None` when
/// its source can't be read.
///
/// Older GridCraft named a column by its header as the cell showed it: the formula under Show
/// Formulas (`="Region"`), a long General number narrowed to the cell width. Now a header is its
/// value through its number format. The saved names are read against the headers as the sheet
/// showed them, both ways (with and without Show Formulas), by one rule for each name:
/// - the layout that places more of the saved fields is read first, the sheet's saved Show
///   Formulas setting on a tie;
/// - a name it places takes that column;
/// - a name only the other layout places takes that layout's column, unless a field the first
///   layout placed already has it (then two fields would read one column);
/// - a name neither places is left as it is.
///
/// Each field then takes the current name of its column, and so does each field a calculated
/// formula names. A calculated field whose name is now also a header is renamed (`Margin2`),
/// with the values that show it, as Excel doesn't allow the two to share a name.
fn migrated(wb: &Workbook, pt: &PivotTable, pivot_sheet: usize) -> Option<PivotTable> {
    use gridcraft_model::pivot_source::{headers, unique_names};
    let (si, r) = resolve_source(wb, &pt.source, pivot_sheet).ok()?;
    let sh = wb.sheet(si)?;
    let current = headers(wb, si, r).ok()?;
    let shown = |formulas: bool| {
        unique_names((r.start.col..=r.end.col).map(|c| crate::display::cell_text_with(wb, sh, CellRef::new(r.start.row, c), formulas))).ok()
    };
    let calc_name = |n: &str| pt.calculated_fields.iter().any(|c| c.name.trim().eq_ignore_ascii_case(n.trim()));
    // A value names a source column before a calculated field, as the report reads it.
    let fields: Vec<&str> = pt
        .rows
        .iter()
        .chain(&pt.columns)
        .map(|f| f.source_col.as_str())
        .chain(pt.filters.iter().map(|f| f.source_col.as_str()))
        .chain(pt.values.iter().map(|v| v.source_col.as_str()).filter(|n| !calc_name(n)))
        .collect();
    let saved_way = sh.show_formulas;
    let lists = [shown(saved_way), shown(!saved_way)];
    let pos = |l: &Option<Vec<String>>, n: &str| l.as_ref()?.iter().position(|h| h.trim().eq_ignore_ascii_case(n.trim()));
    let placed = |l: &Option<Vec<String>>| fields.iter().filter(|n| pos(l, n).is_some()).count();
    let [saved_list, other_list] = &lists;
    let (first, second) = if placed(other_list) > placed(saved_list) { (other_list, saved_list) } else { (saved_list, other_list) };
    // Columns the first layout gives the saved fields: the second layout can't give them again.
    let claimed: Vec<usize> = fields.iter().filter_map(|n| pos(first, n)).collect();

    // Calculated fields keep their own names unless a header now has one of them.
    let mut calc_names: Vec<String> = Vec::new();
    for c in &pt.calculated_fields {
        let taken = |n: &str| current.iter().chain(&calc_names).any(|h| h.eq_ignore_ascii_case(n));
        let base = c.name.trim();
        let mut name = base.to_string();
        let mut k = 2;
        while taken(&name) {
            name = format!("{base}{k}");
            k += 1;
        }
        calc_names.push(name);
    }
    // A saved name's new name: the column it named, else the calculated field it named.
    let column = |n: &str| -> Option<String> {
        let i = pos(first, n).or_else(|| pos(second, n).filter(|i| !claimed.contains(i)))?;
        current.get(i).cloned()
    };
    let calculated = |n: &str| -> Option<String> {
        let i = pt.calculated_fields.iter().position(|c| c.name.trim().eq_ignore_ascii_case(n.trim()))?;
        calc_names.get(i).cloned()
    };
    let mut new = pt.clone();
    for f in new.rows.iter_mut().chain(new.columns.iter_mut()) {
        if let Some(n) = column(&f.source_col) {
            f.source_col = n;
        }
    }
    for f in &mut new.filters {
        if let Some(n) = column(&f.source_col) {
            f.source_col = n;
        }
    }
    for v in &mut new.values {
        if let Some(n) = column(&v.source_col).or_else(|| calculated(&v.source_col)) {
            v.source_col = n;
        }
    }
    let rename = |n: &str| column(n).or_else(|| calculated(n));
    for (c, name) in new.calculated_fields.iter_mut().zip(&calc_names) {
        if let Some(f) = crate::pivot_calc::rename_fields(&c.formula, &rename) {
            c.formula = f;
        }
        c.name = name.clone();
    }
    new.names_from_values = true;
    Some(new)
}

/// Recomputes a pivot and writes its report. With `replace` false, refuses when the target area
/// holds other data. Returns the written area.
pub fn refresh(wb: &mut Workbook, sheet: usize, idx: usize, replace: bool) -> Result<RangeRef, String> {
    let pt = wb.sheet(sheet).and_then(|s| s.pivots.get(idx)).cloned().ok_or("There's no such PivotTable.")?;
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
