use super::frame::{Bx, CatAxis, ValAxis, frame, on, value_range};
use super::legend::{cat_label, plot_kind};
use super::lines::{draw_area_run, draw_clipped_runs, draw_markers, inside_plot, style_series};
use super::trend::{draw_trend_labels, draw_trendlines, max_backward, max_forward, max_forward_slots, supports_trendline};
use super::{
    Chart, ChartData, ChartKind, Ctx, Fmt, HAlign, LABEL_SIZE, MAX_CATS, MAX_LABELS, MAX_MARKERS, MIN_BAR_PX, Prim, Rgba, Scale, SeriesData, TEXT,
    VAlign, WHITE, clean, configured_scale, fixed_scale, nice_scale,
};

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
    fn new(pos: f32, lo: f32, hi: f32) -> Self {
        BarRun { bucket: pos.floor(), a0: pos, a1: pos, lo, hi, merged: false }
    }
    /// Whether `lo..hi` touches the run's value span; a disjoint bar would be joined by a filled gap.
    fn touches(&self, lo: f32, hi: f32) -> bool {
        lo <= self.hi && hi >= self.lo
    }
    fn add(&mut self, pos: f32, lo: f32, hi: f32) {
        self.merged = true;
        self.a0 = self.a0.min(pos);
        self.a1 = self.a1.max(pos);
        self.lo = self.lo.min(lo);
        self.hi = self.hi.max(hi);
    }
    /// `band` is the part of the pixel column reserved for this series (`start`, `len`): in a
    /// clustered chart a merged run stays inside it, so the other series keep a visible strip.
    fn draw(&self, ctx: &mut Ctx, bw: f32, horizontal: bool, color: Rgba, band: Option<(f32, f32)>) {
        let (a0, along) = match band {
            Some((start, len)) if self.merged => (self.bucket + start, len),
            _ => (self.a0, self.a1 - self.a0 + bw),
        };
        if horizontal {
            ctx.rect(self.lo, a0, self.hi - self.lo, along, color, None);
        } else {
            ctx.rect(a0, self.lo, along, self.hi - self.lo, color, None);
        }
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
    bw: f32,
    horizontal: bool,
    color: Rgba,
    nslots: f32,
    k: f32,
}

impl BarSink {
    fn new(bw: f32, horizontal: bool, color: Rgba, nslots: f32, k: f32) -> Self {
        BarSink { run: None, bw, horizontal, color, nslots, k }
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
            if self.horizontal {
                ctx.rect(lo, pos, len, self.bw, self.color, None);
            } else {
                ctx.rect(pos, lo, self.bw, len, self.color, None);
            }
            return;
        }
        match &mut self.run {
            Some(m) if m.bucket == pos.floor() && m.touches(lo, lo + len) => m.add(pos, lo, lo + len),
            run => {
                if let Some(m) = run.replace(BarRun::new(pos, lo, lo + len)) {
                    m.draw(ctx, self.bw, self.horizontal, self.color, band);
                }
            }
        }
    }
    fn finish(self, ctx: &mut Ctx) {
        let band = self.band();
        if let Some(m) = self.run {
            m.draw(ctx, self.bw, self.horizontal, self.color, band);
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

fn axis_scales(
    chart: &Chart,
    horizontal: bool,
    data: &ChartData,
    fams: &[Fam],
    sec: &[bool],
    spans: &[Vec<Option<(f64, f64)>>],
) -> Vec<(Scale, Fmt)> {
    // Axis scales.
    let mut axes: Vec<(Scale, Fmt)> = Vec::new();
    for axis in [false, true] {
        let members: Vec<usize> = (0..data.series.len()).filter(|i| sec.get(*i).copied().unwrap_or(false) == axis).collect();
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
        let (mut lo, hi) = range.unwrap_or((0.0, 0.0));
        let primary_options = if horizontal { &chart.format.x_axis } else { &chart.format.y_axis };
        if !axis && primary_options.logarithmic {
            lo = members.iter().filter_map(|i| spans.get(*i)).flatten().flatten().map(|(_, top)| *top).filter(|v| *v > 0.0).fold(hi, f64::min);
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
        let mut scale = if !axis && all100 && primary_options.logarithmic {
            configured_scale(lo, hi, false, primary_options)
        } else if !axis && all100 && (primary_options.minimum.is_some() || primary_options.maximum.is_some()) {
            configured_scale(scale.min, scale.max, false, primary_options)
        } else {
            scale
        };
        if !axis
            && all100
            && let Some(step) = primary_options.major_unit.filter(|v| v.is_finite() && *v > 0.0)
        {
            scale.step = step;
        }
        if !axis {
            scale.reversed = primary_options.reversed;
        }
        let fmt = if !axis { primary_options.number_format.as_deref().map(Fmt::new).unwrap_or(fmt) } else { fmt };
        axes.push((scale, fmt));
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

/// Whether a line-series kind or an explicit marker override requests point markers.
fn has_markers(chart: &Chart, series: usize, kind: ChartKind) -> bool {
    matches!(kind, ChartKind::LineMarkers | ChartKind::LineStacked100Markers)
        || chart.format.series.get(&series).is_some_and(|style| style.marker.is_some())
}

fn reversed_categories(chart: &Chart, horizontal: bool) -> bool {
    if horizontal {
        chart.format.y_axis.reversed
    } else {
        chart.format.x_axis.reversed && !matches!(chart.kind, ChartKind::Histogram | ChartKind::Waterfall | ChartKind::BoxWhisker | ChartKind::Stock)
    }
}

pub(super) fn cartesian(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx, horizontal: bool) {
    let ncat = data.categories.len().max(data.series.iter().map(|s| s.values.len()).max().unwrap_or(0)).min(MAX_CATS);
    if ncat == 0 {
        return;
    }
    let kinds: Vec<ChartKind> = data.series.iter().enumerate().map(|(i, s)| if horizontal { chart.kind } else { plot_kind(chart, s, i) }).collect();
    let fams: Vec<Fam> = kinds.iter().map(|k| family(*k)).collect();
    let any_primary = data.series.iter().any(|s| !s.secondary);
    let sec: Vec<bool> = data.series.iter().map(|s| s.secondary && any_primary && !horizontal).collect();

    let spans = series_spans(data, &fams, &sec, ncat);

    let axes = axis_scales(chart, horizontal, data, &fams, &sec, &spans);
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
    let (area, table) = super::table::reserve(ctx, chart, data, area, ncat_slots);
    let category_axis = super::table::category_axis(&table, &labels, &outer);
    let plot = frame(ctx, area, category_axis, &y1, y2.as_ref(), horizontal, chart);
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

    let (slot_of, nslots) = column_slots(&fams, &sec);
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
                0 => matches!(fam, Fam::Area | Fam::AreaStack | Fam::Area100),
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
                    // Hairline bars are merged per pixel column (their envelope is what shows).
                    let cull = merge_hairlines(bw, labels_on);
                    let mut sink = BarSink::new(bw, horizontal, s.color, nslots, k);
                    for (j, sp) in row.iter().enumerate() {
                        let Some((b, t)) = sp else { continue };
                        let baseline = if sc.logarithmic && *b <= 0.0 { sc.min } else { *b };
                        let (pb, pt) = (vpos(baseline, &sc), vpos(*t, &sc));
                        let c = center(j);
                        let lo = pb.min(pt);
                        let len = (pb - pt).abs();
                        if cull {
                            sink.bar(ctx, true, c, lo, len);
                        } else if horizontal {
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
                                    ctx.text(mid, y, &t, LABEL_SIZE, on(s.color), (HAlign::Center, VAlign::Middle));
                                } else if v >= 0.0 {
                                    ctx.text(pb.max(pt) + 3.0, y, &t, LABEL_SIZE, TEXT, (HAlign::Left, VAlign::Middle));
                                } else {
                                    ctx.text(lo - 3.0, y, &t, LABEL_SIZE, TEXT, (HAlign::Right, VAlign::Middle));
                                }
                            } else {
                                let x = c - group_width / 2.0 + bw / 2.0 + k * separation * bw;
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
                    if has_markers(chart, i, k) && cw >= 3.0 {
                        draw_markers(ctx, pts.iter().flatten().copied().filter(|p| inside_plot(*p, plot)), 3.5, s.color);
                    }
                    if labels_on {
                        for (j, p) in pts.iter().enumerate() {
                            if let (Some(p), Some(v)) = (p, s.values.get(j).copied().flatten())
                                && inside_plot(*p, plot)
                            {
                                ctx.text(p[0], p[1] - 5.0, &fmt.format(v, None), LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Bottom));
                            }
                        }
                    }
                }
            }
            style_series(ctx, style_start, chart.format.series.get(&i), s.color);
        }
        for (x, y, t) in area_labels.drain(..) {
            ctx.text(x, y, &t, LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Bottom));
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
    super::table::draw(ctx, chart, data, plot, table.as_ref(), reversed_categories(chart, horizontal));
}

// ---------------------------------------------------------------- scatter & bubble

pub(super) fn scatter(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
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
                    ctx.text(p[0], p[1] - 6.0, &label_format.format(*y, None), LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Bottom));
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
