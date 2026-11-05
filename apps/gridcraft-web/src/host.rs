//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! GridCraft in an `<iframe>` (such as a Nextcloud app). Messages are plain objects with a `type`:
//!
//! - `gridcraft:ready` (to the parent): `{ version }`, the app is listening; send a file now.
//! - `gridcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a workbook picked with File › Open or dropped on the window.
//!   With optional `requestId`, `gridcraft:opened` replies with `{ requestId, document, revision }`
//!   only after actual engine import, or `{ requestId, error }`. One correlated import may be pending.
//!   Correlated import refuses unsaved work or pending native drafts before settling them.
//! - `gridcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file GridCraft would otherwise download: Save and Save As (an .xlsx workbook) and Export
//!   as PDF. The parent stores it and reports failures itself.
//! - `gridcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `gridcraft:changed` (to the parent): `{ document, revision, dirty }` when the active
//!   workbook or pending cell/text-box draft changes. Revision is a bounded opaque activity
//!   string, never a JavaScript number. Edits while already dirty
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
//! `?host=port&parent_origin=<canonical HTTP(S) origin>` opts into a one-shot private
//! MessagePort instead. Only a content-free `{type:"gridcraft:hello",protocol:1,bundle}`
//! uses the window channel. The bundle comes from the last `/b/<64hex>/index.html`
//! path segment. The exact parent and origin must connect with that bundle and
//! exactly one port; all document messages then use the port, with a 32 MiB byte limit.
//! Later window frames are ignored. This transport grants no runtime credentials.
//!
//! In legacy window mode, only the parent on the page's own origin is heard or answered. In host mode the page
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
    Open(String, String, Vec<u8>),
    Export(String),
    Validate(String, Vec<u8>),
    RequestSave,
    Acknowledge(String, String, String),
}

struct PortState {
    handshake: crate::port_handshake::Handshake,
    channel: Option<web_sys::MessagePort>,
}

#[derive(Clone)]
pub struct Bridge {
    port: Option<Rc<RefCell<PortState>>>,
    revisions: Rc<RefCell<crate::draft_state::Revisions>>,
    importing: Rc<Cell<bool>>,
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
            port: None,
            revisions: Rc::default(),
            importing: Rc::default(),
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

    /// Opt-in private transport for an opaque sandboxed frame. The configured
    /// origin binds only the handshake; it never supplies runtime credentials.
    pub fn new_port(managed: bool, parent_origin: &str) -> Option<Self> {
        let url = web_sys::Url::new(parent_origin).ok()?;
        if !matches!(url.protocol().as_str(), "http:" | "https:")
            || url.origin() != parent_origin
            || !url.username().is_empty()
            || !url.password().is_empty()
        {
            return None;
        }
        let mut bridge = Self::new(managed)?;
        let path = bridge.window.location().pathname().ok()?;
        let bundle = crate::port_handshake::bundle_from_path(&path)?.to_string();
        bridge.port = Some(Rc::new(RefCell::new(PortState {
            handshake: crate::port_handshake::Handshake::new(parent_origin.to_string(), bundle),
            channel: None,
        })));
        Some(bridge)
    }

    /// Send saved and exported files to the parent instead of downloading them, and hear files
    /// from it.
    pub fn connect(&self, services: &mut Services, inbox: Inbox, ctx: egui::Context) {
        if self.port.is_some() {
            self.listen_port(inbox, ctx);
        } else {
            self.listen(inbox, ctx);
        }
        let bridge = self.clone();
        services.download = Some(Box::new(move |name: &str, bytes: &[u8]| {
            if let Err(e) = bridge.post_save(name, bytes) {
                log::error!("couldn't hand {name} to the page: {e}");
                if bridge.port.is_some() {
                    bridge.post_failed(&e);
                }
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
        if let Some(state) = &self.port {
            let mut state = state.borrow_mut();
            if state.channel.is_none() {
                if let Some(hello) =
                    message(&[("type", "gridcraft:hello".into()), ("protocol", 1.into()), ("bundle", state.handshake.bundle.as_str().into())])
                    && self.parent.post_message(&hello, &state.handshake.parent_origin).is_ok()
                {
                    state.handshake.announced();
                }
                return;
            }
        }
        self.send(&[("type", "gridcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into())]);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "gridcraft:failed".into()), ("error", error.into())]);
    }

    /// After each frame: a workbook handed to the parent this frame counts as saved under the
    /// name it was saved as, and the parent hears when unsaved changes appear or go away.
    pub fn sync(&self, app: &mut SheetApp) {
        self.revisions.borrow_mut().observe(app);
        let requests = std::mem::take(&mut *self.native.borrow_mut());
        for request in requests {
            match request {
                NativeRequest::Open(id, name, bytes) => {
                    let result = crate::native_document::open(app, &name, &bytes).and_then(|opened| {
                        if app
                            .session
                            .active()
                            .is_none_or(|doc| doc.uid.to_string() != opened.document || doc.revision.to_string() != opened.revision)
                        {
                            return Err("The imported workbook is no longer active.".into());
                        }
                        Ok(opened)
                    });
                    self.importing.set(false);
                    match result {
                        Ok(opened) => {
                            self.revisions.borrow_mut().observe(app);
                            self.send(&[
                                ("type", "gridcraft:opened".into()),
                                ("requestId", id.as_str().into()),
                                ("document", opened.document.as_str().into()),
                                ("revision", self.revisions.borrow().revision().into()),
                            ]);
                        }
                        Err(error) => self.open_failed(&id, &error),
                    }
                }
                NativeRequest::Export(id) => match crate::native_document::export(app) {
                    Ok(snapshot) => {
                        self.revisions.borrow_mut().observe(app);
                        let revision = self.revisions.borrow().revision();
                        if self.port.is_some()
                            && let Err(error) = crate::native_document::validate_length(snapshot.bytes.len())
                        {
                            self.send(&[
                                ("type", "gridcraft:native-exported".into()),
                                ("requestId", id.as_str().into()),
                                ("error", error.as_str().into()),
                            ]);
                            continue;
                        }
                        if self.managed
                            && let Err(error) = self.managed_saves.borrow_mut().remember(&id, &snapshot, app, &revision)
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
                            ("revision", revision.as_str().into()),
                            ("bytes", buffer.clone().into()),
                        ]) {
                            let _ = self.post(&message, Some(&js_sys::Array::of1(&buffer)));
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
                            ("revision", self.revisions.borrow().revision().into()),
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
        self.revisions.borrow_mut().observe(app);
        let dirty = crate::draft_state::dirty(app);
        let initial = self.reported.get().is_none();
        if self.reported.get() != Some(dirty) && self.send(&[("type", "gridcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
        let revision = app.session.active().map(|doc| (doc.uid, self.revisions.borrow().sequence()));
        if revision != self.reported_revision.get() || initial && revision.is_none() && self.port.is_some() {
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
        if self.port.is_some() {
            crate::native_document::validate_length(bytes.len())?;
        }
        let buffer = js_sys::Uint8Array::from(bytes).buffer();
        let message = message(&[("type", "gridcraft:save".into()), ("name", name.as_str().into()), ("bytes", buffer.clone().into())])
            .ok_or_else(|| "couldn't build the message".to_string())?;
        self.post(&message, Some(&js_sys::Array::of1(&buffer)))?;
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
            bridge.receive(&event.data(), &inbox, &ctx);
        });
        if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_message.forget();
        }
    }

    fn receive(&self, data: &JsValue, inbox: &Inbox, ctx: &egui::Context) {
        let bridge = self;
        let kind = string(data, "type");
        if kind.as_deref() == Some("gridcraft:save-ack") {
            if !bridge.managed || bridge.native.borrow().len() >= 8 {
                return;
            }
            let field = |key| string(data, key).filter(|value| !value.is_empty() && value.len() <= 128);
            let (Some(id), Some(document), Some(revision)) = (field("requestId"), field("document"), field("revision")) else { return };
            bridge.native.borrow_mut().push(NativeRequest::Acknowledge(id, document, revision));
            ctx.request_repaint();
            return;
        }
        if matches!(kind.as_deref(), Some("gridcraft:export-native" | "gridcraft:validate-native")) {
            let Some(id) = string(data, "requestId").filter(|id| !id.is_empty() && id.len() <= 128) else { return };
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
                match native_bytes(data) {
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
        let name = string(data, "name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Book.xlsx".to_string());
        if js_sys::Reflect::get(data, &"requestId".into()).ok().is_some_and(|value| !value.is_undefined()) {
            let Some(id) = string(data, "requestId").filter(|id| !id.is_empty() && id.len() <= 128) else { return };
            if bridge.importing.get() || bridge.native.borrow().len() >= 8 {
                bridge.open_failed(&id, "Another workbook import is pending.");
                return;
            }
            let bytes = match native_bytes(data) {
                Ok(bytes) => bytes,
                Err(error) => {
                    bridge.open_failed(&id, &error);
                    return;
                }
            };
            bridge.importing.set(true);
            bridge.native.borrow_mut().push(NativeRequest::Open(id, name, bytes));
            ctx.request_repaint();
            return;
        }
        let bytes = if bridge.port.is_some() {
            match native_bytes(data) {
                Ok(bytes) => bytes,
                Err(error) => {
                    bridge.post_failed(&error);
                    return;
                }
            }
        } else {
            let Some(bytes) = bytes(data) else { return };
            bytes
        };
        inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
        ctx.request_repaint();
    }

    fn open_failed(&self, id: &str, error: &str) {
        self.send(&[("type", "gridcraft:opened".into()), ("requestId", id.into()), ("error", error.into())]);
    }

    fn listen_port(&self, inbox: Inbox, ctx: egui::Context) {
        let bridge = self.clone();
        let on_connect = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let Some(state) = &bridge.port else { return };
            if state.borrow().channel.is_some() {
                return; // All window frames are ignored after the one-shot bind.
            }
            let data = event.data();
            if !closed_connect(&data) {
                return;
            }
            let ports = event.ports();
            let Ok(channel) = ports.get(0).dyn_into::<web_sys::MessagePort>() else { return };
            let protocol = js_sys::Reflect::get(&data, &"protocol".into()).ok().and_then(|value| value.as_f64());
            let parent = event.source().is_some_and(|source| js_sys::Object::is(&source, &bridge.parent));
            if !state.borrow_mut().handshake.accept(
                parent,
                &event.origin(),
                string(&data, "type").as_deref(),
                protocol,
                string(&data, "bundle").as_deref(),
                ports.length() as usize,
            ) {
                return;
            }
            let receiver = bridge.clone();
            let inbox = inbox.clone();
            let ctx = ctx.clone();
            let repaint = ctx.clone();
            let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                receiver.receive(&event.data(), &inbox, &ctx);
            });
            channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            channel.start();
            state.borrow_mut().channel = Some(channel);
            on_message.forget(); // Exactly one private listener lives until the frame closes.
            bridge.reported.set(None);
            bridge.reported_revision.set(None);
            bridge.post_ready();
            repaint.request_repaint();
        });
        if self.window.add_event_listener_with_callback("message", on_connect.as_ref().unchecked_ref()).is_ok() {
            on_connect.forget();
        }
    }

    fn post(&self, value: &JsValue, transfer: Option<&js_sys::Array>) -> Result<(), String> {
        let result = if let Some(state) = &self.port {
            let state = state.borrow();
            let channel = state.channel.as_ref().ok_or_else(|| "The private host port is not connected.".to_string())?;
            match transfer {
                Some(transfer) => channel.post_message_with_transferable(value, transfer),
                None => channel.post_message(value),
            }
        } else {
            match transfer {
                Some(transfer) => self.parent.post_message_with_transfer(value, &self.origin, transfer),
                None => self.parent.post_message(value, &self.origin),
            }
        };
        result.map_err(|error| error.as_string().unwrap_or_else(|| format!("{error:?}")))
    }

    /// Post a message to the parent; false if it couldn't be sent.
    fn send(&self, fields: &[(&str, JsValue)]) -> bool {
        message(fields).is_some_and(|m| self.post(&m, None).is_ok())
    }
}

/// A closed plain-object handshake; inherited fields, arrays and extra keys
/// cannot smuggle content or credentials onto the public window channel.
fn closed_connect(value: &JsValue) -> bool {
    if !value.is_object() || value.is_null() || js_sys::Array::is_array(value) {
        return false;
    }
    if !js_sys::Object::is(&js_sys::Object::get_prototype_of(value), &js_sys::Object::get_prototype_of(&js_sys::Object::new())) {
        return false;
    }
    let keys = js_sys::Object::keys(value.unchecked_ref::<js_sys::Object>());
    keys.length() == 3 && keys.iter().all(|key| key.as_string().is_some_and(|key| matches!(key.as_str(), "type" | "protocol" | "bundle")))
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
