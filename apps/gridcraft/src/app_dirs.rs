//! The one place a GridCraft data root is resolved, so the nightly profile, a distro wrapper's
//! `GRIDCRAFT_CONFIG_DIR` and the platform default can never disagree.
//!
//! Everything the desktop writes about the user lives under [`config_dir`]: `ui.json`,
//! `prefs.json` and `logs/` (see `logging`). The control channel's token folder is resolved by
//! `gridcraft_mcp` from the same root at start-up. Nothing else in the app persists user state.

use std::ffi::OsString;
use std::path::PathBuf;

/// A wrapper's explicit data folder (a distro package that must not share the installed app's).
/// It must be absolute. Nightly refuses it: the Launcher alone names a nightly root.
pub const CONFIG_DIR_ENV: &str = "GRIDCRAFT_CONFIG_DIR";

fn default_config_dir(platform: &str, env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    match platform {
        "macos" => env("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/GridCraft")),
        "windows" => env("APPDATA").map(|a| PathBuf::from(a).join("GridCraft")),
        _ => env("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("gridcraft")),
    }
}

/// The folder the settings and the log files live in.
pub fn config_dir_for(platform: &str, env: &impl Fn(&str) -> Option<OsString>, nightly_root: Option<&std::path::Path>) -> Option<PathBuf> {
    if let Some(root) = nightly_root {
        // The profile was already validated (absolute, fenced, writable) before this runs.
        return Some(root.to_path_buf());
    }
    match env(CONFIG_DIR_ENV).filter(|v| !v.is_empty()).map(PathBuf::from) {
        // A relative folder would resolve against the working directory, which is not the user's.
        Some(custom) if custom.is_absolute() => Some(custom),
        Some(_) => None,
        None => default_config_dir(platform, env),
    }
}

pub fn config_dir() -> Option<PathBuf> {
    config_dir_for(std::env::consts::OS, &|key| std::env::var_os(key), crate::launch_profile::current().root())
}

/// Where the log files live: `logs` in the settings folder, next to `ui.json`.
pub fn log_dir() -> Option<PathBuf> {
    Some(config_dir()?.join("logs"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: BTreeMap<String, OsString> = pairs.iter().map(|(k, v)| ((*k).to_string(), OsString::from(v))).collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn each_platform_keeps_its_published_folder() {
        let home = if cfg!(windows) { ("HOME", r"C:\Users\u") } else { ("HOME", "/home/u") };
        assert_eq!(
            config_dir_for("macos", &env(&[home]), None),
            Some(PathBuf::from("/home/u/Library/Application Support/GridCraft"))
        );
        assert_eq!(config_dir_for("windows", &env(&[("APPDATA", r"C:\Users\u\AppData\Roaming")]), None), Some(PathBuf::from(r"C:\Users\u\AppData\Roaming\GridCraft")));
        assert_eq!(config_dir_for("linux", &env(&[home]), None), Some(PathBuf::from("/home/u/.config/gridcraft")));
        assert_eq!(config_dir_for("linux", &env(&[("XDG_CONFIG_HOME", "/xdg"), home]), None), Some(PathBuf::from("/xdg/gridcraft")));
        assert_eq!(config_dir_for("linux", &env(&[("XDG_CONFIG_HOME", "")]), None), None);
    }

    #[test]
    fn an_absolute_wrapper_folder_is_honoured_and_a_relative_one_is_not() {
        let base = [("HOME", "/home/u")];
        assert_eq!(
            config_dir_for("linux", &env(&[("GRIDCRAFT_CONFIG_DIR", "/opt/gridcraft-nightly"), base[0]]), None),
            Some(PathBuf::from("/opt/gridcraft-nightly"))
        );
        assert_eq!(config_dir_for("linux", &env(&[("GRIDCRAFT_CONFIG_DIR", "gridcraft-data"), base[0]]), None), None, "a relative folder is not the user's");
    }

    #[test]
    fn the_nightly_profile_wins_and_never_consults_the_environment() {
        let profile = std::path::Path::new("/var/lib/gridcraft-nightly/profile");
        assert_eq!(
            config_dir_for("linux", &env(&[("GRIDCRAFT_CONFIG_DIR", "/opt/stable"), ("HOME", "/home/u")]), Some(profile)),
            Some(profile.to_path_buf())
        );
        assert_eq!(config_dir_for("linux", &env(&[("HOME", "/home/u")]), Some(profile)).unwrap().join("logs"), profile.join("logs"));
    }
}
