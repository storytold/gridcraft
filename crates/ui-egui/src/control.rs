//! Programmatic control of the running app (agents, tests, MCP).
//!
//! Methods (JSON lines over the host's transport, see `docs/control-protocol.md`):
//! - `engine.execute {command, params}` — run any engine or UI command
//! - `engine.commands` — every command with enablement and params docs
//! - `document.inspect`, `sheet.read {range?}`, `cell.get {cell?}` — read state
//! - `ui.inspect` — UI state (ribbon tab, editor, dialog, grid geometry, perf)
//! - `ui.click {x, y, button?, count?, shift?, cmd?, alt?}`, `ui.drag {x, y, toX, toY}`,
//!   `ui.move {x, y}` — real pointer input in window points
//! - `ui.cell.click {cell}`, `ui.cell.drag {from, to}` — pointer input on a cell's centre
//! - `ui.key {key, shift?, cmd?, alt?, ctrl?}`, `ui.text {text}` — keyboard input
//! - `ui.ribbon {tab}`, `ui.dialog {name}`, `ui.dialog.set {field, value}`,
//!   `ui.dialog.confirm`, `ui.dialog.cancel`, `ui.message.dismiss`
//! - `ui.edit.begin {text?}`, `ui.edit.commit {move?}`, `ui.edit.cancel`
//! - `ui.screenshot {path?}` — PNG of the window (base64 when no path)
//! - `ui.resize {width, height}`, `ui.set {dark?, formulaBar?}`
//! - `app.open {path}`, `app.save {path?}`, `app.quit`

use std::sync::mpsc::Sender;

use gridcraft_engine::core::CellRef;
use serde_json::{Value as Json, json};

use crate::SheetApp;

pub type ControlResponse = Json;

pub struct ControlRequest {
    pub method: String,
    pub params: Json,
    pub reply: Sender<ControlResponse>,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Json) -> (Self, std::sync::mpsc::Receiver<ControlResponse>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self { method: method.into(), params, reply: tx }, rx)
    }
}

pub enum Outcome {
    Done(Json),
    Screenshot { path: Option<String> },
}

#[derive(Default)]
pub struct Shots {
    token: u64,
    queued: Vec<(u64, f64, u32)>,
    pending: Vec<(u64, Option<String>, Sender<ControlResponse>, f64)>,
}

fn ok(v: Json) -> Outcome {
    Outcome::Done(json!({"ok": true, "result": v}))
}
fn err(e: impl std::fmt::Display) -> Outcome {
    Outcome::Done(json!({"ok": false, "error": e.to_string()}))
}
fn wrap(r: Result<Json, String>) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

pub fn inspect(app: &SheetApp, ctx: &egui::Context) -> Json {
    let r = ctx.content_rect();
    let v = app.view();
    json!({
        "ui": serde_json::to_value(&app.ui).unwrap_or_default(),
        "window": [r.width(), r.height()],
        "grid": app.grid.cells_rect.map(|c| json!([c.left(), c.top(), c.width(), c.height()])),
        "scroll": [v.scroll.x, v.scroll.y],
        "editor": app.editor.as_ref().map(|e| json!({"cell": e.cell.a1(), "text": e.text, "formulaBar": e.from_formula_bar, "point": e.can_point()})),
        "dialog": app.dialog.as_ref().map(|d| json!({"name": d.name, "title": d.title, "values": d.values, "tab": d.tab})),
        "message": app.message.as_ref().map(|(t, m)| json!({"title": t, "text": m})),
        "selectedChart": app.selected_chart,
        "mode": format!("{:?}", app.session.mode),
        "documents": app.session.documents().iter().map(|d| json!({"title": d.display_title(), "dirty": d.is_dirty()})).collect::<Vec<_>>(),
        "activeDocument": app.session.active_index(),
        "perf": {"frameMs": app.perf.frame_ms, "gridMs": app.perf.grid_ms, "fps": app.perf.fps},
    })
}

fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(egui::Key::Enter),
        "esc" | "escape" => Some(egui::Key::Escape),
        "delete" | "del" => Some(egui::Key::Delete),
        "backspace" => Some(egui::Key::Backspace),
        "left" => Some(egui::Key::ArrowLeft),
        "right" => Some(egui::Key::ArrowRight),
        "up" => Some(egui::Key::ArrowUp),
        "down" => Some(egui::Key::ArrowDown),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        "pageup" => Some(egui::Key::PageUp),
        "pagedown" => Some(egui::Key::PageDown),
        "home" => Some(egui::Key::Home),
        "end" => Some(egui::Key::End),
        "f2" => Some(egui::Key::F2),
        "f4" => Some(egui::Key::F4),
        "f9" => Some(egui::Key::F9),
        _ => None,
    })
}

fn modifiers(p: &Json) -> egui::Modifiers {
    let b = |n: &str| p.get(n).and_then(Json::as_bool).unwrap_or(false);
    egui::Modifiers {
        alt: b("alt"),
        ctrl: b("ctrl"),
        shift: b("shift"),
        mac_cmd: b("cmd") && cfg!(target_os = "macos"),
        command: b("cmd") || (b("ctrl") && !cfg!(target_os = "macos")),
    }
}

fn cell_center(app: &SheetApp, c: CellRef) -> Option<egui::Pos2> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let rect = app.grid.rect?;
    let geo = crate::grid::Geo::new(sh, rect, app.view().scroll);
    Some(geo.cell_rect(sh, c).center())
}

fn push_click(app: &mut SheetApp, pos: egui::Pos2, button: egui::PointerButton, count: u32, m: egui::Modifiers) {
    app.synthetic.push_back(vec![egui::Event::PointerMoved(pos)]);
    let mut batch = Vec::new();
    for _ in 0..count.max(1) {
        batch.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers: m });
        batch.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers: m });
    }
    app.synthetic.push_back(batch);
}

fn push_drag(app: &mut SheetApp, a: egui::Pos2, b: egui::Pos2, steps: usize, m: egui::Modifiers) {
    app.synthetic.push_back(vec![egui::Event::PointerMoved(a)]);
    app.synthetic.push_back(vec![egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: m }]);
    for i in 1..=steps.max(1) {
        let t = i as f32 / steps.max(1) as f32;
        app.synthetic.push_back(vec![egui::Event::PointerMoved(a + (b - a) * t)]);
    }
    app.synthetic.push_back(vec![egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: m }]);
}

pub fn handle(app: &mut SheetApp, ctx: &egui::Context, method: &str, p: &Json) -> Outcome {
    let s = |k: &str| p.get(k).and_then(Json::as_str);
    let f = |k: &str| p.get(k).and_then(Json::as_f64).unwrap_or(0.0) as f32;
    let out = match method {
        "engine.execute" | "command" | "ui.menu.invoke" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            let params = p.get("params").cloned().unwrap_or(json!({}));
            wrap(app.run(id, if params.is_null() { json!({}) } else { params }))
        }
        "engine.commands" => {
            let mut v: Vec<Json> = app.session.commands().into_iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect();
            for (id, label, params) in [
                ("ui.ribbonTab", "Ribbon Tab", ""),
                ("view.formulaBar", "Formula Bar", ""),
                ("view.collapseRibbon", "Collapse Ribbon", ""),
                ("view.darkMode", "Dark Mode", ""),
                ("view.theme", "Display Theme", ""),
                ("view.zoom100", "100%", ""),
                ("ui.dialog", "Open Dialog", ""),
                ("app.language.set", "Interface Language", "{language: \"en\"|\"zh\"|\"ja\"|\"ko\"|\"ru\"|\"pt\"} (alias: code)"),
                ("app.language.english", "Interface Language: English", ""),
                ("app.language.japanese", "Interface Language: Japanese", ""),
            ] {
                v.push(json!({"id": id, "label": label, "params": params, "enabled": true, "ui": true}));
            }
            ok(Json::Array(v))
        }
        "engine.journal" => ok(json!(app.session.journal.iter().map(|(id, p)| json!({"command": id, "params": p})).collect::<Vec<_>>())),
        "document.inspect" | "sheet.read" | "cell.get" | "selection.stats" => wrap(app.session.run(method, p.clone())),
        "ui.inspect" => ok(inspect(app, ctx)),
        "ui.ribbon" => {
            app.ui.ribbon_tab = s("tab").unwrap_or("Home").to_string();
            app.ui.ribbon_collapsed = false;
            ok(Json::Null)
        }
        "ui.set" => {
            if let Some(v) = p.get("dark").and_then(Json::as_bool) {
                app.ui.dark = v;
                app.ui.system_theme = false;
            }
            if let Some(v) = p.get("formulaBar").and_then(Json::as_bool) {
                app.ui.formula_bar = v;
            }
            if let Some(v) = p.get("ribbonCollapsed").and_then(Json::as_bool) {
                app.ui.ribbon_collapsed = v;
            }
            ok(serde_json::to_value(&app.ui).unwrap_or_default())
        }
        "ui.dialog" => {
            app.open_dialog(s("name").unwrap_or(""), p.clone());
            ok(json!({"dialog": app.dialog.as_ref().map(|d| d.name.clone())}))
        }
        "ui.dialog.set" => match app.dialog.as_mut() {
            Some(d) => {
                let Some(field) = s("field") else { return err("missing `field`") };
                if field == "tab" {
                    d.tab = s("value").unwrap_or("").to_string();
                } else if field == "search" {
                    d.search = s("value").unwrap_or("").to_string();
                } else {
                    d.values.insert(field.to_string(), p.get("value").cloned().unwrap_or(Json::Null));
                }
                ok(json!(d.values))
            }
            None => err("no dialog is open"),
        },
        "ui.dialog.confirm" => match app.dialog.take() {
            Some(d) => match d.command {
                Some(cmd) => {
                    let params = Json::Object(d.values.clone());
                    let r = app.run(cmd, params);
                    if r.is_err() {
                        app.dialog = Some(d);
                    }
                    wrap(r)
                }
                None => {
                    app.dialog = Some(d);
                    err("this dialog has no OK command; drive it with its own commands")
                }
            },
            None => err("no dialog is open"),
        },
        "ui.dialog.cancel" => {
            app.dialog = None;
            ok(Json::Null)
        }
        "ui.message.dismiss" => {
            app.message = None;
            ok(Json::Null)
        }
        "ui.edit.begin" => {
            app.begin_edit(s("text").map(str::to_string), false);
            ok(Json::Null)
        }
        "ui.edit.text" => match app.editor.as_mut() {
            Some(e) => {
                e.text = s("text").unwrap_or("").to_string();
                e.caret = e.text.chars().count();
                ok(Json::Null)
            }
            None => err("not editing"),
        },
        "ui.edit.commit" => {
            let (dr, dc) = match s("move") {
                Some("right") => (0, 1),
                Some("up") => (-1, 0),
                Some("left") => (0, -1),
                Some("none") => (0, 0),
                _ => (1, 0),
            };
            if app.commit_edit(dr, dc, false, false) {
                ok(Json::Null)
            } else {
                err(app.message.clone().map(|m| m.1).unwrap_or_else(|| "could not commit".into()))
            }
        }
        "ui.edit.cancel" => {
            app.cancel_edit();
            ok(Json::Null)
        }
        "ui.key" => {
            let Some(k) = s("key").and_then(key_from) else { return err("unknown or missing `key`") };
            let m = modifiers(p);
            app.synthetic.push_back(vec![
                egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m },
                egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m },
            ]);
            ok(Json::Null)
        }
        "ui.text" => {
            app.synthetic.push_back(vec![egui::Event::Text(s("text").unwrap_or("").to_string())]);
            ok(Json::Null)
        }
        "ui.move" => {
            app.synthetic.push_back(vec![egui::Event::PointerMoved(egui::pos2(f("x"), f("y")))]);
            ok(Json::Null)
        }
        "ui.click" => {
            let button = if s("button") == Some("right") { egui::PointerButton::Secondary } else { egui::PointerButton::Primary };
            push_click(app, egui::pos2(f("x"), f("y")), button, p.get("count").and_then(Json::as_u64).unwrap_or(1) as u32, modifiers(p));
            ok(Json::Null)
        }
        "ui.drag" => {
            push_drag(
                app,
                egui::pos2(f("x"), f("y")),
                egui::pos2(f("toX"), f("toY")),
                p.get("steps").and_then(Json::as_u64).unwrap_or(8) as usize,
                modifiers(p),
            );
            ok(Json::Null)
        }
        "ui.cell.click" => {
            let Some(c) = s("cell").and_then(CellRef::parse) else { return err("missing `cell`") };
            let Some(pos) = cell_center(app, c) else { return err("no grid yet") };
            let button = if s("button") == Some("right") { egui::PointerButton::Secondary } else { egui::PointerButton::Primary };
            push_click(app, pos, button, p.get("count").and_then(Json::as_u64).unwrap_or(1) as u32, modifiers(p));
            ok(json!({"x": pos.x, "y": pos.y}))
        }
        "ui.cell.drag" => {
            let (Some(a), Some(b)) = (s("from").and_then(CellRef::parse), s("to").and_then(CellRef::parse)) else {
                return err("missing `from`/`to`");
            };
            let (Some(pa), Some(pb)) = (cell_center(app, a), cell_center(app, b)) else { return err("no grid yet") };
            push_drag(app, pa, pb, 8, modifiers(p));
            ok(Json::Null)
        }
        "ui.resize" => {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(f("width").max(400.0), f("height").max(300.0))));
            ok(Json::Null)
        }
        "ui.focus" => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ok(Json::Null)
        }
        "ui.screenshot" => return Outcome::Screenshot { path: s("path").map(str::to_string) },
        "app.open" => {
            let Some(path) = s("path") else { return err("missing `path`") };
            app.open_path(path);
            match app.message.take() {
                Some((_, m)) => err(m),
                None => ok(json!({"title": app.session.active().map(|d| d.display_title())})),
            }
        }
        "app.save" => wrap(app.run("file.save", json!({"path": s("path")}))),
        "app.quit" => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ok(Json::Null)
        }
        // Anything else that names a command runs it.
        other if gridcraft_engine::find_command(other).is_some() => wrap(app.run(other, p.clone())),
        other => err(format!("unknown method `{other}`")),
    };
    ctx.request_repaint();
    out
}

pub fn poll(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(rx) = app.control_rx.take() else { return };
    while let Ok(req) = rx.try_recv() {
        let outcome = handle(app, ctx, &req.method, &req.params);
        match outcome {
            Outcome::Done(v) => {
                let _ = req.reply.send(v);
            }
            Outcome::Screenshot { path } => {
                app.shots.token += 1;
                let token = app.shots.token;
                app.shots.queued.push((token, crate::now_ms() + 100.0, 0));
                app.shots.pending.push((token, path, req.reply, crate::now_ms() + 8000.0));
            }
        }
    }
    app.control_rx = Some(rx);
    if !app.synthetic.is_empty() {
        ctx.request_repaint();
    }
}

pub fn issue_screenshots(app: &mut SheetApp, ctx: &egui::Context) {
    let now = crate::now_ms();
    app.shots.queued.retain_mut(|(token, at, frames)| {
        *frames += 1;
        if now >= *at && *frames >= 3 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(*token)));
            false
        } else {
            true
        }
    });
    if !app.shots.queued.is_empty() || !app.shots.pending.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

pub fn collect_screenshots(app: &mut SheetApp, ctx: &egui::Context) {
    if app.shots.pending.is_empty() {
        return;
    }
    let events: Vec<(u64, std::sync::Arc<egui::ColorImage>)> = ctx.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Screenshot { user_data, image, .. } => {
                    let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                    Some((token, image.clone()))
                }
                _ => None,
            })
            .collect()
    });
    for (token, image) in events {
        if let Some(i) = app.shots.pending.iter().position(|(t, ..)| *t == token) {
            let (_, path, reply, _) = app.shots.pending.remove(i);
            let _ = reply.send(save_image(&image, path.as_deref()));
        }
    }
    let now = crate::now_ms();
    app.shots.pending.retain(|(_, _, reply, deadline)| {
        if now < *deadline {
            return true;
        }
        let _ = reply.send(json!({"ok": false, "error": "no frame was presented before the screenshot timeout; make the window visible, use `ui.focus`, then retry `ui.screenshot`"}));
        false
    });
}

/// Encodes a screenshot as PNG to `path`, or returns it as base64.
pub fn save_image(image: &egui::ColorImage, path: Option<&str>) -> Json {
    let [w, h] = image.size;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in &image.pixels {
        rgba.extend_from_slice(&p.to_array());
    }
    let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) else { return json!({"ok": false, "error": "bad image"}) };
    let mut png = Vec::new();
    if let Err(e) = image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png) {
        return json!({"ok": false, "error": e.to_string()});
    }
    match path {
        Some(p) => match std::fs::write(p, &png) {
            Ok(()) => json!({"ok": true, "result": {"path": p, "width": w, "height": h}}),
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        },
        None => json!({"ok": true, "result": {"base64": gridcraft_engine::io::base64_encode(&png), "width": w, "height": h}}),
    }
}
