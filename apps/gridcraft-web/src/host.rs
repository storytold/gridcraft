//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! GridCraft in an `<iframe>` (such as a Nextcloud app). Messages are plain objects with a `type`:
//!
//! - `gridcraft:ready` (to the parent): `{ version, capabilities }`, the app is listening; send a
//!   file now. `capabilities` names the optional messages it understands: `["control"]`.
//! - `gridcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a workbook picked with File › Open or dropped on the window.
//! - `gridcraft:control` (from the parent): `{ id, method, params? }`, a control-channel request
//!   (the desktop channel's methods, `docs/control-protocol.md`), answered with `gridcraft:reply`
//!   (to the parent): `{ id, ok: true, result }` or `{ id, ok: false, error }` (see
//!   [`crate::protocol`]). Opens and requests run in the order they arrive.
//! - `gridcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file GridCraft would otherwise download: Save and Save As (an .xlsx workbook) and Export
//!   as PDF. A workbook handed over by Save counts as saved from then on; the parent stores it
//!   and reports failures itself.
//! - `gridcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away,
//!   also while the page is hidden.
//! - `gridcraft:failed` (to the parent): `{ error }` when the app couldn't start, or when the
//!   page's URL has no origin (`location.origin` is `"null"`), so its parent can't be checked:
//!   host mode then stays off.
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the page
//! starts with an empty workbook instead of a sample, and the parent guards leaving the page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::Value as Json;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

use crate::protocol::{self, Queue};

/// Saving one of these hands the whole workbook to the parent, which stores it: the workbook
/// counts as saved under that name, as it would after Save on the desktop.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

/// The longest message id read before a message's text (to label an error about the text).
const MAX_ID_CHARS: u32 = 256;

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    /// Workbooks to open and control requests from the parent, in the order they arrived.
    queue: Rc<RefCell<Queue>>,
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
        if origin == "null" {
            // The URL has an opaque origin (a scheme the browser gives no origin, a data: URL):
            // it equals no other page's and can't address a message (`postMessage` throws on
            // "null"), so the parent can't be checked. Say so, in a fixed text any page may read,
            // and run as a plain page.
            let error = "GridCraft's page has no origin (location.origin is \"null\"), so it can't check that its parent is on \
                         the same origin; host mode is off";
            log::error!("{error}");
            if let Some(m) = message(&[("type", "gridcraft:failed".into()), ("error", error.into())]) {
                let _ = parent.post_message(&m, "*");
            }
            return None;
        }
        Some(Self { window, parent, origin, reported: Rc::default(), queue: Rc::default() })
    }

    /// Send saved and exported files to the parent instead of downloading them, and hear the
    /// workbooks and control requests it sends.
    pub fn connect(&self, services: &mut Services, ctx: egui::Context) {
        self.listen(ctx);
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| match bridge.post_save(name, bytes) {
            // The parent stores what it gets: a workbook it got is the workbook's own copy.
            Ok(()) => is_workbook(name),
            Err(e) => {
                log::error!("couldn't hand {name} to the page: {e}");
                false
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
        let capabilities = js_sys::Array::of1(&JsValue::from_str("control"));
        self.send(&[("type", "gridcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into()), ("capabilities", capabilities.into())]);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "gridcraft:failed".into()), ("error", error.into())]);
    }

    /// After the app's per-frame logic, which also runs while the page is hidden (when the
    /// browser runs no `ui` pass): run what the parent sent, in order, answer its requests, and
    /// report a change in unsaved work.
    pub fn after_logic(&self, app: &mut SheetApp, ctx: &egui::Context) {
        let pending = self.queue.borrow_mut().take();
        for reply in pending.run(app, ctx) {
            self.post_reply(&reply);
        }
        self.report_dirty(app);
    }

    /// After each frame's `ui` pass, where the user's edits land: report a change in unsaved work.
    pub fn sync(&self, app: &SheetApp) {
        self.report_dirty(app);
    }

    fn report_dirty(&self, app: &SheetApp) {
        let dirty = app.session.documents().iter().any(|d| d.is_dirty());
        if self.reported.get() != Some(dirty) && self.send(&[("type", "gridcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
    }

    fn post_save(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let message = message(&[("type", "gridcraft:save".into()), ("name", name.as_str().into()), ("bytes", buffer.clone().into())])
            .ok_or_else(|| "couldn't build the message".to_string())?;
        self.parent
            .post_message_with_transfer(&message, &self.origin, &js_sys::Array::of1(&buffer))
            .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
        Ok(())
    }

    fn listen(&self, ctx: egui::Context) {
        let bridge = self.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != bridge.origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &bridge.parent)) {
                return;
            }
            let data = event.data();
            match string(&data, "type").as_deref() {
                Some("gridcraft:open") => {
                    let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Book.xlsx".to_string());
                    let Some(bytes) = bytes(&data) else { return };
                    bridge.queue.borrow_mut().open(name, bytes);
                }
                Some(protocol::CONTROL) => bridge.hear_control(&data),
                _ => return,
            }
            ctx.request_repaint();
        });
        if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_message.forget();
        }
    }

    /// A control request waits its turn; one that can't be served is answered at once.
    fn hear_control(&self, data: &JsValue) {
        let id = message_id(data);
        let reply = match json_text(data) {
            Ok(text) => self.queue.borrow_mut().control(&text, id),
            Err(error) => Some(protocol::error(id, error)),
        };
        if let Some(reply) = reply {
            self.post_reply(&reply);
        }
    }

    /// Posts an answer; if it can't go (too large to hand over, say), a short error under the
    /// same id takes its place, so the parent isn't left waiting.
    fn post_reply(&self, reply: &Json) {
        if self.post_json(reply) {
            return;
        }
        let id = reply.get("id").cloned().unwrap_or(Json::Null);
        if !self.post_json(&protocol::error(id.clone(), "the answer couldn't be handed to the page")) {
            log::error!("couldn't answer the page's control request {id}");
        }
    }

    fn post_json(&self, value: &Json) -> bool {
        serde_json::to_string(value)
            .ok()
            .and_then(|text| js_sys::JSON::parse(&text).ok())
            .is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok())
    }

    /// Post a message to the parent; false if it couldn't be sent.
    fn send(&self, fields: &[(&str, JsValue)]) -> bool {
        message(fields).is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok())
    }
}

/// A control message as JSON text. Binary values are refused rather than spelled out byte by
/// byte, and a message over the limit is refused before it is copied out of JavaScript: its
/// length in UTF-16 units is never more than its size in UTF-8 bytes.
fn json_text(data: &JsValue) -> Result<String, &'static str> {
    const UNREADABLE: &str = "the message can't be read as JSON";
    const BINARY: &str = "a control message can't carry an ArrayBuffer or typed array: send bytes as a base64 string, \
                          or a workbook as gridcraft:open";
    let mut binary = false;
    let text = js_sys::JSON::stringify_with_replacer_func(
        data,
        &mut |_key, value| {
            if js_sys::ArrayBuffer::is_view(&value) || value.is_instance_of::<js_sys::ArrayBuffer>() {
                binary = true;
                return Err(wasm_bindgen::JsError::new(BINARY));
            }
            Ok(Some(value))
        },
        None,
    );
    let text = match text {
        Ok(text) if text.is_string() => text,
        _ => return Err(if binary { BINARY } else { UNREADABLE }),
    };
    if text.length() as usize > protocol::MAX_MESSAGE {
        return Err(protocol::TOO_LARGE);
    }
    text.as_string().ok_or(UNREADABLE)
}

/// The message's `id`, read before its text so an error about the text can carry it: a number or
/// a short string, else null.
fn message_id(data: &JsValue) -> Json {
    let id = js_sys::Reflect::get(data, &JsValue::from_str("id")).unwrap_or(JsValue::UNDEFINED);
    if let Some(text) = id.dyn_ref::<js_sys::JsString>() {
        return if text.length() <= MAX_ID_CHARS { text.as_string().map_or(Json::Null, Json::String) } else { Json::Null };
    }
    match id.as_f64() {
        // Whole numbers come back as integers, as the parent wrote them.
        Some(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => Json::from(n as i64),
        Some(n) => serde_json::Number::from_f64(n).map_or(Json::Null, Json::Number),
        None => Json::Null,
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
