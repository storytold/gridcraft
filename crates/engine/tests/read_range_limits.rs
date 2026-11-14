//! Range paging validates the full integer before any narrowing conversion.
use gridcraft_engine::Session;
use serde_json::json;

#[test]
/// Check oversized integer limits cannot narrow to an absent limit or bypass pagination bounds.
fn read_range_rejects_oversized_limits_instead_of_silently_disabling_paging() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "7"})).unwrap();
    for value in [1_000_001_u64, u64::from(u32::MAX) + 1, u64::MAX] {
        let error = s.execute("sheet.read", json!({"range": "A1:A2", "maxCells": value})).unwrap_err();
        assert!(error.to_string().contains("maxCells must be a positive integer"), "{value}: {error}");
    }
    let page = s.execute("sheet.read", json!({"range": "A1:A2", "maxCells": 1})).unwrap();
    assert_eq!(page["range"], "A1");
    assert_eq!(page["next"], "Sheet1!A2");
}
