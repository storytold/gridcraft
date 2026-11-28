use super::{Json, Notice, NoticeKind, OpenWindow, SheetApp, clean_error, json};

impl SheetApp {
    pub fn open_path(&mut self, path: &str) {
        match self.run("file.open", json!({"path": path})) {
            Ok(r) => self.after_open(Some(path), &r),
            // A password-protected workbook: ask for the password; the dialog opens it.
            Err(e) if e.contains(gridcraft_engine::io::PASSWORD_REQUIRED) => self.open_dialog("openPassword", json!({"path": path})),
            Err(e) if e.contains(gridcraft_engine::io::MODIFY_PASSWORD_REQUIRED) => {
                self.open_dialog("openPassword", json!({"path": path, "modify": true}))
            }
            Err(e) => self.message = Some(("GridCraft".into(), clean_error(&e))),
        }
        self.after_engine();
    }

    /// What follows a successful `file.open`: the workbook shows (not the Backstage), the Recent
    /// list (for a file on disk) and the open warnings notice. Shared by File › Open, the
    /// password dialog and the control channel.
    pub fn after_open(&mut self, path: Option<&str>, r: &Json) {
        self.backstage = None;
        if let Some(path) = path {
            self.ui.recent.retain(|p| p != path);
            self.ui.recent.insert(0, path.to_string());
            self.ui.recent.truncate(20);
        }
        self.show_open_warnings(r);
    }

    /// Turns the `warnings` a `file.open` result carries into an info-bar notice, choosing the
    /// most important one (a preserved VBA project outranks, say, a skipped optional part).
    /// Kept deliberately generic: any open-time warning can reach the user this way.
    pub fn show_open_warnings(&mut self, r: &Json) {
        // The opened workbook's window shows them, however it was opened (a script may have run
        // `file.open` on the session directly).
        self.follow_active_window();
        let Some(list) = r.get("warnings").and_then(Json::as_array) else {
            self.notice = None;
            return;
        };
        let msgs: Vec<&str> = list.iter().filter_map(Json::as_str).collect();
        if msgs.is_empty() {
            // A clean open clears any notice left over from the previous document.
            self.notice = None;
            return;
        }
        let vba: Vec<&str> = msgs.iter().copied().filter(|m| m.to_lowercase().contains("macro") || m.contains("VBA")).collect();
        if !vba.is_empty() {
            // The reader says "dropped" when the VBA project was missing or unreadable; never promise to keep it then.
            let dropped = vba.iter().any(|m| m.contains("dropped"));
            let text = if dropped {
                "This workbook's VBA macros could not be loaded and were dropped, so they will not be in the saved file. Keep the original file."
            } else {
                "This workbook contains VBA macros. GridCraft keeps them when you save as .xlsm, but does not run them."
            };
            self.notice = Some(Notice { text: text.into(), kind: NoticeKind::Warning, action: Some(("What this means".into(), "macros".into())) });
        } else {
            let text = if msgs.len() == 1 {
                msgs[0].to_string()
            } else {
                format!("{} items in this file were handled differently. First: {}", msgs.len(), msgs[0])
            };
            self.notice = Some(Notice { text, kind: NoticeKind::Info, action: None });
        }
    }

    /// The open windows in order, for the Switch Windows lists.
    pub fn open_windows(&self) -> Vec<OpenWindow> {
        let active = self.session.active_window();
        self.session
            .windows()
            .iter()
            .filter_map(|w| {
                let d = self.session.documents().iter().find(|d| d.uid == w.doc_uid)?;
                let title = self.session.window_title(w.id)?;
                Some(OpenWindow { window: w.id, doc: d.uid, title, dirty: d.is_dirty(), active: active == Some(w.id) })
            })
            .collect()
    }

    /// Opens files that arrived asynchronously (web file picker, dropped files).
    pub(super) fn poll_async_opens(&mut self) {
        let arrived: Vec<(String, Vec<u8>)> = self
            .services
            .inbox
            .as_ref()
            .map(|i| std::mem::take(&mut *i.lock().unwrap_or_else(std::sync::PoisonError::into_inner)))
            .unwrap_or_default();
        for (name, bytes) in arrived {
            let b64 = gridcraft_engine::io::base64_encode(&bytes);
            match self.run("file.open", json!({"name": &name, "base64": &b64})) {
                // A file picked from File › Open (web) arrives here: show it, not the Backstage.
                Ok(r) => {
                    self.backstage = None;
                    self.show_open_warnings(&r);
                }
                Err(e) if e.contains(gridcraft_engine::io::PASSWORD_REQUIRED) => {
                    self.backstage = None;
                    self.open_dialog("openPassword", json!({"name": name, "base64": b64}));
                }
                Err(e) if e.contains(gridcraft_engine::io::MODIFY_PASSWORD_REQUIRED) => {
                    self.backstage = None;
                    self.open_dialog("openPassword", json!({"name": name, "base64": b64, "modify": true}));
                }
                Err(e) => self.message = Some(("GridCraft".into(), clean_error(&e))),
            }
            self.after_engine();
        }
    }

    /// AutoSave: save workbooks that have an .xlsx file in batches of changes rather than after
    /// each one. A batch is saved once [`AUTOSAVE_EDITS`] changes have piled up since the last
    /// save, or [`AUTOSAVE_DELAY_MS`] after its first change, whichever comes first, so a burst of
    /// edits is saved soon and a single edit is not left unsaved for long. This decision lives in
    /// `logic` so a hidden or logic-only frame still saves; the batch is forgotten once saved, so
    /// a forced hidden UI pass does not save the same change twice.
    pub(super) fn autosave(&mut self, ctx: &egui::Context) {
        let active = self.session.active().filter(|d| d.is_dirty() && d.path.as_deref().is_some_and(is_xlsx)).map(|d| (d.uid, d.revision));
        let Some((uid, revision)) = active.filter(|_| self.ui.autosave) else {
            self.grid.autosave_batch = None;
            return;
        };
        let now = crate::monotonic_ms();
        // A batch starts at the first unsaved change seen: that change is already in `revision`.
        let batch = match self.grid.autosave_batch {
            Some(b) if b.doc == uid => b,
            _ => crate::grid::AutosaveBatch { doc: uid, base_revision: revision.saturating_sub(1), since: now },
        };
        self.grid.autosave_batch = Some(batch);
        let changes = revision.saturating_sub(batch.base_revision);
        let waited = now - batch.since;
        let due = changes >= AUTOSAVE_EDITS || waited >= AUTOSAVE_DELAY_MS;
        // A cell or text box still being edited is saved with the next batch, once it is committed.
        if due && self.editor.is_none() && self.text_box_editor.is_none() {
            self.grid.last_autosave = now;
            self.grid.autosave_batch = None;
            self.store_view_state();
            if let Err(e) = self.session.run("file.save", json!({})) {
                // Say so instead of retrying silently: the switch would look on while nothing saves.
                self.ui.autosave = false;
                self.message =
                    Some(("AutoSave".into(), format!("AutoSave was turned off because the workbook could not be saved: {}", clean_error(&e))));
            }
        } else {
            // Keep checking even when no input arrives while the window is hidden.
            let left = (AUTOSAVE_DELAY_MS - waited).max(AUTOSAVE_RECHECK_MS);
            ctx.request_repaint_after(std::time::Duration::from_millis(left as u64));
        }
    }

    /// Flip AutoSave (title-bar switch / Options ▸ Save). Persisted with the other UI
    /// preferences, so it stays as the user left it across restarts (#1901).
    ///
    /// AutoSave saves only a workbook that already has an .xlsx file. Turning it on for one that
    /// has none (a new workbook, or a CSV or other format) first asks where to save it as .xlsx;
    /// AutoSave turns on only once that save succeeded, and stays off when it is cancelled, so the
    /// switch never shows on for a workbook it cannot save. The switch shows
    /// [`Self::autosave_active`], so with the preference on, a workbook without a file shows it
    /// off, and clicking it asks where to save that workbook instead of turning AutoSave off.
    pub fn toggle_autosave(&mut self) {
        if self.autosave_active() {
            self.ui.autosave = false;
            return;
        }
        if !self.active_has_xlsx_file() {
            if self.services.pick_save.is_none() {
                // The web build saves by downloading a copy: there is no file to keep saving to.
                self.message = Some(("AutoSave".into(), "AutoSave needs a workbook saved to a file on this computer.".into()));
                return;
            }
            self.open_dialog("saveAs", json!({}));
            if !self.active_has_xlsx_file() {
                return;
            }
        }
        self.ui.autosave = true;
    }

    /// What the title bar shows next to the AutoSave switch while it is on: "Saving…" while
    /// changes wait for their batch, "Saved" once the file has them all, and "Not saved to a
    /// file" for a workbook AutoSave cannot write to (one opened or created after AutoSave was
    /// turned on), so the switch never reads as saving work it isn't. `None` while it is off.
    pub fn autosave_status(&self) -> Option<&'static str> {
        if !self.ui.autosave {
            return None;
        }
        let d = self.session.active()?;
        Some(if !d.path.as_deref().is_some_and(is_xlsx) {
            "Not saved to a file"
        } else if d.is_dirty() {
            "Saving…"
        } else {
            "Saved"
        })
    }

    /// Whether AutoSave is saving the active workbook: the preference is on and the workbook has
    /// an .xlsx file to save to. What the AutoSave switch and the Options checkbox show.
    pub fn autosave_active(&self) -> bool {
        self.ui.autosave && self.active_has_xlsx_file()
    }

    /// Closing or quitting with AutoSave on: saves the active workbook to its .xlsx file instead
    /// of asking about its changes, as AutoSave would have a moment later. Returns whether the
    /// workbook no longer has unsaved changes; when the save fails, the caller asks as before.
    pub(crate) fn autosave_before_close(&mut self) -> bool {
        if !self.autosave_active() {
            return false;
        }
        self.store_view_state();
        let saved = self.session.run("file.save", json!({})).is_ok();
        self.grid.autosave_batch = None;
        saved && self.session.active().is_some_and(|d| !d.is_dirty())
    }

    /// Whether the active workbook has an .xlsx file AutoSave can write to.
    fn active_has_xlsx_file(&self) -> bool {
        self.session.active().is_some_and(|d| d.path.as_deref().is_some_and(is_xlsx))
    }
}

/// Changes since the last save after which AutoSave saves without waiting for the delay.
pub(crate) const AUTOSAVE_EDITS: u64 = 10;
/// How long AutoSave lets the first unsaved change wait before it saves.
pub(crate) const AUTOSAVE_DELAY_MS: f64 = 30_000.0;
/// The shortest wait between AutoSave checks while a batch is pending.
const AUTOSAVE_RECHECK_MS: f64 = 1_000.0;

fn is_xlsx(path: &str) -> bool {
    path.ends_with(".xlsx")
}
