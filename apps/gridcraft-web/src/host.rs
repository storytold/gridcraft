//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! GridCraft in an `<iframe>` (such as a Nextcloud app). Messages are plain objects with a `type`:
//!
//! - `gridcraft:ready` (to the parent): `{ version }`, the app is listening; send a file now.
//! - `gridcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a workbook picked with File › Open or dropped on the window.
//! - `gridcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file GridCraft would otherwise download: Save and Save As (an .xlsx workbook) and Export
//!   as PDF. The parent stores it and reports failures itself.
//! - `gridcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `gridcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//! - `gridcraft:command` (from the parent): `{ id, method, params }`, any method of the desktop
//!   control protocol (`docs/control-protocol.md`): `engine.execute` runs any command
//!   (`cell.set`, `selection.set`, `home.bold`…), `cell.get` / `sheet.read` /
//!   `document.inspect` read, `ui.cell.click` clicks. It runs on the next frame, like a command
//!   the user gave, and is undoable the same way.
//! - `gridcraft:result` (to the parent): `{ id, ok, result | error }`, one per command.
//! - `gridcraft:selection` (to the parent): `{ sheet, active, ranges }` (A1 text) once when a
//!   workbook opens and whenever the selection or the active sheet changes.
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the page
//! starts with an empty workbook instead of a sample, and the parent guards leaving the page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gridcraft_ui_egui::control::{self, Outcome};
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use serde_json::{Value as Json, json};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

/// Saving one of these hands the whole workbook to the parent, which stores it: the workbook
/// counts as saved under that name, as it would after Save on the desktop.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

/// Commands the parent may queue between two frames; more are refused, not buffered.
const MAX_QUEUED_COMMANDS: usize = 1_000;

/// A `gridcraft:command` waiting for the next frame.
struct Command {
    id: Json,
    method: String,
    params: Json,
}

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// Names of the files handed to the parent since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    /// Commands from the parent, run on the next frame.
    commands: Rc<RefCell<Vec<Command>>>,
    /// The selection last reported to the parent.
    selection: Rc<RefCell<Option<Json>>>,
}

impl Bridge {
    /// `None` when the page isn't framed (there is no parent to talk to).
    pub fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let origin = window.location().origin().ok()?;
        Some(Self { window, parent, origin, saved: Rc::default(), reported: Rc::default(), commands: Rc::default(), selection: Rc::default() })
    }

    /// Send saved and exported files to the parent instead of downloading them, and hear files
    /// from it.
    pub fn connect(&self, services: &mut Services, inbox: Inbox, ctx: egui::Context) {
        self.listen(inbox, ctx);
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| {
            if let Err(e) = bridge.post_save(name, bytes) {
                log::error!("couldn't hand {name} to the page: {e}");
            }
        }));
    }

    /// `?author=`: the host's name for the user signs new notes and comments.
    pub fn set_author(app: &mut SheetApp, author: &str) {
        let author: String = author.trim().chars().take(MAX_AUTHOR_CHARS).collect();
        if !author.is_empty() {
            app.session.prefs.user_name = author;
        }
    }

    pub fn post_ready(&self) {
        self.send(&[("type", "gridcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into())]);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "gridcraft:failed".into()), ("error", error.into())]);
    }

    /// After each frame: the parent's queued commands run, a workbook handed to the parent this
    /// frame counts as saved under the name it was saved as, and the parent hears when unsaved
    /// changes appear or go away and when the selection moves.
    pub fn sync(&self, app: &mut SheetApp, ctx: &egui::Context) {
        let commands = std::mem::take(&mut *self.commands.borrow_mut());
        if !commands.is_empty() {
            for command in commands {
                let reply = match control::handle(app, ctx, &command.method, &command.params) {
                    Outcome::Done(reply) => reply,
                    Outcome::Screenshot { .. } => json!({"ok": false, "error": "ui.screenshot is not available in the browser"}),
                };
                self.post_result(command.id, &reply);
            }
            ctx.request_repaint();
        }
        let saved = std::mem::take(&mut *self.saved.borrow_mut());
        if let Some(name) = saved.iter().rev().find(|name| is_workbook(name))
            && let Some(doc) = app.session.active_mut()
        {
            doc.saved = doc.wb.clone();
            if doc.path.is_none() {
                doc.title = name.clone();
            }
        }
        let dirty = app.session.documents().iter().any(|d| d.is_dirty());
        if self.reported.get() != Some(dirty) && self.send(&[("type", "gridcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
        let selection = selection_of(app);
        if *self.selection.borrow() != selection {
            let sent = match &selection {
                Some(current) => {
                    let mut message = current.clone();
                    if let Some(fields) = message.as_object_mut() {
                        fields.insert("type".into(), json!("gridcraft:selection"));
                    }
                    self.send_json(&message)
                }
                None => true,
            };
            if sent {
                *self.selection.borrow_mut() = selection;
            }
        }
    }

    /// `reply` is the control protocol's `{ok, result}` or `{ok: false, error}`.
    fn post_result(&self, id: Json, reply: &Json) {
        let mut message = reply.clone();
        if let Some(fields) = message.as_object_mut() {
            fields.insert("type".into(), json!("gridcraft:result"));
            fields.insert("id".into(), id);
        }
        self.send_json(&message);
    }

    /// Post a JSON message to the parent; false if it couldn't be sent.
    fn send_json(&self, value: &Json) -> bool {
        to_js(value).is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok())
    }

    fn post_save(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let message = message(&[("type", "gridcraft:save".into()), ("name", name.as_str().into()), ("bytes", buffer.clone().into())])
            .ok_or_else(|| "couldn't build the message".to_string())?;
        self.parent
            .post_message_with_transfer(&message, &self.origin, &js_sys::Array::of1(&buffer))
            .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
        self.saved.borrow_mut().push(name);
        Ok(())
    }

    fn listen(&self, inbox: Inbox, ctx: egui::Context) {
        let parent = self.parent.clone();
        let origin = self.origin.clone();
        let commands = self.commands.clone();
        let bridge = self.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            if string(&data, "type").as_deref() == Some("gridcraft:command") {
                let id = js_sys::Reflect::get(&data, &JsValue::from_str("id")).ok().and_then(|id| from_js(&id)).unwrap_or(Json::Null);
                let Some(method) = string(&data, "method").filter(|m| !m.is_empty()) else {
                    bridge.post_result(id, &json!({"ok": false, "error": "missing `method`"}));
                    return;
                };
                let params = js_sys::Reflect::get(&data, &JsValue::from_str("params")).ok().and_then(|p| from_js(&p)).unwrap_or(json!({}));
                let mut queue = commands.borrow_mut();
                if queue.len() >= MAX_QUEUED_COMMANDS {
                    drop(queue);
                    bridge.post_result(id, &json!({"ok": false, "error": "too many queued commands"}));
                    return;
                }
                queue.push(Command { id, method, params: if params.is_null() { json!({}) } else { params } });
                ctx.request_repaint();
                return;
            }
            if string(&data, "type").as_deref() != Some("gridcraft:open") {
                return;
            }
            let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Book.xlsx".to_string());
            let Some(bytes) = bytes(&data) else { return };
            inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
            ctx.request_repaint();
        });
        if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_message.forget();
        }
    }

    /// Post a message to the parent; false if it couldn't be sent.
    fn send(&self, fields: &[(&str, JsValue)]) -> bool {
        message(fields).is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok())
    }
}

fn is_workbook(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| WORKBOOK_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn message(fields: &[(&str, JsValue)]) -> Option<js_sys::Object> {
    let m = js_sys::Object::new();
    for (key, value) in fields {
        js_sys::Reflect::set(&m, &JsValue::from_str(key), value).ok()?;
    }
    Some(m)
}

/// The active workbook's selection as the parent sees it: sheet name and A1 text.
fn selection_of(app: &SheetApp) -> Option<Json> {
    let doc = app.session.active()?;
    let sheet = doc.wb.active().map(|s| s.name.clone()).unwrap_or_default();
    let ranges: Vec<String> = doc.selection.ranges.iter().map(|r| r.a1()).collect();
    Some(json!({"sheet": sheet, "active": doc.selection.active.a1(), "ranges": ranges}))
}

fn to_js(value: &Json) -> Option<JsValue> {
    js_sys::JSON::parse(&serde_json::to_string(value).ok()?).ok()
}

fn from_js(value: &JsValue) -> Option<Json> {
    if value.is_undefined() {
        return None;
    }
    let text = js_sys::JSON::stringify(value).ok()?.as_string()?;
    serde_json::from_str(&text).ok()
}

fn string(data: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_string()
}

/// `bytes` as an `ArrayBuffer` or `Uint8Array`.
fn bytes(data: &JsValue) -> Option<Vec<u8>> {
    let bytes = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
    if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
        Some(array.to_vec())
    } else if bytes.is_instance_of::<js_sys::ArrayBuffer>() {
        Some(js_sys::Uint8Array::new(&bytes).to_vec())
    } else {
        None
    }
}
