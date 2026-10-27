use super::chart::format_series;
use super::*;

pub(super) fn shape_fill(s: &mut Session, p: &Json) -> Result<Json> {
    if str_param(p, "kind") == Some("shape") {
        let color = str_param(p, "color").and_then(Color::from_hex).unwrap_or_default();
        return with_shape(s, p, |sh| sh.fill = color);
    }
    format_series(s, p)
}

pub(super) fn shape_outline(s: &mut Session, p: &Json) -> Result<Json> {
    let color = str_param(p, "color").and_then(Color::from_hex).unwrap_or_default();
    with_shape(s, p, |sh| sh.line = color)
}

fn with_shape(s: &mut Session, p: &Json, f: impl FnOnce(&mut Shape)) -> Result<Json> {
    let si = s.doc()?.wb.active_sheet();
    let id = u32_param(p, "id");
    edit(s, |cx| {
        let shapes = &mut cx.sheet_mut(si)?.shapes;
        let sh = match id {
            Some(id) => shapes.iter_mut().find(|x| x.id == id),
            None => shapes.last_mut(),
        }
        .ok_or_else(|| bad("shape", "no such shape"))?;
        f(sh);
        Ok(Json::Null)
    })
}

pub(super) fn arrange(s: &mut Session, p: &Json, forward: bool) -> Result<Json> {
    let id = u32_param(p, "id").ok_or_else(|| bad("arrange", "missing `id`"))?;
    let kind = str_param(p, "kind").unwrap_or("shape").to_string();
    let extreme = bool_param(p, "toFront").or(bool_param(p, "toBack")).unwrap_or(false);
    let si = s.doc()?.wb.active_sheet();
    fn reorder<T>(v: &mut Vec<T>, i: usize, forward: bool, extreme: bool) {
        if i >= v.len() {
            return;
        }
        let item = v.remove(i);
        let j = match (forward, extreme) {
            (true, true) => v.len(),
            (false, true) => 0,
            (true, false) => (i + 1).min(v.len()),
            (false, false) => i.saturating_sub(1),
        };
        v.insert(j, item);
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        match kind.as_str() {
            "chart" => {
                if let Some(i) = sh.charts.iter().position(|c| c.id == id) {
                    reorder(&mut sh.charts, i, forward, extreme);
                }
            }
            "image" => {
                if let Some(i) = sh.images.iter().position(|c| c.id == id) {
                    reorder(&mut sh.images, i, forward, extreme);
                }
            }
            _ => {
                if let Some(i) = sh.shapes.iter().position(|c| c.id == id) {
                    reorder(&mut sh.shapes, i, forward, extreme);
                }
            }
        }
        Ok(Json::Null)
    })
}

pub(super) fn selection_pane(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let mut v = Vec::new();
    for c in &sh.shapes {
        v.push(json!({"kind": "shape", "id": c.id, "name": format!("{:?} {}", c.kind, c.id), "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    for c in &sh.images {
        v.push(json!({"kind": "image", "id": c.id, "name": format!("Picture {}", c.id), "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    for c in &sh.charts {
        v.push(json!({"kind": "chart", "id": c.id, "name": format!("Chart {}", c.id), "title": c.title, "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    Ok(Json::Array(v))
}

fn anchors_of<'a>(sh: &'a mut Sheet, kind: &str) -> Vec<(u32, &'a mut Anchor)> {
    match kind {
        "chart" => sh.charts.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
        "image" => sh.images.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
        _ => sh.shapes.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
    }
}

pub(super) fn align(s: &mut Session, p: &Json) -> Result<Json> {
    let kind = str_param(p, "kind").unwrap_or("shape").to_string();
    let ids: Vec<u32> =
        p.get("ids").and_then(Json::as_array).map(|a| a.iter().filter_map(|v| v.as_u64().map(|x| x as u32)).collect()).unwrap_or_default();
    let how = str_param(p, "align").unwrap_or("left").to_string();
    let si = s.doc()?.wb.active_sheet();
    edit(s, |cx| {
        let sheet = cx.sheet_mut(si)?;
        let geo: Vec<(u32, f64, f64, f32, f32)> = {
            let shc = sheet.clone();
            anchors_of(sheet, &kind)
                .into_iter()
                .filter(|(id, _)| ids.contains(id))
                .map(|(id, a)| (id, shc.col_left(a.cell.col) + a.dx as f64, shc.row_top(a.cell.row) + a.dy as f64, a.width, a.height))
                .collect()
        };
        if geo.len() < 2 {
            return Ok(Json::Null);
        }
        let left = geo.iter().map(|g| g.1).fold(f64::INFINITY, f64::min);
        let right = geo.iter().map(|g| g.1 + g.3 as f64).fold(f64::NEG_INFINITY, f64::max);
        let top = geo.iter().map(|g| g.2).fold(f64::INFINITY, f64::min);
        let bottom = geo.iter().map(|g| g.2 + g.4 as f64).fold(f64::NEG_INFINITY, f64::max);
        let shc = sheet.clone();
        for (id, a) in anchors_of(sheet, &kind) {
            let Some(g) = geo.iter().find(|g| g.0 == id) else { continue };
            let (mut x, mut y) = (g.1, g.2);
            match how.as_str() {
                "left" => x = left,
                "right" => x = right - g.3 as f64,
                "center" => x = (left + right) / 2.0 - g.3 as f64 / 2.0,
                "top" => y = top,
                "bottom" => y = bottom - g.4 as f64,
                _ => y = (top + bottom) / 2.0 - g.4 as f64 / 2.0,
            }
            let c = CellRef::new(shc.row_at(y), shc.col_at(x));
            a.cell = c;
            a.dx = (x - shc.col_left(c.col)) as f32;
            a.dy = (y - shc.row_top(c.row)) as f32;
        }
        Ok(Json::Null)
    })
}

pub(super) fn rotate(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("flip").is_some_and(|f| !f.is_null()) {
        return Err(bad("arrange.rotate", "`flip` is not supported: shapes have no flip state"));
    }
    with_shape(s, p, |sh| std::mem::swap(&mut sh.anchor.width, &mut sh.anchor.height))
}
