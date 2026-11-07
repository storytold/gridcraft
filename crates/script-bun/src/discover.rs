//! Finding Bun. It is never bundled: the user installs it, points `$GRIDCRAFT_BUN` at it, or
//! puts it in the model root's `runtimes/bun` folder (the same place model weights live, see
//! `docs/office-scripts.md`).

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The executable's file name on this platform.
pub fn bun_exe_name() -> &'static str {
    if cfg!(windows) { "bun.exe" } else { "bun" }
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
            if candidate.is_file() {
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

/// Where the app would put Bun: `<model root>/runtimes/bun/bun(.exe)`.
pub fn expected_path() -> PathBuf {
    let root = models_root().unwrap_or_else(|| PathBuf::from("models"));
    root.join("runtimes").join("bun").join(bun_exe_name())
}

/// The model root, first hit wins: `$GRIDCRAFT_MODELS`, the platform settings folder's `models`,
/// `models` beside the executable, then `models` in the working directory.
pub fn models_root() -> Option<PathBuf> {
    if let Some(configured) = std::env::var_os("GRIDCRAFT_MODELS") {
        return Some(PathBuf::from(configured));
    }
    if let Some(dir) = config_root() {
        return Some(dir.join("models"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
        && parent.join("models").is_dir()
    {
        return Some(parent.join("models"));
    }
    let local = PathBuf::from("models");
    local.is_dir().then_some(local)
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
/// non-Bun file the user pointed at can never hang discovery.
pub fn version_of(exe: &Path, timeout: Duration) -> Option<String> {
    let exe = exe.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new(&exe).arg("--version").stdin(std::process::Stdio::null()).output();
        let version = out.ok().filter(|o| o.status.success()).and_then(|o| String::from_utf8(o.stdout).ok());
        let _ = tx.send(version);
    });
    let text = rx.recv_timeout(timeout).ok().flatten()?;
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
}
