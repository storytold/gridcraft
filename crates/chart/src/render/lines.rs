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

/// Chart units (pixels at 100%) per point.
const PX_PER_PT: f32 = 96.0 / 72.0;
/// Thinnest series line drawn, in points: a hairline (`w="0"`) stays visible.
const MIN_SERIES_LINE_PT: f32 = 0.25;
/// Widest series line drawn, in points: Excel takes up to 1584 pt, which would cover the plot.
const MAX_SERIES_LINE_PT: f32 = 20.0;

/// The width in points Excel gives a series line the file leaves unset, by chart style (`c:style`,
/// 1–48), measured in Excel for line, scatter and radar charts: 1.5 pt in the first row of styles,
/// 3.5 in the fourth, 2.5 in the rest. A style outside 1–48 reads as Excel's default, style 2.
pub(super) fn default_series_line_pt(style: u32) -> f32 {
    match style {
        9..=24 | 33..=48 => 2.5,
        25..=32 => 3.5,
        _ => 1.5,
    }
}

/// The width a series line is drawn with: the file's, bounded in points, else the chart style's
/// default (`default_series_line_pt`), converted to chart units.
pub(super) fn series_line_width(s: &crate::SeriesData, chart: &gridcraft_model::Chart) -> f32 {
    let pt = s
        .line_width
        .filter(|w| w.is_finite())
        .map_or_else(|| default_series_line_pt(chart.style), |w| w.clamp(MIN_SERIES_LINE_PT, MAX_SERIES_LINE_PT));
    pt * PX_PER_PT
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
