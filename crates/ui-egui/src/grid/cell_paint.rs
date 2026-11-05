use super::geo::Geo;
use super::{
    BorderLine, BorderStyle, CellRef, CfCache, CfLook, Color, Color32, FontId, HAlign, MAX_COLS, Painter, PatternType, Pos2, RangeRef, Rect, Sheet,
    Stroke, StrokeKind, Style, Underline, VAlign, Value, Workbook, pos2, theme, vec2,
};

/// Resolved look of a cell for painting.
pub(super) struct Look {
    pub(super) style: Style,
    pub(super) fill: Option<Color32>,
    pub(super) text_color: Color32,
}

pub(super) fn col32(c: Color, wb: &Workbook, default: Color32) -> Color32 {
    c.resolve(&wb.theme).map(theme::color32).unwrap_or(default)
}

pub(super) fn numfmt_color(c: gridcraft_engine::numfmt::FormatColor) -> Color32 {
    c.rgb().map(|[r, g, b]| Color32::from_rgb(r, g, b)).unwrap_or(Color32::BLACK)
}

/// Last column of a Center Across Selection run starting at `start`: it extends over following columns (up to `limit`) while `continues` holds.
pub(super) fn center_across_end(start: u32, limit: u32, continues: impl Fn(u32) -> bool) -> u32 {
    let mut end = start;
    while end < limit && end + 1 < MAX_COLS && continues(end + 1) {
        end += 1;
    }
    end
}

pub(super) fn look(wb: &Workbook, si: usize, sh: &Sheet, c: CellRef, cf: &mut CfCache) -> (Look, Option<CfLook>) {
    let mut style = wb.styles.get(sh.style_id(c)).clone();
    if let Some(t) = sh.table_at(c) {
        style = gridcraft_engine::tables::table_cell_style(wb, si, t, c);
    }
    let cfl = cf.look(wb, si, c);
    if let Some(l) = &cfl
        && let Some(st) = &l.style
    {
        if st.fill.pattern != PatternType::None {
            style.fill = st.fill;
        }
        if st.font.color != Color::Auto {
            style.font.color = st.font.color;
        }
        if st.font.bold {
            style.font.bold = true;
        }
        if st.font.italic {
            style.font.italic = true;
        }
        if st.font.underline != Underline::None {
            style.font.underline = st.font.underline;
        }
        if st.font.strike {
            style.font.strike = true;
        }
        // In a differential style General means "not set" (#1163).
        if st.num_fmt.as_str() != "General" {
            style.num_fmt = st.num_fmt.clone();
        }
        for (dst, src) in [
            (&mut style.borders.left, st.borders.left),
            (&mut style.borders.right, st.borders.right),
            (&mut style.borders.top, st.borders.top),
            (&mut style.borders.bottom, st.borders.bottom),
        ] {
            if !src.is_none() {
                *dst = src;
            }
        }
    }
    let mut fill = match style.fill.pattern {
        PatternType::None => None,
        PatternType::Solid => style.fill.fg.resolve(&wb.theme).map(theme::color32),
        _ => {
            // Patterns: approximate with a blend of foreground and background.
            let fg = style.fill.fg.resolve(&wb.theme).unwrap_or([0, 0, 0]);
            let bg = style.fill.bg.resolve(&wb.theme).unwrap_or([255, 255, 255]);
            Some(Color32::from_rgb(
                ((fg[0] as u16 + bg[0] as u16 * 3) / 4) as u8,
                ((fg[1] as u16 + bg[1] as u16 * 3) / 4) as u8,
                ((fg[2] as u16 + bg[2] as u16 * 3) / 4) as u8,
            ))
        }
    };
    if let Some(l) = &cfl
        && let Some(rgb) = l.scale_fill
    {
        fill = Some(theme::color32(rgb));
    }
    let text_color = col32(style.font.color, wb, Color32::BLACK);
    (Look { style, fill, text_color }, cfl)
}

/// Text for a value with General narrowing to the available width.
pub(super) fn display_text(
    wb: &Workbook,
    sh: &Sheet,
    c: CellRef,
    v: &Value,
    st: &Style,
    width_px: f32,
    char_w: f32,
) -> (String, Option<Color32>, bool, Option<(char, usize)>) {
    if sh.show_formulas
        && let Some(f) = sh.cell(c).and_then(|x| x.formula.as_ref())
    {
        return (format!("={}", f.text), None, false, None);
    }
    if !sh.show_zeros && v.as_f64() == Some(0.0) {
        return (String::new(), None, false, None);
    }
    let code = st.num_fmt.as_str();
    // A checkbox cell draws TRUE/FALSE (and empty) as a box; other values show as General.
    if code == "checkbox" && matches!(v, Value::Bool(_) | Value::Empty) {
        return (String::new(), None, false, None);
    }
    if code == "General"
        && let Value::Number(n) = v
    {
        let max_chars = ((width_px - 4.0) / char_w).floor().max(1.0) as usize;
        return match gridcraft_engine::numfmt::format_general_fit(*n, max_chars.min(11)) {
            Some(t) => (t, None, true, None),
            None => ("#".repeat(max_chars.max(1)), None, true, None),
        };
    }
    let f = gridcraft_engine::display::format(v, code, wb);
    (f.text, f.color.map(numfmt_color), f.numeric, f.fill)
}

pub(crate) fn wrapped_cell_text_y(valign: VAlign, rect: Rect, gh: f32, z: f32) -> f32 {
    let pad_v = 1.5 * z;
    let y_top = rect.top() + pad_v;
    // Not clamped to `y_top`: text taller than the row overflows upward (Bottom) or both ways
    // (Center), and the cell's clip rect trims it, so Middle and Bottom stay distinct from Top.
    let y_bottom = rect.bottom() - gh - pad_v;
    match valign {
        VAlign::Top => y_top,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => (y_top + y_bottom) / 2.0,
        VAlign::Bottom => y_bottom,
    }
}

pub(crate) fn cell_text_y(valign: VAlign, rect: Rect, gh: f32, z: f32) -> f32 {
    let top_inset = 1.0 * z;
    let y_top = rect.top() - top_inset;
    let y_bottom = (rect.bottom() - gh - 0.5 * z).max(y_top);
    match valign {
        VAlign::Top => y_top,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => (y_top + y_bottom) / 2.0,
        VAlign::Bottom => y_bottom,
    }
}

/// Where to put a `TextShape` holding a galley of `size` turned by `angle` (egui radians, so
/// negative for upward text) for it to sit in `rect` per the cell's alignment. egui turns the
/// galley about `pos`, its top-left corner, so the turned box is placed first and `pos` follows.
/// General keeps the text at the left. Bottom (the default) puts upward text at the bottom, but
/// downward text at the top, where it starts reading.
pub(crate) fn rotated_text_pos(rect: Rect, size: egui::Vec2, angle: f32, h: HAlign, v: VAlign, pad: f32, indent: f32) -> Pos2 {
    let rot = egui::emath::Rot2::from_angle(angle);
    let turned = Rect::from_points(&[vec2(0.0, 0.0), vec2(size.x, 0.0), vec2(0.0, size.y), size].map(|c| (rot * c).to_pos2()));
    let x = match h {
        HAlign::Center | HAlign::CenterAcross | HAlign::Distributed => rect.center().x - turned.width() / 2.0,
        HAlign::Right => rect.right() - pad - indent - turned.width(),
        _ => rect.left() + pad + indent,
    };
    let y = match v {
        VAlign::Center | VAlign::Justify | VAlign::Distributed => rect.center().y - turned.height() / 2.0,
        VAlign::Bottom if angle < 0.0 => rect.bottom() - pad - turned.height(),
        _ => rect.top() + pad,
    };
    pos2(x, y) - turned.min.to_vec2()
}

/// A merged range, where it is drawn, and the visible rows and columns.
#[derive(Clone, Copy)]
pub(super) struct MergedArea {
    pub(super) m: RangeRef,
    pub(super) rect: Rect,
    pub(super) rows: (u32, u32),
    pub(super) cols: (u32, u32),
}

/// The outline segments of merged area `m` drawn at `rect`: the top and left edges come from the
/// first row's and first column's cells, the bottom and right edges from the last row's and last
/// column's cells, one segment per visible cell (limited to the visible `rows` × `cols`).
pub(super) fn merged_edges(wb: &Workbook, si: usize, sh: &Sheet, geo: &Geo, area: MergedArea, cf: &mut CfCache) -> Vec<(BorderLine, [Pos2; 2])> {
    let MergedArea { m, rect, rows, cols } = area;
    let mut out = Vec::new();
    let (cs, ce) = (m.start.col.max(cols.0), m.end.col.min(cols.1));
    for col in cs..=ce {
        if sh.col_width(col) == 0.0 {
            continue;
        }
        let (x0, x1) = (geo.x(sh, col), geo.x_end(sh, col));
        let top = look(wb, si, sh, CellRef::new(m.start.row, col), cf).0.style.borders.top;
        let bottom = look(wb, si, sh, CellRef::new(m.end.row, col), cf).0.style.borders.bottom;
        out.push((top, [pos2(x0, rect.top()), pos2(x1, rect.top())]));
        out.push((bottom, [pos2(x0, rect.bottom()), pos2(x1, rect.bottom())]));
    }
    let (rs, re) = (m.start.row.max(rows.0), m.end.row.min(rows.1));
    for row in rs..=re {
        if sh.row_height(row) == 0.0 {
            continue;
        }
        let (y0, y1) = (geo.y(sh, row), geo.y_end(sh, row));
        let left = look(wb, si, sh, CellRef::new(row, m.start.col), cf).0.style.borders.left;
        let right = look(wb, si, sh, CellRef::new(row, m.end.col), cf).0.style.borders.right;
        out.push((left, [pos2(rect.left(), y0), pos2(rect.left(), y1)]));
        out.push((right, [pos2(rect.right(), y0), pos2(rect.right(), y1)]));
    }
    out
}

/// The smaller font that makes `text` fit in `room` points of width (Shrink to fit), or `None`
/// when it already fits at its own size.
pub(super) fn shrink_font(p: &Painter, text: &str, font: &FontId, room: f32) -> Option<FontId> {
    let natural = p.layout_no_wrap(text.to_string(), font.clone(), Color32::PLACEHOLDER).size().x;
    let room = room.max(1.0);
    if natural.is_nan() || natural <= room {
        return None;
    }
    // Glyph advances don't scale exactly linearly (rounding), so leave a little slack.
    let size = (font.size * room / natural * 0.97).max(1.0);
    Some(FontId::new(size, font.family.clone()))
}

pub(crate) fn paint_border(p: &Painter, pts: [Pos2; 2], style: BorderStyle, col: Color32) {
    let w = style.width();
    let (a, b) = (pts[0], pts[1]);
    match style {
        BorderStyle::Double => {
            let horiz = (a.y - b.y).abs() < 0.5;
            let off = if horiz { vec2(0.0, 1.5) } else { vec2(1.5, 0.0) };
            p.line_segment([a - off, b - off], Stroke::new(1.0, col));
            p.line_segment([a + off, b + off], Stroke::new(1.0, col));
        }
        BorderStyle::Dashed
        | BorderStyle::MediumDashed
        | BorderStyle::DashDot
        | BorderStyle::MediumDashDot
        | BorderStyle::DashDotDot
        | BorderStyle::MediumDashDotDot
        | BorderStyle::SlantDashDot => {
            p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(w, col), 4.0, 2.0));
        }
        BorderStyle::Dotted | BorderStyle::Hair => {
            p.extend(egui::Shape::dotted_line(&[a, b], col, 2.0, w * 0.5));
        }
        _ => {
            p.line_segment([a, b], Stroke::new(w, col));
        }
    }
}

/// The shape families a conditional-format icon set draws, apart from colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconShape {
    Arrow,
    TrafficLight,
    Rating,
    Quarter,
    Star,
    Triangle,
    Box,
    Dot,
}

/// Which shape an icon set's name draws (`3Arrows`, `5Quarters`, `3Stars`…); names outside the
/// families fall back to a plain dot.
pub fn cf_icon_shape(set: &str) -> IconShape {
    let s = set.to_ascii_lowercase();
    if s.contains("arrow") {
        IconShape::Arrow
    } else if s.contains("trafficlight") {
        IconShape::TrafficLight
    } else if s.contains("rating") {
        IconShape::Rating
    } else if s.contains("quarter") {
        IconShape::Quarter
    } else if s.contains("star") {
        IconShape::Star
    } else if s.contains("triangle") {
        IconShape::Triangle
    } else if s.contains("box") {
        IconShape::Box
    } else {
        IconShape::Dot
    }
}

/// A rating, quarter or box icon: an outline (circle or square) that fills from the bottom with
/// the slot's share of the set — a meter, original to GridCraft.
fn icon_meter(p: &Painter, r: Rect, idx: usize, n: usize, round: bool) {
    if round {
        p.circle_stroke(r.center(), r.width() / 2.0 - 0.5, Stroke::new(1.0, Color32::GRAY));
    } else {
        p.rect_stroke(r, 1.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Inside);
    }
    let frac = idx as f32 / (n - 1).max(1) as f32;
    p.rect_filled(
        Rect::from_min_size(pos2(r.left() + 2.0, r.bottom() - 2.0 - (r.height() - 4.0) * frac), vec2(r.width() - 4.0, (r.height() - 4.0) * frac)),
        1.0,
        Color32::from_rgb(0x2B, 0x7C, 0xD3),
    );
}

pub(super) fn paint_cf_icon(p: &Painter, r: Rect, set: &str, idx: usize) {
    let n = gridcraft_engine::cf::icon_count(set);
    let palette3 = [Color32::from_rgb(0xD1, 0x3B, 0x2F), Color32::from_rgb(0xF2, 0xB8, 0x2E), Color32::from_rgb(0x2E, 0x9E, 0x4F)];
    let col = if n <= 3 {
        palette3.get(idx).copied().unwrap_or(Color32::GRAY)
    } else {
        let t = idx as f32 / (n - 1).max(1) as f32;
        Color32::from_rgb(
            (0xD1 as f32 * (1.0 - t) + 0x2E as f32 * t) as u8,
            (0x3B as f32 * (1.0 - t) + 0x9E as f32 * t) as u8,
            (0x2F as f32 * (1.0 - t) + 0x4F as f32 * t) as u8,
        )
    };
    match cf_icon_shape(set) {
        IconShape::Arrow => {
            let c = r.center();
            let s = r.width() / 2.0;
            let pts = if idx + 1 == n {
                vec![pos2(c.x, c.y - s), pos2(c.x + s, c.y + 0.1 * s), pos2(c.x - s, c.y + 0.1 * s)]
            } else if idx == 0 {
                vec![pos2(c.x, c.y + s), pos2(c.x - s, c.y - 0.1 * s), pos2(c.x + s, c.y - 0.1 * s)]
            } else {
                vec![pos2(c.x + s, c.y), pos2(c.x - 0.1 * s, c.y - s), pos2(c.x - 0.1 * s, c.y + s)]
            };
            p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
        }
        IconShape::Star => {
            // A five-point star, filled by triangles fanned from its centre: the outline is not
            // convex, so a single polygon would tessellate wrong.
            let c = r.center();
            let outer = r.width() / 2.0 - 0.5;
            let pt = |i: usize| {
                let rad = if i.is_multiple_of(2) { outer } else { outer * 0.45 };
                let a = (-90.0 + i as f32 * 36.0).to_radians();
                pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
            };
            for i in 0..10 {
                p.add(egui::Shape::convex_polygon(vec![c, pt(i), pt((i + 1) % 10)], col, Stroke::NONE));
            }
        }
        IconShape::Triangle => {
            p.add(egui::Shape::convex_polygon(
                vec![pos2(r.center().x, r.top() + 0.5), pos2(r.right() - 0.5, r.bottom() - 0.5), pos2(r.left() + 0.5, r.bottom() - 0.5)],
                col,
                Stroke::NONE,
            ));
        }
        IconShape::Rating | IconShape::Quarter => icon_meter(p, r, idx, n, true),
        IconShape::Box => icon_meter(p, r, idx, n, false),
        IconShape::TrafficLight | IconShape::Dot => {
            p.circle_filled(r.center(), r.width() / 2.0 - 0.5, col);
        }
    }
}

/// Where the active cell is on screen: the whole merged cell when it is part of one.
fn active_rect(geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection) -> Rect {
    match sh.merge_at(sel.active) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, sel.active),
    }
}


/// Black or white, whichever reads on `fill` (the threshold chart labels use).
pub(super) fn ink_on(fill: Color32) -> Color32 {
    let l = 0.299 * fill.r() as f32 + 0.587 * fill.g() as f32 + 0.114 * fill.b() as f32;
    if l < 150.0 { Color32::WHITE } else { Color32::BLACK }
}

/// The horizontal alignment to paint with. Cells are painted left to right and a right-to-left
/// sheet is mirrored afterwards, so an explicit Left or Right is swapped first to end up on the
/// side the user chose: Align Right stays on the right in either sheet direction, as in Excel.
/// General alignment is resolved from the value before this and follows the sheet direction.
pub(super) fn painted_halign(h: HAlign, rtl: bool) -> HAlign {
    match h {
        HAlign::Left if rtl => HAlign::Right,
        HAlign::Right if rtl => HAlign::Left,
        h => h,
    }
}
