//! DrawingML charts (`xl/charts/chartN.xml`): reading the common 2-D/3-D chart groups into the
//! model, and writing model charts (with cached series values) from scratch.

use std::fmt::Write as _;

use gridcraft_core::{CellRef, RangeRef, Value, category_levels};
use gridcraft_formula::{BinOp, Expr, Reference, SheetSel};
use gridcraft_model::{Chart, ChartKind, Color, DefinedName, LegendPos, LineDash, Series, Trendline, TrendlineKind, Workbook};

use crate::drawing::{dml_color, srgb};
use crate::xml::{El, esc, num};

// ---------------------------------------------------------------- reading

fn group_kind(g: &El) -> Option<ChartKind> {
    let grouping = g.child_val("grouping").unwrap_or("standard");
    let stacked = grouping == "stacked";
    let pct = grouping == "percentStacked";
    Some(match g.name.as_str() {
        "barChart" | "bar3DChart" => {
            let bar = g.child_val("barDir") == Some("bar");
            match (bar, stacked, pct) {
                (false, true, _) => ChartKind::ColumnStacked,
                (false, _, true) => ChartKind::ColumnStacked100,
                (false, _, _) => ChartKind::ColumnClustered,
                (true, true, _) => ChartKind::BarStacked,
                (true, _, true) => ChartKind::BarStacked100,
                (true, _, _) => ChartKind::BarClustered,
            }
        }
        "lineChart" | "line3DChart" => {
            if stacked || pct {
                ChartKind::LineStacked
            } else {
                let group_markers = g.child_val("marker").is_none_or(|v| v == "1" || v == "true");
                let all_none = g.kids("ser").all(|s| s.path(&["marker", "symbol"]).and_then(|m| m.attr("val")) == Some("none"));
                if !group_markers || (all_none && g.kids("ser").next().is_some()) { ChartKind::Line } else { ChartKind::LineMarkers }
            }
        }
        "pieChart" | "pie3DChart" | "ofPieChart" => ChartKind::Pie,
        "doughnutChart" => ChartKind::Doughnut,
        "areaChart" | "area3DChart" => {
            if stacked || pct {
                ChartKind::AreaStacked
            } else {
                ChartKind::Area
            }
        }
        "scatterChart" => {
            let no_lines = g.kids("ser").all(|s| s.path(&["spPr", "ln"]).is_some_and(|l| l.child("noFill").is_some()));
            if g.child_val("scatterStyle") == Some("marker") || (no_lines && g.kids("ser").next().is_some()) {
                ChartKind::Scatter
            } else {
                ChartKind::ScatterLines
            }
        }
        "bubbleChart" => ChartKind::Bubble,
        "radarChart" => ChartKind::Radar,
        "stockChart" => ChartKind::Stock,
        _ => return None,
    })
}

/// Excel's file notation for "this workbook" in front of a workbook-scoped defined name.
const THIS_BOOK: &str = "[0]!";

/// A chart reference (`<c:f>`) read from a file → model text: drops a leading `=` and the
/// `[0]!` in front of workbook-scoped names (`[0]!Sales` → `Sales`), which the formula parser
/// would take for a structured reference. External books (`[1]Sheet1!$A$1`), quoted sheet
/// names and string literals are kept as they are.
pub(crate) fn chart_ref_from_file(text: &str) -> String {
    let t = text.trim();
    let t = t.strip_prefix('=').unwrap_or(t);
    let mut out = String::with_capacity(t.len());
    let mut quote: Option<char> = None;
    // The last character kept: `[0]!` only counts at the start or after an operator,
    // never inside a structured reference (`T[[0]]`) or a longer name.
    let mut prev: Option<char> = None;
    let mut i = 0;
    while let Some(c) = t.get(i..).and_then(|r| r.chars().next()) {
        let rest = t.get(i..).unwrap_or("");
        if quote.is_none()
            && rest.starts_with(THIS_BOOK)
            && prev.is_none_or(|p| !(p.is_alphanumeric() || "_.\\[]'\"!$#".contains(p)))
            && rest.get(THIS_BOOK.len()..).and_then(|n| n.chars().next()).is_some_and(|n| n.is_alphabetic() || n == '_' || n == '\\')
        {
            i += THIS_BOOK.len();
            continue;
        }
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            _ => {}
        }
        out.push(c);
        prev = Some(c);
        i += c.len_utf8();
    }
    out
}
const MAX_CHART_CACHED_POINTS: usize = 16_777_216;

struct ChartReadBudget {
    remaining: usize,
    exceeded: bool,
}

impl ChartReadBudget {
    fn new() -> Self {
        Self { remaining: MAX_CHART_CACHED_POINTS, exceeded: false }
    }
}

fn data_ref(e: Option<&El>, budget: &mut ChartReadBudget) -> Option<String> {
    let e = e?;
    for k in ["numRef", "strRef", "multiLvlStrRef"] {
        if let Some(f) = e.child(k).and_then(|r| r.child("f")) {
            return Some(chart_ref_from_file(&f.text));
        }
    }
    for k in ["numLit", "strLit"] {
        if let Some(l) = e.child(k) {
            return read_points(l, l, budget).map(|p| literal(&p, k == "numLit"));
        }
    }
    None
}

/// Excel's maximum worksheet row count bounds cache expansion while preserving declared trailing gaps.
const MAX_CACHED_POINTS: usize = gridcraft_core::MAX_ROWS as usize;

/// The points of a cache or literal (`c` holds `ptCount`, `pts` the `<c:pt>`s), by index; missing
/// points are `None`. Points without a valid `idx` (schema-invalid, but some writers omit it) go
/// in document order. A missing, unreadable or too small `ptCount` gives way to the points present.
///
/// The per-chart budget is checked before allocation. If the requested count would exceed the
/// remaining budget, all present points are retained when they fit; only trailing gaps are
/// clamped. If present points extend past the remaining budget, points beyond that boundary are
/// dropped as well, and the caller is warned.
fn read_points<'a>(c: &El, pts: &'a El, budget: &mut ChartReadBudget) -> Option<Vec<Option<&'a str>>> {
    let at = |i: usize, p: &El| p.attr("idx").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(i);
    let placed = || pts.kids("pt").enumerate().map(|(i, p)| (at(i, p), p)).filter(|(i, _)| *i < MAX_CACHED_POINTS);
    let highest = placed().map(|(i, _)| i + 1).max().unwrap_or(0);
    let requested =
        c.child_val("ptCount").and_then(|v| v.trim().parse::<usize>().ok()).map(|n| n.min(MAX_CACHED_POINTS).max(highest)).unwrap_or(highest);
    let count = if requested <= budget.remaining {
        requested
    } else {
        budget.exceeded = true;
        if highest <= budget.remaining { highest } else { budget.remaining }
    };
    budget.remaining = budget.remaining.saturating_sub(count);
    if count == 0 {
        return None;
    }
    let mut vals = vec![None; count];
    for (i, p) in placed() {
        if let (Some(slot), Some(v)) = (vals.get_mut(i), p.child("v")) {
            *slot = Some(v.text.as_str());
        }
    }
    Some(vals)
}

/// Points as an array constant. Missing points become `#N/A` (numbers) or `""` (text) so
/// positions line up.
fn literal<S: AsRef<str>>(points: &[Option<S>], numeric: bool) -> String {
    let items: Vec<String> = points
        .iter()
        .map(|v| match (v.as_ref().map(AsRef::as_ref), numeric) {
            (Some(t), true) => t.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map_or_else(|| "#N/A".to_string(), num),
            (None, true) => "#N/A".to_string(),
            (Some(t), false) => format!("\"{}\"", t.replace('"', "\"\"")),
            (None, false) => "\"\"".to_string(),
        })
        .collect();
    format!("{{{}}}", items.join(","))
}

/// The points cached under a PivotChart series reference.
#[derive(Clone, Debug)]
struct CachedPoints {
    points: Vec<Option<String>>,
    numeric: bool,
    /// A `multiLvlStrRef` of more than one level: one column (or row) of cells per level, the
    /// innermost last. The cache holds the innermost level.
    multi_level: bool,
}

/// What a PivotChart series saved with its references, plotted when the referenced cells don't
/// hold it (see [`plot_cached`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct PivotSeriesCache {
    name: Option<String>,
    categories: Option<CachedPoints>,
    values: Option<CachedPoints>,
    /// The values' `numCache` format code.
    number_format: Option<String>,
}

/// `numCache`, `strCache`, or the innermost level of a `multiLvlStrCache`, under `cat` or `val`.
fn cached_points(e: Option<&El>, budget: &mut ChartReadBudget) -> Option<CachedPoints> {
    let e = e?;
    let owned = |p: Vec<Option<&str>>| p.into_iter().map(|v| v.map(str::to_string)).collect();
    if let Some(c) = e.path(&["numRef", "numCache"]) {
        Some(CachedPoints { points: owned(read_points(c, c, budget)?), numeric: true, multi_level: false })
    } else if let Some(c) = e.path(&["strRef", "strCache"]) {
        Some(CachedPoints { points: owned(read_points(c, c, budget)?), numeric: false, multi_level: false })
    } else {
        let c = e.path(&["multiLvlStrRef", "multiLvlStrCache"])?;
        let multi_level = c.kids("lvl").nth(1).is_some();
        Some(CachedPoints { points: owned(read_points(c, c.child("lvl")?, budget)?), numeric: false, multi_level })
    }
}

fn pivot_series_cache(s: &El, budget: &mut ChartReadBudget) -> PivotSeriesCache {
    PivotSeriesCache {
        name: s.path(&["tx", "strRef", "strCache", "pt", "v"]).map(|v| v.text.clone()),
        categories: cached_points(s.child("cat"), budget),
        values: cached_points(s.child("val"), budget),
        number_format: s
            .path(&["val", "numRef", "numCache", "formatCode"])
            .map(|c| c.text.trim())
            .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("General"))
            .map(str::to_string),
    }
}

/// The references of a series formula: one area, or a union `(Sheet!$A$1:$A$3,Sheet!$A$5)`.
fn ref_areas<'e>(e: &'e Expr, out: &mut Vec<&'e Reference>) -> bool {
    match e {
        Expr::Ref(r) => {
            out.push(r);
            true
        }
        Expr::Paren(x) => ref_areas(x, out),
        Expr::Binary(BinOp::Union, a, b) => ref_areas(a, out) && ref_areas(b, out),
        _ => false,
    }
}

/// Whether a cell shows the cached point: numbers within rounding, text as is. A number cache
/// leaves out cells that hold no number.
fn point_matches(v: &Value, p: Option<&str>, numeric: bool) -> bool {
    match (v, p) {
        (Value::Number(n), Some(t)) => t.trim().parse::<f64>().is_ok_and(|c| c == *n || (c - n).abs() <= 1e-9 * c.abs().max(n.abs())),
        (Value::Number(_), None) => false,
        (_, None) => numeric || v.is_empty(),
        (_, Some(_)) if numeric => false,
        (Value::Text(s), Some(t)) => **s == *t,
        (Value::Bool(b), Some(t)) => t.eq_ignore_ascii_case(if *b { "TRUE" } else { "FALSE" }),
        (Value::Error(e), Some(t)) => e.as_str() == t,
        (Value::Empty, Some(t)) => t.is_empty(),
        (Value::Array(_), Some(_)) => false,
    }
}

/// Whether the cells a series formula refers to (on sheet `home` when unqualified) hold the
/// cached points, one cell per point. Of a multi-level block, the innermost level is compared.
fn live_matches(wb: &Workbook, home: usize, f: &str, p: &CachedPoints) -> bool {
    let Ok(e) = gridcraft_formula::parse(f) else { return false };
    let mut refs = Vec::new();
    if !ref_areas(&e, &mut refs) {
        return false;
    }
    let mut areas = Vec::with_capacity(refs.len());
    for r in refs {
        let si = match &r.sheet {
            SheetSel::Current => Some(home),
            SheetSel::Named(n) => wb.sheet_index(n),
            SheetSel::Span(..) => None,
        };
        let Some(sheet) = si.and_then(|i| wb.sheet(i)) else { return false };
        areas.push((sheet, r.range()));
    }
    let n = p.points.len();
    if p.multi_level {
        // The last column of a block with a row per point, or the last row of one with a
        // column per point.
        let [(sheet, r)] = areas[..] else { return false };
        let leaf = if r.height() as usize == n {
            RangeRef::new(CellRef::new(r.start.row, r.end.col), r.end)
        } else if r.width() as usize == n {
            RangeRef::new(CellRef::new(r.end.row, r.start.col), r.end)
        } else {
            return false;
        };
        areas = vec![(sheet, leaf)];
    }
    if areas.iter().try_fold(0u64, |t, (_, r)| t.checked_add(r.count())) != Some(n as u64) {
        return false;
    }
    areas.iter().flat_map(|(sheet, r)| r.iter().map(|c| sheet.value(c))).zip(&p.points).all(|(v, pt)| point_matches(&v, pt.as_deref(), p.numeric))
}

/// A PivotChart's `<c:f>` formulas only approximate its PivotTable: often just the table's corner
/// cell, or a block that takes in the grand total row. Excel draws the chart from the PivotTable,
/// and the cached points are what it shows. When the cells a series' categories or values refer
/// to (on sheet `si`) don't hold their cached points, the series plots the cached points (and its
/// cached name) instead; refreshing the PivotTable rebinds the chart to live cells. A live
/// multi-level category block keeps its reference and draws only its innermost level.
pub(crate) fn plot_cached(wb: &Workbook, si: usize, ch: &mut Chart, caches: &[PivotSeriesCache]) {
    for (ser, c) in ch.series.iter_mut().zip(caches) {
        let stale = |f: Option<&str>, p: &&CachedPoints| !f.is_some_and(|f| live_matches(wb, si, f, p));
        let categories = c.categories.as_ref().filter(|p| stale(ser.categories.as_deref(), p)).map(|p| literal(&p.points, p.numeric));
        let values = c.values.as_ref().filter(|p| stale(Some(&ser.values), p)).map(|p| literal(&p.points, p.numeric));
        if categories.is_none() && c.categories.as_ref().is_some_and(|p| p.multi_level) {
            ser.innermost_categories = true;
        }
        if categories.is_none() && values.is_none() {
            continue;
        }
        if categories.is_some() {
            ser.categories = categories;
        }
        if let Some(v) = values {
            ser.values = v;
            ser.number_format.clone_from(&c.number_format);
        }
        if let Some(name) = &c.name {
            ser.name = Some(format!("\"{}\"", name.replace('"', "\"\"")));
        }
    }
}

/// [`plot_cached`] for the PivotCharts read from a workbook (by chart id), once every sheet's
/// cells are in.
pub(crate) fn plot_pivot_caches(wb: &mut Workbook, pending: &[(u32, Vec<PivotSeriesCache>)]) {
    for (id, caches) in pending {
        let at = wb.sheets.iter().enumerate().find_map(|(si, sh)| sh.charts.iter().position(|c| c.id == *id).map(|ci| (si, ci)));
        let Some((si, ci)) = at else { continue };
        let Some(mut ch) = wb.sheet(si).and_then(|sh| sh.charts.get(ci)).cloned() else { continue };
        plot_cached(wb, si, &mut ch, caches);
        if let Some(slot) = wb.sheet_mut(si).and_then(|sh| sh.charts.get_mut(ci)) {
            *slot = ch;
        }
    }
}

fn read_series(s: &El, budget: &mut ChartReadBudget) -> Series {
    let name = s
        .child("tx")
        .and_then(|tx| tx.path(&["strRef", "f"]).map(|f| chart_ref_from_file(&f.text)).or_else(|| tx.child("v").map(|v| v.text.clone())));
    let sp = s.child("spPr");
    let color = sp
        .and_then(|p| p.child("solidFill"))
        .and_then(dml_color)
        .or_else(|| sp.and_then(|p| p.path(&["ln", "solidFill"])).and_then(dml_color))
        .or_else(|| s.path(&["marker", "spPr", "solidFill"]).and_then(dml_color));
    let values = s.child("val").or_else(|| s.child("yVal"));
    Series {
        name,
        categories: data_ref(s.child("cat"), budget).or_else(|| data_ref(s.child("xVal"), budget)),
        values: data_ref(s.child("val"), budget).or_else(|| data_ref(s.child("yVal"), budget)).unwrap_or_default(),
        bubble_sizes: data_ref(s.child("bubbleSize"), budget),
        color,
        secondary: false,
        kind: None,
        quartiles: Default::default(),
        smooth: bool_val(s, "smooth"),
        trendlines: s.kids("trendline").filter_map(read_trendline).collect(),
        // Literal values keep the format code saved with them.
        number_format: values
            .filter(|v| v.child("numRef").is_none())
            .and_then(|v| v.path(&["numLit", "formatCode"]))
            .map(|c| c.text.trim())
            .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("General"))
            .map(str::to_string),
        innermost_categories: false,
    }
}

const DASHES: [(LineDash, &str); 11] = [
    (LineDash::Solid, "solid"),
    (LineDash::Dot, "dot"),
    (LineDash::Dash, "dash"),
    (LineDash::LgDash, "lgDash"),
    (LineDash::DashDot, "dashDot"),
    (LineDash::LgDashDot, "lgDashDot"),
    (LineDash::LgDashDotDot, "lgDashDotDot"),
    (LineDash::SysDash, "sysDash"),
    (LineDash::SysDot, "sysDot"),
    (LineDash::SysDashDot, "sysDashDot"),
    (LineDash::SysDashDotDot, "sysDashDotDot"),
];

const EMU_PER_PT: f64 = 12_700.0;

/// A `CT_Boolean` child (`val` defaults to true).
fn bool_val(e: &El, tag: &str) -> bool {
    e.child(tag).is_some_and(|c| c.flag("val", true))
}

fn f64_val(e: &El, tag: &str) -> Option<f64> {
    e.child(tag).and_then(|c| c.attr_f64("val"))
}

/// `<c:trendline>` (ECMA-376 Part 1, 21.2.2.211). Unknown types are dropped.
fn read_trendline(t: &El) -> Option<Trendline> {
    let kind = match t.child_val("trendlineType").unwrap_or("linear") {
        "linear" => TrendlineKind::Linear,
        "exp" => TrendlineKind::Exponential,
        "log" => TrendlineKind::Logarithmic,
        "power" => TrendlineKind::Power,
        "poly" => TrendlineKind::Polynomial(t.child_val("order").and_then(|v| v.trim().parse::<u8>().ok()).unwrap_or(2).clamp(2, 6)),
        "movingAvg" => TrendlineKind::MovingAverage(t.child_val("period").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(2).clamp(2, 255)),
        _ => return None,
    };
    let ln = t.path(&["spPr", "ln"]);
    Some(Trendline {
        kind,
        name: t.child("name").map(|n| n.text.clone()),
        forward: Trendline::clamp_forecast(f64_val(t, "forward").unwrap_or(0.0)),
        backward: Trendline::clamp_forecast(f64_val(t, "backward").unwrap_or(0.0)),
        intercept: f64_val(t, "intercept"),
        display_equation: bool_val(t, "dispEq"),
        display_r_squared: bool_val(t, "dispRSqr"),
        color: ln.and_then(|l| l.child("solidFill")).and_then(dml_color),
        width: ln.and_then(|l| l.attr_f64("w")).map(|w| (w / EMU_PER_PT) as f32).filter(|w| w.is_finite() && *w >= 0.0),
        dash: ln.and_then(|l| l.child("prstDash")).and_then(|d| d.attr("val")).and_then(|v| DASHES.iter().find(|(_, n)| *n == v).map(|(d, _)| *d)),
    })
}

fn rich_title(t: &El) -> Option<String> {
    let tx = t.child("tx")?;
    if let Some(rich) = tx.child("rich") {
        let paras: Vec<String> =
            rich.kids("p").map(|p| p.kids("r").filter_map(|r| r.child("t")).map(|t| t.text.as_str()).collect::<String>()).collect();
        let s = paras.join("\n");
        return if s.is_empty() { None } else { Some(s) };
    }
    if let Some(sr) = tx.child("strRef") {
        if let Some(v) = sr.path(&["strCache", "pt", "v"]) {
            return Some(v.text.clone());
        }
        return sr.child("f").map(|f| f.text.trim().to_string());
    }
    None
}

fn axis_ids(g: &El) -> Vec<String> {
    g.kids("axId").filter_map(|a| a.attr("val")).map(str::to_string).collect()
}

/// A chart part, with the cached points of each series when it is a PivotChart (empty otherwise),
#[cfg(test)]
pub(crate) fn read_chart(root: &El) -> Option<(Chart, Vec<PivotSeriesCache>)> {
    read_chart_with_budget(root).map(|(chart, caches, _)| (chart, caches))
}

pub(crate) fn read_chart_with_budget(root: &El) -> Option<(Chart, Vec<PivotSeriesCache>, bool)> {
    let chart = root.child("chart")?;
    let plot = chart.child("plotArea")?;
    let groups: Vec<(&El, ChartKind)> = plot.children.iter().filter_map(|g| group_kind(g).map(|k| (g, k))).collect();
    let (first, first_kind) = *groups.first()?;
    let first_axes = axis_ids(first);
    let pivot = root.path(&["pivotSource", "name"]).map(|n| strip_book(n.text.trim()).to_string()).filter(|n| !n.is_empty());
    let mut series = vec![];
    let mut caches = vec![];
    let mut budget = ChartReadBudget::new();
    for (gi, (g, k)) in groups.iter().enumerate() {
        for s in g.kids("ser") {
            let mut ser = read_series(s, &mut budget);
            if groups.len() > 1 {
                // Every series of a combo chart keeps its own group's kind, the first group's included.
                ser.kind = Some(*k);
            }
            if gi > 0 {
                ser.secondary = axis_ids(g) != first_axes;
            }
            series.push(ser);
            if pivot.is_some() {
                caches.push(pivot_series_cache(s, &mut budget));
            }
        }
    }
    let kind = if groups.len() > 1 { ChartKind::Combo } else { first_kind };
    let show_val = |e: &El| e.child("dLbls").is_some_and(|d| bool_val(d, "showVal"));
    let data_labels = groups.iter().any(|(g, _)| show_val(g) || g.kids("ser").any(show_val));
    let val_axes: Vec<&El> = plot.kids("valAx").collect();
    let cat_ax = plot.child("catAx").or_else(|| plot.child("dateAx"));
    let scatter = matches!(first_kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble);
    let (x_ax, y_ax) = if scatter {
        let x = val_axes.iter().find(|a| matches!(a.child_val("axPos"), Some("b") | Some("t"))).copied();
        let y = val_axes.iter().find(|a| matches!(a.child_val("axPos"), Some("l") | Some("r"))).copied();
        (x, y)
    } else {
        (cat_ax, val_axes.first().copied())
    };
    let legend = match chart.child("legend") {
        None => LegendPos::None,
        Some(l) => match l.child_val("legendPos").unwrap_or("r") {
            "b" => LegendPos::Bottom,
            "t" => LegendPos::Top,
            "l" => LegendPos::Left,
            _ => LegendPos::Right,
        },
    };
    let ch = Chart {
        id: 0,
        kind,
        anchor: Default::default(),
        // No `<c:title>`: an automatic title (a lone series' name), unless the file deleted it.
        title: match chart.child("title") {
            Some(t) => rich_title(t),
            None if bool_val(chart, "autoTitleDeleted") => Some(String::new()),
            None => None,
        },
        series,
        legend,
        data_labels,
        gridlines: val_axes.iter().any(|a| a.child("majorGridlines").is_some()),
        style: root.child_val("style").and_then(|v| v.parse().ok()).unwrap_or(2),
        x_title: x_ax.and_then(|a| a.child("title")).and_then(rich_title),
        y_title: y_ax.and_then(|a| a.child("title")).and_then(rich_title),
        source: None,
        by_rows: false,
        pivot,
    };
    Some((ch, caches, budget.exceeded))
}

// ---------------------------------------------------------------- writing

#[derive(Clone, Copy, PartialEq)]
enum G {
    Bar { bar: bool, grouping: &'static str },
    Line { stacked: bool, markers: bool },
    Pie,
    Doughnut,
    Area { stacked: bool },
    Scatter { lines: bool },
    Bubble,
    Radar,
    Stock,
}

fn group_for(k: ChartKind) -> G {
    match k {
        ChartKind::ColumnStacked => G::Bar { bar: false, grouping: "stacked" },
        ChartKind::ColumnStacked100 => G::Bar { bar: false, grouping: "percentStacked" },
        ChartKind::BarClustered => G::Bar { bar: true, grouping: "clustered" },
        ChartKind::BarStacked => G::Bar { bar: true, grouping: "stacked" },
        ChartKind::BarStacked100 => G::Bar { bar: true, grouping: "percentStacked" },
        ChartKind::Line => G::Line { stacked: false, markers: false },
        ChartKind::LineMarkers => G::Line { stacked: false, markers: true },
        ChartKind::LineStacked => G::Line { stacked: true, markers: false },
        ChartKind::Pie => G::Pie,
        ChartKind::Doughnut => G::Doughnut,
        ChartKind::Area => G::Area { stacked: false },
        ChartKind::AreaStacked => G::Area { stacked: true },
        ChartKind::Scatter => G::Scatter { lines: false },
        ChartKind::ScatterLines => G::Scatter { lines: true },
        ChartKind::Bubble => G::Bubble,
        ChartKind::Radar => G::Radar,
        ChartKind::Stock => G::Stock,
        // Combo charts start from clustered columns; chartex kinds have their own part.
        _ => G::Bar { bar: false, grouping: "clustered" },
    }
}

const MAX_CACHE: u64 = 100_000;

/// The sheet and range a same-workbook reference (`Sheet1!$B$2:$B$9`) names.
fn ref_target(wb: &Workbook, sheet_idx: usize, f: &str) -> Option<(usize, RangeRef)> {
    let e = gridcraft_formula::parse(f).ok()?;
    let Expr::Ref(r) = e else { return None };
    let si = match &r.sheet {
        SheetSel::Current => sheet_idx,
        SheetSel::Named(n) => wb.sheet_index(n)?,
        SheetSel::Span(..) => return None,
    };
    Some((si, r.range()))
}

/// Values of a same-workbook reference (`Sheet1!$B$2:$B$9`), row-major.
pub(crate) fn ref_values(wb: &Workbook, sheet_idx: usize, f: &str) -> Option<Vec<Value>> {
    let (si, mut range) = ref_target(wb, sheet_idx, f)?;
    let sheet = wb.sheet(si)?;
    // Whole columns/rows: cut the far end at the used range. The start stays put, so point indices keep matching the reference.
    if let Some(u) = sheet.used_range() {
        if range.is_full_cols() {
            range.end.row = range.end.row.min(u.end.row.max(range.start.row));
        }
        if range.is_full_rows() {
            range.end.col = range.end.col.min(u.end.col.max(range.start.col));
        }
    }
    if range.count() > MAX_CACHE {
        return None;
    }
    Some(range.iter().map(|c| sheet.value(c)).collect())
}

fn is_ref(f: &str) -> bool {
    matches!(gridcraft_formula::parse(f), Ok(Expr::Ref(_)))
}

/// The items of an array constant (`{1,"a";#N/A,2}`), row-major; nothing for anything else.
fn array_items(f: &str) -> Vec<Expr> {
    match gridcraft_formula::parse(f) {
        Ok(Expr::Array(rows)) => rows.into_iter().flatten().collect(),
        _ => Vec::new(),
    }
}
/// A chart reference that is a name: its sheet when qualified (`Sheet1!Sales`), the bare
/// name, and the definition formulas would find from sheet `si` (none when undefined). `None`
/// when it isn't a name, or names a sheet the workbook doesn't have.
fn chart_name<'a>(wb: &'a Workbook, si: usize, f: &str) -> Option<(Option<usize>, String, Option<&'a DefinedName>)> {
    let Ok(Expr::Name(n)) = gridcraft_formula::parse(f.trim()) else { return None };
    match n.split_once('!') {
        Some((s, bare)) => {
            let i = wb.sheet_index(s.trim_matches('\''))?;
            Some((Some(i), bare.to_string(), wb.name(bare, i)))
        }
        None => {
            let def = wb.name(&n, si);
            Some((None, n, def))
        }
    }
}

/// A chart reference for `<c:f>`. Excel needs a name qualified there: with a bare `Name` it
/// draws the whole chart blank, with no repair prompt (observed in Excel for Windows,
/// Microsoft 365). A workbook-scoped name is written `[0]!Name`, as Excel writes it, a
/// sheet-scoped one `Sheet!Name`; an undefined name gets `[0]!`, as the file it came from had.
fn chart_ref_to_file(wb: &Workbook, si: usize, f: &str) -> String {
    let on_sheet = |i: usize, n: &str| wb.sheet(i).map(|sh| format!("{}!{n}", gridcraft_formula::quote_sheet(&sh.name)));
    match chart_name(wb, si, f) {
        Some((Some(i), n, _)) => on_sheet(i, &n).unwrap_or_else(|| crate::fmla::text_to_file(f)),
        Some((None, n, def)) => def.and_then(|d| d.scope).and_then(|i| on_sheet(i, &n)).unwrap_or_else(|| format!("{THIS_BOOK}{n}")),
        None => crate::fmla::text_to_file(f),
    }
}

/// A series name that refers to a defined name (as opposed to literal text that parses as one).
fn is_defined_name(wb: &Workbook, si: usize, f: &str) -> bool {
    chart_name(wb, si, f).is_some_and(|(_, _, d)| d.is_some())
}

/// `fmt` is the format code saved with literal values (`numLit`); references save their cells'.
fn num_data(tag: &str, f: Option<&str>, fmt: Option<&str>, wb: &Workbook, si: usize) -> String {
    let f = f.unwrap_or("").trim();
    if f.is_empty() {
        return format!("<c:{tag}><c:numLit><c:ptCount val=\"0\"/></c:numLit></c:{tag}>");
    }
    if f.starts_with('{') {
        let items = array_items(f);
        let fmt = esc(fmt.unwrap_or("General"));
        let mut s = format!("<c:{tag}><c:numLit><c:formatCode>{fmt}</c:formatCode><c:ptCount val=\"{}\"/>", items.len());
        for (i, e) in items.iter().enumerate() {
            let n = match e {
                Expr::Number(n) => Some(*n),
                Expr::Unary(gridcraft_formula::UnOp::Neg, x) => match **x {
                    Expr::Number(n) => Some(-n),
                    _ => None,
                },
                _ => None,
            };
            // Anything else (`#N/A`, text) is a gap: its index is left out.
            if let Some(n) = n {
                let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", num(n));
            }
        }
        let _ = write!(s, "</c:numLit></c:{tag}>");
        return s;
    }
    let mut s = format!("<c:{tag}><c:numRef><c:f>{}</c:f>", esc(&chart_ref_to_file(wb, si, f)));
    if let Some(vals) = ref_values(wb, si, f) {
        let _ = write!(s, "<c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>", vals.len());
        for (i, v) in vals.iter().enumerate() {
            if let Value::Number(n) = v {
                let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", num(*n));
            }
        }
        s.push_str("</c:numCache>");
    }
    let _ = write!(s, "</c:numRef></c:{tag}>");
    s
}

/// Splits the inside of an array constant on `,` and `;` outside double quotes; inside quotes `""` is a literal quote.
/// Unquoted items are trimmed; quoted ones keep the text between the quotes and drop the spaces around them.
fn split_array_items(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut in_quote = false;
    let mut chars = inner.chars().peekable();
    let finish = |cur: &mut String, quoted: &mut bool| {
        let text = std::mem::take(cur);
        let item = if *quoted { text } else { text.trim().to_string() };
        *quoted = false;
        item
    };
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quote && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => {
                if !quoted && cur.trim().is_empty() {
                    cur.clear();
                }
                in_quote = !in_quote;
                quoted = true;
            }
            ',' | ';' if !in_quote => items.push(finish(&mut cur, &mut quoted)),
            c if quoted && !in_quote && c.is_whitespace() => {}
            _ => cur.push(c),
        }
    }
    items.push(finish(&mut cur, &mut quoted));
    items
}

fn str_data(tag: &str, f: &str, wb: &Workbook, si: usize) -> String {
    let f = f.trim();
    if f.starts_with('{') {
        let vals = split_array_items(f.trim_matches(|c| c == '{' || c == '}'));
        let mut s = format!("<c:{tag}><c:strLit><c:ptCount val=\"{}\"/>", vals.len());
        for (i, v) in vals.iter().enumerate() {
            let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(v));
        }
        let _ = write!(s, "</c:strLit></c:{tag}>");
        return s;
    }
    let mut s = format!("<c:{tag}><c:strRef><c:f>{}</c:f>", esc(&chart_ref_to_file(wb, si, f)));
    if let Some(vals) = ref_values(wb, si, f) {
        let _ = write!(s, "<c:strCache><c:ptCount val=\"{}\"/>", vals.len());
        for (i, v) in vals.iter().enumerate() {
            if !v.is_empty() {
                let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(&v.display()));
            }
        }
        s.push_str("</c:strCache>");
    }
    let _ = write!(s, "</c:strRef></c:{tag}>");
    s
}

/// The text of a literal series name: a quoted formula literal (`"Sales ""East"""`) without its
/// quotes and with `""` read as `"`; bare text as is.
pub(crate) fn literal_text(n: &str) -> std::borrow::Cow<'_, str> {
    match n.trim().strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        Some(t) => t.replace("\"\"", "\"").into(),
        None => n.into(),
    }
}

/// `<c:cat>` of a series: a string reference, or a multi-level one (`multiLvlStrRef`) when the
/// category range has more columns than a column of values (or more rows than a row of values:
/// year beside month). Its cache lists the innermost level first, then each outer level with
/// only the cells that start a group, as Excel writes it.
fn cat_data(f: &str, values: &str, wb: &Workbook, si: usize) -> String {
    multi_level_cat(f.trim(), values, wb, si).unwrap_or_else(|| str_data("cat", f, wb, si))
}

fn multi_level_cat(f: &str, values: &str, wb: &Workbook, si: usize) -> Option<String> {
    let (sheet, range) = ref_target(wb, si, f)?;
    let value_range = ref_target(wb, si, values).map(|(_, r)| r);
    let levels = category_levels(&range, value_range.as_ref())?;
    if range.count() > MAX_CACHE {
        return None;
    }
    let sheet = wb.sheet(sheet)?;
    let (w, h) = (range.width() as usize, range.height() as usize);
    let text = |row: usize, col: usize| -> String {
        let v = range.start.offset(row as i64, col as i64).map(|c| sheet.value(c));
        v.filter(|v| !v.is_empty()).map(|v| v.display()).unwrap_or_default()
    };
    // The innermost level (nearest the values), then the outer ones.
    let (n, lvls): (usize, Vec<Vec<String>>) = if levels.by_columns {
        (h, (0..=levels.outer as usize).filter_map(|k| (w - 1).checked_sub(k)).map(|c| (0..h).map(|r| text(r, c)).collect()).collect())
    } else {
        (w, (0..=levels.outer as usize).filter_map(|k| (h - 1).checked_sub(k)).map(|r| (0..w).map(|c| text(r, c)).collect()).collect())
    };
    let mut s = format!("<c:cat><c:multiLvlStrRef><c:f>{}</c:f><c:multiLvlStrCache><c:ptCount val=\"{n}\"/>", esc(&crate::fmla::text_to_file(f)));
    for lvl in &lvls {
        s.push_str("<c:lvl>");
        for (i, v) in lvl.iter().enumerate() {
            if !v.is_empty() {
                let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(v));
            }
        }
        s.push_str("</c:lvl>");
    }
    s.push_str("</c:multiLvlStrCache></c:multiLvlStrRef></c:cat>");
    Some(s)
}

fn series_xml(s: &Series, idx: usize, g: G, wb: &Workbook, si: usize) -> String {
    let mut x = format!("<c:ser><c:idx val=\"{idx}\"/><c:order val=\"{idx}\"/>");
    if let Some(n) = &s.name {
        if is_ref(n) || is_defined_name(wb, si, n) {
            x.push_str(&str_data("tx", n, wb, si));
        } else {
            let _ = write!(x, "<c:tx><c:v>{}</c:v></c:tx>", esc(&literal_text(n)));
        }
    }
    let color = s.color.as_ref().and_then(|c: &Color| srgb(c, &wb.theme));
    let line_like = matches!(g, G::Line { .. } | G::Radar | G::Scatter { lines: true });
    match (&color, line_like) {
        (Some(c), true) => {
            let _ = write!(x, "<c:spPr><a:ln w=\"28575\" cap=\"rnd\"><a:solidFill>{c}</a:solidFill><a:round/></a:ln></c:spPr>");
        }
        (Some(c), false) if g == (G::Scatter { lines: false }) => {
            let _ = write!(
                x,
                "<c:spPr><a:ln w=\"19050\"><a:noFill/></a:ln></c:spPr><c:marker><c:symbol val=\"circle\"/><c:size val=\"5\"/><c:spPr><a:solidFill>{c}</a:solidFill></c:spPr></c:marker>"
            );
        }
        (Some(c), false) => {
            let _ = write!(x, "<c:spPr><a:solidFill>{c}</a:solidFill></c:spPr>");
        }
        (None, _) if g == (G::Scatter { lines: false }) || g == G::Stock => x.push_str("<c:spPr><a:ln w=\"19050\"><a:noFill/></a:ln></c:spPr>"),
        _ => {}
    }
    match g {
        G::Bar { .. } => x.push_str("<c:invertIfNegative val=\"0\"/>"),
        G::Line { markers: false, .. } | G::Scatter { lines: true } | G::Stock => x.push_str("<c:marker><c:symbol val=\"none\"/></c:marker>"),
        G::Bubble => x.push_str("<c:invertIfNegative val=\"0\"/>"),
        _ => {}
    }
    // Trendlines sit between dLbls and the data in CT_LineSer, CT_BarSer, CT_AreaSer,
    // CT_ScatterSer and CT_BubbleSer; pie and radar series have none.
    if !matches!(g, G::Pie | G::Doughnut | G::Radar) {
        for t in &s.trendlines {
            x.push_str(&trendline_xml(t, wb));
        }
    }
    match g {
        G::Scatter { .. } | G::Bubble => {
            if let Some(c) = &s.categories {
                x.push_str(&num_data("xVal", Some(c), None, wb, si));
            }
            x.push_str(&num_data("yVal", Some(&s.values), s.number_format.as_deref(), wb, si));
            if g == G::Bubble {
                x.push_str(&num_data("bubbleSize", s.bubble_sizes.as_deref().or(Some(&s.values)), None, wb, si));
                x.push_str("<c:bubble3D val=\"0\"/>");
            } else {
                let _ = write!(x, "<c:smooth val=\"{}\"/>", s.smooth as u8);
            }
        }
        _ => {
            if let Some(c) = &s.categories {
                x.push_str(&cat_data(c, &s.values, wb, si));
            }
            x.push_str(&num_data("val", Some(&s.values), s.number_format.as_deref(), wb, si));
            if matches!(g, G::Line { .. }) {
                let _ = write!(x, "<c:smooth val=\"{}\"/>", s.smooth as u8);
            }
        }
    }
    x.push_str("</c:ser>");
    x
}

/// `<c:trendline>` with children in schema order: name, spPr, trendlineType, order, period,
/// forward, backward, intercept, dispRSqr, dispEq, trendlineLbl.
fn trendline_xml(t: &Trendline, wb: &Workbook) -> String {
    let mut x = String::from("<c:trendline>");
    if let Some(n) = &t.name {
        let _ = write!(x, "<c:name>{}</c:name>", esc(n));
    }
    let color = t.color.as_ref().and_then(|c| srgb(c, &wb.theme));
    if color.is_some() || t.width.is_some() || t.dash.is_some() {
        x.push_str("<c:spPr><a:ln");
        if let Some(w) = t.width.filter(|w| w.is_finite()) {
            // ST_LineWidth: 0..=20116800 EMU.
            let _ = write!(x, " w=\"{}\"", (f64::from(w) * EMU_PER_PT).round().clamp(0.0, 20_116_800.0) as i64);
        }
        x.push_str(" cap=\"rnd\">");
        if let Some(c) = &color {
            let _ = write!(x, "<a:solidFill>{c}</a:solidFill>");
        }
        if let Some(d) = t.dash.and_then(|d| DASHES.iter().find(|(k, _)| *k == d)) {
            let _ = write!(x, "<a:prstDash val=\"{}\"/>", d.1);
        }
        x.push_str("</a:ln></c:spPr>");
    }
    let ty = match t.kind {
        TrendlineKind::Linear => "linear",
        TrendlineKind::Exponential => "exp",
        TrendlineKind::Logarithmic => "log",
        TrendlineKind::Polynomial(_) => "poly",
        TrendlineKind::Power => "power",
        TrendlineKind::MovingAverage(_) => "movingAvg",
    };
    let _ = write!(x, "<c:trendlineType val=\"{ty}\"/>");
    match t.kind {
        TrendlineKind::Polynomial(o) => {
            let _ = write!(x, "<c:order val=\"{}\"/>", o.clamp(2, 6));
        }
        TrendlineKind::MovingAverage(p) => {
            let _ = write!(x, "<c:period val=\"{}\"/>", p.clamp(2, 255));
        }
        _ => {}
    }
    for (tag, v) in [("forward", t.forward), ("backward", t.backward)] {
        if v.is_finite() && v > 0.0 {
            let _ = write!(x, "<c:{tag} val=\"{}\"/>", num(v));
        }
    }
    if let Some(i) = t.intercept.filter(|i| i.is_finite()) {
        let _ = write!(x, "<c:intercept val=\"{}\"/>", num(i));
    }
    let _ = write!(x, "<c:dispRSqr val=\"{}\"/><c:dispEq val=\"{}\"/>", t.display_r_squared as u8, t.display_equation as u8);
    if t.display_r_squared || t.display_equation {
        x.push_str("<c:trendlineLbl><c:numFmt formatCode=\"General\" sourceLinked=\"0\"/></c:trendlineLbl>");
    }
    x.push_str("</c:trendline>");
    x
}

fn dlbls(show: bool) -> String {
    format!(
        "<c:dLbls><c:showLegendKey val=\"0\"/><c:showVal val=\"{}\"/><c:showCatName val=\"0\"/><c:showSerName val=\"0\"/><c:showPercent val=\"0\"/><c:showBubbleSize val=\"0\"/></c:dLbls>",
        show as u8
    )
}

fn group_xml(g: G, series: &[(usize, &Series)], ch: &Chart, axes: (u32, u32), wb: &Workbook, si: usize) -> String {
    let sers: String = series.iter().map(|(i, s)| series_xml(s, *i, g, wb, si)).collect();
    let dl = dlbls(ch.data_labels);
    let ax = format!("<c:axId val=\"{}\"/><c:axId val=\"{}\"/>", axes.0, axes.1);
    match g {
        G::Bar { bar, grouping } => {
            let overlap = if grouping == "clustered" { String::new() } else { "<c:overlap val=\"100\"/>".into() };
            format!(
                "<c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"{grouping}\"/><c:varyColors val=\"0\"/>{sers}{dl}<c:gapWidth val=\"150\"/>{overlap}{ax}</c:barChart>",
                if bar { "bar" } else { "col" }
            )
        }
        G::Line { stacked, .. } => format!(
            "<c:lineChart><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{sers}{dl}<c:marker val=\"1\"/>{ax}</c:lineChart>",
            if stacked { "stacked" } else { "standard" }
        ),
        G::Pie => format!("<c:pieChart><c:varyColors val=\"1\"/>{sers}{dl}<c:firstSliceAng val=\"0\"/></c:pieChart>"),
        G::Doughnut => {
            format!("<c:doughnutChart><c:varyColors val=\"1\"/>{sers}{dl}<c:firstSliceAng val=\"0\"/><c:holeSize val=\"50\"/></c:doughnutChart>")
        }
        G::Area { stacked } => format!(
            "<c:areaChart><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{sers}{dl}{ax}</c:areaChart>",
            if stacked { "stacked" } else { "standard" }
        ),
        G::Scatter { .. } => format!("<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{sers}{dl}{ax}</c:scatterChart>"),
        G::Bubble => {
            format!("<c:bubbleChart><c:varyColors val=\"0\"/>{sers}{dl}<c:bubbleScale val=\"100\"/><c:showNegBubbles val=\"0\"/>{ax}</c:bubbleChart>")
        }
        G::Radar => format!("<c:radarChart><c:radarStyle val=\"marker\"/><c:varyColors val=\"0\"/>{sers}{dl}{ax}</c:radarChart>"),
        G::Stock => format!("<c:stockChart>{sers}{dl}<c:hiLowLines/>{ax}</c:stockChart>"),
    }
}

fn title_xml(t: &str) -> String {
    let mut s = String::from("<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/>");
    for line in t.split('\n') {
        let _ = write!(s, "<a:p><a:pPr><a:defRPr/></a:pPr><a:r><a:t>{}</a:t></a:r></a:p>", esc(line));
    }
    s.push_str("</c:rich></c:tx><c:overlay val=\"0\"/></c:title>");
    s
}

struct Ax<'a> {
    cat: bool,
    id: u32,
    cross: u32,
    pos: &'a str,
    delete: bool,
    grid: bool,
    title: Option<&'a str>,
    crosses_max: bool,
}

fn axis_xml(a: &Ax<'_>) -> String {
    let mut s = format!(
        "<c:{0}><c:axId val=\"{1}\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"{2}\"/><c:axPos val=\"{3}\"/>",
        if a.cat { "catAx" } else { "valAx" },
        a.id,
        a.delete as u8,
        a.pos
    );
    if a.grid {
        s.push_str("<c:majorGridlines/>");
    }
    if let Some(t) = a.title {
        s.push_str(&title_xml(t));
    }
    let _ = write!(
        s,
        "<c:numFmt formatCode=\"General\" sourceLinked=\"1\"/><c:majorTickMark val=\"out\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:crossAx val=\"{}\"/><c:crosses val=\"{}\"/>",
        a.cross,
        if a.crosses_max { "max" } else { "autoZero" }
    );
    if a.cat {
        s.push_str("<c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx>");
    } else {
        s.push_str("<c:crossBetween val=\"between\"/></c:valAx>");
    }
    s
}

/// A PivotChart link without the workbook in brackets: `[Book.xlsx]Sheet2!PT` → `Sheet2!PT`.
fn strip_book(link: &str) -> &str {
    link.strip_prefix('[').and_then(|r| r.split_once(']')).map_or(link, |(_, rest)| rest)
}

/// Chart part XML for a model chart on sheet `si`.
pub fn write_chart(wb: &Workbook, si: usize, ch: &Chart, book_name: &str) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:roundedCorners val=\"0\"/>",
    );
    if (1..=48).contains(&ch.style) {
        let _ = write!(s, "<c:style val=\"{}\"/>", ch.style);
    }
    // A PivotChart names its PivotTable behind the workbook in brackets (`[Book.xlsx]Sheet2!PivotTable1`);
    // without the brackets Excel shows an ordinary chart.
    if let Some(link) = ch.pivot.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        let link = strip_book(link);
        let _ = write!(s, "<c:pivotSource><c:name>[{}]{}</c:name><c:fmtId val=\"0\"/></c:pivotSource>", esc(book_name), esc(link));
    }
    s.push_str("<c:chart>");
    // `None` is the automatic title; an empty title is one that was turned off.
    match &ch.title {
        Some(t) if !t.is_empty() => {
            s.push_str(&title_xml(t));
            s.push_str("<c:autoTitleDeleted val=\"0\"/>");
        }
        Some(_) => s.push_str("<c:autoTitleDeleted val=\"1\"/>"),
        None => s.push_str("<c:autoTitleDeleted val=\"0\"/>"),
    }
    s.push_str("<c:plotArea><c:layout/>");
    let all: Vec<(usize, &Series)> = ch.series.iter().enumerate().collect();
    let primary_g;
    let mut secondary: Option<G> = None;
    if ch.kind == ChartKind::Combo {
        // Series without a kind of their own: the first is a column, the others lines (as drawn).
        let kind = |i: usize, x: &Series| x.kind.filter(|k| *k != ChartKind::Combo).or((i > 0).then_some(ChartKind::Line));
        let is_line = |k: Option<ChartKind>| matches!(k.map(group_for), Some(G::Line { .. } | G::Area { .. } | G::Radar | G::Scatter { .. }));
        let bars: Vec<(usize, &Series)> = all.iter().filter(|(i, x)| !x.secondary && !is_line(kind(*i, x))).copied().collect();
        let lines: Vec<(usize, &Series)> = all.iter().filter(|(i, x)| !x.secondary && is_line(kind(*i, x))).copied().collect();
        let sec: Vec<(usize, &Series)> = all.iter().filter(|(_, x)| x.secondary).copied().collect();
        primary_g = G::Bar { bar: false, grouping: "clustered" };
        if !bars.is_empty() || (lines.is_empty() && sec.is_empty()) {
            s.push_str(&group_xml(primary_g, &bars, ch, (1, 2), wb, si));
        }
        if !lines.is_empty() {
            s.push_str(&group_xml(G::Line { stacked: false, markers: false }, &lines, ch, (1, 2), wb, si));
        }
        if !sec.is_empty() {
            let g = match sec.first().and_then(|(_, x)| x.kind).map(group_for) {
                Some(b @ G::Bar { .. }) => b,
                _ => G::Line { stacked: false, markers: false },
            };
            s.push_str(&group_xml(g, &sec, ch, (3, 4), wb, si));
            secondary = Some(g);
        }
    } else {
        primary_g = match group_for(ch.kind) {
            // A stock chart has 3 or 4 series (high-low-close, open-high-low-close).
            G::Stock if !(3..=4).contains(&ch.series.len()) => G::Line { stacked: false, markers: false },
            g => g,
        };
        s.push_str(&group_xml(primary_g, &all, ch, (1, 2), wb, si));
    }
    match primary_g {
        G::Pie | G::Doughnut => {}
        G::Scatter { .. } | G::Bubble => {
            s.push_str(&axis_xml(&Ax {
                cat: false,
                id: 1,
                cross: 2,
                pos: "b",
                delete: false,
                grid: false,
                title: ch.x_title.as_deref(),
                crosses_max: false,
            }));
            s.push_str(&axis_xml(&Ax {
                cat: false,
                id: 2,
                cross: 1,
                pos: "l",
                delete: false,
                grid: ch.gridlines,
                title: ch.y_title.as_deref(),
                crosses_max: false,
            }));
        }
        G::Bar { bar: true, .. } => {
            s.push_str(&axis_xml(&Ax {
                cat: true,
                id: 1,
                cross: 2,
                pos: "l",
                delete: false,
                grid: false,
                title: ch.x_title.as_deref(),
                crosses_max: false,
            }));
            s.push_str(&axis_xml(&Ax {
                cat: false,
                id: 2,
                cross: 1,
                pos: "b",
                delete: false,
                grid: ch.gridlines,
                title: ch.y_title.as_deref(),
                crosses_max: false,
            }));
        }
        _ => {
            s.push_str(&axis_xml(&Ax {
                cat: true,
                id: 1,
                cross: 2,
                pos: "b",
                delete: false,
                grid: false,
                title: ch.x_title.as_deref(),
                crosses_max: false,
            }));
            s.push_str(&axis_xml(&Ax {
                cat: false,
                id: 2,
                cross: 1,
                pos: "l",
                delete: false,
                grid: ch.gridlines,
                title: ch.y_title.as_deref(),
                crosses_max: false,
            }));
        }
    }
    if secondary.is_some() {
        s.push_str(&axis_xml(&Ax { cat: true, id: 3, cross: 4, pos: "b", delete: true, grid: false, title: None, crosses_max: false }));
        s.push_str(&axis_xml(&Ax { cat: false, id: 4, cross: 3, pos: "r", delete: false, grid: false, title: None, crosses_max: true }));
    }
    s.push_str("</c:plotArea>");
    let pos = match ch.legend {
        LegendPos::None => None,
        LegendPos::Bottom => Some("b"),
        LegendPos::Top => Some("t"),
        LegendPos::Left => Some("l"),
        LegendPos::Right => Some("r"),
    };
    if let Some(p) = pos {
        let _ = write!(s, "<c:legend><c:legendPos val=\"{p}\"/><c:overlay val=\"0\"/></c:legend>");
    }
    s.push_str("<c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart></c:chartSpace>");
    s
}
