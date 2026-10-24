//! GridCraft's egui frontend: an Excel-style window (title bar with Quick Access Toolbar,
//! ribbon, formula bar, grid, sheet tabs, status bar) over `gridcraft-engine`.
//!
//! The UI is thin: it reads engine state and acts through [`SheetApp::run`] (engine commands by
//! id). Everything it can do is also reachable over the control channel ([`control`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod backstage;
mod border_preview;
mod cell_pictures;
pub mod chartview;
pub mod control;
pub mod credits;
pub mod dialogs;
pub mod editor;
mod floating_pictures;
pub mod fonts;
pub mod formula_bar;
mod formula_locale;
pub mod grid;
pub mod i18n;
pub mod icons;
pub mod keymap;
pub mod keytips;
pub mod outline;
pub mod panes;
pub mod paper_view;
pub mod pivot_pane;
pub mod printing;
pub mod ribbon;
pub mod selection_glide;
pub mod tabs;
pub mod text_box;
pub mod theme;
pub mod widgets;

use std::collections::HashMap;

use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::{Session, UiRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

pub use control::{ControlRequest, ControlResponse};
pub mod desktop_theme;

/// Persisted UI preferences.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    /// The manual preference, retained when following the system appearance.
    pub dark: bool,
    pub system_theme: bool,
    pub ribbon_tab: String,
    pub ribbon_collapsed: bool,
    pub formula_bar_expanded: bool,
    pub status_bar: bool,
    /// Motion in the UI: the selection outline glides to a new selection (View ▸ Animations).
    pub animations: bool,
    /// Enable AutoComplete for cell values (File ▸ Options ▸ Editing): typed text is completed
    /// from the one matching entry in the same column.
    pub autocomplete: bool,
    /// AutoSave (the title-bar switch): save saved-to-file .xlsx workbooks shortly after every
    /// change. A preference like the rest of this struct — it stays on across restarts (#1901).
    pub autosave: bool,
    pub recent: Vec<String>,
    /// One-shot keytip request for a ribbon control: set when a keytip action resolves to
    /// [`keytips::Action::Control`], consumed (and cleared) by the control's render site.
    #[serde(skip)]
    pub keytip_invoke: Option<&'static str>,
    /// Interface language. English by default (so tests and headless renders never depend on the
    /// host's locale); the desktop app starts from [`i18n::Language::system`] when `ui.json` has
    /// no usable language (see [`i18n::saved_language`]). An unknown saved value reads as English
    /// rather than failing the whole `UiState`.
    #[serde(deserialize_with = "i18n::lenient")]
    pub language: i18n::Language,
    /// Interface size in percent on top of the desktop's scale (100 = Auto), applied as egui's
    /// zoom factor so the ribbon, menus, dialogs and grid grow together. Kept within
    /// [`INTERFACE_SCALE_MIN`]..=[`INTERFACE_SCALE_MAX`]; a malformed saved value reads as 100.
    #[serde(deserialize_with = "lenient_interface_scale")]
    pub interface_scale: u16,
}

pub const INTERFACE_SCALE_MIN: u16 = 75;
pub const INTERFACE_SCALE_MAX: u16 = 200;

/// A requested interface size, rounded and clamped to the supported range; `None` when it isn't
/// a finite number.
pub fn clamp_interface_scale(percent: f64) -> Option<u16> {
    percent.is_finite().then(|| percent.round().clamp(INTERFACE_SCALE_MIN as f64, INTERFACE_SCALE_MAX as f64) as u16)
}

fn lenient_interface_scale<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    let v = Json::deserialize(d)?;
    Ok(v.as_f64().and_then(clamp_interface_scale).unwrap_or(100))
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            dark: false,
            system_theme: false,
            ribbon_tab: "Home".into(),
            ribbon_collapsed: false,
            formula_bar_expanded: false,
            status_bar: true,
            animations: true,
            autocomplete: true,
            autosave: false,
            recent: vec![],
            keytip_invoke: None,
            language: i18n::Language::En,
            interface_scale: 100,
        }
    }
}

impl UiState {
    /// The state saved by an earlier run, as the app starts with it. The ribbon tab is written to
    /// the preferences but not restored: Excel opens on Home every launch, so a contextual tab
    /// (Table Design) cannot come back with nothing selected to justify it.
    pub fn at_startup(mut self) -> Self {
        self.ribbon_tab = "Home".into();
        self
    }

    pub fn theme_preference(&self) -> egui::ThemePreference {
        if self.system_theme {
            egui::ThemePreference::System
        } else if self.dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        }
    }

    /// The zoom factor for the interface size preference.
    pub fn zoom_factor(&self) -> f32 {
        clamp_interface_scale(self.interface_scale as f64).unwrap_or(100) as f32 / 100.0
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

/// Severity of a notice shown in the info bar under the ribbon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Warning,
}

/// A dismissible bar under the ribbon, in the spirit of Excel's message bar. Used to tell the
/// user what an opened file carried that GridCraft handled differently (e.g. preserved macros).
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    pub kind: NoticeKind,
    /// Optional button: `(label, dialog name)` — opens that dialog when clicked.
    pub action: Option<(String, String)>,
}

/// Host services the platform shell provides (file pickers, opening URLs).
#[derive(Default)]
pub struct Services {
    /// Desktop: discover configured printers on a worker.
    pub printers: Option<printing::Discover>,
    /// Desktop: submit the PDF with the chosen settings on a worker.
    pub print_native: Option<printing::Submit>,
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
    /// Read the HTML flavour of the system clipboard when the user pastes text, so cells the
    /// source spreadsheet recorded as numbers stay numbers ("Rp10,000.00" → 10000). `None` from
    /// the host pastes the plain text alone.
    pub paste_html: Option<Box<dyn FnMut() -> Option<String>>>,
    /// Read the system clipboard's plain text for the ribbon and menu Paste commands (Cmd/Ctrl+V
    /// gets it from egui's `Event::Paste`). `None` from the host, or no service (web), pastes
    /// GridCraft's own clipboard.
    pub paste_text: Option<Box<dyn FnMut() -> Option<String>>>,
    /// Web: deliver a file to the user (name, bytes).
    pub download: Option<Box<dyn Fn(&str, &[u8])>>,
    /// Web: start an asynchronous file pick; the bytes arrive later in `inbox`.
    pub open_async: Option<Box<dyn Fn()>>,
    /// Files delivered asynchronously (name, bytes), opened on the next frame.
    pub inbox: Option<Inbox>,
    /// Web: hand PDF bytes to the browser's print flow directly (no download,
    /// no intermediate file) — opens the system print dialog on that PDF.
    pub print: Option<Box<dyn Fn(&[u8])>>,
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

/// Which parts of the app's own window chrome are drawn. Everything is on by default; a host
/// that embeds the sheet in its own window (and owns files itself) can turn off the title bar
/// and the File tab, whose Backstage view then never opens. Not persisted: it describes the
/// host, not a user preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chrome {
    /// The title bar: AutoSave, Home, Save, Undo/Redo, the file name, Comments, Share, Search and
    /// the theme switch.
    pub title_bar: bool,
    /// The ribbon's File tab and the Backstage view it opens (also reached by keyboard and
    /// commands such as `ui.backstage`).
    pub file_tab: bool,
}

impl Default for Chrome {
    fn default() -> Self {
        Chrome { title_bar: true, file_tab: true }
    }
}

pub struct SheetApp {
    pub session: Session,
    pub ui: UiState,
    /// The window chrome to draw (see [`Chrome`]).
    pub chrome: Chrome,
    pub views: HashMap<(u64, usize), SheetView>,
    pub editor: Option<editor::EditState>,
    pub text_box_editor: Option<text_box::EditState>,
    pub dialog: Option<dialogs::Dialog>,
    /// The File tab's Backstage view, when open (UI-only; replaces the ribbon, grid and tabs).
    pub backstage: Option<backstage::Backstage>,
    pub message: Option<(String, String)>,
    pub toast: Option<(String, f64)>,
    /// Dismissible info bar under the ribbon (open-time notices).
    pub notice: Option<Notice>,
    pub services: Services,
    pub printing: printing::State,
    print_feedback_seen: Option<printing::Feedback>,
    /// Synthetic input from agents, one batch per frame (drags need several frames).
    pub synthetic: std::collections::VecDeque<Vec<egui::Event>>,
    pub control_rx: Option<std::sync::mpsc::Receiver<ControlRequest>>,
    pub grid: grid::GridState,
    pub perf: Perf,
    pub fonts_ready: bool,
    fonts_set: bool,
    /// Workbook fonts bound into the current font set; a change rebinds (installed fonts only).
    fonts_bound: Vec<String>,
    /// The (doc uid, revision) `fonts_bound` was computed for, so the style table is walked only
    /// when the workbook changes.
    fonts_seen: (u64, u64),
    /// Set while a font-set change is pending activation: skip this frame's paint, like the
    /// startup `fonts_ready` gate (egui applies new definitions at the next pass).
    fonts_rebind: bool,
    /// Han face order the installed fonts were built with; a language switch rebuilds the fonts
    /// only when it changes this (see `theme::HanOrder`).
    fonts_han: Option<theme::HanOrder>,
    effective_dark: bool,
    /// The desktop's light/dark choice, watched while the app runs (Linux has no winit answer).
    desktop_theme: desktop_theme::DesktopTheme,
    /// The last answer [`desktop_theme`] gave, so a frame that asks costs nothing.
    desktop_dark: Option<bool>,
    pub name_box: Option<String>,
    /// Transient ribbon keyboard navigation; never saved with UI preferences.
    pub keytips: keytips::KeyTips,
    pub(crate) shots: control::Shots,
    /// Chart selected on the sheet (id).
    pub selected_chart: Option<u32>,
    /// The system clipboard's text when GridCraft last copied without publishing to it (a
    /// control-channel or MCP `edit.copy`/`edit.cut`). While the system clipboard still holds this
    /// text, GridCraft's copy is the newer one and Paste uses it.
    clipboard_text_at_copy: Option<String>,
    /// The system clipboard's text right after GridCraft's last cut (the published text, or the
    /// text noted for an unpublished cut); `None` after a copy, or once the clipboard is seen
    /// holding other text. Once the cut has been pasted, Paste leaves this text alone.
    cut_text: Option<String>,
    /// A run of Tabs: (doc uid, sheet, column it started in, cell the last Tab landed on).
    /// Enter on that cell goes to the next row in the starting column, as in Excel (#911).
    tab_run: Option<(u64, usize, u32, CellRef)>,
    pub started: f64,
    /// Every unsaved workbook has been answered for quitting (or `app.quit` asked): the next
    /// window close goes through.
    pub allow_close: bool,
}

impl SheetApp {
    pub fn new(session: Session, services: Services) -> SheetApp {
        SheetApp {
            session,
            ui: UiState::default(),
            chrome: Chrome::default(),
            views: HashMap::new(),
            editor: None,
            text_box_editor: None,
            dialog: None,
            backstage: None,
            message: None,
            toast: None,
            notice: None,
            services,
            printing: Default::default(),
            print_feedback_seen: None,
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
            fonts_bound: Vec::new(),
            fonts_seen: (0, 0),
            fonts_rebind: false,
            fonts_han: None,
            effective_dark: false,
            desktop_theme: desktop_theme::DesktopTheme::default(),
            desktop_dark: None,
            name_box: None,
            keytips: keytips::KeyTips::default(),
            shots: control::Shots::default(),
            selected_chart: None,
            clipboard_text_at_copy: None,
            cut_text: None,
            tab_run: None,
            started: now_ms(),
            allow_close: false,
        }
    }

    /// One-time context setup: start cataloging the machine's fonts, then install the fonts and
    /// visuals. Workbook-specific fonts are bound later, once the scan is done (see [`logic`]).
    ///
    /// [`logic`]: SheetApp::logic
    pub fn setup_context(ctx: &egui::Context, dark: bool) {
        fonts::start_scan();
        Self::setup_context_for_language(ctx, dark, i18n::Language::En);
    }

    /// Sets up fonts and visuals for a specific persisted interface language.
    pub fn setup_context_for_language(ctx: &egui::Context, dark: bool, language: i18n::Language) {
        ctx.set_fonts(theme::font_definitions_for_language(language));
        theme::apply(ctx, dark);
    }

    /// The font names the active workbook uses: every style's font plus the theme's heading/body
    /// fonts. These are the families [`fonts`] binds an installed file for.
    pub fn used_fonts(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        if let Some(d) = self.session.active() {
            names.extend(d.wb.styles.iter().map(|(_, s)| s.font.name.clone()));
            names.push(d.wb.theme.major_font.clone());
            names.push(d.wb.theme.minor_font.clone());
            // Text boxes and shapes name their own typefaces.
            names.extend(d.wb.sheets.iter().flat_map(|sh| sh.shapes.iter().filter_map(|s| s.format.font.clone())));
        }
        names
    }

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

    fn run_command(&mut self, id: &str, params: Json) -> Result<Json, String> {
        if matches!(id, "file.save" | "file.saveAs" | "file.saveBytes") {
            self.store_view_state();
        }
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
        let r = self.session.run(id, params);
        self.after_engine();
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

    /// Copy or cut the selected cells to the system clipboard. Engine-only commands keep
    /// their existing internal clipboard behavior and do not write to the host clipboard.
    pub fn copy_to_clipboard(&mut self, ctx: &egui::Context, command: &str) {
        // Only ask the engine for HTML when the host can publish it.
        let result = match self.run_command(command, json!({"html": self.services.copy_html.is_some()})) {
            Ok(result) => result,
            Err(e) => return self.alert(e),
        };
        let Some(text) = result.get("text").and_then(Json::as_str) else { return };
        // The system clipboard gets this copy, so its text decides what Paste pastes.
        self.clipboard_text_at_copy = None;
        self.cut_text = (command == "edit.cut").then(|| text.replace("\r\n", "\n"));
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

    /// Paste text that came from the system clipboard (Cmd/Ctrl+V, or the ribbon and menu Paste),
    /// with the clipboard's HTML flavour when the host can read it.
    ///
    /// Excel pastes a cut once: after GridCraft's cut has been pasted, the system clipboard text
    /// that came with it is not pasted again over the moved block; GridCraft's own (empty)
    /// clipboard is pasted instead, which reports "Nothing to paste.". Other text means the
    /// clipboard has moved on, so the cut's text is forgotten and pastes normally later.
    pub fn paste_external(&mut self, text: String) {
        if Self::same_clipboard_text(Some(&text), self.cut_text.as_deref()) {
            if self.session.clipboard.is_none() {
                return self.run_or_alert("edit.paste", json!({}));
            }
        } else {
            self.cut_text = None;
        }
        let html = self.services.paste_html.as_mut().and_then(|read| read());
        let params = match html {
            Some(html) => json!({"text": text, "html": html}),
            None => json!({"text": text}),
        };
        self.run_or_alert("edit.paste", params);
    }

    /// Ribbon and menu Paste: paste the text on the system clipboard, as Cmd/Ctrl+V does. With no
    /// text there, no host clipboard (web), or the text that was there when GridCraft last copied
    /// without publishing, GridCraft's own clipboard is pasted.
    pub fn paste_from_clipboard(&mut self) {
        match self.system_clipboard_text() {
            Some(text) if self.session.clipboard.is_none() || self.clipboard_text_at_copy.as_ref() != Some(&text) => self.paste_external(text),
            _ => self.run_or_alert("edit.paste", json!({})),
        }
    }

    /// The system clipboard's plain text, with the line endings of egui's `Event::Paste` text.
    /// `None` when it holds no text or the host can't read it.
    fn system_clipboard_text(&mut self) -> Option<String> {
        self.services.paste_text.as_mut().and_then(|read| read()).filter(|t| !t.is_empty()).map(|t| t.replace("\r\n", "\n"))
    }

    /// Whether two clipboard texts are the same copy: trailing line breaks don't count, as in the
    /// engine's `edit.paste`, and CRLF matches LF (egui's `Event::Paste` text may keep the host's).
    fn same_clipboard_text(a: Option<&str>, b: Option<&str>) -> bool {
        fn chars(t: &str) -> impl Iterator<Item = char> + '_ {
            t.trim_end_matches(['\r', '\n']).chars().filter(|c| *c != '\r')
        }
        match (a, b) {
            (Some(a), Some(b)) => chars(a).eq(chars(b)),
            _ => false,
        }
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
    /// Follow the desktop's light/dark choice while the app runs (Linux; the desktop app calls
    /// this once — tests and embedders don't start the watcher thread).
    pub fn watch_desktop_theme(&mut self) {
        self.desktop_theme = desktop_theme::DesktopTheme::start();
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
        if let Err(e) = self.run(id, params) {
            self.alert(e);
        }
    }

    /// Asks for a `.pdf` path with the save picker, then re-runs `file.exportPdf` with it. Other
    /// params (range, sheets, fitToPage) are kept. A cancelled picker does nothing.
    fn export_pdf_with_picker(&mut self, mut params: Json) {
        if !self.commit_edit(0, 0, false, false) {
            return;
        }
        let title = self.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
        let stem = std::path::Path::new(&title).file_stem().and_then(|s| s.to_str()).unwrap_or("Book1").to_string();
        let Some(pick) = &self.services.pick_save else { return };
        let Some(path) = pick(&format!("{stem}.pdf")) else { return };
        // The picker may suggest a workbook extension; the export writes a PDF.
        let path = std::path::Path::new(&path).with_extension("pdf").to_string_lossy().into_owned();
        match &mut params {
            Json::Object(map) => {
                map.insert("path".into(), Json::String(path));
            }
            other => *other = json!({"path": path}),
        }
        if let Err(e) = self.run("file.exportPdf", params) {
            self.alert(e);
        }
    }

    /// Shows a command error: a toast for an unavailable command, else a message box.
    fn alert(&mut self, e: String) {
        if e.contains("is not available right now") {
            self.toast = Some((e, now_ms()));
        } else {
            self.message = Some(("GridCraft".into(), clean_error(&e)));
        }
    }

    /// Builds the selected sheets' print-ready PDF and hands it to the desktop worker or the
    /// browser. Rendering errors use the usual alert; asynchronous spooler errors stay visible
    /// on the Print page. The engine command still returns PDF bytes without submitting.
    pub fn print_now(&mut self, params: Json) {
        if let Err(error) = self.submit_print(params) {
            self.alert(error);
        }
    }

    fn submit_print(&mut self, params: Json) -> Result<Json, String> {
        if self.services.print_native.is_some() && !self.printing.ready() {
            return Err("Select an available printer in File > Print before submitting.".into());
        }
        let pdf = self.run("file.print", params)?;
        let encoded = pdf.get("base64").and_then(Json::as_str).ok_or("The print command did not produce a PDF.")?;
        let bytes = gridcraft_engine::io::base64_decode(encoded).ok_or("Couldn't decode the generated PDF.")?;
        let len = bytes.len();
        if let Some(submit) = &self.services.print_native {
            self.printing.begin(submit(bytes, self.printing.settings.clone())).map_err(|_| "A print job is already being submitted.".to_owned())?;
            self.print_feedback_seen = None;
        } else if let Some(print) = &self.services.print {
            print(&bytes);
        } else {
            return Err("Printing is unavailable here; export a PDF instead.".into());
        }
        Ok(json!({"printing": true, "bytes": len, "pending": self.services.print_native.is_some()}))
    }

    /// UI-only commands (dialogs, view toggles that aren't saved in the file).
    fn run_ui_command(&mut self, id: &str, p: &Json) -> Option<Result<Json, String>> {
        let r = match id {
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
        for req in self.session.take_ui_requests() {
            match req {
                UiRequest::Dialog(name, params) => self.open_dialog(&name, params),
                UiRequest::Message(m) => self.message = Some(("GridCraft".into(), m)),
                UiRequest::EditCell(text) => {
                    let text = text.map(|text| {
                        self.session
                            .active()
                            .map(|d| formula_locale::display_template(&text, self.ui.language.formula_locale(), &d.wb, d.wb.active_sheet))
                            .unwrap_or(text)
                    });
                    self.begin_edit(text, false);
                }
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

    pub fn view_key(&self) -> Option<(u64, usize)> {
        self.session.active().map(|d| (d.uid, d.wb.active_sheet))
    }

    pub fn view(&self) -> SheetView {
        self.view_key().and_then(|k| self.views.get(&k).copied()).unwrap_or_else(|| self.saved_view())
    }

    pub fn view_mut(&mut self) -> Option<&mut SheetView> {
        let k = self.view_key()?;
        let saved = self.saved_view();
        Some(self.views.entry(k).or_insert(saved))
    }

    /// The view a sheet opens with: scrolled to the file's saved first visible cell (`topLeftCell`).
    fn saved_view(&self) -> SheetView {
        let Some(sh) = self.session.active().and_then(|d| d.wb.active()) else { return SheetView::default() };
        if sh.view != gridcraft_engine::model::SheetViewMode::Normal {
            return SheetView::default();
        }
        let (fr, fc) = sh.freeze.unwrap_or((0, 0));
        let top = sh.view_top_left;
        let y = sh.row_top(top.row.max(fr)) - sh.row_top(fr);
        let x = sh.col_left(top.col.max(fc)) - sh.col_left(fc);
        SheetView { scroll: egui::vec2(x.max(0.0) as f32, y.max(0.0) as f32) }
    }

    /// Hands the active document's scroll positions (first visible cell per sheet) and active
    /// cell to the engine so a save writes them (`sheetView topLeftCell`). Normal view only:
    /// Page Layout view scrolls over pages, not cells.
    pub fn store_view_state(&mut self) {
        let Some(d) = self.session.active_mut() else { return };
        let mut tops = Vec::new();
        for (&(uid, si), v) in &self.views {
            let Some(sh) = d.wb.sheets.get(si).filter(|sh| uid == d.uid && sh.view == gridcraft_engine::model::SheetViewMode::Normal) else {
                continue;
            };
            let (fr, fc) = sh.freeze.unwrap_or((0, 0));
            // A hair past the edge so a row that starts exactly at the scroll offset is the one found.
            let row = sh.row_at(sh.row_top(fr) + v.scroll.y.max(0.0) as f64 + 0.01);
            let col = sh.col_at(sh.col_left(fc) + v.scroll.x.max(0.0) as f64 + 0.01);
            tops.push((si, CellRef::new(row, col)));
        }
        d.store_view(&tops);
    }

    /// Starts editing the active cell. `text` replaces the content (typing) or pre-fills it.
    pub fn begin_edit(&mut self, text: Option<String>, from_formula_bar: bool) {
        // The source formula is display-only on spill children. Grid replacement keeps its
        // existing behavior, but no formula-bar entry path may edit that displayed formula.
        if from_formula_bar && formula_bar::active_spill_anchor(self).is_some() {
            return;
        }
        if let Err(e) = self.commit_text_box_edit() {
            self.message = Some(("Text Box".into(), clean_error(&e)));
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
        let locale = self.ui.language.formula_locale();
        let current = if sh.cell_pictures.contains_key(&at) {
            String::new()
        } else {
            match sh.cell(at) {
                Some(c) if c.formula.is_some() => formula_locale::display(&c.input_text(), locale, &d.wb, d.wb.active_sheet),
                _ => gridcraft_engine::display::edit_text(&d.wb, sh, at),
            }
        };
        let (text, replace) = match text {
            Some(t) => (t, true),
            None => (current.clone(), false),
        };
        let mut ed = editor::EditState::new(d.wb.active_sheet, at, text, replace, from_formula_bar);
        ed.locale = locale;
        if replace && self.ui.autocomplete && !ed.is_formula() {
            ed.completion = editor::column_completion(sh, at, &ed.text);
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
        let text = match self.session.active().map(|d| formula_locale::canonical(&text, ed.locale, &d.wb, ed.sheet, ed.cell)) {
            Some(Ok(text)) => text,
            Some(Err(error)) => {
                self.message = Some(("GridCraft".into(), error.to_string()));
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                return false;
            }
            None => text,
        };
        // Data validation.
        if let Some(d) = self.session.active()
            && let Some((dv, msg)) = gridcraft_engine::cmd::data::check_validation(&d.wb, ed.sheet, ed.cell, &text)
        {
            if dv.error_style == gridcraft_engine::model::ErrorStyle::Stop {
                let title = if dv.error_title.is_empty() { "GridCraft".to_string() } else { dv.error_title.clone() };
                self.message = Some((title, msg));
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                return false;
            }
            self.toast = Some((msg, now_ms()));
        }
        // Point mode can leave another sheet active; the edit belongs to the sheet it started on.
        if self.session.active().is_some_and(|d| d.wb.active_sheet != ed.sheet)
            && let Err(e) = self.session.run("sheet.activate", json!({"sheet": ed.sheet}))
        {
            self.message = Some(("GridCraft".into(), clean_error(&e)));
            self.editor = Some(ed);
            self.session.mode = gridcraft_engine::Mode::Edit;
            return false;
        }
        // Committing nothing into an empty cell changes nothing: no undo step (#1855, Backspace has
        // already cleared the cell as its own step).
        let unchanged = !fill_selection
            && !array
            && text.is_empty()
            && self
                .session
                .active()
                .and_then(|d| d.wb.sheet(ed.sheet))
                .is_some_and(|sh| sh.cell(ed.cell).is_none_or(|c| c.formula.is_none() && c.value == gridcraft_engine::core::Value::Empty));
        let r = if unchanged {
            Ok(json!({}))
        } else if fill_selection {
            self.session.run("range.fill", json!({"input": text}))
        } else {
            self.session.run("cell.set", json!({"cell": ed.cell.a1(), "input": text, "array": array}))
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
                self.message = Some(("GridCraft".into(), clean_error(&e)));
                self.editor = Some(ed);
                self.session.mode = gridcraft_engine::Mode::Edit;
                false
            }
        }
    }

    /// Commits the edit with an arrow key. The arrow is an ordinary move, so it ends a run of Tabs
    /// instead of continuing it (Right) or going back to its column like Enter (Down), as in Excel.
    pub fn commit_edit_arrow(&mut self, dr: i64, dc: i64) -> bool {
        let run = self.tab_run.take();
        let ok = self.commit_edit(dr, dc, false, false);
        self.tab_run = if ok { None } else { run };
        ok
    }

    pub fn cancel_edit(&mut self) {
        self.editor = None;
        self.session.mode = gridcraft_engine::Mode::Ready;
    }

    /// Enter/Tab move inside a multi-cell selection, otherwise to the next cell.
    pub fn move_after_enter(&mut self, dr: i64, dc: i64) {
        let multi = self.session.active().is_some_and(|d| !d.selection.is_single_cell());
        // Any other move (arrows, clicks, another sheet) leaves the selection off the cell the
        // last Tab landed on, which ends the run.
        let here = self.session.active().map(|d| (d.uid, d.wb.active_sheet, d.selection.active));
        let run = self.tab_run.take().filter(|&(uid, sheet, _, at)| here == Some((uid, sheet, at)));
        if multi {
            let fwd = dr > 0 || dc > 0;
            let _ = self.session.run("selection.next", json!({"forward": fwd, "byRow": dc != 0}));
        } else if let (Some((_, _, col, at)), 1, 0) = (run, dr, dc) {
            let back = i64::from(col) - i64::from(at.col);
            let _ = self.session.run("selection.move", json!({"dr": 1, "dc": back}));
        } else {
            let _ = self.session.run("selection.move", json!({"dr": dr, "dc": dc}));
            if dr == 0 && dc != 0 {
                let start = run.map(|r| r.2);
                self.tab_run = self.session.active().and_then(|d| {
                    let (uid, sheet) = (d.uid, d.wb.active_sheet);
                    let col = start.or(here.map(|h| h.2.col))?;
                    Some((uid, sheet, col, d.selection.active))
                });
            }
        }
        self.grid.ensure_visible = true;
    }

    /// Terminal feedback also reaches the normal window when printing starts through control.
    pub fn poll_printing(&mut self, ctx: &egui::Context) {
        self.printing.poll(ctx);
        if self.printing.message == self.print_feedback_seen {
            return;
        }
        self.print_feedback_seen = self.printing.message.clone();
        let Some(message @ (printing::Feedback::Submitted(_) | printing::Feedback::Failed(_) | printing::Feedback::WorkerStopped)) =
            self.printing.message.clone()
        else {
            return;
        };
        let text = printing::feedback(&message, self.ui.language, |label| self.ui.language.tr(label).to_owned());
        if matches!(message, printing::Feedback::Submitted(_)) {
            self.toast = Some((text, now_ms()));
        } else {
            self.alert(text);
        }
    }

    /// Holds back a window close (the close button, Alt+F4, the OS quitting the app) while a
    /// workbook has unsaved changes, and asks about them first.
    fn guard_window_close(&mut self, ctx: &egui::Context) {
        if self.allow_close || !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        // A cell still being edited is part of the workbook, as for File › Close.
        if !self.commit_edit(0, 0, false, false) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        if !self.session.documents().iter().any(|d| d.is_dirty()) {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        match self.dialog.as_mut() {
            // Already asking about a workbook (File › Close): answering it now goes on to quit.
            Some(d) if d.name() == "saveChanges" => {
                d.values.insert("quit".into(), json!(true));
            }
            _ => self.continue_quit(ctx),
        }
    }

    /// Asks about the next unsaved workbook before quitting, or closes the window once none is left.
    pub(crate) fn continue_quit(&mut self, ctx: &egui::Context) {
        match self.session.documents().iter().position(|d| d.is_dirty()) {
            Some(i) => {
                self.session.set_active(i);
                let title = self.session.active().map(|d| d.display_title()).unwrap_or_default();
                self.open_dialog("saveChanges", json!({"title": title, "quit": true}));
            }
            None => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Per-frame logic (control channel, screenshots). Call before `ui`.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if self.session.text_width.is_none() {
            let ctx = ctx.clone();
            self.session.text_width = Some(std::sync::Arc::new(move |text, font, exact| theme::cell_text_width(&ctx, text, font, exact)));
        }
        // A keytip control request lives one frame: the ribbon consumes it during `ui`, and
        // anything that never reached a control (collapsed ribbon, tab switch) is dropped.
        self.ui.keytip_invoke = None;
        // Floating pictures' textures go with their workbook, even when no sheet is painted.
        let open: Vec<u64> = self.session.documents().iter().map(|d| d.uid).collect();
        floating_pictures::forget_closed(ctx, &open);
        if self.fonts_han.is_some_and(|han| han != theme::HanOrder::of(self.ui.language)) {
            self.fonts_ready = false;
            self.fonts_set = false;
        }
        if !self.fonts_ready {
            // New fonts apply from the next frame on: paint nothing until then.
            if self.fonts_set {
                self.fonts_ready = true;
            } else {
                SheetApp::setup_context_for_language(ctx, self.ui.dark, self.ui.language);
                self.fonts_set = true;
                // The rebuilt set has no workbook fonts yet: bind them again once it is active.
                self.fonts_bound.clear();
                self.fonts_seen = (0, 0);
                self.fonts_han = Some(theme::HanOrder::of(self.ui.language));
                ctx.request_repaint();
            }
        } else if self.fonts_rebind {
            // A font-set change made last frame is active now.
            self.fonts_rebind = false;
        } else if fonts::scan_done() {
            // Bind the workbook's installed fonts once the background scan has finished. Adding
            // fonts re-applies the font set, which egui activates at the next pass, so skip this
            // frame's paint (same one-frame cost as startup). Only when the workbook changed.
            let seen = self.session.active().map(|d| (d.uid, d.revision)).unwrap_or((0, 0));
            if seen != self.fonts_seen {
                let used = self.used_fonts();
                self.fonts_seen = seen;
                if used != self.fonts_bound {
                    ctx.set_fonts(theme::font_definitions_with(self.ui.language, &used));
                    self.fonts_bound = used;
                    self.fonts_rebind = true;
                    ctx.request_repaint();
                }
            }
        }
        if let Some(answer) = self.desktop_theme.take_change() {
            self.desktop_dark = Some(answer);
        }
        let preference = match (
            self.ui.system_theme,
            ctx.system_theme().or_else(|| self.desktop_dark.map(|d| if d { egui::Theme::Dark } else { egui::Theme::Light })),
        ) {
            (true, Some(egui::Theme::Dark)) => egui::ThemePreference::Dark,
            (true, Some(egui::Theme::Light)) => egui::ThemePreference::Light,
            _ => self.ui.theme_preference(),
        };
        if ctx.options(|o| o.theme_preference) != preference {
            ctx.set_theme(preference);
            ctx.request_repaint();
        }
        self.effective_dark = ctx.theme() == egui::Theme::Dark;
        // Interface size: the preference owns the zoom factor, so egui's own Ctrl/Cmd +/-/0 zoom
        // keys stay off (they would also swallow GridCraft's shortcuts).
        if ctx.options(|o| o.zoom_with_keyboard) {
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
        }
        // egui closes the window on Ctrl+Q (and with Shift or Alt held too) without asking to save
        // (#1443). Excel for Windows has no such key (Ctrl+Q is Quick Analysis); macOS keeps Cmd+Q,
        // the app's Quit there.
        if ctx.os() != egui::os::OperatingSystem::Mac && ctx.options(|o| !o.quit_shortcuts.is_empty()) {
            ctx.options_mut(|o| o.quit_shortcuts.clear());
        }
        let zoom = self.ui.zoom_factor();
        if (ctx.zoom_factor() - zoom).abs() > 0.001 {
            ctx.set_zoom_factor(zoom);
            ctx.request_repaint();
        }
        control::poll(self, ctx);
        self.guard_window_close(ctx);
        // Files read asynchronously (web file picker, dropped files).
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
        if self.session.active().is_none() {
            self.session.new_workbook();
        }
    }

    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        if let Some(mut batch) = self.synthetic.pop_front() {
            raw.events.append(&mut batch);
        }
    }

    /// Flip AutoSave (title-bar switch / Options ▸ Save). Persisted with the other UI
    /// preferences, so it stays as the user left it across restarts (#1901).
    pub fn toggle_autosave(&mut self) {
        self.ui.autosave = !self.ui.autosave;
    }

    /// Lays out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll_printing(&ctx);
        if !self.fonts_ready || self.fonts_rebind {
            ctx.request_repaint();
            return;
        }
        let t0 = now_ms();
        text_box::before_ui(self, &ctx);
        // The language the widgets translate with this frame (see `i18n::current`).
        i18n::set_current(&ctx, self.ui.language);
        let t = theme::Tokens::get(&ctx);
        // A host can turn the File tab off while Backstage is already open: close it.
        if !self.chrome.file_tab {
            self.backstage = None;
        }
        // Keytips opened the File view (Alt, F): its letters keep working there.
        if self.backstage.is_none() || self.keytips.prefix().is_some() {
            self.ribbon_keys(&ctx);
        }
        if self.chrome.title_bar {
            ribbon::title_bar(self, ui);
        }
        if self.backstage.is_some() {
            // File tab: the Backstage view replaces everything below the title bar.
            backstage::show(self, ui);
        } else {
            ribbon::show(self, ui);
            widgets::notice_bar(self, ui);
            if self.session.prefs.formula_bar {
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
        }
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
        if self.ui.autosave
            && self.editor.is_none()
            && self.text_box_editor.is_none()
            && now - self.grid.last_autosave > 2000.0
            && self.session.active().is_some_and(|d| d.is_dirty() && d.path.as_deref().is_some_and(|p| p.ends_with(".xlsx")))
        {
            self.grid.last_autosave = now;
            self.store_view_state();
            let _ = self.session.run("file.save", json!({}));
        }
        // Window title.
        if let Some(d) = self.session.active() {
            let title = format!("{}{}", d.display_title(), if d.is_dirty() { " — Edited" } else { "" });
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
            && self.grid.object_menu.is_none()
            && self.grid.header_menu.is_none()
            && self.grid.fill_menu.is_none()
            && self.grid.filter_menu.is_none()
            && self.grid.list_picker.is_none()
            && self.grid.renaming_tab.is_none()
            && (before.is_some() || !egui::Popup::is_any_open(ctx));
        let actions = self.keytips.process(ctx, enabled);
        // Backing out of F to the tab letters, or finishing an F sequence, closes the File view
        // that Alt, F opened: the command runs on the sheet, as it did without the view.
        if self.backstage.is_some() && before.is_some() && (self.keytips.prefix() == Some("") || !actions.is_empty()) {
            self.backstage = None;
        }
        if before.is_some() && before.as_deref() != self.keytips.prefix() && !ctx.input(|i| i.pointer.any_pressed()) {
            // Leave pointer cancellation to the popup so clicking a menu item still works.
            egui::Popup::close_all(ctx);
        }
        for action in actions {
            match action {
                keytips::Action::Tab("File") => self.open_backstage(backstage::Page::Home),
                keytips::Action::Tab(tab) => {
                    self.run_or_alert("ui.ribbonTab", json!({"tab": tab}));
                    self.ui.ribbon_collapsed = false;
                }
                keytips::Action::Command(id @ ("edit.copy" | "edit.cut")) => self.copy_to_clipboard(ctx, id),
                keytips::Action::Command("edit.paste") => self.paste_from_clipboard(),
                keytips::Action::Command(id) => self.run_or_alert(id, json!({})),
                keytips::Action::CommandWith(id, params) => {
                    let params: Json = serde_json::from_str(params).unwrap_or_else(|_| json!({}));
                    // A Borders preset picked by key tip becomes the split button's face, as a
                    // click on the same menu item does.
                    if id == "home.borders"
                        && let Some(preset) = params.get("preset").and_then(Json::as_str)
                    {
                        self.grid.last_border = preset.to_string();
                    }
                    self.run_or_alert(id, params);
                }
                keytips::Action::Dialog(name) => self.open_dialog(name, json!({})),
                keytips::Action::DialogWith(name, params) => self.open_dialog(name, serde_json::from_str(params).unwrap_or_else(|_| json!({}))),
                // Reserved sequence: fires silently while its command is missing. When the
                // command lands in the catalog it goes live with no table edit.
                keytips::Action::Pending(id) => {
                    if gridcraft_engine::find_command(id).is_some() {
                        self.run_or_alert(id, json!({}));
                    }
                }
                // Route to the ribbon control that owns this keytip: it opens its dropdown or
                // runs its click handler with the right params (the engine command alone would
                // be missing them).
                keytips::Action::Control(id) => self.ui.keytip_invoke = Some(id),
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
                    Some((s, b)) => (Some(gridcraft_engine::core::unquote_sheet(s)), b.to_string()),
                    None => (None, target.clone()),
                };
                if sheet.as_deref().is_none_or(|s| s.eq_ignore_ascii_case(&sh.name)) && RangeRef::parse(&body) == Some(cur) {
                    return n.name.clone();
                }
            }
        }
        if self.grid.drag_select && !cur.is_single() {
            return format!("{}R x {}C", cur.height(), cur.width());
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

pub fn now_ms() -> f64 {
    gridcraft_engine::core::date::unix_now_ms().unwrap_or(0.0)
}

pub fn cell(r: u32, c: u32) -> CellRef {
    CellRef::new(r, c)
}
