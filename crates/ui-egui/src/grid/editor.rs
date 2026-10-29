use super::cell_paint::ink_on;
use super::geo::Geo;
use super::{Align2, CellRef, Color32, FontId, PatternType, Rect, Sheet, SheetApp, Stroke, StrokeKind, Tokens, Workbook, json, pos2, theme, vec2};

/// Room the in-cell editor keeps right of its text. It is sized from the text of the last frame, so
/// the next typed character must still fit: a fixed 14 px was less than one glyph at high zoom,
/// and the field scrolled and snapped back on every key (#2336). `size` is the font size in px.
pub(super) fn editor_slack(size: f32) -> f32 {
    14.0 + size
}

/// Width of the one-line in-cell editor. While the text fits the cell (with the usual 14 px margin) the
/// editor is exactly the cell wide, as in Excel; once it overflows, the extra `editor_slack` applies.
pub(super) fn editor_width(text_w: f32, cell_w: f32, size: f32) -> f32 {
    if text_w + 14.0 <= cell_w { cell_w } else { text_w + editor_slack(size) }
}

/// Draws the in-cell editor, growing and wrapping with its text, mirroring for right-to-left sheets, choosing contrasting caret ink and clipping cells hidden under frozen panes.
pub(super) fn in_cell_editor(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet, wb: &Workbook) {
    let Some(ed) = app.editor.as_ref() else { return };
    if app.view_key() != Some((ed.doc, ed.sheet)) {
        return;
    }
    let cell = ed.cell;
    let rect = match sh.merge_at(cell) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, cell),
    };
    let st = wb.styles.get(sh.style_id(cell)).clone();
    let size = st.font.size * 96.0 / 72.0 * geo.z;
    let font = FontId::new(size, theme::cell_family(&st.font.name, st.font.bold, st.font.italic));
    let lines = ed.text.lines().count().max(1) as f32 + if ed.text.ends_with('\n') { 1.0 } else { 0.0 };
    // Grows with the text: the in-cell editor shows every line and never scrolls (the formula bar
    // is the one that stops at five lines).
    let shown = lines;
    // Wrap Text: the editor keeps the column's width and wraps, growing down as Excel's does.
    // Otherwise it widens to the text's one-line width.
    let wrap = st.align.wrap;
    let (w, h) = if wrap {
        // The field wraps at the width it is given (the editor's less its 2 px inset a side): its
        // frame is `Frame::NONE`, so no text margin is taken off.
        let wrapped = ui.painter().layout(ed.text.clone(), font.clone(), Color32::BLACK, (rect.width() - 4.0).max(1.0));
        (rect.width(), (wrapped.size().y.max(size * 1.3 * shown) + 4.0).max(rect.height()))
    } else {
        let text_w = ui.painter().layout_no_wrap(ed.text.clone(), font.clone(), Color32::BLACK).size().x;
        // Excel keeps the cell's height for one line and only grows for extra lines.
        let h = if lines > 1.0 { (size * 1.3 * lines + 4.0).max(rect.height()) } else { rect.height() };
        (editor_width(text_w, rect.width(), size).min(geo.cells.right() - rect.left()), h)
    };
    // Grows to the left on a right-to-left sheet.
    let erect = geo.mirror_rect(Rect::from_min_size(rect.min, vec2(w, h)));
    let fill = match st.fill.pattern {
        PatternType::Solid => st.fill.fg.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::WHITE),
        _ => Color32::WHITE,
    };
    let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
    let quadrant = Rect::from_min_max(
        pos2(if cell.col >= geo.fc { split.x } else { geo.cells.left() }, if cell.row >= geo.fr { split.y } else { geo.cells.top() }),
        pos2(if cell.col >= geo.fc { geo.cells.right() } else { split.x }, if cell.row >= geo.fr { geo.cells.bottom() } else { split.y }),
    );
    // A cell scrolled entirely under a frozen row/column keeps its edit (Escape still works),
    // but neither the cell nor its helper may paint. Test the cell's own visible quadrant and
    // give the shared editor an empty clip when the cell is hidden: a one-line cell editor's
    // field can be larger than the cell (egui 0.37 gives text fields a minimum height), so its
    // own max_rect is not a reliable "is the cell on screen" test at the frozen boundary. The
    // pre-wrap cell rect must also be positive: a hidden row/column has zero height/width, but
    // Wrap Text can expand the editor below/right of it, which must not paint over the next line.
    let cell_visible = rect.width() > 0.0 && rect.height() > 0.0;
    // `erect` and `ui.clip_rect()` are in screen coordinates; `quadrant` is logical until mirrored.
    let screen_quadrant = geo.mirror_rect(quadrant);
    let on_screen = cell_visible && erect.intersect(screen_quadrant).is_positive() && !geo.hidden_under_frozen(sh, cell);
    let clip = if on_screen { ui.clip_rect().intersect(screen_quadrant) } else { Rect::NOTHING };
    let painter = ui.painter().with_clip_rect(clip);
    painter.rect_filled(erect, 0.0, fill);
    painter.rect_stroke(erect.expand(1.0), 0.0, Stroke::new(2.0, Tokens::get(ui.ctx()).sel_border), StrokeKind::Middle);
    let id = egui::Id::unique("gridcraft.cell_editor");
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(erect.shrink2(vec2(2.0, 1.0))));
    child.set_clip_rect(clip);
    // The theme's caret is meant for the UI chrome and vanishes on some fills (and, in the dark
    // theme, on the white paper): pick black or white against the fill, like Excel.
    child.visuals_mut().text_cursor.stroke.color = ink_on(fill);
    crate::formula_bar::editor_widget(app, &mut child, id, font, wrap, erect.width() - 4.0, erect.height() - 2.0);
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("gridcraft.cell_editor.rect"), erect));
}

/// Test the popup itself, not whether the pointer is over any part of the app.
/// A trigger can sit outside its popup, so its opening click must be ignored.
pub(super) fn popup_dismissed(response: &egui::Response, was_open: bool) -> bool {
    response.ctx.input(|i| !i.focused || i.key_pressed(egui::Key::Escape)) || (was_open && response.clicked_elsewhere())
}

/// The dropdown arrow beside a cell with an in-cell list validation.
pub fn validation_arrow(sh: &Sheet, geo: &Geo, c: CellRef) -> Option<Rect> {
    let dv = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(c)))?;
    if dv.kind != gridcraft_engine::model::ValidationKind::List || !dv.in_cell_dropdown || geo.hidden_under_frozen(sh, c) {
        return None;
    }
    let r = geo.cell_rect(sh, c);
    let s = r.height().clamp(14.0, 22.0);
    Some(Rect::from_min_size(pos2(r.right() + 1.0, r.bottom() - s), vec2(s, s)))
}

/// Rows the list-validation dropdown shows before it scrolls (Excel shows 8).
const LIST_PICKER_MAX_ROWS: usize = 8;

const LIST_PICKER_ROW_H: f32 = 18.0;

/// Popup frame around the rows: 2 px inner margin and the window stroke on each side, rounded up.
const LIST_PICKER_CHROME: f32 = 8.0;

/// Popup height for `n` items: every item up to `LIST_PICKER_MAX_ROWS`, then a scrollbar.
pub(super) fn list_picker_height(n: usize, row_h: f32, gap: f32) -> f32 {
    let rows = n.clamp(1, LIST_PICKER_MAX_ROWS) as f32;
    rows * row_h + (rows - 1.0) * gap
}

/// Draws the validation list popup, choosing above/below and left/right placement for RTL, and applies the clicked item with `cell.set`.
pub(super) fn list_picker(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, was_open: bool) {
    let Some(c) = app.grid.list_picker else { return };
    let Some(d) = app.session.active() else { return };
    let wb = d.wb.clone();
    let si = wb.active_sheet();
    let Some(sh) = wb.sheet(si) else { return };
    let Some(dv) = sh.validations.iter().find(|x| x.ranges.iter().any(|r| r.contains(c))).cloned() else {
        app.grid.list_picker = None;
        return;
    };
    let items = gridcraft_engine::cmd::data::list_items(&wb, si, &dv);
    let r = geo.cell_rect(sh, c);
    // Fit the popup to its rows (up to LIST_PICKER_MAX_ROWS); without a minimum the scroll area collapses to its 64 px default.
    let height = list_picker_height(items.len(), LIST_PICKER_ROW_H, ui.spacing().item_spacing.y);
    // Open below the cell, or above it when the rows would run past the bottom of the window (as Excel does).
    let screen = ui.ctx().content_rect();
    let above = r.bottom() + height + LIST_PICKER_CHROME > screen.bottom() && r.top() - height - LIST_PICKER_CHROME >= screen.top();
    let (at, pivot) = match (above, geo.rtl) {
        (false, false) => (geo.mirror_pos(r.left_bottom()), Align2::LEFT_TOP),
        (false, true) => (geo.mirror_pos(r.left_bottom()), Align2::RIGHT_TOP),
        (true, false) => (geo.mirror_pos(r.left_top()), Align2::LEFT_BOTTOM),
        (true, true) => (geo.mirror_pos(r.left_top()), Align2::RIGHT_BOTTOM),
    };
    let mut close = false;
    let response = egui::Area::new(egui::Id::unique("dv_list")).fixed_pos(at).pivot(pivot).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(2.0).show(ui, |ui| {
            ui.set_min_width(r.width().max(100.0));
            egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).show(ui, |ui| {
                for it in &items {
                    if ui
                        .add(egui::Button::new(it.as_str()).frame_when_inactive(false).min_size(vec2(r.width().max(100.0), LIST_PICKER_ROW_H)))
                        .clicked()
                    {
                        app.run_or_alert("cell.set", json!({"cell": c.a1(), "input": it}));
                        close = true;
                    }
                }
            });
        });
    });
    if close || popup_dismissed(&response.response, was_open) {
        app.grid.list_picker = None;
    }
}
