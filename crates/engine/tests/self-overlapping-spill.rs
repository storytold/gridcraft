//! A dynamic array whose spill area overlaps its own input (a circular reference through the
//! spill) gets the same values from recalculating after an edit as from a full recalculation.
//! The sequences are the command-sequence fuzz's shrunk failures for this.

use std::collections::BTreeSet;

use gridcraft_calc::Calc;
use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_model::Workbook;
use serde_json::{Value as Json, json};

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a == b,
    }
}

/// The first cell whose value differs between the workbook and a full recalculation of it.
fn full_recalc_diff(wb: &Workbook) -> Option<String> {
    let mut fresh = wb.clone();
    Calc::new().recalc_all(&mut fresh);
    for (x, y) in wb.sheets.iter().zip(&fresh.sheets) {
        let mut keys: BTreeSet<CellRef> = BTreeSet::new();
        for s in [x, y] {
            keys.extend(s.cells.iter().map(|(c, _)| c));
            keys.extend(s.spill.keys().copied());
        }
        for c in keys {
            let (a, b) = (x.value(c), y.value(c));
            if !same(&a, &b) {
                return Some(format!("{}!{}: {a:?} in memory, {b:?} after a full recalc", x.name, c.a1()));
            }
        }
    }
    None
}

fn check(sample: &str, steps: &[(&str, Json)]) {
    let mut s = Session::new();
    s.execute("file.new", json!({ "sample": sample })).unwrap();
    for (i, (id, params)) in steps.iter().enumerate() {
        s.execute(id, params.clone()).ok();
        let wb = &s.doc().unwrap().wb;
        assert_eq!(full_recalc_diff(wb), None, "{sample}, after step {i} `{id}` {params}");
    }
}

#[test]
fn sort_over_its_own_cell_then_an_input_edited() {
    // The bead's repro (fuzz seed 506): J17 reads itself and the cells it spills into.
    check("grades", &[("cell.set", json!({"cell": "J17", "input": "=SORT(H17:L19)"})), ("cell.set", json!({"cell": "H18", "input": "5"}))]);
    check(
        "grades",
        &[
            ("cell.set", json!({"cell": "J17", "input": "=SORT(H17:L19)"})),
            ("cell.set", json!({"cell": "H18", "input": "=INDEX(F15:H18,4,1)", "sheet": 0})),
        ],
    );
}

#[test]
fn sort_spilling_over_part_of_its_input() {
    // Seed 1516: the anchor outside its input, the array over part of it.
    check(
        "budget",
        &[("cell.set", json!({"cell": "G9", "input": "=SORT(D12:H15)"})), ("cell.set", json!({"cell": "H15", "input": "=AVERAGE(C19:F23)*6"}))],
    );
    // Seed 1986: the same after rows above it are deleted.
    check(
        "sales",
        &[
            ("cell.set", json!({"cell": "G15", "input": "=SORT(H19:J23)"})),
            ("home.deleteRows", json!({"rows": "8:9"})),
            ("range.setValues", json!({"range": "G18", "values": [[null, 146, "=INDEX(K5:M10,4,1)"]]})),
        ],
    );
}

#[test]
fn arrays_whose_spill_feeds_formulas_they_read() {
    // Seed 637: G14:H20 read J22's array, which reads them.
    check(
        "budget",
        &[
            ("selection.set", json!({"range": "7:9"})),
            ("cell.set", json!({"cell": "J22", "input": "=SORT(G14:K21)"})),
            ("range.fill", json!({"input": "=AVERAGE(A12:E12)*4", "range": "G14:H20"})),
        ],
    );
    // Seed 1445: one array read by another that spills over its own input.
    check(
        "grades",
        &[("cell.set", json!({"cell": "K14", "input": "=SEQUENCE(5)*D8"})), ("cell.set", json!({"cell": "L12", "input": "=UNIQUE(I11:M15)"}))],
    );
}

#[test]
fn transpose_over_its_own_cell_then_cells_unmerged() {
    // Seed 1600.
    check("budget", &[("cell.set", json!({"cell": "G23", "input": "=TRANSPOSE(F23:G29)"})), ("home.unmergeCells", json!({"range": "J20:M24"}))]);
}

#[test]
fn a_formula_reading_an_array_recalculated_after_it() {
    // Seed 124: A8 reads F17's array, and F17 reads A8's: in a pass that recalculates both, A8
    // (first in row order) calculates F17 to read its new array rather than seeing it blank.
    check(
        "budget",
        &[
            ("home.insertSheet", json!({"before": 0})),
            ("cell.set", json!({"cell": "F17", "input": "=TRANSPOSE(C8:E13)"})),
            ("range.setValues", json!({"range": "A8", "values": [["=UNIQUE(J14:M18)"], ["=TRANSPOSE(K18:M19)"]]})),
        ],
    );
}
