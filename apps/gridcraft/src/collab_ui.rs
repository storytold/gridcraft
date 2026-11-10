//! The desktop app's collaboration host (feature `collab`): connects from the environment,
//! installs the [`Collab`] adapter into the UI and keeps it ticking.
//!
//! Configuration, checked once at start-up:
//! - `GRIDCRAFT_COLLAB_URL` — the session socket, e.g. `wss://sync.example/v1/sessions/<id>/ws`;
//! - `GRIDCRAFT_COLLAB_TOKEN` — the cabinet's signed token (never logged);
//! - `GRIDCRAFT_COLLAB_NAME` — the display name others see (default `User`).
//!
//! Without both the URL and the token nothing is connected and the single-user behaviour is
//! untouched.

use std::sync::Mutex;
use std::time::Duration;

use gridcraft_collab::{Presence, Role, classify};
use gridcraft_collab_client::{Client, Config, Synced, Welcome};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::collab::{Collab, PresenceView, SyncStatus};
use serde_json::Value as Json;

type Pending = std::sync::mpsc::Receiver<Result<(Client, Welcome), String>>;

/// The connection being opened on its own runtime thread, until the first frame installs it.
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// Reads the environment and starts connecting. Returns immediately: the UI shows `Connecting`
/// until the handshake finishes.
pub fn start_from_env() {
    let url = std::env::var("GRIDCRAFT_COLLAB_URL").ok().filter(|s| !s.is_empty());
    let token = std::env::var("GRIDCRAFT_COLLAB_TOKEN").ok().filter(|s| !s.is_empty());
    let (Some(url), Some(token)) = (url, token) else { return };
    let name = std::env::var("GRIDCRAFT_COLLAB_NAME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "User".into());
    // One window id per process, made by the client and kept across reconnects (RFC §13.6).
    let cfg = Config { url, token, name, last_version: 0, reconnect: Some(Duration::from_secs(1)), window_id: String::new() };
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
            let _ = send.send(Err("no asynchronous runtime".into()));
            return;
        };
        runtime.block_on(async move {
            match Client::connect(cfg).await {
                Ok((client, welcome)) => {
                    if send.send(Ok((client, welcome))).is_ok() {
                        // The client spawns its own task on this runtime; keep the runtime alive.
                        std::future::pending::<()>().await;
                    }
                }
                Err(e) => {
                    let _ = send.send(Err(e.to_string()));
                }
            }
        });
    });
    if let Ok(mut slot) = PENDING.lock() {
        *slot = Some(receive);
    }
}

/// One call per frame from `App::logic`: installs the connection when the handshake is done.
pub fn tick(app: &mut SheetApp) {
    let received = PENDING.lock().ok().and_then(|slot| slot.as_ref().and_then(|rx| rx.try_recv().ok()));
    match received {
        Some(Ok((client, welcome))) => {
            let role = welcome.role;
            app.collab = Some(Box::new(Adapter::new(client, &welcome, role)));
            app.collab_status = SyncStatus::Connecting;
            if let Ok(mut slot) = PENDING.lock() {
                *slot = None;
            }
        }
        Some(Err(e)) => {
            app.collab_status = SyncStatus::Error(e);
            if let Ok(mut slot) = PENDING.lock() {
                *slot = None;
            }
        }
        None => {
            if app.collab.is_none() && PENDING.lock().ok().is_some_and(|slot| slot.is_some()) {
                app.collab_status = SyncStatus::Connecting;
            }
        }
    }
}

/// The trait object the UI holds: the client, its mapping to the local sheets and the presence
/// throttle.
struct Adapter {
    synced: Synced,
    role: Role,
    last_presence: f64,
}

impl Adapter {
    fn new(client: Client, welcome: &Welcome, role: Role) -> Adapter {
        Adapter { synced: Synced::new(client, welcome), role, last_presence: 0.0 }
    }

    /// Sends this participant's position at most ten times a second (RFC §5).
    fn publish_presence(&mut self, session: &Session) {
        let now = gridcraft_ui_egui::monotonic_ms();
        if now - self.last_presence < 100.0 {
            return;
        }
        self.last_presence = now;
        let Some(doc) = session.active() else { return };
        let Some(sheet) = doc.wb.sheet(doc.wb.active_sheet()) else { return };
        let Some(key) = self.synced.key_for_name(&sheet.name) else { return };
        let selection = &doc.selection;
        let ranges: Vec<String> = selection.ranges.iter().map(RangeRef::a1).collect();
        self.synced.send_presence(Presence { sheet_key: key, active: selection.active.to_string(), ranges, viewport: None });
    }
}

impl Collab for Adapter {
    fn execute(&mut self, session: &mut Session, id: &str, params: Json) -> Option<Result<Json, String>> {
        // Commands outside the allowlist (files, view, printing, code) stay local, and so does the
        // UI command surface; only what travels in a session is taken over. Undo and redo are the
        // exception inside the allowlist: they are not sent as commands — the room turns them into
        // an operation of its own — and the engine's local history knows nothing about the other
        // replicas, so handing them to it would undo a preview here only.
        if !matches!(id, "edit.undo" | "edit.redo") {
            classify(id)?;
        }
        Some(self.synced.execute(session, id, params).map_err(|e| e.to_string()))
    }

    fn poll(&mut self, session: &mut Session) -> Vec<PresenceView> {
        let events = self.synced.poll(session);
        for event in &events {
            match event {
                gridcraft_collab_client::Event::Disconnected { .. } => {}
                gridcraft_collab_client::Event::Error { message, .. } => {
                    self.last_presence = 0.0;
                    let _ = message;
                }
                _ => {}
            }
        }
        self.publish_presence(session);
        let state = self.synced.state();
        state
            .participants
            .iter()
            // Window, not person: a second window of this user is listed like anybody else's.
            .filter(|participant| participant.actor != state.actor || participant.window_id != state.window_id)
            .filter_map(|participant| {
                let presence = participant.presence.as_ref()?;
                let sheet = state.sheets.iter().find(|sheet| sheet.key == presence.sheet_key)?.name.clone();
                let active = CellRef::parse(&presence.active)?;
                let ranges = presence.ranges.iter().filter_map(|range| RangeRef::parse(range)).collect();
                Some(PresenceView {
                    actor: participant.actor.clone(),
                    window_id: participant.window_id.clone(),
                    name: participant.name.clone(),
                    color: PresenceView::colour_for(&participant.actor),
                    sheet,
                    active,
                    ranges,
                })
            })
            .collect()
    }

    fn status(&self) -> SyncStatus {
        let state = self.synced.state();
        if !state.connected { SyncStatus::Connecting } else { SyncStatus::Online { version: state.version, participants: state.participants.len() } }
    }

    fn read_only(&self) -> bool {
        self.role == Role::Viewer
    }

    fn take_notice(&mut self) -> Option<String> {
        self.synced.take_notice()
    }
}
