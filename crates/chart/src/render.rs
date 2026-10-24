//! Chart layout: title, legend, axes and every chart kind, drawn into [`Prim`]s.

use std::collections::HashSet;
use std::f32::consts::{PI, TAU};

use gridcraft_model::{Chart, ChartKind, LegendPos, LineDash, QuartileMethod, Theme, TrendlineKind};

use crate::axis::{Fmt, Scale, clean, configured_scale, fixed_scale, nice_scale};
use crate::resolve::{MAX_POINTS, MAX_SERIES, MAX_TRENDLINES, series_color};
use crate::trend::{auto_name, fit_trendline, moving_average};
use crate::{ChartData, HAlign, Measure, Prim, Rgba, SeriesData, VAlign};

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

#[derive(Clone, Copy, Debug)]
struct Bx {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Bx {
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn bottom(&self) -> f32 {
        self.y + self.h
    }
    fn ok(&self) -> bool {
        self.w.is_finite() && self.h.is_finite() && self.w > 4.0 && self.h > 4.0
    }
    fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

struct Ctx<'a> {
    out: Vec<Prim>,
    m: &'a dyn Measure,
    format: &'a gridcraft_model::ChartFormat,
    size: [f32; 2],
    plot: Option<Bx>,
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
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: f32, y: f32, s: &str, size: f32, color: Rgba, align: HAlign, valign: VAlign) {
        if s.is_empty() {
            return;
        }
        self.text_role("data_labels", x, y, s, size, color, align, valign);
    }
    fn font_size(&self, role: &str, default: f32) -> f32 {
        self.format.text_style(role).size.unwrap_or(default)
    }
    #[allow(clippy::too_many_arguments)]
    fn text_role(&mut self, role: &str, x: f32, y: f32, s: &str, size: f32, color: Rgba, align: HAlign, valign: VAlign) {
        if s.is_empty() {
            return;
        }
        let format = self.format.text_style(role);
        self.out.push(Prim::Text {
            x,
            y,
            text: s.to_string(),
            size: format.size.unwrap_or(size),
            color: format.color.unwrap_or(color),
            bold: format.bold.unwrap_or(false),
            align,
            valign,
            rotation: 0.0,
            role: role.into(),
            format,
        });
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Rgba, stroke: Option<(Rgba, f32)>) {
        self.out.push(Prim::Rect { x, y, w, h, fill: Some(fill), stroke, radius: 0.0 });
    }
    fn line(&mut self, pts: Vec<[f32; 2]>, color: Rgba, width: f32) {
        if pts.len() >= 2 && width > 0.0 {
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

/// Text colour readable on `fill`.
fn on(fill: Rgba) -> Rgba {
    let l = 0.299 * fill[0] as f32 + 0.587 * fill[1] as f32 + 0.114 * fill[2] as f32;
    if l < 150.0 { WHITE } else { DARK_TEXT }
}

/// How a legend entry's swatch is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swatch {
    Box,
    Line,
    LineMarker,
    Marker,
}

/// The kind a series is drawn as (per-series kinds only apply to combo charts).
fn plot_kind(chart: &Chart, s: &SeriesData) -> ChartKind {
    if chart.kind == ChartKind::Combo { if s.kind == ChartKind::Combo { ChartKind::Line } else { s.kind } } else { chart.kind }
}

/// Colour of point `j` for charts that colour by point (pie, doughnut, treemap, sunburst).
fn point_color(data: &ChartData, j: usize) -> Rgba {
    match (j, data.series.first()) {
        (0, Some(s)) => s.color,
        _ => j.checked_rem(data.palette.len()).and_then(|k| data.palette.get(k)).copied().unwrap_or_else(|| series_color(&Theme::default(), j)),
    }
}

fn first_len(data: &ChartData) -> usize {
    data.series.first().map(|s| s.values.len()).unwrap_or(0)
}

fn cat_label(data: &ChartData, j: usize) -> String {
    data.categories.get(j).cloned().unwrap_or_else(|| (j + 1).to_string())
}

/// Legend entries (text, colour, swatch) `render` draws for this chart; empty when hidden.
pub fn legend_entries(chart: &Chart, data: &ChartData) -> Vec<(String, Rgba, Swatch)> {
    if chart.legend == LegendPos::None {
        return vec![];
    }
    match chart.kind {
        ChartKind::Pie | ChartKind::Doughnut | ChartKind::Treemap | ChartKind::Sunburst => {
            let n = first_len(data).min(MAX_CATS);
            (0..n).map(|j| (cat_label(data, j), point_color(data, j), Swatch::Box)).collect()
        }
        ChartKind::Histogram | ChartKind::Funnel | ChartKind::Stock => vec![],
        ChartKind::Waterfall => {
            if data.series.is_empty() {
                return vec![];
            }
            let total = data.series.first().map(|s| s.color).unwrap_or(TEXT);
            vec![("Increase".into(), UP, Swatch::Box), ("Decrease".into(), DOWN, Swatch::Box), ("Total".into(), total, Swatch::Box)]
        }
        _ => {
            let series = data.series.iter().take(MAX_SERIES);
            let mut out: Vec<(String, Rgba, Swatch)> = series
                .clone()
                .enumerate()
                .map(|(i, s)| {
                    let sw = match plot_kind(chart, s) {
                        ChartKind::Line | ChartKind::LineStacked | ChartKind::Radar | ChartKind::ScatterLines => Swatch::Line,
                        ChartKind::LineMarkers => Swatch::LineMarker,
                        ChartKind::Scatter | ChartKind::Bubble => Swatch::Marker,
                        _ => Swatch::Box,
                    };
                    let spec = chart.format.series.get(&i);
                    let sw = if matches!(sw, Swatch::Line | Swatch::LineMarker | Swatch::Marker)
                        && spec.is_some_and(|s| s.marker_size.is_some() || s.marker.as_deref().is_some_and(|m| m != "none"))
                    {
                        if sw == Swatch::Line { Swatch::LineMarker } else { sw }
                    } else {
                        sw
                    };
                    (s.name.clone(), s.color, sw)
                })
                .collect();
            // Trendlines follow the series, named "Linear (Sales)" unless given a name.
            for s in series.filter(|s| supports_trendline(plot_kind(chart, s))) {
                for t in s.trendlines.iter().take(MAX_TRENDLINES) {
                    out.push((t.spec.name.clone().unwrap_or_else(|| auto_name(t.spec.kind, &s.name)), t.color, Swatch::Line));
                }
            }
            out
        }
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

fn chart_title(chart: &Chart, data: &ChartData) -> Option<String> {
    match &chart.title {
        Some(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
        None if data.series.len() == 1 => data.series.first().map(|s| s.name.clone()).filter(|t| !t.is_empty()),
        None => None,
    }
}

/// Lays out and draws a chart into a `w`×`h` box at the origin. Empty for a degenerate box.
pub fn render(chart: &Chart, data: &ChartData, w: f32, h: f32, m: &dyn Measure) -> Vec<Prim> {
    if !(w.is_finite() && h.is_finite()) || w < 1.0 || h < 1.0 || w > 1e7 || h > 1e7 {
        return vec![];
    }
    let data = sanitize(data);
    let mut ctx = Ctx { out: Vec::new(), m, format: &chart.format, size: [w, h], plot: None };
    let area_style = &chart.format.chart_area;
    let border = area_style.border_width.or(chart.format.border_width).unwrap_or(1.0);
    ctx.out.push(Prim::Rect {
        x: 0.0,
        y: 0.0,
        w,
        h,
        fill: Some(area_style.fill.unwrap_or(WHITE)),
        stroke: (border > 0.0).then_some((area_style.border.unwrap_or(GRID), border)),
        radius: 0.0,
    });
    let pad = (w.min(h) * 0.04).clamp(2.0, 10.0);
    let mut area = Bx { x: pad, y: pad, w: w - 2.0 * pad, h: h - 2.0 * pad };
    if let Some(t) = chart_title(chart, &data) {
        let size = ctx.font_size("title", TITLE_SIZE.min(h * 0.12)).max(4.0);
        let t = ctx.fit(&t, size, area.w);
        ctx.text_role("title", w / 2.0, area.y, &t, size, TEXT, HAlign::Center, VAlign::Top);
        let used = size * 1.3 + 4.0;
        area.y += used;
        area.h -= used;
    }
    let entries = legend_entries(chart, &data);
    if !entries.is_empty() && area.ok() && !chart.format.legend_overlay {
        area = draw_legend(&mut ctx, &entries, chart.legend, area, chart, data.series.len());
    }
    if area.ok() && !data.series.is_empty() {
        draw_kind(&mut ctx, chart, &data, area);
    }
    if chart.format.legend_overlay && !entries.is_empty() && area.ok() {
        let plot = ctx.plot.unwrap_or(area);
        let start = ctx.out.len();
        draw_legend(&mut ctx, &entries, LegendPos::Right, plot, chart, data.series.len());
        let position = chart.format.legend_position.unwrap_or([plot.right() / w, (plot.y + 4.0) / h]);
        place_group(&mut ctx, start, position, plot);
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

fn axis_titles(ctx: &mut Ctx, chart: &Chart, mut area: Bx) -> Bx {
    let lh = ctx.font_size("x_axis_title", AXIS_TITLE_SIZE).max(ctx.font_size("y_axis_title", AXIS_TITLE_SIZE)) * 1.4 + 2.0;
    if let Some(t) = chart.x_title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let t = ctx.fit(t, ctx.font_size("x_axis_title", AXIS_TITLE_SIZE), area.w);
        ctx.text_role("x_axis_title", area.cx(), area.bottom(), &t, AXIS_TITLE_SIZE, TEXT, HAlign::Center, VAlign::Bottom);
        area.h -= lh;
    }
    if let Some(t) = chart.y_title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let t = ctx.fit(t, ctx.font_size("y_axis_title", AXIS_TITLE_SIZE), area.h);
        if !t.is_empty() {
            let format = ctx.format.text_style("y_axis_title");
            ctx.out.push(Prim::Text {
                role: "y_axis_title".into(),
                size: format.size.unwrap_or(AXIS_TITLE_SIZE),
                color: format.color.unwrap_or(TEXT),
                bold: format.bold.unwrap_or(false),
                format,
                x: area.x,
                y: area.cy(),
                text: t,
                align: HAlign::Center,
                valign: VAlign::Top,
                rotation: -PI / 2.0,
            });
        }
        area.x += lh;
        area.w -= lh;
    }
    area
}

// ---------------------------------------------------------------- legend

fn swatch_w(sw: Swatch, s: f32) -> f32 {
    if sw == Swatch::Box { s } else { s * 2.0 }
}

fn draw_swatch(ctx: &mut Ctx, sw: Swatch, x: f32, yc: f32, s: f32, color: Rgba) {
    match sw {
        Swatch::Box => ctx.rect(x, yc - s / 2.0, s, s, color, None),
        Swatch::Line => ctx.hline(x, x + 2.0 * s, yc, color, 2.0),
        Swatch::LineMarker => {
            ctx.hline(x, x + 2.0 * s, yc, color, 2.0);
            ctx.out.push(Prim::Circle { cx: x + s, cy: yc, r: s * 0.35, fill: Some(color), stroke: None });
        }
        Swatch::Marker => ctx.out.push(Prim::Circle { cx: x + s, cy: yc, r: s * 0.4, fill: Some(color), stroke: None }),
    }
}

fn draw_legend(ctx: &mut Ctx, entries: &[(String, Rgba, Swatch)], pos: LegendPos, area: Bx, chart: &Chart, series_count: usize) -> Bx {
    let size = ctx.font_size("legend", LABEL_SIZE);
    // Category entries do not correspond to series indices.
    let series_count = match chart.kind {
        ChartKind::Pie | ChartKind::Doughnut | ChartKind::Treemap | ChartKind::Sunburst => 0,
        _ => series_count,
    };
    let marker_size = if series_count > 0 { chart.format.series.values().filter_map(|s| s.marker_size).fold(0.0f32, f32::max) } else { 0.0 };
    let row_h = (size * 1.6).max(marker_size + 4.0);
    let s = (size * 0.8).max(marker_size / 2.0);
    let gap = 4.0;
    let spacing = size * 1.3;
    let item_w = |ctx: &Ctx, e: &(String, Rgba, Swatch)| swatch_w(e.2, s) + gap + ctx.tw(&e.0, size);
    match pos {
        LegendPos::None => area,
        LegendPos::Top | LegendPos::Bottom => {
            let max_rows = ((area.h * 0.3 / row_h).floor() as usize).max(1);
            let mut rows: Vec<Vec<(usize, f32)>> = vec![vec![]];
            let mut cur = 0.0f32;
            for (i, e) in entries.iter().enumerate() {
                let iw = item_w(ctx, e).min(area.w);
                let need = if cur > 0.0 { spacing + iw } else { iw };
                if cur > 0.0 && cur + need > area.w {
                    if rows.len() >= max_rows {
                        break;
                    }
                    rows.push(vec![]);
                    cur = 0.0;
                }
                cur += if cur > 0.0 { spacing + iw } else { iw };
                if let Some(r) = rows.last_mut() {
                    r.push((i, iw));
                }
            }
            let lh = rows.len() as f32 * row_h;
            let y0 = if pos == LegendPos::Bottom { area.bottom() - lh } else { area.y };
            for (ri, row) in rows.iter().enumerate() {
                let total: f32 = row.iter().map(|(_, w)| w).sum::<f32>() + spacing * row.len().saturating_sub(1) as f32;
                let mut x = area.x + (area.w - total).max(0.0) / 2.0;
                let yc = y0 + ri as f32 * row_h + row_h / 2.0;
                for &(i, iw) in row {
                    if let Some(e) = entries.get(i) {
                        let start = ctx.out.len();
                        draw_swatch(ctx, e.2, x, yc, s, e.1);
                        if i < series_count {
                            style_series(ctx, start, chart.format.series.get(&i), e.1);
                        }
                        let sx = swatch_w(e.2, s) + gap;
                        let t = ctx.fit(&e.0, size, (iw - sx).max(0.0) + 0.5);
                        ctx.text_role("legend", x + sx, yc, &t, size, TEXT, HAlign::Left, VAlign::Middle);
                    }
                    x += iw + spacing;
                }
            }
            let used = lh + 4.0;
            if pos == LegendPos::Bottom { Bx { h: area.h - used, ..area } } else { Bx { y: area.y + used, h: area.h - used, ..area } }
        }
        LegendPos::Left | LegendPos::Right => {
            let colw = entries.iter().map(|e| item_w(ctx, e)).fold(0.0f32, f32::max).min(area.w * 0.35);
            let max_rows = ((area.h / row_h).floor() as usize).max(1);
            let n = entries.len().min(max_rows);
            let lh = n as f32 * row_h;
            let y0 = area.y + (area.h - lh).max(0.0) / 2.0;
            let x0 = if pos == LegendPos::Right { area.right() - colw } else { area.x };
            for (i, e) in entries.iter().take(n).enumerate() {
                let yc = y0 + i as f32 * row_h + row_h / 2.0;
                let start = ctx.out.len();
                draw_swatch(ctx, e.2, x0, yc, s, e.1);
                if i < series_count {
                    style_series(ctx, start, chart.format.series.get(&i), e.1);
                }
                let sx = swatch_w(e.2, s) + gap;
                let t = ctx.fit(&e.0, size, (colw - sx).max(0.0) + 0.5);
                ctx.text_role("legend", x0 + sx, yc, &t, size, TEXT, HAlign::Left, VAlign::Middle);
            }
            let used = colw + 8.0;
            if pos == LegendPos::Right { Bx { w: area.w - used, ..area } } else { Bx { x: area.x + used, w: area.w - used, ..area } }
        }
    }
}

// ---------------------------------------------------------------- axes frame

enum CatAxis<'a> {
    /// Category labels and, for a multi-level axis, the outer levels (nearest the labels first;
    /// one entry per category, non-empty where a group starts).
    Labels(&'a [String], &'a [Vec<String>]),
    Numeric(Scale, &'a Fmt),
}

/// Groups of an outer category level: `(first, end)` index pairs, a group starting at every
/// non-empty entry (and at the first category).
fn level_groups(level: &[String], n: usize) -> Vec<(usize, usize)> {
    let mut starts: Vec<usize> = (0..n).filter(|&j| j == 0 || level.get(j).is_some_and(|t| !t.is_empty())).collect();
    starts.push(n);
    starts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect()
}

struct ValAxis<'a> {
    scale: Scale,
    fmt: &'a Fmt,
}

impl ValAxis<'_> {
    fn labels(&self) -> Vec<(f64, String)> {
        self.scale.ticks().into_iter().map(|v| (v, self.fmt.format(v, Some(self.scale.step)))).collect()
    }
}

/// Draws gridlines, axis labels and the category axis line; returns the plot rectangle.
fn plot_surface(ctx: &mut Ctx, plot: Bx, chart: &Chart) {
    ctx.plot = Some(plot);
    let style = &chart.format.plot_area;
    let width = style.border_width.unwrap_or(if style.border.is_some() { 1.0 } else { 0.0 });
    ctx.out.push(Prim::Rect {
        x: plot.x,
        y: plot.y,
        w: plot.w,
        h: plot.h,
        fill: style.fill,
        stroke: (width > 0.0).then_some((style.border.unwrap_or(GRID), width)),
        radius: 0.0,
    });
}

fn tick(ctx: &mut Ctx, x: f32, y: f32, horizontal_axis: bool, kind: Option<&str>, style: &gridcraft_model::ChartStroke) {
    let color = style.color.unwrap_or(AXIS);
    let width = style.width.unwrap_or(0.75);
    if width <= 0.0 {
        return;
    }
    let (inside, outside) = match kind {
        Some("in") => (4.0, 0.0),
        Some("out") => (0.0, 4.0),
        Some("cross") => (3.0, 3.0),
        _ => return,
    };
    if horizontal_axis {
        ctx.vline(x, y - inside, y + outside, color, width);
    } else {
        ctx.hline(x - outside, x + inside, y, color, width);
    }
}

fn minor_ticks(ctx: &mut Ctx, scale: Scale, axis: &gridcraft_model::ChartAxis, plot: Bx, horizontal: bool) {
    let Some(kind) = axis.minor_tick.as_deref().filter(|v| *v != "none") else {
        return;
    };
    let mut values = Vec::new();
    if scale.logarithmic {
        let start = scale.min.max(f64::MIN_POSITIVE).log10().floor() as i32;
        let end = (scale.max.log10().ceil() as i32).min(start.saturating_add(40));
        for exponent in start..=end {
            for multiplier in 2..10 {
                let v = 10f64.powi(exponent) * multiplier as f64;
                if v > scale.min && v < scale.max {
                    values.push(v);
                }
            }
        }
    } else {
        let step = axis.minor_unit.unwrap_or(scale.step / 5.0);
        if !step.is_finite() || step <= 0.0 {
            return;
        }
        let stride = ((scale.max - scale.min) / step / 250.0).ceil().max(1.0);
        for i in 0..250 {
            let v = scale.min + (1.0 + i as f64 * stride) * step;
            if v >= scale.max {
                break;
            }
            let k = (v - scale.min) / scale.step;
            if v > scale.min && (k - k.round()).abs() > 1e-7 {
                values.push(v);
            }
        }
    }
    for value in values.into_iter().take(250) {
        let f = scale.frac(value);
        let start = ctx.out.len();
        tick(
            ctx,
            if horizontal { plot.x + f * plot.w } else { plot.x },
            if horizontal { plot.bottom() } else { plot.bottom() - f * plot.h },
            horizontal,
            Some(kind),
            &axis.tick_line,
        );
        for primitive in &mut ctx.out[start..] {
            if let Prim::Line { pts, .. } = primitive {
                let center = if horizontal { plot.bottom() } else { plot.x };
                for p in pts {
                    let index = if horizontal { 1 } else { 0 };
                    p[index] = center + (p[index] - center) * 0.6;
                }
            }
        }
    }
}

fn frame(ctx: &mut Ctx, area: Bx, cat: CatAxis, y: &ValAxis, y2: Option<&ValAxis>, horizontal: bool, chart: &Chart) -> Bx {
    let reverse_categories =
        chart.format.x_axis.reversed && !matches!(chart.kind, ChartKind::Histogram | ChartKind::Waterfall | ChartKind::BoxWhisker | ChartKind::Stock);
    let grid = chart.gridlines;
    let gx = chart.format.vertical_gridline.color.unwrap_or(GRID);
    let gwx = chart.format.vertical_gridline.width.unwrap_or(0.75);
    let gy = chart.format.horizontal_gridline.color.unwrap_or(GRID);
    let gwy = chart.format.horizontal_gridline.width.unwrap_or(0.75);
    let ax = chart.format.x_axis.line.color.unwrap_or(AXIS);
    let awx = chart.format.x_axis.line.width.unwrap_or(0.75);
    let ay = chart.format.y_axis.line.color.unwrap_or(AXIS);
    let awy = chart.format.y_axis.line.width.unwrap_or(0.75);
    let ls = ctx.font_size("x_axis", LABEL_SIZE).max(ctx.font_size("y_axis", LABEL_SIZE));
    let lh = ls * 1.4;
    let ylabels = y.labels();
    let maxw = |ctx: &Ctx, l: &[(f64, String)]| l.iter().map(|(_, t)| ctx.tw(t, ls)).fold(0.0f32, f32::max);
    if !horizontal {
        let left = (maxw(ctx, &ylabels) + 6.0).min(area.w * 0.4);
        let y2labels = y2.map(|a| a.labels());
        let right = match (&y2labels, &cat) {
            (Some(l), _) => (maxw(ctx, l) + 6.0).min(area.w * 0.3),
            (None, CatAxis::Numeric(s, f)) => (ctx.tw(&f.format(s.max, Some(s.step)), ls) / 2.0).min(area.w * 0.2),
            _ => 4.0,
        };
        let levels = match &cat {
            CatAxis::Labels(_, o) => o.len() as f32,
            CatAxis::Numeric(..) => 0.0,
        };
        let plot = Bx { x: area.x + left, y: area.y + lh * 0.5, w: area.w - left - right, h: area.h - lh * 1.5 - 4.0 - lh * levels };
        if !plot.ok() {
            return plot;
        }
        plot_surface(ctx, plot, chart);
        minor_ticks(ctx, y.scale, &chart.format.y_axis, plot, false);
        if chart.format.y_axis.line.color.is_some() || chart.format.y_axis.line.width.is_some() {
            ctx.vline(plot.x, plot.y, plot.bottom(), ay, awy);
        }
        for (v, t) in &ylabels {
            let yy = plot.bottom() - y.scale.frac(*v) * plot.h;
            tick(ctx, plot.x, yy, false, chart.format.y_axis.major_tick.as_deref(), &chart.format.y_axis.tick_line);
            if grid {
                ctx.hline(plot.x, plot.right(), yy, gy, gwy);
            }
            ctx.text_role("y_axis", plot.x - 4.0, yy, t, ls, TEXT, HAlign::Right, VAlign::Middle);
        }
        if let (Some(a), Some(l)) = (y2, &y2labels) {
            for (v, t) in l {
                let yy = plot.bottom() - a.scale.frac(*v) * plot.h;
                ctx.text_role("y_axis", plot.right() + 4.0, yy, t, ls, TEXT, HAlign::Left, VAlign::Middle);
            }
        }
        let zero = if y.scale.min <= 0.0 && y.scale.max >= 0.0 { 0.0 } else { y.scale.min };
        let y0 = plot.bottom() - y.scale.frac(zero) * plot.h;
        ctx.hline(plot.x, plot.right(), y0, ax, awx);
        match cat {
            CatAxis::Labels(l, outer) => {
                let n = l.len().max(1);
                let cw = plot.w / n as f32;
                for j in (0..n.saturating_sub(1)).step_by(n.div_ceil(250).max(1)) {
                    tick(
                        ctx,
                        plot.x + (j + 1) as f32 * cw,
                        plot.bottom(),
                        true,
                        chart.format.x_axis.minor_tick.as_deref(),
                        &chart.format.x_axis.tick_line,
                    );
                }
                let lw = l.iter().take(2000).map(|t| ctx.tw(t, ls)).fold(0.0f32, f32::max);
                let k = (((lw + 6.0) / cw.max(1e-3)).ceil() as usize).clamp(1, n);
                for (j, t) in l.iter().enumerate().step_by(k) {
                    let t = ctx.fit(t, ls, cw * k as f32 - 2.0);
                    let j = if reverse_categories { n - 1 - j } else { j };
                    tick(
                        ctx,
                        plot.x + (j as f32 + 0.5) * cw,
                        plot.bottom(),
                        true,
                        chart.format.x_axis.major_tick.as_deref(),
                        &chart.format.x_axis.tick_line,
                    );
                    ctx.text_role("x_axis", plot.x + (j as f32 + 0.5) * cw, plot.bottom() + 3.0, &t, ls, TEXT, HAlign::Center, VAlign::Top);
                }
                if chart.format.vertical_gridlines {
                    for j in (0..n).step_by(k) {
                        ctx.vline(plot.x + (j as f32 + 0.5) * cw, plot.y, plot.bottom(), gx, gwx);
                    }
                }
                // Outer levels, one row each below the labels: a group's name centred under its
                // categories, and a separator at each group boundary down through the rows.
                for (i, level) in outer.iter().enumerate() {
                    let top = plot.bottom() + 3.0 + lh * (i as f32 + 1.0);
                    for (first, end) in level_groups(level, n) {
                        let (start, stop) = if reverse_categories { (n - end, n - first) } else { (first, end) };
                        let (x0, x1) = (plot.x + start as f32 * cw, plot.x + stop as f32 * cw);
                        if let Some(t) = level.get(first).filter(|t| !t.is_empty()) {
                            let t = ctx.fit(t, ls, x1 - x0 - 2.0);
                            ctx.text_role("x_axis", (x0 + x1) / 2.0, top, &t, ls, TEXT, HAlign::Center, VAlign::Top);
                        }
                        if first > 0 {
                            let edge = if reverse_categories { x1 } else { x0 };
                            ctx.vline(edge, plot.bottom(), top + lh - 2.0, ax, awx);
                        }
                    }
                    let edge = if reverse_categories { plot.x } else { plot.right() };
                    ctx.vline(edge, plot.bottom(), top + lh - 2.0, ax, awx);
                }
            }
            CatAxis::Numeric(s, f) => {
                minor_ticks(ctx, s, &chart.format.x_axis, plot, true);
                let ticks = s.ticks();
                if chart.format.vertical_gridlines {
                    for v in &ticks {
                        ctx.vline(plot.x + s.frac(*v) * plot.w, plot.y, plot.bottom(), gx, gwx);
                    }
                }
                let labels: Vec<String> = ticks.iter().map(|v| f.format(*v, Some(s.step))).collect();
                let lw = labels.iter().map(|t| ctx.tw(t, ls)).fold(0.0f32, f32::max);
                let slot = plot.w / ticks.len().max(1) as f32;
                let k = (((lw + 6.0) / slot.max(1e-3)).ceil() as usize).max(1);
                for (v, t) in ticks.iter().zip(&labels).step_by(k) {
                    let xx = plot.x + s.frac(*v) * plot.w;
                    tick(ctx, xx, plot.bottom(), true, chart.format.x_axis.major_tick.as_deref(), &chart.format.x_axis.tick_line);
                    ctx.text_role("x_axis", xx, plot.bottom() + 3.0, t, ls, TEXT, HAlign::Center, VAlign::Top);
                }
                if s.min <= 0.0 && s.max >= 0.0 {
                    let xx = plot.x + s.frac(0.0) * plot.w;
                    ctx.vline(xx, plot.y, plot.bottom(), ay, awy);
                }
            }
        }
        plot
    } else {
        let (labels, outer): (Vec<String>, &[Vec<String>]) = match cat {
            CatAxis::Labels(l, o) => (l.to_vec(), o),
            CatAxis::Numeric(..) => (vec![], &[]),
        };
        let lw = (labels.iter().take(2000).map(|t| ctx.tw(t, ls)).fold(0.0f32, f32::max) + 6.0).min(area.w * 0.35);
        // Outer levels stand in columns left of the labels, each as wide as its longest name.
        let level_w: Vec<f32> =
            outer.iter().map(|l| (l.iter().take(2000).map(|t| ctx.tw(t, ls)).fold(0.0f32, f32::max) + 6.0).min(area.w * 0.15)).collect();
        let levels_w: f32 = level_w.iter().sum();
        let right = ylabels.last().map(|(_, t)| ctx.tw(t, ls) / 2.0).unwrap_or(0.0).min(area.w * 0.2) + 2.0;
        let plot = Bx { x: area.x + lw + levels_w, y: area.y + 2.0, w: area.w - lw - levels_w - right, h: area.h - lh - 6.0 };
        if !plot.ok() {
            return plot;
        }
        plot_surface(ctx, plot, chart);
        minor_ticks(ctx, y.scale, &chart.format.x_axis, plot, true);
        let vw = maxw(ctx, &ylabels);
        let slot = plot.w / ylabels.len().max(1) as f32;
        let k = (((vw + 6.0) / slot.max(1e-3)).ceil() as usize).max(1);
        for (i, (v, t)) in ylabels.iter().enumerate() {
            let xx = plot.x + y.scale.frac(*v) * plot.w;
            tick(ctx, xx, plot.bottom(), true, chart.format.x_axis.major_tick.as_deref(), &chart.format.x_axis.tick_line);
            if grid {
                ctx.vline(xx, plot.y, plot.bottom(), gx, gwx);
            }
            if i % k == 0 {
                ctx.text_role("x_axis", xx, plot.bottom() + 3.0, t, ls, TEXT, HAlign::Center, VAlign::Top);
            }
        }
        let zero = if y.scale.min <= 0.0 && y.scale.max >= 0.0 { 0.0 } else { y.scale.min };
        let x0 = plot.x + y.scale.frac(zero) * plot.w;
        ctx.vline(x0, plot.y, plot.bottom(), ay, awy);
        let n = labels.len().max(1);
        let ch = plot.h / n as f32;
        for j in (0..n.saturating_sub(1)).step_by(n.div_ceil(250).max(1)) {
            tick(ctx, plot.x, plot.bottom() - (j + 1) as f32 * ch, false, chart.format.y_axis.minor_tick.as_deref(), &chart.format.y_axis.tick_line);
        }
        let k = ((lh / ch.max(1e-3)).ceil() as usize).clamp(1, n);
        for (j, t) in labels.iter().enumerate().step_by(k) {
            let t = ctx.fit(t, ls, lw - 6.0);
            let j = if chart.format.y_axis.reversed { n - 1 - j } else { j };
            tick(
                ctx,
                plot.x,
                plot.bottom() - (j as f32 + 0.5) * ch,
                false,
                chart.format.y_axis.major_tick.as_deref(),
                &chart.format.y_axis.tick_line,
            );
            ctx.text_role("y_axis", plot.x - 4.0, plot.bottom() - (j as f32 + 0.5) * ch, &t, ls, TEXT, HAlign::Right, VAlign::Middle);
        }
        if chart.format.vertical_gridlines {
            for j in (0..n).step_by(k) {
                ctx.hline(plot.x, plot.right(), plot.bottom() - (j as f32 + 0.5) * ch, gy, gwy);
            }
        }
        // Outer levels: a group's name centred beside its categories, separators at the
        // boundaries running right to the axis.
        let mut x1 = plot.x - lw;
        for (level, w) in outer.iter().zip(&level_w) {
            let x0 = x1 - w;
            for (first, end) in level_groups(level, n) {
                let (start, stop) = if chart.format.y_axis.reversed { (n - end, n - first) } else { (first, end) };
                let (y0, y1) = (plot.bottom() - stop as f32 * ch, plot.bottom() - start as f32 * ch);
                if let Some(t) = level.get(first).filter(|t| !t.is_empty()) {
                    let t = ctx.fit(t, ls, w - 4.0);
                    ctx.text_role("y_axis", (x0 + x1) / 2.0, (y0 + y1) / 2.0, &t, ls, TEXT, HAlign::Center, VAlign::Middle);
                }
                if first > 0 {
                    let edge = if chart.format.y_axis.reversed { y0 } else { y1 };
                    ctx.hline(x0, plot.x, edge, ay, awy);
                }
            }
            let edge = if chart.format.y_axis.reversed { plot.bottom() } else { plot.y };
            ctx.hline(x0, plot.x, edge, ay, awy);
            x1 = x0;
        }
        plot
    }
}

// ---------------------------------------------------------------- helpers

/// Reduces a long polyline to first/min/max/last per pixel column.
fn decimate(pts: Vec<[f32; 2]>, max: usize) -> Vec<[f32; 2]> {
    if pts.len() <= max {
        return pts;
    }
    let mut out = Vec::with_capacity(max * 2);
    let mut i = 0;
    while let Some(p) = pts.get(i) {
        let col = p[0].floor();
        let (start, mut mn, mut mx) = (i, i, i);
        let mut k = i + 1;
        while let Some(q) = pts.get(k) {
            if q[0].floor() != col {
                break;
            }
            if pts.get(mn).is_some_and(|m| q[1] < m[1]) {
                mn = k;
            }
            if pts.get(mx).is_some_and(|m| q[1] > m[1]) {
                mx = k;
            }
            k += 1;
        }
        let mut idx = [start, mn, mx, k - 1];
        idx.sort_unstable();
        let mut last = usize::MAX;
        for ix in idx {
            if ix != last
                && let Some(q) = pts.get(ix)
            {
                out.push(*q);
                last = ix;
            }
        }
        i = k;
    }
    if out.len() > max * 4 {
        let step = out.len() / (max * 2) + 1;
        let last = out.last().copied();
        let mut s: Vec<[f32; 2]> = out.into_iter().step_by(step).collect();
        if let Some(l) = last
            && s.last() != Some(&l)
        {
            s.push(l);
        }
        return s;
    }
    out
}

/// Draws a polyline split at gaps (`None`), decimated when long. With `smooth` (the plot box),
/// each run is interpolated with a Catmull-Rom spline so the line curves through the data points.
fn draw_runs(ctx: &mut Ctx, pts: &[Option<[f32; 2]>], color: Rgba, width: f32, smooth: Option<Bx>) {
    let mut run: Vec<[f32; 2]> = Vec::new();
    for p in pts.iter().chain(std::iter::once(&None)) {
        match p {
            Some(p) => run.push(*p),
            None => {
                if run.len() >= 2 {
                    let r = decimate(std::mem::take(&mut run), MAX_LINE_PTS);
                    let r = match smooth {
                        Some(plot) => smooth_run(r, plot),
                        None => r,
                    };
                    ctx.line(r, color, width);
                } else {
                    run.clear();
                }
            }
        }
    }
}

/// Catmull-Rom spline through `pts`: every segment gets the same number of sub-segments (up to
/// 100), chosen so the whole curve stays within `MAX_LINE_PTS * 2` points. Endpoints are
/// duplicated so the curve passes through the first and last points, and the curve is clamped
/// to the `plot` box (a spline overshoots its data near sharp turns).
fn smooth_run(pts: Vec<[f32; 2]>, plot: Bx) -> Vec<[f32; 2]> {
    let n = pts.len();
    if n < 3 {
        return pts;
    }
    let cap = MAX_LINE_PTS * 2;
    let steps = (cap / n).clamp(1, 100);
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(n.saturating_mul(steps).min(cap) + 1);
    let at = |i: isize| -> [f32; 2] {
        let j = i.clamp(0, n as isize - 1) as usize;
        pts.get(j).copied().unwrap_or([0.0, 0.0])
    };
    let (x0, x1) = (plot.x.min(plot.right()), plot.x.max(plot.right()));
    let (y0, y1) = (plot.y.min(plot.bottom()), plot.y.max(plot.bottom()));
    let clip = |p: [f32; 2]| [p[0].clamp(x0, x1), p[1].clamp(y0, y1)];
    for i in 0..n.saturating_sub(1) {
        let (p0, p1, p2, p3) = (at(i as isize - 1), at(i as isize), at(i as isize + 1), at(i as isize + 2));
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            out.push(clip(catmull(p0, p1, p2, p3, t)));
        }
    }
    out.push(clip(at(n as isize - 1)));
    out
}

/// One Catmull-Rom point between `p1` and `p2` (uniform parameterisation), tangent-scaled by 0.5.
fn catmull(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let t2 = t * t;
    let t3 = t2 * t;
    let mut o = [0.0f32; 2];
    for k in 0..2 {
        let (a0, a1, a2, a3) = (p0[k], p1[k], p2[k], p3[k]);
        o[k] = 0.5 * ((2.0 * a1) + (-a0 + a2) * t + (2.0 * a0 - 5.0 * a1 + 4.0 * a2 - a3) * t2 + (-a0 + 3.0 * a1 - 3.0 * a2 + a3) * t3);
    }
    o
}

fn draw_markers(ctx: &mut Ctx, pts: impl Iterator<Item = [f32; 2]>, r: f32, color: Rgba) {
    let mut seen = HashSet::new();
    for p in pts {
        if seen.len() >= MAX_MARKERS {
            break;
        }
        if seen.insert(((p[0] * 0.5) as i32, (p[1] * 0.5) as i32)) {
            ctx.out.push(Prim::Circle { cx: p[0], cy: p[1], r, fill: Some(color), stroke: Some((WHITE, 0.75)) });
        }
    }
}

fn inside_plot(p: [f32; 2], plot: Bx) -> bool {
    p[0] >= plot.x && p[0] <= plot.right() && p[1] >= plot.y && p[1] <= plot.bottom()
}

fn draw_clipped_runs(ctx: &mut Ctx, pts: &[Option<[f32; 2]>], color: Rgba, plot: Bx, smooth: bool) {
    let units: Vec<_> = pts.iter().map(|p| p.map(|p| (((p[0] - plot.x) / plot.w) as f64, ((p[1] - plot.y) / plot.h) as f64))).collect();
    for run in clip_unit(&units) {
        let pts: Vec<_> = run.into_iter().map(|(x, y)| Some([plot.x + x as f32 * plot.w, plot.y + y as f32 * plot.h])).collect();
        draw_runs(ctx, &pts, color, 2.25, smooth.then_some(plot));
    }
}

fn style_series(ctx: &mut Ctx, start: usize, spec: Option<&gridcraft_model::ChartSeriesFormat>, color: Rgba) {
    let Some(spec) = spec else { return };
    let prims: Vec<_> = ctx.out.drain(start..).collect();
    for mut prim in prims {
        match &mut prim {
            Prim::Line { color: c, width, dash, .. } if *c == color => {
                if spec.no_line {
                    continue;
                }
                *width = spec.line_width.unwrap_or(*width);
                *dash = spec.dashed;
                *c = spec.line_color.unwrap_or(color);
            }
            Prim::Rect { fill: Some(c), stroke, .. } | Prim::Polygon { fill: c, stroke, .. } if *c == color => {
                *c = spec.fill_color.unwrap_or(color);
                if let Some(w) = spec.border_width {
                    *stroke = (w > 0.0).then_some((DARK_TEXT, w));
                }
            }
            Prim::Circle { cx, cy, r, fill, stroke } if *fill == Some(color) => {
                if spec.marker.as_deref() == Some("none") {
                    continue;
                }
                *r = spec.marker_size.unwrap_or(*r * 2.0) / 2.0;
                let marker_color = spec.marker_fill.unwrap_or(color);
                *fill = if spec.marker_no_fill { None } else { Some(marker_color) };
                let border_width = spec.marker_border_width.or(spec.border_width);
                let border_color = spec.marker_border_color.unwrap_or(DARK_TEXT);
                if let Some(w) = border_width {
                    *stroke = (w > 0.0).then_some((border_color, w));
                } else if spec.marker_border_color.is_some() || spec.marker_no_fill {
                    *stroke = Some((border_color, 1.0));
                }
                let (x, y, r) = (*cx, *cy, *r);
                let pts = match spec.marker.as_deref() {
                    Some("square") => Some(vec![[x - r, y - r], [x + r, y - r], [x + r, y + r], [x - r, y + r]]),
                    Some("diamond") => Some(vec![[x, y - r], [x + r, y], [x, y + r], [x - r, y]]),
                    Some("triangle") => Some(vec![[x, y - r], [x + r, y + r], [x - r, y + r]]),
                    Some("cross") => {
                        ctx.line(vec![[x - r, y - r], [x + r, y + r]], spec.marker_border_color.unwrap_or(marker_color), border_width.unwrap_or(1.5));
                        ctx.line(vec![[x - r, y + r], [x + r, y - r]], spec.marker_border_color.unwrap_or(marker_color), border_width.unwrap_or(1.5));
                        continue;
                    }
                    Some("plus") | Some("dash") => {
                        let c = spec.marker_border_color.unwrap_or(marker_color);
                        ctx.line(vec![[x - r, y], [x + r, y]], c, border_width.unwrap_or(1.5));
                        if spec.marker.as_deref() == Some("plus") {
                            ctx.line(vec![[x, y - r], [x, y + r]], c, border_width.unwrap_or(1.5));
                        }
                        continue;
                    }
                    Some("star") => {
                        let pts: Vec<_> = (0..10)
                            .map(|i| {
                                let a = -PI / 2.0 + i as f32 * PI / 5.0;
                                let radius = if i % 2 == 0 { r } else { r * 0.4 };
                                [x + radius * a.cos(), y + radius * a.sin()]
                            })
                            .collect();
                        for i in 0..pts.len() {
                            ctx.out.push(Prim::Polygon {
                                pts: vec![[x, y], pts[i], pts[(i + 1) % pts.len()]],
                                fill: fill.unwrap_or([0, 0, 0, 0]),
                                stroke: None,
                            });
                        }
                        if let Some((color, width)) = *stroke {
                            let mut closed = pts;
                            if let Some(first) = closed.first().copied() {
                                closed.push(first);
                            }
                            ctx.line(closed, color, width);
                        }
                        continue;
                    }
                    _ => None,
                };
                if let Some(pts) = pts {
                    prim = Prim::Polygon { pts, fill: fill.unwrap_or([0, 0, 0, 0]), stroke: *stroke };
                }
            }
            _ => {}
        }
        ctx.out.push(prim);
    }
}

fn draw_area_run(ctx: &mut Ctx, mut top: Vec<[f32; 2]>, mut base: Vec<[f32; 2]>, cw: f32, color: Rgba) {
    if top.len() == 1 {
        // A single point: give it some width.
        if let (Some(t), Some(b)) = (top.first().copied(), base.first().copied()) {
            top = vec![[t[0] - cw * 0.3, t[1]], [t[0] + cw * 0.3, t[1]]];
            base = vec![[b[0] - cw * 0.3, b[1]], [b[0] + cw * 0.3, b[1]]];
        }
    }
    let top = decimate(top, MAX_LINE_PTS * 2);
    let base = decimate(base, MAX_LINE_PTS * 2);
    let mut poly = top;
    poly.extend(base.into_iter().rev());
    if poly.len() >= 3 {
        ctx.out.push(Prim::Polygon { pts: poly, fill: color, stroke: None });
    }
}

fn value_range(it: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    it.fold(None, |acc, v| match acc {
        None => Some((v, v)),
        Some((a, b)) => Some((a.min(v), b.max(v))),
    })
}

// ---------------------------------------------------------------- trendlines

/// Samples along a regression trendline.
const TREND_STEPS: usize = 200;

/// Whether series drawn as `k` show trendlines (Excel offers them on 2-D unstacked column,
/// bar, line, area, scatter and bubble charts).
pub fn supports_trendline(k: ChartKind) -> bool {
    use ChartKind::*;
    matches!(k, ColumnClustered | BarClustered | Line | LineMarkers | Area | Scatter | ScatterLines | Bubble)
}

/// A forecast distance as drawn: finite, non-negative and bounded.
fn forecast(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v.min(1e9) } else { 0.0 }
}

/// Most empty category slots a forward forecast may add after `ncat` categories: four times the
/// data (at least 32), and never past `MAX_CATS` in total. A larger forecast would shrink every
/// column to nothing.
fn max_forward_slots(ncat: usize) -> f64 {
    (ncat.saturating_mul(4).max(32)).min(MAX_CATS.saturating_sub(ncat)) as f64
}

/// Longest forward forecast among `s`'s regression trendlines.
fn max_forward(s: &SeriesData) -> f64 {
    s.trendlines.iter().filter(|t| !matches!(t.spec.kind, TrendlineKind::MovingAverage(_))).map(|t| forecast(t.spec.forward)).fold(0.0, f64::max)
}

/// Longest backward forecast among `s`'s regression trendlines.
fn max_backward(s: &SeriesData) -> f64 {
    s.trendlines.iter().filter(|t| !matches!(t.spec.kind, TrendlineKind::MovingAverage(_))).map(|t| forecast(t.spec.backward)).fold(0.0, f64::max)
}

/// Liang–Barsky: the part of segment `a`–`b` inside the unit square, with whether the start and
/// end were cut.
#[allow(clippy::type_complexity)]
fn clip_segment(a: (f64, f64), b: (f64, f64)) -> Option<((f64, f64), (f64, f64), bool, bool)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.0), (dx, 1.0 - a.0), (-dy, a.1), (dy, 1.0 - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return None;
                }
                t1 = t1.min(r);
            }
        }
    }
    Some(((a.0 + t0 * dx, a.1 + t0 * dy), (a.0 + t1 * dx, a.1 + t1 * dy), t0 > 0.0, t1 < 1.0))
}

/// Clips a sampled polyline in plot fractions (`None` = gap) to the plot box (the unit
/// square), returning the visible runs.
fn clip_unit(pts: &[Option<(f64, f64)>]) -> Vec<Vec<(f64, f64)>> {
    const LIM: f64 = 1e9;
    let ok = |p: &(f64, f64)| p.0.is_finite() && p.1.is_finite();
    let cl = |p: (f64, f64)| (p.0.clamp(-LIM, LIM), p.1.clamp(-LIM, LIM));
    let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut run: Vec<(f64, f64)> = Vec::new();
    for w in pts.windows(2) {
        let seg = match w {
            [Some(a), Some(b)] if ok(a) && ok(b) => clip_segment(cl(*a), cl(*b)),
            _ => None,
        };
        match seg {
            Some((p, q, cut_start, cut_end)) => {
                if cut_start || run.is_empty() {
                    if run.len() >= 2 {
                        runs.push(std::mem::take(&mut run));
                    }
                    run.clear();
                    run.push(p);
                }
                run.push(q);
                if cut_end {
                    runs.push(std::mem::take(&mut run));
                }
            }
            None => {
                if run.len() >= 2 {
                    runs.push(std::mem::take(&mut run));
                }
                run.clear();
            }
        }
    }
    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

/// Draws `s`'s trendlines fitted to its plotted points `pts` (data x, y). `frac` maps data to
/// fractions of the plot box (0..1 inside, along the x and y axes), `px` fractions to pixels.
/// Equation and R² label lines are appended to `labels`. Degenerate fits draw nothing.
fn draw_trendlines(
    ctx: &mut Ctx,
    s: &SeriesData,
    pts: &[(f64, f64)],
    frac: &dyn Fn(f64, f64) -> (f64, f64),
    px: &dyn Fn((f64, f64)) -> [f32; 2],
    labels: &mut Vec<String>,
    window: (f64, f64),
) {
    let Some((xlo, xhi)) = value_range(pts.iter().map(|p| p.0)) else { return };
    for t in s.trendlines.iter().take(MAX_TRENDLINES) {
        let spec = &t.spec;
        let samples: Vec<Option<(f64, f64)>> = match spec.kind {
            TrendlineKind::MovingAverage(p) => moving_average(pts, p).into_iter().map(|(x, y)| Some(frac(x, y))).collect(),
            _ => {
                let Some(fit) = fit_trendline(spec, pts) else { continue };
                // Sample only the part of the forecast that is on the plot.
                let (lo, hi) = ((xlo - forecast(spec.backward)).max(window.0), (xhi + forecast(spec.forward)).min(window.1));
                if spec.display_equation {
                    labels.push(fit.equation());
                }
                if spec.display_r_squared {
                    labels.push(fit.r_squared());
                }
                (0..=TREND_STEPS)
                    .map(|i| {
                        let x = lo + (hi - lo) * i as f64 / TREND_STEPS as f64;
                        fit.eval(x).map(|y| frac(x, y))
                    })
                    .collect()
            }
        };
        let width = spec.width.filter(|w| w.is_finite()).unwrap_or(1.5).clamp(0.25, 12.0);
        let dash = spec.dash != Some(LineDash::Solid);
        for run in clip_unit(&samples) {
            let line = decimate(run.into_iter().map(px).collect(), MAX_LINE_PTS);
            if line.len() >= 2 {
                ctx.out.push(Prim::Line { pts: line, color: t.color, width, dash });
            }
        }
    }
}

/// Trendline equation / R² labels, stacked in the plot's top-right corner.
fn draw_trend_labels(ctx: &mut Ctx, plot: Bx, labels: &[String]) {
    let start = ctx.out.len();
    let size = ctx.font_size("trendline", LABEL_SIZE);
    let lh = size * 1.3;
    for (i, t) in labels.iter().enumerate() {
        let y = plot.y + 4.0 + i as f32 * lh;
        if y + lh > plot.bottom() {
            break;
        }
        let t = ctx.fit(t, size, plot.w - 8.0);
        ctx.text_role("trendline", plot.right() - 4.0, y, &t, size, TEXT, HAlign::Right, VAlign::Top);
    }
    if let Some(position) = ctx.format.trendline_label_position {
        place_group(ctx, start, position, plot);
    }
}

/// Move a complete annotation group, including legend swatches. Clamp to its plot.
fn place_group(ctx: &mut Ctx, start: usize, position: [f32; 2], plot: Bx) {
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    let mut include = |x: f32, y: f32| {
        lo[0] = lo[0].min(x);
        lo[1] = lo[1].min(y);
        hi[0] = hi[0].max(x);
        hi[1] = hi[1].max(y);
    };
    for p in ctx.out.iter().skip(start) {
        match p {
            Prim::Text { x, y, text, size, bold, align, valign, .. } => {
                let w = ctx.m.text_width(text, *size, *bold);
                let h = *size * 1.3;
                let x = *x
                    - match align {
                        HAlign::Left => 0.0,
                        HAlign::Center => w / 2.0,
                        HAlign::Right => w,
                    };
                let y = *y
                    - match valign {
                        VAlign::Top => 0.0,
                        VAlign::Middle => h / 2.0,
                        VAlign::Bottom => h,
                    };
                include(x, y);
                include(x + w, y + h);
            }
            Prim::Rect { x, y, w, h, .. } => {
                include(*x, *y);
                include(x + w, y + h);
            }
            Prim::Circle { cx, cy, r, .. } => {
                include(cx - r, cy - r);
                include(cx + r, cy + r);
            }
            Prim::Line { pts, .. } | Prim::Polygon { pts, .. } => {
                for p in pts {
                    include(p[0], p[1]);
                }
            }
            Prim::Wedge { cx, cy, r_outer, .. } => {
                include(cx - r_outer, cy - r_outer);
                include(cx + r_outer, cy + r_outer);
            }
        }
    }
    if !lo[0].is_finite() || !position.iter().all(|v| v.is_finite()) {
        return;
    }
    let dx = (position[0] * ctx.size[0]).clamp(plot.x, (plot.right() - (hi[0] - lo[0])).max(plot.x)) - lo[0];
    let dy = (position[1] * ctx.size[1]).clamp(plot.y, (plot.bottom() - (hi[1] - lo[1])).max(plot.y)) - lo[1];
    for p in ctx.out.iter_mut().skip(start) {
        match p {
            Prim::Rect { x, y, .. } | Prim::Text { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Prim::Circle { cx, cy, .. } | Prim::Wedge { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            Prim::Line { pts, .. } | Prim::Polygon { pts, .. } => {
                for p in pts {
                    p[0] += dx;
                    p[1] += dy;
                }
            }
        }
    }
}

// ---------------------------------------------------------------- cartesian (column/bar/line/area/combo)

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fam {
    Col,
    ColStack,
    Col100,
    Line,
    LineStack,
    Area,
    AreaStack,
}

fn family(k: ChartKind) -> Fam {
    use ChartKind::*;
    match k {
        ColumnStacked | BarStacked => Fam::ColStack,
        ColumnStacked100 | BarStacked100 => Fam::Col100,
        Line | LineMarkers | Scatter | ScatterLines | Radar | Stock => Fam::Line,
        LineStacked => Fam::LineStack,
        Area => Fam::Area,
        AreaStacked => Fam::AreaStack,
        _ => Fam::Col,
    }
}

fn is_col(f: Fam) -> bool {
    matches!(f, Fam::Col | Fam::ColStack | Fam::Col100)
}
fn is_line(f: Fam) -> bool {
    matches!(f, Fam::Line | Fam::LineStack)
}

fn cartesian(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx, horizontal: bool) {
    let ncat = data.categories.len().max(data.series.iter().map(|s| s.values.len()).max().unwrap_or(0)).min(MAX_CATS);
    if ncat == 0 {
        return;
    }
    let kinds: Vec<ChartKind> = data.series.iter().map(|s| if horizontal { chart.kind } else { plot_kind(chart, s) }).collect();
    let fams: Vec<Fam> = kinds.iter().map(|k| family(*k)).collect();
    let any_primary = data.series.iter().any(|s| !s.secondary);
    let sec: Vec<bool> = data.series.iter().map(|s| s.secondary && any_primary && !horizontal).collect();

    // Totals for 100% stacking, per axis.
    let mut totals = [vec![0.0f64; ncat], vec![0.0f64; ncat]];
    for (i, s) in data.series.iter().enumerate() {
        if fams.get(i) == Some(&Fam::Col100) {
            let a = sec.get(i).copied().unwrap_or(false) as usize;
            for (j, v) in s.values.iter().take(ncat).enumerate() {
                if let (Some(v), Some(t)) = (v, totals.get_mut(a).and_then(|t| t.get_mut(j))) {
                    *t += v.abs();
                }
            }
        }
    }
    // Stack accumulators keyed by (axis, family): positive and negative running sums.
    let mut stacks: Vec<((bool, Fam), Vec<f64>, Vec<f64>)> = Vec::new();
    let mut spans: Vec<Vec<Option<(f64, f64)>>> = Vec::with_capacity(data.series.len());
    for (i, s) in data.series.iter().enumerate() {
        let fam = fams.get(i).copied().unwrap_or(Fam::Col);
        let a = sec.get(i).copied().unwrap_or(false);
        let stacked = matches!(fam, Fam::ColStack | Fam::Col100 | Fam::LineStack | Fam::AreaStack);
        let key = (a, fam);
        let si = if stacked {
            match stacks.iter().position(|(k, _, _)| *k == key) {
                Some(p) => Some(p),
                None => {
                    stacks.push((key, vec![0.0; ncat], vec![0.0; ncat]));
                    Some(stacks.len() - 1)
                }
            }
        } else {
            None
        };
        let mut row = Vec::with_capacity(ncat);
        for j in 0..ncat {
            let v = s.values.get(j).copied().flatten();
            let span = match (si.and_then(|p| stacks.get_mut(p)), fam) {
                (Some((_, pos, neg)), Fam::ColStack | Fam::Col100) => v.and_then(|v| {
                    let v = if fam == Fam::Col100 {
                        let t = totals.get(a as usize).and_then(|t| t.get(j)).copied().unwrap_or(0.0);
                        if t > 0.0 { v / t } else { 0.0 }
                    } else {
                        v
                    };
                    let acc = if v >= 0.0 { pos.get_mut(j) } else { neg.get_mut(j) }?;
                    let base = *acc;
                    *acc += v;
                    Some((base, *acc))
                }),
                (Some((_, pos, _)), _) => {
                    let v = v.unwrap_or(0.0);
                    pos.get_mut(j).map(|acc| {
                        let base = *acc;
                        *acc = clean(*acc + v);
                        (base, *acc)
                    })
                }
                (None, _) => v.map(|v| (0.0, v)),
            };
            row.push(span);
        }
        spans.push(row);
    }

    // Axis scales.
    let mut axes: Vec<(Scale, Fmt)> = Vec::new();
    for axis in [false, true] {
        let members: Vec<usize> = (0..data.series.len()).filter(|i| sec.get(*i).copied().unwrap_or(false) == axis).collect();
        if members.is_empty() {
            continue;
        }
        let force_zero = members.iter().any(|i| fams.get(*i).is_some_and(|f| !is_line(*f)));
        let all100 = members.iter().all(|i| fams.get(*i) == Some(&Fam::Col100));
        let range = value_range(members.iter().flat_map(|i| {
            let line = fams.get(*i).is_some_and(|f| is_line(*f));
            spans.get(*i).into_iter().flatten().flatten().flat_map(move |(b, t)| if line { vec![*t] } else { vec![*b, *t] })
        }));
        let (mut lo, hi) = range.unwrap_or((0.0, 0.0));
        let primary_options = if horizontal { &chart.format.x_axis } else { &chart.format.y_axis };
        if !axis && primary_options.logarithmic {
            lo = members
                .iter()
                .filter_map(|i| data.series.get(*i))
                .flat_map(|s| s.values.iter().flatten().copied())
                .filter(|v| *v > 0.0)
                .fold(hi, f64::min);
        }
        let (scale, fmt) = if all100 {
            (fixed_scale(if lo < -1e-9 { -1.0 } else { 0.0 }, if hi > 1e-9 || lo >= -1e-9 { 1.0 } else { 0.0 }, 0.2), Fmt::new("0%"))
        } else {
            let code = members.first().and_then(|i| data.series.get(*i)).map(|s| s.number_format.as_str()).unwrap_or("General");
            (
                if axis {
                    nice_scale(lo, hi, force_zero)
                } else {
                    configured_scale(lo, hi, force_zero, if horizontal { &chart.format.x_axis } else { &chart.format.y_axis })
                },
                Fmt::new(code),
            )
        };
        let mut scale = if !axis
            && all100
            && (primary_options.minimum.is_some()
                || primary_options.maximum.is_some()
                || primary_options.major_unit.is_some()
                || primary_options.logarithmic)
        {
            configured_scale(scale.min, scale.max, false, primary_options)
        } else {
            scale
        };
        if !axis {
            scale.reversed = primary_options.reversed;
        }
        let fmt = if !axis { primary_options.number_format.as_deref().map(Fmt::new).unwrap_or(fmt) } else { fmt };
        axes.push((scale, fmt));
        if !axis && sec.iter().all(|s| !s) {
            break;
        }
    }
    let Some((s1, f1)) = axes.first() else { return };
    let y1 = ValAxis { scale: *s1, fmt: f1 };
    let y2 = axes.get(1).map(|(s, f)| ValAxis { scale: *s, fmt: f });
    // A forward forecast on a category axis adds empty category slots after the data.
    let fwd = data
        .series
        .iter()
        .enumerate()
        .filter(|(i, _)| kinds.get(*i).is_some_and(|k| supports_trendline(*k)))
        .map(|(_, s)| max_forward(s))
        .fold(0.0, f64::max);
    let ncat_slots = ncat.saturating_add(fwd.min(max_forward_slots(ncat)).ceil() as usize).min(MAX_CATS.max(ncat));
    let labels: Vec<String> = (0..ncat_slots).map(|j| if j < ncat { cat_label(data, j) } else { String::new() }).collect();
    // Outer levels padded to the slots (forecast slots stay in the last group).
    let outer: Vec<Vec<String>> =
        data.category_levels.iter().map(|l| (0..ncat_slots).map(|j| l.get(j).cloned().unwrap_or_default()).collect()).collect();
    let plot = frame(ctx, area, CatAxis::Labels(&labels, &outer), &y1, y2.as_ref(), horizontal, chart);
    if !plot.ok() {
        return;
    }
    let scale_of = |i: usize| -> Scale { if sec.get(i).copied().unwrap_or(false) { y2.as_ref().map(|a| a.scale).unwrap_or(*s1) } else { *s1 } };
    let cat_len = if horizontal { plot.h } else { plot.w };
    let cw = cat_len / ncat_slots as f32;
    let center = |j: usize| {
        let reverse = if horizontal { chart.format.y_axis.reversed } else { chart.format.x_axis.reversed };
        let j = if reverse { ncat_slots - 1 - j } else { j };
        if horizontal { plot.bottom() - (j as f32 + 0.5) * cw } else { plot.x + (j as f32 + 0.5) * cw }
    };
    let vpos = |v: f64, sc: &Scale| {
        let v = if sc.logarithmic && v <= 0.0 { f64::NAN } else { v };
        let f = sc.frac(v).clamp(0.0, 1.0);
        if horizontal { plot.x + f * plot.w } else { plot.bottom() - f * plot.h }
    };

    // Column slots: each clustered series gets one, each stack group shares one.
    let mut slot_keys: Vec<(bool, Fam, usize)> = Vec::new();
    let mut slot_of = vec![0usize; data.series.len()];
    for (i, f) in fams.iter().enumerate() {
        if !is_col(*f) {
            continue;
        }
        let a = sec.get(i).copied().unwrap_or(false);
        let key = if *f == Fam::Col { (a, *f, i) } else { (a, *f, usize::MAX) };
        let k = match slot_keys.iter().position(|k| *k == key) {
            Some(p) => p,
            None => {
                slot_keys.push(key);
                slot_keys.len() - 1
            }
        };
        if let Some(s) = slot_of.get_mut(i) {
            *s = k;
        }
    }
    let nslots = slot_keys.len().max(1) as f32;
    let gap = chart.format.gap_width.unwrap_or(150.0).clamp(0.0, 500.0) / 100.0;
    let separation = 1.0 - chart.format.overlap.unwrap_or(0.0).clamp(-100.0, 100.0) / 100.0;
    let group = 1.0 + (nslots - 1.0) * separation;
    let bw = cw / (group + gap).max(0.01);
    let group_width = bw * group;
    let labels_on = chart.data_labels && ncat <= MAX_LABELS;
    // Area labels wait until every area is filled, so a later (stacked) area can't paint over them.
    let mut area_labels = Vec::new();

    // Areas first, then columns, then lines on top.
    for pass in 0..3 {
        for (i, s) in data.series.iter().enumerate() {
            let fam = fams.get(i).copied().unwrap_or(Fam::Col);
            let want = match pass {
                0 => matches!(fam, Fam::Area | Fam::AreaStack),
                1 => is_col(fam),
                _ => is_line(fam),
            };
            if !want {
                continue;
            }
            let sc = scale_of(i);
            let style_start = ctx.out.len();
            let fmt = Fmt::new(&s.number_format);
            let row = match spans.get(i) {
                Some(r) => r,
                None => continue,
            };
            match pass {
                0 => {
                    let mut top = Vec::with_capacity(ncat * 2);
                    let mut base = Vec::with_capacity(ncat);
                    for (j, sp) in row.iter().enumerate() {
                        let (b, t) = sp.unwrap_or((0.0, 0.0));
                        if sc.logarithmic && !(t.is_finite() && t > 0.0) {
                            draw_area_run(ctx, std::mem::take(&mut top), std::mem::take(&mut base), cw, s.color);
                            continue;
                        }
                        let c = center(j);
                        top.push([c, vpos(t, &sc)]);
                        let baseline = if sc.logarithmic && b <= 0.0 { sc.min } else { b };
                        base.push([c, vpos(baseline, &sc)]);
                        // Source values (not stack totals), just above the top edge like the line labels.
                        if labels_on
                            && sp.is_some()
                            && let Some(v) = s.values.get(j).copied().flatten()
                        {
                            area_labels.push((c, vpos(t, &sc) - 5.0, fmt.format(v, None)));
                        }
                    }
                    draw_area_run(ctx, top, base, cw, s.color);
                }
                1 => {
                    let k = slot_of.get(i).copied().unwrap_or(0) as f32;
                    for (j, sp) in row.iter().enumerate() {
                        let Some((b, t)) = sp else { continue };
                        let baseline = if sc.logarithmic && *b <= 0.0 { sc.min } else { *b };
                        let (pb, pt) = (vpos(baseline, &sc), vpos(*t, &sc));
                        let c = center(j);
                        let lo = pb.min(pt);
                        let len = (pb - pt).abs();
                        if horizontal {
                            let y = c + group_width / 2.0 - bw - k * separation * bw;
                            ctx.rect(lo, y, len, bw, s.color, None);
                        } else {
                            let x = c - group_width / 2.0 + k * separation * bw;
                            ctx.rect(x, lo, bw, len, s.color, None);
                        }
                        if labels_on && let Some(v) = s.values.get(j).copied().flatten() {
                            let t = fmt.format(v, None);
                            let stacked = fam != Fam::Col;
                            let mid = (pb + pt) / 2.0;
                            if horizontal {
                                let y = c + group_width / 2.0 - bw / 2.0 - k * separation * bw;
                                if stacked {
                                    ctx.text(mid, y, &t, LABEL_SIZE, on(s.color), HAlign::Center, VAlign::Middle);
                                } else if v >= 0.0 {
                                    ctx.text(pb.max(pt) + 3.0, y, &t, LABEL_SIZE, TEXT, HAlign::Left, VAlign::Middle);
                                } else {
                                    ctx.text(lo - 3.0, y, &t, LABEL_SIZE, TEXT, HAlign::Right, VAlign::Middle);
                                }
                            } else {
                                let x = c - group_width / 2.0 + bw / 2.0 + k * separation * bw;
                                if stacked {
                                    ctx.text(x, mid, &t, LABEL_SIZE, on(s.color), HAlign::Center, VAlign::Middle);
                                } else if v >= 0.0 {
                                    ctx.text(x, lo - 2.0, &t, LABEL_SIZE, TEXT, HAlign::Center, VAlign::Bottom);
                                } else {
                                    ctx.text(x, lo + len + 2.0, &t, LABEL_SIZE, TEXT, HAlign::Center, VAlign::Top);
                                }
                            }
                        }
                    }
                }
                _ => {
                    let pts: Vec<Option<[f32; 2]>> = row
                        .iter()
                        .enumerate()
                        .map(|(j, sp)| {
                            sp.filter(|(_, t)| !sc.logarithmic || *t > 0.0).map(|(_, t)| {
                                let f = sc.frac_raw(t);
                                if horizontal { [plot.x + f * plot.w, center(j)] } else { [center(j), plot.bottom() - f * plot.h] }
                            })
                        })
                        .collect();
                    draw_clipped_runs(ctx, &pts, s.color, plot, chart.format.series.get(&i).and_then(|s| s.smooth).unwrap_or(s.smooth));
                    let k = kinds.get(i).copied().unwrap_or(ChartKind::Line);
                    if (k == ChartKind::LineMarkers || chart.format.series.get(&i).is_some_and(|s| s.marker.is_some())) && cw >= 3.0 {
                        draw_markers(ctx, pts.iter().flatten().copied().filter(|p| inside_plot(*p, plot)), 3.5, s.color);
                    }
                    if labels_on {
                        for (j, p) in pts.iter().enumerate() {
                            if let (Some(p), Some(v)) = (p, s.values.get(j).copied().flatten())
                                && inside_plot(*p, plot)
                            {
                                ctx.text(p[0], p[1] - 5.0, &fmt.format(v, None), LABEL_SIZE, TEXT, HAlign::Center, VAlign::Bottom);
                            }
                        }
                    }
                }
            }
            style_series(ctx, style_start, chart.format.series.get(&i), s.color);
        }
        for (x, y, t) in area_labels.drain(..) {
            ctx.text(x, y, &t, LABEL_SIZE, TEXT, HAlign::Center, VAlign::Bottom);
        }
    }
    // Trendlines on top: x is the category number 1..n.
    let mut trend_labels = Vec::new();
    for (i, s) in data.series.iter().enumerate() {
        if s.trendlines.is_empty() || !kinds.get(i).is_some_and(|k| supports_trendline(*k)) {
            continue;
        }
        let sc = scale_of(i);
        let pts: Vec<(f64, f64)> = s.values.iter().take(ncat).enumerate().filter_map(|(j, v)| v.map(|v| ((j + 1) as f64, v))).collect();
        let frac = |x: f64, y: f64| {
            let cat = (x - 0.5) / ncat_slots as f64;
            let reversed = if horizontal { chart.format.y_axis.reversed } else { chart.format.x_axis.reversed };
            (if reversed { 1.0 - cat } else { cat }, sc.frac_raw(y) as f64)
        };
        let px = |(fx, fy): (f64, f64)| {
            if horizontal {
                [plot.x + fy as f32 * plot.w, plot.bottom() - fx as f32 * plot.h]
            } else {
                [plot.x + fx as f32 * plot.w, plot.bottom() - fy as f32 * plot.h]
            }
        };
        draw_trendlines(ctx, s, &pts, &frac, &px, &mut trend_labels, (0.5, ncat_slots as f64 + 0.5));
    }
    draw_trend_labels(ctx, plot, &trend_labels);
}

// ---------------------------------------------------------------- scatter & bubble

fn scatter(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let pts_of = |s: &SeriesData| -> Vec<Option<(f64, f64)>> {
        s.values
            .iter()
            .enumerate()
            .map(|(j, v)| {
                let x = match &s.x {
                    Some(xs) => xs.get(j).copied().flatten(),
                    None => Some((j + 1) as f64),
                };
                match (x, v) {
                    (Some(x), Some(y)) => Some((x, *y)),
                    _ => None,
                }
            })
            .collect()
    };
    let all: Vec<Vec<Option<(f64, f64)>>> = data.series.iter().map(pts_of).collect();
    let Some((mut xlo, mut xhi)) = value_range(all.iter().flatten().flatten().map(|p| p.0)) else { return };
    // Forecasts extend the x axis to the trendline's ends.
    for (s, pts) in data.series.iter().zip(&all) {
        if let Some((lo, hi)) = value_range(pts.iter().flatten().map(|p| p.0)).filter(|_| !s.trendlines.is_empty()) {
            xlo = xlo.min(lo - max_backward(s));
            xhi = xhi.max(hi + max_forward(s));
        }
    }
    let Some((ylo, yhi)) = value_range(all.iter().flatten().flatten().map(|p| p.1)) else { return };
    let xlo =
        if chart.format.x_axis.logarithmic { all.iter().flatten().flatten().map(|p| p.0).filter(|v| *v > 0.0).fold(xhi, f64::min) } else { xlo };
    let ylo =
        if chart.format.y_axis.logarithmic { all.iter().flatten().flatten().map(|p| p.1).filter(|v| *v > 0.0).fold(yhi, f64::min) } else { ylo };
    let xs = configured_scale(xlo, xhi, false, &chart.format.x_axis);
    let ys = configured_scale(ylo, yhi, false, &chart.format.y_axis);
    let fx = Fmt::new(chart.format.x_axis.number_format.as_deref().unwrap_or("General"));
    let fy = Fmt::new(
        chart.format.y_axis.number_format.as_deref().unwrap_or_else(|| data.series.first().map(|s| s.number_format.as_str()).unwrap_or("General")),
    );
    let plot = frame(ctx, area, CatAxis::Numeric(xs, &fx), &ValAxis { scale: ys, fmt: &fy }, None, false, chart);
    if !plot.ok() {
        return;
    }
    let map = |(x, y): (f64, f64)| [plot.x + xs.frac_raw(x) * plot.w, plot.bottom() - ys.frac_raw(y) * plot.h];
    let bubble = chart.kind == ChartKind::Bubble;
    let rmax = plot.w.min(plot.h) * 0.12;
    for (i, (s, pts)) in data.series.iter().zip(&all).enumerate() {
        let style_start = ctx.out.len();
        let px: Vec<Option<[f32; 2]>> = pts.iter().map(|p| p.map(map).filter(|p| p[0].is_finite() && p[1].is_finite())).collect();
        if bubble {
            let sizes = s.sizes.as_deref().unwrap_or(&[]);
            let smax = sizes.iter().flatten().fold(0.0f64, |a, b| a.max(*b));
            let mut bubbles: Vec<([f32; 2], f64)> = px
                .iter()
                .enumerate()
                .filter_map(|(j, p)| {
                    let sz = if sizes.is_empty() { Some(1.0) } else { sizes.get(j).copied().flatten() };
                    match (p, sz) {
                        (Some(p), Some(sz)) if sz > 0.0 && inside_plot(*p, plot) => Some((*p, sz)),
                        _ => None,
                    }
                })
                .take(MAX_MARKERS)
                .collect();
            bubbles.sort_by(|a, b| b.1.total_cmp(&a.1));
            let smax = if sizes.is_empty() { 1.0 } else { smax };
            let fill = [s.color[0], s.color[1], s.color[2], 0xC0];
            for (p, sz) in bubbles {
                let r = ((sz / smax).sqrt() as f32 * rmax).max(1.0);
                ctx.out.push(Prim::Circle { cx: p[0], cy: p[1], r, fill: Some(fill), stroke: Some((WHITE, 0.75)) });
            }
        } else {
            let lines = chart.kind == ChartKind::ScatterLines
                || s.kind == ChartKind::ScatterLines
                || chart.format.series.get(&i).is_some_and(|s| s.line_width.is_some() && !s.no_line);
            if lines {
                draw_clipped_runs(ctx, &px, s.color, plot, chart.format.series.get(&i).and_then(|s| s.smooth).unwrap_or(s.smooth));
            }
            if !lines || px.len() <= 500 {
                draw_markers(ctx, px.iter().flatten().copied().filter(|p| inside_plot(*p, plot)), 3.5, s.color);
            }
        }
        if !bubble {
            style_series(ctx, style_start, chart.format.series.get(&i), s.color);
        }
        if chart.data_labels && pts.len() <= MAX_LABELS {
            let label_format = Fmt::new(&s.number_format);
            for (p, raw) in px.iter().zip(pts) {
                if let (Some(p), Some((_, y))) = (p, raw)
                    && inside_plot(*p, plot)
                {
                    ctx.text(p[0], p[1] - 6.0, &label_format.format(*y, None), LABEL_SIZE, TEXT, HAlign::Center, VAlign::Bottom);
                }
            }
        }
    }
    let mut trend_labels = Vec::new();
    for (s, pts) in data.series.iter().zip(&all) {
        if s.trendlines.is_empty() {
            continue;
        }
        let pts: Vec<(f64, f64)> = pts.iter().flatten().copied().collect();
        let frac = |x: f64, y: f64| (xs.frac_raw(x) as f64, ys.frac_raw(y) as f64);
        let px = |(fx, fy): (f64, f64)| [plot.x + fx as f32 * plot.w, plot.bottom() - fy as f32 * plot.h];
        draw_trendlines(ctx, s, &pts, &frac, &px, &mut trend_labels, (xs.min, xs.max));
    }
    draw_trend_labels(ctx, plot, &trend_labels);
}

// ---------------------------------------------------------------- histogram

fn histogram(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let Some(s) = data.series.first() else { return };
    let vals: Vec<f64> = s.values.iter().flatten().copied().collect();
    let Some((lo, hi)) = value_range(vals.iter().copied()) else { return };
    let n = vals.len();
    let span = hi - lo;
    let mut bins = ((n as f64).sqrt().ceil() as usize).clamp(1, 100);
    if !(span.is_finite() && span > 0.0) {
        bins = 1;
    }
    let width = if bins == 1 && span.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) { 1.0 } else { span / bins as f64 };
    // Upper edge of each bin, exactly as shown in the labels below. Bins are right-inclusive (the first is closed on both sides), so a value
    // equal to an edge belongs to the bin that ends there.
    let edges: Vec<f64> = (0..bins).map(|b| if b + 1 == bins { hi.max(lo + b as f64 * width) } else { lo + (b + 1) as f64 * width }).collect();
    let mut counts = vec![0usize; bins];
    for v in &vals {
        let b = edges.partition_point(|e| e < v).min(bins - 1);
        if let Some(c) = counts.get_mut(b) {
            *c += 1;
        }
    }
    let f = Fmt::new(&s.number_format);
    let labels: Vec<String> = (0..bins)
        .map(|b| {
            let a = lo + b as f64 * width;
            let e = edges.get(b).copied().unwrap_or(a);
            let open = if b == 0 { '[' } else { '(' };
            format!("{open}{}, {}]", f.format(a, Some(width / 10.0)), f.format(e, Some(width / 10.0)))
        })
        .collect();
    let maxc = counts.iter().copied().max().unwrap_or(0) as f64;
    let ys = nice_scale(0.0, maxc, true);
    let fc = Fmt::new("General");
    let plot = frame(ctx, area, CatAxis::Labels(&labels, &[]), &ValAxis { scale: ys, fmt: &fc }, None, false, chart);
    if !plot.ok() {
        return;
    }
    let cw = plot.w / bins as f32;
    for (b, c) in counts.iter().enumerate() {
        let top = plot.bottom() - ys.frac(*c as f64) * plot.h;
        let x = plot.x + b as f32 * cw;
        ctx.rect(x, top, cw, plot.bottom() - top, s.color, Some((WHITE, 0.75)));
        if chart.data_labels {
            ctx.text(x + cw / 2.0, top - 2.0, &c.to_string(), LABEL_SIZE, TEXT, HAlign::Center, VAlign::Bottom);
        }
    }
}

// ---------------------------------------------------------------- waterfall

fn waterfall(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let Some(s) = data.series.first() else { return };
    let mut ncat = data.categories.len().max(s.values.len()).min(MAX_CATS);
    if ncat == 0 {
        return;
    }
    let mut labels: Vec<String> = (0..ncat).map(|j| cat_label(data, j)).collect();
    let mut vals: Vec<f64> = (0..ncat).map(|j| s.values.get(j).copied().flatten().unwrap_or(0.0)).collect();
    let mut is_total = vec![false; ncat];
    let last_total = labels.last().is_some_and(|l| l.to_lowercase().contains("total"));
    if let Some(t) = is_total.last_mut() {
        *t = last_total;
    }
    if !last_total {
        labels.push("Total".into());
        vals.push(0.0);
        is_total.push(true);
        ncat += 1;
    }
    let mut bars = Vec::with_capacity(ncat);
    let mut cum = 0.0f64;
    for (j, v) in vals.iter().enumerate() {
        if is_total.get(j).copied().unwrap_or(false) {
            let total = if last_total && j + 1 == ncat && *v != 0.0 { *v } else { cum };
            bars.push((0.0, total, 0u8));
            cum = total;
        } else {
            let top = clean(cum + v);
            bars.push((cum, top, if *v >= 0.0 { 1 } else { 2 }));
            cum = top;
        }
    }
    let (lo, hi) = value_range(bars.iter().flat_map(|(b, t, _)| [*b, *t])).unwrap_or((0.0, 0.0));
    let ys = nice_scale(lo, hi, true);
    let f = Fmt::new(&s.number_format);
    let plot = frame(ctx, area, CatAxis::Labels(&labels, &[]), &ValAxis { scale: ys, fmt: &f }, None, false, chart);
    if !plot.ok() {
        return;
    }
    let cw = plot.w / ncat as f32;
    let bw = cw / 1.5;
    let vy = |v: f64| plot.bottom() - ys.frac(v) * plot.h;
    for (j, (b, t, kind)) in bars.iter().enumerate() {
        let color = match kind {
            1 => UP,
            2 => DOWN,
            _ => s.color,
        };
        let x = plot.x + (j as f32 + 0.5) * cw - bw / 2.0;
        let (yb, yt) = (vy(*b), vy(*t));
        ctx.rect(x, yb.min(yt), bw, (yb - yt).abs().max(0.5), color, None);
        if j + 1 < bars.len() && ncat <= 500 {
            ctx.out.push(Prim::Line { pts: vec![[x + bw, yt], [x + cw, yt]], color: AXIS, width: 0.75, dash: true });
        }
        if chart.data_labels && ncat <= MAX_LABELS {
            let v = if *kind == 0 { *t } else { t - b };
            ctx.text(x + bw / 2.0, yb.min(yt) - 2.0, &f.format(v, None), LABEL_SIZE, TEXT, HAlign::Center, VAlign::Bottom);
        }
    }
}

// ---------------------------------------------------------------- funnel

fn funnel(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let Some(s) = data.series.first() else { return };
    let n = data.categories.len().max(s.values.len()).min(MAX_CATS);
    if n == 0 {
        return;
    }
    let vals: Vec<f64> = (0..n).map(|j| s.values.get(j).copied().flatten().unwrap_or(0.0).abs()).collect();
    let max = vals.iter().copied().fold(0.0f64, f64::max);
    if max.is_nan() || max <= 0.0 {
        return;
    }
    let labels: Vec<String> = (0..n).map(|j| cat_label(data, j)).collect();
    let lw = (labels.iter().take(2000).map(|t| ctx.tw(t, LABEL_SIZE)).fold(0.0f32, f32::max) + 8.0).min(area.w * 0.35);
    let plot = Bx { x: area.x + lw, w: area.w - lw, ..area };
    if !plot.ok() {
        return;
    }
    let slot = plot.h / n as f32;
    let bh = slot * 0.85;
    let f = Fmt::new(&s.number_format);
    let show = slot >= LABEL_SIZE * 0.9;
    for (j, v) in vals.iter().enumerate() {
        let bw = (*v / max) as f32 * plot.w;
        let x = plot.x + (plot.w - bw) / 2.0;
        let y = plot.y + j as f32 * slot + (slot - bh) / 2.0;
        if bw > 0.0 {
            ctx.rect(x, y, bw, bh, s.color, None);
        }
        if show {
            let yc = y + bh / 2.0;
            if chart.data_labels {
                let t = f.format(s.values.get(j).copied().flatten().unwrap_or(0.0), None);
                let color = if ctx.tw(&t, LABEL_SIZE) + 4.0 <= bw { on(s.color) } else { TEXT };
                ctx.text(plot.cx(), yc, &t, LABEL_SIZE, color, HAlign::Center, VAlign::Middle);
            }
            let l = ctx.fit(labels.get(j).map(String::as_str).unwrap_or(""), ctx.font_size("x_axis", LABEL_SIZE), lw - 8.0);
            ctx.text_role("x_axis", plot.x - 6.0, yc, &l, LABEL_SIZE, TEXT, HAlign::Right, VAlign::Middle);
        }
    }
}

// ---------------------------------------------------------------- pie, doughnut, sunburst

fn pie(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx, inner: f32, names: bool) {
    let Some(s) = data.series.first() else { return };
    let vals: Vec<f64> = s.values.iter().take(MAX_CATS).map(|v| v.map(f64::abs).unwrap_or(0.0)).collect();
    let total: f64 = vals.iter().sum();
    if !(total.is_finite() && total > 0.0) {
        return;
    }
    let (cx, cy) = (area.cx(), area.cy());
    let r = area.w.min(area.h) / 2.0 * 0.92;
    if r < 2.0 {
        return;
    }
    let ri = r * inner;
    let f = Fmt::new(&s.number_format);
    let role = if names { "x_axis" } else { "data_labels" };
    let mut a = 0.0f32;
    let mut labels = Vec::new();
    for (j, v) in vals.iter().enumerate() {
        let sweep = (*v / total) as f32 * TAU;
        if sweep > 1e-4 {
            let fill = point_color(data, j);
            ctx.out.push(Prim::Wedge { cx, cy, r_outer: r, r_inner: ri, a0: a, a1: (a + sweep).min(TAU), fill, stroke: Some((WHITE, 1.5)) });
            if (chart.data_labels || names) && vals.len() <= MAX_LABELS {
                let t = if names { cat_label(data, j) } else { f.format(s.values.get(j).copied().flatten().unwrap_or(0.0), None) };
                let mid = a + sweep / 2.0;
                let rr = if ri > 0.0 { (r + ri) / 2.0 } else { r * 0.65 };
                let room = sweep * rr;
                if ctx.tw(&t, LABEL_SIZE) <= room.max(r - ri) && room >= LABEL_SIZE {
                    labels.push((cx + rr * mid.sin(), cy - rr * mid.cos(), t, on(fill)));
                }
            }
        }
        a += sweep;
    }
    for (x, y, t, c) in labels {
        ctx.text_role(role, x, y, &t, LABEL_SIZE, c, HAlign::Center, VAlign::Middle);
    }
}

// ---------------------------------------------------------------- treemap

fn worst(row: &[f64], side: f64) -> f64 {
    let s: f64 = row.iter().sum();
    if s <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }
    let (mn, mx) = row.iter().fold((f64::INFINITY, 0.0f64), |(a, b), v| (a.min(*v), b.max(*v)));
    let s2 = s * s;
    let w2 = side * side;
    (w2 * mx / s2).max(s2 / (w2 * mn))
}

/// Squarified treemap layout of (index, area) items, sorted descending, into `r`.
fn squarify(items: &[(usize, f64)], r: Bx) -> Vec<(usize, Bx)> {
    let total: f64 = items.iter().map(|i| i.1).sum();
    let area = (r.w as f64) * (r.h as f64);
    if !(total > 0.0 && area > 0.0) {
        return vec![];
    }
    let scale = area / total;
    let mut out = Vec::with_capacity(items.len());
    let mut rem = r;
    let mut row: Vec<(usize, f64)> = Vec::new();
    let mut i = 0;
    let layout = |row: &[(usize, f64)], rem: &mut Bx, out: &mut Vec<(usize, Bx)>| {
        let s: f64 = row.iter().map(|x| x.1).sum();
        if s <= 0.0 {
            return;
        }
        if rem.w >= rem.h {
            let cw = (s / rem.h.max(1e-6) as f64) as f32;
            let mut y = rem.y;
            for (j, a) in row {
                let h = (*a / cw.max(1e-6) as f64) as f32;
                out.push((*j, Bx { x: rem.x, y, w: cw, h }));
                y += h;
            }
            rem.x += cw;
            rem.w = (rem.w - cw).max(0.0);
        } else {
            let rh = (s / rem.w.max(1e-6) as f64) as f32;
            let mut x = rem.x;
            for (j, a) in row {
                let w = (*a / rh.max(1e-6) as f64) as f32;
                out.push((*j, Bx { x, y: rem.y, w, h: rh }));
                x += w;
            }
            rem.y += rh;
            rem.h = (rem.h - rh).max(0.0);
        }
    };
    while let Some(&(j, v)) = items.get(i) {
        let a = v * scale;
        let side = rem.w.min(rem.h) as f64;
        let cur: Vec<f64> = row.iter().map(|x| x.1).collect();
        let mut with = cur.clone();
        with.push(a);
        if row.is_empty() || worst(&with, side) <= worst(&cur, side) {
            row.push((j, a));
            i += 1;
        } else {
            layout(&row, &mut rem, &mut out);
            row.clear();
        }
    }
    if !row.is_empty() {
        layout(&row, &mut rem, &mut out);
    }
    out
}

fn treemap(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let Some(s) = data.series.first() else { return };
    let mut items: Vec<(usize, f64)> =
        s.values.iter().take(MAX_MARKERS).enumerate().filter_map(|(j, v)| v.filter(|v| *v > 0.0).map(|v| (j, v))).collect();
    items.sort_by(|a, b| b.1.total_cmp(&a.1));
    let f = Fmt::new(&s.number_format);
    for (j, b) in squarify(&items, area) {
        let fill = point_color(data, j);
        ctx.rect(b.x, b.y, b.w, b.h, fill, Some((WHITE, 1.5)));
        if b.w > 24.0 && b.h > LABEL_SIZE * 1.6 {
            let name = ctx.fit(&cat_label(data, j), ctx.font_size("x_axis", LABEL_SIZE), b.w - 8.0);
            ctx.text_role("x_axis", b.x + 4.0, b.y + 4.0, &name, LABEL_SIZE, on(fill), HAlign::Left, VAlign::Top);
            if chart.data_labels && b.h > LABEL_SIZE * 3.2 {
                let v = items.iter().find(|x| x.0 == j).map(|x| x.1).unwrap_or(0.0);
                let t = ctx.fit(&f.format(v, None), LABEL_SIZE, b.w - 8.0);
                ctx.text(b.x + 4.0, b.y + 4.0 + LABEL_SIZE * 1.4, &t, LABEL_SIZE, on(fill), HAlign::Left, VAlign::Top);
            }
        }
    }
}

// ---------------------------------------------------------------- radar

fn radar(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let ncat = data.categories.len().max(data.series.iter().map(|s| s.values.len()).max().unwrap_or(0)).min(1000);
    if ncat == 0 {
        return;
    }
    let labels: Vec<String> = (0..ncat).map(|j| cat_label(data, j)).collect();
    let lw = labels.iter().map(|t| ctx.tw(t, LABEL_SIZE)).fold(0.0f32, f32::max).min(area.w * 0.2);
    let r = ((area.w - 2.0 * (lw + 8.0)).min(area.h - 2.0 * (LABEL_SIZE * 1.4 + 6.0)) / 2.0).max(0.0);
    if r < 5.0 {
        return;
    }
    let (cx, cy) = (area.cx(), area.cy());
    let (lo, hi) = value_range(data.series.iter().flat_map(|s| s.values.iter().flatten().copied())).unwrap_or((0.0, 0.0));
    let sc = nice_scale(lo, hi, true);
    let f = Fmt::new(data.series.first().map(|s| s.number_format.as_str()).unwrap_or("General"));
    let ang = |j: usize| j as f32 / ncat as f32 * TAU;
    let pt = |a: f32, rr: f32| [cx + rr * a.sin(), cy - rr * a.cos()];
    let ticks = sc.ticks();
    let spacing = r / ticks.len().saturating_sub(1).max(1) as f32;
    let every = ((LABEL_SIZE * 1.3 / spacing.max(1e-3)).ceil() as usize).max(1);
    for (ti, t) in ticks.into_iter().enumerate() {
        let rr = sc.frac(t) * r;
        if rr > 0.5 && chart.gridlines {
            let mut ring: Vec<[f32; 2]> = (0..ncat).map(|j| pt(ang(j), rr)).collect();
            if ncat < 3 {
                ring = (0..=48).map(|k| pt(k as f32 / 48.0 * TAU, rr)).collect();
            } else if let Some(p) = ring.first().copied() {
                ring.push(p);
            }
            ctx.line(ring, GRID, 0.75);
        }
        if ti % every == 0 {
            ctx.text_role("y_axis", cx - 3.0, cy - rr, &f.format(t, Some(sc.step)), LABEL_SIZE * 0.9, TEXT, HAlign::Right, VAlign::Middle);
        }
    }
    let step = ncat.div_ceil(72).max(1);
    for j in (0..ncat).step_by(step) {
        let a = ang(j);
        ctx.line(vec![[cx, cy], pt(a, r)], GRID, 0.75);
        let p = pt(a, r + 6.0);
        let (sa, ca) = (a.sin(), a.cos());
        let align = if sa > 0.1 {
            HAlign::Left
        } else if sa < -0.1 {
            HAlign::Right
        } else {
            HAlign::Center
        };
        let valign = if ca > 0.1 {
            VAlign::Bottom
        } else if ca < -0.1 {
            VAlign::Top
        } else {
            VAlign::Middle
        };
        let t = ctx.fit(labels.get(j).map(String::as_str).unwrap_or(""), ctx.font_size("x_axis", LABEL_SIZE), lw.max(20.0));
        ctx.text_role("x_axis", p[0], p[1], &t, LABEL_SIZE, TEXT, align, valign);
    }
    for s in &data.series {
        let mut pts: Vec<[f32; 2]> = (0..ncat).map(|j| pt(ang(j), sc.frac(s.values.get(j).copied().flatten().unwrap_or(sc.min)) * r)).collect();
        if let Some(p) = pts.first().copied() {
            pts.push(p);
        }
        ctx.line(pts, s.color, 2.0);
    }
}

// ---------------------------------------------------------------- box & whisker

fn quantile(sorted: &[f64], q: f64, method: QuartileMethod) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    // Clamp exclusive ranks for short samples so one or two points remain drawable.
    let pos = match method {
        QuartileMethod::Inclusive => q * (n - 1) as f64,
        QuartileMethod::Exclusive => (q * (n as f64 + 1.0) - 1.0).clamp(0.0, (n - 1) as f64),
    };
    let i = pos.floor() as usize;
    let frac = pos - i as f64;
    let a = sorted.get(i).copied().unwrap_or(0.0);
    let b = sorted.get(i + 1).copied().unwrap_or(a);
    a + (b - a) * frac
}

fn box_whisker(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let labels: Vec<String> = data.series.iter().map(|s| s.name.clone()).collect();
    let Some((lo, hi)) = value_range(data.series.iter().flat_map(|s| s.values.iter().flatten().copied())) else { return };
    let ys = nice_scale(lo, hi, false);
    let f = Fmt::new(data.series.first().map(|s| s.number_format.as_str()).unwrap_or("General"));
    let plot = frame(ctx, area, CatAxis::Labels(&labels, &[]), &ValAxis { scale: ys, fmt: &f }, None, false, chart);
    if !plot.ok() {
        return;
    }
    let n = data.series.len().max(1);
    let cw = plot.w / n as f32;
    let bw = (cw * 0.4).min(80.0);
    let vy = |v: f64| plot.bottom() - ys.frac(v) * plot.h;
    for (i, s) in data.series.iter().enumerate() {
        let mut v: Vec<f64> = s.values.iter().flatten().copied().collect();
        if v.is_empty() {
            continue;
        }
        v.sort_by(f64::total_cmp);
        let method = chart.series.get(i).map(|s| s.quartiles).unwrap_or_default();
        let (q1, med, q3) = (quantile(&v, 0.25, method), quantile(&v, 0.5, method), quantile(&v, 0.75, method));
        let iqr = q3 - q1;
        let (flo, fhi) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
        let wlo = v.iter().copied().find(|x| *x >= flo).unwrap_or(q1);
        let whi = v.iter().rev().copied().find(|x| *x <= fhi).unwrap_or(q3);
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let c = plot.x + (i as f32 + 0.5) * cw;
        ctx.vline(c, vy(whi), vy(q3), s.color, 1.25);
        ctx.vline(c, vy(q1), vy(wlo), s.color, 1.25);
        ctx.hline(c - bw / 4.0, c + bw / 4.0, vy(whi), s.color, 1.25);
        ctx.hline(c - bw / 4.0, c + bw / 4.0, vy(wlo), s.color, 1.25);
        let (yt, yb) = (vy(q3), vy(q1));
        ctx.rect(c - bw / 2.0, yt, bw, (yb - yt).max(0.5), s.color, None);
        ctx.hline(c - bw / 2.0, c + bw / 2.0, vy(med), WHITE, 1.5);
        let (mx, my, d) = (c, vy(mean), 3.0);
        ctx.line(vec![[mx - d, my - d], [mx + d, my + d]], WHITE, 1.25);
        ctx.line(vec![[mx - d, my + d], [mx + d, my - d]], WHITE, 1.25);
        for o in v.iter().filter(|x| **x < flo || **x > fhi).take(MAX_MARKERS) {
            ctx.out.push(Prim::Circle { cx: c, cy: vy(*o), r: 2.5, fill: None, stroke: Some((s.color, 1.0)) });
        }
    }
}

// ---------------------------------------------------------------- stock

fn stock(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let ncat = data.categories.len().max(data.series.iter().map(|s| s.values.len()).max().unwrap_or(0)).min(MAX_CATS);
    if ncat == 0 {
        return;
    }
    let labels: Vec<String> = (0..ncat).map(|j| cat_label(data, j)).collect();
    let Some((lo, hi)) = value_range(data.series.iter().flat_map(|s| s.values.iter().flatten().copied())) else { return };
    let ys = nice_scale(lo, hi, false);
    let f = Fmt::new(data.series.first().map(|s| s.number_format.as_str()).unwrap_or("General"));
    let plot = frame(ctx, area, CatAxis::Labels(&labels, &[]), &ValAxis { scale: ys, fmt: &f }, None, false, chart);
    if !plot.ok() {
        return;
    }
    let ser = |k: usize| data.series.get(k);
    let (open, high, low, close) = match data.series.len() {
        0 => return,
        1 => (None, None, None, ser(0)),
        2 => (None, ser(0), ser(1), None),
        3 => (None, ser(0), ser(1), ser(2)),
        _ => (ser(0), ser(1), ser(2), ser(3)),
    };
    let get = |s: Option<&SeriesData>, j: usize| s.and_then(|s| s.values.get(j).copied().flatten());
    let cw = plot.w / ncat as f32;
    let tick = (cw * 0.25).max(1.0);
    let vy = |v: f64| plot.bottom() - ys.frac(v) * plot.h;
    for j in 0..ncat {
        let c = plot.x + (j as f32 + 0.5) * cw;
        if let (Some(h), Some(l)) = (get(high, j), get(low, j)) {
            ctx.vline(c, vy(h), vy(l), STOCK, 1.25);
        }
        match (get(open, j), get(close, j)) {
            (Some(o), Some(cl)) => {
                let (yo, yc) = (vy(o), vy(cl));
                let bw = (cw * 0.5).max(1.0);
                let fill = if cl >= o { WHITE } else { STOCK };
                ctx.rect(c - bw / 2.0, yo.min(yc), bw, (yo - yc).abs().max(0.75), fill, Some((STOCK, 1.0)));
            }
            (None, Some(cl)) => {
                if high.is_some() {
                    ctx.hline(c, c + tick, vy(cl), STOCK, 1.25);
                } else {
                    ctx.out.push(Prim::Circle { cx: c, cy: vy(cl), r: 2.5, fill: Some(STOCK), stroke: None });
                }
            }
            _ => {}
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
            Prim::Text { x, y, text, size, color, bold, align, valign, rotation, role, format } => {
                if !(fin(x) && fin(y) && fin(size) && fin(rotation)) || text.is_empty() {
                    return None;
                }
                Some(Prim::Text { x: cx(x), y: cy(y), text, size: size.clamp(1.0, 200.0), color, bold, align, valign, rotation, role, format })
            }
        })
        .collect()
}

#[cfg(test)]
mod smooth_tests {
    use super::*;

    #[test]
    fn long_runs_are_smoothed_uniformly_within_the_cap() {
        let plot = Bx { x: 0.0, y: 0.0, w: 3000.0, h: 100.0 };
        let pts: Vec<[f32; 2]> = (0..1500).map(|i| [i as f32 * 2.0, if i % 2 == 0 { 20.0 } else { 80.0 }]).collect();
        let out = smooth_run(pts, plot);
        assert!(out.len() <= MAX_LINE_PTS * 2 + 1, "{}", out.len());
        // Two sub-segments per segment all the way to the end, not 100 each and then a straight tail.
        assert_eq!(out.len(), 1499 * 2 + 1);
        assert_eq!(out.last().copied(), Some([2998.0, 80.0]));
    }

    #[test]
    fn spline_overshoot_is_clamped_to_the_plot() {
        let plot = Bx { x: 0.0, y: 10.0, w: 300.0, h: 100.0 };
        // A step right at the plot edges: Catmull-Rom overshoots past both.
        let pts = vec![[0.0, 110.0], [100.0, 110.0], [110.0, 10.0], [200.0, 10.0], [300.0, 10.0]];
        let out = smooth_run(pts, plot);
        assert!(out.len() > 5);
        assert!(out.iter().all(|p| (0.0..=300.0).contains(&p[0]) && (10.0..=110.0).contains(&p[1])));
    }
}

#[cfg(test)]
mod quartile_tests {
    use super::*;

    #[test]
    fn box_whisker_default_quartiles_are_exclusive() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert_eq!(quantile(&values, 0.25, QuartileMethod::default()), 1.75);
        assert_eq!(quantile(&values, 0.5, QuartileMethod::default()), 3.5);
        assert_eq!(quantile(&values, 0.75, QuartileMethod::default()), 5.25);
    }

    #[test]
    fn quartile_methods_and_short_samples() {
        for (values, exclusive, inclusive) in [
            (vec![1., 2., 3.], (1., 3.), (1.5, 2.5)),
            (vec![1., 2., 3., 4.], (1.25, 3.75), (1.75, 3.25)),
            (vec![1., 2., 3., 4., 5.], (1.5, 4.5), (2., 4.)),
            (vec![1., 2., 3., 4., 5., 6., 7.], (2., 6.), (2.5, 5.5)),
            (vec![1., 1., 2., 3., 5., 8.], (1., 5.75), (1.25, 4.5)),
            (vec![1., 2.], (1., 2.), (1.25, 1.75)),
            (vec![1.], (1., 1.), (1., 1.)),
            (vec![], (0., 0.), (0., 0.)),
        ] {
            for (method, expected) in [(QuartileMethod::Exclusive, exclusive), (QuartileMethod::Inclusive, inclusive)] {
                assert_eq!((quantile(&values, 0.25, method), quantile(&values, 0.75, method)), expected);
            }
        }
    }
}
