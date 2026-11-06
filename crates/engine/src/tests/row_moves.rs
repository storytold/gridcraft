use super::*;

fn values(s: &Session) -> Vec<Value> {
    (0..5).map(|row| v(s, &CellRef::new(row, 0).a1())).collect()
}

#[test]
fn move_rows_preserves_formulas_heights_formats_and_undo_redo() {
    let mut s = s();
    s.execute("range.setValues", json!({"range":"A1", "values":[[1],[2],[3],[4],[5]]})).unwrap();
    s.execute("cell.set", json!({"cell":"B2", "input":"=A2*10"})).unwrap();
    s.execute("cell.set", json!({"cell":"C1", "input":"=A2+A4"})).unwrap();
    s.execute("cell.set", json!({"cell":"D1", "input":"=SUM(2:2)"})).unwrap();
    s.execute("home.rowHeight", json!({"rows":"2:2", "height":41})).unwrap();
    s.execute("home.rowHeight", json!({"rows":"3:3", "height":52})).unwrap();
    s.execute("home.bold", json!({"range":"2:2", "on":true})).unwrap();
    let before = s.doc().unwrap().wb.clone();
    s.doc_mut().unwrap().undo.clear();
    let out = s.execute("sheet.moveRows", json!({"rows":"2:3", "before":4})).unwrap();
    assert_eq!(out["rows"], "3:4");
    assert_eq!(values(&s), vec![1.0.into(), 4.0.into(), 2.0.into(), 3.0.into(), 5.0.into()]);
    assert_eq!(s.execute("cell.get", json!({"cell":"B3"})).unwrap()["formula"], "=A3*10");
    assert_eq!(s.execute("cell.get", json!({"cell":"C1"})).unwrap()["formula"], "=A3+A2");
    assert_eq!(s.execute("cell.get", json!({"cell":"D1"})).unwrap()["formula"], "=SUM(3:3)");
    assert_eq!(v(&s, "B3"), 20.0.into());
    assert_eq!(v(&s, "C1"), 6.0.into());
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!((sh.row_height(2), sh.row_height(3)), (41.0, 52.0));
    assert_eq!(sh.rows.get(&2), before.active().unwrap().rows.get(&1));
    assert_eq!(d.selection.current().a1(), "3:4");
    assert_eq!(d.undo.len(), 1);
    let after = d.wb.clone();
    let (back, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&after).unwrap()).unwrap();
    assert_eq!(back.active().unwrap().value(CellRef::new(2, 1)), 20.0.into());
    assert_eq!(back.active().unwrap().row_height(2), 41.0);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().wb, *before);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().wb, *after);
}

#[test]
fn move_rows_upward_and_inside_selection_noop() {
    let mut s = s();
    s.execute("range.setValues", json!({"range":"A1", "values":[[1],[2],[3],[4],[5]]})).unwrap();
    s.execute("sheet.moveRows", json!({"rows":"4:5", "before":1})).unwrap();
    assert_eq!(values(&s), vec![1.0.into(), 4.0.into(), 5.0.into(), 2.0.into(), 3.0.into()]);
    let before = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    for boundary in 1..=3 {
        s.execute("sheet.moveRows", json!({"rows":"2:3", "before":boundary})).unwrap();
        assert_eq!(*s.doc().unwrap().wb, *before);
        assert_eq!(s.doc().unwrap().undo.len(), undo);
    }
}

#[test]
fn move_rows_rejects_invalid_boundaries_without_mutation() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"A1048576", "input":"keep"})).unwrap();
    let before = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    for params in [
        json!({"rows":"2:3", "before":-1}),
        json!({"rows":"2:3", "before":1048576}),
        json!({"rows":"2:3", "before":4}),
        json!({"rows":"1048576:1048576", "count":2,"before":1}),
    ] {
        assert!(s.execute("sheet.moveRows", params).is_err());
        assert_eq!(*s.doc().unwrap().wb, *before);
        assert_eq!(s.doc().unwrap().undo.len(), undo);
    }
}

#[test]
fn move_rows_checks_named_sheet_protection() {
    let mut s = s();
    s.execute("home.insertSheet", json!({"name":"Protected"})).unwrap();
    s.execute("range.setValues", json!({"range":"A1", "values":[[1],[2],[3],[4],[5]]})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    s.execute("sheet.activate", json!({"sheet":0})).unwrap();
    let before = s.doc().unwrap().wb.clone();
    assert!(s.execute("sheet.moveRows", json!({"sheet":"Protected", "rows":"2:3", "before":4})).unwrap_err().to_string().contains("protected"));
    assert_eq!(*s.doc().unwrap().wb, *before);
}

#[test]
fn move_rows_updates_other_sheets_references() {
    let mut s = s();
    s.execute("sheet.rename", json!({"name":"Source"})).unwrap();
    s.execute("range.setValues", json!({"range":"A1", "values":[[1],[2],[3],[4],[5]]})).unwrap();
    s.execute("home.insertSheet", json!({"name":"References"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1", "input":"=Source!A2+Source!A4"})).unwrap();
    s.execute("sheet.moveRows", json!({"sheet":"Source", "rows":"2:3", "before":4})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"A1"})).unwrap()["formula"], "=Source!A3+Source!A2");
    assert_eq!(v(&s, "A1"), 6.0.into());
}

#[test]
fn move_rows_carries_hidden_rows_in_the_selected_block() {
    let mut s = s();
    s.execute("range.setValues", json!({"range":"A1", "values":[[1],[2],[3],[4],[5]]})).unwrap();
    s.execute("home.hideRows", json!({"rows":"3:3"})).unwrap();
    let hidden = s.doc().unwrap().wb.active().unwrap().rows.get(&2).cloned();
    s.execute("sheet.moveRows", json!({"rows":"2:3", "before":4})).unwrap();
    assert_eq!(values(&s), vec![1.0.into(), 4.0.into(), 2.0.into(), 3.0.into(), 5.0.into()]);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().rows.get(&3), hidden.as_ref());
}

#[test]
fn move_rows_within_sum_range_preserves_total() {
    let mut s = s();
    s.execute("range.setValues", json!({"range":"A1", "values":[[1],[2],[3],[4],[5]]})).unwrap();
    s.execute("cell.set", json!({"cell":"B6", "input":"=SUM(A1:A4)"})).unwrap();
    assert_eq!(v(&s, "B6"), 10.0.into());
    s.execute("sheet.moveRows", json!({"rows":"2:3", "before":4})).unwrap();
    assert_eq!(v(&s, "B6"), 10.0.into());
}

#[test]
fn move_rows_and_columns_preserve_enclosing_sum_ranges() {
    for rows in [true, false] {
        for (source, boundary) in [("2:3", 4), ("4:4", 1), ("1:2", 4), ("3:4", 0)] {
            let mut s = s();
            let vals = if rows { json!([[1], [2], [3], [4], [5]]) } else { json!([[1, 2, 3, 4, 5]]) };
            s.execute("range.setValues", json!({"range":"A1", "values":vals})).unwrap();
            s.execute("cell.set", json!({"cell":"G7", "input":if rows {"=SUM($A$1:$A$4)"} else {"=SUM($A$1:$D$1)"}})).unwrap();
            let before = s.doc().unwrap().wb.clone();
            let (command, params) = if rows {
                ("sheet.moveRows", json!({"rows":source, "before":boundary}))
            } else {
                let cols = match source {
                    "2:3" => "B:C",
                    "4:4" => "D:D",
                    "1:2" => "A:B",
                    _ => "C:D",
                };
                ("sheet.moveColumns", json!({"cols":cols, "before":boundary}))
            };
            s.execute(command, params).unwrap();
            assert_eq!(v(&s, "G7"), 10.0.into(), "rows={rows}, source={source}, before={boundary}");
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(*s.doc().unwrap().wb, *before);
            s.execute("edit.redo", json!({})).unwrap();
            assert_eq!(v(&s, "G7"), 10.0.into());
        }
    }
}

#[test]
fn move_rows_keeps_array_region_with_its_formula_and_rejects_splitting() {
    let mut s = s();
    s.execute("selection.set", json!({"range":"B2:B3"})).unwrap();
    s.execute("cell.set", json!({"cell":"B2", "input":"=ROW(A2:A3)", "array":true})).unwrap();
    let before = s.doc().unwrap().wb.clone();
    assert!(s.execute("sheet.moveRows", json!({"rows":"2:2", "before":4})).unwrap_err().to_string().contains("array"));
    assert_eq!(*s.doc().unwrap().wb, *before);
    s.execute("sheet.moveRows", json!({"rows":"2:3", "before":4})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    let f = sh.cells.get(CellRef::new(2, 1)).unwrap().formula.as_ref().unwrap();
    assert_eq!(f.array.unwrap().a1(), "B3:B4");
    assert_eq!(f.text, "ROW(A3:A4)");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().wb, *before);
}
