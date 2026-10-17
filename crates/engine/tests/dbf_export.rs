use gridcraft_core::{CellRef, Value};
use gridcraft_engine::{Session, io};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell":"A1", "input":"Amount"})).unwrap();
    s.execute("cell.set", json!({"cell":"A2", "input":"=6*7"})).unwrap();
    s
}

#[test]
fn dbf_save_bytes_exports_formula_results_only() {
    let mut s = session();
    let result = s.execute("file.saveBytes", json!({"format":"dbf"})).unwrap();
    assert!(result["name"].as_str().unwrap().ends_with(".dbf"));
    let bytes = io::base64_decode(result["base64"].as_str().unwrap()).unwrap();
    let (wb, _) = io::open_bytes("Amount.dbf", &bytes).unwrap();
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Number(42.0));
    assert!(wb.sheet(0).unwrap().cells.get(CellRef::new(1, 0)).unwrap().formula.is_none());
}

#[test]
fn dbf_save_as_keeps_the_workbooks_file_and_dirty_state() {
    let dir = std::env::temp_dir().join(format!("gridcraft-dbf-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let original = dir.join("Original.xlsx");
    let target = dir.join("Export.dbf");
    let mut s = session();
    s.execute("file.saveAs", json!({"path":original.to_str().unwrap()})).unwrap();
    let original_bytes = std::fs::read(&original).unwrap();
    s.execute("cell.set", json!({"cell":"A2", "input":"=7*7"})).unwrap();
    s.execute("file.saveAs", json!({"path":target.to_str().unwrap()})).unwrap();
    let doc = s.active().unwrap();
    assert_eq!(doc.path.as_deref(), original.to_str());
    assert!(doc.is_dirty());
    assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
    let (wb, _) = io::open_bytes("Export.dbf", &std::fs::read(&target).unwrap()).unwrap();
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Number(49.0));
    std::fs::remove_file(original).unwrap();
    std::fs::remove_file(target).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn invalid_dbf_export_neither_creates_nor_replaces_a_file() {
    let path = std::env::temp_dir().join(format!("gridcraft-dbf-refused-{}.dbf", std::process::id()));
    let mut s = session();
    s.execute("cell.set", json!({"cell":"A1", "input":"Header is too long"})).unwrap();
    assert!(!path.exists());
    assert!(s.execute("file.exportDbf", json!({"path":path.to_str().unwrap()})).is_err());
    assert!(!path.exists());
    std::fs::write(&path, b"existing target").unwrap();
    assert!(s.execute("file.exportDbf", json!({"path":path.to_str().unwrap()})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing target");
    assert!(s.active().unwrap().is_dirty());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn dbf_export_classifies_dates_through_the_engine_number_format_parser() {
    use gridcraft_core::DateSystem;
    use gridcraft_model::{NumFmt, StyleId, Workbook};
    for (format, value, expected) in [
        ("yyyy-mm-dd", 0.0, Some(b'D')),
        ("dd mmm yyyy", 0.0, Some(b'D')),
        ("0.00", 0.0, Some(b'N')),
        (r#"0 "days""#, 0.0, Some(b'N')),
        ("hh:mm", 0.0, None),
        ("[h]:mm", 0.0, None),
        ("[<0]yyyy-mm-dd;0", 25.0, None),
        ("yyyy-mm-dd hh:mm", 0.5, None),
    ] {
        let mut wb = Workbook::new();
        wb.date_system = DateSystem::D1904;
        let style = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(format));
        let sheet = wb.sheet_mut(0).unwrap();
        sheet.set_value(CellRef::new(0, 0), Value::Text("Value".into()));
        sheet.set_value(CellRef::new(1, 0), Value::Number(value));
        sheet.set_style(CellRef::new(1, 0), style);
        let result = io::save_bytes(&wb, "Date.dbf");
        if let Some(kind) = expected {
            let bytes = result.unwrap();
            assert_eq!(bytes[43], kind, "{format}");
            let (loaded, _) = io::open_bytes("Date.dbf", &bytes).unwrap();
            let expected_value = if kind == b'D' { 1462.0 } else { value };
            assert_eq!(loaded.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Number(expected_value), "{format}");
        } else {
            assert!(result.is_err(), "{format}");
        }
    }
}
