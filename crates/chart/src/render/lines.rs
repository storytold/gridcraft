use super::frame::Bx;
use super::trend::clip_unit;
use super::{Ctx, DARK_TEXT, MAX_LINE_PTS, MAX_MARKERS, PI, Prim, Rgba, WHITE};
use std::collections::HashSet;

// ---------------------------------------------------------------- helpers

/// Reduces a long polyline to first/min/max/last per pixel column.
pub(super) fn decimate(pts: Vec<[f32; 2]>, max: usize) -> Vec<[f32; 2]> {
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
pub(super) fn draw_runs(ctx: &mut Ctx, pts: &[Option<[f32; 2]>], color: Rgba, width: f32, smooth: Option<Bx>) {
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

pub(super) fn draw_markers(ctx: &mut Ctx, pts: impl Iterator<Item = [f32; 2]>, r: f32, color: Rgba) {
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

/// Whether a marker lies within the inclusive plot bounds.
pub(super) fn inside_plot(p: [f32; 2], plot: Bx) -> bool {
    p[0] >= plot.x && p[0] <= plot.right() && p[1] >= plot.y && p[1] <= plot.bottom()
}

/// Clips disconnected runs to the plot before drawing or smoothing them.
pub(super) fn draw_clipped_runs(ctx: &mut Ctx, pts: &[Option<[f32; 2]>], color: Rgba, plot: Bx, smooth: bool) {
    let units: Vec<_> = pts.iter().map(|p| p.map(|p| (((p[0] - plot.x) / plot.w) as f64, ((p[1] - plot.y) / plot.h) as f64))).collect();
    for run in clip_unit(&units) {
        let pts: Vec<_> = run.into_iter().map(|(x, y)| Some([plot.x + x as f32 * plot.w, plot.y + y as f32 * plot.h])).collect();
        draw_runs(ctx, &pts, color, 2.25, smooth.then_some(plot));
    }
}

/// Applies series/marker overrides to the primitives emitted since `start`, including legend swatches.
pub(super) fn style_series(ctx: &mut Ctx, start: usize, spec: Option<&gridcraft_model::ChartSeriesFormat>, color: Rgba) {
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

pub(super) fn draw_area_run(ctx: &mut Ctx, mut top: Vec<[f32; 2]>, mut base: Vec<[f32; 2]>, cw: f32, color: Rgba) {
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
