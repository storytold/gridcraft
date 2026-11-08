//! Draw › Ink Replay: animate a sheet's ink strokes in the order they were drawn.
//!
//! The engine command reports the strokes (id, order, point count); this module looks them up
//! in the sheet the report came from and paints the replayed ink over the grid, progressively,
//! while the original ink strokes are left out. Time comes from egui's frame clock and the
//! animation asks for repaints, so nothing blocks.

use egui::epaint::{PathShape, PathStroke};
use gridcraft_engine::cmd::draw::{ink_points, ink_width};
use gridcraft_engine::model::{Anchor, ShapeKind, Sheet};
use serde_json::Value as Json;

use crate::{SheetApp, chartview};

/// Sheet points drawn per second, and the pause between strokes.
const REPLAY_SPEED: f32 = 400.0;
const REPLAY_GAP: f32 = 0.12;
/// No stroke appears instantly, and none takes longer than this.
const MIN_STROKE_SECS: f32 = 0.15;
const MAX_STROKE_SECS: f32 = 2.5;
/// Points and strokes animated from one report, capping ink read from hostile files.
const MAX_STROKE_POINTS: usize = 20_000;
const MAX_STROKES: usize = 2_000;

/// Ink Replay in progress: the strokes in drawing order and how far it has got. The ink it
/// replays stays hidden on the sheet while this is `Some`.
pub struct Replay {
    doc: u64,
    sheet: usize,
    strokes: Vec<ReplayStroke>,
    /// Seconds into the replay.
    t: f32,
    playing: bool,
}

struct ReplayStroke {
    anchor: Anchor,
    points: Vec<[f32; 2]>,
    color: egui::Color32,
    width: f32,
    /// When it starts being drawn and how long it takes, seconds.
    start: f32,
    dur: f32,
    /// Its length in sheet points, which `dur` was derived from.
    length: f32,
}

/// Another window's ink is not this window's replay.
pub(crate) fn replay_for(app: &SheetApp, doc: u64, sheet: usize) -> Option<&Replay> {
    app.grid.replay.as_ref().filter(|r| r.doc == doc && r.sheet == sheet)
}

/// Starts animating the strokes a `draw.inkReplay` report lists, in its order. `false` when the
/// report names no ink that is still on the active sheet.
pub fn start(app: &mut SheetApp, report: &Json) -> bool {
    let Some(d) = app.session.active() else { return false };
    let doc = d.uid;
    let sheet = d.wb.active_sheet();
    let Some(sh) = d.wb.sheet(sheet) else { return false };
    let mut start = 0.0;
    let mut strokes = Vec::new();
    for listed in report.get("strokes").and_then(Json::as_array).into_iter().flatten().take(MAX_STROKES) {
        let Some(id) = listed.get("id").and_then(Json::as_u64).and_then(|v| u32::try_from(v).ok()) else { continue };
        let Some(shape) = sh.shapes.iter().find(|s| s.id == id && s.kind == ShapeKind::Ink) else { continue };
        let points = clean_points(ink_points(shape));
        if points.is_empty() {
            continue;
        }
        let width = ink_width(shape).clamp(0.5, 40.0);
        let base = shape.line.resolve(&d.wb.theme).map(crate::theme::color32).unwrap_or(egui::Color32::DARK_GRAY);
        let length = path_len(&points);
        let dur = (length / REPLAY_SPEED).clamp(MIN_STROKE_SECS, MAX_STROKE_SECS);
        strokes.push(ReplayStroke { anchor: shape.anchor, points, color: chartview::ink_color(base, width), width, start, dur, length });
        start += dur + REPLAY_GAP;
    }
    if strokes.is_empty() {
        return false;
    }
    app.grid.replay = Some(Replay { doc, sheet, strokes, t: 0.0, playing: true });
    true
}

/// Stops the animation, if one is running, and clears the session switch behind it.
pub fn stop(app: &mut SheetApp) {
    app.grid.replay = None;
    app.session.ink_replay = false;
}

/// Advances the replay with the frame clock; call once a frame before the sheet is painted.
pub fn replay_tick(app: &mut SheetApp, ctx: &egui::Context) {
    // The active document or sheet changing ends the replay, as the ink it shows is gone.
    let stale = app.grid.replay.as_ref().is_some_and(|r| app.session.active().is_none_or(|d| d.uid != r.doc || d.wb.active_sheet() != r.sheet));
    if stale {
        stop(app);
        return;
    }
    let Some(r) = app.grid.replay.as_mut() else { return };
    if !r.playing {
        return;
    }
    let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.1);
    r.t = (r.t + dt).min(r.end());
    if r.t >= r.end() {
        r.playing = false;
    }
    ctx.request_repaint();
}

impl Replay {
    fn end(&self) -> f32 {
        self.strokes.last().map_or(0.0, |s| s.start + s.dur)
    }
}

/// Keeps only finite points, up to the per-stroke cap, so hostile ink cannot stall painting.
fn clean_points(points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    points.into_iter().filter(|p| p[0].is_finite() && p[1].is_finite()).take(MAX_STROKE_POINTS).collect()
}

fn path_len(points: &[[f32; 2]]) -> f32 {
    points.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum()
}

/// The replayed ink so far: finished strokes whole, the current one up to where it has got.
pub(crate) fn paint_replay(p: &egui::Painter, geo: &crate::grid::Geo, sh: &Sheet, r: &Replay) {
    for stroke in r.strokes.iter().filter(|s| s.start <= r.t) {
        let rect = chartview::anchor_rect(geo, sh, &stroke.anchor);
        let frac = if stroke.dur > 0.0 { ((r.t - stroke.start) / stroke.dur).clamp(0.0, 1.0) } else { 1.0 };
        let map = |q: [f32; 2]| egui::pos2(rect.left() + q[0] * geo.z, rect.top() + q[1] * geo.z);
        let width = (stroke.width * geo.z).max(1.0);
        if stroke.points.len() == 1 {
            p.circle_filled(map(stroke.points[0]), width / 2.0, stroke.color);
            continue;
        }
        let mut left = stroke.length * frac;
        let mut pts: Vec<egui::Pos2> = Vec::new();
        for w in stroke.points.windows(2) {
            let (a, b) = (w[0], w[1]);
            if pts.is_empty() {
                pts.push(map(a));
            }
            let seg = (b[0] - a[0]).hypot(b[1] - a[1]);
            if seg <= left {
                pts.push(map(b));
                left -= seg;
            } else {
                let f = if seg > 0.0 { left / seg } else { 0.0 };
                pts.push(map([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]));
                break;
            }
        }
        if pts.len() >= 2 {
            p.add(PathShape::line(pts, PathStroke::new(width, stroke.color)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_length_sums_segments() {
        assert!((path_len(&[[0.0, 0.0], [3.0, 4.0], [3.0, 10.0]]) - 11.0).abs() < 1e-6);
        assert!(path_len(&[[1.0, 1.0]]).abs() < 1e-6);
    }

    #[test]
    fn clean_points_drops_non_finite_points() {
        let pts = clean_points(vec![[0.0, 0.0], [f32::NAN, 1.0], [f32::INFINITY, 0.0], [2.0, 3.0]]);
        assert_eq!(pts, vec![[0.0, 0.0], [2.0, 3.0]]);
    }
}
