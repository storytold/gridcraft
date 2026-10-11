//! GridCraft's egui frontend: an Excel-style window (title bar with Quick Access Toolbar,
//! ribbon, formula bar, grid, sheet tabs, status bar) over `gridcraft-engine`.
//!
//! The UI is thin: it reads engine state and acts through [`SheetApp::run`] (engine commands by
//! id). Everything it can do is also reachable over the control channel ([`control`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod border_preview;
mod cell_pictures;
pub mod chartview;
pub mod control;
pub mod credits;
pub mod dialogs;
pub mod editor;
pub mod fnlist;
pub mod formula_bar;
pub mod grid;
pub mod icons;
pub mod keymap;
pub mod keytips;
pub mod l10n;
pub mod options;
pub mod panes;
pub mod pivot_pane;
pub mod ribbon;
pub mod tabs;
pub mod text_box;
pub mod theme;
pub mod widgets;

use std::collections::HashMap;
use std::sync::Arc;

use crate::l10n::Tr;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::{EngineError, Session, UiRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

pub use control::{ControlRequest, ControlResponse};

/// Accept upstream short codes and supported language tags, including regional variants.
fn interface_language(code: &str) -> Option<&'static gridcraft_locale::Language> {
    let normalized = code.trim().replace('_', "-");
    let primary = normalized.split(['-', '.']).next()?.to_ascii_lowercase();
    if !gridcraft_locale::LANGUAGES.iter().any(|l| l.tag.split('-').next().is_some_and(|p| p.eq_ignore_ascii_case(&primary))) {
        return None;
    }
    Some(gridcraft_locale::negotiate_language(&normalized))
}

/// The code old builds detected for `system_locale` and saved when the user never chose: the
/// primary language when they had a translation for it, English otherwise.
fn legacy_system_language(system_locale: &str) -> &'static str {
    let primary = system_locale.trim().split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    ["zh", "ja", "ko", "ru", "pt"].into_iter().find(|code| *code == primary).unwrap_or("en")
}

/// Persisted UI preferences.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    /// The manual preference, retained when following the system appearance.
    pub dark: bool,
    pub system_theme: bool,
    pub ribbon_tab: String,
    pub ribbon_collapsed: bool,
    pub formula_bar: bool,
    pub formula_bar_expanded: bool,
    pub status_bar: bool,
    pub recent: Vec<String>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            dark: false,
            system_theme: false,
            ribbon_tab: "Home".into(),
            ribbon_collapsed: false,
            formula_bar: true,
            formula_bar_expanded: false,
            status_bar: true,
            recent: vec![],
        }
    }
}

impl UiState {
    pub fn theme_preference(&self) -> egui::ThemePreference {
        if self.system_theme {
            egui::ThemePreference::System
        } else if self.dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        }
    }

    pub fn theme_mode(&self) -> &'static str {
        if self.system_theme {
            "system"
        } else if self.dark {
            "dark"
        } else {
            "light"
        }
    }
}

/// Per-sheet view state (scroll position in sheet points).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SheetView {
    pub scroll: egui::Vec2,
}

/// Host services the platform shell provides (file pickers, opening URLs).
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn Fn() -> Option<String>>>,
    pub pick_save: Option<Box<dyn Fn(&str) -> Option<String>>>,
    /// Native picture picker (PNG/JPEG), separate from workbook opening.
    pub pick_picture: Option<Box<dyn Fn() -> Option<String>>>,
    /// Browser picture picker. Each dialog owns its inbox, so canceled picks cannot
    /// insert into a subsequent dialog or open the image as a workbook.
    pub pick_picture_async: Option<Box<dyn Fn(Inbox)>>,
    pub open_url: Option<Box<dyn Fn(&str)>>,
    /// Publish HTML and its plain-text alternative together. `Ok` means the host owns the
    /// write (including any asynchronous fallback); an immediate error uses egui's text path.
    pub copy_html: Option<Box<dyn FnMut(&str, &str) -> Result<(), String>>>,
    /// Web: deliver a file to the user (name, bytes).
    pub download: Option<Box<dyn Fn(&str, &[u8])>>,
    /// Web: start an asynchronous file pick; the bytes arrive later in `inbox`.
    pub open_async: Option<Box<dyn Fn()>>,
    /// Files delivered asynchronously (name, bytes), opened on the next frame.
    pub inbox: Option<Inbox>,
    /// Web: start downloading the font file `fonts/<name>` (CJK text needs one); the bytes arrive
    /// later in `font_inbox` under the same name.
    pub fetch_font: Option<Box<dyn Fn(&str)>>,
    /// Fonts delivered asynchronously (name, bytes), added to the font set on the next frame.
    pub font_inbox: Option<Inbox>,
}

/// Shared queue of files read asynchronously (browser file picker, drag and drop).
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

#[derive(Clone, Debug, Default)]
pub struct Perf {
    pub frame_ms: f64,
    pub grid_ms: f64,
    pub fps: f64,
    last: f64,
}

pub struct SheetApp {
    /// Interface texts for the active UI language (rebuilt on `UiRequest::LocaleChanged`).
    pub l10n: l10n::Localizer,
    pub session: Session,
    pub ui: UiState,
    pub views: HashMap<(u64, usize), SheetView>,
    pub editor: Option<editor::EditState>,
    pub text_box_editor: Option<text_box::EditState>,
    pub dialog: Option<dialogs::Dialog>,
    pub message: Option<(String, String)>,
    pub toast: Option<(String, f64)>,
    pub services: Services,
    /// Synthetic input from agents, one batch per frame (drags need several frames).
    pub synthetic: std::collections::VecDeque<Vec<egui::Event>>,
    pub control_rx: Option<std::sync::mpsc::Receiver<ControlRequest>>,
    pub grid: grid::GridState,
    pub perf: Perf,
    pub fonts_ready: bool,
    fonts_set: bool,
    /// Shortcuts of the grid in the active language (rebuilt with `l10n`).
    pub keymap: keymap::Keymap,
    /// Font set in use (`FontNeeds::key`).
    font_key: Option<theme::FontKey>,
    /// A CJK character was seen in the interface language or a workbook: keep the CJK fonts.
    cjk_seen: bool,
    /// Document (uid, revision) already scanned for CJK text.
    cjk_checked: Option<(u64, u64)>,
    /// Fonts the host delivered (web).
    font_downloads: Vec<(String, Arc<egui::FontData>)>,
    /// Font file already requested from the host.
    font_requested: Option<&'static str>,
    effective_dark: bool,
    pub name_box: Option<String>,
    /// Transient ribbon keyboard navigation; never saved with UI preferences.
    pub keytips: keytips::KeyTips,
    pub(crate) shots: control::Shots,
    /// Chart selected on the sheet (id).
    pub selected_chart: Option<u32>,
    pub started: f64,
}

impl SheetApp {
    pub fn new(session: Session, services: Services) -> SheetApp {
        let l10n = l10n::Localizer::new(session.locale().ui.tag);
        let keymap = keymap::Keymap::new(&l10n, l10n::platform_of(egui::os::OperatingSystem::default()));
        SheetApp {
            l10n,
            session,
            ui: UiState::default(),
            views: HashMap::new(),
            editor: None,
            text_box_editor: None,
            dialog: None,
            message: None,
            toast: None,
            services,
            synthetic: Default::default(),
            control_rx: None,
            grid: grid::GridState {
                last_border: "bottom".into(),
                last_fill: "#FFFF00".into(),
                last_font_color: "#C00000".into(),
                ..Default::default()
            },
            perf: Perf::default(),
            fonts_ready: false,
            fonts_set: false,
            keymap,
            font_key: None,
            cjk_seen: false,
            cjk_checked: None,
            font_downloads: Vec::new(),
            font_requested: None,
            effective_dark: false,
            name_box: None,
            keytips: keytips::KeyTips::default(),
            shots: control::Shots::default(),
            selected_chart: None,
            started: now_ms(),
        }
    }

    /// One-time context setup: visuals and a default font set. [`SheetApp::logic`] replaces the
    /// fonts with the set the interface language and the open workbooks need.
    pub fn setup_context(ctx: &egui::Context, dark: bool) {
        ctx.set_fonts(theme::font_definitions(&theme::FontNeeds::default()));
        theme::apply(ctx, dark);
    }

    /// Import the old interface-only preference only while the engine follows the system.
    /// Unknown or malformed values are ignored without resetting other UI preferences.
    ///
    /// Old builds always saved a language; when the user never picked one, it was their system
    /// detection, which knew few languages and fell back to English (a German system saved `en`).
    /// That value is not a choice, so it keeps following the system, now German.
    pub fn migrate_ui_language(&mut self, saved: &Json) -> bool {
        if self.session.prefs.ui_language != "system" {
            return false;
        }
        let Some(code) = saved.get("language").and_then(Json::as_str) else { return false };
        if code.eq_ignore_ascii_case(legacy_system_language(self.session.system_locale())) {
            return false;
        }
        self.set_interface_language(code).is_ok()
    }

    fn set_interface_language(&mut self, code: &str) -> Result<Json, EngineError> {
        let Some(language) = interface_language(code) else {
            let available = gridcraft_locale::LANGUAGES.iter().map(|l| l.tag).collect::<Vec<_>>().join(", ");
            return Err(EngineError::InvalidLocale(format!("unknown language {code:?} (available: {available})")));
        };
        let mut result = self.session.execute("app.setLocale", json!({"uiLanguage": language.tag}))?;
        self.after_engine();
        result["language"] = json!(language.tag);
        Ok(result)
    }

    /// Runs an engine command (or a UI command) and handles UI requests it makes. Errors are the
    /// engine's English text, which the control channel and tests read.
    pub fn run(&mut self, id: &str, params: Json) -> Result<Json, String> {
        self.run_typed(id, params).map_err(|e| e.to_string())
    }

    /// Like [`SheetApp::run`], with the structured error that [`SheetApp::alert`] translates.
    pub fn run_typed(&mut self, id: &str, params: Json) -> Result<Json, EngineError> {
        if id.starts_with("file.")
            || matches!(
                id,
                "window.activate"
                    | "sheet.activate"
                    | "sheet.next"
                    | "sheet.previous"
                    | "sheet.move"
                    | "sheet.hide"
                    | "sheet.unhide"
                    | "view.newWindow"
                    | "view.hideWindow"
                    | "view.unhideWindow"
                    | "home.insertSheet"
                    | "home.deleteSheet"
                    | "edit.undo"
                    | "edit.redo"
                    | "object.delete"
                    | "object.move"
            )
        {
            self.commit_text_box_edit()?;
        }
        if let Some(r) = self.run_ui_command(id, &params) {
            return r;
        }
        let r = self.session.execute(id, params);
        self.after_engine();
        if id == "file.exportPdf"
            && let Ok(r) = &r
        {
            self.deliver_pdf(r);
        }
        r
    }

    /// Copy or cut the selected cells to the system clipboard. Engine-only commands keep
    /// their existing internal clipboard behavior and do not write to the host clipboard.
    pub fn copy_to_clipboard(&mut self, ctx: &egui::Context, command: &str) {
        // Only ask the engine for HTML when the host can publish it.
        let Ok(result) = self.run(command, json!({"html": self.services.copy_html.is_some()})) else { return };
        let Some(text) = result.get("text").and_then(Json::as_str) else { return };
        if let Some(html) = result.get("html").and_then(Json::as_str)
            && let Some(copy_html) = &mut self.services.copy_html
        {
            match copy_html(html, text) {
                Ok(()) => return,
                Err(error) => log::warn!("HTML clipboard unavailable; copying plain text: {error}"),
            }
        }
        ctx.copy_text(text.to_string());
    }

    /// Web: `file.exportPdf` without a path returns the PDF as base64; hand it to the browser as a download.
    fn deliver_pdf(&self, result: &Json) {
        if result.get("path").is_some() {
            return;
        }
        if let Some(dl) = &self.services.download
            && let Some(b) = result.get("base64").and_then(Json::as_str).and_then(gridcraft_engine::io::base64_decode)
        {
            let title = self.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
            dl(&std::path::Path::new(&title).with_extension("pdf").to_string_lossy(), &b);
        }
    }

    /// Runs a command and shows its error in a message box (for menu/ribbon clicks).
    pub fn run_or_alert(&mut self, id: &str, params: Json) {
        if let Err(e) = self.run_typed(id, params) {
            self.alert(&e);
        }
    }

    /// Shows an engine error in the interface language.
    pub fn alert(&mut self, e: &EngineError) {
        let text = self.error_text(e);
        if matches!(e, EngineError::Disabled(..)) {
            self.toast = Some((text, now_ms()));
        } else {
            self.message = Some(("GridCraft".into(), text));
        }
    }

    /// The message of an engine error in the interface language: the `error-<code>` message of the
    /// language files, else the engine's English text. Invalid parameters show just the detail the
    /// command reports.
    pub fn error_text(&self, e: &EngineError) -> String {
        if let EngineError::BadParams { msg, .. } = e {
            return msg.clone();
        }
        if let EngineError::Other(message) = e {
            return self.l10n.tr(message).into_owned();
        }
        let args = e.args();
        let args: Vec<(&str, l10n::Arg)> = args.iter().map(|(k, v)| (*k, l10n::Arg::from(v))).collect();
        match self.l10n.error(e.code(), &args) {
            Some(text) => text.into_owned(),
            None => self.l10n.tr(&clean_error(&e.to_string())).into_owned(),
        }
    }

    /// Re-reads the session's locale after `UiRequest::LocaleChanged`: interface texts and shortcuts
    /// (the fonts follow in `logic`).
    pub fn rebuild_l10n(&mut self) {
        let locale = self.session.locale();
        self.l10n = l10n::Localizer::new(locale.ui.tag);
        self.keymap = keymap::Keymap::new(&self.l10n, self.keymap.platform());
        self.name_box = None;
        // The AutoFilter list names its blanks row in the interface language.
        self.grid.close_filter_menu();
        // A cell being edited is rewritten in the new formula language and region.
        if let Some(ed) = &mut self.editor {
            let wb = self.session.active().map(|d| &d.wb);
            let sheet = ed.sheet;
            ed.switch_locale(locale, &|n: &str| wb.is_some_and(|wb| wb.knows_name(n, sheet)));
        }
    }

    /// A canonical formula spelled in the formula language and region, with the names of the
    /// active sheet. Text that is not a complete formula is returned unchanged.
    pub fn local_formula(&self, canonical: &str) -> String {
        let wb = self.session.active().map(|d| &d.wb);
        let known = |n: &str| wb.is_some_and(|wb| wb.knows_name(n, wb.active_sheet));
        gridcraft_engine::locale::to_local_formula(canonical, &self.session.locale().dialect(), &known)
    }

    /// `text` with its decimal point replaced by the region's decimal separator (for numbers that
    /// Rust formatted, such as the width shown while dragging a column).
    pub fn localize_decimal(&self, text: &str) -> String {
        text.replace('.', self.session.locale().regional.decimal.encode_utf8(&mut [0u8; 4]))
    }

    /// How number fields show and read numbers in the session's region.
    pub fn number_style(&self) -> widgets::NumberStyle {
        widgets::NumberStyle::new(self.session.locale().regional)
    }

    /// A number typed in the region's spelling and read like cell input (`1,5`, `1.000`, `10%`,
    /// or a date for a date series' stop value), or `None` when `text` is not a number there.
    pub fn parse_number(&self, text: &str) -> Option<f64> {
        let system = self.session.active().map_or(gridcraft_engine::core::DateSystem::D1900, |d| d.wb.date_system);
        gridcraft_engine::core::parse::parse_number_text_in(text, system, &self.session.locale().regional)
    }

    /// `n` spelled with the region's decimal separator.
    pub fn number_text(&self, n: f64) -> String {
        self.number_style().text(n)
    }

    /// UI-only commands (dialogs, view toggles that aren't saved in the file).
    fn run_ui_command(&mut self, id: &str, p: &Json) -> Option<Result<Json, EngineError>> {
        let r = match id {
            "ui.ribbonTab" => {
                self.ui.ribbon_tab = p.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
                Ok(Json::Null)
            }
            "view.formulaBar" => {
                self.ui.formula_bar = p.get("on").and_then(Json::as_bool).unwrap_or(!self.ui.formula_bar);
                Ok(json!({"on": self.ui.formula_bar}))
            }
            "view.collapseRibbon" => {
                self.ui.ribbon_collapsed = !self.ui.ribbon_collapsed;
                Ok(Json::Null)
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
                    _ => return Some(Err(EngineError::BadParams { cmd: id.into(), msg: "theme mode must be system, light, or dark".into() })),
                }
                Ok(json!({"mode": self.ui.theme_mode()}))
            }
            "view.zoom100" => {
                let r = self.session.execute("view.zoom", json!({"percent": 100}));
                self.after_engine();
                r
            }
            "app.language.set" => {
                let code = p.get("language").or_else(|| p.get("code")).and_then(Json::as_str).unwrap_or("");
                self.set_interface_language(code)
            }
            "app.language.english" => self.set_interface_language("en-US"),
            "app.language.japanese" => self.set_interface_language("ja-JP"),
            "ui.dialog" => {
                let name = p.get("name").and_then(Json::as_str).unwrap_or("");
                self.open_dialog(name, p.clone());
                Ok(json!({"dialog": self.dialog.as_ref().map(|d| d.name())}))
            }
            "ui.message.dismiss" => {
                self.message = None;
                Ok(Json::Null)
            }
            _ => return None,
        };
        Some(r)
    }

    /// Picks up requests commands made (dialogs, edit mode…).
    pub fn after_engine(&mut self) {
        for req in self.session.take_ui_requests() {
            match req {
                UiRequest::Dialog(name, params) => self.open_dialog(&name, params),
                UiRequest::Message(m) => self.message = Some(("GridCraft".into(), m)),
                UiRequest::EditCell(text) => {
                    // The engine sends the prefill already spelled in the formula language.
                    self.begin_edit(text, false);
                }
                UiRequest::LocaleChanged => self.rebuild_l10n(),
                UiRequest::OpenUrl(u) => {
                    if let Some(f) = &self.services.open_url {
                        f(&u);
                    }
                }
            }
        }
    }

    /// The fonts the interface language and the text of the open workbooks need.
    pub fn font_needs(&mut self) -> theme::FontNeeds<'_> {
        let cjk = self.needs_cjk();
        theme::FontNeeds { language: self.l10n.tag(), cjk, downloaded: &self.font_downloads }
    }

    /// Whether the font set needs CJK faces. The Options dialog lists every language by its
    /// native name, so opening it makes them needed for good (they load once, not on every open
    /// and close).
    fn needs_cjk(&mut self) -> bool {
        if self.dialog.as_ref().is_some_and(|d| d.name == "options") {
            self.cjk_seen = true;
        }
        if !self.cjk_seen {
            self.cjk_seen = self.scan_cjk();
        }
        self.cjk_seen || theme::is_cjk_language(self.l10n.tag())
    }

    /// Whether the text being edited, or the active workbook (when it changed since the last scan),
    /// has a character that needs a CJK font. The scan reads at most 200,000 cells per workbook.
    fn scan_cjk(&mut self) -> bool {
        if self.editor.as_ref().is_some_and(|e| theme::has_cjk(&e.text)) {
            return true;
        }
        let Some(d) = self.session.active() else { return false };
        let stamp = (d.uid, d.revision);
        if self.cjk_checked == Some(stamp) {
            return false;
        }
        let sheets = &d.wb.sheets;
        let found = sheets.iter().any(|sh| theme::has_cjk(&sh.name))
            || sheets
                .iter()
                .flat_map(|sh| sh.cells.iter())
                .take(200_000)
                .any(|(_, c)| matches!(&c.value, gridcraft_engine::core::Value::Text(t) if theme::has_cjk(t)));
        self.cjk_checked = Some(stamp);
        found
    }

    /// Installs the font set when the needs changed (language, CJK text, a downloaded font arrived),
    /// and asks the host for the CJK font file the web build loads on demand.
    fn refresh_fonts(&mut self, ctx: &egui::Context) {
        let arrived: Vec<(String, Vec<u8>)> = self
            .services
            .font_inbox
            .as_ref()
            .map(|i| std::mem::take(&mut *i.lock().unwrap_or_else(std::sync::PoisonError::into_inner)))
            .unwrap_or_default();
        for (name, bytes) in arrived {
            // A file delivered twice changes nothing (and must not rebuild the fonts).
            if self.font_downloads.iter().any(|(n, _)| *n == name) {
                continue;
            }
            match theme::font_data(bytes, 0) {
                Some(fd) => self.font_downloads.push((name, Arc::new(fd))),
                // Static hosts and proxies answer a missing file with an HTML page.
                None => log::warn!("font {name} is not a TrueType/OpenType file; CJK text shows as boxes without it"),
            }
        }
        let cjk = self.needs_cjk();
        let language = self.l10n.tag();
        if cjk && let Some(fetch) = &self.services.fetch_font {
            let file = theme::web_cjk_font(language);
            if self.font_requested != Some(file) && !self.font_downloads.iter().any(|(n, _)| n == file) {
                fetch(file);
                self.font_requested = Some(file);
            }
        }
        let needs = theme::FontNeeds { language, cjk, downloaded: &self.font_downloads };
        let key = needs.key();
        if self.font_key == Some(key) {
            return;
        }
        ctx.set_fonts(theme::font_definitions(&needs));
        self.font_key = Some(key);
        self.fonts_set = true;
        ctx.request_repaint();
    }

    pub fn open_dialog(&mut self, name: &str, params: Json) {
        if let Err(e) = self.commit_text_box_edit() {
            self.message = Some((self.l10n.tr("Text Box").into_owned(), self.error_text(&e)));
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
                } else if let Some(dl) = &self.services.download
                    && let Ok(r) = self.session.run("file.saveBytes", json!({"format": "xlsx"}))
                    && let Some(b) = r.get("base64").and_then(Json::as_str).and_then(gridcraft_engine::io::base64_decode)
                {
                    let name = r.get("name").and_then(Json::as_str).unwrap_or("Book.xlsx").to_string();
                    dl(&name, &b);
                }
            }
            other => self.dialog = dialogs::Dialog::open(self, other, params),
        }
    }

    pub fn open_path(&mut self, path: &str) {
        match self.run_typed("file.open", json!({"path": path})) {
            Ok(_) => {
                self.ui.recent.retain(|p| p != path);
                self.ui.recent.insert(0, path.to_string());
                self.ui.recent.truncate(20);
            }
            Err(e) => self.message = Some(("GridCraft".into(), self.error_text(&e))),
        }
        self.after_engine();
    }

    pub fn view_key(&self) -> Option<(u64, usize)> {
        self.session.active().map(|d| (d.uid, d.wb.active_sheet))
    }

    pub fn view(&self) -> SheetView {
        self.view_key().and_then(|k| self.views.get(&k).copied()).unwrap_or_default()
    }

    pub fn view_mut(&mut self) -> Option<&mut SheetView> {
        let k = self.view_key()?;
        Some(self.views.entry(k).or_default())
    }

    /// Starts editing the active cell. `text` (what was typed, in the formula language and region)
    /// replaces the content or pre-fills it; without it the cell's input is shown in the formula
    /// language.
    pub fn begin_edit(&mut self, text: Option<String>, from_formula_bar: bool) {
        let loc = self.session.locale();
        if let Err(e) = self.commit_text_box_edit() {
            self.message = Some((self.l10n.tr("Text Box").into_owned(), self.error_text(&e)));
            return;
        }
        let Some(d) = self.session.active() else { return };
        let Some(sh) = d.wb.active() else { return };
        let at = sh.merge_at(d.selection.active).map(|m| m.start).unwrap_or(d.selection.active);
        // A picture has no editable scalar text. Selecting it, F2, or clicking
        // the formula bar must not turn its fallback value into a text edit.
        // Typing still starts an intentional replacement.
        if text.is_none() && sh.cell_pictures.contains_key(&at) {
            return;
        }
        let current = gridcraft_engine::locale::sheet_input_local(sh, at, &loc, &|n: &str| d.wb.knows_name(n, d.wb.active_sheet));
        let (text, replace) = match text {
            Some(t) => (t, true),
            None => (current.clone(), false),
        };
        let mut ed = editor::EditState::new(d.wb.active_sheet, at, text, replace, from_formula_bar, loc);
        if replace && !ed.is_formula() {
            ed.completion = editor::column_completion(sh, at, &ed.text, &ed.locale);
        }
        self.editor = Some(ed);
        self.session.mode = if replace { gridcraft_engine::Mode::Enter } else { gridcraft_engine::Mode::Edit };
    }

    /// Commits the edit; `then` moves the selection (dr, dc) afterwards.
    pub fn commit_edit(&mut self, dr: i64, dc: i64, array: bool, fill_selection: bool) -> bool {
        let Some(ed) = self.editor.take() else { return true };
        self.session.mode = gridcraft_engine::Mode::Ready;
        let text = match &ed.completion {
            Some(full) if full.to_lowercase().starts_with(&ed.text.to_lowercase()) => full.clone(),
            _ => ed.text.clone(),
        };
        // Data validation.
        if let Some(d) = self.session.active()
            && let Some((dv, msg)) = gridcraft_engine::cmd::data::check_validation(&d.wb, ed.sheet, ed.cell, &text)
        {
            // Without a message of its own the rule says what the interface language says.
            let msg = if dv.error_message.is_empty() { self.l10n.text("ui-validation-default-message", &[]).into_owned() } else { msg };
            if dv.error_style == gridcraft_engine::model::ErrorStyle::Stop {
                let title = if dv.error_title.is_empty() { "GridCraft".to_string() } else { dv.error_title.clone() };
                self.message = Some((title, msg));
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                return false;
            }
            self.toast = Some((msg, now_ms()));
        }
        let r = if fill_selection {
            self.session.execute("range.fill", json!({"inputLocal": text}))
        } else {
            self.session.execute("cell.set", json!({"cell": ed.cell.a1(), "inputLocal": text, "array": array}))
        };
        match r {
            Ok(_) => {
                if dr != 0 || dc != 0 {
                    self.move_after_enter(dr, dc);
                }
                self.after_engine();
                true
            }
            Err(e) => {
                self.message = Some(("GridCraft".into(), self.error_text(&e)));
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                false
            }
        }
    }

    pub fn cancel_edit(&mut self) {
        self.editor = None;
        self.session.mode = gridcraft_engine::Mode::Ready;
    }

    /// Enter/Tab move inside a multi-cell selection, otherwise to the next cell.
    pub fn move_after_enter(&mut self, dr: i64, dc: i64) {
        let multi = self.session.active().is_some_and(|d| !d.selection.is_single_cell());
        if multi {
            let fwd = dr > 0 || dc > 0;
            let _ = self.session.run("selection.next", json!({"forward": fwd, "byRow": dc != 0}));
        } else {
            let _ = self.session.run("selection.move", json!({"dr": dr, "dc": dc}));
        }
        self.grid.ensure_visible = true;
    }

    /// Per-frame logic (control channel, screenshots). Call before `ui`.
    pub fn logic(&mut self, ctx: &egui::Context) {
        // The keyboard family can change after start-up (the web build learns it from the user agent).
        let platform = l10n::platform(ctx);
        if platform != self.keymap.platform() {
            self.keymap = keymap::Keymap::new(&self.l10n, platform);
        }
        // The fonts follow the interface language and the text on screen. New fonts apply from the
        // next frame on: the first set is applied before anything is painted.
        let first = !self.fonts_set;
        self.refresh_fonts(ctx);
        if !self.fonts_ready {
            if first {
                theme::apply(ctx, self.ui.dark);
                ctx.request_repaint();
            } else {
                self.fonts_ready = true;
            }
        }
        let preference = self.ui.theme_preference();
        if ctx.options(|o| o.theme_preference) != preference {
            ctx.set_theme(preference);
            ctx.request_repaint();
        }
        self.effective_dark = ctx.theme() == egui::Theme::Dark;
        control::poll(self, ctx);
        // Files read asynchronously (web file picker, dropped files).
        let arrived: Vec<(String, Vec<u8>)> = self
            .services
            .inbox
            .as_ref()
            .map(|i| std::mem::take(&mut *i.lock().unwrap_or_else(std::sync::PoisonError::into_inner)))
            .unwrap_or_default();
        for (name, bytes) in arrived {
            let b64 = gridcraft_engine::io::base64_encode(&bytes);
            if let Err(e) = self.run_typed("file.open", json!({"name": name, "base64": b64})) {
                self.message = Some(("GridCraft".into(), self.error_text(&e)));
            }
            self.after_engine();
        }
        if self.session.active().is_none() {
            self.session.new_workbook();
        }
    }

    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        if let Some(mut batch) = self.synthetic.pop_front() {
            raw.events.append(&mut batch);
        }
    }

    /// Lays out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.fonts_ready {
            ctx.request_repaint();
            return;
        }
        let t0 = now_ms();
        text_box::before_ui(self, &ctx);
        let t = theme::Tokens::get(&ctx);
        self.ribbon_keys(&ctx);
        ribbon::title_bar(self, ui);
        ribbon::show(self, ui);
        if self.ui.formula_bar {
            formula_bar::show(self, ui);
        }
        if self.ui.status_bar {
            tabs::status_bar(self, ui);
        }
        tabs::sheet_tabs(self, ui);
        pivot_pane::show(self, ui);
        panes::show(self, ui);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.grid_bg)).show(ui, |ui| {
            let g0 = now_ms();
            grid::show(self, ui);
            self.perf.grid_ms = now_ms() - g0;
        });
        dialogs::show(self, &ctx);
        widgets::message_box(self, &ctx);
        widgets::toast(self, &ctx);
        control::issue_screenshots(self, &ctx);
        control::collect_screenshots(self, &ctx);
        if !ctx.input(|i| i.focused) {
            // Nothing else to do.
        }
        let now = now_ms();
        self.perf.frame_ms = now - t0;
        if self.perf.last > 0.0 {
            let dt = now - self.perf.last;
            if dt > 0.0 {
                self.perf.fps = self.perf.fps * 0.9 + 100.0 / dt;
            }
        }
        self.perf.last = now;
        // AutoSave: save workbooks that have a file shortly after they change.
        if self.grid.autosave
            && self.editor.is_none()
            && self.text_box_editor.is_none()
            && now - self.grid.last_autosave > 2000.0
            && self.session.active().is_some_and(|d| d.is_dirty() && d.path.as_deref().is_some_and(|p| p.ends_with(".xlsx")))
        {
            self.grid.last_autosave = now;
            let _ = self.session.run("file.save", json!({}));
        }
        // Window title.
        let l = self.l10n;
        if let Some(d) = self.session.active() {
            let name = d.display_title();
            let title = if d.is_dirty() { l.text("ui-window-title-edited", &[("title", l10n::Arg::from(name.as_str()))]).into_owned() } else { name };
            if self.grid.last_title.as_deref() != Some(title.as_str()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
                self.grid.last_title = Some(title);
            }
        }
    }

    fn ribbon_keys(&mut self, ctx: &egui::Context) {
        let before = self.keytips.prefix().map(str::to_string);
        let enabled = self.editor.is_none()
            && self.dialog.is_none()
            && self.message.is_none()
            && self.name_box.is_none()
            && !ctx.text_edit_focused()
            && self.grid.context_menu.is_none()
            && self.grid.header_menu.is_none()
            && self.grid.filter_menu.is_none()
            && self.grid.list_picker.is_none()
            && self.grid.renaming_tab.is_none()
            && (before.is_some() || !egui::Popup::is_any_open(ctx));
        let actions = self.keytips.process(ctx, enabled);
        if before.is_some() && before.as_deref() != self.keytips.prefix() && !ctx.input(|i| i.pointer.any_pressed()) {
            // Leave pointer cancellation to the popup so clicking a menu item still works.
            egui::Popup::close_all(ctx);
        }
        // The legacy Edit/Format paths lead to the same existing Home menus. Switch once, when
        // the prefix changes, not every frame (that would reset a tab the user picked meanwhile).
        let after = self.keytips.prefix();
        if after != before.as_deref() && matches!(after, Some("E" | "O" | "OC")) {
            self.run_or_alert("ui.ribbonTab", json!({"tab": "Home"}));
            self.ui.ribbon_collapsed = false;
        }
        for action in actions {
            match action {
                keytips::Action::Home => {
                    self.run_or_alert("ui.ribbonTab", json!({"tab": "Home"}));
                    self.ui.ribbon_collapsed = false;
                }
                keytips::Action::Command(id) => self.run_or_alert(id, json!({})),
                keytips::Action::Dialog(name) => self.open_dialog(name, json!({})),
            }
        }
    }

    /// Current selection's top-left cell text as A1 (for the Name Box).
    pub fn name_box_text(&self) -> String {
        let Some(d) = self.session.active() else { return String::new() };
        let sel = &d.selection;
        let cur = sel.current();
        // A named range that exactly matches shows its name.
        let sh = d.wb.active();
        if let Some(sh) = sh {
            for n in &d.wb.names {
                let target = n.formula.replace('$', "");
                let (sheet, body) = match target.rsplit_once('!') {
                    Some((s, b)) => (Some(s.trim_matches('\'').to_string()), b.to_string()),
                    None => (None, target.clone()),
                };
                if sheet.as_deref().is_none_or(|s| s.eq_ignore_ascii_case(&sh.name)) && RangeRef::parse(&body) == Some(cur) {
                    return n.name.clone();
                }
            }
        }
        if self.grid.drag_select && !cur.is_single() {
            // The size while dragging uses the formula language's R1C1 letters (pt-BR `5L x 3C`).
            let [row, col] = self.session.locale().formula.r1c1;
            return format!("{}{row} x {}{col}", cur.height(), cur.width());
        }
        sel.active.a1()
    }
}

/// Strips the engine's error prefixes for message boxes.
pub fn clean_error(e: &str) -> String {
    if let Some(rest) = e.split_once("`: ").map(|(_, r)| r) {
        return rest.to_string();
    }
    e.to_string()
}

#[allow(clippy::disallowed_methods)] // the clock is read only off wasm
pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

pub fn cell(r: u32, c: u32) -> CellRef {
    CellRef::new(r, c)
}
