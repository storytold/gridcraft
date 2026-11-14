use super::{s, v};
use gridcraft_core::Value;
use serde_json::json;

#[test]
fn consolidate_numeric_label_identity_ignores_number_format() {
    for (second, first_fmt, second_fmt, expected_rows) in
        [(1.4, "0", "0", 2), (1.1, "0.0", "0.00", 1), (1.4, ";;;", ";;;", 2), (1.1, ";;;", ";;;", 1), (1.4, "General", "General", 2)]
    {
        let mut s = s();
        s.execute("range.setValues", json!({"range":"A3","values":[[1.1,10],[second,20]]})).unwrap();
        s.execute("home.numberFormat", json!({"range":"A3","code":first_fmt})).unwrap();
        s.execute("home.numberFormat", json!({"range":"A4","code":second_fmt})).unwrap();
        let result = s.execute("data.consolidate", json!({"sources":["A3:B3","A4:B4"],"destination":"D1","leftColumn":true})).unwrap();
        assert_eq!(result["rows"], expected_rows, "{first_fmt}, {second_fmt}");
        assert_eq!(v(&s, "D1"), Value::number(1.1));
        assert_eq!(v(&s, "E1"), Value::number(if expected_rows == 1 { 30.0 } else { 10.0 }));
        if expected_rows == 2 {
            assert_eq!(v(&s, "D2"), Value::number(second));
            assert_eq!(v(&s, "E2"), Value::number(20.0));
        }
        assert_eq!(v(&s, "A3"), Value::number(1.1));
        assert_eq!(v(&s, "A4"), Value::number(second));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(v(&s, "D1"), Value::Empty);
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(v(&s, "D1"), Value::number(1.1));
    }
}

#[test]
fn consolidate_numeric_column_labels_keep_values_and_totals() {
    for (second, second_fmt, columns) in [(1.4, "0", 2), (1.1, "0.00", 1), (1.4, ";;;", 2)] {
        let mut s = s();
        s.execute("range.setValues", json!({"range":"A1","values":[[1.1],[10]]})).unwrap();
        s.execute("range.setValues", json!({"range":"D1","values":[[second],[20]]})).unwrap();
        s.execute("home.numberFormat", json!({"range":"A1","code":if second_fmt == ";;;" { ";;;" } else { "0" }})).unwrap();
        s.execute("home.numberFormat", json!({"range":"D1","code":second_fmt})).unwrap();
        let result = s.execute("data.consolidate", json!({"sources":["A1:A2","D1:D2"],"destination":"H1","topRow":true})).unwrap();
        assert_eq!(result["columns"], columns);
        assert_eq!(v(&s, "H1"), Value::number(1.1));
        assert_eq!(v(&s, "H2"), Value::number(if columns == 1 { 30.0 } else { 10.0 }));
        if columns == 2 {
            assert_eq!(v(&s, "I1"), Value::number(second));
            assert_eq!(v(&s, "I2"), Value::number(20.0));
        }
    }
}

#[test]
fn consolidate_numeric_labels_on_both_axes_do_not_collapse() {
    let mut s = s();
    s.execute("range.setValues", json!({"range":"A1","values":[[null,2.1,2.4],[1.1,10,20],[1.4,30,40]]})).unwrap();
    s.execute("home.numberFormat", json!({"range":"A2:A3","code":"0"})).unwrap();
    s.execute("home.numberFormat", json!({"range":"B1:C1","code":"0"})).unwrap();
    let result = s.execute("data.consolidate", json!({"sources":["A1:C3"],"destination":"H1","topRow":true,"leftColumn":true})).unwrap();
    assert_eq!(result["rows"], 2);
    assert_eq!(result["columns"], 2);
    for (cell, expected) in [("I1", 2.1), ("J1", 2.4), ("H2", 1.1), ("H3", 1.4), ("I2", 10.0), ("J2", 20.0), ("I3", 30.0), ("J3", 40.0)] {
        assert_eq!(v(&s, cell), Value::number(expected), "{cell}");
    }
}

#[test]
fn advanced_filter_unique_numeric_identity_ignores_number_format() {
    for copy in [false, true] {
        for (second, first_fmt, second_fmt, count) in [(1.4, "0", "0", 2), (1.1, "0.0", "0.00", 1), (1.4, ";;;", ";;;", 2)] {
            let mut s = s();
            s.execute("range.setValues", json!({"range":"A1","values":[["Amount"],[1.1],[second]]})).unwrap();
            s.execute("home.numberFormat", json!({"range":"A2","code":first_fmt})).unwrap();
            s.execute("home.numberFormat", json!({"range":"A3","code":second_fmt})).unwrap();
            let mut params = json!({"range":"A1:A3","unique":true});
            if copy {
                params["copyTo"] = json!("E1");
            }
            let result = s.execute("data.advancedFilter", params).unwrap();
            assert_eq!(result["matched"], count, "copy={copy}, {first_fmt}, {second_fmt}");
            if copy {
                assert_eq!(v(&s, "E2"), Value::number(1.1));
                assert_eq!(v(&s, "E3"), if count == 2 { Value::number(second) } else { Value::Empty });
            } else {
                assert_eq!(s.doc().unwrap().wb.active().unwrap().is_row_hidden(2), count == 1);
            }
        }
    }
}

#[test]
fn advanced_filter_blank_criteria_test_values_not_display() {
    for (criterion, expected) in [("=", vec![101]), ("<>", vec![102, 103, 104, 105, 106, 107])] {
        let mut s = s();
        s.execute(
            "range.setValues",
            json!({"range":"A1","values":[["Label","ID"],[null,101],["=\"\"",102],[42,103],["hidden",104],[true,105],[0,106],["visible",107]]}),
        )
        .unwrap();
        s.execute("home.numberFormat", json!({"range":"A4:A6","code":";;;"})).unwrap();
        s.execute("cell.set", json!({"cell":"D1","input":"Label"})).unwrap();
        s.execute("cell.set", json!({"cell":"D2","input":format!("'{criterion}")})).unwrap();
        let result = s.execute("data.advancedFilter", json!({"range":"A1:B8","criteria":"D1:D2","copyTo":"F1"})).unwrap();
        assert_eq!(result["matched"], expected.len());
        for (i, id) in expected.iter().enumerate() {
            assert_eq!(v(&s, &format!("G{}", i + 2)), Value::number(*id as f64));
        }
        assert_eq!(v(&s, "A3"), Value::from(""));
        assert_eq!(v(&s, "A4"), Value::number(42.0));
    }
}

#[test]
fn data_keys_keep_numbers_text_and_booleans_distinct() {
    for (first, second) in [("42", "'42"), ("TRUE", "'TRUE"), ("TRUE", "1")] {
        let mut s = s();
        s.execute("cell.set", json!({"cell":"A1","input":"Label"})).unwrap();
        s.execute("cell.set", json!({"cell":"A2","input":first})).unwrap();
        s.execute("cell.set", json!({"cell":"A3","input":second})).unwrap();
        let result = s.execute("data.advancedFilter", json!({"range":"A1:A3","copyTo":"F1","unique":true})).unwrap();
        assert_eq!(result["matched"], 2, "{first} / {second}");
        s.execute("range.setValues", json!({"range":"B2","values":[[10],[20]]})).unwrap();
        let result = s.execute("data.consolidate", json!({"sources":["A2:B2","A3:B3"],"destination":"H1","leftColumn":true})).unwrap();
        assert_eq!(result["rows"], 2, "{first} / {second}");
        assert_eq!(v(&s, "H1"), v(&s, "A2"));
        assert_eq!(v(&s, "H2"), v(&s, "A3"));
    }
}

#[test]
fn data_keys_preserve_case_insensitive_text_and_zero_equality() {
    for (first, second) in [("Alpha", "alpha"), ("0", "-0")] {
        let mut s = s();
        s.execute("cell.set", json!({"cell":"A1","input":"Label"})).unwrap();
        s.execute("cell.set", json!({"cell":"A2","input":first})).unwrap();
        s.execute("cell.set", json!({"cell":"A3","input":second})).unwrap();
        let result = s.execute("data.advancedFilter", json!({"range":"A1:A3","copyTo":"F1","unique":true})).unwrap();
        assert_eq!(result["matched"], 1);
        s.execute("range.setValues", json!({"range":"B2","values":[[10],[20]]})).unwrap();
        let result = s.execute("data.consolidate", json!({"sources":["A2:B2","A3:B3"],"destination":"H1","leftColumn":true})).unwrap();
        assert_eq!(result["rows"], 1);
        assert_eq!(v(&s, "I1"), Value::number(30.0));
    }
}
