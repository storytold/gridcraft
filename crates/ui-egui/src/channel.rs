//! The compiled channel this process belongs to: its OS identity, its window titles and the
//! version the Help ▸ About and File ▸ Account views show.
//!
//! The desktop selects it once, before any window exists (`prepare_launch`), so a launch cannot
//! change it later and nothing here is a user preference. A stable build keeps its old identity
//! and titles byte for byte; the nightly build carries its own app id so docks, window managers
//! and the taskbar keep the two channels apart.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// The stable window/Wayland app id, matching the `.desktop` file and the hicolor icon name.
pub const APP_ID: &str = "ai.storyteller.gridcraft";
/// The nightly window id and Windows AUMID. The package/bundle identity is
/// `ai.storyteller.gridcraft.nightly` (a different string, see packaging/).
pub const NIGHTLY_APP_ID: &str = "ai.storyteller.gridcraft.nightly-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    NightlyV1,
}

static SELECTED: AtomicU8 = AtomicU8::new(0);
static VERSION: OnceLock<&'static str> = OnceLock::new();

/// Fix the channel and its version for this process, once. `version` is the build's own identity
/// (`GRIDCRAFT_BUILT_VERSION` from `build.rs`), not a value read from the environment at runtime.
pub fn select(channel: Channel, version: &'static str) -> Result<(), String> {
    let value = match channel {
        Channel::Stable => 1,
        Channel::NightlyV1 => 2,
    };
    SELECTED
        .compare_exchange(0, value, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "the channel was already selected".to_string())?;
    VERSION.set(version).map_err(|_| "the channel version was already selected".to_string())
}

pub fn current() -> Channel {
    if SELECTED.load(Ordering::Acquire) == 2 { Channel::NightlyV1 } else { Channel::Stable }
}

pub const fn id_for(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => APP_ID,
        Channel::NightlyV1 => NIGHTLY_APP_ID,
    }
}

pub fn app_id() -> &'static str {
    id_for(current())
}

pub const fn name_for(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "GridCraft",
        Channel::NightlyV1 => "GridCraft Nightly",
    }
}

pub fn name() -> &'static str {
    name_for(current())
}

/// The title of a window that shows no document yet: the channel's own name.
pub fn default_title() -> &'static str {
    name()
}

/// Only a stable build runs the stable-only services (the updater, the shared suite state).
pub const fn stable_services_for(channel: Channel) -> bool {
    matches!(channel, Channel::Stable)
}

pub fn stable_services_allowed() -> bool {
    stable_services_for(current())
}

/// The version to show: the selected build's own identity, else the crate version (a web build,
/// or a shell that never selected a channel).
pub fn version_for(channel: Channel, configured: Option<&'static str>) -> &'static str {
    match (channel, configured) {
        (Channel::NightlyV1, Some(version)) => version,
        _ => env!("CARGO_PKG_VERSION"),
    }
}

pub fn version() -> &'static str {
    version_for(current(), VERSION.get().copied())
}

/// The commit a nightly was built from, read back from its version (`…g<sha12>`), if any.
pub fn commit_for(version: &str) -> Option<&str> {
    let (_, suffix) = version.split_once("-nightly.")?;
    let (_, sha) = suffix.rsplit_once(".g")?;
    (sha.len() == 12 && sha.bytes().all(|b| b.is_ascii_hexdigit())).then_some(sha)
}

pub fn commit() -> Option<&'static str> {
    (current() == Channel::NightlyV1).then_some(version()).and_then(commit_for)
}

/// A window title that names the channel whenever a document is shown in it. A stable window keeps
/// exactly the title it always had; a nightly one appends its channel, so two windows on the same
/// workbook are never mistaken for each other in the taskbar or the window menu.
pub fn window_title_for(channel: Channel, base: &str) -> String {
    match channel {
        Channel::Stable => base.to_string(),
        Channel::NightlyV1 => format!("{base} \u{2014} {}", name_for(channel)),
    }
}

pub fn window_title(base: &str) -> String {
    window_title_for(current(), base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_channels_have_distinct_ids_and_names() {
        assert_eq!(id_for(Channel::Stable), "ai.storyteller.gridcraft");
        assert_eq!(id_for(Channel::NightlyV1), "ai.storyteller.gridcraft.nightly-v1");
        assert_ne!(id_for(Channel::Stable), id_for(Channel::NightlyV1));
        assert_eq!(name_for(Channel::Stable), "GridCraft");
        assert_eq!(name_for(Channel::NightlyV1), "GridCraft Nightly");
        assert!(stable_services_for(Channel::Stable));
        assert!(!stable_services_for(Channel::NightlyV1));
    }

    #[test]
    fn stable_titles_and_version_are_unchanged() {
        for base in ["Book1", "Book1 — Edited", "", "sales.xlsx"] {
            assert_eq!(window_title_for(Channel::Stable, base), base);
        }
        assert_eq!(version_for(Channel::Stable, Some("0.19.1-nightly.20261124.7.1.gabcdef012345")), env!("CARGO_PKG_VERSION"));
        assert_eq!(version_for(Channel::Stable, None), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn nightly_titles_name_the_channel_and_keep_the_document() {
        assert_eq!(window_title_for(Channel::NightlyV1, "Book1"), "Book1 \u{2014} GridCraft Nightly");
        assert_eq!(window_title_for(Channel::NightlyV1, ""), " \u{2014} GridCraft Nightly");
        let nightly = "0.19.1-nightly.20261124.7.1.gabcdef012345";
        assert_eq!(version_for(Channel::NightlyV1, Some(nightly)), nightly);
        assert_eq!(version_for(Channel::NightlyV1, None), env!("CARGO_PKG_VERSION"), "a nightly build without its own version is not invented");
    }

    #[test]
    fn the_commit_is_read_back_only_from_a_nightly_version() {
        assert_eq!(commit_for("0.19.1-nightly.20261124.7.1.gabcdef012345"), Some("abcdef012345"));
        assert_eq!(commit_for("0.19.1"), None);
        assert_eq!(commit_for("0.19.1-nightly.20261124.7.1"), None);
        assert_eq!(commit_for("0.19.1-nightly.20261124.7.1.gABCDEF012345"), None, "versions are lower-case");
        assert_eq!(commit_for("0.19.1-nightly.20261124.7.1.gabcdef01234"), None);
    }
}
