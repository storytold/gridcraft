//! Recalculate workbook before saving (#3539): on by default, a manual-mode save recalculates the
//! workbook first, so the document and the file hold the same current results. Excel's behaviour
//! was measured through COM (`Application.CalculateBeforeSave`) with A1 = 1 and B1 `=A1*2`.

use std::io::Read as _;
use std::path::PathBuf;

use gridcraft_engine::{Session, core::CellRef, core::Value, io};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn value(s: &Session, a: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a).unwrap())
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

/// A manual-mode workbook whose B1 (`=A1*2`) still shows 2 after A1 changed to `a1`.
fn stale(a1: &str, recalculate_before_save: bool) -> Session {
    let mut s = session();
    set(&mut s, "A1", "1");
    set(&mut s, "B1", "=A1*2");
    s.execute("formulas.calculationOptions", json!({"mode": "manual", "recalculateBeforeSave": recalculate_before_save})).unwrap();
    set(&mut s, "A1", a1);
    assert_eq!(value(&s, "B1"), Value::Number(2.0), "manual mode leaves B1 as it was");
    s
}

fn dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::UNIX_EPOCH.elapsed().map(|d| d.as_nanos()).unwrap_or_default();
    let dir = std::env::temp_dir().join(format!("gridcraft-calc-on-save-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    z.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
    xml
}

fn calc_pr(bytes: &[u8]) -> String {
    let xml = part(bytes, "xl/workbook.xml");
    let start = xml.find("<calcPr").unwrap();
    let end = xml[start..].find("/>").unwrap();
    xml[start..start + end + 2].to_string()
}

/// B1's result as the file holds it (opening the file would recalculate it).
fn file_b1(bytes: &[u8]) -> Value {
    let xml = part(bytes, "xl/worksheets/sheet1.xml");
    let cell = xml.split("<c r=\"B1\"").nth(1).unwrap();
    let v = cell.split("<v>").nth(1).and_then(|r| r.split("</v>").next()).unwrap();
    Value::Number(v.parse().unwrap())
}

#[test]
fn a_manual_save_recalculates_first() {
    // Measured: the save recalculates, B1 is 10 afterwards and in the file, and `calcPr` has no
    // `calcOnSave`. GridCraft wrote B1 as 2.
    let dir = dir("manual-on");
    let path = dir.join("manual.xlsx");
    let mut s = stale("5", true);
    let undo = s.doc().unwrap().undo.len();
    s.execute("file.save", json!({"path": path})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Number(10.0), "the document holds the current result");
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(file_b1(&bytes), Value::Number(10.0), "and so does the file");
    assert!(calc_pr(&bytes).contains("calcMode=\"manual\""));
    assert!(!calc_pr(&bytes).contains("calcOnSave"), "{}", calc_pr(&bytes));
    // The recalculation is part of the save: no undo step of its own, and the workbook is clean.
    let d = s.doc().unwrap();
    assert_eq!(d.undo.len(), undo);
    assert!(!d.is_dirty());
    // Save As and a save in GridCraft JSON recalculate the same way.
    set(&mut s, "A1", "6");
    s.execute("file.saveAs", json!({"path": dir.join("copy.xlsx")})).unwrap();
    assert_eq!(file_b1(&std::fs::read(dir.join("copy.xlsx")).unwrap()), Value::Number(12.0));
    set(&mut s, "A1", "8");
    s.execute("file.save", json!({"path": dir.join("book.json")})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Number(16.0));
    let (json_wb, _) = io::open_bytes("book.json", &std::fs::read(dir.join("book.json")).unwrap()).unwrap();
    assert!(json_wb.calc.recalculate_before_save);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn with_the_setting_off_a_manual_save_keeps_the_last_results() {
    // Measured (A1 = 7): B1 stays 2 in the workbook and the file, and `calcOnSave="0"` is written.
    let dir = dir("manual-off");
    let path = dir.join("off.xlsx");
    let mut s = stale("7", false);
    s.execute("file.save", json!({"path": path})).unwrap();
    assert_eq!(value(&s, "B1"), Value::Number(2.0));
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(file_b1(&bytes), Value::Number(2.0));
    assert!(calc_pr(&bytes).contains("calcOnSave=\"0\""), "{}", calc_pr(&bytes));
    // The setting comes back with the file, and the next save keeps it.
    s.execute("file.open", json!({"name": "off.xlsx", "base64": io::base64_encode(&bytes)})).unwrap();
    let r = s.execute("formulas.calculationOptions", json!({})).unwrap();
    assert_eq!(r["recalculateBeforeSave"], false);
    assert_eq!(s.execute("file.options", json!({})).unwrap()["calculation"]["recalculateBeforeSave"], false);
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let again = io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
    assert!(calc_pr(&again).contains("calcOnSave=\"0\""), "{}", calc_pr(&again));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn with_the_setting_off_in_automatic_mode_calc_on_save_is_written_too() {
    let mut s = session();
    s.execute("formulas.calculationOptions", json!({"recalculateBeforeSave": false})).unwrap();
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let bytes = io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
    let pr = calc_pr(&bytes);
    assert!(pr.contains("calcOnSave=\"0\"") && !pr.contains("calcMode"), "{pr}");
}

#[test]
fn an_automatic_save_changes_nothing() {
    let dir = dir("automatic");
    let path = dir.join("auto.xlsx");
    let mut s = session();
    set(&mut s, "A1", "1");
    set(&mut s, "B1", "=A1*2");
    s.execute("file.save", json!({"path": path})).unwrap();
    set(&mut s, "A1", "5");
    assert_eq!(value(&s, "B1"), Value::Number(10.0));
    let (wb, undo) = (s.doc().unwrap().wb.clone(), s.doc().unwrap().undo.len());
    s.execute("file.save", json!({})).unwrap();
    s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let d = s.doc().unwrap();
    assert!(std::sync::Arc::ptr_eq(&d.wb, &wb), "the save leaves the workbook as it was");
    assert_eq!(d.undo.len(), undo);
    assert_eq!(file_b1(&std::fs::read(&path).unwrap()), Value::Number(10.0));
    assert!(!calc_pr(&std::fs::read(&path).unwrap()).contains("calcOnSave"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_copy_or_an_export_of_a_manual_workbook_holds_current_results() {
    // Measured: Excel's SaveCopyAs recalculated the open workbook too.
    let dir = dir("copies");
    let mut s = stale("11", true);
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    assert_eq!(file_b1(&io::base64_decode(r["base64"].as_str().unwrap()).unwrap()), Value::Number(22.0));
    assert_eq!(value(&s, "B1"), Value::Number(22.0), "the open workbook is recalculated as well");
    // A recalculation that changes nothing leaves a saved workbook clean.
    s.execute("file.save", json!({"path": dir.join("copy.xlsx")})).unwrap();
    let wb = s.doc().unwrap().wb.clone();
    s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().wb, &wb));
    assert!(!s.doc().unwrap().is_dirty());
    // CSV, through Save As and Export, and the web page export.
    set(&mut s, "A1", "3");
    s.execute("file.save", json!({"path": dir.join("values.csv")})).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("values.csv")).unwrap().trim_end(), "3,6");
    set(&mut s, "A1", "4");
    s.execute("file.exportCsv", json!({"path": dir.join("export.csv")})).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("export.csv")).unwrap().trim_end(), "4,8");
    set(&mut s, "A1", "9");
    s.execute("file.exportHtml", json!({"path": dir.join("page.html")})).unwrap();
    assert!(std::fs::read_to_string(dir.join("page.html")).unwrap().contains(">18<"));
    // With the setting off, the copy keeps the last results.
    s.execute("formulas.calculationOptions", json!({"recalculateBeforeSave": false})).unwrap();
    set(&mut s, "A1", "20");
    s.execute("file.exportCsv", json!({"path": dir.join("stale.csv")})).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("stale.csv")).unwrap().trim_end(), "20,18");
    assert_eq!(value(&s, "B1"), Value::Number(18.0));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_setting_is_on_by_default_and_undoes() {
    let mut s = session();
    let r = s.execute("formulas.calculationOptions", json!({})).unwrap();
    assert_eq!(r["recalculateBeforeSave"], true);
    assert_eq!(s.execute("file.options", json!({})).unwrap()["calculation"]["recalculateBeforeSave"], true);
    let r = s.execute("formulas.calculationOptions", json!({"recalculateBeforeSave": false})).unwrap();
    assert_eq!(r["recalculateBeforeSave"], false);
    assert!(!s.doc().unwrap().wb.calc.recalculate_before_save);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.calc.recalculate_before_save);
    // A GridCraft JSON workbook from before the setting existed opens with it on.
    let bytes = io::save_bytes(&s.doc().unwrap().wb, "book.json").unwrap();
    let mut text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"recalculate_before_save\": true"));
    text = text.replace(",\n    \"recalculate_before_save\": true", "");
    assert!(!text.contains("recalculate_before_save"));
    let (wb, _) = io::open_bytes("book.json", text.as_bytes()).unwrap();
    assert!(wb.calc.recalculate_before_save);
}
