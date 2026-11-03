use super::draw::{Block, PdfMeasure, Xf, draw_prims, rgb_of};
use super::text::wrap_lines;
use super::*;

/// Cell pictures fit the current cell (or merged-cell) rectangle without changing aspect ratio.
pub(super) fn draw_cell_picture(pg: &mut Page, data: &[u8], rect: (f32, f32, f32, f32), k: f32) {
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
    let Ok(mut reader) = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format() else { return };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let Ok(image) = reader.decode() else { return };
    let image =
        image.thumbnail(((w / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32, ((h / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32).to_rgba8();
    let fit = (w / iw as f32).min(h / ih as f32);
    let (pw, ph) = (iw as f32 * fit, ih as f32 * fit);
    pg.image_rgba(x + pad + (w - pw) / 2.0, y + pad + (h - ph) / 2.0, pw, ph, image.as_raw(), image.width(), image.height());
}

/// Sheet pixel rectangle of an anchor.
fn anchor_rect(sh: &Sheet, a: &Anchor) -> (f64, f64, f64, f64) {
    (sh.col_left(a.cell.col) + a.dx as f64, sh.row_top(a.cell.row) + a.dy as f64, a.width.max(0.0) as f64, a.height.max(0.0) as f64)
}

/// Decodes an embedded picture to RGBA, downscaled to at most `max_w`×`max_h` pixels.
fn decode_image(data: &[u8], max_w: u32, max_h: u32) -> Option<(Vec<u8>, u32, u32)> {
    let (w, h) = crate::io::image_size(data)?;
    if w == 0 || h == 0 || w as u64 * h as u64 > 60_000_000 {
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
pub(super) fn draw_objects(
    pg: &mut Page,
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    b: &Block,
    pics: &mut std::collections::HashMap<(usize, usize), Option<(Vec<u8>, u32, u32)>>,
) {
    let block = b.range();
    let (sx0, sy0) = (sh.col_left(block.start.col), sh.row_top(block.start.row));
    let (sx1, sy1) = (sh.col_left(block.end.col.saturating_add(1)), sh.row_top(block.end.row.saturating_add(1)));
    let (bx, by) = (b.xs.first().copied().unwrap_or(0.0), b.ys.first().copied().unwrap_or(0.0));
    let (bw, bh) = (b.xs.last().copied().unwrap_or(bx) - bx, b.ys.last().copied().unwrap_or(by) - by);
    let visible = |r: (f64, f64, f64, f64)| r.2 > 0.0 && r.3 > 0.0 && r.0 < sx1 && r.0 + r.2 > sx0 && r.1 < sy1 && r.1 + r.3 > sy0;
    let place = |r: (f64, f64, f64, f64)| (bx + ((r.0 - sx0) as f32) * b.k, by + ((r.1 - sy0) as f32) * b.k, r.2 as f32 * b.k, r.3 as f32 * b.k);
    pg.save();
    pg.clip_rect(bx, by, bw, bh);
    draw_sparklines(pg, wb, si, sh, b, block);
    for (imi, im) in sh.images.iter().enumerate() {
        let r = anchor_rect(sh, &im.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, w, h) = place(r);
        // About 150 dpi is plenty for print.
        let (mw, mh) = (((w / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32, ((h / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32);
        let decoded = pics.entry((si, imi)).or_insert_with(|| decode_image(&im.data, mw, mh));
        if let Some((px, pw, ph)) = decoded.as_ref() {
            pg.image_rgba(x, y, w, h, px, *pw, *ph);
        }
    }
    for shp in &sh.shapes {
        let r = anchor_rect(sh, &shp.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, w, h) = place(r);
        draw_shape(pg, wb, shp, b, (x, y, w, h));
    }
    for ch in &sh.charts {
        let r = anchor_rect(sh, &ch.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, _, _) = place(r);
        let data = gridcraft_chart::resolve(wb, si, ch);
        let prims = gridcraft_chart::render(ch, &data, r.2 as f32, r.3 as f32, &PdfMeasure);
        draw_prims(pg, &prims, Xf { ox: x, oy: y, k: b.k });
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
        let (x, y, w, h) = b.rect(RangeRef::cell(sp.cell));
        let (wpx, hpx) = (w / b.k, h / b.k);
        let prims = gridcraft_chart::render_sparkline(sp.kind, &vals, [color[0], color[1], color[2], 255], sp.markers, wpx, hpx);
        draw_prims(pg, &prims, Xf { ox: x, oy: y, k: b.k });
    }
}

fn draw_shape(pg: &mut Page, wb: &Workbook, shp: &gridcraft_model::Shape, b: &Block, (x, y, w, h): (f32, f32, f32, f32)) {
    let fill = rgb_of(shp.fill, wb);
    let line = rgb_of(shp.line, wb).filter(|_| !shp.format.no_line).map(|c| (c, (0.75 * b.k).max(0.25)));
    match shp.kind {
        ShapeKind::Ellipse => {
            let pts: Vec<[f32; 2]> = (0..72)
                .map(|i| {
                    let a = i as f32 / 72.0 * std::f32::consts::TAU;
                    [x + w / 2.0 + w / 2.0 * a.cos(), y + h / 2.0 + h / 2.0 * a.sin()]
                })
                .collect();
            pg.polygon(&pts, fill, line);
        }
        ShapeKind::Triangle => pg.polygon(&[[x + w / 2.0, y], [x + w, y + h], [x, y + h]], fill, line),
        ShapeKind::Line | ShapeKind::Arrow => {
            let c = line.map(|l| l.0).or(fill).unwrap_or([0, 0, 0]);
            pg.stroke_line(x, y, x + w, y + h, (1.0 * b.k).max(0.3), c);
            if shp.kind == ShapeKind::Arrow {
                let ang = h.atan2(w);
                let al = 8.0 * b.k;
                let p = |da: f32| [x + w - al * (ang + da).cos(), y + h - al * (ang + da).sin()];
                pg.polygon(&[[x + w, y + h], p(0.4), p(-0.4)], Some(c), None);
            }
        }
        _ => {
            pg.polygon(&[[x, y], [x + w, y], [x + w, y + h], [x, y + h]], fill, line);
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
    let lines = wrap_lines(&shp.text, font, size, (w - 4.0 * b.k).max(size));
    let block_h = lines.len().min(500) as f32 * size * 1.2;
    let mut yy = match anchor {
        ShapeTextAnchor::Top => y + 2.0 * b.k,
        ShapeTextAnchor::Middle => y + (h - block_h) / 2.0,
        ShapeTextAnchor::Bottom => y + h - 2.0 * b.k - block_h,
    } + size * 0.8;
    for l in lines.iter().take(500) {
        let lw = gridcraft_pdf::text_width(font, size, l);
        let lx = match align {
            ShapeTextAlign::Left => x + 2.0 * b.k,
            ShapeTextAlign::Center => x + (w - lw) / 2.0,
            ShapeTextAlign::Right => x + w - 2.0 * b.k - lw,
        };
        pg.text(lx, yy, size, font, color, l);
        yy += size * 1.2;
    }
}
