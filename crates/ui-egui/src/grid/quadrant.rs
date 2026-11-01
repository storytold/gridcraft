use super::cell_paint::{
    Look, MergedArea, cell_text_y, center_across_end, col32, display_text, look, merged_edges, paint_border, paint_cf_icon, painted_halign,
    rotated_text_pos, shrink_font, wrapped_cell_text_y,
};
use super::geo::Geo;
use super::tables::{filter_button_rect, paint_filter_button};
use super::{
    CellRef, CfCache, CfLook, Color32, FontId, HAlign, MAX_COLS, Painter, Pos2, RangeRef, Rect, Sheet, Stroke, StrokeKind, Tokens, Underline, VAlign,
    Value, Workbook, pos2, theme, vec2,
};

/// Mirror what the grid painted into `layer` (the shape indices in `range`) for a right-to-left
/// sheet. Geometry is reflected; text and meshes (glyphs, pictures) keep their reading direction
/// and only move so that their box lands mirrored. Floating objects (`objects`: shape index
/// ranges with their logical rect) move as a whole, so a chart keeps its axes and a picture
/// isn't flipped.
pub(super) fn mirror_painted(
    ctx: &egui::Context,
    layer: egui::LayerId,
    range: std::ops::Range<usize>,
    geo: &Geo,
    objects: &[crate::chartview::PaintedObject],
) {
    let axis = geo.rect.left() + geo.rect.right();
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        for i in range {
            let object = objects.iter().find(|o| o.shapes.contains(&i));
            list.mutate_shape(egui::layers::ShapeIdx(i), |cs| {
                cs.clip_rect = geo.mirror_rect(cs.clip_rect);
                match object {
                    Some(o) => cs.shape.translate(vec2(axis - o.rect.right() - o.rect.left(), 0.0)),
                    None => mirror_shape(&mut cs.shape, axis),
                }
            });
        }
    });
}

/// Reflect one shape across the vertical line `x = axis / 2`.
fn mirror_shape(shape: &mut egui::Shape, axis: f32) {
    use egui::Shape;
    let flip = |p: &mut Pos2| p.x = axis - p.x;
    match shape {
        Shape::Noop | Shape::Callback(_) => {}
        Shape::Vec(v) => v.iter_mut().for_each(|s| mirror_shape(s, axis)),
        Shape::Circle(c) => flip(&mut c.center),
        Shape::Ellipse(e) => flip(&mut e.center),
        Shape::LineSegment { points, .. } => points.iter_mut().for_each(flip),
        Shape::Path(p) => {
            p.points.iter_mut().for_each(flip);
            // Keep the preferred (clockwise) winding for filled polygons.
            p.points.reverse();
        }
        Shape::Rect(r) => {
            r.rect = Rect::from_min_max(pos2(axis - r.rect.right(), r.rect.top()), pos2(axis - r.rect.left(), r.rect.bottom()));
            let c = r.corner_radius;
            r.corner_radius = egui::CornerRadius { nw: c.ne, ne: c.nw, sw: c.se, se: c.sw };
        }
        Shape::QuadraticBezier(b) => b.points.iter_mut().for_each(flip),
        Shape::CubicBezier(b) => b.points.iter_mut().for_each(flip),
        _ => {
            let b = shape.visual_bounding_rect();
            if b.is_finite() {
                shape.translate(vec2(axis - b.right() - b.left(), 0.0));
            }
        }
    }
}

/// Restrict top-aligned wrapped text to complete lines inside the cell.
///
/// Preserve the cell and viewport clip when every line fits. Otherwise,
/// exclude overflowing lines while retaining the visible fitting lines.
fn wrapped_text_clip(p: &Painter, rect: Rect, galley: &egui::Galley, y: f32, valign: VAlign) -> Rect {
    let mut clip = rect.intersect(p.clip_rect());
    if valign != VAlign::Top {
        return clip;
    }

    let Some(last_row) = galley.rows.last() else {
        return clip;
    };

    if y + last_row.rect().bottom() <= rect.bottom() + 0.01 {
        return clip;
    }

    let last_full_bottom = galley.rows.iter().map(|row| y + row.rect().bottom()).rfind(|bottom| *bottom <= rect.bottom() + 0.01);

    clip.max.y = last_full_bottom.map(|bottom| clip.bottom().min(bottom)).unwrap_or(clip.top()).max(clip.top());

    clip
}
pub(crate) struct QuadrantPaint<'a> {
    pub(crate) painter: &'a Painter,
    pub(crate) geo: &'a Geo,
    pub(crate) workbook: &'a Workbook,
    pub(crate) sheet_index: usize,
    pub(crate) sheet: &'a Sheet,
    pub(crate) rows: (u32, u32),
    pub(crate) cols: (u32, u32),
    pub(crate) cf: &'a mut CfCache,
    pub(crate) tokens: &'a Tokens,
}

/// Paint a worksheet quadrant, including backgrounds, borders, and text.
///
/// For top-aligned wrapped text, painting is limited to complete lines
/// fitting within the cell height while respecting viewport clipping.
/// Other vertical alignment modes retain their existing behavior.
pub(crate) fn paint_quadrant(mut quadrant: QuadrantPaint<'_>) {
    let (r0, r1) = quadrant.rows;
    let (c0, c1) = quadrant.cols;
    let range = RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1));
    let merges = visible_merges(&quadrant, range);
    paint_gridlines(&quadrant);
    let cells = visible_cells(&quadrant);
    let looks = paint_backgrounds(&mut quadrant, &cells, &merges);
    let outside = outside_text_cells(&mut quadrant, &merges);
    paint_cells(&mut quadrant, &looks, &outside, &merges);
    paint_borders(&mut quadrant, &looks, &merges);
    paint_sparklines(&quadrant, range);
    paint_filter_buttons(&quadrant, range);
}

fn visible_merges(q: &QuadrantPaint<'_>, range: RangeRef) -> Vec<RangeRef> {
    let (r0, r1) = (range.start.row, range.end.row);
    let (c0, c1) = (range.start.col, range.end.col);
    let near = RangeRef::new(CellRef::new(r0, c0.saturating_sub(20)), CellRef::new(r1, c1.saturating_add(20).min(MAX_COLS - 1)));
    q.sheet.merges.iter().filter(|m| m.intersects(&near)).copied().collect()
}

fn paint_gridlines(q: &QuadrantPaint<'_>) {
    let p = q.painter;
    let geo = q.geo;
    let sh = q.sheet;
    let t = q.tokens;
    let (r0, r1) = q.rows;
    let (c0, c1) = q.cols;
    // Gridlines.
    if sh.show_gridlines {
        let stroke = Stroke::new(1.0, t.gridline);
        let top = geo.y(sh, r0);
        let bottom = geo.y(sh, r1 + 1);
        let left = geo.x(sh, c0);
        let right = geo.x(sh, c1 + 1);
        for c in c0..=c1 + 1 {
            if c <= c1 && sh.col_width(c) == 0.0 {
                continue;
            }
            let x = geo.x(sh, c).round() - 0.5;
            p.line_segment([pos2(x, top), pos2(x, bottom)], stroke);
        }
        for r in r0..=r1 + 1 {
            if r <= r1 && sh.row_height(r) == 0.0 {
                continue;
            }
            let y = geo.y(sh, r).round() - 0.5;
            p.line_segment([pos2(left, y), pos2(right, y)], stroke);
        }
    }
}

fn visible_cells(q: &QuadrantPaint<'_>) -> Vec<CellRef> {
    let sh = q.sheet;
    let (r0, r1) = q.rows;
    let (c0, c1) = q.cols;
    // Merged areas: blank them (no inner gridlines) and paint as one cell.
    let mut cells: Vec<CellRef> = Vec::new();
    for r in r0..=r1 {
        if sh.row_height(r) == 0.0 {
            continue;
        }
        for c in c0..=c1 {
            if sh.col_width(c) == 0.0 {
                continue;
            }
            cells.push(CellRef::new(r, c));
        }
    }
    cells
}

fn paint_backgrounds(q: &mut QuadrantPaint<'_>, cells: &[CellRef], merges: &[RangeRef]) -> Vec<(CellRef, Rect, Look, Option<CfLook>)> {
    let p = q.painter;
    let geo = q.geo;
    let wb = q.workbook;
    let si = q.sheet_index;
    let sh = q.sheet;
    let cf = &mut *q.cf;
    let (r0, _) = q.rows;
    let c0 = q.cols.0;
    let in_merge = |c: CellRef| merges.iter().find(|m| m.contains(c)).copied();
    // Backgrounds.
    let mut looks: Vec<(CellRef, Rect, Look, Option<CfLook>)> = Vec::with_capacity(cells.len().min(20_000));
    for &c in cells {
        let (rect, anchor) = match in_merge(c) {
            Some(m) if m.start != c && !(c.row == r0.max(m.start.row) && c.col == c0.max(m.start.col)) => continue,
            Some(m) => (geo.range_rect(sh, m), m.start),
            None => (geo.cell_rect(sh, c), c),
        };
        let (lk, cfl) = look(wb, si, sh, anchor, cf);
        if let Some(f) = lk.fill {
            p.rect_filled(rect.expand(0.0), 0.0, f);
        } else if in_merge(c).is_some() {
            p.rect_filled(rect.shrink(0.5), 0.0, Color32::WHITE);
        }
        looks.push((anchor, rect, lk, cfl));
    }
    looks
}

fn outside_text_cells(q: &mut QuadrantPaint<'_>, merges: &[RangeRef]) -> Vec<(CellRef, Rect, Look, Option<CfLook>)> {
    let geo = q.geo;
    let wb = q.workbook;
    let si = q.sheet_index;
    let sh = q.sheet;
    let cf = &mut *q.cf;
    let (r0, r1) = q.rows;
    let (c0, c1) = q.cols;
    let in_merge = |c: CellRef| merges.iter().find(|m| m.contains(c)).copied();
    let empty_ok = |c: CellRef| sh.value_ref(c).is_none_or(Value::is_empty) && in_merge(c).is_none();
    // Text cells just outside the quadrant whose text may overflow into it: the nearest non-empty
    // cell left of `c0` and right of `c1` in each row (within the same frozen / scrolled pane).
    let mut outside: Vec<(CellRef, Rect, Look, Option<CfLook>)> = Vec::new();
    let same_pane = |col: u32| (col < geo.fc) == (c0 < geo.fc);
    for r in r0..=r1 {
        if sh.row_height(r) == 0.0 {
            continue;
        }
        let left = (c0.saturating_sub(20)..c0).rev().map(|col| CellRef::new(r, col)).find(|&n| !empty_ok(n));
        let right = (c1.saturating_add(1)..=c1.saturating_add(20).min(MAX_COLS - 1)).map(|col| CellRef::new(r, col)).find(|&n| !empty_ok(n));
        for n in [left, right].into_iter().flatten() {
            if same_pane(n.col) && sh.col_width(n.col) > 0.0 && in_merge(n).is_none() {
                let (lk, cfl) = look(wb, si, sh, n, cf);
                outside.push((n, geo.cell_rect(sh, n), lk, cfl));
            }
        }
    }
    outside
}

fn paint_cells(
    q: &mut QuadrantPaint<'_>,
    looks: &[(CellRef, Rect, Look, Option<CfLook>)],
    outside: &[(CellRef, Rect, Look, Option<CfLook>)],
    merges: &[RangeRef],
) {
    for cell in looks.iter().chain(outside) {
        paint_cell(q, cell, merges);
    }
}

fn paint_cell_decorations(q: &QuadrantPaint<'_>, c: CellRef, rect: Rect, lk: &Look, cfl: Option<&CfLook>) -> bool {
    let v = q.sheet.value(c);
    let p = q.painter;
    let z = q.geo.z;
    if let Some(picture) = q.sheet.cell_pictures.get(&c) {
        crate::cell_pictures::paint(p, rect, picture);
        return true;
    }
    if let Some(l) = cfl
        && let Some((frac, rgb, gradient)) = l.bar
    {
        paint_data_bar(p, rect, frac, rgb, gradient);
    }
    if let Some(l) = cfl
        && let Some((set, idx)) = &l.icon
    {
        let r = Rect::from_min_size(pos2(rect.left() + 2.0, rect.center().y - 6.0 * z), vec2(12.0 * z, 12.0 * z));
        paint_cf_icon(p, r, set, *idx);
    }
    if lk.style.num_fmt.as_str() == "checkbox" && matches!(v, Value::Bool(_) | Value::Empty) {
        paint_checkbox(p, rect, z, v == Value::Bool(true));
        return true;
    }
    false
}

fn paint_data_bar(p: &egui::Painter, rect: Rect, frac: f32, rgb: [u8; 3], gradient: bool) {
    let bar = Rect::from_min_size(pos2(rect.left() + 2.0, rect.top() + 2.0), vec2((rect.width() - 4.0) * frac, rect.height() - 4.0));
    let col = theme::color32(rgb);
    if gradient {
        let mut mesh = egui::Mesh::default();
        let light =
            Color32::from_rgb(((rgb[0] as u16 + 255 * 3) / 4) as u8, ((rgb[1] as u16 + 255 * 3) / 4) as u8, ((rgb[2] as u16 + 255 * 3) / 4) as u8);
        let i = mesh.vertices.len() as u32;
        for (pt, cc) in [(bar.left_top(), col), (bar.right_top(), light), (bar.right_bottom(), light), (bar.left_bottom(), col)] {
            mesh.colored_vertex(pt, cc);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
        p.add(mesh);
        p.rect_stroke(bar, 0.0, Stroke::new(1.0, col), StrokeKind::Inside);
    } else {
        p.rect_filled(bar, 0.0, col);
    }
}

fn paint_checkbox(p: &egui::Painter, rect: Rect, z: f32, on: bool) {
    let s = (11.0 * z * 1.2).clamp(8.0, 24.0);
    let b = Rect::from_center_size(rect.center(), vec2(s, s));
    let color = if on { Color32::from_rgb(0x10, 0x7C, 0x41) } else { Color32::WHITE };
    p.rect_filled(b, 2.0, color);
    p.rect_stroke(b, 2.0, Stroke::new(1.0, if on { color } else { Color32::from_gray(110) }), StrokeKind::Inside);
    if on {
        crate::icons::paint_on_sheet(p, b.shrink(1.5), crate::icons::Icon::Check, Color32::WHITE);
    }
}

fn text_overflows_cell(numeric: bool, value: &Value, rotation: i16, width: f32, rect_width: f32, pad: f32) -> bool {
    !numeric && !matches!(value, Value::Number(_)) && rotation.abs() != 90 && width > rect_width - 2.0 * pad
}

fn should_expand_text(filter_space: f32, overflows: bool, across_end: u32, col: u32, in_merge: bool) -> bool {
    filter_space == 0.0 && (overflows || across_end > col) && !in_merge
}

/// Paint one worksheet cell using its computed appearance and visible geometry.
///
/// Handles text layout, wrapping, alignment, overflow into neighbouring cells,
/// and clipping, while preserving cell decoration and merged-cell constraints.
fn paint_cell(q: &QuadrantPaint<'_>, painted: &(CellRef, Rect, Look, Option<CfLook>), merges: &[RangeRef]) {
    let (cell, rect, lk, cfl) = painted;
    let c = *cell;
    let p = q.painter;
    let geo = q.geo;
    let wb = q.workbook;
    let si = q.sheet_index;
    let sh = q.sheet;
    let t = q.tokens;
    let z = geo.z;
    let (c0, c1) = q.cols;
    let in_merge = |cell| merges.iter().find(|m| m.contains(cell)).copied();
    let empty_ok = |cell| sh.value_ref(cell).is_none_or(Value::is_empty) && in_merge(cell).is_none();
    if paint_cell_decorations(q, c, *rect, lk, cfl.as_ref()) {
        return;
    }
    let v = sh.value(c);
    let mut text_left = rect.left();
    if let Some(l) = cfl
        && let Some((set, idx)) = &l.icon
    {
        let r = Rect::from_min_size(pos2(rect.left() + 2.0, rect.center().y - 6.0 * z), vec2(12.0 * z, 12.0 * z));
        paint_cf_icon(p, r, set, *idx);
        text_left += 15.0 * z;
    }
    if cfl.as_ref().is_some_and(|l| l.hide_value) || v.is_empty() {
        return;
    }
    // Align and clip header text inside the content area, away from the dropdown.
    let filter_space = sh.filter_button_space(c) * z;
    let text_rect = Rect::from_min_max(rect.min, pos2((rect.right() - filter_space).max(rect.left()), rect.bottom()));
    let rect = &text_rect;
    let st = &lk.style;
    let size = st.font.size * 96.0 / 72.0 * z;
    let fam = theme::cell_family(&st.font.name, st.font.bold, st.font.italic);
    let font = FontId::new(size, fam);
    // Digits fit by the cell's own font and size, not the default 11pt (9pt marks in narrow columns).
    let char_w = p.layout_no_wrap("0".to_string(), font.clone(), Color32::PLACEHOLDER).size().x.max(1.0);
    let avail_w = rect.right() - text_left;
    let (mut text, ncolor, numeric, fill_char) = display_text(wb, sh, c, &v, st, avail_w / z.max(0.1) * z, char_w);
    if sh.show_formulas && sh.cell(c).is_some_and(|cell| cell.formula.is_some()) {
        text = crate::formula_locale::display(&text, crate::i18n::current(p.ctx()).formula_locale(), wb, si);
    }
    if text.is_empty() {
        return;
    }
    let color = ncolor.unwrap_or(lk.text_color);
    let halign = match st.align.h {
        HAlign::General => match v {
            Value::Number(_) => HAlign::Right,
            Value::Bool(_) | Value::Error(_) => HAlign::Center,
            _ if numeric => HAlign::Right,
            _ => HAlign::Left,
        },
        h => painted_halign(h, geo.rtl),
    };
    let indent = st.align.indent as f32 * 9.0 * z;
    let pad = 2.5 * z;
    let rotated = st.align.rotation != 0 && st.align.rotation != 255;
    // Rotated text wraps along its own direction (below), not across the column width.
    if !rotated && (st.align.wrap || st.align.h == HAlign::Justify) {
        let wrap_w = (rect.width() - 2.0 * pad - indent).max(4.0);
        let mut job = egui::text::LayoutJob::simple(text.clone(), font.clone(), color, wrap_w);
        job.halign = match halign {
            HAlign::Center | HAlign::CenterAcross => egui::Align::Center,
            HAlign::Right => egui::Align::RIGHT,
            _ => egui::Align::LEFT,
        };
        let galley = p.layout_job(job);
        let x = match halign {
            HAlign::Center | HAlign::CenterAcross => rect.center().x,
            HAlign::Right => rect.right() - pad,
            _ => text_left + pad + indent,
        };
        let y = wrapped_cell_text_y(st.align.v, *rect, galley.size().y, z);
        let text_clip = wrapped_text_clip(p, *rect, &galley, y, st.align.v);
        p.with_clip_rect(text_clip).galley(pos2(x, y), galley, color);
        return;
    }
    // Wrap Text off: Excel ignores the line breaks and draws the whole string on one line.
    let first_line: String = text.chars().filter(|&ch| ch != '\n' && ch != '\r').collect();
    // The fill position indexes `text`; move it back by the breaks removed before it.
    let fill_char = fill_char.map(|(ch, pos)| (ch, pos.saturating_sub(text.get(..pos).map_or(0, |s| s.matches(['\n', '\r']).count()))));
    // Shrink to fit: too-wide content is drawn in a smaller font so it fits the cell (or the
    // merged area): it neither spills, nor is clipped, nor turns into ####.
    let small = (st.align.shrink && !rotated && st.align.h != HAlign::Fill && fill_char.is_none())
        .then(|| shrink_font(p, &first_line, &font, avail_w - 2.0 * pad - indent))
        .flatten();
    let shrunk = small.is_some();
    let font = small.unwrap_or(font);
    let mut galley = p.layout_no_wrap(first_line.clone(), font.clone(), color);
    let mut w = galley.size().x;
    // Numbers and dates never overflow: they show #### when too narrow.
    let hashed = !shrunk && (numeric || matches!(v, Value::Number(_))) && w > avail_w - 2.0 * pad && st.align.h != HAlign::Fill;
    if hashed {
        // Measured on `#` itself: it is wider than `0` in some fonts, and #### must fit its cell.
        let hash_w = p.layout_no_wrap("#".to_string(), font.clone(), color).size().x.max(1.0);
        let n = ((avail_w - 2.0 * pad) / hash_w).floor().max(1.0) as usize;
        galley = p.layout_no_wrap("#".repeat(n), font.clone(), color);
        w = galley.size().x;
    }
    // Repeat fill (`*` in number formats) / Fill alignment. A number already shown as #### keeps
    // it: rebuilding it from the text would overflow and clip its leading digits.
    if let Some((ch, pos)) = fill_char
        && !hashed
        && pos <= first_line.len()
    {
        let base_w = w;
        let cw = p.layout_no_wrap(ch.to_string(), font.clone(), color).size().x.max(1.0);
        let n = (((avail_w - 2.0 * pad) - base_w) / cw).floor().max(0.0) as usize;
        let mut s = first_line.clone();
        if first_line.is_char_boundary(pos) {
            s.insert_str(pos, &ch.to_string().repeat(n));
        }
        galley = p.layout_no_wrap(s, font.clone(), color);
        w = galley.size().x;
    }
    // Center Across Selection: the text is centred over this cell and the empty cells to its right that also use it.
    let across_end = if halign == HAlign::CenterAcross && !rotated && in_merge(c).is_none() {
        center_across_end(c.col, c1 + 20, |col| {
            let n = CellRef::new(c.row, col);
            empty_ok(n) && wb.styles.get(sh.style_id(n)).align.h == HAlign::CenterAcross
        })
    } else {
        c.col
    };
    // Overflow into empty neighbours for text (not upright text, whose width is its line height).
    let mut clip = *rect;
    let overflows = text_overflows_cell(numeric, &v, st.align.rotation, w, rect.width(), pad);
    if should_expand_text(filter_space, overflows, across_end, c.col, in_merge(c).is_some()) {
        let mut right = rect.right();
        let mut left = rect.left();
        if across_end > c.col {
            right = geo.x(sh, across_end + 1);
        }
        if matches!(halign, HAlign::Left | HAlign::Center | HAlign::Justify) {
            let mut cc = c.col + 1;
            while right - rect.left() < w + 2.0 * pad && cc < MAX_COLS && empty_ok(CellRef::new(c.row, cc)) && cc <= c1 + 20 {
                right = geo.x(sh, cc + 1);
                cc += 1;
            }
        }
        if matches!(halign, HAlign::Right | HAlign::Center) {
            let mut cc = c.col;
            while rect.right() - left < w + 2.0 * pad && cc > 0 && empty_ok(CellRef::new(c.row, cc - 1)) && cc + 20 > c0 {
                cc -= 1;
                left = geo.x(sh, cc);
            }
        }
        clip = Rect::from_min_max(pos2(left, rect.top()), pos2(right, rect.bottom()));
        // Paint over the gridlines the text passes through.
        if lk.fill.is_none() {
            let inner = Rect::from_min_max(pos2(clip.left() + 1.0, clip.top() + 0.5), pos2(clip.right() - 1.0, clip.bottom() - 1.0));
            let extra_l = Rect::from_min_max(inner.min, pos2(rect.left() + 1.0, inner.bottom()));
            let extra_r = Rect::from_min_max(pos2(rect.right() - 1.0, inner.top()), inner.max);
            if extra_l.width() > 0.0 {
                p.rect_filled(extra_l, 0.0, t.grid_bg);
            }
            if extra_r.width() > 0.0 {
                p.rect_filled(extra_r, 0.0, t.grid_bg);
            }
        }
    }
    let x = match halign {
        HAlign::CenterAcross => clip.center().x - w / 2.0,
        HAlign::Center | HAlign::Distributed => rect.center().x - w / 2.0,
        HAlign::Right => rect.right() - pad - w - indent,
        _ => text_left + pad + indent,
    };
    let gh = galley.size().y;
    let y = cell_text_y(st.align.v, *rect, gh, z);
    let cp = p.with_clip_rect(clip.intersect(p.clip_rect()));
    if rotated {
        let angle = -(st.align.rotation as f32).to_radians();
        // Upright (±90°) wrapped text wraps to the row height: lines then stack across the column.
        let galley = if st.align.wrap && st.align.rotation.abs() == 90 {
            let job = egui::text::LayoutJob::simple(text.clone(), font.clone(), color, (rect.height() - 2.0 * pad).max(4.0));
            p.layout_job(job)
        } else {
            galley.clone()
        };
        let pos = rotated_text_pos(*rect, galley.size(), angle, painted_halign(st.align.h, geo.rtl), st.align.v, pad, indent);
        let mut shape = egui::epaint::TextShape::new(pos, galley, color);
        shape.angle = angle;
        cp.add(shape);
    } else {
        cp.galley(pos2(x, y), galley.clone(), color);
    }
    paint_text_decorations(&cp, TextDecoration { x, y, width: w, height: gh, color, underline: st.font.underline, strike: st.font.strike });
}

struct TextDecoration {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color32,
    underline: Underline,
    strike: bool,
}

fn paint_text_decorations(p: &Painter, decoration: TextDecoration) {
    let TextDecoration { x, y, width, height, color, underline, strike } = decoration;
    let baseline = y + height * 0.82;
    match underline {
        Underline::None => {}
        Underline::Double | Underline::DoubleAccounting => {
            p.line_segment([pos2(x, baseline + 1.0), pos2(x + width, baseline + 1.0)], Stroke::new(1.0, color));
            p.line_segment([pos2(x, baseline + 3.0), pos2(x + width, baseline + 3.0)], Stroke::new(1.0, color));
        }
        _ => {
            p.line_segment([pos2(x, baseline + 1.0), pos2(x + width, baseline + 1.0)], Stroke::new(1.0, color));
        }
    }
    if strike {
        p.line_segment([pos2(x, y + height * 0.55), pos2(x + width, y + height * 0.55)], Stroke::new(1.0, color));
    }
}

fn paint_borders(q: &mut QuadrantPaint<'_>, looks: &[(CellRef, Rect, Look, Option<CfLook>)], merges: &[RangeRef]) {
    let p = q.painter;
    let geo = q.geo;
    let wb = q.workbook;
    let si = q.sheet_index;
    let sh = q.sheet;
    let cf = &mut *q.cf;
    let t = q.tokens;
    let z = geo.z;
    let (r0, r1) = q.rows;
    let (c0, c1) = q.cols;
    // Borders (over fills and text).
    for (c, rect, lk, _) in looks {
        let b = &lk.style.borders;
        let merge = merges.iter().find(|m| m.start == *c);
        let edges = match merge {
            // A merged area's outline lives on the cells along each of its edges (Excel writes the
            // right edge on the last column's cells and the bottom edge on the last row's), so each
            // edge is drawn cell by cell from the cell it belongs to.
            Some(m) => merged_edges(wb, si, sh, geo, MergedArea { m: *m, rect: *rect, rows: (r0, r1), cols: (c0, c1) }, cf),
            None => vec![
                (b.top, [rect.left_top(), rect.right_top()]),
                (b.bottom, [rect.left_bottom(), rect.right_bottom()]),
                (b.left, [rect.left_top(), rect.left_bottom()]),
                (b.right, [rect.right_top(), rect.right_bottom()]),
            ],
        };
        for (line, pts) in edges {
            if line.is_none() {
                continue;
            }
            let col = col32(line.color, wb, Color32::BLACK);
            paint_border(p, pts, line.style, col);
        }
        if !b.diag_down.is_none() {
            paint_border(p, [rect.left_top(), rect.right_bottom()], b.diag_down.style, col32(b.diag_down.color, wb, Color32::BLACK));
        }
        if !b.diag_up.is_none() {
            paint_border(p, [rect.left_bottom(), rect.right_top()], b.diag_up.style, col32(b.diag_up.color, wb, Color32::BLACK));
        }
        // Comment / note indicator.
        if let Some(cm) = sh.comments.get(c) {
            let tri_col = if cm.threaded { Color32::from_rgb(0x88, 0x55, 0xC8) } else { Color32::from_rgb(0xD0, 0x2E, 0x2E) };
            let s = 6.0 * z.max(0.7);
            p.add(egui::Shape::convex_polygon(
                vec![rect.right_top(), pos2(rect.right(), rect.top() + s), pos2(rect.right() - s, rect.top())],
                tri_col,
                Stroke::NONE,
            ));
        }
        // Background error checking: a number stored as text gets Excel's green corner (#1994).
        if !sh.show_formulas && sh.cell(*c).and_then(gridcraft_engine::cmd::formulas::number_stored_as_text).is_some() {
            let s = 6.0 * z.max(0.7);
            p.add(egui::Shape::convex_polygon(
                vec![rect.left_top(), pos2(rect.left() + s, rect.top()), pos2(rect.left(), rect.top() + s)],
                t.error_check_green,
                Stroke::NONE,
            ));
        }
    }
}

fn paint_sparklines(q: &QuadrantPaint<'_>, range: RangeRef) {
    let p = q.painter;
    let geo = q.geo;
    let wb = q.workbook;
    let si = q.sheet_index;
    let sh = q.sheet;
    // Sparklines.
    for sp in &sh.sparklines {
        if !range.contains(sp.cell) {
            continue;
        }
        let rect = geo.cell_rect(sh, sp.cell).shrink(3.0);
        let vals: Vec<Option<f64>> = match gridcraft_engine::calc::evaluate(wb, si, sp.cell, &sp.source) {
            Value::Array(a) => a.data.iter().map(Value::as_f64).collect(),
            v => vec![v.as_f64()],
        };
        let col = sp.color.resolve(&wb.theme).unwrap_or([0x2B, 0x7C, 0xD3]);
        let prims = gridcraft_chart::render_sparkline(sp.kind, &vals, [col[0], col[1], col[2], 255], sp.markers, rect.width(), rect.height());
        crate::chartview::paint_prims(p, rect.min, &prims, 1.0);
    }
}

fn paint_filter_buttons(q: &QuadrantPaint<'_>, range: RangeRef) {
    let p = q.painter;
    let geo = q.geo;
    let sh = q.sheet;
    // Filter buttons.
    let mut buttons: Vec<(CellRef, bool)> = Vec::new();
    if let Some(af) = &sh.autofilter {
        for col in af.range.start.col..=af.range.end.col {
            let active = af.criteria.iter().any(|(o, _)| af.range.start.col + o == col);
            buttons.push((CellRef::new(af.range.start.row, col), active));
        }
    }
    for tb in &sh.tables {
        if tb.filter_button && tb.header_row {
            for col in tb.range.start.col..=tb.range.end.col {
                buttons.push((CellRef::new(tb.range.start.row, col), false));
            }
        }
    }
    for (c, active) in buttons {
        if !range.contains(c) {
            continue;
        }
        paint_filter_button(p, filter_button_rect(geo, sh, c), active);
    }
}
