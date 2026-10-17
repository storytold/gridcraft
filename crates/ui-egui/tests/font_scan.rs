//! The desktop and web apps set up their context through `setup_context_for_language` (to honour
//! the saved interface language). That path must start the installed-font scan, or the font lists
//! show only the standard names and cells never draw in an installed font. Its own test binary, so
//! no other test can have started the scan first.

use gridcraft_ui_egui::{SheetApp, fonts, i18n::Language};

#[test]
fn language_setup_starts_the_installed_font_scan() {
    assert!(!fonts::scan_started());
    SheetApp::setup_context_for_language(&egui::Context::default(), false, Language::En);
    assert!(fonts::scan_started());
}
