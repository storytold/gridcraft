//! Per-point colours as Excel draws them (observed on synthetic charts, see chart-parity.md).

use gridcraft_model::{Anchor, Chart, ChartKind, Color, DataPoint, LegendPos, MarkerFormat, MarkerSymbol, Series, Workbook};

use crate::*;

const RED: Rgba = [0xFF, 0, 0, 0xFF];
const BLUE: Rgba = [0, 0, 0xFF, 0xFF];
const GREEN: Rgba = [0, 0x80, 0, 0xFF];
const YELLOW: Rgba = [0xFF, 0xFF, 0, 0xFF];
const WHITE: Rgba = [0xFF, 0xFF, 0xFF, 0xFF];

fn col(c: Rgba) -> Color {
    Color::rgb(c[0], c[1], c[2])
}

fn chart(kind: ChartKind, series: Vec<Series>) -> Chart {
    Chart {
        id: 1,
        kind,
        anchor: Anchor::default(),
        title: None,
        series,
        legend: LegendPos::Right,
        data_labels: false,
        gridlines: false,
        style: 2,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
        vary_colors: None,
    }
}

fn ser(values: &str) -> Series {
    Series { categories: Some("{\"w\",\"x\",\"y\",\"z\"}".into()), values: values.into(), ..Series::default() }
}

fn draw(c: &Chart) -> (ChartData, Vec<Prim>) {
    let wb = Workbook::default();
    let d = resolve(&wb, 0, c);
    let prims = render(c, &d, 480.0, 300.0, &ApproxMeasure);
    (d, prims)
}

/// Fills of the bars (rects wider than the legend swatches), left to right.
fn bars(prims: &[Prim]) -> Vec<(Rgba, Option<(Rgba, f32)>)> {
    let mut v: Vec<(f32, Rgba, Option<(Rgba, f32)>)> = prims
        .iter()
        .filter_map(|p| match p {
            Prim::Rect { x, w, h, fill: Some(f), stroke, .. } if *w > 12.0 && *w < 200.0 && *h > 0.0 => Some((*x, *f, *stroke)),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().map(|(_, f, s)| (f, s)).collect()
}

fn wedges(prims: &[Prim]) -> Vec<(Rgba, [f32; 2])> {
    prims.iter().filter_map(|p| if let Prim::Wedge { fill, cx, cy, .. } = p { Some((*fill, [*cx, *cy])) } else { None }).collect()
}

fn palette(n: usize) -> Vec<Rgba> {
    default_palette(&Workbook::default().theme, n)
}

#[test]
fn bar_point_fill_and_border() {
    let mut s = ser("{5,3,4,2}");
    s.points =
        vec![DataPoint { index: 0, fill: Some(col(RED)), ..Default::default() }, DataPoint { index: 2, line: Some(col(BLUE)), ..Default::default() }];
    let (_, p) = draw(&chart(ChartKind::ColumnClustered, vec![s]));
    let b = bars(&p);
    let auto = palette(1)[0];
    assert_eq!(b.iter().map(|x| x.0).collect::<Vec<_>>(), vec![RED, auto, auto, auto]);
    assert_eq!(b[2].1.map(|s| s.0), Some(BLUE), "point border");
    assert_eq!(b[1].1, None);
}

#[test]
fn invert_if_negative_draws_white_outlined_bars_unless_an_inverted_fill_applies() {
    // Automatic fill: white with an outline (the c14 colour is ignored without a series fill).
    let mut a = ser("{5,-3,4,-2}");
    a.invert_if_negative = true;
    a.invert_color = Some(col(YELLOW));
    let (_, p) = draw(&chart(ChartKind::ColumnClustered, vec![a]));
    let b = bars(&p);
    assert_eq!(b[1].0, WHITE);
    assert!(b[1].1.is_some(), "white inverted bars are outlined");
    assert_eq!(b[3].0, WHITE);
    assert_ne!(b[0].0, WHITE);
    // A series fill with an inverted colour: that colour.
    let mut g = ser("{5,-3,4,-2}");
    g.color = Some(col(GREEN));
    g.invert_if_negative = true;
    g.invert_color = Some(col(YELLOW));
    let b = bars(&draw(&chart(ChartKind::BarClustered, vec![g.clone()])).1);
    assert_eq!(b.iter().filter(|x| x.0 == YELLOW).count(), 2);
    assert!(b.iter().filter(|x| x.0 == YELLOW).all(|x| x.1.is_none()));
    // A series fill without one: white.
    g.invert_color = None;
    let b = bars(&draw(&chart(ChartKind::ColumnClustered, vec![g])).1);
    assert_eq!(b.iter().filter(|x| x.0 == WHITE).count(), 2);
}

#[test]
fn invert_wins_over_a_point_fill_unless_the_point_turns_it_off() {
    let mut s = ser("{4,-3,2,-1}");
    s.invert_if_negative = true;
    s.points = vec![
        DataPoint { index: 1, fill: Some(col(RED)), ..Default::default() },
        DataPoint { index: 3, fill: Some(col(RED)), invert_if_negative: Some(false), ..Default::default() },
    ];
    let b = bars(&draw(&chart(ChartKind::ColumnClustered, vec![s])).1);
    assert_eq!(b[1].0, WHITE);
    assert_eq!(b[3].0, RED);
}

#[test]
fn vary_colors_colours_the_only_bar_series_point_by_point() {
    let mut c = chart(ChartKind::ColumnClustered, vec![ser("{4,3,2,1}")]);
    c.vary_colors = Some(true);
    let (d, p) = draw(&c);
    assert!(d.vary_colors && d.legend_by_point);
    assert_eq!(bars(&p).iter().map(|x| x.0).collect::<Vec<_>>(), palette(4));
    let legend = legend_entries(&c, &d);
    assert_eq!(legend.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["w", "x", "y", "z"]);
    assert_eq!(legend.iter().map(|e| e.1).collect::<Vec<_>>(), palette(4));
    // A series fill of its own wins over vary colors (the legend still lists the points).
    c.series[0].color = Some(col(RED));
    let (d, p) = draw(&c);
    assert!(bars(&p).iter().all(|x| x.0 == RED));
    assert_eq!(legend_entries(&c, &d).len(), 4);
}

#[test]
fn vary_colors_is_ignored_with_several_series_and_for_areas() {
    let mut c = chart(ChartKind::ColumnClustered, vec![ser("{4,3,2,1}"), ser("{1,2,3,4}")]);
    c.vary_colors = Some(true);
    let (d, p) = draw(&c);
    assert!(!d.vary_colors && !d.legend_by_point);
    let b = bars(&p);
    let pal = palette(2);
    assert_eq!(b.iter().filter(|x| x.0 == pal[0]).count(), 4);
    assert_eq!(b.iter().filter(|x| x.0 == pal[1]).count(), 4);
    assert_eq!(legend_entries(&c, &d).len(), 2);
    let mut a = chart(ChartKind::Area, vec![ser("{4,3,2,1}")]);
    a.vary_colors = Some(true);
    let (d, _) = draw(&a);
    assert!(!d.vary_colors && !d.legend_by_point);
}

#[test]
fn a_single_series_with_point_formats_lists_its_points_in_the_legend() {
    let mut s = ser("{4,3,2,1}");
    s.points = vec![DataPoint { index: 1, fill: Some(col(RED)), ..Default::default() }];
    let c = chart(ChartKind::ColumnClustered, vec![s]);
    let (d, _) = draw(&c);
    let legend = legend_entries(&c, &d);
    assert_eq!(legend.len(), 4);
    assert_eq!(legend[1].1, RED);
    assert_eq!(legend[0].1, palette(1)[0]);
}

#[test]
fn pie_slices_follow_vary_colors_series_fill_and_point_fill() {
    let pie = |s: Series, vary: Option<bool>| {
        let mut c = chart(ChartKind::Pie, vec![s]);
        c.vary_colors = vary;
        wedges(&draw(&c).1).into_iter().map(|w| w.0).collect::<Vec<_>>()
    };
    assert_eq!(pie(ser("{4,3,2,1}"), None), palette(4), "on by default");
    assert_eq!(pie(ser("{4,3,2,1}"), Some(false)), vec![palette(1)[0]; 4]);
    let mut red = ser("{4,3,2,1}");
    red.color = Some(col(RED));
    assert_eq!(pie(red, None), vec![RED; 4], "a series fill colours every slice");
    let mut one = ser("{4,3,2,1}");
    one.points = vec![DataPoint { index: 1, fill: Some(col(RED)), ..Default::default() }];
    let p = palette(4);
    assert_eq!(pie(one, None), vec![p[0], RED, p[2], p[3]]);
}

#[test]
fn exploded_slices_move_out_and_the_pie_shrinks() {
    let (_, flat) = draw(&chart(ChartKind::Pie, vec![ser("{4,3,2,1}")]));
    let mut s = ser("{4,3,2,1}");
    s.points = vec![DataPoint { index: 2, explosion: Some(25), ..Default::default() }];
    let (_, p) = draw(&chart(ChartKind::Pie, vec![s]));
    let w = wedges(&p);
    let centre = w[0].1;
    assert_eq!(w[1].1, centre);
    assert_ne!(w[2].1, centre, "the exploded slice is offset");
    let r = |prims: &[Prim]| prims.iter().find_map(|p| if let Prim::Wedge { r_outer, .. } = p { Some(*r_outer) } else { None }).unwrap();
    assert!(r(&p) < r(&flat));
}

#[test]
fn line_segments_and_markers_take_point_formats() {
    let mut s = ser("{1,3,2,4}");
    s.points = vec![
        DataPoint {
            index: 1,
            marker: Some(MarkerFormat { symbol: MarkerSymbol::Square, size: Some(9), fill: Some(col(RED)), line: None }),
            ..Default::default()
        },
        DataPoint { index: 3, line: Some(col(GREEN)), ..Default::default() },
    ];
    let (_, p) = draw(&chart(ChartKind::LineMarkers, vec![s]));
    // (Horizontal green lines are the legend swatch of point 3.)
    let green: Vec<&Prim> =
        p.iter().filter(|x| matches!(x, Prim::Line { color, pts, .. } if *color == GREEN && pts.len() == 2 && pts[0][1] != pts[1][1])).collect();
    assert_eq!(green.len(), 1, "one segment, the one ending at point 3");
    assert!(p.iter().any(|x| matches!(x, Prim::Polygon { fill, pts, .. } if *fill == RED && pts.len() == 4)), "a red square marker");
    // The segment's point outlines its marker in the same colour.
    assert!(p.iter().any(|x| matches!(x, Prim::Circle { stroke: Some((c, _)), .. } if *c == GREEN)));
}

#[test]
fn vary_colors_on_a_line_colours_segments_and_markers() {
    let mut c = chart(ChartKind::LineMarkers, vec![ser("{1,3,2,4}")]);
    c.vary_colors = Some(true);
    let (_, p) = draw(&c);
    let pal = palette(4);
    for (k, want) in pal.iter().enumerate().skip(1) {
        assert!(p.iter().any(|x| matches!(x, Prim::Line { color, pts, .. } if *color == *want && pts.len() == 2)), "segment {k}");
        assert!(p.iter().any(|x| matches!(x, Prim::Circle { fill: Some(f), .. } if *f == *want)), "marker {k}");
    }
}

#[test]
fn scatter_point_marker_fill() {
    let mut s = Series { categories: Some("{1,2,3,4}".into()), values: "{2,4,3,5}".into(), ..Series::default() };
    s.points = vec![DataPoint { index: 2, marker: Some(MarkerFormat { fill: Some(col(RED)), ..Default::default() }), ..Default::default() }];
    let (_, p) = draw(&chart(ChartKind::Scatter, vec![s]));
    let fills: Vec<Rgba> =
        p.iter().filter_map(|x| if let Prim::Circle { fill: Some(f), r, .. } = x { (*r > 3.0).then_some(*f) } else { None }).collect();
    assert_eq!(fills.iter().filter(|f| **f == RED).count(), 1);
    assert_eq!(fills.iter().filter(|f| **f == palette(1)[0]).count(), 3);
}

#[test]
fn hostile_point_styles_never_panic() {
    let pts = vec![
        PointStyle { index: usize::MAX, fill: Some(RED), ..Default::default() },
        PointStyle { index: 1, marker_size: Some(f32::NAN), explosion: f32::INFINITY, ..Default::default() },
        PointStyle { index: 1, marker_size: Some(1e30), explosion: -5.0, marker_symbol: MarkerSymbol::Star, ..Default::default() },
        PointStyle { index: 0, marker_symbol: MarkerSymbol::None, invert_if_negative: Some(true), ..Default::default() },
    ];
    for kind in [
        ChartKind::ColumnClustered,
        ChartKind::BarStacked,
        ChartKind::Line,
        ChartKind::LineMarkers,
        ChartKind::Pie,
        ChartKind::Doughnut,
        ChartKind::Scatter,
        ChartKind::ScatterLines,
        ChartKind::Bubble,
        ChartKind::Area,
        ChartKind::Radar,
    ] {
        let s = SeriesData {
            name: "S".into(),
            values: vec![Some(-1.0), Some(2.0), None, Some(f64::NAN)],
            x: Some(vec![Some(1.0), Some(2.0), Some(3.0), Some(4.0)]),
            sizes: Some(vec![Some(1.0); 4]),
            kind,
            points: pts.clone(),
            invert_if_negative: true,
            ..Default::default()
        };
        let d = ChartData { series: vec![s], vary_colors: true, legend_by_point: true, ..Default::default() };
        let c = chart(kind, vec![]);
        let prims = render(&c, &d, 300.0, 200.0, &ApproxMeasure);
        assert!(!prims.is_empty(), "{kind:?}");
        let _ = legend_entries(&c, &d);
    }
}
