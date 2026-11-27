use super::frame::Bx;
use super::{Ctx, MAX_LINE_PTS, MAX_MARKERS, Prim, Rgba, WHITE};
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

/// Decimates the top and baseline of an area chart together at the same horizontal indices so
/// `area_mesh`'s paired vertical-strip topology (`pts[i]` ↔ `pts[2n - 1 - i]`) is preserved.
pub(super) fn decimate_area(top: Vec<[f32; 2]>, base: Vec<[f32; 2]>, max: usize) -> Vec<[f32; 2]> {
    let n = top.len().min(base.len());
    if n == 0 {
        return Vec::new();
    }
    if n <= max {
        let mut poly = top;
        poly.truncate(n);
        poly.extend(base.into_iter().take(n).rev());
        return poly;
    }
    let mut cols: Vec<[usize; 6]> = Vec::new();
    let mut keep = Vec::with_capacity(max * 2);
    let mut i = 0;
    while i < n {
        let col = top[i][0].floor();
        let (start, mut t_mn, mut t_mx, mut b_mn, mut b_mx) = (i, i, i, i, i);
        let mut k = i + 1;
        while k < n && top[k][0].floor() == col {
            if top[k][1] < top[t_mn][1] {
                t_mn = k;
            }
            if top[k][1] > top[t_mx][1] {
                t_mx = k;
            }
            if base[k][1] < base[b_mn][1] {
                b_mn = k;
            }
            if base[k][1] > base[b_mx][1] {
                b_mx = k;
            }
            k += 1;
        }
        let entry = [start, t_mn, t_mx, b_mn, b_mx, k - 1];
        cols.push(entry);
        let mut idx = entry;
        idx.sort_unstable();
        let mut last = usize::MAX;
        for ix in idx {
            if ix != last {
                keep.push(ix);
                last = ix;
            }
        }
        i = k;
    }
    if keep.len() > max * 4 {
        let max_groups = (max * 4 / 6).max(1);
        let group_size = cols.len().div_ceil(max_groups);
        keep.clear();
        for ch in cols.chunks(group_size) {
            let start = ch[0][0];
            let end = ch.last().map_or(start, |c| c[5]);
            let t_mn = ch.iter().map(|c| c[1]).min_by(|&a, &b| top[a][1].total_cmp(&top[b][1])).unwrap_or(start);
            let t_mx = ch.iter().map(|c| c[2]).max_by(|&a, &b| top[a][1].total_cmp(&top[b][1])).unwrap_or(start);
            let b_mn = ch.iter().map(|c| c[3]).min_by(|&a, &b| base[a][1].total_cmp(&base[b][1])).unwrap_or(start);
            let b_mx = ch.iter().map(|c| c[4]).max_by(|&a, &b| base[a][1].total_cmp(&base[b][1])).unwrap_or(start);
            let mut idx = [start, t_mn, t_mx, b_mn, b_mx, end];
            idx.sort_unstable();
            let mut last = usize::MAX;
            for ix in idx {
                if ix != last {
                    keep.push(ix);
                    last = ix;
                }
            }
        }
    }
    let mut poly = Vec::with_capacity(keep.len() * 2);
    for &ix in &keep {
        poly.push(top[ix]);
    }
    for &ix in keep.iter().rev() {
        poly.push(base[ix]);
    }
    poly
}

type ClippedSegment = ((f64, f64), (f64, f64), bool, bool);

/// Liang–Barsky: the part of segment `a`–`b` inside the unit square, with whether the start and
/// end were cut.
fn clip_segment(a: (f64, f64), b: (f64, f64)) -> Option<ClippedSegment> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    if !dx.is_finite() || !dy.is_finite() {
        return None;
    }
    let (mut t0, mut t1) = (0.0, 1.0);
    let (mut entry, mut exit) = (None, None);
    for (p, q, end, edge) in
        [(-dx, a.0, b.0, (0, 0.0)), (dx, 1.0 - a.0, 1.0 - b.0, (0, 1.0)), (-dy, a.1, b.1, (1, 0.0)), (dy, 1.0 - a.1, 1.0 - b.1, (1, 1.0))]
    {
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
                if r > t0 || (r == t0 && q < 0.0) {
                    (t0, entry) = (r, Some(edge));
                }
            } else {
                if r < t0 {
                    return None;
                }
                if r < t1 || (r == t1 && r == 1.0 && end < 0.0) {
                    (t1, exit) = (r, Some(edge));
                }
            }
        }
    }
    // Set the crossed coordinate exactly even if a long segment loses precision at the edge.
    let point = |t, (axis, edge): (usize, f64)| {
        let mut p = [dx.mul_add(t, a.0).clamp(0.0, 1.0), dy.mul_add(t, a.1).clamp(0.0, 1.0)];
        p[axis] = edge;
        (p[0], p[1])
    };
    let start = entry.map_or(a, |edge| point(t0, edge));
    let end = exit.map_or(b, |edge| point(t1, edge));
    Some((start, end, entry.is_some(), exit.is_some()))
}

/// Clips a sampled polyline in plot fractions (`None` = gap) to the plot box (the unit
/// square), returning the visible runs.
pub(super) fn clip_unit(pts: &[Option<(f64, f64)>]) -> Vec<Vec<(f64, f64)>> {
    let ok = |p: &(f64, f64)| p.0.is_finite() && p.1.is_finite();
    let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut run: Vec<(f64, f64)> = Vec::new();
    for w in pts.windows(2) {
        let seg = match w {
            [Some(a), Some(b)] if ok(a) && ok(b) => clip_segment(*a, *b),
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

/// Draws a polyline split at gaps (`None`), decimated when long. With `smooth` (the plot box),
/// each run is interpolated with a Catmull-Rom spline so the line curves through the data points.
///
/// The points of the smoothed runs (data points included) are budgeted across all the runs of the line, not per run:
/// short runs between gaps escape decimation, and 100 sub-segments each would turn a
/// 100,000-point series of three-point runs into millions of points. Past the budget the
/// remaining runs are drawn through their data points only.
pub(super) fn draw_runs(ctx: &mut Ctx, pts: &[Option<[f32; 2]>], color: Rgba, width: f32, smooth: Option<Bx>) {
    let mut budget = MAX_LINE_PTS * 2;
    let mut run: Vec<[f32; 2]> = Vec::new();
    for p in pts.iter().chain(std::iter::once(&None)) {
        match p {
            Some(p) => run.push(*p),
            None => {
                if run.len() >= 2 {
                    let r = decimate(std::mem::take(&mut run), MAX_LINE_PTS);
                    let r = match smooth {
                        Some(plot) => {
                            let r = smooth_run(r, plot, budget);
                            budget = budget.saturating_sub(r.len());
                            r
                        }
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

/// Smooth in plot fractions, then clip the resulting segments before converting to pixels.
pub(super) fn draw_clipped_runs(
    ctx: &mut Ctx,
    pts: &[Option<(f64, f64)>],
    color: Rgba,
    width: f32,
    smooth: bool,
    px: impl Fn((f64, f64)) -> [f32; 2],
) {
    let mut budget = MAX_LINE_PTS * 2;
    for run in pts.split(Option::is_none).filter(|run| run.len() >= 2) {
        let samples;
        let run = if smooth && run.len() >= 3 && budget > run.len() {
            let points: Vec<_> = run.iter().flatten().map(|&(x, y)| [x, y]).collect();
            samples = smooth_points(&points, budget).into_iter().map(|[x, y]| Some((x, y))).collect::<Vec<_>>();
            budget = budget.saturating_sub(samples.len());
            &samples
        } else {
            run
        };
        for visible in clip_unit(run) {
            ctx.line(decimate(visible.into_iter().map(&px).collect(), MAX_LINE_PTS), color, width);
        }
    }
}

/// Catmull-Rom spline through `pts`: every segment gets the same number of sub-segments (up to
/// 100), chosen so the whole curve stays within `cap` points (one per data point when `cap` is
/// smaller than that). Endpoints are
/// duplicated so the curve passes through the first and last points, and the curve is clamped
/// to the `plot` box (a spline overshoots its data near sharp turns).
fn smooth_run(pts: Vec<[f32; 2]>, plot: Bx, cap: usize) -> Vec<[f32; 2]> {
    if pts.len() < 3 {
        return pts;
    }
    let (x0, x1) = (plot.x.min(plot.right()), plot.x.max(plot.right()));
    let (y0, y1) = (plot.y.min(plot.bottom()), plot.y.max(plot.bottom()));
    let points: Vec<_> = pts.into_iter().map(|p| p.map(f64::from)).collect();
    smooth_points(&points, cap).into_iter().map(|[x, y]| [(x as f32).clamp(x0, x1), (y as f32).clamp(y0, y1)]).collect()
}

fn smooth_points(pts: &[[f64; 2]], cap: usize) -> Vec<[f64; 2]> {
    let n = pts.len();
    let steps = (cap / n).clamp(1, 100);
    let mut out = Vec::with_capacity(n.saturating_mul(steps).min(cap) + 1);
    let at = |i: isize| {
        let j = i.clamp(0, n as isize - 1) as usize;
        pts.get(j).copied().unwrap_or([0.0, 0.0])
    };
    for i in 0..n.saturating_sub(1) {
        let (p0, p1, p2, p3) = (at(i as isize - 1), at(i as isize), at(i as isize + 1), at(i as isize + 2));
        for s in 0..steps {
            let t = s as f64 / steps as f64;
            out.push(catmull(p0, p1, p2, p3, t));
        }
    }
    out.push(at(n as isize - 1));
    out
}

/// One Catmull-Rom point between `p1` and `p2` (uniform parameterisation), tangent-scaled by 0.5.
fn catmull(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], t: f64) -> [f64; 2] {
    let t2 = t * t;
    let t3 = t2 * t;
    let mut o = [0.0; 2];
    for k in 0..2 {
        let (a0, a1, a2, a3) = (p0[k], p1[k], p2[k], p3[k]);
        o[k] = 0.5 * ((2.0 * a1) + (-a0 + a2) * t + (2.0 * a0 - 5.0 * a1 + 4.0 * a2 - a3) * t2 + (-a0 + 3.0 * a1 - 3.0 * a2 + a3) * t3);
    }
    o
}

pub(super) fn draw_markers(ctx: &mut Ctx, pts: impl Iterator<Item = [f32; 2]>, r: f32, color: Rgba) {
    markers(ctx, pts, r, color, None);
}

pub(super) fn draw_plot_markers(ctx: &mut Ctx, pts: impl Iterator<Item = [f32; 2]>, r: f32, color: Rgba, plot: Bx) {
    markers(ctx, pts, r, color, Some(plot));
}

fn markers(ctx: &mut Ctx, pts: impl Iterator<Item = [f32; 2]>, r: f32, color: Rgba, plot: Option<Bx>) {
    const STROKE: f32 = 0.75;
    let mut seen = HashSet::new();
    for p in pts {
        if seen.len() >= MAX_MARKERS {
            break;
        }
        if seen.insert(((p[0] * 0.5) as i32, (p[1] * 0.5) as i32)) {
            let outer = r + STROKE / 2.0;
            if let Some(plot) = plot.filter(|b| p[0] - outer < b.x || p[0] + outer > b.right() || p[1] - outer < b.y || p[1] + outer > b.bottom()) {
                // Clip the fill and outline separately so the plot edge gains no outline.
                clipped_disc(ctx, p, outer, WHITE, plot);
                clipped_disc(ctx, p, r - STROKE / 2.0, color, plot);
            } else {
                ctx.out.push(Prim::Circle { cx: p[0], cy: p[1], r, fill: Some(color), stroke: Some((WHITE, STROKE)) });
            }
        }
    }
}

fn clipped_disc(ctx: &mut Ctx, center: [f32; 2], radius: f32, fill: Rgba, plot: Bx) {
    const SIDES: usize = 24;
    let mut pts: Vec<_> = (0..SIDES)
        .map(|i| {
            let (sin, cos) = (std::f32::consts::TAU * i as f32 / SIDES as f32).sin_cos();
            [center[0] + radius * cos, center[1] + radius * sin]
        })
        .collect();
    for (axis, edge, sign) in [(0, plot.x, 1.0), (0, plot.right(), -1.0), (1, plot.y, 1.0), (1, plot.bottom(), -1.0)] {
        let inside = |p: [f32; 2]| (p[axis] - edge) * sign >= 0.0;
        let input = std::mem::take(&mut pts);
        let Some(&last) = input.last() else { return };
        let mut prev = last;
        for p in input {
            if inside(prev) != inside(p) {
                let t = (edge - prev[axis]) / (p[axis] - prev[axis]);
                let mut hit = p;
                hit[axis] = edge;
                hit[1 - axis] = prev[1 - axis] + t * (p[1 - axis] - prev[1 - axis]);
                pts.push(hit);
            }
            if inside(p) {
                pts.push(p);
            }
            prev = p;
        }
    }
    if pts.len() >= 3 {
        ctx.out.push(Prim::Polygon { pts, fill, stroke: None });
    }
}

#[cfg(test)]
mod smooth_tests {
    use super::*;

    #[test]
    fn long_runs_are_smoothed_uniformly_within_the_cap() {
        let plot = Bx { x: 0.0, y: 0.0, w: 3000.0, h: 100.0 };
        let pts: Vec<[f32; 2]> = (0..1500).map(|i| [i as f32 * 2.0, if i % 2 == 0 { 20.0 } else { 80.0 }]).collect();
        let out = smooth_run(pts, plot, MAX_LINE_PTS * 2);
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
        let out = smooth_run(pts, plot, MAX_LINE_PTS * 2);
        assert!(out.len() > 5);
        assert!(out.iter().all(|p| (0.0..=300.0).contains(&p[0]) && (10.0..=110.0).contains(&p[1])));
    }

    /// Security: smoothing budgets its added points across every run of a line. A series of
    /// three-point runs between gaps used to get 100 sub-segments per run, 67 times its data.
    #[test]
    fn gap_separated_runs_share_one_smoothing_budget() {
        let plot = Bx { x: 0.0, y: 0.0, w: 1e6, h: 100.0 };
        let pts: Vec<Option<[f32; 2]>> =
            (0..100_000).map(|i| if i % 4 == 3 { None } else { Some([i as f32, if i % 2 == 0 { 20.0 } else { 80.0 }]) }).collect();
        let m = crate::ApproxMeasure;
        let mut ctx = Ctx { out: Vec::new(), m: &m };
        draw_runs(&mut ctx, &pts, WHITE, 1.0, Some(plot));
        let total: usize = ctx.out.iter().map(|p| if let Prim::Line { pts, .. } = p { pts.len() } else { 0 }).sum();
        // 75,000 data points, plus at most the budget added by interpolation.
        assert!(total <= 75_000 + MAX_LINE_PTS * 2, "{total}");
        assert_eq!(ctx.out.len(), 25_000);
        // The first runs are still smoothed.
        assert!(matches!(ctx.out.first(), Some(Prim::Line { pts, .. }) if pts.len() > 3));
    }

    #[test]
    fn decimate_area_preserves_paired_vertical_strips() {
        let top: Vec<[f32; 2]> = (0..1000).map(|i| [i as f32 * 0.25, ((i * 37) % 100) as f32]).collect();
        let base: Vec<[f32; 2]> = (0..1000).map(|i| [i as f32 * 0.25, 100.0]).collect();
        let poly = decimate_area(top, base, 64);
        assert_eq!(poly.len() % 2, 0);
        let half = poly.len() / 2;
        for i in 0..half {
            assert_eq!(poly[i][0], poly[poly.len() - 1 - i][0]);
            assert_eq!(poly[poly.len() - 1 - i][1], 100.0);
        }
    }

    #[test]
    fn decimate_area_preserves_extrema_across_3000_columns() {
        // 3,000 occupied pixel columns with 6 points per column (18,000 points total),
        // where each column has distinct top and baseline extrema so the initial per-column
        // index list exceeds `MAX_LINE_PTS * 2 * 4` (6,400).
        let max = MAX_LINE_PTS * 2;
        let mut top = Vec::with_capacity(18_000);
        let mut base = Vec::with_capacity(18_000);
        for col in 0..3_000 {
            let y_top = [50.0, 20.0, 80.0, 50.0, 50.0, 50.0];
            let y_base = [150.0, 150.0, 150.0, 120.0, 180.0, 150.0];
            for s in 0..6 {
                let x = col as f32 + s as f32 * 0.1;
                top.push([x, y_top[s]]);
                base.push([x, y_base[s]]);
            }
        }
        // Plant sharp spikes on both top and baseline in interior columns.
        top[777 * 6 + 1][1] = 1.0;
        top[1999 * 6 + 2][1] = 99.0;
        base[1234 * 6 + 3][1] = 101.0;
        base[2501 * 6 + 4][1] = 199.0;

        let poly = decimate_area(top, base, max);
        assert_eq!(poly.len() % 2, 0);
        let half = poly.len() / 2;
        assert!(half <= max * 4, "retained {half} paired vertices, limit is {}", max * 4);
        for i in 0..half {
            assert_eq!(poly[i][0], poly[poly.len() - 1 - i][0]);
        }
        let top_kept = &poly[..half];
        let base_kept = &poly[half..];
        assert_eq!(top_kept.first().map(|p| p[0]), Some(0.0));
        assert_eq!(top_kept.last().map(|p| p[0]), Some(2999.5));
        assert!(top_kept.iter().any(|p| p[1] == 1.0), "top min spike preserved");
        assert!(top_kept.iter().any(|p| p[1] == 99.0), "top max spike preserved");
        assert!(base_kept.iter().any(|p| p[1] == 101.0), "base min spike preserved");
        assert!(base_kept.iter().any(|p| p[1] == 199.0), "base max spike preserved");
    }
}
