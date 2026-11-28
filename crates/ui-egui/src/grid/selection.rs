use super::geo::Geo;
#[cfg(test)]
use super::interact::{hidden_col_edge, hidden_row_edge};
#[cfg(test)]
use super::show::hidden_around;
use super::{CellRef, Color32, Painter, RangeRef, Rect, SelectionMode, Sheet, SheetApp, Stroke, StrokeKind, Tokens, json, pos2, vec2};

/// Where the active cell is on screen: the whole merged cell when it is part of one.
fn active_rect(geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection) -> Rect {
    match sh.merge_at(sel.active) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, sel.active),
    }
}

/// The rectangles shaded for a selection: every area except the active cell, which stays clear so
/// it is plain where typing goes. A single selected cell has no fill. `outline` is where the
/// current area is drawn this frame: it glides to a new selection, and its fill with it.
pub fn selection_fill(geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection, outline: Rect) -> Vec<Rect> {
    selection_fill_where(geo, sh, sel, outline, |_| true)
}

/// `selection_fill` for the areas `keep` accepts (a frozen pane draws only the areas it shows).
fn selection_fill_where(geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection, outline: Rect, keep: impl Fn(&RangeRef) -> bool) -> Vec<Rect> {
    if sel.is_single_cell() {
        return vec![];
    }
    let active = active_rect(geo, sh, sel);
    let area = |r: &RangeRef| if *r == sel.current() { outline } else { geo.range_rect(sh, *r) };
    sel.ranges.iter().filter(|r| keep(r)).flat_map(|r| rect_around(area(r), active)).collect()
}

/// `area` without `hole`: the strips above and below the hole, then what is left beside it. The
/// selection fill is translucent, so the active cell can't be cleared by painting over it; the
/// pieces never overlap for the same reason. Pieces with no size, or whose numbers aren't finite,
/// are dropped.
fn rect_around(area: Rect, hole: Rect) -> impl Iterator<Item = Rect> {
    // `intersect` takes the sane side of a NaN, which would make a broken hole cover the area.
    let hole = if hole.is_finite() { area.intersect(hole) } else { Rect::NOTHING };
    let parts = if hole.is_positive() {
        [
            Rect::from_min_max(area.min, pos2(area.max.x, hole.min.y)),
            Rect::from_min_max(pos2(area.min.x, hole.max.y), area.max),
            Rect::from_min_max(pos2(area.min.x, hole.min.y), pos2(hole.min.x, hole.max.y)),
            Rect::from_min_max(pos2(hole.max.x, hole.min.y), pos2(area.max.x, hole.max.y)),
        ]
    } else {
        [area, Rect::NOTHING, Rect::NOTHING, Rect::NOTHING]
    };
    parts.into_iter().filter(|r| r.is_positive() && r.is_finite())
}

/// True when `r` covers at least one cell of the pane that shows `rows` x `cols` (inclusive).
/// The panes of a frozen sheet share one coordinate system, so a range that lives only in the
/// scrolling pane would otherwise be drawn over the frozen pane as well.
fn range_in_pane(r: RangeRef, rows: (u32, u32), cols: (u32, u32)) -> bool {
    r.start.row <= rows.1 && r.end.row >= rows.0 && r.start.col <= cols.1 && r.end.col >= cols.0
}

/// The four sides of a `width` wide outline centred on `rect`, cut to `clip` (a little beyond it).
///
/// A stroked rectangle that spans a whole column (20 million points tall) loses its precision in
/// the tessellator and its left and right sides vanish; sides filled from rectangles that stay
/// near the viewport do not. Sides wholly outside `clip` are left out.
pub(crate) fn outline_bands(rect: Rect, width: f32, clip: Rect) -> Vec<Rect> {
    if !rect.is_finite() || !clip.is_finite() {
        return Vec::new();
    }
    let h = width / 2.0;
    let clip = clip.expand(width * 2.0);
    let sides = [
        Rect::from_min_max(pos2(rect.left() - h, rect.top() - h), pos2(rect.left() + h, rect.bottom() + h)),
        Rect::from_min_max(pos2(rect.right() - h, rect.top() - h), pos2(rect.right() + h, rect.bottom() + h)),
        Rect::from_min_max(pos2(rect.left() - h, rect.top() - h), pos2(rect.right() + h, rect.top() + h)),
        Rect::from_min_max(pos2(rect.left() - h, rect.bottom() - h), pos2(rect.right() + h, rect.bottom() + h)),
    ];
    sides.into_iter().map(|s| s.intersect(clip)).filter(|s| s.is_finite() && s.width() > 0.0 && s.height() > 0.0).collect()
}

/// `outline` is where the current area's border is drawn this frame (see `selection_glide`).
pub(crate) struct SelectionPaintInput<'a> {
    pub(crate) painter: &'a Painter,
    pub(crate) geo: &'a Geo,
    pub(crate) sheet: &'a Sheet,
    pub(crate) selection: &'a gridcraft_engine::Selection,
    pub(crate) outline: Rect,
    pub(crate) tokens: &'a Tokens,
    pub(crate) app: &'a SheetApp,
    pub(crate) rows: (u32, u32),
    pub(crate) cols: (u32, u32),
}

pub(crate) fn paint_selection(input: SelectionPaintInput<'_>) {
    let SelectionPaintInput { painter: p, geo, sheet: sh, selection: sel, outline, tokens: t, app, rows, cols } = input;
    // Fill all areas except the active cell.
    for rect in selection_fill_where(geo, sh, sel, outline, |r| range_in_pane(*r, rows, cols)) {
        p.rect_filled(rect, 0.0, t.sel_fill);
    }
    let active_range = sh.merge_at(sel.active).unwrap_or_else(|| RangeRef::cell(sel.active));
    let active = active_rect(geo, sh, sel);
    let active_here = range_in_pane(active_range, rows, cols);
    let cur_here = range_in_pane(sel.current(), rows, cols);
    // Editing a formula: outline its references instead of the selection.
    if let Some(ed) = &app.editor
        && ed.is_formula()
        && app.session.active().is_some_and(|d| d.uid == ed.doc)
    {
        let origin = app.session.active().and_then(|d| d.wb.sheet(ed.sheet)).map_or(sh.name.as_str(), |o| o.name.as_str());
        for (_, _, r, sheet, idx) in crate::editor::formula_refs(&ed.text) {
            if !crate::editor::ref_is_on_sheet(sheet.as_deref(), origin, &sh.name) {
                continue;
            }
            if !range_in_pane(r, rows, cols) {
                continue;
            }
            let rr = geo.range_rect(sh, r);
            let col = Tokens::get(p.ctx()).ref_color(idx, true);
            p.rect_filled(rr, 0.0, col.gamma_multiply(0.12));
            p.rect_stroke(rr, 0.0, Stroke::new(1.5, col), StrokeKind::Inside);
            for corner in [rr.left_top(), rr.right_top(), rr.left_bottom(), rr.right_bottom()] {
                p.rect_filled(Rect::from_center_size(corner, vec2(4.0, 4.0)), 0.0, col);
            }
        }
    }
    if cur_here {
        for side in outline_bands(outline.expand(0.5), 2.0, p.clip_rect()) {
            p.rect_filled(side, 0.0, t.sel_border);
        }
    }
    if active_here && sel.ranges.len() > 1 {
        p.rect_stroke(active, 0.0, Stroke::new(1.0, t.sel_border), StrokeKind::Inside);
    }
    // Fill handle (bottom-right of the current area).
    if app.editor.is_none() && range_in_pane(RangeRef::cell(sel.current().end), rows, cols) {
        let h = Rect::from_center_size(outline.right_bottom(), vec2(6.0, 6.0));
        p.rect_filled(h.expand(1.0), 0.0, Color32::WHITE);
        p.rect_filled(h, 0.0, t.sel_border);
    }
    // Fill-handle drag preview.
    if let crate::grid::Drag::Fill { target: Some(tr), .. } = &app.grid.drag
        && range_in_pane(*tr, rows, cols)
    {
        let rr = geo.range_rect(sh, *tr);
        p.extend(egui::Shape::dashed_line(
            &[rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()],
            Stroke::new(1.5, Color32::from_gray(90)),
            4.0,
            3.0,
        ));
    }
    if let crate::grid::Drag::Move { to: Some(to), .. } = &app.grid.drag {
        let r = sel.current();
        let moved = RangeRef::new(*to, to.offset_clamped(r.height() as i64 - 1, r.width() as i64 - 1));
        if range_in_pane(moved, rows, cols) {
            let rr = geo.range_rect(sh, moved);
            p.rect_stroke(rr, 0.0, Stroke::new(2.0, Color32::from_gray(110)), StrokeKind::Middle);
        }
    }
    if let crate::grid::Drag::MoveCols { before: Some(before), .. } = &app.grid.drag {
        let x = geo.x(sh, *before);
        p.line_segment([pos2(x, geo.rect.top()), pos2(x, geo.rect.bottom())], Stroke::new(3.0, t.accent));
    }
    // Marching ants around the copied range.
    if let Some(clip) = &app.session.clipboard
        && app.session.active().is_some_and(|d| d.uid == clip.doc_uid && d.wb.active_sheet() == clip.sheet)
        && range_in_pane(clip.range, rows, cols)
    {
        // A multi-area copy marks each area as selected.
        let areas = if clip.areas.is_empty() { std::slice::from_ref(&clip.range) } else { clip.areas.as_slice() };
        for area in areas {
            let rr = geo.range_rect(sh, *area).expand(0.5);
            let phase = app.grid.marching_phase;
            let pts = [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
            p.extend(egui::Shape::dashed_line_with_offset(&pts, Stroke::new(2.0, t.sel_border), &[4.0], &[4.0], phase));
        }
    }
}

/// `area` without `hole`: horizontal strips above and below `hole`, then what is left beside it.
/// The pieces are disjoint and together with `area ∩ hole` partition `area`.
fn subtract_range(area: RangeRef, hole: RangeRef) -> impl Iterator<Item = RangeRef> {
    let mut parts = [None; 4];
    let Some(cut) = area.intersection(&hole) else {
        parts[0] = Some(area);
        return parts.into_iter().flatten();
    };
    if cut.start.row > area.start.row {
        parts[0] = Some(RangeRef::new(area.start, CellRef::new(cut.start.row - 1, area.end.col)));
    }
    if cut.end.row < area.end.row {
        parts[1] = Some(RangeRef::new(CellRef::new(cut.end.row + 1, area.start.col), area.end));
    }
    if cut.start.col > area.start.col {
        parts[2] = Some(RangeRef::new(CellRef::new(cut.start.row, area.start.col), CellRef::new(cut.end.row, cut.start.col - 1)));
    }
    if cut.end.col < area.end.col {
        parts[3] = Some(RangeRef::new(CellRef::new(cut.start.row, cut.end.col + 1), CellRef::new(cut.end.row, area.end.col)));
    }
    parts.into_iter().flatten()
}

fn merge_expanded(sheet: &Sheet, piece: RangeRef) -> RangeRef {
    let mut sel = gridcraft_engine::Selection::range(piece);
    sel.expand_merges(sheet);
    sel.current()
}

fn piece_overflows(sheet: &Sheet, area: RangeRef, hole: RangeRef, piece: RangeRef) -> bool {
    let expanded = merge_expanded(sheet, piece);
    expanded.intersects(&hole) || !area.contains_range(&expanded)
}

/// Split any subtraction piece whose merge-expansion would pull `hole` back in or spill outside
/// `area` by peeling off crossing merges until every remaining piece stays inside `area \ hole`
/// after `Selection::expand_merges`.
fn split_around_merges(sheet: &Sheet, area: RangeRef, hole: RangeRef, initial: Vec<RangeRef>) -> Vec<RangeRef> {
    let mut work = initial;
    let mut out: Vec<RangeRef> = Vec::new();
    let mut budget = sheet.merges.len().saturating_mul(8).max(32);
    while let Some(p) = work.pop() {
        if budget > 0
            && piece_overflows(sheet, area, hole, p)
            && let Some(&m) = sheet.merges.iter().find(|m| m.intersects(&p) && !p.contains_range(m))
        {
            budget -= 1;
            if !m.intersects(&hole) && area.contains_range(&m) && !out.contains(&m) {
                out.push(m);
            }
            work.extend(subtract_range(p, m));
        } else {
            let expanded = merge_expanded(sheet, p);
            if !expanded.intersects(&hole) && area.contains_range(&expanded) && !out.contains(&expanded) {
                out.push(expanded);
            }
        }
    }
    out
}

fn subtract_for_sheet(sheet: Option<&Sheet>, area: RangeRef, hole: RangeRef) -> Vec<RangeRef> {
    let horiz: Vec<RangeRef> = subtract_range(area, hole).collect();
    if let Some(sh) = sheet
        && !sh.merges.is_empty()
        && horiz.iter().any(|p| piece_overflows(sh, area, hole, *p))
    {
        return split_around_merges(sh, area, hole, horiz);
    }
    horiz
}

/// How a selection press updates the active sheet's selected areas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectMode {
    /// Replace the selection with `clicked`.
    Replace,
    /// Ctrl/Add-mode click: subtract `clicked` if already selected (unless sole), otherwise append.
    Toggle,
    /// Ctrl/Add-mode drag start: append `clicked` (unless already the last area) without deselecting.
    Add,
}

/// The areas after a click or drag start and whether a drag may continue from `clicked`.
fn toggled_ranges_for_sheet(sheet: Option<&Sheet>, mut ranges: Vec<RangeRef>, clicked: RangeRef, mode: SelectMode) -> (Vec<RangeRef>, bool) {
    if mode == SelectMode::Replace {
        return (vec![clicked], true);
    }
    if mode == SelectMode::Toggle && ranges.iter().any(|r| r.contains_range(&clicked)) {
        let remaining: Vec<RangeRef> = ranges.iter().copied().flat_map(|r| subtract_for_sheet(sheet, r, clicked)).collect();
        return (if remaining.is_empty() { ranges } else { remaining }, false);
    }
    if ranges.last() != Some(&clicked) {
        ranges.push(clicked);
    }
    (ranges, true)
}

#[cfg(test)]
fn toggled_ranges(ranges: Vec<RangeRef>, clicked: RangeRef, add: bool) -> (Vec<RangeRef>, bool) {
    toggled_ranges_for_sheet(None, ranges, clicked, if add { SelectMode::Toggle } else { SelectMode::Replace })
}

fn style_selectable(
    wb: &gridcraft_engine::model::Workbook,
    p: &gridcraft_engine::model::SheetProtection,
    style: gridcraft_engine::model::StyleId,
) -> bool {
    if wb.styles.get(style).protection.locked { p.select_locked } else { p.select_unlocked }
}

fn cell_selectable(wb: &gridcraft_engine::model::Workbook, sheet: &Sheet, c: CellRef) -> bool {
    let Some(p) = sheet.protection.as_ref() else { return true };
    style_selectable(wb, p, sheet.style_id(c))
}

/// First index in `lo..=hi` not present in the sorted iterator `covered`.
fn first_gap(lo: u32, hi: u32, covered: impl Iterator<Item = u32>) -> Option<u32> {
    let mut cur = lo;
    for k in covered {
        if k < cur {
            continue;
        }
        if k > cur {
            return Some(cur);
        }
        cur = k.checked_add(1)?;
        if cur > hi {
            return None;
        }
    }
    (cur <= hi).then_some(cur)
}

/// First cell in column `col` (rows `r0..=r1`) with neither a row style nor a cell entry, so the
/// column style (or default style) takes effect there.
fn first_uncovered_row_in_col(sheet: &Sheet, r0: u32, r1: u32, col: u32) -> Option<CellRef> {
    let mut styled = sheet.rows.range(r0..=r1).filter(|(_, m)| m.style.is_some()).map(|(&row, _)| row).peekable();
    let mut row = r0;
    while row <= r1 {
        while let Some(&s) = styled.peek() {
            if s < row {
                styled.next();
            } else if s == row {
                styled.next();
                row = row.checked_add(1)?;
            } else {
                break;
            }
        }
        if row > r1 {
            return None;
        }
        let end = styled.peek().map_or(r1, |&s| s - 1);
        for candidate in row..=end {
            let c = CellRef::new(candidate, col);
            if sheet.cells.get(c).is_none() {
                return Some(c);
            }
        }
        row = end.checked_add(1)?;
    }
    None
}

/// Find a selectable cell inside `r` on a protected sheet using only the sparse cell, row, and
/// column style stores so distant selectable cells are found without scanning empty cells.
fn find_selectable_in_range(wb: &gridcraft_engine::model::Workbook, sheet: &Sheet, r: RangeRef) -> Option<CellRef> {
    let p = sheet.protection.as_ref()?;
    if !p.select_locked && !p.select_unlocked {
        return None;
    }
    if cell_selectable(wb, sheet, r.start) {
        return Some(r.start);
    }
    // 1. Explicit cell styles.
    if let Some((c, _)) = sheet.cells.iter_range(r).find(|(c, _)| cell_selectable(wb, sheet, *c)) {
        return Some(c);
    }
    // 2. Explicit row styles (cells in `sheet.cells` were already checked above).
    for (&row, meta) in sheet.rows.range(r.start.row..=r.end.row) {
        if let Some(style) = meta.style
            && style_selectable(wb, p, style)
            && let Some(c) = (r.start.col..=r.end.col).map(|col| CellRef::new(row, col)).find(|c| sheet.cells.get(*c).is_none())
        {
            return Some(c);
        }
    }
    // Column styles and the default style only take effect on rows with no row style.
    let first_row =
        first_gap(r.start.row, r.end.row, sheet.rows.range(r.start.row..=r.end.row).filter(|(_, m)| m.style.is_some()).map(|(&row, _)| row))?;
    let probe_col = |col: u32| -> Option<CellRef> {
        let fast = CellRef::new(first_row, col);
        if sheet.cells.get(fast).is_none() {
            return Some(fast);
        }
        first_row.checked_add(1).filter(|&next| next <= r.end.row).and_then(|next| first_uncovered_row_in_col(sheet, next, r.end.row, col))
    };
    // 3. Explicit column styles.
    for (&col, meta) in sheet.cols.range(r.start.col..=r.end.col) {
        if let Some(style) = meta.style
            && style_selectable(wb, p, style)
            && let Some(c) = probe_col(col)
        {
            return Some(c);
        }
    }
    // 4. Default style on columns with no column style.
    if style_selectable(wb, p, Default::default()) {
        let mut styled = sheet.cols.range(r.start.col..=r.end.col).filter(|(_, m)| m.style.is_some()).map(|(&col, _)| col).peekable();
        let mut col = r.start.col;
        while col <= r.end.col {
            while let Some(&s) = styled.peek() {
                if s < col {
                    styled.next();
                } else if s == col {
                    styled.next();
                    col = col.checked_add(1)?;
                } else {
                    break;
                }
            }
            if col > r.end.col {
                break;
            }
            let end = styled.peek().map_or(r.end.col, |&s| s - 1);
            for candidate in col..=end {
                if let Some(c) = probe_col(candidate) {
                    return Some(c);
                }
            }
            col = end.checked_add(1)?;
        }
    }
    None
}

/// Choose the active cell after deselecting an area and move its containing range to the end so
/// `Selection`'s invariant ("the last area contains the active cell") holds. Keeps the previous
/// active cell when it is still selected and selectable; otherwise, on a protected sheet, picks a
/// selectable cell from the remaining areas so `selection.set` does not reject the removal.
fn pick_active_after_removal(
    wb: &gridcraft_engine::model::Workbook,
    sheet: &Sheet,
    ranges: &mut Vec<RangeRef>,
    prev_active: CellRef,
) -> Option<CellRef> {
    if cell_selectable(wb, sheet, prev_active)
        && let Some(i) = ranges.iter().rposition(|r| r.contains(prev_active))
    {
        let r = ranges.remove(i);
        ranges.push(r);
        return Some(prev_active);
    }
    if sheet.protection.is_some() {
        for i in (0..ranges.len()).rev() {
            if let Some(c) = find_selectable_in_range(wb, sheet, ranges[i]) {
                let r = ranges.remove(i);
                ranges.push(r);
                return Some(c);
            }
        }
    }
    None
}

/// An arrow (or Page Up/Down, Ctrl+arrow) in Add mode. Without Shift the active cell moves and the
/// selected areas stay, the cell it lands on becoming a provisional last area that the next arrow
/// moves again; with Shift that area grows from its anchor and is kept, so the next arrow starts
/// another one.
pub(super) fn add_mode_move(app: &mut SheetApp, dr: i64, dc: i64, shift: bool, jump: bool) {
    app.grid.ensure_visible = true;
    app.grid.reveal_moving_end = shift;
    if shift {
        let _ = app.run("selection.move", json!({"dr": dr, "dc": dc, "extend": true, "jump": jump}));
        app.grid.add_pending = None;
        return;
    }
    let Some(mut keep) = app.session.active().map(|d| d.selection.ranges.clone()) else { return };
    // Only the provisional area this mode made is dropped, and only while it is still the last area.
    if let Some(p) = app.grid.add_pending.take()
        && keep.last() == Some(&p)
    {
        keep.pop();
    }
    // The engine works out where the cell lands (hidden rows, merged cells, Ctrl+arrow jumps).
    let _ = app.run("selection.move", json!({"dr": dr, "dc": dc, "extend": false, "jump": jump}));
    let Some((area, active)) = app.session.active().map(|d| (d.selection.current(), d.selection.active)) else { return };
    // Landing on an area already kept leaves it kept: the next arrow doesn't drop it.
    let kept = keep.contains(&area);
    keep.retain(|r| *r != area);
    keep.push(area);
    let range = keep.iter().map(|r| r.a1()).collect::<Vec<_>>().join(",");
    let _ = app.run("selection.set", json!({"range": range, "active": active.a1()}));
    app.grid.add_pending = (!kept).then_some(area);
}

/// The active sheet's areas in Add mode, as `selection.set` takes them; empty in other modes.
pub(super) fn add_mode_base(app: &SheetApp) -> Vec<String> {
    if app.grid.selection_mode != SelectionMode::Add {
        return Vec::new();
    }
    let mut ranges = app.session.active().map(|d| d.selection.ranges.iter().map(|x| x.a1()).collect::<Vec<_>>()).unwrap_or_default();
    if let Some(pending) = app.grid.add_pending
        && ranges.last().is_some_and(|range| range == &pending.a1())
    {
        ranges.pop();
    }
    ranges
}

/// Updates the selection for a cell or header click. Returns `true` when `r` was selected/added
/// (so dragging from this press may extend it) and `false` when it was deselected (or was the sole
/// selection).
pub(super) fn select_ranges(app: &mut SheetApp, r: RangeRef, mode: SelectMode, active: CellRef) -> bool {
    // A clicked area is kept: the next Add mode arrow starts a new one.
    app.grid.add_pending = None;
    let (ranges, added, next_active) = {
        let doc = app.session.active();
        let sheet = doc.and_then(|d| d.wb.active());
        let clicked = if !r.is_full_cols() && !r.is_full_rows() { sheet.and_then(|sh| sh.merge_at(active)).unwrap_or(r) } else { r };
        let current: Vec<RangeRef> = if mode != SelectMode::Replace { doc.map(|d| d.selection.ranges.clone()).unwrap_or_default() } else { vec![] };
        let (mut ranges, added) = toggled_ranges_for_sheet(sheet, current, clicked, mode);
        let next_active = if added {
            Some(active)
        } else if let Some(d) = doc
            && let Some(sh) = sheet
        {
            pick_active_after_removal(&d.wb, sh, &mut ranges, d.selection.active)
        } else {
            None
        };
        (ranges, added, next_active)
    };
    let joined = ranges.iter().map(RangeRef::a1).collect::<Vec<_>>().join(",");
    let params = match next_active {
        Some(a) => json!({"range": joined, "active": a.a1()}),
        None => json!({"range": joined}),
    };
    let ok = app.run("selection.set", params).is_ok();
    added && ok
}

#[cfg(test)]
mod selection_fill_tests {
    use super::*;

    #[test]
    fn rect_around_leaves_the_hole_and_never_overlaps() {
        let area = Rect::from_min_max(pos2(100.0, 50.0), pos2(400.0, 250.0));
        let check = |hole: Rect, pieces: usize| {
            let parts: Vec<Rect> = rect_around(area, hole).collect();
            assert_eq!(parts.len(), pieces, "{hole:?}: {parts:?}");
            let cut = area.intersect(hole);
            let cut_area = if cut.is_positive() { cut.area() } else { 0.0 };
            let shaded: f32 = parts.iter().map(Rect::area).sum();
            assert!((shaded - (area.area() - cut_area)).abs() < 0.5, "everything but the hole is shaded: {hole:?}");
            for (i, a) in parts.iter().enumerate() {
                assert!(area.contains_rect(*a), "nothing outside the area: {a:?}");
                assert!(!a.shrink(0.01).intersects(hole), "the hole stays clear: {a:?} vs {hole:?}");
                // The fill is translucent: a part shaded twice would show darker.
                assert!(parts.iter().skip(i + 1).all(|b| !a.shrink(0.01).intersects(*b)), "{parts:?}");
            }
        };
        let cell = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(64.0, 16.0));
        // In the middle, in each corner (where a drag began), along an edge.
        check(cell(200.0, 100.0), 4);
        check(cell(100.0, 50.0), 2);
        check(cell(336.0, 50.0), 2);
        check(cell(100.0, 234.0), 2);
        check(cell(336.0, 234.0), 2);
        check(cell(100.0, 100.0), 3);
        // A merged cell as wide, or as tall, as the area; one that is all of it, or more.
        check(Rect::from_min_max(pos2(100.0, 100.0), pos2(400.0, 132.0)), 2);
        check(Rect::from_min_max(pos2(200.0, 50.0), pos2(264.0, 250.0)), 2);
        check(area, 0);
        check(area.expand(30.0), 0);
        // The active cell is in another area, touches this one, or is only partly inside it.
        check(cell(500.0, 300.0), 1);
        check(cell(400.0, 50.0), 1);
        check(cell(380.0, 240.0), 2);
    }

    #[test]
    fn rect_around_survives_hostile_numbers() {
        let sane = Rect::from_min_max(pos2(0.0, 0.0), pos2(64.0, 16.0));
        let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN, -1.0, 0.0];
        for a in bad {
            for b in bad {
                let weird = Rect::from_min_max(pos2(a, b), pos2(b, a));
                for (area, hole) in [(sane, weird), (weird, sane), (weird, weird)] {
                    for part in rect_around(area, hole) {
                        assert!(part.is_positive() && part.is_finite(), "{area:?} around {hole:?}: {part:?}");
                    }
                }
                // A hole that makes no sense leaves the whole area shaded.
                if !weird.intersects(sane) {
                    assert_eq!(rect_around(sane, weird).collect::<Vec<_>>(), vec![sane]);
                }
            }
        }
    }
}

#[cfg(test)]
mod pane_tests {
    use super::*;

    fn rng(r0: u32, c0: u32, r1: u32, c1: u32) -> RangeRef {
        RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1))
    }

    #[test]
    fn selection_in_scrolling_pane_is_not_drawn_in_frozen_pane() {
        // Frozen: 2 rows and 2 columns; the scrolling pane shows rows 2..=20 and columns 2..=10.
        let frozen_cols = ((2, 20), (0, 1));
        let frozen_rows = ((0, 1), (2, 10));
        let corner = ((0, 1), (0, 1));
        let scrolled = ((2, 20), (2, 10));
        let sel = rng(5, 4, 7, 6);
        assert!(range_in_pane(sel, scrolled.0, scrolled.1));
        assert!(!range_in_pane(sel, frozen_cols.0, frozen_cols.1));
        assert!(!range_in_pane(sel, frozen_rows.0, frozen_rows.1));
        assert!(!range_in_pane(sel, corner.0, corner.1));
    }

    #[test]
    fn ctrl_click_on_a_selected_header_deselects_it() {
        let p = |x: &str| RangeRef::parse(x).unwrap();
        let rs = |v: &[&str]| v.iter().map(|x| p(x)).collect::<Vec<_>>();
        assert_eq!(toggled_ranges(rs(&["A:A", "C:C"]), p("C:C"), true), (rs(&["A:A"]), false));
        assert_eq!(toggled_ranges(rs(&["A:A"]), p("C:C"), true), (rs(&["A:A", "C:C"]), true));
        assert_eq!(toggled_ranges(rs(&["A:A"]), p("A:A"), true), (rs(&["A:A"]), false));
        assert_eq!(toggled_ranges(rs(&["A1", "B2", "3:3"]), p("B2"), true), (rs(&["A1", "3:3"]), false));
        assert_eq!(toggled_ranges(rs(&["A:A", "C:C"]), p("C:C"), false), (rs(&["C:C"]), true));
        // Deselecting inside a dragged range splits that range around the clicked cell/header.
        assert_eq!(toggled_ranges(rs(&["A1", "B1:D3"]), p("C2"), true), (rs(&["A1", "B1:D1", "B3:D3", "B2", "D2"]), false));
        assert_eq!(toggled_ranges(rs(&["A1:C3"]), p("A1"), true), (rs(&["A2:C3", "B1:C1"]), false));
        assert_eq!(toggled_ranges(rs(&["A:C"]), p("B:B"), true), (rs(&["A:A", "C:C"]), false));
        assert_eq!(toggled_ranges(rs(&["1:3"]), p("2:2"), true), (rs(&["1:1", "3:3"]), false));
        // Overlapping ranges both lose the deselected cell.
        let (left, added) = toggled_ranges(rs(&["A1:B2", "B2:C3"]), p("B2"), true);
        assert!(!added);
        assert!(left.iter().all(|r| !r.contains(CellRef::parse("B2").unwrap())));
    }

    #[test]
    fn selection_across_the_freeze_line_is_drawn_in_both_panes() {
        let sel = rng(5, 0, 7, 6);
        assert!(range_in_pane(sel, (2, 20), (0, 1)));
        assert!(range_in_pane(sel, (2, 20), (2, 10)));
    }

    #[test]
    fn hidden_outline_symbols_take_no_room() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let mut sh = Sheet::new("S");
        sh.rows.entry(3).or_default().outline = 1;
        sh.cols.entry(2).or_default().outline = 2;
        let shown = Geo::new(&sh, rect, vec2(0.0, 0.0));
        assert!(shown.band_w > 0.0 && shown.band_h > 0.0);
        sh.show_outline_symbols = false;
        let hidden = Geo::new(&sh, rect, vec2(0.0, 0.0));
        assert_eq!((hidden.band_w, hidden.band_h), (0.0, 0.0));
        assert!(hidden.cells.left() < shown.cells.left() && hidden.cells.top() < shown.cells.top());
        // Without headings too.
        sh.show_headings = false;
        let bare = Geo::new(&sh, rect, vec2(0.0, 0.0));
        assert_eq!((bare.band_w, bare.band_h), (0.0, 0.0));
        // No button answers a click on the empty bands' edges (the grid's left and top), where the
        // level buttons would sit with no headings to hold them.
        for geo in [&hidden, &bare] {
            for i in 0..400 {
                let t = i as f32 * 0.5;
                for p in [pos2(geo.rect.left(), geo.rect.top() + t), pos2(geo.rect.left() + t, geo.rect.top())] {
                    assert_eq!(crate::outline::hit(geo, &sh, p), None, "{p:?}");
                }
            }
        }
    }

    /// Two frozen rows and columns, scrolled right so that columns C:E lie under the frozen columns.
    fn frozen_geo(sh: &mut Sheet) -> Geo {
        sh.freeze = Some((2, 2));
        let scroll = vec2(3.0 * sh.default_col_width + 8.0, 0.0);
        Geo::new(sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)), scroll)
    }

    #[test]
    fn the_edge_just_past_a_hidden_line_unhides_it() {
        let mut sh = Sheet::new("S");
        sh.cols.entry(11).or_default().hidden = true; // L
        sh.rows.entry(26).or_default().hidden = true; // 27
        let geo = Geo::new(&sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 1200.0)), vec2(0.0, 0.0));
        let x = geo.x(&sh, 12); // between K and M
        assert_eq!(hidden_col_edge(&geo, &sh, x + 2.0), Some(11));
        assert_eq!(hidden_col_edge(&geo, &sh, x - 2.0), None); // K's own edge
        assert_eq!(hidden_col_edge(&geo, &sh, geo.x(&sh, 5)), None);
        let y = geo.y(&sh, 27); // between 26 and 28
        assert_eq!(hidden_row_edge(&geo, &sh, y + 2.0), Some(26));
        assert_eq!(hidden_row_edge(&geo, &sh, y - 2.0), None);
    }

    #[test]
    fn headers_mark_the_edges_next_to_hidden_lines() {
        let mut sh = Sheet::new("S");
        sh.cols.entry(0).or_default().hidden = true; // A
        sh.cols.entry(11).or_default().hidden = true; // L
        sh.rows.entry(26).or_default().hidden = true; // 27
        let col = |c| hidden_around(c, |i| sh.col_width(i));
        assert_eq!(col(1), (true, false)); // B, after hidden A
        assert_eq!(col(10), (false, true)); // K
        assert_eq!(col(12), (true, false)); // M
        assert_eq!(col(5), (false, false));
        let row = |r| hidden_around(r, |i| sh.row_height(i));
        assert_eq!(row(25), (false, true));
        assert_eq!(row(27), (true, false));
        assert_eq!(row(0), (false, false));
    }

    #[test]
    fn overlays_skip_cells_scrolled_under_the_frozen_columns() {
        let mut sh = Sheet::new("S");
        let geo = frozen_geo(&mut sh);
        assert!(geo.hidden_under_frozen(&sh, CellRef::new(5, 2)));
        assert!(geo.hidden_under_frozen(&sh, CellRef::new(5, 4)));
        assert!(!geo.hidden_under_frozen(&sh, CellRef::new(5, 5)));
        assert!(!geo.hidden_under_frozen(&sh, CellRef::new(5, 0)));
        let unscrolled = Geo::new(&sh, geo.rect, vec2(0.0, 0.0));
        assert!(!unscrolled.hidden_under_frozen(&sh, CellRef::new(5, 2)));
    }

    #[test]
    fn ranges_are_clipped_to_the_panes_that_hold_them() {
        let mut sh = Sheet::new("S");
        let geo = frozen_geo(&mut sh);
        let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
        assert_eq!(geo.range_panes(rng(5, 3, 7, 6)), [Rect::from_min_max(split, geo.cells.max)]);
        assert_eq!(geo.range_panes(rng(5, 0, 7, 6)).len(), 2);
        assert_eq!(geo.range_panes(rng(0, 0, 7, 6)).len(), 4);
        assert_eq!(geo.range_panes(rng(0, 0, 1, 1)), [Rect::from_min_max(geo.cells.min, split)]);
        // The fill handle of D6:E7 sits over the frozen columns, where nothing of it is shown.
        let handle = geo.range_rect(&sh, rng(5, 3, 6, 4)).right_bottom();
        assert!(handle.x < split.x);
        assert!(!geo.range_panes(rng(6, 4, 6, 4)).iter().any(|r| r.contains(handle)));
    }
}

#[cfg(test)]
mod outline_band_tests {
    use super::*;

    #[test]
    fn whole_column_outline_keeps_left_and_right_sides_near_viewport() {
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));
        let col = Rect::from_min_max(pos2(100.0, 40.0), pos2(180.0, 20_971_540.0));
        let sides = outline_bands(col, 2.0, clip);
        // Left, right and top (in view); the bottom is 20 million points away.
        assert_eq!(sides.len(), 3);
        assert!(sides.iter().all(|s| s.bottom() <= clip.bottom() + 4.0 + f32::EPSILON));
        assert!(sides.iter().any(|s| s.left() < 100.0 && s.right() > 100.0 && s.height() > 500.0));
        assert!(sides.iter().any(|s| s.left() < 180.0 && s.right() > 180.0 && s.height() > 500.0));
    }

    #[test]
    fn outline_outside_the_view_and_non_finite_draw_nothing() {
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));
        assert!(outline_bands(Rect::from_min_max(pos2(900.0, 0.0), pos2(950.0, 20.0)), 2.0, clip).is_empty());
        assert!(outline_bands(Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(1.0, 1.0)), 2.0, clip).is_empty());
    }
}
