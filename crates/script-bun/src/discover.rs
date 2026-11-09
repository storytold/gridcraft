//! Finding Bun. It is never bundled: the user installs it, points `$GRIDCRAFT_BUN` at it, or
//! puts it in the model root's `runtimes/bun` folder (the same place model weights live, see
//! `docs/office-scripts.md`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The executable's file name on this platform.
pub fn bun_exe_name() -> &'static str {
    if cfg!(windows) { "bun.exe" } else { "bun" }
}

/// Whether the file answers `--version`: a candidate that does not is not a usable runtime.
fn usable(exe: &Path) -> bool {
    version_of(exe, Duration::from_secs(3)).is_some()
}

/// The first Bun that exists: `$GRIDCRAFT_BUN`, then `PATH`, then the model root.
pub fn find_exe() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("GRIDCRAFT_BUN") {
        let path = PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(bun_exe_name());
            // A file named `bun` that does not answer `--version` is not Bun: keep looking, so a
            // stale entry earlier on PATH cannot hide a working one (the explicit override above
            // is taken as given).
            if candidate.is_file() && usable(&candidate) {
                return Some(candidate);
            }
        }
    }
    let root = models_root()?;
    let dir = root.join("runtimes").join("bun");
    for name in [bun_exe_name(), "bun"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Where the app would put Bun: `<model root>/runtimes/bun/bun(.exe)`. Unlike [`models_root`] it
/// answers even before the folder exists (the platform settings folder is the default place to
/// install), so Import Bun has a target on a fresh machine.
pub fn expected_path() -> PathBuf {
    let root = models_root().or_else(|| config_root().map(|dir| dir.join("models"))).unwrap_or_else(|| PathBuf::from("models"));
    root.join("runtimes").join("bun").join(bun_exe_name())
}

/// The model root, first hit wins: `$GRIDCRAFT_MODELS`, the platform settings folder's `models`,
/// `models` beside the executable, then `models` in the working directory. Every candidate must
/// exist: a settings folder that is merely configured must not hide a portable install beside the
/// executable.
pub fn models_root() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("GRIDCRAFT_MODELS") {
        return Some(PathBuf::from(configured));
    }
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|parent| parent.join("models")));
    let local = PathBuf::from("models");
    let candidates = config_root().map(|dir| dir.join("models")).into_iter().chain(beside).chain([local]);
    first_existing(candidates)
}

/// The first candidate that is a folder. A path that is only configured (or that a file occupies)
/// is skipped, so a portable install is still found.
fn first_existing(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| candidate.is_dir())
}

fn config_root() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/GridCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("GridCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home().map(|h| h.join(".config"))).map(|c| c.join("gridcraft"))
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `bun --version`, when the executable answers within `timeout`. It runs on a thread so a
/// non-Bun file the user pointed at can never hang discovery; a probe that outlives the timeout is
/// killed and reaped, so repeated imports cannot leak processes or threads.
pub fn version_of(exe: &Path, timeout: Duration) -> Option<String> {
    let mut child = std::process::Command::new(exe)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let child = std::sync::Arc::new(std::sync::Mutex::new(child));
    let (tx, rx) = std::sync::mpsc::channel();
    let reaper = child.clone();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stdout.by_ref().take(64 * 1024).read_to_end(&mut text);
        // Poll with `try_wait` and drop the lock between polls: the timeout path must be able to
        // take the lock and kill a child that never exits (a full pipe, or stdout closed and the
        // program still running).
        loop {
            match reaper.lock() {
                Ok(mut child) => match child.try_wait() {
                    Ok(Some(status)) => {
                        let _ = tx.send((status.success(), text));
                        return;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        let _ = tx.send((false, text));
                        return;
                    }
                },
                Err(_) => {
                    let _ = tx.send((false, text));
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let Ok((ok, text)) = rx.recv_timeout(timeout) else {
        if let Ok(mut child) = child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        return None;
    };
    if !ok {
        return None;
    }
    let text = String::from_utf8(text).ok()?;
    let first = text.lines().next().unwrap_or("").trim();
    if first.is_empty() { None } else { Some(first.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_expected_path_ends_in_the_bun_folder() {
        let path = expected_path();
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some(bun_exe_name()));
        assert!(path.parent().is_some_and(|p| p.ends_with(Path::new("runtimes").join("bun"))));
    }

    #[test]
    fn the_exe_name_matches_the_platform() {
        if cfg!(windows) {
            assert_eq!(bun_exe_name(), "bun.exe");
        } else {
            assert_eq!(bun_exe_name(), "bun");
        }
    }

    #[test]
    fn a_missing_program_has_no_version() {
        assert_eq!(version_of(Path::new("definitely-not-a-real-program-xyz"), Duration::from_millis(500)), None);
    }

    #[test]
    fn a_file_that_does_not_answer_is_not_usable() {
        let path = std::env::temp_dir().join(format!("gridcraft-not-bun-{}", std::process::id()));
        std::fs::write(&path, b"not a runtime").unwrap();
        assert!(!usable(&path), "a text file is not a usable Bun");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_first_existing_folder_wins_and_missing_ones_are_skipped() {
        let base = std::env::temp_dir().join(format!("gridcraft-discover-{}", std::process::id()));
        let missing = base.join("missing");
        let real = base.join("real");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&real).unwrap();
        // A file where a folder would be is not a folder.
        let file = base.join("file");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(first_existing([missing.clone(), file, real.clone()]), Some(real));
        assert_eq!(first_existing([missing.clone()]), None);
        // Whatever the machine has, a model root that is found exists.
        if let Some(root) = models_root() {
            assert!(root.is_dir(), "{} is not a folder", root.display());
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
