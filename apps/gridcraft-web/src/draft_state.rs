//! Bounded host revision pulses include pending native editor drafts.

use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use sha2::{Digest, Sha256};

#[derive(PartialEq)]
struct Fingerprint {
    active: Option<(u64, u64)>,
    cell: Option<(u64, usize, CellRef, [u8; 32])>,
    text_box: Option<(u32, [u8; 32])>,
}

fn changed_cell(app: &SheetApp) -> bool {
    app.editor.as_ref().is_some_and(|edit| edit.original.as_ref().is_none_or(|original| original != &edit.text))
}

/// Draft buffers are native editor state, not yet part of `DocState::wb`.
pub fn dirty(app: &SheetApp) -> bool {
    app.session.documents().iter().any(|doc| doc.is_dirty())
        || changed_cell(app)
        || app.text_box_editor.as_ref().is_some_and(|edit| edit.is_modified())
}

/// Constant-size activity state: never clone or retain an input-sized draft.
/// The counter is an opaque equality token, not a JavaScript number or a lease.
#[derive(Default)]
pub struct Revisions {
    previous: Option<Fingerprint>,
    serial: u64,
}

impl Revisions {
    pub fn observe(&mut self, app: &SheetApp) -> bool {
        let fingerprint = Fingerprint {
            active: app.session.active().map(|doc| (doc.uid, doc.revision)),
            cell: app
                .editor
                .as_ref()
                .filter(|_| changed_cell(app))
                .map(|edit| (edit.doc, edit.sheet, edit.cell, Sha256::digest(edit.text.as_bytes()).into())),
            text_box: app
                .text_box_editor
                .as_ref()
                .filter(|edit| edit.is_modified())
                .map(|edit| (edit.id, Sha256::digest(edit.text.as_bytes()).into())),
        };
        if self.previous.as_ref() == Some(&fingerprint) {
            return false;
        }
        self.previous = Some(fingerprint);
        self.serial = self.serial.wrapping_add(1);
        true
    }

    pub fn sequence(&self) -> u64 {
        self.serial
    }

    pub fn revision(&self) -> String {
        self.sequence().to_string()
    }
}

#[cfg(test)]
mod tests {
    use gridcraft_engine::Session;
    use gridcraft_ui_egui::{Services, SheetApp};
    use serde_json::json;

    fn app() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        app.run("cell.set", json!({"cell":"A1", "input":"first"})).unwrap();
        app
    }

    #[test]
    fn pending_cell_survives_older_ack_and_every_text_edit_pulses_without_enter() {
        let mut app = app();
        let mut revisions = super::Revisions::default();
        revisions.observe(&app);
        let mut saves = crate::managed_save::ManagedSaves::default();
        let first = crate::native_document::export(&mut app).unwrap();
        let first_revision = revisions.revision();
        saves.remember("first", &first, &app, &first_revision).unwrap();
        let engine_revision = app.session.active().unwrap().revision;
        app.begin_edit(Some("second".into()), false);
        assert!(revisions.observe(&app));
        let pending = revisions.revision();
        assert_ne!(first_revision, pending);
        assert!(!revisions.observe(&app));
        app.editor.as_mut().unwrap().text.push('!');
        assert!(revisions.observe(&app));
        assert_ne!(pending, revisions.revision());
        assert_eq!(engine_revision, app.session.active().unwrap().revision);
        assert!(saves.acknowledge(&mut app, "first", &first.document, &first_revision));
        assert!(!app.session.active().unwrap().is_dirty());
        assert!(super::dirty(&app), "a pending cell is unsaved even after the older workbook ACK");
        let second = crate::native_document::export(&mut app).unwrap();
        assert!(revisions.observe(&app), "export flushes the pending native cell before its revision is returned");
        let second_revision = revisions.revision();
        saves.remember("second", &second, &app, &second_revision).unwrap();
        assert!(saves.acknowledge(&mut app, "second", &second.document, &second_revision));
        assert!(!super::dirty(&app));
    }

    #[test]
    fn unchanged_editor_is_clean_and_invalid_draft_stays_dirty_until_cancelled() {
        let mut app = app();
        let first = crate::native_document::export(&mut app).unwrap();
        let mut saves = crate::managed_save::ManagedSaves::default();
        saves.remember("first", &first, &app, &first.revision).unwrap();
        assert!(saves.acknowledge(&mut app, "first", &first.document, &first.revision));
        app.begin_edit(None, false);
        assert!(!super::dirty(&app));
        app.editor.as_mut().unwrap().text = "=1+".into();
        assert!(super::dirty(&app));
        assert!(crate::native_document::export(&mut app).is_err());
        assert!(super::dirty(&app));
        app.cancel_edit();
        assert!(!super::dirty(&app));
    }

    #[test]
    fn pending_text_box_survives_ack_and_flushes_through_the_native_engine() {
        let mut app = app();
        let id = app.run("insert.textBox", json!({"at":"B2","text":"original"})).unwrap()["shape"].as_u64().unwrap() as u32;
        let first = crate::native_document::export(&mut app).unwrap();
        let mut saves = crate::managed_save::ManagedSaves::default();
        saves.remember("first", &first, &app, &first.revision).unwrap();
        assert!(saves.acknowledge(&mut app, "first", &first.document, &first.revision));
        assert!(app.begin_text_box_edit(id));
        assert!(!super::dirty(&app));
        let mut revisions = super::Revisions::default();
        revisions.observe(&app);
        let before = revisions.revision();
        saves.remember("older", &first, &app, &before).unwrap();
        app.text_box_editor.as_mut().unwrap().text = "pending text".into();
        assert!(revisions.observe(&app));
        assert_ne!(before, revisions.revision());
        assert!(saves.acknowledge(&mut app, "older", &first.document, &before));
        assert!(!app.session.active().unwrap().is_dirty());
        assert!(super::dirty(&app));
        let snapshot = crate::native_document::export(&mut app).unwrap();
        let (wb, _) = gridcraft_engine::io::open_bytes("snapshot.json", &snapshot.bytes).unwrap();
        assert_eq!(wb.active().unwrap().shapes.iter().find(|shape| shape.id == id).unwrap().text, "pending text");
        assert!(app.text_box_editor.is_none());
    }

    #[test]
    fn draft_counter_wraps_with_a_bounded_opaque_string() {
        let mut app = app();
        let mut revisions = super::Revisions::default();
        revisions.observe(&app);
        revisions.serial = u64::MAX;
        app.begin_edit(Some("another".into()), false);
        assert!(revisions.observe(&app));
        assert_eq!(revisions.revision(), "0");
    }
}
