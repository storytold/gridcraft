//! An in-process authoritative room: one document, one version counter, one writer.
//!
//! The room is the part of the sync server that must exist before there is a socket: it accepts a
//! normalised [`ClientOp`], deduplicates it by `opId`, runs it through
//! [`gridcraft_engine::Session::apply_remote`], turns the changed cells into the authoritative
//! delta and hands back an [`Applied`]. Structural commands are exclusive (ADR-0004): they must
//! match the head version, and the room bumps `epoch` so a client with old addresses resyncs.

use std::collections::{HashMap, VecDeque};

use gridcraft_engine::Session;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::delta::delta_for;
use crate::keys::SheetKeys;
use crate::ops::{Class, classify};
use crate::protocol::{Applied, ClientOp, CommandEcho, Hint, Kind, Outcome, RejectCode, Rejected};

/// How many applied operations the room remembers for idempotent resends (ADR-0003).
pub const DEDUP_WINDOW: usize = 10_000;

/// A refusal the caller sends to the client as it is.
#[derive(Debug)]
pub enum RoomError {
    Rejected(Rejected),
}

impl RoomError {
    pub fn rejected(&self) -> &Rejected {
        match self {
            RoomError::Rejected(r) => r,
        }
    }
}

/// What a store writes down to restore a room: its counters and its dedup window. The workbook
/// and the sheet keys travel separately (the store owns their format).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoomState {
    pub version: u64,
    pub epoch: u64,
    /// The most recent answers, oldest first. Bounded by [`DEDUP_WINDOW`] when it is written.
    #[serde(default)]
    pub seen: Vec<Applied>,
}

/// The authoritative state of one shared document.
pub struct Room {
    keys: SheetKeys,
    version: u64,
    epoch: u64,
    seen: HashMap<String, Applied>,
    order: VecDeque<String>,
}

impl Room {
    /// A room over the session's active workbook. The actor of each operation comes with the
    /// operation: one room may serve several participants only through the hub, but a test drives
    /// it directly.
    pub fn new(session: &Session) -> Room {
        let mut keys = SheetKeys::new();
        if let Some(d) = session.active() {
            keys.sync(&d.wb);
        }
        Room { keys, version: 0, epoch: 0, seen: HashMap::new(), order: VecDeque::new() }
    }

    /// Rebuilds a room from a stored state: counters, dedup window and the sheet keys issued
    /// before the restart. The caller has already loaded the workbook the state belongs to.
    pub fn restore(keys: SheetKeys, state: RoomState) -> Room {
        let mut room = Room { keys, version: state.version, epoch: state.epoch, seen: HashMap::new(), order: VecDeque::new() };
        for applied in state.seen {
            room.remember(applied);
        }
        room
    }

    /// The state a store keeps across a restart.
    pub fn state(&self) -> RoomState {
        let seen = self.order.iter().filter_map(|id| self.seen.get(id).cloned()).collect();
        RoomState { version: self.version, epoch: self.epoch, seen }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn sheet_keys(&self) -> &SheetKeys {
        &self.keys
    }

    /// Brings the sheet keys in line with the session's workbook. The room does this on every
    /// operation; a test or a resync calls it directly.
    pub fn sync_keys(&mut self, session: &Session) {
        if let Some(d) = session.active() {
            self.keys.sync(&d.wb);
        }
    }

    /// Applies one operation. A repeated `opId` returns the first answer without touching the
    /// document (ADR-0003); a structural command whose `base_version` is not the head is refused
    /// as `stale` with `retryable: true`, because resending against the new head can work.
    pub fn apply(&mut self, session: &mut Session, op: ClientOp, actor: &str) -> Result<Applied, RoomError> {
        if let Some(done) = self.seen.get(&op.op_id) {
            return Ok(done.clone());
        }
        let class = classify(&op.payload.cmd)
            .ok_or_else(|| self.reject(&op, RejectCode::Forbidden, false, "the command does not travel in a shared session"))?;
        let structural = op.kind == Kind::Structural || class == Class::Structural;
        if structural && op.base_version != self.version {
            return Err(self.reject(&op, RejectCode::Stale, true, "a structural edit waits for the head version"));
        }
        let (index, name) = {
            let d = session.active().ok_or_else(|| self.reject(&op, RejectCode::Internal, false, "no document"))?;
            self.keys.sync(&d.wb);
            let key: u64 = op.target.sheet.parse().map_err(|_| self.reject(&op, RejectCode::Stale, false, "not a sheet key"))?;
            let index = self.keys.index_for_key(&d.wb, key).ok_or_else(|| self.reject(&op, RejectCode::Stale, false, "the sheet is gone"))?;
            let name = d.wb.sheet(index).map(|s| s.name.clone()).ok_or_else(|| self.reject(&op, RejectCode::Stale, false, "the sheet is gone"))?;
            (index, name)
        };
        debug_assert!(session.active().is_some_and(|d| d.wb.sheet(index).is_some()));
        let mut params = op.payload.params.clone();
        normalise(&mut params, &op.payload.cmd, &name, op.target.a1.as_deref());
        session.apply_remote(&op.payload.cmd, params.clone()).map_err(|e| self.reject(&op, RejectCode::Invalid, false, &e.to_string()))?;
        let changed = session.take_changed();
        let delta = {
            let d = session.active().ok_or_else(|| self.reject(&op, RejectCode::Internal, false, "no document"))?;
            delta_for(&d.wb, &self.keys, &changed)
        };
        self.version += 1;
        if structural {
            self.epoch += 1;
        }
        let applied = Applied {
            op_id: op.op_id.clone(),
            version: self.version,
            epoch: self.epoch,
            actor: actor.to_string(),
            result: Outcome { command: Some(CommandEcho { id: op.payload.cmd.clone(), params }), delta: Some(delta) },
        };
        self.remember(applied.clone());
        Ok(applied)
    }

    fn remember(&mut self, applied: Applied) {
        let id = applied.op_id.clone();
        self.seen.insert(id.clone(), applied);
        self.order.push_back(id);
        while self.order.len() > DEDUP_WINDOW {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
    }

    fn reject(&self, op: &ClientOp, code: RejectCode, retryable: bool, message: &str) -> RoomError {
        RoomError::Rejected(Rejected {
            op_id: op.op_id.clone(),
            code,
            retryable,
            hint: Some(Hint { version: self.version, epoch: self.epoch }),
            message: Some(message.to_string()),
        })
    }
}

/// Fills in what the UI carried implicitly: the sheet and the address the operation applies to.
/// A command's parameters on the wire are explicit (ADR-0001); the selection never decides.
fn normalise(params: &mut Json, cmd: &str, sheet: &str, a1: Option<&str>) {
    let Some(object) = params.as_object_mut() else { return };
    if !object.contains_key("sheet") {
        object.insert("sheet".to_string(), Json::String(sheet.to_string()));
    }
    let Some(a1) = a1 else { return };
    if object.contains_key("cell") || object.contains_key("range") {
        return;
    }
    if cmd.starts_with("cell.") || cmd.starts_with("selection.") {
        object.insert("cell".to_string(), Json::String(a1.to_string()));
    } else {
        object.insert("range".to_string(), Json::String(a1.to_string()));
    }
}
