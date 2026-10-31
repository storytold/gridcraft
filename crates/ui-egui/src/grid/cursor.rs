use super::geo::Geo;
use super::interact::{OBJECT_RESIZE_RADIUS, col_edge, freeze_handles, hidden_col_edge, hidden_row_edge, on_border, row_edge};
use super::{CellRef, Color32, CursorIcon, Drag, Pos2, RangeRef, Rect, Sheet, SheetApp, Stroke, pos2, vec2};

/// Decide after interaction so drag start and release use the new state in the same frame.
/// Hover bookkeeping remains active while a page boundary owns the cursor.
pub(super) fn update_cursor(app: &mut SheetApp, ui: &egui::Ui, resp: &egui::Response, geo: &Geo, sh: &Sheet, page_break_cursor: bool) {
    if app.text_box_editor.is_some() || app.grid.hf_edit.is_some() {
        return;
    }
    let ctx = ui.ctx();
    let pointer = ctx.input(|i| i.pointer.clone());
    let pos = pointer.interact_pos().or(pointer.hover_pos()).map(|p| geo.mirror_pos(p));
    let in_cells = pos.is_some_and(|p| geo.in_cells(p));
    let (freeze_cols, freeze_rows) = freeze_handles(geo);
    // Hover bookkeeping also serves cell notes, independently of who owns the cursor.
    if resp.hovered()
        && let Some(p) = pos
        && !freeze_cols.contains(p)
        && !freeze_rows.contains(p)
        && !matches!(app.grid.drag, Drag::Freeze { .. })
    {
        app.grid.hover_cell = in_cells.then(|| geo.cell_at(sh, p));
    }
    if page_break_cursor {
        return;
    }
    // Draw tools own the cursor over cells, including while recording a stroke.
    if !app.session.draw_tool.is_empty() && in_cells {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
        return;
    }
    // A held drag owns the cursor even outside the grid; a press below egui's threshold still
    // has Drag::None and uses hover. A click can briefly leave a drag state after release.
    if resp.dragged()
        && pointer.primary_down()
        && let Some(cursor) = drag_cursor(&app.grid.drag)
    {
        ctx.set_cursor_icon(cursor);
        return;
    }
    if !resp.hovered() {
        return;
    }
    let Some(p) = pos else { return };
    hover_cursor(app, ctx, resp, &pointer, geo, sh, p);
}

/// The cursor a held drag shows, or `None` when the drag leaves it to hover.
fn drag_cursor(drag: &Drag) -> Option<CursorIcon> {
    match drag {
        Drag::None => None,
        Drag::Select => Some(CursorIcon::Cell),
        Drag::Rows(_) | Drag::Cols(_) => Some(CursorIcon::Default),
        Drag::Fill { .. } => Some(CursorIcon::Crosshair),
        Drag::Move { .. } | Drag::MoveCols { .. } | Drag::Object { resize: false, .. } => Some(CursorIcon::Move),
        Drag::ResizeCol { .. } => Some(CursorIcon::ResizeColumn),
        Drag::ResizeRow { .. } => Some(CursorIcon::ResizeRow),
        Drag::Freeze { cols, .. } => Some(if *cols { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical }),
        Drag::Object { resize: true, .. } => Some(CursorIcon::ResizeNwSe),
    }
}

/// The cursor over the grid when no drag owns it: freeze bars, header edges, the fill handle, cells.
fn hover_cursor(app: &SheetApp, ctx: &egui::Context, resp: &egui::Response, pointer: &egui::PointerState, geo: &Geo, sh: &Sheet, p: Pos2) {
    let (freeze_cols, freeze_rows) = freeze_handles(geo);
    if freeze_cols.contains(p) || freeze_rows.contains(p) {
        freeze_cursor(app, ctx, resp, pointer, freeze_cols.contains(p));
        return;
    }
    let in_cells = geo.in_cells(p);
    let in_col_header = p.y < geo.cells.top() && p.y >= geo.head_top() && p.x >= geo.cells.left();
    let in_row_header = p.x < geo.cells.left() && p.x >= geo.head_left() && p.y >= geo.cells.top();
    let near_col_edge = in_col_header && col_edge(geo, sh, p.x).is_some();
    let sel = app.session.active().map(|d| d.selection.clone()).unwrap_or_default();
    let cur_rect = geo.range_rect(sh, sel.current());
    let handle = Rect::from_center_size(cur_rect.right_bottom(), vec2(9.0, 9.0));
    let handle_panes = geo.range_panes(RangeRef::cell(sel.current().end));
    let border_panes = geo.range_panes(sel.current());
    let on_handle = handle.contains(p) && handle_panes.iter().any(|r| r.contains(p));
    let on_cur_border = on_border(cur_rect, p) && border_panes.iter().any(|r| r.contains(p));
    let selected_object = crate::chartview::hit(app, geo, sh, p).filter(|&(_, id, _)| app.selected_chart == Some(id));
    let near_corner = selected_object.is_some_and(|(_, _, rect)| (p - rect.right_bottom()).length() < OBJECT_RESIZE_RADIUS);
    if app.grid.drag == Drag::None && near_corner {
        ctx.set_cursor_icon(CursorIcon::ResizeNwSe);
    } else if near_col_edge || (in_row_header && row_edge(geo, sh, p.y).is_some()) {
        header_edge_cursor(app, ctx, pointer, geo, sh, p, near_col_edge);
    } else if on_handle && app.editor.is_none() {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
        if app.grid.drag == Drag::None && !pointer.any_down() {
            resp.clone().on_hover_text_at_pointer("Drag to fill cells. Drag from inside a cell to select a range.");
        }
    } else if in_cells && app.editor.is_none() && on_cur_border {
        ctx.set_cursor_icon(CursorIcon::Move);
    } else if in_cells {
        ctx.set_cursor_icon(CursorIcon::Cell);
    } else if in_col_header {
        let col = geo.col_at(sh, p.x);
        let selected = sel.ranges.len() == 1 && sel.current().is_full_cols() && sel.current().contains(CellRef::new(0, col));
        ctx.set_cursor_icon(if selected { CursorIcon::Move } else { CursorIcon::Default });
    }
}

/// The resize cursor for dragging a freeze bar.
fn freeze_cursor(app: &SheetApp, ctx: &egui::Context, resp: &egui::Response, pointer: &egui::PointerState, cols: bool) {
    ctx.set_cursor_icon(if cols { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical });
    if app.grid.drag == Drag::None && !pointer.any_down() {
        let tip = if cols { "Drag to freeze columns" } else { "Drag to freeze rows" };
        resp.clone().on_hover_text_at_pointer(crate::i18n::current(ctx).tr(tip));
    }
}

/// The resize cursor over a column or row header edge. Just past a hidden column or row, Excel
/// shows a split cursor: dragging or double-clicking there unhides it.
fn header_edge_cursor(app: &SheetApp, ctx: &egui::Context, pointer: &egui::PointerState, geo: &Geo, sh: &Sheet, p: Pos2, near_col_edge: bool) {
    let hidden =
        app.grid.drag == Drag::None && if near_col_edge { hidden_col_edge(geo, sh, p.x).is_some() } else { hidden_row_edge(geo, sh, p.y).is_some() };
    match pointer.hover_pos() {
        Some(at) if hidden => paint_split_cursor(ctx, at, near_col_edge),
        _ => ctx.set_cursor_icon(if near_col_edge { CursorIcon::ResizeColumn } else { CursorIcon::ResizeRow }),
    }
}

/// A split cursor (two bars with arrows either side) painted in place of the OS cursor, which
/// has no such shape. Black on a white outline, like the system cursors.
fn paint_split_cursor(ctx: &egui::Context, at: Pos2, cols: bool) {
    ctx.set_cursor_icon(CursorIcon::None);
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("gridcraft.split_cursor")));
    // Drawn for columns (bars vertical); rows swap the axes.
    let pt = |x: f32, y: f32| if cols { pos2(at.x + x, at.y + y) } else { pos2(at.x + y, at.y + x) };
    let arrow = |dir: f32| vec![pt(dir * 10.0, 0.0), pt(dir * 6.0, -4.0), pt(dir * 6.0, 4.0)];
    for (width, color) in [(3.0, Color32::WHITE), (1.0, Color32::BLACK)] {
        let stroke = Stroke::new(width, color);
        for bx in [-1.5, 1.5] {
            p.line_segment([pt(bx, -8.0), pt(bx, 8.0)], stroke);
        }
        p.line_segment([pt(-7.0, 0.0), pt(7.0, 0.0)], stroke);
        for dir in [-1.0, 1.0] {
            p.add(egui::Shape::convex_polygon(arrow(dir), color, Stroke::new(width - 1.0, color)));
        }
    }
}
