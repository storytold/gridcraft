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
//! - `gridcraft:cmd` (from the parent): `{ id, params?, requestId? }` runs the engine command
//!   `id` (`cell.set`, `home.bold`, …) like a ribbon button. `params` is a JSON string (absent
//!   or `null` means `{}`), and `requestId` an optional string that comes back in the response.
//! - `gridcraft:response` (to the parent): `{ id, requestId?, ok, value? }` or
//!   `{ id, requestId?, ok: false, error }` for every `gridcraft:cmd`, once it has run.
//! - `gridcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the page
//! starts with an empty workbook instead of a sample, and the parent guards leaving the page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

/// Saving one of these hands the whole workbook to the parent, which stores it: the workbook
/// counts as saved under that name, as it would after Save on the desktop.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

/// Largest `id`/`requestId` accepted from the host.
const MAX_ID_BYTES: usize = 1024;

/// Largest `params` string accepted from the host; a command's parameters are small, and the
/// string is parsed into memory.
const MAX_PARAMS_BYTES: usize = 1 << 20;

/// Most commands waiting for the next frame. The queue is drained once per frame, so a throttled
/// tab or a parent that sends faster than frames run would otherwise grow it without bound
/// (the same caps the WordCraft and DeckCraft bridges use).
const MAX_PENDING_COMMANDS: usize = 256;

/// Most `params` text waiting for the next frame, all commands together.
const MAX_PENDING_PARAMS_BYTES: usize = 8 << 20;

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// Names of the files handed to the parent since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    /// Commands the parent sent since the last frame: (id, params JSON string, request id).
    commands: Rc<RefCell<Vec<(String, String, Option<String>)>>>,
    /// Total length of the `params` text of the commands that haven't run yet.
    queued_bytes: Rc<Cell<usize>>,
    /// Total length of the command identifiers that haven't run yet.
    queued_id_bytes: Rc<Cell<usize>>,
    /// The workbook the host opened, by the exact name it was opened as
    /// (the app titles documents with the full file name): docClosed fires once no document
    /// carries it (the app itself always keeps a blank Book around otherwise).
    host_doc: Rc<RefCell<Option<String>>>,
}

impl Bridge {
    /// `None` when the page isn't framed (there is no parent to talk to).
    pub fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let origin = window.location().origin().ok().filter(|o| !matches!(o.as_str(), "" | "null" | "*"))?;
        Some(Self {
            window,
            parent,
            origin,
            saved: Rc::default(),
            reported: Rc::default(),
            commands: Rc::default(),
            queued_bytes: Rc::default(),
            queued_id_bytes: Rc::default(),
            host_doc: Rc::default(),
        })
    }

    /// Remember the workbook the host opened (the app titles documents with the full file
    /// name), to notice it closing later.
    pub fn set_host_doc(&self, name: &str) {
        *self.host_doc.borrow_mut() = Some(name.to_string());
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

    /// How a host command ended (`gridcraft:cmd`), like the WordCraft and DeckCraft bridges:
    /// the response echoes the command's `id` and the host's optional `requestId`; the result
    /// lands in `value`, the failure in `error`.
    pub fn post_response(&self, id: &str, request_id: Option<&str>, ok: bool, text: Option<&str>) {
        let mut fields: Vec<(&str, JsValue)> = vec![("type", "gridcraft:response".into()), ("id", id.into())];
        if let Some(request_id) = request_id {
            fields.push(("requestId", request_id.into()));
        }
        fields.push(("ok", ok.into()));
        if ok {
            if let Some(value) = text {
                fields.push(("value", value.into()));
            }
        } else {
            fields.push(("error", text.unwrap_or("unknown failure").into()));
        }
        self.send(&fields);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "gridcraft:failed".into()), ("error", error.into())]);
    }

    /// Apply save receipts recorded since the last call to the document active right now —
    /// the one that was saved when the receipt was recorded. The command loop calls this
    /// right after a save returns (its document is still active there); [`Self::sync`] calls
    /// it for saves that came from the UI, where the active document doesn't move in between.
    pub fn apply_receipts(&self, app: &mut SheetApp) {
        let saved = std::mem::take(&mut *self.saved.borrow_mut());
        if let Some(name) = saved.iter().rev().find(|name| is_workbook(name))
            && let Some(doc) = app.session.active_mut()
        {
            doc.saved = doc.wb.clone();
            if doc.path.is_none() {
                doc.title = name.clone();
            }
        }
    }

    /// After each frame: a workbook handed to the parent this frame counts as saved under the
    /// name it was saved as, and the parent hears when unsaved changes appear or go away. When
    /// the last workbook closes (File › Close), the parent hears `docClosed` once.
    pub fn sync(&self, app: &mut SheetApp) {
        self.apply_receipts(app);
        let dirty = app.session.documents().iter().any(|d| d.is_dirty());
        if self.reported.get() != Some(dirty) && self.send(&[("type", "gridcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
        // The host's workbook closed (File › Close): the app replaces it with a blank Book, so
        // match on the host-opened name (documents are titled with the full file name) and
        // report docClosed once.
        let host_name = self.host_doc.borrow().clone();
        if let Some(name) = host_name
            && !app.session.documents().iter().any(|d| {
                d.title == name
                    || d.path
                        .as_deref()
                        .is_some_and(|p| std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().to_string()) == Some(name.clone()))
            })
        {
            *self.host_doc.borrow_mut() = None;
            self.send(&[("type", "gridcraft:docClosed".into())]);
        }
    }

    /// Commands the parent sent since the last frame, oldest first: (id, params JSON, request id).
    pub fn take_commands(&self) -> Vec<(String, String, Option<String>)> {
        self.queued_bytes.set(0);
        self.queued_id_bytes.set(0);
        std::mem::take(&mut *self.commands.borrow_mut())
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
        let commands = self.commands.clone();
        let queued_bytes = self.queued_bytes.clone();
        let queued_id_bytes = self.queued_id_bytes.clone();
        let host_doc = self.host_doc.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            match string(&data, "type").as_deref() {
                Some("gridcraft:open") => {
                    let name = string(&data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Book.xlsx".to_string());
                    let Some(bytes) = bytes(&data) else { return };
                    // Remember it to notice the closing later.
                    *host_doc.borrow_mut() = Some(name.clone());
                    inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
                    ctx.request_repaint();
                }
                Some("gridcraft:cmd") => {
                    // Bad and oversized commands are refused at once, like the WordCraft and
                    // DeckCraft bridges; a malformed `params` never runs as `{}` again.
                    let request_id_raw = string(&data, "requestId");
                    if request_id_raw.as_ref().map(|s| s.len()).unwrap_or(0) > MAX_ID_BYTES {
                        bridge.post_response("", request_id_raw.as_deref(), false, Some(&format!("requestId is larger than {MAX_ID_BYTES} bytes")));
                        ctx.request_repaint();
                        return;
                    }
                    let request_id = request_id_raw.filter(|r| !r.is_empty());
                    let id_raw = string(&data, "id");
                    if id_raw.as_ref().map(|s| s.len()).unwrap_or(0) > MAX_ID_BYTES {
                        bridge.post_response("", request_id.as_deref(), false, Some(&format!("id is larger than {MAX_ID_BYTES} bytes")));
                        ctx.request_repaint();
                        return;
                    }
                    let id = id_raw.filter(|i| !i.trim().is_empty());
                    let Some(id) = id else {
                        bridge.post_response("", request_id.as_deref(), false, Some("a command needs an id"));
                        ctx.request_repaint();
                        return;
                    };
                    let refuse = |id: &str, request_id: Option<&str>, why: &str| {
                        bridge.post_response(id, request_id, false, Some(why));
                        ctx.request_repaint();
                    };
                    // Check the JavaScript string length before allocating the Rust copy.
                    let params = match js_sys::Reflect::get(&data, &JsValue::from_str("params")) {
                        Ok(v) if !v.is_undefined() && !v.is_null() => {
                            let js_len = v.dyn_ref::<js_sys::JsString>().map(|s| s.length() as usize);
                            if js_len.unwrap_or(usize::MAX) > MAX_PARAMS_BYTES {
                                return refuse(&id, request_id.as_deref(), &format!("params are larger than {MAX_PARAMS_BYTES} bytes"));
                            }
                            match v.as_string() {
                                Some(text) if text.len() <= MAX_PARAMS_BYTES => text,
                                Some(_) => return refuse(&id, request_id.as_deref(), &format!("params are larger than {MAX_PARAMS_BYTES} bytes")),
                                None => return refuse(&id, request_id.as_deref(), "params must be a string of valid JSON"),
                            }
                        }
                        _ => "{}".to_string(), // absent or `null` means `{}`.
                    };
                    let id_bytes = id.len() + request_id.as_deref().map(str::len).unwrap_or(0);
                    if commands.borrow().len() >= MAX_PENDING_COMMANDS
                        || queued_bytes.get().saturating_add(params.len()) > MAX_PENDING_PARAMS_BYTES
                        || queued_id_bytes.get().saturating_add(id_bytes) > MAX_PENDING_PARAMS_BYTES
                    {
                        return refuse(&id, request_id.as_deref(), "too many commands are waiting; try again");
                    }
                    queued_bytes.set(queued_bytes.get().saturating_add(params.len()));
                    queued_id_bytes.set(queued_id_bytes.get().saturating_add(id_bytes));
                    commands.borrow_mut().push((id, params, request_id));
                    ctx.request_repaint();
                }
                _ => {}
            }
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
