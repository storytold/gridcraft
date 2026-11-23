//! Save never writes over a workbook's own file that another program changed on disk since it was
//! opened or saved here (#4078): it writes nothing and asks, unless the caller says `overwrite`. A
//! file written again with the same bytes, or one that is gone, is no reason to ask.
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::{EngineError, Session};
use serde_json::json;

/// A folder of its own under the system's temporary folder, removed afterwards.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn folder(tag: &str) -> Folder {
    let dir = std::env::temp_dir().join(format!("gridcraft-changed-on-disk-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Folder(dir)
}

/// The bytes of a one-sheet workbook with `a1` in A1, written as `name` (its extension picks the format).
fn workbook_bytes(dir: &Path, name: &str, a1: &str) -> Vec<u8> {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": a1})).unwrap();
    let path = dir.join(format!("made-{name}"));
    s.execute("file.saveAs", json!({"path": path})).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    bytes
}

/// Puts `bytes` at `path` as many programs save a file: written under a temporary name beside it,
/// then renamed over it.
fn replace_by_rename(path: &Path, bytes: &[u8]) {
    let tmp = path.with_file_name(format!(".other-{}.tmp", path.file_name().unwrap().to_string_lossy()));
    std::fs::write(&tmp, bytes).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

/// A1 of the workbook in the file at `path`, read by a session of its own.
fn a1_on_disk(path: &Path) -> Value {
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0))
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

/// Whether `r` is Save's refusal for a file changed on disk.
fn refused(r: &Result<serde_json::Value, EngineError>) -> bool {
    matches!(r, Err(EngineError::Confirm(why)) if why.contains(gridcraft_engine::io::CHANGED_ON_DISK))
}

/// A session with `path` opened; the file holds a workbook with "as opened" in A1.
fn opened(path: &Path) -> Session {
    std::fs::write(path, workbook_bytes(path.parent().unwrap(), "opened.xlsx", "as opened")).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s
}

#[test]
fn a_save_never_replaces_a_file_another_program_renamed_over_it() {
    let dir = folder("rename");
    let path = dir.0.join("Budget.xlsx");
    let mut s = opened(&path);
    let newer = workbook_bytes(&dir.0, "newer.xlsx", "newer from elsewhere");
    replace_by_rename(&path, &newer);

    // With no edit, and with one: nothing written, and the refusal asks.
    let r = s.execute("file.save", json!({}));
    assert!(refused(&r), "{r:?}");
    assert_eq!(std::fs::read(&path).unwrap(), newer, "the newer file is untouched");
    s.execute("cell.set", json!({"cell": "B1", "input": "my edit"})).unwrap();
    let r = s.execute("file.save", json!({}));
    assert!(refused(&r), "{r:?}");
    assert_eq!(std::fs::read(&path).unwrap(), newer, "the newer file is untouched");
    assert!(s.active().unwrap().is_dirty(), "the edit is still unsaved");
    // Save As to the same file is a save to it as well.
    let r = s.execute("file.saveAs", json!({"path": path}));
    assert!(refused(&r), "{r:?}");
    assert_eq!(std::fs::read(&path).unwrap(), newer, "the newer file is untouched");
    // No file is left beside it.
    let names: Vec<String> = std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["Budget.xlsx"]);
}

#[test]
fn overwrite_replaces_the_newer_file_and_the_next_save_goes_through() {
    let dir = folder("overwrite");
    let path = dir.0.join("Budget.xlsx");
    let mut s = opened(&path);
    replace_by_rename(&path, &workbook_bytes(&dir.0, "newer.xlsx", "newer from elsewhere"));
    s.execute("cell.set", json!({"cell": "A1", "input": "mine"})).unwrap();
    s.execute("file.save", json!({"overwrite": true})).unwrap();
    assert_eq!(a1_on_disk(&path), text("mine"));
    assert!(!s.active().unwrap().is_dirty());
    // The file now holds what was written here: an ordinary save goes through.
    s.execute("cell.set", json!({"cell": "A1", "input": "mine again"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(a1_on_disk(&path), text("mine again"));
}

#[test]
fn the_same_bytes_written_again_or_a_file_that_is_gone_is_saved_as_before() {
    let dir = folder("same");
    let path = dir.0.join("Budget.xlsx");
    let mut s = opened(&path);
    // The same bytes under a new file renamed over it, with another modification time.
    let same = std::fs::read(&path).unwrap();
    replace_by_rename(&path, &same);
    let later = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(4_000_000_000);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "first"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(a1_on_disk(&path), text("first"));
    // Changed, then changed back to what this session wrote: the same bytes again.
    let written = std::fs::read(&path).unwrap();
    replace_by_rename(&path, &workbook_bytes(&dir.0, "other.xlsx", "for a while"));
    replace_by_rename(&path, &written);
    s.execute("cell.set", json!({"cell": "A1", "input": "second"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(a1_on_disk(&path), text("second"));
    // Gone: written again.
    std::fs::remove_file(&path).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "third"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(a1_on_disk(&path), text("third"));
}

#[test]
fn a_file_saved_here_is_watched_from_that_save_and_a_save_as_moves_the_watch() {
    let dir = folder("saved");
    let first = dir.0.join("Book.xlsx");
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "made here"})).unwrap();
    s.execute("file.saveAs", json!({"path": first})).unwrap();
    let newer = workbook_bytes(&dir.0, "newer.xlsx", "newer from elsewhere");
    replace_by_rename(&first, &newer);
    assert!(refused(&s.execute("file.save", json!({}))));
    // Save As to a file of its own: written, and that file is the workbook's from then on.
    let second = dir.0.join("Book (mine).xlsx");
    s.execute("file.saveAs", json!({"path": second})).unwrap();
    assert_eq!(a1_on_disk(&second), text("made here"));
    assert_eq!(std::fs::read(&first).unwrap(), newer, "the newer file is untouched");
    s.execute("cell.set", json!({"cell": "A1", "input": "later"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(a1_on_disk(&second), text("later"));
    replace_by_rename(&second, &newer);
    assert!(refused(&s.execute("file.save", json!({}))));
}

#[test]
fn a_csv_saved_over_its_own_file_is_watched_from_that_save() {
    let dir = folder("csv");
    let path = dir.0.join("data.csv");
    std::fs::write(&path, "Item,Amount\nX1,100\n").unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s.execute("cell.set", json!({"cell": "B2", "input": "150"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    // Its own save changed the file: the next save is no reason to ask.
    s.execute("cell.set", json!({"cell": "B2", "input": "175"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("175"));
    replace_by_rename(&path, b"Item,Amount\nX1,900\nX2,50\n");
    let r = s.execute("file.save", json!({}));
    assert!(refused(&r), "{r:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "Item,Amount\nX1,900\nX2,50\n");
}

#[test]
fn a_script_is_refused_with_the_reason_and_may_overwrite_on_purpose() {
    let dir = folder("script");
    let path = dir.0.join("Budget.xlsx");
    let mut s = opened(&path);
    replace_by_rename(&path, &workbook_bytes(&dir.0, "newer.xlsx", "newer from elsewhere"));
    let e = s.execute_headless("file.save", json!({})).unwrap_err().to_string();
    assert!(e.contains("'Budget.xlsx' was changed on disk since it was opened here"), "{e}");
    assert!(e.contains("overwrite: true"), "the way to replace it anyway: {e}");
    assert_eq!(a1_on_disk(&path), text("newer from elsewhere"));
    s.execute_headless("file.save", json!({"overwrite": true})).unwrap();
    assert_eq!(a1_on_disk(&path), text("as opened"));
}

#[test]
fn a_workbook_with_no_file_of_its_own_is_saved_as_before() {
    let dir = folder("nofile");
    let path = dir.0.join("Budget.xlsx");
    std::fs::write(&path, workbook_bytes(&dir.0, "there.xlsx", "already there")).unwrap();
    // Opened from bytes: it has no file, and Save As over an existing file is the caller's choice.
    let mut s = Session::new();
    let b64 = gridcraft_engine::io::base64_encode(&workbook_bytes(&dir.0, "bytes.xlsx", "from bytes"));
    s.execute("file.open", json!({"name": "Budget.xlsx", "base64": b64})).unwrap();
    s.execute("file.saveAs", json!({"path": path})).unwrap();
    assert_eq!(a1_on_disk(&path), text("from bytes"));
}

/// Writes of the workbook's own file other than Save's own leave it the workbook's: the next Save
/// finds no change on disk. An export of the sheet as CSV over its own `.csv`, and a Save of a
/// workbook whose own file is named `.pdf` (which writes a PDF there).
#[test]
fn an_export_over_the_workbooks_own_file_is_no_reason_to_ask() {
    let dir = folder("export");
    let path = dir.0.join("data.csv");
    std::fs::write(&path, "Item,Amount\nX1,100\n").unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s.execute("cell.set", json!({"cell": "B2", "input": "150"})).unwrap();
    s.execute("file.exportCsv", json!({"path": path})).unwrap();
    s.execute("cell.set", json!({"cell": "B2", "input": "175"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("175"));

    let pdf = dir.0.join("Budget.pdf");
    std::fs::write(&pdf, workbook_bytes(&dir.0, "as-pdf.xlsx", "named pdf")).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"path": pdf})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    let r = s.execute("file.save", json!({}));
    assert!(!refused(&r), "{r:?}");
}

/// The workbook's own file is recognised however the path to it is spelled: with `.` or `..` in
/// it, or (on Unix) through a symbolic link. Changed on disk, a save to it is refused; with
/// `overwrite` it is written, and the next save goes through. Another file in the same folder is
/// no reason to ask.
#[test]
fn the_own_file_is_recognised_however_its_path_is_spelled() {
    let dir = folder("spelling");
    std::fs::create_dir_all(dir.0.join("sub")).unwrap();
    let mut spellings = vec![dir.0.join(".").join("Budget.xlsx"), dir.0.join("sub").join("..").join("Budget.xlsx")];
    #[cfg(unix)]
    {
        let link = dir.0.join("link.xlsx");
        std::os::unix::fs::symlink(dir.0.join("Budget.xlsx"), &link).unwrap();
        spellings.push(link);
    }
    for other in spellings {
        let path = dir.0.join("Budget.xlsx");
        let mut s = opened(&path);
        let newer = workbook_bytes(&dir.0, "newer.xlsx", "newer from elsewhere");
        replace_by_rename(&path, &newer);
        s.execute("cell.set", json!({"cell": "A1", "input": "mine"})).unwrap();
        let r = s.execute("file.saveAs", json!({"path": other}));
        assert!(refused(&r), "{}: {r:?}", other.display());
        assert_eq!(std::fs::read(&path).unwrap(), newer, "{}: the newer file is untouched", other.display());
        s.execute("file.saveAs", json!({"path": other, "overwrite": true})).unwrap();
        assert_eq!(a1_on_disk(&other), text("mine"), "{}", other.display());
        s.execute("cell.set", json!({"cell": "A1", "input": "mine again"})).unwrap();
        s.execute("file.save", json!({})).unwrap();
        assert_eq!(a1_on_disk(&other), text("mine again"), "{}", other.display());
    }
    // Another file in the same folder: saved as before, without a question.
    let path = dir.0.join("Budget.xlsx");
    let mut s = opened(&path);
    replace_by_rename(&path, &workbook_bytes(&dir.0, "newer.xlsx", "newer from elsewhere"));
    let other = dir.0.join(".").join("Other.xlsx");
    s.execute("file.saveAs", json!({"path": other})).unwrap();
    assert_eq!(a1_on_disk(&other), text("as opened"));
}
