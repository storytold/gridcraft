//! Holding a trackpad swipe to the axis it starts along.

use egui::vec2;

/// Holds a trackpad swipe to the axis it starts along, so scrolling down a column doesn't drift
/// sideways (or along a row, up and down) with the small off-axis movement every swipe has. A
/// swipe within about 27° of an axis (twice as far along it as across, over its first few
/// points) moves only along that axis until the trackpad ends the gesture or starts another, or
/// egui gives the scroll action up after a pause ([`RAIL_PAUSE`]); a clearly diagonal swipe moves
/// freely. The rail acts only on gestures the trackpad reports with
/// a start and an end (macOS, where the glide after the fingers lift arrives as a gesture of its
/// own and picks its own axis; Wayland, except for the first gesture after a wheel), and only on
/// the gesture's own movement: what else the frame carries (a mouse wheel's notch, or what egui
/// is still smoothing of one) passes beside it. Where no gesture phases arrive (the web,
/// trackpads reported in lines on Windows and X11, mouse wheels) nothing says where a swipe
/// begins or ends, and the scroll passes untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollRail {
    /// A gesture is in progress: the trackpad reported its start and not yet its end.
    active: bool,
    /// How far the gesture has moved along each axis while its axis is being decided.
    travel: egui::Vec2,
    axis: RailAxis,
    /// A modifier holds the gesture's scroll to one axis for the rest of the gesture: egui keeps
    /// a gesture's modifiers from its start and after they're released, so the delta stays there.
    pass: bool,
    /// When the last wheel event came (egui's clock), and whether a trackpad sent it: egui gives
    /// a scroll action up after a pause (see [`ScrollRail::expire`]).
    last: f64,
    trackpad: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum RailAxis {
    #[default]
    Undecided,
    Vertical,
    Horizontal,
    Free,
}

/// Points of movement after which a gesture's axis is decided.
const RAIL_DECIDE: f32 = 8.0;
/// How many times further along an axis than across it a gesture must go to be held to that axis.
const RAIL_RATIO: f32 = 2.0;
/// egui ends a scroll action it has heard nothing of for this long (its `SCROLL_ACTION_TIMEOUT`:
/// some gestures never get their end), or for [`RAIL_RESTING`]
/// when a trackpad sent the last event (macOS names the source; fingers resting mid-gesture send
/// nothing). From then on it takes the modifiers afresh and smooths the larger events as a mouse
/// wheel's notches, so the rail ends its gesture with it: steering the smoothed movement by the
/// events' own would move the scroll across the axis.
const RAIL_PAUSE: f64 = 0.150;
const RAIL_RESTING: f64 = 1.0;

impl ScrollRail {
    /// The trackpad reported the start of a gesture (it can come in a frame with no movement yet).
    /// `forced` when the start carries a modifier that holds the scroll to one axis.
    pub fn begin(&mut self, forced: bool) {
        *self = ScrollRail { active: true, pass: forced, last: self.last, trackpad: self.trackpad, ..ScrollRail::default() };
    }

    /// The trackpad reported the end of a gesture: egui has zeroed what it was smoothing of it,
    /// and what follows without a start of its own isn't a gesture's.
    pub fn end(&mut self) {
        *self = ScrollRail { last: self.last, trackpad: self.trackpad, ..ScrollRail::default() };
    }

    /// A wheel event came at `time` (egui's clock); `trackpad` when a trackpad sent it.
    pub fn heard(&mut self, time: f64, trackpad: bool) {
        self.last = time;
        self.trackpad = trackpad;
    }

    /// Ends the gesture once egui has given the scroll action up: no wheel event for
    /// [`RAIL_PAUSE`], or for [`RAIL_RESTING`] since a trackpad's. Called at the start of each
    /// frame, before its events: egui checks before each event it takes (a frame's events share
    /// its time) and after a frame without any.
    pub fn expire(&mut self, time: f64) {
        let pause = if self.trackpad { RAIL_RESTING } else { RAIL_PAUSE };
        if self.active && time - self.last > pause {
            self.end();
        }
    }

    /// One of the gesture's wheel events carries a modifier that holds the scroll to one axis:
    /// egui keeps it for the gesture and moves the scroll onto that axis, so the rail lets the
    /// rest of the gesture pass. (Outside a gesture nothing is steered, and `begin` starts over.)
    pub fn hold(&mut self) {
        self.pass = true;
    }

    /// The part of `d` (one frame's scroll) to apply. `own` is what the gesture's events moved this
    /// frame, which egui adds to `d` as it is while a gesture is in progress: that part goes along
    /// the gesture's axis only, or whole for a diagonal gesture, and the rest of `d` (a mouse
    /// wheel's notch, or what egui is still smoothing of one) passes beside it. All of `d` passes
    /// when no gesture is in progress, or when a modifier holds the gesture (`begin`, `hold`):
    /// egui has moved the scroll onto one axis already, so the reported axis would be the wrong
    /// one.
    pub fn steer(&mut self, d: egui::Vec2, own: egui::Vec2) -> egui::Vec2 {
        if !self.active || self.pass {
            return d;
        }
        let rest = d - own;
        if self.axis == RailAxis::Undecided {
            self.travel += own.abs();
            let t = self.travel;
            if t.length() >= RAIL_DECIDE {
                self.axis = if t.y >= RAIL_RATIO * t.x {
                    RailAxis::Vertical
                } else if t.x >= RAIL_RATIO * t.y {
                    RailAxis::Horizontal
                } else {
                    RailAxis::Free
                };
            }
        }
        let held = match self.axis {
            RailAxis::Vertical => vec2(0.0, own.y),
            RailAxis::Horizontal => vec2(own.x, 0.0),
            RailAxis::Free => own,
            // Until it's decided, the gesture moves along an axis it already clearly follows, and
            // both ways otherwise.
            RailAxis::Undecided if self.travel.y >= RAIL_RATIO * self.travel.x => vec2(0.0, own.y),
            RailAxis::Undecided if self.travel.x >= RAIL_RATIO * self.travel.y => vec2(own.x, 0.0),
            RailAxis::Undecided => own,
        };
        held + rest
    }
}
