//! Installed Latin fonts inherit the interface fallback for each weight (#2035, #2408).
//!
//! A separate test binary: it runs the process-wide installed-font scan and records the bound
//! families, state the tests in `it/` must not see.

use egui::{Context, FontFamily, FontId};
use gridcraft_ui_egui::{fonts, i18n::Language, theme};

#[test]
fn installed_latin_fonts_keep_cjk_in_all_roles() {
    fonts::catalog();
    let installed = fonts::installed_among(&["Tahoma".into(), "Verdana".into(), "Arial".into()]);
    if installed.is_empty() {
        eprintln!("skipped: no test Latin font is installed");
        return;
    }
    let definitions = theme::font_definitions_with(Language::Zh, &installed);
    let ctx = Context::default();
    ctx.set_fonts(definitions.clone());
    let mut out = ctx.run_ui(Default::default(), |_| {});
    out.textures_delta.clear();
    let ui = FontId::new(16.0, FontFamily::Name(theme::UI.into()));
    if !ctx.fonts_mut(|f| "中文".chars().all(|c| f.has_glyph(&ui, c))) {
        eprintln!("skipped: the interface has no CJK fallback on this host");
        return;
    }
    for name in &installed {
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let role = fonts::role_key(name, bold, italic);
            let family = FontFamily::Name(role.clone().into());
            let chain = &definitions.families[&family];
            let ui_role = if bold { theme::UI_BOLD } else { theme::UI };
            let expected = &definitions.families[&FontFamily::Name(ui_role.into())];
            assert!(chain.ends_with(expected), "{role} must inherit {ui_role}: {chain:?}; expected tail: {expected:?}");
            let id = FontId::new(16.0, family);
            assert!(ctx.fonts_mut(|f| "中文ABC2026".chars().all(|c| f.has_glyph(&id, c))), "{role} must retain CJK glyphs");
            eprintln!("checked {role}: {ui_role} chain and mixed-script glyphs");
        }
    }
}
