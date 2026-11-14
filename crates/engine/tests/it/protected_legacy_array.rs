use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::json;

#[test]
fn protected_legacy_array_refuses_locked_member_cells() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[10], [20], [30]]})).unwrap();
    s.execute("home.lockCell", json!({"range": "C1", "on": false})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    s.execute("selection.set", json!({"range": "C1:C3"})).unwrap();

    assert!(s.execute("cell.set", json!({"cell": "C1", "input": "=A1:A3*2", "array": true})).is_err());

    let sheet = s.doc().unwrap().wb.active().unwrap();
    for address in ["C1", "C2", "C3"] {
        assert_eq!(sheet.value(CellRef::parse(address).unwrap()), Value::Empty, "{address}");
    }
}
