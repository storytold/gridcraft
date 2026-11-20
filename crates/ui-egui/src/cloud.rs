//! GridCraft's half of cloud storage: what a workbook is, said to the shared pages.
//!
//! The contract, the Open and Save pages, the account badge, the share dialog and the conflict
//! modal all live in [`craft_ui::cloud`], behind that crate's `cloud` feature. What is left to an
//! application is one [`CloudApp`] implementation: how a workbook is opened, how it is encoded for
//! a destination, what it is called, and which icon a file row draws. That is this module, with
//! the re-exports the rest of this crate reaches the shared pages through.
//!
//! GridCraft holds no provider credential and speaks no provider protocol. A [`CloudBackend`] is
//! the one thing a host application supplies (see `apps/gridcraft` / `apps/gridcraft-web`); this
//! crate never learns what it reaches. Nothing here opens a socket, so the pages stay buildable
//! for the web and testable with no service running.
//!
//! With no backend connected, [`available`] is `false`, and the cloud section of File ▸ Open and
//! File ▸ Save As, and the account badge in the title bar, are not drawn anywhere in this crate
//! (see `backstage.rs::{open_page, save_as}` and `ribbon/title_bar.rs`).

use std::collections::HashMap;
use std::sync::Arc;

pub use craft_cloud::{Capability, Entry, EntryKind};
pub use craft_ui::cloud::{
    Account, CloudBackend, CloudFiles, Event, Fault, Folder, Job, Post, available, browse, cancel_save_as, document_name, has_target, open_entry,
    open_page, opens, pump, report, save, save_as, save_page, start, who,
};
use craft_ui::cloud::{CloudApp, Destination, DocumentKind};
use gridcraft_engine::model::Workbook;
use serde_json::json;

use crate::SheetApp;

/// What GridCraft opens from the cloud, and what one is called in a sentence.
///
/// The extensions are the set the desktop file picker offers. The content types are read beside
/// them because the extension alone is not always there: a file exported by a web application
/// often has a correct content type and a bare name. `application/octet-stream` is deliberately
/// absent. It names no format at all, so a provider that reports it for an extensionless file
/// says nothing `opens` (or `kind_from_mime`, below) could act on; adding it here would only
/// make every file of an unrecognized kind look openable.
pub const KIND: DocumentKind = DocumentKind {
    extensions: &["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "txt", "json"],
    mimes: &[
        "application/vnd.openxmlformats-officedocument.spreadsheetml",
        "application/vnd.ms-excel",
        "application/vnd.oasis.opendocument.spreadsheet",
        "text/csv",
        "text/tab-separated-values",
        "text/plain",
        "application/json",
    ],
    noun: "workbook",
    default_extension: "xlsx",
};

/// The format a content type names, for a destination name with no extension GridCraft
/// recognizes. Deliberately narrower than `KIND.mimes`: that list also carries `text/plain` and
/// the office/OpenDocument types for matching an entry worth *opening*, where a wrong guess costs
/// nothing because the bytes are read and parsed either way. Guessed wrong here, it writes the
/// wrong package format under a name that promised the right one, so only the content types that
/// name exactly one writable, text-based format are mapped.
fn kind_from_mime(mime: &str) -> Option<gridcraft_engine::io::FileKind> {
    let mime = mime.split(';').next().unwrap_or(mime).trim();
    match mime {
        "text/csv" => Some(gridcraft_engine::io::FileKind::Csv),
        "text/tab-separated-values" => Some(gridcraft_engine::io::FileKind::Tsv),
        "application/json" => Some(gridcraft_engine::io::FileKind::Json),
        _ => None,
    }
}

/// What a cloud command answers with when no host connected a backend.
pub const UNAVAILABLE: &str = "no cloud service is configured";

/// The workbook snapshot each cloud save in flight encoded. Only GridCraft knows what a version
/// of a workbook is, so the shared pages hold none of these.
pub type Sent = HashMap<u64, Arc<Workbook>>;

impl CloudApp for SheetApp {
    fn cloud(&self) -> &CloudFiles {
        &self.cloud
    }

    fn cloud_mut(&mut self) -> &mut CloudFiles {
        &mut self.cloud
    }

    fn kind(&self) -> DocumentKind {
        KIND
    }

    fn active(&self) -> Option<u64> {
        self.session.active().map(|d| d.uid)
    }

    fn title(&self, uid: u64) -> String {
        self.session.document_index(uid).map(|i| self.session.documents()[i].display_title()).unwrap_or_default()
    }

    /// Opens bytes fetched from the cloud through the same path a dropped or web-picked file
    /// takes, so an encrypted workbook asks for its password exactly as a local one does.
    fn open_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<Option<u64>, String> {
        let encoded = gridcraft_engine::io::base64_encode(&bytes);
        match self.run("file.open", json!({"name": name, "base64": &encoded})) {
            Ok(answer) => {
                let uid = self.session.active().map(|d| d.uid);
                self.after_open(None, &answer);
                Ok(uid)
            }
            // The password prompt owns the bytes now. Whether a workbook was opened is not known
            // until somebody types one, so nothing is claimed here.
            Err(why) if why.contains(gridcraft_engine::io::PASSWORD_REQUIRED) => {
                self.open_dialog("openPassword", json!({"name": name, "base64": encoded}));
                Ok(None)
            }
            Err(why) => Err(crate::clean_error(&why)),
        }
    }

    /// The destination's name decides the format, so a workbook opened as `figures.csv` goes back
    /// as a CSV. A name with no extension GridCraft recognizes falls back to the content type a
    /// provider reported for it, when that type names one GridCraft can write; a provider that
    /// issued no extension at all still often says what the thing is. A format GridCraft cannot
    /// write either way is refused by `save_bytes` and the reason is put to the user, which beats
    /// handing them a file that is not what its name says.
    fn encode(&mut self, uid: u64, into: Destination<'_>) -> Result<Vec<u8>, String> {
        if !self.commit_edit(0, 0, false, false) {
            return Err("could not commit the cell being edited".to_owned());
        }
        let index = self.session.document_index(uid).ok_or("that workbook is no longer open")?;
        let snapshot = self.session.documents()[index].wb.clone();
        let bytes = match gridcraft_engine::io::FileKind::from_path(into.name).or_else(|| into.mime.and_then(kind_from_mime)) {
            Some(kind) => gridcraft_engine::io::save_bytes_as(&snapshot, into.name, kind),
            None => gridcraft_engine::io::save_bytes(&snapshot, into.name),
        }
        .map_err(|e| e.to_string())?;
        self.cloud_sent.insert(uid, snapshot);
        Ok(bytes)
    }

    fn close(&mut self, uid: u64) {
        if let Some(index) = self.session.document_index(uid) {
            self.session.close_document(index);
        }
    }

    /// The version these bytes were is now the saved one. An edit made while they were on the
    /// wire stays unsaved, since the snapshot is the one `encode` took.
    fn mark_saved(&mut self, uid: u64) {
        if let Some(snapshot) = self.cloud_sent.remove(&uid)
            && let Some(d) = self.session.document_mut(uid)
        {
            d.saved = snapshot;
        }
        self.backstage = None;
    }

    fn drop_pending(&mut self, uid: u64) {
        self.cloud_sent.remove(&uid);
    }

    fn ready_to_save(&mut self) -> bool {
        self.commit_edit(0, 0, false, false)
    }

    fn sharing(&mut self) -> &mut craft_ui::share::ShareDialog {
        &mut self.sharing
    }

    fn presence(&mut self) -> &mut craft_ui::presence::PresenceBar {
        &mut self.presence
    }

    fn paint_icon(&self, painter: &egui::Painter, rect: egui::Rect, kind: EntryKind, colour: egui::Color32) {
        let icon = match kind {
            EntryKind::Folder => crate::icons::Icon::Folder,
            EntryKind::File => crate::icons::Icon::Book,
        };
        crate::icons::paint(painter, rect, icon, colour);
    }

    /// Through the host, which is the only thing that knows how a link is opened here. A build
    /// whose host wired nothing opens nothing.
    fn open_url(&self, url: &str) {
        if let Some(open) = &self.services.open_url {
            open(url);
        }
    }

    fn say(&mut self, text: String) {
        self.message = Some(("GridCraft".to_owned(), text));
    }

    fn toast(&mut self, text: String) {
        self.toast = Some((text, crate::now_ms()));
    }

    fn go_to_save_as(&mut self) {
        self.backstage = Some(crate::backstage::Backstage::new(crate::backstage::Page::SaveAs));
    }

    fn app_name(&self) -> &str {
        "GridCraft"
    }

    fn tr(&self, text: &str) -> String {
        self.ui.language.tr(text).to_owned()
    }
}
