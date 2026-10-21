//! List drag and drop shared by the PivotTable Fields pane and the macro step editor: a drag
//! source that carries a payload, where a drop lands among a list's rows, the line that marks it,
//! and the label that follows the pointer.

use egui::{DragAndDrop, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::theme::{self, Tokens};

/// Where a drop at `y` lands among rows laid out top to bottom.
pub fn insert_index(rows: &[Rect], y: f32) -> usize {
    rows.iter().filter(|r| r.center().y < y).count()
}

/// Starts dragging `payload` when `r` (a widget sensing drags) begins a drag, and shows the grab
/// cursor over it.
pub fn drag_source<T: std::any::Any + Send + Sync>(ui: &Ui, r: &egui::Response, payload: T) {
    if r.drag_started() {
        DragAndDrop::set_payload(ui.ctx(), payload);
    }
    if r.hovered() && !DragAndDrop::has_any_payload(ui.ctx()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// A drag handle under a widget that only senses clicks (a checkbox): it covers where the widget
/// was last frame (stored under `id`) and is added first, so clicks still reach the widget on top.
pub fn drag_under<T: std::any::Any + Send + Sync>(ui: &mut Ui, id: Id, payload: T) {
    let Some(rect) = ui.ctx().data(|d| d.get_temp::<Rect>(id)) else { return };
    let r = ui.interact(rect, id, Sense::drag());
    drag_source(ui, &r, payload);
}

/// While dragging: outlines `zone` and draws the line where a drop at `y` would land among `rows`.
pub fn drop_marker(ui: &Ui, zone: Rect, rows: &[Rect], y: f32) {
    let t = Tokens::get(ui.ctx());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("dnd_drop_marker")));
    painter.rect_stroke(zone, 4.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    let k = insert_index(rows, y);
    let y = match (rows.get(k), k.checked_sub(1).and_then(|j| rows.get(j))) {
        (Some(r), _) => r.top() - 2.0,
        (None, Some(r)) => r.bottom() + 2.0,
        (None, None) => zone.top() + 4.0,
    };
    painter.line_segment([pos2(zone.left() + 4.0, y), pos2(zone.right() - 4.0, y)], Stroke::new(2.0, t.accent));
}

/// The dragged item's label following the pointer.
pub fn ghost(ui: &Ui, text: &str, at: Pos2) {
    let t = Tokens::get(ui.ctx());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("dnd_ghost")));
    let galley = painter.layout_no_wrap(text.to_string(), theme::ui_font(12.0), t.text);
    let rect = Rect::from_min_size(at + vec2(12.0, 8.0), galley.size() + vec2(12.0, 6.0));
    painter.rect_filled(rect, 3.0, t.input_bg);
    painter.rect_stroke(rect, 3.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(6.0, 3.0), galley, t.text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_lands_before_the_first_row_below_it() {
        let rows: Vec<Rect> = (0..3).map(|i| Rect::from_min_size(pos2(0.0, i as f32 * 20.0), vec2(100.0, 20.0))).collect();
        assert_eq!(insert_index(&rows, -5.0), 0);
        assert_eq!(insert_index(&rows, 15.0), 1);
        assert_eq!(insert_index(&rows, 45.0), 2);
        assert_eq!(insert_index(&rows, 500.0), 3);
        assert_eq!(insert_index(&[], 10.0), 0);
    }
}
