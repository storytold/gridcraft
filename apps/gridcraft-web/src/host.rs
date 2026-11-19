//! Host modes: a page that runs GridCraft (as an iframe, or in its own document) opens workbooks
//! in it and stores what it saves instead of the browser downloading. The messages and the two
//! transports are `craft-web-host`'s (storytold/craft-libs), the same contract as the other Craft
//! apps' bridges, with the `gridcraft:` prefix. This module is GridCraft's part of it.
//!
//! - `?host=parent` (an `<iframe>`, such as a Nextcloud app) talks to the same-origin parent over
//!   `postMessage`; `data-host="page"` on the canvas (the page's own document, no iframe) uses
//!   DOM `CustomEvent`s on the canvas instead. `?host=parent` wins when both are present. With no
//!   parent, or a parent on another origin, there is no bridge: the normal app with downloads.
//! - `gridcraft:ready` once the app is listening; send a file now. GridCraft reads bytes, so it
//!   says `file: false` and a host sends `bytes`.
//! - `gridcraft:open` (from the host): `{ name, bytes }`. It opens like a workbook picked with
//!   File › Open or dropped on the window; a file over [`MAX_OPEN_BYTES`] is refused before it is
//!   copied.
//! - `gridcraft:save` (to the host): `{ name, saveAs, bytes, export }` for every file GridCraft
//!   would otherwise download. With `export: false`, the host stores the XLSX workbook; with
//!   `export: true`, it stores a separate file (such as a PDF) without replacing the workbook.
//!   `saveAs` remains `false`; it does not distinguish Save from Save As here.
//! - `gridcraft:dirty` whenever unsaved changes appear or go away, `gridcraft:failed` when the
//!   app couldn't start.
//!
//! In both host modes the page starts with an empty workbook instead of a sample, saves and PDF
//! exports are handed to the page rather than downloaded, and `?author=` names the user for new
//! notes and comments.

use std::cell::RefCell;
use std::rc::Rc;

use craft_web_host::web::{FileData, Host, Message};
use craft_web_host::{Mode, Outgoing, file_name, message_type};
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use wasm_bindgen::JsValue;

use crate::host_save::{SaveMessage, sync_saved};
use crate::hostmode::open_name;

/// The app id in the host messages (`gridcraft:ready`).
const APP: &str = "gridcraft";

/// The largest workbook taken from the host page, refused before it is copied.
const MAX_OPEN_BYTES: u64 = 512 << 20;

/// The longest user name taken from `?author=`.
const MAX_AUTHOR_CHARS: usize = 200;

#[derive(Clone)]
pub struct Bridge {
    host: Rc<Host>,
    save_target: Rc<SaveTarget>,
    /// Names of the files handed to the host since the last frame.
    saved: Rc<RefCell<Vec<String>>>,
}

impl Bridge {
    /// The bridge for this page, or `None` when GridCraft isn't hosted: no `?host=parent` and no
    /// `data-host="page"` on the canvas, an unframed `?host=parent` page (it stays the normal app
    /// with downloads), or a parent on another origin.
    pub fn detect(canvas: &web_sys::HtmlCanvasElement) -> Option<Self> {
        let host = Host::connect(APP, canvas)?;
        let save_target = match host.mode() {
            Mode::Parent => {
                // Host::connect already admitted only a same-origin parent. Keep that origin
                // as the postMessage target so later navigation can't receive workbook bytes.
                let window = web_sys::window()?;
                let parent = window.parent().ok().flatten()?;
                let origin = window.location().origin().ok()?;
                SaveTarget::Parent { parent, origin }
            }
            Mode::Page => SaveTarget::Page(canvas.clone()),
        };
        Some(Self { host: Rc::new(host), save_target: Rc::new(save_target), saved: Rc::default() })
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
        let _ = self.host.post_ready(env!("CARGO_PKG_VERSION"), false);
    }

    pub fn post_failed(&self, error: &str) {
        let _ = self.host.post_failed(error, None);
    }

    /// After each frame: a workbook handed to the host this frame counts as saved under the
    /// name it was saved as, and the host hears when unsaved changes appear or go away (the
    /// crate does not repeat a state it already reported).
    pub fn sync(&self, app: &mut SheetApp) {
        let saved = std::mem::take(&mut *self.saved.borrow_mut());
        sync_saved(&saved, app);
        self.host.post_dirty(app.session.documents().iter().any(|d| d.is_dirty()));
    }

    fn post_save(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let name = file_name(Some(path), "document");
        self.save_target.post(SaveMessage::new(&name, bytes)).map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
        self.saved.borrow_mut().push(name);
        Ok(())
    }

    fn listen(&self, inbox: Inbox, ctx: egui::Context) {
        self.host.listen(move |message| {
            if let Message::Open { name, data } = message {
                deliver_open(&inbox, &ctx, &name, &data);
            }
        });
    }
}

/// A save-only adapter: the pinned host crate's `post_save` cannot add app-specific fields.
/// All other messages and transport detection still go through `Host`.
enum SaveTarget {
    Parent { parent: web_sys::Window, origin: String },
    Page(web_sys::HtmlCanvasElement),
}

impl SaveTarget {
    fn post(&self, save: SaveMessage<'_>) -> Result<(), JsValue> {
        let buffer = js_sys::Uint8Array::from(save.bytes).buffer();
        let kind = message_type(APP, Outgoing::Save.kind());
        let payload = js_sys::Object::new();
        let set = |key, value: JsValue| js_sys::Reflect::set(&payload, &JsValue::from_str(key), &value);
        set("name", save.name.into())?;
        set("bytes", buffer.clone().into())?;
        set("saveAs", JsValue::FALSE)?;
        set("export", JsValue::from_bool(save.export))?;
        match self {
            Self::Parent { parent, origin } => {
                set("type", kind.as_str().into())?;
                parent.post_message_with_transfer(&payload, origin, &js_sys::Array::of1(&buffer))
            }
            Self::Page(canvas) => {
                let init = web_sys::CustomEventInit::new();
                init.set_detail(&payload.into());
                let event = web_sys::CustomEvent::new_with_event_init_dict(&kind, &init)?;
                canvas.dispatch_event(&event).map(|_| ())
            }
        }
    }
}

/// The file an open message carries, pushed onto the inbox for the next frame, like a dropped
/// file. A host's file is untrusted input: an oversized one is refused before it is copied into
/// the app's memory (the data's own length, not the host's claim).
fn deliver_open(inbox: &Inbox, ctx: &egui::Context, name: &str, data: &FileData) {
    let FileData::Bytes(array) = data else {
        log::warn!("host file {name} came as a File; GridCraft reads bytes only; not opened");
        return;
    };
    if data.size() > MAX_OPEN_BYTES as f64 {
        log::warn!("host file is {} bytes, over the {} MiB host open limit; not opened", data.size(), MAX_OPEN_BYTES >> 20);
        return;
    }
    inbox.lock().unwrap_or_else(|e| e.into_inner()).push((open_name(name), array.to_vec()));
    ctx.request_repaint();
}
