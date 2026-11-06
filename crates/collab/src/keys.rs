//! Session-scoped sheet keys (ADR-0002).
//!
//! A workbook names its sheets, and a name changes; a formula refers to a sheet by name too. The
//! protocol therefore addresses a sheet by a key the server issues for the lifetime of a session,
//! mapped here to the sheet's runtime identity ([`SheetUid`]) so that inserting, moving or
//! renaming a sheet keeps the key with it. A key of a deleted sheet stays as a tombstone: a late
//! operation addressed to it is refused rather than resurrecting whatever now sits at that index.

use std::collections::{BTreeMap, HashSet};

use gridcraft_model::Workbook;

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

    /// How many keys the session has issued.
    pub fn issued(&self) -> u64 {
        self.next
    }
}
