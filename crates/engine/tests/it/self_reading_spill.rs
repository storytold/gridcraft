use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_engine::calc::Calc;
use serde_json::{Value as Json, json};

fn run(s: &mut Session, command: &str, params: Json) {
    s.execute(command, params).unwrap();
}

fn value(s: &Session, cell: &str) -> Value {
    let wb = &s.doc().unwrap().wb;
    wb.sheet(wb.sheet_index("Sales").unwrap()).unwrap().value(CellRef::parse(cell).unwrap())
}

/// Issue #3401 (fuzz seed 108): `=I20:K27` in J24 spills over J24:L31 and reads J24:K27, part of
/// its own spill. L28 is I20:K27's K24, which is the value J24 spills into K24 (J20). The
/// incremental recalculation evaluated J24 once, before its array was laid out, and left L28 at
/// 0 while K24 showed the text.
#[test]
fn a_spill_reading_its_own_area_matches_a_full_recalc() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"sample": "sales"}));
    let label = Value::Text("Year-over-year".into());
    // The sample's label (B17) where the spill's first row reads it.
    run(&mut s, "cell.set", json!({"cell": "J20", "input": "=B17"}));
    assert_eq!(value(&s, "J20"), label, "the sample keeps its label in B17");
    run(&mut s, "cell.set", json!({"cell": "J24", "input": "=I20:K27"}));

    assert_eq!(value(&s, "K24"), label);
    assert_eq!(value(&s, "L28"), label, "L28 reads K24");

    let mut fresh = (*s.doc().unwrap().wb).clone();
    Calc::new().recalc_all(&mut fresh);
    let sheet = fresh.sheet(fresh.sheet_index("Sales").unwrap()).unwrap();
    for at in ["J24", "K24", "L24", "J28", "K28", "L28", "L31"] {
        assert_eq!(value(&s, at), sheet.value(CellRef::parse(at).unwrap()), "{at} after a full recalc");
    }
}
