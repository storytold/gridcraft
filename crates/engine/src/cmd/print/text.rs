use super::draw::{Block, Look, rgb_of};
use super::{CellRef, Color, Font, HAlign, MAX_COLS, Page, Point, RangeRef, Sheet, Style, StyleTable, Underline, VAlign, Value};

const PAD_PX: f32 = 2.0;

/// Word-wraps `text` to `width` points.
pub(super) fn wrap_lines(text: &str, font: Font, size: f32, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if line.is_empty() || gridcraft_pdf::text_width(font, size, &cand) <= width {
                line = cand;
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
            // Break a single over-long word by characters. The cut that fits is found by binary
            // search (each try measures one candidate prefix), so an over-long word costs
            // O(n log n) measurements instead of re-measuring the whole cut per character for
            // every emitted line (`a` × 100k pasted text would be cubic worst case).
            while gridcraft_pdf::text_width(font, size, &line) > width && line.chars().count() > 1 {
                let chars: Vec<char> = line.chars().collect();
                let (mut lo, mut hi) = (1usize, chars.len());
                while lo < hi {
                    let mid = lo + (hi - lo).div_ceil(2);
                    let cut: String = chars[..mid].iter().collect();
                    if gridcraft_pdf::text_width(font, size, &cut) <= width {
                        lo = mid;
                    } else {
                        hi = mid - 1;
                    }
                }
                if lo == chars.len() {
                    break;
                }
                out.push(chars[..lo].iter().collect());
                line = chars[lo..].iter().collect();
            }
            if out.len() > 2000 {
                return out;
            }
        }
        out.push(line);
    }
    out
}

fn font_for(style: &Style) -> Font {
    Font::styled(Font::family_for(&style.font.name), style.font.bold, style.font.italic)
}

fn text_align(align: HAlign, value: &Value) -> HAlign {
    match align {
        HAlign::General => match value {
            Value::Number(_) => HAlign::Right,
            Value::Bool(_) | Value::Error(_) => HAlign::Center,
            _ => HAlign::Left,
        },
        HAlign::CenterAcross | HAlign::Distributed => HAlign::Center,
        HAlign::Justify | HAlign::Fill => HAlign::Left,
        other => other,
    }
}

/// Is a cell free for text to overflow into?
fn empty_for_overflow(sh: &Sheet, c: CellRef, merges: &[RangeRef]) -> bool {
    sh.value_ref(c).is_none_or(|v| v.is_empty()) && !merges.iter().any(|m| m.contains(c))
}

fn extend_text_bounds(sh: &Sheet, c: CellRef, merges: &[RangeRef], k: f32, need: f32, align: HAlign, bounds: (f32, f32)) -> (f32, f32) {
    let (mut clip_l, mut clip_r) = bounds;
    let grow = |dir: i64, mut remaining: f32| -> f32 {
        let mut ext = 0.0;
        let mut col = c.col as i64 + dir;
        while remaining > 0.0 && col >= 0 && col < MAX_COLS as i64 {
            let nc = CellRef::new(c.row, col as u32);
            if !empty_for_overflow(sh, nc, merges) {
                break;
            }
            let width = sh.col_width(nc.col) * k;
            ext += width;
            remaining -= width;
            col += dir;
            if ext > 20_000.0 {
                break;
            }
        }
        ext
    };
    match align {
        HAlign::Right => clip_l -= grow(-1, need),
        HAlign::Center => {
            clip_l -= grow(-1, need / 2.0);
            clip_r += grow(1, need / 2.0);
        }
        _ => clip_r += grow(1, need),
    }
    (clip_l, clip_r)
}

/// Unscaled width of the empty Center Across Selection cells right of `c`, which its text is centred over too.
pub(super) fn center_across_width(styles: &StyleTable, sh: &Sheet, c: CellRef, merges: &[RangeRef]) -> f32 {
    let mut ext = 0.0;
    let mut col = c.col + 1;
    while col < MAX_COLS && ext < 20_000.0 {
        let nc = CellRef::new(c.row, col);
        if !empty_for_overflow(sh, nc, merges) || styles.get(sh.style_id(nc)).align.h != HAlign::CenterAcross {
            break;
        }
        ext += sh.col_width(col);
        col += 1;
    }
    ext
}

pub(super) struct TextPlacement<'a> {
    pub cell: CellRef,
    pub look: &'a Look,
    pub rect: (f32, f32, f32, f32),
    pub merged: bool,
    pub merges: &'a [RangeRef],
    pub limits: (f32, f32),
}

/// How a cell's text is laid out on the page. The PDF draws it, and the print preview places
/// its lines where the PDF does (#3059).
pub(super) struct LaidText {
    pub(super) lines: Vec<String>,
    pub(super) size: f32,
    font: Font,
    color: [u8; 3],
    /// Rotation as stored (0 = none, 255 = stacked).
    pub(super) rot: i16,
    /// The horizontal span the text is clipped to (it overflows into empty neighbours).
    pub(super) clip: (f32, f32),
    pub(super) h_align: HAlign,
    rect: (f32, f32, f32, f32),
    pad: f32,
    indent: f32,
    across: f32,
    first_baseline: f32,
    line_h: f32,
    underline: Underline,
    strike: bool,
}

/// One horizontal line of text as placed on the page: its left edge, baseline and width.
pub(super) struct PlacedLine<'a> {
    pub(super) text: &'a str,
    pub(super) x: f32,
    pub(super) baseline: f32,
    pub(super) width: f32,
}

impl LaidText {
    /// Each horizontal line where it is drawn; lines wholly outside the cell are left out.
    pub(super) fn placed(&self) -> impl Iterator<Item = PlacedLine<'_>> {
        let (x, y, w, h) = self.rect;
        self.lines.iter().enumerate().filter_map(move |(i, line)| {
            let lw = gridcraft_pdf::text_width(self.font, self.size, line);
            let lx = match self.h_align {
                HAlign::Right => x + w - self.pad - lw - self.indent,
                HAlign::Center => x + (w + self.across - lw) / 2.0,
                _ => x + self.pad + self.indent,
            };
            let ly = self.first_baseline + i as f32 * self.line_h;
            if ly - self.size > y + h || ly < y - self.line_h {
                return None;
            }
            Some(PlacedLine { text: line, x: lx, baseline: ly, width: lw })
        })
    }
}

/// Lays out a cell's text as it prints, or `None` when the cell prints no text.
pub(super) fn lay_text(b: &Block, placement: TextPlacement<'_>) -> Option<LaidText> {
    let TextPlacement { cell: c, look: l, rect, merged, merges, limits } = placement;
    let (x, y, w, h) = rect;
    let sh = b.sh;
    let wb = b.wb;
    let v = sh.value(c);
    if v.is_empty() || l.cf.as_ref().is_some_and(|cf| cf.hide_value) {
        return None;
    }
    if !sh.show_zeros && v.as_f64() == Some(0.0) {
        return None;
    }
    let st = &l.style;
    let mut size = st.font.size.clamp(1.0, 409.0) * b.scale;
    let font = font_for(st);
    let pad = PAD_PX * b.k;
    let indent = st.align.indent as f32 * 9.0 * b.k;
    let avail = (w - 2.0 * pad - indent).max(0.0);
    let code = st.num_fmt.as_str();
    let formatted = crate::display::format(&v, code, wb);
    let mut color = formatted.color.and_then(gridcraft_numfmt::FormatColor::rgb).or_else(|| rgb_of(st.font.color, wb)).unwrap_or([0, 0, 0]);
    if l.cf.as_ref().and_then(|x| x.style.as_ref()).is_some_and(|s| s.font.color != Color::Auto) {
        color = rgb_of(st.font.color, wb).unwrap_or(color);
    }
    let is_num = matches!(v, Value::Number(_));
    let wrap = st.align.wrap;
    let mut text = if sh.show_formulas {
        crate::display::cell_text(wb, sh, c)
    } else if code == "General"
        && let Value::Number(n) = v
    {
        let digit = gridcraft_pdf::text_width(font, size, "0").max(0.1);
        let max_chars = (avail / digit).floor().max(1.0) as usize;
        gridcraft_numfmt::format_general_fit(n, max_chars.min(11)).unwrap_or_else(|| "#".repeat(max_chars.min(255)))
    } else {
        // The look's code, which a conditional format can override (#1163).
        crate::display::value_text_as(wb, &v, code)
    };
    if !wrap {
        text = text.replace(['\n', '\r'], " ");
    }
    if text.is_empty() {
        return None;
    }
    let mut tw = gridcraft_pdf::text_width(font, size, &text);
    if st.align.shrink && !wrap && tw > avail && tw > 0.0 {
        size = (size * avail / tw).max(1.0);
        tw = gridcraft_pdf::text_width(font, size, &text);
    }
    if is_num && !wrap && tw > avail + 0.01 && !sh.show_formulas {
        let hash = gridcraft_pdf::text_width(font, size, "#").max(0.1);
        text = "#".repeat(((avail / hash).floor().max(1.0) as usize).min(255));
        tw = gridcraft_pdf::text_width(font, size, &text);
    }
    let h_align = text_align(st.align.h, &v);
    let rot = st.align.rotation;
    let rotated = rot != 0 && rot != 255;
    // Rotated text wraps along its own direction: upright (±90°) text to the row height.
    let lines: Vec<String> = match (wrap, rotated) {
        (true, false) => wrap_lines(&text, font, size, avail.max(size)),
        (true, true) if rot.abs() == 90 => wrap_lines(&text, font, size, (h - 2.0 * pad).max(size)),
        (true, true) => vec![text.lines().next().unwrap_or_default().to_string()],
        (false, _) => vec![text],
    };
    let line_h = size * 1.2;
    let n = lines.len().max(1) as f32;
    let first_baseline = match st.align.v {
        VAlign::Top => y + pad + size * 0.8,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => y + (h - n * line_h) / 2.0 + line_h * 0.5 + size * 0.33,
        VAlign::Bottom => y + h - pad - size * 0.22 - (n - 1.0) * line_h,
    };
    // Overflow into empty neighbours (single-line text, not merged).
    // Center Across Selection spans the empty Center Across cells to its right; the text stays within that run.
    let spans = st.align.h == HAlign::CenterAcross && !wrap && !merged && rot == 0;
    let across = if spans { center_across_width(&wb.styles, sh, c, merges) * b.k } else { 0.0 };
    let (mut clip_l, mut clip_r) = (x, x + w + across);
    if !wrap && !merged && !is_num && rot.abs() != 90 && across <= 0.0 && tw + 2.0 * pad + indent > w {
        let need = tw + 2.0 * pad + indent - w;
        (clip_l, clip_r) = extend_text_bounds(sh, c, merges, b.k, need, h_align, (clip_l, clip_r));
    }
    Some(LaidText {
        lines,
        size,
        font,
        color,
        rot,
        clip: (clip_l.max(limits.0), clip_r.min(limits.1)),
        h_align,
        rect,
        pad,
        indent,
        across,
        first_baseline,
        line_h,
        underline: st.font.underline,
        strike: st.font.strike,
    })
}

pub(super) fn draw_text(pg: &mut Page, b: &Block, placement: TextPlacement<'_>) {
    let Some(t) = lay_text(b, placement) else { return };
    let (x, y, w, h) = t.rect;
    let (size, font, color, line_h, pad, rot) = (t.size, t.font, t.color, t.line_h, t.pad, t.rot);
    let (clip_l, clip_r) = t.clip;
    pg.save();
    pg.clip_rect(clip_l, y, (clip_r - clip_l).max(0.0), h);
    if rot != 0 && rot != 255 {
        // Rotated text: anchored at the bottom-left (upward) or top-left (downward) of the cell.
        // Wrapped lines stack rightward for upward text and leftward for downward text, so
        // downward text starts its first line far enough in for the last to end at the left pad.
        let deg = (rot as f32).clamp(-90.0, 90.0);
        let angle = -deg.to_radians();
        let last = t.lines.len().saturating_sub(1) as f32;
        for (i, line) in t.lines.iter().enumerate() {
            let i = i as f32;
            let (ox, oy) = if deg > 0.0 {
                (x + pad + size * 0.8 * deg.to_radians().sin() + i * line_h, y + h - pad)
            } else {
                (x + pad + (last - i) * line_h, y + pad + size * 0.8)
            };
            pg.text_rotated(Point::new(ox, oy), size, font, color, line, angle);
        }
        pg.restore();
        return;
    }
    if rot == 255 {
        // Vertical stacked text.
        let chars: Vec<char> = t.lines.concat().chars().take(500).collect();
        let mut yy = y + pad + size * 0.8;
        for ch in chars {
            let s = ch.to_string();
            let cw = gridcraft_pdf::text_width(font, size, &s);
            pg.text(x + (w - cw) / 2.0, yy, size, font, color, &s);
            yy += line_h;
            if yy > y + h + line_h {
                break;
            }
        }
        pg.restore();
        return;
    }
    for PlacedLine { text: line, x: lx, baseline: ly, width: lw } in t.placed() {
        pg.text(lx, ly, size, font, color, line);
        if t.underline != Underline::None {
            let uw = (size * 0.06).max(0.2);
            pg.stroke_line(lx, ly + size * 0.12, lx + lw, ly + size * 0.12, uw, color);
            if matches!(t.underline, Underline::Double | Underline::DoubleAccounting) {
                pg.stroke_line(lx, ly + size * 0.24, lx + lw, ly + size * 0.24, uw, color);
            }
        }
        if t.strike {
            pg.stroke_line(lx, ly - size * 0.3, lx + lw, ly - size * 0.3, (size * 0.06).max(0.2), color);
        }
    }
    pg.restore();
}
