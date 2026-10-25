//! Host modes: a page that runs GridCraft (as an iframe, or in its own document) opens workbooks
//! in it and stores what it saves instead of the browser downloading.
//!
//! ## `?host=parent` — an `<iframe>` (such as a Nextcloud app)
//!
//! The framing page and GridCraft talk over `postMessage` between same-origin windows. Messages
//! are plain objects with a `type`:
//!
//! - `gridcraft:ready` (to the parent): `{ version }`, the app is listening; send a file now.
//! - `gridcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a workbook picked with File › Open or dropped on the window.
//! - `gridcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file GridCraft would otherwise download: Save and Save As (an .xlsx workbook) and Export
//!   as PDF. The parent stores it and reports failures itself.
//! - `gridcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `gridcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//!
//! Only the parent window on the page's own origin is heard or answered.
//!
//! ## `data-host="page"` — the page's own document
//!
//! A page that loads the module into its own document (its own `<canvas id="gridcraft_canvas">`
//! with `data-host="page"`, no iframe) gets the same behaviour over DOM `CustomEvent`s on the
//! canvas instead of `postMessage`: the same four events outward (`ready`, `save`, `dirty`,
//! `failed`, with the message fields as the event's `detail`), and the page dispatches
//! `gridcraft:open` on the canvas with `detail: { name, bytes }` to open a workbook. Events don't
//! bubble, so two editors on one page can't hear each other, and the canvas is the only target
//! GridCraft listens on.
//!
//! In both host modes the page starts with an empty workbook instead of a sample, saves and PDF
//! exports are handed to the page rather than downloaded, and `?author=` names the user for new
//! notes and comments.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

use crate::hostmode::open_name;

/// Saving one of these hands the whole workbook to the host, which stores it: the workbook
/// counts as saved under that name, as it would after Save on the desktop.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

/// How a host page is reached. The modes behave the same; they differ only in how an event goes
/// out ([`Transport::send`]) and how an open request comes in ([`Bridge::listen`]).
#[derive(Clone)]
enum Transport {
    /// `?host=parent`: `postMessage` between same-origin windows.
    Parent { window: web_sys::Window, parent: web_sys::Window, origin: String },
    /// `data-host="page"`: DOM `CustomEvent`s on the editor's own canvas.
    Page { canvas: web_sys::HtmlCanvasElement },
}

impl Transport {
    /// Hand an event to the host. In parent mode this is a `postMessage` with a `type` field; in
    /// page mode a `CustomEvent` of that name on the canvas, whose `detail` holds the fields and
    /// which never bubbles (two editors on one page must not hear each other).
    fn send(&self, event: &str, fields: &[(&str, JsValue)], transfer: Option<&js_sys::ArrayBuffer>) -> Result<(), String> {
        match self {
            Transport::Parent { parent, origin, .. } => {
                let mut message_fields = Vec::with_capacity(fields.len() + 1);
                message_fields.push(("type", JsValue::from_str(event)));
                message_fields.extend_from_slice(fields);
                let message = message(&message_fields).ok_or_else(|| "couldn't build the message".to_string())?;
                let sent = match transfer {
                    Some(buffer) => parent.post_message_with_transfer(&message, origin, &js_sys::Array::of1(buffer)),
                    // `None` is only ever passed for the parent mode's small events; the page mode
                    // can't transfer anyway (below).
                    None => parent.post_message(&message, origin),
                };
                sent.map_err(|e| js_error(&e, "couldn't post the message"))
            }
            Transport::Page { canvas } => {
                let detail = message(fields).ok_or_else(|| "couldn't build the event detail".to_string())?;
                let init = web_sys::CustomEventInit::new();
                init.set_bubbles(false);
                init.set_detail(&detail);
                let custom = web_sys::CustomEvent::new_with_event_init_dict(event, &init).map_err(|e| js_error(&e, "couldn't create the event"))?;
                canvas.dispatch_event(&custom).map_err(|e| js_error(&e, "couldn't dispatch the event"))?;
                Ok(())
            }
        }
    }
}

#[derive(Clone)]
pub struct Bridge {
    transport: Transport,
    /// Names of the files handed to the host since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
    /// The unsaved-changes state last reported to the host.
    reported: Rc<Cell<Option<bool>>>,
}

impl Bridge {
    /// The parent bridge for `?host=parent`; `None` when the page isn't framed (there is no
    /// parent to talk to), so opening that URL directly stays the normal app with downloads.
    pub fn new() -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let origin = window.location().origin().ok()?;
        Some(Self { transport: Transport::Parent { window, parent, origin }, saved: Rc::default(), reported: Rc::default() })
    }

    /// The page bridge for `data-host="page"`: the host page runs GridCraft in its own document
    /// and hears the editor's DOM `CustomEvent`s on this canvas.
    pub fn for_page(canvas: &web_sys::HtmlCanvasElement) -> Self {
        Self { transport: Transport::Page { canvas: canvas.clone() }, saved: Rc::default(), reported: Rc::default() }
    }

    /// Send saved and exported files to the host instead of downloading them, and hear files
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
        let _ = self.transport.send("gridcraft:ready", &[("version", env!("CARGO_PKG_VERSION").into())], None);
    }

    pub fn post_failed(&self, error: &str) {
        let _ = self.transport.send("gridcraft:failed", &[("error", error.into())], None);
    }

    /// After each frame: a workbook handed to the host this frame counts as saved under the
    /// name it was saved as, and the host hears when unsaved changes appear or go away.
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
        if self.reported.get() != Some(dirty) && self.transport.send("gridcraft:dirty", &[("dirty", dirty.into())], None).is_ok() {
            self.reported.set(Some(dirty));
        }
    }

    fn post_save(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let name = file_name(path);
        // `gridcraft:save` carries the bytes as an `ArrayBuffer`; the parent bridge transfers it.
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        self.transport.send("gridcraft:save", &[("name", name.as_str().into()), ("bytes", buffer.clone().into())], Some(&buffer))?;
        self.saved.borrow_mut().push(name);
        Ok(())
    }

    fn listen(&self, inbox: Inbox, ctx: egui::Context) {
        match &self.transport {
            Transport::Parent { window, parent, origin } => {
                let parent = parent.clone();
                let origin = origin.clone();
                let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                    if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                        return;
                    }
                    let data = event.data();
                    if string(&data, "type").as_deref() != Some("gridcraft:open") {
                        return;
                    }
                    deliver_open(&inbox, &ctx, &data);
                });
                if window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
                    // The listener lives as long as the page.
                    on_message.forget();
                }
            }
            Transport::Page { canvas } => {
                let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                    let Some(open) = event.dyn_ref::<web_sys::CustomEvent>() else { return };
                    let detail = open.detail();
                    deliver_open(&inbox, &ctx, &detail);
                });
                if canvas.add_event_listener_with_callback("gridcraft:open", on_open.as_ref().unchecked_ref()).is_ok() {
                    // The listener lives as long as the page.
                    on_open.forget();
                }
            }
        }
    }
}

/// The file an open message or event carries, pushed onto the inbox for the next frame, like a
/// dropped file.
fn deliver_open(inbox: &Inbox, ctx: &egui::Context, data: &JsValue) {
    if let Some((name, bytes)) = open_payload(data) {
        inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
        ctx.request_repaint();
    }
}

/// The file an open message or event carries: a usable name and the bytes.
fn open_payload(data: &JsValue) -> Option<(String, Vec<u8>)> {
    let name = open_name(string(data, "name").as_deref());
    let bytes = bytes(data)?;
    Some((name, bytes))
}

fn is_workbook(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| WORKBOOK_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// A message object or `CustomEvent` detail with the given fields, or `None` if one couldn't be
/// set.
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

/// A `JsValue` error as a string, after `what` went wrong.
fn js_error(e: &JsValue, what: &str) -> String {
    format!("{what}: {}", e.as_string().unwrap_or_else(|| format!("{e:?}")))
}
