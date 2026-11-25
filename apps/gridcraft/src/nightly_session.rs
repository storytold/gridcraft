//! App-private Nightly session health, not telemetry and not a Launcher protocol.
//!
//! A live owner holds a separate persistent lease; secondary launches never rewrite its record.
//! Dropping the guard does not clear the marker: only the app's controlled return path does, and
//! on macOS that path is `on_exit` (a ⌘Q ends the process inside AppKit's terminate, after
//! `on_exit`, so nothing after `eframe::run_native` runs there).

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

const RECORD: &str = "nightly-session.json";
const LEASE: &str = "nightly-session.lock";
const MAX_METADATA_BYTES: usize = 256;

pub struct Session {
    _lease: File,
    path: PathBuf,
    own_record: Vec<u8>,
    previous_unclean: bool,
}

fn regular_or_missing(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(io::Error::other("Nightly session state must be a regular file, not an alias or directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Replace `path` with `bytes` without following a link at the destination: the temporary file is
/// created exclusively beside it and renamed over it, so a planted link is replaced, not written
/// through. `gridcraft_engine::io::write_file` does the same for workbooks; the session record is
/// the app's own file and must not depend on the engine's script sandbox policy.
fn atomic_write_no_follow(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path.file_name().ok_or_else(|| io::Error::other("the session record has no file name"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".new");
    let tmp = path.with_file_name(tmp_name);
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let write = (|| -> io::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write
}

/// The owner's session, shared by the app's exit hook and the return path.
pub type Shared = Arc<Mutex<Option<Session>>>;

/// The controlled return ([`Session::finish`]) by whichever path comes first; later calls do nothing.
pub fn finish_once(shared: &Shared, unclean_notice_shown: bool) {
    let watch = shared.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(watch) = watch
        && let Err(error) = watch.finish(unclean_notice_shown)
    {
        log::warn!("Nightly session state was left intact: {error}");
    }
}

impl Session {
    /// The warning belongs to this window: a busy secondary has no health-record coverage, while
    /// Stable and a lease-owning Nightly window preserve their existing result.
    pub fn begin_with_health(root: Option<&Path>, version: &str) -> io::Result<(Option<Self>, bool)> {
        let watch = Self::begin(root, version)?;
        let unavailable = root.is_some() && watch.is_none();
        Ok((watch, unavailable))
    }

    /// `None` is stable, or another owner of this profile. Neither case touches the record.
    /// The supplied root must already be the validated private Nightly profile.
    pub fn begin(root: Option<&Path>, version: &str) -> io::Result<Option<Self>> {
        let Some(root) = root else { return Ok(None) };
        if version.is_empty() || version.len() > MAX_METADATA_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Nightly session provenance is missing or too large"));
        }
        let lease_path = root.join(LEASE);
        regular_or_missing(&lease_path)?;
        let lease = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(lease_path)?;
        match lease.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        let path = root.join(RECORD);
        let previous_unclean = regular_or_missing(&path)?;
        // The commit is read back from the version (`…g<sha12>`), never invented here.
        let commit = gridcraft_ui_egui::channel::commit_for(version);
        let own_record = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "channel": "nightly-v1",
            "version": version,
            "commit": commit,
        }))?;
        atomic_write_no_follow(&path, &own_record)?;
        Ok(Some(Self { _lease: lease, path, own_record, previous_unclean }))
    }

    pub fn previous_unclean(&self) -> bool {
        self.previous_unclean
    }

    /// Controlled return. If an earlier session's interruption was detected but no window showed it
    /// yet (a failed start before the app existed), the record is left for the next owner.
    pub fn finish(self, unclean_notice_shown: bool) -> io::Result<()> {
        if self.previous_unclean && !unclean_notice_shown {
            return Ok(());
        }
        self.complete()
    }

    /// Clear the record this owner wrote, and only that one. An unwind, abort, forced exit or killed
    /// process never calls this.
    pub fn complete(self) -> io::Result<()> {
        if !regular_or_missing(&self.path)? {
            return Ok(());
        }
        let mut current = Vec::new();
        File::open(&self.path)?.take(self.own_record.len() as u64 + 1).read_to_end(&mut current)?;
        if current != self.own_record {
            return Err(io::Error::other("Nightly session state changed; leaving it intact"));
        }
        std::fs::remove_file(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            loop {
                let path =
                    std::env::temp_dir().join(format!("gridcraft-nightly-session-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create owned fixture: {error}"),
                }
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn version() -> &'static str {
        "0.19.1-nightly.20261124.7.1.gabcdef012345"
    }

    #[test]
    fn busy_secondary_reports_unavailable_without_replacing_the_owner_record() {
        let fixture = Fixture::new();
        let (stable, warning) = Session::begin_with_health(None, "stable").unwrap();
        assert!(stable.is_none() && !warning);
        let (first, warning) = Session::begin_with_health(Some(&fixture.0), version()).unwrap();
        assert!(!warning);
        let first = first.unwrap();
        let record = std::fs::read(fixture.0.join(RECORD)).unwrap();
        let (secondary, warning) = Session::begin_with_health(Some(&fixture.0), version()).unwrap();
        assert!(secondary.is_none() && warning, "a secondary window cannot claim recorded exit/crash coverage");
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), record);
        first.complete().unwrap();
        assert!(!fixture.0.join(RECORD).exists());
    }

    #[test]
    fn controlled_completion_is_clean_but_an_unfinished_owner_is_reported_next_time() {
        let fixture = Fixture::new();
        let first = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        assert!(!first.previous_unclean());
        let own = std::fs::read(fixture.0.join(RECORD)).unwrap();
        assert!(Session::begin(Some(&fixture.0), version()).unwrap().is_none());
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), own, "a busy secondary must not replace the owner's record");
        first.complete().unwrap();
        assert!(!fixture.0.join(RECORD).exists());
        let unfinished = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        assert!(!unfinished.previous_unclean());
        drop(unfinished); // Abnormal-path ownership release, not a controlled completion.
        let next = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        assert!(next.previous_unclean());
        next.complete().unwrap();
        assert!(!fixture.0.join(RECORD).exists());
    }

    #[test]
    fn an_interruption_no_window_showed_survives_a_handled_startup_failure() {
        let fixture = Fixture::new();
        drop(Session::begin(Some(&fixture.0), version()).unwrap().unwrap());
        // The renderer failed before the app existed: no notice was shown.
        let failed = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        assert!(failed.previous_unclean());
        failed.finish(false).unwrap();
        // The retry (or the next start) still sees the interruption and shows it.
        let retry = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        assert!(retry.previous_unclean());
        retry.finish(true).unwrap();
        assert!(!fixture.0.join(RECORD).exists(), "a session that showed the notice clears its record");
        let clean = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        assert!(!clean.previous_unclean());
        clean.finish(false).unwrap();
        assert!(!fixture.0.join(RECORD).exists());
    }

    #[test]
    fn the_exit_hook_and_the_return_path_finish_the_session_once() {
        let fixture = Fixture::new();
        let shared: Shared = Arc::new(Mutex::new(Session::begin(Some(&fixture.0), version()).unwrap()));
        // The exit hook (macOS quit: the only path that runs there).
        finish_once(&shared, false);
        assert!(!fixture.0.join(RECORD).exists(), "a quit that ran the exit hook is a clean exit");
        let successor = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        assert!(!successor.previous_unclean());
        let record = std::fs::read(fixture.0.join(RECORD)).unwrap();
        finish_once(&shared, false);
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), record, "the second finish must not touch a successor's record");
        successor.complete().unwrap();
    }

    #[test]
    fn cleanup_never_removes_an_unknown_changed_record() {
        let fixture = Fixture::new();
        let watch = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        std::fs::write(fixture.0.join(RECORD), b"replacement owned by someone else").unwrap();
        assert!(watch.complete().is_err());
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), b"replacement owned by someone else");
    }

    #[test]
    fn provenance_is_bounded_and_never_collects_documents_paths_or_diagnostics() {
        let fixture = Fixture::new();
        assert!(Session::begin(Some(&fixture.0), &"x".repeat(MAX_METADATA_BYTES + 1)).is_err());
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 0);
        assert!(Session::begin(None, "").unwrap().is_none(), "stable has no session-health side effects");
        let watch = Session::begin(Some(&fixture.0), version()).unwrap().unwrap();
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture.0.join(RECORD)).unwrap()).unwrap();
        let fields = data.as_object().unwrap().keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(fields, ["channel", "commit", "schema_version", "version"].into_iter().collect());
        assert_eq!(data["commit"], "abcdef012345", "the commit comes from the version, not the environment");
        watch.complete().unwrap();
    }

    #[test]
    fn a_version_without_a_commit_does_not_invent_one() {
        let fixture = Fixture::new();
        let watch = Session::begin(Some(&fixture.0), "0.19.1").unwrap().unwrap();
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture.0.join(RECORD)).unwrap()).unwrap();
        assert!(data["commit"].is_null());
        watch.complete().unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn reserved_state_aliases_do_not_modify_external_files() {
        let fixture = Fixture::new();
        let external = fixture.0.join("stable-document");
        std::fs::write(&external, b"stable bytes").unwrap();
        for name in [LEASE, RECORD] {
            let alias = fixture.0.join(name);
            std::os::unix::fs::symlink(&external, &alias).unwrap();
            assert!(Session::begin(Some(&fixture.0), version()).is_err());
            assert_eq!(std::fs::read(&external).unwrap(), b"stable bytes");
            std::fs::remove_file(alias).unwrap();
        }
    }

    #[test]
    #[cfg(unix)]
    fn replacing_a_planted_record_link_writes_our_own_file_only() {
        let fixture = Fixture::new();
        let external = fixture.0.join("stable-document");
        std::fs::write(&external, b"stable bytes").unwrap();
        std::os::unix::fs::symlink(&external, fixture.0.join(RECORD)).unwrap();
        // regular_or_missing refuses an alias outright; the temporary writer is what a first write
        // uses, and it never follows one either.
        assert!(Session::begin(Some(&fixture.0), version()).is_err());
        // The link is at RECORD itself: writing there must replace the alias, not follow it.
        atomic_write_no_follow(&fixture.0.join(RECORD), b"ours").unwrap();
        assert_eq!(std::fs::read(fixture.0.join(RECORD)).unwrap(), b"ours");
        assert!(!fixture.0.join(RECORD).is_symlink(), "the alias must be replaced, not written through");
        assert_eq!(std::fs::read(&external).unwrap(), b"stable bytes");
        assert!(!fixture.0.join(".nightly-session.json.new").exists());
    }
}
