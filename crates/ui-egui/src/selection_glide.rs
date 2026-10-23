//! The selection outline glides to a new selection instead of jumping there, like Excel's cell
//! cursor: press an arrow key or click a cell and the border slides over in about a tenth of a
//! second; extend the selection and it grows edge by edge. View ▸ Animations turns it off.
//!
//! The behaviour comes from using Excel as a user; the duration and the easing curve are an
//! estimate ([`GLIDE_SECS`]), not yet measured against Excel.
//!
//! What glides and what doesn't:
//! - the selection changed (arrows, Enter, Tab, a click, a drag, Shift+arrows, the Name Box, any
//!   command): the outline of the current area, its fill and its fill handle glide from wherever
//!   they are drawn right now, so a held arrow key reads as one continuous motion;
//! - the same cells moved on screen (scrolling, zoom, a resized column or window): the outline
//!   stays glued to its cells;
//! - another sheet or workbook, an outline that wasn't drawn the frame before, a move that starts
//!   or ends off screen: it appears in place;
//! - a control-channel screenshot is on its way, or egui's animations are off (the headless
//!   harness turns them off): it appears in place, so scripted pictures never catch it in flight;
//! - Animations is turned off (`view.animations`): it jumps.
//!
//! Only painting is delayed. The selection itself, hit testing (the fill handle, the border you
//! drag) and the cell editor use the real cells at once.
//!
//! The glide is kept as each edge's distance from its target, decaying to zero, so the outline is
//! always "the cells' rectangle plus what is left of the offset", in screen points. Scrolling
//! under a gliding outline therefore carries it along, and an arrow key held at the edge of the
//! window (the sheet scrolls, the cell stays where it is on screen) doesn't make it wobble.
//!
//! A new destination's glide is timed from the previous frame, so the outline already moves in
//! the frame that shows the change. Timed from this frame it would stand still there, and a
//! selection that changes every frame (a held arrow key) would freeze it in place.

use egui::{Rect, pos2};
use gridcraft_engine::core::RangeRef;

/// How long a glide takes, in seconds.
pub const GLIDE_SECS: f64 = 0.12;
/// The most of a new glide that may have passed by the frame showing it: one frame at 60 Hz.
/// egui paints nothing while the app sits idle, so the previous frame can be seconds old; the
/// glide still runs its whole course then.
const HEAD_START_SECS: f64 = 1.0 / 60.0;
/// How far outside the cell area a glide may start or end (screen points).
const OFFSCREEN_SLACK: f32 = 48.0;

/// What the grid knows about the selection outline in one frame.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The workbook (its uid) and sheet on screen: the outline never glides between two of them.
    pub place: (u64, usize),
    /// The current area of the selection.
    pub range: RangeRef,
    /// Where that area's cells are on screen.
    pub target: Rect,
    /// The grid's cell area on screen.
    pub view: Rect,
    /// egui's clock, in seconds.
    pub now: f64,
    /// egui's frame counter: an outline that skipped a frame doesn't glide from its stale place.
    pub frame_nr: u64,
    /// Glides may run this frame; otherwise the outline is on its cells.
    pub animate: bool,
}

/// The outline's motion between frames. Time-based, so it looks the same at any frame rate.
#[derive(Clone, Debug)]
pub struct SelectionGlide {
    /// What the outline was last drawn for.
    last: Option<((u64, usize), RangeRef)>,
    /// Where it was drawn (screen points).
    drawn: Rect,
    /// Each edge's distance from the target when the glide set off: left, top, right, bottom.
    offset: [f32; 4],
    started: f64,
    gliding: bool,
    frame_nr: u64,
    /// The clock at the last frame.
    now: f64,
}

impl Default for SelectionGlide {
    fn default() -> Self {
        SelectionGlide { last: None, drawn: Rect::ZERO, offset: [0.0; 4], started: 0.0, gliding: false, frame_nr: 0, now: 0.0 }
    }
}

impl SelectionGlide {
    /// Advance to this frame; returns where to draw the outline.
    pub fn frame(&mut self, f: Frame) -> Rect {
        let fresh = f.frame_nr.wrapping_sub(self.frame_nr) <= 1;
        self.frame_nr = f.frame_nr;
        let previous = std::mem::replace(&mut self.now, f.now);
        let key = Some((f.place, f.range));
        if self.last != key {
            // A new destination: set off from where the outline is drawn now (mid-glide or at rest),
            // timed from the frame that drew it there so it is already on its way in this one.
            let same_sheet = self.last.is_some_and(|(place, _)| place == f.place);
            self.offset = if same_sheet && fresh { offset_between(self.drawn, f.target, f.view) } else { [0.0; 4] };
            self.started = if fresh && previous <= f.now { previous.max(f.now - HEAD_START_SECS) } else { f.now };
            self.last = key;
        }
        if !f.animate {
            self.offset = [0.0; 4];
        }
        // Arrived when the time is up (or the clock went backwards).
        let progress = (f.now - self.started) / GLIDE_SECS;
        self.gliding = self.offset != [0.0; 4] && (0.0..1.0).contains(&progress);
        let left = if self.gliding { 1.0 - ease_out(progress as f32) } else { 0.0 };
        let [l, t, r, b] = self.offset.map(|o| o * left);
        self.drawn = Rect::from_min_max(pos2(f.target.min.x + l, f.target.min.y + t), pos2(f.target.max.x + r, f.target.max.y + b));
        self.drawn
    }

    /// The outline is on its way somewhere: keep repainting.
    pub fn is_gliding(&self) -> bool {
        self.gliding
    }

    /// Where the outline was drawn last frame (screen points).
    pub fn drawn(&self) -> Rect {
        self.drawn
    }
}

/// Decelerating curve: quick off the mark, soft landing.
fn ease_out(t: f32) -> f32 {
    let r = 1.0 - t.clamp(0.0, 1.0);
    1.0 - r * r * r
}

/// Each edge's distance from `to` back to `from`. No glide (all zero) when either end is off
/// screen or the numbers are not sane. An edge sets off at most one view away, so the far edge of
/// a whole column or row slides in from the border of the window instead of popping in at the end.
fn offset_between(from: Rect, to: Rect, view: Rect) -> [f32; 4] {
    let seen = view.expand(OFFSCREEN_SLACK);
    if !(seen.intersects(from) && seen.intersects(to)) {
        return [0.0; 4];
    }
    let limit = |v: f32| if v.is_finite() { v.abs() } else { 0.0 };
    let (lx, ly) = (limit(view.width()), limit(view.height()));
    let offset = [
        (from.min.x - to.min.x).clamp(-lx, lx),
        (from.min.y - to.min.y).clamp(-ly, ly),
        (from.max.x - to.max.x).clamp(-lx, lx),
        (from.max.y - to.max.y).clamp(-ly, ly),
    ];
    if offset.iter().all(|o| o.is_finite()) { offset } else { [0.0; 4] }
}

/// The marching ants' dash offset around a copied range, by egui's clock: half a point every
/// 60 ms, wrapping with the 8-point dash pattern. Stepping it once per painted frame is steady
/// only while nothing else repaints; a gliding outline repaints every frame and would make the
/// ants lurch forward.
pub fn ants_phase(now: f64) -> f32 {
    let phase = (now / 0.12).rem_euclid(8.0) as f32;
    if phase.is_finite() { phase } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;
    use gridcraft_engine::core::CellRef;

    const PLACE: (u64, usize) = (1, 0);
    const VIEW: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 600.0));
    const DT: f64 = 1.0 / 240.0;

    fn cell(row: u32, col: u32) -> RangeRef {
        RangeRef::cell(CellRef::new(row, col))
    }

    /// A cell-sized rectangle at a place on screen.
    fn at(x: f32, y: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(64.0, 16.0))
    }

    fn near(a: Rect, b: Rect) -> bool {
        (a.min - b.min).length() < 1e-3 && (a.max - b.max).length() < 1e-3
    }

    /// Drives a glide the way the grid does: one frame per step, a clock, a frame counter.
    struct Rig {
        glide: SelectionGlide,
        f: Frame,
    }

    impl Rig {
        fn new(range: RangeRef, target: Rect) -> Self {
            let f = Frame { place: PLACE, range, target, view: VIEW, now: 10.0, frame_nr: 100, animate: true };
            let mut rig = Rig { glide: SelectionGlide::default(), f };
            rig.step(0.0);
            rig
        }

        /// The next frame, `dt` seconds later.
        fn step(&mut self, dt: f64) -> Rect {
            self.f.now += dt;
            self.f.frame_nr += 1;
            self.glide.frame(self.f)
        }

        /// The selection changes; the next frame shows it.
        fn select(&mut self, range: RangeRef, target: Rect) {
            self.f.range = range;
            self.f.target = target;
        }

        /// Steps until the outline rests, returning every rectangle drawn on the way.
        fn settle(&mut self, dt: f64) -> Vec<Rect> {
            let mut drawn = vec![self.step(dt)];
            while self.glide.is_gliding() && drawn.len() < 10_000 {
                drawn.push(self.step(dt));
            }
            drawn
        }
    }

    #[test]
    fn the_first_outline_is_on_its_cells() {
        let r = Rig::new(cell(0, 0), at(10.0, 10.0));
        assert_eq!(r.glide.drawn(), at(10.0, 10.0));
        assert!(!r.glide.is_gliding());
    }

    #[test]
    fn a_move_glides_and_lands_exactly() {
        let (a, b) = (at(0.0, 0.0), at(64.0, 32.0));
        let mut r = Rig::new(cell(0, 0), a);
        r.select(cell(2, 1), b);
        let mut drawn = vec![r.step(0.0)];
        drawn.extend(r.settle(DT));
        assert_eq!(drawn.first(), Some(&a), "it sets off from where it was");
        assert_eq!(drawn.last(), Some(&b), "and lands exactly on the cells");
        assert!(drawn.len() > 20, "over about {GLIDE_SECS} s, not in one frame: {} frames", drawn.len());
        for w in drawn.windows(2) {
            assert!(w[0].min.x <= w[1].min.x && w[0].min.y <= w[1].min.y, "never backwards or past the cells: {w:?}");
            assert!(w[1].min.x <= b.min.x && w[1].min.y <= b.min.y);
            assert!((w[1].width() - 64.0).abs() < 1e-3 && (w[1].height() - 16.0).abs() < 1e-3, "it keeps its shape on the way");
        }
        // Decelerating: more than half of the way in the first third of the time.
        let third = drawn[drawn.len() / 3];
        assert!(third.min.x > 32.0, "{third:?}");
    }

    #[test]
    fn the_glide_is_the_same_at_any_frame_rate() {
        let at_time = |dt: f64, frames: usize| {
            let mut r = Rig::new(cell(0, 0), at(0.0, 0.0));
            r.select(cell(0, 5), at(320.0, 0.0));
            r.step(0.0);
            (0..frames).map(|_| r.step(dt)).last().unwrap()
        };
        // 60 ms into the glide at 50, 100 and 200 frames a second.
        let (slow, mid, fast) = (at_time(0.02, 3), at_time(0.01, 6), at_time(0.005, 12));
        assert!(near(slow, mid) && near(mid, fast), "{slow:?} {mid:?} {fast:?}");
        assert!(slow.min.x > 0.0 && slow.min.x < 320.0);
    }

    #[test]
    fn extending_the_selection_grows_it_edge_by_edge() {
        let one = at(0.0, 0.0);
        let two = Rect::from_min_size(pos2(0.0, 0.0), vec2(128.0, 16.0));
        let mut r = Rig::new(cell(0, 0), one);
        r.select(RangeRef::new(CellRef::new(0, 0), CellRef::new(0, 1)), two);
        r.step(0.0);
        let mid = r.step(GLIDE_SECS / 2.0);
        assert_eq!(mid.min, one.min, "the anchored corner stays put");
        assert!(mid.max.x > one.max.x && mid.max.x < two.max.x && mid.max.y == two.max.y, "{mid:?}");
    }

    #[test]
    fn a_new_move_takes_over_from_where_the_outline_is() {
        let mut r = Rig::new(cell(0, 0), at(0.0, 0.0));
        r.select(cell(0, 1), at(64.0, 0.0));
        r.step(0.0);
        let mid = r.step(GLIDE_SECS / 3.0);
        assert!(mid.min.x > 0.0 && mid.min.x < 64.0);
        // The next arrow key lands before the first glide is over.
        r.select(cell(0, 2), at(128.0, 0.0));
        let next = r.step(0.0);
        assert!(near(next, mid), "no jump when the destination changes: {next:?} vs {mid:?}");
        assert_eq!(r.settle(DT).last(), Some(&at(128.0, 0.0)));
    }

    #[test]
    fn a_selection_changing_every_frame_keeps_up() {
        // A held arrow key: a new destination every frame. The outline moves in each frame that
        // shows one, instead of standing still until the changes stop.
        let mut r = Rig::new(cell(0, 0), at(0.0, 0.0));
        let mut last = r.glide.drawn();
        for col in 1..=30u16 {
            let target = at(64.0 * f32::from(col), 0.0);
            r.select(cell(0, u32::from(col)), target);
            let drawn = r.step(1.0 / 60.0);
            assert!(drawn.min.x > last.min.x, "frame {col}: stood still at {drawn:?}");
            assert!(target.min.x - drawn.min.x < 2.0 * 64.0, "frame {col}: {drawn:?} trails {target:?} by two cells or more");
            last = drawn;
        }
    }

    #[test]
    fn a_change_after_sitting_idle_still_glides() {
        // egui paints nothing while idle: the frame before the change can be long gone.
        let mut r = Rig::new(cell(0, 0), at(0.0, 0.0));
        r.select(cell(0, 1), at(64.0, 0.0));
        let first = r.step(5.0);
        assert!(first.min.x > 0.0 && first.min.x < 32.0, "{first:?}");
        assert!(r.glide.is_gliding());
        assert_eq!(r.settle(DT).last(), Some(&at(64.0, 0.0)));
    }

    #[test]
    fn the_same_cells_moving_on_screen_never_glide() {
        let mut r = Rig::new(cell(4, 0), at(0.0, 64.0));
        // Scrolled, zoomed, a column resized: same range, another rectangle.
        r.f.target = at(0.0, 16.0);
        assert_eq!(r.step(DT), at(0.0, 16.0));
        r.f.target = Rect::from_min_size(pos2(8.0, 24.0), vec2(96.0, 24.0));
        assert_eq!(r.step(DT), r.f.target);
        assert!(!r.glide.is_gliding());
        // Scrolling under a gliding outline carries it along with the sheet.
        r.select(cell(5, 0), at(0.0, 200.0));
        r.step(0.0);
        let before = r.step(GLIDE_SECS / 4.0);
        r.f.target = at(0.0, 300.0);
        let after = r.step(0.0);
        assert!(near(after, before.translate(vec2(0.0, 100.0))), "{after:?} vs {before:?}");
        // An arrow key held at the window's edge: the sheet scrolls and the next cell is drawn
        // where the last one was. Nothing to glide.
        let mut r = Rig::new(cell(30, 0), at(0.0, 580.0));
        r.select(cell(31, 0), at(0.0, 580.0));
        assert_eq!(r.step(DT), at(0.0, 580.0));
        assert!(!r.glide.is_gliding());
    }

    #[test]
    fn another_sheet_a_stale_or_off_screen_outline_appears_in_place() {
        // Another sheet, another workbook.
        let mut r = Rig::new(cell(0, 0), at(0.0, 0.0));
        r.f.place = (1, 1);
        r.select(cell(9, 9), at(500.0, 300.0));
        assert_eq!(r.step(DT), at(500.0, 300.0));
        r.f.place = (2, 1);
        r.select(cell(0, 0), at(0.0, 0.0));
        assert_eq!(r.step(DT), at(0.0, 0.0));
        assert!(!r.glide.is_gliding());
        // The outline wasn't drawn for a few frames: its old place means nothing now.
        r.f.frame_nr += 5;
        r.select(cell(0, 3), at(192.0, 0.0));
        assert_eq!(r.step(DT), at(192.0, 0.0));
        // It was scrolled out of sight, and the move brings the view back.
        let mut r = Rig::new(cell(500, 0), at(0.0, -8000.0));
        r.select(cell(501, 0), at(0.0, 300.0));
        assert_eq!(r.step(DT), at(0.0, 300.0));
        // The move goes somewhere off screen.
        r.select(cell(900, 0), at(0.0, 9000.0));
        assert_eq!(r.step(DT), at(0.0, 9000.0));
        assert!(!r.glide.is_gliding());
    }

    #[test]
    fn far_edges_set_off_no_further_than_one_view_away() {
        // A whole column: its bottom edge is millions of points below the window.
        let column = Rect::from_min_max(pos2(0.0, 0.0), pos2(64.0, 16_000_000.0));
        let mut r = Rig::new(RangeRef::cols(0, 0), column);
        r.select(cell(3, 0), at(0.0, 48.0));
        let first = r.step(0.0);
        assert_eq!(first.min, column.min);
        assert_eq!(first.max.y, 64.0 + VIEW.height(), "from the window's height below, not from row 1,048,576");
        assert_eq!(r.settle(DT).last(), Some(&at(0.0, 48.0)));
        // And back: the far edge leaves the same way.
        r.select(RangeRef::cols(0, 0), column);
        assert_eq!(r.step(0.0).max.y, 16_000_000.0 - VIEW.height());
        assert_eq!(r.settle(DT).last(), Some(&column));
    }

    #[test]
    fn turned_off_or_hostile_numbers_never_glide_or_panic() {
        // No animations this frame: on the cells, and a glide under way stops.
        let mut r = Rig::new(cell(0, 0), at(0.0, 0.0));
        r.f.animate = false;
        r.select(cell(0, 1), at(64.0, 0.0));
        assert_eq!(r.step(DT), at(64.0, 0.0));
        r.f.animate = true;
        r.select(cell(0, 2), at(128.0, 0.0));
        r.step(0.0);
        r.step(DT);
        assert!(r.glide.is_gliding());
        r.f.animate = false;
        assert_eq!(r.step(DT), at(128.0, 0.0));
        assert!(!r.glide.is_gliding());
        // Geometry and clocks that make no sense: whatever is drawn meanwhile, nothing panics,
        // and a sane selection afterwards is drawn on its cells once its glide is over.
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN, -1.0, 0.0] {
            for now in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0e300, 0.0] {
                let mut r = Rig::new(cell(0, 0), at(0.0, 0.0));
                r.f.view = Rect::from_min_max(pos2(v, 0.0), pos2(1000.0, v));
                r.select(cell(0, 1), Rect::from_min_max(pos2(v, v), pos2(64.0, 16.0)));
                r.step(DT);
                r.f.now = now;
                r.select(cell(0, 2), Rect::from_min_size(pos2(64.0, 0.0), egui::Vec2::splat(v)));
                r.step(DT);
                r.step(DT);
                r.f.view = VIEW;
                r.f.now = 50.0;
                r.select(cell(0, 3), at(192.0, 0.0));
                r.step(DT);
                assert_eq!(r.step(1.0), at(192.0, 0.0), "view {v}, clock {now}");
            }
        }
        assert_eq!((ease_out(-3.0), ease_out(0.0), ease_out(1.0), ease_out(7.0)), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn the_ants_march_by_the_clock() {
        assert_eq!(ants_phase(0.0), 0.0);
        assert!((ants_phase(0.06) - 0.5).abs() < 1e-4, "half a point every 60 ms");
        assert!((ants_phase(0.96 + 0.06) - 0.5).abs() < 1e-4, "wrapping with the dash pattern");
        for now in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -12.3, 1.0e300, 7.77] {
            let p = ants_phase(now);
            assert!((0.0..=8.0).contains(&p), "{now}: {p}");
        }
    }
}
