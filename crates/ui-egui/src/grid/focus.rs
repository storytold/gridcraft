use super::geo::Geo;
use super::quadrant::QuadrantPaint;
use super::{CellRef, Color32, Rect, Sheet, Tokens, Workbook, pos2};

/// Focus Cell: the band over the active cell's column and the band over its row, inside one
/// quadrant (`rows` x `cols`). A band is absent when its line sits outside the quadrant, or when
/// the line is hidden.
pub fn focus_bands(geo: &Geo, sh: &Sheet, active: CellRef, rows: (u32, u32), cols: (u32, u32)) -> (Option<Rect>, Option<Rect>) {
    // The edge before line `i`. The first scrolling line moves with the scroll, so the frozen
    // pane ends at its own last line instead.
    let x_end = |col: u32| match col.checked_sub(1) {
        Some(last) if col == geo.fc => geo.x(sh, last) + sh.col_width(last) * geo.z,
        _ => geo.x(sh, col),
    };
    let y_end = |row: u32| match row.checked_sub(1) {
        Some(last) if row == geo.fr => geo.y(sh, last) + sh.row_height(last) * geo.z,
        _ => geo.y(sh, row),
    };
    let (top, bottom) = (geo.y(sh, rows.0), y_end(rows.1.saturating_add(1)));
    let (left, right) = (geo.x(sh, cols.0), x_end(cols.1.saturating_add(1)));
    let in_cols = active.col >= cols.0 && active.col <= cols.1 && sh.col_width(active.col) > 0.0;
    let in_rows = active.row >= rows.0 && active.row <= rows.1 && sh.row_height(active.row) > 0.0;
    let band = |a: f32, b: f32, lo: f32, hi: f32| Rect::from_min_max(pos2(a, lo), pos2(b, hi));
    (
        in_cols.then(|| band(geo.x(sh, active.col), x_end(active.col.saturating_add(1)), top, bottom)),
        in_rows.then(|| band(left, right, geo.y(sh, active.row), y_end(active.row.saturating_add(1)))),
    )
}

/// Paints the Focus Cell bands of one quadrant, when the sheet has Focus Cell on. With `within`,
/// only the part of the bands inside that area.
pub(crate) fn paint_focus_bands(q: &QuadrantPaint<'_>, within: Option<Rect>) {
    if !q.sheet.focus_cell {
        return;
    }
    let (col, row) = focus_bands(q.geo, q.sheet, q.active, q.rows, q.cols);
    let tint = focus_tint(q.sheet, q.workbook, q.tokens);
    for band in col.into_iter().chain(row) {
        let part = within.map_or(band, |area| band.intersect(area));
        if part.is_positive() {
            q.painter.rect_filled(part, 0.0, tint);
        }
    }
}

/// The alpha of the Focus Cell tint. The selection fill's own alpha, so the two read alike.
const FOCUS_ALPHA: u8 = 0x24;

/// The tint of the Focus Cell bands: the sheet's chosen color, or the theme's own.
pub(super) fn focus_tint(sh: &Sheet, wb: &Workbook, t: &Tokens) -> Color32 {
    match sh.focus_cell_color.and_then(|c| c.resolve(&wb.theme)) {
        Some([r, g, b]) => Color32::from_rgba_unmultiplied(r, g, b, FOCUS_ALPHA),
        None => t.focus_cell,
    }
}
