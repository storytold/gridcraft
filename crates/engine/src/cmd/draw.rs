//! Draw tab: pen and eraser tools, ink strokes, the pen gallery, Ink to Shape.

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, Color, InkEffect, InkStyle, InkTool, Shape, ShapeKind};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cmd, edit, has_doc, ok, str_param, u32_param};

/// Excel's pen widths in millimetres (the Highlighter has its own wider steps).
pub const PEN_WIDTHS_MM: [f32; 6] = [0.25, 0.35, 0.5, 1.0, 2.0, 3.5];
pub const HIGHLIGHTER_WIDTHS_MM: [f32; 5] = [2.0, 4.0, 6.0, 8.0, 10.0];

/// The most points one stroke may carry (the same limit the InkML reader enforces).
pub const MAX_INK_POINTS: usize = 20_000;

/// The sheet-coordinate bound a stroke point may not pass (the InkML reader's `MAX_SHEET`).
pub const MAX_STROKE_COORD: f64 = 1e7;

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
    /// The [`InkEffect`] JSON name, when the pen is textured.
    pub effect: Option<String>,
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

/// The canonical JSON name of a texture (the model owns the table, so the engine, xlsx and UI
/// cannot drift).
pub fn effect_name(effect: InkEffect) -> &'static str {
    effect.name()
}

/// Parses a texture by its JSON name; `None` for `"none"` or an empty string.
pub fn parse_effect(s: &str) -> Option<InkEffect> {
    InkEffect::from_name(s)
}

/// The base colour Excel stores for a texture's stroke.
pub fn effect_base(effect: InkEffect) -> &'static str {
    effect.base_hex()
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

pub fn specs() -> Vec<CommandSpec> {
    // Every pen command returns the same object: `{pen: gallery id, brush, color, width, effect}`.
    // `width` is millimetres (`widthMm` is the same value under its canonical name). `effect` is
    // the texture's JSON name, or null when the pen is solid. `brush` and `kind` are the same
    // parameter: `brush` is canonical and `kind` is kept as an alias for older macros, and every
    // spec lists both so the docs match the code.
    vec![
        cmd!(
            noundo "draw.pen",
            "Pen",
            ["Draw", "Tools"],
            None,
            "{on?: bool, pen?: gallery id, brush|kind?: pen|pencil|highlighter|fountainPen|brushPen, color?: #RRGGBB, width?: mm, widthMm?: mm, effect?: rainbow|galaxy|lava|ocean|roseGold|gold|silver|bronze|none} → {pen: gallery id, brush, color, width (mm), effect or null}",
            has_doc,
            pen
        ),
        cmd!(
            noundo "draw.addPen",
            "Add Pen",
            ["Draw", "Tools"],
            None,
            "{brush|kind: pen|pencil|highlighter|fountainPen|brushPen, color?: #RRGGBB, width?: mm, widthMm?: mm, effect?: rainbow|galaxy|lava|ocean|roseGold|gold|silver|bronze|none} → {pen: gallery id, brush, color, width (mm), effect or null}",
            has_doc,
            add_pen
        ),
        cmd!(
            noundo "draw.updatePen",
            "Change Pen",
            ["Draw", "Tools"],
            None,
            "{id, brush|kind?: pen|pencil|highlighter|fountainPen|brushPen, color?: #RRGGBB, width?: mm, widthMm?: mm, effect?: rainbow|galaxy|lava|ocean|roseGold|gold|silver|bronze|none} → {pen: gallery id, brush, color, width (mm), effect or null}",
            has_doc,
            update_pen
        ),
        cmd!(
            noundo "draw.deletePen",
            "Delete Tool",
            ["Draw", "Tools"],
            None,
            "{id} → {removed: true}; an unknown id is an error, as in draw.updatePen",
            has_doc,
            delete_pen
        ),
        cmd!(
            query "draw.pens",
            "Pens",
            ["Draw", "Tools"],
            None,
            "{} → {pens: [{pen: gallery id, brush, color, width (mm), effect or null},…]}, Excel's six default pens first, at most 1024 entries",
            has_doc,
            list_pens
        ),
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
            "{points: [[x,y],…] in sheet points (at most 20000; |x| and |y| at most 1e7), brush|kind?: pen|pencil|highlighter|fountainPen|brushPen, color?: #RRGGBB (an invalid colour is an error), width?: mm (millimetres: 1 mm = 3.7795 pt; callers that pass points must convert), widthMm?: mm, effect?: rainbow|galaxy|lava|ocean|roseGold|gold|silver|bronze|none, pressure?: [0..32767] once per point (a wrong length or a non-number is an error)} → {shape}",
            has_doc,
            stroke
        ),
        cmd!("draw.inkToShape", "Ink to Shape", [], None, "{id?: shape id} → {kind} (needs the Draw tab's Ink to Shape)", has_doc, ink_to_shape),
    ]
}

/// A pen's colour as a model colour: any `#RRGGBB` (or `#AARRGGBB`); `None` if it is not one.
fn parse_color(s: &str) -> Option<Color> {
    Color::from_hex(s)
}

/// The colour a stroke commits: a textured pen's base colour (Excel stores the texture's own
/// colour), otherwise the requested one or the pen blue.
fn stroke_color(color: Option<Color>, effect: Option<InkEffect>, fallback: &str) -> Color {
    if let Some(e) = effect
        && let Some(c) = parse_color(effect_base(e))
    {
        return c;
    }
    color.or_else(|| parse_color(fallback)).unwrap_or(Color::rgb(0x1F, 0x5F, 0xC9))
}

/// The one shape every pen command returns: the gallery id it stands for (null for the pen in
/// hand when no gallery entry matches), its brush, colour, thickness in millimetres and texture.
fn pen_json(id: Option<u32>, tool: &str, color: &str, width: f32, effect: Option<&str>) -> Json {
    json!({"pen": id, "brush": tool, "color": color, "width": width, "effect": effect})
}

/// [`pen_json`] for a gallery pen.
fn gallery_pen_json(pen: &DrawPen) -> Json {
    pen_json(Some(pen.id), &pen.tool, &pen.color, pen.width, pen.effect.as_deref())
}

/// A millimetre width from `widthMm` (the canonical name) or `width`; a wrong type is an error
/// rather than a silently ignored field.
fn width_mm(s: &str, p: &Json) -> Result<Option<f32>> {
    let Some((key, v)) = ["widthMm", "width"].iter().find_map(|k| p.get(*k).filter(|v| !v.is_null()).map(|v| (*k, v))) else {
        return Ok(None);
    };
    let w = v.as_f64().filter(|w| w.is_finite()).ok_or_else(|| bad(s, format!("{key} must be a finite number of millimetres")))?;
    Ok(Some((w as f32).clamp(0.05, 20.0)))
}

/// A texture parameter: `None` for the none spellings, an error for an unknown name.
fn effect_param(s: &str, e: &str) -> Result<Option<InkEffect>> {
    let parsed = parse_effect(e);
    let is_none = e.trim().is_empty() || matches!(e.trim().to_ascii_lowercase().as_str(), "none" | "solid");
    if parsed.is_none() && !is_none {
        return Err(bad(s, format!("unknown effect '{e}'")));
    }
    Ok(parsed)
}

fn pen(s: &mut Session, p: &Json) -> Result<Json> {
    s.ensure_draw_pens();
    let selecting = ["brush", "kind", "color", "width", "widthMm", "effect", "pen"].iter().any(|k| p.get(*k).is_some());
    let on = bool_param(p, "on").unwrap_or(selecting || s.draw_tool != "pen");

    // Every parameter is validated into locals first: a rejected call must leave the session as it
    // was, not half-switched to the new tool.
    let mut brush = s.draw_brush.clone();
    let mut color = s.draw_color.clone();
    let mut width = s.draw_width;
    let mut effect = s.draw_effect.clone();

    // Selecting a gallery pen loads its brush, colour, thickness and texture.
    if let Some(id) = u32_param(p, "pen") {
        let chosen = s.draw_pens.iter().find(|q| q.id == id).cloned().ok_or_else(|| bad("draw.pen", format!("no pen {id} in the gallery")))?;
        brush = chosen.tool;
        color = chosen.color;
        width = chosen.width;
        effect = chosen.effect.unwrap_or_default();
    }
    if let Some(name) = str_param(p, "brush").or_else(|| str_param(p, "kind")) {
        let tool = parse_tool(name).ok_or_else(|| bad("draw.pen", format!("unknown brush '{name}'")))?;
        brush = tool_name(tool).to_string();
    }
    let tool = parse_tool(&brush).unwrap_or(InkTool::Pen);
    let color_param = match str_param(p, "color") {
        Some(c) if parse_color(c).is_some() => Some(c.to_string()),
        Some(c) => return Err(bad("draw.pen", format!("'{c}' is not a #RRGGBB colour"))),
        None => None,
    };
    if let Some(c) = &color_param {
        color = c.clone();
    }
    match width_mm("draw.pen", p)? {
        Some(w) => width = w,
        None if width <= 0.0 => width = *widths_mm(tool).first().unwrap_or(&0.5),
        None => {}
    }
    if let Some(e) = str_param(p, "effect") {
        let parsed = effect_param("draw.pen", e)?;
        effect = parsed.map(|q| effect_name(q).to_string()).unwrap_or_default();
        if let Some(q) = parsed
            && color_param.is_none()
        {
            color = effect_base(q).to_string();
        }
    }

    s.draw_tool = if on { "pen".into() } else { String::new() };
    s.draw_brush = brush;
    s.draw_color = color;
    s.draw_width = width;
    s.draw_effect = effect;
    // The gallery entry the pen in hand stands for, when it matches one exactly.
    let in_hand = s
        .draw_pens
        .iter()
        .find(|q| {
            q.tool == s.draw_brush
                && q.color.eq_ignore_ascii_case(&s.draw_color)
                && (q.width - s.draw_width).abs() < 0.01
                && q.effect.as_deref().unwrap_or_default() == s.draw_effect
        })
        .map(|q| q.id);
    Ok(pen_json(in_hand, &s.draw_brush, &s.draw_color, s.draw_width, parse_effect(&s.draw_effect).map(effect_name)))
}

fn add_pen(s: &mut Session, p: &Json) -> Result<Json> {
    s.ensure_draw_pens();
    let name = str_param(p, "brush").or_else(|| str_param(p, "kind")).unwrap_or("pen");
    let tool = parse_tool(name).ok_or_else(|| bad("draw.addPen", format!("unknown brush '{name}'")))?;
    let effect = match str_param(p, "effect") {
        Some(e) => effect_param("draw.addPen", e)?,
        None => None,
    };
    let color = match str_param(p, "color") {
        Some(c) if parse_color(c).is_some() => c.to_string(),
        Some(c) => return Err(bad("draw.addPen", format!("'{c}' is not a #RRGGBB colour"))),
        None => effect.map(effect_base).unwrap_or("#1F5FC9").to_string(),
    };
    let width = width_mm("draw.addPen", p)?.unwrap_or(*widths_mm(tool).first().unwrap_or(&0.5));
    let id = s.next_draw_pen_id();
    let pen = DrawPen { id, tool: tool_name(tool).to_string(), color, width, effect: effect.map(|e| effect_name(e).to_string()) };
    s.draw_pens.push(pen.clone());
    // A new pen becomes the one being drawn with, as in Excel's Add menu.
    s.draw_tool = "pen".into();
    s.draw_brush = pen.tool.clone();
    s.draw_color = pen.color.clone();
    s.draw_width = pen.width;
    s.draw_effect = pen.effect.clone().unwrap_or_default();
    Ok(gallery_pen_json(&pen))
}

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
    let color_param = match str_param(p, "color") {
        Some(c) if parse_color(c).is_some() => Some(c.to_string()),
        Some(c) => return Err(bad("draw.updatePen", format!("'{c}' is not a #RRGGBB colour"))),
        None => None,
    };
    if let Some(c) = &color_param {
        pen.color = c.clone();
    }
    if let Some(w) = width_mm("draw.updatePen", p)? {
        pen.width = w;
    }
    if let Some(e) = str_param(p, "effect") {
        let parsed = effect_param("draw.updatePen", e)?;
        pen.effect = parsed.map(|q| effect_name(q).to_string());
        if let Some(q) = parsed
            && color_param.is_none()
        {
            pen.color = effect_base(q).to_string();
        }
    }
    // The pen in hand follows its gallery entry when it is the entry being changed. A front end
    // that edits another entry selects it afterwards (`draw.pen`), so the tile it just changed is
    // the one it draws with.
    let in_hand = s.draw_tool == "pen"
        && s.draw_brush == old.tool
        && s.draw_color.eq_ignore_ascii_case(&old.color)
        && (s.draw_width - old.width).abs() < 0.01
        && s.draw_effect == old.effect.clone().unwrap_or_default();
    s.draw_pens[idx] = pen.clone();
    if in_hand {
        s.draw_brush = pen.tool.clone();
        s.draw_color = pen.color.clone();
        s.draw_width = pen.width;
        s.draw_effect = pen.effect.clone().unwrap_or_default();
    }
    Ok(gallery_pen_json(&pen))
}

fn list_pens(s: &mut Session, _p: &Json) -> Result<Json> {
    s.ensure_draw_pens();
    Ok(json!({"pens": s.draw_pens.iter().map(gallery_pen_json).collect::<Vec<_>>()}))
}

fn delete_pen(s: &mut Session, p: &Json) -> Result<Json> {
    s.ensure_draw_pens();
    let id = u32_param(p, "id").ok_or_else(|| bad("draw.deletePen", "the id of the pen to delete is required"))?;
    if id < DEFAULT_PEN_COUNT {
        return Err(bad("draw.deletePen", "Excel's default pens cannot be deleted"));
    }
    let idx = s.draw_pens.iter().position(|q| q.id == id).ok_or_else(|| bad("draw.deletePen", format!("no pen {id} in the gallery")))?;
    let pen = s.draw_pens.remove(idx);
    if s.draw_tool == "pen"
        && s.draw_brush == pen.tool
        && s.draw_color.eq_ignore_ascii_case(&pen.color)
        && (s.draw_width - pen.width).abs() < 0.01
        && s.draw_effect == pen.effect.clone().unwrap_or_default()
    {
        // Deleting the pen in hand puts the default pen back in hand.
        s.draw_brush = "pen".into();
        s.draw_effect.clear();
        s.draw_color = "#1F5FC9".into();
        s.draw_width = *widths_mm(InkTool::Pen).first().unwrap_or(&0.5);
    }
    Ok(json!({"removed": true}))
}

fn eraser(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.draw_tool != "eraser");
    s.draw_tool = if on { "eraser".into() } else { String::new() };
    Ok(json!({"tool": s.draw_tool}))
}

fn parse_points(t: &str) -> Vec<[f32; 2]> {
    t.split(';').filter_map(|pt| pt.split_once(',').and_then(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?]))).collect()
}

fn stroke(s: &mut Session, p: &Json) -> Result<Json> {
    let raw = p.get("points").and_then(Json::as_array).ok_or_else(|| bad("draw.stroke", "points must be an array of [x, y] pairs"))?;
    // Reject a bad point instead of dropping it: a filtered point would shift every pressure
    // value past it onto the wrong place.
    let mut pts: Vec<[f64; 2]> = Vec::with_capacity(raw.len().min(MAX_INK_POINTS));
    for q in raw {
        let (Some(x), Some(y)) = (q.get(0).and_then(Json::as_f64), q.get(1).and_then(Json::as_f64)) else {
            return Err(bad("draw.stroke", "every point must be a pair of numbers"));
        };
        if !x.is_finite() || !y.is_finite() || x.abs() > MAX_STROKE_COORD || y.abs() > MAX_STROKE_COORD {
            return Err(bad("draw.stroke", format!("a stroke point is outside the sheet (|x| and |y| at most {MAX_STROKE_COORD})")));
        }
        if pts.len() == MAX_INK_POINTS {
            return Err(bad("draw.stroke", format!("a stroke holds at most {MAX_INK_POINTS} points")));
        }
        pts.push([x, y]);
    }
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
        Some(e) => effect_param("draw.stroke", e)?,
        None => parse_effect(&s.draw_effect),
    };
    // An invalid colour is refused, as `draw.pen`, `draw.addPen` and `draw.updatePen` do; only a
    // missing one falls back.
    let color = match str_param(p, "color") {
        Some(c) => Some(parse_color(c).ok_or_else(|| bad("draw.stroke", format!("'{c}' is not a #RRGGBB colour")))?),
        None => None,
    };
    let fallback = if s.draw_color.is_empty() { "#1F5FC9" } else { s.draw_color.as_str() };
    let line = stroke_color(color, effect, fallback);
    let width_mm = match width_mm("draw.stroke", p)? {
        Some(w) => w,
        None => {
            let w = if s.draw_width <= 0.0 { *widths_mm(tool).first().unwrap_or(&0.5) } else { s.draw_width };
            w.clamp(0.05, 20.0)
        }
    };
    let (width_cm, height_cm) = ink_size_cm(tool, width_mm);
    // Pressure is per point: a non-number is an error, not a dropped entry (which would shift
    // every later value onto an earlier point), and extra entries are an error too.
    let pressure: Vec<u16> = match p.get("pressure") {
        None => Vec::new(),
        Some(v) => {
            let arr = v.as_array().ok_or_else(|| bad("draw.stroke", "pressure must be an array of numbers"))?;
            if arr.len() > pts.len() {
                return Err(bad("draw.stroke", "pressure has more entries than the stroke has points"));
            }
            arr.iter()
                .map(|x| {
                    x.as_f64()
                        .filter(|v| v.is_finite())
                        .map(|v| v.clamp(0.0, 32767.0).round() as u16)
                        .ok_or_else(|| bad("draw.stroke", "every pressure entry must be a number"))
                })
                .collect::<Result<Vec<_>>>()?
        }
    };

    let d = s.doc()?;
    let si = d.wb.active_sheet();
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let cell = CellRef::new(sh.row_at(miny), sh.col_at(minx));
    let dx = (minx - sh.col_left(cell.col)) as f32;
    let dy = (miny - sh.row_top(cell.row)) as f32;
    let text = pts.iter().map(|q| format!("{:.1},{:.1}", q[0] - minx, q[1] - miny)).collect::<Vec<_>>().join(";");
    let order = sh.shapes.iter().filter_map(|x| x.ink.as_ref()).map(|i| i.order).max().map_or(0, |o| o.saturating_add(1));
    // A reopened workbook holds keys minted by an earlier process, whose counter started at zero,
    // so a freshly minted key can be one the workbook already uses: mint past those. A set, not a
    // vector, so a shape-heavy sheet does not make every stroke quadratic.
    let held: std::collections::HashSet<&str> =
        d.wb.sheets.iter().flat_map(|sheet| sheet.shapes.iter()).filter_map(|x| x.ink.as_ref()).filter_map(|i| i.key.as_deref()).collect();
    let key = gridcraft_model::ink_creation_id_free(|k| held.contains(k));
    let id = d.wb.next_object_id();
    let px_width = cm_to_px(width_cm).round().clamp(1.0, 255.0) as u8;
    // The texture's phase follows the stroke's place on the sheet (Excel's anchorX/anchorY are
    // negative multiples of the texture's cell); scaleFactor 0.5 is Excel's default.
    let anchor = [-minx as f32, -miny as f32];
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
        ink.ink = None;
        ink.fill = if kind == ShapeKind::Line { color } else { Color::Auto };
        ink.text = String::new();
        Ok(json!({"kind": format!("{kind:?}")}))
    })
}
