//! An in-process authoritative room: one document, one version counter, one writer.
//!
//! The room is the part of the sync server that must exist before there is a socket: it accepts a
//! normalised [`ClientOp`], deduplicates it by `opId`, runs it through
//! [`gridcraft_engine::Session::apply_remote`], turns the changed cells into the authoritative
//! delta and hands back an [`Applied`]. Structural commands are exclusive (ADR-0004): they must
//! match the head version, and the room bumps `epoch` so a client with old addresses resyncs.

use std::collections::{BTreeMap, HashMap, VecDeque};

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::{CellState, Session};
use gridcraft_model::{Sheet, Workbook};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::delta::delta_for;
use crate::keys::SheetKeys;
use crate::ops::{Class, classify};
use crate::protocol::{Applied, CellChange, ClientOp, CommandEcho, Hint, Kind, Outcome, RejectCode, Rejected, SheetDelta, SheetName};

/// How many applied operations the room remembers for idempotent resends (ADR-0003).
pub const DEDUP_WINDOW: usize = 10_000;

/// How many steps one participant may undo (ADR-0010).
pub const UNDO_STEPS: usize = 200;

/// One cell a step touched: its state before and after. An undo puts `before` back only while the
/// cell still holds `after` (an edit by somebody else wins).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellSnap {
    pub sheet: String,
    pub a1: String,
    pub before: CellState,
    pub after: CellState,
}

/// One participant's reversible step. Structural steps are not kept in v1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub op_id: String,
    pub cells: Vec<CellSnap>,
}

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
    /// Per participant, oldest first: the steps `undo` may reverse.
    history: HashMap<String, VecDeque<Step>>,
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
        Room { keys, version: 0, epoch: 0, seen: HashMap::new(), order: VecDeque::new(), history: HashMap::new() }
    }

    /// Rebuilds a room from a stored state: counters, dedup window and the sheet keys issued
    /// before the restart. The caller has already loaded the workbook the state belongs to.
    pub fn restore(keys: SheetKeys, state: RoomState) -> Room {
        let mut room =
            Room { keys, version: state.version, epoch: state.epoch, seen: HashMap::new(), order: VecDeque::new(), history: HashMap::new() };
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
        if op.epoch < self.epoch {
            return Err(self.reject(&op, RejectCode::Stale, false, "the document structure changed under this operation; resync"));
        }
        let class = classify(&op.payload.cmd)
            .ok_or_else(|| self.reject(&op, RejectCode::Forbidden, false, "the command does not travel in a shared session"))?;
        let structural = op.kind == Kind::Structural || class == Class::Structural;
        if structural && op.base_version != self.version {
            return Err(self.reject(&op, RejectCode::Stale, true, "a structural edit waits for the head version"));
        }
        // The workbook is copy-on-write: keeping the old `Arc` costs nothing until the command
        // touches a band, and it is what makes a step reversible.
        let before_wb = session.active().map(|d| d.wb.clone());
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
        let (delta, sheets) = {
            let d = session.active().ok_or_else(|| self.reject(&op, RejectCode::Internal, false, "no document"))?;
            let delta = delta_for(&d.wb, &self.keys, &changed);
            let sheets = if structural {
                self.keys.sync(&d.wb);
                Some(
                    (0..d.wb.sheets.len())
                        .filter_map(|index| {
                            let key = self.keys.key_for_index(&d.wb, index)?;
                            Some(SheetName { key: key.to_string(), name: d.wb.sheets[index].name.clone() })
                        })
                        .collect(),
                )
            } else {
                None
            };
            (delta, sheets)
        };
        let step_cells = if structural {
            Vec::new()
        } else {
            match (&before_wb, session.active()) {
                (Some(before), Some(after)) => snapshots(before, &after.wb, &self.keys, &delta),
                _ => Vec::new(),
            }
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
            sheets,
        };
        self.remember(applied.clone());
        if !step_cells.is_empty() {
            let steps = self.history.entry(actor.to_string()).or_default();
            steps.push_back(Step { op_id: op.op_id.clone(), cells: step_cells });
            while steps.len() > UNDO_STEPS {
                steps.pop_front();
            }
        }
        Ok(applied)
    }

    /// Reverses the last step of `actor` (ADR-0010): a new operation at the next version, never a
    /// rewind of somebody else's work. Every touched cell must still hold what this step left
    /// there; one that somebody else edited refuses the whole undo as `conflict` with the
    /// current head, so the client can resync and try again.
    pub fn undo(&mut self, session: &mut Session, actor: &str, op_id: Option<&str>) -> Result<Applied, RoomError> {
        let Some(step) = self.history.get(actor).and_then(|steps| steps.back()).cloned() else {
            return Err(self.undo_reject(RejectCode::Invalid, false, "there is nothing to undo"));
        };
        if let Some(wanted) = op_id
            && wanted != step.op_id
        {
            return Err(self.undo_reject(RejectCode::Conflict, false, "that is not the last edit of this participant"));
        }
        let mut groups: BTreeMap<usize, Vec<(CellRef, CellState)>> = BTreeMap::new();
        {
            let d = session.active().ok_or_else(|| self.undo_reject(RejectCode::Internal, false, "no document"))?;
            for cell in &step.cells {
                let key: u64 = cell.sheet.parse().map_err(|_| self.undo_reject(RejectCode::Internal, false, "bad sheet key in history"))?;
                let index =
                    self.keys.index_for_key(&d.wb, key).ok_or_else(|| self.undo_reject(RejectCode::Stale, false, "a sheet of this step is gone"))?;
                let sheet = d.wb.sheet(index).ok_or_else(|| self.undo_reject(RejectCode::Stale, false, "a sheet of this step is gone"))?;
                let at = CellRef::parse(&cell.a1).ok_or_else(|| self.undo_reject(RejectCode::Internal, false, "bad address in history"))?;
                if snap(sheet, at) != cell.after {
                    return Err(self.undo_reject(RejectCode::Conflict, true, "another participant changed a cell of this step"));
                }
                groups.entry(index).or_default().push((at, cell.before.clone()));
            }
        }
        for (index, cells) in &groups {
            session.restore_cells(*index, cells).map_err(|e| self.undo_reject(RejectCode::Internal, false, &e.to_string()))?;
        }
        // `restore_cells` writes the model directly (no command reached the change recorder), so
        // the delta is built from the snapshots that were just put back.
        let mut by_sheet: BTreeMap<String, Vec<CellChange>> = BTreeMap::new();
        for cell in &step.cells {
            let Ok(value) = serde_json::to_value(&cell.before.value) else { continue };
            by_sheet.entry(cell.sheet.clone()).or_default().push(CellChange {
                a1: cell.a1.clone(),
                value,
                formula: cell.before.formula.clone(),
                style: Some(cell.before.style),
            });
        }
        let delta: Vec<SheetDelta> = by_sheet
            .into_iter()
            .map(|(sheet, cells)| SheetDelta { sheet, cells, objects_deleted: Vec::new(), objects_created: Vec::new() })
            .collect();
        let _ = session.take_changed();
        self.version += 1;
        if let Some(steps) = self.history.get_mut(actor) {
            steps.pop_back();
        }
        Ok(Applied {
            op_id: format!("undo-{}", step.op_id),
            version: self.version,
            epoch: self.epoch,
            actor: actor.to_string(),
            result: Outcome { command: None, delta: Some(delta) },
            sheets: None,
        })
    }

    fn undo_reject(&self, code: RejectCode, retryable: bool, message: &str) -> RoomError {
        RoomError::Rejected(Rejected {
            op_id: "undo".into(),
            code,
            retryable,
            hint: Some(Hint { version: self.version, epoch: self.epoch }),
            message: Some(message.to_string()),
        })
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

/// The cells of one applied delta as snapshots: what they held in `before`, what they hold in
/// `after`. Sheets are matched by session key, so a structural move cannot confuse the pair.
fn snapshots(before: &Workbook, after: &Workbook, keys: &SheetKeys, delta: &[crate::protocol::SheetDelta]) -> Vec<CellSnap> {
    let mut out = Vec::new();
    for sheet in delta {
        let Ok(key) = sheet.sheet.parse::<u64>() else { continue };
        let Some(index) = keys.index_for_key(before, key) else { continue };
        let (Some(old), Some(new)) = (before.sheet(index), after.sheet(index)) else { continue };
        for change in &sheet.cells {
            let Some(at) = CellRef::parse(&change.a1) else { continue };
            out.push(CellSnap { sheet: sheet.sheet.clone(), a1: change.a1.clone(), before: snap(old, at), after: snap(new, at) });
        }
    }
    out
}

fn snap(sheet: &Sheet, at: CellRef) -> CellState {
    let cell = sheet.cell(at);
    CellState {
        value: cell.map(|cell| cell.value.clone()).unwrap_or(Value::Empty),
        formula: cell.and_then(|cell| cell.formula.as_ref().map(|formula| formula.text.clone())),
        style: sheet.style_id(at).0,
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
