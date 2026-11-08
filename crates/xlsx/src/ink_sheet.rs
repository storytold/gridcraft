//! The `GridCraft Ink` veryHidden worksheet: the extension channel for ink data Excel cannot
//! hold.
//!
//! Excel keeps a veryHidden worksheet and its cell values across open and save (verified against
//! Excel 365), while it drops unknown `inkml:brushProperty` names, unreferenced `customXml` parts
//! and unknown `a:ext` children. So the parts of a GridCraft ink stroke the format has no room
//! for — the imported texture a pen paints with, the scale it repeats at, and a brush subtype
//! Excel cannot tell apart (Fountain Pen and Brush Pen are identical in InkML) — travel here,
//! keyed by the stroke's `a16:creationId` GUID, which Excel does preserve.
//!
//! The sheet is written only while there is such data; a workbook whose ink Excel can represent
//! whole never gains it, so its file stays what Excel would have written.
//!
//! Layout (row-major, so a person opening the sheet sees a small table):
//!
//! ```text
//! A1  GridCraft Ink      B1  1 (format version)
//! A2  kind               B2  key        C2 name  D2 mime  E2 width  F2 height  G2 data
//! A3  texture            B3  <id>       C3 …     D3 …     E3 …      F3 …       G3.. base64 chunks
//! A4  ink                B4  <GUID>     C4 {"texture":"…","scale":0.5,"tool":"brushPen"}
//! ```
//!
//! Base64 is split under the 32,767-character cell limit. Everything read back is hostile input:
//! sizes, counts and ids are capped and re-checked through [`InkTexture::from_parts`].

use std::collections::BTreeMap;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::ink::{InkTexture, MAX_INK_TEXTURE_BYTES};
use gridcraft_model::{InkTool, Sheet, Visibility, Workbook};
use serde::{Deserialize, Serialize};

/// The sheet's name, and the marker its A1 holds so a sheet of the same name that is not ours
/// (a person's own sheet) is never mistaken for it.
pub const INK_SHEET_NAME: &str = "GridCraft Ink";
const INK_SHEET_MARKER: &str = "GridCraft Ink";
/// The layout version written in B1; only this one is read.
const INK_SHEET_VERSION: f64 = 1.0;
/// Characters of base64 per cell, under the 32,767 a cell may hold.
const CHUNK_CHARS: usize = 30_000;
/// The most textures and ink entries a sheet may declare: far beyond any real drawing, so a
/// hostile file can't make the reader allocate without bound.
const MAX_INK_ENTRIES: usize = 100_000;
/// The longest stable key (a GUID is 38 characters) and name kept from a file.
const MAX_KEY_CHARS: usize = 64;

/// The GridCraft-only data of one ink stroke, as the hidden sheet's JSON holds it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InkExtras {
    /// Id of the imported texture the stroke paints with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    /// How many sheet points one repeat of the texture covers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
    /// The GridCraft brush subtype (`fountainPen`, `brushPen`, …): Excel cannot tell the last two
    /// apart, and it drops the private `brushProperty` that would.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Draw order for Ink Replay. Excel drops the private `gridcraftOrder` brush property, so this
    /// is the only channel that survives an Excel round trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
}

impl InkExtras {
    /// Whether the record holds anything at all.
    pub fn is_empty(&self) -> bool {
        self.texture.is_none() && self.scale.is_none() && self.tool.is_none() && self.order.is_none()
    }
}

/// Everything the hidden sheet carries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InkSheetData {
    pub textures: Vec<InkTexture>,
    /// Stable stroke key (`a16:creationId`) → the stroke's GridCraft-only data.
    pub ink: BTreeMap<String, InkExtras>,
}

impl InkSheetData {
    /// Whether a save needs the sheet: no textures and no stroke extras means nothing to keep.
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty() && self.ink.is_empty()
    }
}

/// Whether `sheet` is GridCraft's own hidden ink sheet: veryHidden, named as ours (a numeric
/// suffix is possible when a sheet of that name exists), and carrying the marker in A1 (so a
/// person's sheet of that name is left alone).
pub fn is_ink_sheet(sheet: &Sheet) -> bool {
    sheet.visibility == Visibility::VeryHidden
        && sheet.name.starts_with(INK_SHEET_NAME)
        && sheet.cells.get(CellRef::new(0, 0)).and_then(|c| c.value.as_text()) == Some(INK_SHEET_MARKER)
}

/// A name for the sheet that no other sheet uses: `GridCraft Ink`, then `GridCraft Ink 2`, ….
/// The reader does not depend on the suffix (it looks for the marker), so this only keeps the
/// workbook valid.
pub fn unique_name(wb: &Workbook) -> String {
    if wb.sheet_index(INK_SHEET_NAME).is_none() {
        return INK_SHEET_NAME.to_string();
    }
    (2..).map(|n| format!("{INK_SHEET_NAME} {n}")).find(|name| wb.sheet_index(name).is_none()).unwrap_or_else(|| INK_SHEET_NAME.to_string())
}

/// Puts one value at `row`/`col` of the sheet the layout draws.
fn put(sheet: &mut Sheet, row: u32, col: u32, value: Value) {
    sheet.set_value(CellRef::new(row, col), value);
}

/// Builds the hidden sheet holding `data`. The caller adds it to the workbook it saves (and only
/// then), so the sheet exists in the file exactly while there is data for it.
pub fn build(name: &str, data: &InkSheetData) -> Sheet {
    let mut sheet = Sheet::new(name);
    sheet.visibility = Visibility::VeryHidden;
    put(&mut sheet, 0, 0, Value::text(INK_SHEET_MARKER));
    put(&mut sheet, 0, 1, Value::number(INK_SHEET_VERSION));
    for (col, title) in ["kind", "key", "name", "mime", "width", "height", "data"].iter().enumerate() {
        put(&mut sheet, 1, col as u32, Value::text(*title));
    }
    let mut row = 2;
    for texture in &data.textures {
        put(&mut sheet, row, 0, Value::text("texture"));
        put(&mut sheet, row, 1, Value::text(texture.id.as_str()));
        put(&mut sheet, row, 2, Value::text(texture.name.as_str()));
        put(&mut sheet, row, 3, Value::text(texture.mime.as_str()));
        put(&mut sheet, row, 4, Value::number(f64::from(texture.width)));
        put(&mut sheet, row, 5, Value::number(f64::from(texture.height)));
        let encoded = gridcraft_model::b64::encode(&texture.data);
        for (i, chunk) in encoded.as_bytes().chunks(CHUNK_CHARS).enumerate() {
            // The chunks are ASCII base64, so slicing the string by bytes is safe.
            let chunk = String::from_utf8_lossy(chunk);
            put(&mut sheet, row, 6 + i as u32, Value::text(chunk.as_ref()));
        }
        row += 1;
    }
    for (key, extras) in &data.ink {
        if extras.is_empty() {
            continue;
        }
        let Ok(json) = serde_json::to_string(extras) else { continue };
        put(&mut sheet, row, 0, Value::text("ink"));
        put(&mut sheet, row, 1, Value::text(key.as_str()));
        put(&mut sheet, row, 2, Value::text(json));
        row += 1;
    }
    sheet
}

/// Reads the hidden sheet back. Anything malformed is skipped rather than refused: a damaged
/// extension must never stop a workbook from opening.
pub fn parse(sheet: &Sheet) -> Option<InkSheetData> {
    if !is_ink_sheet(sheet) || sheet.cells.get(CellRef::new(0, 1)).and_then(|c| c.value.as_f64()) != Some(INK_SHEET_VERSION) {
        return None;
    }
    let mut rows: BTreeMap<u32, BTreeMap<u32, String>> = BTreeMap::new();
    let all = gridcraft_core::RangeRef::new(CellRef::new(0, 0), CellRef::new(gridcraft_core::MAX_ROWS - 1, gridcraft_core::MAX_COLS - 1));
    for (at, cell) in sheet.cells.iter_range(all) {
        if let Ok(text) = cell.value.to_text() {
            rows.entry(at.row).or_default().insert(at.col, text);
        }
    }
    let mut data = InkSheetData::default();
    for row in rows.values() {
        let text = |col: u32| row.get(&col).map(String::as_str).unwrap_or("");
        match text(0) {
            "texture" if data.textures.len() < gridcraft_model::MAX_INK_TEXTURES => {
                let Some(width) = text(4).parse::<u32>().ok() else { continue };
                let Some(height) = text(5).parse::<u32>().ok() else { continue };
                let encoded: String = row.range(6..).map(|(_, chunk)| chunk.as_str()).collect();
                if encoded.len() / 4 * 3 > MAX_INK_TEXTURE_BYTES + 3 {
                    continue;
                }
                let Some(bytes) = gridcraft_model::b64::decode(&encoded) else { continue };
                let Some(texture) = InkTexture::from_parts(text(1).to_string(), text(2), text(3), std::sync::Arc::new(bytes), width, height) else {
                    continue;
                };
                if !data.textures.iter().any(|t| t.id == texture.id) {
                    data.textures.push(texture);
                }
            }
            "ink" if data.ink.len() < MAX_INK_ENTRIES => {
                let key = text(1);
                if key.is_empty() || key.chars().count() > MAX_KEY_CHARS {
                    continue;
                }
                let Ok(extras) = serde_json::from_str::<InkExtras>(text(2)) else { continue };
                let extras = InkExtras {
                    texture: extras.texture.filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
                    scale: extras.scale.filter(|s| s.is_finite() && *s > 0.0).map(|s| s.clamp(0.5, 512.0)),
                    tool: extras.tool.filter(|t| !t.trim().is_empty() && t.chars().count() <= 32),
                    order: extras.order,
                };
                if !extras.is_empty() {
                    data.ink.insert(key.to_string(), extras);
                }
            }
            _ => {}
        }
    }
    Some(data)
}

/// The scale a texture repeat may have: the same bounds the engine's pen accepts.
fn clamp_scale(scale: f32) -> Option<f32> {
    (scale.is_finite() && scale > 0.0).then(|| scale.clamp(0.5, 512.0))
}

/// Which brush kinds InkML cannot carry whole: Excel's Pencil is an effect, and Fountain Pen and
/// Brush Pen are byte-identical in the file, so a stroke read back needs the subtype from here.
fn tool_needs_extension(tool: InkTool) -> bool {
    matches!(tool, InkTool::Pencil | InkTool::FountainPen | InkTool::BrushPen)
}

/// The JSON name of a brush kind, as the hidden sheet stores it.
fn tool_name(tool: InkTool) -> &'static str {
    match tool {
        InkTool::Pen => "pen",
        InkTool::Pencil => "pencil",
        InkTool::Highlighter => "highlighter",
        InkTool::FountainPen => "fountainPen",
        InkTool::BrushPen => "brushPen",
    }
}

/// The brush a stored `tool` name means; `None` for a name this version does not know.
fn tool_from_name(name: &str) -> Option<InkTool> {
    Some(match name {
        "pen" => InkTool::Pen,
        "pencil" => InkTool::Pencil,
        "highlighter" => InkTool::Highlighter,
        "fountainPen" => InkTool::FountainPen,
        "brushPen" => InkTool::BrushPen,
        _ => return None,
    })
}

/// What a save must keep that the file format itself cannot: the imported textures, and for each
/// stroke its texture id, repeat scale, brush subtype and draw order, keyed by the stroke's stable
/// `a16:creationId`. `None` when there is nothing of the sort, so an Excel-representable workbook
/// never gains the sheet.
pub fn from_workbook(wb: &Workbook) -> Option<InkSheetData> {
    let mut data = InkSheetData { textures: wb.ink_textures.clone(), ink: BTreeMap::new() };
    for sheet in &wb.sheets {
        for shape in &sheet.shapes {
            let Some(ink) = &shape.ink else { continue };
            // The same id the drawing writer emits: a stored key is normalised to canonical
            // lowercase, and a missing or malformed one becomes the shape's deterministic GUID.
            let key = crate::drawing::creation_id(shape);
            let extras = InkExtras {
                texture: ink.texture.clone().filter(|id| InkTexture::is_valid_id(id)),
                scale: ink.texture.as_ref().and_then(|_| clamp_scale(ink.texture_scale)),
                // Excel drops the private brushProperty names, so the subtype (Fountain Pen and
                // Brush Pen are identical in InkML) and the draw order live here. Order 0 is the
                // default a single stroke needs no record for.
                tool: tool_needs_extension(ink.tool).then(|| tool_name(ink.tool).to_string()),
                order: (ink.order > 0).then_some(ink.order),
            };
            if !extras.is_empty() {
                data.ink.insert(key.to_string(), extras);
            }
        }
    }
    (!data.is_empty()).then_some(data)
}

/// Puts the hidden sheet's data back on the workbook: the texture store, and each stroke's
/// texture, repeat scale, brush subtype and draw order where its key matches.
pub fn apply_to_workbook(wb: &mut Workbook, data: &InkSheetData) {
    wb.ink_textures = data.textures.clone();
    for sheet in wb.sheets.iter_mut() {
        let sheet = std::sync::Arc::make_mut(sheet);
        for shape in &mut sheet.shapes {
            let Some(ink) = shape.ink.as_mut() else { continue };
            let Some(extras) = ink.key.as_deref().and_then(|key| data.ink.get(key)) else { continue };
            if let Some(id) = &extras.texture {
                ink.texture = Some(id.clone());
            }
            if let Some(scale) = extras.scale {
                ink.texture_scale = scale;
            }
            if let Some(tool) = extras.tool.as_deref().and_then(tool_from_name) {
                ink.tool = tool;
            }
            if let Some(order) = extras.order {
                ink.order = order;
            }
        }
    }
}
