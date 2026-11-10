//! The on-disk state of one server process (`docs/collab/server.md`): one directory per file,
//! atomic files, no user input in a path.
//!
//! ```text
//! sessions/<fileId>/
//!   base.<V>.xlsx     the authoritative workbook at base version V
//!   keys.<V>.json     session sheet keys issued until then
//!   seen.<V>.json     the room state (version, epoch, dedup window) at V
//!   journal.jsonl     applied operations after V
//!   meta.json         file, owner, name, version, epoch, base_version
//! ```
//!
//! A snapshot is four files, and `meta.json` is its commit point: it is written last, it names
//! the version the other three describe, and the readers take that version from it. A crash
//! between two writes therefore leaves the previous snapshot whole — a base at V with a
//! `seen.json` at V — instead of a new base that a restart would replay onto a second time
//! (CodeRabbit 5846575558, `store.rs:285`).
//!
//! The base is the real `.xlsx` the server writes, not a private JSON model: the reader and the
//! writer already round-trip everything unmodelled (custom XML, connections, OLE, form controls,
//! printer settings) and their round-trip is covered by the engine's own tests. A snapshot is
//! therefore the same bytes `commit` would produce, and a restart needs no second serializer.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gridcraft_collab::{Applied, ClientOp, KeyFile};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::cabinet::Published;

const BASE: &str = "base.xlsx";
const KEYS: &str = "keys.json";
const SEEN: &str = "seen.json";
const JOURNAL: &str = "journal.jsonl";
const META: &str = "meta.json";

/// `base.xlsx` at version 7 is `base.7.xlsx`: the three files of one snapshot carry the version
/// they describe, so a half-written snapshot is never read as the current one.
fn versioned(name: &str, version: u64) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.{version}.{ext}"),
        None => format!("{name}.{version}"),
    }
}

/// The inverse of [`versioned`]: `("base.xlsx", 7)` for `base.7.xlsx`. `None` for a name that is
/// not a versioned snapshot file (`journal.jsonl`, `meta.json`, a temporary file).
fn versioned_parts(name: &str) -> Option<(&'static str, u64)> {
    let (stem, ext) = name.rsplit_once('.')?;
    let (family, version) = stem.rsplit_once('.')?;
    let version: u64 = version.parse().ok()?;
    let family = match (family, ext) {
        ("base", "xlsx") => BASE,
        ("keys", "json") => KEYS,
        ("seen", "json") => SEEN,
        _ => return None,
    };
    Some((family, version))
}

/// What produced one journaled version (`docs/collab/server.md`).
///
/// An operation is replayed through the room, so its engine rules and its step of the history run
/// once more. An undo/redo is replayed from the answer it produced: the step it reversed lives in
/// the room's memory only (ADR-0010 §6) and is not part of the volume, while `applied` already
/// carries every cell or sheet the operation put back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum JournalOp {
    Op { op: ClientOp },
    Undo,
    Redo,
}

/// One applied operation of the journal: enough to replay it after a restart and to answer a
/// client that missed it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    /// Seconds since the epoch.
    pub at: u64,
    pub actor: String,
    pub op: JournalOp,
    pub applied: Applied,
}

/// `meta.json`: what the directory belongs to and where its base and journal meet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMeta {
    pub session_id: String,
    pub file_id: i64,
    pub app: String,
    pub owner: String,
    pub name: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// Head version.
    pub version: u64,
    pub epoch: u64,
    /// Version of `base.xlsx`; the journal holds the operations after it.
    pub base_version: u64,
    /// The last commit the cabinet accepted (`INTEGRATION.md` §5.2). The etag is what the next
    /// commit sends as `expectedVersion`, so a restart does not lose the conflict check. Absent
    /// when the room never published or its volume predates the callback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cabinet: Option<Published>,
}

/// What a snapshot write needs, captured under the server lock and written without it.
pub struct Snapshot {
    pub meta: StoredMeta,
    pub workbook: std::sync::Arc<gridcraft_model::Workbook>,
    pub keys: gridcraft_collab::SheetKeys,
    pub state: gridcraft_collab::RoomState,
}

/// Why the store could not do something. Messages carry a path or a format error, never the
/// contents of a cell.
#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    Json(serde_json::Error),
    Xlsx(String),
    Load(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "storage io: {e}"),
            StoreError::Json(e) => write!(f, "storage json: {e}"),
            StoreError::Xlsx(e) => write!(f, "storage xlsx: {e}"),
            StoreError::Load(e) => write!(f, "storage load: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> StoreError {
        StoreError::Io(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> StoreError {
        StoreError::Json(e)
    }
}

/// The volume the server keeps its sessions on.
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Uses `root`, creating it when it does not exist.
    pub fn open(root: impl AsRef<Path>) -> Result<Store, StoreError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("sessions"))?;
        Ok(Store { root })
    }

    fn dir(&self, file_id: i64) -> PathBuf {
        self.root.join("sessions").join(file_id.to_string())
    }

    /// Writes `base.<version>.xlsx` atomically. The caller has already validated `file_id`.
    pub fn write_base(&self, file_id: i64, version: u64, bytes: &[u8]) -> Result<(), StoreError> {
        self.atomic(file_id, &versioned(BASE, version), bytes)
    }

    pub fn read_base(&self, file_id: i64, version: u64) -> Result<Option<Vec<u8>>, StoreError> {
        self.read_bytes(file_id, BASE, version)
    }

    pub fn write_keys(
        &self,
        file_id: i64,
        version: u64,
        workbook: &gridcraft_model::Workbook,
        keys: &gridcraft_collab::SheetKeys,
    ) -> Result<(), StoreError> {
        self.write_json(file_id, &versioned(KEYS, version), &keys.to_file(workbook))
    }

    pub fn read_keys(&self, file_id: i64, version: u64) -> Result<Option<KeyFile>, StoreError> {
        self.read_json(file_id, KEYS, version)
    }

    pub fn write_state(&self, file_id: i64, version: u64, state: &gridcraft_collab::RoomState) -> Result<(), StoreError> {
        self.write_json(file_id, &versioned(SEEN, version), state)
    }

    pub fn read_state(&self, file_id: i64, version: u64) -> Result<Option<gridcraft_collab::RoomState>, StoreError> {
        self.read_json(file_id, SEEN, version)
    }

    pub fn write_meta(&self, file_id: i64, meta: &StoredMeta) -> Result<(), StoreError> {
        self.atomic(file_id, META, &serde_json::to_vec(meta)?)
    }

    pub fn read_meta(&self, file_id: i64) -> Result<Option<StoredMeta>, StoreError> {
        match fs::read(self.dir(file_id).join(META)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(StoreError::Json),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Rewrites the journal atomically: a crash leaves either the old file or the new one.
    pub fn write_journal(&self, file_id: i64, entries: &[JournalEntry]) -> Result<(), StoreError> {
        let mut bytes = Vec::new();
        for entry in entries {
            serde_json::to_writer(&mut bytes, entry)?;
            bytes.push(b'\n');
        }
        self.atomic(file_id, JOURNAL, &bytes)
    }

    /// Appends one operation. A crash can leave a torn last line, which [`Store::read_journal`]
    /// drops; everything before it is intact.
    pub fn append_journal(&self, file_id: i64, entry: &JournalEntry) -> Result<(), StoreError> {
        use std::io::Write;
        let dir = self.dir(file_id);
        fs::create_dir_all(&dir)?;
        let mut line = serde_json::to_vec(entry)?;
        line.push(b'\n');
        let mut file = fs::OpenOptions::new().create(true).append(true).open(dir.join(JOURNAL))?;
        file.write_all(&line)?;
        file.flush()?;
        Ok(())
    }

    /// Reads the journal. A torn last line is ignored (a crash between write and newline); any
    /// other malformed line is an error, because replaying past it would silently lose work.
    pub fn read_journal(&self, file_id: i64) -> Result<Vec<JournalEntry>, StoreError> {
        let path = self.dir(file_id).join(JOURNAL);
        let text = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        let mut lines = text.split(|b| *b == b'\n').peekable();
        while let Some(line) = lines.next() {
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<JournalEntry>(line) {
                Ok(entry) => out.push(entry),
                Err(_) if lines.peek().is_none() => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(out)
    }

    /// Reads the journal for a session that is about to take new appends, and drops a torn tail
    /// **on disk** first.
    ///
    /// [`Store::read_journal`] ignores a torn last line, but the bytes stay in the file: the next
    /// [`Store::append_journal`] writes its entry straight after them and the two become one line
    /// that is malformed and no longer last (the newline the append ends with leaves an empty
    /// remainder after it), so [`Store::read_journal`] refuses the whole file and a restart loses
    /// every operation after the snapshot. Rewriting the journal with the entries that parse is
    /// the same file the appends expect.
    pub fn read_journal_repairing(&self, file_id: i64) -> Result<Vec<JournalEntry>, StoreError> {
        let entries = self.read_journal(file_id)?;
        let path = self.dir(file_id).join(JOURNAL);
        let Ok(bytes) = fs::read(&path) else { return Ok(entries) };
        if bytes.is_empty() || bytes.last() == Some(&b'\n') {
            return Ok(entries);
        }
        self.write_journal(file_id, &entries)?;
        Ok(entries)
    }

    /// Every session on the volume, in file order.
    pub fn sessions(&self) -> Result<Vec<StoredMeta>, StoreError> {
        let dir = self.root.join("sessions");
        let mut out = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Some(file_id) = entry.file_name().to_str().and_then(|name| name.parse::<i64>().ok()) else {
                continue;
            };
            // One damaged `meta.json` must not stop the server from serving the others: a scan
            // that failed here restored no session at all.
            match self.read_meta(file_id) {
                Ok(Some(meta)) => out.push(meta),
                Ok(None) => {}
                Err(e) => log::warn!("session fileId={file_id} has no readable meta.json: {e}"),
            }
        }
        out.sort_by_key(|meta| meta.file_id);
        Ok(out)
    }

    /// Size of `journal.jsonl` in bytes; `None` when the session has no journal yet.
    pub fn journal_size(&self, file_id: i64) -> Result<Option<u64>, StoreError> {
        self.file_size(file_id, JOURNAL)
    }

    /// Bytes of the base files of this session, every version on the volume counted; `None` when
    /// the session has no base yet.
    pub fn base_size(&self, file_id: i64) -> Result<Option<u64>, StoreError> {
        let dir = self.dir(file_id);
        let Ok(entries) = fs::read_dir(&dir) else { return Ok(None) };
        let mut total = None;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name != BASE && versioned_parts(name).is_none_or(|(family, _)| family != BASE) {
                continue;
            }
            let len = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            total = Some(total.unwrap_or(0) + len);
        }
        Ok(total)
    }

    fn file_size(&self, file_id: i64, name: &str) -> Result<Option<u64>, StoreError> {
        match fs::metadata(self.dir(file_id).join(name)) {
            Ok(meta) => Ok(Some(meta.len())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Removes a session after the cabinet closed it or committed it.
    pub fn remove(&self, file_id: i64) -> Result<(), StoreError> {
        match fs::remove_dir_all(self.dir(file_id)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes one snapshot without holding the server lock: the base, the keys, the room state
    /// and the metadata. The journal is trimmed by the caller once this has returned.
    ///
    /// The metadata is the commit point. The three files of the snapshot carry the version they
    /// describe, so a crash before `meta.json` is written leaves the previous snapshot complete
    /// and readable — a restart replays from the version `meta.json` names, never from a base
    /// whose dedup window and metadata still describe an older one.
    pub fn write_snapshot(&self, snapshot: &Snapshot) -> Result<(), StoreError> {
        let file_id = snapshot.meta.file_id;
        let version = snapshot.meta.base_version;
        let bytes = gridcraft_xlsx::write_xlsx(&snapshot.workbook).map_err(|e| StoreError::Xlsx(e.to_string()))?;
        self.write_base(file_id, version, &bytes)?;
        self.write_keys(file_id, version, &snapshot.workbook, &snapshot.keys)?;
        self.write_state(file_id, version, &snapshot.state)?;
        self.write_meta(file_id, &snapshot.meta)?;
        self.prune_snapshots(file_id, version);
        Ok(())
    }

    /// Removes the snapshot files of every version but `keep`, once `meta.json` names it. A
    /// failure is logged, not returned: the snapshot itself is on disk and a stale base only
    /// costs space.
    fn prune_snapshots(&self, file_id: i64, keep: u64) {
        let dir = self.dir(file_id);
        let Ok(entries) = fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if versioned_parts(name).is_none_or(|(_, version)| version == keep) {
                continue;
            }
            if let Err(e) = fs::remove_file(entry.path()) {
                log::warn!("cannot remove the old snapshot {} of fileId={file_id}: {e}", entry.path().display());
            }
        }
    }

    fn write_json<T: Serialize>(&self, file_id: i64, name: &str, value: &T) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec(value)?;
        self.atomic(file_id, name, &bytes)
    }

    fn read_bytes(&self, file_id: i64, name: &str, version: u64) -> Result<Option<Vec<u8>>, StoreError> {
        let dir = self.dir(file_id);
        // A volume written before the files carried their version has the plain name; it is read
        // until the first snapshot of the new build replaces it.
        match fs::read(dir.join(versioned(name, version))) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => match fs::read(dir.join(name)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Err(e) => Err(e.into()),
        }
    }

    fn read_json<T: DeserializeOwned>(&self, file_id: i64, name: &str, version: u64) -> Result<Option<T>, StoreError> {
        match self.read_bytes(file_id, name, version)? {
            Some(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(StoreError::Json),
            None => Ok(None),
        }
    }

    fn atomic(&self, file_id: i64, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
        use std::io::Write;

        // Every write gets its own temporary path. Two writers of one document can overlap — a
        // snapshot's `meta.json` and the metadata a commit stores, two commits, a slow snapshot
        // and a new one — and a shared `{name}.tmp` let them interleave their bytes, or made one
        // `rename` fail with `NotFound` after the other had already moved the file. The bytes are
        // on disk before the rename, so a crash still leaves either the old file or the new one.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        let dir = self.dir(file_id);
        fs::create_dir_all(&dir)?;
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = dir.join(format!("{name}.{}.{seq}.tmp", std::process::id()));
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, dir.join(name))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("gridcraft-collab-store-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        Store::open(&root).unwrap()
    }

    /// Every writer gets its own temporary path: two overlapping writes of one file must not
    /// interleave their bytes or make each other's `rename` fail.
    #[test]
    fn concurrent_writes_of_one_file_do_not_collide() {
        use std::sync::Arc;

        let store = Arc::new(store());
        let mut handles = Vec::new();
        for writer in 0..8u64 {
            let store = store.clone();
            handles.push(std::thread::spawn(move || {
                for round in 0..20u64 {
                    let state = gridcraft_collab::RoomState { version: writer * 100 + round, epoch: 0, seen: Vec::new() };
                    store.write_state(1, 0, &state).expect("one atomic write");
                }
            }));
        }
        for handle in handles {
            handle.join().expect("no writer panics");
        }
        let state = store.read_state(1, 0).expect("the file decodes").expect("the file is there");
        assert!(state.version < 800, "the file holds one writer's value, not a mixture");
    }

    fn entry(version: u64) -> JournalEntry {
        JournalEntry {
            at: 1_000 + version,
            actor: "ann".into(),
            op: JournalOp::Op {
                op: serde_json::from_str(
                    r#"{"opId":"op-1","doc":"s","kind":"command","target":{"sheet":"1"},"payload":{"cmd":"cell.set","params":{}}}"#,
                )
                .unwrap(),
            },
            applied: serde_json::from_str(&format!(r#"{{"opId":"op-{version}","version":{version},"epoch":0,"actor":"ann","result":{{}}}}"#))
                .unwrap(),
        }
    }

    #[test]
    fn a_reversal_entry_carries_its_effect_and_not_the_step_it_reversed() {
        let store = store();
        let reversal = JournalEntry {
            at: 2_000,
            actor: "ann".into(),
            op: JournalOp::Undo,
            applied: serde_json::from_str(
                r#"{"opId":"undo-op-1","version":2,"epoch":0,"actor":"ann","result":{"delta":[{"sheet":"1","cells":[{"a1":"A1","value":null}]}]}}"#,
            )
            .unwrap(),
        };
        store.write_journal(7, &[entry(1), reversal.clone()]).unwrap();
        let read = store.read_journal(7).unwrap();
        assert_eq!(read, vec![entry(1), reversal]);
        let line = fs::read_to_string(store.dir(7).join(JOURNAL)).unwrap();
        assert!(line.contains(r#""kind":"undo""#), "{line}");
        assert!(!line.contains(r#""step""#), "the undo history is not part of the volume: {line}");
        assert!(!line.contains(r#""before""#), "the undo history is not part of the volume: {line}");
    }

    #[test]
    fn a_snapshot_round_trips_its_files() {
        let store = store();
        store.write_base(7, 3, b"PK\x03\x04not-really").unwrap();
        store.write_keys(7, 3, &gridcraft_model::Workbook::new(), &gridcraft_collab::SheetKeys::new()).unwrap();
        store.write_state(7, 3, &gridcraft_collab::RoomState::default()).unwrap();
        let meta = StoredMeta {
            session_id: "s".into(),
            file_id: 7,
            app: "gridcraft".into(),
            owner: "ann".into(),
            name: "plan.xlsx".into(),
            created_at: 1,
            updated_at: 2,
            version: 3,
            epoch: 1,
            base_version: 3,
            cabinet: None,
        };
        store.write_meta(7, &meta).unwrap();
        assert_eq!(store.read_base(7, 3).unwrap().unwrap(), b"PK\x03\x04not-really");
        assert_eq!(store.read_meta(7).unwrap().unwrap(), meta);
        assert_eq!(store.sessions().unwrap(), vec![meta]);
    }

    #[test]
    fn the_cabinet_etag_survives_a_restart() {
        let store = store();
        let meta = StoredMeta {
            session_id: "s".into(),
            file_id: 8,
            app: "gridcraft".into(),
            owner: "ann".into(),
            name: "plan.xlsx".into(),
            created_at: 1,
            updated_at: 2,
            version: 4,
            epoch: 0,
            base_version: 4,
            cabinet: Some(Published { version: 42, etag: "1794186677".into() }),
        };
        store.write_meta(8, &meta).unwrap();
        let loaded = store.read_meta(8).unwrap().expect("meta");
        assert_eq!(loaded.cabinet, Some(Published { version: 42, etag: "1794186677".into() }), "the next commit expects the cabinet's etag");
        // A meta written before the callback existed still reads.
        let legacy = serde_json::json!({
            "sessionId": "s", "fileId": 8, "app": "gridcraft", "owner": "ann", "name": "plan.xlsx",
            "createdAt": 1, "updatedAt": 2, "version": 4, "epoch": 0, "baseVersion": 4
        });
        std::fs::write(store.dir(8).join(META), legacy.to_string()).unwrap();
        assert_eq!(store.read_meta(8).unwrap().map(|meta| meta.cabinet), Some(None));
    }

    #[test]
    fn the_journal_keeps_order_and_ignores_a_torn_last_line() {
        let store = store();
        store.write_journal(7, &[entry(1), entry(2)]).unwrap();
        assert_eq!(store.read_journal(7).unwrap(), vec![entry(1), entry(2)]);
        let path = store.dir(7).join(JOURNAL);
        let mut text = fs::read(&path).unwrap();
        text.extend_from_slice(b"{\"at\":1003,\"actor\":\"ann\"");
        fs::write(&path, &text).unwrap();
        assert_eq!(store.read_journal(7).unwrap(), vec![entry(1), entry(2)]);
    }

    /// A torn tail is dropped on disk before the session takes new appends: the bytes a crash
    /// left behind would otherwise join the next entry into one malformed line, and the restart
    /// after it would refuse the journal.
    #[test]
    fn a_torn_journal_tail_is_dropped_before_the_next_append() {
        let store = store();
        store.write_journal(7, &[entry(1), entry(2)]).unwrap();
        let path = store.dir(7).join(JOURNAL);
        let mut text = fs::read(&path).unwrap();
        text.extend_from_slice(b"{\"at\":1003,\"actor\":\"ann\"");
        fs::write(&path, &text).unwrap();
        assert_eq!(store.read_journal_repairing(7).unwrap(), vec![entry(1), entry(2)]);
        store.append_journal(7, &entry(3)).unwrap();
        assert_eq!(store.read_journal(7).unwrap(), vec![entry(1), entry(2), entry(3)], "the next append starts on its own line");
    }

    /// One unreadable `meta.json` is one session that cannot be restored, not a volume that
    /// restores nothing.
    #[test]
    fn a_damaged_meta_does_not_hide_the_other_sessions() {
        let store = store();
        let meta = |file_id: i64| StoredMeta {
            session_id: format!("s{file_id}"),
            file_id,
            app: "gridcraft".into(),
            owner: "ann".into(),
            name: "plan.xlsx".into(),
            created_at: 1,
            updated_at: 2,
            version: 1,
            epoch: 0,
            base_version: 1,
            cabinet: None,
        };
        store.write_meta(1, &meta(1)).unwrap();
        store.write_meta(2, &meta(2)).unwrap();
        fs::write(store.dir(2).join(META), b"{\"sessionId\": ").unwrap();
        let sessions = store.sessions().unwrap();
        assert_eq!(sessions.iter().map(|meta| meta.file_id).collect::<Vec<_>>(), vec![1], "the readable session survives");
    }

    fn snapshot(version: u64) -> Snapshot {
        Snapshot {
            meta: StoredMeta {
                session_id: "s".into(),
                file_id: 7,
                app: "gridcraft".into(),
                owner: "ann".into(),
                name: "plan.xlsx".into(),
                created_at: 1,
                updated_at: 2,
                version,
                epoch: 0,
                base_version: version,
                cabinet: None,
            },
            workbook: std::sync::Arc::new(gridcraft_model::Workbook::new()),
            keys: gridcraft_collab::SheetKeys::new(),
            state: gridcraft_collab::RoomState { version, epoch: 0, seen: Vec::new() },
        }
    }

    /// `meta.json` is the commit point of a snapshot: a crash after a newer base was written but
    /// before its metadata leaves the previous one readable, so the restart replays onto the base
    /// its `seen.json` describes instead of applying operations a second time.
    #[test]
    fn the_meta_file_is_the_commit_point_of_a_snapshot() {
        let store = store();
        store.write_snapshot(&snapshot(5)).unwrap();
        let five = store.read_base(7, 5).unwrap().expect("the base of version 5");
        // A crash in the middle of the next snapshot: its base is on the volume, its metadata is
        // not, and the room never reached version 7.
        store.write_base(7, 7, b"PK\x03\x04the-next-base").unwrap();
        let meta = store.read_meta(7).unwrap().expect("meta");
        assert_eq!(meta.base_version, 5, "the metadata still names the complete snapshot");
        assert_eq!(store.read_base(7, meta.base_version).unwrap().expect("a base"), five, "the restart reads the version its metadata names");
        // The next snapshot completes: it becomes the point a restart reads, and the old files go.
        store.write_snapshot(&snapshot(7)).unwrap();
        assert_eq!(store.read_meta(7).unwrap().expect("meta").base_version, 7);
        assert!(!store.dir(7).join("base.5.xlsx").exists(), "the previous version is pruned once meta.json names the new one");
        assert!(!store.dir(7).join("seen.5.json").exists());
        assert!(store.dir(7).join("base.7.xlsx").exists());
    }

    #[test]
    fn an_empty_volume_has_no_sessions() {
        let store = store();
        assert!(store.sessions().unwrap().is_empty());
        assert!(store.read_base(9, 0).unwrap().is_none());
        assert!(store.read_journal(9).unwrap().is_empty());
        store.remove(9).unwrap();
    }
}
