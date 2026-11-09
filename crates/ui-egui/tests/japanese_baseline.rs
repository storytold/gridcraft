//! Japanese and Latin glyphs in one line must share one baseline, including when the Japanese
//! comes from a script fallback (a Calibri cell, the Segoe UI formula bar and the ribbon), and the
//! line must keep the primary font's height so a vertically centred menu item does not shift it.
//!
//! Its own test binary: it loads the host's script fallback faces, which the shared harness must
//! not see.

use egui::epaint::text::{Fonts, TextOptions};
use egui::{Color32, FontFamily, FontId};
use gridcraft_ui_egui::theme;

/// The bottom of a glyph's bitmap, relative to the row. A glyph that stands on the baseline has
/// the same bottom as another that stands on it.
fn glyph_bottom(g: &egui::epaint::text::Glyph) -> f32 {
    g.pos.y + g.uv_rect.offset.y + g.uv_rect.size.y
}

#[test]
fn japanese_and_latin_share_one_baseline() {
    let definitions = theme::font_definitions_for_language(gridcraft_ui_egui::i18n::Language::Ja);
    let mut fonts = Fonts::new(TextOptions::default(), definitions);
    fonts.begin_pass(TextOptions::default(), Default::default());

    let mut checked = 0;
    for family in [theme::CELL, theme::UI] {
        let font = FontId::new(14.0, FontFamily::Name(family.into()));
        if !fonts.with_pixels_per_point(1.0).has_glyph(&font, '日') {
            continue; // no Japanese face on this host (a minimal Linux image)
        }
        let row_height = fonts.with_pixels_per_point(1.0).row_height(&font);
        let galley = fonts.with_pixels_per_point(1.0).layout_no_wrap("日本語１1".to_owned(), font, Color32::WHITE);
        let glyphs = &galley.rows[0].row.glyphs;

        // `１` (U+FF11, the fourth glyph) is a full-width form egui's 0.90-scaled icon font also
        // maps. It must come from the same Japanese face as 日, or it is drawn at another size.
        let japanese = glyphs[0];
        let full_width = glyphs[3];
        assert_eq!(
            (full_width.font_face_height, full_width.font_face_ascent),
            (japanese.font_face_height, japanese.font_face_ascent),
            "{family}: U+FF11 resolved to a face other than the Japanese one"
        );

        // The line keeps the primary font's height: a widget that centres the line box (menu
        // items, the Backstage sidebar) must place Japanese exactly where it places Latin.
        assert!((galley.size().y - row_height).abs() <= 0.5, "{family}: the Japanese line changed the row height");

        // `１` and `1` both stand on the baseline, so their bitmap bottoms meet after alignment.
        let half_width = glyphs[4];
        let difference = (glyph_bottom(&full_width) - glyph_bottom(&half_width)).abs();
        assert!(difference <= 1.5, "{family}: U+FF11 and U+0031 sit {} px apart vertically", difference);
        checked += 1;
    }
    if checked == 0 {
        eprintln!("no Japanese face on this host; the mixed-script baseline check was not run");
    }
}
