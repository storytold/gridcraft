//! Work and memory are bounded by the product of the chart limits (series x points, trendlines
//! per chart, hairline bars), and clean data is rendered without a copy.

use std::borrow::Cow;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{ChartKind, Series, Trendline, TrendlineKind, Workbook};

use super::{assert_sane, chart};
use crate::render::{sanitize, sanitized};
use crate::resolve::{MAX_POINTS, MAX_TOTAL_POINTS, MAX_TRENDLINES, points_per_series};
use crate::*;

fn data_of(series: usize, points: usize) -> ChartData {
    ChartData {
        palette: vec![],
        categories: (0..points).map(|i| i.to_string()).collect(),
        series: (0..series)
            .map(|s| SeriesData {
                name: format!("S{s}"),
                values: (0..points).map(|i| Some(((i * 7 + s * 13) % 100 + 1) as f64)).collect(),
                color: series_color(&Default::default(), s),
                ..SeriesData::default()
            })
            .collect(),
    }
}

fn series_rects(prims: &[Prim], color: Rgba) -> Vec<(f32, f32, f32, f32)> {
    prims
        .iter()
        .filter_map(|p| match p {
            Prim::Rect { x, y, w, h, fill: Some(f), .. } if *f == color => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_point_budget_keeps_every_series_with_fewer_points() {
    assert_eq!(points_per_series(1), MAX_POINTS);
    assert_eq!(points_per_series(20), MAX_POINTS);
    assert_eq!(points_per_series(21), MAX_TOTAL_POINTS / 21);
    assert_eq!(points_per_series(255), MAX_TOTAL_POINTS / 255);
    assert_eq!(points_per_series(0), MAX_POINTS);

    // 25 series x 100,000 points is 2.5 million: over budget.
    let d = data_of(25, MAX_POINTS);
    let kept = sanitized(&d);
    assert_eq!(kept.series.len(), 25, "no series is dropped");
    assert!(kept.series.iter().all(|s| s.values.len() == MAX_TOTAL_POINTS / 25));
    assert_eq!(kept.categories.len(), MAX_TOTAL_POINTS / 25);
    assert!(kept.series.iter().map(|s| s.values.len()).sum::<usize>() <= MAX_TOTAL_POINTS);
    // The first points are the ones kept.
    assert_eq!(kept.series[3].values[..5], d.series[3].values[..5]);

    // Drawing it stays bounded: hairline columns are merged per pixel column.
    let mut c = chart(ChartKind::ColumnClustered);
    c.data_labels = false;
    c.legend = LegendPos::None;
    let prims = render(&c, &d, 400.0, 300.0, &ApproxMeasure);
    assert_sane(&prims, 400.0, 300.0);
    assert!(prims.len() < 25 * 450 + 200, "{} primitives", prims.len());
}

#[test]
fn trendlines_are_limited_per_chart() {
    let line = |kind| TrendlineData { spec: Trendline { kind, ..Trendline::default() }, color: [1, 2, 3, 255] };
    let mut d = data_of(3, 30);
    for s in &mut d.series {
        s.trendlines = vec![line(TrendlineKind::Linear); 10];
    }
    let kept = sanitized(&d);
    let counts: Vec<usize> = kept.series.iter().map(|s| s.trendlines.len()).collect();
    assert_eq!(counts, vec![10, MAX_TRENDLINES - 10, 0], "series order decides which trendlines stay");
    let entries = legend_entries(&chart(ChartKind::Line), &kept);
    assert_eq!(entries.len(), 3 + MAX_TRENDLINES);
}

#[test]
fn hairline_columns_are_merged_and_wide_ones_are_not() {
    let mut c = chart(ChartKind::ColumnClustered);
    c.data_labels = false;
    c.legend = LegendPos::None;

    // Wide bars (>= 0.25 px): one rectangle per series and category, as before.
    let d = data_of(3, 40);
    let prims = render(&c, &d, 640.0, 400.0, &ApproxMeasure);
    for s in &d.series {
        assert_eq!(series_rects(&prims, s.color).len(), 40);
    }

    // 4 series x 20,000 categories in 400 px: the bars are hairlines.
    let d = data_of(4, 20_000);
    let prims = render(&c, &d, 400.0, 300.0, &ApproxMeasure);
    assert_sane(&prims, 400.0, 300.0);
    for s in &d.series {
        let n = series_rects(&prims, s.color).len();
        assert!((1..=420).contains(&n), "{n} rectangles for one series");
    }

    // The merged rectangles keep the envelope: alternating 1 and 10 draws the full-height bar.
    let mut d = data_of(1, 20_000);
    for (i, v) in d.series[0].values.iter_mut().enumerate() {
        *v = Some(if i % 2 == 0 { 1.0 } else { 10.0 });
    }
    let prims = render(&c, &d, 400.0, 300.0, &ApproxMeasure);
    let rects = series_rects(&prims, d.series[0].color);
    let tallest = rects.iter().map(|r| r.3).fold(0.0f32, f32::max);
    assert!(rects.len() > 100 && tallest > 100.0);
    assert!(rects.iter().all(|r| (r.3 - tallest).abs() < 0.01), "every pixel column shows the tall bar");
    // And together they cover the plot's width without a gap wider than a pixel.
    let mut xs: Vec<(f32, f32)> = rects.iter().map(|r| (r.0, r.0 + r.2)).collect();
    xs.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(xs.windows(2).all(|w| w[1].0 - w[0].1 < 1.0));

    // Horizontal bars too.
    let mut c = chart(ChartKind::BarClustered);
    c.data_labels = false;
    c.legend = LegendPos::None;
    let prims = render(&c, &d, 400.0, 300.0, &ApproxMeasure);
    let n = series_rects(&prims, d.series[0].color).len();
    assert!((1..=320).contains(&n), "{n} bars");
}

#[test]
fn clean_data_is_rendered_without_a_copy() {
    let d = data_of(3, 50);
    assert!(matches!(sanitized(&d), Cow::Borrowed(_)));

    // Dirty data takes the copying path and comes out as `sanitize` has always produced it.
    let mut dirty = data_of(3, 50);
    dirty.series[0].values[4] = Some(f64::NAN);
    dirty.series[1].values[5] = Some(f64::INFINITY);
    dirty.series[2].values[6] = Some(1e301);
    dirty.series[2].x = Some(vec![Some(f64::NEG_INFINITY), Some(2.0)]);
    dirty.series[1].sizes = Some(vec![Some(-1e305)]);
    let got = sanitized(&dirty);
    assert!(matches!(got, Cow::Owned(_)));
    assert_eq!(*got, sanitize(&dirty));
    assert_eq!(got.series[0].values[4], None);
    assert_eq!(got.series[2].values[6], Some(1e300));

    // Over the series cap (within the point budget) the same.
    let many = data_of(300, 3);
    let got = sanitized(&many);
    assert_eq!(got.series.len(), 255);
    assert_eq!(*got, sanitize(&many));
}

#[test]
fn resolve_keeps_every_series_within_the_point_budget() {
    // 40 series x 60,000 points is 2.4 million: over the 2 million budget.
    let mut wb = Workbook::new();
    if let Some(s) = wb.sheet_mut(0) {
        for r in 0..60_000u32 {
            s.set_value(CellRef::new(r, 0), Value::Number(f64::from(r)));
        }
    }
    let mut c = chart(ChartKind::ColumnClustered);
    c.series =
        (0..40).map(|_| Series { values: "Sheet1!$A$1:$A$1000000".into(), categories: Some("A1:A1000000".into()), ..Series::default() }).collect();
    let d = resolve(&wb, 0, &c);
    assert_eq!(d.series.len(), 40);
    assert!(d.series.iter().all(|s| s.values.len() == MAX_TOTAL_POINTS / 40));
    assert_eq!(d.categories.len(), MAX_TOTAL_POINTS / 40);
    assert_eq!(d.series[7].values.first(), Some(&Some(0.0)));
    // Trendlines: 16 per chart, in series order.
    c.series = (0..4).map(|_| Series { values: "A1:A5".into(), trendlines: vec![Default::default(); 9], ..Series::default() }).collect();
    let counts: Vec<usize> = resolve(&wb, 0, &c).series.iter().map(|s| s.trendlines.len()).collect();
    assert_eq!(counts, vec![9, 7, 0, 0]);
}
