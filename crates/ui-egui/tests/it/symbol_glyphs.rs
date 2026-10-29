//! The interface's symbol characters must not render as empty boxes (#197, #54): the UI font often
//! lacks arrows, check marks and geometric shapes, so egui's bundled symbol faces (Hack and the
//! monochrome emoji fonts), the optional permitted craft-fonts faces, and code-drawn chrome icons
//! back it up.
//!
//! The checks ask the font set whether it has a real glyph for each character (`has_glyph`), which
//! is the property the user sees. The replacement-box probe is a separate test that documents why
//! "differs from the tofu glyph" alone is not evidence that a face has the character.
//!
//! Two modes are deliberate:
//! - no-font/default: no `CRAFT_FONTS_DIR`. Bundled/system faces provide `↘ ◀ ↑ 📋`, and the three
//!   symbols no bundled face has (`✕ ✓ ⌃`) are chrome-only gaps covered by code-drawn icons and
//!   their ASCII equivalents. `✓` and `⌃` stay documented optional-input/platform gaps.
//! - extended acceptance: `CRAFT_FONTS_DIR` set (release/acceptance builds also set
//!   `CRAFT_FONTS_REQUIRED=1`). All seven exact codepoints and Korean UI text must render, or the
//!   test fails; it never skips a loaded or required face set.

use egui::{FontFamily, FontId};
use gridcraft_ui_egui::icons::{self, Icon};
use gridcraft_ui_egui::theme;

/// Symbols the bundled faces provide on a host with no symbol, CJK or Latin-Extended face:
/// Hack supplies `↘ ◀ ↑`; Noto Emoji and emoji-icon-font supply `📋`.
const BUNDLED_REQUIRED_UI_SYMBOLS: &str = "↘◀↑📋";

/// The rest of what the interface draws as text that egui's bundled faces cover: Ubuntu-Light
/// supplies punctuation and maths, Hack the arrows/triangles and keyboard arrows, and the
/// monochrome emoji faces ⌘ and ★.
const BUNDLED_UI_SYMBOLS: &str = "▲▼▸▾←→↓↳↗⇧⌘★•›≠≤≥∞";

/// The production union. `✕ ✓ ⌃` have no bundled egui face: with craft-fonts, Noto Sans Symbols 2
/// supplies `✕` and `✓`, and Noto Sans Symbols supplies `⌃`; `📋` still comes from egui's emoji.
const CRAFT_UI_SYMBOLS: &str = "↘✕◀↑📋✓⌃";

/// Korean interface labels the extended acceptance set must render (not a tiny sample).
const KOREAN_UI_TEXT: &[&str] = &["데이터", "한글", "파일", "편집", "보기", "삽입", "서식", "저장", "열기", "닫기", "인쇄", "취소", "확인"];

/// A context with the deterministic bundled + pinned craft-fonts set installed, plus the families
/// the interface uses. System fonts are deliberately excluded so this check is the same on CI.
fn themed() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(theme::pinned_font_definitions());
    let mut out = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    out.textures_delta.clear(); // nothing consumes the atlas in a headless test
    ctx
}

/// Returns the characters in `symbols` that lack a glyph in the given font family.
fn missing(ctx: &egui::Context, symbols: &str, family: FontFamily) -> String {
    let font = FontId::new(14.0, family);
    symbols.chars().filter(|&c| !ctx.fonts_mut(|f| f.has_glyph(&font, c))).collect()
}

/// Returns the proportional and cell font families checked by the symbol-glyph tests.
fn families() -> [FontFamily; 2] {
    [FontFamily::Proportional, FontFamily::Name(theme::CELL.into())]
}

#[test]
/// Every required bundled interface symbol resolves in both checked font families.
fn bundled_required_ui_symbols_resolve_to_real_glyphs() {
    let ctx = themed();
    for family in families() {
        let missing = missing(&ctx, BUNDLED_REQUIRED_UI_SYMBOLS, family.clone());
        assert!(
            missing.is_empty(),
            "these required interface symbols have no real glyph in {family:?}: {missing} — \
             egui must be built with `monochrome_emoji_fonts` and the Hack fallback must stay in \
             theme::font_definitions"
        );
    }
}

#[test]
/// The remaining bundled interface symbols resolve in both checked font families.
fn bundled_symbols_cover_the_rest_of_the_interface() {
    let ctx = themed();
    for family in families() {
        let missing = missing(&ctx, BUNDLED_UI_SYMBOLS, family.clone());
        assert!(missing.is_empty(), "egui's bundled faces have no real glyph in {family:?} for: {missing}");
    }
}

#[test]
/// U+10FFFD is the only tofu probe and does not resolve in either family.
fn unknown_codepoint_is_the_tofu_probe_not_a_symbol() {
    // A private-use code point no font covers. This is the only place that relies on the
    // replacement box: real-glyph checks above use `has_glyph`, because comparing a glyph's
    // texture with the replacement glyph would pass for two distinct missing characters.
    let ctx = themed();
    for family in families() {
        let font = FontId::new(14.0, family.clone());
        assert!(!ctx.fonts_mut(|f| f.has_glyph(&font, '\u{10FFFD}')), "U+10FFFD must not resolve in {family:?}");
    }
}

#[test]
/// `✕`/`✓` map to code icons and `⌃` falls back to `^`; the semantic `×` and `^` resolve.
fn chrome_symbols_without_a_bundled_glyph_have_code_drawn_or_semantic_fallbacks() {
    // No bundled face has ✕, ✓ or ⌃. The buttons that used to paint ✕/✓ as text now draw the
    // original code icons Icon::Close/Icon::Check, and the one Control-key tooltip falls back to
    // "^" when ⌃ is absent. Nothing here suppresses a glyph check or the accessibility check.
    assert_eq!(icons::fallback_for_symbol('✕'), Some(Icon::Close), "the close buttons must be code-painted");
    assert_eq!(icons::fallback_for_symbol('✓'), Some(Icon::Check), "the Symbol gallery check must be code-painted");
    assert_eq!(theme::symbol_fallback('⌃'), Some('^'), "the Control-key glyph needs a readable ASCII fallback");
    let ctx = themed();
    for family in families() {
        let missing = missing(&ctx, "×^", family.clone());
        assert!(missing.is_empty(), "the semantic fallbacks × and ^ must resolve in {family:?}: missing {missing}");
    }
}

#[test]
/// Without craft fonts it prints the optional `✓`/`⌃` gaps as a diagnostic; the strict bundled checks still run separately.
fn no_font_mode_documents_the_optional_symbol_gaps() {
    if theme::craft_fonts_loaded() {
        return;
    }
    let ctx = themed();
    let missing: String = families().iter().map(|family| missing(&ctx, "✓⌃", family.clone())).collect();
    // A diagnostic, not an acceptance skip: the bundled assertions above still run strictly.
    eprintln!(
        "no-font mode: {missing:?} are optional-input/platform gaps (set CRAFT_FONTS_DIR for the \
         extended acceptance set); chrome uses code-drawn icons or ASCII fallbacks instead"
    );
}

#[test]
/// With craft fonts, the seven-symbol union, Korean samples and exact permitted faces are checked; a required build without them fails instead of skipping.
fn extended_craft_fonts_cover_the_required_symbol_union_and_korean() {
    if !theme::craft_fonts_loaded() {
        // Optional-input diagnostic mode only. A required acceptance build must fail, not skip.
        assert!(!theme::craft_fonts_required(), "CRAFT_FONTS_REQUIRED=1 but the build embedded no craft-fonts faces");
        eprintln!("diagnostic: no CRAFT_FONTS_DIR; the extended seven-symbol and Korean acceptance checks were not run");
        return;
    }
    let ctx = themed();
    for family in families() {
        let missing_symbols = missing(&ctx, CRAFT_UI_SYMBOLS, family.clone());
        assert!(
            missing_symbols.is_empty(),
            "the craft-fonts production set has no real glyph in {family:?} for {missing_symbols:?}; \
             Noto Sans Symbols supplies ⌃↘↑ and Noto Sans Symbols 2 supplies ✕◀✓, while egui's emoji supply 📋"
        );
        for sample in KOREAN_UI_TEXT {
            let missing = missing(&ctx, sample, family.clone());
            assert!(missing.is_empty(), "Korean interface text {sample:?} has no glyph for {missing:?} in {family:?}");
        }
    }
    assert_exact_permitted_faces();
}

/// The required faces are embedded, the native-only Chinese face keeps its contract when present,
/// and no barred family entered the set. Native builds embed every non-barred manifest row (for
/// example the Thai, Arabic, Georgian and Hebrew faces), so this asserts the required set rather
/// than claiming the embedded set is limited to the five web faces.
fn assert_exact_permitted_faces() {
    let faces: Vec<_> = theme::craft_font_scripts().collect();
    let expected: &[(&str, &str, &[&str])] = &[
        ("BIZ UDPGothic", "Regular", &["Jpan", "Latn"]),
        ("BIZ UDPGothic", "Bold", &["Jpan", "Latn"]),
        ("NanumGothic", "Regular", &["Hang", "Kore"]),
        ("Noto Sans Symbols", "Regular", &["Zsym"]),
        ("Noto Sans Symbols 2", "Regular", &["Zsym"]),
    ];
    for (family, style, scripts) in expected {
        assert!(
            faces.iter().any(|(f, s, declared)| f == family && s == style && *declared == *scripts),
            "craft-fonts must embed {family} {style} with scripts {scripts:?}; embedded: {faces:?}"
        );
    }
    if theme::chinese_font_loaded() {
        assert!(
            faces.iter().any(|(f, s, declared)| *f == "LXGW WenKai Lite" && *s == "Regular" && *declared == ["Hani", "Hans"].as_slice()),
            "the native-only LXGW Chinese face keeps its Hani,Hans script tags; embedded: {faces:?}"
        );
    }
    for (family, _, _) in &faces {
        assert!(!family.contains("CJK"), "the barred Noto CJK collection must never be embedded: {family}");
        assert!(!family.starts_with("Source ") && !family.starts_with("Adobe "), "a barred Adobe/Source face was embedded: {family}");
    }
}

#[test]
/// Advertised script samples resolve in the craft faces; Chinese samples join only when the LXGW face was embedded, and a required build without faces fails.
fn craft_fonts_cover_the_advertised_scripts() {
    if !theme::craft_fonts_loaded() {
        assert!(!theme::craft_fonts_required(), "CRAFT_FONTS_REQUIRED=1 but the build embedded no craft-fonts faces");
        eprintln!("craft-fonts not embedded (set CRAFT_FONTS_DIR to the pinned checkout); script coverage check not run");
        return;
    }
    // Chinese needs the native-only LXGW face (the only face that could supply it otherwise, Noto
    // Sans CJK SC, is barred), so those samples join the strict set only when it was embedded.
    let mut samples = vec![
        "カテゴリ",
        "한글",
        "데이터",
        "파일",
        "편집",
        "보기",
        "삽입",
        "서식",
        "저장",
        "열기",
        "닫기",
        "인쇄",
        "취소",
        "확인",
        "ทดสอบ",
        "مرحبا",
        "ქართული",
        "Příliš žluťoučký kůň",
        "STRAẞENBAHN",
    ];
    if theme::chinese_font_loaded() {
        samples.extend(["数据", "自动换行", "文件", "编辑", "插入"]);
    }
    for sample in samples {
        assert!(
            theme::craft_fonts_cover(sample),
            "the permitted craft-fonts faces have no face for {sample:?}; add the OFL face to craft-fonts rather than relaxing this test"
        );
    }
}
