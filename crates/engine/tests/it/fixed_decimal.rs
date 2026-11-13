//! File › Options › Editing › Automatically insert a decimal point (#3540): a preference kept in
//! prefs.json and set with `options.fixedDecimal`, which the cell editor applies to the whole
//! numbers typed in (`cmd::edit::fixed_decimal_entry`). Commands enter what they're given.

use gridcraft_engine::cmd::edit::fixed_decimal_entry;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::{Prefs, Session};
use serde_json::json;

fn book() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn value(s: &Session, a1: &str) -> Value {
    let d = s.active().unwrap();
    d.wb.active().unwrap().value(CellRef::parse(a1).unwrap())
}

#[test]
fn it_is_off_with_two_places_and_empty_params_only_report_it() {
    let mut s = book();
    assert!(!s.prefs.fixed_decimal);
    assert_eq!(s.prefs.fixed_decimal_places, 2);
    assert_eq!(s.execute("options.fixedDecimal", json!({})).unwrap(), json!({"on": false, "places": 2}));
    assert!(s.take_ui_requests().is_empty(), "no dialog");
    assert_eq!(s.execute("options.fixedDecimal", json!({"on": true})).unwrap(), json!({"on": true, "places": 2}));
    assert_eq!(s.execute("options.fixedDecimal", json!({"places": -3})).unwrap(), json!({"on": true, "places": -3}));
    assert_eq!(s.execute("options.fixedDecimal", json!({"on": null, "places": 4.0})).unwrap(), json!({"on": true, "places": 4}));
    assert_eq!(s.execute("options.fixedDecimal", json!({})).unwrap(), json!({"on": true, "places": 4}), "{{}} changes nothing");
    assert_eq!(s.execute("options.fixedDecimal", json!({"on": false, "places": 100})).unwrap(), json!({"on": false, "places": 100}));
    assert_eq!(s.execute("options.fixedDecimal", json!({"places": -100})).unwrap()["places"], -100);
    // A preference: the workbook is neither changed nor dirty, and there is no undo step.
    let d = s.active().unwrap();
    assert!(!d.is_dirty());
    assert!(d.undo.is_empty());
    assert!(s.journal.iter().all(|(id, _)| id != "options.fixedDecimal"), "not journaled");
}

#[test]
fn bad_settings_are_refused_and_change_nothing() {
    let mut s = book();
    s.execute("options.fixedDecimal", json!({"on": true, "places": 3})).unwrap();
    for bad in [
        json!({"places": 101}),
        json!({"places": -101}),
        json!({"places": 2.5}),
        json!({"places": "2"}),
        json!({"places": true}),
        json!({"places": 1e300}),
        json!({"places": u64::MAX}),
        json!({"places": i64::MIN}),
        json!({"places": [2]}),
        json!({"on": "yes"}),
        json!({"on": 1}),
        json!({"on": false, "places": 500}),
        json!({"on": "no", "places": 1}),
    ] {
        assert!(s.execute("options.fixedDecimal", bad.clone()).is_err(), "{bad}");
        assert_eq!((s.prefs.fixed_decimal, s.prefs.fixed_decimal_places), (true, 3), "{bad} changed nothing");
    }
}

#[test]
fn a_typed_whole_number_gets_its_decimal_point_and_commands_keep_what_they_are_given() {
    let mut s = book();
    let typed = |s: &Session, cell: &str, input: &str| {
        let d = s.active().unwrap();
        fixed_decimal_entry(&s.prefs, &d.wb, 0, CellRef::parse(cell).unwrap(), input)
    };
    assert_eq!(typed(&s, "A1", "2834"), None, "off by default");
    s.execute("options.fixedDecimal", json!({"on": true})).unwrap();
    assert_eq!(typed(&s, "A1", "2834").as_deref(), Some("28.34"));
    assert_eq!(typed(&s, "A1", "-25").as_deref(), Some("-0.25"));
    for kept in ["1.5", "=A1", "abc", "5%", "1e3", "1/2", ""] {
        assert_eq!(typed(&s, "A1", kept), None, "{kept}");
    }
    s.execute("options.fixedDecimal", json!({"places": -2})).unwrap();
    assert_eq!(typed(&s, "A1", "2834").as_deref(), Some("283400"));
    s.execute("options.fixedDecimal", json!({"places": 0})).unwrap();
    assert_eq!(typed(&s, "A1", "2834"), None, "0 places");
    s.execute("options.fixedDecimal", json!({"places": 2})).unwrap();

    // Text cells keep the digits typed; percent cells read them as a percent (3 → 3%) instead.
    s.execute("home.numberFormat", json!({"range": "B1", "format": "Text"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "C1", "format": "Percentage"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "D1", "format": "0.00"})).unwrap();
    assert_eq!(typed(&s, "B1", "2834"), None);
    assert_eq!(typed(&s, "C1", "2834"), None);
    assert_eq!(typed(&s, "D1", "2834").as_deref(), Some("28.34"), "other number formats take it");

    // The text the editor enters reads as the number typing it gives.
    let entry = typed(&s, "A1", "2834").unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": entry})).unwrap();
    assert_eq!(value(&s, "A1"), Value::Number(28.34));
    // `cell.set`, `range.setValues` and fill enter what they're given.
    s.execute("cell.set", json!({"cell": "A2", "input": "2834"})).unwrap();
    s.execute("cell.set", json!({"cell": "A3", "value": 2834})).unwrap();
    s.execute("range.setValues", json!({"range": "A4", "values": [["2834"]]})).unwrap();
    s.execute("selection.set", json!({"range": "A5:A6"})).unwrap();
    s.execute("range.fill", json!({"input": "2834"})).unwrap();
    for cell in ["A2", "A3", "A4", "A5", "A6"] {
        assert_eq!(value(&s, cell), Value::Number(2834.0), "{cell}");
    }
}

#[test]
fn range_fill_with_a_fixed_decimal_decides_it_for_each_cell_by_its_format() {
    // Ctrl+Enter passes `fixedDecimal` while the option is on: each cell of the selection takes
    // the decimal point as typing into it would, whatever the active cell's format.
    let mut s = book();
    s.execute("home.numberFormat", json!({"range": "B1:B4", "format": "Text"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "C1:C4", "format": "Percentage"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "D1:D4", "format": "0.00"})).unwrap();
    s.execute("selection.set", json!({"range": "A1:D1"})).unwrap();
    s.execute("range.fill", json!({"input": "2834", "fixedDecimal": 2})).unwrap();
    assert_eq!(value(&s, "A1"), Value::Number(28.34));
    assert_eq!(value(&s, "B1"), Value::from("2834"), "Text keeps the digits");
    assert_eq!(value(&s, "C1"), Value::Number(28.34), "a percentage reads the whole number as a percent: 2834%");
    assert_eq!(value(&s, "D1"), Value::Number(28.34));
    // From a Text cell, the others still take it.
    s.execute("selection.set", json!({"range": "A2:D2", "active": "B2"})).unwrap();
    s.execute("range.fill", json!({"input": "-25", "fixedDecimal": -2})).unwrap();
    assert_eq!(
        [value(&s, "A2"), value(&s, "B2"), value(&s, "C2"), value(&s, "D2")],
        [Value::Number(-2500.0), Value::from("-25"), Value::Number(-0.25), Value::Number(-2500.0)]
    );
    // An entry that isn't a whole number is entered as typed; `null` is no fixed decimal.
    s.execute("selection.set", json!({"range": "A3:D3"})).unwrap();
    s.execute("range.fill", json!({"input": "1.5", "fixedDecimal": 2})).unwrap();
    assert_eq!((value(&s, "A3"), value(&s, "B3")), (Value::Number(1.5), Value::from("1.5")));
    s.execute("range.fill", json!({"input": "2834", "fixedDecimal": null})).unwrap();
    assert_eq!((value(&s, "A3"), value(&s, "D3")), (Value::Number(2834.0), Value::Number(2834.0)));
    // Places the option can't have are refused, and nothing is filled.
    s.execute("selection.set", json!({"range": "A4:D4"})).unwrap();
    for bad in [json!(101), json!(-101), json!(2.5), json!("2"), json!(true), json!([2])] {
        assert!(s.execute("range.fill", json!({"input": "2834", "fixedDecimal": bad.clone()})).is_err(), "{bad}");
        assert_eq!(value(&s, "A4"), Value::Empty, "{bad}");
    }
}

#[test]
fn prefs_json_keeps_the_setting_and_older_or_edited_files_still_load() {
    let prefs = Prefs { fixed_decimal: true, fixed_decimal_places: -3, ..Prefs::default() };
    let saved = serde_json::to_value(&prefs).unwrap();
    assert_eq!((saved["fixedDecimal"].clone(), saved["fixedDecimalPlaces"].clone()), (json!(true), json!(-3)));
    let back: Prefs = serde_json::from_value(saved).unwrap();
    assert_eq!((back.fixed_decimal, back.fixed_decimal_places), (true, -3));

    let old: Prefs = serde_json::from_value(json!({"r1c1": true})).unwrap();
    assert_eq!((old.fixed_decimal, old.fixed_decimal_places), (false, 2), "a prefs.json from before the option");
    // Places the command would refuse give 2, and lose only themselves.
    for places in [json!(1000), json!(-101), json!(2.5), json!("4"), json!(null), json!(1e300), json!([3])] {
        let edited: Prefs = serde_json::from_value(json!({"r1c1": true, "fixedDecimal": true, "fixedDecimalPlaces": places})).unwrap();
        assert_eq!((edited.r1c1, edited.fixed_decimal, edited.fixed_decimal_places), (true, true, 2), "{places}");
    }
}
