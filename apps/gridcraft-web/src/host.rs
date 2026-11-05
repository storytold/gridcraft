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
//! - `gridcraft:changed` (to the parent): `{ document, revision, dirty }` when the active
//!   workbook changes. Document and revision are opaque strings; edits while already dirty
//!   still notify the host, so it can debounce a fresh native export.
//! - `gridcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//! - `gridcraft:export-native` (from the parent): `{ requestId }`; replies with
//!   `gridcraft:native-exported` `{ requestId, document, revision, bytes }` (native JSON ArrayBuffer), or `{ requestId, error }`.
//!   Pending drafts are committed first. Export does not claim a durable save or clear dirty state.
//! - `gridcraft:validate-native` (from the parent): `{ requestId, bytes }`; replies with
//!   `gridcraft:native-validated` `{ requestId, valid, error? }` without changing the document.
//!
//! - `gridcraft:closed` (to the parent): no workbook is active.
//! - With `?host=parent&save=managed`, workbook Save emits `gridcraft:save-request`
//!   `{ document, revision }` without marking it saved. After durable storage of the latest
//!   native export, send `gridcraft:save-ack` `{ requestId, document, revision }`.
//!   Only that exact exported workbook becomes the saved baseline; later edits remain dirty.
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

enum NativeRequest {
    Export(String),
    Validate(String, Vec<u8>),
    RequestSave,
    Acknowledge(String, String, String),
}

#[derive(Clone)]
pub struct Bridge {
    managed: bool,
    managed_saves: Rc<RefCell<crate::managed_save::ManagedSaves>>,
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// Names of the files handed to the parent since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
    native: Rc<RefCell<Vec<NativeRequest>>>,
    reported_revision: Rc<Cell<Option<(u64, u64)>>>,
}

impl Bridge {
    /// `None` when the page isn't framed (there is no parent to talk to).
    pub fn new(managed: bool) -> Option<Self> {
        let window = web_sys::window()?;
        let parent = window.parent().ok().flatten()?;
        if js_sys::Object::is(&parent, &window) {
            return None;
        }
        let origin = window.location().origin().ok()?;
        Some(Self {
            managed,
            managed_saves: Rc::default(),
            window,
            parent,
            origin,
            saved: Rc::default(),
            reported: Rc::default(),
            native: Rc::default(),
            reported_revision: Rc::default(),
        })
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

    /// After each frame: a workbook handed to the parent this frame counts as saved under the
    /// name it was saved as, and the parent hears when unsaved changes appear or go away.
    pub fn sync(&self, app: &mut SheetApp) {
        let requests = std::mem::take(&mut *self.native.borrow_mut());
        for request in requests {
            match request {
                NativeRequest::Export(id) => match crate::native_document::export(app) {
                    Ok(snapshot) => {
                        if self.managed
                            && let Err(error) = self.managed_saves.borrow_mut().remember(&id, &snapshot, app)
                        {
                            self.send(&[
                                ("type", "gridcraft:native-exported".into()),
                                ("requestId", id.as_str().into()),
                                ("error", error.as_str().into()),
                            ]);
                            continue;
                        }
                        let buffer = js_sys::Uint8Array::from(snapshot.bytes.as_slice()).buffer();
                        if let Some(message) = message(&[
                            ("type", "gridcraft:native-exported".into()),
                            ("requestId", id.as_str().into()),
                            ("document", snapshot.document.as_str().into()),
                            ("revision", snapshot.revision.as_str().into()),
                            ("bytes", buffer.clone().into()),
                        ]) {
                            let _ = self.parent.post_message_with_transfer(&message, &self.origin, &js_sys::Array::of1(&buffer));
                        }
                    }
                    Err(error) => {
                        self.send(&[
                            ("type", "gridcraft:native-exported".into()),
                            ("requestId", id.as_str().into()),
                            ("error", error.as_str().into()),
                        ]);
                    }
                },
                NativeRequest::RequestSave => {
                    if let Some(doc) = app.session.active() {
                        self.send(&[
                            ("type", "gridcraft:save-request".into()),
                            ("document", doc.uid.to_string().into()),
                            ("revision", doc.revision.to_string().into()),
                        ]);
                    }
                }
                NativeRequest::Acknowledge(id, document, revision) => {
                    self.managed_saves.borrow_mut().acknowledge(app, &id, &document, &revision);
                }
                NativeRequest::Validate(id, bytes) => {
                    let result = crate::native_document::validate(&bytes);
                    let error = result.as_ref().err().map(String::as_str).unwrap_or("");
                    self.send(&[
                        ("type", "gridcraft:native-validated".into()),
                        ("requestId", id.as_str().into()),
                        ("valid", result.is_ok().into()),
                        ("error", error.into()),
                    ]);
                }
            }
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
        let revision = app.session.active().map(|doc| (doc.uid, doc.revision));
        if revision != self.reported_revision.get() {
            if let Some((document, revision)) = revision {
                self.send(&[
                    ("type", "gridcraft:changed".into()),
                    ("document", document.to_string().into()),
                    ("revision", revision.to_string().into()),
                    ("dirty", dirty.into()),
                ]);
            } else {
                self.send(&[("type", "gridcraft:closed".into())]);
            }
            self.reported_revision.set(revision);
        }
    }

    fn post_save(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
        if self.managed && is_workbook(&name) {
            if self.native.borrow().len() >= 8 {
                return Err("The native document request queue is full.".into());
            }
            self.native.borrow_mut().push(NativeRequest::RequestSave);
            return Ok(());
        }
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
            let kind = string(&data, "type");
            if kind.as_deref() == Some("gridcraft:save-ack") {
                if !bridge.managed || bridge.native.borrow().len() >= 8 {
                    return;
                }
                let field = |key| string(&data, key).filter(|value| !value.is_empty() && value.len() <= 128);
                let (Some(id), Some(document), Some(revision)) = (field("requestId"), field("document"), field("revision")) else { return };
                bridge.native.borrow_mut().push(NativeRequest::Acknowledge(id, document, revision));
                ctx.request_repaint();
                return;
            }
            if matches!(kind.as_deref(), Some("gridcraft:export-native" | "gridcraft:validate-native")) {
                let Some(id) = string(&data, "requestId").filter(|id| !id.is_empty() && id.len() <= 128) else { return };
                let response =
                    if kind.as_deref() == Some("gridcraft:export-native") { "gridcraft:native-exported" } else { "gridcraft:native-validated" };
                if bridge.native.borrow().len() >= 8 {
                    bridge.send(&[
                        ("type", response.into()),
                        ("requestId", id.as_str().into()),
                        ("valid", false.into()),
                        ("error", "The native document request queue is full.".into()),
                    ]);
                    return;
                }
                let request = if kind.as_deref() == Some("gridcraft:export-native") {
                    NativeRequest::Export(id)
                } else {
                    match native_bytes(&data) {
                        Ok(bytes) => NativeRequest::Validate(id, bytes),
                        Err(error) => {
                            bridge.send(&[
                                ("type", response.into()),
                                ("requestId", id.as_str().into()),
                                ("valid", false.into()),
                                ("error", error.as_str().into()),
                            ]);
                            return;
                        }
                    }
                };
                bridge.native.borrow_mut().push(request);
                ctx.request_repaint();
                return;
            }
            if kind.as_deref() != Some("gridcraft:open") {
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

/// The validation RPC checks size before allocating or copying the incoming payload.
fn native_bytes(data: &JsValue) -> Result<Vec<u8>, String> {
    let value = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).map_err(|_| "Native workbook bytes are required.".to_string())?;
    let array = if let Some(array) = value.dyn_ref::<js_sys::Uint8Array>() {
        array.clone()
    } else if value.is_instance_of::<js_sys::ArrayBuffer>() {
        js_sys::Uint8Array::new(&value)
    } else {
        return Err("Native workbook bytes are required.".into());
    };
    crate::native_document::validate_length(array.length() as usize)?;
    Ok(array.to_vec())
}
