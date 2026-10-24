//! The pictures in the Conditional Formatting menu, as in Excel: an icon beside every item, and a
//! preview of the preset beside each Data Bar, Color Scale and Icon Set.

use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, StrokeKind, pos2, vec2};

/// What to draw beside a menu item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CfGlyph<'a> {
    HighlightCells,
    TopBottom,
    DataBars,
    ColorScales,
    IconSets,
    NewRule,
    ClearRules,
    ManageRules,
    /// A cell holding a short operator or sample (`>`, `<`, `=`, `ab`, ...).
    Cell(&'a str),
    /// A ranked list with the top (`true`) or bottom rows marked.
    Rank {
        top: bool,
        percent: bool,
    },
    /// A column of values with the average line, cells above (`true`) or below it marked.
    Average {
        above: bool,
    },
    /// Three gradient bars of different lengths in `color`.
    DataBar(Color32),
    /// The scale's colours, low to high, across a strip of cells.
    ColorScale(&'a [Color32]),
    /// Every icon of the set, highest first, drawn as cells draw them.
    IconSet(&'a str),
}

const MARK: Color32 = Color32::from_rgb(0xF8, 0xC8, 0xCC);
const MARK_EDGE: Color32 = Color32::from_rgb(0xC0, 0x30, 0x40);

/// How wide `glyph` is drawn at a row height of 18: previews need more room than icons.
pub fn width(glyph: CfGlyph) -> f32 {
    match glyph {
        CfGlyph::DataBar(_) | CfGlyph::ColorScale(_) => 40.0,
        CfGlyph::IconSet(set) => 18.0 * gridcraft_engine::cf::icon_count(set) as f32 - 4.0,
        _ => 18.0,
    }
}

/// A tiny grid of `rows` x `cols` cells in `r`, with `fill(row, col)` choosing each cell's colour.
fn grid(p: &Painter, r: Rect, rows: usize, cols: usize, line: Color32, fill: impl Fn(usize, usize) -> Option<Color32>) {
    let (w, h) = (r.width() / cols as f32, r.height() / rows as f32);
    for row in 0..rows {
        for col in 0..cols {
            let cell = Rect::from_min_size(pos2(r.left() + col as f32 * w, r.top() + row as f32 * h), vec2(w, h));
            if let Some(c) = fill(row, col) {
                p.rect_filled(cell, 0.0, c);
            }
        }
    }
    for i in 0..=cols {
        let x = r.left() + i as f32 * w;
        p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], Stroke::new(1.0, line));
    }
    for i in 0..=rows {
        let y = r.top() + i as f32 * h;
        p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, line));
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// A bar fading from `c` to white, as Excel's gradient data bars do.
fn gradient_bar(p: &Painter, r: Rect, c: Color32) {
    let steps = 8;
    let w = r.width() / steps as f32;
    for i in 0..steps {
        let seg = Rect::from_min_size(pos2(r.left() + i as f32 * w, r.top()), vec2(w + 0.5, r.height()));
        p.rect_filled(seg, 0.0, mix(c, Color32::WHITE, i as f32 / steps as f32 * 0.85));
    }
    p.rect_stroke(r, 0.0, Stroke::new(1.0, c), StrokeKind::Inside);
}

fn triangle(p: &Painter, at: egui::Pos2, s: f32, up: bool, c: Color32) {
    let d = if up { -s } else { s };
    p.add(egui::Shape::convex_polygon(vec![pos2(at.x, at.y + d), pos2(at.x - s, at.y - d * 0.4), pos2(at.x + s, at.y - d * 0.4)], c, Stroke::NONE));
}

/// Paints `glyph` in `r` (18 px tall), `text` being the menu's text colour.
pub fn paint(p: &Painter, r: Rect, glyph: CfGlyph, text: Color32) {
    let line = text.gamma_multiply(0.55);
    let sq = Rect::from_center_size(r.center(), vec2(16.0, 16.0));
    match glyph {
        CfGlyph::HighlightCells => {
            grid(p, sq, 3, 3, line, |row, col| ((row + col) % 3 == 1).then_some(MARK));
            p.rect_stroke(sq, 0.0, Stroke::new(1.0, text), StrokeKind::Inside);
        }
        CfGlyph::TopBottom => {
            grid(p, sq, 4, 2, line, |row, _| (row == 0).then_some(MARK));
            p.rect_stroke(sq, 0.0, Stroke::new(1.0, text), StrokeKind::Inside);
        }
        CfGlyph::DataBars => {
            for (i, len) in [0.9f32, 0.55, 0.3].into_iter().enumerate() {
                let bar = Rect::from_min_size(pos2(sq.left(), sq.top() + 1.0 + i as f32 * 5.0), vec2(sq.width() * len, 4.0));
                gradient_bar(p, bar, Color32::from_rgb(0x63, 0x8E, 0xC6));
            }
        }
        CfGlyph::ColorScales => {
            let cols = [Color32::from_rgb(0x63, 0xBE, 0x7B), Color32::from_rgb(0xFF, 0xEB, 0x84), Color32::from_rgb(0xF8, 0x69, 0x6B)];
            grid(p, sq, 3, 2, line, |row, _| Some(cols[row]));
        }
        CfGlyph::IconSets => {
            for (i, idx) in [2usize, 1, 0].into_iter().enumerate() {
                let cell = Rect::from_min_size(pos2(sq.left() + 3.0, sq.top() + i as f32 * 5.5), vec2(5.0, 5.0));
                crate::grid::paint_cf_icon(p, cell, "3Arrows", idx);
                p.line_segment([pos2(sq.left() + 10.0, cell.center().y), pos2(sq.right(), cell.center().y)], Stroke::new(1.0, line));
            }
        }
        CfGlyph::NewRule => {
            grid(p, sq.shrink2(vec2(0.0, 2.0)).translate(vec2(-2.0, 0.0)), 2, 2, line, |_, _| None);
            let c = pos2(sq.right() - 3.0, sq.bottom() - 3.0);
            p.line_segment([pos2(c.x - 4.0, c.y), pos2(c.x + 4.0, c.y)], Stroke::new(2.0, Color32::from_rgb(0x2E, 0x9E, 0x4F)));
            p.line_segment([pos2(c.x, c.y - 4.0), pos2(c.x, c.y + 4.0)], Stroke::new(2.0, Color32::from_rgb(0x2E, 0x9E, 0x4F)));
        }
        CfGlyph::ClearRules => {
            grid(p, sq.shrink2(vec2(0.0, 2.0)).translate(vec2(-2.0, 0.0)), 2, 2, line, |row, col| (row == 0 && col == 0).then_some(MARK));
            crate::icons::paint(
                p,
                Rect::from_min_size(pos2(sq.right() - 9.0, sq.bottom() - 9.0), vec2(10.0, 10.0)),
                crate::icons::Icon::Eraser,
                text,
            );
        }
        CfGlyph::ManageRules => {
            p.rect_stroke(sq.shrink(1.0), 1.0, Stroke::new(1.0, text), StrokeKind::Inside);
            for (i, c) in [MARK, Color32::from_rgb(0x63, 0xBE, 0x7B), Color32::from_rgb(0x63, 0x8E, 0xC6)].into_iter().enumerate() {
                let y = sq.top() + 4.0 + i as f32 * 4.0;
                p.rect_filled(Rect::from_min_size(pos2(sq.left() + 3.0, y - 1.5), vec2(3.0, 3.0)), 0.0, c);
                p.line_segment([pos2(sq.left() + 8.0, y), pos2(sq.right() - 3.0, y)], Stroke::new(1.0, line));
            }
        }
        CfGlyph::Cell(s) => {
            p.rect_filled(sq, 1.0, MARK);
            p.rect_stroke(sq, 1.0, Stroke::new(1.0, MARK_EDGE), StrokeKind::Inside);
            let size = if s.chars().count() > 1 { 9.0 } else { 12.0 };
            p.text(sq.center(), Align2::CENTER_CENTER, s, FontId::proportional(size), MARK_EDGE);
        }
        CfGlyph::Rank { top, percent } => {
            grid(p, sq, 4, 1, line, |row, _| ((top && row == 0) || (!top && row == 3)).then_some(MARK));
            let label = if percent { "%" } else { "10" };
            let ty = if top { sq.top() + 2.0 } else { sq.bottom() - 2.0 };
            p.text(
                pos2(sq.center().x, ty),
                if top { Align2::CENTER_TOP } else { Align2::CENTER_BOTTOM },
                label,
                FontId::proportional(7.0),
                MARK_EDGE,
            );
            triangle(p, pos2(sq.center().x, if top { sq.bottom() - 4.0 } else { sq.top() + 4.0 }), 3.0, top, text);
        }
        CfGlyph::Average { above } => {
            let heights = [0.8f32, 0.35, 0.6, 0.25];
            let avg = heights.iter().sum::<f32>() / heights.len() as f32;
            for (i, h) in heights.into_iter().enumerate() {
                let bar = Rect::from_min_max(
                    pos2(sq.left() + i as f32 * 4.0 + 0.5, sq.bottom() - sq.height() * h),
                    pos2(sq.left() + i as f32 * 4.0 + 3.5, sq.bottom()),
                );
                let marked = if above { h > avg } else { h < avg };
                p.rect_filled(bar, 0.0, if marked { MARK_EDGE } else { line });
            }
            let y = sq.bottom() - sq.height() * avg;
            p.line_segment([pos2(sq.left(), y), pos2(sq.right(), y)], Stroke::new(1.0, text));
        }
        CfGlyph::DataBar(c) => {
            for (i, len) in [1.0f32, 0.6, 0.3].into_iter().enumerate() {
                gradient_bar(p, Rect::from_min_size(pos2(r.left(), r.top() + 1.0 + i as f32 * 5.5), vec2(r.width() * len, 4.5)), c);
            }
        }
        CfGlyph::ColorScale(cols) => {
            let n = 4;
            let w = r.width() / n as f32;
            for i in 0..n {
                let t = i as f32 / (n - 1) as f32 * (cols.len() - 1) as f32;
                let (a, f) = (t.floor() as usize, t.fract());
                let c = mix(cols[a], cols[(a + 1).min(cols.len() - 1)], f);
                p.rect_filled(Rect::from_min_size(pos2(r.left() + i as f32 * w, r.top() + 2.0), vec2(w, r.height() - 4.0)), 0.0, c);
            }
            p.rect_stroke(r.shrink2(vec2(0.0, 2.0)), 0.0, Stroke::new(1.0, line), StrokeKind::Inside);
        }
        CfGlyph::IconSet(set) => {
            let n = gridcraft_engine::cf::icon_count(set);
            // Highest first, as Excel's gallery shows them.
            for (i, idx) in (0..n).rev().enumerate() {
                crate::grid::paint_cf_icon(p, Rect::from_min_size(pos2(r.left() + i as f32 * 18.0, r.center().y - 7.0), vec2(14.0, 14.0)), set, idx);
            }
        }
    }
}
