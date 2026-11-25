use super::draw::{Block, PdfMeasure, Xf, draw_prims, rgb_of};
use super::text::wrap_lines;
use super::{Anchor, Baseline, CellRef, Font, Page, Point, RangeRef, Rect, ShapeKind, Sheet, TextDirection, Value, Workbook};
use std::borrow::Cow;
use std::collections::hash_map::Entry;

/// A decoded picture: RGBA pixels, width, height.
type Decoded = (Vec<u8>, u32, u32);

/// Most bytes of decoded pictures one render memoizes; a picture decoded past it is used once
/// and dropped (and decoded again on the next page that prints it).
const MAX_PIC_CACHE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum PicKey {
    /// A floating picture: sheet index and position in `Sheet::images`.
    Floating(usize, usize),
    /// A cell picture: sheet index, cell and the thumbnail size it is decoded at.
    Cell(usize, CellRef, u32, u32),
}

/// Decoded pictures reused across the pages (and repeated title rows) that print them, so a
/// picture is decoded once per render instead of once per page (#2389).
#[derive(Default)]
pub(super) struct PicCache {
    map: std::collections::HashMap<PicKey, Option<Decoded>>,
    /// Bytes of pixels in `map`.
    bytes: u64,
    /// Laid-out charts (with their [`chart_cost`](super::chart_cost)) keyed by (sheet, chart),
    /// rendered once however many pages they touch. Only charts that can still be drawn are kept.
    pub(super) charts: std::collections::HashMap<(usize, usize), (Vec<gridcraft_chart::Prim>, usize)>,
    /// Charts too big for what was left of the budget: never drawn, so not rendered again.
    pub(super) rejected_charts: std::collections::HashSet<(usize, usize)>,
    /// Chart cost units emitted so far. Each page a chart touches gets its own copy in the PDF,
    /// so a workbook-controlled anchor over thousands of pages would multiply them without bound.
    pub(super) chart_prims: usize,
}

impl PicCache {
    /// The decoded picture for `key`, running `decode` the first time. A failed decode is
    /// remembered too; a decode that does not fit in the cache is returned without being kept.
    fn get(&mut self, key: PicKey, decode: impl FnOnce() -> Option<Decoded>) -> Option<Cow<'_, Decoded>> {
        let slot = match self.map.entry(key) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => {
                let decoded = decode();
                let size = decoded.as_ref().map_or(0, |d| d.0.len() as u64);
                if self.bytes.saturating_add(size) > MAX_PIC_CACHE_BYTES {
                    return decoded.map(Cow::Owned);
                }
                self.bytes = self.bytes.saturating_add(size);
                e.insert(decoded)
            }
        };
        slot.as_ref().map(Cow::Borrowed)
    }

    /// The chart's primitives for one more page, or `None` once they no longer fit
    /// [`MAX_CHART_PRIMS`](super::MAX_CHART_PRIMS). `render` runs at most once per chart; a chart
    /// that doesn't fit is dropped at once and remembered, so its vectors are never retained.
    pub(super) fn chart(&mut self, key: (usize, usize), render: impl FnOnce() -> Vec<gridcraft_chart::Prim>) -> Option<&[gridcraft_chart::Prim]> {
        if self.rejected_charts.contains(&key) {
            return None;
        }
        let (prims, cost) = match self.charts.remove(&key) {
            Some(cached) => cached,
            None => {
                let prims = render();
                let cost = super::chart_cost(&prims);
                (prims, cost)
            }
        };
        if self.chart_prims.saturating_add(cost) > super::MAX_CHART_PRIMS {
            self.rejected_charts.insert(key);
            return None;
        }
        self.chart_prims += cost;
        Some(&self.charts.entry(key).or_insert((prims, cost)).0)
    }
}

/// Whether a picture decoded into at most `max_w`×`max_h` pixels can still be kept by the PDF.
/// The document's image room only shrinks while it is built, so a picture refused here would be
/// refused on every later page too and is not decoded at all.
fn fits(room: u64, max_w: u32, max_h: u32) -> bool {
    u64::from(max_w).saturating_mul(u64::from(max_h)).saturating_mul(4) <= room
}

/// Cell pictures fit the current cell (or merged-cell) rectangle without changing aspect ratio.
/// `at` (sheet index and cell) keys the decode in `pics`.
pub(super) fn draw_cell_picture(pg: &mut Page, pics: &mut PicCache, at: (usize, CellRef), data: &[u8], rect: (f32, f32, f32, f32), k: f32) {
    let (x, y, w, h) = rect;
    let pad = 2.0 * k;
    let (w, h) = ((w - 2.0 * pad).max(0.0), (h - 2.0 * pad).max(0.0));
    if w <= 0.0 || h <= 0.0 || data.len() > 16 * 1024 * 1024 {
        return;
    }
    let Some((iw, ih)) = crate::io::image_size(data) else { return };
    if iw == 0 || ih == 0 || iw > 8192 || ih > 8192 || u64::from(iw) * u64::from(ih) > 16_000_000 {
        return;
    }
    let (tw, th) = (((w / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32, ((h / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32);
    let room = pg.image_room();
    let Some(image) = pics.get(PicKey::Cell(at.0, at.1, tw, th), || decode_cell_picture(data, tw, th, room)) else { return };
    let (px, px_w, px_h) = &*image;
    let fit = (w / iw as f32).min(h / ih as f32);
    let (pw, ph) = (iw as f32 * fit, ih as f32 * fit);
    pg.image_rgba(Rect::new(x + pad + (w - pw) / 2.0, y + pad + (h - ph) / 2.0, pw, ph), px, [*px_w, *px_h]);
}

/// Decodes a cell picture to RGBA scaled to fit `tw`×`th` pixels, unless the PDF has no `room`
/// left for it.
fn decode_cell_picture(data: &[u8], tw: u32, th: u32, room: u64) -> Option<Decoded> {
    if !fits(room, tw, th) {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(tw, th).to_rgba8();
    let (pw, ph) = (image.width(), image.height());
    Some((image.into_raw(), pw, ph))
}

/// Sheet pixel rectangle of an anchor.
/// Most source lines printed for an equation that has no layout.
pub(super) const MAX_FALLBACK_LINES: usize = 500;

/// The lines of `text` printed in place of an equation that has no layout: those of its first
/// `MAX_SOURCE_BYTES` (a longer source never lays out, and a file can carry megabytes) that fit a
/// frame `h` points tall at `line_h` points per line (at least the first), and never more than
/// [`MAX_FALLBACK_LINES`]. A single `pg.text` call would print every newline as `?`.
pub(super) fn fallback_lines(text: &str, h: f32, line_h: f32) -> impl Iterator<Item = &str> {
    let text = crate::cmd::equation::source_excerpt(text);
    let fit = h / line_h;
    let n = if fit >= MAX_FALLBACK_LINES as f32 {
        MAX_FALLBACK_LINES
    } else if fit >= 1.0 {
        fit as usize
    } else {
        1
    };
    text.lines().take(n)
}

/// Object rectangle `(x, y, w, h)` in printed sheet-point coordinates relative to A1,
/// matching the coordinate space of each page block's `[px0..px1] × [py0..py1]` bounds.
fn anchor_printed_rect(b: &Block, sh: &Sheet, a: &Anchor) -> (f64, f64, f64, f64) {
    let kx = f64::from(b.kx);
    let ky = f64::from(b.ky);
    let k = f64::from(b.k);
    let w = f64::from(a.width.max(0.0));
    let h = f64::from(a.height.max(0.0));
    let cx = sh.col_left(a.cell.col);
    let cy = b.lay.row_top(sh, a.cell.row);
    let dx = f64::from(a.dx);
    let dy = f64::from(a.dy);
    match a.mode {
        gridcraft_model::AnchorMode::Absolute => (cx * kx + dx * k, cy * ky + dy * k, w * k, h * k),
        gridcraft_model::AnchorMode::MoveOnly => ((cx + dx) * kx, (cy + dy) * ky, w * k, h * k),
        gridcraft_model::AnchorMode::MoveAndSize => ((cx + dx) * kx, (cy + dy) * ky, w * kx, h * ky),
    }
}

/// Decodes an embedded picture to RGBA, downscaled to at most `max_w`×`max_h` pixels, unless the
/// PDF has no `room` left for it.
fn decode_image(data: &[u8], max_w: u32, max_h: u32, room: u64) -> Option<Decoded> {
    let (w, h) = crate::io::image_size(data)?;
    if w == 0 || h == 0 || w as u64 * h as u64 > 60_000_000 || !fits(room, w.min(max_w.max(1)), h.min(max_h.max(1))) {
        return None;
    }
    let img = image::load_from_memory(data).ok()?;
    let img = if img.width() > max_w.max(1) || img.height() > max_h.max(1) { img.thumbnail(max_w.max(1), max_h.max(1)) } else { img };
    let rgba = img.to_rgba8();
    let (pw, ph) = (rgba.width(), rgba.height());
    Some((rgba.into_raw(), pw, ph))
}

/// Draws charts, pictures, shapes and sparklines overlapping the body block of a page.
/// `pics` memoizes decoded pictures for the whole render (one page at a time draws each
/// picture again, but the decode happens once).
pub(super) fn draw_objects(pg: &mut Page, wb: &Workbook, si: usize, sh: &Sheet, b: &Block, pics: &mut PicCache) {
    let block = b.range();
    let (px0, py0) = (sh.col_left(block.start.col) * f64::from(b.kx), b.lay.row_top(sh, block.start.row) * f64::from(b.ky));
    let (px1, py1) =
        (sh.col_left(block.end.col.saturating_add(1)) * f64::from(b.kx), b.lay.row_top(sh, block.end.row.saturating_add(1)) * f64::from(b.ky));
    let (bx, by) = (b.xs.first().copied().unwrap_or(0.0), b.ys.first().copied().unwrap_or(0.0));
    let (bw, bh) = (b.xs.last().copied().unwrap_or(bx) - bx, b.ys.last().copied().unwrap_or(by) - by);
    let visible = |r: (f64, f64, f64, f64)| r.2 > 0.0 && r.3 > 0.0 && r.0 < px1 && r.0 + r.2 > px0 && r.1 < py1 && r.1 + r.3 > py0;
    let place = |r: (f64, f64, f64, f64)| {
        let ox = bx + (r.0 - px0) as f32;
        let oy = by + (r.1 - py0) as f32;
        let w = r.2 as f32;
        let h = r.3 as f32;
        (b.text_x(ox, w), oy, w, h)
    };
    pg.save();
    pg.clip_rect(b.text_x(bx, bw), by, bw, bh);
    draw_sparklines(pg, wb, si, sh, b, block);
    for (imi, im) in sh.images.iter().enumerate() {
        let r = anchor_printed_rect(b, sh, &im.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, w, h) = place(r);
        // About 150 dpi is plenty for print.
        let (mw, mh) = (((w / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32, ((h / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32);
        let room = pg.image_room();
        if let Some(decoded) = pics.get(PicKey::Floating(si, imi), || decode_image(&im.data, mw, mh, room)) {
            let (px, pw, ph) = &*decoded;
            pg.image_rgba(Rect::new(x, y, w, h), px, [*pw, *ph]);
        }
    }
    for shp in &sh.shapes {
        let r = anchor_printed_rect(b, sh, &shp.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, w, h) = place(r);
        if shp.kind == ShapeKind::Equation {
            let ink = rgb_of(shp.line, wb).unwrap_or([0, 0, 0]);
            match shp.math.get(&shp.text) {
                Some(Ok(list)) => crate::cmd::equation::draw_pdf(pg, list, (x, y, w, h), crate::cmd::equation::PADDING * b.k, ink),
                _ => {
                    // No layout (the source does not parse, or the file was past the layout
                    // budget): the source, one text run per line, as many as the frame holds.
                    let line_h = 12.0 * b.scale;
                    // A long line stays inside the frame; this clip nests in the block's.
                    pg.save();
                    pg.clip_rect(x, y, w, h);
                    for (i, line) in fallback_lines(&shp.text, h, line_h).enumerate() {
                        pg.text(x + 2.0 * b.k, y + line_h * (i + 1) as f32, 10.0 * b.scale, Font::Courier, ink, line);
                    }
                    pg.restore();
                }
            }
            continue;
        }
        draw_shape(pg, wb, shp, b, (x, y, w, h));
    }
    for (chi, ch) in sh.charts.iter().enumerate() {
        let r = anchor_printed_rect(b, sh, &ch.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, w, h) = place(r);
        let chart_w = if b.k > 0.0 { w / b.k } else { ch.anchor.width.max(0.0) };
        let chart_h = if b.k > 0.0 { h / b.k } else { ch.anchor.height.max(0.0) };
        // As for pictures, each page gets its own copy of the drawing commands.
        let prims = pics.chart((si, chi), || {
            let data = gridcraft_chart::resolve(wb, si, ch);
            gridcraft_chart::render(ch, &data, chart_w, chart_h, &PdfMeasure)
        });
        if let Some(prims) = prims {
            draw_prims(pg, prims, Xf { ox: x, oy: y, k: b.k });
        }
    }
    pg.restore();
}

fn draw_sparklines(pg: &mut Page, wb: &Workbook, si: usize, sh: &Sheet, b: &Block, block: RangeRef) {
    for sp in &sh.sparklines {
        if !block.contains(sp.cell) {
            continue;
        }
        let vals: Vec<Option<f64>> = match gridcraft_calc::evaluate(wb, si, sp.cell, &sp.source) {
            Value::Array(a) => a.iter().take(10_000).map(Value::as_f64).collect(),
            v => vec![v.as_f64()],
        };
        let color = rgb_of(sp.color, wb).unwrap_or([0x15, 0x60, 0x82]);
        let (x, y, w, h) = b.drawn_rect(RangeRef::cell(sp.cell));
        let (wpx, hpx) = (w / b.k, h / b.k);
        let mut prims = gridcraft_chart::render_sparkline(sp.kind, &vals, [color[0], color[1], color[2], 255], sp.markers, wpx, hpx);
        if sh.right_to_left {
            for prim in &mut prims {
                match prim {
                    gridcraft_chart::Prim::Rect { x, w, .. } => *x = wpx - *x - *w,
                    gridcraft_chart::Prim::Line { pts, .. } => pts.iter_mut().for_each(|p| p[0] = wpx - p[0]),
                    gridcraft_chart::Prim::Circle { cx, .. } => *cx = wpx - *cx,
                    _ => {}
                }
            }
        }
        draw_prims(pg, &prims, Xf { ox: x, oy: y, k: b.k });
    }
}

fn draw_shape(pg: &mut Page, wb: &Workbook, shp: &gridcraft_model::Shape, b: &Block, (x, y, w, h): (f32, f32, f32, f32)) {
    let fill = rgb_of(shp.fill, wb);
    let line = rgb_of(shp.line, wb).filter(|_| !shp.format.no_line).map(|c| (c, (0.75 * b.k).max(0.25)));
    let angle = if shp.rotation.is_finite() { shp.rotation.to_radians() } else { 0.0 };
    let (sn, cs) = angle.sin_cos();
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let turn = |[px, py]: [f32; 2]| {
        let (dx, dy) = (px - cx, py - cy);
        [cx + dx * cs - dy * sn, cy + dx * sn + dy * cs]
    };
    match shp.kind {
        ShapeKind::Ellipse => {
            let pts: Vec<[f32; 2]> = (0..72)
                .map(|i| {
                    let a = i as f32 / 72.0 * std::f32::consts::TAU;
                    turn([x + w / 2.0 + w / 2.0 * a.cos(), y + h / 2.0 + h / 2.0 * a.sin()])
                })
                .collect();
            pg.polygon(&pts, fill, line);
        }
        ShapeKind::Triangle => {
            let pts = [[x + w / 2.0, y], [x + w, y + h], [x, y + h]].map(turn);
            pg.polygon(&pts, fill, line);
        }
        ShapeKind::Line | ShapeKind::Arrow => {
            let c = line.map(|l| l.0).or(fill).unwrap_or([0, 0, 0]);
            let [x0, y0] = turn([x, y]);
            let [x1, y1] = turn([x + w, y + h]);
            pg.stroke_line(x0, y0, x1, y1, (1.0 * b.k).max(0.3), c);
            if shp.kind == ShapeKind::Arrow {
                let ang = h.atan2(w);
                let al = 8.0 * b.k;
                let p = |da: f32| turn([x + w - al * (ang + da).cos(), y + h - al * (ang + da).sin()]);
                pg.polygon(&[turn([x + w, y + h]), p(0.4), p(-0.4)], Some(c), None);
            }
        }
        _ => {
            let pts = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(turn);
            pg.polygon(&pts, fill, line);
        }
    }
    if shp.text.is_empty() {
        return;
    }
    use gridcraft_model::{ShapeTextAlign, ShapeTextAnchor};
    let fm = &shp.format;
    let text_box = shp.kind == ShapeKind::TextBox;
    let size = fm.font_size.unwrap_or(11.0).clamp(1.0, 4000.0) * b.scale;
    let font = Font::styled(Font::family_for(fm.font.as_deref().unwrap_or("")), fm.bold, fm.italic);
    let color = fm.text_color.and_then(|c| rgb_of(c, wb)).unwrap_or([0, 0, 0]);
    let align = fm.align.unwrap_or(if text_box { ShapeTextAlign::Left } else { ShapeTextAlign::Center });
    let anchor = fm.anchor.unwrap_or(if text_box { ShapeTextAnchor::Top } else { ShapeTextAnchor::Middle });
    let lines = wrap_lines(&shp.text, font, size, (w - 4.0 * b.k).max(size), TextDirection::Auto);
    let block_h = lines.len().min(500) as f32 * size * 1.2;
    let mut yy = match anchor {
        ShapeTextAnchor::Top => y + 2.0 * b.k,
        ShapeTextAnchor::Middle => y + (h - block_h) / 2.0,
        ShapeTextAnchor::Bottom => y + h - 2.0 * b.k - block_h,
    } + size * 0.8;
    for l in lines.iter().take(500) {
        let lw = gridcraft_pdf::text_width_with_direction(font, size, &l.text, l.direction);
        let lx = match align {
            ShapeTextAlign::Left => x + 2.0 * b.k,
            ShapeTextAlign::Center => x + (w - lw) / 2.0,
            ShapeTextAlign::Right => x + w - 2.0 * b.k - lw,
        };
        // WordArt's drop shadow: the line again, offset, in the shadow colour (under the text).
        if let Some(sc) = fm.text_shadow.and_then(|c| rgb_of(c, wb)) {
            let off = (size * 0.06).max(0.5);
            let [sx, sy] = turn([lx + off, yy + off]);
            pg.text_rotated_with_direction(Baseline { origin: Point::new(sx, sy), angle }, size, font, sc, &l.text, l.direction);
        }
        // WordArt's text outline, as on screen: the line in the outline colour, offset in eight
        // directions by half the outline width, under the filled text.
        if let Some((c, ow)) = fm.text_outline
            && let Some(oc) = rgb_of(c, wb)
        {
            let d = (ow * b.scale / 2.0).clamp(0.25, 20.0);
            for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
                let [ox, oy] = turn([lx + dx * d, yy + dy * d]);
                pg.text_rotated_with_direction(Baseline { origin: Point::new(ox, oy), angle }, size, font, oc, &l.text, l.direction);
            }
        }
        let [tx, ty] = turn([lx, yy]);
        pg.text_rotated_with_direction(Baseline { origin: Point::new(tx, ty), angle }, size, font, color, &l.text, l.direction);
        yy += size * 1.2;
    }
}
