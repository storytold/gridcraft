//! Draw/Format uses stable workbook object IDs, not whichever shape the local user most
//! recently selected. The ribbon's shape and ink writers act on the active sheet only.
use super::super::context;
use gridcraft_engine::{
    DocState, cmd,
    core::CellRef,
    model::{ShapeKind, Sheet},
};
use serde_json::{Value as Json, json};

pub(super) fn field(key: &str) -> Option<Json> {
    Some(match key {
        "shape" => json!({"type":"integer","minimum":0,"maximum":u32::MAX}),
        "shapeKind" => json!({"type":"string","enum":["rectangle","roundedRectangle","ellipse","triangle","line","arrow","textBox"]}),
        "shapeFill" | "shapeLine" => json!({"type":"string","maxLength":7,"description":"#RRGGBB"}),
        "penWidth" => json!({"type":"number","minimum":1,"maximum":40}),
        "inkPath" => json!({"type":"array","minItems":2,"maxItems":256,"items":{"type":"array","minItems":2,"maxItems":2,
            "items":{"type":"number","minimum":0,"maximum":4096}}}),
        _ => return None,
    })
}

pub(super) fn inspect(d: &DocState, sh: &Sheet, p: &Json) -> Json {
    let offset = p["offset"].as_u64().unwrap_or(0) as usize;
    context::page(
        sh.shapes.iter().skip(offset).map(|s| {
            let ink = s.kind == ShapeKind::Ink;
            json!({"id":s.id,"kind":format!("{:?}",s.kind),"at":s.anchor.cell.a1(),"dx":s.anchor.dx,"dy":s.anchor.dy,
            "width":s.anchor.width,"height":s.anchor.height,"rotation":s.rotation,
            "fill":(!ink).then(||s.fill.hex(&d.wb.theme)).flatten(),"outline":s.line.hex(&d.wb.theme),
            "penWidth":ink.then(||cmd::draw::ink_width(s)),
            "text":(!ink).then(||context::clip(&s.text,256))})
        }),
        offset,
        24,
    )
}

fn ensure_rgb(s: &str) -> Result<(), String> {
    if s.len() == 7 && s.starts_with('#') && s.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit) {
        Ok(())
    } else {
        Err("Supply an RGB color as #RRGGBB.".into())
    }
}

pub(super) fn prepare(d: &DocState, sh: &Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    if d.wb.active().is_none_or(|s| s.uid != sh.uid) {
        return Err("Select the requested worksheet first: native drawing actions use the active sheet.".into());
    }
    if let Some(color) = p["color"].as_str() {
        ensure_rgb(color)?;
    }
    match name {
        "create_shape" => {
            p["kind"] = p.as_object_mut().and_then(|o| o.remove("shapeKind")).ok_or("Missing shapeKind.")?;
            for (from, to) in [("shapeFill", "fill"), ("shapeLine", "line")] {
                if let Some(v) = p.as_object_mut().and_then(|o| o.remove(from)) {
                    ensure_rgb(v.as_str().ok_or("Invalid color.")?)?;
                    p[to] = v;
                }
            }
        }
        "add_ink_stroke" => {
            let at = p["at"].as_str().and_then(CellRef::parse).ok_or("Supply a local A1 anchor.")?;
            let points = p["inkPath"].as_array().filter(|a| (2..=256).contains(&a.len())).ok_or("Supply 2–256 ink points.")?;
            let (x, y) = (sh.col_left(at.col), sh.row_top(at.row));
            // Native draw.stroke uses absolute sheet points; offsets keep agent coordinates
            // independent of zoom, scrolling, hidden rows and custom cell dimensions.
            let path = points
                .iter()
                .map(|pair| {
                    let pt = pair.as_array().filter(|a| a.len() == 2).ok_or("Each ink point must be [x,y].")?;
                    let axis =
                        |i: usize| pt[i].as_f64().filter(|v| (0.0..=4096.0).contains(v)).ok_or("Ink offsets must be within 0–4096 sheet points.");
                    Ok(json!([x + axis(0)?, y + axis(1)?]))
                })
                .collect::<Result<Vec<_>, &str>>()?;
            let mut native = json!({"points":path});
            if let Some(width) = p["penWidth"].as_f64() {
                native["width"] = json!(width);
            }
            if let Some(color) = p["color"].as_str() {
                native["color"] = json!(color);
            }
            *p = native;
        }
        "ink_to_shape" | "shape_fill" | "shape_outline" | "edit_shape_text" => {
            let id = p["shape"].as_u64().and_then(|v| u32::try_from(v).ok()).ok_or("Missing shape ID.")?;
            let mut hits = sh.shapes.iter().filter(|s| s.id == id);
            let target = hits.next().ok_or("Shape not found on the requested sheet.")?;
            if hits.next().is_some() {
                return Err("Ambiguous shape ID.".into());
            }
            if name == "ink_to_shape" && (target.kind != ShapeKind::Ink || target.text.len() > 128 * 1024) {
                return Err("Select a native ink stroke within the 128 KiB conversion limit.".into());
            }
            if name == "shape_fill"
                && !matches!(
                    target.kind,
                    ShapeKind::Rectangle | ShapeKind::RoundedRectangle | ShapeKind::Triangle | ShapeKind::Ellipse | ShapeKind::TextBox
                )
            {
                return Err("Fill requires a closed geometric shape or a text box.".into());
            }
            if name == "edit_shape_text" && !matches!(target.kind, ShapeKind::TextBox | ShapeKind::WordArt) {
                return Err("Select a text box or WordArt object.".into());
            }
            p.as_object_mut().ok_or("Invalid shape arguments.")?.remove("shape");
            p["id"] = json!(id);
            if matches!(name, "shape_fill" | "shape_outline") {
                p["kind"] = json!("shape");
            }
        }
        _ => return Err("Unknown drawing action.".into()),
    }
    Ok(())
}
