//! Draw tab: pen and eraser tools, ink strokes, Ink to Shape, and the Action Pen (handwriting to
//! text; the model and the recognizer are described in `docs/action-pen.md`).

use gridcraft_core::CellRef;
use gridcraft_ink::{Accelerator, InstallState, ModelFile, TROCR_SMALL_HANDWRITTEN};
use gridcraft_model::{Anchor, Color, Shape, ShapeKind};
use serde_json::{Value as Json, json};
use std::path::{Path, PathBuf};

use super::{
    CommandSpec, EngineError, Result, Session, always, bad, bool_param, cell_param_on, cmd, edit, f64_param, has_doc, ok, str_param, u32_param,
};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "draw.pen", "Pen", ["Draw", "Tools"], None, "{on?: bool, color?: hex, width?: 2}", has_doc, pen),
        cmd!(noundo "draw.eraser", "Eraser", ["Draw", "Tools"], None, "{on?: bool}", has_doc, eraser),
        cmd!(
            noundo "draw.actionPen",
            "Action Pen",
            ["Draw", "Tools"],
            None,
            "{on?: bool} turns handwriting-to-text on; needs the handwriting model (ink.status)",
            always,
            action_pen
        ),
        cmd!(
            "draw.actionPenInk",
            "Action Pen Ink",
            [],
            None,
            "{points: [[x,y],…] in sheet points, cell?: \"B2\"} recognizes the stroke and types it into the cell",
            always,
            action_pen_ink
        ),
        cmd!(
            query "ink.status",
            "Handwriting Model",
            [],
            None,
            "{} reports whether the handwriting model is installed, where it goes, its licence and size",
            always,
            ink_status
        ),
        cmd!(
            query "ink.modelImport",
            "Import Handwriting Model",
            [],
            None,
            "{path: a folder holding the model files, or one of them} copies and verifies them into the model directory",
            always,
            model_import
        ),
        cmd!(query "ink.modelRemove", "Remove Handwriting Model", [], None, "{} deletes the downloaded model", always, model_remove),
        cmd!(noundo "draw.lasso", "Lasso Select", ["Draw", "Tools"], None, "{} (selects ink by dragging; same as Select Objects)", has_doc, |s, _| {
            s.draw_tool = String::new();
            ok()
        }),
        cmd!("draw.stroke", "Ink Stroke", [], None, "{points: [[x,y],…] in sheet points, color?: hex, width?: 2}", has_doc, stroke),
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
    Ok(json!({"tool": s.draw_tool, "color": s.draw_color, "width": s.draw_width}))
}

fn eraser(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.draw_tool != "eraser");
    s.draw_tool = if on { "eraser".into() } else { String::new() };
    Ok(json!({"tool": s.draw_tool}))
}

// ---------------------------------------------------------------- Action Pen

/// Where the handwriting model's files go, as a person would see it.
fn model_directory(s: &Session) -> String {
    match s.models_dir.as_deref() {
        Some(root) => TROCR_SMALL_HANDWRITTEN.dir(Path::new(root)).display().to_string(),
        None => format!("<models>/{}/ (open Draw › Action Pen in the desktop app)", TROCR_SMALL_HANDWRITTEN.id),
    }
}

/// The error `draw.actionPen` and `draw.actionPenInk` give when no recognizer is installed.
fn no_recognizer(s: &Session, cmd: &'static str) -> EngineError {
    bad(
        cmd,
        format!(
            "The handwriting model is not installed, so the Action Pen is unavailable. Download it from Draw › Action Pen, or copy its files into {}.",
            model_directory(s)
        ),
    )
}

/// `draw.actionPen {on?}`: turns the handwriting tool on or off. Turning it on without a model
/// is refused with the directory to install it in (never a panic, never a silent no-op).
fn action_pen(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.draw_tool != "actionPen");
    if on && s.ink.is_none() {
        return Err(no_recognizer(s, "draw.actionPen"));
    }
    s.draw_tool = if on { "actionPen".into() } else { String::new() };
    Ok(json!({"tool": s.draw_tool, "ready": s.ink.is_some()}))
}

/// `draw.actionPenInk {points, cell?}`: recognises one stroke and types the result into the cell
/// under it (or `cell`). One undo step, through the same input path as typing, so "42" is a
/// number and "3/4/2024" a date; no ink shape is kept.
fn action_pen_ink(s: &mut Session, p: &Json) -> Result<Json> {
    let pts = points_param(p);
    if pts.is_empty() {
        return Err(bad("draw.actionPenInk", "a stroke needs at least one point in `points`"));
    }
    let Some(recognizer) = s.ink.clone() else { return Err(no_recognizer(s, "draw.actionPenInk")) };
    let si = s.doc()?.wb.active_sheet();
    let at = match cell_param_on(s, p, "cell", si)? {
        Some(c) => c,
        None => {
            let sh = s.doc()?.wb.sheet(si).ok_or(EngineError::NoDocument)?;
            let minx = pts.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min).max(0.0);
            let miny = pts.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min).max(0.0);
            CellRef::new(sh.row_at(miny), sh.col_at(minx))
        }
    };
    let pen = if s.draw_width <= 0.0 { 2.0 } else { s.draw_width }.clamp(0.5, 40.0);
    let points: Vec<[f32; 2]> = pts.iter().map(|q| [q[0] as f32, q[1] as f32]).collect();
    let image = gridcraft_ink::raster::rasterize(&points, pen).map_err(|e| bad("draw.actionPenInk", e.to_string()))?;
    let text = recognizer.recognize(&image).map_err(|e| bad("draw.actionPenInk", e.to_string()))?.trim().to_string();
    if text.is_empty() {
        return Err(bad("draw.actionPenInk", "Nothing was recognized in that stroke. Write larger, or try again."));
    }
    if text.chars().count() > 1_000 {
        return Err(bad("draw.actionPenInk", "The recognized text is too long for one cell."));
    }
    edit(s, |cx| {
        let cell = super::edit::input_to_cell(&text, si, at, &mut cx.wb)?;
        let sh = cx.sheet_mut(si)?;
        match cell {
            Some(c) => sh.set_cell(at, c),
            None => {
                sh.remove_cell(at);
            }
        }
        cx.touch(si, at);
        Ok(Json::Null)
    })?;
    let value = s.doc()?.wb.sheet(si).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "text": text, "value": value.display()}))
}

/// The backend a build without a recognizer would use: CPU, the one every platform has.
const CPU_ONLY: &[Accelerator] = &[Accelerator::Cpu];

/// `ink.status {accelerator?}`: what the UI's model panel shows. Cheap (one `stat` per model
/// file), so the panel may ask every frame.
///
/// `accelerator` asks what a specific backend would give: the shipped build runs on CPU, and
/// asking for one it does not have answers with `acceleratorMessage` instead of failing, so a
/// caller can say "runs on CPU; OpenVINO NPU/GPU not available" and carry on (the feature
/// behaves identically on CPU).
fn ink_status(s: &mut Session, p: &Json) -> Result<Json> {
    let m = &TROCR_SMALL_HANDWRITTEN;
    let (state, directory) = match s.models_dir.as_deref().map(PathBuf::from) {
        Some(root) => (m.check_sizes(&root), m.dir(&root).display().to_string()),
        None => (InstallState::Incomplete(vec!["no models directory is configured".to_string()]), model_directory(s)),
    };
    let missing = match &state {
        InstallState::Incomplete(files) => files.clone(),
        _ => Vec::new(),
    };
    let files_present = state.is_installed();
    let ready = s.ink.is_some();
    let active = s.ink.as_ref().map_or(Accelerator::Cpu, |r| r.accelerator());
    let available: &'static [Accelerator] = s.ink.as_ref().map_or(CPU_ONLY, |r| r.accelerators());
    let requested = str_param(p, "accelerator").map(str::trim).filter(|name| !name.is_empty());
    let mut accelerator_message = None;
    if let Some(name) = requested {
        match Accelerator::parse(name) {
            Some(want) if available.contains(&want) => {}
            Some(want) => {
                accelerator_message = Some(format!(
                    "Handwriting runs on {}; {} is not available in this build, so recognition stays on {}.",
                    active.label(),
                    want.label(),
                    active.label()
                ))
            }
            None => {
                return Err(bad(
                    "ink.status",
                    format!(
                        "unknown accelerator `{name}`; this build runs on {} (available: {})",
                        active.label(),
                        available.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                ));
            }
        }
    }
    let hint = if ready {
        "Action Pen is ready: draw with the Action Pen tool and the writing becomes text.".to_string()
    } else if files_present {
        "The model files are present but were not loaded; restart GridCraft or download the model again.".to_string()
    } else {
        format!("Download the handwriting model, or copy its files into {directory}.")
    };
    Ok(json!({
        "model": m.id,
        "name": m.name,
        "revision": m.revision,
        "licence": m.licence,
        "source": m.source,
        "bytes": m.total_bytes(),
        "runtime": m.runtime,
        "accelerator": active.as_str(),
        "accelerators": available.iter().map(|a| a.as_str()).collect::<Vec<_>>(),
        "requestedAccelerator": requested,
        "acceleratorMessage": accelerator_message,
        "directory": directory,
        "filesPresent": files_present,
        "ready": ready,
        "missing": missing,
        "hint": hint,
    }))
}

/// Copies `from` over `to` through a temporary file, and only keeps it when its size and
/// SHA-256 match the manifest. A failed check leaves the previous file (or nothing) in place.
fn copy_verified(from: &Path, to: &Path, f: &ModelFile) -> Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| bad("ink.modelImport", format!("{}: {e}", dir.display())))?;
    }
    let part = to.with_extension("part");
    std::fs::copy(from, &part).map_err(|e| bad("ink.modelImport", format!("{}: {e}", from.display())))?;
    let check = (|| {
        let size = std::fs::metadata(&part).map_err(|e| bad("ink.modelImport", format!("{}: {e}", part.display())))?.len();
        if size != f.bytes {
            return Err(bad("ink.modelImport", format!("{} is {size} bytes, expected {} for {}", from.display(), f.bytes, f.path)));
        }
        let hash = gridcraft_ink::manifest::sha256_file(&part).map_err(|e| bad("ink.modelImport", e.to_string()))?;
        if !hash.eq_ignore_ascii_case(f.sha256) {
            return Err(bad(
                "ink.modelImport",
                format!("{} is not the file the manifest names for {} (SHA-256 {hash}); check the download", from.display(), f.path),
            ));
        }
        Ok(())
    })();
    if check.is_err() {
        let _ = std::fs::remove_file(&part);
        return check;
    }
    std::fs::rename(&part, to).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        bad("ink.modelImport", format!("{}: {e}", to.display()))
    })
}

/// `ink.modelImport {path}`: copies a model file (or a folder of them) into the model directory,
/// verifying each one. Importing the files one at a time works; the answer says what is missing.
fn model_import(s: &mut Session, p: &Json) -> Result<Json> {
    let path = str_param(p, "path")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| bad("ink.modelImport", "`path` is required: a folder holding the model's files, or one of the files"))?;
    let Some(root) = s.models_dir.as_deref().map(PathBuf::from) else {
        return Err(bad("ink.modelImport", "No model directory is configured. The desktop app sets one; `models/` beside the executable works too."));
    };
    let m = &TROCR_SMALL_HANDWRITTEN;
    let src = PathBuf::from(path);
    let dir = m.dir(&root);
    let mut copied: Vec<&str> = Vec::new();
    if src.is_dir() {
        for f in m.files {
            let name = f.path.rsplit('/').next().unwrap_or(f.path);
            // The folder as laid out upstream, or with the files side by side.
            let from = [src.join(f.path), src.join(name)].into_iter().find(|c| c.is_file());
            let Some(from) = from else { continue };
            copy_verified(&from, &dir.join(f.path), f)?;
            copied.push(f.path);
        }
        if copied.is_empty() {
            return Err(bad(
                "ink.modelImport",
                format!("{} holds none of the model's files ({})", src.display(), m.files.iter().map(|f| f.path).collect::<Vec<_>>().join(", ")),
            ));
        }
    } else if src.is_file() {
        let name = src.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let f = m.file_named(name).ok_or_else(|| {
            bad(
                "ink.modelImport",
                format!("`{name}` is not one of the model's files ({})", m.files.iter().map(|f| f.path).collect::<Vec<_>>().join(", ")),
            )
        })?;
        copy_verified(&src, &dir.join(f.path), f)?;
        copied.push(f.path);
    } else {
        return Err(bad("ink.modelImport", format!("{}: no such file or folder", src.display())));
    }
    let missing = match m.check_sizes(&root) {
        InstallState::Installed => Vec::new(),
        InstallState::Incomplete(files) => files,
        InstallState::Refused(_) => Vec::new(),
    };
    let installed = missing.is_empty();
    Ok(json!({"copied": copied, "installed": installed, "missing": missing, "directory": dir.display().to_string()}))
}

/// `ink.modelRemove {}`: deletes the model's directory and turns the tool off.
fn model_remove(s: &mut Session, _p: &Json) -> Result<Json> {
    let Some(root) = s.models_dir.as_deref().map(PathBuf::from) else {
        return Err(bad("ink.modelRemove", "No model directory is configured."));
    };
    let dir = TROCR_SMALL_HANDWRITTEN.dir(&root);
    if !dir.is_dir() {
        return Err(bad("ink.modelRemove", format!("The handwriting model is not installed in {}.", dir.display())));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| bad("ink.modelRemove", format!("{}: {e}", dir.display())))?;
    s.ink = None;
    if s.draw_tool == "actionPen" {
        s.draw_tool = String::new();
    }
    Ok(json!({"removed": dir.display().to_string()}))
}

fn parse_points(t: &str) -> Vec<[f32; 2]> {
    t.split(';').filter_map(|pt| pt.split_once(',').and_then(|(x, y)| Some([x.parse().ok()?, y.parse().ok()?]))).collect()
}

/// `{points: [[x, y], …]}` as sheet points: non-finite pairs dropped, at most 20,000 kept.
fn points_param(p: &Json) -> Vec<[f64; 2]> {
    p.get("points")
        .and_then(Json::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|q| Some([q.get(0)?.as_f64()?, q.get(1)?.as_f64()?]))
                .filter(|q| q[0].is_finite() && q[1].is_finite())
                .take(20_000)
                .collect()
        })
        .unwrap_or_default()
}

fn stroke(s: &mut Session, p: &Json) -> Result<Json> {
    let pts: Vec<[f64; 2]> = points_param(p);
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
    let d = s.doc()?;
    let si = d.wb.active_sheet();
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let cell = CellRef::new(sh.row_at(miny), sh.col_at(minx));
    let dx = (minx - sh.col_left(cell.col)) as f32;
    let dy = (miny - sh.row_top(cell.row)) as f32;
    let text = pts.iter().map(|q| format!("{:.1},{:.1}", q[0] - minx, q[1] - miny)).collect::<Vec<_>>().join(";");
    let id = d.wb.next_object_id();
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
    };
    edit(s, |cx| {
        cx.sheet_mut(si)?.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
}

/// Ink width is stored in the red channel of `fill` (shapes have no width field).
pub fn ink_width(sh: &Shape) -> f32 {
    match sh.fill {
        Color::Rgb(v) => ((v >> 16) & 0xFF).max(1) as f32,
        _ => 2.0,
    }
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
