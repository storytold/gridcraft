//! The desktop and web apps set up their context with `setup_context_for_language`, not
//! `setup_context`. That path must start the installed-font scan too, or the font lists only ever
//! show the standard names and cells can't use installed fonts.

use gridcraft_ui_egui::SheetApp;

#[test]
fn setting_up_for_a_language_starts_the_font_scan() {
    let ctx = egui::Context::default();
    SheetApp::setup_context_for_language(&ctx, false, gridcraft_ui_egui::i18n::Language::En);
    // The scan runs on a background thread; on wasm and machines without fonts it finishes empty.
    // A bounded native-only poll avoids Instant::now, which traps on wasm. On wasm the scan
    // finishes synchronously without touching the file system or sleeping.
    #[cfg(not(target_arch = "wasm32"))]
    for _ in 0..3_000 {
        if gridcraft_ui_egui::fonts::scan_done() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(gridcraft_ui_egui::fonts::scan_done(), "setup_context_for_language started the font scan");
}
