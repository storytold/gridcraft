//! The Save Changes sheet as AppKit sees it, on the main thread (`harness = false`): cancelling one
//! window's question takes down only that question's sheet, even when another window now carries
//! the title the first was asked under (two workbooks named Book.xlsx in two folders).

#[cfg(target_os = "macos")]
#[path = "../src/save_sheet.rs"]
mod save_sheet;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAlert, NSApplication};
    use objc2_foundation::NSString;

    // cargo nextest lists the tests first: this binary is one test.
    if std::env::args().any(|a| a == "--list") {
        if !std::env::args().any(|a| a == "--ignored") {
            println!("mac_save_sheet_appkit: test");
        }
        return;
    }
    let mtm = MainThreadMarker::new().expect("harness = false runs on the main thread");
    NSApplication::sharedApplication(mtm).finishLaunching();
    // An alert's panel stands in for a workbook window: AppKit makes it without unsafe code.
    let window = |title: &str| {
        let w = NSAlert::new(mtm).window();
        w.setTitle(&NSString::from_str(title));
        w.orderFront(None);
        w
    };
    let ask = save_sheet::hook(&egui::Context::default());

    let first = window("Book.xlsx — Edited");
    let first_answer = ask(1, "Book.xlsx — Edited", "Save?", "").expect("a sheet on the first window");
    // A save clears the first window's marker; the other Book.xlsx then has unsaved changes.
    first.setTitle(&NSString::from_str("Book.xlsx"));
    let second = window("Book.xlsx — Edited");
    let second_answer = ask(2, "Book.xlsx — Edited", "Save?", "").expect("a sheet on the second window");
    assert!(first.attachedSheet().is_some() && second.attachedSheet().is_some(), "each window has its sheet");

    save_sheet::end(1);
    assert!(first.attachedSheet().is_none(), "the cancelled question's sheet is gone");
    assert!(second.attachedSheet().is_some(), "the other window's sheet, under the same title, stays");
    assert_eq!(first_answer.try_recv().as_deref(), Ok("cancel"), "the ended sheet answers Cancel");
    assert!(second_answer.try_recv().is_err(), "the other one is still asking");

    save_sheet::end(2);
    assert!(second.attachedSheet().is_none());
    println!("mac_save_sheet_appkit: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
