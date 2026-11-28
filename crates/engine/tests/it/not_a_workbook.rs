//! A file that is no workbook and no text is not opened as cells (#4299): a PDF, under any name and
//! by path or by its bytes, and other bytes that are no text, are refused with words, and the file
//! stays as it was. Text, under any name, opens as before; so does an empty file.
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::json;

/// A folder of its own under the system's temporary folder, removed afterwards.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn folder(tag: &str) -> Folder {
    let dir = std::env::temp_dir().join(format!("gridcraft-not-a-workbook-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Folder(dir)
}

/// A small one-page PDF, written in code: its text part is plain ASCII, so it reads as text.
fn pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>",
        "<< /Length 26 >>\nstream\nBT /F1 12 Tf (Hello) Tj ET\nendstream",
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for at in offsets {
        out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

/// The start of a PNG picture: its signature and a header, then pixels.
fn picture() -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10\x08\x06\0\0\0".to_vec();
    out.extend((0..200u8).map(|b| b.wrapping_mul(37)));
    out
}

/// Opens `bytes` as the file `name` in `dir`, by path and by its bytes: both refused, with `words`,
/// and the file as it was.
fn refused(dir: &std::path::Path, name: &str, bytes: &[u8], words: &str) {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    s.new_workbook();
    let e = s.execute("file.open", json!({"path": path})).unwrap_err().to_string();
    assert!(e.contains(words) && e.contains(name), "{name}: {e}");
    let b64 = gridcraft_engine::io::base64_encode(bytes);
    let e = s.execute("file.open", json!({"name": name, "base64": b64})).unwrap_err().to_string();
    assert!(e.contains(words), "{name} as bytes: {e}");
    assert_eq!(s.documents().len(), 1, "{name}: nothing opened");
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "{name}: the file as it was");
}

fn opened_a1(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Value {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path})).unwrap();
    s.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0))
}

#[test]
fn a_pdf_is_refused_under_any_name() {
    let dir = folder("pdf");
    for name in
        ["scan.pdf", "scan.PDF", "statement.csv", "statement.txt", "statement.xlsx", "statement.xls", "statement.ods", "statement", "statement.dat"]
    {
        refused(&dir.0, name, &pdf(), gridcraft_engine::io::NOT_A_SHEET_PDF);
    }
}

#[test]
fn bytes_that_are_no_text_are_refused() {
    let dir = folder("binary");
    // Text in UTF-16 without its byte order mark: every other byte is a NUL.
    let utf16: Vec<u8> = "Item,Amount\r\nX1,100\r\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
    for (name, bytes) in [("photo.png", picture()), ("photo.csv", picture()), ("photo", picture()), ("items.txt", utf16)] {
        refused(&dir.0, name, &bytes, gridcraft_engine::io::NOT_A_SHEET);
    }
}

#[test]
fn text_under_any_name_and_an_empty_file_open_as_before() {
    let dir = folder("text");
    let text = |s: &str| Value::Text(s.into());
    // A `.txt` is split at tabs, so its comma-separated line stays one cell; the others at commas.
    for (name, a1) in [("items.csv", "Item"), ("items.txt", "Item,Amount"), ("items.log", "Item"), ("items", "Item")] {
        assert_eq!(opened_a1(&dir.0, name, b"Item,Amount\r\nX1,100\r\n"), text(a1), "{name}");
    }
    // Text in a one-byte encoding (Windows-1252: 0xE9 is "é") has bytes over 0x7F but no control
    // characters, so it is text.
    assert_eq!(opened_a1(&dir.0, "cafe.csv", b"Caf\xE9,100\r\n"), text("Café"));
    // With a byte order mark, UTF-16 is text.
    let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("Item,Amount\r\nX1,100\r\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
    assert_eq!(opened_a1(&dir.0, "items16.csv", &utf16), text("Item"));
    // An empty file is an empty sheet.
    assert_eq!(opened_a1(&dir.0, "empty.csv", b""), Value::Empty);
}
