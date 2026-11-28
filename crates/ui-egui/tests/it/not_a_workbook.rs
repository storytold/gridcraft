//! A PDF reached by a name typed in the Open window, or by the Recent list, is refused with words
//! and opens nothing (#4299); the file stays as it was. (A file dropped on a window goes the same
//! way, `SheetApp::open_path`.)
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::{Services, SheetApp, backstage::Page};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A folder of its own under the system's temporary folder, removed afterwards.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A made-up PDF named `scan.pdf` in a folder of its own.
fn pdf_file(tag: &str) -> (Folder, PathBuf, Vec<u8>) {
    let dir = std::env::temp_dir().join(format!("gridcraft-not-a-workbook-ui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scan.pdf");
    let bytes = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n".to_vec();
    std::fs::write(&path, &bytes).unwrap();
    (Folder(dir), path, bytes)
}

/// The words, one workbook (the blank one), and the file as it was.
fn refused(h: &Harness, path: &std::path::Path, bytes: &[u8]) {
    let message = h.state().message.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
    assert!(message.contains("scan.pdf") && message.contains(gridcraft_engine::io::NOT_A_SHEET_PDF), "{message}");
    assert_eq!(h.state().session.documents().len(), 1, "nothing opened");
    assert!(h.state().session.active().unwrap().path.is_none());
    assert_eq!(std::fs::read(path).unwrap(), bytes, "the file as it was");
}

#[test]
fn a_pdf_named_in_the_open_window_is_refused() {
    let (_dir, path, bytes) = pdf_file("typed");
    let picked = path.to_string_lossy().into_owned();
    let services = Services { pick_open: Some(Box::new(move || Some(picked.clone()))), ..Default::default() };
    let mut h = crate::support::services_harness(services, 4);
    h.state_mut().run_or_alert("file.open", json!({}));
    h.run_steps(3);
    refused(&h, &path, &bytes);
    assert!(h.state().ui.recent.is_empty(), "not put in the Recent list");
}

#[test]
fn a_pdf_in_the_recent_list_is_refused() {
    let (_dir, path, bytes) = pdf_file("recent");
    let mut h = crate::support::services_harness(Services::default(), 4);
    // Put there by an earlier build, which opened it.
    h.state_mut().ui.recent.insert(0, path.to_string_lossy().into_owned());
    h.state_mut().open_backstage(Page::Open);
    h.run_steps(3);
    h.get_by_label("scan.pdf").click();
    h.run_steps(3);
    refused(&h, &path, &bytes);
}
