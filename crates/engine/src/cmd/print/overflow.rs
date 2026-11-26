//! Text that doesn't fit where it is printed: the `sheet.overflow` query.
//!
//! Each text cell is measured as `text::draw_text` lays it out for the PDF (font, size, padding, indent, wrapping,
//! spilling into empty neighbours, Center Across Selection), at scale 1 so the result doesn't depend on the page
//! setup's zoom. Shapes are measured as `objects::draw_shape` wraps their text.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{HAlign, ShapeKind, Sheet, Workbook};
use gridcraft_pdf::{Font, TextDirection};
use serde_json::{Value as Json, json};

use super::PX_TO_PT;
use super::objects::anchor_rect;
use super::text::{PAD_PX, center_across_width, extend_text_bounds, font_for, text_align, text_direction, wrap_lines};

/// Below this many points a difference is rounding, not overflow.
const SLACK: f32 = 0.5;

fn round(v: f32) -> f64 {
    (f64::from(v) * 100.0).round() / 100.0
}

fn excerpt(text: &str) -> String {
    text.chars().take(80).collect()
}

/// The text cells and shapes of one sheet whose text doesn't fit (sizes in points).
pub(super) fn sheet_overflow(wb: &Workbook, sh: &Sheet, out: &mut Vec<Json>) {
    let k = PX_TO_PT;
    let merges = &sh.merges;
    for (c, cell) in sh.cells.iter() {
        let Value::Text(text) = &cell.value else { continue };
        if text.is_empty() || sh.show_formulas {
            continue;
        }
        // a merged range is measured from its first cell; its other cells are covered
        let range = merges.iter().find(|m| m.contains(c));
        if range.is_some_and(|m| m.start != c) {
            continue;
        }
        let (w_px, h_px) = match range {
            Some(m) => (
                (m.start.col..=m.end.col).map(|col| sh.col_width(col)).sum::<f32>(),
                (m.start.row..=m.end.row).map(|row| sh.row_height(row)).sum::<f32>(),
            ),
            None => (sh.col_width(c.col), sh.row_height(c.row)),
        };
        if w_px <= 0.0 || h_px <= 0.0 {
            continue; // hidden
        }
        if let Some(item) = cell_overflow(wb, sh, c, text, (w_px * k, h_px * k), range.is_some(), merges) {
            out.push(item);
        }
    }
    for shp in &sh.shapes {
        if shp.text.is_empty() || matches!(shp.kind, ShapeKind::Icon | ShapeKind::Ink | ShapeKind::Line | ShapeKind::Arrow) {
            continue;
        }
        let (x, y, w, h) = anchor_rect(sh, &shp.anchor);
        let (w, h) = (w as f32 * k, h as f32 * k);
        let fm = &shp.format;
        let size = fm.font_size.unwrap_or(11.0).clamp(1.0, 4000.0);
        let font = Font::styled(Font::family_for(fm.font.as_deref().unwrap_or("")), fm.bold, fm.italic);
        let lines = wrap_lines(&shp.text, font, size, (w - 4.0 * k).max(size), TextDirection::Auto);
        let needed = lines.len().min(500) as f32 * size * 1.2;
        let available = (h - 4.0 * k).max(0.0);
        if needed > available + SLACK {
            out.push(json!({
                "sheet": sh.name, "shape": shp.id, "anchor": [x, y], "kind": "shape",
                "needed": round(needed), "available": round(available), "over": round(needed - available), "text": excerpt(&shp.text),
            }));
        }
    }
}

/// One text cell: clipped (doesn't wrap and is wider than its cell plus the empty cells it may spill into), tall
/// (wraps into more lines than the row height shows) or shrunk (Shrink to fit draws it smaller).
fn cell_overflow(wb: &Workbook, sh: &Sheet, c: CellRef, text: &str, (w, h): (f32, f32), merged: bool, merges: &[RangeRef]) -> Option<Json> {
    let k = PX_TO_PT;
    let st = wb.styles.get(sh.style_id(c));
    let size = st.font.size.clamp(1.0, 409.0);
    let font = font_for(st);
    let pad = PAD_PX * k;
    let indent = st.align.indent as f32 * 9.0 * k;
    let avail = (w - 2.0 * pad - indent).max(0.0);
    let wrap = st.align.wrap;
    let text = if wrap { text.to_string() } else { text.replace(['\n', '\r'], " ") };
    let rot = st.align.rotation;
    if rot != 0 && rot != 255 {
        return None; // rotated text: not measured
    }
    let direction = text_direction(st.align.reading_order.unwrap_or_default()).resolve(&text);
    let tw = gridcraft_pdf::text_width_with_direction(font, size, &text, direction);
    let a1 = c.a1();
    let item = |kind: &str, needed: f32, available: f32| {
        json!({"sheet": sh.name, "cell": a1, "kind": kind, "needed": round(needed), "available": round(available),
               "over": round(needed - available), "text": excerpt(&text)})
    };
    if wrap {
        let lines = wrap_lines(&text, font, size, avail.max(size), direction);
        // draw_text's line height is at least 1.2 × size (more for tall glyphs)
        let needed = lines.len().max(1) as f32 * size * 1.2 + 2.0 * pad;
        return (needed > h + SLACK).then(|| item("tall", needed, h));
    }
    if st.align.shrink && tw > avail && tw > 0.0 {
        let mut v = item("shrunk", tw, avail);
        if let Some(o) = v.as_object_mut() {
            o.insert("scale".into(), json!(round(avail / tw)));
        }
        return Some(v);
    }
    let value = Value::Text(text.as_str().into());
    let h_align = text_align(st.align.h, &value, direction, sh.right_to_left);
    let spans = st.align.h == HAlign::CenterAcross && !merged;
    let across = if spans { center_across_width(&wb.styles, sh, c, merges) * k } else { 0.0 };
    let needed = tw + 2.0 * pad + indent;
    let (mut clip_l, mut clip_r) = (0.0, w + across);
    if !merged && across <= 0.0 && needed > w {
        (clip_l, clip_r) = extend_text_bounds(sh, c, merges, k, needed - w, h_align, (clip_l, clip_r));
    }
    let available = clip_r - clip_l;
    (needed > available + SLACK).then(|| item("clipped", needed, available))
}
