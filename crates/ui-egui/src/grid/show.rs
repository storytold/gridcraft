#[cfg(test)]
use super::Trace;
use super::cursor::update_cursor;
use super::editor::{in_cell_editor, list_picker, validation_arrow};
use super::geo::Geo;
use super::interact::{interact, paint_freeze_handles};
use super::menus::{context_menu, error_check_tag, fill_menu, fill_options_tag, filter_menu, header_menu, object_menu};
use super::quadrant::{QuadrantPaint, mirror_painted, paint_quadrant};
use super::selection::{SelectionPaintInput, paint_selection};
use super::tables::{TableHeading, paint_filter_button, table_headings};
use super::{
    Align2, Arc, CfCache, Color32, DISPLAY_SCALE, Drag, MAX_COLS, MAX_ROWS, Painter, PaneQuad, RangeRef, Rect, Sense, Sheet, SheetApp, Stroke,
    StrokeKind, Tokens, WHEEL_ZOOM_IDLE, WheelZoom, Workbook, col_to_letters, heading_font_size, json, pos2, theme, vec2, wheel_zoom_step,
    zoom_glide_step,
};
#[cfg(test)]
use super::{CellRef, Pos2};

pub fn show(app: &mut SheetApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let available = ui.available_rect_before_wrap();
    // A right-to-left sheet has its vertical scrollbar on the left.
    let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|s| s.right_to_left);
    let bar = 14.0_f32.min(available.width());
    let rect = if rtl {
        Rect::from_min_max(pos2(available.left() + bar, available.top()), available.max)
    } else {
        Rect::from_min_max(available.min, pos2(available.right() - bar, available.bottom()))
    };
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    app.grid.rect = Some(rect);
    let Some(d) = app.session.active() else { return };
    let wb: Arc<Workbook> = d.wb.clone();
    let si = wb.active_sheet();
    let Some(sh) = wb.sheet(si) else { return };
    let sel = d.selection.clone();
    let place = (d.uid, si);
    let mut view = app.view();
    // Page Layout view: pages on a desk, rulers above and left of the headings.
    let paper = sh.view == gridcraft_engine::model::SheetViewMode::PageLayout;
    let ruler = if paper && !app.session.hide_ruler { crate::paper_view::RULER } else { 0.0 };
    // Right-to-left sheets carry the vertical ruler on the right, beside the row headings.
    let grid_rect = if rtl {
        Rect::from_min_max(pos2(rect.left(), (rect.top() + ruler).min(rect.bottom())), pos2((rect.right() - ruler).max(rect.left()), rect.bottom()))
    } else {
        Rect::from_min_max(pos2((rect.left() + ruler).min(rect.right()), (rect.top() + ruler).min(rect.bottom())), rect.max)
    };
    let make_geo = |app: &mut SheetApp, scroll: egui::Vec2| -> Geo {
        if paper {
            let probe = Geo::new(sh, grid_rect, scroll);
            if let Some(g) = crate::paper_view::grid_for(app, &wb, si, sh, probe.cells.size(), scroll) {
                let g = crate::paper_view::with_dragged_margin(app, &wb, si, g);
                return Geo::paged(sh, grid_rect, scroll, g);
            }
        }
        Geo::new(sh, rect, scroll)
    };
    // Scrolling.
    let (scroll_delta, zoom_delta, modifiers, now) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.time));
    if (resp.hovered() || resp.contains_pointer()) && !crate::text_box::contains_pointer(app, ui.ctx()) {
        if zoom_delta != 1.0 {
            // egui's zoom factor is e^(speed × points); one notch is `line_scroll_speed` points.
            let notch = ui.ctx().options(|o| o.input_options.scroll_zoom_speed * o.input_options.line_scroll_speed);
            let turned = zoom_delta.ln() / notch;
            let mut zooming = WheelZoom::at(app, place, sh.zoom, now);
            if now - zooming.time > WHEEL_ZOOM_IDLE {
                zooming.notches = 0.0;
            }
            if turned.is_finite() {
                zooming.notches = (zooming.notches + turned).clamp(-32.0, 32.0);
            }
            zooming.time = now;
            // A notch steps once 60% of it has turned, and the rest of it brings `notches` back to
            // about 0. (Past half, so what's left after a step can't step back.) Steps count from
            // where a glide is going, not from where it has got to.
            let from = zooming.glide.map_or(sh.zoom, |(to, _)| to);
            let mut z = from;
            while zooming.notches >= 0.6 {
                z = wheel_zoom_step(z, true);
                zooming.notches -= 1.0;
            }
            while zooming.notches <= -0.6 {
                z = wheel_zoom_step(z, false);
                zooming.notches += 1.0;
            }
            if z != from {
                zooming.glide = Some((z, sh.zoom));
            }
            app.grid.wheel_zoom = Some(zooming);
        } else if scroll_delta != egui::Vec2::ZERO {
            let z = if paper { make_geo(app, view.scroll).z } else { sh.zoom as f32 / 100.0 * DISPLAY_SCALE };
            let mut d = scroll_delta;
            // A horizontal swipe moves the content the way the fingers go, which is towards
            // higher columns on a mirrored sheet.
            if sh.right_to_left {
                d.x = -d.x;
            }
            if modifiers.shift && d.x == 0.0 {
                d = vec2(d.y, 0.0);
            }
            view.scroll -= d / z;
        }
    }
    // A one-finger touch drag pans the sheet, the content following the finger; it doesn't select.
    // (Two fingers pinch-zoom through `zoom_delta` above.)
    if resp.drag_started() && ui.input(|i| i.any_touches()) {
        app.grid.touch_pan = true;
    }
    if app.grid.touch_pan && resp.dragged() && ui.input(|i| i.multi_touch().is_none()) {
        let z = if paper { make_geo(app, view.scroll).z } else { sh.zoom as f32 / 100.0 * DISPLAY_SCALE };
        let mut d = resp.drag_delta();
        if sh.right_to_left {
            d.x = -d.x;
        }
        view.scroll -= d / z;
    }
    update_zoom_glide(app, ui, place, sh.zoom, now);
    view.scroll = view.scroll.max(egui::Vec2::ZERO);
    let (mut max_x, mut max_y) = (sh.col_left(MAX_COLS - 20) as f32, sh.row_top(MAX_ROWS - 40) as f32);
    if paper {
        // Page layout units run past sheet units (margins and gaps); the end is known once the
        // pages reach the last row / column.
        let g = make_geo(app, view.scroll);
        if let Some(pg) = &g.paper {
            let (ex, ey) = pg.extent();
            max_x = if pg.cols_complete { (ex as f32 - g.cells.width() / g.z).max(0.0) } else { 1.0e9 };
            max_y = if pg.rows_complete { (ey as f32 - g.cells.height() / g.z).max(0.0) } else { 1.0e9 };
        }
    }
    view.scroll.x = view.scroll.x.min(max_x);
    view.scroll.y = view.scroll.y.min(max_y);
    let mut geo = make_geo(app, view.scroll);
    // Keep the active cell (or the moving end of an extension) visible after keyboard moves.
    ActiveCellVisibility { app, wb: &wb, si, sh, selection: &sel, paper, make_geo: &make_geo }.update(&mut view, &mut geo);
    // Snap scrolling to whole rows/columns at rest like Excel? Keep smooth (better on trackpads).
    if let Some(v) = app.view_mut() {
        *v = view;
    }
    app.grid.cells_rect = Some(geo.mirror_rect(geo.cells));
    // The selection outline glides to a new selection. It stays on its cells when Animations is
    // off, when egui's animations are (the headless harness), and while a screenshot is on its way.
    // A mouse button held down (a click or a drag selecting cells) moves it at once, as in Excel:
    // a gliding outline trails the pointer and reads as lag.
    let outline = selection_outline(app, ui, &geo, sh, &sel, place, now);
    app.grid.geo = Some(geo.clone());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.grid_bg);
    // Everything up to the headers is painted in logical (left-to-right) coordinates.
    let mirror_from = painter.add(egui::Shape::Noop);
    let mut cf = CfCache::new();
    let quads = if geo.paper.is_some() {
        crate::paper_view::paint_pages(
            crate::paper_view::PagePaintInput {
                app,
                painter: &painter,
                geo: &geo,
                workbook: &wb,
                sheet_index: si,
                sheet: sh,
                selection: &sel,
                outline,
                tokens: &t,
            },
            &mut cf,
        )
    } else {
        paint_panes(
            PanesInput { app, painter: &painter, geo: &geo, workbook: &wb, sheet_index: si, sheet: sh, selection: &sel, outline, tokens: &t },
            &mut cf,
        )
    };
    // Objects above cells. A hidden object can't stay selected: Delete or the Format pane would act
    // on something the user can't see.
    if crate::chartview::objects_hidden(&wb) {
        app.selected_chart = None;
    }
    let objects = crate::chartview::paint_objects(app, &painter.with_clip_rect(geo.cells), &geo, &wb, si, sh);
    paint_overlays(OverlayInput {
        app,
        ui,
        painter: &painter.with_clip_rect(geo.cells),
        geo: &geo,
        workbook: &wb,
        sheet_index: si,
        sheet: sh,
        selection: &sel,
    });
    if sh.view == gridcraft_engine::model::SheetViewMode::PageBreakPreview {
        paint_pages(app, &painter.with_clip_rect(geo.cells), &geo, sh);
    }
    if geo.paper.is_some() {
        crate::paper_view::paint_headers_footers(app, ui, &painter, &geo, &wb, sh);
    }
    // Headers.
    if sh.show_headings {
        let headings = table_headings(&geo, sh, sel.active);
        paint_headers(&painter, &geo, sh, &sel, &t, &quads, &headings);
        paint_freeze_handles(&painter, &geo, sh, &app.grid.drag, &t);
    }
    crate::outline::paint(&painter, &geo, sh, &t);
    if geo.rtl {
        let mirror_to = painter.add(egui::Shape::Noop);
        mirror_painted(ui.ctx(), painter.layer_id(), mirror_from.0..mirror_to.0, &geo, &objects);
    }
    if geo.paper.is_some() {
        crate::paper_view::paint_rulers(app, &painter, rect, &geo, &t);
    }
    show_grid_interactions(app, ui, &resp, &geo, sh, &wb);
    let track_x = vertical_scroll_track(available, rect, rtl);
    // The track starts below the column headings and frozen rows; the bar's column beside them
    // takes its colour too instead of showing a light square in dark mode.
    ui.painter().rect_filled(Rect::from_min_max(pos2(track_x.0, rect.top()), pos2(track_x.1, rect.bottom())), 0.0, t.tab_bar);
    vertical_scrollbar(app, ui, sh, &geo, track_x, max_y, &t);
    update_clipboard_repaint(app, ui, now);
}

fn vertical_scroll_track(available: Rect, grid: Rect, rtl: bool) -> (f32, f32) {
    if rtl { (available.left(), grid.left()) } else { (grid.right(), available.right()) }
}

fn update_clipboard_repaint(app: &mut SheetApp, ui: &egui::Ui, now: f64) {
    if app.session.clipboard.is_some() {
        // By the clock, not per frame: a gliding outline repaints more often than the ants step.
        app.grid.marching_phase = crate::selection_glide::ants_phase(now);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
}

struct ActiveCellVisibility<'a, F> {
    app: &'a mut SheetApp,
    wb: &'a Workbook,
    si: usize,
    sh: &'a Sheet,
    selection: &'a gridcraft_engine::Selection,
    paper: bool,
    make_geo: &'a F,
}

fn selection_outline(
    app: &mut SheetApp,
    ui: &egui::Ui,
    geo: &Geo,
    sh: &Sheet,
    sel: &gridcraft_engine::Selection,
    place: (u64, usize),
    now: f64,
) -> Rect {
    let pointer_selecting = ui.input(|i| i.pointer.any_down());
    let outline = app.grid.glide.frame(crate::selection_glide::Frame {
        place,
        range: sel.current(),
        target: geo.range_rect(sh, sel.current()),
        view: geo.cells,
        now,
        frame_nr: ui.ctx().cumulative_frame_nr(),
        animate: app.ui.animations && ui.ctx().global_style().animation_time > 0.0 && !app.shots.in_flight() && !pointer_selecting,
    });
    if app.grid.glide.is_gliding() {
        ui.ctx().request_repaint();
    }
    outline
}

impl<F: Fn(&mut SheetApp, egui::Vec2) -> Geo> ActiveCellVisibility<'_, F> {
    fn update(&mut self, view: &mut crate::SheetView, geo: &mut Geo) {
        if !self.app.grid.ensure_visible {
            return;
        }
        let app = &mut *self.app;
        let wb = self.wb;
        let si = self.si;
        let sh = self.sh;
        let sel = self.selection;
        let paper = self.paper;

        app.grid.ensure_visible = false;
        let moving = std::mem::take(&mut app.grid.reveal_moving_end);
        let cell = if moving { sel.moving_end() } else { sel.active };
        let from = app.grid.reveal_from.take();
        let target = app.editor.as_ref().and_then(|e| e.point_cell).unwrap_or(cell);
        // A whole column's moving end sits on the sheet's last row (a whole row's on its last
        // column): when an extension moves it only across, the view keeps its place the other way.
        // An extension that reaches the last row itself (Ctrl+Shift+Down past the data) still
        // follows it.
        let extended = (moving && target == cell).then(|| sel.current());
        let before = view.scroll;
        if paper {
            crate::paper_view::ensure_covers(app, wb, si, target);
            *geo = (self.make_geo)(app, view.scroll);
        }
        let cr = geo.cell_rect(sh, target);
        let area = Rect::from_min_max(pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h), geo.cells.max);
        if target.row >= geo.fr {
            if cr.top() < area.top() {
                view.scroll.y -= (area.top() - cr.top()) / geo.z;
            } else if cr.bottom() > area.bottom() - 2.0 {
                view.scroll.y += (cr.bottom() - area.bottom() + 2.0) / geo.z;
            }
        } else if from.is_some_and(|f| f.row >= geo.fr) {
            // A move from the scrolled rows into the frozen ones (Page Up or Ctrl+Up onto a frozen
            // row 1) brings the scrolled rows back to their top; the rows it passed over would
            // otherwise stay out of sight.
            view.scroll.y = 0.0;
        }
        if target.col >= geo.fc {
            if cr.left() < area.left() {
                view.scroll.x -= (area.left() - cr.left()) / geo.z;
            } else if cr.right() > area.right() - 2.0 {
                view.scroll.x += ((cr.right() - area.right() + 2.0) / geo.z).min(cr.left() - area.left());
            }
        } else if from.is_some_and(|f| f.col >= geo.fc) {
            view.scroll.x = 0.0;
        }
        if extended.is_some_and(|r| r.is_full_cols()) && from.is_some_and(|f| f.row == cell.row) {
            view.scroll.y = before.y;
        }
        if extended.is_some_and(|r| r.is_full_rows()) && from.is_some_and(|f| f.col == cell.col) {
            view.scroll.x = before.x;
        }
        view.scroll = view.scroll.max(egui::Vec2::ZERO);
        *geo = (self.make_geo)(app, view.scroll);
    }
}

fn show_grid_interactions(app: &mut SheetApp, ui: &mut egui::Ui, resp: &egui::Response, geo: &Geo, sh: &Sheet, wb: &Workbook) {
    let previous_popups = (app.grid.filter_menu, app.grid.list_picker, app.grid.header_menu, app.grid.fill_menu);
    // A pan the grid never saw end (it was hidden mid-drag) must not swallow the next press.
    if app.grid.touch_pan && !resp.dragged() && !resp.drag_stopped() {
        app.grid.touch_pan = false;
    }
    if app.grid.touch_pan {
        // Skip the frames of a touch pan, its release included.
        if !resp.dragged() {
            app.grid.touch_pan = false;
        }
    } else {
        crate::paper_view::interact(app, ui, resp, geo, sh, wb);
        interact(app, ui, resp, geo, sh, wb);
    }
    update_cursor(app, ui, resp, geo, sh);
    crate::text_box::show(app, ui, geo, sh);
    if app.editor.as_ref().is_some_and(|e| !e.from_formula_bar) {
        in_cell_editor(app, ui, geo, sh, wb);
    }
    filter_menu(app, ui, geo, previous_popups.0 == app.grid.filter_menu);
    list_picker(app, ui, geo, previous_popups.1 == app.grid.list_picker);
    header_menu(app, ui, previous_popups.2 == app.grid.header_menu);
    context_menu(app, ui);
    fill_menu(app, ui, previous_popups.3 == app.grid.fill_menu);
    fill_options_tag(app, ui, geo, sh);
    object_menu(app, ui);
    error_check_tag(app, ui, geo, sh);
}

fn update_zoom_glide(app: &mut SheetApp, ui: &egui::Ui, place: (u64, usize), zoom: u16, now: f64) {
    let mut zooming = WheelZoom::at(app, place, zoom, now);
    if let Some((to, shown)) = zooming.glide {
        let glides = app.ui.animations && ui.ctx().global_style().animation_time > 0.0 && !app.shots.in_flight();
        let share = egui::emath::exponential_smooth_factor(0.9, 0.1, ui.input(|i| i.stable_dt).min(0.1));
        let z = if glides { zoom_glide_step(shown, to, share) } else { to };
        let _ = app.session.run("view.zoom", json!({"percent": z}));
        zooming.glide = (z != to).then_some((to, z));
        ui.ctx().request_repaint();
    }
    if app.grid.wheel_zoom.is_some_and(|w| w.glide.is_some()) {
        app.grid.wheel_zoom = Some(zooming);
    }
}

/// Normal view: up to four panes (frozen rows/columns), each painted with its own clip.
struct PanesInput<'a> {
    app: &'a SheetApp,
    painter: &'a Painter,
    geo: &'a Geo,
    workbook: &'a Workbook,
    sheet_index: usize,
    sheet: &'a Sheet,
    selection: &'a gridcraft_engine::Selection,
    outline: Rect,
    tokens: &'a Tokens,
}

fn paint_panes(input: PanesInput<'_>, cf: &mut CfCache) -> Vec<PaneQuad> {
    let PanesInput { app, painter, geo, workbook: wb, sheet_index: si, sheet: sh, selection: sel, outline, tokens: t } = input;
    // Quadrants: (rows, cols, clip).
    let (sr0, sr1) = geo.scroll_rows(sh);
    let (sc0, sc1) = geo.scroll_cols(sh);
    let fr_rows = (0, geo.fr.saturating_sub(1));
    let fr_cols = (0, geo.fc.saturating_sub(1));
    let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
    let mut quads = vec![((sr0, sr1), (sc0, sc1), Rect::from_min_max(split, geo.cells.max))];
    if geo.fr > 0 {
        quads.push((fr_rows, (sc0, sc1), Rect::from_min_max(pos2(split.x, geo.cells.top()), pos2(geo.cells.right(), split.y))));
    }
    if geo.fc > 0 {
        quads.push(((sr0, sr1), fr_cols, Rect::from_min_max(pos2(geo.cells.left(), split.y), pos2(split.x, geo.cells.bottom()))));
    }
    if geo.fr > 0 && geo.fc > 0 {
        quads.push((fr_rows, fr_cols, Rect::from_min_max(geo.cells.min, split)));
    }
    for (rows, cols, clip) in &quads {
        if rows.0 > rows.1 || cols.0 > cols.1 {
            continue;
        }
        let p = painter.with_clip_rect(*clip);
        paint_quadrant(QuadrantPaint { painter: &p, geo, workbook: wb, sheet_index: si, sheet: sh, rows: *rows, cols: *cols, cf, tokens: t });
        paint_selection(SelectionPaintInput { painter: &p, geo, sheet: sh, selection: sel, outline, tokens: t, app, rows: *rows, cols: *cols });
    }
    // Freeze lines.
    if geo.fr > 0 {
        painter.line_segment([pos2(geo.cells.left(), split.y), pos2(geo.cells.right(), split.y)], Stroke::new(1.0, Color32::from_gray(150)));
    }
    if geo.fc > 0 {
        painter.line_segment([pos2(split.x, geo.cells.top()), pos2(split.x, geo.cells.bottom())], Stroke::new(1.0, Color32::from_gray(150)));
    }
    quads
}

/// The worksheet is painted virtually, so it needs its own scrollbar rather than a
/// ScrollArea containing all million rows. Keep the scroll extent stable during a drag.
fn vertical_scrollbar(app: &mut SheetApp, ui: &mut egui::Ui, sh: &Sheet, geo: &Geo, track_x: (f32, f32), limit: f32, t: &Tokens) {
    let track = Rect::from_min_max(pos2(track_x.0, geo.cells.top() + geo.frozen_h), pos2(track_x.1, geo.cells.bottom()));
    if track.height() <= 0.0 || track.width() <= 0.0 {
        return;
    }
    let visible = track.height() / geo.z;
    let scroll = app.view().scroll.y;
    let last = sh.used_range().map(|r| r.end.row.saturating_add(10)).unwrap_or(100).clamp(100, MAX_ROWS - 1);
    let content = geo.scroll_top_of(sh, last);
    let extent = (content - visible).max(scroll).max(0.0).min(limit);
    let id = ui.scope_id().with("vscroll");
    let resp = ui.interact(track, id, Sense::click_and_drag());
    let drag_id = id.with("extent");
    if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, extent));
    }
    let extent = if resp.dragged() { ui.ctx().data(|d| d.get_temp::<f32>(drag_id)).unwrap_or(extent) } else { extent };
    let thumb_h = (track.height() * visible / (extent + visible)).clamp(24.0_f32.min(track.height()), track.height());
    let travel = track.height() - thumb_h;
    let thumb_top = track.top() + if extent > 0.0 { travel * (scroll / extent).clamp(0.0, 1.0) } else { 0.0 };
    let thumb = Rect::from_min_size(pos2(track.left() + 3.0, thumb_top), vec2((track.width() - 6.0).max(0.0), thumb_h));
    ui.painter().rect_filled(track, 0.0, t.tab_bar);
    ui.painter().rect_filled(thumb, 4.0, if resp.hovered() || resp.dragged() { t.text_dim } else { t.text_disabled });
    let mut next = scroll;
    if resp.dragged() && travel > 0.0 {
        next += resp.drag_delta().y / travel * extent;
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        if p.y < thumb.top() {
            next -= visible;
        } else if p.y > thumb.bottom() {
            next += visible;
        }
    }
    if resp.hovered() {
        next -= ui.input(|i| i.smooth_scroll_delta.y) / geo.z;
    }
    next = next.clamp(0.0, limit);
    if next != scroll {
        if let Some(v) = app.view_mut() {
            v.scroll.y = next;
        }
        ui.ctx().request_repaint();
    }
}

/// The horizontal counterpart of `vertical_scrollbar`, drawn in the sheet tab bar as in Excel. The tab
/// bar is laid out before the grid, so this reads the last frame's geometry and leaves clamping the
/// far end to the grid, which runs later in the same frame.
pub(crate) fn horizontal_scrollbar(app: &mut SheetApp, ui: &mut egui::Ui, track: Rect, t: &Tokens) {
    app.grid.hscroll = Some(track);
    if track.height() <= 0.0 || track.width() <= 0.0 {
        return;
    }
    let Some(geo) = app.grid.geo.clone() else { return };
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    let Some(sh) = wb.active() else { return };
    let visible = ((geo.cells.width() - geo.frozen_w) / geo.z).max(0.0);
    let scroll = app.view().scroll.x;
    let last = sh.used_range().map(|r| r.end.col.saturating_add(10)).unwrap_or(26).clamp(26, MAX_COLS - 1);
    let content = geo.scroll_left_of(sh, last);
    let extent = (content - visible).max(scroll).max(0.0);
    let id = ui.scope_id().with("hscroll");
    let resp = ui.interact(track, id, Sense::click_and_drag());
    let drag_id = id.with("extent");
    if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, extent));
    }
    let extent = if resp.dragged() { ui.ctx().data(|d| d.get_temp::<f32>(drag_id)).unwrap_or(extent) } else { extent };
    let thumb_w = (track.width() * visible / (extent + visible).max(1.0)).clamp(24.0_f32.min(track.width()), track.width());
    let travel = track.width() - thumb_w;
    let offset = if extent > 0.0 { travel * (scroll / extent).clamp(0.0, 1.0) } else { 0.0 };
    // A right-to-left sheet scrolls towards the left, so its thumb starts at the right end.
    let dir = if geo.rtl { -1.0 } else { 1.0 };
    let thumb_left = if geo.rtl { track.right() - thumb_w - offset } else { track.left() + offset };
    let thumb = Rect::from_min_size(pos2(thumb_left, track.top() + 3.0), vec2(thumb_w, (track.height() - 6.0).max(0.0)));
    ui.painter().rect_filled(track, 0.0, t.tab_bar);
    ui.painter().rect_filled(thumb, 4.0, if resp.hovered() || resp.dragged() { t.text_dim } else { t.text_disabled });
    let mut next = scroll;
    if resp.dragged() && travel > 0.0 {
        next += dir * resp.drag_delta().x / travel * extent;
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        if p.x < thumb.left() {
            next -= dir * visible;
        } else if p.x > thumb.right() {
            next += dir * visible;
        }
    }
    if resp.hovered() {
        let d = ui.input(|i| i.smooth_scroll_delta);
        next -= (if d.x != 0.0 { dir * d.x } else { d.y }) / geo.z;
    }
    next = next.max(0.0);
    if next != scroll {
        if let Some(v) = app.view_mut() {
            v.scroll.x = next;
        }
        ui.ctx().request_repaint();
    }
}

pub(crate) fn paint_headers(
    p: &Painter,
    geo: &Geo,
    sh: &Sheet,
    sel: &gridcraft_engine::Selection,
    t: &Tokens,
    quads: &[PaneQuad],
    headings: &[TableHeading],
) {
    let font = theme::ui_font(heading_font_size(geo.z));
    // The headers' own area: inside the outline bands.
    let r = Rect::from_min_max(pos2(geo.head_left(), geo.head_top()), geo.rect.max);
    let top = Rect::from_min_max(pos2(geo.cells.left(), r.top()), pos2(r.right(), geo.cells.top()));
    let left = Rect::from_min_max(pos2(r.left(), geo.cells.top()), pos2(geo.cells.left(), r.bottom()));
    p.rect_filled(top, 0.0, t.header_bg);
    p.rect_filled(left, 0.0, t.header_bg);
    let bounds = sel.bounds();
    let col_sel = |c: u32| sel.ranges.iter().any(|x| c >= x.start.col && c <= x.end.col);
    let row_sel = |rr: u32| sel.ranges.iter().any(|x| rr >= x.start.row && rr <= x.end.row);
    let full_col = |c: u32| sel.ranges.iter().any(|x| x.is_full_cols() && c >= x.start.col && c <= x.end.col);
    let full_row = |rr: u32| sel.ranges.iter().any(|x| x.is_full_rows() && rr >= x.start.row && rr <= x.end.row);
    let _ = bounds;
    // Hidden columns and rows leave a double divider where they sit, as in Excel.
    let hidden_mark = Stroke::new(1.0, t.header_text.gamma_multiply(0.6));
    let mut done_cols = std::collections::HashSet::new();
    let mut done_rows = std::collections::HashSet::new();
    for (rows, cols, clip) in quads {
        let pc = p.with_clip_rect(Rect::from_min_max(pos2(clip.left(), top.top()), pos2(clip.right(), top.bottom())));
        if cols.0 <= cols.1 {
            for c in cols.0..=cols.1 {
                if !done_cols.insert((c, clip.left() as i32)) {
                    continue;
                }
                let w = sh.col_width(c) * geo.z;
                if w <= 0.0 {
                    continue;
                }
                let x = geo.x(sh, c);
                let cr = Rect::from_min_size(pos2(x, top.top()), vec2(w, top.height()));
                if col_sel(c) {
                    pc.rect_filled(cr, 0.0, if full_col(c) { t.header_all_bg } else { t.header_sel_bg });
                    pc.line_segment([pos2(cr.left(), cr.bottom() - 1.0), pos2(cr.right(), cr.bottom() - 1.0)], Stroke::new(2.0, t.accent));
                }
                pc.line_segment([pos2(cr.right() - 0.5, cr.top() + 3.0), pos2(cr.right() - 0.5, cr.bottom())], Stroke::new(1.0, t.header_line));
                let (before, after) = hidden_around(c, |i| sh.col_width(i));
                for x in [before.then_some(cr.left()), after.then_some(cr.right())].into_iter().flatten() {
                    for dx in [-1.5, 1.5] {
                        pc.line_segment([pos2(x + dx, cr.top() + 3.0), pos2(x + dx, cr.bottom())], hidden_mark);
                    }
                }
                let color = if col_sel(c) { t.header_sel_text } else { t.header_text };
                match headings.iter().find(|h| h.col == c) {
                    Some(h) => {
                        // The name is cut off at the button, as a cell's text is at its neighbour.
                        let end = h.button.map_or(cr.right(), |b| b.left()) - 2.0;
                        let room = Rect::from_min_max(cr.min, pos2(end.max(cr.left()), cr.bottom())).intersect(pc.clip_rect());
                        let pad = 4.0 * (geo.z / DISPLAY_SCALE).max(0.75);
                        pc.with_clip_rect(room).text(pos2(cr.left() + pad, cr.center().y), Align2::LEFT_CENTER, &h.name, font.clone(), color);
                        if let Some(b) = h.button {
                            paint_filter_button(&pc, b, h.filtered);
                        }
                    }
                    None => {
                        pc.text(cr.center(), Align2::CENTER_CENTER, col_to_letters(c), font.clone(), color);
                    }
                }
            }
        }
        let pr = p.with_clip_rect(Rect::from_min_max(pos2(left.left(), clip.top()), pos2(left.right(), clip.bottom())));
        if rows.0 <= rows.1 {
            for rr in rows.0..=rows.1 {
                if !done_rows.insert((rr, clip.top() as i32)) {
                    continue;
                }
                let h = sh.row_height(rr) * geo.z;
                if h <= 0.0 {
                    continue;
                }
                let y = geo.y(sh, rr);
                let cr = Rect::from_min_size(pos2(left.left(), y), vec2(left.width(), h));
                if row_sel(rr) {
                    pr.rect_filled(cr, 0.0, if full_row(rr) { t.header_all_bg } else { t.header_sel_bg });
                    pr.line_segment([pos2(cr.right() - 1.0, cr.top()), pos2(cr.right() - 1.0, cr.bottom())], Stroke::new(2.0, t.accent));
                }
                pr.line_segment([pos2(cr.left() + 3.0, cr.bottom() - 0.5), pos2(cr.right(), cr.bottom() - 0.5)], Stroke::new(1.0, t.header_line));
                let (before, after) = hidden_around(rr, |i| sh.row_height(i));
                for y in [before.then_some(cr.top()), after.then_some(cr.bottom())].into_iter().flatten() {
                    for dy in [-1.5, 1.5] {
                        pr.line_segment([pos2(cr.left() + 3.0, y + dy), pos2(cr.right(), y + dy)], hidden_mark);
                    }
                }
                let color = if row_sel(rr) { t.header_sel_text } else { t.header_text };
                pr.text(cr.center(), Align2::CENTER_CENTER, (rr as u64 + 1).to_string(), font.clone(), color);
            }
        }
    }
    // Corner (select all) and edges.
    let corner = Rect::from_min_max(r.min, geo.cells.min);
    p.rect_filled(corner, 0.0, t.header_bg);
    let tri = vec![
        pos2(corner.right() - 3.0, corner.top() + 5.0),
        pos2(corner.right() - 3.0, corner.bottom() - 3.0),
        pos2(corner.left() + 7.0, corner.bottom() - 3.0),
    ];
    p.add(egui::Shape::convex_polygon(tri, Color32::from_gray(190), Stroke::NONE));
    p.line_segment([pos2(r.left(), geo.cells.top() - 0.5), pos2(r.right(), geo.cells.top() - 0.5)], Stroke::new(1.0, t.header_line));
    p.line_segment([pos2(geo.cells.left() - 0.5, r.top()), pos2(geo.cells.left() - 0.5, r.bottom())], Stroke::new(1.0, t.header_line));
}

/// Whether the line before / after visible line `i` is hidden (zero size), so its header edges
/// get the hidden marker. Line 0 has nothing before it.
pub(super) fn hidden_around(i: u32, size: impl Fn(u32) -> f32) -> (bool, bool) {
    (i > 0 && size(i - 1) <= 0.0, i < u32::MAX && size(i + 1) <= 0.0)
}

/// Paints the validation arrow and input message, notes, the hyperlink tooltip and trace precedent/dependent arrows.
pub(crate) struct OverlayInput<'a> {
    app: &'a mut SheetApp,
    ui: &'a egui::Ui,
    painter: &'a Painter,
    geo: &'a Geo,
    workbook: &'a Workbook,
    sheet_index: usize,
    sheet: &'a Sheet,
    selection: &'a gridcraft_engine::Selection,
}

pub(crate) fn paint_overlays(input: OverlayInput<'_>) {
    let OverlayInput { app, ui, painter: p, geo, workbook: wb, sheet_index: si, sheet: sh, selection: sel } = input;
    // Validation dropdown arrow for the active cell.
    if app.editor.is_none()
        && let Some(r) = validation_arrow(sh, geo, sel.active)
    {
        let t = Tokens::get(p.ctx());
        p.rect_filled(r, 2.0, t.sheet_control_bg);
        p.rect_stroke(r, 2.0, Stroke::new(1.0, t.sheet_control_border), StrokeKind::Inside);
        crate::icons::paint_on_sheet(p, r.shrink(3.0), crate::icons::Icon::Chevron, t.sheet_control_text);
    }
    // Input message of the active cell's validation, hidden while its list is open: it would cover the items.
    if app.editor.is_none()
        && app.grid.list_picker != Some(sel.active)
        && let Some(dv) = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(sel.active)))
        && dv.show_input
        && !(dv.input_title.is_empty() && dv.input_message.is_empty())
        && !geo.hidden_under_frozen(sh, sel.active)
    {
        let r = geo.cell_rect(sh, sel.active);
        let (at, pivot) = (geo.mirror_pos(pos2(r.left() + 8.0, r.bottom() + 6.0)), if geo.rtl { Align2::RIGHT_TOP } else { Align2::LEFT_TOP });
        egui::Area::new(egui::Id::unique("dv_input")).fixed_pos(at).pivot(pivot).order(egui::Order::Tooltip).interactable(false).show(
            ui.ctx(),
            |ui| {
                egui::Frame::popup(ui.style()).fill(Color32::from_rgb(0xFF, 0xFF, 0xE1)).show(ui, |ui| {
                    ui.set_max_width(220.0);
                    if !dv.input_title.is_empty() {
                        ui.label(egui::RichText::new(&dv.input_title).strong().color(Color32::BLACK));
                    }
                    ui.label(egui::RichText::new(&dv.input_message).color(Color32::BLACK));
                });
            },
        );
    }
    // Notes: shown on hover (or always when marked visible).
    let hover = app.grid.hover_cell;
    let all_notes = app.session.show_all_notes;
    for (c, cm) in &sh.comments {
        let show = cm.visible || (all_notes && !cm.threaded) || hover == Some(*c);
        if !show || app.grid.drag != Drag::None {
            continue;
        }
        let r = geo.cell_rect(sh, *c);
        if !p.clip_rect().intersects(r) || geo.hidden_under_frozen(sh, *c) {
            continue;
        }
        let at = pos2(r.right() + 10.0, r.top());
        p.line_segment([r.right_top(), at + vec2(0.0, 6.0)], Stroke::new(1.0, Color32::from_gray(90)));
        let (at, pivot) = (geo.mirror_pos(at), if geo.rtl { Align2::RIGHT_TOP } else { Align2::LEFT_TOP });
        egui::Area::new(egui::Id::unique(("note", c.row, c.col))).fixed_pos(at).pivot(pivot).order(egui::Order::Tooltip).interactable(false).show(
            ui.ctx(),
            |ui| {
                egui::Frame::popup(ui.style()).fill(if cm.threaded { Color32::WHITE } else { Color32::from_rgb(0xFF, 0xFF, 0xE1) }).show(ui, |ui| {
                    ui.set_max_width(240.0);
                    ui.label(egui::RichText::new(format!("{}:", cm.author)).strong().color(Color32::BLACK));
                    ui.label(egui::RichText::new(&cm.text).color(Color32::BLACK));
                    for (a, t) in &cm.replies {
                        ui.separator();
                        ui.label(egui::RichText::new(format!("{a}: {t}")).color(Color32::BLACK));
                    }
                });
            },
        );
    }
    // Hyperlink tooltip.
    if let Some(h) = hover.and_then(|c| sh.hyperlinks.get(&c))
        && app.editor.is_none()
    {
        let tip = h.tooltip.clone().unwrap_or_else(|| format!("{}\nClick once to follow. Click and hold to select this cell.", h.target));
        egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::unique("link_tip"), egui::PopupAnchor::Pointer).show(|ui| {
            ui.label(tip);
        });
    }
    if let Some(o) = &app.grid.macro_overlay {
        crate::macro_editor::paint_overlay(p, geo, sh, o);
    }
    // Trace precedents / dependents arrows.
    if let Some(trace) = &app.grid.trace
        && app.session.active().is_some_and(|d| d.uid == trace.uid && d.revision == trace.revision)
        && trace.sheet == si
        && let Some(items) = trace.items.as_array()
    {
        let blue = Color32::from_rgb(0x1F, 0x5F, 0xC9);
        let active = geo.cell_rect(sh, trace.cell).center();
        for it in items {
            if it["sheet"].as_str().is_some_and(|s| !s.eq_ignore_ascii_case(&sh.name)) {
                continue;
            }
            let target = it["range"].as_str().or(it["cell"].as_str()).and_then(RangeRef::parse);
            let Some(t) = target else { continue };
            let tr = geo.range_rect(sh, t);
            if !t.is_single() {
                for pane in geo.range_panes(t) {
                    p.with_clip_rect(pane.intersect(p.clip_rect())).rect_stroke(tr, 0.0, Stroke::new(1.5, blue), StrokeKind::Inside);
                }
            }
            let (from, to) = if it.get("range").is_some() { (tr.center(), active) } else { (active, tr.center()) };
            p.line_segment([from, to], Stroke::new(1.5, blue));
            p.circle_filled(from, 3.0, blue);
            let d = (to - from).normalized();
            let n = vec2(-d.y, d.x);
            p.add(egui::Shape::convex_polygon(vec![to, to - d * 9.0 + n * 4.0, to - d * 9.0 - n * 4.0], blue, Stroke::NONE));
        }
    }
    let _ = (wb, si);
}

/// Paints cached page ranges, page labels and the print-area frame in the active sheet view.
fn paint_pages(app: &mut SheetApp, p: &Painter, geo: &Geo, sh: &Sheet) {
    let Some(d) = app.session.active() else { return };
    let key = (d.uid, d.revision, d.wb.active_sheet());
    if app.grid.pages.as_ref().is_none_or(|(k, _)| *k != key) {
        let pages: Vec<(RangeRef, u32)> = app
            .session
            .run("file.printPreview", json!({}))
            .ok()
            .and_then(|v| {
                v["pageList"].as_array().map(|a| {
                    a.iter()
                        .filter(|pg| pg["sheet"].as_str() == Some(sh.name.as_str()))
                        .filter_map(|pg| Some((RangeRef::parse(pg["range"].as_str()?)?, pg["page"].as_u64()? as u32)))
                        .collect()
                })
            })
            .unwrap_or_default();
        app.grid.pages = Some((key, pages));
    }
    let Some((_, pages)) = &app.grid.pages else { return };
    if pages.is_empty() {
        return;
    }
    // Grey everything, then clear the printed pages.
    let mut printed = pages[0].0;
    for (r, _) in pages {
        printed = printed.union(r);
    }
    let pr = geo.range_rect(sh, printed);
    let grey = Color32::from_black_alpha(70);
    let c = p.clip_rect();
    for r in [
        Rect::from_min_max(c.min, pos2(c.right(), pr.top())),
        Rect::from_min_max(pos2(c.left(), pr.bottom()), c.max),
        Rect::from_min_max(pos2(c.left(), pr.top()), pos2(pr.left(), pr.bottom())),
        Rect::from_min_max(pos2(pr.right(), pr.top()), pos2(c.right(), pr.bottom())),
    ] {
        if r.is_positive() {
            p.rect_filled(r, 0.0, grey);
        }
    }
    let blue = Color32::from_rgb(0x2E, 0x6F, 0xD8);
    for (r, n) in pages {
        let rr = geo.range_rect(sh, *r);
        if !rr.intersects(c) {
            continue;
        }
        p.extend(egui::Shape::dashed_line(
            &[rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()],
            Stroke::new(2.0, blue),
            6.0,
            3.0,
        ));
        p.text(
            rr.center(),
            Align2::CENTER_CENTER,
            app.ui.language.tr("Page {page}").replace("{page}", &n.to_string()),
            theme::ui_bold((rr.height().min(rr.width()) / 6.0).clamp(14.0, 72.0)),
            Color32::from_black_alpha(45),
        );
    }
    p.rect_stroke(pr, 0.0, Stroke::new(3.0, blue), StrokeKind::Outside);
}

#[cfg(test)]
mod trace_arrow_tests {
    use super::*;

    fn app() -> SheetApp {
        let mut s = gridcraft_engine::Session::new();
        s.new_workbook();
        SheetApp::new(s, Default::default())
    }

    fn select(app: &mut SheetApp, cell: &str) {
        app.run("selection.set", json!({"cell": cell})).unwrap();
    }

    /// What the ribbon's Trace Precedents / Trace Dependents buttons do.
    fn trace(app: &mut SheetApp, id: &str) {
        let r = app.run(id, json!({})).unwrap();
        app.grid.trace = Trace::new(app, r);
    }

    /// The arrows `paint_overlays` draws on the active sheet, as (tail, head) points.
    fn arrows(app: &mut SheetApp) -> (Geo, Vec<(Pos2, Pos2)>) {
        let d = app.session.active().unwrap();
        let (wb, si, sel) = (d.wb.clone(), d.wb.active_sheet(), d.selection.clone());
        let sh = wb.sheet(si).unwrap();
        let geo = Geo::new(sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0)), vec2(0.0, 0.0));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.painter().clone();
            paint_overlays(OverlayInput { app, ui, painter: &p, geo: &geo, workbook: &wb, sheet_index: si, sheet: sh, selection: &sel });
        });
        // No renderer consumes the font atlas upload here.
        out.textures_delta.clear();
        let blue = Color32::from_rgb(0x1F, 0x5F, 0xC9);
        let mut lines = Vec::new();
        for s in out.shapes {
            if let egui::Shape::LineSegment { points, stroke } = s.shape
                && stroke.color == blue
            {
                lines.push((points[0], points[1]));
            }
        }
        (geo, lines)
    }

    fn center(app: &SheetApp, geo: &Geo, cell: &str) -> Pos2 {
        let d = app.session.active().unwrap();
        let sh = d.wb.sheet(d.wb.active_sheet()).unwrap();
        geo.cell_rect(sh, CellRef::parse(cell).unwrap()).center()
    }

    #[test]
    fn precedent_arrows_stay_on_the_traced_cell_when_the_selection_moves() {
        let mut app = app();
        for (c, v) in [("B2", "1"), ("B3", "2"), ("D2", "1"), ("D3", "2"), ("F7", "=B2+B3+SUM(D2:D3)")] {
            app.run("cell.set", json!({"cell": c, "input": v})).unwrap();
        }
        select(&mut app, "F7");
        trace(&mut app, "formulas.tracePrecedents");
        select(&mut app, "H7");
        let (geo, lines) = arrows(&mut app);
        assert_eq!(lines.len(), 3, "B2, B3 and D2:D3: {lines:?}");
        let f7 = center(&app, &geo, "F7");
        assert!(lines.iter().all(|&(_, head)| head == f7), "every arrow points at F7, not at the active H7: {lines:?}");
    }

    #[test]
    fn dependent_arrows_stay_on_the_traced_cell_when_the_selection_moves() {
        let mut app = app();
        for (c, v) in [("B2", "1"), ("D4", "=B2*2"), ("E6", "=B2+1")] {
            app.run("cell.set", json!({"cell": c, "input": v})).unwrap();
        }
        select(&mut app, "B2");
        trace(&mut app, "formulas.traceDependents");
        select(&mut app, "C9");
        let (geo, lines) = arrows(&mut app);
        assert_eq!(lines.len(), 2, "D4 and E6: {lines:?}");
        let b2 = center(&app, &geo, "B2");
        assert!(lines.iter().all(|&(tail, _)| tail == b2), "every arrow starts at B2, not at the active C9: {lines:?}");
    }

    #[test]
    fn arrows_stay_on_the_sheet_of_the_traced_cell() {
        let mut app = app();
        app.run("home.insertSheet", json!({"name": "Two"})).unwrap();
        app.run("cell.set", json!({"sheet": "Sheet1", "cell": "B2", "input": "1"})).unwrap();
        app.run("sheet.activate", json!({"sheet": "Two"})).unwrap();
        app.run("cell.set", json!({"cell": "F7", "input": "=Sheet1!B2"})).unwrap();
        select(&mut app, "F7");
        trace(&mut app, "formulas.tracePrecedents");
        // The precedent lives on Sheet1, but the arrow belongs to Two's F7.
        app.run("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
        assert!(arrows(&mut app).1.is_empty(), "no arrow on Sheet1 from B2 to where F7 would be");
    }

    #[test]
    fn inserting_a_row_removes_the_arrows() {
        let mut app = app();
        app.run("cell.set", json!({"cell": "B2", "input": "1"})).unwrap();
        app.run("cell.set", json!({"cell": "F7", "input": "=B2"})).unwrap();
        select(&mut app, "F7");
        trace(&mut app, "formulas.tracePrecedents");
        assert_eq!(arrows(&mut app).1.len(), 1);
        app.run("home.insertRows", json!({"rows": "4"})).unwrap();
        assert!(arrows(&mut app).1.is_empty(), "F7 is now F8: the old arrows would point at the wrong cell");
    }

    #[test]
    fn arrows_stay_in_their_workbook() {
        let mut app = app();
        app.session.new_workbook();
        // Two books with the same cells and the same revision: only the document tells them apart.
        for book in [1, 0] {
            app.session.set_active(book);
            app.run("cell.set", json!({"cell": "B2", "input": "1"})).unwrap();
            app.run("cell.set", json!({"cell": "F7", "input": "=B2"})).unwrap();
        }
        select(&mut app, "F7");
        trace(&mut app, "formulas.tracePrecedents");
        assert_eq!(arrows(&mut app).1.len(), 1);
        app.session.set_active(1);
        let revisions: Vec<u64> = app.session.documents().iter().map(|d| d.revision).collect();
        assert_eq!(revisions[0], revisions[1]);
        assert!(arrows(&mut app).1.is_empty(), "Book1's arrows are not drawn on Book2");
    }
}
