//! Chart layout: title, legend, axes and every chart kind, drawn into [`Prim`]s.

use std::f32::consts::{PI, TAU};

use gridcraft_model::{Chart, ChartKind, LineDash, QuartileMethod, Theme, TrendlineKind};

use crate::axis::{Fmt, Scale, clean, fixed_scale, nice_scale};
use crate::resolve::{MAX_POINTS, MAX_SERIES, MAX_TRENDLINES, series_color};
use crate::trend::{auto_name, fit_trendline, moving_average};
use crate::{ChartData, HAlign, Measure, Prim, Rgba, SeriesData, VAlign};

mod cartesian;
mod frame;
mod kinds;
mod legend;
mod lines;
mod trend;

use cartesian::{cartesian, scatter};
use frame::{Bx, axis_titles, chart_title};
use kinds::{box_whisker, funnel, histogram, pie, radar, stock, treemap, waterfall};
use legend::draw_legend;
pub use legend::{Swatch, legend_entries};
pub use trend::supports_trendline;

pub(crate) const TEXT: Rgba = [0x59, 0x59, 0x59, 0xFF];
const DARK_TEXT: Rgba = [0x26, 0x26, 0x26, 0xFF];
const GRID: Rgba = [0xD9, 0xD9, 0xD9, 0xFF];
const AXIS: Rgba = [0xBF, 0xBF, 0xBF, 0xFF];
const WHITE: Rgba = [0xFF, 0xFF, 0xFF, 0xFF];
const STOCK: Rgba = [0x40, 0x40, 0x40, 0xFF];
pub(crate) const UP: Rgba = [0x3E, 0x9B, 0x5F, 0xFF];
pub(crate) const DOWN: Rgba = [0xD6, 0x4F, 0x45, 0xFF];

const TITLE_SIZE: f32 = 14.0;
const LABEL_SIZE: f32 = 9.0;
const AXIS_TITLE_SIZE: f32 = 10.0;
/// Most categories drawn by category-axis charts.
const MAX_CATS: usize = 20_000;
/// Lines with more points than this are decimated (min/max per pixel column).
const MAX_LINE_PTS: usize = 2000;
/// Most markers per series (after de-duplicating by pixel).
const MAX_MARKERS: usize = 5000;
/// Data labels are skipped when a series has more points than this.
const MAX_LABELS: usize = 200;

struct Ctx<'a> {
    out: Vec<Prim>,
    m: &'a dyn Measure,
}

impl Ctx<'_> {
    fn tw(&self, s: &str, size: f32) -> f32 {
        let w = self.m.text_width(s, size, false);
        if w.is_finite() { w.max(0.0) } else { 0.0 }
    }
    fn fit(&self, s: &str, size: f32, maxw: f32) -> String {
        if self.tw(s, size) <= maxw {
            return s.to_string();
        }
        let chars: Vec<char> = s.chars().collect();
        let mut n = chars.len().min(400);
        while n > 0 {
            n -= 1;
            let t: String = chars.iter().take(n).collect::<String>() + "…";
            if self.tw(&t, size) <= maxw {
                return if n == 0 { String::new() } else { t };
            }
        }
        String::new()
    }
    fn text(&mut self, x: f32, y: f32, s: &str, size: f32, color: Rgba, (align, valign): (HAlign, VAlign)) {
        if s.is_empty() {
            return;
        }
        self.out.push(Prim::Text { x, y, text: s.to_string(), size, color, bold: false, align, valign, rotation: 0.0 });
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Rgba, stroke: Option<(Rgba, f32)>) {
        self.out.push(Prim::Rect { x, y, w, h, fill: Some(fill), stroke, radius: 0.0 });
    }
    fn line(&mut self, pts: Vec<[f32; 2]>, color: Rgba, width: f32) {
        if pts.len() >= 2 {
            self.out.push(Prim::Line { pts, color, width, dash: false });
        }
    }
    fn hline(&mut self, x0: f32, x1: f32, y: f32, color: Rgba, width: f32) {
        self.line(vec![[x0, y], [x1, y]], color, width);
    }
    fn vline(&mut self, x: f32, y0: f32, y1: f32, color: Rgba, width: f32) {
        self.line(vec![[x, y0], [x, y1]], color, width);
    }
}

fn sanitize_vals(v: &[Option<f64>]) -> Vec<Option<f64>> {
    v.iter().take(MAX_POINTS).map(|x| x.filter(|n| n.is_finite()).map(clean)).collect()
}

fn sanitize(d: &ChartData) -> ChartData {
    ChartData {
        palette: d.palette.iter().take(MAX_POINTS).copied().collect(),
        categories: d.categories.iter().take(MAX_POINTS).cloned().collect(),
        category_levels: d.category_levels.iter().take(3).map(|l| l.iter().take(MAX_POINTS).cloned().collect()).collect(),
        series: d
            .series
            .iter()
            .take(MAX_SERIES)
            .map(|s| SeriesData {
                name: s.name.clone(),
                values: sanitize_vals(&s.values),
                x: s.x.as_deref().map(sanitize_vals),
                sizes: s.sizes.as_deref().map(sanitize_vals),
                color: s.color,
                kind: s.kind,
                secondary: s.secondary,
                smooth: s.smooth,
                trendlines: s.trendlines.iter().take(MAX_TRENDLINES).cloned().collect(),
                number_format: s.number_format.clone(),
            })
            .collect(),
    }
}

/// Lays out and draws a chart into a `w`×`h` box at the origin. Empty for a degenerate box.
pub fn render(chart: &Chart, data: &ChartData, w: f32, h: f32, m: &dyn Measure) -> Vec<Prim> {
    if !(w.is_finite() && h.is_finite()) || w < 1.0 || h < 1.0 || w > 1e7 || h > 1e7 {
        return vec![];
    }
    let data = sanitize(data);
    let mut ctx = Ctx { out: Vec::new(), m };
    ctx.out.push(Prim::Rect { x: 0.0, y: 0.0, w, h, fill: Some(WHITE), stroke: Some((GRID, 1.0)), radius: 0.0 });
    let pad = (w.min(h) * 0.04).clamp(2.0, 10.0);
    let mut area = Bx { x: pad, y: pad, w: w - 2.0 * pad, h: h - 2.0 * pad };
    if let Some(t) = chart_title(chart, &data) {
        let size = TITLE_SIZE.min(h * 0.12).max(4.0);
        let t = ctx.fit(&t, size, area.w);
        ctx.text(w / 2.0, area.y, &t, size, TEXT, (HAlign::Center, VAlign::Top));
        let used = size * 1.3 + 4.0;
        area.y += used;
        area.h -= used;
    }
    let entries = legend_entries(chart, &data);
    if !entries.is_empty() && area.ok() {
        area = draw_legend(&mut ctx, &entries, chart.legend, area);
    }
    if area.ok() && !data.series.is_empty() {
        draw_kind(&mut ctx, chart, &data, area);
    }
    finalize(ctx.out, w, h)
}

fn draw_kind(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    use ChartKind::*;
    match chart.kind {
        Pie => pie(ctx, chart, data, area, 0.0, false),
        Doughnut => pie(ctx, chart, data, area, 0.5, false),
        Sunburst => pie(ctx, chart, data, area, 0.2, true),
        Treemap => treemap(ctx, chart, data, area),
        Funnel => funnel(ctx, chart, data, area),
        Radar => radar(ctx, chart, data, area),
        _ => {
            let area = axis_titles(ctx, chart, area);
            if !area.ok() {
                return;
            }
            match chart.kind {
                BarClustered | BarStacked | BarStacked100 => cartesian(ctx, chart, data, area, true),
                Scatter | ScatterLines | Bubble => scatter(ctx, chart, data, area),
                Histogram => histogram(ctx, chart, data, area),
                Waterfall => waterfall(ctx, chart, data, area),
                BoxWhisker => box_whisker(ctx, chart, data, area),
                Stock => stock(ctx, chart, data, area),
                _ => cartesian(ctx, chart, data, area, false),
            }
        }
    }
}

// ---------------------------------------------------------------- final safety pass

fn fin(v: f32) -> bool {
    v.is_finite()
}

fn clamp_stroke(s: Option<(Rgba, f32)>) -> Option<(Rgba, f32)> {
    s.filter(|(_, w)| fin(*w) && *w > 0.0).map(|(c, w)| (c, w.min(50.0)))
}

/// Drops non-finite primitives and clamps every coordinate into the `w`×`h` box.
pub(crate) fn finalize(prims: Vec<Prim>, w: f32, h: f32) -> Vec<Prim> {
    let cx = |v: f32| v.clamp(0.0, w);
    let cy = |v: f32| v.clamp(0.0, h);
    let pts_ok = |pts: &[[f32; 2]]| pts.iter().all(|p| fin(p[0]) && fin(p[1]));
    prims
        .into_iter()
        .filter_map(|p| match p {
            Prim::Rect { x, y, w: rw, h: rh, fill, stroke, radius } => {
                if !(fin(x) && fin(y) && fin(rw) && fin(rh)) {
                    return None;
                }
                let (x0, x1) = (cx(x.min(x + rw)), cx(x.max(x + rw)));
                let (y0, y1) = (cy(y.min(y + rh)), cy(y.max(y + rh)));
                if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 {
                    return None;
                }
                let radius = if fin(radius) { radius.clamp(0.0, (x1 - x0).min(y1 - y0) / 2.0) } else { 0.0 };
                Some(Prim::Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0, fill, stroke: clamp_stroke(stroke), radius })
            }
            Prim::Line { pts, color, width, dash } => {
                if !pts_ok(&pts) || pts.len() < 2 || !fin(width) {
                    return None;
                }
                let pts = pts.into_iter().map(|p| [cx(p[0]), cy(p[1])]).collect();
                Some(Prim::Line { pts, color, width: width.clamp(0.1, 50.0), dash })
            }
            Prim::Polygon { pts, fill, stroke } => {
                if !pts_ok(&pts) || pts.len() < 3 {
                    return None;
                }
                let pts = pts.into_iter().map(|p| [cx(p[0]), cy(p[1])]).collect();
                Some(Prim::Polygon { pts, fill, stroke: clamp_stroke(stroke) })
            }
            Prim::Wedge { cx: x, cy: y, r_outer, r_inner, a0, a1, fill, stroke } => {
                if ![x, y, r_outer, r_inner, a0, a1].iter().all(|v| fin(*v)) {
                    return None;
                }
                let (x, y) = (cx(x), cy(y));
                let maxr = x.min(w - x).min(y).min(h - y);
                let ro = r_outer.min(maxr);
                if ro <= 0.0 {
                    return None;
                }
                Some(Prim::Wedge { cx: x, cy: y, r_outer: ro, r_inner: r_inner.clamp(0.0, ro), a0, a1, fill, stroke: clamp_stroke(stroke) })
            }
            Prim::Circle { cx: x, cy: y, r, fill, stroke } => {
                if !(fin(x) && fin(y) && fin(r)) {
                    return None;
                }
                let (x, y) = (cx(x), cy(y));
                let r = r.min(x.min(w - x).min(y).min(h - y));
                if r <= 0.0 {
                    return None;
                }
                Some(Prim::Circle { cx: x, cy: y, r, fill, stroke: clamp_stroke(stroke) })
            }
            Prim::Text { x, y, text, size, color, bold, align, valign, rotation } => {
                if !(fin(x) && fin(y) && fin(size) && fin(rotation)) || text.is_empty() {
                    return None;
                }
                Some(Prim::Text { x: cx(x), y: cy(y), text, size: size.clamp(1.0, 200.0), color, bold, align, valign, rotation })
            }
        })
        .collect()
}
