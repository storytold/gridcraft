//! Host save payloads and workbook receipts, shared with native regression tests.

use gridcraft_ui_egui::SheetApp;

/// Only these outputs replace the editable workbook in the host.
const WORKBOOK_EXTS: &[&str] = &["xlsx"];

pub struct SaveMessage<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
    pub export: bool,
}

impl<'a> SaveMessage<'a> {
    pub fn new(name: &'a str, bytes: &'a [u8]) -> Self {
        // All other outputs are exports, not necessarily PDFs.
        Self { name, bytes, export: !is_workbook(name) }
    }
}

fn is_workbook(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| WORKBOOK_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// Preserve the bridge's existing rule: only a workbook receipt marks the document saved.
pub fn sync_saved(saved: &[String], app: &mut SheetApp) {
    if let Some(name) = saved.iter().rev().find(|name| is_workbook(name))
        && let Some(doc) = app.session.active_mut()
    {
        doc.saved = doc.wb.clone();
        if doc.path.is_none() {
            doc.title = name.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn save_payload_marks_exports_and_preserves_name_and_bytes() {
        let bytes = [0, 1, 127, 128, 255];
        for (name, export) in [("finance.xlsx", false), ("finance.XLSX", false), ("finance.pdf", true), ("finance.PDF", true)] {
            let message = SaveMessage::new(name, &bytes);
            assert_eq!(message.export, export, "{name}");
            assert_eq!(message.name, name);
            assert_eq!(message.bytes, bytes.as_slice());
            assert_eq!(message.bytes.as_ptr(), bytes.as_ptr(), "the payload borrows the original bytes");
        }
    }

    #[test]
    fn other_outputs_are_exports_not_workbook_saves() {
        for name in ["finance.csv", "finance.bin", "finance", "finance.xlsx.pdf", "finance.xlsx.bak"] {
            assert!(SaveMessage::new(name, &[]).export, "{name}");
        }
        assert!(!SaveMessage::new("finance.pdf.xlsx", &[]).export);
    }

    fn edited_app() -> Result<SheetApp, String> {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("cell.set", json!({"cell": "A1", "input": "42"})).map_err(|e| e.to_string())?;
        Ok(SheetApp::new(session, Default::default()))
    }

    #[test]
    fn pdf_receipts_leave_the_workbook_dirty_and_its_title_unchanged() -> Result<(), String> {
        for name in ["finance.pdf", "finance.PDF"] {
            let mut app = edited_app()?;
            let doc = app.session.active().ok_or("no active document")?;
            assert!(doc.is_dirty());
            let title = doc.title.clone();
            let saved = doc.saved.clone();
            sync_saved(&[name.to_string()], &mut app);
            let doc = app.session.active().ok_or("no active document")?;
            assert!(doc.is_dirty(), "{name}");
            assert!(Arc::ptr_eq(&saved, &doc.saved), "an export must not replace the saved snapshot");
            assert_eq!(doc.title, title);
        }
        Ok(())
    }

    #[test]
    fn workbook_receipts_mark_saved_and_later_exports_do_not_replace_them() -> Result<(), String> {
        for name in ["finance.xlsx", "finance.XLSX"] {
            let mut app = edited_app()?;
            assert!(app.session.active().ok_or("no active document")?.is_dirty());
            sync_saved(&[name.to_string(), "finance.pdf".to_string()], &mut app);
            let doc = app.session.active().ok_or("no active document")?;
            assert!(!doc.is_dirty());
            assert!(Arc::ptr_eq(&doc.saved, &doc.wb));
            assert_eq!(doc.title, name);
            app.session.execute("cell.set", json!({"cell": "A1", "input": "43"})).map_err(|e| e.to_string())?;
            sync_saved(&["finance.PDF".to_string()], &mut app);
            assert!(app.session.active().ok_or("no active document")?.is_dirty(), "a subsequent export leaves new edits unsaved");
        }
        Ok(())
    }
}
