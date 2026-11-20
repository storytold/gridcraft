//! The macOS menu bar as AppKit sees it, on the main thread (`harness = false`): install it through
//! the real muda backend (`src/native_menu.rs`) and read NSApp back. The Window menu must be NSApp's
//! `windowsMenu`: that is where macOS adds Move & Resize, Fill and Center, the targets of the system
//! window tiling shortcuts (Fn+Ctrl+arrows, Fn+F), and lists the open windows (#716).

// The app's own module, built into this test (cargo builds harness = false tests with cfg(test), so its
// unit tests compile here too without a harness to run them).
#[cfg(target_os = "macos")]
#[path = "../src/native_menu.rs"]
mod native_menu;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let mtm = MainThreadMarker::new().expect("harness = false runs on the main thread");
    let nsapp = NSApplication::sharedApplication(mtm);
    nsapp.finishLaunching();

    let ctx = egui::Context::default();
    let mut menu = native_menu::NativeMenu::install(&ctx);

    let bar = nsapp.mainMenu().expect("a main menu");
    let tops: Vec<String> = (0..bar.numberOfItems()).filter_map(|i| bar.itemAtIndex(i)?.submenu()).map(|m| m.title().to_string()).collect();
    assert_eq!(tops, ["GridCraft", "File", "Edit", "View", "Insert", "Format", "Tools", "Data", "Window", "Help"]);
    let windows_menu = nsapp.windowsMenu().expect("AppKit knows the Window menu");
    assert_eq!(windows_menu.title().to_string(), "Window");

    // AppKit lists the workbook windows itself; the menu adds no second list, which also went stale
    // as workbooks opened (muda removes by its own index, which AppKit's own items shift).
    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    let mut app = gridcraft_ui_egui::SheetApp::new(session, Default::default());
    menu.poll(&mut app, &ctx, egui::ViewportId::ROOT);
    let first = app.open_windows()[0].label();
    let _ = app.run("file.new", serde_json::json!({}));
    menu.poll(&mut app, &ctx, egui::ViewportId::ROOT);
    assert_eq!(app.open_windows().len(), 2, "two workbook windows");
    let titles: Vec<String> = (0..windows_menu.numberOfItems()).filter_map(|i| windows_menu.itemAtIndex(i)).map(|m| m.title().to_string()).collect();
    assert!(!titles.contains(&first), "no workbook entries of our own in {titles:?}");

    println!("mac_menu_appkit: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
