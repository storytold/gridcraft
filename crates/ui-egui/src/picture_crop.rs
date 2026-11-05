//! Crop mode on a floating picture, as in Excel: Picture Format › Crop (or the picture's menu)
//! puts black crop handles on the selected picture's frame. Dragging a handle moves that edge of
//! the frame while the picture stays where it is on the sheet, so the edge cuts into it (dragged
//! back out, it shows what was cut, never past the picture's own edge); dragging inside the frame
//! moves the picture under it. The cut-off parts show dimmed around the frame. Enter, Escape,
//! Crop again or a click elsewhere end it. Each drag is one `picture.crop` undo step.

use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Image, Sheet};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::grid::geo::Geo;

/// Handles 0..8 (NW, N, NE, E, SE, S, SW, W); [`PAN`] is a drag inside the frame.
pub(crate) const PAN: u8 = 8;
/// The smallest frame a crop leaves (sheet points).
const MIN_SIDE: f32 = 4.0;
/// The most of the picture one side may cut off.
const MAX_SIDE: f32 = 0.95;
/// How near a crop handle a press has to be (screen points).
const GRAB: f32 = 8.0;

/// A frame in sheet points: x, y, width, height.
pub(crate) type Frame = (f32, f32, f32, f32);

/// The whole picture under frame `f` cropped `crop` (sheet points).
fn whole(f: Frame, crop: [f32; 4]) -> Frame {
    let w = f.2 / (1.0 - crop[0] - crop[2]).max(0.01);
    let h = f.3 / (1.0 - crop[1] - crop[3]).max(0.01);
    (f.0 - crop[0] * w, f.1 - crop[1] * h, w, h)
}

/// The crop that frame `f` makes of picture `pic` (both sheet points).
fn crop_of(f: Frame, pic: Frame) -> [f32; 4] {
    let c = [(f.0 - pic.0) / pic.2, (f.1 - pic.1) / pic.3, (pic.0 + pic.2 - f.0 - f.2) / pic.2, (pic.1 + pic.3 - f.1 - f.3) / pic.3];
    c.map(|v| if v.is_finite() { v.clamp(0.0, MAX_SIDE) } else { 0.0 })
}

/// Frame and crop after dragging crop handle `handle` of frame `f` (cropped `crop`) by `(dx, dy)`
/// sheet points. The picture stays put; the frame stays inside it and at least [`MIN_SIDE`].
pub(crate) fn crop_drag(f: Frame, crop: [f32; 4], handle: u8, (dx, dy): (f32, f32)) -> (Frame, [f32; 4]) {
    let pic = whole(f, crop);
    let (dx, dy) = (if dx.is_finite() { dx } else { 0.0 }, if dy.is_finite() { dy } else { 0.0 });
    if handle == PAN {
        // The picture moves under the frame, which keeps covering it.
        let px = (pic.0 + dx).clamp(f.0 + f.2 - pic.2, f.0);
        let py = (pic.1 + dy).clamp(f.1 + f.3 - pic.3, f.1);
        return (f, crop_of(f, (px, py, pic.2, pic.3)));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f.0, f.1, f.0 + f.2, f.1 + f.3);
    let (left, top, right, bottom) = (matches!(handle, 0 | 6 | 7), matches!(handle, 0..=2), matches!(handle, 2..=4), matches!(handle, 4..=6));
    // Each edge between the picture's own edge and the opposite one.
    let (min_w, min_h) = (MIN_SIDE.max(pic.2 * (1.0 - 2.0 * MAX_SIDE).max(0.0)), MIN_SIDE.max(pic.3 * (1.0 - 2.0 * MAX_SIDE).max(0.0)));
    if left {
        x0 = (x0 + dx).clamp(pic.0, (x1 - min_w).max(pic.0));
    }
    if right {
        x1 = (x1 + dx).clamp((x0 + min_w).min(pic.0 + pic.2), pic.0 + pic.2);
    }
    if top {
        y0 = (y0 + dy).clamp(pic.1, (y1 - min_h).max(pic.1));
    }
    if bottom {
        y1 = (y1 + dy).clamp((y0 + min_h).min(pic.1 + pic.3), pic.1 + pic.3);
    }
    let nf = (x0, y0, x1 - x0, y1 - y0);
    (nf, crop_of(nf, pic))
}

/// The picture in crop mode: the selected picture, while crop mode is on for it.
pub(crate) fn cropping<'a>(app: &SheetApp, sh: &'a Sheet) -> Option<&'a Image> {
    let id = app.crop.filter(|id| app.selected_image() == Some(*id))?;
    sh.images.iter().find(|im| im.id == id)
}

/// Crop mode on for the selected picture, or off. `false` when no picture is selected.
pub(crate) fn toggle(app: &mut SheetApp) -> bool {
    if app.crop.take().is_some() {
        return true;
    }
    let Some(sh) = app.session.active().and_then(|d| d.wb.active()) else { return false };
    let Some(id) = app.selected_image().filter(|id| sh.images.iter().any(|im| im.id == *id)) else { return false };
    app.crop = Some(id);
    true
}

/// The frame of picture `im` in sheet points.
pub(crate) fn frame_of(sh: &Sheet, im: &Image) -> Frame {
    let a = &im.anchor;
    (sh.col_left(a.cell.col) as f32 + a.dx, sh.row_top(a.cell.row) as f32 + a.dy, a.width, a.height.max(1.0))
}

/// The handle of the frame `r` (screen) under `p`, nearest first, else [`PAN`] inside it.
pub(crate) fn handle_at(r: Rect, p: Pos2) -> Option<u8> {
    let pts = handles(r);
    let near = pts.iter().enumerate().map(|(i, q)| (i, q.distance(p))).filter(|(_, d)| *d <= GRAB).min_by(|a, b| a.1.total_cmp(&b.1));
    match near {
        Some((i, _)) => Some(i as u8),
        None if r.contains(p) => Some(PAN),
        None => None,
    }
}

/// The 8 handle points of `r`: NW, N, NE, E, SE, S, SW, W.
fn handles(r: Rect) -> [Pos2; 8] {
    let (c, mn, mx) = (r.center(), r.min, r.max);
    [mn, pos2(c.x, mn.y), pos2(mx.x, mn.y), pos2(mx.x, c.y), mx, pos2(c.x, mx.y), pos2(mn.x, mx.y), pos2(mn.x, c.y)]
}

/// `picture.crop` params putting picture `id` in frame `f` (sheet points) with `crop`.
pub(crate) fn params(sh: &Sheet, id: u32, f: Frame, crop: [f32; 4]) -> Json {
    let (x, y) = (f.0.max(0.0), f.1.max(0.0));
    let at = CellRef::new(sh.row_at(f64::from(y)), sh.col_at(f64::from(x)));
    json!({
        "id": id, "left": crop[0], "top": crop[1], "right": crop[2], "bottom": crop[3],
        "at": at.a1(), "dx": x - sh.col_left(at.col) as f32, "dy": y - sh.row_top(at.row) as f32,
        "width": f.2, "height": f.3,
    })
}

/// The cursor over crop handle `h`.
pub(crate) fn cursor(h: u8) -> egui::CursorIcon {
    match h {
        0 | 4 => egui::CursorIcon::ResizeNwSe,
        2 | 6 => egui::CursorIcon::ResizeNeSw,
        1 | 5 => egui::CursorIcon::ResizeVertical,
        3 | 7 => egui::CursorIcon::ResizeHorizontal,
        _ => egui::CursorIcon::Move,
    }
}

/// Crop mode over picture `im` framed `r` on screen: the whole picture dimmed around the frame
/// (`tex`, when it is drawn), its outline, and the black crop marks.
pub(crate) fn paint(p: &Painter, r: Rect, im: &Image, tex: Option<&egui::TextureHandle>) {
    let (lt, rb) = im.visible_uv();
    let (vw, vh) = ((rb[0] - lt[0]).max(0.01), (rb[1] - lt[1]).max(0.01));
    let (w, h) = (r.width() / vw, r.height() / vh);
    let full = Rect::from_min_size(pos2(r.min.x - lt[0] * w, r.min.y - lt[1] * h), vec2(w, h));
    let dim = Color32::from_rgba_unmultiplied(30, 30, 30, 120);
    let bands = [
        Rect::from_min_max(full.min, pos2(full.max.x, r.min.y)),
        Rect::from_min_max(pos2(full.min.x, r.max.y), full.max),
        Rect::from_min_max(pos2(full.min.x, r.min.y), pos2(r.min.x, r.max.y)),
        Rect::from_min_max(pos2(r.max.x, r.min.y), pos2(full.max.x, r.max.y)),
    ];
    let uv = |b: Rect| {
        Rect::from_min_max(pos2((b.min.x - full.min.x) / w, (b.min.y - full.min.y) / h), pos2((b.max.x - full.min.x) / w, (b.max.y - full.min.y) / h))
    };
    for b in bands.into_iter().filter(|b| b.width() > 0.5 && b.height() > 0.5) {
        if let Some(t) = tex {
            p.image(t.id(), b, uv(b), Color32::WHITE);
        }
        p.rect_filled(b, 0.0, dim);
    }
    p.rect_stroke(full, 0.0, Stroke::new(1.0, dim), egui::StrokeKind::Outside);
    p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Inside);
    // An L at each corner, a bar mid-edge, black with a white rim.
    let mark = |a: Pos2, b: Pos2| {
        p.line_segment([a, b], Stroke::new(6.0, Color32::WHITE));
        p.line_segment([a, b], Stroke::new(4.0, Color32::BLACK));
    };
    let arm = 12.0_f32.min(r.width() / 3.0).min(r.height() / 3.0).max(3.0);
    let (mn, mx, c) = (r.min, r.max, r.center());
    for (corner, sx, sy) in [(mn, 1.0, 1.0), (pos2(mx.x, mn.y), -1.0, 1.0), (mx, -1.0, -1.0), (pos2(mn.x, mx.y), 1.0, -1.0)] {
        mark(corner, corner + vec2(sx * arm, 0.0));
        mark(corner, corner + vec2(0.0, sy * arm));
    }
    for (mid, horizontal) in [(pos2(c.x, mn.y), true), (pos2(c.x, mx.y), true), (pos2(mn.x, c.y), false), (pos2(mx.x, c.y), false)] {
        let d = if horizontal { vec2(arm * 0.7, 0.0) } else { vec2(0.0, arm * 0.7) };
        mark(mid - d, mid + d);
    }
}

/// Presses in crop mode (call before the usual object handling). `true`: the press was crop
/// mode's (a crop drag started, or a click inside the picture). A press elsewhere, Enter or Escape
/// end crop mode; a press elsewhere then goes on as usual (`false`).
pub(crate) fn press(app: &mut SheetApp, ctx: &egui::Context, geo: &Geo, sh: &Sheet, at: Option<Pos2>, started_drag: bool, clicked: bool) -> bool {
    let Some(im) = cropping(app, sh) else {
        app.crop = None;
        return false;
    };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
        app.crop = None;
        return false;
    }
    if !(started_drag || clicked) {
        return false;
    }
    let Some(p) = at else { return false };
    let r = crate::chartview::anchor_rect(geo, sh, &im.anchor);
    let Some(handle) = handle_at(r, p) else {
        app.crop = None;
        return false;
    };
    if started_drag {
        let (id, frame, crop) = (im.id, frame_of(sh, im), im.crop);
        app.grid.drag = crate::grid::Drag::Crop { id, handle, start: p, frame, crop };
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn an_edge_cuts_into_the_picture_which_stays_put() {
        // A 200 × 100 picture; the right edge in by 50: a quarter cut off.
        let (f, c) = crop_drag((100.0, 50.0, 200.0, 100.0), [0.0; 4], 3, (-50.0, 0.0));
        assert!(close(f.0, 100.0) && close(f.2, 150.0) && close(c[2], 0.25), "{f:?} {c:?}");
        // Then the top-left corner in by 20 × 10.
        let (g, d) = crop_drag(f, c, 0, (20.0, 10.0));
        assert!(close(g.0, 120.0) && close(g.1, 60.0) && close(g.2, 130.0) && close(g.3, 90.0), "{g:?}");
        assert!(close(d[0], 0.1) && close(d[1], 0.1) && close(d[2], 0.25) && close(d[3], 0.0), "{d:?}");
        // The whole picture is where it was.
        let w = whole(g, d);
        assert!(close(w.0, 100.0) && close(w.1, 50.0) && close(w.2, 200.0) && close(w.3, 100.0), "{w:?}");
    }

    #[test]
    fn edges_stop_at_the_picture_and_short_of_each_other() {
        let f = (0.0, 0.0, 100.0, 100.0);
        // Out past the picture's own edge: it stops there.
        let (g, c) = crop_drag(f, [0.2, 0.0, 0.0, 0.0], 7, (-500.0, 0.0));
        assert!(close(c[0], 0.0) && close(g.2, 125.0), "{g:?} {c:?}");
        // In past the other edge: a sliver stays.
        let (g, c) = crop_drag(f, [0.0; 4], 7, (500.0, 0.0));
        assert!(g.2 >= MIN_SIDE - 1e-3 && c[0] <= MAX_SIDE, "{g:?} {c:?}");
    }

    #[test]
    fn a_drag_inside_moves_the_picture_under_the_frame() {
        let f = (0.0, 0.0, 100.0, 100.0);
        // 200 wide, 25 % cut each side; 20 right shows 10 % less cut on the left.
        let (g, c) = crop_drag(f, [0.25, 0.0, 0.25, 0.0], PAN, (20.0, 0.0));
        assert_eq!(g, f);
        assert!(close(c[0], 0.15) && close(c[2], 0.35), "{c:?}");
        // Never so far that the frame shows past the picture.
        let (_, c) = crop_drag(f, [0.25, 0.0, 0.25, 0.0], PAN, (500.0, 0.0));
        assert!(close(c[0], 0.0) && close(c[2], 0.5), "{c:?}");
    }

    #[test]
    fn the_nearest_handle_wins_on_a_small_picture() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(6.0, 6.0));
        assert_eq!(handle_at(r, pos2(6.0, 3.0)), Some(3));
        assert_eq!(handle_at(r, pos2(0.0, 0.0)), Some(0));
        let big = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        assert_eq!(handle_at(big, pos2(50.0, 50.0)), Some(PAN));
        assert_eq!(handle_at(big, pos2(300.0, 50.0)), None);
    }
}
