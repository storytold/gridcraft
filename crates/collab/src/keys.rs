//! Session-scoped sheet keys (ADR-0002).
//!
//! A workbook names its sheets, and a name changes; a formula refers to a sheet by name too. The
//! protocol therefore addresses a sheet by a key the server issues for the lifetime of a session,
//! mapped here to the sheet's runtime identity ([`SheetUid`]) so that inserting, moving or
//! renaming a sheet keeps the key with it. A key of a deleted sheet stays as a tombstone: a late
//! operation addressed to it is refused rather than resurrecting whatever now sits at that index.

use std::collections::{BTreeMap, HashSet};

use gridcraft_model::Workbook;
use serde::{Deserialize, Serialize};

/// The persisted form of a key map. A [`SheetUid`] is a runtime identity and is never in a file,
/// so the map is written by sheet **name**: names are unique in a workbook, a rename keeps the
/// key (the snapshot after it stores the new name), and a deleted sheet leaves a tombstone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyFile {
    /// The last key issued.
    pub next: u64,
    /// Key → the name of the sheet it belongs to at the time of the snapshot.
    #[serde(default)]
    pub live: Vec<(u64, String)>,
    /// Keys whose sheet is gone.
    #[serde(default)]
    pub dead: Vec<u64>,
}

/// Keys of the sheets of one session.
#[derive(Clone, Debug, Default)]
pub struct SheetKeys {
    next: u64,
    /// Sheet identity (runtime `SheetUid`) → key.
    key_of_uid: BTreeMap<u64, u64>,
    /// Key → sheet identity.
    uid_of_key: BTreeMap<u64, u64>,
    /// Keys whose sheet is gone; never given out again in this session.
    dead: HashSet<u64>,
}

impl SheetKeys {
    pub fn new() -> SheetKeys {
        SheetKeys::default()
    }

    /// Brings the map in line with `wb`: a new sheet gets a key, a sheet that went away leaves a
    /// tombstone. Existing keys never change.
    pub fn sync(&mut self, wb: &Workbook) {
        let live: Vec<u64> = wb.sheets.iter().map(|s| s.uid.get()).collect();
        for uid in &live {
            if !self.key_of_uid.contains_key(uid) {
                self.next += 1;
                let key = self.next;
                self.key_of_uid.insert(*uid, key);
                self.uid_of_key.insert(key, *uid);
            }
        }
        let live: HashSet<u64> = live.into_iter().collect();
        let gone: Vec<u64> = self.key_of_uid.keys().copied().filter(|uid| !live.contains(uid)).collect();
        for uid in gone {
            if let Some(key) = self.key_of_uid.remove(&uid) {
                self.uid_of_key.remove(&key);
                self.dead.insert(key);
            }
        }
    }

    /// The key of the sheet at `index` of `wb`.
    pub fn key_for_index(&self, wb: &Workbook, index: usize) -> Option<u64> {
        let uid = wb.sheets.get(index)?.uid.get();
        self.key_of_uid.get(&uid).copied()
    }

    /// Where the sheet with `key` is now, if it still exists.
    pub fn index_for_key(&self, wb: &Workbook, key: u64) -> Option<usize> {
        let uid = *self.uid_of_key.get(&key)?;
        wb.sheets.iter().position(|s| s.uid.get() == uid)
    }

    /// Whether `key` belonged to a sheet that was deleted in this session.
    pub fn is_dead(&self, key: u64) -> bool {
        self.dead.contains(&key)
    }

    /// Gives a sheet that a structural undo brings back its old key again. A deleted sheet leaves
    /// a tombstone; the step that removed it keeps the key in its image, and restoring the sheet
    /// with the same runtime identity before the next [`SheetKeys::sync`] keeps the key with it.
    pub fn revive(&mut self, key: u64, uid: u64) {
        if let Some(old) = self.uid_of_key.insert(key, uid) {
            self.key_of_uid.remove(&old);
        }
        if let Some(previous) = self.key_of_uid.insert(uid, key) {
            self.uid_of_key.remove(&previous);
        }
        self.dead.remove(&key);
        self.next = self.next.max(key);
    }

    /// How many keys the session has issued.
    pub fn issued(&self) -> u64 {
        self.next
    }

    /// The persisted form: keys by sheet name, plus the tombstones.
    pub fn to_file(&self, wb: &Workbook) -> KeyFile {
        let live = self
            .uid_of_key
            .iter()
            .filter_map(|(key, uid)| {
                let sheet = wb.sheets.iter().find(|sheet| sheet.uid.get() == *uid)?;
                Some((*key, sheet.name.clone()))
            })
            .collect();
        KeyFile { next: self.next, live, dead: self.dead.iter().copied().collect() }
    }

    /// Rebuilds a key map over `wb` from its persisted form. A key whose sheet is not in this
    /// workbook becomes a tombstone: the operation addressed to it is refused rather than
    /// resurrecting a sheet that now has that name.
    pub fn from_file(wb: &Workbook, file: KeyFile) -> SheetKeys {
        let mut keys = SheetKeys { next: file.next, ..SheetKeys::default() };
        for (key, name) in file.live {
            match wb.sheets.iter().find(|sheet| sheet.name == name) {
                Some(sheet) => {
                    keys.key_of_uid.insert(sheet.uid.get(), key);
                    keys.uid_of_key.insert(key, sheet.uid.get());
                    keys.next = keys.next.max(key);
                }
                None => {
                    keys.dead.insert(key);
                }
            }
        }
        for key in file.dead {
            keys.dead.insert(key);
            keys.next = keys.next.max(key);
        }
        keys
    }
}
