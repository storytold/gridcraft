//! File › Open's panel on macOS, on the main thread (`harness = false`): it is a window of its own,
//! not a sheet on a workbook window (storytold/gridcraft#2908 makes the other pickers sheets), and
//! returns at once instead of running rfd's modal loop, which aborts on resize (#2906).

#[cfg(target_os = "macos")]
#[path = "../src/open_panel.rs"]
#[allow(dead_code)]
mod open_panel;

/// main.rs's list, which `open_panel` reads from the crate root.
#[cfg(target_os = "macos")]
const SPREADSHEETS: &[&str] = &["xlsx", "csv"];

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    // nextest lists a test binary before running it: answer as libtest would.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--list") {
        if !args.iter().any(|a| a == "--ignored") {
            println!("mac_open_panel: test");
        }
        return;
    }
    let mtm = MainThreadMarker::new().expect("harness = false runs on the main thread");
    NSApplication::sharedApplication(mtm).finishLaunching();
    let (tx, rx) = std::sync::mpsc::channel();
    let panel = open_panel::show(mtm, tx, egui::Context::default());
    assert!(panel.isVisible(), "the panel is up when show returns");
    assert!(panel.sheetParent().is_none(), "a window of its own, not a sheet");
    #[allow(deprecated)]
    let types: Vec<String> = panel.allowedFileTypes().expect("filtered").iter().map(|t| t.to_string()).collect();
    assert_eq!(types, SPREADSHEETS);
    panel.close(); // `cancel:` is unsafe in objc2-app-kit, which this workspace forbids
    assert!(!panel.isVisible());
    assert!(rx.try_recv().is_err(), "closing it opens nothing");
    println!("mac_open_panel: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
