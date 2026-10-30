//! Icon names: `from_name` resolves every documented name, and no name is listed twice.
//!
//! The tables are [`icons::LIBRARY`] (the Insert › Icons gallery) and [`icons::MODEL_ICONS`] (the
//! Power Pivot / data-model vocabulary). Both are documented in the `icons` module doc.

use gridcraft_ui_egui::icons::{self, Icon};

fn named() -> impl Iterator<Item = (&'static str, Icon)> {
    icons::LIBRARY.iter().chain(icons::MODEL_ICONS).copied()
}

#[test]
fn every_documented_name_resolves_to_its_icon() {
    for (name, icon) in named() {
        assert_eq!(icons::from_name(name), Some(icon), "{name:?} must resolve to {icon:?}");
        assert_eq!(icons::from_name(&name.to_ascii_uppercase()), Some(icon), "names are case-insensitive: {name:?}");
    }
}

#[test]
fn every_data_model_name_resolves() {
    for (name, icon) in icons::MODEL_ICONS.iter().copied() {
        assert_eq!(icons::from_name(name), Some(icon), "{name:?}");
    }
    // Every data-model glyph answers to at least one name; the table is documented in icons.rs.
    // (A gap here means a variant was added without a `MODEL_ICONS` row.)
    let reached: std::collections::HashSet<Icon> = icons::MODEL_ICONS.iter().map(|(_, icon)| *icon).collect();
    assert_eq!(reached.len(), 29, "each of the 29 data-model glyphs answers to at least one name");
}

#[test]
fn no_name_is_listed_twice() {
    let mut seen = std::collections::HashSet::new();
    for (name, _) in named() {
        assert!(seen.insert(name.to_ascii_lowercase()), "{name:?} is listed more than once");
    }
}

#[test]
fn unknown_names_resolve_to_nothing() {
    assert_eq!(icons::from_name(""), None);
    assert_eq!(icons::from_name("not-an-icon"), None);
    assert_eq!(icons::from_name("data model with spaces"), None);
}
