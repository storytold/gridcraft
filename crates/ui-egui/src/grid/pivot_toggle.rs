//! The +/- glyph in the row-number gutter for a collapsible PivotTable row group (Excel's
//! outline buttons on a pivot report), and the click that runs `pivot.collapse` / `pivot.collapse`
//! (expand) for it. Unlike the sheet's own Data ▸ Group outline (`crate::outline`), there is no
//! persisted metadata saying which rows are pivot group headers: [`gridcraft_engine::pivot::row_toggles`]
//! re-derives it from the live `PivotTable` model, the same way the report's cells were written.

use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::model::Workbook;
use gridcraft_engine::pivot::RowToggle;
use serde_json::json;

use super::geo::Geo;
use crate::SheetApp;
use crate::theme::Tokens;

/// Size of the +/- glyph box at 100% zoom (points): deliberately large and high-contrast so it
/// reads at a glance next to the plain row numbers, not just technically clickable (a 9px, then a
/// 12px box both turned out easy to miss/dismiss as noise at a normal screen distance -- #live report).
const GLYPH: f32 = 22.0;

/// The pivot row-group toggle for sheet row `rr`, if any, from `app.grid.pivot_row_toggles`
/// (recomputed only when the doc, its revision or the sheet changed since it was last built).
pub(super) fn toggle_at(app: &mut SheetApp, wb: &Workbook, si: usize, rr: u32) -> Option<RowToggle> {
    let d = app.session.active()?;
    let key = (d.uid, d.revision, si);
    if !matches!(&app.grid.pivot_row_toggles, Some((k, _)) if *k == key) {
        app.grid.pivot_row_toggles = Some((key, gridcraft_engine::pivot::row_toggles(wb, si)));
    }
    let (_, cached) = app.grid.pivot_row_toggles.as_ref()?;
    cached.iter().find_map(|(range, toggles)| {
        if rr < range.start.row || rr > range.end.row {
            return None;
        }
        toggles.get((rr - range.start.row) as usize).cloned().flatten()
    })
}

/// Where row `rr`'s toggle glyph has room to sit: inside the row-number header when headings are
/// shown (there's already a gutter there), or `geo.pivot_gutter_w`'s own lane left of the cells
/// when they're off (no row-number gutter exists at all, so `Geo` reserved this one instead).
pub(super) fn slot_rect(geo: &Geo, sh: &gridcraft_engine::model::Sheet, rr: u32) -> Rect {
    let y = geo.y(sh, rr);
    let h = sh.row_height(rr) * geo.z;
    let (left, w) = if geo.header_w > 0.0 { (geo.head_left(), geo.header_w) } else { (geo.cells.left() - geo.pivot_gutter_w, geo.pivot_gutter_w) };
    Rect::from_min_size(pos2(left, y), vec2(w, h))
}

/// Where the glyph sits within its slot (its left edge, vertically centred). Floored at 18px
/// regardless of zoom or row height, so it stays visible and clickable even in a short row.
pub(super) fn glyph_rect(geo: &Geo, slot: Rect) -> Rect {
    let s = (GLYPH * (geo.z / super::DISPLAY_SCALE).max(0.6)).max(18.0);
    Rect::from_min_size(pos2(slot.left() + 1.5, slot.center().y - s / 2.0), vec2(s, s))
}

pub(super) fn paint_glyph(p: &Painter, r: Rect, collapsed: bool) {
    let t = Tokens::get(p.ctx());
    p.rect_filled(r, 2.0, t.accent);
    p.rect_stroke(r, 2.0, Stroke::new(1.5, t.sheet_control_text), StrokeKind::Inside);
    let m = r.center();
    let pad = r.width() * 0.22;
    let w = 2.5_f32.max(r.width() * 0.12);
    p.line_segment([pos2(r.left() + pad, m.y), pos2(r.right() - pad, m.y)], Stroke::new(w, Color32::WHITE));
    if collapsed {
        p.line_segment([pos2(m.x, r.top() + pad), pos2(m.x, r.bottom() - pad)], Stroke::new(w, Color32::WHITE));
    }
}

/// The row whose glyph is at `p` (logical coordinates), if any, with its toggle.
pub(super) fn hit(app: &mut SheetApp, wb: &Workbook, si: usize, geo: &Geo, sh: &gridcraft_engine::model::Sheet, p: Pos2) -> Option<(u32, RowToggle)> {
    if p.x >= geo.cells.left() || p.y < geo.cells.top() {
        return None;
    }
    let rr = geo.row_at(sh, p.y);
    let slot = slot_rect(geo, sh, rr);
    if p.x < slot.left() || !glyph_rect(geo, slot).contains(p) {
        return None;
    }
    toggle_at(app, wb, si, rr).map(|t| (rr, t))
}

/// Paints the toggle glyphs in `geo.pivot_gutter_w`'s own lane (headings off: there's no
/// row-number header to draw them inside, as `show::paint_headers` does when there is one).
pub(super) fn paint_gutter(app: &mut SheetApp, p: &Painter, geo: &Geo, wb: &Workbook, si: usize, sh: &gridcraft_engine::model::Sheet, quads: &[super::PaneQuad]) {
    if geo.pivot_gutter_w <= 0.0 {
        return;
    }
    let lane = Rect::from_min_max(pos2(geo.cells.left() - geo.pivot_gutter_w, geo.cells.top()), pos2(geo.cells.left(), geo.rect.bottom()));
    let t = Tokens::get(p.ctx());
    p.rect_filled(lane, 0.0, t.header_bg);
    for (rows, _cols, clip) in quads {
        if rows.0 > rows.1 {
            continue;
        }
        let pc = p.with_clip_rect(Rect::from_min_max(pos2(lane.left(), clip.top()), pos2(lane.right(), clip.bottom())));
        for rr in geo.shown_rows(rows.0, rows.1) {
            if sh.row_height(rr) * geo.z <= 0.0 {
                continue;
            }
            if let Some(toggle) = toggle_at(app, wb, si, rr) {
                let slot = slot_rect(geo, sh, rr);
                paint_glyph(&pc, glyph_rect(geo, slot), toggle.collapsed);
            }
        }
    }
}

/// Runs the `pivot.collapse` / `pivot.collapse` (expand) command for a clicked glyph.
pub(super) fn run(app: &mut SheetApp, t: &RowToggle) {
    app.run_or_alert("pivot.collapse", json!({"field": t.field, "items": [t.item], "collapse": !t.collapsed}));
}
