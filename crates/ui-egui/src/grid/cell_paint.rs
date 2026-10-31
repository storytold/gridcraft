use super::geo::Geo;
use std::sync::Arc;

use super::{
    BorderLine, BorderStyle, CellRef, CfCache, CfLook, Color, Color32, FontId, HAlign, MAX_COLS, Painter, PatternType, Pos2, RangeRef, Rect, Sheet,
    Stroke, Style, Underline, VAlign, Value, Workbook, pos2, theme, vec2,
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

/// The galley top for a cell's text when a reading is drawn in a strip at the top of the cell:
/// the text is placed in what is left below `reading_bottom`, and a row too short for both clips
/// the text at the bottom rather than drawing it over the reading (View › Show › Furigana).
pub(crate) fn cell_text_y_below_reading(valign: VAlign, rect: Rect, gh: f32, z: f32, reading_bottom: f32, wrapped: bool) -> f32 {
    let top = reading_bottom.clamp(rect.top(), rect.bottom());
    let below = Rect::from_min_max(pos2(rect.left(), top), rect.max);
    let y = if wrapped { wrapped_cell_text_y(valign, below, gh, z) } else { cell_text_y(valign, below, gh, z) };
    y.max(top)
}

/// Whether a cell shows its furigana: horizontal text only. Rotated text and vertical stacked text
/// (rotation 255) would need the reading to follow the rotation, so they show none.
pub(crate) fn furigana_is_shown(rotation: i16) -> bool {
    rotation == 0
}

/// The font size of a cell's furigana: about half the cell text's size, capped to the cell's top
/// strip (45% of its height) and shrunk until the reading — measured at half size as
/// `width_at_half` — fits `avail`, so a long reading never spills into a neighbouring cell.
/// Never below 1 px, and the cell (and row) height never changes.
pub(crate) fn furigana_font_size(size: f32, cell_h: f32, avail: f32, width_at_half: f32) -> f32 {
    let half = size * 0.5;
    let cap = half.min((cell_h * 0.45).max(1.0));
    if !(cap > 0.0 && avail > 0.0 && width_at_half > 0.0) {
        return cap.max(1.0);
    }
    // The reading's width grows with its font size, so this is the size at which it just fits.
    let shrink = half * avail / width_at_half;
    shrink.min(cap).max(1.0)
}

/// A cell's phonetic (ふりがな) reading laid out above its text: the galley, where it is drawn,
/// and the bottom of the top strip it takes from the cell text. The row height is unchanged.
pub(crate) struct CellFurigana {
    galley: Arc<egui::Galley>,
    pos: Pos2,
    pub(crate) bottom: f32,
    color: Color32,
}

impl CellFurigana {
    pub(crate) fn paint(&self, p: &Painter, rect: Rect) {
        p.with_clip_rect(rect.intersect(p.clip_rect())).galley(self.pos, self.galley.clone(), self.color);
    }
}

/// Lays out a cell's phonetic (ふりがな) reading above its text at [`furigana_font_size`], aligned
/// like the text and clipped to the cell: View › Show › Furigana, Excel's ふりがな表示. Rotated and
/// vertical cells are skipped by the caller. `None` when there is nothing to draw.
pub(crate) fn layout_cell_furigana(
    p: &Painter,
    rect: Rect,
    text_size: f32,
    z: f32,
    font: &FontId,
    color: Color32,
    h_align: HAlign,
    reading: &str,
) -> Option<CellFurigana> {
    if reading.is_empty() || !rect.width().is_finite() || rect.width() <= 0.0 {
        return None;
    }
    let pad = 2.5 * z;
    let avail = (rect.width() - 2.0 * pad).max(1.0);
    let half_font = FontId::new(text_size * 0.5, font.family.clone());
    let width_at_half = p.layout_no_wrap(reading.to_string(), half_font, color).size().x;
    let fs = furigana_font_size(text_size, rect.height(), avail, width_at_half);
    let galley = p.layout_no_wrap(reading.to_string(), FontId::new(fs, font.family.clone()), color);
    let w = galley.size().x;
    let x = match h_align {
        HAlign::Right => rect.right() - pad - w,
        HAlign::Center | HAlign::CenterAcross | HAlign::Distributed => rect.center().x - w / 2.0,
        _ => rect.left() + pad,
    };
    // The cell's top strip; the row height is unchanged, so a tall reading stays inside the cell.
    let y = rect.top() + 1.0 * z;
    Some(CellFurigana { bottom: y + galley.size().y, galley, pos: pos2(x, y), color })
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
        let (x0, x1) = (geo.x(sh, col), geo.x(sh, col.saturating_add(1)));
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
        let (y0, y1) = (geo.y(sh, row), geo.y(sh, row.saturating_add(1)));
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
    let lower = set.to_ascii_lowercase();
    if lower.contains("arrow") {
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
    } else if lower.contains("rating") || lower.contains("quarter") {
        p.circle_stroke(r.center(), r.width() / 2.0 - 0.5, Stroke::new(1.0, Color32::GRAY));
        let frac = idx as f32 / (n - 1).max(1) as f32;
        p.rect_filled(
            Rect::from_min_size(pos2(r.left() + 2.0, r.bottom() - 2.0 - (r.height() - 4.0) * frac), vec2(r.width() - 4.0, (r.height() - 4.0) * frac)),
            1.0,
            Color32::from_rgb(0x2B, 0x7C, 0xD3),
        );
    } else {
        p.circle_filled(r.center(), r.width() / 2.0 - 0.5, col);
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

#[cfg(test)]
mod furigana_tests {
    use super::*;

    #[test]
    fn only_horizontal_cells_show_a_reading() {
        assert!(furigana_is_shown(0), "horizontal text shows its reading");
        assert!(!furigana_is_shown(255), "vertical stacked text would get a horizontal reading");
        for angle in [-90, -45, 45, 90] {
            assert!(!furigana_is_shown(angle), "rotated text at {angle}° would get a horizontal reading");
        }
    }

    #[test]
    fn a_reading_and_the_cell_text_never_share_the_cell_top() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 60.0));
        let reading_bottom = 9.0;
        for wrapped in [false, true] {
            for valign in [VAlign::Top, VAlign::Center, VAlign::Bottom] {
                let y = cell_text_y_below_reading(valign, rect, 20.0, 1.0, reading_bottom, wrapped);
                assert!(y >= reading_bottom, "{valign:?} wrapped={wrapped}: text starts at {y}, over the reading's bottom {reading_bottom}");
            }
        }
        // Top alignment starts the text right below the reading; the wrapped path keeps its inset.
        assert!((cell_text_y_below_reading(VAlign::Top, rect, 20.0, 1.0, reading_bottom, false) - 9.0).abs() < 1e-6);
        assert!((cell_text_y_below_reading(VAlign::Top, rect, 20.0, 1.0, reading_bottom, true) - 10.5).abs() < 1e-6);
        // A row too short for both keeps the text below the reading (the cell clips its bottom)
        // instead of moving Center or Bottom up into the reading.
        let short = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 16.0));
        for valign in [VAlign::Center, VAlign::Bottom] {
            let y = cell_text_y_below_reading(valign, short, 20.0, 1.0, reading_bottom, false);
            assert!((y - reading_bottom).abs() < 1e-6, "{valign:?}: {y}");
        }
    }

    #[test]
    fn furigana_is_half_the_cell_font_and_shrinks_to_fit_the_cell() {
        // A short reading takes half the cell text's size.
        assert!((furigana_font_size(20.0, 100.0, 200.0, 10.0) - 10.0).abs() < 1e-6);
        // A reading wider than the cell shrinks so that it fits the cell width…
        let long = furigana_font_size(20.0, 100.0, 100.0, 400.0);
        assert!((long - 2.5).abs() < 1e-6, "{long}");
        // …and one that would need more than the cell's top strip is capped there.
        assert!((furigana_font_size(20.0, 10.0, 1000.0, 1.0) - 4.5).abs() < 1e-6);
        // The row height itself is never part of the result, only the cell's height.
        assert!((furigana_font_size(20.0, 100.0, 200.0, 10.0) - 10.0).abs() < 1e-6);
        // Degenerate cells and zero sizes still give a positive size, never an infinite one.
        assert_eq!(furigana_font_size(0.0, 0.0, 10.0, 10.0), 1.0);
        assert_eq!(furigana_font_size(20.0, 20.0, 0.0, 10.0), 9.0);
        assert!(furigana_font_size(20.0, f32::NAN, 10.0, 10.0).is_finite());
    }

    #[test]
    fn phonetic_text_is_only_asked_for_when_the_option_is_on() {
        // The screen path reads the engine helper only behind the option, as the PDF path does;
        // this pins the engine helper's contract the grid draws: joining and the none cases.
        use gridcraft_engine::model::{Cell, Formula, PhoneticRun, PhoneticString};
        let mut wb = Workbook::new();
        {
            let sh = std::sync::Arc::make_mut(&mut wb.sheets[0]);
            sh.set_value(CellRef::new(0, 0), Value::text("東京"));
        }
        let sh = wb.sheet(0).expect("one sheet");
        assert_eq!(gridcraft_engine::display::phonetic_text(&wb, sh, CellRef::new(0, 0)), None);
        wb.phonetics.push(PhoneticString {
            text: "東京".into(),
            runs: vec![PhoneticRun { start: 0, end: 1, text: "トウ".into() }, PhoneticRun { start: 1, end: 2, text: "キョウ".into() }],
            props: None,
        });
        let sh = wb.sheet(0).expect("one sheet");
        assert_eq!(gridcraft_engine::display::phonetic_text(&wb, sh, CellRef::new(0, 0)).as_deref(), Some("トウキョウ"));
        // The grid asks the same index once per paint pass rather than scanning per cell.
        let index = gridcraft_engine::display::PhoneticIndex::new(&wb);
        assert_eq!(index.reading(sh, CellRef::new(0, 0)), Some("トウキョウ"));
        let f = Cell::formula(Formula::new("=A1"));
        std::sync::Arc::make_mut(&mut wb.sheets[0]).set_cell(CellRef::new(1, 1), f);
        let sh = wb.sheet(0).expect("one sheet");
        assert_eq!(gridcraft_engine::display::phonetic_text(&wb, sh, CellRef::new(1, 1)), None);
    }
}
