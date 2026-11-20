//! "Save changes?" as a sheet on the workbook's own window, the way Mac apps ask it, instead of
//! GridCraft's built-in dialog: the hook for `Services::save_prompt`. A sheet holds only its
//! window, so the other windows still take input. The same sheet as WordCraft's (storytold/wordcraft#6864).

use block2::RcBlock;
use gridcraft_ui_egui::SavePrompt;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertThirdButtonReturn, NSApplication, NSModalResponse, NSWindow};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::sync::mpsc::channel;

/// A Save Changes sheet up now: the id it was asked with, its document window and the sheet.
type Asked = (u64, Retained<NSWindow>, Retained<NSWindow>);

thread_local! {
    static ASKING: RefCell<Vec<Asked>> = const { RefCell::new(Vec::new()) };
}

/// The hook for `Services::end_save_prompt`: the sheet asked with `id` goes, answering Cancel
/// to nobody; other sheets stay, even on a window with the same title.
pub fn end(id: u64) {
    let gone: Vec<_> = ASKING.with_borrow(|a| a.iter().filter(|(i, ..)| *i == id).map(|(_, w, s)| (w.clone(), s.clone())).collect());
    // Outside the borrow: ending runs the completion handler, which takes the sheet off ASKING.
    for (window, sheet) in gone {
        window.endSheet(&sheet);
    }
}

pub fn hook(ctx: &egui::Context) -> SavePrompt {
    let ctx = ctx.clone();
    Box::new(move |id, title, question, detail| {
        let mtm = MainThreadMarker::new()?;
        // No window that is surely the workbook's: the app asks in its own dialog instead.
        let window = window_titled(mtm, title)?;
        // A sheet on a window in the Dock would ask out of sight.
        if window.isMiniaturized() {
            window.deminiaturize(None);
        }
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(question));
        alert.setInformativeText(&NSString::from_str(detail));
        // Save is the default button, Cancel answers Escape and Don't Save sits apart, as in every
        // Mac app.
        alert.addButtonWithTitle(&NSString::from_str("Save"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel")).setKeyEquivalent(&NSString::from_str("\u{1b}"));
        alert.addButtonWithTitle(&NSString::from_str("Don't Save"));
        let (answer, rx) = channel();
        let ctx = ctx.clone();
        let keep = alert.clone();
        let sheet = alert.window();
        ASKING.with_borrow_mut(|a| a.push((id, window.clone(), sheet.clone())));
        let done = RcBlock::new(move |response: NSModalResponse| {
            let _ = &keep; // the alert lives until it has answered
            ASKING.with_borrow_mut(|a| a.retain(|(.., s)| *s != sheet));
            let reply = match response {
                r if r == NSAlertFirstButtonReturn => "save",
                r if r == NSAlertThirdButtonReturn => "dontSave",
                _ => "cancel",
            };
            let _ = answer.send(reply.into());
            ctx.request_repaint();
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&done));
        Some(rx)
    })
}

/// The one window titled `title`, shown or in the Dock: each window carries its workbook's name.
/// Two workbooks with the same name are left to the built-in dialog, so Don't Save never answers
/// for the wrong one.
/// shortcut: matches by title; upgrade to the viewport's own window handle if egui exposes it.
fn window_titled(mtm: MainThreadMarker, title: &str) -> Option<Retained<NSWindow>> {
    let windows = NSApplication::sharedApplication(mtm).windows();
    let mut found = windows.iter().filter(|w| (w.isVisible() || w.isMiniaturized()) && w.title().to_string() == title);
    let window = found.next()?;
    found.next().is_none().then_some(window)
}

#[cfg(test)]
mod tests {
    #[test]
    fn without_the_workbooks_window_the_app_asks_itself() {
        let hook = super::hook(&egui::Context::default());
        // Off the main thread, as here, or with no such window, no sheet can be placed.
        assert!(hook(1, "Book1 — Edited", "Do you want to save the changes you made to Book1?", "").is_none());
    }
}
