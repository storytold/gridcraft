use gridcraft_brush::{MAX_SAMPLES, Preset, Raster, Sample, Style};
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS};
use gridcraft_model::{Anchor, AnchorMode, Image, Sheet};
use serde::Deserialize;

use super::{CommandSpec, EngineError, Json, Result, Session, bad, bool_param, cmd, edit, has_doc, json, u32_param};

const NEW_STROKE: &str = "draw.brushStroke";
const EDIT_STROKE: &str = "draw.brushEdit";

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "draw.brush", "PhotoCraft Brush", ["Draw", "Tools"], None,
            "{on?: bool, preset?: hardRound|softRound|chalk|highlighter, style?: {preset,color:[r,g,b],size,hardness,opacity,flow,smoothing,pressureSize,seed}}",
            has_doc, select),
        cmd!(
            "draw.brushStroke",
            "Brush Stroke",
            [],
            None,
            "{samples: [{x,y,pressure?,tiltX?,tiltY?,timeMs?}], style?: BrushStyle} — sheet coordinates, one undo entry",
            has_doc,
            stroke
        ),
        cmd!("draw.brushEdit", "Edit Brush Stroke", [], None, "{id: image id, style: BrushStyle} — restyle a saved brush stroke", has_doc, restyle),
        cmd!(query "draw.brushInspect", "Inspect Brush Stroke", [], None, "{id: image id} — brush source and settings", has_doc, inspect),
    ]
}

fn parse_style(p: &Json, default: Style, command: &str) -> Result<Style> {
    p.get("style")
        .map_or(Ok(default), |value| Style::deserialize(value).map_err(|e| bad(command, e.to_string())))?
        .validate()
        .map_err(|e| bad(command, e.to_string()))
}

fn select(s: &mut Session, p: &Json) -> Result<Json> {
    let preset = p.get("preset").map(Preset::deserialize).transpose().map_err(|e| bad("draw.brush", e.to_string()))?;
    let style = parse_style(p, preset.map_or(s.draw_brush, Style::for_preset), "draw.brush")?;
    let on = bool_param(p, "on").unwrap_or(p.get("style").is_some() || preset.is_some() || s.draw_tool != "brush");
    s.draw_tool = if on { "brush".into() } else { String::new() };
    s.draw_brush = style;
    Ok(json!({"tool": s.draw_tool, "style": style}))
}

fn stroke(s: &mut Session, p: &Json) -> Result<Json> {
    let style = parse_style(p, s.draw_brush, NEW_STROKE)?;
    let raw = p.get("samples").and_then(Json::as_array).ok_or_else(|| bad(NEW_STROKE, "missing samples array"))?;
    if raw.is_empty() || raw.len() > MAX_SAMPLES {
        return Err(bad(NEW_STROKE, "expected 1 to 4096 samples"));
    }
    let samples: Vec<Sample> =
        raw.iter().map(Sample::deserialize).collect::<std::result::Result<_, _>>().map_err(|e| bad(NEW_STROKE, e.to_string()))?;
    let doc = s.doc()?;
    let index = doc.wb.active_sheet();
    let sheet = doc.wb.sheet(index).ok_or(EngineError::NoDocument)?;
    check_sheet(sheet, NEW_STROKE)?;
    let (right, bottom) = (sheet.col_left(MAX_COLS), sheet.row_top(MAX_ROWS));
    if samples.iter().any(|v| !(0.0..right).contains(&v.x) || !(0.0..bottom).contains(&v.y)) {
        return Err(bad(NEW_STROKE, "sample outside the worksheet"));
    }
    let first = samples.first().ok_or_else(|| bad(NEW_STROKE, "missing first sample"))?;
    let origin = [first.x, first.y];
    let placement = Placement { origin, scale: [1.0; 2], mode: AnchorMode::Absolute, mirrored: sheet.right_to_left };
    let (anchor, data) = raster_picture(sheet, placement, style, &samples, NEW_STROKE)?;
    let id = doc.wb.next_object_id();
    let picture = Image { id, anchor, data: data.into(), mime: "image/png".into(), alt: "Brush stroke".into() };
    edit(s, |cx| {
        cx.sheet_mut(index)?.images.push(picture);
        Ok(json!({"image": id}))
    })
}

fn inspect(s: &mut Session, p: &Json) -> Result<Json> {
    let doc = s.doc()?;
    let sheet = doc.wb.sheet(doc.wb.active_sheet()).ok_or(EngineError::NoDocument)?;
    let (_, source) = find_source(sheet, p, "draw.brushInspect")?;
    Ok(json!(source))
}

fn restyle(s: &mut Session, p: &Json) -> Result<Json> {
    let doc = s.doc()?;
    let index = doc.wb.active_sheet();
    let sheet = doc.wb.sheet(index).ok_or(EngineError::NoDocument)?;
    check_sheet(sheet, EDIT_STROKE)?;
    let (picture, source) = find_source(sheet, p, EDIT_STROKE)?;
    if p.get("style").is_none() {
        return Err(bad(EDIT_STROKE, "missing brush style"));
    }
    let style = parse_style(p, source.style, EDIT_STROKE)?;
    let a = picture.anchor;
    let id = picture.id;
    if style == source.style {
        return Ok(json!({"image": id}));
    }
    let units = [f64::from(a.width) / f64::from(source.size[0]), f64::from(a.height) / f64::from(source.size[1])];
    if !units.iter().all(|v| v.is_finite() && (0.0..=10_000.0).contains(v) && *v > 0.0) {
        return Err(bad(EDIT_STROKE, "invalid brush picture size"));
    }
    let origin = [
        sheet.col_left(a.cell.col) + f64::from(a.dx) - f64::from(source.offset[0]) * units[0],
        sheet.row_top(a.cell.row) + f64::from(a.dy) - f64::from(source.offset[1]) * units[1],
    ];
    let scale = units.map(|u| u * source.pixels_per_unit);
    let placement = Placement { origin, scale, mode: a.mode, mirrored: source.mirrored };
    let (anchor, data) = raster_picture(sheet, placement, style, &source.samples, EDIT_STROKE)?;
    edit(s, |cx| {
        let picture =
            cx.sheet_mut(index)?.images.iter_mut().find(|pic| pic.id == id).ok_or_else(|| bad(EDIT_STROKE, "brush stroke no longer exists"))?;
        picture.anchor = anchor;
        picture.data = data.into();
        Ok(json!({"image": id}))
    })
}

fn find_source<'a>(sheet: &'a Sheet, p: &Json, command: &str) -> Result<(&'a Image, gridcraft_brush::Source)> {
    let id = u32_param(p, "id").ok_or_else(|| bad(command, "missing image id"))?;
    let image = sheet.images.iter().find(|pic| pic.id == id).ok_or_else(|| bad(command, "no such picture"))?;
    let source = gridcraft_brush::source(&image.data).ok_or_else(|| bad(command, "editable brush data is unavailable"))?;
    Ok((image, source))
}

fn check_sheet(sheet: &Sheet, command: &str) -> Result<()> {
    if sheet.is_protected() { Err(bad(command, "the worksheet is protected")) } else { Ok(()) }
}

struct Placement {
    origin: [f64; 2],
    /// The image may have been resized after drawing. Keep that resize when its style changes.
    scale: [f64; 2],
    mode: AnchorMode,
    mirrored: bool,
}

fn raster_picture(sheet: &Sheet, placement: Placement, style: Style, samples: &[Sample], command: &str) -> Result<(Anchor, Vec<u8>)> {
    let Placement { origin, scale, mode, mirrored } = placement;
    let mut raster = gridcraft_brush::render(style, samples).map_err(|e| bad(command, e.to_string()))?;
    let units = scale.map(|factor| factor / raster.pixels_per_unit);
    raster.crop_origin(origin, units).map_err(|e| bad(command, e.to_string()))?;
    let anchor = anchor(sheet, origin, units, mode, &raster);
    if anchor.width > 10_000.0 || anchor.height > 10_000.0 {
        return Err(bad(command, "brush picture exceeds the worksheet object size limit"));
    }
    if mirrored {
        raster.mirror_x();
    }
    let png = gridcraft_brush::encode(&raster, style, samples, mirrored).map_err(|e| bad(command, e.to_string()))?;
    Ok((anchor, png))
}

fn anchor(sheet: &Sheet, origin: [f64; 2], units: [f64; 2], mode: AnchorMode, raster: &Raster) -> Anchor {
    let x = (origin[0] + f64::from(raster.offset[0]) * units[0]).max(0.0);
    let y = (origin[1] + f64::from(raster.offset[1]) * units[1]).max(0.0);
    let cell = CellRef::new(sheet.row_at(y), sheet.col_at(x));
    Anchor {
        cell,
        dx: (x - sheet.col_left(cell.col)) as f32,
        dy: (y - sheet.row_top(cell.row)) as f32,
        width: (f64::from(raster.size[0]) * units[0]) as f32,
        height: (f64::from(raster.size[1]) * units[1]) as f32,
        mode,
    }
}
