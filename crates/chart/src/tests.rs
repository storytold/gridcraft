use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Anchor, Chart, ChartKind, Color, LegendPos, Series, SparklineKind, Style, Workbook};

use crate::*;

const ALL_KINDS: [ChartKind; 25] = [
    ChartKind::ColumnClustered,
    ChartKind::ColumnStacked,
    ChartKind::ColumnStacked100,
    ChartKind::BarClustered,
    ChartKind::BarStacked,
    ChartKind::BarStacked100,
    ChartKind::Line,
    ChartKind::LineMarkers,
    ChartKind::LineStacked,
    ChartKind::Pie,
    ChartKind::Doughnut,
    ChartKind::Area,
    ChartKind::AreaStacked,
    ChartKind::Scatter,
    ChartKind::ScatterLines,
    ChartKind::Bubble,
    ChartKind::Radar,
    ChartKind::Histogram,
    ChartKind::Waterfall,
    ChartKind::Funnel,
    ChartKind::Treemap,
    ChartKind::Sunburst,
    ChartKind::BoxWhisker,
    ChartKind::Stock,
    ChartKind::Combo,
];

fn chart(kind: ChartKind) -> Chart {
    Chart {
        id: 1,
        kind,
        anchor: Anchor::default(),
        title: Some("Quarterly Sales".into()),
        series: vec![],
        legend: LegendPos::Bottom,
        data_labels: true,
        gridlines: true,
        style: 0,
        x_title: Some("Quarter".into()),
        y_title: Some("Revenue".into()),
        source: None,
        by_rows: false,
    }
}

fn series(name: &str, values: &[f64], color: Rgba, kind: ChartKind) -> SeriesData {
    SeriesData {
        name: name.into(),
        values: values.iter().map(|v| Some(*v)).collect(),
        x: Some(values.iter().enumerate().map(|(i, _)| Some(i as f64 * 1.5 + 2.0)).collect()),
        sizes: Some(values.iter().map(|v| Some(v.abs() + 1.0)).collect()),
        color,
        kind,
        secondary: false,
        smooth: false,
        number_format: "$#,##0".into(),
    }
}

fn sample(kind: ChartKind) -> ChartData {
    let cats = ["Q1", "Q2", "Q3", "Q4", "Q5", "Q6"];
    let k2 = if kind == ChartKind::Combo { ChartKind::Line } else { kind };
    let k1 = if kind == ChartKind::Combo { ChartKind::ColumnClustered } else { kind };
    let mut b = series("East", &[38000.0, 30000.0, 45000.0, 61870.0, 52000.0, 41000.0], [200, 80, 40, 255], k2);
    b.secondary = kind == ChartKind::Combo;
    ChartData {
        categories: cats.iter().map(|s| s.to_string()).collect(),
        series: vec![
            series("North", &[12000.0, -4000.0, 23000.0, 31000.0, 18000.0, 27000.0], [20, 96, 130, 255], k1),
            b,
            series("South", &[9000.0, 11000.0, 8000.0, 15000.0, 21000.0, 17000.0], [25, 107, 36, 255], k2),
        ],
    }
}

fn coords(p: &Prim) -> Vec<f32> {
    match p {
        Prim::Rect { x, y, w, h, .. } => vec![*x, *y, x + w, y + h],
        Prim::Line { pts, width, .. } => pts.iter().flat_map(|p| [p[0], p[1]]).chain([*width]).collect(),
        Prim::Polygon { pts, .. } => pts.iter().flat_map(|p| [p[0], p[1]]).collect(),
        Prim::Wedge { cx, cy, r_outer, r_inner, a0, a1, .. } => vec![*cx, *cy, *r_outer, *r_inner, *a0, *a1],
        Prim::Circle { cx, cy, r, .. } => vec![*cx, *cy, *r],
        Prim::Text { x, y, size, rotation, .. } => vec![*x, *y, *size, *rotation],
    }
}

fn assert_sane(prims: &[Prim], w: f32, h: f32) {
    let eps = 0.01;
    for p in prims {
        assert!(coords(p).iter().all(|v| v.is_finite()), "non-finite: {p:?}");
        let inside = |x: f32, y: f32| x >= -eps && x <= w + eps && y >= -eps && y <= h + eps;
        match p {
            Prim::Rect { x, y, w: rw, h: rh, .. } => assert!(inside(*x, *y) && inside(x + rw, y + rh), "{p:?}"),
            Prim::Line { pts, .. } | Prim::Polygon { pts, .. } => assert!(pts.iter().all(|q| inside(q[0], q[1])), "{p:?}"),
            Prim::Wedge { cx, cy, r_outer, .. } => assert!(inside(cx - r_outer, cy - r_outer) && inside(cx + r_outer, cy + r_outer), "{p:?}"),
            Prim::Circle { cx, cy, r, .. } => assert!(inside(cx - r, cy - r) && inside(cx + r, cy + r), "{p:?}"),
            Prim::Text { x, y, .. } => assert!(inside(*x, *y), "{p:?}"),
        }
    }
}

fn texts(prims: &[Prim]) -> Vec<&str> {
    prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn every_kind_renders_sanely() {
    let m = ApproxMeasure;
    for kind in ALL_KINDS {
        for legend in [LegendPos::Bottom, LegendPos::Top, LegendPos::Left, LegendPos::Right] {
            let mut c = chart(kind);
            c.legend = legend;
            let d = sample(kind);
            let (w, h) = (640.0, 400.0);
            let prims = render(&c, &d, w, h, &m);
            assert!(prims.len() > 3, "{kind:?}: too few prims");
            assert_sane(&prims, w, h);
            let t = texts(&prims);
            assert!(t.contains(&"Quarterly Sales"), "{kind:?}: no title");
            let entries = legend_entries(&c, &d);
            for (name, _, _) in &entries {
                assert!(t.contains(&name.as_str()), "{kind:?} {legend:?}: legend entry {name} missing");
            }
            let n_shapes = prims.iter().filter(|p| !matches!(p, Prim::Text { .. })).count();
            assert!(n_shapes > 2, "{kind:?}: nothing drawn");
        }
    }
}

#[test]
fn legend_counts() {
    let d = sample(ChartKind::ColumnClustered);
    assert_eq!(legend_entries(&chart(ChartKind::ColumnClustered), &d).len(), 3);
    assert_eq!(legend_entries(&chart(ChartKind::Pie), &d).len(), 6);
    assert_eq!(legend_entries(&chart(ChartKind::Waterfall), &d).len(), 3);
    assert_eq!(legend_entries(&chart(ChartKind::Histogram), &d).len(), 0);
    let mut c = chart(ChartKind::Line);
    c.legend = LegendPos::None;
    assert!(legend_entries(&c, &d).is_empty());
}

#[test]
fn nice_axis() {
    let s = nice_scale(0.0, 61870.0, true);
    assert_eq!((s.min, s.max, s.step), (0.0, 70000.0, 10000.0));
    let s = nice_scale(0.0, 100.0, true);
    assert_eq!((s.min, s.max, s.step), (0.0, 120.0, 20.0));
    let s = nice_scale(-4000.0, 31000.0, true);
    assert!(s.min < -4000.0 && s.min >= -10000.0 && s.max >= 31000.0);
    // All zero.
    let s = nice_scale(0.0, 0.0, true);
    assert_eq!((s.min, s.max), (0.0, 1.0));
    // Far from zero: no forced zero.
    let s = nice_scale(50.0, 55.0, false);
    assert!(s.min >= 45.0 && s.max <= 60.0, "{s:?}");
    // Close to zero: zero included.
    assert_eq!(nice_scale(10.0, 90.0, false).min, 0.0);
    // Negative only.
    let s = nice_scale(-80.0, -10.0, false);
    assert_eq!(s.max, 0.0);
    assert!(s.min <= -80.0);
    for (lo, hi) in [(0.0, 1e-300), (-1e308, 1e308), (f64::NAN, 5.0), (0.001, 0.0013), (3.0, 3.0), (-7.0, -7.0), (1e15, 1e15 + 1.0)] {
        let s = nice_scale(lo, hi, false);
        assert!(s.min.is_finite() && s.max.is_finite() && s.step.is_finite() && s.max > s.min && s.step > 0.0, "{lo} {hi} {s:?}");
        let n = s.ticks().len();
        assert!((2..=51).contains(&n));
    }
    // Ticks count in 4..=10 for typical data.
    for hi in [1.0, 7.0, 13.0, 99.0, 250.0, 1234.0, 61870.0, 999999.0] {
        let n = nice_scale(0.0, hi, true).ticks().len();
        assert!((4..=10).contains(&n), "{hi}: {n}");
    }
}

#[test]
fn axis_labels_use_number_format() {
    assert_eq!(format_number(40000.0, "$#,##0", Some(10000.0)), "$40,000");
    assert_eq!(format_number(0.30000000000000004, "General", Some(0.1)), "0.3");
    assert_eq!(format_number(1e-17, "General", Some(0.2)), "0");
    let c = chart(ChartKind::ColumnClustered);
    let prims = render(&c, &sample(ChartKind::ColumnClustered), 640.0, 400.0, &ApproxMeasure);
    let t = texts(&prims);
    assert!(t.contains(&"$70,000"), "{t:?}");
    assert!(t.contains(&"$40,000"));
}

#[test]
fn resolve_from_workbook() {
    let mut wb = Workbook::new();
    let cur = wb.styles.intern(Style { num_fmt: gridcraft_model::NumFmt(Arc::from("$#,##0")), ..Style::default() });
    {
        let s = wb.sheet_mut(0).unwrap();
        s.name = "Sales".into();
        s.set_value(CellRef::new(0, 1), Value::from("Revenue"));
        for (i, (q, v)) in [("Q1", 100.0), ("Q2", 250.0), ("Q3", 175.0)].iter().enumerate() {
            let r = i as u32 + 1;
            s.set_value(CellRef::new(r, 0), Value::from(*q));
            s.set_value(CellRef::new(r, 1), Value::Number(*v));
            s.set_style(CellRef::new(r, 1), cur);
        }
        s.set_value(CellRef::new(2, 2), Value::Number(5.0));
    }
    let mut c = chart(ChartKind::ColumnClustered);
    c.series = vec![
        Series {
            name: Some("Sales!$B$1".into()),
            categories: Some("Sales!$A$2:$A$4".into()),
            values: "Sales!$B$2:$B$4".into(),
            bubble_sizes: None,
            color: None,
            secondary: false,
            kind: None,
            smooth: false,
        },
        Series {
            name: None,
            categories: None,
            values: "=Sales!$C$2:$C$4".into(),
            bubble_sizes: None,
            color: Some(Color::rgb(1, 2, 3)),
            secondary: true,
            kind: None,
            smooth: false,
        },
        Series {
            name: Some("Literal".into()),
            categories: None,
            values: "{1,2,3,4}".into(),
            bubble_sizes: None,
            color: None,
            secondary: false,
            kind: None,
            smooth: false,
        },
    ];
    let d = resolve(&wb, 0, &c);
    assert_eq!(d.categories, vec!["Q1", "Q2", "Q3"]);
    assert_eq!(d.series.len(), 3);
    let s0 = &d.series[0];
    assert_eq!(s0.name, "Revenue");
    assert_eq!(s0.values, vec![Some(100.0), Some(250.0), Some(175.0)]);
    assert_eq!(s0.number_format, "$#,##0");
    assert_eq!(s0.color, series_color(&wb.theme, 0));
    let t = wb.theme.colors[4];
    assert_eq!(s0.color, [(t >> 16) as u8, (t >> 8) as u8, t as u8, 255]);
    let s1 = &d.series[1];
    assert_eq!(s1.name, "Series2");
    assert_eq!(s1.values, vec![None, Some(5.0), None]);
    assert_eq!(s1.color, [1, 2, 3, 255]);
    assert!(s1.secondary);
    assert_eq!(d.series[2].name, "Literal");
    assert_eq!(d.series[2].values.len(), 4);

    // Default categories.
    c.series.remove(0);
    let d = resolve(&wb, 0, &c);
    assert_eq!(d.categories, vec!["1", "2", "3", "4"]);

    // Combo kinds and scatter X values.
    let mut c = chart(ChartKind::Combo);
    c.series = vec![
        Series {
            name: None,
            categories: None,
            values: "Sales!B2:B4".into(),
            bubble_sizes: None,
            color: None,
            secondary: false,
            kind: None,
            smooth: false,
        },
        Series {
            name: None,
            categories: None,
            values: "Sales!B2:B4".into(),
            bubble_sizes: None,
            color: None,
            secondary: true,
            kind: None,
            smooth: false,
        },
    ];
    let d = resolve(&wb, 0, &c);
    assert_eq!(d.series[0].kind, ChartKind::ColumnClustered);
    assert_eq!(d.series[1].kind, ChartKind::Line);
    let mut c = chart(ChartKind::Bubble);
    c.series = vec![Series {
        name: None,
        categories: Some("Sales!B2:B4".into()),
        values: "Sales!B2:B4".into(),
        bubble_sizes: Some("{1,2,3}".into()),
        color: None,
        secondary: false,
        kind: None,
        smooth: false,
    }];
    let d = resolve(&wb, 0, &c);
    assert_eq!(d.series[0].x.as_ref().map(|x| x.len()), Some(3));
    assert_eq!(d.series[0].sizes, Some(vec![Some(1.0), Some(2.0), Some(3.0)]));

    // Bad formulas never panic.
    let mut c = chart(ChartKind::Line);
    c.series = vec![Series {
        name: Some("=Nope!A1".into()),
        categories: Some("((".into()),
        values: "Missing!A1:A3".into(),
        bubble_sizes: Some("".into()),
        color: None,
        secondary: false,
        kind: None,
        smooth: false,
    }];
    let d = resolve(&wb, 7, &c);
    assert_eq!(d.series.len(), 1);
    let _ = render(&c, &d, 300.0, 200.0, &ApproxMeasure);
}

#[test]
fn palette_beyond_six_series() {
    let theme = gridcraft_model::Theme::default();
    let p = default_palette(&theme, 14);
    assert_eq!(p.len(), 14);
    assert_ne!(p[0], p[6]);
    assert_ne!(p[6], p[12]);
}

#[test]
fn scatter_with_text_x_values_numbers_the_points() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    for (i, (label, v)) in [("North", 3.0), ("South", 5.0), ("East", 4.0)].iter().enumerate() {
        s.set_value(CellRef::new(i as u32, 0), Value::from(*label));
        s.set_value(CellRef::new(i as u32, 1), Value::Number(*v));
        s.set_value(CellRef::new(i as u32, 2), Value::Number(i as f64 * 10.0));
    }
    for kind in [ChartKind::Scatter, ChartKind::ScatterLines, ChartKind::Bubble] {
        let mut c = chart(kind);
        c.series = vec![Series { categories: Some("Sheet1!$A$1:$A$3".into()), values: "Sheet1!$B$1:$B$3".into(), ..Default::default() }];
        let d = resolve(&wb, 0, &c);
        assert_eq!(d.series[0].x, None, "{kind:?}");
        let prims = render(&c, &d, 400.0, 300.0, &ApproxMeasure);
        let empty = render(&c, &ChartData { categories: vec![], series: vec![] }, 400.0, 300.0, &ApproxMeasure);
        assert!(prims.len() > empty.len() + 2, "{kind:?} draws its points");
        // Numeric X values are kept.
        c.series[0].categories = Some("Sheet1!$C$1:$C$3".into());
        assert_eq!(resolve(&wb, 0, &c).series[0].x, Some(vec![Some(0.0), Some(10.0), Some(20.0)]));
    }
}

fn pixel(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2], img[i + 3]]
}

#[test]
fn rasterize_bar_center_has_bar_color() {
    let mut c = chart(ChartKind::ColumnClustered);
    c.data_labels = false;
    let d = sample(ChartKind::ColumnClustered);
    let (w, h) = (640u32, 400u32);
    let prims = render(&c, &d, w as f32, h as f32, &ApproxMeasure);
    let img = rasterize(&prims, w, h, [255, 255, 255, 255]);
    assert_eq!(img.len(), (w * h * 4) as usize);
    // Find the bars of the East series (tallest) and check their centres.
    let east = d.series[1].color;
    let bars: Vec<_> = prims
        .iter()
        .filter_map(|p| match p {
            Prim::Rect { x, y, w, h, fill: Some(f), .. } if *f == east && *w > 10.0 => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect();
    assert_eq!(bars.len(), 6);
    for (x, y, bw, bh) in bars {
        let px = pixel(&img, w, (x + bw / 2.0) as u32, (y + bh / 2.0) as u32);
        assert_eq!(px, east);
    }
    // Primitive-level checks.
    let img = rasterize(&[Prim::Circle { cx: 10.0, cy: 10.0, r: 5.0, fill: Some([255, 0, 0, 255]), stroke: None }], 20, 20, [0, 0, 0, 0]);
    assert_eq!(pixel(&img, 20, 10, 10), [255, 0, 0, 255]);
    assert_eq!(pixel(&img, 20, 0, 0)[3], 0);
    let wedge = Prim::Wedge {
        cx: 50.0,
        cy: 50.0,
        r_outer: 40.0,
        r_inner: 20.0,
        a0: 0.0,
        a1: std::f32::consts::FRAC_PI_2,
        fill: [0, 0, 255, 255],
        stroke: None,
    };
    let img = rasterize(&[wedge], 100, 100, [255, 255, 255, 255]);
    assert_eq!(pixel(&img, 100, 79, 30), [0, 0, 255, 255]); // upper-right quadrant ring
    assert_eq!(pixel(&img, 100, 50, 50), [255, 255, 255, 255]); // hole
    assert_eq!(pixel(&img, 100, 25, 70), [255, 255, 255, 255]); // other quadrant
    assert!(rasterize(&[], 0, 10, [0; 4]).is_empty());
    assert!(rasterize(&[], 100_000, 100_000, [0; 4]).is_empty());
}

#[test]
fn pie_starts_at_twelve_clockwise() {
    let c = chart(ChartKind::Pie);
    let d = sample(ChartKind::Pie);
    let prims = render(&c, &d, 400.0, 400.0, &ApproxMeasure);
    let wedges: Vec<_> = prims
        .iter()
        .filter_map(|p| match p {
            Prim::Wedge { a0, a1, r_inner, stroke, .. } => Some((*a0, *a1, *r_inner, *stroke)),
            _ => None,
        })
        .collect();
    assert_eq!(wedges.len(), 6);
    assert_eq!(wedges[0].0, 0.0);
    assert!(wedges.windows(2).all(|w| w[1].0 >= w[0].1 - 1e-4));
    assert!((wedges[5].1 - std::f32::consts::TAU).abs() < 1e-3);
    assert!(wedges.iter().all(|w| w.2 == 0.0 && w.3.map(|s| s.0) == Some([255; 4])));
    let prims = render(&chart(ChartKind::Doughnut), &d, 400.0, 400.0, &ApproxMeasure);
    let ok = prims.iter().any(|p| matches!(p, Prim::Wedge { r_outer, r_inner, .. } if (r_inner / r_outer - 0.5).abs() < 1e-3));
    assert!(ok);
}

#[test]
fn negative_values_put_axis_below_zero() {
    let c = chart(ChartKind::ColumnClustered);
    let d = ChartData { categories: vec![], series: vec![series("N", &[-5.0, 3.0, -2.0], [1, 2, 3, 255], ChartKind::ColumnClustered)] };
    let prims = render(&c, &d, 400.0, 300.0, &ApproxMeasure);
    let t = texts(&prims);
    assert!(t.iter().any(|s| s.starts_with('-') || s.starts_with("($") || s.starts_with("-$")), "{t:?}");
    assert!(t.contains(&"1") && t.contains(&"3"), "default categories: {t:?}");
}

#[test]
fn edge_cases_never_panic() {
    let m = ApproxMeasure;
    let empty = ChartData::default();
    let zeros = ChartData { categories: vec!["a".into(), "b".into()], series: vec![series("z", &[0.0, 0.0], [0, 0, 0, 255], ChartKind::Line)] };
    let weird = ChartData {
        categories: vec![String::new(); 3],
        series: vec![SeriesData {
            name: "w".repeat(500),
            values: vec![Some(f64::NAN), Some(f64::INFINITY), None, Some(-1e308), Some(1e308)],
            x: Some(vec![Some(f64::NAN)]),
            sizes: Some(vec![Some(-1.0), Some(0.0)]),
            ..SeriesData::default()
        }],
    };
    for kind in ALL_KINDS {
        let c = chart(kind);
        for d in [&empty, &zeros, &weird] {
            for (w, h) in [(0.0, 0.0), (-5.0, 100.0), (f32::NAN, 10.0), (1.0, 1.0), (20.0, 15.0), (640.0, 400.0), (5000.0, 30.0), (1e6, 1e6)] {
                let p = render(&c, d, w, h, &m);
                if !(w > 0.0 && h > 0.0) || w.is_nan() {
                    assert!(p.is_empty());
                } else {
                    assert_sane(&p, w, h);
                }
            }
        }
    }
}

#[test]
fn thousands_of_points_are_decimated() {
    let n = 50_000;
    let vals: Vec<f64> = (0..n).map(|i| ((i as f64) * 0.01).sin() * 100.0 + i as f64 * 0.001).collect();
    let mut c = chart(ChartKind::Line);
    c.data_labels = true;
    let d = ChartData { categories: vec![], series: vec![series("big", &vals, [9, 9, 9, 255], ChartKind::Line)] };
    let prims = render(&c, &d, 800.0, 400.0, &ApproxMeasure);
    let max_line = prims
        .iter()
        .filter_map(|p| match p {
            Prim::Line { pts, .. } => Some(pts.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    assert!(max_line > 10 && max_line <= 4 * 2000, "{max_line}");
    assert!(prims.len() < 10_000);
    for kind in [ChartKind::Scatter, ChartKind::Histogram, ChartKind::AreaStacked, ChartKind::BoxWhisker] {
        let p = render(&chart(kind), &d, 800.0, 400.0, &ApproxMeasure);
        assert_sane(&p, 800.0, 400.0);
    }
}

#[test]
fn smooth_line_adds_interpolated_points_and_keeps_endpoints() {
    let m = ApproxMeasure;
    let cats = ["a", "b", "c", "d"].map(String::from).to_vec();
    let line_pts = |smooth: bool, m: &ApproxMeasure| -> usize {
        let mut s = series("L", &[1.0, 5.0, 2.0, 6.0], [10, 20, 30, 255], ChartKind::Line);
        s.smooth = smooth;
        let d = ChartData { categories: cats.clone(), series: vec![s] };
        render(&chart(ChartKind::Line), &d, 600.0, 400.0, m)
            .iter()
            .filter_map(|p| match p {
                Prim::Line { pts, .. } => Some(pts.len()),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    };
    let straight = line_pts(false, &m);
    let curved = line_pts(true, &m);
    assert_eq!(straight, 4, "straight polyline keeps one point per datum");
    assert!(curved > straight, "smooth line interpolates extra points: {curved} vs {straight}");
    // Rendering either way must stay finite and inside the canvas.
    let mut s = series("L", &[1.0, 5.0, 2.0, 6.0], [10, 20, 30, 255], ChartKind::Line);
    s.smooth = true;
    let d = ChartData { categories: cats, series: vec![s] };
    assert_sane(&render(&chart(ChartKind::Line), &d, 600.0, 400.0, &m), 600.0, 400.0);
}

#[test]
fn smooth_line_splits_at_gaps() {
    let m = ApproxMeasure;
    let mut s = series("L", &[1.0, 5.0, 2.0, 6.0], [10, 20, 30, 255], ChartKind::Line);
    s.smooth = true;
    s.values[1] = None; // gap between two data points
    let d = ChartData { categories: vec!["a".into(), "b".into(), "c".into(), "d".into()], series: vec![s] };
    let prims = render(&chart(ChartKind::Line), &d, 600.0, 400.0, &m);
    // Two separate runs (before/after the gap) → at least two line primitives.
    let lines = prims.iter().filter(|p| matches!(p, Prim::Line { .. })).count();
    assert!(lines >= 2, "a gap must split the smoothed line into runs, got {lines}");
    assert_sane(&prims, 600.0, 400.0);
}

/// Small deterministic PRNG (xorshift) for the fuzz test.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f64 {
        (self.next() % 1_000_000) as f64 / 1_000_000.0
    }
}

#[test]
fn fuzz_random_data_and_sizes() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let m = ApproxMeasure;
    for iter in 0..400 {
        let kind = ALL_KINDS[(r.next() % ALL_KINDS.len() as u64) as usize];
        let nser = (r.next() % 6) as usize;
        let npts = (r.next() % 40) as usize;
        let mag = 10f64.powi((r.next() % 30) as i32 - 10);
        let val = |r: &mut Rng| -> Option<f64> {
            match r.next() % 12 {
                0 => None,
                1 => Some(f64::NAN),
                2 => Some(0.0),
                3 => Some(-r.f() * mag),
                _ => Some(r.f() * mag),
            }
        };
        let series: Vec<SeriesData> = (0..nser)
            .map(|i| SeriesData {
                name: format!("S{i}"),
                values: (0..npts).map(|_| val(&mut r)).collect(),
                x: if r.next().is_multiple_of(2) { Some((0..npts).map(|_| val(&mut r)).collect()) } else { None },
                sizes: if r.next().is_multiple_of(2) { Some((0..npts).map(|_| val(&mut r)).collect()) } else { None },
                color: [(r.next() % 256) as u8, 100, 50, 255],
                kind: ALL_KINDS[(r.next() % ALL_KINDS.len() as u64) as usize],
                secondary: r.next().is_multiple_of(3),
                smooth: r.next().is_multiple_of(4),
                number_format: ["General", "$#,##0", "0.00%", "yyyy-mm-dd", "#,##0.0;[Red]-#,##0.0", "@"][(r.next() % 6) as usize].into(),
            })
            .collect();
        let ncat = (r.next() % 50) as usize;
        let d = ChartData { categories: (0..ncat).map(|i| "c".repeat(i % 7) + &i.to_string()).collect(), series };
        let mut c = chart(kind);
        c.legend = [LegendPos::None, LegendPos::Bottom, LegendPos::Top, LegendPos::Left, LegendPos::Right][(r.next() % 5) as usize];
        c.data_labels = r.next().is_multiple_of(2);
        c.gridlines = r.next().is_multiple_of(2);
        if r.next().is_multiple_of(3) {
            c.title = None;
        }
        let w = [0.0, -3.0, 1.0, 12.0, 80.0, 320.0, 1200.0][(r.next() % 7) as usize] as f32 + r.f() as f32 * 10.0;
        let h = [0.0, 2.0, 30.0, 200.0, 900.0][(r.next() % 5) as usize] as f32 + r.f() as f32 * 10.0;
        let p = render(&c, &d, w, h, &m);
        if w >= 1.0 && h >= 1.0 {
            assert_sane(&p, w, h);
        }
        if iter % 20 == 0 && w >= 1.0 && h >= 1.0 && w < 2000.0 && h < 2000.0 {
            let img = rasterize(&p, w as u32, h as u32, [255; 4]);
            assert_eq!(img.len(), (w as u32 * h as u32 * 4) as usize);
        }
        let vals: Vec<Option<f64>> = (0..npts).map(|_| val(&mut r)).collect();
        for k in [SparklineKind::Line, SparklineKind::Column, SparklineKind::WinLoss] {
            let sp = render_sparkline(k, &vals, [0, 0, 0, 255], r.next().is_multiple_of(2), w, h);
            if w >= 1.0 && h >= 1.0 {
                assert_sane(&sp, w.max(1.0), h.max(1.0));
            }
        }
    }
}

#[test]
fn sparklines() {
    let v = [Some(1.0), Some(-2.0), None, Some(4.0), Some(0.0), Some(3.0)];
    let line = render_sparkline(SparklineKind::Line, &v, [1, 2, 3, 255], true, 60.0, 16.0);
    assert_eq!(line.iter().filter(|p| matches!(p, Prim::Line { .. })).count(), 2);
    assert_eq!(line.iter().filter(|p| matches!(p, Prim::Circle { .. })).count(), 5);
    assert_sane(&line, 60.0, 16.0);
    let col = render_sparkline(SparklineKind::Column, &v, [1, 2, 3, 255], false, 60.0, 16.0);
    assert_eq!(col.len(), 5);
    assert_sane(&col, 60.0, 16.0);
    let wl = render_sparkline(SparklineKind::WinLoss, &v, [1, 2, 3, 255], false, 60.0, 16.0);
    assert_eq!(wl.len(), 4);
    assert!(render_sparkline(SparklineKind::Line, &v, [0; 4], false, 0.0, 10.0).is_empty());
    assert!(render_sparkline(SparklineKind::Line, &[], [0; 4], false, 10.0, 10.0).is_empty());
}

#[test]
fn treemap_and_histogram_shapes() {
    let d = ChartData {
        categories: vec!["a".into(), "b".into(), "c".into(), "d".into()],
        series: vec![series("t", &[6.0, 6.0, 4.0, 3.0], [10, 20, 30, 255], ChartKind::Treemap)],
    };
    let mut c = chart(ChartKind::Treemap);
    c.title = None;
    c.legend = LegendPos::None;
    let prims = render(&c, &d, 600.0, 400.0, &ApproxMeasure);
    let rects: Vec<f32> = prims
        .iter()
        .skip(1)
        .filter_map(|p| match p {
            Prim::Rect { w, h, .. } => Some(w * h),
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 4);
    // Areas proportional to values.
    assert!((rects[0] / rects[3] - 2.0).abs() < 0.05, "{rects:?}");

    let vals: Vec<f64> = (0..100).map(|i| i as f64).collect();
    let d = ChartData { categories: vec![], series: vec![series("h", &vals, [10, 20, 30, 255], ChartKind::Histogram)] };
    let prims = render(&chart(ChartKind::Histogram), &d, 600.0, 400.0, &ApproxMeasure);
    let bins = prims.iter().filter(|p| matches!(p, Prim::Rect { fill: Some([10, 20, 30, 255]), .. })).count();
    assert_eq!(bins, 10);
}
