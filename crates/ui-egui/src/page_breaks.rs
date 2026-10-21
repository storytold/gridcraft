//! Dragging print-area borders and page breaks in Page Break Preview.

use egui::{Color32, CursorIcon, Key, Modifiers, Pos2, Response, Stroke, Ui, pos2, vec2};
use gridcraft_engine::core::{MAX_COLS, MAX_ROWS, RangeRef};
use gridcraft_engine::model::{Sheet, SheetViewMode};
use serde_json::json;

use crate::{SheetApp, grid::Geo};

/// Pagination and full print area cached for a single document revision and active sheet.
pub struct PageCache {
    pub key: (u64, u64, usize),
    pub pages: Vec<(RangeRef, u32)>,
    pub area: RangeRef,
}

/// A logical grid boundary; outer edges resize the print area, inner edges move breaks.
#[derive(Clone, Copy)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    Column(u32),
    Row(u32),
}

impl Edge {
    /// Whether snapping and cursor feedback use the column axis.
    fn horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right | Self::Column(_))
    }
}

/// Transient drag state tied to one document revision and sheet; no workbook edits occur here.
pub struct BoundaryDrag {
    key: (u64, u64, usize),
    area: RangeRef,
    edge: Edge,
    original: u32,
    target: u32,
}

/// Finds a visible blue line within five points, giving the outer frame priority at corners.
fn hit(geo: &Geo, sh: &Sheet, area: RangeRef, pages: &[(RangeRef, u32)], p: Pos2) -> Option<(Edge, u32)> {
    if !geo.cells.contains(p) {
        return None;
    }
    let r = geo.range_rect(sh, area);
    let near = |a: f32, b: f32| (a - b).abs() <= 5.0;
    if p.y >= r.top() - 5.0 && p.y <= r.bottom() + 5.0 {
        if near(p.x, r.left()) {
            return Some((Edge::Left, area.start.col));
        }
        if near(p.x, r.right()) {
            return Some((Edge::Right, area.end.col + 1));
        }
        for (page, _) in pages {
            let line = page.end.col + 1;
            if line <= area.end.col && near(p.x, geo.x(sh, line)) {
                return Some((Edge::Column(line), line));
            }
        }
    }
    if p.x >= r.left() - 5.0 && p.x <= r.right() + 5.0 {
        if near(p.y, r.top()) {
            return Some((Edge::Top, area.start.row));
        }
        if near(p.y, r.bottom()) {
            return Some((Edge::Bottom, area.end.row + 1));
        }
        for (page, _) in pages {
            let line = page.end.row + 1;
            if line <= area.end.row && near(p.y, geo.y(sh, line)) {
                return Some((Edge::Row(line), line));
            }
        }
    }
    None
}

/// Snaps to the nearest cell edge, keeping a resized print area at least one cell wide and tall.
fn snap(d: &BoundaryDrag, geo: &Geo, sh: &Sheet, p: Pos2) -> u32 {
    let horizontal = d.edge.horizontal();
    let i = if horizontal { geo.col_at(sh, p.x) } else { geo.row_at(sh, p.y) };
    let (a, b, value) = if horizontal {
        (geo.x(sh, i), geo.x(sh, i) + sh.col_width(i) * geo.z, p.x)
    } else {
        (geo.y(sh, i), geo.y(sh, i) + sh.row_height(i) * geo.z, p.y)
    };
    let line = i.saturating_add(u32::from(value > (a + b) / 2.0));
    match d.edge {
        Edge::Left => line.min(d.area.end.col),
        Edge::Right => line.clamp(d.area.start.col + 1, MAX_COLS),
        Edge::Top => line.min(d.area.end.row),
        Edge::Bottom => line.clamp(d.area.start.row + 1, MAX_ROWS),
        Edge::Column(_) => line.min(MAX_COLS),
        Edge::Row(_) => line.min(MAX_ROWS),
    }
}

/// Sends one command on release; unrelated print settings and cell contents remain intact.
fn commit(app: &mut SheetApp, d: BoundaryDrag) {
    if d.target == d.original {
        return;
    }
    if let Edge::Column(from) | Edge::Row(from) = d.edge {
        app.run_or_alert(
            "pageLayout.movePageBreak",
            json!({"sheet": d.key.2,
            "axis": if d.edge.horizontal() { "column" } else { "row" }, "from": from + 1, "to": d.target + 1}),
        );
    } else {
        let mut area = d.area;
        match d.edge {
            Edge::Left => area.start.col = d.target,
            Edge::Right => area.end.col = d.target - 1,
            Edge::Top => area.start.row = d.target,
            Edge::Bottom => area.end.row = d.target - 1,
            _ => return,
        }
        app.run_or_alert("pageLayout.pageSetup", json!({"sheet": d.key.2, "printArea": area.to_string()}));
    }
}

/// Captures blue-boundary gestures before cell selection, previews snapping, and scrolls near edges.
/// Logical coordinates are mirrored for RTL sheets; stale drags and Escape never change the book.
pub fn interact(app: &mut SheetApp, ui: &mut Ui, resp: &Response, geo: &Geo, sh: &Sheet) -> bool {
    let key = app.session.active().map(|d| (d.uid, d.revision, d.wb.active_sheet));
    if sh.view != SheetViewMode::PageBreakPreview
        || app.dialog.is_some()
        || app.message.is_some()
        || app.editor.is_some()
        || app.text_box_editor.is_some()
        || app.grid.hf_edit.is_some()
        || (app.grid.page_break_drag.is_none() && app.grid.drag != crate::grid::Drag::None)
        || app.grid.page_break_drag.as_ref().is_some_and(|d| Some(d.key) != key)
    {
        app.grid.page_break_drag = None;
        return false;
    }
    let pointer = ui.input(|i| i.pointer.clone());
    if app.grid.page_break_drag.is_some() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        app.grid.page_break_drag = None;
        return true;
    }
    let Some(p) = pointer.interact_pos().or(pointer.hover_pos()).map(|p| geo.mirror_pos(p)) else {
        if !pointer.primary_down() {
            app.grid.page_break_drag = None;
        }
        return false;
    };
    if app.grid.page_break_drag.is_none() {
        let Some(cache) = &app.grid.pages else { return false };
        let pages = &cache.pages;
        if pages.is_empty() {
            return false;
        }
        let area = cache.area;
        let Some((edge, line)) = hit(geo, sh, area, pages, p) else { return false };
        if !resp.hovered() && !resp.contains_pointer() {
            return false;
        }
        ui.ctx().set_cursor_icon(if edge.horizontal() { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical });
        if pointer.primary_pressed()
            && let Some(key) = key
        {
            app.grid.page_break_drag = Some(BoundaryDrag { key, area, edge, original: line, target: line });
        } else {
            return !pointer.any_down() && !pointer.any_released();
        }
    }
    let Some(mut d) = app.grid.page_break_drag.take() else { return false };
    d.target = snap(&d, geo, sh, p);
    ui.ctx().set_cursor_icon(if d.edge.horizontal() { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical });
    let r = geo.range_rect(sh, d.area);
    let points = if d.edge.horizontal() {
        [pos2(geo.x(sh, d.target), r.top()), pos2(geo.x(sh, d.target), r.bottom())]
    } else {
        [pos2(r.left(), geo.y(sh, d.target)), pos2(r.right(), geo.y(sh, d.target))]
    };
    ui.painter()
        .with_clip_rect(geo.mirror_rect(geo.cells))
        .line_segment(points.map(|p| geo.mirror_pos(p)), Stroke::new(3.0, Color32::from_rgb(46, 111, 216)));
    if pointer.primary_released() {
        commit(app, d);
    } else if pointer.primary_down() {
        let delta = if d.edge.horizontal() {
            vec2(
                if p.x > geo.cells.right() - 20.0 {
                    24.0
                } else if p.x < geo.cells.left() + 20.0 {
                    -24.0
                } else {
                    0.0
                },
                0.0,
            )
        } else {
            vec2(
                0.0,
                if p.y > geo.cells.bottom() - 20.0 {
                    20.0
                } else if p.y < geo.cells.top() + 20.0 {
                    -20.0
                } else {
                    0.0
                },
            )
        };
        if delta != egui::Vec2::ZERO
            && let Some(view) = app.view_mut()
        {
            view.scroll = (view.scroll + delta).max(egui::Vec2::ZERO);
            ui.ctx().request_repaint();
        }
        app.grid.page_break_drag = Some(d);
    }
    true
}
