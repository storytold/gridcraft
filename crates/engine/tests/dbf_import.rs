//! Original synthetic DBF fixtures, including safe import/save behavior through commands.
use gridcraft_core::{CellRef, Value};
use gridcraft_engine::{Session, io};
use serde_json::json;

fn table() -> Vec<u8> {
    let mut bytes = vec![0; 65];
    bytes[0] = 3;
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    bytes[8..10].copy_from_slice(&65u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&6u16.to_le_bytes());
    bytes[29] = 3;
    bytes[32..36].copy_from_slice(b"Name");
    bytes[43] = b'C';
    bytes[48] = 5;
    bytes[64] = 0x0d;
    bytes.extend_from_slice(b" Ada  \x1a");
    bytes
}

#[test]
fn dbf_opens_as_typed_sheet_and_exports_to_xlsx() {
    let (wb, warnings) = io::open_bytes("People.DBF", &table()).unwrap();
    assert!(warnings.is_empty());
    let sheet = wb.sheet(0).unwrap();
    assert_eq!(sheet.name, "People");
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::Text("Name".into()));
    assert_eq!(sheet.value(CellRef::new(1, 0)), Value::Text("Ada".into()));
    let (reopened, _) = io::open_bytes("People.xlsx", &io::save_bytes(&wb, "People.xlsx").unwrap()).unwrap();
    assert_eq!(reopened.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Text("Ada".into()));
    let (exported, _) = io::open_bytes("People.dbf", &io::save_bytes(&wb, "People.dbf").unwrap()).unwrap();
    assert_eq!(exported.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Text("Ada".into()));
}

#[test]
fn command_import_has_no_dbf_save_target_and_malformed_input_is_atomic() {
    let mut session = Session::new();
    session.execute("file.open", json!({"name":"People.dbf", "base64":io::base64_encode(&table())})).unwrap();
    let doc = session.active().unwrap();
    assert_eq!(doc.title, "People (imported).xlsx");
    assert!(doc.path.is_none());
    let count = session.documents().len();
    assert!(session.execute("file.open", json!({"name":"Broken.dbf", "base64":io::base64_encode(b"garbage")})).is_err());
    assert_eq!(session.documents().len(), count);
    assert_eq!(session.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Text("Ada".into()));
}

#[test]
fn xlsx_content_wins_over_dbf_extension() {
    let (wb, _) = io::open_bytes("People.dbf", &table()).unwrap();
    let xlsx = io::save_bytes(&wb, "People.xlsx").unwrap();
    let (wb, _) = io::open_bytes("misnamed.dbf", &xlsx).unwrap();
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Text("Ada".into()));
}

#[test]
fn ordinary_save_never_overwrites_the_source_dbf() {
    let path = std::env::temp_dir().join(format!("gridcraft-dbf-import-{}.dbf", std::process::id()));
    // create_new refuses to overwrite even an unexpected pre-existing test file.
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
    file.write_all(&table()).unwrap();
    drop(file);
    let mut session = Session::new();
    session.execute("file.open", json!({"path":path.to_str().unwrap()})).unwrap();
    assert!(session.active().unwrap().path.is_none());
    session.execute("cell.set", json!({"cell":"A2", "input":"Changed"})).unwrap();
    session.execute("file.save", json!({})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), table());
    std::fs::remove_file(path).unwrap();
}
