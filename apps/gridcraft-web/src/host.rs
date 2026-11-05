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
//! - `gridcraft:mcp` (from the parent): `{ id, method, params }`, with a unique string id and
//!   `tools/list` or `tools/call`; `gridcraft:mcp-result` returns `{ id, result }`.
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the page
//! starts with an empty workbook instead of a sample, and the parent guards leaving the page.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use gridcraft_mcp::{Backend, ToolResult};
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use serde_json::{Value, json};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

/// Saving one of these hands the whole workbook to the parent, which stores it: the workbook
/// counts as saved under that name, as it would after Save on the desktop.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

// Retain request identities for this page's lifetime. At the bound, reject new work rather
// than evict identities and risk executing a delayed duplicate mutation a second time.
const MAX_MCP_IDS: usize = 16_384;
const MAX_MCP_QUEUE: usize = 128;

#[derive(Default)]
struct McpInbox {
    seen: HashSet<String>,
    queued: Vec<(String, Value)>,
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
    /// Requests are admitted by the listener and run only on the app's normal frame thread.
    mcp: Rc<RefCell<McpInbox>>,
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
        Some(Self { window, parent, origin, saved: Rc::default(), reported: Rc::default(), mcp: Rc::default() })
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

    /// Reuse upstream MCP schemas, dispatch and command handling against the visible workbook.
    pub fn poll_mcp(&self, app: &mut SheetApp, ctx: &egui::Context) {
        let queued = std::mem::take(&mut self.mcp.borrow_mut().queued);
        for (id, request) in queued {
            let result = match request.get("method").and_then(Value::as_str) {
                Some("tools/list") => json!({"tools": gridcraft_mcp::tool_definitions()}),
                Some("tools/call") => {
                    let params = request.get("params").unwrap_or(&Value::Null);
                    match params.get("name").and_then(Value::as_str) {
                        Some(name) => {
                            gridcraft_mcp::call_tool(&mut BrowserBackend { app, ctx }, name, params.get("arguments").unwrap_or(&Value::Null))
                                .to_value()
                        }
                        None => ToolResult::error("tools/call requires a tool name").to_value(),
                    }
                }
                _ => ToolResult::error("unsupported browser MCP method; use tools/list or tools/call").to_value(),
            };
            self.post_mcp(&id, result);
        }
    }

    fn post_mcp(&self, id: &str, result: Value) {
        // JSON round-tripping retains the upstream MCP content-block/result shape.
        if let Ok(result) = js_sys::JSON::parse(&result.to_string()) {
            self.send(&[("type", "gridcraft:mcp-result".into()), ("id", id.into()), ("result", result)]);
        }
    }

    fn enqueue_mcp(&self, data: &JsValue, ctx: &egui::Context) {
        let Some(id) = string(data, "id").filter(|id| !id.is_empty() && id.len() <= 200) else {
            self.post_mcp("", ToolResult::error("browser MCP requires a nonempty string id of at most 200 bytes").to_value());
            return;
        };
        let mut inbox = self.mcp.borrow_mut();
        let error = if inbox.seen.contains(&id) {
            Some("duplicate browser MCP request id; the request will not run again")
        } else if inbox.seen.len() >= MAX_MCP_IDS {
            Some("browser MCP request identity limit reached; new work is blocked for this page")
        } else {
            // Remember even a rejected request: redelivery can never turn rejection into a mutation.
            inbox.seen.insert(id.clone());
            if inbox.queued.len() >= MAX_MCP_QUEUE { Some("browser MCP request queue is full") } else { None }
        };
        if let Some(error) = error {
            drop(inbox);
            self.post_mcp(&id, ToolResult::error(error).to_value());
            return;
        }
        let request = js_sys::JSON::stringify(data).ok().and_then(|s| s.as_string()).and_then(|s| serde_json::from_str::<Value>(&s).ok());
        match request {
            Some(request) => {
                inbox.queued.push((id, request));
                ctx.request_repaint();
            }
            None => {
                drop(inbox);
                self.post_mcp(&id, ToolResult::error("invalid browser MCP request").to_value());
            }
        }
    }

    /// After each frame: a workbook handed to the parent this frame counts as saved under the
    /// name it was saved as, and the parent hears when unsaved changes appear or go away.
    pub fn sync(&self, app: &mut SheetApp) {
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
        let bridge = self.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            if string(&data, "type").as_deref() == Some("gridcraft:mcp") {
                bridge.enqueue_mcp(&data, &ctx);
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

/// A borrowing backend routes upstream MCP tools into the app; it never creates another Session.
struct BrowserBackend<'a> {
    app: &'a mut SheetApp,
    ctx: &'a egui::Context,
}

impl Backend for BrowserBackend<'_> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        // Browser screenshots require asynchronous frame capture and are outside this adapter.
        if method == "ui.screenshot" {
            return Err("browser screenshots are not supported by this MCP adapter".into());
        }
        match gridcraft_ui_egui::control::handle(self.app, self.ctx, method, &params) {
            gridcraft_ui_egui::control::Outcome::Done(reply) => {
                if reply.get("ok").and_then(Value::as_bool) == Some(true) {
                    Ok(reply.get("result").cloned().unwrap_or(Value::Null))
                } else {
                    Err(reply.get("error").and_then(Value::as_str).unwrap_or("browser command failed").to_string())
                }
            }
            gridcraft_ui_egui::control::Outcome::Screenshot { .. } => Err("asynchronous browser screenshots are unsupported".into()),
        }
    }

    fn has_ui(&self) -> bool {
        false
    }

    fn describe(&self) -> String {
        "the visible browser workbook".into()
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
