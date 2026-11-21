//! Recalculation after a hidden name is rewritten without a structural edit (custom views, the
//! Watch Window list): only the formulas reading the name are recalculated.

use super::*;

#[test]
fn rewriting_a_hidden_name_recalculates_only_its_readers() {
    // Saving a custom view rewrites a hidden name without a structural edit. The formulas that
    // read that name, directly or through another name, are recalculated; the rest of the
    // workbook isn't (a full recalculation of a 50K-formula workbook for a saved view would
    // take as long as opening it).
    let mut s = Session::new();
    s.new_workbook();
    for r in 1..=50 {
        s.execute("cell.set", json!({"cell": format!("B{r}"), "input": format!("=ROW()+{r}")})).unwrap();
    }
    // The name two steps away is defined before the name it reads: it is found through which
    // names mention which, not by the order the names are listed in.
    s.execute("formulas.defineName", json!({"name": "Twice_removed", "refersTo": "=Alias+1"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Alias", "refersTo": "=LEN(_sc_custom_views)"})).unwrap();
    s.execute("view.customViews", json!({"save": "first"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_custom_views)"})).unwrap();
    s.execute("cell.set", json!({"cell": "D1", "input": "=Alias"})).unwrap();
    s.execute("cell.set", json!({"cell": "E1", "input": "=Twice_removed"})).unwrap();
    // A LAMBDA reading the name, called from a cell and from another name's definition.
    s.execute("formulas.defineName", json!({"name": "View_length", "refersTo": "=LAMBDA(extra,LEN(_sc_custom_views)+extra)"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Called_length", "refersTo": "=View_length(1)"})).unwrap();
    s.execute("cell.set", json!({"cell": "F1", "input": "=View_length(0)"})).unwrap();
    s.execute("cell.set", json!({"cell": "G1", "input": "=Called_length"})).unwrap();
    let len = |s: &Session, at: &str| match s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(at).unwrap()) {
        Value::Number(n) => n,
        other => panic!("{at}: expected a length, got {other:?}"),
    };
    let length_before = len(&s, "A1");
    assert!(length_before > 0.0);
    assert_eq!((len(&s, "D1"), len(&s, "E1")), (length_before, length_before + 1.0));
    assert_eq!((len(&s, "F1"), len(&s, "G1")), (length_before, length_before + 1.0));
    // The length of the text the hidden name now holds: its definition is a quoted string with
    // inner quotes doubled.
    let stored_length = |s: &Session| {
        let wb = &s.doc().unwrap().wb;
        let definition = &wb.names.iter().find(|n| n.name == "_sc_custom_views").unwrap().formula;
        let text = definition.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap().replace("\"\"", "\"");
        text.chars().count() as f64
    };
    assert_eq!(stored_length(&s), length_before, "the length is read from the stored definition");
    s.execute("view.customViews", json!({"save": "second view with a longer name"})).unwrap();
    let length_after = stored_length(&s);
    assert!(length_after > length_before, "the second view lengthens the stored text");
    assert_eq!(len(&s, "A1"), length_after, "the direct reader sees the new definition");
    assert_eq!(len(&s, "D1"), length_after, "the reader through another name sees it too");
    assert_eq!(len(&s, "E1"), length_after + 1.0, "and the reader two names away");
    assert_eq!(len(&s, "F1"), len(&s, "A1"), "the cell calling a LAMBDA that reads it");
    assert_eq!(len(&s, "G1"), len(&s, "A1") + 1.0, "and the name calling it");
    assert_eq!(s.doc().unwrap().calc.last_recalc_cells, 5, "only the five readers were recalculated");
}

#[test]
fn a_formula_reading_the_watch_list_name_follows_the_list() {
    // The list is kept in a hidden defined name and changed without an undo step; a formula
    // that reads that name is recalculated like the reader of any other name. The formula is
    // entered once the name exists, so it reads the name directly (one naming a name that doesn't
    // exist yet is dynamic and recalculated anyway) and the search for the name's readers must
    // find it.
    let mut s = s();
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number("[\"Sheet1!B1\"]".len() as f64));
    let steps = s.doc().unwrap().undo.len();
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0));
    assert_eq!(s.doc().unwrap().undo.len(), steps, "no undo step for the watch list");
}

#[test]
fn a_watch_list_change_that_recalculates_a_reader_moves_the_revision() {
    // Front ends cache what they show per revision: the PivotTable Fields pane keeps the
    // `pivot.fields` answer per (document, revision, pivot). A header that reads the watch list's
    // name changes its field's name when the list changes, so the revision moves with it. A list
    // change that no formula reads leaves the revision alone. The header is entered once the
    // name exists (an empty list, after a first add and remove), so it reads the name directly
    // and the search for the name's readers must find it.
    let mut s = s();
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=IF(LEN(_sc_watch)>2,\"Watched\",\"Plain\")"})).unwrap();
    s.execute("range.setValues", json!({"range": "B1", "values": [["Units"], [10], [5]]})).unwrap();
    s.execute("range.setValues", json!({"range": "A2", "values": [["East"], ["West"]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s.execute("insert.pivotTable", json!({})).unwrap();
    let names = |s: &mut Session| -> Vec<serde_json::Value> {
        s.execute("pivot.fields", json!({})).unwrap()["fields"].as_array().unwrap().iter().map(|f| f["name"].clone()).collect()
    };
    assert_eq!(names(&mut s), [json!("Plain"), json!("Units")]);
    let revision = s.doc().unwrap().revision;
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    assert_eq!(names(&mut s), [json!("Watched"), json!("Units")]);
    assert!(s.doc().unwrap().revision > revision, "the header's new name comes with a new revision");

    let mut unread = self::s();
    unread.execute("cell.set", json!({"cell": "A1", "input": "=1+1"})).unwrap();
    let revision = unread.doc().unwrap().revision;
    unread.execute("formulas.watchWindow", json!({"add": "Sheet1!A1"})).unwrap();
    assert_eq!(unread.doc().unwrap().revision, revision, "no reader, no new revision");
}

#[test]
fn undo_and_redo_keep_the_watch_list_and_recalculate_its_readers() {
    // Undo and redo restore an earlier workbook but carry the current watch list into it. The
    // restored workbook was calculated with its own list, so a formula reading the list's name
    // must be recalculated for the carried one; nothing else depends on the name.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "1"})).unwrap();
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    let one = Value::Number("[\"Sheet1!B1\"]".len() as f64);
    assert_eq!(v(&s, "A1"), one);
    // The workbook before B1 was set had no watch list and A1 = #NAME?.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), one, "the carried list is read after undo");
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0));
    // The workbook redo restores had the one-entry list and A1 = 13.
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0), "the carried list is read after redo");
}

/// Formulas that reach the watch list's name only through INDIRECT's text: directly, through a
/// name defined as `=INDIRECT("_sc_watch")`, and through a LAMBDA name given the name as text.
const WATCH_TEXT_READERS: [(&str, &str); 3] = [
    ("D1", "=IFERROR(ROWS(INDIRECT(\"_sc_watch\")),\"text\")"),
    ("E1", "=IFERROR(ROWS(Watch_alias),\"text\")"),
    ("F1", "=Watch_rows(\"_sc_watch\")"),
];

/// A session with the three readers of [`WATCH_TEXT_READERS`] over five values in A1:A5.
fn watch_text_session(manual: bool) -> Session {
    let mut s = s();
    if manual {
        s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    }
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3], [4], [5]]})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Watch_alias", "refersTo": "=INDIRECT(\"_sc_watch\")"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Watch_rows", "refersTo": "=LAMBDA(label,IFERROR(ROWS(INDIRECT(label)),\"text\"))"})).unwrap();
    for (at, input) in WATCH_TEXT_READERS {
        s.execute("cell.set", json!({"cell": at, "input": input})).unwrap();
    }
    s
}

/// What the readers show.
fn watch_text_shown(s: &Session) -> Vec<Value> {
    WATCH_TEXT_READERS.iter().map(|(at, _)| v(s, at)).collect()
}

/// Makes the hidden name refer to A1:A5, as a file could carry it, without an undo step, and
/// calculates: the readers count five rows.
fn carry_watch_range(s: &mut Session) {
    {
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        wb.names.retain(|n| n.name != "_sc_watch");
        wb.names.push(gridcraft_model::DefinedName {
            name: "_sc_watch".into(),
            scope: None,
            formula: "Sheet1!$A$1:$A$5".into(),
            comment: String::new(),
            hidden: true,
        });
    }
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(watch_text_shown(s), vec![Value::Number(5.0); 3]);
}

#[test]
fn a_watch_list_change_recalculates_the_formulas_reading_its_name_through_indirect() {
    // The watch list's name holding a range (as a file could carry it) is read by formulas that
    // reach it only through INDIRECT's text, so none of them mentions the name. They are dynamic,
    // and a Watch Window change, or the list undo and redo carry into the workbook they restore,
    // recalculates them as a calculation of everything would, and moves the revision.
    // Each step shows `expected`, what a full calculation gives; the workbook an undo or redo
    // restores holds the other value, calculated with its own list.
    let step = |s: &mut Session, cmd: &str, params: serde_json::Value, expected: &[Value]| {
        let revision = s.doc().unwrap().revision;
        s.execute(cmd, params.clone()).unwrap();
        let shown = watch_text_shown(s);
        assert_eq!(shown, expected, "{cmd} {params}");
        assert!(s.doc().unwrap().revision > revision, "{cmd} {params}: the revision moves");
        s.execute("formulas.calculateNow", json!({})).unwrap();
        assert_eq!(shown, watch_text_shown(s), "{cmd} {params}: the readers show what a full calculation gives");
    };
    let five = vec![Value::Number(5.0); 3];
    let text = vec![Value::text("text"); 3];
    // The list replaces the range, and undo carries it into a workbook that still held the range.
    let mut s = watch_text_session(false);
    carry_watch_range(&mut s);
    s.execute("cell.set", json!({"cell": "Y1", "input": "1"})).unwrap();
    step(&mut s, "formulas.watchWindow", json!({"add": "Sheet1!B1"}), &text);
    step(&mut s, "edit.undo", json!({}), &text);
    // Undo carries the range into a workbook without the name; removing an entry replaces the
    // range by the list, and redo carries that list into a workbook that held the range.
    let mut s = watch_text_session(false);
    s.execute("cell.set", json!({"cell": "Y1", "input": "1"})).unwrap();
    carry_watch_range(&mut s);
    step(&mut s, "edit.undo", json!({}), &five);
    step(&mut s, "formulas.watchWindow", json!({"remove": "Sheet1!B1"}), &text);
    step(&mut s, "edit.redo", json!({}), &text);
}

#[test]
fn in_manual_mode_the_formulas_reading_the_watch_list_through_indirect_wait() {
    // In manual mode the same readers keep their values until the next calculation, and a Watch
    // Window change leaves the revision alone.
    let mut s = watch_text_session(true);
    carry_watch_range(&mut s);
    s.execute("cell.set", json!({"cell": "Y1", "input": "1"})).unwrap();
    let five = vec![Value::Number(5.0); 3];
    let text = vec![Value::text("text"); 3];
    let revision = s.doc().unwrap().revision;
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    assert_eq!(watch_text_shown(&s), five, "the readers wait after the Watch Window change");
    assert_eq!(s.doc().unwrap().revision, revision, "nothing was recalculated, so the revision stays");
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(watch_text_shown(&s), text);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(watch_text_shown(&s), five, "the readers wait after undo carries the list");
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(watch_text_shown(&s), text);
}

#[test]
fn a_name_changed_alone_recalculates_a_formula_reading_it_through_indirect() {
    // Saving a Solver model without keeping the solution rewrites `solver_opt` and changes no
    // cell. A formula that reaches the name only through INDIRECT's text mentions no name, so it
    // is not among the name's readers; it is dynamic, and dynamic formulas are recalculated when a
    // name changes as they are after any edit, and nothing else is.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    s.execute("cell.set", json!({"cell": "B10", "input": "=A1"})).unwrap();
    s.execute("cell.set", json!({"cell": "B20", "input": "=A1*2"})).unwrap();
    let model = |objective: &str| json!({"objective": objective, "variables": "A1", "constraints": ["A1 <= 2"], "method": "simplex", "keep": false, "saveModel": true});
    s.execute("data.solver", model("B10")).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=ROW(INDIRECT(\"solver_opt\"))"})).unwrap();
    assert_eq!(v(&s, "C1"), Value::Number(10.0));
    s.execute("data.solver", model("B20")).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(1.0), "keep: false leaves the variable alone");
    assert_eq!(v(&s, "C1"), Value::Number(20.0), "the formula follows the saved objective");
    assert_eq!(s.doc().unwrap().calc.last_recalc_cells, 1, "only the dynamic formula was recalculated");
}

#[test]
fn the_readers_of_a_rewritten_name_are_found_by_their_text_in_any_case() {
    // The search reads a formula's text before walking its tree: a reader spelling the name in
    // other letters, one reaching it two names away, and one whose tree is parsed from its text
    // only when asked are all found; a formula holding the name only in a string literal may be
    // walked, but isn't a reader and keeps its value.
    let mut s = s();
    s.execute("view.customViews", json!({"save": "first"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Outer_Length", "refersTo": "=INNER_length+1"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Inner_Length", "refersTo": "=LEN(_SC_Custom_Views)"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_SC_CUSTOM_VIEWS)"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=outer_LENGTH"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=LEN(\"_sc_custom_views\")"})).unwrap();
    s.execute("cell.set", json!({"cell": "D1", "input": "=LEN(_sc_custom_views)*2"})).unwrap();
    // D1 loses its cached tree, as a formula whose text didn't parse into one would have none.
    {
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let cell = wb.sheet_mut(0).unwrap().cells.get_mut(CellRef::parse("D1").unwrap()).unwrap();
        std::sync::Arc::make_mut(cell.formula.as_mut().unwrap()).expr = None;
    }
    let number = |s: &Session, at: &str| match v(s, at) {
        Value::Number(n) => n,
        other => panic!("{at}: expected a number, got {other:?}"),
    };
    let before = number(&s, "A1");
    assert_eq!((number(&s, "B1"), number(&s, "C1"), number(&s, "D1")), (before + 1.0, 16.0, before * 2.0));
    s.execute("view.customViews", json!({"save": "second view with a longer name"})).unwrap();
    let after = number(&s, "A1");
    assert!(after > before, "the reader spelling the name in capitals sees the new definition");
    assert_eq!(number(&s, "B1"), after + 1.0, "the reader two names away, spelled in mixed case");
    assert_eq!(number(&s, "C1"), 16.0, "the string literal is no reader");
    assert_eq!(number(&s, "D1"), after * 2.0, "the reader without a cached tree");
    assert_eq!(s.doc().unwrap().calc.last_recalc_cells, 3, "only the three readers were recalculated");
}

#[test]
fn in_manual_mode_a_rewritten_names_readers_wait_for_the_next_calculation() {
    // As for an edited cell's dependents in manual mode, the readers of a hidden name a command
    // rewrites are left for F9 or the switch back to automatic, which calculate everything.
    for finish in ["formulas.calculateNow", "automatic"] {
        let mut s = s();
        s.execute("view.customViews", json!({"save": "first"})).unwrap();
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!Z1"})).unwrap();
        s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_custom_views)"})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": "=A1+1"})).unwrap();
        s.execute("cell.set", json!({"cell": "C1", "input": "=LEN(_sc_watch)"})).unwrap();
        s.execute("cell.set", json!({"cell": "D1", "input": "=C1+1"})).unwrap();
        s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
        let shown = |s: &Session| ["A1", "B1", "C1", "D1"].map(|at| v(s, at));
        let before = shown(&s);
        let watch = Value::Number("[\"Sheet1!Z1\"]".len() as f64);
        assert_eq!(before[2], watch);
        s.execute("view.customViews", json!({"save": "second view with a longer name"})).unwrap();
        assert_eq!(shown(&s), before, "{finish}: a saved view leaves the readers for the next calculation");
        s.execute("cell.set", json!({"cell": "Y1", "input": "1"})).unwrap();
        let revision = s.doc().unwrap().revision;
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!Z2"})).unwrap();
        assert_eq!(shown(&s), before, "{finish}: so does the Watch Window");
        assert_eq!(s.doc().unwrap().revision, revision, "{finish}: nothing was recalculated, so the revision stays");
        // Undo carries the current list into the workbook it restores, and leaves its readers too.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(shown(&s), before, "{finish}: undo keeps the list and leaves its readers");
        match finish {
            "automatic" => s.execute("formulas.calculationOptions", json!({"mode": "automatic"})).unwrap(),
            _ => s.execute(finish, json!({})).unwrap(),
        };
        let Value::Number(views) = v(&s, "A1") else { panic!("{finish}: A1 is a length") };
        let Value::Number(old) = before[0].clone() else { panic!("{finish}: A1 was a length") };
        assert!(views > old, "{finish}: the reader sees the new views");
        let list = "[\"Sheet1!Z1\",\"Sheet1!Z2\"]".len() as f64;
        assert_eq!(shown(&s), [Value::Number(views), Value::Number(views + 1.0), Value::Number(list), Value::Number(list + 1.0)], "{finish}");
    }
}

#[test]
fn a_database_function_follows_a_computed_criterion_through_the_commands() {
    // The criteria cell compares each record's H cell with a named threshold; the D-function
    // formula mentions no name. Editing H3 changes the second record's criterion, and the
    // D-function shows what a full calculation gives.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "T1", "input": "0"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Threshold", "refersTo": "=Sheet1!$T$1"})).unwrap();
    for (at, input) in [
        ("A1", "Amount"),
        ("A2", "1"),
        ("A3", "2"),
        ("C1", "Check"),
        ("H2", "1"),
        ("H3", "1"),
        ("C2", "=$H2>Threshold"),
        ("E1", "=DSUM($A$1:$A$3,\"Amount\",$C$1:$C$2)"),
    ] {
        s.execute("cell.set", json!({"cell": at, "input": input})).unwrap();
    }
    assert_eq!(v(&s, "E1"), Value::Number(3.0));
    s.execute("cell.set", json!({"cell": "H3", "input": "-1"})).unwrap();
    let after_edit = v(&s, "E1");
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "E1"), Value::Number(1.0));
    assert_eq!(after_edit, v(&s, "E1"), "after editing H3, E1 shows what a full calculation gives");
}

/// A Solver model of C5 over `variables`, saved without keeping a solution: it rewrites the
/// hidden `solver_*` names (`solver_adj` names the variables) and no cell.
fn saved_solver_model(variables: &str) -> serde_json::Value {
    json!({"objective": "C5", "variables": variables, "constraints": [format!("{variables} <= 2")], "method": "simplex", "keep": false, "saveModel": true})
}

/// What Trace Precedents lists for `at`.
fn traced_precedents(s: &mut Session, at: &str) -> serde_json::Value {
    s.execute("formulas.tracePrecedents", json!({"cell": at})).unwrap()
}

#[test]
fn in_manual_mode_goal_seek_follows_a_rewritten_name() {
    // Goal Seek calculates a copy of the graph in automatic mode. Saving the Solver model again
    // with B2 as its variable makes `solver_adj` name B2 instead of B1; in manual mode C1, which
    // reads the name, has not been calculated since, but its precedents follow the name, so Goal
    // Seek finds the B2 that makes C1 25.
    let mut s = s();
    s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    s.execute("range.setValues", json!({"range": "B1", "values": [[1], [1]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C5", "input": "=B1+B2"})).unwrap();
    s.execute("data.solver", saved_solver_model("B1")).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=SUM(solver_adj)*3+4"})).unwrap();
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "C1"), Value::Number(7.0));
    s.execute("data.solver", saved_solver_model("B2")).unwrap();
    let sought = s.execute("data.goalSeek", json!({"set": "C1", "to": 25, "changing": "B2"})).unwrap();
    assert_eq!(sought["converged"], json!(true), "{sought}");
    let Value::Number(changed) = v(&s, "B2") else { panic!("B2 is a number") };
    assert!((changed - 7.0).abs() < 1e-9, "B2 = {changed}");
}

#[test]
fn in_manual_mode_a_rewritten_names_readers_keep_their_values_and_follow_it_in_the_graph() {
    // In manual mode the reader of a rewritten hidden name keeps its value until the next
    // calculation, and Trace Precedents and Trace Dependents show the new definition's cells at
    // once: after a Solver model is saved again, after a Watch Window change, and after undo and
    // redo carry the watch list into the workbook they restore.
    let mut s = s();
    s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    s.execute("range.setValues", json!({"range": "B1", "values": [[1], [5]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C5", "input": "=B1+B2"})).unwrap();
    s.execute("data.solver", saved_solver_model("B1")).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=SUM(solver_adj)*3+4"})).unwrap();
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "C1"), Value::Number(7.0));
    assert_eq!(traced_precedents(&mut s, "C1"), json!([{"sheet": "Sheet1", "range": "B1"}]));
    s.execute("data.solver", saved_solver_model("B2")).unwrap();
    assert_eq!(v(&s, "C1"), Value::Number(7.0), "the reader waits for the next calculation");
    assert_eq!(traced_precedents(&mut s, "C1"), json!([{"sheet": "Sheet1", "range": "B2"}]), "its precedents don't");
    let dependents = s.execute("formulas.traceDependents", json!({"cell": "B2"})).unwrap();
    assert_eq!(dependents, json!([{"sheet": "Sheet1", "cell": "C1"}, {"sheet": "Sheet1", "cell": "C5"}]));
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "C1"), Value::Number(19.0), "the next calculation reads B2");

    // The watch list's name holding A1:A5, as a file could carry it, then replaced by the list.
    let mut s = watch_text_session(true);
    s.execute("cell.set", json!({"cell": "G1", "input": "=SUM(_sc_watch)"})).unwrap();
    carry_watch_range(&mut s);
    assert_eq!(v(&s, "G1"), Value::Number(15.0));
    let range = json!([{"sheet": "Sheet1", "range": "A1:A5"}]);
    assert_eq!(traced_precedents(&mut s, "G1"), range);
    s.execute("cell.set", json!({"cell": "Y1", "input": "1"})).unwrap();
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    assert_eq!(v(&s, "G1"), Value::Number(15.0), "the reader waits after the Watch Window change");
    assert_eq!(traced_precedents(&mut s, "G1"), json!([]), "the list's text reads no cell");
    // Undo restores the workbook before Y1, whose name held the range, and carries the list in.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(traced_precedents(&mut s, "G1"), json!([]), "undo carries the list");
    // With the range carried in again, redo restores the workbook holding the list and carries
    // the range in.
    carry_watch_range(&mut s);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(traced_precedents(&mut s, "G1"), range, "redo carries the range");
}

#[test]
fn leaving_manual_mode_calculates_a_rewritten_names_readers() {
    // In manual mode the reader of a hidden name a Watch Window change rewrites waits; leaving
    // manual mode, for Automatic or for Automatic except tables, calculates it, as F9 does. The
    // reader is entered before the name exists, so it is dynamic; leaving manual mode calculates
    // the whole workbook in any case, so this doesn't rest on the search for the name's readers.
    let one = Value::Number("[\"Sheet1!B1\"]".len() as f64);
    let two = Value::Number("[\"Sheet1!B1\",\"Sheet1!B2\"]".len() as f64);
    for mode in ["automatic", "automaticExceptTables"] {
        let mut s = s();
        s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
        s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!B2"})).unwrap();
        assert_eq!(v(&s, "A1"), one, "{mode}: the reader waits in manual mode");
        s.execute("formulas.calculationOptions", json!({"mode": mode})).unwrap();
        assert_eq!(v(&s, "A1"), two, "{mode}: leaving manual mode calculates the reader");
        s.execute("cell.set", json!({"cell": "C1", "input": "1"})).unwrap();
        assert_eq!(v(&s, "A1"), two, "{mode}: after an unrelated edit");
    }
}

#[test]
fn reordering_two_spellings_of_one_name_recalculates_its_readers() {
    // A file can carry two names that differ only in case, and a formula reads the first one
    // listed. Removing an absent entry from the Watch Window rewrites `_sc_watch` behind the other
    // spelling without changing either definition, so the reader now reads the other one.
    let mut s = s();
    {
        let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
        for (name, formula) in [("_sc_watch", "\"[]\""), ("_SC_WATCH", "\"shadow\"")] {
            wb.names.push(gridcraft_model::DefinedName {
                name: name.into(),
                scope: None,
                formula: formula.into(),
                comment: String::new(),
                hidden: true,
            });
        }
    }
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0), "the first spelling is read");
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!Z9"})).unwrap();
    let names: Vec<_> = s.doc().unwrap().wb.names.iter().map(|n| (n.name.clone(), n.formula.clone())).collect();
    assert_eq!(names, [("_SC_WATCH".to_string(), "\"shadow\"".to_string()), ("_sc_watch".to_string(), "\"[]\"".to_string())]);
    assert_eq!(v(&s, "A1"), Value::Number(6.0), "the reader follows the spelling now listed first");
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(6.0), "as a full calculation gives");
}

#[test]
fn undo_and_redo_around_a_name_reader_whose_array_covers_the_names_cells_match_a_full_recalculation() {
    // B4's array [C4, D4] spills into C4, a cell `scale_row` reads, once A1 is 1. Undo and redo of
    // that edit put the workbook back without calculating; later edits give what a full
    // recalculation gives.
    let mut s = s();
    s.execute("formulas.defineName", json!({"name": "scale_row", "refersTo": "=Sheet1!$C$4:$D$4"})).unwrap();
    s.execute("cell.set", json!({"cell": "D4", "input": "2"})).unwrap();
    s.execute("cell.set", json!({"cell": "B4", "input": "=IF(A1=1,scale_row+0,5)"})).unwrap();
    let matches_full = |s: &Session, when: &str| {
        let d = s.doc().unwrap();
        let mut full = (*d.wb).clone();
        gridcraft_calc::Calc::default().recalc_all(&mut full);
        for at in ["B4", "C4", "D4"] {
            assert_eq!(v(s, at), full.active().unwrap().value(CellRef::parse(at).unwrap()), "{at} {when}");
        }
    };
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    assert_eq!(v(&s, "B4"), Value::Number(2.0));
    matches_full(&s, "with the array laid out");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "B4"), Value::Number(5.0), "undone: a single value");
    matches_full(&s, "after the undo");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(v(&s, "B4"), Value::Number(2.0), "redone: the array is back over C4");
    matches_full(&s, "after the redo");
    for (input, shown) in [("1", 1.0), ("7", 7.0)] {
        s.execute("cell.set", json!({"cell": "D4", "input": input})).unwrap();
        assert_eq!(v(&s, "B4"), Value::Number(shown), "D4 = {input}");
        matches_full(&s, &format!("after the redo and D4 = {input}"));
    }
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "B4"), Value::Number(1.0), "D4 = 1 again");
    matches_full(&s, "after undoing D4 = 7");
}
