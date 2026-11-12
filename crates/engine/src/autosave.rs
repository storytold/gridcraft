//! AutoSave jobs: a save of a document's own file that can run away from the UI thread.
//!
//! Taking a job is cheap: the workbook is an `Arc` snapshot, and edits made while the job runs
//! copy on write, so they never touch what is being saved. When the job finishes,
//! [`Session::finish_save`] marks the document clean only if it hasn't changed since.

use std::sync::Arc;

use gridcraft_model::Workbook;

use crate::{Result, Session, io};

/// A snapshot of one document's workbook and the file it saves to.
#[derive(Clone, Debug)]
pub struct SaveJob {
    /// The document the snapshot came from ([`crate::DocState::uid`]).
    pub uid: u64,
    pub path: String,
    wb: Arc<Workbook>,
    kind: io::FileKind,
}

impl SaveJob {
    /// Serialises the snapshot and writes it (temporary file, then rename). Safe on any thread;
    /// returns the number of bytes written.
    pub fn run(&self) -> Result<usize> {
        let rebased = io::source_dir(&self.path).and_then(|f| gridcraft_xlsx::rebase_links(&self.wb, &f));
        let wb = rebased.as_ref().unwrap_or(&self.wb);
        let bytes = io::save_bytes_as(wb, &self.path, self.kind)?;
        io::write_file(&self.path, &bytes)?;
        Ok(bytes.len())
    }
}

impl Session {
    /// A save job for the active document when it has unsaved changes and its own file is a
    /// workbook format that keeps everything (XLSX); `None` otherwise.
    pub fn autosave_job(&self) -> Option<SaveJob> {
        let d = self.active()?;
        if !d.is_dirty() {
            return None;
        }
        let path = d.path.clone()?;
        let kind = io::FileKind::from_path(&path).or(d.content_kind)?;
        (kind == io::FileKind::Xlsx).then(|| SaveJob { uid: d.uid, path, wb: d.wb.clone(), kind })
    }

    /// Records a job that finished writing. The document becomes clean when it still holds the
    /// saved snapshot; one changed since stays dirty. Returns whether the document is still open
    /// on the same file.
    pub fn finish_save(&mut self, job: &SaveJob) -> bool {
        let Some(d) = self.docs.iter_mut().find(|d| d.uid == job.uid) else { return false };
        if d.path.as_deref() != Some(job.path.as_str()) {
            return false;
        }
        d.saved = job.wb.clone();
        true
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn session_with_file(name: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("gridcraft-autosave-job-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.xlsx");
        let mut s = Session::new();
        s.new_workbook();
        s.active_mut().unwrap().path = Some(path.to_string_lossy().into_owned());
        (s, dir)
    }

    fn saved_a1(dir: &std::path::Path) -> gridcraft_core::Value {
        let bytes = std::fs::read(dir.join("book.xlsx")).unwrap();
        let (wb, _) = crate::io::open_bytes("book.xlsx", &bytes).unwrap();
        wb.sheet(0).unwrap().value(gridcraft_core::CellRef::new(0, 0))
    }

    #[test]
    fn clean_documents_and_other_formats_have_no_job() {
        let (mut s, dir) = session_with_file("none");
        assert!(s.autosave_job().is_none(), "nothing to save");
        s.active_mut().unwrap().path = Some(dir.join("book.csv").to_string_lossy().into_owned());
        s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
        assert!(s.autosave_job().is_none(), "CSV loses formatting; AutoSave leaves it alone");
        s.active_mut().unwrap().path = None;
        assert!(s.autosave_job().is_none(), "no file yet");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn job_saves_the_snapshot_and_cleans_the_document() {
        let (mut s, dir) = session_with_file("clean");
        s.execute("cell.set", json!({"cell": "A1", "input": "first"})).unwrap();
        let job = s.autosave_job().unwrap();
        let worker = job.clone();
        let written = std::thread::spawn(move || worker.run()).join().unwrap().unwrap();
        assert!(written > 0);
        assert!(s.finish_save(&job));
        assert!(!s.active().unwrap().is_dirty());
        assert_eq!(saved_a1(&dir), gridcraft_core::Value::text("first"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn edits_made_while_saving_stay_unsaved() {
        let (mut s, dir) = session_with_file("racing");
        s.execute("cell.set", json!({"cell": "A1", "input": "snapshot"})).unwrap();
        let job = s.autosave_job().unwrap();
        // The user keeps typing while the worker writes.
        s.execute("cell.set", json!({"cell": "A1", "input": "newer"})).unwrap();
        job.run().unwrap();
        assert!(s.finish_save(&job));
        assert!(s.active().unwrap().is_dirty(), "the newer edit is not in the file");
        assert_eq!(saved_a1(&dir), gridcraft_core::Value::text("snapshot"), "the file holds the snapshot, never a mix");
        // The next job picks the newer edit up.
        let next = s.autosave_job().unwrap();
        next.run().unwrap();
        s.finish_save(&next);
        assert!(!s.active().unwrap().is_dirty());
        assert_eq!(saved_a1(&dir), gridcraft_core::Value::text("newer"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn finished_job_for_a_closed_or_moved_document_changes_nothing() {
        let (mut s, dir) = session_with_file("moved");
        s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
        let job = s.autosave_job().unwrap();
        s.active_mut().unwrap().path = Some(dir.join("other.xlsx").to_string_lossy().into_owned());
        assert!(!s.finish_save(&job), "Save As moved the document");
        assert!(s.active().unwrap().is_dirty());
        s.close_document(0);
        assert!(!s.finish_save(&job));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
