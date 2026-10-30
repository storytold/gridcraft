use super::*;

/// Sheet1 and Sheet2 with a workbook range name, a name scoped to each sheet, a constant, a
/// formula and a hidden name.
fn go_to_workbook() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.insertSheet", json!({"name": "Sheet2"})).unwrap();
    s.execute("selection.set", json!({"sheet": 0})).unwrap();
    let define = |s: &mut Session, name: &str, refers: &str, scope: &str| {
        s.execute("formulas.defineName", json!({"name": name, "refersTo": refers, "scope": scope})).unwrap();
    };
    define(&mut s, "Price", "=Sheet1!$B$2:$B$5", "Workbook");
    define(&mut s, "type", "=Sheet1!$D$2:$D$5", "Workbook");
    define(&mut s, "Areas", "=Sheet1!$A$1,Sheet1!$C$3:$C$4", "Workbook");
    define(&mut s, "Local", "=Sheet2!$C$3", "Sheet2");
    define(&mut s, "Own", "=Sheet1!$E$1", "Sheet1");
    define(&mut s, "Rate", "=0.07", "Workbook");
    define(&mut s, "Total", "=SUM(Sheet1!$B$2:$B$5)", "Workbook");
    let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
    wb.names.push(gridcraft_model::DefinedName {
        name: "Secret".into(),
        scope: None,
        formula: "Sheet1!$Z$9".into(),
        comment: String::new(),
        hidden: true,
    });
    s
}

/// "The Go to popup window shows named ranges on every worksheet in your workbook." Names
/// that are not ranges, and hidden names, are not listed.
#[test]
fn go_to_lists_the_named_ranges_of_every_sheet() {
    let mut s = go_to_workbook();
    let list = s.execute("edit.goTo", json!({"list": true})).unwrap();
    assert_eq!(list["names"], json!(["Areas", "Own", "Price", "Sheet2!Local", "type"]), "{list}");
    assert_eq!(list["recent"], json!([]), "{list}");
    // Seen from Sheet2, its own name is bare and Sheet1's is qualified.
    s.execute("selection.set", json!({"sheet": 1})).unwrap();
    let list = s.execute("edit.goTo", json!({"list": true})).unwrap();
    assert_eq!(list["names"], json!(["Areas", "Local", "Price", "Sheet1!Own", "type"]), "{list}");
}

#[test]
fn go_to_a_name_scoped_to_another_sheet_goes_to_that_sheet() {
    let mut s = go_to_workbook();
    let out = s.execute("edit.goTo", json!({"reference": "Sheet2!Local"})).unwrap();
    assert_eq!(out["selection"], "C3", "{out}");
    assert_eq!(s.doc().unwrap().wb.active_sheet(), 1);
    // A name of several areas selects all of them.
    let out = s.execute("edit.goTo", json!({"reference": "Areas"})).unwrap();
    assert_eq!(out["selection"], "A1,C3:C4", "{out}");
    assert_eq!(s.doc().unwrap().wb.active_sheet(), 0);
}

/// A name matches in any case and any script, as a formula matches it.
#[test]
fn go_to_matches_a_sheet_name_in_any_case() {
    let mut s = go_to_workbook();
    s.execute("formulas.defineName", json!({"name": "Größe", "refersTo": "=Sheet2!$D$4", "scope": "Sheet2"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "GRÖSSE", "refersTo": "=Sheet1!$F$6", "scope": "Workbook"})).unwrap();
    let out = s.execute("edit.goTo", json!({"reference": "Sheet2!GRÖSSE"})).unwrap();
    assert_eq!(out["selection"], "D4", "{out}");
    // Seen from Sheet2, its own name hides the workbook name it shares.
    let list = s.execute("edit.goTo", json!({"list": true})).unwrap();
    assert_eq!(list["names"], json!(["Areas", "Größe", "Local", "Price", "Sheet1!Own", "type"]), "{list}");
}

/// "You can enter multiple named ranges or cell references in the Reference box. Separating
/// each with a comma ... Excel will highlight all the ranges."
#[test]
fn go_to_selects_every_reference_and_name_in_a_list() {
    let mut s = go_to_workbook();
    let out = s.execute("edit.goTo", json!({"reference": "B14:C22,F19:G30,H21:H29"})).unwrap();
    assert_eq!(out["selection"], "B14:C22,F19:G30,H21:H29", "{out}");
    let out = s.execute("edit.goTo", json!({"reference": "Price, Type"})).unwrap();
    assert_eq!(out["selection"], "B2:B5,D2:D5", "{out}");
    let out = s.execute("edit.goTo", json!({"reference": "Sheet2!$A$1,Sheet2!B2"})).unwrap();
    assert_eq!(out["selection"], "A1,B2", "{out}");
    assert_eq!(s.doc().unwrap().wb.active_sheet(), 1);
    // Areas on two sheets, and a part that is not a reference, leave the selection alone.
    for bad in ["A1,Sheet1!B2", "A1,Rate", "A1,", "Total"] {
        assert!(s.execute("edit.goTo", json!({"reference": bad})).is_err(), "{bad}");
        assert_eq!(s.doc().unwrap().selection.a1(), "A1,B2", "{bad}");
    }
}

#[test]
fn go_to_reads_the_parts_of_a_long_reference_one_at_a_time() {
    let mut parts = reference_parts("A1, 'Q1, Q2'!B2,SUM(C3,D4)");
    assert_eq!(parts.next(), Some("A1"));
    assert_eq!(parts.next(), Some("'Q1, Q2'!B2"));
    assert_eq!(parts.next(), Some("SUM(C3,D4)"));
    assert_eq!(parts.next(), None);
    assert_eq!(reference_parts("").collect::<Vec<_>>(), vec![""]);
    let mut s = go_to_workbook();
    let err = s.execute("edit.goTo", json!({"reference": "A1,".repeat(GO_TO_MAX_AREAS + 1)})).unwrap_err();
    assert!(err.to_string().contains("at most"), "{err}");
    let bad = format!("Total{}", ",".repeat(1_000_000));
    assert!(s.execute("edit.goTo", json!({"reference": bad})).is_err());
}

/// "The Go to box keeps track of ranges as you enter them, and you can return to any of them."
#[test]
fn go_to_keeps_the_references_entered_newest_first() {
    let mut s = go_to_workbook();
    s.execute("edit.goTo", json!({"reference": "B14:C22,F19:G30", "record": true})).unwrap();
    s.execute("edit.goTo", json!({"reference": "Sheet2!A5", "record": true})).unwrap();
    // Names are listed already; a call without `record` (the Name Box) is not kept.
    s.execute("edit.goTo", json!({"reference": "Price", "record": true})).unwrap();
    s.execute("edit.goTo", json!({"reference": "Z9"})).unwrap();
    s.execute("edit.goTo", json!({"reference": "b14:c22,f19:g30", "record": true})).unwrap();
    let list = s.execute("edit.goTo", json!({"list": true})).unwrap();
    assert_eq!(list["recent"], json!(["Sheet1!B14:C22,Sheet1!F19:G30", "Sheet2!A5"]), "{list}");
    // Going back from Sheet2 to a recent reference returns to its sheet.
    s.execute("selection.set", json!({"sheet": 1})).unwrap();
    let out = s.execute("edit.goTo", json!({"reference": "Sheet1!B14:C22,Sheet1!F19:G30", "record": true})).unwrap();
    assert_eq!(out["selection"], "B14:C22,F19:G30", "{out}");
    assert_eq!(s.doc().unwrap().wb.active_sheet(), 0);
    for i in 0..GO_TO_RECENT + 2 {
        s.execute("edit.goTo", json!({"reference": format!("A{}", i + 1), "record": true})).unwrap();
    }
    let list = s.execute("edit.goTo", json!({"list": true})).unwrap();
    assert_eq!(list["recent"].as_array().map(Vec::len), Some(GO_TO_RECENT), "{list}");
    assert_eq!(list["recent"][0], format!("Sheet1!A{}", GO_TO_RECENT + 2), "{list}");
}

#[test]
fn cell_set_takes_numbers_for_input_and_value() {
    // #697: a JSON number for `input` used to clear the cell instead of entering it.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": 123.5})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "value": 77})).unwrap();
    s.execute("cell.set", json!({"cell": "A3", "input": true})).unwrap();
    s.execute("cell.set", json!({"cell": "A4", "input": "=A1+A2"})).unwrap();
    let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
    assert_eq!(get(&mut s, "A1")["value"], json!(123.5));
    assert_eq!(get(&mut s, "A2")["value"], json!(77.0));
    assert_eq!(get(&mut s, "A3")["value"], json!(true));
    assert_eq!(get(&mut s, "A4")["value"], json!(200.5));
    assert!(s.execute("cell.set", json!({"cell": "A5"})).is_err());
}

/// Every string in `values` went through typed input, read with en-US rules, in silence: a
/// caller sending "05.04.2026" from a dd.mm.yyyy export stored 4 May, "2.500" became 2.5 and
/// "4.25 %" became a percentage (#2255). `parse: false` stores a string as the text it is.
#[test]
fn set_values_can_store_strings_as_the_text_they_are() {
    let mut s = Session::new();
    s.new_workbook();
    let strings = ["05.04.2026", "02/11/2026", "2.500", "4.25 %", "=1+1", "'keep"];
    let row: Vec<Json> = strings.iter().map(|v| json!(v)).collect();
    let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();

    // The default is unchanged: the dates swap, the thousands separator is a decimal point.
    s.execute("range.setValues", json!({"range": "A1", "values": [row.clone()]})).unwrap();
    assert_eq!(get(&mut s, "A1")["text"], "5/4/2026", "dd.mm.yyyy read as mm.dd.yyyy");
    assert_eq!(get(&mut s, "C1")["value"], json!(2.5), "\"2.500\" read as a decimal");
    assert_eq!(get(&mut s, "D1")["value"], json!(0.0425), "\"4.25 %\" read as a percentage");

    // With `parse: false` each string is the text it was given, and a formula is still a
    // formula.
    s.execute("range.setValues", json!({"range": "A3", "parse": false, "values": [row.clone()]})).unwrap();
    for (i, want) in strings.iter().enumerate() {
        let cell = format!("{}3", (b'A' + i as u8) as char);
        let got = get(&mut s, &cell);
        if *want == "=1+1" {
            assert_eq!(got["value"], json!(2.0), "{cell} is still a formula: {got}");
            assert_eq!(got["formula"], "=1+1", "{got}");
        } else {
            assert_eq!(got["value"], json!(*want), "{cell} is the text it was given: {got}");
            assert!(got["formula"].is_null(), "{cell} is not a formula: {got}");
        }
    }
    // Numbers and booleans are untouched either way.
    s.execute("range.setValues", json!({"range": "A5", "parse": false, "values": [[12.5, true, null]]})).unwrap();
    assert_eq!(get(&mut s, "A5")["value"], json!(12.5));
    assert_eq!(get(&mut s, "B5")["value"], json!(true));
    assert_eq!(get(&mut s, "C5")["text"], "");
    // `parse` is a flag, and anything else is a usage error rather than a quiet default.
    s.execute("range.setValues", json!({"range": "A7", "parse": true, "values": [["2.500"]]})).unwrap();
    assert_eq!(get(&mut s, "A7")["value"], json!(2.5));
    assert!(s.execute("range.setValues", json!({"range": "A8", "parse": "no", "values": [["x"]]})).is_err());
    assert!(s.execute("range.setValues", json!({"range": "A8", "parse": 0, "values": [["x"]]})).is_err());
    // A line break still turns on Wrap Text.
    s.execute("range.setValues", json!({"range": "A9", "parse": false, "values": [["one\ntwo"]]})).unwrap();
    assert_eq!(get(&mut s, "A9")["value"], json!("one\ntwo"));
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert!(d.wb.styles.get(sh.style_id(CellRef::parse("A9").unwrap())).align.wrap);
}

#[test]
fn legacy_array_entry() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "B2", "values": [[1], [2]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C4", "input": "old"})).unwrap();
    s.execute("selection.set", json!({"range": "C2:C4"})).unwrap();
    s.execute("cell.set", json!({"cell": "C2", "input": "=B2:B3*2", "array": true})).unwrap();
    let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
    assert_eq!(get(&mut s, "C3")["value"], json!(4.0));
    // Past the result Excel shows #N/A; the old content of the range is replaced.
    assert_eq!(get(&mut s, "C4")["text"], "#N/A");
    let bar = s.doc().unwrap().wb.active().unwrap().cell(CellRef::parse("C2").unwrap()).map(|c| c.bar_text());
    assert_eq!(bar.as_deref(), Some("{=B2:B3*2}"));
}

#[test]
fn an_array_entered_outside_the_selection_covers_only_its_cell() {
    // Found by the command-sequence fuzzer: the array took the selection's range even with
    // `cell` elsewhere (or on another sheet), so its formula sat outside its own range and the
    // selected cells were wiped.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "K20", "input": "keep"})).unwrap();
    s.execute("selection.set", json!({"range": "K20:K22"})).unwrap();
    s.execute("cell.set", json!({"cell": "H5", "input": "=1+1", "array": true})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    let f = sh.cell(CellRef::parse("H5").unwrap()).and_then(|c| c.formula.clone()).unwrap();
    assert_eq!(f.array, Some(RangeRef::parse("H5").unwrap()));
    assert_eq!(sh.value(CellRef::parse("K20").unwrap()), Value::text("keep"));
}

#[test]
fn wildcards() {
    assert!(wildcard("hello", "h*o"));
    assert!(wildcard("hello", "h?llo"));
    assert!(!wildcard("hello", "h?lo"));
    assert!(wildcard("a*b", "a~*b"));
    assert!(!wildcard("axb", "a~*b"));
    assert!(wildcard("", "*"));
}

#[test]
fn tsv() {
    assert_eq!(parse_tsv("a\tb\n1\t2\n"), vec![vec!["a", "b"], vec!["1", "2"]]);
    assert_eq!(parse_tsv("\"x\ty\"\tz"), vec![vec!["x\ty", "z"]]);
    assert_eq!(parse_tsv("\"he said \"\"hi\"\"\""), vec![vec!["he said \"hi\""]]);
}

#[test]
fn paste_special_operations_keep_text_errors_and_formulas() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [["'123", "Total", "=NA()", "=B3*2", 5], [1, null, null, null, null], [null, 4, null, null, null]]}),
    )
    .unwrap();
    let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
    assert_eq!(get(&mut s, "A1")["value"], "123");
    s.execute("edit.copy", json!({"range": "A2"})).unwrap();
    s.execute("selection.set", json!({"range": "A1:E1"})).unwrap();
    s.execute("edit.pasteSpecial", json!({"what": "values", "operation": "multiply"})).unwrap();
    assert_eq!(get(&mut s, "A1")["value"], json!(123.0));
    assert_eq!(get(&mut s, "B1")["value"], "Total");
    assert_eq!(get(&mut s, "C1")["value"]["error"], "#N/A");
    assert_eq!(get(&mut s, "D1")["formula"], "=(B3*2)*1");
    assert_eq!(get(&mut s, "D1")["value"], json!(8.0));
    assert_eq!(get(&mut s, "E1")["value"], json!(5.0));
}

#[test]
fn cut_paste_that_overflows_the_grid_moves_nothing() {
    // #997: the source was emptied first and the cells that fell off the grid were skipped.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2]]})).unwrap();
    let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
    s.execute("edit.cut", json!({"range": "A1:A2"})).unwrap();
    // Transposed, the 2 rows become 2 columns and the second one is past XFD.
    assert!(s.execute("edit.pasteSpecial", json!({"what": "all", "transpose": true, "at": "XFD1"})).is_err());
    assert_eq!(get(&mut s, "A1")["value"], json!(1.0));
    assert_eq!(get(&mut s, "A2")["value"], json!(2.0));
    // Not transposed, the second row would be past the last row.
    assert!(s.execute("edit.pasteSpecial", json!({"what": "all", "at": "A1048576"})).is_err());
    assert_eq!(get(&mut s, "A1")["value"], json!(1.0));
    assert_eq!(get(&mut s, "A2")["value"], json!(2.0));
    // Not transposed it still fits in column XFD.
    s.execute("edit.pasteSpecial", json!({"what": "all", "at": "XFD1"})).unwrap();
    assert_eq!(get(&mut s, "XFD1")["value"], json!(1.0));
    assert_eq!(get(&mut s, "XFD2")["value"], json!(2.0));
}

#[test]
fn replace_case_insensitive() {
    assert_eq!(replace_ci("Apple apple APPLE", "apple", "pear"), "pear pear pear");
}

#[test]
fn replace_case_insensitive_keeps_text_when_folding_moves_boundaries() {
    // U+212A KELVIN SIGN folds to `k` (3→1 bytes) while U+0130 folds to `i` plus a
    // combining dot (2→3 bytes): the folded text is the same length but its byte positions
    // no longer line up with the original, and the old slicing dropped the tail.
    let mut s = Session::new();
    s.new_workbook();
    let text = "\u{212A}\u{0130}\u{0130}";
    s.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    s.execute("edit.replace", json!({"what": "k", "with": "x"})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::text(text));
}
