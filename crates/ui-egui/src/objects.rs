//! Selecting, resizing, deleting and opening floating objects (charts, pictures, shapes and
//! slicers). Painting and hit-testing live in `chartview`.

use egui::{CursorIcon, Pos2, Rect, Vec2};
use gridcraft_engine::model::{ObjKind, ObjRef, ShapeKind, Sheet};
use serde_json::json;

use crate::SheetApp;

/// One of the eight resize handles on a selected object's frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Handle {
    pub const ALL: [Handle; 8] =
        [Handle::TopLeft, Handle::Top, Handle::TopRight, Handle::Left, Handle::Right, Handle::BottomLeft, Handle::Bottom, Handle::BottomRight];

    /// Which edges the handle moves: -1 the left or top edge, 1 the right or bottom edge, 0 neither.
    pub fn sides(self) -> (i8, i8) {
        match self {
            Handle::TopLeft => (-1, -1),
            Handle::Top => (0, -1),
            Handle::TopRight => (1, -1),
            Handle::Left => (-1, 0),
            Handle::Right => (1, 0),
            Handle::BottomLeft => (-1, 1),
            Handle::Bottom => (0, 1),
            Handle::BottomRight => (1, 1),
        }
    }

    /// Where the handle sits on `r`.
    pub fn pos(self, r: Rect) -> Pos2 {
        let (sx, sy) = self.sides();
        let x = match sx {
            -1 => r.left(),
            1 => r.right(),
            _ => r.center().x,
        };
        let y = match sy {
            -1 => r.top(),
            1 => r.bottom(),
            _ => r.center().y,
        };
        Pos2::new(x, y)
    }

    /// The pointer shape over the handle (on a right-to-left sheet the diagonals swap, as the
    /// frame is mirrored on screen).
    pub fn cursor(self, rtl: bool) -> CursorIcon {
        match self {
            Handle::TopLeft | Handle::BottomRight if !rtl => CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft if rtl => CursorIcon::ResizeNwSe,
            Handle::TopLeft | Handle::BottomRight | Handle::TopRight | Handle::BottomLeft => CursorIcon::ResizeNeSw,
            Handle::Left | Handle::Right => CursorIcon::ResizeHorizontal,
            Handle::Top | Handle::Bottom => CursorIcon::ResizeVertical,
        }
    }
}

/// The handle of `r` under `p`: the nearest one within reach. Corners reach further than edge
/// midpoints, and the reach shrinks on small objects so that their middle still moves them.
pub fn handle_at(r: Rect, p: Pos2) -> Option<Handle> {
    let small = r.width().min(r.height());
    Handle::ALL
        .into_iter()
        .map(|h| {
            let (sx, sy) = h.sides();
            let reach = if sx != 0 && sy != 0 { 10.0f32 } else { 7.0 }.min((small / 3.0).max(4.0));
            (h, (h.pos(r) - p).length(), reach)
        })
        .filter(|(_, d, reach)| d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(h, ..)| h)
}

/// The smallest size (points) a resize leaves: lines and arrows may lie flat, every other object
/// keeps some body.
pub fn min_size(sh: &Sheet, o: ObjRef) -> Vec2 {
    let flat = o.kind == ObjKind::Shape && sh.shapes.iter().any(|s| s.id == o.id && matches!(s.kind, ShapeKind::Line | ShapeKind::Arrow));
    if flat { Vec2::new(1.0, 0.0) } else { Vec2::new(8.0, 8.0) }
}

/// The sheet-point rectangle `[x0, y0, x1, y1]` after dragging `handle` by `d` points. Only the
/// grabbed edges move; they never cross the opposite edge (each keeps `min` from it) or leave
/// the sheet's top-left corner.
pub fn resized(orig: [f32; 4], handle: Handle, d: Vec2, min: Vec2) -> [f32; 4] {
    let [mut x0, mut y0, mut x1, mut y1] = orig;
    let (sx, sy) = handle.sides();
    match sx {
        -1 => x0 = (x0 + d.x).min(x1 - min.x).max(0.0),
        1 => x1 = (x1 + d.x).max(x0 + min.x),
        _ => {}
    }
    match sy {
        -1 => y0 = (y0 + d.y).min(y1 - min.y).max(0.0),
        1 => y1 = (y1 + d.y).max(y0 + min.y),
        _ => {}
    }
    [x0, y0, x1, y1]
}

/// What double-clicking an object opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenAction {
    /// Edit a text box's text in place (`text_box.rs`).
    EditText(u32),
    /// The Format Chart pane.
    FormatChart(u32),
    /// A form control's editor: a button's text, else Format Control.
    EditControl(u32),
    /// Nothing beyond selecting it.
    Select,
}

pub fn open_action(sh: &Sheet, o: ObjRef) -> OpenAction {
    match o.kind {
        ObjKind::Chart => OpenAction::FormatChart(o.id),
        ObjKind::Shape if sh.shapes.iter().any(|s| s.id == o.id && s.kind == ShapeKind::TextBox) => OpenAction::EditText(o.id),
        ObjKind::Control => OpenAction::EditControl(o.id),
        ObjKind::Shape | ObjKind::Image | ObjKind::Slicer => OpenAction::Select,
    }
}

impl SheetApp {
    /// Double-click on an object: selects it and opens what edits it.
    pub fn open_object(&mut self, o: ObjRef) -> OpenAction {
        self.selected_object = Some(o);
        let action = self.session.active().and_then(|d| d.wb.active()).map_or(OpenAction::Select, |sh| open_action(sh, o));
        match action {
            OpenAction::EditText(id) => {
                self.begin_text_box_edit(id);
            }
            OpenAction::FormatChart(_) => self.grid.pane = Some("formatChart".into()),
            OpenAction::EditControl(id) => crate::control_ui::edit(self, id),
            OpenAction::Select => {}
        }
        action
    }

    /// The selected object's id when it is a chart (the Chart Design tab and Format Chart pane).
    pub fn selected_chart(&self) -> Option<u32> {
        self.selected_object.filter(|o| o.kind == ObjKind::Chart).map(|o| o.id)
    }

    /// Selects the object with id `id` on the active sheet, whatever its kind; `None` (or an id
    /// that is not there) clears the selection.
    pub fn select_object_id(&mut self, id: Option<u32>) {
        self.selected_object = id.and_then(|id| self.session.active().and_then(|d| d.wb.active()).and_then(|sh| sh.object_ref(id)));
    }

    /// Deletes the selected object (Delete and Backspace). False when nothing on the active
    /// sheet is selected, so the key clears cells instead.
    pub fn delete_selected_object(&mut self) -> bool {
        let Some(o) = self.selected_object.take() else { return false };
        if !self.session.active().and_then(|d| d.wb.active()).is_some_and(|sh| sh.has_object(o)) {
            return false;
        }
        self.run_or_alert("object.delete", json!({"kind": o.kind.name(), "id": o.id}));
        true
    }

    /// Drops the selection when its object is gone from the active sheet (deleted, undone, or
    /// another sheet or workbook became active).
    pub fn prune_selected_object(&mut self) {
        if let Some(o) = self.selected_object
            && !self.session.active().and_then(|d| d.wb.active()).is_some_and(|sh| sh.has_object(o))
        {
            self.selected_object = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn handles_sit_on_the_frame_and_corners_win() {
        let r = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 100.0));
        for h in Handle::ALL {
            assert_eq!(handle_at(r, h.pos(r)), Some(h));
        }
        assert_eq!(handle_at(r, r.center()), None);
        assert_eq!(handle_at(r, pos2(106.0, 106.0)), Some(Handle::TopLeft));
        // A tiny object keeps its middle for moving.
        let tiny = Rect::from_min_size(pos2(0.0, 0.0), vec2(12.0, 12.0));
        assert_eq!(handle_at(tiny, tiny.center()), None);
    }

    #[test]
    fn resizing_moves_only_the_grabbed_edges_and_never_inverts() {
        let o = [10.0, 20.0, 110.0, 70.0];
        let m = vec2(8.0, 8.0);
        assert_eq!(resized(o, Handle::BottomRight, vec2(5.0, 6.0), m), [10.0, 20.0, 115.0, 76.0]);
        assert_eq!(resized(o, Handle::TopLeft, vec2(5.0, 6.0), m), [15.0, 26.0, 110.0, 70.0]);
        assert_eq!(resized(o, Handle::Top, vec2(50.0, 6.0), m), [10.0, 26.0, 110.0, 70.0]);
        assert_eq!(resized(o, Handle::Right, vec2(-500.0, 6.0), m), [10.0, 20.0, 18.0, 70.0]);
        assert_eq!(resized(o, Handle::Left, vec2(-500.0, 0.0), m), [0.0, 20.0, 110.0, 70.0]);
        assert_eq!(resized(o, Handle::BottomLeft, vec2(500.0, 500.0), m), [102.0, 20.0, 110.0, 570.0]);
    }

    #[test]
    fn diagonal_cursors_swap_on_a_mirrored_sheet() {
        assert_eq!(Handle::BottomRight.cursor(false), CursorIcon::ResizeNwSe);
        assert_eq!(Handle::BottomRight.cursor(true), CursorIcon::ResizeNeSw);
        assert_eq!(Handle::TopRight.cursor(false), CursorIcon::ResizeNeSw);
        assert_eq!(Handle::TopRight.cursor(true), CursorIcon::ResizeNwSe);
        assert_eq!(Handle::Left.cursor(true), CursorIcon::ResizeHorizontal);
    }
}
