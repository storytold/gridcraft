use super::editor::validation_arrow;
use super::geo::Geo;
use super::keyboard::keyboard;
use super::menus::set_fill_tag;
use super::selection::select_ranges;
use super::tables::{is_filter_button, table_headings};
use super::{
    CellRef, Color32, CursorIcon, Drag, EditState, MAX_COLS, MAX_ROWS, Painter, Pos2, RangeRef, Rect, Sheet, SheetApp, Stroke, Tokens, Workbook,
    json, pos2, vec2,
};

// ------------------------------------------------------------------ interaction

/// How close (in points) a press must be to an object's bottom-right corner to resize it instead of moving it.
pub(super) const OBJECT_RESIZE_RADIUS: f32 = 10.0;

struct InteractionInput<'a> {
    ui: &'a mut egui::Ui,
    response: &'a egui::Response,
    geo: &'a Geo,
    sheet: &'a Sheet,
    workbook: &'a Workbook,
    context: egui::Context,
    press_origin: Option<Pos2>,
    any_pointer_down: bool,
    modifiers: egui::Modifiers,
    position: Option<Pos2>,
    primary_clicked: bool,
    in_cells: bool,
    in_column_header: bool,
    in_row_header: bool,
    in_corner: bool,
    selection: gridcraft_engine::Selection,
    current_rect: Rect,
    handle: Rect,
    handle_panes: Vec<Rect>,
    border_panes: Vec<Rect>,
    freeze_columns: Rect,
    freeze_rows: Rect,
}

impl InteractionInput<'_> {
    fn on_handle(&self, p: Pos2) -> bool {
        self.handle.contains(p) && self.handle_panes.iter().any(|r| r.contains(p))
    }

    fn on_current_border(&self, p: Pos2) -> bool {
        on_border(self.current_rect, p) && self.border_panes.iter().any(|r| r.contains(p))
    }
}

pub(super) fn interact(app: &mut SheetApp, ui: &mut egui::Ui, resp: &egui::Response, geo: &Geo, sh: &Sheet, wb: &Workbook) {
    // Text selection and editing must not also move the object or edit worksheet cells.
    if app.text_box_editor.is_some() || app.grid.hf_edit.is_some() {
        return;
    }
    let ctx = ui.ctx().clone();
    // Enter/Space synthesize egui clicks; only real pointer clicks use sheet coordinates.
    let primary_clicked = resp.clicked_by(egui::PointerButton::Primary);
    let pointer = ctx.input(|i| i.pointer.clone());
    let mods = ctx.input(|i| i.modifiers);
    // Hit-testing works in logical coordinates (see `Geo::rtl`).
    let pos = pointer.interact_pos().or(pointer.hover_pos()).map(|p| geo.mirror_pos(p));
    let in_cells = pos.is_some_and(|p| geo.in_cells(p));
    let in_col_header = pos.is_some_and(|p| p.y < geo.cells.top() && p.y >= geo.head_top() && p.x >= geo.cells.left());
    let in_row_header = pos.is_some_and(|p| p.x < geo.cells.left() && p.x >= geo.head_left() && p.y >= geo.cells.top());
    let in_corner = pos.is_some_and(|p| p.x < geo.cells.left() && p.y < geo.cells.top() && p.x >= geo.head_left() && p.y >= geo.head_top());
    let sel = app.session.active().map(|d| d.selection.clone()).unwrap_or_default();
    let cur_rect = geo.range_rect(sh, sel.current());
    let handle = Rect::from_center_size(cur_rect.right_bottom(), vec2(9.0, 9.0));
    // The fill handle and the move border only react in the panes where they are painted, not
    // over the frozen rows/columns that a scrolled selection lies under.
    let handle_panes = geo.range_panes(RangeRef::cell(sel.current().end));
    let border_panes = geo.range_panes(sel.current());
    let (freeze_cols, freeze_rows) = freeze_handles(geo);
    let input = InteractionInput {
        ui,
        response: resp,
        geo,
        sheet: sh,
        workbook: wb,
        context: ctx.clone(),
        press_origin: pointer.press_origin(),
        any_pointer_down: pointer.any_down(),
        modifiers: mods,
        position: pos,
        primary_clicked,
        in_cells,
        in_column_header: in_col_header,
        in_row_header,
        in_corner,
        selection: sel.clone(),
        current_rect: cur_rect,
        handle,
        handle_panes,
        border_panes,
        freeze_columns: freeze_cols,
        freeze_rows,
    };
    if handle_draw_tool(app, &input) {
        return;
    }
    // A plain click on a slicer value tile toggles it (Ctrl/Cmd for multi-select); the clear
    // button resets it. Runs before the generic object select/drag so tiles stay clickable.
    if primary_clicked
        && !resp.drag_started()
        && let Some(p) = pos
        && let Some((id, act)) = crate::chartview::slicer_click(geo, sh, wb, wb.active_sheet(), p)
    {
        app.selected_chart = Some(id);
        match act {
            crate::chartview::SlicerAction::Toggle(value) => {
                let _ = app.run("slicer.toggle", json!({"slicer": id, "value": value, "multi": mods.command || mods.ctrl}));
            }
            crate::chartview::SlicerAction::Clear => {
                let _ = app.run("slicer.clear", json!({"slicer": id}));
            }
        }
        return;
    }
    // Objects (charts, pictures, shapes, slicers) take clicks first. Only the primary button moves
    // them: a right-click that wobbles into a drag still opens the object's menu (#2067).
    let primary_drag = resp.drag_started_by(egui::PointerButton::Primary);
    if primary_drag || primary_clicked {
        // A drag is recognised only after the pointer has moved, so grab the object where the press began.
        let at = if primary_drag { pointer.press_origin().or(pos) } else { pos };
        if let Some(p) = at
            && let Some((kind, id, rect)) = crate::chartview::hit(app, geo, sh, p)
        {
            app.selected_chart = Some(id);
            if kind == "shape"
                && resp.double_clicked()
                && sh.shapes.iter().any(|s| s.id == id && s.kind == gridcraft_engine::model::ShapeKind::TextBox)
            {
                app.begin_text_box_edit(id);
                return;
            }
            if kind == "chart" && resp.double_clicked() {
                app.grid.pane = Some("formatChart".into());
            }
            if primary_drag {
                let a = object_anchor(sh, kind, id);
                let resize = (p - rect.right_bottom()).length() < OBJECT_RESIZE_RADIUS;
                if let Some(a) = a {
                    app.grid.drag = Drag::Object { kind, id, start: p, orig: (a.cell, a.dx, a.dy, a.width, a.height), resize };
                }
            }
            return;
        }
        app.selected_chart = None;
    }
    // A right-click on a chart, picture, shape or slicer opens the object's menu, not the cell's
    // under it; a right press that wobbles into a drag counts too (#2067).
    let secondary_drag = resp.drag_started_by(egui::PointerButton::Secondary);
    if (resp.secondary_clicked() || secondary_drag)
        && let Some(p) = if secondary_drag { pointer.press_origin().map(|p| geo.mirror_pos(p)).or(pos) } else { pos }
        && let Some((kind, id, _)) = crate::chartview::hit(app, geo, sh, p)
    {
        app.selected_chart = Some(id);
        app.grid.object_menu = Some((geo.mirror_pos(p), kind, id));
        return;
    }

    if handle_header_autofit(app, &input) {
        return;
    }

    // Double-click the fill handle: a double-click is also a click, so run it before the handle press returns.
    if resp.double_clicked()
        && let Some(p) = pos
        && input.on_handle(p)
    {
        // Fill down as far as the neighbouring column goes.
        let src = sel.current();
        let neighbour = if src.start.col > 0 { src.start.col - 1 } else { src.end.col + 1 };
        let cap = src.end.row.saturating_add(1_000_000).min(MAX_ROWS - 1);
        let last = if src.end.row < cap { sh.last_contiguous_nonempty(neighbour, src.end.row + 1, cap) } else { src.end.row };
        if last > src.end.row {
            let target = RangeRef::new(src.start, CellRef::new(last, src.end.col));
            let _ = app.run("edit.autoFill", json!({"source": src.a1(), "target": target.a1()}));
        }
        app.grid.drag = Drag::None;
        app.grid.drag_select = false;
        resp.request_focus();
        return;
    }

    // Press: choose the interaction mode before pointer movement is applied.
    if resp.drag_started() || (primary_clicked && app.grid.drag == Drag::None) {
        handle_press(app, &input);
        return;
    }

    handle_dragging(app, &input);

    handle_release(app, &input);

    handle_context_menu_and_keyboard(app, &input);
}

fn handle_draw_tool(app: &mut SheetApp, input: &InteractionInput<'_>) -> bool {
    let (in_cells, pos, resp, geo, sh, primary_clicked) =
        (input.in_cells, input.position, input.response, input.geo, input.sheet, input.primary_clicked);
    let ctx = input.context.clone();
    if !app.session.draw_tool.is_empty() && in_cells {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
        if app.session.draw_tool == "pen" {
            if let Some(p) = pos
                && resp.dragged()
            {
                // Through the cell under the pointer, so frozen panes and pages map right.
                let c = geo.cell_at(sh, p);
                let sx = sh.col_left(c.col) as f32 + (p.x - geo.x(sh, c.col)) / geo.z;
                let sy = sh.row_top(c.row) as f32 + (p.y - geo.y(sh, c.row)) / geo.z;
                app.grid.ink.push([sx, sy]);
            }
            if !app.grid.ink.is_empty() {
                // Live preview.
                let pts: Vec<Pos2> = app
                    .grid
                    .ink
                    .iter()
                    .map(|q| {
                        let (col, row) = (sh.col_at(q[0].max(0.0) as f64), sh.row_at(q[1].max(0.0) as f64));
                        geo.mirror_pos(pos2(
                            geo.x(sh, col) + (q[0] - sh.col_left(col) as f32) * geo.z,
                            geo.y(sh, row) + (q[1] - sh.row_top(row) as f32) * geo.z,
                        ))
                    })
                    .collect();
                input
                    .ui
                    .painter()
                    .add(egui::epaint::PathShape::line(pts, egui::epaint::PathStroke::new(2.0 * geo.z, Color32::from_rgb(0x1F, 0x5F, 0xC9))));
            }
            if resp.drag_stopped() && app.grid.ink.len() >= 2 {
                let pts: Vec<[f32; 2]> = std::mem::take(&mut app.grid.ink);
                let _ = app.run("draw.stroke", json!({"points": pts}));
            }
            return true;
        }
        if app.session.draw_tool == "eraser" && (primary_clicked || resp.dragged()) {
            if let Some(p) = pos
                && let Some(("shape", id, _)) = crate::chartview::hit(app, geo, sh, p)
                && sh.shapes.iter().any(|s| s.id == id && s.kind == gridcraft_engine::model::ShapeKind::Ink)
            {
                let _ = app.run("object.delete", json!({"kind": "shape", "id": id}));
            }
            return true;
        }
    }
    false
}

fn handle_header_autofit(app: &mut SheetApp, input: &InteractionInput<'_>) -> bool {
    let (geo, sh, sel, pos) = (input.geo, input.sheet, &input.selection, input.position);
    let (in_col_header, in_row_header) = (input.in_column_header, input.in_row_header);
    let resp = input.response;
    if !resp.double_clicked() {
        return false;
    }
    let Some(p) = pos else { return false };

    let unhide = if in_col_header {
        hidden_col_edge(geo, sh, p.x).map(|col| ("home.unhideColumns", json!({"cols": RangeRef::cols(col, col).a1()})))
    } else if in_row_header {
        hidden_row_edge(geo, sh, p.y).map(|row| ("home.unhideRows", json!({"rows": RangeRef::rows(row, row).a1()})))
    } else {
        None
    };
    let autofit = if unhide.is_some() {
        unhide
    } else if in_col_header {
        col_edge(geo, sh, p.x).map(|(col, _)| {
            let selected = sel.ranges.iter().any(|r| r.is_full_cols() && col >= r.start.col && col <= r.end.col);
            ("home.autofitColumnWidth", if selected { json!({}) } else { json!({"cols": RangeRef::cols(col, col).a1()}) })
        })
    } else if in_row_header {
        row_edge(geo, sh, p.y).map(|(row, _)| {
            let selected = sel.ranges.iter().any(|r| r.is_full_rows() && row >= r.start.row && row <= r.end.row);
            ("home.autofitRowHeight", if selected { json!({}) } else { json!({"rows": RangeRef::rows(row, row).a1()}) })
        })
    } else {
        None
    };
    if let Some((command, params)) = autofit {
        app.grid.drag = Drag::None;
        app.grid.drag_select = false;
        app.run_or_alert(command, params);
        resp.request_focus();
        return true;
    }
    false
}

fn handle_column_header_press(app: &mut SheetApp, input: &InteractionInput<'_>, p: Pos2) {
    let (geo, sh, wb, sel) = (input.geo, input.sheet, input.workbook, &input.selection);
    let mods = input.modifiers;
    let resp = input.response;
    let in_col_header = input.in_column_header;
    if in_col_header {
        if let Some((c, x)) = col_edge(geo, sh, p.x) {
            app.grid.drag = Drag::ResizeCol { col: c, start: p.x, orig: sh.col_width(c).max(0.0) * 0.0 + (x - geo.x(sh, c)) / geo.z };
            return;
        }
        let c = geo.col_at(sh, p.x);
        // A Table heading's filter button, while the header names stand in for the letters.
        if let Some(h) = table_headings(geo, sh, sel.active).into_iter().find(|h| h.col == c && h.button.is_some_and(|b| b.contains(p))) {
            app.grid.filter_menu = Some((c, h.header_row, geo.mirror_pos(p)));
            return;
        }
        if point_header(app, wb, sh, RangeRef::cols(c, c), mods.shift, mods.command) {
            return;
        }
        // An armed Format Painter paints the columns dragged over instead of moving them.
        if resp.drag_started()
            && !mods.any()
            && app.session.format_painter.is_none()
            && sel.ranges.len() == 1
            && sel.current().is_full_cols()
            && sel.current().contains(CellRef::new(0, c))
        {
            app.grid.drag = Drag::MoveCols { source: sel.current(), before: None };
            return;
        }
        let range = if mods.shift { RangeRef::cols(sel.anchor.col.min(c), sel.anchor.col.max(c)) } else { RangeRef::cols(c, c) };
        select_ranges(app, range, mods.command, CellRef::new(geo.row_at(sh, geo.cells.top() + 1.0), c));
        app.grid.drag = Drag::Cols(if mods.shift { sel.anchor.col } else { c });
    }
}

fn handle_row_header_press(app: &mut SheetApp, input: &InteractionInput<'_>, p: Pos2) {
    let (geo, sh, wb, sel) = (input.geo, input.sheet, input.workbook, &input.selection);
    let mods = input.modifiers;
    let in_row_header = input.in_row_header;
    if in_row_header {
        if let Some((r, y)) = row_edge(geo, sh, p.y) {
            app.grid.drag = Drag::ResizeRow { row: r, start: p.y, orig: (y - geo.y(sh, r)) / geo.z };
            return;
        }
        let r = geo.row_at(sh, p.y);
        if point_header(app, wb, sh, RangeRef::rows(r, r), mods.shift, mods.command) {
            return;
        }
        let range = if mods.shift { RangeRef::rows(sel.anchor.row.min(r), sel.anchor.row.max(r)) } else { RangeRef::rows(r, r) };
        select_ranges(app, range, mods.command, CellRef::new(r, geo.col_at(sh, geo.cells.left() + 1.0)));
        app.grid.drag = Drag::Rows(if mods.shift { sel.anchor.row } else { r });
    }
}

fn handle_press(app: &mut SheetApp, input: &InteractionInput<'_>) {
    let (geo, sh, wb, resp) = (input.geo, input.sheet, input.workbook, input.response);
    let (pos, mods, sel) = (input.position, input.modifiers, &input.selection);
    let (primary_clicked, in_cells, in_col_header, in_row_header, in_corner) =
        (input.primary_clicked, input.in_cells, input.in_column_header, input.in_row_header, input.in_corner);
    let (freeze_cols, freeze_rows) = (input.freeze_columns, input.freeze_rows);
    let on_handle = |p| input.on_handle(p);
    let on_cur_border = |p| input.on_current_border(p);
    let pointer = input.press_origin;

    // Where the press began (a drag is only recognised after the pointer has moved).
    let Some(p) = pointer.map(|p| geo.mirror_pos(p)).or(pos) else { return };
    // The outline band's − / + and level buttons act on release, like buttons.
    if let Some(action) = crate::outline::hit(geo, sh, p) {
        if primary_clicked {
            let (command, params) = action.command();
            app.run_or_alert(command, params);
        }
        return;
    }
    // Freeze handles sit on the header edges (and the corner's edges when nothing is frozen).
    if freeze_cols.contains(p) || freeze_rows.contains(p) {
        app.grid.drag = Drag::Freeze { cols: freeze_cols.contains(p), to: None };
        return;
    }
    if in_corner {
        let _ = app.session.run("selection.set", json!({"range": RangeRef::all().a1()}));
        return;
    }
    if in_col_header {
        handle_column_header_press(app, input, p);
        return;
    }
    if in_row_header {
        handle_row_header_press(app, input, p);
        return;
    }
    if !in_cells {
        return;
    }
    let c = geo.cell_at(sh, p);
    // Data validation list arrow next to the active cell.
    if let Some(r) = validation_arrow(sh, geo, sel.active)
        && r.contains(p)
    {
        app.grid.list_picker = Some(sel.active);
        return;
    }
    // Filter dropdown buttons.
    if is_filter_button(sh, geo, p) {
        app.grid.filter_menu = Some((c.col, c.row, geo.mirror_pos(p)));
        return;
    }
    // A dialog's formula box in point mode (New Formatting Rule) takes the reference; the
    // selection, which the dialog acts on, stays.
    if let Some(d) = app.dialog.as_mut().filter(|d| d.picks_references()) {
        d.pick_reference(c, &sh.name, false);
        app.grid.drag = Drag::Select;
        app.grid.drag_select = true;
        if resp.drag_started()
            && let Some(p) = pos
        {
            select_to_pointer(app, geo, sh, wb, p);
        }
        return;
    }
    // Point mode while editing a formula: clicking inserts a reference.
    if let Some(ed) = app.editor.as_mut()
        && ed.can_point()
    {
        let anchor = if mods.shift && ed.point_at_caret() {
            ed.point_anchor.filter(|(sheet, _)| *sheet == wb.active_sheet()).map(|(_, cell)| cell).unwrap_or(c)
        } else {
            c
        };
        let range = RangeRef::new(anchor, c);
        let text = if range.is_single() { c.a1() } else { range.a1() };
        let sheet_name = (ed.sheet != wb.active_sheet()).then(|| sh.name.clone());
        let r = match sheet_name {
            Some(n) => format!("{}!{text}", gridcraft_engine::formula::quote_sheet(&n)),
            None => text,
        };
        if mods.command && !mods.shift {
            ed.append_ref(&r);
        } else {
            ed.insert_ref(&r);
        }
        ed.request_focus = true;
        ed.point_cell = Some(c);
        ed.point_anchor = Some((wb.active_sheet(), anchor));
        app.grid.drag = Drag::Select;
        app.grid.drag_select = true;
        if resp.drag_started()
            && let Some(p) = pos
        {
            select_to_pointer(app, geo, sh, wb, p);
        }
        return;
    }
    if app.editor.is_some() && !app.commit_edit(0, 0, false, false) {
        return;
    }
    if on_handle(p) {
        app.grid.drag = Drag::Fill { target: None, menu: resp.drag_started_by(egui::PointerButton::Secondary) };
        return;
    }
    if on_cur_border(p) && resp.drag_started() {
        app.grid.drag = Drag::Move { grab: c, to: None };
        return;
    }
    let now = crate::now_ms();
    if handle_cell_double_click(app, input, c, now) {
        return;
    }
    app.grid.last_click = Some((c, now));
    if mods.shift {
        let _ = app.session.run("selection.set", json!({"range": RangeRef::new(sel.anchor, c).a1(), "active": sel.active.a1()}));
        if let Some(d) = app.session.active_mut() {
            d.selection.anchor = sel.anchor;
        }
    } else if mods.command {
        // Ctrl+click (without Alt) on a hyperlink's text follows it instead of adding the cell to the selection.
        if let Some(h) = sh.hyperlinks.get(&c).cloned()
            && primary_clicked
            && !mods.alt
            && p.x < geo.cell_rect(sh, c).left() + geo.cell_rect(sh, c).width() * 0.8
        {
            follow_link(app, &h.target);
        } else {
            select_ranges(app, RangeRef::cell(c), true, c);
        }
    } else {
        let _ = app.session.run("selection.set", json!({"cell": c.a1()}));
        if primary_clicked && wb.styles.get(sh.style_id(c)).num_fmt.as_str() == "checkbox" {
            let _ = app.run("cell.toggleCheckbox", json!({"cell": c.a1()}));
        }
        // A click on a hyperlink's text follows it (like Excel); elsewhere in the cell selects.
        if let Some(h) = sh.hyperlinks.get(&c).cloned()
            && primary_clicked
            && !mods.any()
        {
            let r = geo.cell_rect(sh, c);
            if p.x < r.left() + r.width() * 0.8 {
                follow_link(app, &h.target);
            }
        }
    }
    app.grid.drag = Drag::Select;
    app.grid.drag_select = true;
    if resp.drag_started()
        && let Some(p) = pos
    {
        // The first recognised drag already includes movement away from the press.
        select_to_pointer(app, geo, sh, wb, p);
    }
}

fn handle_cell_double_click(app: &mut SheetApp, input: &InteractionInput<'_>, c: CellRef, now: f64) -> bool {
    let (sh, mods) = (input.sheet, input.modifiers);
    if let Some((last, at)) = app.grid.last_click
        && last == c
        && now - at < 450.0
        && !mods.shift
    {
        app.grid.last_click = None;
        // A PivotTable's value cell shows its source records on a new sheet (Excel's Show
        // Details); its labels and headers still edit.
        if sh.pivots.iter().any(|pt| pt.last_range.is_some_and(|r| r.contains(c))) {
            let not_a_value_cell =
                gridcraft_engine::EngineError::Disabled("pivot.showDetails".into(), gridcraft_engine::pivot::NOT_A_VALUE_CELL.into()).to_string();
            match app.run("pivot.showDetails", json!({"cell": c.a1()})) {
                Ok(_) => return true,
                Err(e) if e == not_a_value_cell => {}
                Err(e) => {
                    app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                    return true;
                }
            }
        }
        // Following a hyperlink requires a single click in Excel; double-click edits.
        app.begin_edit(None, false);
        return true;
    }
    false
}

fn handle_context_menu_and_keyboard(app: &mut SheetApp, input: &InteractionInput<'_>) {
    let (geo, sh, sel, pos) = (input.geo, input.sheet, &input.selection, input.position);
    let resp = input.response;
    let primary_clicked = input.primary_clicked;
    let (in_cells, in_col_header, in_row_header) = (input.in_cells, input.in_column_header, input.in_row_header);
    let ctx = input.context.clone();
    // Right-click: context menu.
    if resp.secondary_clicked()
        && let Some(p) = pos
    {
        if in_cells && !sel.contains(geo.cell_at(sh, p)) {
            let _ = app.session.run("selection.set", json!({"cell": geo.cell_at(sh, p).a1()}));
        }
        if in_col_header || in_row_header {
            let (rows, idx) = if in_row_header { (true, geo.row_at(sh, p.y)) } else { (false, geo.col_at(sh, p.x)) };
            let inside = sel.ranges.iter().any(|r| {
                if rows {
                    r.is_full_rows() && idx >= r.start.row && idx <= r.end.row
                } else {
                    r.is_full_cols() && idx >= r.start.col && idx <= r.end.col
                }
            });
            if !inside {
                let range = if rows { RangeRef::rows(idx, idx) } else { RangeRef::cols(idx, idx) };
                let _ = app.session.run("selection.set", json!({"range": range.a1()}));
            }
            app.grid.header_menu = Some((geo.mirror_pos(p), rows));
        } else {
            app.grid.context_menu = Some(geo.mirror_pos(p));
            app.grid.context_menu_frame = input.ui.ctx().cumulative_frame_nr();
        }
    }

    if primary_clicked || resp.drag_started() {
        resp.request_focus();
    }
    keyboard(app, &ctx, resp, geo, sh);
}

fn handle_dragging(app: &mut SheetApp, input: &InteractionInput<'_>) {
    let (geo, sh, wb, sel, pos) = (input.geo, input.sheet, input.workbook, &input.selection, input.position);
    let resp = input.response;
    if resp.dragged()
        && let Some(p) = pos
    {
        // Auto-scroll near edges.
        let edge = 24.0;
        let mut auto = egui::Vec2::ZERO;
        if p.y > geo.cells.bottom() - edge {
            auto.y = 20.0;
        }
        if p.y < geo.cells.top() + edge * 0.5 && !matches!(app.grid.drag, Drag::ResizeRow { .. }) {
            auto.y = -20.0;
        }
        if p.x > geo.cells.right() - edge {
            auto.x = 30.0;
        }
        if p.x < geo.cells.left() + edge * 0.5 && !matches!(app.grid.drag, Drag::ResizeCol { .. }) {
            auto.x = -30.0;
        }
        let pc = CellRef::new(
            geo.row_at(sh, p.y.clamp(geo.cells.top() + 1.0, geo.cells.bottom() - 1.0)),
            geo.col_at(sh, p.x.clamp(geo.cells.left() + 1.0, geo.cells.right() - 1.0)),
        );
        match app.grid.drag.clone() {
            Drag::Select => {
                select_to_pointer(app, geo, sh, wb, p);
                scroll_by(app, auto);
            }
            Drag::Rows(anchor) => {
                let r = geo.row_at(sh, p.y);
                let _ =
                    app.session.run("selection.set", json!({"range": RangeRef::rows(anchor.min(r), anchor.max(r)).a1(), "active": sel.active.a1()}));
                scroll_by(app, vec2(0.0, auto.y));
            }
            Drag::Cols(anchor) => {
                let c = geo.col_at(sh, p.x);
                let _ =
                    app.session.run("selection.set", json!({"range": RangeRef::cols(anchor.min(c), anchor.max(c)).a1(), "active": sel.active.a1()}));
                scroll_by(app, vec2(auto.x, 0.0));
            }
            Drag::MoveCols { source, .. } => {
                let col = geo.col_at(sh, p.x.clamp(geo.cells.left() + 1.0, geo.cells.right() - 1.0));
                let midpoint = geo.x(sh, col) + sh.col_width(col) * geo.z * 0.5;
                let before = (col + u32::from(p.x >= midpoint)).min(MAX_COLS - 1);
                let before = (!(source.start.col..=source.end.col + 1).contains(&before)).then_some(before);
                app.grid.drag = Drag::MoveCols { source, before };
                scroll_by(app, vec2(auto.x, 0.0));
            }
            Drag::ResizeCol { col, start, orig } => {
                let w = (orig + (p.x - start) / geo.z).max(0.0);
                app.toast =
                    Some((format!("Width: {:.2} ({} pixels)", gridcraft_engine::cmd::format::points_to_chars(w as f64), w.round()), crate::now_ms()));
                // Inside a selection of whole columns, every selected column takes the new width
                // (Ctrl-selected areas included); otherwise only the dragged one.
                let cols = if sel.ranges.iter().any(|r| r.is_full_cols() && col >= r.start.col && col <= r.end.col) {
                    sel.ranges.iter().filter(|r| r.is_full_cols()).map(|r| r.a1()).collect::<Vec<_>>().join(",")
                } else {
                    RangeRef::cols(col, col).a1()
                };
                let _ = app.session.run("home.columnWidth", json!({"cols": cols, "width": w.round()}));
                coalesce_last_undo(app);
            }
            Drag::ResizeRow { row, start, orig } => {
                let h = (orig + (p.y - start) / geo.z).max(0.0);
                app.toast = Some((format!("Height: {:.2} ({} pixels)", h * 0.75, h.round()), crate::now_ms()));
                let rows = if sel.ranges.iter().any(|r| r.is_full_rows() && row >= r.start.row && row <= r.end.row) {
                    sel.ranges
                        .iter()
                        .filter(|r| r.is_full_rows())
                        .map(|r| format!("{}:{}", r.start.row + 1, r.end.row + 1))
                        .collect::<Vec<_>>()
                        .join(",")
                } else {
                    format!("{}:{}", row + 1, row + 1)
                };
                let _ = app.session.run("home.rowHeight", json!({"rows": rows, "height": h.round()}));
                coalesce_last_undo(app);
            }
            Drag::Freeze { cols, .. } => {
                app.grid.drag = Drag::Freeze { cols, to: Some(freeze_boundary(geo, sh, p, cols)) };
            }
            Drag::Fill { menu, .. } => {
                let src = sel.current();
                let target = fill_target(src, pc);
                app.grid.drag = Drag::Fill { target: Some(target), menu };
                scroll_by(app, auto);
            }
            Drag::Move { grab, .. } => {
                let src = sel.current();
                let dr = pc.row as i64 - grab.row as i64;
                let dc = pc.col as i64 - grab.col as i64;
                app.grid.drag = Drag::Move { grab, to: Some(src.start.offset_clamped(dr, dc)) };
                scroll_by(app, auto);
            }
            Drag::Object { kind, id, start, orig, resize } => {
                let d = (p - start) / geo.z;
                let (cell, dx, dy, w, h) = orig;
                let params = if resize {
                    json!({"kind": kind, "id": id, "width": (w + d.x).max(20.0), "height": (h + d.y).max(20.0)})
                } else {
                    // Re-anchor at the cell under the new top-left.
                    let x0 = sh.col_left(cell.col) as f32 + dx + d.x;
                    let y0 = sh.row_top(cell.row) as f32 + dy + d.y;
                    let nc = CellRef::new(sh.row_at(y0.max(0.0) as f64), sh.col_at(x0.max(0.0) as f64));
                    json!({"kind": kind, "id": id, "at": nc.a1(), "dx": x0.max(0.0) - sh.col_left(nc.col) as f32, "dy": y0.max(0.0) - sh.row_top(nc.row) as f32})
                };
                let _ = app.session.run("object.move", params);
                coalesce_last_undo(app);
            }
            Drag::None => {}
        }
    }
}

fn handle_release(app: &mut SheetApp, input: &InteractionInput<'_>) {
    let (geo, sh, wb, sel, pos, mods, resp) =
        (input.geo, input.sheet, input.workbook, &input.selection, input.position, input.modifiers, input.response);
    let ctx = input.context.clone();
    if resp.drag_stopped() || (!input.any_pointer_down && app.grid.drag != Drag::None && !resp.dragged()) {
        match std::mem::take(&mut app.grid.drag) {
            Drag::Select => {
                if resp.drag_stopped()
                    && let Some(p) = pos
                {
                    select_to_pointer(app, geo, sh, wb, p);
                }
                format_painter_apply(app);
            }
            // Whole rows or columns clicked or dragged over by their headers.
            Drag::Rows(_) | Drag::Cols(_) => format_painter_apply(app),
            // Esc mid-drag lands here too (egui ends the drag on Esc): no menu, and the key is
            // spent on the drag so the grid's Esc doesn't also drop the copy.
            Drag::Fill { target: Some(t), menu: true } => {
                if let Some(p) = pos
                    && t != sel.current()
                    && !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                {
                    app.grid.fill_menu = Some((sel.current(), t, geo.mirror_pos(p)));
                }
            }
            Drag::Fill { target: Some(t), menu: false } => {
                let src = sel.current();
                if t != src {
                    // Ctrl (Cmd on macOS) swaps copying and extending, as in Excel.
                    let mode = if mods.command { "alternate" } else { "series" };
                    match app.run("edit.autoFill", json!({"source": src.a1(), "target": t.a1(), "mode": mode})) {
                        Ok(_) => set_fill_tag(app, src, t),
                        Err(e) => app.message = Some(("GridCraft".into(), crate::clean_error(&e))),
                    }
                }
            }
            Drag::Freeze { cols, to: Some(to) } => {
                let (rows_now, cols_now) = (geo.fr, geo.fc);
                let (rows, cols) = if cols { (rows_now, to) } else { (to, cols_now) };
                if (rows, cols) != (rows_now, cols_now) {
                    // A1 (nothing left frozen) unfreezes.
                    app.run_or_alert("view.freezePanes", json!({"cell": CellRef::new(rows, cols).a1()}));
                }
            }
            Drag::Move { to: Some(to), .. } => {
                let src = sel.current();
                if to != src.start {
                    let _ = app.session.run("edit.cut", json!({"range": src.a1()}));
                    let _ = app.run("edit.paste", json!({"at": to.a1()}));
                }
            }
            Drag::MoveCols { source, before: Some(before) } => {
                if let Err(e) = app.run("sheet.moveColumns", json!({"cols": source.a1(), "before": before})) {
                    app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                }
            }
            _ => {}
        }
        app.grid.drag_select = false;
    }
}

/// An armed Format Painter pastes onto the cells just clicked or dragged over (not while
/// pointing a formula reference, which also drags a Select).
fn format_painter_apply(app: &mut SheetApp) {
    if app.session.format_painter.is_some()
        && !app.editor.as_ref().is_some_and(|ed| ed.can_point())
        && !app.dialog.as_ref().is_some_and(|d| d.picks_references())
    {
        let dest = app.session.active().map(|d| d.selection.current().a1());
        if let Some(dest) = dest
            && let Err(e) = app.run("edit.formatPainter", json!({"apply": dest}))
        {
            app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
        }
    }
}

/// Point mode on a header: inserts a whole-row or whole-column reference instead of selecting.
fn point_header(app: &mut SheetApp, wb: &Workbook, sh: &Sheet, range: RangeRef, extend: bool, add: bool) -> bool {
    let Some(ed) = app.editor.as_mut().filter(|ed| ed.can_point()) else { return false };
    let edit_sheet = ed.sheet;
    let active_sheet = wb.active_sheet();
    let range = if extend {
        ed.point
            .and_then(|point| crate::editor::formula_refs(&ed.text).into_iter().find(|(start, end, ..)| (*start, *end) == point))
            .and_then(|(_, _, previous, previous_sheet, _)| {
                let previous_sheet = previous_sheet.as_deref().map(|name| wb.sheet_index(name)).unwrap_or(Some(edit_sheet));
                if previous_sheet != Some(active_sheet) {
                    return None;
                }
                if range.is_full_cols() && previous.is_full_cols() {
                    Some(RangeRef::cols(previous.start.col.min(range.start.col), previous.end.col.max(range.end.col)))
                } else if range.is_full_rows() && previous.is_full_rows() {
                    Some(RangeRef::rows(previous.start.row.min(range.start.row), previous.end.row.max(range.end.row)))
                } else {
                    None
                }
            })
            .unwrap_or(range)
    } else {
        range
    };
    let reference = range.a1();
    let reference =
        if ed.sheet != wb.active_sheet() { format!("{}!{reference}", gridcraft_engine::formula::quote_sheet(&sh.name)) } else { reference };
    if add {
        ed.append_ref(&reference);
    } else {
        ed.insert_ref(&reference);
    }
    ed.point_anchor = None;
    ed.point_cell = Some(range.start);
    ed.request_focus = true;
    true
}

fn select_to_pointer(app: &mut SheetApp, geo: &Geo, sh: &Sheet, wb: &Workbook, p: Pos2) {
    let cell = CellRef::new(
        geo.row_at(sh, p.y.clamp(geo.cells.top() + 1.0, geo.cells.bottom() - 1.0)),
        geo.col_at(sh, p.x.clamp(geo.cells.left() + 1.0, geo.cells.right() - 1.0)),
    );
    if let Some(d) = app.dialog.as_mut().filter(|d| d.picks_references()) {
        d.pick_reference(cell, &sh.name, true);
    } else if let Some(ed) = app.editor.as_mut()
        && ed.point.is_some()
    {
        if let Some(start) = ed.point_anchor.filter(|(sheet, _)| *sheet == wb.active_sheet()).map(|(_, cell)| cell).or(ed.point_cell) {
            let range = RangeRef::new(start, cell);
            let text = if range.is_single() { range.start.a1() } else { range.a1() };
            let text = if ed.sheet != wb.active_sheet() { format!("{}!{text}", gridcraft_engine::formula::quote_sheet(&sh.name)) } else { text };
            ed.insert_ref(&text);
            ed.point_cell = Some(cell);
            ed.point_anchor = Some((wb.active_sheet(), start));
        }
    } else if let Some(d) = app.session.active() {
        // Read the current anchor: starting a drag may have just replaced the selection.
        let anchor = d.selection.anchor;
        let active = d.selection.active;
        let mut ranges: Vec<String> = d.selection.ranges.iter().map(|r| r.a1()).collect();
        ranges.pop();
        ranges.push(RangeRef::new(anchor, cell).a1());
        let _ = app.session.run("selection.set", json!({"range": ranges.join(","), "active": active.a1()}));
        if let Some(d) = app.session.active_mut() {
            d.selection.anchor = anchor;
        }
    }
}

fn coalesce_last_undo(app: &mut SheetApp) {
    // Merge consecutive drag steps into one undo entry.
    if let Some(d) = app.session.active_mut()
        && d.undo.len() >= 2
    {
        let n = d.undo.len();
        if d.undo[n - 1].label == d.undo[n - 2].label {
            d.undo.remove(n - 1);
        }
    }
}

fn scroll_by(app: &mut SheetApp, d: egui::Vec2) {
    if d != egui::Vec2::ZERO
        && let Some(v) = app.view_mut()
    {
        v.scroll = (v.scroll + d).max(egui::Vec2::ZERO);
    }
}

fn fill_target(src: RangeRef, pc: CellRef) -> RangeRef {
    let below = pc.row as i64 - src.end.row as i64;
    let above = src.start.row as i64 - pc.row as i64;
    let right = pc.col as i64 - src.end.col as i64;
    let left = src.start.col as i64 - pc.col as i64;
    let vert = below.max(above);
    let horiz = right.max(left);
    if vert <= 0 && horiz <= 0 {
        return src;
    }
    if vert >= horiz {
        if below > 0 {
            RangeRef::new(src.start, CellRef::new(pc.row, src.end.col))
        } else {
            RangeRef::new(CellRef::new(pc.row, src.start.col), src.end)
        }
    } else if right > 0 {
        RangeRef::new(src.start, CellRef::new(src.end.row, pc.col))
    } else {
        RangeRef::new(CellRef::new(src.start.row, pc.col), src.end)
    }
}

pub(super) fn on_border(r: Rect, p: Pos2) -> bool {
    let outer = r.expand(3.0);
    let inner = r.shrink(3.0);
    outer.contains(p) && !inner.contains(p)
}

/// The freeze handles, as in Excel for the web: a bar in the upper part of the column-header strip
/// on the frozen columns' right edge, and one in the left part of the row-header strip on the
/// frozen rows' bottom edge. With nothing frozen they sit on the corner's edges. The rest of the
/// header edge keeps resizing the column or row.
pub fn freeze_handles(geo: &Geo) -> (Rect, Rect) {
    let x = geo.cells.left() + geo.frozen_w;
    let y = geo.cells.top() + geo.frozen_h;
    let cols = Rect::from_min_max(pos2(x - 3.0, geo.head_top()), pos2(x + 3.0, geo.head_top() + geo.header_h * 0.4));
    let rows = Rect::from_min_max(pos2(geo.head_left(), y - 3.0), pos2(geo.head_left() + geo.header_w * 0.4, y + 3.0));
    (cols, rows)
}

/// How many columns (or rows) a freeze handle dropped at `p` freezes: the line boundary nearest
/// the pointer, limited so the frozen pane leaves room for the scrolling one.
fn freeze_boundary(geo: &Geo, sh: &Sheet, p: Pos2, cols: bool) -> u32 {
    if cols {
        if p.x <= geo.cells.left() {
            return 0;
        }
        let c = geo.col_at(sh, p.x);
        let (l, r) = (geo.x(sh, c), geo.x(sh, c.saturating_add(1).min(MAX_COLS - 1)));
        let b = if p.x - l > r - p.x { c.saturating_add(1) } else { c };
        let room = geo.ci.at((((geo.cells.width() - 40.0) / geo.z).max(0.0)) as f64);
        b.min(room).min(MAX_COLS - 1)
    } else {
        if p.y <= geo.cells.top() {
            return 0;
        }
        let rr = geo.row_at(sh, p.y);
        let (t, b2) = (geo.y(sh, rr), geo.y(sh, rr.saturating_add(1).min(MAX_ROWS - 1)));
        let b = if p.y - t > b2 - p.y { rr.saturating_add(1) } else { rr };
        let room = geo.ri.at((((geo.cells.height() - 40.0) / geo.z).max(0.0)) as f64);
        b.min(room).min(MAX_ROWS - 1)
    }
}

/// Paints the freeze handles and, while one is dragged, where the freeze line will go.
pub(super) fn paint_freeze_handles(p: &Painter, geo: &Geo, sh: &Sheet, drag: &Drag, t: &Tokens) {
    let (cols, rows) = freeze_handles(geo);
    let bar = Color32::from_gray(150);
    p.rect_filled(Rect::from_center_size(cols.center(), vec2(3.0, cols.height())), 1.0, bar);
    p.rect_filled(Rect::from_center_size(rows.center(), vec2(rows.width(), 3.0)), 1.0, bar);
    if let Drag::Freeze { cols, to: Some(to) } = *drag {
        let stroke = Stroke::new(2.0, t.accent);
        if cols {
            let x = geo.cells.left() + sh.col_left(to) as f32 * geo.z;
            p.line_segment([pos2(x, geo.head_top()), pos2(x, geo.cells.bottom())], stroke);
        } else {
            let y = geo.cells.top() + sh.row_top(to) as f32 * geo.z;
            p.line_segment([pos2(geo.head_left(), y), pos2(geo.cells.right(), y)], stroke);
        }
    }
}

pub(super) fn col_edge(geo: &Geo, sh: &Sheet, x: f32) -> Option<(u32, f32)> {
    let c = geo.col_at(sh, x);
    for cc in [c, c.saturating_sub(1)] {
        let edge = geo.x(sh, cc) + sh.col_width(cc) * geo.z;
        if (x - edge).abs() <= 4.0 {
            return Some((cc, edge));
        }
    }
    None
}

pub(super) fn row_edge(geo: &Geo, sh: &Sheet, y: f32) -> Option<(u32, f32)> {
    let r = geo.row_at(sh, y);
    for rr in [r, r.saturating_sub(1)] {
        let edge = geo.y(sh, rr) + sh.row_height(rr) * geo.z;
        if (y - edge).abs() <= 3.0 {
            return Some((rr, edge));
        }
    }
    None
}

/// The hidden column whose (zero-width) edge is at `x`: only just right of the boundary, as the
/// edge just left of it belongs to the visible column there.
pub(super) fn hidden_col_edge(geo: &Geo, sh: &Sheet, x: f32) -> Option<u32> {
    col_edge(geo, sh, x).map(|(c, _)| c).filter(|&c| sh.col_width(c) <= 0.0)
}

pub(super) fn hidden_row_edge(geo: &Geo, sh: &Sheet, y: f32) -> Option<u32> {
    row_edge(geo, sh, y).map(|(r, _)| r).filter(|&r| sh.row_height(r) <= 0.0)
}

fn object_anchor(sh: &Sheet, kind: &str, id: u32) -> Option<gridcraft_engine::model::Anchor> {
    match kind {
        "chart" => sh.charts.iter().find(|c| c.id == id).map(|c| c.anchor),
        "image" => sh.images.iter().find(|c| c.id == id).map(|c| c.anchor),
        "slicer" => sh.slicers.iter().find(|c| c.id == id).map(|c| c.anchor),
        _ => sh.shapes.iter().find(|c| c.id == id).map(|c| c.anchor),
    }
}

pub(super) fn delete_selected_object(app: &mut SheetApp, sh: &Sheet) -> bool {
    let Some(id) = app.selected_chart else { return false };
    let kind = if sh.charts.iter().any(|object| object.id == id) {
        "chart"
    } else if sh.images.iter().any(|object| object.id == id) {
        "image"
    } else if sh.shapes.iter().any(|object| object.id == id) {
        "shape"
    } else {
        return false;
    };
    app.run_or_alert("object.delete", json!({"kind": kind, "id": id}));
    app.selected_chart = None;
    true
}

/// A click on a hyperlink. References select; web and e-mail addresses ask first (the dialog's
/// OK runs `link.open`); every other kind is refused with a message. The decision is the
/// engine's ([`gridcraft_engine::links::classify`]), the same one `link.open` applies.
pub fn follow_link(app: &mut SheetApp, target: &str) {
    use gridcraft_engine::links::{LinkKind, classify};
    match classify(target) {
        LinkKind::Internal(r) => {
            let _ = app.session.run("edit.goTo", json!({"reference": r.trim_start_matches('#')}));
            app.grid.ensure_visible = true;
        }
        LinkKind::External(url) => app.open_dialog("openLink", json!({"url": url})),
        LinkKind::Refused(why) => app.message = Some(("Hyperlink".into(), why)),
    }
}

/// Moves the pointed cell one step, or with `jump` (Ctrl+arrow) to the edge of its data region;
/// `extend` (Shift) moves the far end of the pointed range instead. Keeps the pointed cell visible.
pub fn point_move(app: &mut SheetApp, ed: &mut EditState, dr: i64, dc: i64, extend: bool, jump: bool) {
    let Some(d) = app.session.active() else { return };
    let Some(sheet) = d.wb.active() else { return };
    let from = ed.point_cell.unwrap_or(ed.cell);
    let to = if jump { gridcraft_engine::selection::jump(sheet, from, dr.signum(), dc.signum()) } else { from.offset_clamped(dr, dc) };
    let anchor = if extend {
        // The anchor is the corner opposite the moving end: the written range is normalized, so
        // after extending up or left its first cell is the moving end, not the anchor.
        let pointed = ed.point.and_then(|(s, e)| ed.text.get(s..e)).and_then(RangeRef::parse);
        let inferred_anchor = pointed
            .map(|r| {
                CellRef::new(
                    if from.row == r.start.row { r.end.row } else { r.start.row },
                    if from.col == r.start.col { r.end.col } else { r.start.col },
                )
            })
            .unwrap_or(from);
        ed.point_anchor.filter(|(sheet, _)| *sheet == d.wb.active_sheet()).map(|(_, cell)| cell).unwrap_or(inferred_anchor)
    } else {
        to
    };
    let text = if extend { RangeRef::new(anchor, to).a1() } else { to.a1() };
    let sheet = d.wb.active_sheet();
    ed.insert_ref(&text);
    ed.point_cell = Some(to);
    ed.point_anchor = Some((sheet, anchor));
    app.grid.ensure_visible = true;
}
