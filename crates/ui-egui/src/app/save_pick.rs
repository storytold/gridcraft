//! Save and export after a file panel.
//!
//! Windows and Linux block in `pick_save` and finish in the same call. macOS must not:
//! `NSSavePanel.runModal` from inside winit's event handler aborts the process (#2790).
//! The macOS host starts a sheet (`pick_save_async`) and the path arrives on a later frame.

use super::{Json, SheetApp, json};

/// Shown when close or quit would drop a workbook whose save sheet is still open.
pub(crate) const SAVE_SHEET_BUSY: &str = "Finish the save dialog that's already open.";

/// What to do with the path the save panel returns.
pub(crate) enum SaveFollow {
    /// `file.save`. A path with no extension is written as `.xlsx`.
    SaveAs { doc: u64, then: SaveThen },
    /// `file.exportCsv`, `file.exportHtml`, or another export. The extension is forced.
    Export { doc: u64, command: String, ext: String },
    /// `file.exportPdf`, keeping the caller's other parameters.
    ExportPdf { doc: u64, params: Json },
}

/// What happens after a Save As sheet. Exports never carry this: a CSV, HTML, or PDF leaves
/// the workbook dirty, so a close or quit waiting on a real save must not run.
#[derive(Clone, Copy)]
pub(crate) enum SaveThen {
    Stay,
    /// Save from the close or quit prompt. Close the workbook once that save leaves it clean.
    Close {
        quit: bool,
    },
}

impl SaveFollow {
    pub(crate) fn save_as(doc: u64) -> Self {
        Self::SaveAs { doc, then: SaveThen::Stay }
    }
}

/// `kit-prod.csv` → `kit-prod`. An empty stem becomes `Book1`.
pub(crate) fn file_stem(title: &str) -> &str {
    std::path::Path::new(title).file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).unwrap_or("Book1")
}

impl SheetApp {
    /// Shows the save panel. A blocking picker finishes now and reports whether a file was written.
    /// A sheet stores `follow` and finishes from [`Self::poll_save_sheet`].
    pub(crate) fn start_save(&mut self, suggested: &str, follow: SaveFollow) -> bool {
        if self.services.pick_save_async.is_some() {
            if self.save_follow.is_some() {
                self.message = Some(("GridCraft".into(), SAVE_SHEET_BUSY.into()));
                return false;
            }
            self.save_follow = Some(follow);
            if let Some(start) = &self.services.pick_save_async {
                start(suggested);
            }
            return false;
        }
        let Some(path) = self.services.pick_save.as_ref().and_then(|pick| pick(suggested)) else { return false };
        self.finish_save(follow, path)
    }

    /// Applies a path chosen by the macOS save sheet, and opens files chosen by the open sheet.
    pub(crate) fn poll_save_sheet(&mut self, ctx: &egui::Context) {
        if let Some(queue) = self.services.picked_open.clone() {
            let ready = {
                let mut guard = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                std::mem::take(&mut *guard)
            };
            for path in ready.into_iter().flatten() {
                self.open_path(&path);
            }
        }
        if self.save_follow.is_some() {
            // The sheet's completion also asks for a frame. This covers a wake that landed
            // after this frame had already decided not to run again.
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let Some(queue) = self.services.picked_save.clone() else { return };
        let ready = {
            let mut guard = queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *guard)
        };
        for answer in ready {
            let Some(follow) = self.save_follow.take() else { continue };
            let then = match &follow {
                SaveFollow::SaveAs { doc, then } => Some((*doc, *then)),
                SaveFollow::Export { .. } | SaveFollow::ExportPdf { .. } => None,
            };
            let Some(path) = answer else { continue };
            if !self.finish_save(follow, path) {
                continue;
            }
            // The close prompt's Save started this sheet. Finish that close only when the
            // workbook itself was saved. Cancelling never reaches here.
            if let Some((doc, SaveThen::Close { quit })) = then
                && self.session.documents().iter().any(|d| d.uid == doc && !d.is_dirty())
            {
                self.run_or_alert("file.close", json!({"force": true, "document": true}));
                if quit {
                    self.continue_quit(ctx);
                }
            }
        }
    }

    /// The close or quit prompt's Save opened a sheet for `doc`. Remember to close it once
    /// the sheet writes a clean workbook. A sheet already opened for something else is left
    /// alone.
    pub(crate) fn arm_close_after_save(&mut self, doc: u64, quit: bool) {
        if let Some(SaveFollow::SaveAs { doc: pending, then }) = &mut self.save_follow
            && *pending == doc
        {
            *then = SaveThen::Close { quit };
        }
    }

    /// The workbook a save sheet is waiting on.
    pub(crate) fn pending_save_doc(&self) -> Option<u64> {
        match self.save_follow.as_ref() {
            Some(SaveFollow::SaveAs { doc, .. } | SaveFollow::Export { doc, .. } | SaveFollow::ExportPdf { doc, .. }) => Some(*doc),
            None => None,
        }
    }

    /// Close and quit wait while a save sheet is open. `None` is a quit: it closes every workbook.
    /// A window close passes the workbook it would drop. Returns whether the close was refused.
    pub(crate) fn refuse_close_for_save_sheet(&mut self, doc: Option<u64>) -> bool {
        let blocked = match self.pending_save_doc() {
            None => false,
            Some(_) if doc.is_none() => true,
            Some(pending) => doc == Some(pending),
        };
        if blocked {
            self.message = Some(("GridCraft".into(), SAVE_SHEET_BUSY.into()));
        }
        blocked
    }

    /// Programmatic quit does not ask. Drop the sheet's follow-up so a path that arrives as the
    /// process leaves is not written.
    pub(crate) fn cancel_pending_save(&mut self) {
        self.save_follow = None;
    }

    fn finish_save(&mut self, follow: SaveFollow, path: String) -> bool {
        let doc = match &follow {
            SaveFollow::SaveAs { doc, .. } | SaveFollow::Export { doc, .. } | SaveFollow::ExportPdf { doc, .. } => *doc,
        };
        if !self.activate_saved_doc(doc) {
            return false;
        }
        let result = match follow {
            SaveFollow::SaveAs { .. } => {
                let path = if std::path::Path::new(&path).extension().is_none() { format!("{path}.xlsx") } else { path };
                self.run("file.save", json!({"path": path}))
            }
            SaveFollow::Export { command, ext, .. } => {
                let path = std::path::Path::new(&path).with_extension(&ext).to_string_lossy().into_owned();
                self.run(&command, json!({"path": path}))
            }
            SaveFollow::ExportPdf { mut params, .. } => {
                let path = std::path::Path::new(&path).with_extension("pdf").to_string_lossy().into_owned();
                match &mut params {
                    Json::Object(map) => {
                        map.insert("path".into(), Json::String(path));
                    }
                    other => *other = json!({"path": path}),
                }
                self.run("file.exportPdf", params)
            }
        };
        match result {
            Ok(_) => {
                self.backstage = None;
                true
            }
            Err(e) => {
                self.alert(e);
                false
            }
        }
    }

    fn activate_saved_doc(&mut self, doc: u64) -> bool {
        let Some(window) = self.session.doc_window(doc) else {
            self.message = Some(("GridCraft".into(), "That workbook was closed before the file was chosen.".into()));
            return false;
        };
        if self.session.active_window() != Some(window) {
            self.enter_window(window);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::file_stem;

    #[test]
    fn an_open_csv_saves_as_a_workbook_under_the_same_stem() {
        assert_eq!(file_stem("kit-prod.csv"), "kit-prod");
        assert_eq!(format!("{}.xlsx", file_stem("kit-prod.csv")), "kit-prod.xlsx");
        assert_eq!(file_stem("Book1"), "Book1");
        assert_eq!(file_stem(""), "Book1");
    }
}
