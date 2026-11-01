use super::output::Output;
use super::{CellRef, Chart, HashMap, HashSet, PivotTable, RangeRef, Series, Workbook, distinct_labels, quote_sheet, read_source, resolve_source};

// ---------------------------------------------------------------- PivotCharts

/// The link a PivotChart stores to its PivotTable: `Sheet2!PivotTable1`.
pub fn chart_link(sheet: &str, pivot: &str) -> String {
    format!("{}!{pivot}", quote_sheet(sheet))
}

/// (sheet, PivotTable name) of a PivotChart link; a leading `[Book.xlsx]` is ignored.
pub fn parse_chart_link(link: &str) -> Option<(String, String)> {
    let t = link.trim();
    let t = match t.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map(|(_, r)| r).unwrap_or(rest),
        None => t,
    };
    let (sheet, name) = t.rsplit_once('!')?;
    let sheet = sheet.trim();
    let sheet = gridcraft_core::unquote_sheet(sheet);
    let name = name.trim();
    if sheet.is_empty() || name.is_empty() {
        return None;
    }
    Some((sheet, name.to_string()))
}

/// Whether a PivotChart link points at `pivot` on `sheet`.
pub fn links_to(link: &str, sheet: &str, pivot: &str) -> bool {
    parse_chart_link(link).is_some_and(|(s, n)| s.eq_ignore_ascii_case(sheet) && n.eq_ignore_ascii_case(pivot))
}

/// An absolute reference to `col` over grid `rows` (contiguous runs joined into a union).
fn rows_ref(sheet: &str, origin: CellRef, col: u32, rows: &[u32]) -> Option<String> {
    let q = quote_sheet(sheet);
    let c = origin.col.checked_add(col)?;
    let letters = gridcraft_core::col_to_letters(c);
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(&start) = rows.get(i) {
        let mut end = start;
        while rows.get(i + 1).is_some_and(|n| Some(*n) == end.checked_add(1)) {
            end += 1;
            i += 1;
        }
        i += 1;
        let (a, b) = (origin.row.checked_add(start)? + 1, origin.row.checked_add(end)? + 1);
        parts.push(if a == b { format!("{q}!${letters}${a}") } else { format!("{q}!${letters}${a}:${letters}${b}") });
    }
    match parts.len() {
        0 => None,
        1 => parts.pop(),
        _ => Some(format!("({})", parts.join(","))),
    }
}

/// Most category rows a PivotChart plots.
const MAX_CHART_ROWS: usize = 10_000;

/// A PivotChart's series for a report written at `origin`: categories are the innermost row
/// items, one series per item column (column items × value fields); totals are left out.
pub fn chart_series(sheet: &str, origin: CellRef, out: &Output) -> Vec<Series> {
    chart_series_items(sheet, origin, out).into_iter().map(|(s, _)| s).collect()
}

fn chart_series_items<'a>(sheet: &str, origin: CellRef, out: &'a Output) -> Vec<(Series, &'a [String])> {
    let rows: Vec<u32> = out.chart_rows.iter().copied().take(MAX_CHART_ROWS).collect();
    let categories = out.label_col.and_then(|c| rows_ref(sheet, origin, c, &rows));
    out.chart_cols
        .iter()
        .filter_map(|(c, items)| {
            let name = items.join(" - ");
            Some((
                Series {
                    name: Some(format!("\"{}\"", name.replace('"', "\"\""))),
                    categories: categories.clone(),
                    values: rows_ref(sheet, origin, *c, &rows)?,
                    bubble_sizes: None,
                    color: None,
                    secondary: false,
                    kind: None,
                    quartiles: Default::default(),
                    smooth: false,
                    trendlines: Vec::new(),
                    label_number_format: None,
                },
                items.as_slice(),
            ))
        })
        .collect()
}

/// Every PivotChart of a PivotTable, in sheet and chart order, with the sheet it sits on.
fn linked_charts<'a>(wb: &'a Workbook, sheet: &'a str, pivot: &'a str) -> impl Iterator<Item = (usize, &'a Chart)> + 'a {
    wb.sheets
        .iter()
        .enumerate()
        .flat_map(move |(i, sh)| sh.charts.iter().filter(move |c| c.pivot.as_deref().is_some_and(|l| links_to(l, sheet, pivot))).map(move |c| (i, c)))
}

/// Most header cells read for one series name.
const MAX_NAME_CELLS: usize = 16;

/// Where a PivotTable's report was last written: its whole `area` and the `top` row of the
/// report proper (below any report filters), which holds the "Column Labels" caption.
#[derive(Clone, Copy)]
pub(super) struct OldReport {
    pub(super) area: RangeRef,
    pub(super) top: u32,
}

/// A PivotChart series' name: a literal, the joined items GridCraft writes (one item), or a
/// reference to the report's header cells (a PivotChart read from a file).
enum SeriesName {
    Literal(Vec<String>),
    Cells(usize, RangeRef),
}

fn parse_series_name(wb: &Workbook, chart_sheet: usize, name: &str) -> SeriesName {
    let literal = || SeriesName::Literal(vec![gridcraft_chart::series_name(wb, chart_sheet, name)].into_iter().filter(|s| !s.is_empty()).collect());
    // Only a sheet reference names header cells; a bare word is a literal name, as charts read it.
    let t = name.trim();
    let t = t.strip_prefix('=').unwrap_or(t);
    if t.starts_with('"') || !t.contains('!') {
        return literal();
    }
    match resolve_source(wb, t, chart_sheet) {
        Ok((si, r)) => SeriesName::Cells(si, r),
        Err(_) => literal(),
    }
}

/// The pivot items header cells `r` on sheet `si` name, each column top to bottom. Inside the old
/// `report` on `pivot_sheet`, Excel's names start at the report's top row, the "Column Labels" (or
/// colHeaderCaption) caption, which is skipped, and a blank cell takes the nearest label to its
/// left: an outer item's label is written only above its first column. That search stops at
/// `first_col` (the report's first data column, so it never reaches the row labels) on the
/// outermost row, and on an inner row at the column of the label found in the row above, so
/// the blank inner rows under a collapsed item stay blank.
fn header_items(wb: &Workbook, si: usize, r: RangeRef, pivot_sheet: usize, report: Option<OldReport>, first_col: u32) -> Vec<String> {
    let Some(sh) = wb.sheet(si) else { return Vec::new() };
    let text = |c: CellRef| {
        let cell = sh.cells.get(c).filter(|cell| !cell.value.is_empty())?;
        Some(crate::display::format(&cell.value, wb.styles.get(cell.style).num_fmt.as_str(), wb).text)
    };
    let inside = report.filter(|o| si == pivot_sheet && o.area.contains(r.start));
    let top = match inside {
        Some(o) if r.start.row == o.top && r.end.row > r.start.row => r.start.row + 1,
        _ => r.start.row,
    };
    let mut items = Vec::new();
    let cells = (r.start.col..=r.end.col).flat_map(|col| (top..=r.end.row).map(move |row| CellRef::new(row, col))).take(MAX_NAME_CELLS);
    let mut bound = r.start.col;
    for c in cells {
        if c.row == top {
            bound = if inside.is_some() { first_col.min(c.col) } else { c.col };
        }
        if let Some((col, label)) = (bound..=c.col).rev().find_map(|col| text(CellRef::new(c.row, col)).map(|t| (col, t))) {
            bound = col;
            items.push(label);
        }
    }
    items
}

/// Whether a series `key` names the same pivot items as the rebuilt `items`: item by item, as
/// GridCraft's joined name, or, when `value_item` (the last of `items` is a value field's name,
/// which Excel can show above the column items), with that name at another level and the
/// column items in the same order.
pub(super) fn joined_name_match(key: &[String], items: &[String]) -> bool {
    key != items && key.join(" - ") == items.join(" - ")
}

pub(super) fn value_item_match(key: &[String], items: &[String], value_item: bool) -> bool {
    let Some((value, columns)) = items.split_last().filter(|_| value_item && key.len() == items.len()) else { return false };
    key.iter().enumerate().any(|(p, k)| k == value && key.iter().enumerate().filter(|(i, _)| *i != p).map(|(_, x)| x).eq(columns))
}

/// Whether a series `key` names the same pivot items as the rebuilt `items`: item by item, as
/// GridCraft's joined name, or, when `value_item` (the last of `items` is a value field's name,
/// which Excel can show above the column items), with that name at another level and the
/// column items in the same order.
pub(super) fn same_items(key: &[String], items: &[String], value_item: bool) -> bool {
    key == items || value_item_match(key, items, value_item) || joined_name_match(key, items)
}

/// Number of rebuilt item sequences matching a saved key under joined-name matching.
pub(super) fn joined_match_count(key: &[String], rebuilt: &[&[String]]) -> usize {
    rebuilt.iter().filter(|items| joined_name_match(key, items)).count()
}

pub(super) fn rebuilt_item_set<'a>(items: impl IntoIterator<Item = &'a [String]>) -> HashSet<&'a [String]> {
    items.into_iter().collect()
}

/// One PivotChart's data label formats, read before its report is rewritten.
#[derive(Default)]
pub(super) struct ChartFormats {
    /// Every current series' items with its typed format (`None` without one), then the formats
    /// an earlier refresh remembered for series it dropped.
    keyed: Vec<(Vec<String>, Option<String>)>,
    /// How many of `keyed` are the current series'.
    current: usize,
    /// The current series' formats in order when every series is named by header cells (a
    /// PivotChart read from a file); see [`rebind_charts`] for when they are kept by position.
    by_position: Option<Vec<Option<String>>>,
}

/// The items of the pivot's row and column fields, counting the ones a filter hides.
fn pivot_items(wb: &Workbook, pt: &PivotTable, pivot_sheet: usize) -> HashSet<String> {
    let mut labels = HashSet::new();
    if let Ok(src) = read_source(wb, &pt.source, pivot_sheet) {
        for f in pt.rows.iter().chain(&pt.columns) {
            if let Some(c) = src.col(&f.source_col) {
                labels.extend(distinct_labels(wb, &src, c, f.date_group, usize::MAX));
            }
        }
    }
    labels
}

/// The data label formats of every PivotChart of a PivotTable by series items (names that are
/// references are read from the old `report`, so call this before the report is rewritten).
/// Excel ties a PivotChart series' formatting to its pivot items, not to its position, so a
/// filter that hides a leading item doesn't pass its format to the next series.
pub(super) fn chart_label_formats(
    wb: &Workbook,
    pt: &PivotTable,
    sheet: &str,
    pivot_sheet: usize,
    report: Option<OldReport>,
    value_item: bool,
) -> Vec<ChartFormats> {
    linked_charts(wb, sheet, &pt.name)
        .map(|(si, ch)| {
            let names: Vec<Option<SeriesName>> = ch.series.iter().map(|s| s.name.as_deref().map(|n| parse_series_name(wb, si, n))).collect();
            // The report's first data column: the leftmost series' header column.
            let first_col = names
                .iter()
                .flatten()
                .filter_map(|n| match n {
                    SeriesName::Cells(s, r) if *s == pivot_sheet && report.is_some_and(|o| o.area.contains(r.start)) => Some(r.start.col),
                    _ => None,
                })
                .min()
                .unwrap_or(0);
            let current: Vec<(Vec<String>, Option<String>)> = names
                .iter()
                .zip(&ch.series)
                .filter_map(|(n, s)| {
                    let items = match n.as_ref()? {
                        SeriesName::Literal(items) => items.clone(),
                        SeriesName::Cells(csi, r) => header_items(wb, *csi, *r, pivot_sheet, report, first_col),
                    };
                    (!items.is_empty()).then(|| (items, s.label_number_format.clone()))
                })
                .collect();
            let by_position = names
                .iter()
                .all(|n| matches!(n, Some(SeriesName::Cells(..))))
                .then(|| ch.series.iter().map(|s| s.label_number_format.clone()).collect());
            let dropped: Vec<(Vec<String>, Option<String>)> = ch
                .pivot_label_formats
                .iter()
                .filter(|(k, _)| !current.iter().any(|(c, _)| same_items(c, k, value_item)))
                .map(|(k, f)| (k.clone(), Some(f.clone())))
                .collect();
            let current_len = current.len();
            ChartFormats { keyed: current.into_iter().chain(dropped).collect(), current: current_len, by_position }
        })
        .collect()
}

/// Rebuilds the series of every PivotChart of a PivotTable after its report was written; each
/// series takes the label format `label_formats` (from [`chart_label_formats`]) has for its
/// items. An item-by-item match comes first; a looser [`same_items`] match only uses a key that
/// no rebuilt series has item by item, so two series with the same labels in another order keep
/// their own formats. Series named by header cells keep their formats by position when no name
/// matches, the series count is unchanged, and no name shows any item of the pivot's row or
/// column fields (hidden ones included) or is only value field names: captions that aren't
/// GridCraft's item labels. Names that show the pivot's items are matched by item only, since a
/// filter can swap the shown items and keep their count.
pub(super) fn rebind_charts(
    wb: &mut Workbook,
    pt: &PivotTable,
    sheet: &str,
    pivot_sheet: usize,
    origin: CellRef,
    out: &Output,
    label_formats: Vec<ChartFormats>,
) {
    let linked = |ch: &Chart| ch.pivot.as_deref().is_some_and(|l| links_to(l, sheet, &pt.name));
    let hits: Vec<usize> = (0..wb.sheets.len()).filter(|i| wb.sheet(*i).is_some_and(|sh| sh.charts.iter().any(linked))).collect();
    if hits.is_empty() {
        return;
    }
    let value_item = out.chart_value_item;
    let (series, items): (Vec<Series>, Vec<&[String]>) = chart_series_items(sheet, origin, out).into_iter().unzip();
    let rebuilt_set = rebuilt_item_set(items.iter().copied());
    let rebuilt = |k: &[String]| rebuilt_set.contains(k);
    let mut joined_counts: HashMap<Vec<String>, usize> = HashMap::new();
    for fmts in &label_formats {
        for (key, _) in &fmts.keyed {
            joined_counts.entry(key.clone()).or_insert_with(|| joined_match_count(key, &items));
        }
    }
    let mut pivot_labels: Option<HashSet<String>> = None;
    // Each chart's formats for the rebuilt series, and whether they go by position.
    let plans: Vec<(ChartFormats, Vec<Option<String>>, bool)> = label_formats
        .into_iter()
        .map(|fmts| {
            let matched: Vec<Option<&Option<String>>> = items
                .iter()
                .map(|it| {
                    let exact = fmts.keyed.iter().find(|(k, _)| k.as_slice() == *it);
                    exact
                        .or_else(|| {
                            fmts.keyed.iter().find(|(k, _)| {
                                !rebuilt(k)
                                    && (value_item_match(k, it, value_item) || joined_name_match(k, it) && joined_counts.get(k).copied() == Some(1))
                            })
                        })
                        .map(|(_, f)| f)
                })
                .collect();
            let by_position = fmts.by_position.as_ref().is_some_and(|p| p.len() == items.len() && matched.iter().all(Option::is_none)) && {
                let labels = pivot_labels.get_or_insert_with(|| pivot_items(wb, pt, pivot_sheet));
                let value_name = |x: &String| pt.values.iter().any(|v| v.name == *x);
                !fmts.keyed.iter().take(fmts.current).any(|(k, _)| k.iter().any(|x| labels.contains(x)) || k.iter().all(value_name))
            };
            let f = match (by_position, &fmts.by_position) {
                (true, Some(p)) => p.clone(),
                _ => matched.into_iter().map(|f| f.cloned().flatten()).collect(),
            };
            (fmts, f, by_position)
        })
        .collect();
    let mut plans = plans.into_iter();
    for i in hits {
        let Some(sh) = wb.sheet_mut(i) else { continue };
        for ch in sh.charts.iter_mut().filter(|c| linked(c)) {
            let (fmts, f, by_position) = plans.next().unwrap_or_default();
            ch.series = series.clone();
            for (new, f) in ch.series.iter_mut().zip(f) {
                new.label_number_format = f;
            }
            // Remembered under the rebuilt items, so the next refresh matches item by item, with
            // the formats of series that are gone; after a fallback by position, not under the
            // captions the formats were found by.
            let mut kept: Vec<(Vec<String>, String)> =
                items.iter().zip(&ch.series).filter_map(|(k, s)| Some((k.to_vec(), s.label_number_format.clone()?))).collect();
            let skip = if by_position { fmts.current } else { 0 };
            kept.extend(
                fmts.keyed
                    .into_iter()
                    .skip(skip)
                    .filter_map(|(k, f)| Some((k, f?)))
                    .filter(|(k, _)| !items.iter().any(|i| same_items(k, i, value_item))),
            );
            ch.pivot_label_formats = kept;
        }
    }
}

/// Clears a pivot's last output area.
pub fn clear_output(wb: &mut Workbook, sheet: usize, idx: usize) {
    let Some(shm) = wb.sheet_mut(sheet) else { return };
    let Some(r) = shm.pivots.get(idx).and_then(|p| p.last_range) else { return };
    shm.take_cells(r);
    if let Some(p) = shm.pivots.get_mut(idx) {
        p.last_range = None;
    }
}
