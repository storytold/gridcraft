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
    let _menu = native_menu::NativeMenu::install(&ctx);

    let bar = nsapp.mainMenu().expect("a main menu");
    let tops: Vec<String> = (0..bar.numberOfItems()).filter_map(|i| bar.itemAtIndex(i)?.submenu()).map(|m| m.title().to_string()).collect();
    assert_eq!(tops, ["GridCraft", "File", "Edit", "View", "Insert", "Format", "Tools", "Data", "Window", "Help"]);
    assert_eq!(nsapp.windowsMenu().map(|m| m.title().to_string()).as_deref(), Some("Window"), "AppKit knows the Window menu");

    // The Window menu: no documents yet, so the cycling items come first, with the system's Cmd+`
    // shortcut.
    let window = nsapp.windowsMenu().expect("Window menu");
    let titles = |m: &objc2_app_kit::NSMenu| -> Vec<String> {
        (0..m.numberOfItems())
            .filter_map(|i| m.itemAtIndex(i))
            .map(|it| if it.isSeparatorItem() { "-".into() } else { it.title().to_string() })
            .collect()
    };
    let before = titles(&window);
    assert_eq!(before.get(0..5).unwrap_or_default(), ["Next Window", "Previous Window", "New Window", "Next Sheet", "Previous Sheet"], "{before:?}");
    let next = window.itemAtIndex(0).expect("Next Window");
    assert_eq!(next.keyEquivalent().to_string(), "`");
    assert!(next.keyEquivalentModifierMask().contains(objc2_app_kit::NSEventModifierFlags::Command));
    let previous = window.itemAtIndex(1).expect("Previous Window");
    assert_eq!(previous.keyEquivalent().to_string(), "`");
    assert!(previous.keyEquivalentModifierMask().contains(objc2_app_kit::NSEventModifierFlags::Command | objc2_app_kit::NSEventModifierFlags::Shift));

    // The open workbooks end the menu after a separator, the active one checked, as Excel's
    // Window menu lists them: AppKit's view of the menu agrees with the sync state. A changed set
    // of documents replaces the submenu (muda places items by position, AppKit adds its own), so
    // the Window menu is fetched again from NSApp after every poll.
    let mut menu = _menu;
    let mut app = gridcraft_ui_egui::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
    app.session.new_workbook();
    app.session.new_workbook();
    let uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
    menu.poll(&mut app, &ctx);
    let on = |m: &objc2_app_kit::NSMenu, i: isize| m.itemAtIndex(i).map(|it| it.state() == objc2_app_kit::NSControlStateValueOn);
    let tail = |v: &[String], n: usize| v.get(v.len().saturating_sub(n)..).unwrap_or_default().to_vec();
    let window = nsapp.windowsMenu().expect("Window menu");
    let listed = titles(&window);
    assert_eq!(
        tail(&listed, 8),
        ["Next Window", "Previous Window", "New Window", "Next Sheet", "Previous Sheet", "-", "Book1", "Book2"],
        "{listed:?}"
    );
    let n = window.numberOfItems();
    assert_eq!((on(&window, n - 2), on(&window, n - 1)), (Some(false), Some(true)));
    assert_eq!(menu.listed_windows(), vec![(uids[0], "Book1".to_string()), (uids[1], "Book2".to_string())]);
    let book1 = window.itemAtIndex(n - 2).expect("Book1");

    // Switching the active document only moves the check mark: same NSMenuItems, no rebuild.
    app.session.set_active(0);
    menu.poll(&mut app, &ctx);
    assert_eq!((on(&window, n - 2), on(&window, n - 1)), (Some(true), Some(false)));
    assert!(
        window.itemAtIndex(n - 2).is_some_and(|it| objc2::rc::Retained::as_ptr(&it) == objc2::rc::Retained::as_ptr(&book1)),
        "Book1's item was kept"
    );
    assert_eq!(titles(&window), listed);

    // An edit changes the title in place: same NSMenuItem, new text.
    app.session.execute("cell.set", serde_json::json!({"cell": "A1", "input": "1"})).expect("edit");
    menu.poll(&mut app, &ctx);
    assert_eq!(titles(&window).get((n - 2) as usize).map(String::as_str), Some("Book1 — Edited"));
    assert!(
        window.itemAtIndex(n - 2).is_some_and(|it| objc2::rc::Retained::as_ptr(&it) == objc2::rc::Retained::as_ptr(&book1)),
        "Book1's item was kept through the retitle"
    );

    // Closing a document replaces the submenu: still NSApp's windows menu, the list one shorter.
    app.session.close_document(1);
    menu.poll(&mut app, &ctx);
    let window = nsapp.windowsMenu().expect("Window menu after a close");
    let listed = titles(&window);
    assert_eq!(tail(&listed, 3), ["Previous Sheet", "-", "Book1 — Edited"], "{listed:?}");
    assert_eq!(on(&window, window.numberOfItems() - 1), Some(true));
    assert_eq!(menu.listed_windows(), vec![(uids[0], "Book1 — Edited".to_string())]);

    // No visible window: the separator goes too.
    app.session.hidden_windows.push(uids[0]);
    menu.poll(&mut app, &ctx);
    let window = nsapp.windowsMenu().expect("Window menu with nothing listed");
    let listed = titles(&window);
    assert_eq!(tail(&listed, 2), ["Next Sheet", "Previous Sheet"], "{listed:?}");
    assert!(menu.listed_windows().is_empty());

    // And back: the list returns at the end, with the shortcuts intact on the fresh submenu.
    app.session.hidden_windows.clear();
    menu.poll(&mut app, &ctx);
    let window = nsapp.windowsMenu().expect("Window menu again");
    let again = titles(&window);
    assert_eq!(tail(&again, 3), ["Previous Sheet", "-", "Book1 — Edited"], "{again:?}");
    assert_eq!(on(&window, window.numberOfItems() - 1), Some(true));
    assert_eq!(window.itemAtIndex(0).map(|it| it.keyEquivalent().to_string()).as_deref(), Some("`"));

    println!("mac_menu_appkit: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
