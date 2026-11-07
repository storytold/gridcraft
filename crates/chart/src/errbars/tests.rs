//! Error bar amounts (as Excel measures them) and how they are drawn.

use gridcraft_model::{Anchor, Chart, ChartKind, ErrorAmount, ErrorBarDirection, ErrorBarSides, ErrorBars, LegendPos};

use super::{ErrorSpan, error_spans};
use crate::{ApproxMeasure, ChartData, ErrorBarData, Prim, Rgba, SeriesData, render};

const EB: Rgba = [1, 2, 3, 255];
const S1: [f64; 8] = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];

fn bars(amount: ErrorAmount) -> ErrorBarData {
    ErrorBarData { spec: ErrorBars { amount, ..ErrorBars::default() }, color: EB, ..ErrorBarData::default() }
}

fn vals(v: &[f64]) -> Vec<Option<f64>> {
    v.iter().map(|v| Some(*v)).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn standard_deviation_bars_are_centred_on_the_series_mean() {
    // Excel draws the same bar at every point: mean 5 ± one sample standard deviation.
    let sp = error_spans(&bars(ErrorAmount::StdDev(1.0)), &vals(&S1));
    let sd = (32.0f64 / 7.0).sqrt();
    for s in sp.iter().flatten() {
        assert_eq!(s.center, Some(5.0));
        assert!(close(s.minus, sd) && close(s.plus, sd), "{s:?}");
    }
    let two = error_spans(&bars(ErrorAmount::StdDev(2.0)), &vals(&S1));
    assert!(close(two[0].unwrap().plus, 2.0 * sd));
}

#[test]
fn standard_error_is_the_sample_deviation_over_root_n_at_each_point() {
    let sp = error_spans(&bars(ErrorAmount::StdErr), &vals(&S1));
    let se = (32.0f64 / 7.0).sqrt() / 8f64.sqrt();
    for s in sp.iter().flatten() {
        assert_eq!(s.center, None);
        assert!(close(s.minus, se) && close(s.plus, se), "{s:?}");
    }
    // Blank points are skipped by the statistics and get no bar.
    let mut v = vals(&S1);
    v.push(None);
    let sp = error_spans(&bars(ErrorAmount::StdErr), &v);
    assert!(sp[8].is_none() && close(sp[0].unwrap().plus, se));
}

#[test]
fn fixed_percentage_and_sides() {
    let f = error_spans(&bars(ErrorAmount::Fixed(1.5)), &vals(&[3.0, -2.0]));
    assert_eq!(f[1], Some(ErrorSpan { center: None, minus: 1.5, plus: 1.5 }));
    // A percentage of the value's size, negative values too.
    let p = error_spans(&bars(ErrorAmount::Percentage(10.0)), &vals(&[50.0, -20.0]));
    assert!(close(p[0].unwrap().plus, 5.0) && close(p[1].unwrap().minus, 2.0));
    let mut plus = bars(ErrorAmount::Fixed(1.0));
    plus.spec.sides = ErrorBarSides::Plus;
    assert_eq!(error_spans(&plus, &vals(&[3.0]))[0], Some(ErrorSpan { center: None, minus: 0.0, plus: 1.0 }));
    plus.spec.sides = ErrorBarSides::Minus;
    assert_eq!(error_spans(&plus, &vals(&[3.0]))[0], Some(ErrorSpan { center: None, minus: 1.0, plus: 0.0 }));
}

#[test]
fn custom_amounts_come_per_point_and_a_single_value_applies_to_all() {
    let mut c = bars(ErrorAmount::Custom { plus: None, minus: None });
    c.plus = vals(&[0.5]);
    c.minus = vec![Some(1.0), None, Some(-2.0)];
    let sp = error_spans(&c, &vals(&[1.0, 2.0, 3.0, 4.0]));
    let got: Vec<(f64, f64)> = sp.iter().flatten().map(|s| (s.minus, s.plus)).collect();
    assert_eq!(got, vec![(1.0, 0.5), (0.0, 0.5), (2.0, 0.5), (0.0, 0.5)]);
}

#[test]
fn hostile_amounts_give_finite_lengths() {
    for a in [ErrorAmount::Fixed(f64::NAN), ErrorAmount::Fixed(f64::INFINITY), ErrorAmount::Percentage(-f64::INFINITY), ErrorAmount::StdDev(f64::NAN)]
    {
        for s in error_spans(&bars(a.clone()), &vals(&[1e308, -1e308, 0.0])).iter().flatten() {
            assert!(s.minus.is_finite() && s.plus.is_finite() && s.minus >= 0.0 && s.plus >= 0.0, "{a:?}: {s:?}");
        }
    }
    assert!(error_spans(&bars(ErrorAmount::StdErr), &[]).is_empty());
    assert!(error_spans(&bars(ErrorAmount::StdDev(1.0)), &[None, Some(f64::NAN)]).iter().all(Option::is_none));
}

// ---------------------------------------------------------------- drawing

fn chart(kind: ChartKind) -> Chart {
    Chart {
        id: 1,
        kind,
        anchor: Anchor::default(),
        title: None,
        series: vec![],
        legend: LegendPos::None,
        data_labels: false,
        data_table: None,
        gridlines: false,
        style: 0,
        hole_size: 50,
        first_slice_angle: 0,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
    }
}

fn series(values: &[f64], color: Rgba, kind: ChartKind, eb: Vec<ErrorBarData>) -> SeriesData {
    SeriesData { values: vals(values), color, kind, error_bars: eb, ..SeriesData::default() }
}

fn data(series: Vec<SeriesData>) -> ChartData {
    let n = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    ChartData { palette: vec![], category_levels: vec![], categories: (1..=n).map(|i| format!("c{i}")).collect(), series }
}

fn lines(prims: &[Prim], color: Rgba) -> Vec<[[f32; 2]; 2]> {
    prims
        .iter()
        .filter_map(|p| match p {
            Prim::Line { pts, color: c, .. } if *c == color && pts.len() == 2 => Some([pts[0], pts[1]]),
            _ => None,
        })
        .collect()
}

fn vertical(l: &[[f32; 2]; 2]) -> bool {
    (l[0][0] - l[1][0]).abs() < 0.01 && (l[0][1] - l[1][1]).abs() > 0.01
}

fn horizontal(l: &[[f32; 2]; 2]) -> bool {
    (l[0][1] - l[1][1]).abs() < 0.01 && (l[0][0] - l[1][0]).abs() > 0.01
}

fn draw(kind: ChartKind, d: &ChartData) -> Vec<Prim> {
    let prims = render(&chart(kind), d, 600.0, 400.0, &ApproxMeasure);
    for p in &prims {
        if let Prim::Line { pts, width, .. } = p {
            assert!(width.is_finite() && pts.iter().flatten().all(|v| v.is_finite() && (-0.01..=600.01).contains(v)), "{p:?}");
        }
    }
    prims
}

/// Top and bottom pixel of each vertical stem, left to right.
fn stems(prims: &[Prim]) -> Vec<(f32, f32, f32)> {
    let mut s: Vec<(f32, f32, f32)> =
        lines(prims, EB).iter().filter(|l| vertical(l)).map(|l| (l[0][0], l[0][1].min(l[1][1]), l[0][1].max(l[1][1]))).collect();
    s.sort_by(|a, b| a.0.total_cmp(&b.0));
    s
}

#[test]
fn line_chart_bars_run_from_each_point_with_caps() {
    let line: Rgba = [9, 9, 9, 255];
    let d = data(vec![series(&S1, line, ChartKind::Line, vec![bars(ErrorAmount::Fixed(1.0))])]);
    let prims = draw(ChartKind::Line, &d);
    let st = stems(&prims);
    assert_eq!(st.len(), 8);
    // The data line's points: the stems are centred on them and 2 units long.
    let pts = prims
        .iter()
        .find_map(|p| match p {
            Prim::Line { pts, color, .. } if *color == line => Some(pts.clone()),
            _ => None,
        })
        .unwrap();
    let unit = (pts[0][1] - pts[1][1]) / 2.0; // values 2 → 4
    for ((x, top, bottom), p) in st.iter().zip(&pts) {
        assert!((x - p[0]).abs() < 0.01);
        assert!(((top + bottom) / 2.0 - p[1]).abs() < 0.01 && ((bottom - top) - 2.0 * unit).abs() < 0.01, "{top} {bottom} {p:?}");
    }
    // Two caps a point.
    assert_eq!(lines(&prims, EB).iter().filter(|l| horizontal(l)).count(), 16);
    let mut nocap = bars(ErrorAmount::Fixed(1.0));
    nocap.spec.no_end_cap = true;
    let prims = draw(ChartKind::Line, &data(vec![series(&S1, line, ChartKind::Line, vec![nocap])]));
    assert_eq!(lines(&prims, EB).iter().filter(|l| horizontal(l)).count(), 0);
    // One-sided bars cap only their far end.
    let mut plus = bars(ErrorAmount::Fixed(1.0));
    plus.spec.sides = ErrorBarSides::Plus;
    let prims = draw(ChartKind::Line, &data(vec![series(&S1, line, ChartKind::Line, vec![plus])]));
    assert_eq!(lines(&prims, EB).iter().filter(|l| horizontal(l)).count(), 8);
}

#[test]
fn standard_deviation_bars_line_up_across_points() {
    let d = data(vec![series(&S1, [9, 9, 9, 255], ChartKind::LineMarkers, vec![bars(ErrorAmount::StdDev(1.0))])]);
    let st = stems(&draw(ChartKind::LineMarkers, &d));
    assert_eq!(st.len(), 8);
    assert!(st.iter().all(|s| (s.1 - st[0].1).abs() < 0.01 && (s.2 - st[0].2).abs() < 0.01), "{st:?}");
    // Plus only: up from the mean, capped at the top end alone (as Excel draws it).
    let mut plus = bars(ErrorAmount::StdDev(1.0));
    plus.spec.sides = ErrorBarSides::Plus;
    let prims = draw(ChartKind::LineMarkers, &data(vec![series(&S1, [9, 9, 9, 255], ChartKind::LineMarkers, vec![plus])]));
    let half = stems(&prims);
    assert!(half.iter().all(|s| (s.1 - st[0].1).abs() < 0.01 && (s.2 - (st[0].1 + st[0].2) / 2.0).abs() < 0.01), "{half:?}");
    assert_eq!(lines(&prims, EB).iter().filter(|l| horizontal(l)).count(), 8);
}

#[test]
fn the_value_axis_grows_to_hold_the_bars() {
    let line: Rgba = [9, 9, 9, 255];
    let without = draw(ChartKind::Line, &data(vec![series(&S1, line, ChartKind::Line, vec![])]));
    let with = draw(ChartKind::Line, &data(vec![series(&S1, line, ChartKind::Line, vec![bars(ErrorAmount::Fixed(20.0))])]));
    let labels = |p: &[Prim]| -> Vec<f64> {
        p.iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => text.replace(',', "").parse::<f64>().ok(),
                _ => None,
            })
            .collect()
    };
    let max = |v: Vec<f64>| v.into_iter().fold(f64::MIN, f64::max);
    let min = |v: Vec<f64>| v.into_iter().fold(f64::MAX, f64::min);
    assert!(max(labels(&without)) < 20.0 && max(labels(&with)) >= 29.0 && min(labels(&with)) <= -18.0);
    // Every bar fits inside the plot, uncut: all 16 caps are drawn.
    assert_eq!(lines(&with, EB).iter().filter(|l| horizontal(l)).count(), 16);
}

#[test]
fn column_bars_sit_on_their_own_column_and_bar_chart_bars_run_across() {
    let (a, b): (Rgba, Rgba) = ([10, 0, 0, 255], [20, 0, 0, 255]);
    let d = data(vec![
        series(&[3.0, 4.0], a, ChartKind::ColumnClustered, vec![]),
        series(&[5.0, 6.0], b, ChartKind::ColumnClustered, vec![bars(ErrorAmount::Fixed(1.0))]),
    ]);
    let prims = draw(ChartKind::ColumnClustered, &d);
    let cols: Vec<(f32, f32)> = prims
        .iter()
        .filter_map(|p| match p {
            Prim::Rect { x, y, w, fill: Some(f), .. } if *f == b => Some((x + w / 2.0, *y)),
            _ => None,
        })
        .collect();
    let st = stems(&prims);
    assert_eq!((cols.len(), st.len()), (2, 2));
    for ((cx, top), (x, t, bt)) in cols.iter().zip(&st) {
        assert!((cx - x).abs() < 0.01 && ((t + bt) / 2.0 - top).abs() < 0.01, "column {cx},{top} stem {x},{t},{bt}");
    }
    // Horizontal bars: the error bars run along the horizontal value axis.
    let prims = draw(ChartKind::BarClustered, &d);
    let eb = lines(&prims, EB);
    assert_eq!(eb.iter().filter(|l| horizontal(l)).count(), 2);
    assert_eq!(eb.iter().filter(|l| vertical(l)).count(), 4, "caps across");
}

#[test]
fn stacked_bars_start_at_the_stack_top() {
    let (a, b): (Rgba, Rgba) = ([10, 0, 0, 255], [20, 0, 0, 255]);
    let d = data(vec![
        series(&[3.0], a, ChartKind::ColumnStacked, vec![]),
        series(&[5.0], b, ChartKind::ColumnStacked, vec![bars(ErrorAmount::Fixed(1.0))]),
    ]);
    let prims = draw(ChartKind::ColumnStacked, &d);
    let top = prims
        .iter()
        .find_map(|p| match p {
            Prim::Rect { y, fill: Some(f), .. } if *f == b => Some(*y),
            _ => None,
        })
        .unwrap();
    let st = stems(&prims);
    assert_eq!(st.len(), 1);
    assert!(((st[0].1 + st[0].2) / 2.0 - top).abs() < 0.01);
    // 100% stacked charts draw none (their values are shares, not data units).
    assert!(lines(&draw(ChartKind::ColumnStacked100, &d), EB).is_empty());
    assert!(lines(&draw(ChartKind::AreaStacked100, &d), EB).is_empty());
}

#[test]
fn scatter_charts_draw_x_and_y_bars() {
    let mut s = series(&[1.0, 4.0, 9.0], [9, 9, 9, 255], ChartKind::Scatter, vec![]);
    s.x = Some(vals(&[1.0, 2.0, 3.0]));
    let mut x = bars(ErrorAmount::Percentage(10.0));
    x.spec.direction = ErrorBarDirection::X;
    s.error_bars = vec![x, bars(ErrorAmount::StdErr)];
    let prims = draw(ChartKind::Scatter, &data(vec![s.clone()]));
    let eb = lines(&prims, EB);
    // Per point: one horizontal X stem with two vertical caps, one vertical Y stem with two horizontal caps.
    assert_eq!(eb.iter().filter(|l| horizontal(l)).count(), 9);
    assert_eq!(eb.iter().filter(|l| vertical(l)).count(), 9);
    // Bubble charts too.
    s.kind = ChartKind::Bubble;
    assert_eq!(lines(&draw(ChartKind::Bubble, &data(vec![s])), EB).len(), 18);
}

#[test]
fn pies_and_hostile_bars_draw_nothing_odd() {
    let d = data(vec![series(&S1, [9, 9, 9, 255], ChartKind::Pie, vec![bars(ErrorAmount::Fixed(1.0))])]);
    assert!(lines(&draw(ChartKind::Pie, &d), EB).is_empty());
    for k in [
        ChartKind::ColumnClustered,
        ChartKind::BarStacked,
        ChartKind::Line,
        ChartKind::Area,
        ChartKind::AreaStacked,
        ChartKind::Scatter,
        ChartKind::Bubble,
    ] {
        for a in [ErrorAmount::Fixed(f64::NAN), ErrorAmount::Fixed(1e300), ErrorAmount::Percentage(f64::INFINITY), ErrorAmount::StdDev(1e200)] {
            let mut e = bars(a);
            e.spec.width = Some(f32::NAN);
            let mut s = series(&[1e300, -1e300, 0.0, f64::MAX], [9, 9, 9, 255], k, vec![e.clone()]);
            s.error_bars.push(ErrorBarData { spec: ErrorBars { direction: ErrorBarDirection::X, ..e.spec.clone() }, ..e });
            draw(k, &data(vec![s]));
        }
    }
}

#[test]
fn bubbles_that_are_not_drawn_have_no_error_bars() {
    let mut s = series(&[1.0, 2.0, 3.0, 4.0], [9, 9, 9, 255], ChartKind::Bubble, vec![bars(ErrorAmount::Fixed(0.5))]);
    s.x = Some(vals(&[1.0, 2.0, 3.0, 4.0]));
    // Sizes: drawn, missing, zero, drawn.
    s.sizes = Some(vec![Some(1.0), None, Some(0.0), Some(2.0)]);
    let eb = lines(&draw(ChartKind::Bubble, &data(vec![s.clone()])), EB);
    // One vertical stem per drawn bubble (the caps are horizontal).
    assert_eq!(eb.iter().filter(|l| vertical(l)).count(), 2);
    // Without sizes every bubble is drawn.
    s.sizes = None;
    assert_eq!(lines(&draw(ChartKind::Bubble, &data(vec![s])), EB).iter().filter(|l| vertical(l)).count(), 4);
}

#[test]
fn a_chart_draws_a_bounded_number_of_error_bars() {
    let vals: Vec<f64> = (0..10_000).map(|i| (i % 50) as f64).collect();
    let d = data((0..3).map(|_| series(&vals, [9, 9, 9, 255], ChartKind::Line, vec![bars(ErrorAmount::Fixed(1.0))])).collect());
    // Uncapped this is 30 000 bars of three lines; the budget is 20 000 bars for the whole chart.
    let n = lines(&draw(ChartKind::Line, &d), EB).len();
    assert!(n > 0 && n <= 60_000, "{n}");
}

#[test]
fn bubbles_past_the_marker_cap_have_no_error_bars() {
    let n = 6000;
    let mut s = series(&vec![1.0; n], [9, 9, 9, 255], ChartKind::Bubble, vec![bars(ErrorAmount::Fixed(0.5))]);
    s.x = Some(vals(&(0..n).map(|i| i as f64).collect::<Vec<_>>()));
    let eb = lines(&draw(ChartKind::Bubble, &data(vec![s])), EB);
    // Only the first 5000 bubbles are drawn, so only they carry a bar (one vertical stem each).
    assert_eq!(eb.iter().filter(|l| vertical(l)).count(), 5000);
}
