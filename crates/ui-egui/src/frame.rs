//! The selection frame around a floating object (chart, picture, shape, slicer): its eight
//! handles, the rotation handle above a shape, which one a press grabs, and the rect a handle
//! drag gives. A turned frame is the rect turned about its centre, `deg` clockwise.

use egui::{CursorIcon, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

/// Handle radius and the radius a press grabs a handle within (screen points).
pub const HANDLE_R: f32 = 4.0;
const GRAB_R: f32 = 10.0;
/// Frames smaller than this (screen points) show only corner handles on that axis.
const EDGE_HANDLES_FROM: f32 = 40.0;
/// How far the rotation handle sits above the frame's top edge (screen points), and the step
/// Shift snaps the angle to (degrees).
const ROTATE_GAP: f32 = 22.0;
pub const SNAP_DEG: f32 = 15.0;

/// Screen point `p` turned `deg` degrees clockwise about `c`.
pub fn turn(p: Pos2, c: Pos2, deg: f32) -> Pos2 {
    let (s, k) = deg.to_radians().sin_cos();
    let (u, v) = (p.x - c.x, p.y - c.y);
    pos2(c.x + u * k - v * s, c.y + u * s + v * k)
}

/// One of the eight handles: `x` / `y` are -1 (left/top), 0 (middle) or 1 (right/bottom).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    pub x: i8,
    pub y: i8,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle { x: -1, y: -1 },
        Handle { x: 0, y: -1 },
        Handle { x: 1, y: -1 },
        Handle { x: 1, y: 0 },
        Handle { x: 1, y: 1 },
        Handle { x: 0, y: 1 },
        Handle { x: -1, y: 1 },
        Handle { x: -1, y: 0 },
    ];
    pub fn is_corner(self) -> bool {
        self.x != 0 && self.y != 0
    }
    /// The resize cursor, picked for the handle's direction on a frame turned `deg`.
    pub fn cursor(self, deg: f32) -> CursorIcon {
        let a = f32::from(self.y).atan2(f32::from(self.x)).to_degrees() + deg;
        // Eight compass sectors of 45°; opposite sectors share a cursor.
        match ((a / 45.0).round() as i32).rem_euclid(4) {
            0 => CursorIcon::ResizeHorizontal,
            1 => CursorIcon::ResizeNwSe,
            2 => CursorIcon::ResizeVertical,
            _ => CursorIcon::ResizeNeSw,
        }
    }
    pub fn at(self, f: Rect) -> Pos2 {
        let k = |v: i8| (f32::from(v) + 1.0) / 2.0;
        pos2(f.min.x + f.width() * k(self.x), f.min.y + f.height() * k(self.y))
    }
    pub fn at_turned(self, f: Rect, deg: f32) -> Pos2 {
        turn(self.at(f), f.center(), deg)
    }
}

/// What a press on a selected object grabbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grab {
    Move,
    Resize(Handle),
    Rotate,
}

impl Grab {
    pub fn cursor(self, deg: f32) -> CursorIcon {
        match self {
            Grab::Move => CursorIcon::Move,
            Grab::Resize(h) => h.cursor(deg),
            Grab::Rotate => CursorIcon::Crosshair,
        }
    }
}

/// The handles a frame shows: corners always, edge middles when there's room.
pub fn handles(f: Rect) -> impl Iterator<Item = Handle> {
    let (wide, tall) = (f.width() >= EDGE_HANDLES_FROM, f.height() >= EDGE_HANDLES_FROM);
    Handle::ALL.into_iter().filter(move |h| (h.x != 0 || wide) && (h.y != 0 || tall))
}

/// Where the rotation handle of frame `f` turned `deg` is: above the middle of its top edge.
pub fn rotate_handle(f: Rect, deg: f32) -> Pos2 {
    turn(pos2(f.center().x, f.min.y - ROTATE_GAP), f.center(), deg)
}

/// The handle of frame `f` turned `deg` under screen point `p`; `rotate` offers the rotation
/// handle too.
pub fn grab_at(f: Rect, deg: f32, rotate: bool, p: Pos2) -> Option<Grab> {
    if rotate && rotate_handle(f, deg).distance(p) <= GRAB_R {
        return Some(Grab::Rotate);
    }
    let d = |h: &Handle| h.at_turned(f, deg).distance(p);
    handles(f).filter(|h| d(h) <= GRAB_R).min_by(|a, b| d(a).total_cmp(&d(b))).map(Grab::Resize)
}

/// The angle (degrees clockwise, `[0, 360)`) that points frame `f`'s rotation handle at `p`,
/// by whole degrees or, with `snap`, by [`SNAP_DEG`].
pub fn angle_to(f: Rect, p: Pos2, snap: bool) -> f32 {
    let d = p - f.center();
    // The handle is straight up (-90°) when unturned.
    let deg = d.y.atan2(d.x).to_degrees() + 90.0;
    let step = if snap { SNAP_DEG } else { 1.0 };
    ((deg / step).round() * step).rem_euclid(360.0)
}

/// Draw frame `f` turned `deg`: outline, handles and, with `rotate`, the rotation handle.
pub fn paint(p: &Painter, f: Rect, deg: f32, rotate: bool, resize: bool) {
    let line = Stroke::new(1.0, egui::Color32::from_gray(120));
    if deg == 0.0 {
        p.rect_stroke(f, 0.0, line, StrokeKind::Outside);
    } else {
        let c = f.center();
        let pts = [f.left_top(), f.right_top(), f.right_bottom(), f.left_bottom(), f.left_top()].map(|q| turn(q, c, deg));
        p.add(egui::Shape::line(pts.to_vec(), line));
    }
    let knob = |c: Pos2| {
        p.circle_filled(c, HANDLE_R, egui::Color32::WHITE);
        p.circle_stroke(c, HANDLE_R, Stroke::new(1.0, egui::Color32::from_gray(100)));
    };
    if rotate {
        let r = rotate_handle(f, deg);
        p.line_segment([turn(f.center_top(), f.center(), deg), r], line);
        knob(r);
        // A circling arrow: three quarters of a ring.
        let ring: Vec<Pos2> = (0..=12).map(|i| r + vec2((i as f32 * 0.4).cos(), (i as f32 * 0.4).sin()) * (HANDLE_R * 0.55)).collect();
        p.add(egui::Shape::line(ring, Stroke::new(1.0, egui::Color32::from_gray(80))));
    }
    if resize {
        for h in handles(f) {
            knob(h.at_turned(f, deg));
        }
    }
}

/// [`resized`] for a frame turned `deg`: the drag is taken along the frame's own axes, and the
/// handle opposite the grabbed one stays where it is.
pub fn resized_turned(r: Rect, deg: f32, h: Handle, d: egui::Vec2, min: f32, keep_aspect: bool) -> Rect {
    if deg == 0.0 {
        return resized(r, h, d, min, keep_aspect);
    }
    let local = turn(pos2(d.x, d.y), Pos2::ZERO, -deg).to_vec2();
    let n = resized(r, h, local, min, keep_aspect);
    let opp = Handle { x: -h.x, y: -h.y };
    n.translate(opp.at_turned(r, deg) - opp.at_turned(n, deg))
}

/// `r` with handle `h` dragged by `d`; the opposite edges stay put. Never smaller than `min`;
/// `keep_aspect` keeps the proportions when a corner is dragged.
pub fn resized(r: Rect, h: Handle, d: egui::Vec2, min: f32, keep_aspect: bool) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
    match h.x {
        -1 => x0 = (x0 + d.x).min(x1 - min),
        1 => x1 = (x1 + d.x).max(x0 + min),
        _ => {}
    }
    match h.y {
        -1 => y0 = (y0 + d.y).min(y1 - min),
        1 => y1 = (y1 + d.y).max(y0 + min),
        _ => {}
    }
    if keep_aspect && h.is_corner() && r.width() > 0.0 && r.height() > 0.0 {
        // Follow whichever side changed more.
        let (kx, ky) = ((x1 - x0) / r.width(), (y1 - y0) / r.height());
        let k = if (kx - 1.0).abs() >= (ky - 1.0).abs() { kx } else { ky };
        let k = k.max(min / r.width()).max(min / r.height());
        let (w, hh) = (r.width() * k, r.height() * k);
        if h.x < 0 {
            x0 = x1 - w;
        } else {
            x1 = x0 + w;
        }
        if h.y < 0 {
            y0 = y1 - hh;
        } else {
            y1 = y0 + hh;
        }
    }
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn r() -> Rect {
        Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 100.0))
    }

    #[test]
    fn the_left_handle_moves_only_the_left_edge() {
        let n = resized(r(), Handle { x: -1, y: 0 }, vec2(-30.0, 50.0), 20.0, false);
        assert_eq!(n, Rect::from_min_max(pos2(70.0, 100.0), pos2(300.0, 200.0)));
    }

    #[test]
    fn the_top_left_corner_never_crosses_the_opposite_edges() {
        let n = resized(r(), Handle { x: -1, y: -1 }, vec2(500.0, 500.0), 20.0, false);
        assert_eq!(n, Rect::from_min_max(pos2(280.0, 180.0), pos2(300.0, 200.0)));
    }

    #[test]
    fn shift_keeps_the_proportions() {
        let n = resized(r(), Handle { x: 1, y: 1 }, vec2(200.0, 0.0), 20.0, true);
        assert_eq!(n.size(), vec2(400.0, 200.0));
        assert_eq!(n.min, r().min);
    }

    #[test]
    fn small_frames_drop_the_edge_handles() {
        let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(30.0, 100.0));
        assert!(handles(small).all(|h| h.x != 0));
        assert_eq!(handles(r()).count(), 8);
    }

    #[test]
    fn cursors_follow_the_turned_handle() {
        let right = Handle { x: 1, y: 0 };
        assert_eq!(right.cursor(0.0), CursorIcon::ResizeHorizontal);
        assert_eq!(right.cursor(90.0), CursorIcon::ResizeVertical);
        assert_eq!(right.cursor(45.0), CursorIcon::ResizeNwSe);
        assert_eq!(Handle { x: 1, y: -1 }.cursor(0.0), CursorIcon::ResizeNeSw);
    }

    #[test]
    fn the_rotation_handle_points_at_the_pointer() {
        let c = r().center();
        assert_eq!(angle_to(r(), c + vec2(0.0, -50.0), false), 0.0);
        assert_eq!(angle_to(r(), c + vec2(50.0, 0.0), false), 90.0);
        assert_eq!(angle_to(r(), c + vec2(-50.0, 0.0), false), 270.0);
        assert_eq!(angle_to(r(), c + vec2(50.0, -40.0), true), 45.0);
        assert_eq!(grab_at(r(), 0.0, true, rotate_handle(r(), 0.0)), Some(Grab::Rotate));
        assert_eq!(grab_at(r(), 0.0, false, rotate_handle(r(), 0.0)), None);
    }

    #[test]
    fn a_turned_resize_keeps_the_opposite_handle_in_place() {
        let h = Handle { x: 1, y: 0 };
        let n = resized_turned(r(), 90.0, h, vec2(0.0, 40.0), 20.0, false);
        // Turned 90°, the right handle points down: a downward drag widens the frame.
        assert!((n.width() - 240.0).abs() < 0.01, "{n:?}");
        let opp = Handle { x: -1, y: 0 };
        assert!((opp.at_turned(n, 90.0) - opp.at_turned(r(), 90.0)).length() < 0.01);
    }
}
