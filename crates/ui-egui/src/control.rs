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

use craft_ui::capture::CaptureSlot;
use gridcraft_engine::core::CellRef;
use serde_json::{Value as Json, json};

use crate::SheetApp;

mod screenshots;
mod synthetic;

pub use screenshots::{
    CaptureGeometry, SCREENSHOT_DOCUMENT_CHANGED, SCREENSHOT_MAX_AGGREGATE_BYTES, SCREENSHOT_MAX_BASE64_BYTES, SCREENSHOT_MAX_ENCODING_BYTES,
    SCREENSHOT_MAX_IMAGE_BYTES, SCREENSHOT_MAX_PEAK_BYTES, SCREENSHOT_MAX_REQUESTS, SCREENSHOT_MAX_RETRIES, SCREENSHOT_VIEWPORT_CHANGED, Shots,
    UNKNOWN_CAPTURE_BYTES, drain_screenshots, encoding_working_bytes, issue_screenshots, save_image,
};
use screenshots::{Shot, check_encoding_budget, check_peak_budget, reject_shot};
#[cfg(test)]
use screenshots::{base64_len, png_worst_bytes, validate_image};
pub(crate) use screenshots::{current_capture_geometry, note_capture_geometry};
pub use synthetic::{MAX_QUEUED_SYNTHETIC_BYTES, MAX_QUEUED_SYNTHETIC_EVENTS, MAX_SYNTHETIC_TEXT_BYTES};
use synthetic::{cell_center, key_from, modifiers, push_click, push_drag, push_key, push_move, push_text};

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

/// [`run`] in another window (`window` or `doc` from the request), bringing back the active
/// window afterwards unless the command itself switched windows.
fn run_in(app: &mut SheetApp, target: gridcraft_engine::Target, id: &str, params: Json) -> Result<Json, String> {
    let guard = app.session.enter_target(target).map_err(|e| e.to_string())?;
    let r = run(app, id, params);
    app.session.leave_target(guard);
    r
}

/// A command called by its id as the method. Its params may name a `window` or `doc` to act in,
/// except for the window commands, whose own params those are.
fn direct(app: &mut SheetApp, id: &str, p: &Json, via_app: bool) -> Result<Json, String> {
    let mut params = p.clone();
    let target = if gridcraft_engine::windows::is_window_command(id) {
        gridcraft_engine::Target::Active
    } else {
        let t = gridcraft_engine::Target::from_json(p).map_err(|e| e.to_string())?;
        if let Some(o) = params.as_object_mut() {
            o.remove("window");
            o.remove("doc");
        }
        t
    };
    if via_app {
        return run_in(app, target, id, params);
    }
    let guard = app.session.enter_target(target).map_err(|e| e.to_string())?;
    let r = app.session.run(id, params);
    app.session.leave_target(guard);
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
        "windows": app.session.window_list(),
        "activeWindow": app.session.active_window(),
        "perf": {"frameMs": app.perf.frame_ms, "gridMs": app.perf.grid_ms, "fps": app.perf.fps},
    })
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

/// Dispatches one control request: commands, inspection, UI state, dialogs, editing, synthetic input, open/save/quit and screenshots. Unknown methods (and unknown command ids) return an error. Immediate answers request a repaint; `ui.screenshot` is admitted asynchronously instead of being answered here.
pub fn handle(app: &mut SheetApp, ctx: &egui::Context, method: &str, p: &Json) -> Outcome {
    let s = |k: &str| p.get(k).and_then(Json::as_str);
    let f = |k: &str| p.get(k).and_then(Json::as_f64).unwrap_or(0.0) as f32;
    let out = match method {
        "engine.execute" | "command" | "ui.menu.invoke" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            let params = p.get("params").cloned().unwrap_or(json!({}));
            let target = match gridcraft_engine::Target::from_json(p) {
                Ok(t) => t,
                Err(e) => return err(e),
            };
            wrap(run_in(app, target, id, if params.is_null() { json!({}) } else { params }))
        }
        "engine.commands" => {
            let mut v: Vec<Json> = app.session.commands().into_iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect();
            let language_params = format!("{{language: {}}} (alias: code)", crate::i18n::Language::code_list("|"));
            for (id, label, params) in [
                ("ui.ribbonTab", "Ribbon Tab", ""),
                ("app.checkForUpdates", "Check for Updates…", ""),
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
        "document.inspect" | "sheet.read" | "cell.get" | "selection.stats" => wrap(direct(app, method, p, false)),
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
                    let home = app.ui_window();
                    let r = app.run_dialog_command(cmd, params);
                    if r.is_err() {
                        app.return_dialog(home, d);
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
            // The macro step editor's draft, overlays and debug run go with its own close.
            if app.dialog.as_ref().is_some_and(|d| d.name == "macroEditor") {
                return match crate::macro_editor::close_now(app) {
                    Ok(()) => ok(Json::Null),
                    Err(e) => err(e),
                };
            }
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
            match push_key(app, k, modifiers(p)) {
                Ok(()) => ok(Json::Null),
                Err(e) => err(e),
            }
        }
        "ui.text" => {
            let text = s("text").unwrap_or("");
            // Check the borrowed payload first: do not make a second, potentially large copy
            // before the per-event bound is known to hold.
            if text.len() > MAX_SYNTHETIC_TEXT_BYTES {
                return err(format!("text input is {} bytes, over the {MAX_SYNTHETIC_TEXT_BYTES}-byte limit for one event", text.len()));
            }
            match push_text(app, text.to_string()) {
                Ok(()) => ok(Json::Null),
                Err(e) => err(e),
            }
        }
        "ui.move" => match push_move(app, egui::pos2(f("x"), f("y"))) {
            Ok(()) => ok(Json::Null),
            Err(e) => err(e),
        },
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
        other if gridcraft_engine::find_command(other).is_some() => wrap(direct(app, other, p, true)),
        other => err(format!("unknown method `{other}`")),
    };
    ctx.request_repaint();
    out
}

/// Drains queued control requests, drops requests whose client stopped waiting, handles immediate outcomes and admits screenshots, then requests a repaint while synthetic input remains.
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
            Outcome::Screenshot { path } => admit(app, ctx, path, req.reply),
        }
    }
    app.control_rx = Some(rx);
    if !app.synthetic.is_empty() {
        ctx.request_repaint();
    }
}

/// Put the request token on every screenshot reply (success, rejection, timeout), so a client can
/// correlate an asynchronous capture even when the reply order differs from the request order.
fn with_token(mut reply: Json, token: u64) -> Json {
    if let Some(object) = reply.as_object_mut() {
        object.insert("request".into(), json!(token));
    }
    reply
}

/// Returns the next screenshot token, wrapping and skipping zero.
fn next_token(app: &mut SheetApp) -> u64 {
    let mut token = app.shots.token.wrapping_add(1);
    if token == 0 {
        token = 1;
    }
    app.shots.token = token;
    token
}

/// Admit one screenshot request against the count and byte budgets **before** any capture is
/// issued. A rejected request gets one error reply and never evicts an earlier request.
fn admit(app: &mut SheetApp, ctx: &egui::Context, path: Option<String>, reply: Sender<ControlResponse>) {
    let token = next_token(app);
    let geometry = current_capture_geometry(app, ctx);
    let expected = geometry.expected_size();
    let base64 = path.is_none();
    // A minimized or not-yet-laid-out window has no size: reserve the largest image the
    // per-image limit allows and keep the request queued. Issuance waits for a real size, and the
    // deadline reports a timeout if none arrives.
    let bytes = match expected {
        Some(size) => {
            let Some(bytes) = geometry.retained_bytes() else {
                reject_shot(reply, token, "screenshot failed: the window size overflows the screenshot budget");
                return;
            };
            if bytes > SCREENSHOT_MAX_IMAGE_BYTES {
                reject_shot(
                    reply,
                    token,
                    format!(
                        "screenshot budget exceeded: {bytes} bytes for one {}×{} image, over the {SCREENSHOT_MAX_IMAGE_BYTES}-byte per-image limit",
                        size[0], size[1]
                    ),
                );
                return;
            }
            if let Err(message) = check_encoding_budget(size, base64) {
                reject_shot(reply, token, message);
                return;
            }
            bytes
        }
        None => UNKNOWN_CAPTURE_BYTES,
    };
    if app.shots.requests.len() >= SCREENSHOT_MAX_REQUESTS {
        reject_shot(
            reply,
            token,
            format!("screenshot budget exceeded: {} requests are already outstanding (limit {SCREENSHOT_MAX_REQUESTS})", app.shots.requests.len()),
        );
        return;
    }
    let reserved = app.shots.reserved().saturating_add(bytes);
    if reserved > SCREENSHOT_MAX_AGGREGATE_BYTES {
        reject_shot(
            reply,
            token,
            format!(
                "screenshot budget exceeded: {reserved} bytes reserved by outstanding requests, over the {SCREENSHOT_MAX_AGGREGATE_BYTES}-byte aggregate limit"
            ),
        );
        return;
    }
    if let Some(size) = expected
        && let Err(message) = check_peak_budget(reserved, size, base64)
    {
        reject_shot(reply, token, message);
        return;
    }
    let now = crate::monotonic_ms();
    let document = app.session.active().map(|d| d.uid);
    let epoch = note_capture_geometry(app, geometry);
    app.shots.requests.push(Shot {
        token,
        path,
        reply,
        deadline: now + 8000.0,
        ready_at: now + 100.0,
        frames: 0,
        settle_pass: None,
        issued: false,
        retries: 0,
        geometry,
        viewport_id: geometry.viewport_id,
        document,
        epoch,
        expected,
        bytes,
        // Issued only from `ui`, so the slot is created there against the geometry in force.
        slot: None,
    });
    // A queued request with a capturable size has not issued a paint-requesting command yet;
    // `logic` keeps asking for hidden paints until `issue_screenshots` runs. A size-less request
    // asks for none (see `Shots::needs_hidden_paint`).
    ctx.request_repaint();
}

/// Builds the standard token-carrying timeout reply for a screenshot request.
fn timeout_reply(message: &str, token: u64) -> Json {
    json!({"ok": false, "error": message, "request": token})
}

#[cfg(test)]
mod tests;
