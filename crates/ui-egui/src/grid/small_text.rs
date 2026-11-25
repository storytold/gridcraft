//! Small cell text drawn in black and white on the pixel grid, the way Excel keeps it readable.
//!
//! Measured in Excel on synthetic sheets (Windows, 125% display scale): up to about 11 pixels its cell
//! text is pure black and white, hinted to whole pixels, with no antialiasing. Antialiased text at
//! those sizes blurs together next to it. So one line of cell text up to [`UP_TO`] pixels is drawn
//! here: outlines hinted by the font's own instructions for black and white, scan converted as
//! TrueType defines it (pixel centres, with dropout control so a thin stroke doesn't vanish), one
//! pixel larger than the cell's size. A face drawn for small sizes ([`SMALL_FACE_FILES`]) stands in for the
//! cell's font when the system has it.
//!
//! Glyphs are cached in a texture atlas in the text's colour, so a plain mesh draws them.
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use egui::epaint::{Mesh, Vertex};
use egui::mutex::Mutex;
use egui::{Color32, ColorImage, Context, FontData, FontFamily, FontId, Id, Painter, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};
use gridcraft_engine::model::VAlign;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, HintingInstance, OutlinePen, Target};
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// Up to this size in physical pixels Excel's cell text is pure black and white.
pub(super) const UP_TO: u32 = 11;
/// Up to this size a bold cell font's strokes are widened by a pixel: hinting leaves them as thin as
/// the regular face's.
const BOLD_WIDEN_UP_TO: u32 = 9;
/// A face drawn for small sizes (Tahoma), used instead of the cell's own when the system has it: the
/// files of its regular and bold styles.
const SMALL_FACE_FILES: (&str, &str) = ("tahoma.ttf", "tahomabd.ttf");
/// Side of the square glyph atlas, in pixels.
const ATLAS: usize = 2048;

/// One glyph's pixels and where they sit against the pen and the baseline, in pixels.
struct Glyph {
    left: i32,
    top: i32,
    w: usize,
    h: usize,
    on: Vec<bool>,
    advance: f32,
}

/// A glyph in the atlas: face, size, glyph and colour.
type Placed = (String, u32, u32, [u8; 4]);

#[derive(Default)]
struct Cache {
    glyphs: HashMap<(String, u32, u32), Option<Arc<Glyph>>>,
    placed: HashMap<Placed, [usize; 2]>,
    texture: Option<TextureHandle>,
    cursor: [usize; 2],
    shelf: usize,
    /// The frame the atlas ran out in. It starts again in a later one, since this one's meshes still
    /// point into it.
    full_at: Option<u64>,
}

type Shared = Arc<Mutex<Cache>>;

fn cache(ctx: &Context) -> Shared {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Shared>(Id::unique("gridcraft.small_text")).clone())
}

/// What to draw a line of text with: its face, the cell's own face whose metrics place it, and the
/// cache key.
struct Face {
    key: String,
    data: Arc<FontData>,
    own: Arc<FontData>,
    /// A bold cell font whose small strokes get a pixel wider. A face made for small sizes has its own.
    widen_bold: bool,
}

fn font_ref(data: &FontData) -> Option<FontRef<'_>> {
    FontRef::from_index(data.font.as_ref().as_ref(), data.index).ok()
}

/// The face for small sizes, regular and bold, read from the system's font folders the first time
/// small text is drawn. Its files are looked for by name rather than through the catalog of
/// installed fonts, whose background scan the desktop app doesn't start.
fn small_faces() -> &'static [Option<Arc<FontData>>; 2] {
    static FACES: OnceLock<[Option<Arc<FontData>>; 2]> = OnceLock::new();
    FACES.get_or_init(|| {
        let dirs = crate::fonts::system_font_dirs();
        let load = |file: &str| {
            let path = dirs.iter().find_map(|dir| {
                std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(file)))
            })?;
            Some(Arc::new(FontData::from_owned(std::fs::read(path).ok()?)))
        };
        [load(SMALL_FACE_FILES.0), load(SMALL_FACE_FILES.1)]
    })
}

/// Whether `family` is one of the italic cell roles: `cell-italic`, `cell-bold-italic`, or an
/// installed family's `…:ni` and `…:bi` (see `fonts::role_key`). Its face may still be upright, when
/// the family has no italic.
fn asks_italic(family: &FontFamily) -> bool {
    let FontFamily::Name(name) = family else { return false };
    name.ends_with("-italic") || (name.starts_with("fnt:") && name.rsplit(':').next().is_some_and(|style| style.ends_with('i')))
}

fn face(ctx: &Context, family: &FontFamily) -> Option<Face> {
    let (key, own) = ctx.fonts_mut(|f| {
        let defs = f.definitions();
        let key = defs.families.get(family)?.first()?.clone();
        let data = defs.font_data.get(&key)?.clone();
        Some((key, data))
    })?;
    let attributes = font_ref(&own).map(|f| f.attributes());
    let bold = attributes.is_some_and(|a| a.weight.value() >= 600.0);
    // The small face has no italic: italic text keeps the cell's font, slanted as it is.
    let italic = asks_italic(family) || attributes.is_some_and(|a| a.style != skrifa::attribute::Style::Normal);
    if !italic && let Some(small) = small_faces().get(usize::from(bold)).cloned().flatten() {
        return Some(Face { key: format!("small:{}", u8::from(bold)), data: small, own, widen_bold: false });
    }
    Some(Face { key, data: own.clone(), own, widen_bold: bold })
}

type Segment = ([f32; 2], [f32; 2]);

/// Collects an outline as straight segments, in pixels with y going down from the baseline.
#[derive(Default)]
struct Polygons {
    lines: Vec<Segment>,
    at: [f32; 2],
    first: [f32; 2],
}

impl Polygons {
    fn to(&mut self, p: [f32; 2]) {
        self.lines.push((self.at, p));
        self.at = p;
    }
}

impl OutlinePen for Polygons {
    fn move_to(&mut self, x: f32, y: f32) {
        self.at = [x, -y];
        self.first = self.at;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.to([x, -y]);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let ([ax, ay], [bx, by], [ex, ey]) = (self.at, [cx, -cy], [x, -y]);
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let u = 1.0 - t;
            self.to([u * u * ax + 2.0 * u * t * bx + t * t * ex, u * u * ay + 2.0 * u * t * by + t * t * ey]);
        }
    }
    fn curve_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let ([ax, ay], [bx, by], [cx, cy], [ex, ey]) = (self.at, [c1x, -c1y], [c2x, -c2y], [x, -y]);
        for i in 1..=12 {
            let t = i as f32 / 12.0;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.to([w0 * ax + w1 * bx + w2 * cx + w3 * ex, w0 * ay + w1 * by + w2 * cy + w3 * ey]);
        }
    }
    fn close(&mut self) {
        let first = self.first;
        self.to(first);
    }
}

/// Up to this size glyphs are drawn a pixel larger: Excel's small text is as tall as the next size
/// up. At [`UP_TO`] they aren't, so the next size, drawn the usual way, isn't smaller than this one.
const LARGER_UP_TO: u32 = UP_TO - 1;

/// The pixel size glyphs of a `nominal` size are drawn at.
fn drawn_size(nominal: u32) -> u32 {
    if nominal <= LARGER_UP_TO { nominal + 1 } else { nominal }
}

fn rasterize(font: &FontRef<'_>, gid: GlyphId, nominal: u32, widen_bold: bool) -> Option<Glyph> {
    let px = drawn_size(nominal) as f32;
    let outlines = font.outline_glyphs();
    let outline = outlines.get(gid)?;
    let hinting = HintingInstance::new(&outlines, Size::new(px), LocationRef::default(), Target::Mono).ok()?;
    let mut pen = Polygons::default();
    let adjusted = outline.draw(DrawSettings::hinted(&hinting, false), &mut pen).ok()?;
    let linear = font.glyph_metrics(Size::new(px), LocationRef::default()).advance_width(gid).unwrap_or(0.0);
    let advance = adjusted.advance_width.unwrap_or(linear).round();
    let Some((left, top, w, h)) = bounds(&pen.lines) else {
        return Some(Glyph { left: 0, top: 0, w: 0, h: 0, on: Vec::new(), advance });
    };
    let mut on = bilevel(&pen.lines, left, top, w, h);
    if widen_bold && nominal <= BOLD_WIDEN_UP_TO {
        widen(&mut on, w);
    }
    Some(Glyph { left, top, w, h, on, advance })
}

/// The pixel box of an outline, with a pixel to spare on the right for a widened bold.
fn bounds(lines: &[Segment]) -> Option<(i32, i32, usize, usize)> {
    if lines.is_empty() {
        return None;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for ([ax, ay], [bx, by]) in lines {
        x0 = x0.min(*ax).min(*bx);
        x1 = x1.max(*ax).max(*bx);
        y0 = y0.min(*ay).min(*by);
        y1 = y1.max(*ay).max(*by);
    }
    let left = x0.floor() as i32;
    let top = y0.floor() as i32;
    let w = (x1.ceil() as i32 - left + 1).max(1) as usize;
    let h = (y1.ceil() as i32 - top).max(1) as usize;
    Some((left, top, w, h))
}

/// Turns on the pixel right of every pixel that is on, row by row.
fn widen(mask: &mut [bool], w: usize) {
    for row in mask.chunks_mut(w.max(1)) {
        for x in (1..row.len()).rev() {
            if row.get(x - 1).copied().unwrap_or(false)
                && let Some(m) = row.get_mut(x)
            {
                *m = true;
            }
        }
    }
}

/// Where the outline crosses one scan line, with each crossing's direction, sorted. `rows` scans along
/// x at height `at`; otherwise along y at `at`.
fn crossings(lines: &[Segment], at: f32, rows: bool) -> Vec<(f32, i32)> {
    let mut out: Vec<(f32, i32)> = lines
        .iter()
        .filter_map(|([ax, ay], [bx, by])| {
            // `s` is the scanned coordinate, `u` the one returned.
            let (sa, sb, ua, ub) = if rows { (*ay, *by, *ax, *bx) } else { (*ax, *bx, *ay, *by) };
            let (lo, hi) = if sa < sb { (sa, sb) } else { (sb, sa) };
            if sa == sb || at < lo || at >= hi {
                return None;
            }
            let t = (at - sa) / (sb - sa);
            Some((ua + t * (ub - ua), if sb > sa { 1 } else { -1 }))
        })
        .collect();
    out.sort_by(|p, q| p.0.total_cmp(&q.0));
    out
}

/// The spans inside the outline along one scan line, by the non-zero winding rule.
fn spans(cross: &[(f32, i32)]) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    let (mut winding, mut start) = (0, 0.0);
    for &(x, dir) in cross {
        let was = winding;
        winding += dir;
        if was == 0 && winding != 0 {
            start = x;
        } else if was != 0 && winding == 0 {
            out.push((start, x));
        }
    }
    out
}

/// Black-and-white scan conversion as TrueType defines it: a pixel is on when its centre is inside the
/// outline, and a stroke thinner than a pixel that misses every centre still turns on the pixel it
/// passes through (dropout control), across rows and across columns.
fn bilevel(lines: &[Segment], left: i32, top: i32, w: usize, h: usize) -> Vec<bool> {
    let mut mask = vec![false; w * h];
    let mut set = |x: i64, y: i64| {
        if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y))
            && x < w
            && let Some(m) = mask.get_mut(y * w + x)
        {
            *m = true;
        }
    };
    let (left, top) = (left as f32, top as f32);
    for y in 0..h {
        for (a, b) in spans(&crossings(lines, top + y as f32 + 0.5, true)) {
            let (first, last) = ((a - left - 0.5).ceil() as i64, (b - left - 0.5).floor() as i64);
            if first > last {
                set(((a + b) / 2.0 - left).floor() as i64, y as i64);
            }
            for x in first..=last {
                set(x, y as i64);
            }
        }
    }
    for x in 0..w {
        for (a, b) in spans(&crossings(lines, left + x as f32 + 0.5, false)) {
            if (a - top - 0.5).ceil() > (b - top - 0.5).floor() {
                set(x as i64, ((a + b) / 2.0 - top).floor() as i64);
            }
        }
    }
    mask
}

fn glyph(c: &mut Cache, face: &Face, font: &FontRef<'_>, ch: char, px: u32) -> Option<(u32, Arc<Glyph>)> {
    let gid = font.charmap().map(ch)?;
    if gid.to_u32() == 0 {
        return None;
    }
    let g =
        c.glyphs.entry((face.key.clone(), px, gid.to_u32())).or_insert_with(|| rasterize(font, gid, px, face.widen_bold).map(Arc::new)).clone()?;
    Some((gid.to_u32(), g))
}

/// The font's size in physical pixels, when it is small enough to be drawn here.
fn small_size(ctx: &Context, font_id: &FontId) -> Option<u32> {
    let px = (font_id.size * ctx.pixels_per_point()).round();
    (px >= 1.0 && px <= UP_TO as f32).then_some(px as u32)
}

/// Where the baseline goes, in pixels. The glyphs drawn are larger than the cell font's, and maybe of
/// another face, so their line is placed as the cell aligns it: for bottom alignment descenders end
/// where the cell font's do, above the gridline; for top, capitals start where its capitals do; for
/// centre, capitals and digits are centred in the cell, as Excel centres them whatever the font.
fn baseline(face: &Face, px: u32, place: &Place, ppp: f32) -> i32 {
    // Ascent, descent and capital height, in pixels.
    let metrics = |data: &FontData, size: u32| {
        font_ref(data).map_or((0.0, 0.0, 0.0), |f| {
            let m = f.metrics(Size::new(size as f32), LocationRef::default());
            (m.ascent, -m.descent, m.cap_height.unwrap_or(m.ascent * 0.72))
        })
    };
    let (own_ascent, own_descent, own_cap) = metrics(&face.own, px);
    let (_, descent, cap) = metrics(&face.data, drawn_size(px));
    let own = (place.top * ppp).round() + own_ascent.round();
    let at = match place.valign {
        // A pixel up: the cell font's descenders end on the row's last pixel, where its gridline is.
        VAlign::Bottom => own + own_descent.round() - descent.round() - 1.0,
        // Capitals start where the cell font's would.
        VAlign::Top => own - own_cap + cap,
        // Capitals and digits centred in the cell, between its top and its gridline (the row's last
        // pixel), descenders below, as Excel centres them whatever the font. The faces' ascents
        // differ, so they can't be what is centred.
        VAlign::Center | VAlign::Justify | VAlign::Distributed => {
            let (top, bottom) = ((place.cell.0 * ppp).round(), (place.cell.1 * ppp).round() - 1.0);
            (top + bottom) / 2.0 + cap / 2.0
        }
    };
    at.round() as i32
}

/// The width in points of `text` as [`paint`] draws it, or `None` when it would leave the text to the
/// usual drawing: a size over [`UP_TO`] pixels, or a character the face lacks.
pub(super) fn width(ctx: &Context, font_id: &FontId, text: &str) -> Option<f32> {
    let px = small_size(ctx, font_id)?;
    let face = face(ctx, &font_id.family)?;
    let font = font_ref(&face.data)?;
    let shared = cache(ctx);
    let mut c = shared.lock();
    let mut pen = 0.0;
    for ch in text.chars() {
        pen += glyph(&mut c, &face, &font, ch, px)?.1.advance;
    }
    Some(pen / ctx.pixels_per_point())
}

/// Draws one line of `text` starting at `x`, placed vertically by `line`. Returns false, having drawn
/// nothing, when it leaves the text to the usual drawing.
/// Where a line goes vertically: the cell's alignment, the top of the cell font's line box, and the
/// cell's top and bottom edges, in points.
pub(super) struct Place {
    pub valign: VAlign,
    pub top: f32,
    pub cell: (f32, f32),
}

pub(super) fn paint(p: &Painter, font_id: &FontId, text: &str, x: f32, line: &Place, color: Color32) -> bool {
    let ctx = p.ctx();
    let ppp = ctx.pixels_per_point();
    let Some(px) = small_size(ctx, font_id) else { return false };
    let Some(face) = face(ctx, &font_id.family) else { return false };
    let Some(font) = font_ref(&face.data) else { return false };
    // The baseline stays where the cell's own font puts it, also when a face for small sizes stands in.
    let baseline = baseline(&face, px, line, ppp);
    let pass = ctx.cumulative_pass_nr();
    let shared = cache(ctx);
    let mut c = shared.lock();
    if c.full_at.is_some_and(|full| full != pass) {
        c.placed.clear();
        c.cursor = [0, 0];
        c.shelf = 0;
        c.full_at = None;
    }
    let mut glyphs = Vec::new();
    for ch in text.chars() {
        let Some(g) = glyph(&mut c, &face, &font, ch, px) else { return false };
        glyphs.push(g);
    }
    let texture = c
        .texture
        .get_or_insert_with(|| {
            ctx.load_texture("gridcraft.small_text", ColorImage::filled([ATLAS, ATLAS], Color32::TRANSPARENT), TextureOptions::NEAREST)
        })
        .id();
    let mut mesh = Mesh::with_texture(texture);
    let mut pen = (x * ppp).round();
    for (gid, g) in glyphs {
        if g.w > 0 {
            // The atlas is full for this frame: nothing is drawn yet, so the usual drawing takes over.
            let Some(spot) = place(&mut c, (face.key.clone(), px, gid, color.to_array()), &g, pass) else { return false };
            let min = pos2((pen as i32 + g.left) as f32 / ppp, (baseline + g.top) as f32 / ppp);
            let rect = Rect::from_min_size(min, vec2(g.w as f32 / ppp, g.h as f32 / ppp));
            let uv = Rect::from_min_size(
                pos2(spot[0] as f32 / ATLAS as f32, spot[1] as f32 / ATLAS as f32),
                vec2(g.w as f32 / ATLAS as f32, g.h as f32 / ATLAS as f32),
            );
            add_quad(&mut mesh, rect, uv);
        }
        pen += g.advance;
    }
    p.add(mesh);
    true
}

fn add_quad(mesh: &mut Mesh, rect: Rect, uv: Rect) {
    let i = mesh.vertices.len() as u32;
    let v = |pos: Pos2, uv: Pos2| Vertex { pos, uv, color: Color32::WHITE };
    mesh.vertices.extend([
        v(rect.left_top(), uv.left_top()),
        v(rect.right_top(), uv.right_top()),
        v(rect.right_bottom(), uv.right_bottom()),
        v(rect.left_bottom(), uv.left_bottom()),
    ]);
    mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
}

/// Where a glyph in this colour sits in the atlas, adding it when it isn't there yet. `None` when the
/// atlas is full for this frame.
fn place(c: &mut Cache, key: Placed, g: &Glyph, pass: u64) -> Option<[usize; 2]> {
    if let Some(spot) = c.placed.get(&key) {
        return Some(*spot);
    }
    if g.w + 1 > ATLAS || g.h + 1 > ATLAS {
        return None;
    }
    if c.cursor[0] + g.w + 1 > ATLAS {
        c.cursor = [0, c.cursor[1] + c.shelf + 1];
        c.shelf = 0;
    }
    if c.cursor[1] + g.h + 1 > ATLAS {
        c.full_at = Some(pass);
        return None;
    }
    let spot = c.cursor;
    c.cursor[0] += g.w + 1;
    c.shelf = c.shelf.max(g.h);
    let [r, gr, b, a] = key.3;
    let ink = Color32::from_rgba_premultiplied(r, gr, b, a);
    let pixels: Vec<Color32> = g.on.iter().map(|on| if *on { ink } else { Color32::TRANSPARENT }).collect();
    if let Some(t) = c.texture.as_mut() {
        t.set_partial(spot, ColorImage::new([g.w, g.h], pixels), TextureOptions::NEAREST);
    }
    c.placed.insert(key, spot);
    Some(spot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Segment> {
        vec![([x0, y0], [x1, y0]), ([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])]
    }

    fn on(mask: &[bool], w: usize) -> Vec<String> {
        mask.chunks(w).map(|r| r.iter().map(|b| if *b { '#' } else { '.' }).collect()).collect()
    }

    /// A pixel is on when its centre is inside.
    #[test]
    fn pixels_whose_centres_are_inside_are_on() {
        let mask = bilevel(&rect(1.2, 1.2, 4.8, 3.8), 0, 0, 6, 5);
        assert_eq!(on(&mask, 6), ["......", ".####.", ".####.", ".####.", "......"]);
    }

    /// A stroke thinner than a pixel keeps a pixel in every row and column it passes through.
    #[test]
    fn a_thin_stroke_does_not_vanish() {
        let vertical = bilevel(&rect(2.1, 0.0, 2.4, 4.0), 0, 0, 5, 4);
        assert_eq!(on(&vertical, 5), ["..#..", "..#..", "..#..", "..#.."]);
        let horizontal = bilevel(&rect(0.0, 1.1, 4.0, 1.4), 0, 0, 4, 3);
        assert_eq!(on(&horizontal, 4), ["....", "####", "...."]);
    }

    /// A hole wound the other way stays empty.
    #[test]
    fn winding_decides_what_is_inside() {
        let mut ring = rect(0.0, 0.0, 6.0, 6.0);
        ring.extend(rect(2.0, 2.0, 4.0, 4.0).into_iter().map(|(a, b)| (b, a)));
        let mask = bilevel(&ring, 0, 0, 6, 6);
        assert_eq!(on(&mask, 6), ["######", "######", "##..##", "##..##", "######", "######"]);
    }

    #[test]
    fn italic_roles_are_recognised() {
        let named = |n: &str| FontFamily::Name(n.into());
        for italic in ["cell-italic", "cell-bold-italic", "fnt:Arial:ni", "fnt:Arial:bi"] {
            assert!(asks_italic(&named(italic)), "{italic}");
        }
        for upright in ["cell", "cell-bold", "fnt:Arial:nn", "fnt:Arial:bn", "fnt:Times New Roman:bn", "ui"] {
            assert!(!asks_italic(&named(upright)), "{upright}");
        }
        assert!(!asks_italic(&FontFamily::Proportional));
    }

    #[test]
    fn widening_adds_a_pixel_to_the_right() {
        let mut mask = vec![false, true, false, false, true, false];
        widen(&mut mask, 3);
        assert_eq!(mask, [false, true, true, false, true, true]);
    }

    /// Hinted glyphs of a real font come out in whole pixels, one pixel larger than asked.
    #[test]
    fn a_real_glyph_is_whole_pixels() {
        let bytes = epaint_default_fonts::UBUNTU_LIGHT;
        let font = FontRef::new(bytes).expect("the test font parses");
        let gid = font.charmap().map('H').expect("the test font has H");
        let g = rasterize(&font, gid, 8, false).expect("H draws");
        assert!(g.on.iter().any(|b| *b) && g.w > 2, "H has pixels: {}x{}", g.w, g.h);
        assert!(g.advance.fract() == 0.0 && g.advance > 0.0, "a whole-pixel advance: {}", g.advance);
        let rows = on(&g.on, g.w);
        assert!(rows.iter().all(|r| r.starts_with('#') || r.chars().all(|c| c == '.')), "the stem fills its column: {rows:?}");
    }
}
