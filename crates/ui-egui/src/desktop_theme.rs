//! Standard XDG appearance settings, shared with the other Craft desktop hosts.
//!
//! The watcher decodes numeric `org.freedesktop.appearance/color-scheme`, wakes egui on changes,
//! bounds bus calls, and stops when dropped. Non-Linux/model-only builds remain inert.

pub use craft_desktop::appearance::Watcher as DesktopTheme;
