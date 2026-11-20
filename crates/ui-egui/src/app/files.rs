use super::{Json, Notice, NoticeKind, OpenWindow, SheetApp, clean_error, json};

impl SheetApp {
    pub fn open_path(&mut self, path: &str) {
        match self.run("file.open", json!({"path": path})) {
            Ok(r) => self.after_open(Some(path), &r),
            // A password-protected workbook: ask for the password; the dialog opens it.
            Err(e) if e.contains(gridcraft_engine::io::PASSWORD_REQUIRED) => self.open_dialog("openPassword", json!({"path": path})),
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
                Err(e) => self.message = Some(("GridCraft".into(), clean_error(&e))),
            }
            self.after_engine();
        }
    }

    /// AutoSave: save workbooks that have a file shortly after they change. The frame only takes
    /// a snapshot; a worker thread writes it ([`crate::autosave`]). A change made within the 2 s gate
    /// asks for a frame when the gate opens, so an idle window still saves it.
    pub(super) fn autosave(&mut self, ctx: &egui::Context) {
        const GATE_MS: f64 = 2000.0;
        if let Some(job) = &self.autosave_job {
            match job.poll() {
                Some(result) => {
                    if let Some(job) = self.autosave_job.take() {
                        self.finish_autosave(&job.job, job.started, result);
                    }
                }
                // Still writing: the worker wakes the window when it is done.
                None => return,
            }
        }
        // After the poll: a finished save may have reset the failure count.
        let gate = GATE_MS * f64::from(1u32 << self.autosave_failures.min(5));
        let pending =
            self.ui.autosave && self.session.active().is_some_and(|d| d.is_dirty() && d.path.as_deref().is_some_and(|p| p.ends_with(".xlsx")));
        if !pending {
            return;
        }
        let now = crate::monotonic_ms();
        let wait = self.grid.last_autosave + gate - now;
        if wait >= 0.0 {
            // Not yet: come back when the gate opens, even if nothing else wakes the window.
            ctx.request_repaint_after(std::time::Duration::from_millis(wait.min(gate) as u64 + 1));
            return;
        }
        if self.editor.is_some() || self.text_box_editor.is_some() {
            // Committing the edit runs a frame, which saves then.
            return;
        }
        self.grid.last_autosave = now;
        self.store_view_state();
        let Some(job) = self.session.autosave_job() else { return };
        match crate::autosave::InFlight::start(job.clone(), ctx) {
            Some(in_flight) => self.autosave_job = Some(in_flight),
            // No worker thread (the browser): save here.
            None => {
                let result = crate::autosave::run(&job);
                self.finish_autosave(&job, now, result);
            }
        }
        self.perf.autosave_ms = crate::monotonic_ms() - now;
    }

    /// Records a finished AutoSave. A failed one leaves the workbook dirty, so AutoSave tries
    /// again after the gate, which doubles with every failure in a row.
    fn finish_autosave(&mut self, job: &gridcraft_engine::autosave::SaveJob, started: f64, result: Result<usize, String>) {
        self.perf.autosave_write_ms = crate::monotonic_ms() - started;
        // The 2 s gate runs from the end of the write, so a slow save is never followed at once by the next.
        self.grid.last_autosave = crate::monotonic_ms();
        match result {
            Ok(_) => {
                self.session.finish_save(job);
                self.perf.autosaves = self.perf.autosaves.saturating_add(1);
                self.autosave_failures = 0;
            }
            Err(e) => {
                self.autosave_failures = self.autosave_failures.saturating_add(1);
                log::warn!("AutoSave of {} failed: {e}", job.path);
            }
        }
    }

    /// Waits for an AutoSave that is still being written, so a command that saves, opens or
    /// closes a file never races it.
    pub fn finish_pending_autosave(&mut self) {
        if let Some(in_flight) = self.autosave_job.take() {
            let job = in_flight.job.clone();
            let started = in_flight.started;
            let result = in_flight.wait();
            self.finish_autosave(&job, started, result);
        }
    }

    /// Flip AutoSave (title-bar switch / Options ▸ Save). Persisted with the other UI
    /// preferences, so it stays as the user left it across restarts (#1901).
    pub fn toggle_autosave(&mut self) {
        self.ui.autosave = !self.ui.autosave;
    }
}
