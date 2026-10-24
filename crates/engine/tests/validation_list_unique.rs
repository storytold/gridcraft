//! A List validation sourced from a range shows unique values only (#1932).

use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use serde_json::json;

fn session_with_fruit() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1:A5", "values": [["Apple"], ["Banana"], ["Apple"], ["Apple"], ["Banana"]]})).unwrap();
    s.execute("data.validation", json!({"range": "C1", "type": "list", "formula1": "A1:A5"})).unwrap();
    s
}

fn dropdown(s: &Session) -> Vec<String> {
    let d = s.doc().unwrap();
    let wb = &d.wb;
    let sh = wb.active().unwrap();
    let dv = sh.validations.iter().find(|v| v.ranges.iter().any(|r| r.contains(CellRef::parse("C1").unwrap()))).unwrap();
    gridcraft_engine::cmd::data::list_items(wb, wb.active_sheet, dv)
}

#[test]
fn range_sourced_list_shows_unique_values() {
    let s = session_with_fruit();
    assert_eq!(dropdown(&s), vec!["Apple".to_string(), "Banana".to_string()]);
}

#[test]
fn literal_list_keeps_duplicates_as_typed() {
    // Control: a typed "Apple,Banana,Apple" list keeps the user's entries verbatim.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("data.validation", json!({"range": "C1", "type": "list", "formula1": "\"Apple,Banana,Apple\""})).unwrap();
    let d = s.doc().unwrap();
    let wb = &d.wb;
    let sh = wb.active().unwrap();
    let dv = sh.validations.iter().find(|v| v.ranges.iter().any(|r| r.contains(CellRef::parse("C1").unwrap()))).unwrap();
    let items = gridcraft_engine::cmd::data::list_items(wb, wb.active_sheet, dv);
    assert_eq!(items, vec!["Apple".to_string(), "Banana".to_string(), "Apple".to_string()]);
}

#[test]
fn list_validation_still_accepts_every_source_value() {
    // Membership is unaffected by dedup: typing either fruit validates.
    let s = session_with_fruit();
    for fruit in ["Apple", "Banana"] {
        let d = s.doc().unwrap();
        let bad = gridcraft_engine::cmd::data::check_validation(&d.wb, d.wb.active_sheet, CellRef::parse("C1").unwrap(), fruit);
        assert!(bad.is_none(), "{fruit} must validate");
    }
    let d = s.doc().unwrap();
    let bad = gridcraft_engine::cmd::data::check_validation(&d.wb, d.wb.active_sheet, CellRef::parse("C1").unwrap(), "Cherry");
    assert!(bad.is_some(), "Cherry must be rejected");
}
