//! `setup_context_for_language` is what both apps call, and it must start the machine's font scan:
//! without it the font list stays on the standard names and workbook fonts never bind (#4).
//!
//! Its own test binary: the scan fills the process-wide font catalog, which the shared harness and
//! its pinned font set must not see.

use std::time::{Duration, Instant};

#[test]
#[allow(clippy::disallowed_methods)] // this test runs natively only, where the clock is available
fn setting_up_the_context_starts_the_font_scan() {
    let ctx = egui::Context::default();
    gridcraft_ui_egui::SheetApp::setup_context_for_language(&ctx, false, gridcraft_ui_egui::i18n::Language::En);
    let start = Instant::now();
    while !gridcraft_ui_egui::fonts::scan_done() {
        assert!(start.elapsed() < Duration::from_secs(60), "the scan starts and finishes");
        std::thread::sleep(Duration::from_millis(10));
    }
}
