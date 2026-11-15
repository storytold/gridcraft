//! "Highlight the active cell's row" (File ▸ Options ▸ General): a tint across the whole row of
//! the active cell, so the eye keeps its place in a wide table.

use super::geo::Geo;
use super::quadrant::QuadrantPaint;
use super::{Rect, Sheet, pos2};

/// The tinted band of `row` inside one quadrant (`rows` x `cols`): none when the row is outside
/// the quadrant or hidden.
pub fn active_row_band(geo: &Geo, sh: &Sheet, row: u32, rows: (u32, u32), cols: (u32, u32)) -> Option<Rect> {
    let height = sh.row_height(row) * geo.z;
    if row < rows.0 || row > rows.1 || height <= 0.0 {
        return None;
    }
    let top = geo.y(sh, row);
    let right = geo.x(sh, cols.1) + sh.col_width(cols.1) * geo.z;
    Some(Rect::from_min_max(pos2(geo.x(sh, cols.0), top), pos2(right, top + height)))
}

/// Paints the band of one quadrant, when the preference is on. With `within`, only the part of it
/// inside that area (text running past its cell has just cleared it).
pub(super) fn paint_active_row(q: &QuadrantPaint<'_>, within: Option<Rect>) {
    let Some(band) = q.active_row.and_then(|row| active_row_band(q.geo, q.sheet, row, q.rows, q.cols)) else { return };
    let part = within.map_or(band, |area| band.intersect(area));
    if part.is_positive() {
        q.painter.rect_filled(part, 0.0, q.tokens.active_row);
    }
}
