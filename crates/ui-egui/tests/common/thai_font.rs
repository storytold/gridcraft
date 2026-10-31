//! Strict runtime font fixture for Thai rendering tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

const PINNED_NOTO_SANS_THAI_SHA256: &str = "5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8";
const REQUIRED_THAI_TEST_FAMILY: &str = "gridcraft-required-thai-test-face";

/// Loads the pinned Thai face for tests that assert Thai glyph geometry. A missing, unreadable, or
/// different font fails with its path and expected hash; these tests must never claim success on
/// an egui or host fallback face.
pub fn required_font_definitions() -> egui::FontDefinitions {
    use sha2::{Digest, Sha256};

    let fonts_root =
        std::env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../craft-fonts"));
    let path = fonts_root.join("fonts/noto-sans-thai/NotoSansThai.ttf");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("required pinned Noto Sans Thai font could not be read at {}: {error}; set CRAFT_FONTS_DIR", path.display()));
    let sha256 = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    assert_eq!(
        sha256,
        PINNED_NOTO_SANS_THAI_SHA256,
        "required Noto Sans Thai font at {} has the wrong SHA-256; expected {}, found {}",
        path.display(),
        PINNED_NOTO_SANS_THAI_SHA256,
        sha256
    );

    let mut fonts = gridcraft_ui_egui::theme::pinned_font_definitions();
    let key = "NotoSansThai".to_string();
    fonts.font_data.insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in [
        egui::FontFamily::Proportional,
        egui::FontFamily::Name(gridcraft_ui_egui::theme::CELL.into()),
        egui::FontFamily::Name(gridcraft_ui_egui::theme::CELL_BOLD.into()),
        egui::FontFamily::Name(gridcraft_ui_egui::theme::UI.into()),
    ] {
        if let Some(chain) = fonts.families.get_mut(&family)
            && !chain.contains(&key)
        {
            chain.insert(0, key.clone());
        }
    }
    fonts.families.insert(egui::FontFamily::Name(REQUIRED_THAI_TEST_FAMILY.into()), vec![key]);
    fonts
}

/// Checks Thai coverage through a family that contains only the SHA-256-verified Thai face.
pub fn assert_required_coverage(ctx: &egui::Context) {
    let font = egui::FontId::new(16.0, egui::FontFamily::Name(REQUIRED_THAI_TEST_FAMILY.into()));
    for ch in "กขไทยภาษาเก้า".chars() {
        let mut has_glyph = false;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            has_glyph = ui.fonts_mut(|fonts| fonts.has_glyph(&font, ch));
        });
        output.textures_delta.clear();
        assert!(has_glyph, "pinned Noto Sans Thai face has no glyph for {ch:?}");
    }
}
