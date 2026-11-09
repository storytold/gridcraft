//! Draw tab: pen and eraser tools, ink strokes, the pen gallery, Ink to Shape.

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, Color, InkEffect, InkStyle, InkTexture, InkTool, MAX_INK_TEXTURES, Shape, ShapeKind};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cmd, edit, f64_param, has_doc, ok, str_param, u32_param};

/// Excel's pen widths in millimetres (the Highlighter has its own wider steps).
pub const PEN_WIDTHS_MM: [f32; 6] = [0.25, 0.35, 0.5, 1.0, 2.0, 3.5];
pub const HIGHLIGHTER_WIDTHS_MM: [f32; 6] = [2.0, 4.0, 6.0, 8.0, 10.0, 10.0];

/// How many stroke widths one repeat of an imported pen texture spans when the pen doesn't
/// say, and the bounds it is kept within. The tile length is `width * scale`: a scale of 8 on
/// a pen paints one repeat every eight stroke widths, so repeats grow with the brush.
pub const DEFAULT_TEXTURE_SCALE: f32 = 8.0;
pub const MIN_TEXTURE_SCALE: f32 = 0.5;
pub const MAX_TEXTURE_SCALE: f32 = 512.0;

/// A repeat size from a parameter or the pen's state, kept finite and within
/// [`MIN_TEXTURE_SCALE`]–[`MAX_TEXTURE_SCALE`].
pub fn texture_scale(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v.clamp(MIN_TEXTURE_SCALE, MAX_TEXTURE_SCALE) } else { DEFAULT_TEXTURE_SCALE }
}

/// One pen of the Draw tab's gallery: its brush kind, colour, thickness in millimetres and
/// texture. Excel's default pens have ids 0–5; the person's own pens follow.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawPen {
    pub id: u32,
    /// The brush's [`InkTool`] JSON name.
    pub tool: String,
    /// `#RRGGBB`.
    pub color: String,
    /// Thickness in millimetres.
    pub width: f32,
    /// The [`InkEffect`] JSON name, when the pen paints with one of Excel's textures.
    pub effect: Option<String>,
    /// The id of an imported texture ([`gridcraft_model::InkTexture`]) the pen paints with; it
    /// wins over `effect`.
    pub texture: Option<String>,
    /// How many stroke widths one repeat of `texture` spans.
    pub texture_scale: f32,
}

/// How many default pens Excel's Drawing Tools gallery shows (their ids are 0 to this minus 1).
pub const DEFAULT_PEN_COUNT: u32 = 6;

/// Excel's default pens: a textured Galaxy pen, a red pen, a Pencil, a Highlighter, a Fountain
/// Pen and a Brush Pen. They are the gallery's first entries and cannot be deleted.
pub fn default_pens() -> Vec<DrawPen> {
    let pen = |id, tool: &str, color: &str, width, effect: Option<&str>| DrawPen {
        id,
        tool: tool.to_string(),
        color: color.to_string(),
        width,
        effect: effect.map(str::to_string),
        texture: None,
        texture_scale: DEFAULT_TEXTURE_SCALE,
    };
    vec![
        pen(0, "pen", effect_base(InkEffect::Galaxy), 0.5, Some("galaxy")),
        pen(1, "pen", "#D13B2F", 0.5, None),
        pen(2, "pencil", "#767676", 0.5, None),
        pen(3, "highlighter", "#F2E33A", 4.0, None),
        pen(4, "fountainPen", "#1F3864", 0.5, None),
        pen(5, "brushPen", "#2E7D4F", 0.5, None),
    ]
}

/// A default pen's label in the gallery, by its stable id; a custom pen has none (its brush kind
/// labels it).
pub fn pen_label(pen: &DrawPen) -> Option<&'static str> {
    match pen.id {
        0 => Some("Galaxy pen"),
        1 => Some("Red pen"),
        2 => Some("Pencil"),
        3 => Some("Highlighter"),
        4 => Some("Fountain Pen"),
        5 => Some("Brush Pen"),
        _ => None,
    }
}

/// The canonical JSON name of a brush kind, accepting a few spellings people and macros use.
pub fn tool_name(tool: InkTool) -> &'static str {
    match tool {
        InkTool::Pen => "pen",
        InkTool::Pencil => "pencil",
        InkTool::Highlighter => "highlighter",
        InkTool::FountainPen => "fountainPen",
        InkTool::BrushPen => "brushPen",
    }
}

/// Parses a brush kind by its JSON name (case and `_`/`-` insensitive).
pub fn parse_tool(s: &str) -> Option<InkTool> {
    match s.trim().to_ascii_lowercase().replace(['_', '-', ' '], "").as_str() {
        "pen" => Some(InkTool::Pen),
        "pencil" => Some(InkTool::Pencil),
        "highlighter" => Some(InkTool::Highlighter),
        "fountainpen" | "fountain" => Some(InkTool::FountainPen),
        "brushpen" | "brush" => Some(InkTool::BrushPen),
        _ => None,
    }
}

/// The canonical JSON name of a texture.
pub fn effect_name(effect: InkEffect) -> &'static str {
    match effect {
        InkEffect::Rainbow => "rainbow",
        InkEffect::Galaxy => "galaxy",
        InkEffect::Lava => "lava",
        InkEffect::Ocean => "ocean",
        InkEffect::RoseGold => "roseGold",
        InkEffect::Gold => "gold",
        InkEffect::Silver => "silver",
        InkEffect::Bronze => "bronze",
    }
}

/// Parses a texture by its JSON name; `None` for `"none"` or an empty string.
pub fn parse_effect(s: &str) -> Option<InkEffect> {
    match s.trim().to_ascii_lowercase().replace(['_', '-', ' '], "").as_str() {
        "" | "none" | "solid" => None,
        "rainbow" => Some(InkEffect::Rainbow),
        "galaxy" => Some(InkEffect::Galaxy),
        "lava" => Some(InkEffect::Lava),
        "ocean" => Some(InkEffect::Ocean),
        "rosegold" => Some(InkEffect::RoseGold),
        "gold" => Some(InkEffect::Gold),
        "silver" => Some(InkEffect::Silver),
        "bronze" => Some(InkEffect::Bronze),
        _ => None,
    }
}

/// The base colour Excel stores for a texture's stroke.
pub fn effect_base(effect: InkEffect) -> &'static str {
    match effect {
        InkEffect::Rainbow => "#E83A6B",
        InkEffect::Galaxy => "#5B3EA8",
        InkEffect::Lava => "#D94F1E",
        InkEffect::Ocean => "#1B7FB5",
        InkEffect::RoseGold => "#C98A7A",
        InkEffect::Gold => "#C99A2E",
        InkEffect::Silver => "#9AA3AD",
        InkEffect::Bronze => "#9A673A",
    }
}

/// A brush kind's thickness steps in millimetres.
pub fn widths_mm(tool: InkTool) -> &'static [f32] {
    match tool {
        InkTool::Highlighter => &HIGHLIGHTER_WIDTHS_MM,
        _ => &PEN_WIDTHS_MM,
    }
}

/// The InkML width/height (cm) of a stroke `width_mm` millimetres thick in `tool`'s brush:
/// a Pencil is six times as tall across the stroke and a Highlighter twice.
pub fn ink_size_cm(tool: InkTool, width_mm: f32) -> (f32, f32) {
    let w = (width_mm / 10.0).clamp(0.001, 5.0);
    match tool {
        InkTool::Pencil => (w, 6.0 * w),
        InkTool::Highlighter => (w, 2.0 * w),
        _ => (w, w),
    }
}

const PX_PER_CM: f32 = 37.795_276;

/// Centimetres to sheet points (96 dpi pixels).
pub fn cm_to_px(cm: f32) -> f32 {
    cm * PX_PER_CM
}

/// Sheet points (96 dpi pixels) to centimetres.
pub fn px_to_cm(px: f32) -> f32 {
    px / PX_PER_CM
}

/// Every `draw.*` command the CLI and the host can call, with its parameter shapes.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "draw.pen",
            "Pen",
            ["Draw", "Tools"],
            None,
            "{on?: bool, brush?: pen|pencil|highlighter|fountainPen|brushPen, color?: #RRGGBB, width?: mm, effect?: rainbow|galaxy|lava|ocean|roseGold|gold|silver|bronze|none, texture?: imported id | \"\" (none), textureScale?: 8, pen?: gallery id}",
            has_doc,
            pen
        ),
        cmd!(
            noundo "draw.addPen",
            "Add Pen",
            ["Draw", "Tools"],
            None,
            "{brush|kind: pen|pencil|highlighter|fountainPen|brushPen, color?: #RRGGBB, width?: mm, effect?: name, texture?: imported id, textureScale?: 8} → {pen}",
            has_doc,
            add_pen
        ),
        cmd!(
            noundo "draw.updatePen",
            "Change Pen",
            ["Draw", "Tools"],
            None,
            "{id, brush?: kind, color?: #RRGGBB, width?: mm, effect?: name, texture?: imported id | \"\"} → {pen}",
            has_doc,
            update_pen
        ),
        cmd!(noundo "draw.deletePen", "Delete Tool", ["Draw", "Tools"], None, "{id} → {removed}", has_doc, delete_pen),
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
            "{points: [[x,y],…] in sheet points, brush?: kind, color?: hex, width?: mm, effect?: name, texture?: imported id, textureScale?: 8, pressure?: [0..32767] per point}",
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
        cmd!("draw.inkToShape", "Ink to Shape", [], None, "{id?: shape id} → {kind} (needs the Draw tab's Ink to Shape)", has_doc, ink_to_shape),
    ]
}

/// A pen's colour as a model colour: any `#RRGGBB` (or `#AARRGGBB`); `None` if it is not one.
fn parse_color(s: &str) -> Option<Color> {
    Color::from_hex(s)
}

/// The colour a stroke commits: a textured pen's base colour (Excel stores the texture's own
/// colour), otherwise the requested one or the pen blue.
fn stroke_color(color: Option<&str>, effect: Option<InkEffect>, fallback: &str) -> Color {
    let pick = effect.map(effect_base).or(color).unwrap_or(fallback);
    parse_color(pick).unwrap_or(Color::rgb(0x1F, 0x5F, 0xC9))
}

/// `draw.pen`: turn the pen on or off, or take a gallery pen's brush, colour, width, effect and
/// texture into the hand that paints.
fn pen(s: &mut Session, p: &Json) -> Result<Json> {
    s.ensure_draw_pens();
    let selecting = ["brush", "kind", "color", "width", "effect", "texture", "textureScale", "pen"].iter().any(|k| p.get(*k).is_some());
    let on = bool_param(p, "on").unwrap_or(selecting || s.draw_tool != "pen");
    s.draw_tool = if on { "pen".into() } else { String::new() };

    // Selecting a gallery pen loads its brush, colour, thickness and texture.
    if let Some(id) = u32_param(p, "pen") {
        let chosen = s.draw_pens.iter().find(|q| q.id == id).cloned().ok_or_else(|| bad("draw.pen", format!("no pen {id} in the gallery")))?;
        s.draw_brush = chosen.tool;
        s.draw_color = chosen.color;
        s.draw_width = chosen.width;
        s.draw_effect = chosen.effect.unwrap_or_default();
        s.draw_texture = chosen.texture.unwrap_or_default();
        s.draw_texture_scale = texture_scale(chosen.texture_scale);
    }
    if let Some(name) = str_param(p, "brush").or_else(|| str_param(p, "kind")) {
        let tool = parse_tool(name).ok_or_else(|| bad("draw.pen", format!("unknown brush '{name}'")))?;
        s.draw_brush = tool_name(tool).to_string();
    }
    let tool = parse_tool(&s.draw_brush).unwrap_or(InkTool::Pen);
    if let Some(c) = str_param(p, "color") {
        if parse_color(c).is_none() {
            return Err(bad("draw.pen", format!("'{c}' is not a #RRGGBB colour")));
        }
        s.draw_color = c.to_string();
    }
    if let Some(w) = f64_param(p, "width") {
        if !w.is_finite() {
            return Err(bad("draw.pen", "the width must be a finite number of millimetres"));
        }
        s.draw_width = (w as f32).clamp(0.05, 20.0);
    } else if s.draw_width <= 0.0 {
        s.draw_width = *widths_mm(tool).first().unwrap_or(&0.5);
    }
    if let Some(e) = str_param(p, "effect") {
        let effect = parse_effect(e);
        // Four standard names must not silently fall back to solid; only the none spellings do.
        let is_none = e.trim().is_empty() || matches!(e.trim().to_ascii_lowercase().as_str(), "none" | "solid");
        if effect.is_none() && !is_none {
            return Err(bad("draw.pen", format!("unknown effect '{e}'")));
        }
        s.draw_effect = effect.map(|q| effect_name(q).to_string()).unwrap_or_default();
        if effect.is_some() && str_param(p, "color").is_none() {
            s.draw_color = effect.map(effect_base).unwrap_or("#1F5FC9").to_string();
        }
    }
    let effect = parse_effect(&s.draw_effect);
    if s.draw_color.eq_ignore_ascii_case("none") {
        s.draw_color = "#1F5FC9".into();
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
        "brush": s.draw_brush,
        "color": s.draw_color,
        "width": s.draw_width,
        "effect": effect.map(effect_name),
        "texture": s.draw_texture,
        "textureScale": texture_scale(s.draw_texture_scale),
    }))
}

/// `draw.addPen`: add a pen to the gallery from a brush and optional colour, width, effect and
/// texture, and answer the pen it minted.
fn add_pen(s: &mut Session, p: &Json) -> Result<Json> {
    s.ensure_draw_pens();
    let name = str_param(p, "brush").or_else(|| str_param(p, "kind")).unwrap_or("pen");
    let tool = parse_tool(name).ok_or_else(|| bad("draw.addPen", format!("unknown brush '{name}'")))?;
    let effect = match str_param(p, "effect") {
        Some(e) => {
            let parsed = parse_effect(e);
            let is_none = e.trim().is_empty() || matches!(e.trim().to_ascii_lowercase().as_str(), "none" | "solid");
            if parsed.is_none() && !is_none {
                return Err(bad("draw.addPen", format!("unknown effect '{e}'")));
            }
            parsed
        }
        None => None,
    };
    let color = match str_param(p, "color") {
        Some(c) if parse_color(c).is_some() => c.to_string(),
        Some(c) => return Err(bad("draw.addPen", format!("'{c}' is not a #RRGGBB colour"))),
        None => effect.map(effect_base).unwrap_or("#1F5FC9").to_string(),
    };
    let default_width = *widths_mm(tool).first().unwrap_or(&0.5);
    let width = match f64_param(p, "width") {
        Some(w) if w.is_finite() => (w as f32).clamp(0.05, 20.0),
        Some(_) => return Err(bad("draw.addPen", "the width must be a finite number of millimetres")),
        None => default_width,
    };
    let id = s.draw_pens.iter().map(|q| q.id).max().map_or(1, |m| m.saturating_add(1));
    let texture = match str_param(p, "texture").map(str::trim).filter(|id| !id.is_empty()) {
        Some(texture) => {
            if s.doc()?.wb.ink_texture(texture).is_none() {
                return Err(bad("draw.addPen", "no imported texture has that id"));
            }
            Some(texture.to_string())
        }
        None => None,
    };
    let scale = f64_param(p, "textureScale").map(|v| texture_scale(v as f32)).unwrap_or(DEFAULT_TEXTURE_SCALE);
    let pen = DrawPen {
        id,
        tool: tool_name(tool).to_string(),
        color,
        width,
        effect: effect.map(|e| effect_name(e).to_string()),
        texture,
        texture_scale: scale,
    };
    s.draw_pens.push(pen.clone());
    // A new pen becomes the one being drawn with, as in Excel's Add menu.
    s.draw_tool = "pen".into();
    s.draw_brush = pen.tool.clone();
    s.draw_color = pen.color.clone();
    s.draw_width = pen.width;
    s.draw_effect = pen.effect.clone().unwrap_or_default();
    s.draw_texture = pen.texture.clone().unwrap_or_default();
    s.draw_texture_scale = pen.texture_scale;
    Ok(json!({
        "pen": id,
        "brush": pen.tool,
        "color": pen.color,
        "width": pen.width,
        "effect": pen.effect,
        "texture": pen.texture,
        "textureScale": pen.texture_scale,
    }))
}

/// `draw.updatePen`: change the gallery pen whose id is given, leaving the fields it did not name
/// as they were; the pen in hand follows when it is the one changed.
fn update_pen(s: &mut Session, p: &Json) -> Result<Json> {
    s.ensure_draw_pens();
    let id = u32_param(p, "id").ok_or_else(|| bad("draw.updatePen", "the id of the pen to change is required"))?;
    let idx = s.draw_pens.iter().position(|q| q.id == id).ok_or_else(|| bad("draw.updatePen", format!("no pen {id} in the gallery")))?;
    let old = s.draw_pens.get(idx).cloned().ok_or_else(|| bad("draw.updatePen", "gone"))?;
    let mut pen = old.clone();
    if let Some(name) = str_param(p, "brush").or_else(|| str_param(p, "kind")) {
        let tool = parse_tool(name).ok_or_else(|| bad("draw.updatePen", format!("unknown brush '{name}'")))?;
        pen.tool = tool_name(tool).to_string();
    }
    if let Some(c) = str_param(p, "color") {
        if parse_color(c).is_none() {
            return Err(bad("draw.updatePen", format!("'{c}' is not a #RRGGBB colour")));
        }
        pen.color = c.to_string();
    }
    if let Some(w) = f64_param(p, "width") {
        if !w.is_finite() {
            return Err(bad("draw.updatePen", "the width must be a finite number of millimetres"));
        }
        pen.width = (w as f32).clamp(0.05, 20.0);
    }
    if let Some(e) = str_param(p, "effect") {
        let parsed = parse_effect(e);
        let is_none = e.trim().is_empty() || matches!(e.trim().to_ascii_lowercase().as_str(), "none" | "solid");
        if parsed.is_none() && !is_none {
            return Err(bad("draw.updatePen", format!("unknown effect '{e}'")));
        }
        pen.effect = parsed.map(|q| effect_name(q).to_string());
        if parsed.is_some() && str_param(p, "color").is_none() {
            pen.color = parsed.map(effect_base).unwrap_or("#1F5FC9").to_string();
        }
    }
    if let Some(id) = p.get("texture").and_then(Json::as_str) {
        let id = id.trim();
        if !id.is_empty() && s.doc()?.wb.ink_texture(id).is_none() {
            return Err(bad("draw.updatePen", "no imported texture has that id"));
        }
        pen.texture = (!id.is_empty()).then(|| id.to_string());
    }
    if let Some(v) = f64_param(p, "textureScale") {
        pen.texture_scale = texture_scale(v as f32);
    }
    // The pen in hand follows its gallery entry when it is the entry being changed.
    let in_hand = s.draw_tool == "pen"
        && s.draw_brush == old.tool
        && s.draw_color.eq_ignore_ascii_case(&old.color)
        && (s.draw_width - old.width).abs() < 0.01
        && s.draw_effect == old.effect.clone().unwrap_or_default()
        && s.draw_texture == old.texture.clone().unwrap_or_default()
        && texture_scale(s.draw_texture_scale) == texture_scale(old.texture_scale);
    s.draw_pens[idx] = pen.clone();
    if in_hand {
        s.draw_brush = pen.tool.clone();
        s.draw_color = pen.color.clone();
        s.draw_width = pen.width;
        s.draw_effect = pen.effect.clone().unwrap_or_default();
        s.draw_texture = pen.texture.clone().unwrap_or_default();
        s.draw_texture_scale = pen.texture_scale;
    }
    Ok(json!({
        "pen": pen.id,
        "brush": pen.tool,
        "color": pen.color,
        "width": pen.width,
        "effect": pen.effect,
        "texture": pen.texture,
        "textureScale": pen.texture_scale,
    }))
}

/// `draw.deletePen`: drop a gallery pen, refusing Excel's built-in ones and putting the default
/// pen back in hand when the deleted pen was the one painting.
fn delete_pen(s: &mut Session, p: &Json) -> Result<Json> {
    s.ensure_draw_pens();
    let id = u32_param(p, "id").ok_or_else(|| bad("draw.deletePen", "the id of the pen to delete is required"))?;
    if id < DEFAULT_PEN_COUNT {
        return Err(bad("draw.deletePen", "Excel's default pens cannot be deleted"));
    }
    let doomed = s.draw_pens.iter().find(|q| q.id == id).cloned();
    let before = s.draw_pens.len();
    s.draw_pens.retain(|q| q.id != id);
    let removed = s.draw_pens.len() != before;
    if let Some(pen) = doomed
        && s.draw_tool == "pen"
        && s.draw_brush == pen.tool
        && s.draw_color.eq_ignore_ascii_case(&pen.color)
        && (s.draw_width - pen.width).abs() < 0.01
        && s.draw_effect == pen.effect.clone().unwrap_or_default()
        && s.draw_texture == pen.texture.clone().unwrap_or_default()
        && texture_scale(s.draw_texture_scale) == texture_scale(pen.texture_scale)
    {
        // Deleting the pen in hand puts the default pen back in hand.
        s.draw_brush = "pen".into();
        s.draw_effect.clear();
        s.draw_color = "#1F5FC9".into();
        s.draw_width = *widths_mm(InkTool::Pen).first().unwrap_or(&0.5);
        s.draw_texture.clear();
        s.draw_texture_scale = DEFAULT_TEXTURE_SCALE;
    }
    Ok(json!({"removed": removed}))
}

/// `draw.eraser`: turn the eraser on or off, which turns the pen off when it comes on.
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

/// `draw.textureImport`: stores an imported PNG or JPEG in the workbook's texture store, or reuses
/// the entry the same bytes already made. The command is one undo step, and a refused image leaves
/// the workbook untouched.
fn texture_import(s: &mut Session, p: &Json) -> Result<Json> {
    let (name, data) = if let Some(b) = str_param(p, "base64") {
        // Check the encoded length first, so a hostile parameter is refused before decoding.
        let longest = gridcraft_model::MAX_INK_TEXTURE_BYTES.div_ceil(3) * 4 + 4;
        if b.len() > longest {
            return Err(bad("draw.textureImport", "brush textures must be at most 16 MiB"));
        }
        let data = crate::io::base64_decode(b).ok_or_else(|| bad("draw.textureImport", "invalid base64"))?;
        (str_param(p, "name").unwrap_or("Texture").to_string(), data)
    } else if let Some(path) = str_param(p, "path").filter(|path| !path.trim().is_empty()) {
        // Bound the read itself: a 2 GB file must not be allocated before the 16 MiB check.
        let data = crate::io::read_file_limited(path, gridcraft_model::MAX_INK_TEXTURE_BYTES as u64)?;
        let name = str_param(p, "name")
            .map(str::to_string)
            .unwrap_or_else(|| path.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or("Texture").to_string());
        (name, data)
    } else {
        return Err(bad("draw.textureImport", "choose a PNG or JPEG image to import (give `path`, or `name` and `base64`)"));
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

/// `draw.textureRemove`: drops a texture from the workbook and takes it out of the pen in hand and
/// of every gallery pen, so nothing keeps painting with an image the store no longer holds.
fn texture_remove(s: &mut Session, p: &Json) -> Result<Json> {
    let id = str_param(p, "id").ok_or_else(|| bad("draw.textureRemove", "missing `id`"))?.to_string();
    if s.doc()?.wb.ink_texture(&id).is_none() {
        return Ok(json!({"removed": false}));
    }
    if s.draw_texture == id {
        s.draw_texture.clear();
    }
    for pen in &mut s.draw_pens {
        if pen.texture.as_deref() == Some(id.as_str()) {
            pen.texture = None;
        }
    }
    edit(s, |cx| {
        let removed = cx.wb.remove_ink_texture(&id).is_some();
        Ok(json!({"removed": removed}))
    })
}

/// `draw.textureList`: the workbook's textures (with their encoded byte length), the texture the
/// pen in hand paints with and the repeat size it uses.
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

/// The stroke's `x,y;x,y` text as points, skipping any pair that is not two numbers.
fn parse_points(t: &str) -> Vec<[f32; 2]> {
    t.split(';').filter_map(|pt| pt.split_once(',').and_then(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?]))).collect()
}

/// `draw.stroke`: a freehand stroke of `points` in sheet points, with its brush and draw order.
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

    let tool = match str_param(p, "brush").or_else(|| str_param(p, "kind")) {
        Some(name) => parse_tool(name).ok_or_else(|| bad("draw.stroke", format!("unknown brush '{name}'")))?,
        None => parse_tool(&s.draw_brush).unwrap_or(InkTool::Pen),
    };
    let effect = match str_param(p, "effect") {
        Some(e) => {
            let parsed = parse_effect(e);
            let is_none = e.trim().is_empty() || matches!(e.trim().to_ascii_lowercase().as_str(), "none" | "solid");
            if parsed.is_none() && !is_none {
                return Err(bad("draw.stroke", format!("unknown effect '{e}'")));
            }
            parsed
        }
        None => parse_effect(&s.draw_effect),
    };
    let fallback = if s.draw_color.is_empty() { "#1F5FC9" } else { s.draw_color.as_str() };
    let line = stroke_color(str_param(p, "color"), effect, fallback);
    let width_mm = match f64_param(p, "width") {
        Some(w) if w.is_finite() => (w as f32).clamp(0.05, 20.0),
        Some(_) => return Err(bad("draw.stroke", "the width must be a finite number of millimetres")),
        None => {
            let w = if s.draw_width <= 0.0 { *widths_mm(tool).first().unwrap_or(&0.5) } else { s.draw_width };
            w.clamp(0.05, 20.0)
        }
    };
    let (width_cm, height_cm) = ink_size_cm(tool, width_mm);
    let pressure: Vec<u16> = p
        .get("pressure")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 32767.0).round() as u16).take(pts.len()).collect())
        .unwrap_or_default();
    // A texture the caller names and one the pen carries are different promises: an explicit id
    // must exist (a typo is an error), while the pen's is session state — undo can take the
    // imported texture away and another workbook never held it — so a stale one paints plainly.
    let explicit = p.get("texture").and_then(Json::as_str).map(str::trim).filter(|id| !id.is_empty()).map(str::to_string);
    let from_session = explicit.is_none();
    let mut texture = explicit.or_else(|| (!s.draw_texture.is_empty()).then(|| s.draw_texture.clone()));
    let texture_scale = f64_param(p, "textureScale").map(|v| texture_scale(v as f32)).unwrap_or_else(|| texture_scale(s.draw_texture_scale));

    let d = s.doc()?;
    if let Some(id) = &texture
        && d.wb.ink_texture(id).is_none()
    {
        if !from_session {
            return Err(bad("draw.stroke", "no imported texture has that id"));
        }
        // The pen's texture is not in this workbook (undone, or another workbook): paint plainly.
        texture = None;
    }
    let si = d.wb.active_sheet();
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let cell = CellRef::new(sh.row_at(miny), sh.col_at(minx));
    let dx = (minx - sh.col_left(cell.col)) as f32;
    let dy = (miny - sh.row_top(cell.row)) as f32;
    let text = pts.iter().map(|q| format!("{:.1},{:.1}", q[0] - minx, q[1] - miny)).collect::<Vec<_>>().join(";");
    let order = sh.shapes.iter().filter_map(|x| x.ink.as_ref()).map(|i| i.order).max().map_or(0, |o| o.saturating_add(1));
    let id = d.wb.next_object_id();
    let px_width = cm_to_px(width_cm).round().clamp(1.0, 255.0) as u8;
    // The texture's phase follows the stroke's place on the sheet (Excel's anchorX/anchorY are
    // negative multiples of the texture's cell); scaleFactor 0.5 is Excel's default.
    let anchor = [-minx as f32, -miny as f32];
    // A reopened workbook holds keys minted by an earlier process, whose counter started at zero,
    // so a freshly minted key can be one the workbook already uses: mint past those.
    let held: Vec<&str> =
        d.wb.sheets.iter().flat_map(|sheet| sheet.shapes.iter()).filter_map(|x| x.ink.as_ref()).filter_map(|i| i.key.as_deref()).collect();
    let key = gridcraft_model::ink_creation_id_free(|k| held.contains(&k));
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
        fill: Color::from_hex(&format!("#{px_width:02X}0000")).unwrap_or_default(),
        line,
        text,
        format: Default::default(),
        ink: Some(InkStyle {
            tool,
            width_cm,
            height_cm,
            effect,
            anchor,
            scale: if effect.is_some() { 0.5 } else { 1.0 },
            pressure,
            order,
            texture,
            texture_scale,
            key: Some(key),
        }),
    };
    edit(s, |cx| {
        cx.sheet_mut(si)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
}

/// Ink width in sheet points: a brush's own width when it has an [`InkStyle`], otherwise the red
/// channel of `fill` (the legacy encoding).
pub fn ink_width(sh: &Shape) -> f32 {
    if let Some(ink) = &sh.ink {
        return cm_to_px(ink.width_cm).max(1.0);
    }
    match sh.fill {
        Color::Rgb(v) => ((v >> 16) & 0xFF).max(1) as f32,
        _ => 2.0,
    }
}

/// A stroke's width across itself in sheet points (a Pencil and a Highlighter are broader than
/// their `width_cm`).
pub fn ink_height(sh: &Shape) -> f32 {
    if let Some(ink) = &sh.ink {
        return cm_to_px(ink.height_cm).max(1.0);
    }
    ink_width(sh)
}

/// The width a brush paints across its stroke, in cm: a Highlighter is its height (twice the pen
/// width), a Pencil 0.8 of its width, and every other brush paints its width. The live preview,
/// the committed stroke and their texture tiles all measure the stroke this way.
pub fn across_cm(tool: InkTool, width_cm: f32, height_cm: f32) -> f32 {
    match tool {
        InkTool::Highlighter => height_cm,
        InkTool::Pencil => width_cm * 0.8,
        _ => width_cm,
    }
}

/// [`across_cm`] for a committed stroke, in sheet points.
pub fn ink_across(sh: &Shape) -> f32 {
    match &sh.ink {
        Some(ink) => cm_to_px(across_cm(ink.tool, ink.width_cm, ink.height_cm)).max(1.0),
        None => ink_width(sh),
    }
}

/// The stroke's points in sheet points, relative to its anchor, as `draw.stroke` took them.
pub fn ink_points(sh: &Shape) -> Vec<[f32; 2]> {
    parse_points(&sh.text)
}

/// `draw.inkToShape`: convert a stroke (the given shape id, or the last one drawn) into the
/// geometry that best fits it.
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
        ink.ink = None;
        ink.fill = if kind == ShapeKind::Line { color } else { Color::Auto };
        ink.text = String::new();
        Ok(json!({"kind": format!("{kind:?}")}))
    })
}
