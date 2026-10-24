//! The interface's symbol characters must not render as empty boxes (#197, #54): the UI font often
//! lacks arrows, check marks and geometric shapes, so a symbol face backs it up.

use egui::{Color32, FontFamily, FontId};
use gridcraft_ui_egui::theme;

/// Symbols the interface draws as text (buttons, menus, dialogs, tooltips, Insert › Symbol).
const UI_SYMBOLS: &str = "▲▼▸▾✓✕←↑→↓↳↘↗⇧⌃⌘★•›≠≤≥∞";

#[test]
fn ui_symbols_have_glyphs_when_a_symbol_face_is_installed() {
    if !theme::symbol_face_installed() {
        return; // a minimal machine without any symbol face; nothing to assert
    }
    let ctx = egui::Context::default();
    ctx.set_fonts(theme::font_definitions());
    let mut out = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    out.textures_delta.clear(); // nothing consumes the atlas in a headless test
    for family in [FontFamily::Proportional, FontFamily::Name(theme::CELL.into())] {
        let font = FontId::new(14.0, family);
        let glyphs = |text: &str| {
            let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::BLACK));
            galley.rows.iter().flat_map(|r| r.glyphs.iter().map(|g| (g.chr, g.uv_rect))).collect::<Vec<_>>()
        };
        // A private-use code point no font covers renders as the replacement box.
        let tofu: Vec<_> = glyphs("\u{10FFFD}").into_iter().map(|(_, uv)| uv).collect();
        let missing: String = glyphs(UI_SYMBOLS).into_iter().filter(|(_, uv)| tofu.contains(uv)).map(|(c, _)| c).collect();
        assert!(missing.is_empty(), "these interface symbols render as boxes in {:?}: {missing}", font.family);
    }
}
