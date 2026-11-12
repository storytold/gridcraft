//! Native Office ink: the InkML part behind a drawing's `xdr:contentPart`, and the PNG fallback
//! Excel requires beside it.
//!
//! Excel stores a Draw-tab stroke as `mc:AlternateContent`: the 2016-ink choice is an
//! `xdr:contentPart` whose `customXml` relationship names an InkML part, and the fallback is a
//! picture of the rendered stroke. Excel refuses a contentPart without a fallback, so the PNG is
//! mandatory. The InkML part is written in the shape Excel writes (context, traceFormat,
//! channelProperties, brush, trace) with the W3C InkML difference encoding.
//!
//! GridCraft-only brush data (the pen subtype Excel cannot represent and the draw order Ink Replay
//! needs) rides in two extra `brushProperty` names. Excel drops unknown brush properties on save
//! and never rejects them, so they cost nothing when Excel rewrites the file.
//!
//! A part without `<inkml:timestamp>` is accepted by Excel (observed on 2026-11-07) and is what we
//! write: the model has no per-stroke time, and a clock would make saves non-deterministic.

use std::collections::HashSet;
use std::sync::Arc;

use gridcraft_model::{Color, InkEffect, InkStyle, InkTool, Shape, ShapeKind, Sheet, Theme};

use super::emu_px;
use crate::package::{MAX_PART, Rel, normalize};
use crate::read::Ctx;
use crate::tables::EMU_PER_PX;
use crate::xml::{self, El, esc_attr};

/// Content type of an InkML part (`[Content_Types].xml`).
pub(crate) const CT: &str = "application/inkml+xml";
/// Relationship kind of a drawing's content part.
pub(crate) const REL_CUSTOM_XML: &str = "customXml";
/// InkML namespace.
const NS: &str = "http://www.w3.org/2003/InkML";
/// 1 cm = 37.7952756 px at 96 dpi.
const PX_PER_CM: f64 = 96.0 / 2.54;
/// One px in the trace's 1/1000 cm units.
const THOUSANDTHS_PER_PX: f64 = 1000.0 / PX_PER_CM;
/// Most points a stroke may hold (the same cap as the legacy polyline writer).
pub(crate) const MAX_POINTS: usize = 20_000;
/// Most traces one InkML part may hold: a mark, not a whole drawing.
const MAX_TRACES: usize = 1_024;
/// Deepest `<traceGroup>` nesting read: a group names a brush, not a whole document tree.
const MAX_TRACE_DEPTH: u8 = 8;
/// Largest coordinate magnitude in the trace's units (Excel's traceFormat min/max).
const MAX_COORD: i64 = 2_147_483_647;
/// Largest coordinate accepted in sheet points (the same bound as the legacy reader).
const MAX_SHEET: f64 = 1e7;
/// Longest edge of the fallback picture, in px.
const MAX_PNG: f64 = 1024.0;
/// The most pixels all fallback pictures in one save may rasterise. The picture is a fallback
/// Excel shows when it cannot render the InkML, not art: past this budget a stroke gets a 1×1
/// transparent placeholder, so a workbook of thousands of long strokes cannot make saving
/// quadratic in the stroke count.
pub(crate) const MAX_FALLBACK_PIXELS_PER_SAVE: u64 = 64 * 1024 * 1024;
/// Pressure channel's maximum.
const MAX_PRESSURE: i64 = 32_767;

// ---------------------------------------------------------------- reading

/// The parts declared as `application/inkml+xml` in `[Content_Types].xml`, normalized.
///
/// A contentPart is only trusted when its part carries that type; anything else is not ink.
pub(super) fn declared_parts(cx: &mut Ctx<'_>) -> Option<HashSet<String>> {
    let bytes = match cx.pkg.read_limited("[Content_Types].xml", 16 * 1024 * 1024) {
        Ok(Some(b)) => b,
        _ => return None,
    };
    let root = xml::parse(&bytes).ok()?;
    Some(root.kids("Override").filter(|o| o.attr("ContentType") == Some(CT)).filter_map(|o| o.attr("PartName").map(normalize)).collect())
}

/// The parts `[Content_Types].xml` declares as InkML, read at most once per package: it is a
/// whole-part decompression and parse, so a workbook with many drawings must not repeat it for
/// every one of them (each read also spends the package's decompression budget).
pub(super) fn ink_parts(cx: &mut Ctx<'_>) -> Arc<HashSet<String>> {
    if let Some(parts) = &cx.inkml_parts {
        return Arc::clone(parts);
    }
    #[cfg(test)]
    {
        cx.report.content_types_reads += 1;
    }
    let parts = Arc::new(declared_parts(cx).unwrap_or_default());
    cx.inkml_parts = Some(Arc::clone(&parts));
    parts
}

/// Reads the InkML part behind `content_part` into editable ink shapes: one per `<trace>`, since
/// an InkML trace is a stroke. Empty when the relationship, the part, its content type or the
/// markup cannot be used; the caller warns and falls back to the picture.
pub(super) fn read(
    cx: &mut Ctx<'_>,
    drawing: &El,
    rels: &[Rel],
    content_part: &El,
    sheet: &Sheet,
    anchor: gridcraft_model::Anchor,
    declared: &HashSet<String>,
) -> Vec<Shape> {
    read_traces(cx, drawing, rels, content_part, sheet, anchor, declared).unwrap_or_default()
}

/// Every trace below `el` in document order: a bare `<trace>` and the traces inside nested
/// `<traceGroup>`s, to a bounded depth. False when the part holds too many traces or nests too
/// deeply to be a mark.
fn collect_traces<'a>(el: &'a El, depth: u8, out: &mut Vec<&'a El>) -> bool {
    if depth > MAX_TRACE_DEPTH {
        return false;
    }
    for child in &el.children {
        match child.name.as_str() {
            "trace" => {
                out.push(child);
                if out.len() > MAX_TRACES {
                    return false;
                }
            }
            "traceGroup" if !collect_traces(child, depth + 1, out) => return false,
            _ => {}
        }
    }
    true
}

/// The traces of a part in document order. A part with more than [`MAX_TRACES`] strokes is
/// refused: it is a drawing, not a mark.
fn traces(root: &El) -> Option<Vec<&El>> {
    let mut out = Vec::new();
    if !collect_traces(root, 0, &mut out) || out.is_empty() {
        return None;
    }
    Some(out)
}

/// The ink shapes a drawing's InkML part holds, anchored where `anchor` says: `None` when the
/// part is missing, undeclared or unreadable, so the caller falls back to the picture.
fn read_traces(
    cx: &mut Ctx<'_>,
    drawing: &El,
    rels: &[Rel],
    content_part: &El,
    sheet: &Sheet,
    anchor: gridcraft_model::Anchor,
    declared: &HashSet<String>,
) -> Option<Vec<Shape>> {
    let rid = content_part.attr("id")?;
    let rel = rels.iter().find(|r| r.id == rid && !r.external)?;
    if rel.kind != REL_CUSTOM_XML || !declared.contains(&normalize(&rel.target)) {
        return None;
    }
    // The part is optional: the fallback picture is read after it, so a read that fails partway is
    // charged to this part's allowance rather than ending the package's budget.
    let bytes = match cx.pkg.read_optional_limited(&rel.target, MAX_PART) {
        Ok(Some(b)) => b,
        _ => return None,
    };
    let root = xml::parse(&bytes).ok()?;
    if root.name != "ink" {
        return None;
    }
    let definitions = root.child("definitions")?;
    let (style, color) = brush(definitions);
    let anchor = anchor_for(content_part, anchor, sheet)?;
    let width_px = (style.width_cm * PX_PER_CM as f32).clamp(1.0, 255.0).round();
    let fill = Color::Rgb((width_px as u32) << 16);
    // The part's GUID names the first stroke. Later traces mint ids no earlier stroke holds
    // (`cx.ink_keys`), and past the GUIDs of parts not yet read (`cx.ink_reserved`), so two
    // parts never hand out the same new id. Reservations live apart from assignments: claiming
    // this part's own GUID consumes its reservation but can never take a key an earlier stroke
    // already owns — that `insert` is false (a copied sheet, another tool's file), and this
    // part's first trace mints a free one instead.
    reserve_other_part_guids(drawing, content_part, &mut cx.ink_reserved);
    let part_key = creation_id_of(content_part).filter(|key| {
        cx.ink_reserved.remove(key);
        cx.ink_keys.insert(key.clone())
    });
    let mut total = 0usize;
    let mut shapes = Vec::new();
    for (i, trace) in traces(&root)?.into_iter().enumerate() {
        let (points, pressure) = decode(trace, definitions)?;
        total = total.checked_add(points.len())?;
        if total > MAX_POINTS {
            return None;
        }
        let mut image = style.clone();
        // The part's GUID names the first stroke and is kept for the hidden sheet's extras lookup;
        // a part without a valid GUID, and every later trace in one part, gets a free key past
        // every held and reserved id, so a gridcraft save writes one part per stroke without
        // repeating an id.
        image.key = match (i, part_key.clone()) {
            (0, Some(first)) => Some(first),
            _ => Some(mint_ink_key(&mut cx.ink_keys, &cx.ink_reserved)),
        };
        image.pressure = pressure;
        // The part carries one brush; successive strokes keep their drawing order.
        image.order = style.order.saturating_add(i as u32);
        let text = points.iter().map(|p| format!("{},{}", p[0] as f32, p[1] as f32)).collect::<Vec<_>>().join(";");
        shapes.push(Shape {
            id: 0,
            kind: ShapeKind::Ink,
            anchor,
            // The legacy red-channel width, for readers that do not know `ink`.
            fill,
            line: color,
            text,
            format: Default::default(),
            math: Default::default(),
            ink: Some(image),
        });
    }
    Some(shapes)
}

/// The ink's box from `xdr14:xfrm` (`a:off`/`a:ext`, EMU). Excel writes the same box in the
/// anchor's `from`/`to`, so `base` is the fallback when the transform is missing or unusable. A
/// non-zero `off` is the absolute sheet position; an all-zero one is relative to the anchor.
fn anchor_for(content_part: &El, base: gridcraft_model::Anchor, sheet: &Sheet) -> Option<gridcraft_model::Anchor> {
    let Some(xf) = content_part.child("xfrm") else { return Some(base) };
    let ext = xf.child("ext");
    let width = ext.map(|e| emu_px(e.attr_i64("cx"))).unwrap_or(0.0);
    let height = ext.map(|e| emu_px(e.attr_i64("cy"))).unwrap_or(0.0);
    let off = xf.child("off");
    let x = off.map(|o| emu_px(o.attr_i64("x"))).unwrap_or(0.0);
    let y = off.map(|o| emu_px(o.attr_i64("y"))).unwrap_or(0.0);
    let (cell, dx, dy) = if x > 0.0 || y > 0.0 {
        let (x, y) = (x.max(0.0) as f64, y.max(0.0) as f64);
        let (col, row) = (sheet.col_at(x), sheet.row_at(y));
        (gridcraft_core::CellRef::new(row, col), (x - sheet.col_left(col)) as f32, (y - sheet.row_top(row)) as f32)
    } else {
        (base.cell, base.dx, base.dy)
    };
    Some(gridcraft_model::Anchor {
        cell,
        dx,
        dy,
        width: if width > 0.0 { width } else { base.width },
        height: if height > 0.0 { height } else { base.height },
        mode: base.mode,
    })
}

/// The `a16:creationId` GUID a contentPart carries, bounded to a plausible GUID.
fn creation_id_of(content_part: &El) -> Option<String> {
    let id = content_part.path(&["nvContentPartPr", "cNvPr", "extLst", "ext", "creationId"])?.attr("id")?;
    // ST_Guid or nothing: a malformed id is not an identity, and must never be written back.
    gridcraft_model::ink_guid(id)
}

/// Reserves the GUIDs of the *other* parts in this drawing before any trace mints a key, so an
/// unkeyed trace in an earlier-read part cannot mint the GUID a later-read part already carries.
/// Parts are read in document order, but their GUIDs are all visible up front; the part's own
/// GUID stays for the `insert` at the read site, which treats an already-held GUID (a copied
/// sheet, another tool's file) as absent. Parts are matched by their relationship id.
fn reserve_other_part_guids(drawing: &El, content_part: &El, reserved: &mut HashSet<String>) {
    let here = content_part.attr("id");
    for anchor in &drawing.children {
        for obj in &anchor.children {
            let Some(choice) = (obj.name == "AlternateContent").then(|| obj.child("Choice")).flatten() else { continue };
            let Some(part) = choice.child("contentPart") else { continue };
            if part.attr("id") == here {
                continue;
            }
            if let Some(key) = creation_id_of(part) {
                reserved.insert(key);
            }
        }
    }
}

/// One channel's difference-decoding state: its last value, its last first difference, and the
/// difference order written for that value. A `*` stores nothing here: it repeats the value and
/// changes no order.
#[derive(Clone, Copy, Default)]
struct Channel {
    value: i64,
    diff: i64,
    prefix: Option<char>,
}

/// The decoded points (sheet points relative to the ink's origin) and pressure values, from the
/// W3C InkML trace encoding: `!` explicit, `'` first difference, `"` second difference, `*` no
/// change, and no prefix inheriting the previous prefix of that channel.
fn decode(trace: &El, definitions: &El) -> Option<(Vec<[f64; 2]>, Vec<u16>)> {
    let context = referenced(definitions, "context", trace.attr("contextRef"))?;
    let format = context.child("inkSource")?.child("traceFormat")?;
    let channels: Vec<&El> = format.kids("channel").collect();
    if channels.is_empty() || channels.len() > 16 {
        return None;
    }
    let mut names = Vec::with_capacity(channels.len());
    let mut res = Vec::with_capacity(channels.len());
    let props = context.child("inkSource").and_then(|s| s.child("channelProperties"));
    for ch in &channels {
        let name = ch.attr("name")?;
        if !matches!(ch.attr("type").unwrap_or("integer"), "integer" | "real" | "decimal") {
            return None;
        }
        names.push(name);
        let resolution = props
            .and_then(|p| {
                p.kids("channelProperty")
                    .find(|c| c.attr("channel") == Some(name) && c.attr("name") == Some("resolution"))
                    .and_then(|c| c.attr_f64("value"))
            })
            .filter(|v| v.is_finite() && *v > 0.0 && *v <= 1e9)
            .unwrap_or(1000.0);
        res.push(resolution);
    }
    let (xi, yi) = (names.iter().position(|n| *n == "X")?, names.iter().position(|n| *n == "Y")?);
    let fi = names.iter().position(|n| *n == "F");

    let mut state = vec![Channel::default(); channels.len()];
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut pressure: Vec<u16> = Vec::new();
    for point in trace.text.split(',') {
        if point.trim().is_empty() {
            continue;
        }
        let mut rest = point;
        let mut values = Vec::with_capacity(channels.len());
        for st in state.iter_mut() {
            let (written, raw, tail) = scan_value(rest)?;
            rest = tail;
            if written == Some('*') {
                // `*` repeats the value and leaves the channel's difference order as it was, so the
                // first difference is nought from here on and a later unprefixed value still
                // applies its own number to it.
                st.diff = 0;
                if st.value.unsigned_abs() > MAX_COORD as u64 {
                    return None;
                }
                values.push(st.value);
                continue;
            }
            // A value with no prefix of its own carries the channel's previous difference order.
            let prefix = written.or(st.prefix).unwrap_or('!');
            let raw = raw?;
            let value = match prefix {
                '!' => raw,
                '\'' => st.value.checked_add(raw)?,
                '"' => st.value.checked_add(st.diff.checked_add(raw)?)?,
                _ => return None,
            };
            // `unsigned_abs` so i64::MIN cannot panic or wrap past the cap.
            if value.unsigned_abs() > MAX_COORD as u64 {
                return None;
            }
            let diff = match prefix {
                '!' => {
                    let d = value.checked_sub(st.value)?;
                    if st.prefix.is_none() { 0 } else { d }
                }
                '\'' => raw,
                '"' => st.diff.checked_add(raw)?,
                _ => st.diff,
            };
            *st = Channel { value, diff, prefix: Some(prefix) };
            values.push(value);
        }
        if !rest.bytes().all(is_wsp) || points.len() == MAX_POINTS {
            return None;
        }
        let x = values.get(xi).copied().unwrap_or(0) as f64 / res.get(xi).copied().unwrap_or(1000.0) * PX_PER_CM;
        let y = values.get(yi).copied().unwrap_or(0) as f64 / res.get(yi).copied().unwrap_or(1000.0) * PX_PER_CM;
        if !x.is_finite() || !y.is_finite() || x.abs() > MAX_SHEET || y.abs() > MAX_SHEET {
            return None;
        }
        points.push([x, y]);
        if let Some(fi) = fi {
            pressure.push(values.get(fi).copied().unwrap_or(0).clamp(0, MAX_PRESSURE) as u16);
        }
    }
    if points.is_empty() {
        return None;
    }
    Some((points, pressure))
}

/// An ASCII whitespace byte, as the trace grammar allows it between values.
fn is_wsp(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// Scans one value of a trace point as the InkML grammar allows it: an optional difference-order
/// prefix (`!`, `'`, `"` or `*`), optional whitespace, an optional minus sign and the digits
/// (`value ::= difference_order? wsp* "-"? wsp* integer`). Whitespace before a value is optional,
/// so Excel's compact `'2117'529` holds two values; a prefix, a sign or whitespace separates them.
/// Returns the prefix (when written), its number (`None` for `*`) and the rest of the point.
fn scan_value(s: &str) -> Option<(Option<char>, Option<i64>, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_wsp(bytes[i]) {
        i += 1;
    }
    let prefix = match bytes.get(i) {
        Some(b @ (b'!' | b'\'' | b'"' | b'*')) => {
            i += 1;
            Some(*b as char)
        }
        _ => None,
    };
    while i < bytes.len() && is_wsp(bytes[i]) {
        i += 1;
    }
    let rest = s.get(i..)?;
    if prefix == Some('*') {
        return Some((prefix, None, rest));
    }
    let b = rest.as_bytes();
    let neg = b.first() == Some(&b'-');
    let mut j = usize::from(neg);
    while j < b.len() && is_wsp(b[j]) {
        j += 1;
    }
    let digits = j;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j == digits {
        return None;
    }
    // Parse the sign with the digits (keeps i64::MIN representable); whitespace between them,
    // which Excel never writes, needs a tiny copy.
    let n = if digits == usize::from(neg) {
        rest.get(..j)?.parse::<i64>().ok()?
    } else {
        let mut t = String::with_capacity(j - digits + 1);
        if neg {
            t.push('-');
        }
        t.push_str(rest.get(digits..j)?);
        t.parse::<i64>().ok()?
    };
    Some((prefix, Some(n), rest.get(j..)?))
}

/// The definition element (`context`, `brush`) named by a `#id` reference.
fn referenced<'a>(definitions: &'a El, name: &'a str, reference: Option<&str>) -> Option<&'a El> {
    let id = reference?.trim().strip_prefix('#')?;
    definitions.kids(name).find(|e| e.attr("id") == Some(id))
}

/// The brush's style and colour. Excel writes one `brush` per part; missing properties take the
/// defaults a plain Pen has.
fn brush(definitions: &El) -> (InkStyle, Color) {
    let mut style = InkStyle::default();
    let mut color = Color::rgb(0, 0, 0);
    let mut gridcraft_tool = None;
    let mut ink_effects = None;
    let mut mask_pen = false;
    // Whether the file gave each dimension: `InkStyle::default` has a height, so an absent
    // `height` must not look like a stated one.
    let mut width = None;
    let mut height = None;
    let Some(brush) = definitions.child("brush") else { return (style, color) };
    for p in brush.kids("brushProperty") {
        let (Some(name), Some(value)) = (p.attr("name"), p.attr("value")) else { continue };
        match name {
            "width" => width = brush_cm(value).or(width),
            "height" => height = brush_cm(value).or(height),
            "color" => color = Color::from_hex(value).unwrap_or(color),
            "tip" => mask_pen |= value.trim() == "rectangle",
            "rasterOp" => mask_pen |= value.trim() == "maskPen",
            "inkEffects" => ink_effects = Some(value.trim().to_ascii_lowercase()),
            "anchorX" => style.anchor[0] = finite(value).unwrap_or(0.0),
            "anchorY" => style.anchor[1] = finite(value).unwrap_or(0.0),
            "scaleFactor" => style.scale = finite(value).filter(|v| *v > 0.0).unwrap_or(1.0),
            "gridcraftOrder" => style.order = value.trim().parse::<u32>().unwrap_or(0),
            "gridcraftTool" => gridcraft_tool = Some(value.trim().to_ascii_lowercase()),
            _ => {}
        }
    }
    // A brush with no height is as tall as it is wide (InkML's default), and one with no width
    // takes the plain pen's. Without this a width-0.35 brush read back as the model's 0.05 height.
    let width = width.unwrap_or(style.width_cm);
    style.width_cm = width;
    style.height_cm = height.unwrap_or(width);
    style.effect = ink_effects.as_deref().and_then(effect);
    // Excel writes an F (pressure) channel for every plain pen, so it says nothing about the
    // tool: Fountain Pen and Brush Pen are indistinguishable in an Excel file. Without a
    // GridCraft `gridcraftTool`, an inkEffects pencil is a Pencil, a rectangle maskPen is a
    // Highlighter, a texture is a Pen with that effect, and everything else is a Pen.
    style.tool = match gridcraft_tool.as_deref() {
        Some("pencil") => InkTool::Pencil,
        Some("highlighter") => InkTool::Highlighter,
        Some("fountainPen") | Some("fountainpen") => InkTool::FountainPen,
        Some("brushPen") | Some("brushpen") => InkTool::BrushPen,
        Some(_) => InkTool::Pen,
        None if ink_effects.as_deref() == Some("pencil") => InkTool::Pencil,
        None if mask_pen => InkTool::Highlighter,
        _ => InkTool::Pen,
    };
    (style, color)
}

/// A brush dimension in cm, clamped to the range Excel accepts; a value that is not a finite
/// number leaves the default in place.
fn brush_cm(v: &str) -> Option<f32> {
    finite(v).map(|v| v.clamp(gridcraft_model::MIN_INK_CM, gridcraft_model::MAX_INK_CM))
}

/// The first finite `f32` in `v`, ignoring surrounding whitespace.
fn finite(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// The pen texture an `inkEffects` name stands for; an unknown name is a solid stroke.
fn effect(name: &str) -> Option<InkEffect> {
    match name {
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

// ---------------------------------------------------------------- writing

/// The parts a native ink stroke needs: the InkML markup, the fallback PNG and the `creationId`.
pub(crate) struct Parts {
    pub xml: String,
    pub png: Vec<u8>,
    pub creation_id: String,
}

/// The InkML part, the fallback picture and the creation id for `shape`, or `None` when its
/// geometry cannot be represented (the caller then writes the legacy polyline). `budget` is the
/// save's remaining fallback-picture pixels, spent as pictures are rasterised.
pub(crate) fn parts(shape: &Shape, theme: &Theme, budget: &mut u64) -> Option<Parts> {
    if shape.kind != ShapeKind::Ink {
        return None;
    }
    let ink = shape.ink.as_ref()?;
    let points = parse_points(&shape.text)?;
    let pressure: Vec<i64> = if ink.pressure.len() == points.len() && !ink.pressure.is_empty() {
        ink.pressure.iter().map(|v| i64::from((*v).min(MAX_PRESSURE as u16))).collect()
    } else {
        Vec::new()
    };
    // Only recorded pressure gets an F channel: a Fountain/Pen brush with none must read back
    // with an empty pressure vector, not a run of zeros. The tool still round-trips through
    // `gridcraftTool`.
    let use_f = !pressure.is_empty();
    let color = shape.line.resolve(theme).unwrap_or([0, 0, 0]);
    let trace = encode_trace(&points, use_f.then_some(pressure.as_slice()));
    let xml = inkml_xml(ink, &color, use_f, &trace);
    let png = fallback_png(shape, ink, &points, color, budget)?;
    Some(Parts { xml, png, creation_id: creation_id(shape) })
}

/// Points from `Shape::text` (`x,y;…`, sheet points relative to the anchor), bounded.
fn parse_points(text: &str) -> Option<Vec<[i64; 2]>> {
    let mut out = Vec::new();
    for point in text.split(';') {
        let point = point.trim();
        if point.is_empty() {
            continue;
        }
        if out.len() == MAX_POINTS {
            return None;
        }
        let (x, y) = point.split_once(',')?;
        let (x, y) = (x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?);
        if !x.is_finite() || !y.is_finite() || x.abs() > MAX_SHEET || y.abs() > MAX_SHEET {
            return None;
        }
        let (x, y) = (trace_units(x)?, trace_units(y)?);
        out.push([x, y]);
    }
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// A sheet-point coordinate in the trace's 1/1000 cm units, or `None` past Excel's range.
fn trace_units(px: f64) -> Option<i64> {
    let v = (px * THOUSANDTHS_PER_PX).round();
    if !v.is_finite() || v.abs() > MAX_COORD as f64 {
        return None;
    }
    Some(v as i64)
}

/// The W3C difference encoding Excel writes: the first point explicit, the second its first
/// differences, every later point its second differences (prefixed once per channel, then
/// inherited).
fn encode_trace(points: &[[i64; 2]], pressure: Option<&[i64]>) -> String {
    let channels = 2 + usize::from(pressure.is_some());
    let mut tokens: Vec<String> = Vec::with_capacity(points.len());
    let value =
        |i: usize, c: usize| -> i64 { if c < 2 { points.get(i).map_or(0, |q| q[c]) } else { pressure.and_then(|f| f.get(i).copied()).unwrap_or(0) } };
    for i in 0..points.len() {
        let mut point = String::new();
        for c in 0..channels {
            if c > 0 {
                point.push(' ');
            }
            match i {
                0 => point.push_str(&format!("!{}", value(i, c))),
                1 => point.push_str(&format!("'{}", value(i, c).saturating_sub(value(0, c)))),
                2 => point.push_str(&format!("\"{}", second(points, pressure, c, 2))),
                _ => point.push_str(&format!("{}", second(points, pressure, c, i))),
            }
        }
        tokens.push(point);
    }
    tokens.join(",")
}

/// The second difference at index `i`: `(v[i] - v[i-1]) - (v[i-1] - v[i-2])`, saturating so a
/// point at the cap cannot wrap.
fn second(points: &[[i64; 2]], pressure: Option<&[i64]>, channel: usize, i: usize) -> i64 {
    let v = |k: usize| -> i64 {
        if channel < 2 { points.get(k).map_or(0, |q| q[channel]) } else { pressure.and_then(|f| f.get(k).copied()).unwrap_or(0) }
    };
    v(i).saturating_sub(v(i.saturating_sub(1))).saturating_sub(v(i.saturating_sub(1)).saturating_sub(v(i.saturating_sub(2))))
}

/// The whole InkML part: one context with the trace format, one brush, one trace.
fn inkml_xml(ink: &InkStyle, color: &[u8; 3], f: bool, trace: &str) -> String {
    let width = ink.width_cm.clamp(gridcraft_model::MIN_INK_CM, gridcraft_model::MAX_INK_CM);
    let height = ink.height_cm.clamp(gridcraft_model::MIN_INK_CM, gridcraft_model::MAX_INK_CM);
    let short = |v: f32| v.to_string();
    let mut props = format!(
        "<inkml:brushProperty name=\"width\" value=\"{}\" units=\"cm\"/><inkml:brushProperty name=\"height\" value=\"{}\" units=\"cm\"/><inkml:brushProperty name=\"color\" value=\"#{:02X}{:02X}{:02X}\"/>",
        short(width),
        short(height),
        color[0],
        color[1],
        color[2]
    );
    match ink.tool {
        InkTool::Pencil => props.push_str("<inkml:brushProperty name=\"ignorePressure\" value=\"1\"/>"),
        InkTool::Highlighter => props.push_str(
            "<inkml:brushProperty name=\"tip\" value=\"rectangle\"/><inkml:brushProperty name=\"rasterOp\" value=\"maskPen\"/><inkml:brushProperty name=\"ignorePressure\" value=\"1\"/>",
        ),
        _ => {}
    }
    let effect_name = ink.effect.map(effect_name);
    if ink.tool == InkTool::Pencil && effect_name.is_none() {
        props.push_str("<inkml:brushProperty name=\"inkEffects\" value=\"pencil\"/>");
    }
    let phase = ink.anchor != [0.0, 0.0] || (ink.scale - 1.0).abs() > f32::EPSILON;
    if effect_name.is_some() || phase {
        if let Some(name) = effect_name {
            props.push_str(&format!("<inkml:brushProperty name=\"inkEffects\" value=\"{name}\"/>"));
        }
        let scale = if ink.scale > 0.0 { ink.scale } else { 1.0 };
        props.push_str(&format!(
            "<inkml:brushProperty name=\"anchorX\" value=\"{}\"/><inkml:brushProperty name=\"anchorY\" value=\"{}\"/><inkml:brushProperty name=\"scaleFactor\" value=\"{}\"/>",
            short(ink.anchor[0]),
            short(ink.anchor[1]),
            short(scale)
        ));
    }
    // GridCraft's own round-trip data (Excel drops unknown brush properties on save). Only the
    // tools Excel cannot represent carry it: Fountain Pen and Brush Pen are indistinguishable
    // there, and a Pencil with a texture loses its `inkEffects=pencil` marker to the texture.
    let tool = match ink.tool {
        InkTool::FountainPen => Some("fountainPen"),
        InkTool::BrushPen => Some("brushPen"),
        InkTool::Pencil if ink.effect.is_some() => Some("pencil"),
        _ => None,
    };
    if let Some(tool) = tool {
        props.push_str(&format!("<inkml:brushProperty name=\"gridcraftTool\" value=\"{tool}\"/>"));
    }
    props.push_str(&format!("<inkml:brushProperty name=\"gridcraftOrder\" value=\"{}\"/>", ink.order));

    let mut channels = String::from(
        "<inkml:channel name=\"X\" type=\"integer\" min=\"-2.14748E9\" max=\"2.14748E9\" units=\"cm\"/><inkml:channel name=\"Y\" type=\"integer\" min=\"-2.14748E9\" max=\"2.14748E9\" units=\"cm\"/>",
    );
    let mut channel_props = String::from(
        "<inkml:channelProperty channel=\"X\" name=\"resolution\" value=\"1000\" units=\"1/cm\"/><inkml:channelProperty channel=\"Y\" name=\"resolution\" value=\"1000\" units=\"1/cm\"/>",
    );
    if f {
        channels.push_str("<inkml:channel name=\"F\" type=\"integer\" max=\"32767\" units=\"dev\"/>");
        channel_props.push_str("<inkml:channelProperty channel=\"F\" name=\"resolution\" value=\"0\" units=\"1/dev\"/>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<inkml:ink xmlns:inkml=\"{NS}\"><inkml:definitions><inkml:context xml:id=\"ctx0\"><inkml:inkSource xml:id=\"inkSrc0\"><inkml:traceFormat>{channels}</inkml:traceFormat><inkml:channelProperties>{channel_props}</inkml:channelProperties></inkml:inkSource></inkml:context><inkml:brush xml:id=\"br0\">{props}</inkml:brush></inkml:definitions><inkml:trace contextRef=\"#ctx0\" brushRef=\"#br0\">{}</inkml:trace></inkml:ink>",
        esc_attr(trace)
    )
}

/// The `inkEffects` name Excel writes for a texture.
fn effect_name(e: InkEffect) -> &'static str {
    match e {
        InkEffect::Rainbow => "rainbow",
        InkEffect::Galaxy => "galaxy",
        InkEffect::Lava => "lava",
        InkEffect::Ocean => "ocean",
        InkEffect::RoseGold => "rosegold",
        InkEffect::Gold => "gold",
        InkEffect::Silver => "silver",
        InkEffect::Bronze => "bronze",
    }
}

/// A creation id no stroke this workbook already holds uses, past held keys and reservations for
/// parts not yet read, reserved as it is minted.
pub(crate) fn mint_ink_key(held: &mut HashSet<String>, reserved: &HashSet<String>) -> String {
    let key = gridcraft_model::ink_creation_id_free(|k| held.contains(k) || reserved.contains(k));
    held.insert(key.clone());
    key
}

/// The stroke's `a16:creationId`: its stored key in canonical lowercase when it is an ST_Guid,
/// else a deterministic GUID from the shape id. Ids are unique across a workbook's shapes, and a
/// malformed key (JSON, an older model, a hand edit) is replaced rather than written, since Excel
/// requires ST_Guid and an invalid id is not an identity.
pub fn creation_id(shape: &Shape) -> String {
    if let Some(guid) = shape.ink.as_ref().and_then(|i| i.key.as_deref()).and_then(gridcraft_model::ink_guid) {
        return guid;
    }
    let id = u64::from(shape.id);
    format!("{{{id:08x}-0000-4000-8000-{id:012x}}}")
}

/// A transparent PNG of the stroke in its colour, at the anchor's aspect ratio. Excel needs it as
/// the contentPart's `mc:Fallback`. `budget` is the save's remaining pixels: a picture that does
/// not fit becomes a 1×1 transparent placeholder, so the InkML stroke still opens natively while
/// the raster work stays bounded per save.
fn fallback_png(shape: &Shape, ink: &InkStyle, points: &[[i64; 2]], color: [u8; 3], budget: &mut u64) -> Option<Vec<u8>> {
    let w = f64::from(shape.anchor.width).clamp(1.0, MAX_SHEET);
    let h = f64::from(shape.anchor.height).clamp(1.0, MAX_SHEET);
    let scale = (MAX_PNG / w.max(h)).min(1.0);
    let pw = ((w * scale).round() as u32).clamp(1, MAX_PNG as u32);
    let ph = ((h * scale).round() as u32).clamp(1, MAX_PNG as u32);
    let pixels = u64::from(pw) * u64::from(ph);
    if pixels > *budget {
        return Some(placeholder_png());
    }
    *budget -= pixels;
    let mut rgba = vec![0u8; (pw as usize) * (ph as usize) * 4];
    let radius = (f64::from(ink.width_cm) * PX_PER_CM * scale / 2.0).clamp(0.5, 24.0);
    // Every stamp is charged the square it may touch to the same budget, so the raster work one
    // save can ask for is bounded whether it comes from many strokes or from one long one.
    let stamp_pixels = ((2.0 * radius + 3.0).powi(2)).ceil() as u64;
    let pts: Vec<[f64; 2]> = points.iter().map(|p| [p[0] as f64 / THOUSANDTHS_PER_PX * scale, p[1] as f64 / THOUSANDTHS_PER_PX * scale]).collect();
    if pts.len() == 1 && stamp_pixels <= *budget {
        *budget -= stamp_pixels;
        stamp(&mut rgba, pw, ph, pts[0][0], pts[0][1], radius, color);
    }
    for pair in pts.windows(2) {
        if *budget < stamp_pixels {
            break;
        }
        let (a, b) = (pair[0], pair[1]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let steps = (len / 0.5).ceil().clamp(1.0, 512.0) as usize;
        for i in 0..=steps {
            if *budget < stamp_pixels {
                break;
            }
            *budget -= stamp_pixels;
            let t = i as f64 / steps as f64;
            stamp(&mut rgba, pw, ph, a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, radius, color);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, pw, ph);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
    }
    Some(out)
}

/// A 1×1 transparent PNG, the fallback a stroke past the save's pixel budget gets: deterministic
/// bytes, so every such stroke shares one `xl/media` part.
fn placeholder_png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 1, 1);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = enc.write_header() {
        let _ = writer.write_image_data(&[0, 0, 0, 0]);
    }
    out
}

/// A soft round dab, source-over into the transparent picture.
fn stamp(img: &mut [u8], w: u32, h: u32, cx: f64, cy: f64, r: f64, color: [u8; 3]) {
    let x0 = (cx - r - 1.0).floor().max(0.0) as u32;
    let y0 = (cy - r - 1.0).floor().max(0.0) as u32;
    let x1 = ((cx + r + 1.0).ceil() as i64).clamp(0, i64::from(w) - 1) as u32;
    let y1 = ((cy + r + 1.0).ceil() as i64).clamp(0, i64::from(h) - 1) as u32;
    if cx + r + 1.0 < 0.0 || cy + r + 1.0 < 0.0 {
        return;
    }
    for py in y0..=y1 {
        for px in x0..=x1 {
            let dx = px as f64 + 0.5 - cx;
            let dy = py as f64 + 0.5 - cy;
            let coverage = (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let i = ((py * w + px) * 4) as usize;
            let Some(pixel) = img.get_mut(i..i + 4) else { continue };
            let alpha = (f64::from(pixel[3]) / 255.0).max(coverage);
            pixel[0] = color[0];
            pixel[1] = color[1];
            pixel[2] = color[2];
            pixel[3] = (alpha * 255.0).round() as u8;
        }
    }
}

/// The `xdr14:xfrm` of a contentPart, in EMU from the sheet's origin.
pub(super) fn xfrm_xml(shape: &Shape, sheet: &Sheet) -> String {
    let a = &shape.anchor;
    let x = sheet.col_left(a.cell.col) + f64::from(a.dx);
    let y = sheet.row_top(a.cell.row) + f64::from(a.dy);
    let emu = |v: f64| ((v.max(0.0)) * EMU_PER_PX).round() as i64;
    format!(
        "<xdr14:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></xdr14:xfrm>",
        emu(x),
        emu(y),
        emu(f64::from(a.width.max(0.0))),
        emu(f64::from(a.height.max(0.0)))
    )
}
