//! The python.* commands.

use gridcraft_core::{CellError, Value};
use serde_json::json;

use super::{s, v};

fn set(s: &mut crate::Session, cell: &str, code: &str) -> serde_json::Value {
    s.execute("python.set", json!({"cell": cell, "code": code})).unwrap()
}

#[test]
fn set_stores_the_code_and_blocks_the_cell_until_it_can_run() {
    let mut s = s();
    let r = set(&mut s, "B1", "xl('A1') + 1");
    assert_eq!(r["cell"], "B1");
    assert_eq!(v(&s, "B1"), Value::Error(CellError::Blocked));
    let got = s.execute("cell.get", json!({"cell": "B1"})).unwrap();
    assert_eq!(got["python"]["code"], "xl('A1') + 1");
    assert_eq!(got["python"]["output"], "value");
    assert_eq!(got["input"], "xl('A1') + 1");
    // The active cell by default.
    s.execute("selection.set", json!({"cell": "D4"})).unwrap();
    s.execute("python.set", json!({"code": "2"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "D4"})).unwrap()["python"]["code"], "2");
    // One undo step takes the cell back.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.execute("cell.get", json!({"cell": "D4"})).unwrap()["python"].is_null());
}

#[test]
fn unchanged_code_keeps_its_cached_result() {
    let mut s = s();
    set(&mut s, "A1", "21 * 2");
    // As a file would have cached it.
    {
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let sh = wb.sheet_mut(0).unwrap();
        let cell = sh.cells.get_mut(gridcraft_core::CellRef::new(0, 0)).unwrap();
        let py = std::sync::Arc::make_mut(cell.py.as_mut().unwrap());
        py.output = Some(gridcraft_model::PyOutput {
            value: Value::Number(42.0),
            type_name: "int".into(),
            preview: "42".into(),
            convertible: true,
            ..Default::default()
        });
        cell.value = Value::Number(42.0);
    }
    set(&mut s, "A1", "21 * 2");
    assert_eq!(v(&s, "A1"), Value::Number(42.0), "same code, same result");
    s.execute("python.set", json!({"cell": "A1", "code": "21 * 2", "output": "object"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["python"]["output"], "object");
    set(&mut s, "A1", "21 * 3");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Blocked), "new code has no result yet");
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["python"]["output"], "object", "the return type stays");
}

#[test]
fn set_refuses_bad_params_long_code_headers_and_protected_cells() {
    let mut s = s();
    for p in [
        json!({"cell": "A1"}),
        json!({"cell": "A1", "code": 5}),
        json!({"cell": "not a cell", "code": "1"}),
        json!({"cell": "A1", "code": "1", "output": "sideways"}),
    ] {
        assert!(s.execute("python.set", p.clone()).is_err(), "{p}");
    }
    // The limit is the stored PY(…) formula's 8,192 characters.
    assert!(s.execute("python.set", json!({"cell": "A1", "code": "y".repeat(8_080)})).is_err());
    s.execute("python.set", json!({"cell": "A1", "code": "y".repeat(8_079)})).unwrap();
    let quoted = format!("{}{}", "\"".repeat(1_200), "y".repeat(5_800));
    let e = s.execute("python.set", json!({"cell": "A2", "code": quoted})).unwrap_err();
    assert!(e.to_string().contains("8192") || e.to_string().contains("8,192"), "{e}");
    // A table header is the column's name.
    s.execute("range.setValues", json!({"range": "C1", "values": [["Name", "Score"], ["a", 1]]})).unwrap();
    s.execute("insert.table", json!({"range": "C1:D2", "headers": true})).unwrap();
    assert!(s.execute("python.set", json!({"cell": "D1", "code": "1"})).is_err());
    assert_eq!(v(&s, "D1"), Value::text("Score"));
    set(&mut s, "D2", "1");
    // A locked cell on a protected sheet refuses it.
    s.execute("review.protectSheet", json!({})).unwrap();
    let e = s.execute("python.set", json!({"cell": "F1", "code": "1"})).unwrap_err();
    assert!(e.to_string().contains("protected"), "{e}");
}

#[test]
fn set_refuses_a_new_choice_of_return_type_on_a_locked_cell() {
    let mut s = s();
    set(&mut s, "H1", "1");
    s.execute("review.protectSheet", json!({})).unwrap();
    // The same code and return type, so the edit itself finds nothing to refuse; only the choice is new.
    let e = s.execute("python.set", json!({"cell": "H1", "code": "1", "output": "value"})).unwrap_err();
    assert!(e.to_string().contains("protected"), "{e}");
    assert!(!py_at(&s, "H1").output_chosen, "the cell is as it was");
    // Nothing new stays nothing to refuse.
    set(&mut s, "H1", "1");
    assert!(!py_at(&s, "H1").output_chosen);
}

#[test]
fn set_over_a_spilling_formula_takes_the_spill_away() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=SEQUENCE(3)"})).unwrap();
    assert_eq!(v(&s, "A3"), Value::Number(3.0));
    set(&mut s, "A1", "[1, 2, 3]");
    assert_eq!(v(&s, "A3"), Value::Empty);
    assert!(s.doc().unwrap().wb.sheet(0).unwrap().spill_ranges.is_empty());
}

/// A cached result as a file would have kept it on the cell: `[[1], [2], [3]]`, a list.
fn cache_list(s: &mut crate::Session, cell: &str) {
    let at = gridcraft_core::CellRef::parse(cell).unwrap();
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    let sh = wb.sheet_mut(0).unwrap();
    let py = std::sync::Arc::make_mut(sh.cells.get_mut(at).unwrap().py.as_mut().unwrap());
    let arr = gridcraft_core::Array::new(3, 1, vec![1.0.into(), 2.0.into(), 3.0.into()]).map(Value::from).unwrap();
    py.output = Some(gridcraft_model::PyOutput {
        value: arr,
        type_name: "list".into(),
        preview: "[1, 2, 3]".into(),
        convertible: true,
        ..Default::default()
    });
}

/// A1 holds `[1, 2, 3]` with its cached list spilled over A1:A3 as Excel values; B1 reads A3.
fn cached_spill_with_a_reader() -> crate::Session {
    let mut s = s();
    set(&mut s, "A1", "[1, 2, 3]");
    cache_list(&mut s, "A1");
    s.execute("formulas.calculateNow", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=SUM(A3)"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(1.0));
    assert_eq!(v(&s, "B1"), Value::Number(3.0));
    s
}

fn output_of(s: &mut crate::Session, cell: &str) -> serde_json::Value {
    s.execute("cell.get", json!({"cell": cell})).unwrap()["python"]["output"].clone()
}

fn undo_depth(s: &mut crate::Session) -> Option<usize> {
    s.execute("history.list", json!({})).unwrap()["undo"].as_array().map(Vec::len)
}

#[test]
fn output_switches_the_return_type_and_drops_a_result_of_the_other_type() {
    let mut s = cached_spill_with_a_reader();
    // The mode it already has keeps the result, and is now a choice. Once chosen, it changes nothing
    // and adds no undo step.
    let r = s.execute("python.output", json!({"cell": "A1", "mode": "value"})).unwrap();
    assert_eq!(r["output"], "value");
    assert_eq!(v(&s, "A3"), Value::Number(3.0), "the kept result still spills");
    let undo = undo_depth(&mut s);
    s.execute("python.output", json!({"cell": "A1", "mode": "value"})).unwrap();
    assert_eq!(undo_depth(&mut s), undo);
    // Without `mode` it switches. The cached list is an Excel value's result: as with python.set
    // and a new return type, the cell has no result until it runs, and its old spill goes with
    // its readers recalculated.
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let r = s.execute("python.output", json!({})).unwrap();
    assert_eq!(r["output"], "object");
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Blocked));
    assert_eq!(v(&s, "A3"), Value::Empty, "the old spill goes");
    assert_eq!(v(&s, "B1"), Value::Number(0.0), "and what read it is recalculated");
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["python"]["code"], "[1, 2, 3]", "the code stays");
    // The same as python.set with the same code and the other return type.
    let mut by_set = cached_spill_with_a_reader();
    by_set.execute("python.set", json!({"cell": "A1", "code": "[1, 2, 3]", "output": "object"})).unwrap();
    for c in ["A1", "A3", "B1"] {
        assert_eq!(v(&s, c), v(&by_set, c), "{c}");
    }
    assert_eq!(output_of(&mut s, "A1"), output_of(&mut by_set, "A1"));
    // Switching back brings no result back: the dropped one belonged to the old type.
    s.execute("python.output", json!({"cell": "A1", "mode": "value"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Error(CellError::Blocked));
    assert_eq!(v(&s, "A3"), Value::Empty);
    // Each switch is one undo step, and undoing the first brings the result and its spill back.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(output_of(&mut s, "A1"), "object");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(output_of(&mut s, "A1"), "value");
    assert_eq!(v(&s, "A1"), Value::Number(1.0));
    assert_eq!(v(&s, "A3"), Value::Number(3.0));
    assert_eq!(v(&s, "B1"), Value::Number(3.0));
    // A cell with no result keeps showing #BLOCKED!.
    set(&mut s, "C1", "x = 1");
    s.execute("python.output", json!({"cell": "C1"})).unwrap();
    assert_eq!(v(&s, "C1"), Value::Error(CellError::Blocked));
    assert_eq!(output_of(&mut s, "C1"), "object");
}

#[test]
fn output_refuses_table_headers_and_locked_cells_as_set_does() {
    let mut s = s();
    // A Python cell in a table's header row (as a file could hold one): python.set refuses to
    // put code there, and python.output refuses to change it.
    s.execute("range.setValues", json!({"range": "C1", "values": [["Name", "Score"], ["a", 1]]})).unwrap();
    s.execute("insert.table", json!({"range": "C1:D2", "headers": true})).unwrap();
    {
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let cell = wb.sheet_mut(0).unwrap().cells.get_mut(gridcraft_core::CellRef::parse("D1").unwrap()).unwrap();
        cell.py = Some(std::sync::Arc::new(gridcraft_model::PyCode::new("'Score'")));
    }
    let e = s.execute("python.output", json!({"cell": "D1"})).unwrap_err();
    assert!(e.to_string().contains("table header"), "{e}");
    assert_eq!(output_of(&mut s, "D1"), "value");
    // So does choosing the return type already in effect.
    let e = s.execute("python.output", json!({"cell": "D1", "mode": "value"})).unwrap_err();
    assert!(e.to_string().contains("table header"), "{e}");
    assert!(!py_at(&s, "D1").output_chosen);
    // A locked cell on a protected sheet refuses it; an unlocked one formatted Hidden takes it,
    // as it takes python.set.
    set(&mut s, "F1", "1");
    set(&mut s, "G1", "2");
    s.execute("home.formatCells", json!({"range": "G1", "style": {"protection": {"locked": false, "hidden": true}}})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    let e = s.execute("python.output", json!({"cell": "F1"})).unwrap_err();
    assert!(e.to_string().contains("protected"), "{e}");
    assert_eq!(output_of(&mut s, "F1"), "value");
    // So is choosing the return type already in effect, which keeps the result and changes the cell.
    let e = s.execute("python.output", json!({"cell": "F1", "mode": "value"})).unwrap_err();
    assert!(e.to_string().contains("protected"), "{e}");
    assert!(!py_at(&s, "F1").output_chosen);
    s.execute("python.set", json!({"cell": "G1", "code": "2"})).unwrap();
    s.execute("python.output", json!({"cell": "G1"})).unwrap();
    assert_eq!(output_of(&mut s, "G1"), "object");
}

#[test]
fn output_refuses_cells_without_python_and_bad_modes() {
    let mut s = s();
    // Empty params: the active cell, which holds no Python code.
    let e = s.execute("python.output", json!({})).unwrap_err();
    assert!(e.to_string().contains("isn't a Python cell"), "{e}");
    s.execute("cell.set", json!({"cell": "B1", "input": "=1+1"})).unwrap();
    assert!(s.execute("python.output", json!({"cell": "B1"})).is_err());
    set(&mut s, "A1", "1");
    for p in [json!({"cell": "A1", "mode": "sideways"}), json!({"cell": "A1", "mode": 1}), json!({"cell": "nowhere"})] {
        assert!(s.execute("python.output", p.clone()).is_err(), "{p}");
    }
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["python"]["output"], "value");
    // python.set refuses a return type that isn't text too, rather than ignoring it.
    assert!(s.execute("python.set", json!({"cell": "A1", "code": "1", "output": true})).is_err());
}

#[test]
fn a_recorded_switch_keeps_the_return_type_it_set() {
    // python.output without `mode` switches, so a recording keeps the return type the switch set
    // (as Bold records `on`): played back on a cell that already has it, nothing switches back.
    let mut s = s();
    set(&mut s, "A1", "1");
    s.execute("macro.record", json!({"relative": false})).unwrap();
    s.execute("python.output", json!({"cell": "A1"})).unwrap();
    s.execute("macro.record", json!({"stop": true, "name": "Rec"})).unwrap();
    assert_eq!(output_of(&mut s, "A1"), "object");
    let m = s.execute("macro.get", json!({"name": "Rec"})).unwrap();
    let m = crate::macros::parse_macro(&m).unwrap();
    let steps = crate::macros::command_steps(&m.steps);
    let step = steps.iter().find(|(_, id, _)| *id == "python.output").map(|(_, _, p)| (*p).clone());
    assert_eq!(step.and_then(|p| p.get("mode").cloned()), Some(json!("object")));
    for _ in 0..2 {
        s.execute("macro.run", json!({"name": "Rec", "onConflict": "overwrite"})).unwrap();
        assert_eq!(output_of(&mut s, "A1"), "object");
    }
    // A run inside a script is recorded the same way.
    s.execute("automate.newScript", json!({"name": "Switch", "commands": [{"command": "python.output", "params": {"cell": "A1"}}]})).unwrap();
    s.execute("macro.record", json!({"relative": false})).unwrap();
    s.execute("automate.runScript", json!({"name": "Switch"})).unwrap();
    s.execute("macro.record", json!({"stop": true, "name": "Rec2"})).unwrap();
    assert_eq!(output_of(&mut s, "A1"), "value");
    for _ in 0..2 {
        s.execute("macro.run", json!({"name": "Rec2", "onConflict": "overwrite"})).unwrap();
        assert_eq!(output_of(&mut s, "A1"), "value");
    }
}

fn is_python(s: &mut crate::Session, cell: &str) -> bool {
    !s.execute("cell.get", json!({"cell": cell})).unwrap()["python"].is_null()
}

#[test]
fn clear_keeps_what_the_cell_and_its_spill_show_as_plain_values() {
    let mut s = cached_spill_with_a_reader();
    s.execute("cell.set", json!({"cell": "B1", "input": "=SUM(A1:A3)"})).unwrap();
    s.execute("home.bold", json!({"range": "A1:A2"})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let undo = undo_depth(&mut s);
    // Empty params: the active cell.
    let r = s.execute("python.clear", json!({})).unwrap();
    assert_eq!(r["cell"], "A1");
    assert_eq!(undo_depth(&mut s), undo.map(|n| n + 1), "one undo step");
    assert!(!is_python(&mut s, "A1"), "the code goes");
    for (c, n) in [("A1", 1.0), ("A2", 2.0), ("A3", 3.0)] {
        assert_eq!(v(&s, c), Value::Number(n), "{c} keeps its value");
        assert_eq!(s.execute("cell.get", json!({"cell": c})).unwrap()["input"], n.to_string(), "{c} holds it as a plain value");
    }
    let sh = s.doc().unwrap().wb.sheet(0).unwrap().clone();
    assert!(sh.spill_ranges.is_empty() && sh.spill.is_empty(), "no spill is left");
    let a2 = gridcraft_core::CellRef::parse("A2").unwrap();
    assert!(s.doc().unwrap().wb.styles.get(sh.style_id(a2)).font.bold, "formats stay");
    assert_eq!(v(&s, "B1"), Value::Number(6.0), "the reader keeps its value");
    // The values are cells of their own now: editing one is no spill conflict, and the reader
    // follows them.
    s.execute("cell.set", json!({"cell": "A2", "input": "20"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(1.0), "no #SPILL!");
    assert_eq!(v(&s, "B1"), Value::Number(24.0));
}

#[test]
fn clear_recalculates_what_reads_the_cells() {
    let mut s = cached_spill_with_a_reader();
    s.execute("cell.set", json!({"cell": "B2", "input": "=ISFORMULA(A1)"})).unwrap();
    s.execute("cell.set", json!({"cell": "B3", "input": "=ROWS(A1#)"})).unwrap();
    assert_eq!(v(&s, "B3"), Value::Number(3.0));
    s.execute("python.clear", json!({"cell": "A1"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Number(3.0), "a reader of a spilled cell keeps its value");
    assert_eq!(v(&s, "B2"), Value::Bool(false), "a reader of the cell sees a plain value");
    assert_eq!(v(&s, "B3"), Value::Error(CellError::Ref), "a spill reference finds no spill");
}

#[test]
fn clear_is_undone_in_one_step_with_code_return_type_and_result() {
    let mut s = cached_spill_with_a_reader();
    s.execute("python.set", json!({"cell": "C1", "code": "x = 1", "output": "object"})).unwrap();
    s.execute("python.clear", json!({"cell": "A1"})).unwrap();
    s.execute("python.clear", json!({"cell": "C1"})).unwrap();
    assert!(!is_python(&mut s, "C1"));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(output_of(&mut s, "C1"), "object");
    s.execute("edit.undo", json!({})).unwrap();
    let got = s.execute("cell.get", json!({"cell": "A1"})).unwrap();
    assert_eq!(got["python"]["code"], "[1, 2, 3]");
    assert_eq!(got["python"]["output"], "value");
    let sh = s.doc().unwrap().wb.sheet(0).unwrap().clone();
    let a1 = gridcraft_core::CellRef::new(0, 0);
    assert!(sh.cell(a1).and_then(|c| c.py.as_ref()).is_some_and(|py| py.output.is_some()), "the cached result is back");
    assert_eq!(sh.spill_ranges.get(&a1).map(|r| r.a1()), Some("A1:A3".to_string()), "and spills again");
    assert_eq!(v(&s, "A3"), Value::Number(3.0));
    assert!(sh.cell(gridcraft_core::CellRef::new(2, 0)).is_none_or(|c| !c.has_content()), "A3 holds nothing of its own");
    assert_eq!(v(&s, "B1"), Value::Number(3.0));
    s.execute("edit.redo", json!({})).unwrap();
    assert!(!is_python(&mut s, "A1"));
    assert_eq!(v(&s, "A3"), Value::Number(3.0));
}

#[test]
fn clear_refuses_cells_without_python_headers_and_locked_cells() {
    let mut s = s();
    // Empty params: the active cell, which holds no Python code.
    let e = s.execute("python.clear", json!({})).unwrap_err();
    assert!(e.to_string().contains("isn't a Python cell"), "{e}");
    s.execute("cell.set", json!({"cell": "B1", "input": "=1+1"})).unwrap();
    assert!(s.execute("python.clear", json!({"cell": "B1"})).is_err());
    assert_eq!(s.execute("cell.get", json!({"cell": "B1"})).unwrap()["input"], "=1+1");
    assert!(s.execute("python.clear", json!({"cell": "nowhere"})).is_err());
    // A Python cell in a table's header row (as a file could hold one) is left alone, as
    // python.set and python.output leave it.
    s.execute("range.setValues", json!({"range": "C1", "values": [["Name", "Score"], ["a", 1]]})).unwrap();
    s.execute("insert.table", json!({"range": "C1:D2", "headers": true})).unwrap();
    {
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let cell = wb.sheet_mut(0).unwrap().cells.get_mut(gridcraft_core::CellRef::parse("D1").unwrap()).unwrap();
        cell.py = Some(std::sync::Arc::new(gridcraft_model::PyCode::new("'Score'")));
    }
    let e = s.execute("python.clear", json!({"cell": "D1"})).unwrap_err();
    assert!(e.to_string().contains("table header"), "{e}");
    assert!(is_python(&mut s, "D1"));
}

#[test]
fn clear_on_a_protected_sheet_needs_unlocked_cells_and_never_returns_hidden_code() {
    let mut s = cached_spill_with_a_reader();
    set(&mut s, "F1", "1");
    set(&mut s, "G1", "'secret code'");
    // G1 is unlocked and its code hidden; A1 is unlocked but its result spills over locked cells.
    s.execute("home.formatCells", json!({"range": "G1", "style": {"protection": {"locked": false, "hidden": true}}})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": {"protection": {"locked": false}}})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    let undo = undo_depth(&mut s);
    let e = s.execute("python.clear", json!({"cell": "F1"})).unwrap_err();
    assert!(e.to_string().contains("protected"), "{e}");
    assert!(is_python(&mut s, "F1"));
    let e = s.execute("python.clear", json!({"cell": "A1"})).unwrap_err();
    assert!(e.to_string().contains("protected"), "a spill over locked cells: {e}");
    assert!(is_python(&mut s, "A1"));
    assert_eq!(v(&s, "A3"), Value::Number(3.0));
    assert_eq!(undo_depth(&mut s), undo, "a refusal changes nothing");
    let r = s.execute("python.clear", json!({"cell": "G1"})).unwrap();
    assert!(!r.to_string().contains("secret"), "{r}");
    assert!(!is_python(&mut s, "G1"));
}

/// Sets what the Python cell at `cell` shows, as a file would have cached it: `value`, the cell's
/// value, and `output`, its kept result (none for an error the code raised).
fn cache_shown(s: &mut crate::Session, cell: &str, value: Value, output: Option<Value>) {
    let at = gridcraft_core::CellRef::parse(cell).unwrap();
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    let c = wb.sheet_mut(0).unwrap().cells.get_mut(at).unwrap();
    c.value = value;
    std::sync::Arc::make_mut(c.py.as_mut().unwrap()).output = output.map(|value| gridcraft_model::PyOutput {
        value,
        type_name: "object".into(),
        preview: String::new(),
        convertible: true,
        ..Default::default()
    });
}

fn bold(s: &mut crate::Session, cell: &str) -> bool {
    let sh = s.doc().unwrap().wb.sheet(0).unwrap().clone();
    s.doc().unwrap().wb.styles.get(sh.style_id(gridcraft_core::CellRef::parse(cell).unwrap())).font.bold
}

#[test]
fn clear_leaves_a_never_run_cell_empty_with_its_format() {
    let mut s = s();
    set(&mut s, "B1", "x = 1");
    s.execute("home.bold", json!({"range": "B1"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=ISBLANK(B1)"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Error(CellError::Blocked));
    let r = s.execute("python.clear", json!({"cell": "B1"})).unwrap();
    assert!(r["value"].is_null() || r["value"] == "", "{r}");
    assert_eq!(v(&s, "B1"), Value::Empty, "#BLOCKED! is no result: the cell is empty");
    assert!(!is_python(&mut s, "B1"));
    assert!(bold(&mut s, "B1"), "the format stays");
    assert_eq!(v(&s, "C1"), Value::Bool(true), "a reader sees an empty cell");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Error(CellError::Blocked));
    assert!(is_python(&mut s, "B1"));
}

#[test]
fn clear_empties_a_cached_python_error_and_keeps_an_ordinary_one() {
    let mut s = s();
    set(&mut s, "A1", "raise ValueError()");
    cache_shown(&mut s, "A1", Value::Error(CellError::Python), None);
    set(&mut s, "A2", "1 / 0 in Excel terms");
    cache_shown(&mut s, "A2", Value::Error(CellError::Div0), Some(Value::Error(CellError::Div0)));
    for c in ["A1", "A2"] {
        s.execute("python.clear", json!({"cell": c})).unwrap();
        assert!(!is_python(&mut s, c));
    }
    assert_eq!(v(&s, "A1"), Value::Empty, "#PYTHON! is no value an ordinary cell holds");
    assert_eq!(v(&s, "A2"), Value::Error(CellError::Div0), "an ordinary error the result cached stays");
    assert_eq!(s.execute("cell.get", json!({"cell": "A2"})).unwrap()["input"], "#DIV/0!");
}

#[test]
fn clear_keeps_a_mixed_spill_and_empties_its_python_errors() {
    let mut s = s();
    set(&mut s, "A1", "mixed");
    let mixed = vec![
        Value::Number(1.0),
        Value::text("a"),
        Value::Bool(true),
        Value::Error(CellError::Div0),
        Value::Error(CellError::Timeout),
        Value::Number(6.0),
    ];
    let arr = gridcraft_core::Array::new(6, 1, mixed.clone()).map(Value::from).unwrap();
    cache_shown(&mut s, "A1", Value::Number(1.0), Some(arr));
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "A5"), Value::Error(CellError::Timeout));
    s.execute("python.clear", json!({"cell": "A1"})).unwrap();
    for (i, want) in mixed.iter().enumerate() {
        let c = format!("A{}", i + 1);
        let want = if matches!(want, Value::Error(CellError::Timeout)) { Value::Empty } else { want.clone() };
        assert_eq!(v(&s, &c), want, "{c}");
    }
    assert!(s.doc().unwrap().wb.sheet(0).unwrap().spill_ranges.is_empty());
}

fn py_at(s: &crate::Session, cell: &str) -> gridcraft_model::PyCode {
    let at = gridcraft_core::CellRef::parse(cell).unwrap();
    s.doc().unwrap().wb.active().unwrap().cell(at).unwrap().py.as_deref().unwrap().clone()
}

/// The cell's last run gave a picture.
fn cache_picture(s: &mut crate::Session, cell: &str) {
    let at = gridcraft_core::CellRef::parse(cell).unwrap();
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    let c = wb.sheet_mut(0).unwrap().cells.get_mut(at).unwrap();
    c.value = Value::text("Image");
    let picture = gridcraft_model::CellPicture { data: vec![1, 2, 3], mime: "image/png".into(), alt: "plot".into() };
    let image = gridcraft_model::PyImage { picture: std::sync::Arc::new(picture), width: 4, height: 4, density: 1, floor: None, fitted: None };
    std::sync::Arc::make_mut(c.py.as_mut().unwrap()).output = Some(gridcraft_model::PyOutput {
        value: Value::text("Image"),
        type_name: "Image".into(),
        preview: "Image".into(),
        image: Some(image),
        ..Default::default()
    });
}

#[test]
fn a_return_type_asked_for_is_a_choice_though_it_is_already_in_effect() {
    let mut s = s();
    set(&mut s, "A1", "plot()");
    assert!(!py_at(&s, "A1").output_chosen);
    s.execute("python.output", json!({"cell": "A1", "mode": "value"})).unwrap();
    assert!(py_at(&s, "A1").output_chosen);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(!py_at(&s, "A1").output_chosen, "undo takes the choice back");
    s.execute("edit.redo", json!({})).unwrap();
    // So a picture from a later run goes in the cell, rather than behaving as a Python Object.
    cache_picture(&mut s, "A1");
    let py = py_at(&s, "A1");
    assert!(!py.shows_object() && py.image_in_cell().is_some());
}

#[test]
fn python_set_records_a_return_type_it_is_given_and_keeps_an_earlier_choice() {
    let mut s = s();
    set(&mut s, "A1", "1");
    assert!(!py_at(&s, "A1").output_chosen, "code alone is no choice");
    s.execute("python.set", json!({"cell": "A1", "code": "1", "output": "value"})).unwrap();
    assert!(py_at(&s, "A1").output_chosen);
    set(&mut s, "A1", "2");
    assert!(py_at(&s, "A1").output_chosen, "new code keeps it");
    s.execute("python.set", json!({"cell": "B1", "code": "1", "output": "object"})).unwrap();
    let py = py_at(&s, "B1");
    assert!(py.object && py.output_chosen);
}

#[test]
fn a_new_return_type_or_new_code_keeps_what_a_text_does_with_its_lines() {
    let mut s = s();
    set(&mut s, "A1", "text");
    {
        let at = gridcraft_core::CellRef::parse("A1").unwrap();
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let c = wb.sheet_mut(0).unwrap().cells.get_mut(at).unwrap();
        std::sync::Arc::make_mut(c.py.as_mut().unwrap()).spill_lines = true;
    }
    s.execute("python.output", json!({"cell": "A1", "mode": "object"})).unwrap();
    assert!(py_at(&s, "A1").spill_lines, "python.output");
    set(&mut s, "A1", "other");
    assert!(py_at(&s, "A1").spill_lines, "python.set");
}

#[test]
fn a_picture_with_no_choice_keeps_its_result_when_its_return_type_becomes_a_choice() {
    // It behaves as a Python Object, so a switch moves it into the cell, with no new run.
    let mut s = s();
    set(&mut s, "A1", "plot()");
    cache_picture(&mut s, "A1");
    assert!(py_at(&s, "A1").shows_object());
    let r = s.execute("python.output", json!({"cell": "A1"})).unwrap();
    assert_eq!(r["output"], "value");
    let py = py_at(&s, "A1");
    assert!(py.output_chosen && !py.shows_object() && py.image_in_cell().is_some());

    // Asking for the Python Object it already behaves as leaves it showing, with its result.
    set(&mut s, "B1", "plot()");
    cache_picture(&mut s, "B1");
    let r = s.execute("python.output", json!({"cell": "B1", "mode": "object"})).unwrap();
    assert_eq!(r["output"], "object");
    let py = py_at(&s, "B1");
    assert!(py.object && py.output_chosen && py.shows_object() && py.image().is_some());
    assert_eq!(v(&s, "B1"), Value::text("Image"));
}

#[test]
fn replace_keeps_what_the_cell_chose() {
    let mut s = s();
    s.execute("python.set", json!({"cell": "A1", "code": "x = 1", "output": "value"})).unwrap();
    {
        let at = gridcraft_core::CellRef::parse("A1").unwrap();
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let c = wb.sheet_mut(0).unwrap().cells.get_mut(at).unwrap();
        std::sync::Arc::make_mut(c.py.as_mut().unwrap()).spill_lines = true;
    }
    assert_eq!(s.execute("edit.replace", json!({"what": "x", "with": "y"})).unwrap()["replaced"], 1);
    let py = py_at(&s, "A1");
    assert_eq!(py.code, "y = 1");
    assert!(py.output_chosen, "the return type stays a choice");
    assert!(py.spill_lines, "what a text does with its lines stays");
}

#[test]
fn clear_leaves_no_python_only_error_behind() {
    use CellError::{Blocked, Busy, Connect, Python, Timeout};
    for error in [Blocked, Busy, Connect, Python, Timeout] {
        let mut s = s();
        set(&mut s, "A1", "1");
        {
            let at = gridcraft_core::CellRef::parse("A1").unwrap();
            let d = s.doc_mut().unwrap();
            std::sync::Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap().cells.get_mut(at).unwrap().value = Value::Error(error);
        }
        s.execute("python.clear", json!({"cell": "A1"})).unwrap();
        assert_eq!(v(&s, "A1"), Value::Empty, "{error:?}");
    }
}

#[test]
fn a_protected_sheet_refuses_a_new_linked_picture() {
    let mut s = s();
    set(&mut s, "A1", "plot()");
    cache_picture(&mut s, "A1");
    let image = s.execute("python.displayOverCells", json!({"cell": "A1", "at": "C1"})).unwrap()["image"].clone();
    s.execute("review.protectSheet", json!({})).unwrap();
    let count = |s: &crate::Session| s.doc().unwrap().wb.active().unwrap().images.len();
    let before = count(&s);
    let e = s.execute("python.displayOverCells", json!({"cell": "A1", "at": "E1"})).unwrap_err();
    assert!(e.to_string().contains("protected"), "{e}");
    let e = s.execute("python.createReference", json!({"id": image})).unwrap_err();
    assert!(e.to_string().contains("protected"), "{e}");
    assert_eq!(count(&s), before, "no picture was added");
}

#[test]
fn python_set_with_the_same_code_keeps_a_cell_waiting_for_its_result() {
    let mut s = s();
    set(&mut s, "A1", "1");
    cache_list(&mut s, "A1");
    {
        let at = gridcraft_core::CellRef::parse("A1").unwrap();
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let c = wb.sheet_mut(0).unwrap().cells.get_mut(at).unwrap();
        std::sync::Arc::make_mut(c.py.as_mut().unwrap()).busy = true;
    }
    set(&mut s, "A1", "1");
    assert!(py_at(&s, "A1").busy);
}

#[test]
fn python_set_with_the_same_code_keeps_a_picture_with_no_choice_for_either_return_type() {
    for (output, object) in [("object", true), ("value", false)] {
        let mut s = s();
        set(&mut s, "A1", "plot()");
        cache_picture(&mut s, "A1");
        s.execute("python.set", json!({"cell": "A1", "code": "plot()", "output": output})).unwrap();
        let py = py_at(&s, "A1");
        assert!(py.image().is_some(), "{output}");
        assert_eq!((py.output_chosen, py.shows_object()), (true, object), "{output}");
    }
}

/// A linked picture names its source by a reference, which goes on naming it when the sheet is renamed;
/// and it exists only while the source can give it a picture.
#[test]
fn a_linked_picture_follows_a_renamed_sheet_and_goes_with_its_source() {
    let mut s = s();
    set(&mut s, "A1", "plot()");
    cache_picture(&mut s, "A1");
    s.execute("python.displayOverCells", json!({"cell": "A1", "at": "C1"})).unwrap();
    let sources = |s: &crate::Session| -> Vec<Option<String>> {
        s.doc().unwrap().wb.active().unwrap().images.iter().map(|o| o.python_source.clone()).collect()
    };
    assert_eq!(sources(&s), [Some("Sheet1!$A$1".to_string())]);
    s.execute("sheet.rename", json!({"name": "My Data"})).unwrap();
    assert_eq!(sources(&s), [Some("'My Data'!$A$1".to_string())], "the reference follows the name");
    s.execute("cell.set", json!({"cell": "A1", "input": "5"})).unwrap();
    assert_eq!(sources(&s), Vec::<Option<String>>::new(), "the picture goes when the cell stops being a Python cell");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(sources(&s).len(), 1, "and undo brings it back");
}
