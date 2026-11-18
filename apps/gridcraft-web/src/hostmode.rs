//! Pure host-mode rules, shared by the bridge ([`crate::host`]) and testable without a browser
//! (like [`crate::prefs`]): the name the bridge fills in when a host's open request has none, and
//! the wiring of [`Services::host_save`] / [`Services::download`] onto a host post callback.
//! Which pages host the app is `craft-web-host`'s.

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_ui_egui::{Services, SheetApp};

/// The file name used when an open request carries no usable one.
pub const DEFAULT_OPEN_NAME: &str = "Book.xlsx";

/// Saved files that count as a workbook: the document is then clean under the name it was saved as.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

/// The name `craft-web-host` gives an open request that carries no usable one (it cleans a host's
/// name and falls back to this).
const NAMELESS: &str = "document";

/// A usable file name for an open request: the cleaned name the crate hands over, or
/// [`DEFAULT_OPEN_NAME`] when the host sent none.
pub fn open_name(name: &str) -> String {
    if name == NAMELESS { DEFAULT_OPEN_NAME.to_string() } else { name.to_string() }
}

/// Extracts the bare file name from `path`.
pub fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// Returns whether `name` has a workbook extension (`.xlsx`).
pub fn is_workbook(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| WORKBOOK_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// Wires [`Services::host_save`] (for `.xlsx` saves with `save_as` forwarded; the caller marks the
/// workbook saved inline on `Ok(())`) and [`Services::download`] (for exports and fallback
/// downloads with `save_as = false`, recording the file name in `saved`).
pub fn wire_save_services<F>(services: &mut Services, saved: Rc<RefCell<Vec<String>>>, post: F)
where
    F: Fn(&str, &[u8], bool) -> Result<(), String> + Clone + 'static,
{
    let post_wb = post.clone();
    services.host_save = Some(Box::new(move |path: &str, bytes: &[u8], save_as: bool| {
        let name = file_name(path);
        post_wb(&name, bytes, save_as)
    }));
    services.download = Some(Box::new(move |path: &str, bytes: &[u8]| {
        let name = file_name(path);
        match post(&name, bytes, false) {
            Ok(()) => saved.borrow_mut().push(name),
            Err(e) => log::error!("couldn't hand {name} to the page: {e}"),
        }
    }));
}

/// Marks the active workbook clean under the most recent `.xlsx` file name recorded in `saved`.
pub fn apply_saved(app: &mut SheetApp, saved: &RefCell<Vec<String>>) {
    let names = std::mem::take(&mut *saved.borrow_mut());
    if let Some(name) = names.iter().rev().find(|name| is_workbook(name)) {
        app.mark_active_saved_xlsx(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;

    #[test]
    fn open_names_fall_back_to_the_default_when_the_host_sent_none() {
        assert_eq!(open_name("Budget.xlsx"), "Budget.xlsx");
        assert_eq!(open_name("Sheet2!Book.xlsx"), "Sheet2!Book.xlsx");
        assert_eq!(open_name("document"), DEFAULT_OPEN_NAME, "what the crate says for a missing or blank name");
    }

    #[test]
    fn wire_save_services_forwards_save_as_without_tracking_and_download_tracks_saved_workbooks() {
        type SaveCall = (String, Vec<u8>, bool);
        let calls: Rc<RefCell<Vec<SaveCall>>> = Rc::default();
        let saved: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut services = Services::default();
        let log = calls.clone();
        wire_save_services(&mut services, saved.clone(), move |name, bytes, save_as| {
            log.borrow_mut().push((name.to_string(), bytes.to_vec(), save_as));
            Ok(())
        });

        services.host_save.as_ref().unwrap()("/tmp/Quarterly.xlsx", b"xlsx-1", false).unwrap();
        services.host_save.as_ref().unwrap()("dir/Copy.xlsx", b"xlsx-2", true).unwrap();
        assert!(saved.borrow().is_empty(), "host_save callers mark the workbook clean inline and do not push to saved");

        services.download.as_ref().unwrap()("/tmp/Report.pdf", b"pdf");
        services.download.as_ref().unwrap()("/tmp/Fallback.xlsx", b"xlsx-3");
        assert_eq!(*saved.borrow(), vec!["Report.pdf".to_string(), "Fallback.xlsx".to_string()]);
        assert_eq!(
            *calls.borrow(),
            vec![
                ("Quarterly.xlsx".to_string(), b"xlsx-1".to_vec(), false),
                ("Copy.xlsx".to_string(), b"xlsx-2".to_vec(), true),
                ("Report.pdf".to_string(), b"pdf".to_vec(), false),
                ("Fallback.xlsx".to_string(), b"xlsx-3".to_vec(), false),
            ]
        );

        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        app.run("cell.set", serde_json::json!({"cell": "A1", "input": "7"})).unwrap();
        assert!(app.session.active().unwrap().is_dirty());
        apply_saved(&mut app, &saved);
        assert!(saved.borrow().is_empty());
        let doc = app.session.active().unwrap();
        assert!(!doc.is_dirty());
        assert_eq!(doc.display_title(), "Fallback.xlsx");
        assert_eq!(doc.wb.source_path.as_deref(), Some(std::path::Path::new("Fallback.xlsx")));
        assert_eq!(doc.content_kind, Some(gridcraft_engine::io::FileKind::Xlsx));
    }
}
