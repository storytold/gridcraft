use super::geo::Geo;
#[cfg(test)]
use super::interact::{hidden_col_edge, hidden_row_edge};
#[cfg(test)]
use super::show::hidden_around;
use super::{CellRef, Color32, Painter, RangeRef, Rect, Sheet, SheetApp, Stroke, StrokeKind, Tokens, json, pos2, vec2};

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

/// The width of the selection's outline, in points.
const OUTLINE_W: f32 = 2.0;

/// Where the gridline that marks the cell edge at `edge` is centred: it is drawn a point wide,
/// half a point before the edge rounded to a whole point.
pub(super) fn gridline_centre(edge: f32) -> f32 {
    edge.round() - 0.5
}

/// The device pixels the gridline at the cell edge `edge` covers: from the near side of its
/// first to the far side of its last, in points.
///
/// The renderer moves a line onto device pixels: one an odd number of pixels wide is centred on
/// a pixel, an even one between two (epaint's `Stroke::round_center_to_pixel`). This repeats
/// that rule, so that the selection's outline can be laid exactly on the gridline.
/// `tests/it/selection_outline.rs` compares the two as the renderer tessellates them, at several
/// display scales, so a renderer that changes its rule is noticed.
pub(super) fn gridline_span(edge: f32, ppp: f32) -> (f32, f32) {
    gridline_pixels(gridline_centre(edge), ppp)
}

/// [`gridline_span`] for an upright gridline on a sheet whose painting is then reflected across
/// `mirror` (right-to-left; [`Geo::mirror_axis`]). The renderer moves the reflected line onto
/// pixels, so its pixels are found there and reflected back: reflected with everything else, the
/// span covers them exactly.
pub(super) fn gridline_span_mirrored(edge: f32, ppp: f32, mirror: Option<f32>) -> (f32, f32) {
    match mirror {
        Some(axis) => {
            let (first, past) = gridline_pixels(axis - gridline_centre(edge), ppp);
            (axis - past, axis - first)
        }
        None => gridline_span(edge, ppp),
    }
}

/// The device pixels a gridline the renderer is given centred at `centre` covers, in points.
fn gridline_pixels(centre: f32, ppp: f32) -> (f32, f32) {
    let ppp = if ppp.is_finite() && ppp > 0.0 { ppp } else { 1.0 };
    let width = ppp;
    let centre = centre * ppp;
    let odd = width <= 1.0 || (width * 0.5 + 0.25).fract() > 0.5;
    let centre = if odd { (centre - 0.5).round() + 0.5 } else { centre.round() };
    // The pixels it covers half of or more: a fainter fringe beside them doesn't count.
    let first = (centre - width / 2.0).round();
    let past = (centre + width / 2.0).round().max(first + 1.0);
    (first / ppp, past / ppp)
}

/// The selection's outline round `cells`: its left, top, right and bottom sides, on a display of
/// `ppp` pixels to a point.
///
/// Each side lies on the gridline that bounds the cells and reaches outward from it, a whole
/// number of device pixels wide. So the inside of the outline is exactly the inside of the
/// cells, no gridline shows inside it, and every edge is sharp at any display scale. That is
/// where Excel puts its outline (measured on a display at 150%: the gridline's own pixel and
/// the one outside it, on each side).
///
/// `mirror` is the line a right-to-left sheet's painting is reflected across
/// ([`Geo::mirror_axis`]), `None` on a left-to-right sheet. The sides are given where they are
/// painted, before the reflection, and lie on the gridlines once it is done.
pub fn outline_sides(cells: Rect, ppp: f32, mirror: Option<f32>) -> [Rect; 4] {
    let ppp = if ppp.is_finite() && ppp > 0.0 { ppp } else { 1.0 };
    let (left, top) = (gridline_span_mirrored(cells.left(), ppp, mirror), gridline_span(cells.top(), ppp));
    let (right, bottom) = (gridline_span_mirrored(cells.right(), ppp, mirror), gridline_span(cells.bottom(), ppp));
    // A whole number of pixels, and never thinner than the gridline it lies on.
    let w = ((OUTLINE_W * ppp).round().max(1.0) / ppp).max(left.1 - left.0);
    let inner = Rect::from_min_max(pos2(left.1, top.1), pos2(right.0, bottom.0));
    let outer = inner.expand(w);
    [
        Rect::from_min_max(outer.min, pos2(inner.left(), outer.bottom())),
        Rect::from_min_max(outer.min, pos2(outer.right(), inner.top())),
        Rect::from_min_max(pos2(inner.right(), outer.top()), outer.max),
        Rect::from_min_max(pos2(outer.left(), inner.bottom()), outer.max),
    ]
}

/// The selection's outline round `cells` as it is painted: the sides of [`outline_sides`], cut to
/// `clip` (a little beyond it).
///
/// A side that spans a whole column (20 million points tall) loses its precision in the
/// tessellator and vanishes; sides that stay near the viewport do not. Sides wholly outside
/// `clip` are left out.
pub(crate) fn outline_bands(cells: Rect, ppp: f32, mirror: Option<f32>, clip: Rect) -> Vec<Rect> {
    if !cells.is_finite() || !clip.is_finite() {
        return Vec::new();
    }
    let clip = clip.expand(OUTLINE_W * 2.0);
    outline_sides(cells, ppp, mirror)
        .into_iter()
        .map(|s| s.intersect(clip))
        .filter(|s| s.is_finite() && s.width() > 0.0 && s.height() > 0.0)
        .collect()
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
        for side in outline_bands(outline, p.ctx().pixels_per_point(), geo.mirror_axis(), p.clip_rect()) {
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

/// The areas after a click: with `add` (Ctrl) the clicked area is added, or removed when it is already
/// selected (unless it is the only one, so the selection never ends up empty).
fn toggled_ranges(mut ranges: Vec<String>, clicked: String, add: bool) -> Vec<String> {
    if !add {
        return vec![clicked];
    }
    if ranges.len() > 1 && ranges.contains(&clicked) {
        ranges.retain(|x| *x != clicked);
    } else if !ranges.contains(&clicked) {
        ranges.push(clicked);
    }
    ranges
}

pub(super) fn select_ranges(app: &mut SheetApp, r: RangeRef, add: bool, active: CellRef) {
    let current: Vec<String> =
        if add { app.session.active().map(|d| d.selection.ranges.iter().map(|x| x.a1()).collect()).unwrap_or_default() } else { vec![] };
    let clicked = r.a1();
    let ranges = toggled_ranges(current, clicked.clone(), add);
    // A removed area takes the active cell with it; `selection.set` then puts it at the start of the last area.
    let params =
        if ranges.contains(&clicked) { json!({"range": ranges.join(","), "active": active.a1()}) } else { json!({"range": ranges.join(",")}) };
    let _ = app.session.run("selection.set", params);
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
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(toggled_ranges(s(&["A:A", "C:C"]), "C:C".into(), true), s(&["A:A"]));
        assert_eq!(toggled_ranges(s(&["A:A"]), "C:C".into(), true), s(&["A:A", "C:C"]));
        assert_eq!(toggled_ranges(s(&["A:A"]), "A:A".into(), true), s(&["A:A"]));
        assert_eq!(toggled_ranges(s(&["A1", "B2", "3:3"]), "B2".into(), true), s(&["A1", "3:3"]));
        assert_eq!(toggled_ranges(s(&["A:A", "C:C"]), "C:C".into(), false), s(&["C:C"]));
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
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            let sides = outline_bands(col, ppp, None, clip);
            // Left, right and top (in view); the bottom is 20 million points away.
            assert_eq!(sides.len(), 3, "{ppp}");
            assert!(sides.iter().all(|s| s.bottom() <= clip.bottom() + 4.0 + f32::EPSILON), "{ppp}");
            // Each lies on the gridline at its edge and reaches outward from it.
            assert!(sides.iter().any(|s| s.right() <= 100.0 && s.left() >= 96.0 && s.height() > 500.0), "{ppp}: the left side");
            assert!(sides.iter().any(|s| s.left() >= 178.0 && s.right() <= 183.0 && s.height() > 500.0), "{ppp}: the right side");
        }
    }

    #[test]
    fn outline_outside_the_view_and_non_finite_draw_nothing() {
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));
        assert!(outline_bands(Rect::from_min_max(pos2(900.0, 0.0), pos2(950.0, 20.0)), 1.0, None, clip).is_empty());
        assert!(outline_bands(Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(1.0, 1.0)), 1.0, None, clip).is_empty());
        assert!(outline_bands(Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), 1.0, None, Rect::NAN).is_empty());
        // In view, it is the four sides of `outline_sides`, as they are.
        let cells = Rect::from_min_max(pos2(100.0, 40.0), pos2(180.0, 60.0));
        assert_eq!(outline_bands(cells, 1.5, None, clip), outline_sides(cells, 1.5, None).to_vec());
    }

    #[test]
    fn a_gridline_is_found_where_the_renderer_puts_it() {
        // A pixel to a point: the point before the edge. Two: the same point, two pixels.
        assert_eq!(gridline_span(100.0, 1.0), (99.0, 100.0));
        assert_eq!(gridline_span(100.3, 1.0), (99.0, 100.0), "the grid rounds the edge to a whole point first");
        assert_eq!(gridline_span(100.0, 2.0), (99.0, 100.0));
        // One and a half: two pixels, wherever the nearest pixel boundary falls.
        for edge in [100.0, 101.0, 102.0, 103.0] {
            let (first, past) = gridline_span(edge, 1.5);
            assert_eq!(((past - first) * 1.5).round(), 2.0, "{edge}");
            assert!(first <= edge - 0.5 && edge - 0.5 <= past, "{edge}: round the line's own centre");
            assert!((first * 1.5 - (first * 1.5).round()).abs() < 1e-3, "{edge}: on whole pixels");
        }
        // One and a quarter: one pixel, the one its centre falls in.
        let (first, past) = gridline_span(100.0, 1.25);
        assert_eq!(((past - first) * 1.25).round(), 1.0);
        // A scale that makes no sense is read as one.
        for ppp in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            assert_eq!(gridline_span(100.0, ppp), (99.0, 100.0), "{ppp}");
        }
    }

    #[test]
    fn the_outline_covers_each_gridline_and_reaches_outward() {
        let cells = Rect::from_min_max(pos2(100.0, 50.0), pos2(160.0, 66.0));
        // At a pixel a point: each gridline's own pixel and the one outside it.
        let [left, top, right, bottom] = outline_sides(cells, 1.0, None);
        assert_eq!((left.left(), left.right()), (98.0, 100.0));
        assert_eq!((top.top(), top.bottom()), (48.0, 50.0));
        assert_eq!((right.left(), right.right()), (159.0, 161.0));
        assert_eq!((bottom.top(), bottom.bottom()), (65.0, 67.0));
        // The sides meet at the corners.
        assert_eq!((top.left(), top.right()), (98.0, 161.0));
        assert_eq!((left.top(), left.bottom()), (48.0, 67.0));
        // A whole number of pixels at every scale, on its gridline, never thinner than it.
        for ppp in [1.0_f32, 1.25, 1.5, 1.75, 2.0, 3.0] {
            let [left, top, right, bottom] = outline_sides(cells, ppp, None);
            let (px, grid) = (left.width() * ppp, gridline_span(cells.left(), ppp));
            assert!((px - px.round()).abs() < 1e-3 && px >= 1.0, "{ppp}: {px} pixels");
            assert!(px + 1e-3 >= (grid.1 - grid.0) * ppp, "{ppp}: covers its gridline");
            assert_eq!(left.right(), grid.1, "{ppp}: its inner edge is the gridline's");
            assert_eq!(right.left(), gridline_span(cells.right(), ppp).0, "{ppp}");
            assert_eq!(top.bottom(), gridline_span(cells.top(), ppp).1, "{ppp}");
            assert_eq!(bottom.top(), gridline_span(cells.bottom(), ppp).0, "{ppp}");
            for side in [left, top, right, bottom] {
                let weight = side.width().min(side.height());
                assert!((weight - left.width()).abs() < 1e-3, "{ppp}: every side the same weight ({weight})");
            }
        }
        // Numbers that make no sense give no panic, and nothing that would be painted.
        for (cells, ppp) in [(Rect::NAN, 1.0), (cells, f32::NAN), (Rect::from_min_max(pos2(0.0, 0.0), pos2(f32::INFINITY, 10.0)), 1.5)] {
            let _ = outline_sides(cells, ppp, None).map(|side| side.is_finite() && side.is_positive());
        }
    }
}
