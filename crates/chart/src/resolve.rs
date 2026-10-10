//! Pulls chart data (names, categories, values, formats, colours) out of a workbook.

use gridcraft_calc::evaluate;
use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Chart, ChartKind, Theme, Workbook, style::apply_tint};
use gridcraft_numfmt::{NumberFormat, format_value_in};

use crate::{ChartData, Rgba, SeriesData};

/// Cap on points per series (keeps whole-column references from exploding).
pub(crate) const MAX_POINTS: usize = 100_000;
/// Cap on series per chart.
pub(crate) const MAX_SERIES: usize = 255;

/// Tints used for series beyond the six theme accents: lighter, darker, lighter still…
const CYCLE_TINTS: [f64; 6] = [0.0, 0.4, -0.25, 0.6, -0.5, 0.2];

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
            let name = name.strip_prefix('\'').and_then(|n| n.strip_suffix('\'')).map(|n| n.replace("''", "'")).unwrap_or_else(|| name.to_string());
            (wb.sheet_index(&name)?, f.get(i + 1..)?)
        }
        None => (sheet, f),
    };
    let r = RangeRef::parse(addr)?;
    Some((sh, r))
}

/// Evaluates a series formula to a flat list of values (row-major), capped.
fn eval_list(wb: &Workbook, sheet: usize, formula: &str) -> Vec<Value> {
    let f = formula.trim();
    if f.is_empty() {
        return vec![];
    }
    match evaluate(wb, sheet, CellRef::default(), f) {
        Value::Array(a) => a.iter().take(MAX_POINTS).cloned().collect(),
        v => vec![v],
    }
}

fn to_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) if n.is_finite() => Some(*n),
        _ => None,
    }
}

fn nums(wb: &Workbook, sheet: usize, formula: &str) -> Vec<Option<f64>> {
    eval_list(wb, sheet, formula).iter().map(to_num).collect()
}

/// Number format code of the first cell of a reference ("General" when not a reference).
fn ref_format(wb: &Workbook, sheet: usize, formula: &str) -> String {
    parse_ref(wb, sheet, formula)
        .and_then(|(sh, r)| wb.sheet(sh).map(|s| wb.styles.get(s.style_id(r.start)).num_fmt.0.to_string()))
        .unwrap_or_else(|| "General".into())
}

fn value_text(wb: &Workbook, v: &Value) -> String {
    match v {
        Value::Array(a) => a.get(0, 0).map(|v| value_text(wb, v)).unwrap_or_default(),
        _ => v.to_text_in(&wb.locale).unwrap_or_else(|e| wb.locale.formula.local_error(e.as_str()).into()),
    }
}

/// Category labels, formatted with each source cell's number format (dates, currency…).
fn category_texts(wb: &Workbook, sheet: usize, formula: &str) -> Vec<String> {
    let vals = eval_list(wb, sheet, formula);
    let cells: Option<(usize, RangeRef)> = parse_ref(wb, sheet, formula);
    let src = cells.and_then(|(sh, r)| wb.sheet(sh).map(|s| (s, r)));
    vals.iter()
        .enumerate()
        .map(|(i, v)| {
            if let (Some((s, r)), Value::Number(_)) = (src, v) {
                let w = r.width().max(1) as usize;
                let at = r.start.offset((i / w) as i64, (i % w) as i64);
                if let Some(c) = at {
                    let code = &wb.styles.get(s.style_id(c)).num_fmt.0;
                    if !code.eq_ignore_ascii_case("general") {
                        return format_value_in(v, &NumberFormat::parse(code), wb.date_system, &wb.locale).text;
                    }
                }
            }
            value_text(wb, v)
        })
        .collect()
}

/// A series name: a reference/formula is evaluated, a quoted or bare literal is used as is.
fn series_name(wb: &Workbook, sheet: usize, name: &str) -> String {
    let t = name.trim();
    if let Some(q) = t.strip_prefix('"').and_then(|q| q.strip_suffix('"')) {
        return q.replace("\"\"", "\"");
    }
    if t.starts_with('=') || parse_ref(wb, sheet, t).is_some() && t.contains('!') {
        let vals = eval_list(wb, sheet, t);
        let parts: Vec<String> = vals.iter().map(|v| value_text(wb, v)).filter(|s| !s.is_empty()).take(8).collect();
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

/// Evaluates a chart's series formulas: names, values, categories (default "1", "2", …), X
/// values (scatter/bubble), bubble sizes, number formats and colours (explicit, else the
/// default palette derived from the workbook theme accents).
pub fn resolve(wb: &Workbook, sheet: usize, chart: &Chart) -> ChartData {
    let numeric_x = matches!(chart.kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble);
    let mut series = Vec::new();
    let mut categories: Option<Vec<String>> = None;
    for (i, s) in chart.series.iter().take(MAX_SERIES).enumerate() {
        let values = nums(wb, sheet, &s.values);
        let name = s.name.as_deref().map(|n| series_name(wb, sheet, n)).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Series{}", i + 1));
        let color =
            s.color.as_ref().and_then(|c| c.resolve(&wb.theme)).map(|[r, g, b]| [r, g, b, 0xFF]).unwrap_or_else(|| series_color(&wb.theme, i));
        let kind = effective_kind(chart.kind, s.kind, i);
        let x = match (&s.categories, numeric_x || matches!(kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble)) {
            // X values with text in them (labels) are ignored: the points are numbered 1..n.
            (Some(f), true) => Some(eval_list(wb, sheet, f)).filter(|xs| !xs.iter().any(Value::is_text)).map(|xs| xs.iter().map(to_num).collect()),
            _ => None,
        };
        if categories.is_none()
            && let Some(f) = &s.categories
        {
            let c = category_texts(wb, sheet, f);
            if !c.is_empty() {
                categories = Some(c);
            }
        }
        let sizes = s.bubble_sizes.as_deref().map(|f| nums(wb, sheet, f));
        series.push(SeriesData {
            name,
            values,
            x,
            sizes,
            color,
            kind,
            secondary: s.secondary,
            smooth: s.smooth,
            number_format: ref_format(wb, sheet, &s.values),
        });
    }
    let n = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    let categories = categories.unwrap_or_else(|| (1..=n).map(|i| i.to_string()).collect());
    ChartData { categories, series }
}
