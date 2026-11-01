//! Pulls chart data (names, categories, values, formats, colours) out of a workbook.

use gridcraft_calc::evaluate;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value, category_levels, number_to_text};
use gridcraft_model::{Chart, ChartKind, Theme, Workbook, style::apply_tint};
use gridcraft_numfmt::{NumberFormat, format_value};

use crate::{ChartData, Rgba, SeriesData, TrendlineData};

/// Cap on points per series (keeps whole-column references from exploding).
pub(crate) const MAX_POINTS: usize = 100_000;
/// Cap on series per chart.
pub(crate) const MAX_SERIES: usize = 255;
/// Cap on trendlines per chart (not per series: each one is a fit over a series' points).
pub(crate) const MAX_TRENDLINES: usize = 16;
/// Budget for series x points in one chart. When a chart would hold more, every series (and the
/// category list) keeps its first `MAX_TOTAL_POINTS / series` points; no series is dropped.
pub(crate) const MAX_TOTAL_POINTS: usize = 2_000_000;

/// Points kept per series (and categories) in a chart of `series` series: `MAX_POINTS`, or less
/// when `series x MAX_POINTS` would pass `MAX_TOTAL_POINTS`.
pub(crate) fn points_per_series(series: usize) -> usize {
    (MAX_TOTAL_POINTS / series.clamp(1, MAX_SERIES)).clamp(1, MAX_POINTS)
}

/// Tints used for series beyond the six theme accents: lighter, darker, lighter still…
const CYCLE_TINTS: [f64; 6] = [0.0, 0.4, -0.25, 0.6, -0.5, 0.2];
/// `series_color` repeats after this many colours (six accents × the tints).
pub(crate) const PALETTE_CYCLE: usize = 6 * CYCLE_TINTS.len();

/// Colour of the `i`-th series: theme accents 1–6, then tinted variations of them.
pub fn series_color(theme: &Theme, i: usize) -> Rgba {
    let accent = i % 6;
    let cycle = (i / 6) % CYCLE_TINTS.len();
    let base = theme.colors.get(4 + accent).copied().unwrap_or(0x156082);
    let rgb = [(base >> 16) as u8, (base >> 8) as u8, base as u8];
    let tint = CYCLE_TINTS.get(cycle).copied().unwrap_or(0.0);
    let [r, g, b] = apply_tint(rgb, tint);
    [r, g, b, 0xFF]
}

/// The first `n` series colours for `theme`.
pub fn default_palette(theme: &Theme, n: usize) -> Vec<Rgba> {
    (0..n.min(4096)).map(|i| series_color(theme, i)).collect()
}

/// Splits `Sheet!A1:B2` / `'My Sheet'!$A$1` / `A1:B2` into a sheet index and range.
fn parse_ref(wb: &Workbook, sheet: usize, formula: &str) -> Option<(usize, RangeRef)> {
    let f = formula.trim().trim_start_matches('=').trim();
    let (sh, addr) = match f.rfind('!') {
        Some(i) => {
            let name = f.get(..i)?.trim();
            let name = gridcraft_core::unquote_sheet(name);
            (wb.sheet_index(&name)?, f.get(i + 1..)?)
        }
        None => (sheet, f),
    };
    let r = RangeRef::parse(addr)?;
    Some((sh, r))
}

/// The parts of a union reference `(Sheet1!$A$1:$A$3,Sheet1!$A$5)`, or `None`.
fn union_parts(f: &str) -> Option<Vec<&str>> {
    let inner = f.strip_prefix('(')?.strip_suffix(')')?;
    let mut parts = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, false, 0usize);
    for (i, c) in inner.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                parts.push(inner.get(start..i)?);
                start = i + 1;
            }
            _ => {}
        }
        if depth < 0 {
            return None;
        }
    }
    parts.push(inner.get(start..)?);
    (parts.len() > 1).then_some(parts)
}

/// Evaluates a series formula to a flat list of values (row-major), capped. A union of
/// references (`(A1:A3,A5)`, as PivotCharts use) is read part by part.
fn eval_list(wb: &Workbook, sheet: usize, formula: &str) -> Vec<Value> {
    eval_list_in(wb, sheet, formula, MAX_POINTS, 0)
}

/// calc reads a reference of up to this many cells at its full size and trims a larger one to the
/// sheet's used range (`FULL_SIZE_CELLS` in `gridcraft_calc::eval`). A clipped reference must stay
/// above it, or the clip would turn a trimmed read into a full-size one with trailing blanks.
const CALC_FULL_SIZE_CELLS: u64 = 16_384;
/// Bounded references up to this many cells are read at their full size too
/// (`FULL_SIZE_BOUNDED_CELLS` in `gridcraft_calc::eval`).
const CALC_FULL_SIZE_BOUNDED_CELLS: u64 = 1_048_576;

/// `formula` as a plain reference (`Sheet!A1:A1000000`) cut after the rows that can still hold
/// one of the first `limit` points, so that calc never reads the rest; `None` when `formula` is
/// no reference or already small enough. Values are read row by row, and calc trims only the
/// bottom and right of a reference, so keeping `limit` rows keeps the first `limit` values.
fn clipped_reference(wb: &Workbook, sheet: usize, formula: &str, limit: usize) -> Option<String> {
    let (sh, mut r) = parse_ref(wb, sheet, formula)?;
    let cells = u64::from(r.height()).saturating_mul(u64::from(r.width()));
    // calc trims a block that runs to the sheet's edge, and any block over a column's worth, to
    // the used range. Do it here, or the clipped block below would be read at its full size.
    let to_edge = r.end.row == MAX_ROWS - 1 || r.end.col == MAX_COLS - 1;
    let trimmed = cells > CALC_FULL_SIZE_CELLS && (to_edge || cells > CALC_FULL_SIZE_BOUNDED_CELLS);
    let mut changed = false;
    if trimmed {
        let end = match wb.sheet(sh).and_then(|s| s.used_range()) {
            Some(u) => CellRef::new(r.end.row.min(u.end.row.max(r.start.row)), r.end.col.min(u.end.col.max(r.start.col))),
            None => r.start,
        };
        changed = end != r.end;
        r = RangeRef::new(r.start, end);
    }
    let prefix = formula.rfind('!').and_then(|i| formula.get(..=i)).unwrap_or("");
    let (h, w) = (u64::from(r.height()), u64::from(r.width()));
    if h.saturating_mul(w) > CALC_FULL_SIZE_CELLS {
        let keep = (limit as u64).max(CALC_FULL_SIZE_CELLS / w + 1);
        if h > keep {
            let end = CellRef::new(r.start.row.saturating_add(u32::try_from(keep).ok()?).saturating_sub(1), r.end.col);
            r = RangeRef::new(r.start, end);
            changed = true;
        }
    }
    changed.then(|| format!("{prefix}{r}"))
}

fn eval_list_in(wb: &Workbook, sheet: usize, formula: &str, limit: usize, depth: u32) -> Vec<Value> {
    let f = formula.trim();
    if f.is_empty() {
        return vec![];
    }
    if depth < 4
        && let Some(parts) = union_parts(f)
    {
        let mut out = Vec::new();
        for p in parts {
            if out.len() >= limit {
                break;
            }
            let rest = limit - out.len();
            out.extend(eval_list_in(wb, sheet, p, rest, depth + 1).into_iter().take(rest));
        }
        return out;
    }
    let clipped = clipped_reference(wb, sheet, f, limit);
    match evaluate(wb, sheet, CellRef::default(), clipped.as_deref().unwrap_or(f)) {
        Value::Array(a) => a.iter().take(limit).cloned().collect(),
        v => vec![v],
    }
}

fn to_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) if n.is_finite() => Some(*n),
        _ => None,
    }
}

fn nums(wb: &Workbook, sheet: usize, formula: &str, limit: usize) -> Vec<Option<f64>> {
    eval_list_in(wb, sheet, formula, limit, 0).iter().map(to_num).collect()
}

/// Number format code of the first cell of a reference ("General" when not a reference).
fn ref_format(wb: &Workbook, sheet: usize, formula: &str) -> String {
    parse_ref(wb, sheet, formula)
        .and_then(|(sh, r)| wb.sheet(sh).map(|s| wb.styles.get(s.style_id(r.start)).num_fmt.0.to_string()))
        .unwrap_or_else(|| "General".into())
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Empty => String::new(),
        Value::Number(n) => number_to_text(*n),
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(e) => e.as_str().into(),
        Value::Array(a) => a.get(0, 0).map(value_text).unwrap_or_default(),
    }
}

/// Category labels, formatted with each source cell's number format (dates, currency…), and
/// the outer levels of a multi-level axis: a category range wider than a column of values (or
/// taller than a row of values) groups its innermost labels, year beside month. Each outer level
/// has one entry per category, non-empty where a group starts, as the cells are.
fn category_table(wb: &Workbook, sheet: usize, formula: &str, values: &str, limit: usize) -> (Vec<String>, Vec<Vec<String>>) {
    let vals = eval_list_in(wb, sheet, formula, limit, 0);
    let cells: Option<(usize, RangeRef)> = parse_ref(wb, sheet, formula);
    let src = cells.and_then(|(sh, r)| wb.sheet(sh).map(|s| (s, r)));
    let texts: Vec<String> = vals
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if let (Some((s, r)), Value::Number(_)) = (src, v) {
                let w = r.width().max(1) as usize;
                let at = r.start.offset((i / w) as i64, (i % w) as i64);
                if let Some(c) = at {
                    let code = &wb.styles.get(s.style_id(c)).num_fmt.0;
                    if !code.eq_ignore_ascii_case("general") {
                        return format_value(v, &NumberFormat::parse_cached(code), wb.date_system).text;
                    }
                }
            }
            value_text(v)
        })
        .collect();
    let Some((_, r)) = src else { return (texts, vec![]) };
    let value_range = parse_ref(wb, sheet, values).map(|(_, r)| r);
    let Some(levels) = category_levels(&r, value_range.as_ref()) else { return (texts, vec![]) };
    let (w, h) = (r.width().max(1) as usize, r.height().max(1) as usize);
    if texts.len() != w.saturating_mul(h) {
        return (texts, vec![]); // truncated or not a plain block: a flat list
    }
    let at = |row: usize, col: usize| texts.get(row * w + col).cloned().unwrap_or_default();
    let column = |c: usize| (0..h).map(|row| at(row, c)).collect::<Vec<String>>();
    let row = |rr: usize| (0..w).map(|c| at(rr, c)).collect::<Vec<String>>();
    let outer = levels.outer as usize;
    if levels.by_columns {
        (column(w - 1), (1..=outer).filter_map(|k| (w - 1).checked_sub(k)).map(column).collect())
    } else {
        (row(h - 1), (1..=outer).filter_map(|k| (h - 1).checked_sub(k)).map(row).collect())
    }
}

/// Whether `t` names a defined name seen from `sheet` (`Name`, or `Sheet1!Name` looked up on
/// that sheet), the way the evaluator looks names up.
fn is_defined_name(wb: &Workbook, sheet: usize, t: &str) -> bool {
    match t.split_once('!') {
        Some((s, bare)) => wb.sheet_index(s.trim_matches('\'')).is_some_and(|i| wb.name(bare, i).is_some()),
        None => wb.name(t, sheet).is_some(),
    }
}

/// A series name: a reference/formula or a defined name (`[0]!Title` in a file) is evaluated,
/// a quoted or other bare literal is used as is.
fn series_name(wb: &Workbook, sheet: usize, name: &str) -> String {
    let t = name.trim();
    if let Some(q) = t.strip_prefix('"').and_then(|q| q.strip_suffix('"')) {
        return q.replace("\"\"", "\"");
    }
    if t.starts_with('=') || parse_ref(wb, sheet, t).is_some() && t.contains('!') || is_defined_name(wb, sheet, t) {
        let vals = eval_list(wb, sheet, t);
        let parts: Vec<String> = vals.iter().map(value_text).filter(|s| !s.is_empty()).take(8).collect();
        return parts.join(" ");
    }
    t.to_string()
}

fn effective_kind(chart_kind: ChartKind, series_kind: Option<ChartKind>, i: usize) -> ChartKind {
    match (chart_kind, series_kind) {
        (ChartKind::Combo, Some(k)) if k != ChartKind::Combo => k,
        (ChartKind::Combo, _) => {
            if i == 0 {
                ChartKind::ColumnClustered
            } else {
                ChartKind::Line
            }
        }
        (_, Some(k)) if k != ChartKind::Combo => k,
        (k, _) => k,
    }
}

/// The kind series `i` of `chart` is drawn as: its own kind in a combo chart, else the chart's.
pub fn series_plot_kind(chart: &Chart, i: usize) -> ChartKind {
    if chart.kind == ChartKind::Combo { effective_kind(chart.kind, chart.series.get(i).and_then(|s| s.kind), i) } else { chart.kind }
}

/// Evaluates a chart's series formulas: names, values, categories (default "1", "2", …), X
/// values (scatter/bubble), bubble sizes, number formats and colours (explicit, else the
/// default palette derived from the workbook theme accents).
pub fn resolve(wb: &Workbook, sheet: usize, chart: &Chart) -> ChartData {
    let numeric_x = matches!(chart.kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble);
    let mut series = Vec::new();
    let mut categories: Option<Vec<String>> = None;
    let limit = points_per_series(chart.series.len());
    let mut trend_left = MAX_TRENDLINES;
    let mut category_levels = Vec::new();
    for (i, s) in chart.series.iter().take(MAX_SERIES).enumerate() {
        let values = nums(wb, sheet, &s.values, limit);
        let name = s.name.as_deref().map(|n| series_name(wb, sheet, n)).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Series{}", i + 1));
        let color =
            s.color.as_ref().and_then(|c| c.resolve(&wb.theme)).map(|[r, g, b]| [r, g, b, 0xFF]).unwrap_or_else(|| series_color(&wb.theme, i));
        let kind = effective_kind(chart.kind, s.kind, i);
        let x = match (&s.categories, numeric_x || matches!(kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble)) {
            // X values with text in them (labels) are ignored: the points are numbered 1..n.
            (Some(f), true) => {
                Some(eval_list_in(wb, sheet, f, limit, 0)).filter(|xs| !xs.iter().any(Value::is_text)).map(|xs| xs.iter().map(to_num).collect())
            }
            _ => None,
        };
        if categories.is_none()
            && let Some(f) = &s.categories
        {
            let (mut c, outer) = category_table(wb, sheet, f, &s.values, limit);
            c.truncate(limit);
            if !c.is_empty() {
                categories = Some(c);
                category_levels = outer;
            }
        }
        let sizes = s.bubble_sizes.as_deref().map(|f| nums(wb, sheet, f, limit));
        let trendlines = s
            .trendlines
            .iter()
            .take(trend_left)
            .map(|t| TrendlineData {
                spec: t.clone(),
                color: t.color.as_ref().and_then(|c| c.resolve(&wb.theme)).map(|[r, g, b]| [r, g, b, 0xFF]).unwrap_or(color),
            })
            .collect::<Vec<_>>();
        trend_left = trend_left.saturating_sub(trendlines.len());
        series.push(SeriesData {
            name,
            values,
            x,
            sizes,
            color,
            kind,
            secondary: s.secondary,
            smooth: s.smooth,
            trendlines,
            number_format: ref_format(wb, sheet, &s.values),
        });
    }
    let n = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    let categories = categories.unwrap_or_else(|| (1..=n).map(|i| i.to_string()).collect());
    let palette = default_palette(&wb.theme, n.min(PALETTE_CYCLE));
    ChartData { categories, category_levels, series, palette }
}

#[cfg(test)]
mod tests {
    use gridcraft_model::Workbook;

    use super::*;

    fn book(rows: u32, cols: u32) -> Workbook {
        let mut wb = Workbook::new();
        if let Some(s) = wb.sheet_mut(0) {
            s.name = "My Sheet".into();
            for r in 0..rows {
                for c in 0..cols {
                    s.set_value(CellRef::new(r, c), Value::Number(f64::from(r * 10 + c)));
                }
            }
        }
        wb
    }

    #[test]
    fn a_long_reference_is_clipped_before_it_is_evaluated() {
        let wb = book(120_000, 3);
        let rows = |f: &str, limit| {
            let g = clipped_reference(&wb, 0, f, limit)?;
            let (_, r) = parse_ref(&wb, 0, &g)?;
            Some((g, u64::from(r.height())))
        };
        // 1,000,000 rows: at most `MAX_POINTS` of them are asked for.
        assert_eq!(rows("'My Sheet'!$A$1:$A$1000000", MAX_POINTS), Some(("'My Sheet'!A1:A100000".to_string(), 100_000)));
        assert_eq!(rows("A5:A1048576", MAX_POINTS).map(|r| r.1), Some(100_000));
        assert_eq!(rows("A:A", 50_000).map(|r| r.1), Some(50_000));
        // Never below what keeps calc trimming the clipped reference like the whole one.
        assert_eq!(rows("A1:A1000000", 100).map(|r| r.1), Some(16_385));
        // Wide references keep whole rows.
        assert_eq!(rows("A1:C1000000", MAX_POINTS).map(|r| r.1), Some(100_000));
        // Small references, single cells and anything that isn't a plain reference are left alone.
        assert_eq!(rows("A1:A9000", MAX_POINTS), None);
        assert_eq!(rows("A1:A20000", MAX_POINTS), None);
        assert_eq!(rows("B2", MAX_POINTS), None);
        assert_eq!(rows("SUM(A1:A1000000)", MAX_POINTS), None);
        assert_eq!(rows("{1,2,3}", MAX_POINTS), None);
    }

    #[test]
    fn clipping_does_not_change_the_points() {
        for (rows, cols) in [(10, 1), (30_000, 1), (30_000, 2), (3, 4)] {
            let wb = book(rows, cols);
            for f in ["'My Sheet'!A1:A1000000", "'My Sheet'!$A$3:$B$1048576", "A:A", "A1:C1000000", "(A1:A1000000,B1:B1000000)", "A2:A20000"] {
                for limit in [100, 7_843, 25_000, MAX_POINTS] {
                    let clipped = eval_list_in(&wb, 0, f, limit, 0);
                    let whole: Vec<Value> = match evaluate(&wb, 0, CellRef::default(), f) {
                        Value::Array(a) => a.iter().take(limit).cloned().collect(),
                        v => vec![v],
                    };
                    if f.starts_with('(') {
                        // A union is read part by part; each part keeps its own order.
                        assert!(clipped.len() <= limit);
                        continue;
                    }
                    assert_eq!(clipped, whole, "{f} limit {limit} on {rows}x{cols}");
                }
            }
        }
    }
}
