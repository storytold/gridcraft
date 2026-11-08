//! Draw tab: pen and eraser tools, ink strokes, Ink to Shape.

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, Color, InkStyle, InkTexture, InkTool, MAX_INK_TEXTURES, Shape, ShapeKind};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cmd, edit, f64_param, has_doc, ok, str_param, u32_param};

/// The size in sheet points one repeat of a pen texture covers when the pen doesn't say.
pub const DEFAULT_TEXTURE_SCALE: f32 = 8.0;
/// The smallest and largest repeat size a pen texture may use, in sheet points.
pub const MIN_TEXTURE_SCALE: f32 = 0.5;
pub const MAX_TEXTURE_SCALE: f32 = 512.0;

/// A repeat size from a parameter or the pen's state, kept finite and within
/// [`MIN_TEXTURE_SCALE`]–[`MAX_TEXTURE_SCALE`].
pub fn texture_scale(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v.clamp(MIN_TEXTURE_SCALE, MAX_TEXTURE_SCALE) } else { DEFAULT_TEXTURE_SCALE }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "draw.pen", "Pen", ["Draw", "Tools"], None, "{on?: bool, color?: hex, width?: 2, texture?: id | \"\" (none), textureScale?: 8}", has_doc, pen),
        cmd!(noundo "draw.eraser", "Eraser", ["Draw", "Tools"], None, "{on?: bool}", has_doc, eraser),
        cmd!(noundo "draw.lasso", "Lasso Select", ["Draw", "Tools"], None, "{} (selects ink by dragging; same as Select Objects)", has_doc, |s, _| {
            s.draw_tool = String::new();
            ok()
        }),
        cmd!(
            "draw.stroke",
            "Ink Stroke",
            [],
            None,
            "{points: [[x,y],…] in sheet points, color?: hex, width?: 2, texture?: id, textureScale?: 8}",
            has_doc,
            stroke
        ),
        cmd!(
            "draw.textureImport",
            "Import Brush Texture",
            ["Draw", "Tools"],
            None,
            "{path} | {name, base64} → {id, name, width, height, added}; a PNG or JPEG up to 16 MiB and 8192 px a side. Importing the same bytes again reuses the entry.",
            has_doc,
            texture_import
        ),
        cmd!("draw.textureRemove", "Remove Brush Texture", ["Draw", "Tools"], None, "{id} → {removed}", has_doc, texture_remove),
        cmd!(query "draw.textureList", "Brush Textures", [], None, "{} → {textures: [{id, name, mime, width, height, bytes}], selected, scale}", has_doc, texture_list),
        cmd!(
            "draw.inkToShape",
            "Ink to Shape",
            ["Draw", "Convert"],
            None,
            "{id?} converts an ink stroke (default: the last) into a rectangle, ellipse, triangle or line",
            has_doc,
            ink_to_shape
        ),
    ]
}

fn pen(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.draw_tool != "pen");
    s.draw_tool = if on { "pen".into() } else { String::new() };
    if let Some(c) = str_param(p, "color") {
        s.draw_color = c.to_string();
    }
    if let Some(w) = f64_param(p, "width") {
        s.draw_width = w.clamp(0.5, 40.0) as f32;
    }
    if let Some(id) = p.get("texture").and_then(Json::as_str) {
        if !id.is_empty() && s.doc()?.wb.ink_texture(id).is_none() {
            return Err(bad("draw.pen", "no imported texture has that id"));
        }
        s.draw_texture = id.to_string();
    }
    if let Some(v) = f64_param(p, "textureScale") {
        s.draw_texture_scale = texture_scale(v as f32);
    }
    Ok(json!({
        "tool": s.draw_tool,
        "color": s.draw_color,
        "width": s.draw_width,
        "texture": s.draw_texture,
        "textureScale": texture_scale(s.draw_texture_scale),
    }))
}

fn eraser(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.draw_tool != "eraser");
    s.draw_tool = if on { "eraser".into() } else { String::new() };
    Ok(json!({"tool": s.draw_tool}))
}

/// Validate an imported texture before mutating the workbook: header dimensions alone do not
/// prove the pixels decode, and a hostile image must not get to the store.
fn validate_texture(data: &[u8]) -> Result<(&'static str, u32, u32)> {
    use gridcraft_model::{MAX_INK_TEXTURE_BYTES, MAX_INK_TEXTURE_PIXELS, MAX_INK_TEXTURE_SIDE};
    if data.is_empty() {
        return Err(bad("draw.textureImport", "the image is empty"));
    }
    if data.len() > MAX_INK_TEXTURE_BYTES {
        return Err(bad("draw.textureImport", "brush textures must be at most 16 MiB"));
    }
    let format = image::guess_format(data).map_err(|_| bad("draw.textureImport", "choose a valid PNG or JPEG image"))?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(bad("draw.textureImport", "brush textures support PNG and JPEG")),
    };
    let (width, height) = image::ImageReader::with_format(std::io::Cursor::new(data), format)
        .into_dimensions()
        .map_err(|_| bad("draw.textureImport", "invalid image dimensions"))?;
    if width == 0
        || height == 0
        || width > MAX_INK_TEXTURE_SIDE
        || height > MAX_INK_TEXTURE_SIDE
        || u64::from(width) * u64::from(height) > MAX_INK_TEXTURE_PIXELS
    {
        return Err(bad("draw.textureImport", "brush textures are limited to 8192 pixels a side and 16 million pixels"));
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(data), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_INK_TEXTURE_SIDE);
    limits.max_image_height = Some(MAX_INK_TEXTURE_SIDE);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| bad("draw.textureImport", "the image cannot be decoded within the 64 MiB limit"))?;
    Ok((mime, width, height))
}

fn texture_import(s: &mut Session, p: &Json) -> Result<Json> {
    let (name, data) = if let Some(b) = str_param(p, "base64") {
        // Check the encoded length first, so a hostile parameter is refused before decoding.
        let longest = gridcraft_model::MAX_INK_TEXTURE_BYTES.div_ceil(3) * 4 + 4;
        if b.len() > longest {
            return Err(bad("draw.textureImport", "brush textures must be at most 16 MiB"));
        }
        let data = crate::io::base64_decode(b).ok_or_else(|| bad("draw.textureImport", "invalid base64"))?;
        (str_param(p, "name").unwrap_or("Texture").to_string(), data)
    } else if let Some(path) = str_param(p, "path") {
        let data = crate::io::read_file(path)?;
        let name = str_param(p, "name")
            .map(str::to_string)
            .unwrap_or_else(|| path.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or("Texture").to_string());
        (name, data)
    } else {
        return Err(bad("draw.textureImport", "give `path`, or `name` and `base64`"));
    };
    let (mime, width, height) = validate_texture(&data)?;
    let texture = InkTexture::new(&name, mime, std::sync::Arc::new(data), width, height)
        .ok_or_else(|| bad("draw.textureImport", "the image cannot be stored"))?;
    let id = texture.id.clone();
    let name = texture.name.clone();
    let d = s.doc()?;
    if d.wb.ink_textures.len() >= MAX_INK_TEXTURES && d.wb.ink_texture(&id).is_none() {
        return Err(bad("draw.textureImport", "the workbook already holds the most brush textures it can (1024)"));
    }
    edit(s, |cx| {
        let (index, added) = cx.wb.add_ink_texture(texture);
        Ok(json!({"id": id, "name": name, "width": width, "height": height, "index": index, "added": added}))
    })
}

fn texture_remove(s: &mut Session, p: &Json) -> Result<Json> {
    let id = str_param(p, "id").ok_or_else(|| bad("draw.textureRemove", "missing `id`"))?.to_string();
    if s.doc()?.wb.ink_texture(&id).is_none() {
        return Ok(json!({"removed": false}));
    }
    if s.draw_texture == id {
        s.draw_texture.clear();
    }
    edit(s, |cx| {
        let removed = cx.wb.remove_ink_texture(&id).is_some();
        Ok(json!({"removed": removed}))
    })
}

fn texture_list(s: &mut Session, _p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let textures: Vec<Json> =
        d.wb.ink_textures
            .iter()
            .map(|t| json!({"id": t.id, "name": t.name, "mime": t.mime, "width": t.width, "height": t.height, "bytes": t.data.len()}))
            .collect();
    Ok(json!({
        "textures": textures,
        "selected": s.draw_texture,
        "scale": texture_scale(s.draw_texture_scale),
    }))
}

fn parse_points(t: &str) -> Vec<[f32; 2]> {
    t.split(';').filter_map(|pt| pt.split_once(',').and_then(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?]))).collect()
}

fn stroke(s: &mut Session, p: &Json) -> Result<Json> {
    let pts: Vec<[f64; 2]> = p
        .get("points")
        .and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|q| Some([q.get(0)?.as_f64()?, q.get(1)?.as_f64()?]))
                .filter(|q| q[0].is_finite() && q[1].is_finite())
                .take(20_000)
                .collect()
        })
        .unwrap_or_default();
    if pts.len() < 2 {
        return Err(bad("draw.stroke", "a stroke needs at least two points"));
    }
    let minx = pts.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min).max(0.0);
    let miny = pts.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min).max(0.0);
    let maxx = pts.iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max);
    let maxy = pts.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max);
    let color =
        str_param(p, "color").map(str::to_string).unwrap_or_else(|| if s.draw_color.is_empty() { "#1F5FC9".into() } else { s.draw_color.clone() });
    let width = f64_param(p, "width").map(|w| w as f32).unwrap_or(if s.draw_width <= 0.0 { 2.0 } else { s.draw_width }).clamp(0.5, 40.0);
    let texture = p
        .get("texture")
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| (!s.draw_texture.is_empty()).then(|| s.draw_texture.clone()));
    let texture_scale = f64_param(p, "textureScale").map(|v| texture_scale(v as f32)).unwrap_or_else(|| texture_scale(s.draw_texture_scale));
    let d = s.doc()?;
    if let Some(id) = &texture
        && d.wb.ink_texture(id).is_none()
    {
        return Err(bad("draw.stroke", "no imported texture has that id"));
    }
    let si = d.wb.active_sheet();
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let cell = CellRef::new(sh.row_at(miny), sh.col_at(minx));
    let dx = (minx - sh.col_left(cell.col)) as f32;
    let dy = (miny - sh.row_top(cell.row)) as f32;
    let text = pts.iter().map(|q| format!("{:.1},{:.1}", q[0] - minx, q[1] - miny)).collect::<Vec<_>>().join(";");
    let id = d.wb.next_object_id();
    let order = sh.shapes.iter().filter_map(|x| x.ink.as_ref()).map(|i| i.order).max().map_or(0, |o| o.saturating_add(1));
    let width_cm = px_to_cm(width);
    let shape = Shape {
        math: Default::default(),
        id,
        kind: ShapeKind::Ink,
        anchor: Anchor {
            cell,
            dx,
            dy,
            width: (maxx - minx).max(1.0) as f32,
            height: (maxy - miny).max(1.0) as f32,
            // Ink is a freehand mark: pinned where it was drawn, it does not follow cells.
            mode: gridcraft_model::AnchorMode::Absolute,
        },
        fill: Color::from_hex(&format!("#{:02X}0000", width.round().clamp(0.0, 255.0) as u8)).unwrap_or_default(),
        line: Color::from_hex(&color).unwrap_or(Color::rgb(0x1F, 0x5F, 0xC9)),
        text,
        format: Default::default(),
        ink: Some(InkStyle {
            tool: InkTool::Pen,
            width_cm,
            height_cm: width_cm,
            order,
            texture,
            texture_scale,
            key: Some(gridcraft_model::ink_creation_id(None)),
            ..InkStyle::default()
        }),
    };
    edit(s, |cx| {
        cx.sheet_mut(si)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
}

/// Ink width is stored in the red channel of `fill` (shapes have no width field). A stroke with
/// an [`InkStyle`] answers from its brush, in px, as before.
pub fn ink_width(sh: &Shape) -> f32 {
    if let Some(ink) = &sh.ink {
        return cm_to_px(ink.width_cm).max(1.0);
    }
    match sh.fill {
        Color::Rgb(v) => ((v >> 16) & 0xFF).max(1) as f32,
        _ => 2.0,
    }
}

/// Excel's ink measurements are cm; a sheet point is 1/96 inch.
const PX_PER_CM: f32 = 96.0 / 2.54;

fn px_to_cm(px: f32) -> f32 {
    (px / PX_PER_CM).clamp(0.001, 100.0)
}

fn cm_to_px(cm: f32) -> f32 {
    (cm * PX_PER_CM).clamp(0.1, 10_000.0)
}

pub fn ink_points(sh: &Shape) -> Vec<[f32; 2]> {
    parse_points(&sh.text)
}

fn ink_to_shape(s: &mut Session, p: &Json) -> Result<Json> {
    let si = s.doc()?.wb.active_sheet();
    let id = u32_param(p, "id");
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let idx = match id {
            Some(id) => sh.shapes.iter().position(|x| x.id == id && x.kind == ShapeKind::Ink),
            None => sh.shapes.iter().rposition(|x| x.kind == ShapeKind::Ink),
        }
        .ok_or_else(|| bad("draw.inkToShape", "no ink stroke to convert"))?;
        let ink = sh.shapes.get_mut(idx).ok_or_else(|| bad("draw.inkToShape", "gone"))?;
        let pts = parse_points(&ink.text);
        let (w, h) = (ink.anchor.width.max(1.0), ink.anchor.height.max(1.0));
        let closed = match (pts.first(), pts.last()) {
            (Some(a), Some(b)) => ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt() < 0.25 * w.max(h),
            _ => false,
        };
        let kind = if !closed {
            ShapeKind::Line
        } else {
            // Corners hugging the bounding box mean a rectangle; one apex at the top a triangle.
            let near = |cx: f32, cy: f32| pts.iter().any(|q| ((q[0] - cx).powi(2) + (q[1] - cy).powi(2)).sqrt() < 0.15 * w.min(h).max(8.0));
            let corners = [near(0.0, 0.0), near(w, 0.0), near(0.0, h), near(w, h)].iter().filter(|b| **b).count();
            let apex = near(w / 2.0, 0.0);
            if corners >= 3 {
                ShapeKind::Rectangle
            } else if apex && near(0.0, h) && near(w, h) {
                ShapeKind::Triangle
            } else {
                ShapeKind::Ellipse
            }
        };
        let color = ink.line;
        ink.kind = kind;
        ink.fill = if kind == ShapeKind::Line { color } else { Color::Auto };
        ink.text = String::new();
        if kind == ShapeKind::Line && pts.first().zip(pts.last()).is_some_and(|(a, b)| a[1] > b[1]) {
            // Lines are drawn top-left to bottom-right; keep the stroke direction simple.
        }
        Ok(json!({"kind": format!("{kind:?}")}))
    })
}
