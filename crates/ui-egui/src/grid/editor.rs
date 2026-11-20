use super::cell_paint::ink_on;
use super::geo::Geo;
use super::selection::{gridline_span, gridline_span_mirrored, outline_sides};
use super::{
    Align2, CellRef, Color32, FontId, MAX_COLS, MAX_ROWS, PatternType, Pos2, RangeRef, Rect, Sheet, SheetApp, Tokens, Workbook, json, pos2, theme,
    vec2,
};

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
    // Wrap Text: the editor keeps the column's width and wraps, growing down as Excel's does.
    // Otherwise it widens to the text's one-line width.
    let wrap = st.align.wrap;
    let (need, text_w) = if wrap {
        // The field wraps at the width it is given (the editor's less its 2 px inset a side): its
        // frame is `Frame::NONE`, so no text margin is taken off.
        let wrapped = ui.painter().layout(ed.text.clone(), font.clone(), Color32::BLACK, (rect.width() - 4.0).max(1.0));
        let rows = (wrapped.rows.len() as f32).max(lines);
        (vec2(rect.width(), editor_height(size, rows, rect.height())), 0.0)
    } else {
        let text_w = ui.painter().layout_no_wrap(ed.text.clone(), font.clone(), Color32::BLACK).size().x;
        (vec2(editor_width(text_w, rect.width(), size), editor_height(size, lines, rect.height())), text_w)
    };
    // The editor is its cell until the text needs more, and then takes whole cells.
    let grown = editor_size(geo, sh, cell, rect, need);
    // The text field stays as wide as its text needs. On a right-to-left sheet the editor grows
    // leftwards but the text runs from the field's left edge: in a field as wide as the cells
    // grown over, the text would sit over the next cell and not its own.
    let field = vec2(need.x.min(grown.x), grown.y);
    // Grows to the left on a right-to-left sheet.
    let erect = geo.mirror_rect(Rect::from_min_size(rect.min, field));
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
    // The highlight is the selection's own outline and stays on the cell, as in Excel: the editor
    // is laid over the cells its text needs, and clears the sides of the highlight it runs over.
    let ppp = ui.ctx().pixels_per_point();
    let [left, top, right, bottom] = outline_sides(rect, ppp, geo.mirror_axis());
    let cover = editor_cover(rect, Rect::from_min_size(rect.min, grown), field.x, text_w, geo.mirror_axis(), ppp);
    let green = Tokens::get(ui.ctx()).sel_border;
    let paint = |r: Rect, colour: Color32| {
        if r.is_finite() && r.is_positive() {
            painter.rect_filled(geo.mirror_rect(r), 0.0, colour);
        }
    };
    // The cell inside its gridlines, and the cells it has grown over. Those reach over the
    // gridline between them and the cell, which is how the line the text runs over is cleared.
    paint(Rect::from_min_max(pos2(left.right(), top.bottom()), pos2(right.left(), bottom.top())), fill);
    for cells in &cover.cells {
        paint(*cells, fill);
    }
    for side in highlight_sides([left, top, right, bottom], cover.over, cover.tall).into_iter().flatten() {
        paint(side, green);
    }
    let id = egui::Id::unique("gridcraft.cell_editor");
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(erect.shrink2(vec2(2.0, 1.0))));
    child.set_clip_rect(clip);
    // The theme's caret is meant for the UI chrome and vanishes on some fills (and, in the dark
    // theme, on the white paper): pick black or white against the fill, like Excel.
    child.visuals_mut().text_cursor.stroke.color = ink_on(fill);
    crate::formula_bar::editor_widget(app, &mut child, id, font, wrap, erect.width() - 4.0, erect.height() - 2.0);
    // Where the editor is painted: the cells it has grown over, not only its text field.
    let painted = geo.mirror_rect(Rect::from_min_size(rect.min, grown));
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("gridcraft.cell_editor.rect"), painted));
}

/// Where the text starts inside the in-cell editor, and the width of the caret after it.
const EDITOR_TEXT_AT: f32 = 2.0;
const EDITOR_CARET: f32 = 2.0;

/// How tall the in-cell editor's text needs it to be, for `lines` lines in a font of `font`
/// points, in a cell `cell` points tall.
///
/// Excel keeps the cell's height for one line and only grows for extra lines. Those need what
/// the lines themselves take: two lines of the default font fit two rows of the default height.
fn editor_height(font: f32, lines: f32, cell: f32) -> f32 {
    if lines <= 1.0 { cell } else { (font * 1.3 * lines).max(cell) }
}

/// The in-cell editor's size for the cell at `rect`, whose text needs `need`.
///
/// While the text fits, the editor is the cell. Text that needs more is given the cells after
/// it, a whole column or row at a time, so the editor always ends on a gridline: a cell it
/// covers is covered entirely, and its contents never show beside the text being typed.
fn editor_size(geo: &Geo, sh: &Sheet, cell: CellRef, rect: Rect, need: egui::Vec2) -> egui::Vec2 {
    let area = sh.merge_at(cell).unwrap_or_else(|| RangeRef::cell(cell));
    // A frozen pane cuts the editor off at its edge: there is nothing past it to grow over.
    let last_col = if area.end.col < geo.fc { geo.fc.saturating_sub(1) } else { MAX_COLS - 1 };
    let last_row = if area.end.row < geo.fr { geo.fr.saturating_sub(1) } else { MAX_ROWS - 1 };
    let in_view = vec2(geo.cells.right() - rect.left(), geo.cells.bottom() - rect.top());
    let w = editor_extent(need.x, rect.width(), in_view.x, |n| {
        let col = area.end.col.checked_add(n).filter(|col| *col <= last_col)?;
        Some(geo.cell_rect(sh, CellRef::new(cell.row, col)).right() - rect.left())
    });
    let h = editor_extent(need.y, rect.height(), in_view.y, |n| {
        let row = area.end.row.checked_add(n).filter(|row| *row <= last_row)?;
        Some(geo.cell_rect(sh, CellRef::new(row, cell.col)).bottom() - rect.top())
    });
    vec2(w.min(in_view.x), h)
}

/// The in-cell editor's size along one axis: the cell's `own` while `need` fits in it, and
/// otherwise out to the far edge of the first following column or row that gives it room.
/// `far_edge(n)` is the far edge of the `n`th one after the cell, `None` when there is none.
/// Past `in_view` nothing shows, so the search stops there.
fn editor_extent(need: f32, own: f32, in_view: f32, far_edge: impl Fn(u32) -> Option<f32>) -> f32 {
    if !need.is_finite() || need <= own {
        return own;
    }
    // Hidden columns and rows have no width: a long run of them is not searched for ever.
    (1..=2048).map_while(far_edge).find(|edge| *edge >= need || *edge >= in_view).unwrap_or(need)
}

/// What the in-cell editor does to what is round its cell, in the grid's logical coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
struct EditorCover {
    /// The cells it has grown over, inside the gridlines that bound them.
    cells: Vec<Rect>,
    /// Its text has reached the highlight's far side, which is left out.
    over: bool,
    /// It is taller than its cell: the highlight's bottom side is left out.
    tall: bool,
}

/// What an in-cell editor of the rectangle `editor` covers round its cell at `cell`.
///
/// The highlight's far side goes only once the text (`text_w` points wide) has reached it, and
/// its bottom side whenever the editor is taller than the cell. The top and near sides always
/// stay. On a right-to-left sheet (`mirror` is its axis, as for [`outline_sides`]) the text
/// starts at the far edge of its field, `field_w` points from the cell's near edge, so the line it
/// has to reach is the field's width less the cell's away.
fn editor_cover(cell: Rect, editor: Rect, field_w: f32, text_w: f32, mirror: Option<f32>, ppp: f32) -> EditorCover {
    // A rectangle with something in it, or nothing.
    let solid = |min: Pos2, max: Pos2| {
        let rect = Rect::from_min_max(min, max);
        (rect.is_finite() && rect.is_positive()).then_some(rect)
    };
    let [left, top, right, bottom] = outline_sides(cell, ppp, mirror);
    // The inside of the cell, between its gridlines.
    let inner = Rect::from_min_max(pos2(left.right(), top.bottom()), pos2(right.left(), bottom.top()));
    let wide = editor.right() > cell.right() + 0.5;
    let tall = editor.bottom() > cell.bottom() + 0.5;
    // The gridlines that bound the cells it has grown over stay: it stops short of them.
    let (far, below) = (gridline_span_mirrored(editor.right(), ppp, mirror).0, gridline_span(editor.bottom(), ppp).0);
    let mut cells = Vec::new();
    if wide {
        cells.extend(solid(pos2(inner.right(), inner.top()), pos2(far, below)));
    }
    if tall {
        cells.extend(solid(pos2(inner.left(), inner.bottom()), pos2(inner.right(), below)));
    }
    let to_the_line = if mirror.is_some() { cell.left() + field_w - inner.right() } else { inner.right() - cell.left() };
    EditorCover { cells, over: wide && EDITOR_TEXT_AT + text_w + EDITOR_CARET >= to_the_line, tall }
}

/// The sides of the highlight round a cell being edited (`sides`, as [`outline_sides`] gives
/// them: left, top, right, bottom) that stay: the far side goes once the text has run over it
/// (`over`), and the bottom side when the editor is taller than the cell (`tall`). A side that
/// stays stops at the line of one that went, so none reaches across a gridline the text cleared.
fn highlight_sides([left, top, right, bottom]: [Rect; 4], over: bool, tall: bool) -> [Option<Rect>; 4] {
    let down_to_the_line = |side: Rect| if tall { Rect::from_min_max(side.min, pos2(side.max.x, side.max.y.min(bottom.top()))) } else { side };
    let across_to_the_line = |side: Rect| if over { Rect::from_min_max(side.min, pos2(side.max.x.min(right.left()), side.max.y)) } else { side };
    [
        Some(down_to_the_line(left)),
        Some(across_to_the_line(top)),
        (!over).then(|| down_to_the_line(right)),
        (!tall).then(|| across_to_the_line(bottom)),
    ]
}

/// Whether the in-cell editor is shown for `area` of the sheet `sh`, the one on screen. It then
/// draws that area's highlight itself, which is the selection's own outline less the sides the
/// text clears, so the outline is not drawn under it as well: its corners would show there.
pub(super) fn edits_in_cell(app: &SheetApp, sh: &Sheet, area: RangeRef) -> bool {
    app.editor.as_ref().is_some_and(|ed| {
        !ed.from_formula_bar && app.view_key() == Some((ed.doc, ed.sheet)) && sh.merge_at(ed.cell).unwrap_or_else(|| RangeRef::cell(ed.cell)) == area
    })
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
    let items = gridcraft_engine::cmd::data::list_items(&wb, si, c, &dv);
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

#[cfg(test)]
mod editor_tests {
    use super::*;
    use gridcraft_engine::model::LineInfo;

    const C4: CellRef = CellRef { row: 3, col: 2 };
    /// The size of the font typed in, in points.
    const FONT: f32 = 12.0;

    /// A new sheet and its geometry, with nothing scrolled.
    fn sheet(change: impl FnOnce(&mut Sheet)) -> (Sheet, Geo) {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut sh = session.active().unwrap().wb.active().unwrap().clone();
        change(&mut sh);
        let geo = Geo::new(&sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 600.0)), egui::Vec2::ZERO);
        (sh, geo)
    }

    /// What the text of the editor of the cell at `at` needs, `text_w` wide in `lines` lines, as
    /// `in_cell_editor` works it out for a cell without Wrap Text.
    fn need(at: Rect, text_w: f32, lines: f32) -> egui::Vec2 {
        vec2(editor_width(text_w, at.width(), FONT), editor_height(FONT, lines, at.height()))
    }

    #[test]
    fn a_one_line_editor_is_as_tall_as_its_cell() {
        assert_eq!(editor_height(14.67, 1.0, 20.0), 20.0);
        assert_eq!(editor_height(14.67, 1.0, 64.0), 64.0, "a tall cell keeps its height");
        assert_eq!(editor_height(24.0, 1.0, 20.0), 20.0, "and so does a row shorter than its letters");
        // More lines than one need what the lines take.
        assert!(editor_height(14.67, 2.0, 20.0) > 38.0);
        assert_eq!(editor_height(14.67, 3.0, 200.0), 200.0);
        // On screen the default is an 11.73-point font in a 16-point row: a row a line.
        for lines in 2..12 {
            let need = editor_height(11.73, lines as f32, 16.0);
            assert!(need > 16.0 * (lines - 1) as f32 && need <= 16.0 * lines as f32, "{lines} lines need {need}");
        }
        // Numbers that make no sense give a number.
        for (font, lines, cell) in
            [(f32::NAN, 2.0, 20.0), (14.0, f32::NAN, 20.0), (14.0, 1.0, f32::NAN), (-5.0, -1.0, -1.0), (f32::INFINITY, 2.0, 20.0)]
        {
            let _ = editor_height(font, lines, cell);
        }
    }

    #[test]
    fn the_editor_is_its_cell_until_the_text_needs_more_and_then_takes_whole_cells() {
        let (sh, geo) = sheet(|_| {});
        let at = geo.cell_rect(&sh, C4);
        let far = |col: u32| geo.cell_rect(&sh, CellRef::new(3, col)).right() - at.left();
        let size = |text_w: f32, lines: f32| editor_size(&geo, &sh, C4, at, need(at, text_w, lines));
        let slack = editor_slack(FONT);
        assert_eq!(size(0.0, 1.0), at.size());
        assert_eq!(size(at.width() - 14.0, 1.0), at.size(), "text that just fits");
        assert_eq!(size(at.width() - 13.0, 1.0), vec2(far(3), at.height()), "a point more takes the whole of D4");
        assert_eq!(size(far(3) - slack - 0.5, 1.0).x, far(3), "with room for the next letter");
        assert_eq!(size(far(3) - slack + 0.5, 1.0).x, far(4), "and then the whole of E4");
        assert_eq!(size(far(5) - slack - 6.0, 1.0).x, far(5));
        // Downwards too: three lines need more than the row, and get whole rows.
        let below = |row: u32| geo.cell_rect(&sh, CellRef::new(row, 2)).bottom() - at.top();
        let tall = size(0.0, 3.0);
        let need = editor_height(FONT, 3.0, at.height());
        assert_eq!(tall.x, at.width());
        let rows = (4..40).find(|row| below(*row) >= need).unwrap();
        assert_eq!(tall.y, below(rows), "the first gridline that gives the lines room");
        assert!(below(rows - 1) < need);
        assert_eq!(rows, 5, "three lines take C4, C5 and C6");
        // It stops where the grid does.
        assert_eq!(size(1.0e9, 1.0).x, geo.cells.right() - at.left());
        // Numbers that make no sense leave the editor a size.
        for need in [vec2(f32::NAN, 16.0), vec2(10.0, f32::NAN), vec2(f32::INFINITY, f32::INFINITY), vec2(-50.0, -3.0)] {
            let size = editor_size(&geo, &sh, C4, at, need);
            assert!(size.x.is_finite() && size.y.is_finite(), "{need:?}: {size:?}");
        }
        assert_eq!(size(-50.0, -3.0), at.size());
    }

    #[test]
    fn the_editor_grows_past_a_hidden_column_over_a_merged_cell_and_not_out_of_a_frozen_pane() {
        // D is hidden: the next gridline is E's.
        let (sh, geo) = sheet(|sh| {
            sh.cols.insert(3, LineInfo { hidden: true, ..Default::default() });
        });
        let at = geo.cell_rect(&sh, C4);
        let e4 = geo.cell_rect(&sh, CellRef::new(3, 4));
        assert_eq!(geo.cell_rect(&sh, CellRef::new(3, 3)).width(), 0.0);
        assert_eq!(editor_size(&geo, &sh, C4, at, need(at, at.width(), 1.0)).x, e4.right() - at.left());
        // C4:D5 is merged: the editor starts as all of it, and grows from its far edges.
        let merged = RangeRef::new(C4, CellRef::new(4, 3));
        let (sh, geo) = sheet(|sh| sh.merges.push(merged));
        let at = geo.range_rect(&sh, merged);
        assert_eq!(editor_size(&geo, &sh, C4, at, need(at, 10.0, 1.0)), at.size());
        let e4 = geo.cell_rect(&sh, CellRef::new(3, 4));
        assert_eq!(editor_size(&geo, &sh, C4, at, need(at, at.width(), 1.0)).x, e4.right() - at.left());
        // Columns A to C are frozen: C4 has no cell in its pane to grow over, so its editor is
        // only as wide as its text needs, and the pane's edge cuts it off.
        let (sh, geo) = sheet(|sh| sh.freeze = Some((0, 3)));
        let at = geo.cell_rect(&sh, C4);
        assert_eq!(editor_size(&geo, &sh, C4, at, need(at, at.width(), 1.0)).x, at.width() + editor_slack(FONT));
        // B4, in the same pane, still has C4.
        let b4 = geo.cell_rect(&sh, CellRef::new(3, 1));
        assert_eq!(editor_size(&geo, &sh, CellRef::new(3, 1), b4, need(b4, b4.width(), 1.0)).x, at.right() - b4.left());
    }

    #[test]
    fn the_highlight_loses_only_the_sides_the_editor_runs_over() {
        let cell = Rect::from_min_max(pos2(100.0, 50.0), pos2(160.0, 66.0));
        // Right to left about a whole point, which leaves the lines where they are at a pixel a point.
        let cover = |editor: Rect, text_w: f32, rtl: bool| editor_cover(cell, editor, editor.width(), text_w, rtl.then_some(1000.0), 1.0);
        assert_eq!(cover(cell, 30.0, false), EditorCover::default(), "an editor the size of its cell covers nothing else");
        // Grown over the next column, with the text still short of the line between them.
        let grown = Rect::from_min_max(cell.min, pos2(220.0, 66.0));
        let short = cover(grown, 50.0, false);
        assert_eq!(short.cells, [Rect::from_min_max(pos2(159.0, 50.0), pos2(219.0, 65.0))], "the gridline between them and the next cell");
        assert!(!short.over && !short.tall, "the line stays until the text reaches it");
        // The text reaches the line: it goes. The bottom line stays.
        let over = cover(grown, 56.0, false);
        assert_eq!(over.cells, short.cells);
        assert!(over.over && !over.tall);
        // Taller than the cell: the bottom line goes, and the cells below are covered up to the
        // gridline at the cell's right, which stays.
        let down = cover(Rect::from_min_max(cell.min, pos2(160.0, 98.0)), 10.0, false);
        assert_eq!(down.cells, [Rect::from_min_max(pos2(100.0, 65.0), pos2(159.0, 97.0))]);
        assert!(down.tall && !down.over);
        // Both ways: both lines.
        let both = cover(Rect::from_min_max(cell.min, pos2(220.0, 98.0)), 200.0, false);
        assert_eq!(both.cells.len(), 2);
        assert!(both.over && both.tall);
        // Right to left, the text starts at the editor's far edge: the line is the grown part away.
        assert!(!cover(grown, 50.0, true).over);
        assert!(cover(grown, 57.0, true).over);
        let far = Rect::from_min_max(cell.min, pos2(400.0, 66.0));
        assert!(!cover(far, 57.0, true).over, "text that starts far off has further to come");
        assert!(cover(far, 57.0, false).over);
        // A text field narrower than the cells grown over ends nearer: its text reaches the line sooner.
        let field = |text_w: f32| editor_cover(cell, grown, 90.0, text_w, Some(1000.0), 1.0).over;
        assert!(!field(26.0) && field(27.0));
        // Numbers that make no sense paint nothing.
        for (editor, text_w) in [
            (Rect::from_min_max(cell.min, pos2(f32::NAN, f32::NAN)), 60.0),
            (Rect::from_min_max(cell.min, pos2(f32::INFINITY, f32::INFINITY)), 60.0),
            (grown, f32::NAN),
            (Rect::from_min_max(cell.min, pos2(160.6, 66.0)), 600.0),
        ] {
            let cover = cover(editor, text_w, false);
            assert!(cover.cells.iter().all(|r| r.is_finite() && r.is_positive()), "{editor:?}: {cover:?}");
        }
    }

    #[test]
    fn the_sides_that_stay_stop_at_the_line_the_text_cleared() {
        let cell = Rect::from_min_max(pos2(100.0, 50.0), pos2(160.0, 66.0));
        let sides = outline_sides(cell, 1.0, None);
        let [left, top, right, bottom] = sides;
        // Nothing cleared: the whole outline, corners and all.
        assert_eq!(highlight_sides(sides, false, false), [Some(left), Some(top), Some(right), Some(bottom)]);
        // The text runs over the right side: it goes, and the top and bottom stop at its line.
        let [l, t, r, b] = highlight_sides(sides, true, false);
        assert_eq!((l, r), (Some(left), None));
        assert_eq!(t, Some(Rect::from_min_max(top.min, pos2(right.left(), top.bottom()))));
        assert_eq!(b, Some(Rect::from_min_max(bottom.min, pos2(right.left(), bottom.bottom()))));
        assert_eq!(right.left(), 159.0, "the gridline's own pixel, at a pixel a point");
        // Taller than the cell: the bottom goes, and the left and right stop at its line.
        let [l, t, r, b] = highlight_sides(sides, false, true);
        assert_eq!((t, b), (Some(top), None));
        assert_eq!(l, Some(Rect::from_min_max(left.min, pos2(left.right(), bottom.top()))));
        assert_eq!(r, Some(Rect::from_min_max(right.min, pos2(right.right(), bottom.top()))));
        // Both: the left and top stay, each stopping at the other's line.
        let [l, t, r, b] = highlight_sides(sides, true, true);
        assert_eq!((r, b), (None, None));
        assert_eq!(l.map(|s| s.bottom()), Some(bottom.top()));
        assert_eq!(t.map(|s| s.right()), Some(right.left()));
        // Numbers that make no sense give no panic.
        let _ = highlight_sides(outline_sides(Rect::NAN, 1.0, None), true, true);
    }
}
