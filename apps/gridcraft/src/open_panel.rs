//! File › Open on macOS: the system Open panel as a window of its own, as Numbers and TextEdit
//! show it. Not rfd's blocking `runModal`, which runs a modal loop inside winit's event handler and
//! aborts when the panel is resized (#2906), and not a sheet on the front workbook either: Open
//! belongs to no workbook (storytold/gridcraft#2908 makes the other file pickers sheets). The
//! picked workbook opens in a window of its own (`SheetApp::open_path`).

use gridcraft_ui_egui::SheetApp;
use objc2::MainThreadMarker;
use objc2::rc::Weak;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{NSArray, NSString};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Makes File › Open show the panel; the picked paths arrive on the returned receiver, for
/// [`poll`]. Without the main thread (never, in the app) File › Open keeps the blocking picker.
pub fn connect(app: &mut SheetApp, ctx: &egui::Context) -> Receiver<String> {
    let (tx, rx) = channel();
    if let Some(mtm) = MainThreadMarker::new() {
        let ctx = ctx.clone();
        app.services.open_async = Some(Box::new(move || {
            show(mtm, tx.clone(), ctx.clone());
        }));
    }
    rx
}

/// Opens the workbooks picked since the last frame.
pub fn poll(picked: &Receiver<String>, app: &mut SheetApp) {
    for path in picked.try_iter() {
        app.open_path(&path);
    }
}

/// Shows the panel and returns at once; `answer` gets the picked path (nothing when cancelled).
pub fn show(mtm: MainThreadMarker, answer: Sender<String>, ctx: egui::Context) -> objc2::rc::Retained<NSOpenPanel> {
    let panel = NSOpenPanel::openPanel(mtm);
    let exts: Vec<_> = crate::SPREADSHEETS.iter().map(|e| NSString::from_str(e)).collect();
    #[allow(deprecated)] // UTType needs another framework; rfd's macOS backend uses this too.
    panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&exts)));
    // Weak: the panel keeps the handler alive until it closes, so a strong reference would leak it.
    let weak = Weak::from_retained(&panel);
    let handler = block2::RcBlock::new(move |response| {
        if response == NSModalResponseOK
            && let Some(path) = weak.load().and_then(|panel| panel.URL()).and_then(|url| url.path())
        {
            let _ = answer.send(path.to_string());
            ctx.request_repaint();
        }
    });
    panel.beginWithCompletionHandler(&handler);
    panel
}
