//! Discovery and loading of installed system fonts.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::OnceLock;

#[cfg(not(target_arch = "wasm32"))]
fn database() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();

    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn families() -> &'static [String] {
    use std::collections::BTreeSet;

    static NAMES: OnceLock<Vec<String>> = OnceLock::new();

    NAMES.get_or_init(|| {
        let mut names = BTreeSet::new();

        for face in database().faces() {
            for (name, _) in &face.families {
                // Hide internal dot-prefixed font family names.
                if !name.trim_start().starts_with('.') && !name.trim().is_empty() {
                    names.insert(name.clone());
                }
            }
        }

        names.into_iter().collect()
    })
}

/// Presentation-only grouping. Font names remain unchanged.
#[cfg(not(target_arch = "wasm32"))]
pub fn grouped_families() -> &'static [(String, Vec<String>)] {
    use std::collections::{BTreeMap, BTreeSet};

    static GROUPS: OnceLock<Vec<(String, Vec<String>)>> =
        OnceLock::new();

    GROUPS.get_or_init(|| {
        let mut groups: BTreeMap<String, BTreeSet<String>> =
            BTreeMap::new();

        for face in database().faces() {
            // Each face may have localized aliases.
            // Keep one selectable family name per face.
            let Some((name, _)) = face.families.iter().find(
                |(name, _)| {
                    !name.trim().is_empty()
                        && !name.trim_start().starts_with('.')
                }
            ) else {
                continue;
            };

            let group = font_series_name(name);

            groups
                .entry(group)
                .or_default()
                .insert(name.clone());
        }

        groups
            .into_iter()
            .map(|(group, names)| {
                (group, names.into_iter().collect())
            })
            .collect()
    })
}

/// Higher-level UI series are not always encoded in font metadata.
#[cfg(not(target_arch = "wasm32"))]
fn font_series_name(name: &str) -> String {
    // Group Noto typeface series while retaining each real family.
    for (prefix, series) in [
        ("Noto Sans", "Noto Sans"),
        ("Noto Serif", "Noto Serif"),
        ("Noto Mono", "Noto Mono"),
    ] {
        if name == prefix || name.starts_with(&format!("{prefix} ")) {
            return series.to_owned();
        }
    }

    if matches!(name, "MingLiU" | "PMingLiU") {
        return "MingLiU".to_owned();
    }

    // Hiragino Pro/ProN/Std/StdN are separate real families
    // but belong together in the font picker.
    if name.starts_with("Hiragino ") {
        for suffix in [" ProN", " Pro", " StdN", " Std"] {
            if let Some(base) = name.strip_suffix(suffix) {
                return base.to_owned();
            }
        }
    }

    // Regional/language variants.
    for suffix in [" TC", " SC", " HK", " MO", "-簡", "-繁", "－簡", "－繁", "-简", "－简", " 簡", " 繁", " 简"] {
        if let Some(base) = name.strip_suffix(suffix)
            && !base.is_empty()
        {
            return base.to_owned();
        }
    }

    name.to_owned()
}

#[cfg(target_arch = "wasm32")]
pub fn grouped_families() -> &'static [(String, Vec<String>)] {
    &[]
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load(name: &str) -> Option<egui::FontData> {
    use fontdb::{Family, Query, Stretch, Style, Weight};

    let db = database();

    let id = db.query(&Query { families: &[Family::Name(name)], weight: Weight::NORMAL, stretch: Stretch::Normal, style: Style::Normal })?;

    let face = db.face(id)?;

    if !face.families.iter().any(|(family, _)| family.eq_ignore_ascii_case(name)) {
        return None;
    }

    db.with_face_data(id, |bytes, index| {
        let mut font = egui::FontData::from_owned(bytes.to_vec());
        font.index = index;

        // DFKai-SB requires TrueType hinting for correct outlines.
        if name.eq_ignore_ascii_case("DFKai-SB") {
            font.tweak.hinting = Some(true);
            font.tweak.hinting_target = egui::epaint::text::HintingTarget::Mono;
        }

        font
    })
}

#[cfg(target_arch = "wasm32")]
pub fn families() -> &'static [String] {
    &[]
}

#[cfg(target_arch = "wasm32")]
pub fn load(_name: &str) -> Option<egui::FontData> {
    None
}

/// Names registered at runtime, scoped to this application's font context.
#[cfg(not(target_arch = "wasm32"))]
fn registered() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    use std::sync::{Mutex, OnceLock};
    static LOADED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    LOADED.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn is_registered(name: &str) -> bool {
    registered().lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(name)
}

#[cfg(target_arch = "wasm32")]
pub fn is_registered(_name: &str) -> bool {
    false
}

/// Register a selected system font for subsequent frames.
#[cfg(not(target_arch = "wasm32"))]
pub fn register(ctx: &egui::Context, name: &str) -> bool {
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

    if is_registered(name) {
        return true;
    }

    let Some(data) = load(name) else {
        return false;
    };

    let font_key = format!("installed-dynamic-{name}");
    ctx.add_font(FontInsert::new(
        &font_key,
        data,
        vec![InsertFontFamily { family: egui::FontFamily::Name(name.into()), priority: FontPriority::Highest }],
    ));

    pending().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(name.to_string());

    ctx.request_repaint();
    true
}

#[cfg(target_arch = "wasm32")]
pub fn register(_ctx: &egui::Context, _name: &str) -> bool {
    false
}

/// Fonts requested in the current frame, not yet active.
#[cfg(not(target_arch = "wasm32"))]
fn pending() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    use std::sync::{Mutex, OnceLock};
    static PENDING: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// Activate fonts requested during the preceding frame.
#[cfg(not(target_arch = "wasm32"))]
pub fn activate_pending() {
    let names: Vec<String> = {
        let mut guard = pending().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.drain().collect()
    };

    registered().lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(names);
}

#[cfg(target_arch = "wasm32")]
pub fn activate_pending() {}
