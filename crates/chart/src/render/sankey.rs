//! Sankey: nodes in columns, flows as bands whose width is the flow value.
//! Data: the categories' outer level holds each flow's source, the inner level its target,
//! the first series the values.

use super::frame::Bx;
use super::{Chart, ChartData, Ctx, Fmt, HAlign, Hit, LABEL_SIZE, Prim, Rgba, TEXT, VAlign};

/// Most flows drawn; the rest are dropped.
const MAX_FLOWS: usize = 2000;
/// Points along each edge of a flow's band.
const CURVE_STEPS: usize = 16;

/// One flow: its data point (row of the source table), source and target node, value.
#[derive(Clone, Copy)]
struct Flow {
    row: usize,
    from: usize,
    to: usize,
    value: f64,
}

/// The flows and the node names in first-seen order. Blank, non-positive and self-referencing
/// flows are skipped.
fn flows(data: &ChartData) -> (Vec<Flow>, Vec<String>) {
    let (Some(s), Some(sources)) = (data.series.first(), data.category_levels.first()) else { return (vec![], vec![]) };
    let mut names: Vec<String> = Vec::new();
    let mut index = std::collections::HashMap::new();
    let mut node = |name: &str| -> usize {
        *index.entry(name.to_string()).or_insert_with(|| {
            names.push(name.to_string());
            names.len() - 1
        })
    };
    let mut out = Vec::new();
    for (row, v) in s.values.iter().enumerate().take(MAX_FLOWS) {
        let (Some(value), Some(from), Some(to)) = (v.filter(|v| *v > 0.0), sources.get(row), data.categories.get(row)) else { continue };
        if from.is_empty() || to.is_empty() || from == to {
            continue;
        }
        out.push(Flow { row, from: node(from), to: node(to), value });
    }
    (out, names)
}

/// Column of each node: the longest chain of flows leading into it. A cycle stops growing at
/// the node count, so the loop always ends.
fn columns(flows: &[Flow], n: usize) -> Vec<usize> {
    let mut col = vec![0usize; n];
    for _ in 0..n {
        let mut changed = false;
        for f in flows {
            let want = col.get(f.from).map_or(0, |c| c + 1);
            if let Some(c) = col.get_mut(f.to)
                && *c < want
                && want < n
            {
                *c = want;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    col
}

fn alpha(c: Rgba, a: u8) -> Rgba {
    [c[0], c[1], c[2], a]
}

fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// A band from `(x0, y0..y0+w)` to `(x1, y1..y1+w)`, eased horizontally at both ends.
fn band(x0: f32, y0: f32, x1: f32, y1: f32, w: f32) -> Vec<[f32; 2]> {
    let edge = |dy: f32| -> Vec<[f32; 2]> {
        (0..=CURVE_STEPS).map(|i| i as f32 / CURVE_STEPS as f32).map(|t| [x0 + (x1 - x0) * t, y0 + dy + (y1 - y0) * ease(t)]).collect()
    };
    let mut pts = edge(0.0);
    pts.extend(edge(w).into_iter().rev());
    pts
}

/// Whether `p` lies on the band [`band`] draws.
fn on_band(p: [f32; 2], x0: f32, y0: f32, x1: f32, y1: f32, w: f32) -> bool {
    if !(x1 > x0 && p[0] >= x0 && p[0] <= x1) {
        return false;
    }
    let top = y0 + (y1 - y0) * ease((p[0] - x0) / (x1 - x0));
    // Thin bands stay easy to point at.
    let slack = ((3.0 - w) / 2.0).max(0.0);
    p[1] >= top - slack && p[1] <= top + w + slack
}

pub(super) fn sankey(ctx: &mut Ctx, chart: &Chart, data: &ChartData, area: Bx) {
    let (flows, names) = flows(data);
    let n = names.len();
    let col = columns(&flows, n);
    // Back edges of a cycle can't be drawn left to right; they're left out.
    let flows: Vec<Flow> = flows.into_iter().filter(|f| col.get(f.from) < col.get(f.to)).collect();
    let last = col.iter().copied().max().unwrap_or(0);
    let Some(series) = data.series.first() else { return };
    if flows.is_empty() || last == 0 {
        return;
    }
    let (mut inflow, mut outflow) = (vec![0.0f64; n], vec![0.0f64; n]);
    for f in &flows {
        if let Some(o) = outflow.get_mut(f.from) {
            *o += f.value;
        }
        if let Some(i) = inflow.get_mut(f.to) {
            *i += f.value;
        }
    }
    let size: Vec<f64> = inflow.iter().zip(&outflow).map(|(i, o)| i.max(*o)).collect();
    let (colr, sizer) = (&col, &size);
    let in_col = |k: usize| (0..n).filter(move |i| colr.get(*i) == Some(&k) && sizer.get(*i).is_some_and(|v| *v > 0.0));
    let node_w = (area.w * 0.02).clamp(4.0, 14.0);
    let gap = (area.h * 0.04).min(12.0);
    // One scale for every column, set by the fullest one.
    let scale = (0..=last)
        .filter_map(|k| {
            let total: f64 = in_col(k).filter_map(|i| size.get(i)).sum();
            let count = in_col(k).count();
            let room = area.h - gap * count.saturating_sub(1) as f32;
            (total > 0.0).then(|| room.max(1.0) / total as f32)
        })
        .fold(f32::INFINITY, f32::min);
    if !(scale.is_finite() && scale > 0.0) {
        return;
    }
    let mut top = vec![0.0f32; n];
    let x = |i: usize| area.x + col.get(i).copied().unwrap_or(0) as f32 / last as f32 * (area.w - node_w);
    for k in 0..=last {
        let mut y = area.y;
        for i in in_col(k) {
            if let Some(t) = top.get_mut(i) {
                *t = y;
            }
            y += size.get(i).copied().unwrap_or(0.0) as f32 * scale + gap;
        }
    }
    let color = |i: usize| if data.palette.is_empty() { TEXT } else { data.palette.get(i % data.palette.len()).copied().unwrap_or(TEXT) };
    let f = Fmt::new(&series.number_format);
    let focused = |row: usize| ctx.focus.contains(&row);
    let any_focus = flows.iter().any(|fl| focused(fl.row));
    let lit = |on: bool| {
        if !any_focus {
            0x66
        } else if on {
            0xB3
        } else {
            0x1A
        }
    };
    let name = |i: usize| names.get(i).map(String::as_str).unwrap_or("");
    let mut hit = None;
    let (mut out_y, mut in_y) = (top.clone(), top.clone());
    let mut bands = Vec::with_capacity(flows.len());
    for fl in &flows {
        let w = fl.value as f32 * scale;
        let (Some(ys), Some(yt)) =
            (out_y.get_mut(fl.from).map(|y| std::mem::replace(y, *y + w)), in_y.get_mut(fl.to).map(|y| std::mem::replace(y, *y + w)))
        else {
            continue;
        };
        let (x0, x1) = (x(fl.from) + node_w, x(fl.to));
        if let Some(p) = ctx.probe
            && on_band(p, x0, ys, x1, yt, w)
        {
            let label = format!("{} \u{2192} {}: {}", name(fl.from), name(fl.to), f.format(fl.value, None));
            hit = Some(Hit { points: vec![fl.row], label });
        }
        bands
            .push((focused(fl.row), Prim::Polygon { pts: band(x0, ys, x1, yt, w), fill: alpha(color(fl.from), lit(focused(fl.row))), stroke: None }));
    }
    // Emphasised bands go on top of the faded ones.
    bands.sort_by_key(|(on, _)| *on);
    ctx.out.extend(bands.into_iter().map(|(_, p)| p));
    for i in 0..n {
        let (Some(&y), Some(&v)) = (top.get(i), size.get(i)) else { continue };
        let h = v as f32 * scale;
        if h <= 0.0 {
            continue;
        }
        let touches: Vec<usize> = flows.iter().filter(|fl| fl.from == i || fl.to == i).map(|fl| fl.row).collect();
        let on = !any_focus || touches.iter().any(|r| focused(*r));
        ctx.rect(x(i), y, node_w, h, if on { color(i) } else { alpha(color(i), 0x55) }, None);
        if let Some(p) = ctx.probe
            && p[0] >= x(i) - 2.0
            && p[0] <= x(i) + node_w + 2.0
            && p[1] >= y
            && p[1] <= y + h
        {
            hit = Some(Hit { points: touches, label: format!("{}: {}", name(i), f.format(v, None)) });
        }
        if h + gap < LABEL_SIZE * 0.9 {
            continue;
        }
        let label = if chart.data_labels { format!("{} {}", name(i), f.format(v, None)) } else { name(i).to_string() };
        let right = col.get(i) != Some(&last);
        let lx = if right { x(i) + node_w + 4.0 } else { x(i) - 4.0 };
        let label = ctx.fit(&label, LABEL_SIZE, area.w / (last as f32 + 1.0));
        ctx.text(lx, y + h / 2.0, &label, LABEL_SIZE, TEXT, (if right { HAlign::Left } else { HAlign::Right }, VAlign::Middle));
    }
    if hit.is_some() {
        ctx.hit = hit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_the_longest_chain_and_survive_cycles() {
        let fl = |from, to| Flow { row: 0, from, to, value: 1.0 };
        let f = [fl(0, 1), fl(1, 2), fl(0, 2)];
        assert_eq!(columns(&f, 3), vec![0, 1, 2]);
        let cycle = [fl(0, 1), fl(1, 0)];
        assert!(columns(&cycle, 2).iter().all(|c| *c < 2));
    }
}
