//! A Save that finds the workbook's own file changed on disk since it was opened here (another
//! program replaced it since): the engine writes nothing and refuses
//! ([`gridcraft_engine::io::CHANGED_ON_DISK`]). A Save the person asked for (the keys, a menu, the
//! Save Changes question) puts the question to them: Save a Copy (the main button), Overwrite, or
//! Cancel. A script gets the engine's answer. AutoSave never replaces such a file: it stops for that
//! workbook, without asking, and the information bar says why.

use super::{ChangedAnswer, Json, Notice, NoticeKind, SheetApp, json};

/// The information bar's line for a workbook AutoSave stopped for.
pub(crate) const AUTOSAVE_STOPPED: &str =
    "AutoSave is off for this workbook: its file was changed on disk after it was opened here. Press Save to choose what to do.";

/// Whether `e`, the error of a Save, is the engine's refusal to replace a file changed on disk.
pub(crate) fn is_changed_on_disk(e: &str) -> bool {
    e.contains(gridcraft_engine::io::CHANGED_ON_DISK)
}

/// The name proposed for a copy of the workbook whose own file is `path`: its name marked as a
/// copy (`Budget (copy).xlsx`, then `Budget (copy 2).xlsx` and so on), the first such name that no
/// file in that folder has yet. `None` for a path with no file name.
pub(crate) fn copy_name(path: &str, lang: crate::i18n::Language) -> Option<String> {
    let path = std::path::Path::new(path);
    let stem = path.file_stem()?.to_str()?;
    let ext = path.extension().and_then(std::ffi::OsStr::to_str);
    let name = |n: u32| {
        let marked = match n {
            1 => lang.tr("{name} (copy)").replace("{name}", stem),
            n => lang.tr("{name} (copy {n})").replace("{name}", stem).replace("{n}", &n.to_string()),
        };
        ext.map_or_else(|| marked.clone(), |ext| format!("{marked}.{ext}"))
    };
    // A hundred names are tried; past them the last is proposed, and the file chooser asks before
    // it replaces a file.
    let free = (1..100).map(name).find(|candidate| !path.with_file_name(candidate).exists());
    Some(free.unwrap_or_else(|| name(100)))
}

impl SheetApp {
    /// Asks about the active workbook, whose Save with `params` was refused because its file was
    /// changed on disk. `then` is what follows a save that goes through: `"close"` the workbook, or
    /// `"quit"` (close it and go on with the quit).
    pub(crate) fn ask_changed_on_disk(&mut self, params: Json, then: Option<&str>) {
        let name = self.session.active().map(|d| d.display_title()).unwrap_or_default();
        self.open_dialog("fileChanged", json!({"name": name, "params": params, "then": then}));
    }

    /// Answers the question about the active workbook's file changed on disk: `values` are the
    /// question's (the Save's parameters, and what follows a save that goes through). Save a Copy
    /// opens the Save As window on the workbook's name marked as a copy; Overwrite saves over the
    /// newer file; Cancel writes nothing and stops a close or a quit.
    pub fn answer_changed_on_disk(&mut self, answer: ChangedAnswer, values: &serde_json::Map<String, Json>, ctx: &egui::Context) {
        let mut params = values.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({}));
        match answer {
            ChangedAnswer::Cancel => return,
            ChangedAnswer::Overwrite => {
                params["overwrite"] = json!(true);
                self.run_or_alert("file.save", params);
            }
            ChangedAnswer::SaveCopy => {
                let lang = self.ui.language;
                let Some(copy) = self.session.active().and_then(|d| d.path.clone()).and_then(|p| copy_name(&p, lang)) else { return };
                self.open_dialog("saveAs", json!({"suggested": copy}));
            }
        }
        // A close or a quit goes on only once the workbook was saved.
        let then = values.get("then").and_then(Json::as_str);
        if matches!(then, Some("close" | "quit")) && self.session.active().is_some_and(|d| !d.is_dirty()) {
            self.run_or_alert("file.close", json!({"force": true, "document": true}));
            if then == Some("quit") {
                self.continue_quit(ctx);
            }
        }
    }

    /// AutoSave met the active workbook's file changed on disk: it stops for that workbook (it asks
    /// nothing and tries no more), and the information bar of each of its windows says why.
    pub(super) fn stop_autosave(&mut self) {
        let Some(d) = self.session.active() else { return };
        let hold = crate::AutosaveHold { doc: d.uid, disk: d.disk.clone(), saved: d.saved.clone() };
        let doc = hold.doc;
        let open: Vec<u64> = self.session.documents().iter().map(|d| d.uid).collect();
        self.autosave_held.retain(|h| open.contains(&h.doc) && h.doc != doc);
        self.autosave_held.push(hold);
        let notice = self.autosave_stopped_notice();
        self.notices_of(doc, |n| *n = Some(notice.clone()));
    }

    /// The information bar's line for a workbook AutoSave stopped for.
    fn autosave_stopped_notice(&self) -> Notice {
        Notice { text: self.ui.language.tr(AUTOSAVE_STOPPED).to_string(), kind: NoticeKind::Warning, action: None }
    }

    /// A window shown for the first time: when AutoSave stopped for its workbook, its information
    /// bar says so, as the workbook's other windows do.
    pub(crate) fn note_autosave_held(&mut self) {
        if self.session.active().is_some_and(|d| self.autosave_held.iter().any(|h| h.doc == d.uid)) && self.notice.is_none() {
            self.notice = Some(self.autosave_stopped_notice());
        }
    }

    /// AutoSave goes on for each workbook it stopped for whose own file was written since, whoever
    /// wrote it (Overwrite, a script's or the control channel's save, Save As to a file that is
    /// now its own): the file's stamp or the saved snapshot changed. The line that said it stopped
    /// goes from each of its windows. A workbook that was closed is dropped.
    pub(super) fn resume_autosave(&mut self) {
        let mut resumed = Vec::new();
        self.autosave_held.retain(|h| {
            let Some(d) = self.session.documents().iter().find(|d| d.uid == h.doc) else { return false };
            let written = d.disk != h.disk || !std::sync::Arc::ptr_eq(&d.saved, &h.saved);
            if written {
                resumed.push(h.doc);
            }
            !written
        });
        let text = self.ui.language.tr(AUTOSAVE_STOPPED).to_string();
        for doc in resumed {
            self.notices_of(doc, |n| {
                if n.as_ref().is_some_and(|n| n.text == text) {
                    *n = None;
                }
            });
        }
    }
}
