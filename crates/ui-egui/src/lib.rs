//! GridCraft's egui frontend: an Excel-style window (title bar with Quick Access Toolbar,
//! ribbon, formula bar, grid, sheet tabs, status bar) over `gridcraft-engine`.
//!
//! The UI is thin: it reads engine state and acts through [`SheetApp::run`] (engine commands by
//! id). Everything it can do is also reachable over the control channel ([`control`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// A string in the interface language ([`i18n::t`]).
#[macro_export]
macro_rules! tl {
    ($s:expr) => {
        $crate::i18n::t($s)
    };
}

pub mod chartview;
pub mod control;
pub mod credits;
pub mod dialogs;
pub mod editor;
pub mod formula_bar;
pub mod grid;
pub mod i18n;
pub mod icons;
pub mod panes;
pub mod pivot_pane;
pub mod ribbon;
pub mod tabs;
pub mod theme;
pub mod widgets;

use std::collections::HashMap;

use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::{Session, UiRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

pub use control::{ControlRequest, ControlResponse};

/// Persisted UI preferences.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub dark: bool,
    pub ribbon_tab: String,
    pub ribbon_collapsed: bool,
    pub formula_bar: bool,
    pub formula_bar_expanded: bool,
    pub status_bar: bool,
    pub recent: Vec<String>,
    /// Interface language: `auto` (follow the system) or a code from [`i18n::LANGUAGES`].
    pub language: String,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            dark: false,
            ribbon_tab: "Home".into(),
            ribbon_collapsed: false,
            formula_bar: true,
            formula_bar_expanded: false,
            status_bar: true,
            recent: vec![],
            language: i18n::AUTO.into(),
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
    pub open_url: Option<Box<dyn Fn(&str)>>,
    /// Web: deliver a file to the user (name, bytes).
    pub download: Option<Box<dyn Fn(&str, &[u8])>>,
    /// Web: start an asynchronous file pick; the bytes arrive later in `inbox`.
    pub open_async: Option<Box<dyn Fn()>>,
    /// Files delivered asynchronously (name, bytes), opened on the next frame.
    pub inbox: Option<Inbox>,
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
    pub session: Session,
    pub ui: UiState,
    pub views: HashMap<(u64, usize), SheetView>,
    pub editor: Option<editor::EditState>,
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
    pub name_box: Option<String>,
    pub(crate) shots: control::Shots,
    /// Chart selected on the sheet (id).
    pub selected_chart: Option<u32>,
    pub started: f64,
}

impl SheetApp {
    pub fn new(session: Session, services: Services) -> SheetApp {
        SheetApp {
            session,
            ui: UiState::default(),
            views: HashMap::new(),
            editor: None,
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
            name_box: None,
            shots: control::Shots::default(),
            selected_chart: None,
            started: now_ms(),
        }
    }

    /// One-time context setup: fonts and visuals.
    pub fn setup_context(ctx: &egui::Context, dark: bool) {
        ctx.set_fonts(theme::font_definitions());
        theme::apply(ctx, dark);
    }

    /// Runs an engine command (or a UI command) and handles UI requests it makes.
    pub fn run(&mut self, id: &str, params: Json) -> Result<Json, String> {
        if let Some(r) = self.run_ui_command(id, &params) {
            return r;
        }
        let r = self.session.run(id, params);
        self.after_engine();
        r
    }

    /// Runs a command and shows its error in a message box (for menu/ribbon clicks).
    pub fn run_or_alert(&mut self, id: &str, params: Json) {
        if let Err(e) = self.run(id, params) {
            if e.contains("is not available right now") {
                self.toast = Some((e, now_ms()));
            } else {
                self.message = Some(("GridCraft".into(), clean_error(&e)));
            }
        }
    }

    /// UI-only commands (dialogs, view toggles that aren't saved in the file).
    fn run_ui_command(&mut self, id: &str, p: &Json) -> Option<Result<Json, String>> {
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
                self.ui.dark = p.get("on").and_then(Json::as_bool).unwrap_or(!self.ui.dark);
                Ok(json!({"on": self.ui.dark}))
            }
            "view.zoom100" => return Some(self.session.run("view.zoom", json!({"percent": 100})).inspect(|_| self.after_engine())),
            "ui.dialog" => {
                let name = p.get("name").and_then(Json::as_str).unwrap_or("");
                self.open_dialog(name, p.clone());
                Ok(json!({"dialog": self.dialog.as_ref().map(|d| d.name())}))
            }
            "ui.message.dismiss" => {
                self.message = None;
                Ok(Json::Null)
            }
            "ui.language" => {
                if let Some(v) = p.get("value").and_then(Json::as_str) {
                    match i18n::normalize_pref(v) {
                        Some(code) => self.ui.language = code.to_string(),
                        None => {
                            let codes: Vec<&str> = i18n::Lang::all().map(i18n::Lang::code).collect();
                            return Some(Err(format!("unknown language `{v}`; use auto or one of {}", codes.join(", "))));
                        }
                    }
                }
                let lang = i18n::Lang::from_pref(&self.ui.language);
                let available: Vec<Json> = i18n::Lang::all().map(|l| json!({"code": l.code(), "name": l.name()})).collect();
                Ok(json!({"language": self.ui.language, "effective": lang.code(), "available": available}))
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
                UiRequest::EditCell(text) => self.begin_edit(text, false),
                UiRequest::OpenUrl(u) => {
                    if let Some(f) = &self.services.open_url {
                        f(&u);
                    }
                }
            }
        }
    }

    pub fn open_dialog(&mut self, name: &str, params: Json) {
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
        match self.session.run("file.open", json!({"path": path})) {
            Ok(_) => {
                self.ui.recent.retain(|p| p != path);
                self.ui.recent.insert(0, path.to_string());
                self.ui.recent.truncate(20);
            }
            Err(e) => self.message = Some(("GridCraft".into(), clean_error(&e))),
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

    /// Starts editing the active cell. `text` replaces the content (typing) or pre-fills it.
    pub fn begin_edit(&mut self, text: Option<String>, from_formula_bar: bool) {
        let Some(d) = self.session.active() else { return };
        let Some(sh) = d.wb.active() else { return };
        let at = d.selection.active;
        let current = sh.cell(at).map(|c| c.input_text()).unwrap_or_default();
        let (text, replace) = match text {
            Some(t) => (t, true),
            None => (current.clone(), false),
        };
        let mut ed = editor::EditState::new(d.wb.active_sheet, at, text, replace, from_formula_bar);
        if replace && !ed.is_formula() {
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
        let r = if fill_selection {
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

    /// New workbooks follow the interface language, as in Excel: a German interface makes
    /// `Mappe1` with `Tabelle1`. The first workbook exists before the language is known, so while
    /// nobody has touched it, it is renamed too.
    fn follow_language(&mut self, lang: i18n::Lang) {
        let new = gridcraft_engine::lang::Lang::from_tag(lang.code());
        let old = self.session.lang;
        if new == old {
            return;
        }
        self.session.lang = new;
        let Some(d) = self.session.active_mut() else { return };
        let untouched = d.path.is_none() && !d.is_dirty() && d.undo.is_empty();
        let Some(n) = d.title.strip_prefix(old.book_base()).filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) else { return };
        if !untouched {
            return;
        }
        d.title = format!("{}{n}", new.book_base());
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        for i in 0..wb.sheets.len() {
            if let Some(sh) = wb.sheet_mut(i)
                && let Some(k) = sh.name.strip_prefix(old.sheet_base()).filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_digit()))
                && sh.used_range().is_none()
            {
                sh.name = format!("{}{k}", new.sheet_base());
            }
        }
    }

    /// Per-frame logic (control channel, screenshots). Call before `ui`.
    pub fn logic(&mut self, ctx: &egui::Context) {
        let lang = i18n::Lang::from_pref(&self.ui.language);
        i18n::set_current(lang);
        self.follow_language(lang);
        if !self.fonts_ready {
            // New fonts apply from the next frame on: paint nothing until then.
            if self.fonts_set {
                self.fonts_ready = true;
            } else {
                SheetApp::setup_context(ctx, self.ui.dark);
                self.fonts_set = true;
                ctx.request_repaint();
            }
        }
        if ctx.global_style().visuals.dark_mode != self.ui.dark {
            theme::apply(ctx, self.ui.dark);
        }
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
            if let Err(e) = self.session.run("file.open", json!({"name": name, "base64": b64})) {
                self.message = Some(("GridCraft".into(), clean_error(&e)));
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
        i18n::set_current(i18n::Lang::from_pref(&self.ui.language));
        if !self.fonts_ready {
            ctx.request_repaint();
            return;
        }
        let t0 = now_ms();
        let t = theme::Tokens::get(&ctx);
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
            && now - self.grid.last_autosave > 2000.0
            && self.session.active().is_some_and(|d| d.is_dirty() && d.path.as_deref().is_some_and(|p| p.ends_with(".xlsx")))
        {
            self.grid.last_autosave = now;
            let _ = self.session.run("file.save", json!({}));
        }
        // Window title.
        if let Some(d) = self.session.active() {
            let title = if d.is_dirty() { format!("{} — {}", d.display_title(), tl!("Edited")) } else { d.display_title() };
            if self.grid.last_title.as_deref() != Some(title.as_str()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
                self.grid.last_title = Some(title);
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
