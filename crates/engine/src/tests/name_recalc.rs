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
        wb.names_mut().retain(|n| n.name != "_sc_watch");
        wb.names_mut().push(gridcraft_model::DefinedName {
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
            wb.names_mut().push(gridcraft_model::DefinedName {
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

#[test]
fn volatile_formulas_keep_their_values_when_no_formula_reads_the_rewritten_name() {
    // The Watch Window list and a saved custom view are hidden names holding text, so INDIRECT's
    // text can't make a reference of them. With no formula mentioning them, adding a watch entry,
    // the undo and redo that carry the list into another snapshot, and saving a view calculate
    // nothing: the volatile formulas keep the values they had (undo and redo the snapshot's), as
    // after any change no formula reads.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=RAND()"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "=INDIRECT(\"C1\")+RAND()"})).unwrap();
    s.execute("cell.set", json!({"cell": "A4", "input": "=C9+1"})).unwrap();
    let shown = |s: &Session| [v(s, "A1"), v(s, "A2"), v(s, "A4")];
    let before_edit = shown(&s);
    s.execute("cell.set", json!({"cell": "C9", "input": "5"})).unwrap();
    let after_edit = shown(&s);
    assert_ne!(after_edit[0], before_edit[0], "an edit draws again");
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!A4"})).unwrap();
    assert_eq!(shown(&s), after_edit, "after adding a watch entry");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(shown(&s), before_edit, "after the undo that carries the list");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(shown(&s), after_edit, "after the redo");
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!A4"})).unwrap();
    assert_eq!(shown(&s), before_edit, "after removing the entry");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(shown(&s), after_edit, "after the redo that carries the shorter list");
    s.execute("view.customViews", json!({"save": "plain"})).unwrap();
    assert_eq!(shown(&s), after_edit, "after saving a custom view");

    // A formula reading the list's name is recalculated as the list changes, with the volatile
    // formulas, as any edit recalculates them.
    s.execute("cell.set", json!({"cell": "B1", "input": "=LEN(_sc_watch)"})).unwrap();
    let length = v(&s, "B1");
    let drawn = v(&s, "A1");
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!A4"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Number("[\"Sheet1!A4\"]".len() as f64), "the reader follows the list");
    assert_ne!(v(&s, "B1"), length);
    assert_ne!(v(&s, "A1"), drawn, "the pass that recalculated the reader drew again");
}

#[test]
fn a_long_scenario_list_draws_no_volatile_value() {
    // The scenario list is stored as 250-character text literals joined with `&`, a Watch Window
    // list longer than 8,192 characters as a definition that doesn't parse (it evaluates to
    // `#NAME?`), and the two names defined below take the list's length or join the list to text
    // (a name read bare, `=(_sc_watch)`, could give a reference: a caller can bind the name). None
    // of them can give INDIRECT a reference. With no formula reading them, adding a scenario, a
    // long watch entry, and the entries changed beside the two names calculate nothing: the
    // volatile formulas keep the values they had, as on main.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=RAND()"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "=INDIRECT(\"C1\")+RAND()"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "5"})).unwrap();
    let shown = |s: &Session| [v(s, "A1"), v(s, "A2")];
    let stored = |s: &Session, name: &str| s.doc().unwrap().wb.names.iter().find(|n| n.name == name).map(|n| n.formula.clone()).unwrap();
    let drawn = shown(&s);
    let comment = "scenario comment words ".repeat(20);
    s.execute("data.scenarioAdd", json!({"name": "first_case", "changing": "C1", "comment": comment})).unwrap();
    assert!(stored(&s, "_sc_scenarios").contains("\"&\""), "the scenario list is stored in chunks");
    assert_eq!(shown(&s), drawn, "after adding a scenario");
    s.execute("formulas.watchWindow", json!({"add": "x".repeat(9_000)})).unwrap();
    assert!(gridcraft_formula::parse_name(&stored(&s, "_sc_watch")).is_err(), "the long watch list doesn't parse");
    assert_eq!(shown(&s), drawn, "after adding a long watch entry");

    s.execute("formulas.defineName", json!({"name": "Watch_len", "refersTo": "=LEN(_sc_watch)"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Watch_copy", "refersTo": "=_sc_watch&\"\""})).unwrap();
    let drawn = shown(&s);
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    assert_eq!(shown(&s), drawn, "after adding an entry to the long list");
    s.execute("formulas.watchWindow", json!({"remove": "x".repeat(9_000)})).unwrap();
    assert!(gridcraft_formula::parse_name(&stored(&s, "_sc_watch")).is_ok(), "the shorter watch list parses");
    assert_eq!(shown(&s), drawn, "after removing the long entry");
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B2"})).unwrap();
    assert_eq!(shown(&s), drawn, "after adding an entry to the short list");
}

/// Names that reach the watch list's name through other names and give references (one, two and
/// three names away, and a LAMBDA reading the first), and a LAMBDA that takes a name's text. The
/// row the first picks grows with the list's text.
const WATCH_ALIASES: [(&str, &str); 5] = [
    ("Watch_target", "=INDIRECT(\"Sheet1!A\"&MIN(5,1+INT(LEN(_sc_watch)/12)))"),
    ("Watch_relay", "=Watch_target"),
    ("Watch_far", "=OFFSET(Watch_relay,0,0)"),
    ("Text_row", "=LAMBDA(label,ROW(INDIRECT(label)))"),
    ("Watch_offset", "=LAMBDA(step,ROW(OFFSET(Watch_target,step,0)))"),
];

/// A session with [`WATCH_ALIASES`] over five values in A1:A5 and a formula in each of `readers`.
fn watch_alias_session(readers: &[(&str, &str)]) -> Session {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3], [4], [5]]})).unwrap();
    for (name, refers_to) in WATCH_ALIASES {
        s.execute("formulas.defineName", json!({"name": name, "refersTo": refers_to})).unwrap();
    }
    for (at, input) in readers {
        s.execute("cell.set", json!({"cell": at, "input": input})).unwrap();
    }
    s
}

/// Runs `cmd` and checks that each of `readers` shows what a full recalculation gives, and that the
/// first shows `expected`.
fn alias_step(s: &mut Session, readers: &[(&str, &str)], cmd: &str, params: serde_json::Value, expected: &Value) {
    s.execute(cmd, params.clone()).unwrap();
    let d = s.doc().unwrap();
    let mut full = (*d.wb).clone();
    gridcraft_calc::Calc::default().recalc_all(&mut full);
    for (at, _) in readers {
        assert_eq!(v(s, at), full.active().unwrap().value(CellRef::parse(at).unwrap()), "{at} after {cmd} {params}");
    }
    assert_eq!(&v(s, readers[0].0), expected, "{} after {cmd} {params}", readers[0].0);
}

/// Defines, changes, carries through undo and redo, and removes the watch list's name, checking
/// `readers` after each step: the first reads a name that picks row 1 for an empty list, then a row
/// further down for each entry.
fn walk_the_watch_list(readers: &[(&str, &str)]) {
    let row = |r: f64| Value::Number(r);
    let mut s = watch_alias_session(readers);
    // The name is defined by the first entry, changed by the second, and changed back.
    alias_step(&mut s, readers, "formulas.watchWindow", json!({"add": "Sheet1!B1"}), &row(2.0));
    alias_step(&mut s, readers, "formulas.watchWindow", json!({"add": "Sheet1!B2"}), &row(3.0));
    alias_step(&mut s, readers, "formulas.watchWindow", json!({"remove": "Sheet1!B2"}), &row(2.0));
    // Undo carries a two-entry list into a snapshot calculated with one entry, redo a one-entry
    // list into one calculated with two.
    s.execute("cell.set", json!({"cell": "Y1", "input": "1"})).unwrap();
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B3"})).unwrap();
    alias_step(&mut s, readers, "edit.undo", json!({}), &row(3.0));
    alias_step(&mut s, readers, "formulas.watchWindow", json!({"remove": "Sheet1!B3"}), &row(2.0));
    alias_step(&mut s, readers, "edit.redo", json!({}), &row(2.0));
    // Undo into a snapshot holding the list, from a workbook without the name (as a file could
    // leave it), removes the name there.
    s.execute("cell.set", json!({"cell": "Y2", "input": "1"})).unwrap();
    {
        let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
        wb.names_mut().retain(|n| n.name != "_sc_watch");
    }
    s.execute("formulas.calculateNow", json!({})).unwrap();
    let gone = v(&s, readers[0].0);
    assert!(matches!(gone, Value::Error(_)), "without the name: {gone:?}");
    alias_step(&mut s, readers, "edit.undo", json!({}), &gone);
}

#[test]
fn a_formula_reaching_the_watch_list_through_an_alias_in_indirect_text_follows_it() {
    // The list's name is text before and after each change, but names that read it give
    // references, and formulas reach those names only through INDIRECT's text: one, two and three
    // names away, and through a LAMBDA given the name's text. No formula mentions an affected name,
    // so none is a reader; they are dynamic, and the pass that evaluates them must run.
    walk_the_watch_list(&[
        ("D1", "=ROW(INDIRECT(\"Watch_target\"))"),
        ("D2", "=ROW(INDIRECT(\"Watch_relay\"))"),
        ("D3", "=ROW(INDIRECT(\"Watch_far\"))"),
        ("D4", "=Text_row(\"Watch_far\")"),
    ]);
}

#[test]
fn a_formula_reading_an_alias_of_the_watch_list_follows_it() {
    // The same names read directly, one, two and three names away, and through a LAMBDA whose
    // body reads one of them: these formulas are the readers, found through which names mention which.
    walk_the_watch_list(&[("E1", "=ROW(Watch_target)"), ("E2", "=ROW(Watch_relay)"), ("E3", "=ROW(Watch_far)"), ("E4", "=Watch_offset(0)")]);
}

#[test]
fn a_formula_reaching_a_rewritten_reference_through_an_alias_follows_it() {
    // Saving a Solver model rewrites `solver_opt`, a reference, with no cell edited. A name giving
    // a number from it is read directly, and a name giving a reference from it only through
    // INDIRECT's text; both follow the saved objective, as a full recalculation gives.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    s.execute("cell.set", json!({"cell": "B10", "input": "=A1"})).unwrap();
    s.execute("cell.set", json!({"cell": "B20", "input": "=A1*2"})).unwrap();
    let model = |objective: &str| json!({"objective": objective, "variables": "A1", "constraints": ["A1 <= 2"], "method": "simplex", "keep": false, "saveModel": true});
    s.execute("data.solver", model("B10")).unwrap();
    s.execute("formulas.defineName", json!({"name": "Objective_row", "refersTo": "=ROW(solver_opt)"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Objective_next", "refersTo": "=OFFSET(solver_opt,1,0)"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=Objective_row"})).unwrap();
    s.execute("cell.set", json!({"cell": "C2", "input": "=ROW(INDIRECT(\"Objective_next\"))"})).unwrap();
    assert_eq!((v(&s, "C1"), v(&s, "C2")), (Value::Number(10.0), Value::Number(11.0)));
    s.execute("data.solver", model("B20")).unwrap();
    assert_eq!((v(&s, "C1"), v(&s, "C2")), (Value::Number(20.0), Value::Number(21.0)), "both follow the saved objective");
    let mut full = (*s.doc().unwrap().wb).clone();
    gridcraft_calc::Calc::default().recalc_all(&mut full);
    for at in ["C1", "C2"] {
        assert_eq!(v(&s, at), full.active().unwrap().value(CellRef::parse(at).unwrap()), "{at}");
    }
}

#[test]
fn every_kind_of_name_writer_recalculates_its_readers() {
    // Each command below writes defined names: the hidden ones holding custom views, scripts,
    // scenarios, the Watch Window list and the Solver model, which are written without a
    // structural edit, and the name and sheet commands, which are structural. After each, the
    // formula reading the name it writes shows its new value, and every reader shows what a full
    // recalculation gives.
    let mut s = s();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    let other = s.doc().unwrap().wb.sheet_index("Other").unwrap();
    s.execute("sheet.activate", json!({"sheet": other})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "7"})).unwrap();
    let first = s.doc().unwrap().wb.sheet_index("Sheet1").unwrap();
    s.execute("sheet.activate", json!({"sheet": first})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "1"})).unwrap();
    for (at, input) in [("B10", "=C1"), ("B20", "=C1*2"), ("B30", "=C1*3")] {
        s.execute("cell.set", json!({"cell": at, "input": input})).unwrap();
    }
    s.execute("formulas.defineName", json!({"name": "Rate", "refersTo": "=1"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Remote", "refersTo": "=Other!$A$1"})).unwrap();
    let readers = [
        ("A1", "=LEN(_sc_custom_views)"),
        ("A2", "=LEN(_sc_scripts)"),
        ("A3", "=LEN(_sc_scenarios)"),
        ("A4", "=LEN(_sc_watch)"),
        ("A5", "=ROW(solver_opt)"),
        ("A6", "=Rate*2"),
        ("A7", "=Remote+1"),
    ];
    for (at, input) in readers {
        s.execute("cell.set", json!({"cell": at, "input": input})).unwrap();
    }
    let model = |objective: &str| json!({"objective": objective, "variables": "C1", "constraints": ["C1 <= 2"], "method": "simplex", "keep": false, "saveModel": true});
    // The command, its params, and the reader whose value it changes (`None`: a rename, after
    // which the reader shows the same value through the new name).
    let steps: [(&str, serde_json::Value, Option<&str>); 18] = [
        ("view.customViews", json!({"save": "first"}), Some("A1")),
        ("view.customViews", json!({"delete": "first"}), Some("A1")),
        ("automate.newScript", json!({"name": "tidy", "commands": []}), Some("A2")),
        ("automate.deleteScript", json!({"name": "tidy"}), Some("A2")),
        ("data.scenarioAdd", json!({"name": "first_case", "changing": "C1"}), Some("A3")),
        ("data.scenarioDelete", json!({"name": "first_case"}), Some("A3")),
        ("formulas.watchWindow", json!({"add": "Sheet1!B10"}), Some("A4")),
        ("formulas.watchWindow", json!({"remove": "Sheet1!B10"}), Some("A4")),
        ("data.solver", model("B10"), Some("A5")),
        ("data.solverSaveModel", json!({"to": "H1"}), None),
        ("data.solver", model("B20"), Some("A5")),
        ("data.solverLoadModel", json!({"from": "H1"}), Some("A5")),
        ("data.solverReset", json!({}), Some("A5")),
        ("formulas.defineName", json!({"name": "Rate", "refersTo": "=3"}), Some("A6")),
        ("formulas.defineName", json!({"name": "Pace", "refersTo": "=3", "oldName": "Rate"}), None),
        ("formulas.deleteName", json!({"name": "Pace"}), Some("A6")),
        ("sheet.rename", json!({"sheet": "Other", "name": "Far"}), None),
        ("home.deleteSheet", json!({"sheet": "Far"}), Some("A7")),
    ];
    for (command, params, changes) in steps {
        let before = changes.map(|at| v(&s, at));
        s.execute(command, params.clone()).unwrap();
        if let (Some(at), Some(before)) = (changes, before) {
            assert_ne!(v(&s, at), before, "{command} {params}: {at} follows the name it writes");
        }
        let mut full = (*s.doc().unwrap().wb).clone();
        gridcraft_calc::Calc::default().recalc_all(&mut full);
        for (at, _) in readers {
            assert_eq!(v(&s, at), full.active().unwrap().value(CellRef::parse(at).unwrap()), "{command} {params}: {at}");
        }
    }
    assert_eq!(v(&s, "A5"), Value::Error(gridcraft_core::CellError::Name), "the model is gone after Reset All");
}

#[test]
fn a_formula_binding_a_called_name_to_a_lambda_follows_the_watch_list_through_indirect() {
    // `picker` is defined as text, but the formula reaching `Watch_pick` through INDIRECT binds
    // `picker` to a LAMBDA in its LET, and the evaluator calls that binding: `Watch_pick` then
    // gives a reference picked by the watch list's length. A watch entry recalculates the formula,
    // as a full recalculation gives.
    let mut s = s();
    s.execute("cell.set", json!({"cell": "C1", "input": "10"})).unwrap();
    s.execute("cell.set", json!({"cell": "C2", "input": "20"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "picker", "refersTo": "=\"unused\""})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Watch_pick", "refersTo": "=picker(_sc_watch)"})).unwrap();
    let input = "=LET(picker,LAMBDA(list,IF(LEN(list)>20,Sheet1!$C$2,Sheet1!$C$1)),INDIRECT(\"Watch_pick\"))+0";
    s.execute("cell.set", json!({"cell": "A1", "input": input})).unwrap();
    for (entry, expected) in [("Sheet1!B10", 10.0), ("Sheet1!B20", 20.0)] {
        s.execute("formulas.watchWindow", json!({"add": entry})).unwrap();
        let mut full = (*s.doc().unwrap().wb).clone();
        gridcraft_calc::Calc::default().recalc_all(&mut full);
        let recalculated = full.active().unwrap().value(CellRef::parse("A1").unwrap());
        assert_eq!(recalculated, Value::Number(expected), "after adding {entry}, a full recalculation");
        assert_eq!(v(&s, "A1"), recalculated, "after adding {entry}");
    }
}

#[test]
fn a_slider_linked_to_a_reader_of_the_watch_list_follows_it() {
    // A slider linked to A1, which reads the watch list's name, moves with A1 when a watch entry
    // recalculates it, and when undo and redo carry the list into a snapshot calculated with
    // another list, as a commit moves it after an edit.
    let mut s = s();
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
    s.execute("insert.slider", json!({"at": "D2", "link": "$A$1", "min": 0, "max": 100, "step": 1})).unwrap();
    let slider = |s: &Session| s.doc().unwrap().wb.active().unwrap().controls.first().map(|c| f64::from(c.value));
    let check = |s: &Session, step: &str, list: &str| {
        let length = list.len() as f64;
        assert_eq!(v(s, "A1"), Value::Number(length), "A1 {step}");
        assert_eq!(slider(s), Some(length), "the slider {step}");
    };
    let (empty, one, two) = ("[]", "[\"Sheet1!B1\"]", "[\"Sheet1!B1\",\"Sheet1!B2\"]");
    check(&s, "with an empty list", empty);
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
    check(&s, "after a watch entry is added", one);
    s.execute("cell.set", json!({"cell": "Z1", "input": "1"})).unwrap();
    s.execute("formulas.watchWindow", json!({"add": "Sheet1!B2"})).unwrap();
    check(&s, "after a second entry is added", two);
    // The snapshot before Z1 was set was calculated with one entry.
    s.execute("edit.undo", json!({})).unwrap();
    check(&s, "after the undo that carries two entries", two);
    s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B2"})).unwrap();
    check(&s, "after the second entry is removed", one);
    // The snapshot redo restores was calculated with two entries.
    s.execute("edit.redo", json!({})).unwrap();
    check(&s, "after the redo that carries one entry", one);
}

#[test]
fn a_formula_binding_the_name_the_watch_list_reads_follows_it_through_indirect() {
    // The watch list's name, as a file can leave it, is defined as `edge` or `Sheet1!edge`, and
    // `edge` as 0 workbook-wide: neither definition gives a reference. The formula binds the name
    // as written to a reference in its LET, and the evaluator reads that binding first, so
    // INDIRECT("_sc_watch") gives row 2 until a watch entry rewrites the list as text, and #REF!
    // after, as a full recalculation gives.
    for (alias, input) in
        [("=edge", "=LET(edge,$B$2,ROW(INDIRECT(\"_sc_watch\")))"), ("=Sheet1!edge", "=LET(Sheet1!edge,$B$2,ROW(INDIRECT(\"_sc_watch\")))")]
    {
        let mut s = s();
        s.execute("formulas.defineName", json!({"name": "edge", "refersTo": "=0"})).unwrap();
        s.execute("formulas.defineName", json!({"name": "_sc_watch", "refersTo": alias, "force": true})).unwrap();
        s.execute("cell.set", json!({"cell": "C5", "input": input})).unwrap();
        assert_eq!(v(&s, "C5"), Value::Number(2.0), "{alias}: before");
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
        let mut full = (*s.doc().unwrap().wb).clone();
        gridcraft_calc::Calc::default().recalc_all(&mut full);
        let recalculated = full.active().unwrap().value(CellRef::parse("C5").unwrap());
        assert_eq!(recalculated, Value::Error(gridcraft_core::CellError::Ref), "{alias}: a full recalculation");
        assert_eq!(v(&s, "C5"), recalculated, "{alias}: after adding a watch entry");
    }
}
