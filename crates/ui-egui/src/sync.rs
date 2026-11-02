//! Zero-server, end-to-end encrypted P2P collaboration ("Share Mode") powered by `simple-sync`.
//!
//! All local workbook/sheet mutations on the shared workbook pass through
//! [`SheetApp::run_engine_synced`], which enforces View-Only permissions, serializes
//! deterministic engine commands into [`SharedMutation`] payloads, and broadcasts them
//! over `simple-sync`. Incoming [`simple_sync::SyncEvent`]s are polled once per frame in
//! [`SheetApp::logic`] and applied directly to the shared workbook via
//! [`SheetApp::apply_remote_mutation`] without re-broadcasting.

use std::sync::Arc;

use gridcraft_engine::Selection;
use gridcraft_engine::core::RangeRef;
use gridcraft_engine::model::{Sheet, Workbook};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::{Notice, NoticeKind, SheetApp, clean_error, now_ms};

/// Collaboration session status, mirroring `simple_sync::ShareRole` across all targets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShareStatus {
    /// Not sharing or connected to any session.
    #[default]
    Offline,
    /// Acting as the Host (source of truth) for the shared workbook.
    Hosting,
    /// Connected to a Host with full read/write permissions.
    JoinedCanEdit,
    /// Connected to a Host in read-only mode (local edits to the shared workbook are blocked).
    JoinedViewOnly,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<simple_sync::ShareRole> for ShareStatus {
    fn from(role: simple_sync::ShareRole) -> Self {
        match role {
            simple_sync::ShareRole::Offline => Self::Offline,
            simple_sync::ShareRole::Hosting => Self::Hosting,
            simple_sync::ShareRole::JoinedCanEdit => Self::JoinedCanEdit,
            simple_sync::ShareRole::JoinedViewOnly => Self::JoinedViewOnly,
        }
    }
}

/// Wire payload broadcast for every deterministic workbook/sheet mutation on the shared document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedMutation {
    /// Active sheet name on the shared workbook immediately before the command ran.
    pub sheet_name: String,
    /// Cell selection on the shared workbook immediately before the command ran.
    pub selection: Selection,
    /// Selected floating object `(kind, id)` on the shared workbook before the command ran.
    pub selected_object: Option<(String, u32)>,
    /// Engine command identifier (e.g. `"cell.set"`, `"home.bold"`, `"insert.table"`).
    pub id: String,
    /// JSON-encoded command parameters (enriched with resolved toggle state when applicable).
    pub params_json: String,
}

impl SharedMutation {
    /// Serializes this mutation into a compact binary wire frame, enforcing `MAX_MUTATION_BYTES`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn to_wire(&self) -> Result<Vec<u8>, String> {
        let bytes = bincode::serialize(self).map_err(|e| format!("failed to serialize mutation: {e}"))?;
        if bytes.len() > simple_sync::MAX_MUTATION_BYTES {
            return Err(format!("mutation payload ({} bytes) exceeds MAX_MUTATION_BYTES ({} bytes)", bytes.len(), simple_sync::MAX_MUTATION_BYTES));
        }
        Ok(bytes)
    }

    /// Deserializes a binary wire frame back into a [`SharedMutation`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_wire(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > simple_sync::MAX_MUTATION_BYTES {
            return Err(format!("mutation payload ({} bytes) exceeds MAX_MUTATION_BYTES ({} bytes)", bytes.len(), simple_sync::MAX_MUTATION_BYTES));
        }
        bincode::deserialize(bytes).map_err(|e| format!("failed to deserialize mutation: {e}"))
    }
}

/// How an engine command interacts with a live Share Mode session on the shared workbook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandSyncPolicy {
    /// Deterministic workbook/sheet mutation (or clipboard/format-painter staging for a mutation)
    /// that is serialized into [`SharedMutation`] and broadcast to peers.
    Shared,
    /// Purely local navigation, view, window, file, or query command. Runs locally without
    /// broadcasting and without showing an unshared-action warning.
    LocalOnly,
    /// Action that mutates or depends on local-only state (protection passwords, macros/scripts,
    /// local file paths, or undo/redo during live collaboration). Never dropped silently:
    /// surfaces a clear user notice/toast explaining why it is not shared.
    UnsharedNotice(&'static str),
}

pub const NOTICE_PROTECTION: &str = "Sheet and workbook protection changes stay on this device and aren't shared with peers.";
pub const NOTICE_MACROS: &str = "Macros and automation scripts stay on this device and aren't shared with peers.";
pub const NOTICE_LOCAL_FILE: &str = "Local file imports stay on this device and aren't shared with peers.";
pub const NOTICE_UNDO_REDO: &str = "Undo and Redo are disabled on the shared workbook while live sharing is active.";
pub const NOTICE_VIEW_ONLY: &str = "View-only session — edits are disabled.";

/// Classifies an engine command id and its parameters into a [`CommandSyncPolicy`].
pub fn classify_command(id: &str, params: &Json) -> CommandSyncPolicy {
    if matches!(id, "edit.undo" | "edit.redo") {
        return CommandSyncPolicy::UnsharedNotice(NOTICE_UNDO_REDO);
    }
    if matches!(id, "review.protectSheet" | "review.unprotectSheet" | "review.protectWorkbook" | "review.allowEditRanges") {
        return CommandSyncPolicy::UnsharedNotice(NOTICE_PROTECTION);
    }
    if id.starts_with("automate.") || id.starts_with("macro.") {
        return CommandSyncPolicy::UnsharedNotice(NOTICE_MACROS);
    }
    if uses_local_file_path(id, params) {
        return CommandSyncPolicy::UnsharedNotice(NOTICE_LOCAL_FILE);
    }
    if is_local_only_command(id) {
        return CommandSyncPolicy::LocalOnly;
    }
    CommandSyncPolicy::Shared
}

fn uses_local_file_path(id: &str, params: &Json) -> bool {
    let has_path = params.get("path").and_then(Json::as_str).is_some_and(|s| !s.is_empty());
    match id {
        "insert.picture" => has_path && params.get("base64").and_then(Json::as_str).is_none_or(str::is_empty),
        "data.fromTextCsv" | "data.getData" => has_path && params.get("text").and_then(Json::as_str).is_none_or(str::is_empty),
        "pageLayout.background" => has_path && params.get("base64").and_then(Json::as_str).is_none_or(str::is_empty),
        _ => false,
    }
}

fn is_local_only_command(id: &str) -> bool {
    if id.starts_with("selection.") || id.starts_with("window.") {
        return true;
    }
    matches!(
        id,
        "sheet.activate"
            | "sheet.next"
            | "sheet.previous"
            | "edit.selectAll"
            | "edit.goTo"
            | "edit.goToSpecial"
            | "edit.find"
            | "edit.beginEdit"
            | "view.zoom"
            | "view.zoom100"
            | "view.zoomToSelection"
            | "view.normal"
            | "view.pageLayout"
            | "view.pageBreakPreview"
            | "view.workbookViews"
            | "view.split"
            | "view.newWindow"
            | "view.arrangeAll"
            | "view.hideWindow"
            | "view.unhideWindow"
            | "view.switchWindows"
            | "view.ruler"
            | "view.formulaBar"
            | "view.statusBar"
            | "view.macros"
            | "file.new"
            | "file.open"
            | "file.save"
            | "file.saveAs"
            | "file.saveBytes"
            | "file.close"
            | "file.exportCsv"
            | "file.exportHtml"
            | "file.exportPdf"
            | "file.print"
            | "file.printPreview"
            | "file.pageSetup"
            | "file.options"
            | "formulas.insertFunction"
            | "formulas.functions"
            | "formulas.nameManager"
            | "formulas.useInFormula"
            | "formulas.tracePrecedents"
            | "formulas.traceDependents"
            | "formulas.removeArrows"
            | "formulas.errorChecking"
            | "formulas.evaluateFormula"
            | "formulas.evaluate"
            | "formulas.watchWindow"
            | "formulas.calculateNow"
            | "formulas.calculateSheet"
            | "formulas.selectCells"
            | "formulas.recentlyUsed"
            | "formulas.financial"
            | "formulas.logical"
            | "formulas.text"
            | "formulas.dateTime"
            | "formulas.lookupReference"
            | "formulas.mathTrig"
            | "formulas.moreFunctions"
            | "data.validate"
            | "data.circleInvalid"
            | "data.queriesConnections"
            | "review.previousComment"
            | "review.nextComment"
            | "review.showComments"
            | "review.showAllNotes"
            | "review.spelling"
            | "review.addToDictionary"
            | "review.workbookStatistics"
            | "review.checkAccessibility"
            | "draw.pen"
            | "draw.eraser"
            | "draw.lasso"
            | "link.open"
    )
}

fn is_clipboard_staging_command(id: &str) -> bool {
    matches!(id, "edit.copy" | "edit.cut" | "edit.clearClipboard" | "edit.formatPainter")
}

/// Enriches toggle-command parameters with the resolved state from the command's result, so
/// replaying the command on a peer sets the exact same state rather than flipping it.
pub fn enrich_params_with_result(id: &str, params: Json, result: &Json) -> Json {
    let mut cleaned = gridcraft_engine::cmd::journal_params(&params);
    let state = match id {
        "python.output" => result.get("output").filter(|o| o.is_string()).map(|o| ("mode", o)),
        _ => result.get("on").filter(|o| o.is_boolean()).map(|o| ("on", o)),
    };
    if let Some((key, val)) = state {
        if cleaned.is_null() {
            cleaned = Json::Object(serde_json::Map::new());
        }
        if let Json::Object(map) = &mut cleaned
            && map.get(key).is_none_or(Json::is_null)
        {
            map.insert(key.into(), val.clone());
        }
    }
    cleaned
}

/// Sanitizes local-only workbook state (macros, VBA projects, protection password hashes, local
/// paths) and encodes the workbook to `.xlsx` bytes for the base document snapshot.
pub fn encode_workbook_for_share(wb: &Workbook) -> Result<Vec<u8>, String> {
    let mut clean = wb.clone();
    clean.vba_project = None;
    clean.macros = gridcraft_engine::model::Macros::default();
    clean.macros_read.clear();
    clean.macros_unread = Arc::default();
    clean.structure_password = None;
    clean.source_dir = None;
    clean.source_path = None;
    for sheet_arc in &mut clean.sheets {
        if sheet_arc.protection.as_ref().is_some_and(|p| p.password.is_some()) || !sheet_arc.edit_ranges.is_empty() {
            let sheet: &mut Sheet = Arc::make_mut(sheet_arc);
            if let Some(prot) = &mut sheet.protection {
                prot.password = None;
            }
            for er in &mut sheet.edit_ranges {
                er.password = None;
            }
        }
    }
    gridcraft_engine::io::save_bytes_as(&clean, "Shared.xlsx", gridcraft_engine::io::FileKind::Xlsx).map_err(|e| e.to_string())
}

/// Decodes a shared base document snapshot from `.xlsx` bytes into a [`Workbook`].
pub fn decode_workbook_from_share(bytes: &[u8]) -> Result<Workbook, String> {
    let (mut wb, _) = gridcraft_engine::io::open_bytes("Shared.xlsx", bytes).map_err(|e| e.to_string())?;
    wb.source_dir = None;
    wb.source_path = None;
    wb.preserved = None;
    for sheet_arc in &mut wb.sheets {
        if sheet_arc.file_id != 0 || sheet_arc.preserved.is_some() {
            let sh = Arc::make_mut(sheet_arc);
            sh.file_id = 0;
            sh.preserved = None;
        }
    }
    Ok(wb)
}

#[cfg(not(target_arch = "wasm32"))]
enum PendingOutcome {
    Hosted(simple_sync::SyncEngine, Result<simple_sync::ShareLinks, String>),
    Joined(simple_sync::SyncEngine, Result<simple_sync::ShareRole, String>),
    Stopped(simple_sync::SyncEngine, Result<(), String>),
}

/// Per-app collaboration state held on [`SheetApp`].
pub struct SyncState {
    #[cfg(not(target_arch = "wasm32"))]
    engine: Option<simple_sync::SyncEngine>,
    #[cfg(not(target_arch = "wasm32"))]
    engine_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    engine_config: Option<simple_sync::SyncConfig>,
    #[cfg(not(target_arch = "wasm32"))]
    pending_rx: Option<std::sync::mpsc::Receiver<PendingOutcome>>,
    pub pending_label: Option<&'static str>,
    pub shared_doc_uid: Option<u64>,
    pub edit_ticket: Option<String>,
    pub view_ticket: Option<String>,
    pub join_input: String,
    pub last_error: Option<String>,
    pub(crate) applying_remote: bool,
    pub(crate) remote_clipboard: Option<gridcraft_engine::Clipboard>,
    pub(crate) remote_format_painter: Option<(Arc<Workbook>, usize, RangeRef, bool)>,
    cached_status: ShareStatus,
    cached_peers: usize,
}

impl Default for SyncState {
    fn default() -> Self {
        Self::new("gridcraft")
    }
}

impl SyncState {
    pub fn new(name: &str) -> Self {
        #[cfg(target_arch = "wasm32")]
        let _ = name;
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            engine: None,
            #[cfg(not(target_arch = "wasm32"))]
            engine_name: name.to_string(),
            #[cfg(not(target_arch = "wasm32"))]
            engine_config: None,
            #[cfg(not(target_arch = "wasm32"))]
            pending_rx: None,
            pending_label: None,
            shared_doc_uid: None,
            edit_ticket: None,
            view_ticket: None,
            join_input: String::new(),
            last_error: None,
            applying_remote: false,
            remote_clipboard: None,
            remote_format_painter: None,
            cached_status: ShareStatus::Offline,
            cached_peers: 0,
        }
    }

    /// Creates a [`SyncState`] configured with a custom [`simple_sync::SyncConfig`] (e.g.
    /// `SyncConfig::local_only()` for fast offline integration tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_config(name: &str, config: simple_sync::SyncConfig) -> Self {
        let mut state = Self::new(name);
        state.engine_config = Some(config.clone());
        state.engine = simple_sync::SyncEngine::with_config(name, config).ok();
        state
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_engine(&mut self) -> Option<simple_sync::SyncEngine> {
        if let Some(engine) = self.engine.take() {
            return Some(engine);
        }
        if self.pending_rx.is_some() {
            return None;
        }
        match &self.engine_config {
            Some(cfg) => simple_sync::SyncEngine::with_config(&self.engine_name, cfg.clone()).ok(),
            None => simple_sync::SyncEngine::new(&self.engine_name).ok(),
        }
    }

    /// Current collaboration status (`Offline`, `Hosting`, `JoinedCanEdit`, `JoinedViewOnly`).
    pub fn status(&self) -> ShareStatus {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(engine) = &self.engine {
            return ShareStatus::from(engine.role());
        }
        self.cached_status
    }

    /// Number of connected peers in the active Share Mode session.
    pub fn connected_peers(&self) -> usize {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(engine) = &self.engine {
            return engine.connected_peers_count();
        }
        self.cached_peers
    }

    /// Whether a background Share/Join/Stop operation is currently in flight.
    pub fn is_busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending_rx.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Returns `true` if `doc_uid` is the currently shared workbook in an active session.
    pub fn is_sharing_doc(&self, doc_uid: u64) -> bool {
        self.status() != ShareStatus::Offline && self.shared_doc_uid == Some(doc_uid)
    }

    /// Broadcasts a [`SharedMutation`] to all connected peers if the session allows editing.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn broadcast(&mut self, mutation: &SharedMutation) -> Result<(), String> {
        if self.applying_remote {
            return Ok(());
        }
        let status = self.status();
        if !matches!(status, ShareStatus::Hosting | ShareStatus::JoinedCanEdit) {
            return Ok(());
        }
        if let Some(engine) = &self.engine {
            engine.broadcast_mutation(mutation).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub fn broadcast(&mut self, _mutation: &SharedMutation) -> Result<(), String> {
        Ok(())
    }
}

impl SheetApp {
    /// Whether the local user is allowed to edit the currently active workbook.
    ///
    /// Returns `false` when the active workbook is the shared workbook and this peer joined
    /// with a View-Only link (`ShareStatus::JoinedViewOnly`). Other open workbooks remain
    /// editable even while viewing a shared workbook in View-Only mode.
    pub fn local_editing_allowed(&self) -> bool {
        let Some(active_uid) = self.session.active().map(|d| d.uid) else {
            return true;
        };
        !(self.sync.status() == ShareStatus::JoinedViewOnly && self.sync.shared_doc_uid == Some(active_uid))
    }

    /// Shows the standard View-Only toast when an edit is refused on a View-Only shared workbook.
    pub(crate) fn notify_view_only_refused(&mut self) {
        self.toast = Some((NOTICE_VIEW_ONLY.into(), now_ms()));
    }

    /// Starts hosting a live Share Mode session for the active workbook on a background thread.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_sharing(&mut self) {
        if self.sync.is_busy() {
            return;
        }
        if self.settle_drafts().is_err() {
            return;
        }
        let Some(d) = self.session.active() else {
            return;
        };
        let doc_uid = d.uid;
        let filename = d.display_title();
        let doc_bytes = match encode_workbook_for_share(&d.wb) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.sync.last_error = Some(e.clone());
                self.toast = Some((format!("Failed to prepare workbook for sharing: {e}"), now_ms()));
                return;
            }
        };
        let Some(mut engine) = self.sync.ensure_engine() else {
            return;
        };
        self.sync.shared_doc_uid = Some(doc_uid);
        self.sync.last_error = None;
        self.sync.pending_label = Some("Starting Share Mode…");
        let (tx, rx) = std::sync::mpsc::channel();
        self.sync.pending_rx = Some(rx);
        std::thread::spawn(move || {
            let res = engine.enable_share_mode(&filename, doc_bytes).map_err(|e| e.to_string());
            let _ = tx.send(PendingOutcome::Hosted(engine, res));
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn start_sharing(&mut self) {}

    /// Joins an existing Share Mode session via a `gridcraft://...` or `ssync://...` ticket on a
    /// background thread. The received base document will load into the active workbook slot.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_joining(&mut self, ticket: String) {
        if self.sync.is_busy() {
            return;
        }
        let trimmed = ticket.trim().to_string();
        if trimmed.is_empty() {
            let msg = "Paste a Share link to join.".to_string();
            self.sync.last_error = Some(msg.clone());
            self.toast = Some((msg, now_ms()));
            return;
        }
        let _ = self.settle_drafts();
        if self.session.active().is_none() {
            self.session.new_workbook();
        }
        let doc_uid = self.session.active().map(|d| d.uid);
        let Some(mut engine) = self.sync.ensure_engine() else {
            return;
        };
        self.sync.shared_doc_uid = doc_uid;
        self.sync.last_error = None;
        self.sync.pending_label = Some("Joining session…");
        let (tx, rx) = std::sync::mpsc::channel();
        self.sync.pending_rx = Some(rx);
        std::thread::spawn(move || {
            let res = engine.join_via_ticket(&trimmed).map_err(|e| e.to_string());
            let _ = tx.send(PendingOutcome::Joined(engine, res));
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn start_joining(&mut self, _ticket: String) {}

    /// Stops hosting or leaves the current Share Mode session on a background thread.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn stop_sharing(&mut self) {
        if self.sync.is_busy() {
            return;
        }
        let Some(mut engine) = self.sync.engine.take() else {
            self.sync.cached_status = ShareStatus::Offline;
            self.sync.cached_peers = 0;
            self.sync.shared_doc_uid = None;
            self.sync.edit_ticket = None;
            self.sync.view_ticket = None;
            self.sync.remote_clipboard = None;
            self.sync.remote_format_painter = None;
            return;
        };
        self.sync.pending_label = Some("Stopping Share Mode…");
        let (tx, rx) = std::sync::mpsc::channel();
        self.sync.pending_rx = Some(rx);
        std::thread::spawn(move || {
            let res = engine.disable_share_mode().map_err(|e| e.to_string());
            let _ = tx.send(PendingOutcome::Stopped(engine, res));
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn stop_sharing(&mut self) {}

    /// Polls in-flight Share Mode lifecycle operations and incoming `SyncEvent`s without blocking.
    pub fn poll_sync(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.poll_sync_pending(ctx);
            self.poll_sync_events(ctx);
            if self.sync.is_busy() || self.sync.status() != ShareStatus::Offline {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ctx;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn poll_sync_pending(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.sync.pending_rx else {
            return;
        };
        let Ok(outcome) = rx.try_recv() else {
            return;
        };
        self.sync.pending_rx = None;
        self.sync.pending_label = None;
        match outcome {
            PendingOutcome::Hosted(engine, res) => {
                self.sync.cached_status = ShareStatus::from(engine.role());
                self.sync.cached_peers = engine.connected_peers_count();
                self.sync.engine = Some(engine);
                match res {
                    Ok(links) => {
                        self.sync.edit_ticket = Some(links.edit_uri);
                        self.sync.view_ticket = Some(links.view_only_uri);
                        self.toast = Some(("Share Mode is live. Copy an Edit or View-Only link from the title bar.".into(), now_ms()));
                    }
                    Err(e) => {
                        self.sync.shared_doc_uid = None;
                        self.sync.last_error = Some(e.clone());
                        self.toast = Some((format!("Share failed: {e}"), now_ms()));
                    }
                }
            }
            PendingOutcome::Joined(engine, res) => {
                let status = ShareStatus::from(engine.role());
                self.sync.cached_status = status;
                self.sync.cached_peers = engine.connected_peers_count();
                self.sync.engine = Some(engine);
                match res {
                    Ok(_) => {
                        self.sync.join_input.clear();
                        if status == ShareStatus::JoinedViewOnly {
                            if self.editor.as_ref().is_some_and(|ed| Some(ed.doc) == self.sync.shared_doc_uid) {
                                self.cancel_edit();
                            }
                            self.cancel_text_box_edit();
                            self.toast = Some(("Joined session in View-Only mode.".into(), now_ms()));
                        } else {
                            self.toast = Some(("Joined live collaboration session.".into(), now_ms()));
                        }
                    }
                    Err(e) => {
                        self.sync.shared_doc_uid = None;
                        self.sync.last_error = Some(e.clone());
                        self.toast = Some((format!("Join failed: {e}"), now_ms()));
                    }
                }
            }
            PendingOutcome::Stopped(engine, res) => {
                self.sync.cached_status = ShareStatus::Offline;
                self.sync.cached_peers = 0;
                self.sync.shared_doc_uid = None;
                self.sync.edit_ticket = None;
                self.sync.view_ticket = None;
                self.sync.remote_clipboard = None;
                self.sync.remote_format_painter = None;
                self.sync.engine = Some(engine);
                if let Err(e) = res {
                    self.sync.last_error = Some(e.clone());
                    self.toast = Some((format!("Stop sharing error: {e}"), now_ms()));
                } else {
                    self.toast = Some(("Share Mode stopped.".into(), now_ms()));
                }
            }
        }
        ctx.request_repaint();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn poll_sync_events(&mut self, ctx: &egui::Context) {
        // Check if the shared document was closed locally while sharing.
        if let Some(uid) = self.sync.shared_doc_uid
            && self.sync.status() != ShareStatus::Offline
            && self.session.document_index(uid).is_none()
        {
            self.stop_sharing();
            return;
        }
        let mut events = Vec::new();
        if let Some(engine) = &mut self.sync.engine {
            while let Some(ev) = engine.poll() {
                events.push(ev);
            }
        }
        if events.is_empty() {
            return;
        }
        for event in events {
            match event {
                simple_sync::SyncEvent::BaseDocumentReady { bytes, .. } => {
                    if let Err(e) = self.apply_remote_base_document(&bytes) {
                        self.sync.last_error = Some(e.clone());
                        self.toast = Some((format!("Failed to load shared workbook: {e}"), now_ms()));
                    }
                }
                simple_sync::SyncEvent::RemoteMutation { mutation_bytes, .. } => match SharedMutation::from_wire(&mutation_bytes) {
                    Ok(mutation) => {
                        if let Err(e) = self.apply_remote_mutation(&mutation) {
                            self.sync.last_error = Some(e.clone());
                        }
                    }
                    Err(e) => {
                        self.sync.last_error = Some(e.clone());
                    }
                },
                simple_sync::SyncEvent::CrdtUpdated { .. } => {}
                simple_sync::SyncEvent::PeerJoined { .. } | simple_sync::SyncEvent::PeerLeft { .. } => {
                    if let Some(engine) = &self.sync.engine {
                        self.sync.cached_peers = engine.connected_peers_count();
                    }
                }
                simple_sync::SyncEvent::Error(err) => {
                    self.sync.last_error = Some(err.clone());
                    self.toast = Some((format!("Sync error: {err}"), now_ms()));
                }
            }
        }
        if let Some(engine) = &self.sync.engine {
            self.sync.cached_status = ShareStatus::from(engine.role());
            self.sync.cached_peers = engine.connected_peers_count();
        }
        ctx.request_repaint();
    }

    /// Loads the host's base document snapshot into the shared workbook slot without broadcasting.
    pub fn apply_remote_base_document(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut wb = decode_workbook_from_share(bytes)?;
        wb.refresh_equations();
        let mut calc = gridcraft_engine::calc::Calc::new();
        calc.rebuild(&wb);
        let wb_arc = Arc::new(wb);
        let sel = Selection::at(wb_arc.active().map(|s| s.view_active).unwrap_or_default());

        let prev_window = self.session.active_window();
        let target_idx = self.sync.shared_doc_uid.and_then(|uid| self.session.document_index(uid));

        if let Some(idx) = target_idx {
            let stay_elsewhere = self.session.active_index() != idx;
            self.session.set_active(idx);
            if let Some(d) = self.session.active_mut() {
                d.wb = wb_arc.clone();
                d.saved = wb_arc;
                d.calc = calc;
                d.selection = sel;
                d.undo.clear();
                d.redo.clear();
                d.sheet_selections.clear();
                d.unlocked_ranges.clear();
                d.scripts_trusted = false;
                d.trust = gridcraft_engine::Trust::FromFile;
                d.revision = d.revision.saturating_add(1);
            }
            if stay_elsewhere && let Some(w) = prev_window {
                self.session.activate_window(w);
            }
        } else {
            let doc = gridcraft_engine::DocState::from_saved((*wb_arc).clone(), None, "Shared.xlsx".into(), gridcraft_engine::Trust::FromFile);
            let uid = doc.uid;
            self.session.add_document(doc);
            self.sync.shared_doc_uid = Some(uid);
        }
        self.follow_open_draft();
        Ok(())
    }

    /// Applies an incoming [`SharedMutation`] to the shared workbook without re-broadcasting.
    ///
    /// Strictly separated from [`SheetApp::run_engine_synced`] so remote mutations never loop back
    /// into `broadcast_mutation` and never disturb another workbook the local user is viewing.
    pub fn apply_remote_mutation(&mut self, mutation: &SharedMutation) -> Result<(), String> {
        let Some(shared_uid) = self.sync.shared_doc_uid else {
            return Err("no shared document registered".into());
        };
        let Some(shared_idx) = self.session.document_index(shared_uid) else {
            return Err("shared document is no longer open".into());
        };
        let params: Json = serde_json::from_str(&mutation.params_json).map_err(|e| format!("invalid mutation JSON: {e}"))?;

        let prev_window = self.session.active_window();
        let was_on_other_doc = self.session.active_index() != shared_idx;

        self.session.set_active(shared_idx);
        let (local_sheet_name, local_selection) = match self.session.active() {
            Some(d) => (d.wb.active().map(|s| s.name.clone()), d.selection.clone()),
            None => return Err("shared document unavailable".into()),
        };

        if let Some(d) = self.session.active_mut() {
            if let Some(si) = d.wb.sheet_index(&mutation.sheet_name) {
                d.wb.set_active_sheet(si);
            }
            d.selection = mutation.selection.clone();
        }
        let sheet_before_cmd = self.session.active().and_then(|d| d.wb.active().map(|s| s.name.clone()));

        let local_clipboard = self.session.clipboard.take();
        let local_format_painter = self.session.format_painter.take();
        self.session.clipboard = self.sync.remote_clipboard.take();
        self.session.format_painter = self.sync.remote_format_painter.take();
        let requests_before = self.session.ui_requests.len();

        self.sync.applying_remote = true;
        let exec_res = self.session.execute(&mutation.id, params).map_err(|e| e.to_string());
        self.sync.applying_remote = false;

        self.sync.remote_clipboard = self.session.clipboard.take();
        self.sync.remote_format_painter = self.session.format_painter.take();
        self.session.clipboard = local_clipboard;
        self.session.format_painter = local_format_painter;
        self.session.ui_requests.truncate(requests_before);

        // Preserve the local user's sheet and cursor on the shared workbook when the remote
        // command did not intentionally switch sheets (e.g. `home.insertSheet` / `home.deleteSheet`).
        if let Some(d) = self.session.active_mut() {
            let sheet_after_cmd = d.wb.active().map(|s| s.name.clone());
            let cmd_switched_sheet = sheet_before_cmd != sheet_after_cmd
                || matches!(mutation.id.as_str(), "home.insertSheet" | "home.deleteSheet" | "sheet.move" | "sheet.hide" | "sheet.unhide");
            if !cmd_switched_sheet
                && let Some(local_name) = &local_sheet_name
                && let Some(local_si) = d.wb.sheet_index(local_name)
            {
                d.wb.set_active_sheet(local_si);
            }
            if d.wb.active().map(|s| s.name.as_str()) == local_sheet_name.as_deref() {
                d.selection = local_selection;
            }
        }

        if was_on_other_doc && let Some(w) = prev_window {
            self.session.activate_window(w);
        }
        self.follow_open_draft();
        exec_res.map(|_| ())
    }

    /// Runs an engine command initiated by the local user, enforcing View-Only permissions on the
    /// shared workbook and broadcasting deterministic mutations to connected peers.
    pub fn run_engine_synced(&mut self, id: &str, params: Json) -> Result<Json, String> {
        let active_uid = self.session.active().map(|d| d.uid);
        let on_shared_doc = active_uid.is_some_and(|uid| self.sync.is_sharing_doc(uid));

        if !on_shared_doc {
            return self.session.run(id, params);
        }

        let policy = classify_command(id, &params);
        match policy {
            CommandSyncPolicy::LocalOnly => self.session.run(id, params),
            CommandSyncPolicy::UnsharedNotice(notice) => {
                if !self.local_editing_allowed() {
                    self.notify_view_only_refused();
                    return Err(format!("command `{id}` is not available right now: {NOTICE_VIEW_ONLY}"));
                }
                if matches!(id, "edit.undo" | "edit.redo") {
                    self.toast = Some((notice.into(), now_ms()));
                    return Err(format!("command `{id}` is not available right now: {notice}"));
                }
                let res = self.session.run(id, params);
                if res.is_ok() {
                    self.notice = Some(Notice { text: notice.into(), kind: NoticeKind::Info, action: None });
                    self.toast = Some((notice.into(), now_ms()));
                }
                res
            }
            CommandSyncPolicy::Shared => {
                // Allow `edit.copy` locally in View-Only mode without broadcasting it.
                if !self.local_editing_allowed() {
                    if id == "edit.copy" {
                        return self.session.run(id, params);
                    }
                    self.notify_view_only_refused();
                    return Err(format!("command `{id}` is not available right now: {NOTICE_VIEW_ONLY}"));
                }

                let pre = self
                    .session
                    .active()
                    .map(|d| (d.uid, d.wb.clone(), d.wb.active().map(|s| s.name.clone()).unwrap_or_default(), d.selection.clone()));
                let selected_object = self.selected_object.map(|o| (o.kind.name().to_string(), o.id));

                let res = self.session.run(id, params.clone());
                if let (Ok(result_json), Some((doc_uid, wb_before, sheet_name, selection))) = (&res, pre) {
                    let wb_changed = self.session.documents().iter().find(|d| d.uid == doc_uid).is_some_and(|d| !Arc::ptr_eq(&d.wb, &wb_before));
                    let stages_clipboard = is_clipboard_staging_command(id);
                    if wb_changed || stages_clipboard {
                        if stages_clipboard {
                            self.sync.remote_clipboard = self.session.clipboard.clone();
                            self.sync.remote_format_painter = self.session.format_painter.clone();
                        }
                        let enriched = enrich_params_with_result(id, params, result_json);
                        if let Ok(params_json) = serde_json::to_string(&enriched) {
                            let mutation = SharedMutation { sheet_name, selection, selected_object, id: id.to_string(), params_json };
                            if let Err(err) = self.sync.broadcast(&mutation) {
                                self.sync.last_error = Some(err.clone());
                                self.toast = Some((format!("Sync broadcast error: {clean}", clean = clean_error(&err)), now_ms()));
                            }
                        }
                    }
                }
                res
            }
        }
    }
}

/// Renders active Share Mode status and action buttons inline on the title bar when a session is
/// live or starting/stopping.
pub fn share_title_bar_inline(app: &mut SheetApp, ui: &mut egui::Ui) {
    if let Some(label) = app.sync.pending_label {
        ui.label(egui::RichText::new(label).size(12.0));
        ui.add(egui::Spinner::new().size(14.0));
        return;
    }
    let peers = app.sync.connected_peers();
    match app.sync.status() {
        ShareStatus::Offline => {}
        ShareStatus::Hosting => {
            if ui.small_button("Stop Sharing").on_hover_text("Disconnect all peers and end the live session").clicked() {
                app.stop_sharing();
            }
            if let Some(ticket) = app.sync.view_ticket.clone()
                && ui.small_button("Copy View-Only Link").on_hover_text("Copy read-only invite link").clicked()
            {
                ui.ctx().copy_text(ticket);
                app.toast = Some(("Copied View-Only link to clipboard.".into(), now_ms()));
            }
            if let Some(ticket) = app.sync.edit_ticket.clone()
                && ui.small_button("Copy Edit Link").on_hover_text("Copy read/write invite link").clicked()
            {
                ui.ctx().copy_text(ticket);
                app.toast = Some(("Copied Edit link to clipboard.".into(), now_ms()));
            }
            ui.colored_label(egui::Color32::from_rgb(40, 180, 99), format!("🟢 Live ({peers} Peers)"));
        }
        ShareStatus::JoinedCanEdit => {
            if ui.small_button("Leave").on_hover_text("Disconnect from the shared session").clicked() {
                app.stop_sharing();
            }
            ui.colored_label(egui::Color32::from_rgb(40, 180, 99), format!("🟢 Live ({peers} Peers)"));
        }
        ShareStatus::JoinedViewOnly => {
            if ui.small_button("Leave").on_hover_text("Disconnect from the shared session").clicked() {
                app.stop_sharing();
            }
            ui.colored_label(egui::Color32::from_rgb(230, 145, 56), "🔒 View Only");
        }
    }
}

/// Renders the collaboration controls inside the title bar's `Share` popup menu.
pub fn share_popup_menu(app: &mut SheetApp, ui: &mut egui::Ui) {
    ui.set_min_width(240.0);
    if let Some(label) = app.sync.pending_label {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0));
            ui.label(label);
        });
        ui.separator();
        return;
    }
    let peers = app.sync.connected_peers();
    match app.sync.status() {
        ShareStatus::Offline => {
            if ui.button("Share Document").on_hover_text("Start a live end-to-end encrypted P2P session").clicked() {
                app.start_sharing();
                ui.close();
            }
            ui.add_space(4.0);
            ui.label("Join via Link:");
            let resp = ui.add(egui::TextEdit::singleline(&mut app.sync.join_input).hint_text("Paste gridcraft:// link…").desired_width(220.0));
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Join").clicked() || enter {
                let ticket = app.sync.join_input.clone();
                app.start_joining(ticket);
                ui.close();
            }
        }
        ShareStatus::Hosting => {
            ui.colored_label(egui::Color32::from_rgb(40, 180, 99), format!("🟢 Live ({peers} Peers)"));
            if let Some(ticket) = app.sync.edit_ticket.clone()
                && ui.button("Copy Edit Link").clicked()
            {
                ui.ctx().copy_text(ticket);
                app.toast = Some(("Copied Edit link to clipboard.".into(), now_ms()));
                ui.close();
            }
            if let Some(ticket) = app.sync.view_ticket.clone()
                && ui.button("Copy View-Only Link").clicked()
            {
                ui.ctx().copy_text(ticket);
                app.toast = Some(("Copied View-Only link to clipboard.".into(), now_ms()));
                ui.close();
            }
            if ui.button("Stop Sharing").clicked() {
                app.stop_sharing();
                ui.close();
            }
        }
        ShareStatus::JoinedCanEdit => {
            ui.colored_label(egui::Color32::from_rgb(40, 180, 99), format!("🟢 Live ({peers} Peers)"));
            if ui.button("Leave Session").clicked() {
                app.stop_sharing();
                ui.close();
            }
        }
        ShareStatus::JoinedViewOnly => {
            ui.colored_label(egui::Color32::from_rgb(230, 145, 56), "🔒 View Only");
            if ui.button("Leave Session").clicked() {
                app.stop_sharing();
                ui.close();
            }
        }
    }
    if let Some(err) = &app.sync.last_error {
        ui.colored_label(egui::Color32::from_rgb(210, 60, 60), format!("⚠ {err}"));
    }
    ui.separator();
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::time::{Duration, Instant};

    use gridcraft_engine::Session;
    use gridcraft_engine::core::{CellRef, RangeRef};
    use serde_json::json;
    use simple_sync::SyncConfig;

    use super::{
        CommandSyncPolicy, NOTICE_LOCAL_FILE, NOTICE_MACROS, NOTICE_PROTECTION, NOTICE_UNDO_REDO, NOTICE_VIEW_ONLY, ShareStatus, SharedMutation,
        SyncState, classify_command,
    };
    use crate::{Services, SheetApp};

    fn test_app(name: &str) -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        app.sync = SyncState::with_config(name, SyncConfig::local_only());
        app
    }

    #[allow(clippy::disallowed_methods)]
    fn spin_until(mut apps: Vec<&mut SheetApp>, mut pred: impl FnMut(&[&mut SheetApp]) -> bool) {
        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            for app in &mut apps {
                app.poll_sync(&ctx);
            }
            if pred(&apps) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(pred(&apps), "timed out waiting for sync condition");
    }

    #[test]
    fn shared_mutation_wire_round_trip_all_categories() {
        let samples = vec![
            SharedMutation {
                sheet_name: "Sheet1".into(),
                selection: gridcraft_engine::Selection::at(CellRef::new(0, 0)),
                selected_object: None,
                id: "cell.set".into(),
                params_json: json!({"cell": "A1", "input": "=SUM(B1:B5)", "array": false}).to_string(),
            },
            SharedMutation {
                sheet_name: "Revenue".into(),
                selection: gridcraft_engine::Selection::range(RangeRef::new(CellRef::new(1, 1), CellRef::new(4, 3))),
                selected_object: None,
                id: "home.bold".into(),
                params_json: json!({"on": true}).to_string(),
            },
            SharedMutation {
                sheet_name: "Sheet1".into(),
                selection: gridcraft_engine::Selection::at(CellRef::new(2, 2)),
                selected_object: Some(("shape".into(), 7)),
                id: "shape.setText".into(),
                params_json: json!({"id": 7, "text": "Hello P2P"}).to_string(),
            },
            SharedMutation {
                sheet_name: "Sheet1".into(),
                selection: gridcraft_engine::Selection::at(CellRef::new(0, 0)),
                selected_object: Some(("chart".into(), 1)),
                id: "insert.table".into(),
                params_json: json!({"range": "A1:C4", "headers": true}).to_string(),
            },
        ];

        for mutation in samples {
            let wire = mutation.to_wire().expect("serialize should succeed");
            let decoded = SharedMutation::from_wire(&wire).expect("deserialize should succeed");
            assert_eq!(decoded, mutation);
        }

        // Verify MAX_MUTATION_BYTES enforcement.
        let oversized = SharedMutation {
            sheet_name: "Sheet1".into(),
            selection: gridcraft_engine::Selection::default(),
            selected_object: None,
            id: "cell.set".into(),
            params_json: "x".repeat(simple_sync::MAX_MUTATION_BYTES + 1024),
        };
        assert!(oversized.to_wire().is_err());
    }

    #[test]
    fn command_catalog_classification_covers_all_specs() {
        for spec in gridcraft_engine::command_specs() {
            let policy = classify_command(spec.id, &json!({}));
            match spec.id {
                "edit.undo" | "edit.redo" => assert_eq!(policy, CommandSyncPolicy::UnsharedNotice(NOTICE_UNDO_REDO)),
                "review.protectSheet" | "review.unprotectSheet" | "review.protectWorkbook" | "review.allowEditRanges" => {
                    assert_eq!(policy, CommandSyncPolicy::UnsharedNotice(NOTICE_PROTECTION));
                }
                id if id.starts_with("automate.") || id.starts_with("macro.") => {
                    assert_eq!(policy, CommandSyncPolicy::UnsharedNotice(NOTICE_MACROS));
                }
                _ => {}
            }
        }
        assert_eq!(classify_command("insert.picture", &json!({"path": "/tmp/local.png"})), CommandSyncPolicy::UnsharedNotice(NOTICE_LOCAL_FILE));
        assert_eq!(classify_command("data.fromTextCsv", &json!({"path": "/tmp/local.csv"})), CommandSyncPolicy::UnsharedNotice(NOTICE_LOCAL_FILE));
        assert_eq!(classify_command("data.fromTextCsv", &json!({"text": "a,b\n1,2"})), CommandSyncPolicy::Shared);
    }

    #[test]
    fn host_edit_joiner_and_view_only_and_kill_switch() {
        let mut host = test_app("Host");
        let mut editor = test_app("Editor");
        let mut viewer = test_app("Viewer");

        // Initial content on Host before sharing.
        host.run("cell.set", json!({"cell": "A1", "input": "Initial"})).expect("set A1");
        host.run("cell.set", json!({"cell": "B1", "input": "10"})).expect("set B1");

        // Host enables Share Mode.
        host.start_sharing();
        spin_until(vec![&mut host], |apps| apps[0].sync.status() == ShareStatus::Hosting && apps[0].sync.edit_ticket.is_some());

        let edit_link = host.sync.edit_ticket.clone().expect("edit ticket");
        let view_link = host.sync.view_ticket.clone().expect("view ticket");
        assert_ne!(edit_link, view_link);

        // Editor joins with edit link; Viewer joins with view-only link.
        editor.start_joining(edit_link);
        viewer.start_joining(view_link);
        spin_until(vec![&mut host, &mut editor, &mut viewer], |apps| {
            let e_val = apps[1].session.active().and_then(|d| d.wb.active()).map(|s| s.value(CellRef::new(0, 0)));
            let v_val = apps[2].session.active().and_then(|d| d.wb.active()).map(|s| s.value(CellRef::new(0, 0)));
            apps[1].sync.status() == ShareStatus::JoinedCanEdit
                && apps[2].sync.status() == ShareStatus::JoinedViewOnly
                && e_val == Some(gridcraft_engine::core::Value::Text("Initial".into()))
                && v_val == Some(gridcraft_engine::core::Value::Text("Initial".into()))
        });

        assert_eq!(host.session.active().expect("host doc").wb, editor.session.active().expect("editor doc").wb);
        assert_eq!(host.session.active().expect("host doc").wb, viewer.session.active().expect("viewer doc").wb);

        // Host makes an edit; it arrives on both Editor and Viewer.
        host.run("cell.set", json!({"cell": "B2", "input": "32"})).expect("host set B2");
        host.run("cell.set", json!({"cell": "B3", "input": "=SUM(B1:B2)"})).expect("host set B3");
        spin_until(vec![&mut host, &mut editor, &mut viewer], |apps| {
            apps[1].session.active().expect("editor").wb == apps[0].session.active().expect("host").wb
                && apps[2].session.active().expect("viewer").wb == apps[0].session.active().expect("host").wb
        });

        // Editor makes an edit; it reaches Host and Viewer.
        editor.run("cell.set", json!({"cell": "C1", "input": "FromEditor"})).expect("editor set C1");
        spin_until(vec![&mut host, &mut editor, &mut viewer], |apps| {
            apps[0].session.active().expect("host").wb == apps[1].session.active().expect("editor").wb
                && apps[2].session.active().expect("viewer").wb == apps[1].session.active().expect("editor").wb
        });

        // View-Only joiner attempts local edits -> refused and document stays unchanged.
        assert!(!viewer.local_editing_allowed());
        let viewer_wb_before = viewer.session.active().expect("viewer").wb.clone();
        let err = viewer.run("cell.set", json!({"cell": "A1", "input": "Hacked"})).expect_err("view-only edit must fail");
        assert!(err.contains(NOTICE_VIEW_ONLY));
        assert_eq!(viewer.toast.as_ref().map(|(m, _)| m.as_str()), Some(NOTICE_VIEW_ONLY));
        viewer.begin_edit(Some("Hacked".into()), false);
        assert!(viewer.editor.is_none());
        assert_eq!(viewer.session.active().expect("viewer").wb, viewer_wb_before);

        // Kill switch on Host returns Host to Offline.
        host.stop_sharing();
        spin_until(vec![&mut host], |apps| apps[0].sync.status() == ShareStatus::Offline && !apps[0].sync.is_busy());
        assert!(host.sync.edit_ticket.is_none());
        assert!(host.sync.view_ticket.is_none());

        editor.stop_sharing();
        viewer.stop_sharing();
        spin_until(vec![&mut editor, &mut viewer], |apps| {
            apps[0].sync.status() == ShareStatus::Offline && apps[1].sync.status() == ShareStatus::Offline
        });
    }

    #[test]
    fn late_joiner_and_command_categories_converge() {
        let mut host = test_app("HostLate");
        host.start_sharing();
        spin_until(vec![&mut host], |apps| apps[0].sync.status() == ShareStatus::Hosting && apps[0].sync.edit_ticket.is_some());
        let edit_link = host.sync.edit_ticket.clone().expect("edit ticket");

        // Host performs a variety of command categories BEFORE the late joiner connects:
        // 1. Cells & formulas
        host.run("cell.set", json!({"cell": "A1", "input": "Item"})).expect("A1");
        host.run("cell.set", json!({"cell": "B1", "input": "Amount"})).expect("B1");
        host.run("cell.set", json!({"cell": "A2", "input": "Beta"})).expect("A2");
        host.run("cell.set", json!({"cell": "B2", "input": "20"})).expect("B2");
        host.run("cell.set", json!({"cell": "A3", "input": "Alpha"})).expect("A3");
        host.run("cell.set", json!({"cell": "B3", "input": "10"})).expect("B3");
        // 2. Formatting (toggle + explicit)
        host.run("selection.set", json!({"range": "A1:B1"})).expect("select A1:B1");
        host.run("home.bold", json!({})).expect("bold toggle");
        host.run("home.fillColor", json!({"color": "#D9E1F2"})).expect("fill color");
        // 3. Column width / Row height
        host.run("home.columnWidth", json!({"cols": "A:B", "width": 96})).expect("col width");
        host.run("home.rowHeight", json!({"rows": "1:1", "height": 28})).expect("row height");
        // 4. Data sort
        host.run("selection.set", json!({"range": "A2:B3"})).expect("select A2:B3");
        host.run("data.sortAscending", json!({})).expect("sort ascending");
        // 5. Copy + Paste
        host.run("selection.set", json!({"range": "A1:B3"})).expect("select A1:B3");
        host.run("edit.copy", json!({})).expect("copy");
        host.run("edit.paste", json!({"at": "D1"})).expect("paste at D1");
        // 6. Sheet insert & rename
        host.run("home.insertSheet", json!({})).expect("insert sheet");
        host.run("sheet.rename", json!({"name": "Summary"})).expect("rename sheet");
        host.run("cell.set", json!({"cell": "A1", "input": "=SUM(Sheet1!B2:B3)"})).expect("cross-sheet formula");

        // Now a late joiner connects and must converge to the exact same workbook!
        let mut late_joiner = test_app("LateJoiner");
        late_joiner.start_joining(edit_link);
        spin_until(vec![&mut host, &mut late_joiner], |apps| {
            apps[1].sync.status() == ShareStatus::JoinedCanEdit
                && apps[1].session.active().expect("late").wb == apps[0].session.active().expect("host").wb
        });

        assert_eq!(late_joiner.session.active().expect("late").wb, host.session.active().expect("host").wb);

        host.stop_sharing();
        late_joiner.stop_sharing();
        spin_until(vec![&mut host, &mut late_joiner], |apps| {
            apps[0].sync.status() == ShareStatus::Offline && apps[1].sync.status() == ShareStatus::Offline
        });
    }

    #[test]
    fn multi_document_isolation_and_unshared_notices() {
        let mut host = test_app("HostMulti");
        let mut peer = test_app("PeerMulti");

        host.run("cell.set", json!({"cell": "A1", "input": "SharedDoc"})).expect("set shared A1");
        let shared_host_uid = host.session.active().expect("host active").uid;

        host.start_sharing();
        spin_until(vec![&mut host], |apps| apps[0].sync.status() == ShareStatus::Hosting && apps[0].sync.edit_ticket.is_some());
        let edit_link = host.sync.edit_ticket.clone().expect("edit ticket");

        peer.start_joining(edit_link);
        spin_until(vec![&mut host, &mut peer], |apps| {
            apps[1].sync.status() == ShareStatus::JoinedCanEdit
                && apps[1].session.active().expect("peer").wb == apps[0].session.active().expect("host").wb
        });
        let shared_peer_uid = peer.sync.shared_doc_uid.expect("peer shared uid");

        // Open a second, unshared workbook on both Host and Peer.
        host.run("file.new", json!({})).expect("host second workbook");
        let unshared_host_uid = host.session.active().expect("host second").uid;
        assert_ne!(shared_host_uid, unshared_host_uid);

        peer.run("file.new", json!({})).expect("peer second workbook");
        let unshared_peer_uid = peer.session.active().expect("peer second").uid;
        assert_ne!(shared_peer_uid, unshared_peer_uid);

        // Host edits its unshared workbook -> must NOT be sent to Peer's shared or unshared workbook.
        host.run("cell.set", json!({"cell": "A1", "input": "PrivateHostNote"})).expect("edit unshared host");

        // Switch Host back to shared workbook and edit B1 while Peer is still viewing its unshared workbook.
        let host_shared_idx = host.session.document_index(shared_host_uid).expect("host shared idx");
        host.session.set_active(host_shared_idx);
        host.run("cell.set", json!({"cell": "B1", "input": "RemoteWhilePeerBusy"})).expect("edit shared B1");

        spin_until(vec![&mut host, &mut peer], |apps| {
            let peer_shared_idx = apps[1].session.document_index(shared_peer_uid).expect("idx");
            let peer_shared_wb = &apps[1].session.documents()[peer_shared_idx].wb;
            let host_shared_idx = apps[0].session.document_index(shared_host_uid).expect("idx");
            let host_shared_wb = &apps[0].session.documents()[host_shared_idx].wb;
            peer_shared_wb == host_shared_wb
        });

        // Peer's active workbook is STILL its unshared second workbook, and its A1 is still empty!
        assert_eq!(peer.session.active().expect("peer active").uid, unshared_peer_uid);
        assert_eq!(
            peer.session.active().expect("peer active").wb.active().expect("sheet").value(CellRef::new(0, 0)),
            gridcraft_engine::core::Value::Empty
        );

        // Unshared command on the shared workbook surfaces a clear notice/toast and is not silently dropped.
        host.run("review.protectSheet", json!({})).expect("protect sheet locally");
        assert_eq!(host.notice.as_ref().map(|n| n.text.as_str()), Some(NOTICE_PROTECTION));

        // Undo on the shared workbook during live collaboration is refused with NOTICE_UNDO_REDO so peers don't desync.
        let undo_err = host.run("edit.undo", json!({})).expect_err("undo during share should be refused");
        assert!(undo_err.contains(NOTICE_UNDO_REDO));

        host.stop_sharing();
        peer.stop_sharing();
        spin_until(vec![&mut host, &mut peer], |apps| apps[0].sync.status() == ShareStatus::Offline && apps[1].sync.status() == ShareStatus::Offline);
    }
}
