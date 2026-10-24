use gridcraft_model::*;
use serde_json::json;

use super::*;

fn sample_macro(name: &str) -> Macro {
    parse_macro(&json!({
        "name": name,
        "description": "Totals the amounts",
        "shortcut": "Ctrl+Shift+T",
        "inputs": [{"name": "Input", "rows": [1, 500], "cols": [2, 2], "header": true, "colTypes": ["text", "number"], "nonEmpty": true}],
        "steps": [
            {"type": "command", "id": "cell.set", "params": {"cell": "Output", "input": "=SUM({{Input.col(2)}})"}, "breakpoint": true, "note": "the total"},
            {"type": "repeat", "over": "Input.rows(2..)", "of": "rows", "max": 100, "body": [
                {"type": "if", "cell": "Row.cell(1,2)", "op": "gt", "value": 100, "then": [
                    {"command": "home.bold", "params": {"range": "Row"}}
                ], "else": [{"type": "stop", "message": "done"}]}
            ]},
            {"type": "command", "id": "home.italic", "enabled": false}
        ],
        "output": {"anchor": "Input.offset(0,3)", "onConflict": "shiftDown"},
        "permissions": ["data.getData"]
    }))
    .unwrap()
}

#[test]
fn macros_read_from_json_in_both_step_forms() {
    let m = sample_macro("Totals");
    assert_eq!(m.v, MACRO_VERSION);
    assert_eq!(m.inputs[0].col_types, [ColType::Text, ColType::Number]);
    assert_eq!(m.output.on_conflict, OnConflict::ShiftDown);
    assert_eq!(m.step_count(), 6);
    assert!(m.steps[0].breakpoint && m.steps[0].enabled && !m.steps[2].enabled);
    let StepKind::Repeat { body, max, .. } = &m.steps[1].kind else { panic!("{:?}", m.steps[1]) };
    assert_eq!(*max, 100);
    let StepKind::If { then, otherwise, op, .. } = &body[0].kind else { panic!() };
    assert_eq!(*op, CompareOp::Gt);
    // The short form of earlier scripts reads as a command step.
    assert_eq!(then[0].kind, StepKind::Command { id: "home.bold".into(), params: json!({"range": "Row"}) });
    assert!(matches!(&otherwise[0].kind, StepKind::Stop { message, rollback: false } if message == "done"));
    // Writing and reading again gives the same macro; defaults stay out of the JSON.
    let v = macro_json(&m);
    assert_eq!(v["steps"][0]["type"], "command");
    assert!(v["steps"][1].get("enabled").is_none() && v["steps"][1].get("breakpoint").is_none());
    assert_eq!(v["steps"][2]["enabled"], false);
    assert_eq!(parse_macro(&v).unwrap(), m);
    // A minimal macro gets the defaults.
    let m = parse_macro(&json!({"name": " Bare "})).unwrap();
    assert_eq!((m.name.as_str(), m.output.on_conflict, m.inputs.len()), ("Bare", OnConflict::Ask, 0));
}

#[test]
fn macros_over_the_limits_are_refused() {
    let flat = |n: usize| json!({"name": "Big", "steps": vec![json!({"command": "cell.set"}); n]});
    assert!(parse_macro(&flat(MAX_MACRO_STEPS)).is_ok());
    assert!(parse_macro(&flat(MAX_MACRO_STEPS + 1)).unwrap_err().contains("10000 steps"));
    // Nested Repeat steps: 4 Repeats one inside the other are fine (the innermost holds a
    // command, or nothing), a 5th is not.
    let nest = |repeats: usize| {
        let mut step = json!({"command": "cell.set"});
        for _ in 0..repeats {
            step = json!({"type": "repeat", "over": "Input", "body": [step]});
        }
        json!({"name": "Deep", "steps": [step]})
    };
    assert!(parse_macro(&nest(MAX_MACRO_DEPTH)).is_ok());
    assert_eq!(parse_macro(&nest(MAX_MACRO_DEPTH)).unwrap().step_count(), MAX_MACRO_DEPTH + 1);
    assert!(parse_macro(&nest(MAX_MACRO_DEPTH + 1)).unwrap_err().contains("4 deep"));
    let mut empty = json!({"type": "repeat", "over": "Input", "body": []});
    for _ in 1..MAX_MACRO_DEPTH {
        empty = json!({"type": "if", "cell": "A1", "op": "eq", "value": 1, "then": [empty], "else": []});
    }
    let deep = parse_macro(&json!({"name": "Deep", "steps": [empty]})).unwrap();
    assert_eq!(deep.step_count(), MAX_MACRO_DEPTH);
    assert!(deep.check_limits().is_ok(), "the model's check agrees with the parser");
    // Far deeper than serde would allow is refused before it is read.
    assert!(parse_macro(&nest(200)).is_err());
    for bad in [
        json!({"name": ""}),
        json!({"name": "x".repeat(256)}),
        json!({"name": "a\nb"}),
        json!({"name": "A", "steps": {}}),
        json!({"name": "A", "steps": [1]}),
        json!({"name": "A", "steps": [{"type": "nope"}]}),
        json!({"name": "A", "steps": [{"type": "if", "cell": "A1", "op": "between"}]}),
        json!({"name": "A", "inputs": [{"name": ""}]}),
        json!({"name": "A", "inputs": [{"name": "Has space"}]}),
        json!({"name": "A", "inputs": [{"name": "row"}]}),
        json!({"name": "A", "inputs": [{"name": "In"}, {"name": "IN"}]}),
        json!({"name": "A", "output": {"onConflict": "explode"}}),
        json!([]),
    ] {
        assert!(parse_macro(&bad).is_err(), "{bad}");
    }
}

fn names_of(wb: &Workbook) -> Vec<String> {
    wb.names.iter().map(|n| n.name.clone()).collect()
}

#[test]
fn macros_round_trip_through_xlsx_and_json() {
    let mut wb = Workbook::new();
    wb.macros.push(sample_macro("Totals"));
    // Large enough to need several hidden names.
    let long: Vec<Step> = (0..300).map(|r| Step::command("cell.set", json!({"cell": format!("A{r}"), "input": "x\"y".repeat(10)}))).collect();
    wb.macros.push(Macro { name: "Long".into(), steps: long, ..Macro::default() });
    for file in ["book.xlsx", "book.json"] {
        let bytes = crate::io::save_bytes(&wb, file).unwrap();
        let (back, warnings) = crate::io::open_bytes(file, &bytes).unwrap();
        assert!(warnings.is_empty(), "{file}: {warnings:?}");
        assert_eq!(back.macros, wb.macros, "{file}");
        assert!(back.names.is_empty(), "{file}: macros stay out of the names once opened: {:?}", names_of(&back));
    }
    // In the xlsx file they are hidden names; the long one spans several.
    let bytes = crate::io::save_bytes(&wb, "book.xlsx").unwrap();
    let (raw, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let names = names_of(&raw);
    assert!(names.contains(&"_gc_macro_0_0".to_string()), "{names:?}");
    assert!(names.iter().filter(|n| n.starts_with("_gc_macro_1_")).count() >= 2, "{names:?}");
    assert!(raw.names.iter().all(|n| n.hidden));
    // Saving without macros writes no macro names, even if stale ones linger.
    let mut empty = raw.clone();
    empty.macros.clear();
    let (again, _) = gridcraft_xlsx::read_xlsx(&crate::io::save_bytes(&empty, "x.xlsx").unwrap()).unwrap();
    assert!(again.names.iter().all(|n| !n.name.starts_with("_gc_macro_")));
    // The JSON format keeps the field (and leaves it out when there are no macros).
    let json = String::from_utf8(crate::io::save_bytes(&wb, "book.json").unwrap()).unwrap();
    assert!(json.contains("\"macros\""));
    let plain = String::from_utf8(crate::io::save_bytes(&Workbook::new(), "book.json").unwrap()).unwrap();
    assert!(!plain.contains("\"macros\""));
}

fn hidden(name: &str, text: &str) -> DefinedName {
    DefinedName { name: name.into(), scope: None, formula: format!("\"{}\"", text.replace('"', "\"\"")), comment: String::new(), hidden: true }
}

#[test]
fn a_visible_name_that_looks_like_macro_storage_is_left_alone() {
    // Macro storage is hidden workbook-scope names; a visible `_gc_macro_0_0` someone defined
    // is neither read as a macro nor removed when the workbook opens.
    let mut wb = Workbook::new();
    let name = DefinedName { hidden: false, ..hidden("_gc_macro_0_0", "x") };
    wb.names.push(name.clone());
    let warnings = load(&mut wb);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(wb.macros.is_empty());
    assert_eq!(wb.names, vec![name]);
}

#[test]
fn input_names_that_read_as_cell_references_are_refused() {
    // A recorded step holds plain A1 text; an input called `Q1` would take its place.
    let with_input = |name: &str| json!({"name": "M", "inputs": [{"name": name}]});
    for name in ["Q1", "AB12", "Sum1", "a1"] {
        let err = parse_macro(&with_input(name)).unwrap_err();
        assert!(err.contains("reads as a cell reference"), "{name}: {err}");
    }
    for name in ["Amount", "Q", "Sales_Q1", "Total1a"] {
        assert!(parse_macro(&with_input(name)).is_ok(), "{name}");
    }
    assert!(parse_macro(&with_input("Row")).unwrap_err().contains("token of its own"));
}

#[test]
fn a_hidden_macro_name_this_build_did_not_write_is_kept() {
    // `_gc_macro_index_0` is not a numbered item: it is neither read nor dropped on open, and a
    // save with macros keeps it beside the items written.
    let mut wb = Workbook::new();
    let theirs = hidden("_gc_macro_index_0", "{}");
    wb.names.push(theirs.clone());
    wb.macros.push(sample_macro("Totals"));
    let (raw, _) = gridcraft_xlsx::read_xlsx(&crate::io::save_bytes(&wb, "book.xlsx").unwrap()).unwrap();
    let names = names_of(&raw);
    assert!(names.contains(&"_gc_macro_index_0".to_string()) && names.contains(&"_gc_macro_0_0".to_string()), "{names:?}");
    let (back, warnings) = crate::io::open_bytes("book.xlsx", &crate::io::save_bytes(&wb, "book.xlsx").unwrap()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back.macros, wb.macros);
    assert_eq!(names_of(&back), ["_gc_macro_index_0"]);
}

#[test]
fn workbook_snapshots_share_the_macro_list_until_it_changes() {
    // An undo snapshot or a copy made for a cell edit holds the same list, not a copy of every
    // step; the undo budget charges a list only once it is a snapshot's own.
    let mut wb = Workbook::new();
    wb.macros.push(sample_macro("Totals"));
    let snapshot = wb.clone();
    assert!(wb.macros.shares(&snapshot.macros));
    assert_eq!(crate::history_retained_bytes(&snapshot, &wb), std::mem::size_of::<Workbook>());
    wb.macros.push(sample_macro("More"));
    assert!(!wb.macros.shares(&snapshot.macros), "a write copies the list first");
    assert_eq!(snapshot.macros.len(), 1);
    assert!(crate::history_retained_bytes(&snapshot, &wb) > std::mem::size_of::<Workbook>());
    // The copy is per list, not per workbook copy: cloning again shares the new list.
    let again = wb.clone();
    assert!(wb.macros.shares(&again.macros));
    let rebuilt: Macros = wb.macros.iter().cloned().collect();
    assert_eq!(rebuilt, wb.macros);
    assert_eq!(rebuilt.into_iter().count(), 2);
}

#[test]
fn a_macro_built_in_code_goes_through_the_size_check_too() {
    // `check` is the one check for every path, so a macro that passes it never makes a save fail.
    let big = Macro {
        name: "Big".into(),
        steps: vec![Step::command("cell.set", json!({"input": "x".repeat(crate::cmd::stash::MAX_ITEM_CHARS)}))],
        ..Macro::default()
    };
    assert!(big.check_limits().is_ok(), "the model's limits say nothing about the size");
    assert!(check(&big).unwrap_err().contains("too large"));
    let mut wb = Workbook::new();
    wb.macros.push(big);
    assert!(crate::io::save_bytes(&wb, "book.xlsx").is_err(), "a macro that fails `check` fails the save");
}

#[test]
fn a_save_refuses_a_clashing_visible_name_before_changing_anything() {
    let mut wb = Workbook::new();
    wb.macros.push(sample_macro("A"));
    wb.macros.push(sample_macro("B"));
    let own = DefinedName { hidden: false, ..hidden("_GC_MACRO_1_0", "mine") };
    wb.names.push(own.clone());
    wb.names.push(hidden("_gc_macro_7_0", "stale"));
    let err = crate::io::save_bytes(&wb, "book.xlsx").unwrap_err().to_string();
    assert!(err.contains("_GC_MACRO_1_0"), "{err}");
    assert_eq!(wb.names.len(), 2, "nothing changed");
    wb.names.remove(0);
    let (raw, _) = gridcraft_xlsx::read_xlsx(&crate::io::save_bytes(&wb, "book.xlsx").unwrap()).unwrap();
    let names = names_of(&raw);
    assert_eq!(names, ["_gc_macro_0_0", "_gc_macro_1_0"], "the stale item went in the same pass");
}

#[test]
fn scripts_and_vba_stay_as_they_are() {
    // Automate scripts keep their own storage, and a VBA project is carried through unchanged.
    let mut wb = Workbook::new();
    let scripts = json!({"Fill": [{"command": "cell.set", "params": {"cell": "A1", "input": "x"}}]});
    wb.names.push(hidden("_sc_scripts", &scripts.to_string()));
    wb.vba_project = Some(std::sync::Arc::new(b"\xd0\xcf\x11\xe0 synthetic vbaProject".to_vec()));
    wb.macros.push(sample_macro("Totals"));
    let bytes = crate::io::save_bytes(&wb, "book.xlsm").unwrap();
    let (back, warnings) = crate::io::open_bytes("book.xlsm", &bytes).unwrap();
    // The only warning is the reader's note that the VBA project is kept and not run.
    assert!(warnings.iter().all(|w| w.contains("VBA project")), "{warnings:?}");
    assert_eq!(back.macros, wb.macros);
    assert_eq!(names_of(&back), ["_sc_scripts"], "the scripts item is untouched");
    assert_eq!(back.names[0].formula, wb.names[0].formula);
    assert_eq!(back.vba_project, wb.vba_project);
}

#[test]
fn damaged_or_oversized_macros_are_dropped_with_a_warning() {
    let mut wb = Workbook::new();
    wb.names.push(hidden("_gc_macro_0_0", &json!({"name": "Good"}).to_string()));
    wb.names.push(hidden("_gc_macro_1_0", "{not json"));
    wb.names.push(hidden("_gc_macro_2_0", &json!({"name": "Huge", "steps": vec![json!({"command": "x"}); MAX_MACRO_STEPS + 1]}).to_string()));
    wb.names.push(hidden("_gc_macro_3_0", &json!({"name": "good"}).to_string()));
    wb.names.push(hidden("_gc_macro_10_0", &json!({"name": "Tenth"}).to_string()));
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let (back, warnings) = crate::io::open_bytes("hostile.xlsx", &bytes).unwrap();
    let names: Vec<&str> = back.macros.iter().map(|m| m.name.as_str()).collect();
    // Saved order is by number (10 after 3); a clashing name gets a suffix.
    assert_eq!(names, ["Good", "good (2)", "Tenth"]);
    assert!(back.names.is_empty(), "{:?}", names_of(&back));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("2 macros"), "{warnings:?}");

    // The JSON format: more than the limit, and one over the step limit.
    let mut wb = Workbook::new();
    wb.macros = (0..MAX_MACROS + 5).map(|i| Macro { name: format!("M{i}"), ..Macro::default() }).collect();
    wb.macros[0].steps = vec![Step::command("x", json!({})); MAX_MACRO_STEPS + 1];
    let bytes = crate::io::save_bytes(&wb, "many.json").unwrap();
    let (back, warnings) = crate::io::open_bytes("many.json", &bytes).unwrap();
    // Only the first MAX_MACROS entries are read, as in xlsx.
    assert_eq!(back.macros.len(), MAX_MACROS - 1);
    assert_eq!(back.macros[0].name, "M1");
    assert!(warnings[0].starts_with("6 macros"), "{warnings:?}");

    // A macro too large for the hidden names is refused, so a save never fails on it.
    let huge = "x".repeat(crate::cmd::stash::MAX_ITEM_CHARS);
    let err = parse_macro(&json!({"name": "Huge", "steps": [{"command": "cell.set", "params": {"input": huge}}]})).unwrap_err();
    assert!(err.contains("too large"), "{err}");
    let mut wb = Workbook::new();
    wb.macros.push(Macro { name: "Huge".into(), steps: vec![Step::command("cell.set", json!({"input": huge}))], ..Macro::default() });
    let bytes = crate::io::save_bytes(&wb, "huge.json").unwrap();
    let (back, warnings) = crate::io::open_bytes("huge.json", &bytes).unwrap();
    assert!(back.macros.is_empty() && warnings[0].starts_with("1 macro "), "{warnings:?}");
}

#[test]
fn names() {
    let mut wb = Workbook::new();
    wb.macros.push(Macro { name: "Macro1".into(), ..Macro::default() });
    assert_eq!(unique_name(&wb, "macro1"), "macro1 (2)");
    assert_eq!(unique_name(&wb, "Other"), "Other");
    assert_eq!(unique_name(&wb, ""), "Macro");
    // A long name is shortened to make room for the suffix.
    let long = "y".repeat(MAX_MACRO_NAME);
    wb.macros.push(Macro { name: long.clone(), ..Macro::default() });
    let renamed = unique_name(&wb, &long);
    assert_eq!(renamed, format!("{} (2)", "y".repeat(MAX_MACRO_NAME - 4)));
    wb.macros.push(Macro { name: renamed, ..Macro::default() });
    assert_eq!(unique_name(&wb, &long).chars().count(), MAX_MACRO_NAME);
    assert_eq!(find(&wb, " MACRO1 "), Some(0));
    assert_eq!(find(&wb, "Macro2"), None);
}

#[test]
fn json_field_macros_get_the_name_check_too() {
    // A JSON workbook can't bring in a name that macro.save and macro.rename refuse.
    let mut wb = Workbook::new();
    wb.macros.push(Macro { name: "a\nb".into(), ..Macro::default() });
    wb.macros.push(Macro { name: "Fine".into(), ..Macro::default() });
    let bytes = crate::io::save_bytes(&wb, "ctl.json").unwrap();
    let (back, warnings) = crate::io::open_bytes("ctl.json", &bytes).unwrap();
    let names: Vec<&str> = back.macros.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Fine"]);
    assert!(warnings.len() == 1 && warnings[0].starts_with("1 macro "), "{warnings:?}");
}

#[test]
fn duplicate_names_at_the_length_limit_survive_save_and_reopen() {
    // Two macros with the same 255-character name: the second is renamed within the limit, so a
    // save and a second open keep both.
    let long = "é".repeat(MAX_MACRO_NAME);
    for file in ["dup.json", "dup.xlsx"] {
        let mut wb = Workbook::new();
        wb.macros.push(Macro { name: long.clone(), ..Macro::default() });
        wb.macros.push(Macro { name: long.clone(), description: "second".into(), ..Macro::default() });
        let (first, warnings) = crate::io::open_bytes(file, &crate::io::save_bytes(&wb, file).unwrap()).unwrap();
        assert!(warnings.is_empty(), "{file}: {warnings:?}");
        assert_eq!(first.macros.len(), 2, "{file}");
        let renamed = &first.macros[1].name;
        assert_eq!(renamed.chars().count(), MAX_MACRO_NAME, "{file}");
        assert!(renamed.ends_with(" (2)") && renamed.starts_with("éé"), "{file}: {renamed}");
        let (again, warnings) = crate::io::open_bytes(file, &crate::io::save_bytes(&first, file).unwrap()).unwrap();
        assert!(warnings.is_empty(), "{file}: {warnings:?}");
        assert_eq!(again.macros, first.macros, "{file}");
    }
}

#[test]
#[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
fn many_hidden_macro_names_are_read_in_one_pass() {
    // A hostile file with many damaged macro items: only the first MAX_MACROS are read, and the
    // names are gathered and removed in one pass instead of one scan of every name per item.
    let mut wb = Workbook::new();
    let items = 20_000;
    for i in 0..items {
        wb.names.push(hidden(&format!("_gc_macro_{i}_0"), "{not json"));
    }
    wb.names.push(hidden("_gc_other_0", "kept"));
    let start = std::time::Instant::now();
    let warnings = load(&mut wb);
    let took = start.elapsed();
    assert!(wb.macros.is_empty());
    assert_eq!(names_of(&wb), ["_gc_other_0"]);
    assert!(warnings.len() == 1 && warnings[0].starts_with(&format!("{items} macros")), "{warnings:?}");
    assert!(took < std::time::Duration::from_secs(3), "{took:?}");
}

#[test]
fn a_damaged_json_macro_drops_alone() {
    // One macro this build can't read (a newer step type, an unknown value, the wrong shape) is
    // dropped with the usual warning; the workbook and its other macros open.
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().set_value(gridcraft_core::CellRef::new(0, 0), gridcraft_core::Value::text("kept"));
    let mut v: serde_json::Value = serde_json::from_slice(&crate::io::save_bytes(&wb, "book.json").unwrap()).unwrap();
    v["macros"] = json!([
        {"name": "Good"},
        {"name": "Newer", "steps": [{"type": "fromALaterBuild"}]},
        {"name": "Odd", "output": {"onConflict": "explode"}},
        {"name": "Short", "steps": [{"command": "cell.set", "params": {"cell": "A1", "input": "x"}}]},
        "not a macro"
    ]);
    let (back, warnings) = crate::io::open_bytes("book.json", v.to_string().as_bytes()).unwrap();
    let names: Vec<&str> = back.macros.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Good", "Short"]);
    assert!(matches!(&back.macros[1].steps[0].kind, StepKind::Command { id, .. } if id == "cell.set"));
    assert!(back.macros_read.is_empty());
    assert!(warnings.len() == 1 && warnings[0].starts_with("3 macros"), "{warnings:?}");
    assert_eq!(back.sheet(0).unwrap().value(gridcraft_core::CellRef::new(0, 0)), gridcraft_core::Value::text("kept"));
}

#[test]
fn a_macro_in_a_newer_format_is_dropped_not_rewritten() {
    // A newer build's macro (`v` 2, with a field this build doesn't know) would lose that field
    // here and be saved back as if complete, so it is dropped with the usual warning instead.
    // Version 1 and a missing version are read as before.
    let future = json!({"name": "Future", "v": MACRO_VERSION + 1, "fromALaterBuild": {"x": 1}, "steps": [{"command": "cell.set"}]});
    let current = json!({"name": "Current", "v": MACRO_VERSION, "steps": [{"command": "cell.set"}]});
    let missing = json!({"name": "Missing"});
    let err = parse_macro(&future).unwrap_err();
    assert!(err.contains("newer version") && err.contains("format 2"), "{err}");
    assert!(parse_macro(&json!({"name": "Huge", "v": u64::from(u32::MAX) + 1})).unwrap_err().contains("newer version"));
    // A macro built in code goes through the same check on every path.
    assert!(Macro { name: "Built".into(), v: MACRO_VERSION + 1, ..Macro::default() }.check_limits().is_err());
    let check = |file: &str, back: &Workbook, warnings: &[String]| {
        let names: Vec<&str> = back.macros.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Current", "Missing"], "{file}");
        assert!(back.macros.iter().all(|m| m.v == MACRO_VERSION), "{file}");
        assert!(warnings.len() == 1 && warnings[0].starts_with("1 macro ") && warnings[0].contains("newer version"), "{file}: {warnings:?}");
        // Saving and opening again leaves no reduced copy of it behind.
        let (again, warnings) = crate::io::open_bytes(file, &crate::io::save_bytes(back, file).unwrap()).unwrap();
        assert!(warnings.is_empty(), "{file}: {warnings:?}");
        assert_eq!(again.macros, back.macros, "{file}");
    };
    // The JSON format's field.
    let mut v: serde_json::Value = serde_json::from_slice(&crate::io::save_bytes(&Workbook::new(), "book.json").unwrap()).unwrap();
    v["macros"] = json!([future, current, missing]);
    let (back, warnings) = crate::io::open_bytes("book.json", v.to_string().as_bytes()).unwrap();
    check("book.json", &back, &warnings);
    // Hidden names in xlsx.
    let mut wb = Workbook::new();
    for (i, m) in [&future, &current, &missing].into_iter().enumerate() {
        wb.names.push(hidden(&format!("_gc_macro_{i}_0"), &m.to_string()));
    }
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let (back, warnings) = crate::io::open_bytes("book.xlsx", &bytes).unwrap();
    assert!(back.names.is_empty(), "{:?}", names_of(&back));
    check("book.xlsx", &back, &warnings);
}

#[test]
fn many_macros_with_one_name_all_survive_save_and_reopen() {
    // MAX_MACROS macros with the same name, at the length limit, so the suffixes run from " (2)"
    // to " (1000)" and the kept part of the name shrinks as they widen.
    let long = "é".repeat(MAX_MACRO_NAME);
    for file in ["same.json", "same.xlsx"] {
        let mut wb = Workbook::new();
        wb.macros = (0..MAX_MACROS).map(|i| Macro { name: long.clone(), description: format!("{i}"), ..Macro::default() }).collect();
        let (first, warnings) = crate::io::open_bytes(file, &crate::io::save_bytes(&wb, file).unwrap()).unwrap();
        assert!(warnings.is_empty(), "{file}: {warnings:?}");
        assert_eq!(first.macros.len(), MAX_MACROS, "{file}");
        let names: std::collections::HashSet<String> = first.macros.iter().map(|m| m.name.to_lowercase()).collect();
        assert_eq!(names.len(), MAX_MACROS, "{file}: every name is different");
        assert!(first.macros.iter().all(|m| m.name.chars().count() <= MAX_MACRO_NAME && m.check_limits().is_ok()), "{file}");
        assert_eq!(first.macros[0].name, long);
        assert_eq!(first.macros[9].name, format!("{} (10)", "é".repeat(MAX_MACRO_NAME - 5)));
        assert_eq!(first.macros[MAX_MACROS - 1].name, format!("{} (1000)", "é".repeat(MAX_MACRO_NAME - 7)));
        assert!(first.macros.iter().enumerate().all(|(i, m)| m.description == format!("{i}")), "{file}: order kept");
        let (again, warnings) = crate::io::open_bytes(file, &crate::io::save_bytes(&first, file).unwrap()).unwrap();
        assert!(warnings.is_empty(), "{file}: {warnings:?}");
        assert_eq!(again.macros, first.macros, "{file}");
    }
}

#[test]
fn naming_clashing_macros_takes_linear_work() {
    // Each suffix is looked up about once, not once per earlier macro with the same name.
    let mut taken = Taken::default();
    // Names a file may already use, in the way of the first suffixes.
    for n in 2..=500 {
        taken.insert(&format!("Same ({n})"));
    }
    let mut out = Vec::new();
    for _ in 0..MAX_MACROS {
        let name = taken.unique("SAME");
        taken.insert(&name);
        out.push(name);
    }
    assert_eq!(out[..3], ["SAME", "SAME (501)", "SAME (502)"]);
    assert!(taken.probes <= 3 * MAX_MACROS, "{} lookups", taken.probes);
    // The smallest free suffix is still the one used.
    let mut taken = Taken::default();
    for name in ["A", "A (3)"] {
        taken.insert(name);
    }
    assert_eq!(taken.unique("a"), "a (2)");
    taken.insert("a (2)");
    assert_eq!(taken.unique("a"), "a (4)");
}
