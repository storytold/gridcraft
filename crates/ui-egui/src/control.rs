//! Programmatic control of the running app (agents, tests, MCP).
//!
//! Methods (JSON lines over the host's transport, see `docs/control-protocol.md`):
//! - `engine.execute {command, params}` — run any engine or UI command
//! - `engine.commands {search?}` — every command with enablement and params docs
//! - `document.inspect`, `sheet.read {range?}`, `cell.get {cell?}` — read state
//! - `ui.inspect` — UI state (ribbon tab, editor, dialog, grid geometry, perf)
//! - `ui.click {x, y, button?, count?, shift?, cmd?, alt?}`, `ui.drag {x, y, toX, toY}`,
//!   `ui.move {x, y}` — real pointer input in window points
//! - `ui.cell.click {cell}`, `ui.cell.drag {from, to}` — pointer input on a cell's centre
//! - `ui.key {key, shift?, cmd?, alt?, ctrl?}`, `ui.text {text}` — keyboard input
//! - `ui.ribbon {tab}`, `ui.dialog {name}`, `ui.dialog.set {field, value}`,
//!   `ui.dialog.confirm`, `ui.dialog.cancel`, `ui.message.dismiss`
//! - `ui.edit.begin {text?}`, `ui.edit.commit {move?}`, `ui.edit.cancel`
//! - `ui.screenshot {path?}` — PNG of the window (base64 when no path); animations are settled
//! - `ui.resize {width, height}`, `ui.set {dark?, formulaBar?, ribbonCollapsed?, animations?}`
//! - `app.open {path, password?}`, `app.save {path?}`, `app.quit`

use std::sync::mpsc::Sender;

use gridcraft_engine::core::CellRef;
use serde_json::{Value as Json, json};

use crate::SheetApp;

pub type ControlResponse = Json;

pub struct ControlRequest {
    pub method: String,
    pub params: Json,
    pub reply: Sender<ControlResponse>,
    /// Who has the request: [`PENDING`], [`TAKEN`] (the app runs it) or [`ABANDONED`] (the sender
    /// gave up waiting, so the app must not run it late: a client told its request timed out had
    /// it applied anyway on the next UI pass, and an agent that retried ran it twice (#1914).
    pub state: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

/// Queued, not yet run.
pub const PENDING: u8 = 0;
/// The app has started it: it will answer.
pub const TAKEN: u8 = 1;
/// Its sender gave up waiting (the reply timeout expired): it is dropped unrun.
pub const ABANDONED: u8 = 2;

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Json) -> (Self, std::sync::mpsc::Receiver<ControlResponse>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self { method: method.into(), params, reply: tx, state: Default::default() }, rx)
    }

    /// Claim the request for running; false when its sender already gave up on it.
    pub fn take(&self) -> bool {
        use std::sync::atomic::Ordering;
        self.state.compare_exchange(PENDING, TAKEN, Ordering::SeqCst, Ordering::SeqCst).is_ok() || self.state.load(Ordering::SeqCst) == TAKEN
    }

    /// Give up on the request; false when the app has already started it (it will answer).
    pub fn abandon(state: &std::sync::atomic::AtomicU8) -> bool {
        use std::sync::atomic::Ordering;
        state.compare_exchange(PENDING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst).is_ok() || state.load(Ordering::SeqCst) == ABANDONED
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

impl Shots {
    /// A screenshot was asked for and hasn't been delivered yet: animations hold still, so the
    /// picture shows the settled state.
    pub(crate) fn in_flight(&self) -> bool {
        !self.queued.is_empty() || !self.pending.is_empty()
    }
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

/// Runs a command for an agent. Commands that move the selection scroll the active cell into
/// view, as the Go To dialog, the Name Box and the arrow keys do.
fn run(app: &mut SheetApp, id: &str, params: Json) -> Result<Json, String> {
    let r = app.run(id, params);
    if r.is_ok() && matches!(id, "edit.goTo" | "edit.goToSpecial" | "selection.set" | "selection.move") {
        app.grid.ensure_visible = true;
    }
    r
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
        "backstage": app.backstage.as_ref().map(|b| b.page.id()),
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
    Some(geo.screen_cell_rect(sh, c).center())
}

/// Most synthetic pointer events a control client may keep queued at once (the queue drains one batch per frame).
const MAX_QUEUED_POINTER_EVENTS: usize = 10_000;

/// Refuses a request that would push the queued pointer events past [`MAX_QUEUED_POINTER_EVENTS`]; the queue is left unchanged.
fn pointer_budget(app: &SheetApp, extra: usize) -> Result<(), String> {
    let queued: usize = app.synthetic.iter().map(Vec::len).sum();
    if queued.saturating_add(extra) > MAX_QUEUED_POINTER_EVENTS {
        return Err(format!("too many pointer events (limit {MAX_QUEUED_POINTER_EVENTS} queued at once)"));
    }
    Ok(())
}

fn push_click(app: &mut SheetApp, pos: egui::Pos2, button: egui::PointerButton, count: u32, m: egui::Modifiers) -> Result<(), String> {
    let count = count.max(1) as usize;
    pointer_budget(app, count.saturating_mul(2).saturating_add(1))?;
    app.synthetic.push_back(vec![egui::Event::PointerMoved(pos)]);
    let mut batch = Vec::with_capacity(count * 2);
    for _ in 0..count {
        batch.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers: m });
        batch.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers: m });
    }
    app.synthetic.push_back(batch);
    Ok(())
}

fn push_drag(app: &mut SheetApp, a: egui::Pos2, b: egui::Pos2, steps: usize, m: egui::Modifiers) -> Result<(), String> {
    let steps = steps.max(1);
    pointer_budget(app, steps.saturating_add(3))?;
    app.synthetic.push_back(vec![egui::Event::PointerMoved(a)]);
    app.synthetic.push_back(vec![egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: m }]);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        app.synthetic.push_back(vec![egui::Event::PointerMoved(a + (b - a) * t)]);
    }
    app.synthetic.push_back(vec![egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: m }]);
    Ok(())
}

/// Keeps the commands whose id, label or ribbon path contains `search` (case-insensitive); `None` keeps all.
fn filter_commands(commands: Vec<Json>, search: Option<&str>) -> Vec<Json> {
    let Some(q) = search.map(str::to_ascii_lowercase) else { return commands };
    let has = |c: &Json, k: &str| c.get(k).and_then(Json::as_str).is_some_and(|v| v.to_ascii_lowercase().contains(&q));
    let in_menu = |c: &Json| {
        let menu = c.get("menu").and_then(Json::as_array);
        menu.is_some_and(|m| m.iter().filter_map(Json::as_str).any(|v| v.to_ascii_lowercase().contains(&q)))
    };
    commands.into_iter().filter(|c| has(c, "id") || has(c, "label") || in_menu(c)).collect()
}

pub fn handle(app: &mut SheetApp, ctx: &egui::Context, method: &str, p: &Json) -> Outcome {
    let s = |k: &str| p.get(k).and_then(Json::as_str);
    let f = |k: &str| p.get(k).and_then(Json::as_f64).unwrap_or(0.0) as f32;
    let out = match method {
        "engine.execute" | "command" | "ui.menu.invoke" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            let params = p.get("params").cloned().unwrap_or(json!({}));
            wrap(run(app, id, if params.is_null() { json!({}) } else { params }))
        }
        "engine.commands" => {
            let mut v: Vec<Json> = app.session.commands().into_iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect();
            let language_params = format!("{{language: {}}} (alias: code)", crate::i18n::Language::code_list("|"));
            for (id, label, params) in [
                ("ui.ribbonTab", "Ribbon Tab", ""),
                ("view.collapseRibbon", "Collapse Ribbon", ""),
                ("view.expandFormulaBar", "Expand Formula Bar", "{on?: bool} (toggles without it) → {on}"),
                ("view.darkMode", "Dark Mode", ""),
                ("view.theme", "Display Theme", ""),
                ("view.animations", "Animations", "{on?: bool} (toggles without it) → {on}"),
                ("options.autoComplete", "Enable AutoComplete for cell values", "{on?: bool} (toggles without it) → {on}"),
                ("view.interfaceScale", "Interface Size", "{percent: 75–200 | \"auto\"} (100 = auto; clamped)"),
                ("ui.dialog", "Open Dialog", ""),
                ("ui.backstage", "File (Backstage view)", "{page?: home|new|open|info|saveAs|print|export|options|account|close}"),
                (
                    "ui.printSettings",
                    "Print Settings",
                    "{printer?: discovered name, copies?: 1..999, duplex?: printerDefault|off|longEdge|shortEdge|null, grayscale?: bool, refresh?: bool} → {printer,copies,duplex,grayscale,printers,ready,busy,discovering,status,detail}; {} inspects without submitting",
                ),
                (
                    "ui.printSubmit",
                    "Submit Print Job",
                    "existing file.print sheet/pagination params → {printing,bytes,pending}; submits without opening a dialog",
                ),
                ("app.language.set", "Interface Language", language_params.as_str()),
                ("app.language.english", "Interface Language: English", ""),
                ("app.language.japanese", "Interface Language: Japanese", ""),
            ] {
                v.push(json!({"id": id, "label": label, "params": params, "enabled": true, "ui": true}));
            }
            ok(Json::Array(filter_commands(v, s("search"))))
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
                let _ = app.run("view.formulaBar", json!({"on": v}));
            }
            if let Some(v) = p.get("ribbonCollapsed").and_then(Json::as_bool) {
                app.ui.ribbon_collapsed = v;
            }
            if let Some(v) = p.get("animations").and_then(Json::as_bool) {
                app.ui.animations = v;
            }
            let mut out = serde_json::to_value(&app.ui).unwrap_or_default();
            if let Some(o) = out.as_object_mut() {
                o.insert("formulaBar".into(), json!(app.session.prefs.formula_bar));
            }
            ok(out)
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
                    d.set_value(field, p.get("value").cloned().unwrap_or(Json::Null));
                }
                ok(json!(d.values))
            }
            None => err("no dialog is open"),
        },
        "ui.dialog.confirm" => match app.dialog.take() {
            Some(d) => match d.command {
                Some(cmd) => {
                    let params = crate::dialogs::build_params(&d);
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
            let count = p.get("count").and_then(Json::as_u64).unwrap_or(1).min(u64::from(u32::MAX)) as u32;
            match push_click(app, egui::pos2(f("x"), f("y")), button, count, modifiers(p)) {
                Ok(()) => ok(Json::Null),
                Err(e) => err(e),
            }
        }
        "ui.drag" => {
            let steps = p.get("steps").and_then(Json::as_u64).unwrap_or(8).min(u64::from(u32::MAX)) as usize;
            match push_drag(app, egui::pos2(f("x"), f("y")), egui::pos2(f("toX"), f("toY")), steps, modifiers(p)) {
                Ok(()) => ok(Json::Null),
                Err(e) => err(e),
            }
        }
        "ui.cell.click" => {
            let Some(c) = s("cell").and_then(CellRef::parse) else { return err("missing `cell`") };
            let Some(pos) = cell_center(app, c) else { return err("no grid yet") };
            let button = if s("button") == Some("right") { egui::PointerButton::Secondary } else { egui::PointerButton::Primary };
            let count = p.get("count").and_then(Json::as_u64).unwrap_or(1).min(u64::from(u32::MAX)) as u32;
            match push_click(app, pos, button, count, modifiers(p)) {
                Ok(()) => ok(json!({"x": pos.x, "y": pos.y})),
                Err(e) => err(e),
            }
        }
        "ui.cell.drag" => {
            let (Some(a), Some(b)) = (s("from").and_then(CellRef::parse), s("to").and_then(CellRef::parse)) else {
                return err("missing `from`/`to`");
            };
            let (Some(pa), Some(pb)) = (cell_center(app, a), cell_center(app, b)) else { return err("no grid yet") };
            match push_drag(app, pa, pb, 8, modifiers(p)) {
                Ok(()) => ok(Json::Null),
                Err(e) => err(e),
            }
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
            // Not `open_path`: that asks for a missing password in a dialog, and programmatic
            // calls never open one. A password-protected workbook takes `password` instead.
            let r = app.run("file.open", json!({"path": path, "password": s("password")}));
            if let Ok(r) = &r {
                app.after_open(Some(path), r);
            }
            app.after_engine();
            match (r, app.message.take()) {
                (Err(e), _) => err(crate::clean_error(&e)),
                (Ok(_), Some((_, m))) => err(m),
                (Ok(_), None) => ok(json!({"title": app.session.active().map(|d| d.display_title())})),
            }
        }
        "app.save" => wrap(app.run("file.save", json!({"path": s("path")}))),
        "app.quit" => {
            // Programmatic calls never open dialogs: quitting from here doesn't ask about unsaved work.
            app.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ok(Json::Null)
        }
        // Anything else that names a command runs it.
        other if gridcraft_engine::find_command(other).is_some() => wrap(run(app, other, p.clone())),
        other => err(format!("unknown method `{other}`")),
    };
    ctx.request_repaint();
    out
}

pub fn poll(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(rx) = app.control_rx.take() else { return };
    while let Ok(req) = rx.try_recv() {
        // A request whose sender stopped waiting is dropped unrun: running it would apply a
        // command the client was told had failed, and its reply would go nowhere (#1914).
        if !req.take() {
            continue;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Json> {
        vec![
            json!({"id": "home.borders", "label": "Borders", "menu": ["Home", "Font"]}),
            json!({"id": "home.bold", "label": "Bold", "menu": ["Home", "Font"]}),
            json!({"id": "ui.ribbonTab", "label": "Ribbon Tab", "enabled": true, "ui": true}),
        ]
    }

    fn ids(v: &[Json]) -> Vec<&str> {
        v.iter().filter_map(|c| c.get("id").and_then(Json::as_str)).collect()
    }

    #[test]
    fn search_filters_case_insensitively() {
        assert_eq!(ids(&filter_commands(sample(), Some("border"))), ["home.borders"]);
        assert_eq!(filter_commands(sample(), Some("BORDER")), filter_commands(sample(), Some("border")));
        assert_eq!(ids(&filter_commands(sample(), Some("ribbon"))), ["ui.ribbonTab"]);
        assert_eq!(ids(&filter_commands(sample(), Some("font"))), ["home.borders", "home.bold"]);
        assert!(filter_commands(sample(), Some("catalog_no_match_7391")).is_empty());
        assert_eq!(filter_commands(sample(), None).len(), 3);
    }

    fn test_app() -> SheetApp {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        SheetApp::new(session, crate::Services::default())
    }

    #[test]
    fn oversized_pointer_requests_are_refused_and_leave_the_queue_alone() {
        let mut app = test_app();
        let p = egui::pos2(20.0, 20.0);
        let m = egui::Modifiers::NONE;
        let btn = egui::PointerButton::Primary;
        assert!(push_click(&mut app, p, btn, 2, m).is_ok());
        assert_eq!(app.synthetic.iter().map(Vec::len).sum::<usize>(), 5);
        assert!(push_click(&mut app, p, btn, u32::MAX, m).is_err());
        assert!(push_drag(&mut app, p, p, usize::MAX, m).is_err());
        assert!(push_drag(&mut app, p, p, MAX_QUEUED_POINTER_EVENTS, m).is_err());
        assert_eq!(app.synthetic.len(), 2);
        assert_eq!(app.synthetic.iter().map(Vec::len).sum::<usize>(), 5);
        // Already queued input counts toward the budget.
        assert!(push_click(&mut app, p, btn, (MAX_QUEUED_POINTER_EVENTS / 2) as u32, m).is_err());
        assert_eq!(app.synthetic.len(), 2);
        // Normal requests still queue in full.
        assert!(push_drag(&mut app, p, p + egui::vec2(40.0, 40.0), 3, m).is_ok());
        assert_eq!(app.synthetic.len(), 8);
    }

    #[test]
    fn print_command_discovery_documents_settings_and_submission() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, crate::Services::default());
        let Outcome::Done(reply) = handle(&mut app, &egui::Context::default(), "engine.commands", &json!({"search":"print"})) else {
            panic!("command discovery must return immediately");
        };
        assert_eq!(reply["ok"], true);
        let commands = reply["result"].as_array().unwrap();
        for (id, expected_docs) in [
            ("ui.printSettings", &["copies", "duplex", "refresh", "discovering", "status", "detail"][..]),
            ("ui.printSubmit", &["file.print", "pagination", "pending", "without opening a dialog"][..]),
        ] {
            let command = commands.iter().find(|command| command["id"] == id).unwrap();
            assert_eq!(command["ui"], true);
            let docs = command["params"].as_str().unwrap();
            for expected in expected_docs {
                assert!(docs.contains(expected), "{id}: missing {expected}");
            }
        }
        assert!(!app.printing.busy(), "discovery must not submit a job");
        assert!(app.backstage.is_none(), "discovery must not open settings");
    }

    #[test]
    fn language_command_docs_list_every_language() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, crate::Services::default());
        let Outcome::Done(reply) = handle(&mut app, &egui::Context::default(), "engine.commands", &json!({"search":"language"})) else {
            panic!("command discovery must return immediately");
        };
        let commands = reply["result"].as_array().unwrap();
        let docs = commands.iter().find(|c| c["id"] == "app.language.set").unwrap()["params"].as_str().unwrap();
        for l in crate::i18n::Language::ALL {
            assert!(docs.contains(&format!("\"{}\"", l.code())), "app.language.set docs miss {:?}: {docs}", l.code());
        }
    }
}
