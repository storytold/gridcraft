//! The macOS menu bar as AppKit sees it, on the main thread (`harness = false`): install it through
//! the real muda backend (`src/native_menu.rs`) and read NSApp back. The Window menu must be NSApp's
//! `windowsMenu`: that is where macOS adds Move & Resize, Fill and Center, the targets of the system
//! window tiling shortcuts (Fn+Ctrl+arrows, Fn+F), and lists the open windows (#716).

// The app's own module, built into this test (cargo builds harness = false tests with cfg(test), so its
// unit tests compile here too without a harness to run them).
#[cfg(target_os = "macos")]
#[path = "../src/native_menu.rs"]
#[allow(unused_imports, dead_code)]
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
    assert_eq!(nsapp.windowsMenu().map(|m| m.title().to_string()).as_deref(), Some("Window"), "AppKit knows the Window menu");

    let mut app = gridcraft_ui_egui::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
    app.ui.language = gridcraft_ui_egui::i18n::Language::Fr;
    menu.poll(&mut app, &ctx);
    let tops: Vec<String> = (0..bar.numberOfItems()).filter_map(|i| bar.itemAtIndex(i)?.submenu()).map(|m| m.title().to_string()).collect();
    assert_eq!(tops, ["GridCraft", "Fichier", "Édition", "Affichage", "Insertion", "Format", "Outils", "Données", "Fenêtre", "Aide"]);
    let file = bar.itemAtIndex(1).and_then(|item| item.submenu()).expect("File menu");
    assert_eq!(file.itemAtIndex(0).map(|item| item.title().to_string()).as_deref(), Some("Nouveau classeur"));
    assert_eq!(nsapp.windowsMenu().map(|m| m.title().to_string()).as_deref(), Some("Fenêtre"));
    app.ui.language = gridcraft_ui_egui::i18n::Language::En;
    menu.poll(&mut app, &ctx);
    assert_eq!(file.itemAtIndex(0).map(|item| item.title().to_string()).as_deref(), Some("New Workbook"));
    assert_eq!(bar.itemAtIndex(2).and_then(|item| item.submenu()).map(|m| m.title().to_string()).as_deref(), Some("Edit"));

    println!("mac_menu_appkit: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
