//! Parquet participates in the same file commands as other data imports/exports.
use gridcraft_engine::{
    Session,
    core::{CellRef, Value},
    io,
};
use serde_json::json;

fn table() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    for (cell, input) in [("A1", "Name"), ("B1", "Amount"), ("A2", "First"), ("B2", "=5*2")] {
        s.execute("cell.set", json!({"cell":cell,"input":input})).unwrap();
    }
    s
}
#[test]
fn command_roundtrip_exports_results_and_import_has_no_save_target() {
    let mut s = table();
    let out = s.execute("file.saveBytes", json!({"format":"parquet"})).unwrap();
    let bytes = io::base64_decode(out["base64"].as_str().unwrap()).unwrap();
    assert!(bytes.starts_with(b"PAR1"));
    s.execute("file.open", json!({"name":"Data.parquet","base64":out["base64"]})).unwrap();
    let d = s.active().unwrap();
    assert!(d.path.is_none());
    assert!(d.title.contains("imported"));
    let sh = d.wb.sheet(0).unwrap();
    assert_eq!(sh.name, "Data");
    assert_eq!(sh.value(CellRef::new(1, 1)), Value::Number(10.0));
    assert!(sh.cell(CellRef::new(1, 1)).unwrap().formula.is_none());
    s.execute("file.save", json!({})).unwrap();
    assert!(s.ui_requests.iter().any(|r| matches!(r,gridcraft_engine::UiRequest::Dialog(id,_) if id=="saveAs")));
}
#[test]
fn content_wins_over_misleading_extension_and_failed_open_keeps_document() {
    let mut s = table();
    let out = s.execute("file.saveBytes", json!({"format":"parquet"})).unwrap();
    s.execute("file.open", json!({"name":"Data.csv","base64":out["base64"]})).unwrap();
    assert!(s.active().unwrap().path.is_none());
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(1, 1)), Value::Number(10.0));
    let count = s.documents().len();
    let error = s.execute("file.open", json!({"name":"broken.parquet","base64":io::base64_encode(b"not parquet")})).unwrap_err();
    assert!(error.to_string().contains("Parquet"));
    assert_eq!(s.documents().len(), count);
    let xlsx = s.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
    assert!(s.execute("file.open", json!({"name":"really-xlsx.parquet","base64":xlsx["base64"]})).is_ok());
}
#[test]
fn export_selected_sheet_and_save_as_do_not_change_document_or_dirty_state() {
    let mut s = table();
    let dir = std::env::temp_dir().join(format!("gridcraft-parquet-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("table.parquet");
    let dirty = s.active().unwrap().is_dirty();
    s.execute("file.exportParquet", json!({"path":file,"sheet":0})).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    assert!(io::open_bytes("table.parquet", &bytes).is_ok());
    let mut opened = Session::new();
    opened.execute("file.open", json!({"path":file})).unwrap();
    assert_eq!(opened.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(1, 1)), Value::Number(10.0));
    assert!(opened.active().unwrap().path.is_none());
    s.execute("file.saveAs", json!({"path":file})).unwrap();
    assert_eq!(s.active().unwrap().is_dirty(), dirty);
    assert!(s.active().unwrap().path.is_none());
    std::fs::remove_file(file).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn owned_input_preserves_borrowed_format_detection_values_and_warnings() {
    let s = table();
    let wb = &s.active().unwrap().wb;
    for (name, bytes) in [
        ("table.parquet", io::save_bytes(wb, "table.parquet").unwrap()),
        ("misnamed.csv", io::save_bytes(wb, "table.parquet").unwrap()),
        ("table.xlsx", io::save_bytes(wb, "table.xlsx").unwrap()),
        ("table.csv", io::save_bytes(wb, "table.csv").unwrap()),
        ("really-xlsx.parquet", io::save_bytes(wb, "table.xlsx").unwrap()),
    ] {
        let borrowed = io::open_bytes(name, &bytes).unwrap();
        let owned = io::open_owned_bytes_with_password(name, bytes, None).unwrap();
        assert_eq!(borrowed, owned, "{name}");
    }
    for bytes in [b"PAR1broken".to_vec(), b"not parquet".to_vec()] {
        let borrowed = io::open_bytes("bad.parquet", &bytes).unwrap_err().to_string();
        let owned = io::open_owned_bytes_with_password("bad.parquet", bytes, None).unwrap_err().to_string();
        assert_eq!(borrowed, owned);
    }
}
