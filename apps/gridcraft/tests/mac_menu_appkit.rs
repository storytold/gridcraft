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
    let mut menu = native_menu::NativeMenu::install(&ctx, gridcraft_ui_egui::i18n::Language::En);

    let bar = nsapp.mainMenu().expect("a main menu");
    let tops: Vec<String> = (0..bar.numberOfItems()).filter_map(|i| bar.itemAtIndex(i)?.submenu()).map(|m| m.title().to_string()).collect();
    assert_eq!(tops, ["GridCraft", "File", "Edit", "View", "Insert", "Format", "Tools", "Data", "Window", "Help"]);
    let windows_menu = nsapp.windowsMenu().expect("AppKit knows the Window menu");
    assert_eq!(windows_menu.title().to_string(), "Window");

    // A poll lists the open workbook windows at the end of the Window menu (#716).
    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    let mut app = gridcraft_ui_egui::SheetApp::new(session, Default::default());
    menu.poll(&mut app, &ctx, egui::ViewportId::ROOT);
    let open = app.open_windows();
    assert_eq!(open.len(), 1, "one workbook window");
    let last = windows_menu.itemAtIndex(windows_menu.numberOfItems() - 1).expect("a Window menu item");
    assert_eq!(last.title().to_string(), open[0].label(), "the Window menu lists the open workbook");

    // Switching the interface language relabels the installed menus in place.
    let mut app = gridcraft_ui_egui::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
    app.ui.language = gridcraft_ui_egui::i18n::Language::Fr;
    menu.sync_language(&app);
    let tops: Vec<String> = (0..bar.numberOfItems()).filter_map(|i| bar.itemAtIndex(i)?.submenu()).map(|m| m.title().to_string()).collect();
    assert_eq!(tops, ["GridCraft", "Fichier", "Édition", "Affichage", "Insertion", "Format", "Outils", "Données", "Fenêtre", "Aide"]);
    let file = bar.itemAtIndex(1).and_then(|item| item.submenu()).expect("File menu");
    assert_eq!(file.itemAtIndex(0).map(|item| item.title().to_string()).as_deref(), Some("Nouveau classeur"));
    assert_eq!(nsapp.windowsMenu().map(|m| m.title().to_string()).as_deref(), Some("Fenêtre"));
    assert_eq!(nsapp.helpMenu().map(|m| m.title().to_string()).as_deref(), Some("Aide"), "AppKit knows the translated Help menu");
    app.ui.language = gridcraft_ui_egui::i18n::Language::En;
    menu.sync_language(&app);
    assert_eq!(file.itemAtIndex(0).map(|item| item.title().to_string()).as_deref(), Some("New Workbook"));
    assert_eq!(bar.itemAtIndex(2).and_then(|item| item.submenu()).map(|m| m.title().to_string()).as_deref(), Some("Edit"));

    println!("mac_menu_appkit: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
