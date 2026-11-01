use super::{
    INTERFACE_SCALE_MAX, INTERFACE_SCALE_MIN, Json, NoticeKind, SheetApp, UiRequest, backstage, clamp_interface_scale, clean_error, dialogs,
    formula_locale, i18n, json, now_ms, printing,
};

impl SheetApp {
    /// Runs an engine command (or a UI command) and handles UI requests it makes.
    pub fn run(&mut self, id: &str, params: Json) -> Result<Json, String> {
        let r = self.run_command(id, params);
        // This copy leaves the system clipboard alone: note what it holds, so Paste can tell
        // that GridCraft's copy is newer while that text is still there.
        if matches!(id, "edit.copy" | "edit.cut") && r.is_ok() {
            self.clipboard_text_at_copy = self.system_clipboard_text();
            self.cut_text = if id == "edit.cut" { self.clipboard_text_at_copy.clone() } else { None };
        }
        r
    }

    pub(super) fn run_command(&mut self, id: &str, params: Json) -> Result<Json, String> {
        let switches_window = id.starts_with("window.")
            || matches!(id, "view.newWindow" | "view.hideWindow" | "view.unhideWindow")
            || (id == "view.switchWindows" && ["window", "doc"].iter().any(|k| params.get(*k).is_some_and(|d| !d.is_null())));
        if id.starts_with("file.") || switches_window {
            self.settle_drafts()?;
        }
        if matches!(id, "file.save" | "file.saveAs" | "file.saveBytes") {
            self.store_view_state();
        }
        if matches!(
            id,
            "sheet.activate"
                | "sheet.next"
                | "sheet.previous"
                | "sheet.move"
                | "sheet.hide"
                | "sheet.unhide"
                | "home.insertSheet"
                | "home.deleteSheet"
                | "edit.undo"
                | "edit.redo"
                | "object.delete"
                | "object.move"
        ) {
            self.commit_text_box_edit()?;
        }
        if let Some(r) = self.run_ui_command(id, &params) {
            return r;
        }
        let before = self.session.active_window();
        let r = self.session.run(id, params);
        if id.starts_with("file.") && self.session.active_window() != before {
            // File › New or Open from the Backstage view: the window it was in goes back to its sheet.
            self.backstage = None;
        }
        // Requests the command made (a dialog, a draft) belong to the window it left active.
        self.follow_active_window();
        self.after_engine();
        if id == "formulas.autoSum"
            && let Some(ed) = self.editor.as_mut()
            && ed.text.starts_with('=')
            && ed.text.ends_with("()")
        {
            ed.caret = ed.text.chars().count().saturating_sub(1);
            ed.anchor = ed.caret;
            ed.sync_caret = true;
        }
        if id == "view.arrangeAll"
            && self.native_windows
            && let Ok(r) = &r
        {
            let arrange = r.get("arrange").and_then(Json::as_str).unwrap_or("tiled").to_string();
            let windows = r.get("windows").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_u64).collect()).unwrap_or_default();
            self.arrange_windows = Some((arrange, windows));
        }
        // The new chart is selected, as after inserting any object, so Delete removes it, not its data.
        if id == "insert.chart"
            && let Ok(r) = &r
            && let Some(chart) = r.get("chart").and_then(Json::as_u64).and_then(|id| u32::try_from(id).ok())
        {
            self.selected_chart = Some(chart);
        }
        if id == "file.exportPdf"
            && let Ok(r) = &r
        {
            self.deliver_pdf(r);
        }
        r
    }

    pub fn run_or_alert(&mut self, id: &str, params: Json) {
        // A menu/shortcut opens the settings first; the Print page's button submits explicitly.
        if id == "file.print" {
            if self.services.print_native.is_some() {
                self.open_backstage(backstage::Page::Print);
                return;
            }
            return self.print_now(params);
        }
        // Menu-style Export as PDF with no file named: ask where to save, like Save As does.
        // Programmatic callers use `run` and keep getting the PDF bytes back.
        if id == "file.exportPdf" && self.services.pick_save.is_some() && params.get("path").and_then(Json::as_str).is_none_or(str::is_empty) {
            return self.export_pdf_with_picker(params);
        }
        // Recommended Charts with no chart type named (ribbon button, keytip, command search) asks
        // which chart to insert; programmatic callers of `run` still get the first recommendation.
        if id == "insert.recommendedCharts" && params.get("type").is_none() {
            return self.open_dialog("recommendedCharts", json!({}));
        }
        // Switch Windows with no window named (key tip, command search) opens the View tab's
        // list to choose from; programmatic callers of `run` get the list back.
        if id == "view.switchWindows" && ["window", "doc"].iter().all(|k| params.get(*k).is_none_or(Json::is_null)) {
            self.ui.ribbon_tab = "View".into();
            self.ui.ribbon_collapsed = false;
            self.open_switch_windows = true;
            return;
        }
        match self.run(id, params) {
            // A refused cell draft already shows its own message (with the rule's title).
            Err(e) if self.message.as_ref().is_some_and(|(_, m)| *m == e) => {}
            Err(e) => self.alert(e),
            Ok(_) => {}
        }
    }

    /// Shows a command error: a toast for an unavailable command, else a message box.
    pub(super) fn alert(&mut self, e: String) {
        // Shown in the window the user ends up in, also after a command that switched windows.
        self.follow_active_window();
        if e.contains("is not available right now") {
            self.toast = Some((e, now_ms()));
        } else {
            self.message = Some(("GridCraft".into(), clean_error(&e)));
        }
    }

    /// UI-only commands (dialogs, view toggles that aren't saved in the file).
    fn run_ui_command(&mut self, id: &str, p: &Json) -> Option<Result<Json, String>> {
        let r = match id {
            "app.checkForUpdates" | "help.checkForUpdates" => {
                self.check_for_updates();
                Ok(Json::Null)
            }
            "ui.printSettings" => printing::settings(&mut self.printing, p, self.services.printers.as_ref()),
            "ui.printSubmit" => self.submit_print(p.clone()),
            "ui.ribbonTab" => {
                self.ui.ribbon_tab = p.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
                Ok(Json::Null)
            }
            "view.collapseRibbon" => {
                self.ui.ribbon_collapsed = !self.ui.ribbon_collapsed;
                Ok(Json::Null)
            }
            "view.expandFormulaBar" => {
                self.ui.formula_bar_expanded = p.get("on").and_then(Json::as_bool).unwrap_or(!self.ui.formula_bar_expanded);
                Ok(json!({"on": self.ui.formula_bar_expanded}))
            }
            "view.darkMode" => {
                let current = if self.ui.system_theme { self.effective_dark } else { self.ui.dark };
                self.ui.dark = p.get("on").and_then(Json::as_bool).unwrap_or(!current);
                self.ui.system_theme = false;
                Ok(json!({"on": self.ui.dark}))
            }
            "view.theme" => {
                match p.get("mode").and_then(Json::as_str) {
                    Some("system") => self.ui.system_theme = true,
                    Some(mode @ ("light" | "dark")) => {
                        self.ui.dark = mode == "dark";
                        self.ui.system_theme = false;
                    }
                    _ => return Some(Err("theme mode must be system, light, or dark".into())),
                }
                Ok(json!({"mode": self.ui.theme_mode()}))
            }
            "view.animations" => {
                self.ui.animations = p.get("on").and_then(Json::as_bool).unwrap_or(!self.ui.animations);
                Ok(json!({"on": self.ui.animations}))
            }
            "options.autoComplete" => {
                self.ui.autocomplete = p.get("on").and_then(Json::as_bool).unwrap_or(!self.ui.autocomplete);
                Ok(json!({"on": self.ui.autocomplete}))
            }
            "view.interfaceScale" => {
                // `{"percent": 125}` or `{"percent": "auto"}`; out-of-range values are clamped.
                let percent = match p.get("percent") {
                    Some(Json::String(s)) if s.eq_ignore_ascii_case("auto") => Some(100),
                    Some(v) => v.as_f64().and_then(clamp_interface_scale),
                    None => None,
                };
                match percent {
                    Some(v) => {
                        self.ui.interface_scale = v;
                        Ok(json!({"percent": v}))
                    }
                    None => Err(format!("missing `percent` ({INTERFACE_SCALE_MIN}–{INTERFACE_SCALE_MAX}, or \"auto\")")),
                }
            }
            "app.language.set" => {
                // `{"language": "ja"}`; `code` is accepted as an alias.
                let code = p.get("language").or_else(|| p.get("code")).and_then(Json::as_str).unwrap_or("");
                match i18n::Language::parse(code) {
                    Some(l) => {
                        self.ui.language = l;
                        Ok(json!({"language": l}))
                    }
                    None => Err(format!("unknown language {code:?} (use one of {})", i18n::Language::code_list(", "))),
                }
            }
            "app.language.english" => {
                self.ui.language = i18n::Language::En;
                Ok(json!({"language": i18n::Language::En}))
            }
            "app.language.japanese" => {
                self.ui.language = i18n::Language::Ja;
                Ok(json!({"language": i18n::Language::Ja}))
            }
            "ui.backstage" => {
                // `{"page": "info"}` opens File › Info; `{"page": "close"}` returns to the sheet.
                match p.get("page").and_then(Json::as_str).map(str::trim) {
                    Some("close" | "none" | "") => self.backstage = None,
                    Some(id) => match backstage::Page::parse(id) {
                        Some(page) => self.open_backstage(page),
                        None => {
                            return Some(Err(format!(
                                "unknown File page {id:?} (use home, new, open, info, saveAs, print, export, options or close)"
                            )));
                        }
                    },
                    None => self.open_backstage(backstage::Page::Home),
                }
                Ok(json!({"page": self.backstage.as_ref().map(|b| b.page.id())}))
            }
            "ui.dialog" => {
                let name = p.get("name").and_then(Json::as_str).unwrap_or("");
                self.open_dialog(name, p.clone());
                Ok(json!({"dialog": self.dialog.as_ref().map(|d| d.name())}))
            }
            "ui.message.dismiss" => {
                self.message = None;
                Ok(Json::Null)
            }
            "ui.notice" => Ok(match &self.notice {
                Some(n) => json!({
                    "text": n.text,
                    "kind": match n.kind { NoticeKind::Warning => "warning", NoticeKind::Info => "info" },
                    "action": n.action,
                }),
                None => Json::Null,
            }),
            "ui.notice.dismiss" => {
                self.notice = None;
                Ok(Json::Null)
            }
            _ => return None,
        };
        Some(r)
    }

    /// Picks up requests commands made (dialogs, edit mode…).
    pub fn after_engine(&mut self) {
        // The requests belong to the window the command left active.
        self.follow_active_window();
        for req in self.session.take_ui_requests() {
            match req {
                UiRequest::Dialog(name, params) => self.open_dialog(&name, params),
                UiRequest::Message(m) => self.message = Some(("GridCraft".into(), m)),
                UiRequest::EditCell(text) => {
                    let text = text.map(|text| {
                        self.session
                            .active()
                            .map(|d| formula_locale::display_template(&text, self.ui.language.formula_locale(), &d.wb, d.wb.active_sheet()))
                            .unwrap_or(text)
                    });
                    self.begin_edit(text, false);
                }
                UiRequest::InsertName(name) => dialogs::insert_name(self, &name),
                UiRequest::OpenUrl(u) => {
                    if let Some(f) = &self.services.open_url {
                        f(&u);
                    }
                }
            }
        }
    }

    /// Whether the active sheet of the active document is protected (drives the Protect/Unprotect
    /// Sheet labels and actions).
    pub fn active_sheet_protected(&self) -> bool {
        self.session.active().and_then(|d| d.wb.active().map(|s| s.is_protected())).unwrap_or(false)
    }

    pub fn open_dialog(&mut self, name: &str, params: Json) {
        if let Err(e) = self.commit_text_box_edit() {
            self.message = Some(("Text Box".into(), clean_error(&e)));
            return;
        }
        // Every Protect Sheet entry point (ribbon menus, keytips, tab menu) becomes Unprotect Sheet
        // on a protected sheet. A sheet without a password is unprotected straight away.
        let name = if name == "protectSheet" && self.active_sheet_protected() { "unprotectSheet" } else { name };
        if name == "unprotectSheet"
            && self
                .session
                .active()
                .and_then(|d| d.wb.active())
                .and_then(|s| s.protection.as_ref())
                .is_some_and(|p| p.password.is_none() && p.legacy_password_hash.is_none())
        {
            self.run_or_alert("review.unprotectSheet", json!({}));
            return;
        }
        match name {
            "open" => {
                if let Some(start) = &self.services.open_async {
                    start();
                } else if let Some(pick) = &self.services.pick_open
                    && let Some(path) = pick()
                {
                    self.open_path(&path);
                }
            }
            "saveAs" => {
                let suggested = self.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
                if let Some(pick) = &self.services.pick_save {
                    if let Some(path) = pick(&suggested) {
                        let path = if std::path::Path::new(&path).extension().is_none() { format!("{path}.xlsx") } else { path };
                        self.run_or_alert("file.save", json!({"path": path}));
                    }
                } else if self.services.download.is_some()
                    && let Ok(r) = self.run("file.saveBytes", json!({"format": "xlsx"}))
                    && let Some(dl) = &self.services.download
                    && let Some(b) = r.get("base64").and_then(Json::as_str).and_then(gridcraft_engine::io::base64_decode)
                {
                    let name = r.get("name").and_then(Json::as_str).unwrap_or("Book.xlsx").to_string();
                    dl(&name, &b);
                }
            }
            // File › Options (`file.options`) and the File tab itself open the Backstage view.
            "options" => self.open_backstage(backstage::Page::Options),
            "backstage" => {
                let page = params.get("page").and_then(Json::as_str).and_then(backstage::Page::parse).unwrap_or(backstage::Page::Home);
                self.open_backstage(page);
            }
            // The macro step editor stays until it closes itself (its draft, overlays and debug
            // run go with it), so another dialog can't replace it.
            _ if self.dialog.as_ref().is_some_and(|d| d.name == "macroEditor") => {
                self.message = Some((
                    "Macro".into(),
                    "Close the macro step editor first: it keeps its window until it closes itself, so its steps and debug run are not lost.".into(),
                ));
            }
            other => self.dialog = dialogs::Dialog::open(self, other, params),
        }
    }

    /// Opens the File tab's Backstage view on `page`, committing a cell still being edited first
    /// (File actions work on the whole workbook). A draft that can't be committed keeps the sheet.
    pub fn open_backstage(&mut self, page: backstage::Page) {
        if !self.chrome.file_tab {
            return;
        }
        if !self.commit_edit(0, 0, false, false) {
            return;
        }
        match &mut self.backstage {
            Some(b) => b.page = page,
            None => self.backstage = Some(backstage::Backstage::new(page)),
        }
    }
}
