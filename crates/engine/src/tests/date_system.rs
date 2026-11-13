//! Use 1904 date system (`formulas.calculationOptions` `date1904`, #3538). Behaviour observed in
//! Excel through COM (`Workbook.Date1904`) with synthetic values: stored numbers are kept, so
//! 46023 (1 January 2026) shows 2 January 2030; formulas are recalculated in the new system; a
//! date typed afterwards is stored in it; switching back restores every result; other open
//! workbooks keep their own system.

use gridcraft_core::{CellRef, DateSystem, Value};
use serde_json::json;

use super::{reopen, s, v};
use crate::Session;

fn num(s: &Session, a: &str) -> f64 {
    v(s, a).as_f64().unwrap_or(f64::NAN)
}

fn shown(s: &Session, a: &str) -> String {
    let d = s.doc().unwrap();
    crate::display::cell_text(&d.wb, d.wb.active().unwrap(), CellRef::parse(a).unwrap())
}

fn system(s: &Session) -> DateSystem {
    s.doc().unwrap().wb.date_system
}

/// The measured cells: a date constant, and formulas that build, read and subtract dates.
fn measured_book() -> Session {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "46023"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "A1", "code": "yyyy-mm-dd"})).unwrap();
    for (cell, input) in
        [("A2", "=DATE(2026,1,1)"), ("A3", "=DATEVALUE(\"2026-01-01\")"), ("A4", "=YEAR(A1)"), ("A5", "=A1-DATE(2025,12,31)"), ("A6", "=A1+1")]
    {
        s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }
    s
}

fn measured(s: &Session) -> [f64; 6] {
    ["A1", "A2", "A3", "A4", "A5", "A6"].map(|a| num(s, a))
}

const IN_1900: [f64; 6] = [46023.0, 46023.0, 46023.0, 2026.0, 1.0, 46024.0];
const IN_1904: [f64; 6] = [46023.0, 44561.0, 44561.0, 2030.0, 1463.0, 46024.0];

#[test]
fn switching_keeps_stored_numbers_and_recalculates_formulas() {
    let mut s = measured_book();
    assert_eq!(system(&s), DateSystem::D1900);
    assert_eq!(measured(&s), IN_1900);
    assert_eq!(shown(&s, "A1"), "2026-01-01");
    let r = s.execute("formulas.calculationOptions", json!({"date1904": true})).unwrap();
    assert_eq!(r["date1904"], true);
    assert_eq!(system(&s), DateSystem::D1904);
    assert_eq!(measured(&s), IN_1904);
    // The constant is the same number, so it shows a date four years and a day later.
    assert_eq!(shown(&s, "A1"), "2030-01-02");
    let r = s.execute("file.options", json!({})).unwrap();
    assert_eq!(r["calculation"]["date1904"], true);
    // A date typed now is stored in the 1904 system.
    s.execute("cell.set", json!({"cell": "B1", "input": "2026-01-01"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Number(44561.0));
    assert_eq!(shown(&s, "B1"), "2026-01-01");
    // Switched off again: every cell and result as before the switch; the typed date keeps its
    // number too.
    let r = s.execute("formulas.calculationOptions", json!({"date1904": false})).unwrap();
    assert_eq!(r["date1904"], false);
    assert_eq!(measured(&s), IN_1900);
    assert_eq!(shown(&s, "A1"), "2026-01-01");
    assert_eq!(v(&s, "B1"), Value::Number(44561.0));
}

#[test]
fn switching_is_one_undo_step_and_marks_the_workbook_changed() {
    let mut s = measured_book();
    reopen(&mut s);
    assert!(!s.doc().unwrap().is_dirty());
    s.execute("formulas.calculationOptions", json!({"date1904": true})).unwrap();
    assert!(s.doc().unwrap().is_dirty());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(system(&s), DateSystem::D1900);
    assert_eq!(measured(&s), IN_1900);
    assert!(!s.doc().unwrap().is_dirty());
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(system(&s), DateSystem::D1904);
    assert_eq!(measured(&s), IN_1904);
}

#[test]
fn switching_recalculates_in_manual_mode_too() {
    let mut s = measured_book();
    s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    s.execute("formulas.calculationOptions", json!({"date1904": true})).unwrap();
    assert_eq!(measured(&s), IN_1904);
    s.execute("formulas.calculationOptions", json!({"date1904": false})).unwrap();
    assert_eq!(measured(&s), IN_1900);
    assert_eq!(s.doc().unwrap().wb.calc.mode, gridcraft_model::CalcMode::Manual);
}

#[test]
fn the_date_system_is_saved_and_read_back() {
    let mut s = measured_book();
    s.execute("formulas.calculationOptions", json!({"date1904": true})).unwrap();
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let bytes = crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut book = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("xl/workbook.xml").unwrap(), &mut book).unwrap();
    assert!(book.contains("date1904=\"1\""), "{book}");
    reopen(&mut s);
    assert_eq!(system(&s), DateSystem::D1904);
    assert_eq!(measured(&s), IN_1904);
    assert_eq!(s.execute("formulas.calculationOptions", json!({})).unwrap()["date1904"], true);
    // Off again, the file says nothing of 1904.
    s.execute("formulas.calculationOptions", json!({"date1904": false})).unwrap();
    reopen(&mut s);
    assert_eq!(system(&s), DateSystem::D1900);
    assert_eq!(measured(&s), IN_1900);
}

#[test]
fn other_open_workbooks_keep_their_own_system() {
    let mut s = measured_book();
    let first = s.doc().unwrap().uid;
    s.execute("file.new", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=DATE(2026,1,1)"})).unwrap();
    s.execute("formulas.calculationOptions", json!({"date1904": true})).unwrap();
    assert_eq!(num(&s, "A1"), 44561.0);
    s.execute("window.activate", json!({"doc": first})).unwrap();
    assert_eq!(system(&s), DateSystem::D1900);
    assert_eq!(measured(&s), IN_1900);
    assert_eq!(s.execute("file.options", json!({})).unwrap()["calculation"]["date1904"], false);
}

#[test]
fn empty_params_report_the_date_system_and_change_nothing() {
    let mut s = measured_book();
    let r = s.execute("formulas.calculationOptions", json!({})).unwrap();
    assert_eq!(r["date1904"], false);
    assert_eq!(s.execute("file.options", json!({})).unwrap()["calculation"]["date1904"], false);
    // A value that isn't a boolean is ignored, like the other switches.
    s.execute("formulas.calculationOptions", json!({"date1904": "yes"})).unwrap();
    s.execute("formulas.calculationOptions", json!({"date1904": 1})).unwrap();
    assert_eq!(system(&s), DateSystem::D1900);
    assert_eq!(measured(&s), IN_1900);
}
