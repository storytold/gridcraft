//! macOS file panels as sheets.
//!
//! `NSSavePanel.runModal` spins a nested run loop inside winit's event handler. winit 0.30
//! panics on that re-entry, and the panic cannot unwind, so the process aborts (#2790).
//! A sheet (`beginSheetModalForWindow`) returns immediately. The chosen path is delivered
//! on a later frame.
//!
//! The format menu (`showsContentTypes`) is off unless we turn it on. `rfd` never does, and
//! it also drops filter names, so Excel Workbook was not a choice. The types here are UTIs.
//! `.xlsx` is exported by `packaging/macos/Info.plist.in` so the panel can resolve it.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use gridcraft_ui_egui::{Inbox, SheetApp};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSModalResponse, NSModalResponseOK, NSSavePanel, NSWindow};
use objc2_foundation::{NSArray, NSString};

/// Extension, then the UTI the save panel is given. The suggested name's type comes first.
const SAVE_TYPES: [(&str, &str); 4] = [
    ("xlsx", "org.openxmlformats.spreadsheetml.sheet"),
    ("csv", "public.comma-separated-values-text"),
    ("html", "public.html"),
    ("pdf", "com.adobe.pdf"),
];

pub(crate) fn content_types_for(suggested: &str) -> Vec<&'static str> {
    let ext = std::path::Path::new(suggested).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    // CSV, HTML, and PDF exports force that extension after the panel returns. Offering Excel
    // Workbook would confirm an `.xlsx` path that is never written, and the rewritten file can
    // replace an existing one without the panel's confirmation.
    if let Some(ext) = ext.as_deref()
        && matches!(ext, "csv" | "html" | "pdf")
        && let Some((_, uti)) = SAVE_TYPES.iter().find(|(name, _)| *name == ext)
    {
        return vec![*uti];
    }
    let mut types = Vec::with_capacity(SAVE_TYPES.len());
    if let Some(ext) = ext.as_deref()
        && let Some((_, uti)) = SAVE_TYPES.iter().find(|(name, _)| *name == ext)
    {
        types.push(*uti);
    }
    for (name, uti) in SAVE_TYPES {
        if ext.as_deref() != Some(name) {
            types.push(uti);
        }
    }
    types
}

struct SaveSlot {
    generation: u64,
    panel: Option<Retained<NSSavePanel>>,
}

thread_local! {
    static SAVE_PANEL: RefCell<SaveSlot> = const { RefCell::new(SaveSlot { generation: 0, panel: None }) };
}

/// Replaces the blocking pickers. Call once the egui context exists, on the main thread.
pub fn install(app: &mut SheetApp, ctx: egui::Context) {
    let saved: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let saved_for_panel = Arc::clone(&saved);
    let ctx_save = ctx.clone();
    app.services.pick_save = None;
    app.services.picked_save = Some(saved);
    app.services.pick_save_async = Some(Box::new(move |suggested| present_save(suggested, Arc::clone(&saved_for_panel), ctx_save.clone())));

    let opened: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let opened_for_panel = Arc::clone(&opened);
    let ctx_open = ctx.clone();
    app.services.pick_open = None;
    app.services.picked_open = Some(opened);
    app.services.pick_open_async = Some(Box::new(move || present_open(Arc::clone(&opened_for_panel), ctx_open.clone())));

    let ctx_picture = ctx;
    app.services.pick_picture = None;
    app.services.pick_picture_async = Some(Box::new(move |inbox| present_picture(inbox, ctx_picture.clone())));
}

fn push_path(queue: &Mutex<Vec<Option<String>>>, path: Option<String>) {
    queue.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(path);
}

fn present_save(suggested: &str, queue: Arc<Mutex<Vec<Option<String>>>>, ctx: egui::Context) {
    let Some(mtm) = MainThreadMarker::new() else {
        log::error!("save panel: not on the main thread");
        push_path(&queue, None);
        ctx.request_repaint();
        return;
    };
    let panel = NSSavePanel::savePanel(mtm);
    panel.setCanCreateDirectories(true);
    panel.setExtensionHidden(false);
    panel.setShowsContentTypes(true);
    let types = content_types_for(suggested);
    panel.setAllowsOtherFileTypes(types.len() != 1);
    let names: Vec<Retained<NSString>> = types.iter().map(|uti| NSString::from_str(uti)).collect();
    let allowed = NSArray::from_retained_slice(&names);
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&allowed));
    panel.setNameFieldStringValue(&NSString::from_str(suggested));

    let generation = SAVE_PANEL.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.generation = slot.generation.wrapping_add(1);
        slot.panel = Some(panel.clone());
        slot.generation
    });

    let handler = RcBlock::new(move |result: NSModalResponse| {
        let path = SAVE_PANEL.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.generation != generation {
                return None;
            }
            let panel = slot.panel.take()?;
            if result != NSModalResponseOK {
                return None;
            }
            panel.URL().and_then(|url| url.path()).map(|path| path.to_string())
        });
        // A stale completion (a newer panel replaced this one) must not cancel the new save.
        let current = SAVE_PANEL.with(|slot| slot.borrow().generation);
        if current == generation {
            push_path(&queue, path);
            ctx.request_repaint();
        }
    });

    let app = NSApplication::sharedApplication(mtm);
    let parent: Option<Retained<NSWindow>> = app.mainWindow().or_else(|| app.windows().firstObject());
    if let Some(window) = parent.as_ref() {
        panel.beginSheetModalForWindow_completionHandler(window, &handler);
    } else {
        panel.beginWithCompletionHandler(&handler);
    }
}

fn present_open(queue: Arc<Mutex<Vec<Option<String>>>>, ctx: egui::Context) {
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("Spreadsheets", &["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "txt", "json"])
        .add_filter("Excel Workbook", &["xlsx", "xlsm"])
        .add_filter("Excel 97-2003 Workbook (import)", &["xls"])
        .add_filter("Excel Binary Workbook (data import)", &["xlsb"])
        .add_filter("OpenDocument Spreadsheet (data import)", &["ods"])
        .add_filter("CSV", &["csv"])
        .pick_file();
    spawn_panel("gridcraft-open", move || {
        let chosen = pollster::block_on(dialog);
        push_path(&queue, chosen.and_then(|file| file.path().to_str().map(str::to_string)));
        ctx.request_repaint();
    });
}

fn present_picture(inbox: Inbox, ctx: egui::Context) {
    let dialog = rfd::AsyncFileDialog::new().add_filter("Pictures (PNG, JPEG)", &["png", "jpg", "jpeg"]).pick_file();
    spawn_panel("gridcraft-picture", move || {
        let chosen = pollster::block_on(dialog);
        if let Some(file) = chosen
            && let Some(path) = file.path().to_str()
            && let Ok(bytes) = std::fs::read(path)
        {
            let name = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("picture").to_string();
            inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((name, bytes));
        }
        ctx.request_repaint();
    });
}

fn spawn_panel(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.to_string()).spawn(work) {
        log::error!("{name}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::content_types_for;

    #[test]
    fn a_workbook_save_lists_xlsx_first() {
        let types = content_types_for("kit-prod.xlsx");
        assert_eq!(types.first().copied(), Some("org.openxmlformats.spreadsheetml.sheet"));
        assert!(types.contains(&"public.comma-separated-values-text"));
        assert!(types.contains(&"public.html"));
        assert!(types.contains(&"com.adobe.pdf"));
    }

    #[test]
    fn an_export_panel_offers_only_that_type() {
        assert_eq!(content_types_for("Report.pdf"), vec!["com.adobe.pdf"]);
        assert_eq!(content_types_for("Report.PDF"), vec!["com.adobe.pdf"]);
        assert_eq!(content_types_for("kit.csv"), vec!["public.comma-separated-values-text"]);
        assert_eq!(content_types_for("page.html"), vec!["public.html"]);
    }
}
