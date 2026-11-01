use super::draw::{Block, Look, rgb_of};
use super::{
    Baseline, CellRef, Color, Font, HAlign, MAX_COLS, Page, Point, RangeRef, ReadingOrder, Sheet, Style, StyleTable, TextDirection, Underline,
    VAlign, Value,
};
use unicode_segmentation::UnicodeSegmentation;

const PAD_PX: f32 = 2.0;

pub(super) struct TextLine {
    pub(super) text: String,
    pub(super) direction: TextDirection,
}

/// Word-wraps paragraphs, keeping their base direction on every continuation line.
pub(super) fn wrap_lines(text: &str, font: Font, size: f32, width: f32, direction: TextDirection) -> Vec<TextLine> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let direction = direction.resolve(para);
        let measured = |text: &str| gridcraft_pdf::text_width_with_direction(font, size, text, direction);
        let mut line = String::new();
        for word in para.split(' ') {
            let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if line.is_empty() || measured(&cand) <= width {
                line = cand;
            } else {
                out.push(TextLine { text: std::mem::take(&mut line), direction });
                line = word.to_string();
            }
            // Keep marks with their base letter. Each line's split is searched from its own start,
            // so a long word costs about its emitted lines, each measured in its final joined form.
            if measured(&line) > width {
                let boundaries: Vec<usize> = line.grapheme_indices(true).map(|(i, _)| i).chain(std::iter::once(line.len())).collect();
                let graphemes = boundaries.len().saturating_sub(1);
                let piece =
                    |from: usize, to: usize| boundaries.get(from).zip(boundaries.get(to)).and_then(|(&a, &b)| line.get(a..b)).unwrap_or_default();
                let mut start = 0;
                while graphemes - start > 1 {
                    let left = graphemes - start;
                    let fits = |n: usize| measured(piece(start, start + n)) <= width;
                    let mut split = 1;
                    while split < left && fits((split * 2).min(left)) {
                        split = (split * 2).min(left);
                    }
                    if split == left {
                        break;
                    }
                    let (mut low, mut high) = (split + 1, (split * 2).min(left) - 1);
                    while low <= high {
                        let mid = low + (high - low) / 2;
                        if fits(mid) {
                            split = mid;
                            low = mid + 1;
                        } else {
                            high = mid - 1;
                        }
                    }
                    out.push(TextLine { text: piece(start, start + split).to_string(), direction });
                    start += split;
                    if out.len() > 2000 {
                        return out;
                    }
                }
                line = piece(start, graphemes).to_string();
            }
            if out.len() > 2000 {
                return out;
            }
        }
        out.push(TextLine { text: line, direction });
    }
    out
}

fn font_for(style: &Style) -> Font {
    Font::styled(Font::family_for(&style.font.name), style.font.bold, style.font.italic)
}

fn text_direction(order: ReadingOrder) -> TextDirection {
    match order {
        ReadingOrder::Context => TextDirection::Context,
        ReadingOrder::LeftToRight => TextDirection::LeftToRight,
        ReadingOrder::RightToLeft => TextDirection::RightToLeft,
    }
}

fn text_align(align: HAlign, value: &Value, right_to_left_text: bool) -> HAlign {
    match align {
        HAlign::General => match value {
            Value::Number(_) => HAlign::Right,
            Value::Bool(_) | Value::Error(_) => HAlign::Center,
            _ if right_to_left_text => HAlign::Right,
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

pub(super) fn draw_text(pg: &mut Page, b: &Block, placement: TextPlacement<'_>) {
    let TextPlacement { cell: c, look: l, rect, merged, merges, limits } = placement;
    let (x, y, w, h) = rect;
    let sh = b.sh;
    let wb = b.wb;
    let v = sh.value(c);
    if v.is_empty() || l.cf.as_ref().is_some_and(|cf| cf.hide_value) {
        return;
    }
    if !sh.show_zeros && v.as_f64() == Some(0.0) {
        return;
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
        return;
    }
    let direction = text_direction(st.align.reading_order.unwrap_or_default()).resolve(&text);
    let mut tw = gridcraft_pdf::text_width_with_direction(font, size, &text, direction);
    if st.align.shrink && !wrap && tw > avail && tw > 0.0 {
        size = (size * avail / tw).max(1.0);
        tw = gridcraft_pdf::text_width_with_direction(font, size, &text, direction);
    }
    if is_num && !wrap && tw > avail + 0.01 && !sh.show_formulas {
        let hash = gridcraft_pdf::text_width(font, size, "#").max(0.1);
        text = "#".repeat(((avail / hash).floor().max(1.0) as usize).min(255));
        tw = gridcraft_pdf::text_width_with_direction(font, size, &text, direction);
    }
    let h_align = text_align(st.align.h, &v, direction == TextDirection::RightToLeft && !sh.right_to_left);
    let rot = st.align.rotation;
    let rotated = rot != 0 && rot != 255;
    // Rotated text wraps along its own direction: upright (±90°) text to the row height.
    let lines: Vec<TextLine> = match (wrap, rotated) {
        (true, false) => wrap_lines(&text, font, size, avail.max(size), direction),
        (true, true) if rot.abs() == 90 => wrap_lines(&text, font, size, (h - 2.0 * pad).max(size), direction),
        (true, true) => vec![TextLine { text: text.lines().next().unwrap_or_default().to_string(), direction }],
        (false, _) => vec![TextLine { text, direction }],
    };
    let extents = lines
        .iter()
        .filter_map(|line| pg.text_vertical_extents_with_direction(font, size, &line.text, line.direction))
        .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)))
        .map(|(ascent, descent)| (ascent.max(size * 0.8), descent.max(size * 0.22)));
    let line_h = extents.map_or(size * 1.2, |(ascent, descent)| (ascent + descent + size * 0.2).max(size * 1.2));
    let n = lines.len().max(1) as f32;
    let first_baseline = if let Some((ascent, descent)) = extents {
        match st.align.v {
            VAlign::Top => y + pad + ascent,
            VAlign::Center | VAlign::Justify | VAlign::Distributed => y + (h - (n - 1.0) * line_h - ascent - descent) / 2.0 + ascent,
            VAlign::Bottom => y + h - pad - descent - (n - 1.0) * line_h,
        }
    } else {
        match st.align.v {
            VAlign::Top => y + pad + size * 0.8,
            VAlign::Center | VAlign::Justify | VAlign::Distributed => y + (h - n * line_h) / 2.0 + line_h * 0.5 + size * 0.33,
            VAlign::Bottom => y + h - pad - size * 0.22 - (n - 1.0) * line_h,
        }
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
    let clip_l = clip_l.max(limits.0);
    let clip_r = clip_r.min(limits.1);
    pg.save();
    pg.clip_rect(b.text_x(clip_l, clip_r - clip_l), y, (clip_r - clip_l).max(0.0), h);
    if rotated {
        // Rotated text: anchored at the bottom-left (upward) or top-left (downward) of the cell.
        // Wrapped lines stack rightward for upward text and leftward for downward text, so
        // downward text starts its first line far enough in for the last to end at the left pad.
        let deg = (rot as f32).clamp(-90.0, 90.0);
        let angle = -deg.to_radians();
        let last = lines.len().saturating_sub(1) as f32;
        for (i, t) in lines.iter().enumerate() {
            let i = i as f32;
            let (ox, oy) = if deg > 0.0 {
                (x + pad + extents.map_or(size * 0.8, |e| e.0) * deg.to_radians().sin() + i * line_h, y + h - pad)
            } else {
                (x + pad + (last - i) * line_h, y + pad + extents.map_or(size * 0.8, |e| e.0))
            };
            let tw = gridcraft_pdf::text_width_with_direction(font, size, &t.text, t.direction);
            let (sn, cs) = angle.sin_cos();
            let (ascent, descent) = extents.unwrap_or((size * 0.8, size * 0.22));
            let ox = if sh.right_to_left { b.mirror_axis - ox - tw * cs - (ascent - descent) * sn } else { ox };
            pg.text_rotated_with_direction(Baseline { origin: Point::new(ox, oy), angle }, size, font, color, &t.text, t.direction);
        }
        pg.restore();
        return;
    }
    if rot == 255 {
        // Vertical stacked text.
        let stacked: String = lines.iter().map(|line| line.text.as_str()).collect();
        let chars: Vec<&str> = stacked.graphemes(true).take(500).collect();
        let mut yy = y + pad + extents.map_or(size * 0.8, |e| e.0);
        for ch in chars {
            let cw = gridcraft_pdf::text_width(font, size, ch);
            pg.text_with_direction(Point::new(b.text_x(x + (w - cw) / 2.0, cw), yy), size, font, color, ch, direction);
            yy += line_h;
            if yy > y + h + line_h {
                break;
            }
        }
        pg.restore();
        return;
    }
    for (i, line) in lines.iter().enumerate() {
        let lw = gridcraft_pdf::text_width_with_direction(font, size, &line.text, line.direction);
        let lx = match h_align {
            HAlign::Right => x + w - pad - lw - indent,
            HAlign::Center => x + (w + across - lw) / 2.0,
            _ => x + pad + indent,
        };
        let ly = first_baseline + i as f32 * line_h;
        if ly - size > y + h || ly < y - line_h {
            continue;
        }
        let lx = b.text_x(lx, lw);
        pg.text_with_direction(Point::new(lx, ly), size, font, color, &line.text, line.direction);
        if st.font.underline != Underline::None {
            let uw = (size * 0.06).max(0.2);
            pg.stroke_line(lx, ly + size * 0.12, lx + lw, ly + size * 0.12, uw, color);
            if matches!(st.font.underline, Underline::Double | Underline::DoubleAccounting) {
                pg.stroke_line(lx, ly + size * 0.24, lx + lw, ly + size * 0.24, uw, color);
            }
        }
        if st.font.strike {
            pg.stroke_line(lx, ly - size * 0.3, lx + lw, ly - size * 0.3, (size * 0.06).max(0.2), color);
        }
    }
    pg.restore();
}
