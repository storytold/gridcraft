//! The display region (set by the interface from its language) changes how values are shown and
//! edited, never what is stored, calculated or written to files.

use gridcraft_engine::core::{CellRef, Region};
use gridcraft_engine::display::{cell_text, edit_text, set_region};
use serde_json::json;

fn session() -> gridcraft_engine::Session {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    s
}

fn shown(s: &gridcraft_engine::Session, a1: &str) -> (String, String) {
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let c = CellRef::parse(a1).unwrap();
    (cell_text(&d.wb, sh, c), edit_text(&d.wb, sh, c))
}

/// Types `text` into `a1` the way the interface does in `region`.
fn type_in(s: &mut gridcraft_engine::Session, a1: &str, text: &str, region: Region) {
    let en = gridcraft_engine::core::parse::delocalize_input(text, region).unwrap_or_else(|| text.to_string());
    s.execute("cell.set", json!({"cell": a1, "input": en})).unwrap();
}

#[test]
fn values_show_and_edit_in_the_region_and_files_stay_en_us() {
    let mut s = session();
    set_region(Region::EsEs);
    type_in(&mut s, "A1", "1.234,50", Region::EsEs);
    type_in(&mut s, "A2", "5/11/2026", Region::EsEs);
    type_in(&mut s, "A3", "12,50 €", Region::EsEs);
    type_in(&mut s, "A4", "0,25", Region::EsEs);
    s.execute("cell.set", json!({"cell": "A5", "input": "=A1*2+0.5"})).unwrap();
    s.execute("cell.set", json!({"cell": "A6", "input": "=TEXT(A1,\"#,##0.00\")"})).unwrap();
    assert_eq!(shown(&s, "A1"), ("1.234,50".into(), "1234,5".into()));
    assert_eq!(shown(&s, "A2"), ("05/11/2026".into(), "05/11/2026".into()));
    assert_eq!(shown(&s, "A3"), ("12,50 €".into(), "12,5".into()));
    assert_eq!(shown(&s, "A4").0, "0,25");
    assert_eq!(shown(&s, "A5").0, "2469,5");
    // Formula results are the same everywhere.
    assert_eq!(shown(&s, "A6").0, "1,234.50");
    // Stored values and the API are en-US.
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(sh.input_text(CellRef::parse("A1").unwrap()), "1234.5");
    // Delimited files are en-US.
    let path = std::env::temp_dir().join(format!("gridcraft-region-{}.csv", std::process::id()));
    s.execute("file.saveAs", json!({"path": path.to_string_lossy()})).unwrap();
    let csv = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(csv.contains("\"1,234.50\"") && csv.contains("11/5/2026") && csv.contains("0.25"), "{csv}");

    set_region(Region::UkUa);
    assert_eq!(shown(&s, "A1").0, "1\u{a0}234,50");
    assert_eq!(shown(&s, "A2"), ("05.11.2026".into(), "05.11.2026".into()));
    type_in(&mut s, "B1", "31.12.2026", Region::UkUa);
    assert_eq!(shown(&s, "B1").0, "31.12.2026");

    set_region(Region::EnUs);
    assert_eq!(shown(&s, "A1"), ("1,234.50".into(), "1234.5".into()));
    assert_eq!(shown(&s, "A2").0, "11/5/2026");
    assert_eq!(shown(&s, "B1").0, "12/31/2026");
}

#[test]
fn the_region_belongs_to_the_thread_that_set_it() {
    set_region(Region::EsEs);
    let other = std::thread::spawn(gridcraft_engine::display::region).join().unwrap();
    assert_eq!(other, Region::EnUs);
    assert_eq!(gridcraft_engine::display::region(), Region::EsEs);
    set_region(Region::EnUs);
}

#[test]
fn the_api_and_html_export_stay_en_us() {
    let mut s = session();
    s.execute("cell.set", json!({"cell": "A1", "input": "1234.5"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "A1", "code": "#,##0.00"})).unwrap();
    set_region(Region::EsEs);
    assert_eq!(shown(&s, "A1").0, "1.234,50");
    let got = s.execute("cell.get", json!({"cell": "A1"})).unwrap();
    assert_eq!(got["text"], "1,234.50", "{got}");
    let html = gridcraft_engine::io::to_html(&s.doc().unwrap().wb, 0).unwrap();
    assert!(html.contains("1,234.50") && !html.contains("1.234,50"), "{html}");
    // The display region is back afterwards.
    assert_eq!(shown(&s, "A1").0, "1.234,50");
    set_region(Region::EnUs);
}

#[test]
fn a_numeric_table_header_names_its_column_in_en_us() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Item", 2026.5], ["a", 1]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
    set_region(Region::UkUa);
    s.execute("cell.set", json!({"cell": "B1", "input": "2027.5"})).unwrap();
    let d = s.doc().unwrap();
    let t = &d.wb.active().unwrap().tables[0];
    assert_eq!(t.columns[1].name, "2027.5");
    set_region(Region::EnUs);
}
