//! The on-disk state of one server process (`docs/collab/server.md`): one directory per file,
//! atomic files, no user input in a path.
//!
//! ```text
//! sessions/<fileId>/
//!   base.xlsx       the authoritative workbook at `base_version`
//!   keys.json       session sheet keys issued until then
//!   seen.json       the room state (version, epoch, dedup window) at `base_version`
//!   journal.jsonl   applied operations after `base_version`
//!   meta.json       file, owner, name, version, epoch, base_version
//! ```
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

const BASE: &str = "base.xlsx";
const KEYS: &str = "keys.json";
const SEEN: &str = "seen.json";
const JOURNAL: &str = "journal.jsonl";
const META: &str = "meta.json";

/// One applied operation of the journal: enough to replay it after a restart and to answer a
/// client that missed it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    /// Seconds since the epoch.
    pub at: u64,
    pub actor: String,
    pub op: ClientOp,
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

    /// Writes `base.xlsx` atomically. The caller has already validated `file_id`.
    pub fn write_base(&self, file_id: i64, bytes: &[u8]) -> Result<(), StoreError> {
        self.atomic(file_id, BASE, bytes)
    }

    pub fn read_base(&self, file_id: i64) -> Result<Option<Vec<u8>>, StoreError> {
        match fs::read(self.dir(file_id).join(BASE)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write_keys(&self, file_id: i64, workbook: &gridcraft_model::Workbook, keys: &gridcraft_collab::SheetKeys) -> Result<(), StoreError> {
        self.write_json(file_id, KEYS, &keys.to_file(workbook))
    }

    pub fn read_keys(&self, file_id: i64) -> Result<Option<KeyFile>, StoreError> {
        self.read_json(file_id, KEYS)
    }

    pub fn write_state(&self, file_id: i64, state: &gridcraft_collab::RoomState) -> Result<(), StoreError> {
        self.write_json(file_id, SEEN, state)
    }

    pub fn read_state(&self, file_id: i64) -> Result<Option<gridcraft_collab::RoomState>, StoreError> {
        self.read_json(file_id, SEEN)
    }

    pub fn write_meta(&self, file_id: i64, meta: &StoredMeta) -> Result<(), StoreError> {
        self.write_json(file_id, META, meta)
    }

    pub fn read_meta(&self, file_id: i64) -> Result<Option<StoredMeta>, StoreError> {
        self.read_json(file_id, META)
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
            if let Some(meta) = self.read_meta(file_id)? {
                out.push(meta);
            }
        }
        out.sort_by_key(|meta| meta.file_id);
        Ok(out)
    }

    /// Size of `journal.jsonl` in bytes; `None` when the session has no journal yet.
    pub fn journal_size(&self, file_id: i64) -> Result<Option<u64>, StoreError> {
        self.file_size(file_id, JOURNAL)
    }

    /// Size of `base.xlsx` in bytes; `None` when the session has no base yet.
    pub fn base_size(&self, file_id: i64) -> Result<Option<u64>, StoreError> {
        self.file_size(file_id, BASE)
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
    pub fn write_snapshot(&self, snapshot: &Snapshot) -> Result<(), StoreError> {
        let bytes = gridcraft_xlsx::write_xlsx(&snapshot.workbook).map_err(|e| StoreError::Xlsx(e.to_string()))?;
        self.write_base(snapshot.meta.file_id, &bytes)?;
        self.write_keys(snapshot.meta.file_id, &snapshot.workbook, &snapshot.keys)?;
        self.write_state(snapshot.meta.file_id, &snapshot.state)?;
        self.write_meta(snapshot.meta.file_id, &snapshot.meta)?;
        Ok(())
    }

    fn write_json<T: Serialize>(&self, file_id: i64, name: &str, value: &T) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec(value)?;
        self.atomic(file_id, name, &bytes)
    }

    fn read_json<T: DeserializeOwned>(&self, file_id: i64, name: &str) -> Result<Option<T>, StoreError> {
        match fs::read(self.dir(file_id).join(name)) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(StoreError::Json),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn atomic(&self, file_id: i64, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let dir = self.dir(file_id);
        fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!("{name}.tmp"));
        fs::write(&tmp, bytes)?;
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

    fn entry(version: u64) -> JournalEntry {
        JournalEntry {
            at: 1_000 + version,
            actor: "ann".into(),
            op: serde_json::from_str(r#"{"opId":"op-1","doc":"s","kind":"command","target":{"sheet":"1"},"payload":{"cmd":"cell.set","params":{}}}"#)
                .unwrap(),
            applied: serde_json::from_str(&format!(r#"{{"opId":"op-{version}","version":{version},"epoch":0,"actor":"ann","result":{{}}}}"#))
                .unwrap(),
        }
    }

    #[test]
    fn a_snapshot_round_trips_its_files() {
        let store = store();
        store.write_base(7, b"PK\x03\x04not-really").unwrap();
        store.write_keys(7, &gridcraft_model::Workbook::new(), &gridcraft_collab::SheetKeys::new()).unwrap();
        store.write_state(7, &gridcraft_collab::RoomState::default()).unwrap();
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
        };
        store.write_meta(7, &meta).unwrap();
        assert_eq!(store.read_base(7).unwrap().unwrap(), b"PK\x03\x04not-really");
        assert_eq!(store.read_meta(7).unwrap().unwrap(), meta);
        assert_eq!(store.sessions().unwrap(), vec![meta]);
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

    #[test]
    fn an_empty_volume_has_no_sessions() {
        let store = store();
        assert!(store.sessions().unwrap().is_empty());
        assert!(store.read_base(9).unwrap().is_none());
        assert!(store.read_journal(9).unwrap().is_empty());
        store.remove(9).unwrap();
    }
}
