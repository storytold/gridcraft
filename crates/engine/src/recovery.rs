//! Recovery copies of unsaved workbooks, for the desktop app (`file.recover`).
//!
//! While a workbook has unsaved changes, a copy of it is written to the recovery folder at most
//! every [`INTERVAL`], so a crash, a forced restart or a power cut loses only the last few
//! seconds of work. Saving the workbook, closing it (Don't Save) or undoing back to the saved
//! state removes its copy. Copies left by a run that ended without saving are listed by
//! [`Session::recovery_copies`] and restored as new, unsaved workbooks.
//!
//! The desktop app turns this on with [`Session::enable_recovery`]. Headless sessions (the CLI,
//! MCP, tests) and the web build never write recovery files. A workbook opened from a
//! password-protected file gets no copy: it would sit on disk without its password.
//!
//! Layout: `<root>/<id>/copy.xlsx` (the workbook, written as Save writes it), `info.json`
//! (`{name, savedAt}`), and `owner.lock`, an advisory lock the running app holds on each record
//! it owns, so a second GridCraft never lists or deletes a copy that is still in use. Encoding
//! and file I/O run on one worker thread, never while typing: a copy is a shared snapshot of the
//! workbook (`Arc`), so taking it costs nothing.

use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::{Result, Session, cmd::bad};

/// A recovery copy left by an earlier run: its record id, the workbook's title when the copy
/// was made, and when (seconds since 1970).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryInfo {
    pub id: String,
    pub name: String,
    pub saved_at: u64,
}

impl Session {
    /// Turns on recovery copies, kept in `root` (the app's settings folder + `recovery`), and
    /// starts looking for copies an earlier run left.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_recovery(&mut self, root: std::path::PathBuf) -> std::result::Result<(), String> {
        self.recovery = Some(desktop::Recovery::start(root)?);
        Ok(())
    }

    /// Called every frame by the desktop app: writes the copies that are due and removes those
    /// no workbook needs any more. Returns how soon it wants to be called again (a copy is due
    /// then, or the worker is busy), or `None` when nothing is waiting.
    pub fn recovery_tick(&mut self) -> Option<std::time::Duration> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(r) = &mut self.recovery {
            return r.tick(&self.docs, desktop::now());
        }
        None
    }

    /// [`Session::recovery_tick`] as if it were `now` (tests).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn recovery_tick_at(&mut self, now: std::time::Instant) {
        if let Some(r) = &mut self.recovery {
            r.tick(&self.docs, now);
        }
    }

    /// The recovery worker, for tests: waits for its answers, or stops it as a crash would
    /// (nothing is written or removed, and its locks go with it).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn recovery_worker(&mut self) -> Option<&mut desktop::Recovery> {
        self.recovery.as_mut()
    }

    /// Copies an earlier run left (not the open workbooks' own), newest first.
    pub fn recovery_copies(&self) -> Vec<RecoveryInfo> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(r) = &self.recovery {
            return r.copies().to_vec();
        }
        Vec::new()
    }

    /// Whether the search for an earlier run's copies has finished (the desktop app shows them
    /// once it has).
    pub fn recovery_ready(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(r) = &self.recovery {
            return r.ready();
        }
        false
    }

    /// Right before the app exits: writes a last copy of every workbook still unsaved, removes
    /// the copies no workbook needs, and waits (a few seconds at most) for the worker to finish,
    /// so a clean quit leaves nothing to recover and a forced one loses nothing.
    pub fn finish_recovery(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(r) = &mut self.recovery {
            r.finish(&self.docs);
        }
    }

    /// `file.recover`: lists the copies, restores one as a new unsaved workbook, or discards one.
    pub(crate) fn recover(&mut self, p: &Json) -> Result<Json> {
        const CMD: &str = "file.recover";
        let id = |key: &str| -> Result<Option<String>> {
            match p.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(v) => v
                    .as_str()
                    .map(|s| Some(s.to_string()))
                    .ok_or_else(|| bad(CMD, format!("`{key}` must be a recovery id (a string from `documents`)"))),
            }
        };
        match (id("restore")?, id("discard")?) {
            (Some(_), Some(_)) => Err(bad(CMD, "choose restore or discard")),
            (Some(id), None) => self.restore_copy(&id),
            (None, Some(id)) => {
                self.discard_copy(&id)?;
                Ok(self.recovery_state())
            }
            (None, None) => Ok(self.recovery_state()),
        }
    }

    /// `file.recover`'s answer: whether recovery is on, the listed copies and the last error.
    fn recovery_state(&self) -> Json {
        #[cfg(not(target_arch = "wasm32"))]
        let (enabled, error) = match &self.recovery {
            Some(r) => (r.ready(), r.error().map(str::to_string)),
            None => (false, None),
        };
        #[cfg(target_arch = "wasm32")]
        let (enabled, error): (bool, Option<String>) = (false, None);
        json!({"enabled": enabled, "documents": self.recovery_copies(), "error": error})
    }

    /// Opens a listed copy as a new workbook with unsaved changes and no file (Save asks where
    /// to put it; the workbook it was copied from stays as it was). It replaces an untouched
    /// blank Book1, as File › Open does, and keeps its copy in the record it came from until it
    /// is saved or closed.
    #[cfg(not(target_arch = "wasm32"))]
    fn restore_copy(&mut self, id: &str) -> Result<Json> {
        let r = self.recovery.as_mut().ok_or_else(disabled)?;
        let (info, bytes) = r.read(id).map_err(crate::EngineError::Other)?;
        // A copy is always written as .xlsx: anything else in its place is damage, never a file
        // to import as text. Errors name the copy as the list shows it.
        if gridcraft_xlsx::sniff(&bytes) != gridcraft_xlsx::Format::Xlsx {
            return Err(crate::EngineError::Other(format!("We can't open '{}': the recovery copy is damaged.", info.name)));
        }
        let (wb, warnings) = crate::io::open_bytes(&info.name, &bytes)?;
        if self.only_untouched_blank() {
            self.close_document(0);
        }
        let stem = std::path::Path::new(&info.name).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book");
        // A copy of a restored workbook already carries the mark.
        let stem = stem.strip_suffix(" (Recovered)").unwrap_or(stem);
        // Read from bytes, like any file: its active content asks before it runs.
        let mut doc = crate::DocState::new(wb, None, format!("{stem} (Recovered)"), crate::Trust::FromFile);
        // Unsaved until the person saves it.
        doc.saved = std::sync::Arc::new(gridcraft_model::Workbook::default());
        let uid = doc.uid;
        let i = self.add_document(doc);
        if let Some(r) = &mut self.recovery {
            r.adopt(id, uid);
        }
        let d = self.doc()?;
        Ok(json!({"index": i, "title": d.display_title(), "warnings": warnings}))
    }
    /// The web build keeps no recovery copies.
    #[cfg(target_arch = "wasm32")]
    fn restore_copy(&mut self, _id: &str) -> Result<Json> {
        Err(disabled())
    }

    /// `file.recover {discard}`: deletes a listed copy (see `desktop::Recovery::discard`).
    #[cfg(not(target_arch = "wasm32"))]
    fn discard_copy(&mut self, id: &str) -> Result<()> {
        let r = self.recovery.as_mut().ok_or_else(disabled)?;
        r.discard(id).map_err(crate::EngineError::Other)
    }
    /// The web build keeps no recovery copies.
    #[cfg(target_arch = "wasm32")]
    fn discard_copy(&mut self, _id: &str) -> Result<()> {
        Err(disabled())
    }
}

/// The error `file.recover` gives where recovery is off (headless sessions, the web build).
fn disabled() -> crate::EngineError {
    crate::EngineError::Disabled("file.recover".into(), "recovery copies are kept by the desktop app only".into())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod desktop {
    // Desktop only (the module is off wasm), where the clock works.
    #![allow(clippy::disallowed_methods)]
    use std::collections::HashMap;
    use std::fs::{self, File};
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::{Duration, Instant};

    use gridcraft_model::Workbook;
    use serde::{Deserialize, Serialize};

    use super::RecoveryInfo;
    use crate::DocState;

    pub(crate) fn now() -> Instant {
        Instant::now()
    }

    /// How often a workbook with unsaved changes gets a new copy, at most.
    pub(crate) const INTERVAL: Duration = Duration::from_secs(10);
    /// A copy bigger than this is not read back (recovery files are input like any other).
    const MAX_COPY_BYTES: u64 = 512 << 20;
    const MAX_INFO_BYTES: u64 = 64 << 10;
    /// Records looked at when listing; any past this are left alone.
    const MAX_RECORDS: usize = 256;
    /// How long quitting waits for copies still being written or removed.
    const FINISH_WAIT: Duration = Duration::from_secs(10);
    const COPY: &str = "copy.xlsx";
    const INFO: &str = "info.json";
    const LOCK: &str = "owner.lock";

    enum Job {
        Scan,
        Save {
            uid: u64,
            id: String,
            name: String,
            wb: Arc<Workbook>,
        },
        Discard(String),
        /// Keeps the worker busy until released (tests).
        #[cfg(test)]
        Hold(Receiver<()>),
    }

    enum Done {
        Scanned(Vec<RecoveryInfo>),
        Saved,
        /// Record `id` is gone.
        Discarded(String),
        /// Writing workbook `uid`'s copy failed: it is tried again at the next interval.
        SaveFailed(u64, String),
        /// Deleting record `id` failed: it stays tracked and is tried again (see `discarding`).
        DiscardFailed(String, String),
    }

    /// One open workbook's copy: its record (once one is written), the revision it holds, and
    /// when the next one may be written.
    struct Slot {
        id: Option<String>,
        revision: Option<u64>,
        due: Instant,
    }

    /// The app's recovery worker and what it knows: one per process.
    pub(crate) struct Recovery {
        root: PathBuf,
        jobs: Option<Sender<Job>>,
        done: Receiver<Done>,
        worker: Option<std::thread::JoinHandle<()>>,
        /// Jobs sent and not answered yet.
        pending: usize,
        slots: HashMap<u64, Slot>,
        /// Records being deleted, until the worker confirms it: `None` while queued, or when
        /// to try again after a failed delete, so a copy nobody needs never stays on disk.
        discarding: HashMap<String, Option<Instant>>,
        copies: Vec<RecoveryInfo>,
        ready: bool,
        error: Option<String>,
        /// How long Discard waits for the worker's answer.
        discard_wait: Duration,
    }

    impl Recovery {
        /// Creates the recovery folder, starts the worker and asks it for the copies an earlier run left.
        pub(crate) fn start(root: PathBuf) -> Result<Recovery, String> {
            private_dir(&root).map_err(|e| format!("recovery folder {}: {e}", root.display()))?;
            let (jobs, rx) = mpsc::channel();
            let (tx, done) = mpsc::channel();
            let store_root = root.clone();
            let worker = std::thread::Builder::new()
                .name("gridcraft-recovery".into())
                .spawn(move || work(Store { root: store_root, locks: HashMap::new() }, rx, tx))
                .map_err(|e| format!("recovery worker: {e}"))?;
            let mut r = Recovery {
                root,
                jobs: Some(jobs),
                done,
                worker: Some(worker),
                pending: 0,
                slots: HashMap::new(),
                discarding: HashMap::new(),
                copies: Vec::new(),
                ready: false,
                error: None,
                discard_wait: FINISH_WAIT,
            };
            r.send(Job::Scan);
            Ok(r)
        }

        /// Copies an earlier run left, newest first.
        pub(crate) fn copies(&self) -> &[RecoveryInfo] {
            &self.copies
        }
        /// Whether the first scan of the recovery folder has finished.
        pub(crate) fn ready(&self) -> bool {
            self.ready
        }
        /// The last thing that went wrong, for `file.recover`.
        pub(crate) fn error(&self) -> Option<&str> {
            self.error.as_deref()
        }

        /// Asks the worker to delete record `id` and tracks it until the worker confirms.
        fn queue_discard(&mut self, id: String) {
            self.discarding.insert(id.clone(), None);
            self.send(Job::Discard(id));
        }

        /// Hands a job to the worker and counts it until its answer arrives.
        fn send(&mut self, job: Job) {
            match self.jobs.as_ref().map(|j| j.send(job)) {
                Some(Ok(())) => self.pending += 1,
                _ => self.error = Some("the recovery worker stopped".into()),
            }
        }

        /// Takes in the worker's answers; with `wait`, until every job is answered or
        /// `deadline` passes.
        fn receive(&mut self, deadline: Option<Instant>) {
            while self.pending > 0 {
                let answer = match deadline {
                    None => match self.done.try_recv() {
                        Ok(a) => a,
                        Err(_) => return,
                    },
                    Some(at) => match self.done.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(a) => a,
                        Err(_) => return,
                    },
                };
                self.pending -= 1;
                match answer {
                    Done::Scanned(found) => {
                        self.copies = found;
                        self.ready = true;
                    }
                    Done::Saved => {}
                    Done::Discarded(id) => {
                        // Only now does a listed copy leave the list: until the worker confirms,
                        // it is still on disk.
                        self.discarding.remove(&id);
                        self.copies.retain(|c| c.id != id);
                    }
                    Done::DiscardFailed(id, e) => {
                        if let Some(retry) = self.discarding.get_mut(&id) {
                            *retry = Some(Instant::now() + INTERVAL);
                        }
                        log::warn!("recovery: {e}");
                        self.error = Some(e);
                    }
                    Done::SaveFailed(uid, e) => {
                        // The copy on disk (if any) is older than the slot says: write it again
                        // once the interval is up, even if the workbook doesn't change.
                        if let Some(slot) = self.slots.get_mut(&uid) {
                            slot.revision = None;
                        }
                        log::warn!("recovery: {e}");
                        self.error = Some(e);
                    }
                }
            }
        }

        /// See [`crate::Session::recovery_tick`].
        pub(crate) fn tick(&mut self, docs: &[DocState], now: Instant) -> Option<Duration> {
            self.receive(None);
            let mut jobs = Vec::new();
            let mut drop = Vec::new();
            // Workbooks closed since the last tick (saved, or Don't Save): their copies go.
            self.slots.retain(|uid, slot| {
                let open = docs.iter().any(|d| d.uid == *uid);
                if !open && let Some(id) = slot.id.take() {
                    drop.push(id);
                }
                open
            });
            let mut next: Option<Duration> = None;
            // Deletes that failed are tried again once their interval is up.
            for (id, retry) in &self.discarding {
                match retry {
                    Some(at) if now >= *at => drop.push(id.clone()),
                    Some(at) => next = Some(next.map_or(*at - now, |n| n.min(*at - now))),
                    None => {}
                }
            }
            for d in docs {
                let slot = self.slots.entry(d.uid).or_insert_with(|| Slot { id: None, revision: None, due: now + INTERVAL });
                if !wants_copy(d) {
                    // Saved, or undone back to the saved state: nothing to recover.
                    if let Some(id) = slot.id.take() {
                        drop.push(id);
                    }
                    slot.revision = None;
                    continue;
                }
                if slot.revision == Some(d.revision) {
                    continue;
                }
                if now < slot.due {
                    next = Some(next.map_or(slot.due - now, |n| n.min(slot.due - now)));
                    continue;
                }
                let id = slot.id.get_or_insert_with(new_id).clone();
                slot.revision = Some(d.revision);
                slot.due = now + INTERVAL;
                jobs.push(Job::Save { uid: d.uid, id, name: d.display_title(), wb: d.wb.clone() });
            }
            for id in drop {
                self.queue_discard(id);
            }
            for job in jobs {
                self.send(job);
            }
            if self.pending > 0 {
                let soon = Duration::from_millis(100);
                next = Some(next.map_or(soon, |n| n.min(soon)));
            }
            next
        }

        /// See [`crate::Session::finish_recovery`].
        pub(crate) fn finish(&mut self, docs: &[DocState]) {
            // A last copy of every unsaved change, due or not, including a workbook no tick has
            // seen yet.
            let now = Instant::now();
            for d in docs {
                self.slots.entry(d.uid).or_insert_with(|| Slot { id: None, revision: None, due: now }).due = now;
            }
            // ...and one more try at every delete that failed.
            for retry in self.discarding.values_mut() {
                if retry.is_some() {
                    *retry = Some(now);
                }
            }
            self.tick(docs, now);
            self.jobs = None;
            self.receive(Some(Instant::now() + FINISH_WAIT));
            if self.pending == 0
                && let Some(w) = self.worker.take()
            {
                let _ = w.join();
            }
        }

        /// A listed copy's info and bytes, to restore.
        pub(crate) fn read(&self, id: &str) -> Result<(RecoveryInfo, Vec<u8>), String> {
            let info = self.copies.iter().find(|c| c.id == id).cloned().ok_or("no such recovery copy")?;
            if self.discarding.contains_key(id) {
                return Err("This copy is being discarded.".into());
            }
            let bytes = read_capped(&self.root.join(id).join(COPY), MAX_COPY_BYTES)?;
            Ok((info, bytes))
        }

        /// Waits for the worker to answer everything sent so far (tests).
        #[cfg(test)]
        pub(crate) fn settle(&mut self) {
            self.receive(Some(Instant::now() + FINISH_WAIT));
        }

        /// Stops the worker the way a crash does: nothing more is written or removed (tests).
        #[cfg(test)]
        pub(crate) fn crash(&mut self) {
            self.settle();
            self.jobs = None;
            if let Some(w) = self.worker.take() {
                let _ = w.join();
            }
        }

        /// The restored workbook `uid` keeps its copy in record `id` from now on.
        pub(crate) fn adopt(&mut self, id: &str, uid: u64) {
            self.copies.retain(|c| c.id != id);
            self.slots.insert(uid, Slot { id: Some(id.to_string()), revision: None, due: Instant::now() + INTERVAL });
        }

        /// Deletes a listed copy, as the person asked. The copy stays listed until the worker
        /// confirms it is gone; Discard waits for that (a few seconds at most) and otherwise
        /// says why it isn't gone yet. A failed delete is tried again like any other.
        pub(crate) fn discard(&mut self, id: &str) -> Result<(), String> {
            if !self.copies.iter().any(|c| c.id == id) {
                return Err("no such recovery copy".into());
            }
            // Not already on its way (a failed one goes again now).
            if !matches!(self.discarding.get(id), Some(None)) {
                self.queue_discard(id.to_string());
            }
            self.receive(Some(Instant::now() + self.discard_wait));
            match self.discarding.get(id) {
                None => Ok(()),
                Some(None) => Err("The copy is still being discarded; it stays listed until it is gone.".into()),
                Some(Some(_)) => Err(self.error.clone().unwrap_or_else(|| "the recovery copy could not be removed".into())),
            }
        }

        /// Keeps the worker busy until the returned sender is used or dropped (tests).
        #[cfg(test)]
        pub(crate) fn hold(&mut self) -> Sender<()> {
            let (release, held) = mpsc::channel();
            self.send(Job::Hold(held));
            release
        }

        /// Discard's wait for the worker (tests).
        #[cfg(test)]
        pub(crate) fn set_discard_wait(&mut self, wait: Duration) {
            self.discard_wait = wait;
        }
    }

    /// A random record id: 32 lowercase hex digits.
    fn new_id() -> String {
        use std::hash::{BuildHasher, Hasher};
        let mut out = String::with_capacity(32);
        for _ in 0..2 {
            // `RandomState` is seeded from the OS's random source.
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
            out.push_str(&format!("{:016x}", h.finish()));
        }
        out
    }

    /// Whether an open workbook should have a current recovery copy.
    fn wants_copy(d: &DocState) -> bool {
        d.is_dirty() && !d.from_encrypted
    }

    /// Whether a folder name is a record id this module made (so nothing else is touched).
    fn id_ok(id: &str) -> bool {
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }

    /// Seconds since 1970, for a copy's `savedAt`.
    fn now_secs() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    /// The worker: runs jobs in order until the app drops its end of the channel.
    fn work(mut store: Store, jobs: Receiver<Job>, done: Sender<Done>) {
        for job in jobs {
            let answer = match job {
                Job::Scan => Done::Scanned(store.scan()),
                Job::Save { uid, id, name, wb } => match store.save(&id, &name, &wb) {
                    Ok(()) => Done::Saved,
                    Err(e) => Done::SaveFailed(uid, format!("could not keep a recovery copy of {name}: {e}")),
                },
                Job::Discard(id) => match store.discard(&id) {
                    Ok(()) => Done::Discarded(id),
                    Err(e) => Done::DiscardFailed(id, format!("could not remove a recovery copy: {e}")),
                },
                #[cfg(test)]
                Job::Hold(release) => {
                    let _ = release.recv();
                    Done::Saved
                }
            };
            if done.send(answer).is_err() {
                return;
            }
        }
    }

    /// The recovery folder, as the worker sees it. It holds the lock of every record this
    /// process owns or has listed.
    struct Store {
        root: PathBuf,
        locks: HashMap<String, File>,
    }

    impl Store {
        /// Locks record `id`'s `owner.lock`, creating the record. Fails when another running
        /// GridCraft holds it.
        fn lock(&mut self, id: &str) -> Result<(), String> {
            if self.locks.contains_key(id) {
                return Ok(());
            }
            let dir = self.root.join(id);
            private_dir(&dir).map_err(|e| e.to_string())?;
            let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(LOCK)).map_err(|e| e.to_string())?;
            file.try_lock().map_err(|e| format!("the recovery copy is in use: {e}"))?;
            self.locks.insert(id.to_string(), file);
            Ok(())
        }

        /// Records left by runs that have ended (their lock is free), newest first. Each stays
        /// locked by this process while it is listed. A record with no copy in it (a run that
        /// ended before its first copy was written) is removed.
        fn scan(&mut self) -> Vec<RecoveryInfo> {
            let Ok(entries) = fs::read_dir(&self.root) else { return Vec::new() };
            let mut found = Vec::new();
            for entry in entries.flatten().take(MAX_RECORDS) {
                let Some(id) = entry.file_name().to_str().filter(|n| id_ok(n)).map(str::to_string) else { continue };
                if !entry.file_type().is_ok_and(|t| t.is_dir()) || self.lock(&id).is_err() {
                    continue;
                }
                let dir = self.root.join(&id);
                // A write the crash cut short (the copy before it is whole); it can be as large as a copy.
                for tmp in [format!("{COPY}.tmp"), format!("{INFO}.tmp")] {
                    let _ = fs::remove_file(dir.join(tmp));
                }
                let copy_ok = fs::metadata(dir.join(COPY)).is_ok_and(|m| m.is_file() && m.len() <= MAX_COPY_BYTES);
                if !copy_ok {
                    if let Err(e) = self.discard(&id) {
                        log::warn!("recovery: cannot remove the empty record {id}: {e}");
                    }
                    continue;
                }
                let info: Option<Info> = read_capped(&dir.join(INFO), MAX_INFO_BYTES).ok().and_then(|b| serde_json::from_slice(&b).ok());
                let (name, saved_at) = match info {
                    Some(i) => (i.name, i.saved_at),
                    None => ("Book".into(), 0),
                };
                found.push(RecoveryInfo { id, name, saved_at });
            }
            found.sort_by_key(|a| std::cmp::Reverse(a.saved_at));
            found
        }

        /// Writes a copy of `wb` into record `id`: to a temporary file, flushed to disk, then
        /// renamed over the last copy, so a crash mid-write leaves the previous copy whole.
        fn save(&mut self, id: &str, name: &str, wb: &Workbook) -> Result<(), String> {
            self.lock(id)?;
            let dir = self.root.join(id);
            let bytes = crate::io::save_bytes(wb, COPY).map_err(|e| e.to_string())?;
            replace(&dir, COPY, &bytes)?;
            let info = serde_json::to_vec(&Info { name: name.to_string(), saved_at: now_secs() }).map_err(|e| e.to_string())?;
            replace(&dir, INFO, &info)?;
            sync_dir(&dir);
            Ok(())
        }

        /// Deletes record `id` and lets go of its lock.
        fn discard(&mut self, id: &str) -> Result<(), String> {
            if !id_ok(id) {
                return Err("invalid recovery id".into());
            }
            let dir = self.root.join(id);
            if !dir.exists() {
                self.locks.remove(id);
                return Ok(());
            }
            self.lock(id)?;
            for name in [COPY, INFO, &format!("{COPY}.tmp"), &format!("{INFO}.tmp")] {
                match fs::remove_file(dir.join(name)) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
                    _ => {}
                }
            }
            // Closing the lock first: Windows cannot delete an open file.
            self.locks.remove(id);
            let _ = fs::remove_file(dir.join(LOCK));
            fs::remove_dir(&dir).map_err(|e| e.to_string())?;
            sync_dir(&self.root);
            Ok(())
        }
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Info {
        name: String,
        saved_at: u64,
    }

    /// Replaces `dir/name` with `bytes` through a flushed temporary file.
    fn replace(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
        let tmp = dir.join(format!("{name}.tmp"));
        let written = (|| -> std::io::Result<()> {
            let mut f = private_file(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            fs::rename(&tmp, dir.join(name))
        })();
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        Ok(())
    }

    /// Reads a recovery file, refusing one larger than `cap` (recovery files are input like any other).
    fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
        let f = File::open(path).map_err(|e| e.to_string())?;
        if f.metadata().map_err(|e| e.to_string())?.len() > cap {
            return Err("the recovery copy is too large".into());
        }
        let mut out = Vec::new();
        f.take(cap + 1).read_to_end(&mut out).map_err(|e| e.to_string())?;
        if out.len() as u64 > cap {
            return Err("the recovery copy is too large".into());
        }
        Ok(out)
    }

    /// A folder only this user can read: copies hold the workbook's data.
    fn private_dir(path: &Path) -> std::io::Result<()> {
        let mut b = fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(path)
    }

    /// Creates (or empties) a file only this user can read.
    fn private_file(path: &Path) -> std::io::Result<File> {
        let mut o = fs::OpenOptions::new();
        o.create(true).truncate(true).write(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        o.open(path)
    }

    /// Flushes a folder's entries (the rename) to disk where the platform can.
    fn sync_dir(path: &Path) {
        #[cfg(unix)]
        if let Ok(d) = File::open(path) {
            let _ = d.sync_all();
        }
        #[cfg(not(unix))]
        let _ = path;
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::disallowed_methods)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use serde_json::json;

    use super::desktop::INTERVAL;
    use crate::Session;

    fn folder(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("gridcraft-recovery-{name}-{}-{nonce}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The records in the recovery folder (each a folder named by its id).
    fn records(dir: &Path) -> usize {
        std::fs::read_dir(dir).map(|e| e.flatten().filter(|e| e.path().is_dir()).count()).unwrap_or(0)
    }

    /// A desktop session on `dir`, after its first frame (the app ticks every frame from launch).
    fn app(dir: &Path) -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.enable_recovery(dir.to_path_buf()).unwrap();
        s.recovery_tick_at(Instant::now());
        s.recovery_worker().unwrap().settle();
        s
    }

    /// Two unsaved edits in the active workbook.
    fn type_candy(s: &mut Session) {
        s.execute("cell.set", json!({"cell": "A1", "input": "Peanut butter cups"})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": "12"})).unwrap();
    }

    /// The bug this prevents: an unsaved workbook was gone after a forced restart, because
    /// nothing but File › Save ever wrote it to disk.
    #[test]
    fn an_unsaved_workbook_survives_a_crash_and_restores_as_unsaved() {
        let dir = folder("crash");
        let mut a = app(&dir);
        type_candy(&mut a);
        // Not yet due: typing never writes a copy straight away.
        a.recovery_tick_at(Instant::now());
        a.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 0);
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.recovery_worker().unwrap().crash();
        assert_eq!(records(&dir), 1);

        let mut b = app(&dir);
        let copies = b.recovery_copies();
        assert_eq!(copies.len(), 1, "the next launch lists the copy");
        assert_eq!(copies[0].name, "Book1");
        assert!(copies[0].saved_at > 0);
        let listed = b.execute("file.recover", json!({})).unwrap();
        assert_eq!(listed["enabled"], true);
        assert_eq!(listed["documents"][0]["id"], copies[0].id.as_str());

        let r = b.execute("file.recover", json!({"restore": copies[0].id})).unwrap();
        assert_eq!(r["title"], "Book1 (Recovered)");
        assert_eq!(b.documents().len(), 1, "it replaces the untouched blank Book1");
        let d = b.active().unwrap();
        assert!(d.is_dirty(), "a restored copy is unsaved work");
        assert!(d.path.is_none(), "Save asks where to put it");
        assert_eq!(b.execute("cell.get", json!({"cell": "A1"})).unwrap()["value"], "Peanut butter cups");
        assert_eq!(b.execute("cell.get", json!({"cell": "B1"})).unwrap()["value"], json!(12.0));
        assert!(b.recovery_copies().is_empty(), "the restored workbook keeps the copy as its own");

        // Saving the restored workbook removes the copy.
        let saved = dir.join("candy.xlsx");
        b.execute("file.save", json!({"path": saved.to_string_lossy()})).unwrap();
        b.recovery_tick_at(Instant::now() + INTERVAL);
        b.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_copy_follows_later_changes() {
        let dir = folder("latest");
        let mut a = app(&dir);
        type_candy(&mut a);
        let t = Instant::now() + INTERVAL;
        a.recovery_tick_at(t);
        a.execute("cell.set", json!({"cell": "B1", "input": "15"})).unwrap();
        // Still inside the interval after the first copy: no second copy yet.
        a.recovery_tick_at(t);
        a.recovery_tick_at(t + INTERVAL);
        a.recovery_worker().unwrap().crash();
        let mut b = app(&dir);
        let id = b.recovery_copies()[0].id.clone();
        b.execute("file.recover", json!({"restore": id})).unwrap();
        assert_eq!(b.execute("cell.get", json!({"cell": "B1"})).unwrap()["value"], json!(15.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_closing_or_undoing_removes_the_copy() {
        let dir = folder("clean");
        let mut a = app(&dir);
        type_candy(&mut a);
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 1);
        a.execute("edit.undo", json!({})).unwrap();
        a.execute("edit.undo", json!({})).unwrap();
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 0, "undone back to the saved state");

        type_candy(&mut a);
        a.recovery_tick_at(Instant::now() + 2 * INTERVAL);
        a.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 1);
        a.execute("file.close", json!({"force": true, "document": true})).unwrap();
        a.recovery_tick_at(Instant::now() + 2 * INTERVAL);
        a.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 0, "Don't Save");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quitting_keeps_unsaved_work_and_leaves_nothing_else() {
        let dir = folder("quit");
        // No frame has run yet: quitting still writes the last change, without waiting.
        let mut a = Session::new();
        a.new_workbook();
        a.enable_recovery(dir.clone()).unwrap();
        type_candy(&mut a);
        a.finish_recovery();
        assert_eq!(records(&dir), 1);
        let mut b = app(&dir);
        assert_eq!(b.recovery_copies().len(), 1);
        let id = b.recovery_copies()[0].id.clone();
        b.execute("file.recover", json!({"discard": id.clone()})).unwrap();
        b.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 0);
        assert!(b.execute("file.recover", json!({"restore": id})).is_err(), "a discarded copy is gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bug this prevents: a copy that failed to write (disk full, a locked file) was not
    /// written again until the workbook changed, so an idle workbook kept no current copy.
    #[test]
    fn a_failed_copy_is_written_again_without_another_edit() {
        let dir = folder("retry");
        let mut a = app(&dir);
        type_candy(&mut a);
        // A file where the recovery folder should be: no record can be made, whoever runs this.
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::write(&dir, b"not a folder").unwrap();
        let t = Instant::now() + INTERVAL;
        a.recovery_tick_at(t);
        a.recovery_worker().unwrap().settle();
        assert!(a.execute("file.recover", json!({})).unwrap()["error"].is_string(), "the failure is reported");
        std::fs::remove_file(&dir).unwrap();
        std::fs::create_dir(&dir).unwrap();
        a.recovery_tick_at(t + INTERVAL);
        a.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 1, "written again at the next interval");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Makes record `id`'s copy undeletable (a folder where the file was), or deletable again.
    fn jam(dir: &Path, on: bool) {
        let copy = record(dir).join("copy.xlsx");
        if on {
            std::fs::remove_file(&copy).unwrap();
            std::fs::create_dir_all(copy.join("held")).unwrap();
        } else {
            std::fs::remove_dir_all(&copy).unwrap();
            std::fs::write(&copy, b"").unwrap();
        }
    }

    /// The bug this prevents: a copy whose delete failed was forgotten, stayed on disk and was
    /// offered again at the next start although nobody needed it.
    #[test]
    fn a_failed_delete_is_tried_again() {
        let dir = folder("delete-retry");
        let mut a = app(&dir);
        type_candy(&mut a);
        let t = Instant::now() + INTERVAL;
        a.recovery_tick_at(t);
        a.recovery_worker().unwrap().settle();
        jam(&dir, true);
        a.execute("edit.undo", json!({})).unwrap();
        a.execute("edit.undo", json!({})).unwrap();
        a.recovery_tick_at(t);
        a.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 1, "the delete failed");
        jam(&dir, false);
        a.recovery_tick_at(t + INTERVAL);
        a.recovery_worker().unwrap().settle();
        assert_eq!(records(&dir), 0, "tried again at the next interval");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Discard reports a delete that failed instead of pretending it worked.
    #[test]
    fn a_failed_discard_is_reported_and_the_copy_stays_listed() {
        let dir = folder("discard-fails");
        let mut a = app(&dir);
        type_candy(&mut a);
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.recovery_worker().unwrap().crash();
        let mut b = app(&dir);
        let id = b.recovery_copies()[0].id.clone();
        jam(&dir, true);
        assert!(b.execute("file.recover", json!({"discard": id.clone()})).is_err());
        assert_eq!(b.recovery_copies().len(), 1, "still listed, to try again");
        jam(&dir, false);
        b.execute("file.recover", json!({"discard": id})).unwrap();
        assert!(b.recovery_copies().is_empty());
        assert_eq!(records(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bug this prevents: a Discard still under way when its wait ran out reported success
    /// and dropped the copy from the list; when the delete then failed, the copy stayed on disk
    /// without being listed.
    #[test]
    fn a_discard_still_under_way_keeps_the_copy_listed_until_it_is_gone() {
        let dir = folder("discard-pending");
        let mut a = app(&dir);
        type_candy(&mut a);
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.recovery_worker().unwrap().crash();
        let mut b = app(&dir);
        let id = b.recovery_copies()[0].id.clone();
        jam(&dir, true);
        let w = b.recovery_worker().unwrap();
        w.set_discard_wait(std::time::Duration::ZERO);
        let release = w.hold();
        assert!(b.execute("file.recover", json!({"discard": id.clone()})).is_err(), "not gone yet");
        assert_eq!(b.recovery_copies().len(), 1);
        assert!(b.execute("file.recover", json!({"restore": id.clone()})).is_err(), "no restoring a copy being discarded");
        release.send(()).unwrap();
        b.recovery_worker().unwrap().settle();
        assert_eq!(b.recovery_copies().len(), 1, "the delete failed: still listed");
        jam(&dir, false);
        b.recovery_tick_at(Instant::now() + 2 * INTERVAL);
        b.recovery_worker().unwrap().settle();
        assert!(b.recovery_copies().is_empty(), "the retry removed it, and the list follows");
        assert_eq!(records(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The record of the one copy in `dir`.
    fn record(dir: &Path) -> PathBuf {
        std::fs::read_dir(dir).unwrap().flatten().find(|e| e.path().is_dir()).unwrap().path()
    }

    /// An app that crashed with unsaved candy in `dir`, and the next launch.
    fn crashed_then_relaunched(dir: &Path) -> (Session, String) {
        let mut a = app(dir);
        type_candy(&mut a);
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.recovery_worker().unwrap().crash();
        let b = app(dir);
        let id = b.recovery_copies()[0].id.clone();
        (b, id)
    }

    /// The bug this prevents: restoring, crashing and restoring again titled the workbook
    /// "Book1 (Recovered) (Recovered)".
    #[test]
    fn a_copy_restored_twice_is_marked_once() {
        let dir = folder("twice");
        let (mut b, id) = crashed_then_relaunched(&dir);
        b.execute("file.recover", json!({"restore": id})).unwrap();
        b.recovery_tick_at(Instant::now() + INTERVAL);
        b.recovery_worker().unwrap().crash();
        let mut c = app(&dir);
        let id = c.recovery_copies()[0].id.clone();
        let r = c.execute("file.recover", json!({"restore": id})).unwrap();
        assert_eq!(r["title"], "Book1 (Recovered)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bug this prevents: a copy holding text restored as a text import instead of failing,
    /// and a damaged copy's error named `recovered.xlsx`, a file the person never sees.
    #[test]
    fn a_damaged_copy_is_refused_by_its_listed_name() {
        let dir = folder("damaged");
        let (mut b, id) = crashed_then_relaunched(&dir);
        let copy = record(&dir).join("copy.xlsx");
        for junk in [&b"Peanut butter cups,12\n"[..], b"PK\x03\x04 cut short"] {
            std::fs::write(&copy, junk).unwrap();
            let e = b.execute("file.recover", json!({"restore": id.clone()})).unwrap_err().to_string();
            assert!(e.contains("'Book1'") && !e.contains("recovered.xlsx"), "{e}");
            assert_eq!(b.documents().len(), 1, "the blank Book1 stays");
            assert_eq!(b.recovery_copies().len(), 1, "still listed, to discard");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bug this prevents: a crash mid-write left `copy.xlsx.tmp`, as large as a copy, in the
    /// record for as long as the record stayed.
    #[test]
    fn a_write_a_crash_cut_short_is_cleared_at_the_next_start() {
        let dir = folder("tmp");
        let mut a = app(&dir);
        type_candy(&mut a);
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.recovery_worker().unwrap().crash();
        let tmp = record(&dir).join("copy.xlsx.tmp");
        std::fs::write(&tmp, b"half a copy").unwrap();
        let b = app(&dir);
        assert!(!tmp.exists());
        assert_eq!(b.recovery_copies().len(), 1, "the last whole copy is still offered");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_running_app_never_lists_a_live_copy() {
        let dir = folder("live");
        let mut a = app(&dir);
        type_candy(&mut a);
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.recovery_worker().unwrap().settle();
        let b = app(&dir);
        assert!(b.recovery_copies().is_empty(), "the first app still holds its copy");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_workbook_from_a_password_protected_file_gets_no_copy() {
        let dir = folder("encrypted");
        let mut a = app(&dir);
        type_candy(&mut a);
        if let Some(d) = a.active_mut() {
            d.from_encrypted = true;
        }
        a.recovery_tick_at(Instant::now() + INTERVAL);
        a.finish_recovery();
        assert_eq!(records(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_the_desktop_app_nothing_is_written() {
        let mut s = Session::new();
        s.new_workbook();
        type_candy(&mut s);
        assert_eq!(s.recovery_tick(), None);
        let r = s.execute("file.recover", json!({})).unwrap();
        assert_eq!(r["enabled"], false);
        assert!(s.execute("file.recover", json!({"restore": "0"})).is_err());
        assert!(s.execute("file.recover", json!({"restore": 3})).is_err());
    }
}
