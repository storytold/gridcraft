use gridcraft_engine::Session;
use serde_json::json;

#[test]
fn autofilter_ignores_surrounding_spaces_2911() {
    let mut s = Session::new();
    s.new_workbook();

    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [
                ["Name"],
                ["Card"],
                ["Card "],
                [" Card"],
                ["  Card  "],
                ["card"],
                ["CARD"],
                ["Cardx"],
                ["Ca rd"],
                [" "],
                [""]
            ]
        }),
    )
    .unwrap();

    s.execute("data.filter", json!({"range": "A1:A11"})).unwrap();

    for value in ["Card", " Card", "Card "] {
        let result = s.execute("data.filterBy", json!({"column": "A", "values": [value]})).unwrap();

        assert_eq!(result["hiddenRows"], json!(4), "{value:?}");

        let sh = s.doc().unwrap().wb.active().unwrap();

        for row in 1..=6 {
            assert!(!sh.is_row_hidden(row), "row {row} should be visible for {value:?}");
        }

        for row in 7..=10 {
            assert!(sh.is_row_hidden(row), "row {row} should be hidden for {value:?}");
        }
    }

    for value in ["Card", " Card", "Card "] {
        let result = s
            .execute(
                "data.filterBy",
                json!({
                    "column": "A",
                    "custom": {"op": "=", "value": value}
                }),
            )
            .unwrap();

        assert_eq!(result["hiddenRows"], json!(4), "{value:?}");

        let sh = s.doc().unwrap().wb.active().unwrap();

        for row in 1..=6 {
            assert!(!sh.is_row_hidden(row), "{value:?}: row {row}");
        }

        for row in 7..=10 {
            assert!(sh.is_row_hidden(row), "{value:?}: row {row}");
        }
    }
}

#[test]
fn filter_whitespace_wildcard_2911() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "parse":false,
               "values":[["Name"], [" Card "], ["Cardx"], ["Other"]]}),
    )
    .unwrap();
    s.execute("data.filter", json!({"range":"A1:A4"})).unwrap();

    let r = s.execute("data.filterBy", json!({"column":"A", "custom":{"op":"=", "value":"Card*"}})).unwrap();
    assert_eq!(r["hiddenRows"], json!(1));

    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1));
    assert!(!sh.is_row_hidden(2));
    assert!(sh.is_row_hidden(3));
}

#[test]
fn filter_whitespace_custom_blank_distinction_2911() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "parse":false,
               "values":[["Name"], [null], [""], [" "], ["   "], ["Card"]]}),
    )
    .unwrap();
    s.execute("data.filter", json!({"range":"A1:A6"})).unwrap();

    let r = s.execute("data.filterBy", json!({"column":"A", "custom":{"op":"=", "value":" "}})).unwrap();
    assert_eq!(r["hiddenRows"], json!(4));

    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.is_row_hidden(1));
    assert!(sh.is_row_hidden(2));
    assert!(!sh.is_row_hidden(3));
    assert!(!sh.is_row_hidden(4));
    assert!(sh.is_row_hidden(5));
}

#[test]
fn filter_whitespace_custom_not_equal_2911() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "parse":false,
               "values":[["Name"], [" Card "], ["Card"], ["Other"]]}),
    )
    .unwrap();
    s.execute("data.filter", json!({"range":"A1:A4"})).unwrap();

    let r = s.execute("data.filterBy", json!({"column":"A", "custom":{"op":"<>", "value":"Card"}})).unwrap();
    assert_eq!(r["hiddenRows"], json!(2));

    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.is_row_hidden(1));
    assert!(sh.is_row_hidden(2));
    assert!(!sh.is_row_hidden(3));
}
