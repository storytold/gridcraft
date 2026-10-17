//! GridCraft's half of Open and Save against cloud storage.
//!
//! The pages themselves are `craft_ui::cloud`. Every Craft app offers the same
//! ones. What is here is what only GridCraft can answer. A document is a
//! workbook. These extensions and these content types are what that means.
//! Bytes become one through `file.open` and go back through `save_bytes`. The
//! words are the ones `i18n` has.
//!
//! GridCraft holds no provider credential and speaks no provider protocol.
//! Every call goes to the CollabCraft service, which is the single place a
//! provider is configured and permissions are decided. Replacing Nextcloud
//! with something else changes nothing here.
//!
//! Nothing in this file opens a socket either. The transport half is a
//! [`CloudBackend`] the host application supplies (see `apps/gridcraft/cloud`).
//! Every CollabCraft call blocks. A blocking read of a twenty megabyte
//! workbook inside a frame would freeze the window for seconds. So the pages
//! state a [`Job`], the host runs it on a worker thread, and the answer
//! arrives on a channel that [`pump`] drains on a later frame.

use std::collections::HashMap;
use std::sync::Arc;

// Re-exported so a host application, and a test, can name these types without
// depending on the crates they come from.
pub use craft_cloud::{Capability, Entry, EntryKind};
pub use craft_ui::cloud::{
    Account, Busy, CloudApp, CloudBackend, CloudFiles, Conflict, CopyFormat, Destination, DocumentKind, Event, Fault, Folder, Job, Link, Origin,
    Post, Source, UNAVAILABLE, available, browse, can_converse, conflict_modal, destination_name, has_target, link, load_shared, open_entry,
    open_page, origin, parent_of, prefers_cloud, progress, pump, reload_active, save, save_as, save_page, share_button, shares, sign_in, sign_out,
    start, who, work_together,
};
use egui::{Color32, Painter, Rect, Ui};
use gridcraft_engine::io::FileKind;
use gridcraft_engine::model::Workbook;
use serde_json::{Value as Json, json};

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::{SheetApp, now_ms};

/// Extensions GridCraft opens. The same set the desktop file picker offers.
const OPENS: [&str; 9] = ["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "txt", "json"];

/// Content types GridCraft opens, matched against the start of what the
/// provider said. Read beside the extension because neither is reliable
/// alone. A WebDAV server hands back `application/octet-stream` for a
/// perfectly good `.xlsx`, and a file exported by a web application often has
/// a correct content type and no extension at all.
const MIMES: [&str; 7] = [
    "application/vnd.openxmlformats-officedocument.spreadsheetml",
    "application/vnd.ms-excel",
    "application/vnd.oasis.opendocument.spreadsheet",
    "text/csv",
    "text/tab-separated-values",
    "text/plain",
    "application/json",
];

/// What a GridCraft document is, as a cloud file list needs to know.
pub const WORKBOOK: DocumentKind = DocumentKind { extensions: &OPENS, mimes: &MIMES, noun: "workbook", default_extension: "xlsx" };

/// The Save a Copy formats the Share menu offers under the share itself.
const COPIES: [(&str, &str); 3] =
    [("xlsx", "Save a Copy as Excel Workbook (.xlsx)…"), ("csv", "Export as CSV…"), ("html", "Export as Web Page (.html)…")];

/// The workbook snapshot each save in flight encoded.
///
/// Only GridCraft knows what a version of a workbook is. The pages hold none
/// of these. A save that succeeds clears the dirty flag for exactly the
/// version that reached the server. An edit made while the bytes were on the
/// wire stays unsaved.
pub type Sent = HashMap<u64, Arc<Workbook>>;

/// Whether GridCraft can open this entry.
pub fn opens(entry: &Entry) -> bool {
    craft_ui::cloud::opens(entry, WORKBOOK)
}

/// A typed name as a workbook file name. Any folder separators are dropped and
/// a missing extension becomes `.xlsx`.
pub fn workbook_name(typed: &str) -> String {
    craft_ui::cloud::document_name(typed, WORKBOOK)
}

/// The whole of the cloud state, for the control channel and for tests.
pub fn status(app: &SheetApp) -> Json {
    craft_ui::cloud::report(app)
}

/// The Share menu's export half, as the control it is drawn in wants it.
pub fn copies() -> Vec<CopyFormat> {
    COPIES.iter().map(|(id, label)| CopyFormat::new(*id, *label)).collect()
}

/// The host part of the service address, for the account menu's first line.
///
/// Read from the environment and compiled in nowhere, because the service is
/// deployment-specific. Somebody with an account on two deployments needs to
/// know which one a workbook is about to land in.
fn service_host() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        return web_sys::window().and_then(|w| w.location().host().ok()).unwrap_or_default();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ["GRIDCRAFT_CLOUD_URL", "CRAFT_CLOUD_URL", "COLLABCRAFT_URL"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .map(|value| value.trim().trim_end_matches('/').to_owned())
            .find(|value| !value.is_empty())
            .map(|value| value.trim_start_matches("https://").trim_start_matches("http://").to_owned())
            .unwrap_or_default()
    }
}

/// The format a save should be written in, when the name cannot say.
///
/// A provider that issued no extension still says what the thing is. A file
/// exported by a web application commonly has a correct content type and a
/// bare name, and `exported` from such a service is a real CSV. Writing a
/// workbook over it would replace a readable file with one nothing opens.
///
/// `None` leaves the name to decide, which is the ordinary case and the one
/// that refuses an extension this application cannot write.
fn kind_of(into: Destination<'_>) -> Option<FileKind> {
    if !into.extension().is_empty() {
        return None;
    }
    let mime = into.mime?;
    let mime = mime.split(';').next().unwrap_or(mime).trim();
    match mime {
        "text/csv" => Some(FileKind::Csv),
        "text/tab-separated-values" => Some(FileKind::Tsv),
        "application/json" => Some(FileKind::Json),
        "text/html" => Some(FileKind::Html),
        // Everything else this application opens is a package format whose
        // writer wants a real name. Letting `save_bytes` refuse says which
        // formats are available, which is more use than a guess.
        _ => None,
    }
}

impl CloudApp for SheetApp {
    fn cloud(&self) -> &CloudFiles {
        &self.cloud
    }

    fn cloud_mut(&mut self) -> &mut CloudFiles {
        &mut self.cloud
    }

    fn kind(&self) -> DocumentKind {
        WORKBOOK
    }

    fn active(&self) -> Option<u64> {
        self.session.active().map(|d| d.uid)
    }

    fn title(&self, uid: u64) -> String {
        self.session.documents().iter().find(|d| d.uid == uid).map(|d| d.display_title()).unwrap_or_default()
    }

    fn open_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<Option<u64>, String> {
        let encoded = gridcraft_engine::io::base64_encode(&bytes);
        match self.run("file.open", json!({"name": name, "base64": &encoded})) {
            Ok(result) => {
                self.backstage = None;
                self.show_open_warnings(&result);
                Ok(self.session.active().map(|d| d.uid))
            }
            // Protected bytes are not a workbook until a password arrives.
            // The prompt carries them on. Nothing is said about them yet.
            Err(error) if error.contains(gridcraft_engine::io::PASSWORD_REQUIRED) => {
                self.backstage = None;
                self.open_dialog("openPassword", json!({"name": name, "base64": encoded}));
                Ok(None)
            }
            Err(error) => Err(crate::clean_error(&error)),
        }
    }

    fn encode(&mut self, uid: u64, into: Destination<'_>) -> Result<Vec<u8>, String> {
        let doc = self.session.documents().iter().find(|d| d.uid == uid).ok_or("no workbook open")?;
        let snapshot = doc.wb.clone();
        // The destination decides the format. A workbook opened from the cloud
        // as a CSV goes back as a CSV. Writing XLSX under that name would hand
        // somebody a file their spreadsheet refuses to open, with nothing
        // saying why. `save_bytes` refuses an extension it cannot write and
        // names the ones it can, which is an answer a person can act on.
        let bytes = match kind_of(into) {
            Some(kind) => gridcraft_engine::io::save_bytes_as(&snapshot, into.name, kind).map_err(|e| e.to_string())?,
            None => gridcraft_engine::io::save_bytes(&snapshot, into.name).map_err(|e| e.to_string())?,
        };
        self.cloud_sent.insert(uid, snapshot);
        Ok(bytes)
    }

    fn close(&mut self, uid: u64) {
        if let Some(index) = self.session.documents().iter().position(|d| d.uid == uid) {
            self.session.close_document(index);
        }
    }

    fn mark_saved(&mut self, uid: u64) {
        let Some(snapshot) = self.cloud_sent.remove(&uid) else { return };
        if let Some(doc) = self.session.document_mut(uid) {
            doc.saved = snapshot;
        }
    }

    fn drop_pending(&mut self, uid: u64) {
        self.cloud_sent.remove(&uid);
    }

    fn ready_to_save(&mut self) -> bool {
        // A save has to include the cell the user is still typing in.
        self.commit_edit(0, 0, false, false)
    }

    fn sharing(&mut self) -> &mut craft_ui::share::ShareDialog {
        &mut self.share
    }

    fn presence(&mut self) -> &mut craft_ui::presence::PresenceBar {
        &mut self.presence
    }

    fn paint_icon(&self, painter: &Painter, rect: Rect, kind: EntryKind, colour: Color32) {
        let icon = if kind == EntryKind::Folder { Icon::Folder } else { Icon::Book };
        icons::paint(painter, rect, icon, colour);
    }

    fn open_url(&self, url: &str) {
        if let Some(open) = &self.services.open_url {
            open(url);
        }
    }

    fn say(&mut self, text: String) {
        self.message = Some(("GridCraft".to_owned(), text));
    }

    fn toast(&mut self, text: String) {
        self.toast = Some((text, now_ms()));
    }

    fn go_to_save_as(&mut self) {
        self.open_backstage(crate::backstage::Page::SaveAs);
        if let Some(backstage) = &mut self.backstage {
            backstage.save_to_cloud = Some(true);
        }
    }

    fn app_name(&self) -> &str {
        "GridCraft"
    }

    fn tr(&self, text: &str) -> String {
        self.ui.language.tr(text).to_owned()
    }

    fn service(&self) -> String {
        service_host()
    }

    fn save_note(&self) -> Option<String> {
        Some(self.tr("The workbook is saved as an Excel workbook (.xlsx) in the folder above."))
    }

    fn style(&self, ui: &Ui) -> craft_ui::cloud::CloudStyle {
        let t = Tokens::get(ui.ctx());
        craft_ui::cloud::CloudStyle {
            surface: t.ribbon,
            border: t.ribbon_border,
            text: t.text,
            dim: t.text_dim,
            hover: t.hover,
            soft: t.accent_soft,
            family: theme::ui_family(),
            bold: theme::ui_bold_family(),
        }
    }
}
