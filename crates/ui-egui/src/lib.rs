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
pub mod code_editor;
pub mod control;
mod craft_fonts_policy;
pub mod credits;
pub mod dialogs;
#[cfg(not(target_arch = "wasm32"))]
pub mod doc_windows;
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
#[cfg(not(target_arch = "wasm32"))]
mod updates_ui;
pub mod widgets;
mod window_ui;

pub use window_ui::WindowUi;

use std::collections::HashMap;

use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::{Session, UiRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

pub use control::{ControlRequest, ControlResponse};

mod app;
mod clock;
mod services;
mod state;

pub use clock::{monotonic_ms, now_ms};
pub use services::{Chrome, Inbox, Services};
pub use state::{NoticeKind, OpenWindow, SaveAnswer, SheetView};
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

/// A dismissible bar under the ribbon, in the spirit of Excel's message bar. Used to tell the
/// user what an opened file carried that GridCraft handled differently (e.g. preserved macros).
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    pub kind: NoticeKind,
    /// Optional button: `(label, dialog name)` — opens that dialog when clicked.
    pub action: Option<(String, String)>,
}

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
    /// The window chrome to draw (see [`Chrome`]).
    pub chrome: Chrome,
    /// Scroll position per (window id, sheet index).
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
    /// Synthetic input from agents, one batch per step (drags need several steps).
    pub synthetic: std::collections::VecDeque<Vec<egui::Event>>,
    /// The front synthetic batch was injected into `RawInput` but no `ui` pass has applied it
    /// yet. eframe retains that unconsumed input across hidden logic-only ticks; the flag makes
    /// the hook offer it once and asks for a hidden paint until a UI pass applies it.
    pub(crate) synthetic_pending: bool,
    /// `cumulative_pass_nr` when the front batch was last injected. A completed egui pass that
    /// did not apply it (the font gate's early return) consumed the events with `begin_pass`
    /// without reaching input handling, so the hook must offer the same step again.
    pub(crate) synthetic_injected_at_pass: Option<u64>,
    pub control_rx: Option<std::sync::mpsc::Receiver<ControlRequest>>,
    pub grid: grid::GridState,
    pub perf: Perf,
    pub fonts_ready: bool,
    fonts_set: bool,
    /// Test harness set: rebuilds use the deterministic pinned font set instead of the host scan.
    prepared_fonts: bool,
    /// Workbook fonts bound into the current font set; a change rebinds (installed fonts only).
    fonts_bound: Vec<String>,
    /// The open workbooks' (uid, revision), folded, that `fonts_bound` was computed for, so the
    /// style tables are walked only when a workbook changes.
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
    /// Monotonic epoch of [`control::CaptureGeometry`]: any resize/DPI/zoom/surface change bumps
    /// it, including a change back to an earlier value, so a capture taken in another layout is
    /// never saved against the current geometry.
    pub(crate) capture_epoch: u64,
    /// The geometry `capture_epoch` describes; `None` before the first frame.
    pub(crate) capture_geometry: Option<control::CaptureGeometry>,
    /// The recorded geometry came from a native `RawInput` frame (see `raw_input_hook`). When
    /// false (a bare test/embedder that never runs the hook) capture code falls back to egui's
    /// current input instead of reusing a stale recorded value.
    pub(crate) capture_geometry_from_raw: bool,
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
    /// View › Switch Windows opens its list on the next frame the View tab draws (asked for by a
    /// key tip, the command search or a menu, possibly while another tab was showing).
    pub(crate) open_switch_windows: bool,
    /// The window whose interface state the per-window fields above hold (see [`WindowUi`]).
    ui_window: Option<u64>,
    /// The other windows' interface state, by window id.
    parked: HashMap<u64, WindowUi>,
    /// Each window is a native window of its own ([`doc_windows`] on the desktop): a window's
    /// close button closes that window, and quitting is asked for separately. Without it (web,
    /// tests, embedding hosts) one window shows the active workbook window, and its close button
    /// quits.
    pub native_windows: bool,
    /// View › Arrange All's layout and windows, waiting for the native windows to move.
    pub arrange_windows: Option<(String, Vec<u64>)>,
    /// The update check, the startup prompt and a running self-update (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    updates_rt: updates_ui::Runtime,
}

/// Strips the engine's error prefixes for message boxes.
pub fn clean_error(e: &str) -> String {
    if let Some(rest) = e.split_once("`: ").map(|(_, r)| r) {
        return rest.to_string();
    }
    e.to_string()
}

pub fn cell(r: u32, c: u32) -> CellRef {
    CellRef::new(r, c)
}
