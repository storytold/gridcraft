//! Bulk output and what-if inputs must respect generated dynamic-array cells (#3919–#3942).

use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

fn assert_spill(s: &Session, anchor: &str, sum: f64) {
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.spill_ranges.contains_key(&CellRef::parse(anchor).unwrap()), "{anchor} must successfully spill before the command");
    assert_eq!(value(s, "H1"), Value::Number(sum));
}

fn refuses_unchanged(s: &mut Session, command: &str, params: Json) {
    let before = s.doc().unwrap().clone();
    let error = s.execute(command, params).expect_err("a generated spill destination must be refused");
    assert!(error.to_string().contains("part of an array"), "{command}: {error}");
    let after = s.doc().unwrap();
    assert!(Arc::ptr_eq(&before.wb, &after.wb), "{command} changed the workbook on refusal");
    assert_eq!(after.undo.len(), before.undo.len(), "{command} changed undo history");
    assert_eq!(after.redo.len(), before.redo.len(), "{command} changed redo history");
    assert_eq!(after.selection, before.selection, "{command} changed the selection");
}

#[test]
fn text_to_columns_refuses_spill_members_including_empty_fields() {
    for (anchor, formula, sum) in [("E1", "=SEQUENCE(3)", 6.0), ("C2", "=SEQUENCE(1,3)", 6.0), ("C1", "=SEQUENCE(3,3)", 45.0)] {
        for source in ["'11,22,33", "'11,,33"] {
            let mut s = session();
            set(&mut s, "A1", source);
            set(&mut s, anchor, formula);
            set(&mut s, "H1", &format!("=SUM({anchor}#)"));
            assert_spill(&s, anchor, sum);
            let sum = value(&s, "H1");
            refuses_unchanged(&mut s, "data.textToColumns", json!({"range": "A1", "delimiters": [","], "destination": "D2"}));
            assert_eq!(value(&s, "H1"), sum);
        }
    }
}

#[test]
fn text_to_columns_requires_full_spill_coverage_even_when_output_includes_anchor() {
    let mut s = session();
    set(&mut s, "A1", "'11,22,33");
    set(&mut s, "D2", "=SEQUENCE(3)");
    set(&mut s, "H1", "=SUM(D2#)");
    assert_spill(&s, "D2", 6.0);
    refuses_unchanged(&mut s, "data.textToColumns", json!({"range": "A1", "delimiters": [","], "destination": "D2"}));
}

#[test]
fn text_to_columns_checks_each_rows_actual_fields_instead_of_a_bounding_box() {
    let mut s = session();
    set(&mut s, "A1", "'11,22");
    set(&mut s, "A2", "'33");
    set(&mut s, "D2", "=SEQUENCE(2,2)");
    set(&mut s, "H1", "=SUM(D2#)");
    assert_spill(&s, "D2", 10.0);
    refuses_unchanged(&mut s, "data.textToColumns", json!({"range": "A1:A2", "delimiters": [","], "destination": "D2"}));
}

#[test]
fn advanced_filter_refuses_spill_members_atomically() {
    for formula in ["=SEQUENCE(1,3)", "=SEQUENCE(3,2)"] {
        let mut s = session();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Amount", "ID"], [11, 101], [22, 102]]})).unwrap();
        set(&mut s, "C3", formula);
        set(&mut s, "H1", "=SUM(C3#)");
        assert_spill(&s, "C3", if formula == "=SEQUENCE(1,3)" { 6.0 } else { 21.0 });
        refuses_unchanged(&mut s, "data.advancedFilter", json!({"range": "A1:B3", "copyTo": "D2", "unique": false}));
        // Engine failures are atomic, including headers (Excel writes headers before refusing).
        assert_eq!(value(&s, "D2"), Value::Empty);
    }
}

fn table_setup(s: &mut Session) {
    s.execute("range.setValues", json!({"range": "A1", "values": [[10, 20]]})).unwrap();
    s.execute("range.setValues", json!({"range": "C3", "values": [["=$A$1+10*$B$1", 1, 2], [3, 777, null], [4, 999, null]]})).unwrap();
}

#[test]
fn data_table_refuses_partial_spills_with_or_without_the_anchor() {
    for (anchor, formula, range, sum) in
        [("E2", "=SEQUENCE(4)", "E2:E5", 10.0), ("B4", "=SEQUENCE(1,4)", "B4:E4", 10.0), ("D4", "=SEQUENCE(3,2)", "D4:E6", 21.0)]
    {
        let mut s = session();
        table_setup(&mut s);
        s.execute("edit.clearContents", json!({"range": range})).unwrap();
        set(&mut s, anchor, formula);
        set(&mut s, "H1", &format!("=SUM({anchor}#)"));
        assert_spill(&s, anchor, sum);
        refuses_unchanged(&mut s, "data.dataTable", json!({"range": "C3:E5", "rowInput": "A1", "columnInput": "B1"}));
    }
}

#[test]
fn what_if_variables_refuse_spill_members_before_trials_or_reports() {
    for (anchor, formula, variable) in [("A1", "=SEQUENCE(3)", "A2"), ("A2", "=SEQUENCE(1,3)", "B2")] {
        for manual in [false, true] {
            for command in ["data.goalSeek", "data.solver"] {
                let mut s = session();
                set(&mut s, anchor, formula);
                set(&mut s, "E1", &format!("={variable}*2"));
                set(&mut s, "G1", &format!("=SUM({anchor}#)"));
                set(&mut s, "H1", &format!("=SUM({anchor}#)"));
                assert_spill(&s, anchor, 6.0);
                if manual {
                    s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
                }
                let params = if command == "data.goalSeek" {
                    json!({"set": "E1", "to": 20, "changing": variable})
                } else {
                    json!({"objective": "E1", "variables": variable, "constraints": [format!("{variable} <= 10")], "method": "simplex", "reports": ["answer"]})
                };
                refuses_unchanged(&mut s, command, params);
                assert_eq!(value(&s, "G1"), Value::Number(6.0));
            }
        }
    }
}

#[test]
fn what_if_variables_refuse_spill_members_even_without_keeping_solution() {
    let mut s = session();
    set(&mut s, "A1", "=SEQUENCE(3)");
    set(&mut s, "B1", "=2*A2");
    set(&mut s, "H1", "=SUM(A1#)");
    assert_spill(&s, "A1", 6.0);
    refuses_unchanged(
        &mut s,
        "data.solver",
        json!({"objective": "B1", "variables": "A2", "constraints": ["A2 <= 10"], "method": "simplex", "keep": false, "saveModel": false}),
    );
}

#[test]
fn complete_spill_outputs_and_filter_body_anchor_replacement_remain_allowed() {
    let mut s = session();
    set(&mut s, "A1", "'11,22,33");
    set(&mut s, "D2", "=SEQUENCE(1,3)");
    s.execute("data.textToColumns", json!({"range": "A1", "delimiters": [","], "destination": "D2"})).unwrap();
    assert_eq!(value(&s, "F2"), Value::Number(33.0));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(value(&s, "F2"), Value::Number(3.0));
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(value(&s, "F2"), Value::Number(33.0));

    let mut s = session();
    table_setup(&mut s);
    s.execute("edit.clearContents", json!({"range": "D4:E5"})).unwrap();
    set(&mut s, "D4", "=SEQUENCE(2,2)");
    s.execute("data.dataTable", json!({"range": "C3:E5", "rowInput": "A1", "columnInput": "B1"})).unwrap();
    assert_eq!(value(&s, "D4"), Value::Number(31.0));
    assert_eq!(value(&s, "E5"), Value::Number(42.0));

    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Amount", "ID"], [11, 101], [22, 102]]})).unwrap();
    set(&mut s, "D3", "=SEQUENCE(3)");
    s.execute("data.advancedFilter", json!({"range": "A1:B3", "copyTo": "D2"})).unwrap();
    assert_eq!(value(&s, "D3"), Value::Number(11.0));
    assert_eq!(value(&s, "D5"), Value::Empty);
}

#[test]
fn disjoint_spills_and_ordinary_what_if_variables_stay_working() {
    for command in ["data.goalSeek", "data.solver"] {
        let mut s = session();
        set(&mut s, "A2", "1");
        set(&mut s, "B1", "=2*A2");
        set(&mut s, "D1", "=SEQUENCE(3)");
        set(&mut s, "G1", "=SUM(D1#)");
        let params = if command == "data.goalSeek" {
            json!({"set": "B1", "to": 20, "changing": "A2"})
        } else {
            json!({"objective": "B1", "variables": "A2", "constraints": ["A2 <= 10"], "method": "simplex", "saveModel": false})
        };
        s.execute(command, params).unwrap();
        assert!((value(&s, "B1").as_f64().unwrap() - 20.0).abs() < 0.001);
        assert_eq!(value(&s, "G1"), Value::Number(6.0));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(value(&s, "A2"), Value::Number(1.0));
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(value(&s, "G1"), Value::Number(6.0));
    }
}

#[test]
fn bulk_output_commands_preserve_disjoint_spills() {
    for command in ["data.textToColumns", "data.advancedFilter", "data.dataTable"] {
        let mut s = session();
        let params = match command {
            "data.textToColumns" => {
                set(&mut s, "A1", "'11,22,33");
                json!({"range": "A1", "delimiters": [","], "destination": "D2"})
            }
            "data.advancedFilter" => {
                s.execute("range.setValues", json!({"range": "A1", "values": [["Amount", "ID"], [11, 101], [22, 102]]})).unwrap();
                json!({"range": "A1:B3", "copyTo": "D2"})
            }
            _ => {
                table_setup(&mut s);
                json!({"range": "C3:E5", "rowInput": "A1", "columnInput": "B1"})
            }
        };
        set(&mut s, "J1", "=SEQUENCE(3)");
        set(&mut s, "N1", "=SUM(J1#)");
        s.execute(command, params).unwrap();
        assert_eq!(value(&s, "N1"), Value::Number(6.0));
    }
}
