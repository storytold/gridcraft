use super::frame::{Bx, CatAxis, ValAxis, frame, on, value_range};
use super::legend::cat_label;
use super::lines::{decimate_area, draw_clipped_runs, draw_markers, draw_plot_markers, draw_runs};
use super::trend::{draw_trend_labels, draw_trendlines, max_backward, max_forward, max_forward_slots, supports_trendline};
use super::{
    Chart, ChartData, ChartKind, Ctx, Fmt, HAlign, LABEL_SIZE, MAX_CATS, MAX_LABELS, MAX_LINE_PTS, MAX_MARKERS, MIN_BAR_PX, Prim, Rgba, Scale,
    SeriesData, TEXT, VAlign, WHITE, clean, nice_scale,
};
use crate::axis::{MAX_ABS, chart_scale};
use gridcraft_model::{ChartAxis, SeriesAxes};

// ---------------------------------------------------------------- cartesian (column/bar/line/area/combo)

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fam {
    Col,
    ColStack,
    Col100,
    Line,
    LineStack,
    Line100,
    Area,
    AreaStack,
    Area100,
}

/// Stack accumulator: (axis, family) key with positive and negative running sums.
type Stack = ((bool, Fam), Vec<f64>, Vec<f64>);

fn family(k: ChartKind) -> Fam {
    use ChartKind::*;
    match k {
        ColumnStacked | BarStacked => Fam::ColStack,
        ColumnStacked100 | BarStacked100 => Fam::Col100,
        Line | LineMarkers | Scatter | ScatterLines | Radar | Stock => Fam::Line,
        LineStacked => Fam::LineStack,
        LineStacked100 | LineStacked100Markers => Fam::Line100,
        Area => Fam::Area,
        AreaStacked => Fam::AreaStack,
        AreaStacked100 => Fam::Area100,
        _ => Fam::Col,
    }
}

/// Families stacked to 100%: each category's values are shares of its total.
fn is_100(f: Fam) -> bool {
    matches!(f, Fam::Col100 | Fam::Line100 | Fam::Area100)
}

fn is_col(f: Fam) -> bool {
    matches!(f, Fam::Col | Fam::ColStack | Fam::Col100)
}

fn is_line(f: Fam) -> bool {
    matches!(f, Fam::Line | Fam::LineStack | Fam::Line100)
}

/// Line kinds that draw a marker at each point.
fn has_markers(k: ChartKind) -> bool {
    matches!(k, ChartKind::LineMarkers | ChartKind::LineStacked100Markers)
}

/// Stroke widths for explicitly selected Excel chart styles.
fn line_style_width(style: u32) -> f32 {
    match style {
        1 => 3.0,
        9 => 5.0,
        _ => 2.25,
    }
}

/// Bars that fall into one pixel column, merged: extent along the category axis (`a0..=a1`, the
/// bars' start positions) and along the value axis (`lo..hi`).
struct BarRun {
    bucket: f32,
    a0: f32,
    a1: f32,
    lo: f32,
    hi: f32,
    /// More than one bar was merged.
    merged: bool,
}

impl BarRun {
    /// Starts a single-bar run at `pos` spanning `lo..hi`.
    fn new(pos: f32, lo: f32, hi: f32) -> Self {
        BarRun { bucket: pos.floor(), a0: pos, a1: pos, lo, hi, merged: false }
    }
    /// Whether `lo..hi` touches the run's value span; a disjoint bar would be joined by a filled gap.
    fn touches(&self, lo: f32, hi: f32) -> bool {
        lo <= self.hi && hi >= self.lo
    }
    /// Extends the merged run to cover `pos` and `lo..hi`.
    fn add(&mut self, pos: f32, lo: f32, hi: f32) {
        self.merged = true;
        self.a0 = self.a0.min(pos);
        self.a1 = self.a1.max(pos);
        self.lo = self.lo.min(lo);
        self.hi = self.hi.max(hi);
    }
    /// `band` is the part of the pixel column reserved for this series (`start`, `len`): in a
    /// clustered chart a merged run stays inside it, so the other series keep a visible strip.
    fn draw(&self, ctx: &mut Ctx, sink: &BarSink, band: Option<(f32, f32)>) {
        let (a0, along) = match band {
            Some((start, len)) if self.merged => (self.bucket + start, len),
            _ => (self.a0, self.a1 - self.a0 + sink.bw),
        };
        sink.draw_rect(ctx, a0, along, self.lo, self.hi, None);
    }
}

/// Whether bars of width `bw` are merged per pixel column (never when they carry data labels).
fn merge_hairlines(bw: f32, labels_on: bool) -> bool {
    bw < MIN_BAR_PX && !labels_on
}

/// Draws the bars of one series: a rectangle each, or, for hairline bars, one merged [`BarRun`]
/// per pixel column.
struct BarSink {
    run: Option<BarRun>,
    plot: Bx,
    bw: f32,
    horizontal: bool,
    color: Rgba,
    stroke: Option<(Rgba, f32)>,
    nslots: f32,
    k: f32,
}

impl BarSink {
    /// Creates a bar emitter for series slot `k` of `nslots`.
    fn new(plot: Bx, bw: f32, horizontal: bool, color: Rgba, stroke: Option<(Rgba, f32)>, nslots: f32, k: f32) -> Self {
        BarSink { run: None, plot, bw, horizontal, color, stroke, nslots, k }
    }
    fn draw_rect(&self, ctx: &mut Ctx, pos: f32, along: f32, lo: f32, hi: f32, stroke: Option<(Rgba, f32)>) {
        let (x0, y0, x1, y1) = if self.horizontal { (lo, pos, hi, pos + along) } else { (pos, lo, pos + along, hi) };
        let x = x0.max(self.plot.x);
        let y = y0.max(self.plot.y);
        let w = x1.min(self.plot.right()) - x;
        let h = y1.min(self.plot.bottom()) - y;
        if w > 0.0 && h > 0.0 {
            // Omit a narrow filled bar's outline so its fill survives; unfilled bars need their outlines.
            let stroke = stroke.filter(|(_, sw)| self.color[3] == 0 || (w > *sw && h > *sw));
            ctx.rect(x, y, w, h, self.color, stroke);
        }
    }
    /// Start of this series' slot along the category axis, for the category centered at `c`.
    fn pos(&self, c: f32) -> f32 {
        if self.horizontal { c + self.nslots * self.bw / 2.0 - (self.k + 1.0) * self.bw } else { c - self.nslots * self.bw / 2.0 + self.k * self.bw }
    }
    /// This series' strip of a pixel column (offset, length) when the columns hold several
    /// clustered series, in the direction the slots run.
    fn band(&self) -> Option<(f32, f32)> {
        (self.nslots > 1.0).then(|| {
            let idx = if self.horizontal { self.nslots - 1.0 - self.k } else { self.k };
            (idx / self.nslots, 1.0 / self.nslots)
        })
    }
    /// Draws the bar at category center `c` spanning `lo..lo + len` on the value axis, or, when
    /// `merge` is set, adds it to the current run.
    fn bar(&mut self, ctx: &mut Ctx, merge: bool, c: f32, lo: f32, len: f32) {
        let pos = self.pos(c);
        let band = self.band();
        if !merge {
            self.draw_rect(ctx, pos, self.bw, lo, lo + len, self.stroke);
            return;
        }
        match &mut self.run {
            Some(m) if m.bucket == pos.floor() && m.touches(lo, lo + len) => m.add(pos, lo, lo + len),
            run => {
                if let Some(m) = run.replace(BarRun::new(pos, lo, lo + len)) {
                    m.draw(ctx, self, band);
                }
            }
        }
    }
    /// Flushes any pending hairline bar run at the end of the series.
    fn finish(self, ctx: &mut Ctx) {
        let band = self.band();
        if let Some(m) = &self.run {
            m.draw(ctx, &self, band);
        }
    }
}

fn series_spans(data: &ChartData, fams: &[Fam], sec: &[bool], ncat: usize) -> Vec<Vec<Option<(f64, f64)>>> {
    // Totals for 100% stacking, per axis and family (100% columns, lines and areas stack apart).
    let mut totals: Vec<((bool, Fam), Vec<f64>)> = Vec::new();
    for (i, s) in data.series.iter().enumerate() {
        let fam = fams.get(i).copied().unwrap_or(Fam::Col);
        if is_100(fam) {
            let key = (sec.get(i).copied().unwrap_or(false), fam);
            let p = match totals.iter().position(|(k, _)| *k == key) {
                Some(p) => p,
                None => {
                    totals.push((key, vec![0.0; ncat]));
                    totals.len() - 1
                }
            };
            for (j, v) in s.values.iter().take(ncat).enumerate() {
                if let (Some(v), Some(t)) = (v, totals.get_mut(p).and_then(|(_, t)| t.get_mut(j))) {
                    *t += v.abs();
                }
            }
        }
    }
    // A value's share of its category's total, for the 100% families.
    let share = |key: (bool, Fam), j: usize, v: f64| {
        let t = totals.iter().find(|(k, _)| *k == key).and_then(|(_, t)| t.get(j)).copied().unwrap_or(0.0);
        if t > 0.0 { v / t } else { 0.0 }
    };
    // Stack accumulators keyed by (axis, family): positive and negative running sums.
    let mut stacks: Vec<Stack> = Vec::new();
    let mut spans: Vec<Vec<Option<(f64, f64)>>> = Vec::with_capacity(data.series.len());
    for (i, s) in data.series.iter().enumerate() {
        let fam = fams.get(i).copied().unwrap_or(Fam::Col);
        let a = sec.get(i).copied().unwrap_or(false);
        let stacked = matches!(fam, Fam::ColStack | Fam::Col100 | Fam::LineStack | Fam::Line100 | Fam::AreaStack | Fam::Area100);
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
        // A series without values has no spans: 255 empty series over 20,000 categories would
        // otherwise fill ~5 million entries.
        let n = if s.values.is_empty() { 0 } else { ncat };
        let mut row = Vec::with_capacity(n);
        for j in 0..n {
            let v = s.values.get(j).copied().flatten();
            let span = match (si.and_then(|p| stacks.get_mut(p)), fam) {
                (Some((_, pos, neg)), Fam::ColStack | Fam::Col100) => v.and_then(|v| {
                    let v = if fam == Fam::Col100 { share(key, j, v) } else { v };
                    let acc = if v >= 0.0 { pos.get_mut(j) } else { neg.get_mut(j) }?;
                    let base = *acc;
                    *acc += v;
                    Some((base, *acc))
                }),
                (Some((_, pos, _)), _) => {
                    let v = v.unwrap_or(0.0);
                    let v = if is_100(fam) { share(key, j, v) } else { v };
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

    spans
}

struct AxisScale<'a> {
    secondary: bool,
    scale: Scale,
    fmt: Fmt,
    spec: Option<&'a SeriesAxes>,
}

impl AxisScale<'_> {
    fn value_reversed(&self, horizontal: bool) -> bool {
        horizontal && self.spec.is_some_and(|a| a.x.reversed)
    }

    fn categories_reversed(&self, horizontal: bool) -> bool {
        horizontal && self.spec.is_some_and(|a| a.y.reversed)
    }

    fn frame_axis(&self, horizontal: bool) -> ValAxis<'_> {
        ValAxis { scale: self.scale, reversed: self.value_reversed(horizontal), fmt: &self.fmt }
    }
}

fn axis_scales<'a>(
    chart: &'a Chart,
    data: &ChartData,
    fams: &[Fam],
    sec: &[bool],
    spans: &[Vec<Option<(f64, f64)>>],
    horizontal: bool,
) -> Vec<AxisScale<'a>> {
    // Axis scales.
    let mut axes = Vec::new();
    for axis in [false, true] {
        let members: Vec<usize> = (0..data.series.len())
            .filter(|i| sec.get(*i).copied().unwrap_or(false) == axis && !(horizontal && data.series.get(*i).is_some_and(|s| is_xy(s.kind))))
            .collect();
        if members.is_empty() {
            continue;
        }
        let force_zero = members.iter().any(|i| fams.get(*i).is_some_and(|f| !is_line(*f)));
        let all100 = members.iter().all(|i| fams.get(*i).is_some_and(|f| is_100(*f)));
        let range = value_range(members.iter().flat_map(|i| {
            let line = fams.get(*i).is_some_and(|f| is_line(*f));
            // Lines contribute only their top value, bars both ends (no per-point `Vec`).
            spans.get(*i).into_iter().flatten().flatten().flat_map(move |(b, t)| [*b, *t].into_iter().skip(usize::from(line)))
        }));
        let (lo, hi) = range.unwrap_or((0.0, 0.0));
        let spec = members.iter().find_map(|i| chart.series.get(*i).and_then(|s| s.axes.as_ref()));
        let (scale, fmt) = if all100 {
            (Scale { min: if lo < -1e-9 { -1.0 } else { 0.0 }, max: if hi > 1e-9 || lo >= -1e-9 { 1.0 } else { 0.0 }, step: 0.2 }, Fmt::new("0%"))
        } else {
            let code = members.first().and_then(|i| data.series.get(*i)).map(|s| s.number_format.as_str()).unwrap_or("General");
            let value_axis = spec.map(|a| if horizontal { &a.x } else { &a.y });
            (chart_scale(lo, hi, force_zero, value_axis), Fmt::new(code))
        };
        axes.push(AxisScale { secondary: axis, scale, fmt, spec });
        if !axis && sec.iter().all(|s| !s) {
            break;
        }
    }
    axes
}

fn column_slots(fams: &[Fam], sec: &[bool]) -> (Vec<usize>, f32) {
    // Column slots: each clustered series gets one, each stack group shares one.
    let mut slot_keys: Vec<(bool, Fam, usize)> = Vec::new();
    let mut slot_of = vec![0usize; fams.len()];
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
    (slot_of, slot_keys.len().max(1) as f32)
}

struct OverlayAxes<'a> {
    spec: Option<&'a SeriesAxes>,
    x: Scale,
    y: Scale,
}

fn numeric_points(s: &SeriesData) -> impl Iterator<Item = Option<(f64, f64)>> + '_ {
    s.values.iter().enumerate().map(|(j, y)| {
        let x = s.x.as_ref().map_or(Some((j + 1) as f64), |xs| xs.get(j).copied().flatten())?;
        Some((x, (*y)?))
    })
}

fn is_xy(kind: ChartKind) -> bool {
    matches!(kind, ChartKind::Scatter | ChartKind::ScatterLines)
}

fn overlay_x_scale<'a>(default: Scale, spec: Option<&ChartAxis>, members: impl Iterator<Item = &'a SeriesData>) -> Scale {
    let Some(spec) = spec else { return default };
    let min = spec.min.filter(|n| n.abs() <= MAX_ABS);
    let max = spec.max.filter(|n| n.abs() <= MAX_ABS);
    let bound = match (min, max) {
        (Some(bound), None) | (None, Some(bound)) => bound,
        _ => return default.with_bounds(Some(spec)),
    };
    let axis = ChartAxis { min, max, ..*spec };
    let (lo, hi) = value_range(members.flat_map(|s| numeric_points(s).flatten().map(|p| p.0))).unwrap_or((default.min, default.max));
    if (min.is_some() && hi > bound) || (max.is_some() && lo < bound) {
        let scale = chart_scale(lo, hi, false, Some(&axis));
        if (min.is_some() && scale.min == bound) || (max.is_some() && scale.max == bound) {
            return scale;
        }
    }
    // No point can determine the opposite endpoint. Use the bar span, anchored at the saved bound.
    default.anchored_at(bound, min.is_some())
}

impl OverlayAxes<'_> {
    fn fractions(&self, x: f64, y: f64) -> (f64, f64) {
        let fx = (x - self.x.min) / (self.x.max - self.x.min);
        let fy = (y - self.y.min) / (self.y.max - self.y.min);
        (if self.spec.is_some_and(|a| a.x.reversed) { 1.0 - fx } else { fx }, if self.spec.is_some_and(|a| a.y.reversed) { 1.0 - fy } else { fy })
    }
}

fn xy_pixel(plot: Bx, (fx, fy): (f64, f64)) -> [f32; 2] {
    [plot.x + fx as f32 * plot.w, plot.y + (1.0 - fy) as f32 * plot.h]
}

fn xy_overlay(ctx: &mut Ctx, s: &SeriesData, axes: &OverlayAxes<'_>, plot: Bx, chart: &Chart) {
    let pts: Vec<_> = numeric_points(s).map(|p| p.map(|(x, y)| axes.fractions(x, y))).collect();
    if s.kind == ChartKind::ScatterLines {
        draw_clipped_runs(ctx, &pts, s.color, line_style_width(chart.style), s.smooth, |p| xy_pixel(plot, p));
    }
    let labels = chart.data_labels && pts.len() <= MAX_LABELS;
    if s.kind == ChartKind::Scatter || labels {
        let visible: Vec<_> =
            pts.into_iter().map(|p| p.filter(|(x, y)| (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y)).map(|p| xy_pixel(plot, p))).collect();
        if s.kind == ChartKind::Scatter {
            draw_plot_markers(ctx, visible.iter().flatten().copied(), 3.5, s.color, plot);
        }
        if labels {
            draw_point_labels(ctx, s, &visible);
        }
    }
}

fn draw_line_series(ctx: &mut Ctx, s: &SeriesData, pts: &[Option<[f32; 2]>], plot: Bx, style: u32, markers: bool, labels: bool) {
    draw_runs(ctx, pts, s.color, line_style_width(style), s.smooth.then_some(plot));
    if markers {
        draw_markers(ctx, pts.iter().flatten().copied(), 3.5, s.color);
    }
    if labels {
        draw_point_labels(ctx, s, pts);
    }
}

fn draw_point_labels(ctx: &mut Ctx, s: &SeriesData, pts: &[Option<[f32; 2]>]) {
    let fmt = Fmt::new(&s.number_format);
    for (p, v) in pts.iter().zip(&s.values) {
        if let (Some(p), Some(v)) = (p, v) {
            ctx.text(p[0], p[1] - 5.0, &fmt.format(*v, None), LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Bottom));
        }
    }
}

fn reverse_categories(labels: &mut [String], outer: &mut [Vec<String>]) {
    labels.reverse();
    for level in outer {
        let mut flipped = vec![String::new(); labels.len()];
        for (first, end) in super::frame::level_groups(level, labels.len()) {
            if let (Some(name), Some(slot)) = (level.get(first), flipped.get_mut(labels.len() - end)) {
                *slot = name.clone();
            }
        }
        *level = flipped;
    }
}

/// Renders Cartesian column, bar, line, area, and combination charts.
///
/// Resolves series families and their axes, calculates stacking and
/// category positions, and draws the corresponding primitives.
/// Explicit chart style 9 adds white outlines to column and bar
/// rectangles, including stacked and 100%-stacked variants.
pub(super) fn cartesian(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx, horizontal: bool) {
    let xy_combo = chart.horizontal_xy_combo();
    let ncat = data
        .categories
        .len()
        .max(data.series.iter().filter(|s| !xy_combo || !is_xy(s.kind)).map(|s| s.values.len()).max().unwrap_or(0))
        .min(MAX_CATS);
    if ncat == 0 {
        return;
    }
    let kinds: Vec<ChartKind> = data.series.iter().map(|s| if horizontal && !xy_combo { chart.kind } else { s.kind }).collect();
    let fams: Vec<Fam> = kinds.iter().map(|k| family(*k)).collect();
    let any_primary = data.series.iter().any(|s| !s.secondary);
    let sec: Vec<bool> = data.series.iter().map(|s| s.secondary && any_primary && (!horizontal || xy_combo)).collect();

    let spans = series_spans(data, &fams, &sec, ncat);

    let axes = axis_scales(chart, data, &fams, &sec, &spans, horizontal);
    let Some(first_axis) = axes.first() else { return };
    let axis_for = |secondary| axes.iter().find(|a| a.secondary == secondary).unwrap_or(first_axis);
    let y1 = first_axis.frame_axis(horizontal);
    let y2 = axes.get(1).map(|a| a.frame_axis(horizontal));
    // A forward forecast on a category axis adds empty category slots after the data.
    let fwd = data
        .series
        .iter()
        .enumerate()
        .filter(|(i, s)| !(xy_combo && is_xy(s.kind)) && kinds.get(*i).is_some_and(|k| supports_trendline(*k)))
        .map(|(_, s)| max_forward(s))
        .fold(0.0, f64::max);
    let ncat_slots = ncat.saturating_add(fwd.min(max_forward_slots(ncat)).ceil() as usize).min(MAX_CATS.max(ncat));
    let mut labels: Vec<String> = (0..ncat_slots).map(|j| if j < ncat { cat_label(data, j) } else { String::new() }).collect();
    // Outer levels padded to the slots (forecast slots stay in the last group).
    let mut outer: Vec<Vec<String>> =
        data.category_levels.iter().map(|l| (0..ncat_slots).map(|j| l.get(j).cloned().unwrap_or_default()).collect()).collect();
    if first_axis.categories_reversed(horizontal) {
        reverse_categories(&mut labels, &mut outer);
    }
    let (area, table) = super::table::reserve(ctx, chart, data, area, ncat_slots);
    let category_axis = super::table::category_axis(&table, &labels, &outer);
    let plot = frame(ctx, area, category_axis, &y1, y2.as_ref(), horizontal, chart.gridlines);
    if !plot.ok() {
        return;
    }
    let axis_of = |i| axis_for(sec.get(i).copied().unwrap_or(false));
    let cat_len = if horizontal { plot.h } else { plot.w };
    let cw = cat_len / ncat_slots as f32;
    let center = |j: usize, reversed| {
        let j = if reversed { ncat_slots - 1 - j } else { j };
        if horizontal { plot.bottom() - (j as f32 + 0.5) * cw } else { plot.x + (j as f32 + 0.5) * cw }
    };
    let overlay_axes = [false, true].map(|secondary| {
        let members = data.series.iter().filter(|s| s.secondary == secondary && is_xy(s.kind));
        let (lo, hi) = value_range(members.clone().flat_map(|s| s.values.iter().flatten().copied())).unwrap_or((0.0, 1.0));
        let spec = chart.series.iter().filter(|s| s.secondary == secondary && s.kind.is_some_and(is_xy)).find_map(|s| s.axes.as_ref());
        OverlayAxes {
            spec,
            x: overlay_x_scale(axis_for(secondary).scale, spec.map(|a| &a.x), members),
            y: nice_scale(lo, hi, false).with_overlay_bounds(spec.map(|a| &a.y)),
        }
    });

    let (slot_of, nslots) = column_slots(&fams, &sec);
    let bw = cw / (nslots + 1.5);
    let labels_on = chart.data_labels && ncat <= MAX_LABELS;
    // Area labels wait until every area is filled, so a later (stacked) area can't paint over them.
    let mut area_labels = Vec::new();

    // Areas first, then columns, then lines on top.
    for pass in 0..3 {
        for (i, s) in data.series.iter().enumerate() {
            let fam = fams.get(i).copied().unwrap_or(Fam::Col);
            let want = match pass {
                0 => matches!(fam, Fam::Area | Fam::AreaStack | Fam::Area100),
                1 => is_col(fam),
                _ => is_line(fam),
            };
            if !want {
                continue;
            }
            // Invisible offsets still take part in stacking and scale calculation.
            if s.color[3] == 0 && s.outline_color.is_none() {
                continue;
            }
            if xy_combo && is_xy(s.kind) {
                xy_overlay(ctx, s, &overlay_axes[usize::from(s.secondary)], plot, chart);
                continue;
            }
            let axis = axis_of(i);
            let sc = axis.scale;
            let center = |j| center(j, axis.categories_reversed(horizontal));
            let value_reversed = axis.value_reversed(horizontal);
            let vpos = |v: f64, sc: &Scale| {
                let f = sc.frac(v);
                let f = if value_reversed { 1.0 - f } else { f };
                if horizontal { plot.x + f * plot.w } else { plot.bottom() - f * plot.h }
            };
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
                        let c = center(j);
                        top.push([c, vpos(t, &sc)]);
                        base.push([c, vpos(b, &sc)]);
                        // Source values (not stack totals), just above the top edge like the line labels.
                        if labels_on
                            && sp.is_some()
                            && let Some(v) = s.values.get(j).copied().flatten()
                        {
                            area_labels.push((c, vpos(t, &sc) - 5.0, fmt.format(v, None)));
                        }
                    }
                    if top.len() == 1 {
                        // A single point: give it some width.
                        if let (Some(t), Some(b)) = (top.first().copied(), base.first().copied()) {
                            top = vec![[t[0] - cw * 0.3, t[1]], [t[0] + cw * 0.3, t[1]]];
                            base = vec![[b[0] - cw * 0.3, b[1]], [b[0] + cw * 0.3, b[1]]];
                        }
                    }
                    let poly = decimate_area(top, base, MAX_LINE_PTS * 2);
                    if poly.len() >= 3 {
                        ctx.out.push(Prim::Polygon { pts: poly, fill: s.color, stroke: None });
                    }
                }
                1 => {
                    let k = slot_of.get(i).copied().unwrap_or(0) as f32;
                    // Hairline bars are merged per pixel column (their envelope is what shows).
                    let cull = merge_hairlines(bw, labels_on) && s.outline_color.is_none();
                    let stroke = s.outline_color.map(|c| (c, 1.0)).or_else(|| (chart.style == 9).then_some((WHITE, 1.0)));
                    let mut sink = BarSink::new(plot, bw, horizontal, s.color, stroke, nslots, k);
                    for (j, sp) in row.iter().enumerate() {
                        let Some((b, t)) = sp else { continue };
                        let (pb, pt) = (vpos(*b, &sc), vpos(*t, &sc));
                        let c = center(j);
                        let lo = pb.min(pt);
                        let len = (pb - pt).abs();
                        sink.bar(ctx, cull, c, lo, len);
                        if labels_on && let Some(v) = s.values.get(j).copied().flatten() {
                            let t = fmt.format(v, None);
                            let stacked = fam != Fam::Col;
                            let mid = (pb + pt) / 2.0;
                            if horizontal {
                                let y = c + nslots * bw / 2.0 - (k + 0.5) * bw;
                                if stacked {
                                    ctx.text(mid, y, &t, LABEL_SIZE, on(s.color), (HAlign::Center, VAlign::Middle));
                                } else if pt < pb || (pt == pb && value_reversed != (v < 0.0)) {
                                    ctx.text(pt - 3.0, y, &t, LABEL_SIZE, TEXT, (HAlign::Right, VAlign::Middle));
                                } else {
                                    ctx.text(pt + 3.0, y, &t, LABEL_SIZE, TEXT, (HAlign::Left, VAlign::Middle));
                                }
                            } else {
                                let x = c - nslots * bw / 2.0 + (k + 0.5) * bw;
                                if stacked {
                                    ctx.text(x, mid, &t, LABEL_SIZE, on(s.color), (HAlign::Center, VAlign::Middle));
                                } else if v >= 0.0 {
                                    ctx.text(x, lo - 2.0, &t, LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Bottom));
                                } else {
                                    ctx.text(x, lo + len + 2.0, &t, LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Top));
                                }
                            }
                        }
                    }
                    sink.finish(ctx);
                }
                _ => {
                    let pts: Vec<Option<[f32; 2]>> = row.iter().enumerate().map(|(j, sp)| sp.map(|(_, t)| [center(j), vpos(t, &sc)])).collect();
                    let k = kinds.get(i).copied().unwrap_or(ChartKind::Line);
                    draw_line_series(ctx, s, &pts, plot, chart.style, has_markers(k) && cw >= 3.0, labels_on);
                }
            }
        }
        for (x, y, t) in area_labels.drain(..) {
            ctx.text(x, y, &t, LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Bottom));
        }
    }
    // Trendlines on top, using the same axes as the points.
    let mut trend_labels = Vec::new();
    for (i, s) in data.series.iter().enumerate() {
        if s.trendlines.is_empty() || !kinds.get(i).is_some_and(|k| supports_trendline(*k)) {
            continue;
        }
        if xy_combo && is_xy(s.kind) {
            let axes = &overlay_axes[usize::from(s.secondary)];
            let pts: Vec<_> = numeric_points(s).flatten().collect();
            draw_trendlines(ctx, s, &pts, &|x, y| axes.fractions(x, y), &|point| xy_pixel(plot, point), &mut trend_labels, (axes.x.min, axes.x.max));
            continue;
        }
        let axis = axis_of(i);
        let sc = axis.scale;
        let reverse_cat = axis.categories_reversed(horizontal);
        let reverse_value = axis.value_reversed(horizontal);
        let pts: Vec<(f64, f64)> = s.values.iter().take(ncat).enumerate().filter_map(|(j, v)| v.map(|v| ((j + 1) as f64, v))).collect();
        let frac = |x: f64, y: f64| {
            let fx = (x - 0.5) / ncat_slots as f64;
            let fy = (y - sc.min) / (sc.max - sc.min);
            (if reverse_cat { 1.0 - fx } else { fx }, if reverse_value { 1.0 - fy } else { fy })
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
    super::table::draw(ctx, chart, data, plot, table.as_ref());
}

// ---------------------------------------------------------------- scatter & bubble

pub(super) fn scatter(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let all: Vec<Vec<_>> = data.series.iter().map(|s| numeric_points(s).collect()).collect();
    let Some((mut xlo, mut xhi)) = value_range(all.iter().flatten().flatten().map(|p| p.0)) else { return };
    // Forecasts extend the x axis to the trendline's ends.
    for (s, pts) in data.series.iter().zip(&all) {
        if let Some((lo, hi)) = value_range(pts.iter().flatten().map(|p| p.0)).filter(|_| !s.trendlines.is_empty()) {
            xlo = xlo.min(lo - max_backward(s));
            xhi = xhi.max(hi + max_forward(s));
        }
    }
    let Some((ylo, yhi)) = value_range(all.iter().flatten().flatten().map(|p| p.1)) else { return };
    let xs = nice_scale(xlo, xhi, false);
    let ys = nice_scale(ylo, yhi, false);
    let fx = Fmt::new("General");
    let fy = Fmt::new(data.series.first().map(|s| s.number_format.as_str()).unwrap_or("General"));
    let plot = frame(ctx, area, CatAxis::Numeric(xs, &fx), &ValAxis { scale: ys, reversed: false, fmt: &fy }, None, false, chart.gridlines);
    if !plot.ok() {
        return;
    }
    let map = |(x, y): (f64, f64)| [plot.x + xs.frac(x) * plot.w, plot.bottom() - ys.frac(y) * plot.h];
    let bubble = chart.kind == ChartKind::Bubble;
    let rmax = plot.w.min(plot.h) * 0.12;
    for (s, pts) in data.series.iter().zip(&all) {
        let px: Vec<Option<[f32; 2]>> = pts.iter().map(|p| p.map(map)).collect();
        if bubble {
            let sizes = s.sizes.as_deref().unwrap_or(&[]);
            let smax = sizes.iter().flatten().fold(0.0f64, |a, b| a.max(*b));
            let mut bubbles: Vec<([f32; 2], f64)> = px
                .iter()
                .enumerate()
                .filter_map(|(j, p)| {
                    let sz = if sizes.is_empty() { Some(1.0) } else { sizes.get(j).copied().flatten() };
                    match (p, sz) {
                        (Some(p), Some(sz)) if sz > 0.0 => Some((*p, sz)),
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
            let lines = s.kind == ChartKind::ScatterLines;
            if lines {
                draw_runs(ctx, &px, s.color, 2.0, s.smooth.then_some(plot));
            }
            if !lines || px.len() <= 500 {
                draw_markers(ctx, px.iter().flatten().copied(), 3.5, s.color);
            }
        }
        if chart.data_labels && pts.len() <= MAX_LABELS {
            for (p, raw) in px.iter().zip(pts) {
                if let (Some(p), Some((_, y))) = (p, raw) {
                    ctx.text(p[0], p[1] - 6.0, &fy.format(*y, None), LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Bottom));
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
        let frac = |x: f64, y: f64| ((x - xs.min) / (xs.max - xs.min), (y - ys.min) / (ys.max - ys.min));
        let px = |(fx, fy): (f64, f64)| [plot.x + fx as f32 * plot.w, plot.bottom() - fy as f32 * plot.h];
        draw_trendlines(ctx, s, &pts, &frac, &px, &mut trend_labels, (xs.min, xs.max));
    }
    draw_trend_labels(ctx, plot, &trend_labels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_series_get_no_span_entries() {
        let empty = SeriesData::default();
        let full = SeriesData { values: vec![Some(1.0); 3], ..SeriesData::default() };
        let data = ChartData { series: vec![empty.clone(), full, empty], ..ChartData::default() };
        let fams = [Fam::Col; 3];
        let spans = series_spans(&data, &fams, &[false; 3], MAX_CATS);
        assert_eq!(spans.iter().map(Vec::len).collect::<Vec<_>>(), [0, MAX_CATS, 0]);
    }
}
