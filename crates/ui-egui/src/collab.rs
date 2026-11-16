//! The UI side of a shared session: where other participants are looking, how the connection is
//! doing, and the one place a user command can be taken over by the collaboration layer.
//!
//! The transport and the local document live in `gridcraft-collab-client`, which the desktop app
//! links; this crate knows only this trait, so the web build and single-user builds carry no
//! network code.

use egui::{Color32, Painter, Stroke, StrokeKind, Ui};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use serde_json::Value as Json;

use crate::SheetApp;
use crate::grid::geo::Geo;
use crate::grid::selection::range_in_pane;
use gridcraft_engine::model::Sheet;

/// Where one other participant is: the sheet (by name), the active cell and the selected areas.
///
/// A participant is a window (RFC §13.6): one person with two windows is two views, both with the
/// same `actor` and a different `window_id`.
#[derive(Clone, Debug, PartialEq)]
pub struct PresenceView {
    pub actor: String,
    pub window_id: String,
    pub name: String,
    pub color: Color32,
    pub sheet: String,
    pub active: CellRef,
    pub ranges: Vec<RangeRef>,
}

impl PresenceView {
    /// A stable colour per actor, so the same person keeps it for the whole session.
    pub fn colour_for(actor: &str) -> Color32 {
        const PALETTE: [Color32; 8] = [
            Color32::from_rgb(0x1E, 0x88, 0xE5),
            Color32::from_rgb(0xE5, 0x39, 0x35),
            Color32::from_rgb(0x43, 0xA0, 0x47),
            Color32::from_rgb(0xFB, 0x8C, 0x00),
            Color32::from_rgb(0x8E, 0x24, 0xAA),
            Color32::from_rgb(0x00, 0xAC, 0xC1),
            Color32::from_rgb(0xD8, 0x1B, 0x60),
            Color32::from_rgb(0x6D, 0x4C, 0x41),
        ];
        let mut hash = 0u32;
        for byte in actor.as_bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(u32::from(*byte));
        }
        PALETTE[(hash as usize) % PALETTE.len()]
    }
}

/// How the shared session is doing, for the status bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum SyncStatus {
    /// Collaboration is compiled in but not configured.
    #[default]
    Off,
    Connecting,
    Online {
        version: u64,
        participants: usize,
    },
    Error(String),
}

/// One participant as the host bridge reports them (`session-state`/`presence`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionParticipant {
    pub client_id: String,
    pub role: String,
    pub color: Option<String>,
}

/// What the collaboration layer does for the UI.
pub trait Collab {
    /// Who is in the session. The web shell sends it to the cabinet; a
    /// desktop session (no host) leaves the default.
    fn session_participants(&self) -> Vec<SessionParticipant> {
        Vec::new()
    }
    /// Takes a command over when it travels in the session. `None` runs it locally as usual.
    fn execute(&mut self, session: &mut Session, id: &str, params: Json) -> Option<Result<Json, String>>;
    /// Delivers what arrived since the last frame and returns where the others are.
    fn poll(&mut self, session: &mut Session) -> Vec<PresenceView>;
    fn status(&self) -> SyncStatus;
    /// A viewer's document: the ribbon and the editor are disabled.
    fn read_only(&self) -> bool {
        false
    }
    /// A refusal of an undo/redo to show once (the room's reason, never cell contents).
    fn take_notice(&mut self) -> Option<String> {
        None
    }
    /// Replaces the session credential without opening a new session. The host emits
    /// `gridcraft:session` again when the cabinet renews the token; a transport that can swap the
    /// JWT in place keeps its unconfirmed operations, and one that cannot says nothing.
    fn renew_token(&mut self, _token: &str) {}
}

impl SheetApp {
    /// Drains the collaboration events once per frame (before painting, after input).
    pub fn poll_collab(&mut self) {
        let SheetApp { collab, session, collab_views, collab_status, .. } = self;
        if let Some(collab) = collab.as_mut() {
            *collab_views = collab.poll(session);
            *collab_status = collab.status();
            if let Some(note) = collab.take_notice() {
                let SheetApp { message, .. } = self;
                *message = Some(("GridCraft".into(), note));
            }
        }
    }

    /// Whether the session is read-only for this participant.
    pub fn collab_read_only(&self) -> bool {
        self.collab.as_ref().is_some_and(|collab| collab.read_only())
    }
}

/// Paints other participants' selections on the sheet being drawn, above the local selection.
pub(crate) fn paint_presence(p: &Painter, geo: &Geo, sheet: &Sheet, app: &SheetApp, rows: (u32, u32), cols: (u32, u32)) {
    for view in &app.collab_views {
        if view.sheet != sheet.name {
            continue;
        }
        for range in &view.ranges {
            if !range_in_pane(*range, rows, cols) {
                continue;
            }
            let rect = geo.range_rect(sheet, *range);
            p.rect_filled(rect, 0.0, view.color.gamma_multiply(0.08));
            p.rect_stroke(rect, 0.0, Stroke::new(1.5, view.color), StrokeKind::Inside);
        }
        let active = geo.cell_rect(sheet, view.active);
        p.rect_stroke(active, 0.0, Stroke::new(2.0, view.color), StrokeKind::Outside);
    }
}

/// The status-bar segment: the connection state and who is here.
pub(crate) fn status_segment(app: &SheetApp, ui: &mut Ui, dim: Color32) {
    let Some(status) = app.collab.as_ref().map(|collab| collab.status()) else { return };
    let (label, colour) = match status {
        SyncStatus::Off => return,
        SyncStatus::Connecting => ("Connecting…".to_string(), dim),
        SyncStatus::Online { version, participants } => (format!("Live · v{version} · {participants}"), Color32::from_rgb(0x2E, 0x7D, 0x32)),
        SyncStatus::Error(e) => (format!("Offline: {e}"), Color32::from_rgb(0xC6, 0x28, 0x28)),
    };
    ui.add_space(12.0);
    ui.add(egui::Label::new(egui::RichText::new(label).font(crate::theme::ui_font(12.5)).color(colour)).selectable(false));
    for view in &app.collab_views {
        ui.add_space(8.0);
        ui.add(egui::Label::new(egui::RichText::new("●").font(crate::theme::ui_font(12.5)).color(view.color)).selectable(false));
        ui.add(egui::Label::new(egui::RichText::new(&view.name).font(crate::theme::ui_font(12.5)).color(dim)).selectable(false));
    }
}
