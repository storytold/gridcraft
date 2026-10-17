//! The `postMessage` bridge to a same-origin parent page (`?host=parent`), for pages that embed
//! GridCraft in an `<iframe>` (such as a Nextcloud app). Messages are plain objects with a `type`:
//!
//! - `gridcraft:ready` (to the parent): `{ version, capabilities }`, the app is listening; send a
//!   file now. `capabilities` names the optional messages it understands: `["font"]`.
//! - `gridcraft:open` (from the parent): `{ name, bytes }`, bytes an `ArrayBuffer` or
//!   `Uint8Array`. It opens like a workbook picked with File › Open or dropped on the window.
//! - `gridcraft:font` (from the parent): `{ name, bytes, index? }`, a TrueType or OpenType file
//!   (not WOFF; `index` picks the face in a collection) that every font family falls back on:
//!   the browser has no system fonts, so this is how text in scripts egui's own fonts lack
//!   (Chinese, Japanese, Korean) gets glyphs. Send fonts after `gridcraft:ready` and before
//!   `gridcraft:open`; the app redraws with them at its next frame. Answered with
//!   `gridcraft:fontResult` (to the parent): `{ name, ok: true, added, covers }` (`added` false
//!   when a font of that name was already added and this one was ignored), or
//!   `{ name, ok: false, error }` for a file that isn't a readable font, one egui can't draw
//!   from (no glyph outlines, as in a file cut off in transfer, or no units per em), one over
//!   64 MiB, or a font past 32 (256 MiB in all).
//! - `gridcraft:save` (to the parent): `{ name, bytes }` (a transferred `ArrayBuffer`) for every
//!   file GridCraft would otherwise download: Save and Save As (an .xlsx workbook) and Export
//!   as PDF. The parent stores it and reports failures itself.
//! - `gridcraft:dirty` (to the parent): `{ dirty }` whenever unsaved changes appear or go away.
//! - `gridcraft:failed` (to the parent): `{ error }` when the app couldn't start.
//!
//! Only the parent window on the page's own origin is heard or answered. In host mode the page
//! starts with an empty workbook instead of a sample, and the parent guards leaving the page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gridcraft_ui_egui::{Inbox, Services, SheetApp, theme};
use serde_json::Value as Json;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

use crate::font_message;

/// Saving one of these hands the whole workbook to the parent, which stores it: the workbook
/// counts as saved under that name, as it would after Save on the desktop.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    /// Names of the files handed to the parent since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
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
        Some(Self { window, parent, origin, saved: Rc::default(), reported: Rc::default() })
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
        let capabilities = js_sys::Array::of1(&JsValue::from_str("font"));
        self.send(&[("type", "gridcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into()), ("capabilities", capabilities.into())]);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "gridcraft:failed".into()), ("error", error.into())]);
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
                    inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
                }
                Some("gridcraft:font") => {
                    let name = string(&data, "name").unwrap_or_default();
                    let result = add_font(&name, &data);
                    if let Err(e) = &result {
                        log::error!("couldn't add font {name:?}: {e}");
                    }
                    if !bridge.post_json(&font_message::font_result(&name, &result)) {
                        log::error!("couldn't tell the page about font {name:?}");
                    }
                }
                _ => return,
            }
            ctx.request_repaint();
        });
        if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_message.forget();
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

/// `gridcraft:font`, checked in the order `app.font.add` is: the face number, the bytes, then what
/// `theme::add_font` checks before reading a font, so a font already added or too large is never
/// copied out of JavaScript. The app notices an added font and rebuilds its fonts at the next frame.
fn add_font(name: &str, data: &JsValue) -> Result<theme::FontAdded, String> {
    let index = js_sys::Reflect::get(data, &JsValue::from_str("index")).unwrap_or(JsValue::UNDEFINED);
    let index = if index.is_undefined() || index.is_null() {
        Json::Null
    } else {
        // Anything but a finite number becomes a value `face_index` refuses.
        index.as_f64().and_then(serde_json::Number::from_f64).map_or(Json::Bool(false), Json::Number)
    };
    let index = theme::face_index(Some(&index))?;
    let bytes = byte_array(data).ok_or("`bytes` must be an ArrayBuffer or a Uint8Array")?;
    if let Some(known) = theme::check_new_font(name, bytes.length() as usize)? {
        return Ok(known);
    }
    theme::add_font(name, bytes.to_vec(), index)
}

fn string(data: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_string()
}

/// `bytes` as an `ArrayBuffer` or `Uint8Array`.
fn bytes(data: &JsValue) -> Option<Vec<u8>> {
    byte_array(data).map(|array| array.to_vec())
}

/// A view of `bytes` (an `ArrayBuffer` or `Uint8Array`); nothing is copied yet.
fn byte_array(data: &JsValue) -> Option<js_sys::Uint8Array> {
    let bytes = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
    if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
        Some(array.clone())
    } else if bytes.is_instance_of::<js_sys::ArrayBuffer>() {
        Some(js_sys::Uint8Array::new(&bytes))
    } else {
        None
    }
}
