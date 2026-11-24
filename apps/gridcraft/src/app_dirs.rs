//! The one place a GridCraft data root is resolved, so the nightly profile and the platform
//! default can never disagree.
//!
//! Everything the desktop writes about the user lives under [`config_dir`]: `ui.json`,
//! `prefs.json` and `logs/` (see `logging`). The control channel's token folder is resolved by
//! `gridcraft_mcp` from the same root at start-up. Nothing else in the app persists user state.
//! A nightly takes its root from the Launcher's validated profile and nothing else, so stable has
//! no environment override of its own: the platform's published folder is the whole story.

use std::ffi::OsString;
use std::path::PathBuf;

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
    match nightly_root {
        // The profile was already validated (absolute, fenced, writable) before this runs.
        Some(root) => Some(root.to_path_buf()),
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
        assert_eq!(config_dir_for("macos", &env(&[home]), None), Some(PathBuf::from("/home/u/Library/Application Support/GridCraft")));
        assert_eq!(
            config_dir_for("windows", &env(&[("APPDATA", r"C:\Users\u\AppData\Roaming")]), None),
            Some(PathBuf::from(r"C:\Users\u\AppData\Roaming\GridCraft"))
        );
        assert_eq!(config_dir_for("linux", &env(&[home]), None), Some(PathBuf::from("/home/u/.config/gridcraft")));
        assert_eq!(config_dir_for("linux", &env(&[("XDG_CONFIG_HOME", "/xdg"), home]), None), Some(PathBuf::from("/xdg/gridcraft")));
        assert_eq!(config_dir_for("linux", &env(&[("XDG_CONFIG_HOME", "")]), None), None);
    }

    #[test]
    fn an_empty_xdg_config_home_falls_back_to_the_home_folder() {
        let home = [("HOME", "/home/u")];
        assert_eq!(config_dir_for("linux", &env(&[("XDG_CONFIG_HOME", ""), home[0]]), None), Some(PathBuf::from("/home/u/.config/gridcraft")));
        assert_eq!(config_dir_for("linux", &env(&[("XDG_CONFIG_HOME", "/xdg"), home[0]]), None), Some(PathBuf::from("/xdg/gridcraft")));
    }

    #[test]
    fn the_nightly_profile_is_the_whole_answer() {
        let profile = std::path::Path::new("/var/lib/gridcraft-nightly/profile");
        assert_eq!(config_dir_for("linux", &env(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/u")]), Some(profile)), Some(profile.to_path_buf()));
        assert_eq!(config_dir_for("linux", &env(&[("HOME", "/home/u")]), Some(profile)).unwrap().join("logs"), profile.join("logs"));
    }
}
