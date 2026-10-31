//! A managed host only acknowledges the exact native snapshot it durably stored.

use std::sync::Arc;

use gridcraft_engine::model::Workbook;
use gridcraft_ui_egui::SheetApp;

use crate::native_document::Snapshot;

struct Pending {
    request: String,
    document: String,
    revision: String,
    workbook: Arc<Workbook>,
}

/// Retains only the latest exported baseline. Exporting and requesting a save
/// never change `DocState::saved`; only a matching host acknowledgement does.
#[derive(Default)]
pub struct ManagedSaves {
    pending: Option<Pending>,
}

impl ManagedSaves {
    pub fn remember(&mut self, request: &str, snapshot: &Snapshot, app: &SheetApp, revision: &str) -> Result<(), String> {
        let doc = app.session.active().ok_or_else(|| "No workbook is open.".to_string())?;
        if doc.uid.to_string() != snapshot.document || doc.revision.to_string() != snapshot.revision {
            return Err("The native snapshot no longer identifies the active document.".into());
        }
        self.pending = Some(Pending {
            request: request.to_string(),
            document: snapshot.document.clone(),
            revision: revision.to_string(),
            workbook: doc.wb.clone(),
        });
        Ok(())
    }

    pub fn acknowledge(&mut self, app: &mut SheetApp, request: &str, document: &str, revision: &str) -> bool {
        let Some(pending) = &self.pending else { return false };
        if pending.request != request
            || pending.document != document
            || pending.revision != revision
            || app.session.active().is_none_or(|doc| doc.uid.to_string() != document)
        {
            return false;
        }
        let Some(pending) = self.pending.take() else { return false };
        let Some(doc) = app.session.active_mut() else { return false };
        // Later edits remain different from this exact exported workbook.
        doc.saved = pending.workbook;
        true
    }
}

#[cfg(test)]
mod tests {
    use gridcraft_engine::Session;
    use gridcraft_ui_egui::{Services, SheetApp};
    use serde_json::json;
    use std::sync::Arc;

    fn app() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        app.run("cell.set", json!({"cell":"A1", "input":"first"})).unwrap();
        app
    }

    #[test]
    fn acknowledgement_retains_later_edits_then_exact_latest_snapshot_clears_dirty() {
        let mut app = app();
        let mut saves = super::ManagedSaves::default();
        let first = crate::native_document::export(&mut app).unwrap();
        saves.remember("request-1", &first, &app, &first.revision).unwrap();
        assert!(app.session.active().unwrap().is_dirty());
        app.run("cell.set", json!({"cell":"A1", "input":"second"})).unwrap();
        assert!(saves.acknowledge(&mut app, "request-1", &first.document, &first.revision));
        assert!(app.session.active().unwrap().is_dirty());
        let second = crate::native_document::export(&mut app).unwrap();
        saves.remember("request-2", &second, &app, &second.revision).unwrap();
        assert!(saves.acknowledge(&mut app, "request-2", &second.document, &second.revision));
        assert!(!app.session.active().unwrap().is_dirty());
    }

    #[test]
    fn old_or_foreign_acknowledgement_cannot_replace_the_latest_baseline() {
        let mut app = app();
        let mut saves = super::ManagedSaves::default();
        let first = crate::native_document::export(&mut app).unwrap();
        saves.remember("request-1", &first, &app, &first.revision).unwrap();
        app.run("cell.set", json!({"cell":"A1", "input":"second"})).unwrap();
        let second = crate::native_document::export(&mut app).unwrap();
        saves.remember("request-2", &second, &app, &second.revision).unwrap();
        let baseline = app.session.active().unwrap().saved.clone();
        for (id, document, revision) in [
            ("request-1", first.document.as_str(), first.revision.as_str()),
            ("wrong-request", second.document.as_str(), second.revision.as_str()),
            ("request-2", "wrong-document", second.revision.as_str()),
            ("request-2", second.document.as_str(), "wrong-revision"),
        ] {
            assert!(!saves.acknowledge(&mut app, id, document, revision));
            assert!(Arc::ptr_eq(&app.session.active().unwrap().saved, &baseline));
        }
        assert!(app.session.active().unwrap().is_dirty());
        assert!(saves.acknowledge(&mut app, "request-2", &second.document, &second.revision));
        assert!(!app.session.active().unwrap().is_dirty());
    }

    #[test]
    fn an_acknowledgement_for_another_document_leaves_its_baseline_untouched() {
        let mut app = app();
        let mut saves = super::ManagedSaves::default();
        let first = crate::native_document::export(&mut app).unwrap();
        saves.remember("request-1", &first, &app, &first.revision).unwrap();
        app.session.new_workbook();
        app.run("cell.set", json!({"cell":"A1", "input":"new document"})).unwrap();
        let baseline = app.session.active().unwrap().saved.clone();
        assert!(!saves.acknowledge(&mut app, "request-1", &first.document, &first.revision));
        assert!(Arc::ptr_eq(&app.session.active().unwrap().saved, &baseline));
        assert!(app.session.active().unwrap().is_dirty());
    }
}
