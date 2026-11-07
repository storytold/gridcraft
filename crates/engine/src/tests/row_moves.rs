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
fn moving_data_from_the_last_line_is_refused_without_loss() {
    let mut rows = s();
    rows.execute("cell.set", json!({"cell":"A1048576", "input":"keep row"})).unwrap();
    let before = rows.doc().unwrap().wb.clone();
    let err = rows.execute("sheet.moveRows", json!({"rows":"1048576:1048576", "before":0})).unwrap_err();
    assert!(err.to_string().contains("cannot be shifted off of the worksheet"));
    assert_eq!(*rows.doc().unwrap().wb, *before);

    let mut cols = s();
    cols.execute("cell.set", json!({"cell":"XFD1", "input":"keep column"})).unwrap();
    let before = cols.doc().unwrap().wb.clone();
    let err = cols.execute("sheet.moveColumns", json!({"cols":"XFD:XFD", "before":0})).unwrap_err();
    assert!(err.to_string().contains("cannot be shifted off of the worksheet"));
    assert_eq!(*cols.doc().unwrap().wb, *before);
}

#[test]
fn move_rows_refuses_tail_line_metadata_without_mutation() {
    for count in [1, 2] {
        let mut rows = s();
        let tail = gridcraft_core::MAX_ROWS - count;
        std::sync::Arc::make_mut(&mut rows.doc_mut().unwrap().wb)
            .sheet_mut(0)
            .unwrap()
            .rows
            .insert(tail, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
        let before = rows.doc().unwrap().wb.clone();
        let err = rows.execute("sheet.moveRows", json!({"rows":"2:2", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*rows.doc().unwrap().wb, *before);
    }
}

#[test]
fn move_columns_refuses_tail_line_metadata_without_mutation() {
    for count in [1, 2] {
        let mut cols = s();
        let tail = gridcraft_core::MAX_COLS - count;
        std::sync::Arc::make_mut(&mut cols.doc_mut().unwrap().wb)
            .sheet_mut(0)
            .unwrap()
            .cols
            .insert(tail, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
        let before = cols.doc().unwrap().wb.clone();
        let err = cols.execute("sheet.moveColumns", json!({"cols":"B:B", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*cols.doc().unwrap().wb, *before);
    }
}

#[test]
fn move_rows_refuses_tail_merge_without_mutation() {
    for count in [1, 2] {
        let mut rows = s();
        let tail = gridcraft_core::MAX_ROWS - count;
        let sh = std::sync::Arc::make_mut(&mut rows.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        sh.merges.push(gridcraft_core::RangeRef::new(CellRef::new(tail.saturating_sub(1), 0), CellRef::new(tail, 0)));
        let before = rows.doc().unwrap().wb.clone();
        let err = rows.execute("sheet.moveRows", json!({"rows":"2:2", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*rows.doc().unwrap().wb, *before);
    }
}

#[test]
fn move_columns_refuses_tail_merge_without_mutation() {
    for count in [1, 2] {
        let mut cols = s();
        let tail = gridcraft_core::MAX_COLS - count;
        let sh = std::sync::Arc::make_mut(&mut cols.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        sh.merges.push(gridcraft_core::RangeRef::new(CellRef::new(0, tail.saturating_sub(1)), CellRef::new(0, tail)));
        let before = cols.doc().unwrap().wb.clone();
        let err = cols.execute("sheet.moveColumns", json!({"cols":"B:B", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*cols.doc().unwrap().wb, *before);
    }
}

#[test]
fn move_rows_refuses_tail_cell_picture_map_without_mutation() {
    for count in [1, 2] {
        let mut rows = s();
        let tail = gridcraft_core::MAX_ROWS - count;
        let sh = std::sync::Arc::make_mut(&mut rows.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        sh.cell_pictures.insert(
            CellRef::new(tail, 0),
            std::sync::Arc::new(gridcraft_model::CellPicture { data: vec![1], mime: "image/png".into(), alt: "tail".into() }),
        );
        let before = rows.doc().unwrap().wb.clone();
        let err = rows.execute("sheet.moveRows", json!({"rows":"2:2", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*rows.doc().unwrap().wb, *before);
    }
}

#[test]
fn move_columns_refuses_tail_cell_picture_map_without_mutation() {
    for count in [1, 2] {
        let mut cols = s();
        let tail = gridcraft_core::MAX_COLS - count;
        let sh = std::sync::Arc::make_mut(&mut cols.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        sh.cell_pictures.insert(
            CellRef::new(0, tail),
            std::sync::Arc::new(gridcraft_model::CellPicture { data: vec![1], mime: "image/png".into(), alt: "tail".into() }),
        );
        let before = cols.doc().unwrap().wb.clone();
        let err = cols.execute("sheet.moveColumns", json!({"cols":"B:B", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*cols.doc().unwrap().wb, *before);
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

#[test]
fn move_rows_preserves_sheet_edge_range_reference() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"Z1", "input":"=SUM(A5:A1048576)"})).unwrap();
    s.execute("sheet.moveRows", json!({"rows":"2:3", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"Z1"})).unwrap()["formula"], "=SUM(A3:A1048576)");
}

#[test]
fn move_rows_preserves_sheet_edge_cell_reference() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"AA1", "input":"=A1048576"})).unwrap();
    s.execute("sheet.moveRows", json!({"rows":"2:3", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"AA1"})).unwrap()["formula"], "=A1048576");
}

#[test]
fn move_rows_preserves_sheet_edge_absolute_and_mixed_references() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"AC1", "input":"=$A$1048576+A$1048576+$A1048576+SUM(5:1048576)+SUM($A$5:A$1048576)"})).unwrap();
    s.execute("sheet.moveRows", json!({"rows":"2:3", "before":9})).unwrap();
    assert_eq!(
        s.execute("cell.get", json!({"cell":"AC1"})).unwrap()["formula"],
        "=$A$1048576+A$1048576+$A1048576+SUM(3:1048576)+SUM($A$3:A$1048576)"
    );
}

#[test]
fn move_rows_preserves_sheet_edge_cross_sheet_references() {
    let mut s = s();
    s.execute("sheet.rename", json!({"name":"Source"})).unwrap();
    s.execute("home.insertSheet", json!({"name":"Ref"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1", "input":"=Source!A1048576+SUM(Source!A5:A1048576)+SUM(Source!5:1048576)"})).unwrap();
    s.execute("sheet.moveRows", json!({"sheet":"Source", "rows":"2:3", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"A1"})).unwrap()["formula"], "=Source!A1048576+SUM(Source!A3:A1048576)+SUM(Source!3:1048576)");
}

#[test]
fn move_columns_preserves_sheet_edge_range_reference() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"A2", "input":"=SUM(E1:XFD1)"})).unwrap();
    s.execute("sheet.moveColumns", json!({"cols":"B:C", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"A2"})).unwrap()["formula"], "=SUM(C1:XFD1)");
}

#[test]
fn move_columns_preserves_sheet_edge_cell_reference() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"A3", "input":"=XFD1"})).unwrap();
    s.execute("sheet.moveColumns", json!({"cols":"B:C", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"A3"})).unwrap()["formula"], "=XFD1");
}

#[test]
fn move_columns_preserves_sheet_edge_absolute_and_mixed_references() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"A4", "input":"=$XFD$1+XFD$1+$XFD1+SUM(E:XFD)+SUM($E$1:XFD$1)"})).unwrap();
    s.execute("sheet.moveColumns", json!({"cols":"B:C", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"A4"})).unwrap()["formula"], "=$XFD$1+XFD$1+$XFD1+SUM(C:XFD)+SUM($C$1:XFD$1)");
}

#[test]
fn move_columns_preserves_sheet_edge_cross_sheet_references() {
    let mut s = s();
    s.execute("sheet.rename", json!({"name":"Source"})).unwrap();
    s.execute("home.insertSheet", json!({"name":"Ref"})).unwrap();
    s.execute("cell.set", json!({"cell":"A1", "input":"=Source!XFD1+SUM(Source!E1:XFD1)+SUM(Source!E:XFD)"})).unwrap();
    s.execute("sheet.moveColumns", json!({"sheet":"Source", "cols":"B:C", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"A1"})).unwrap()["formula"], "=Source!XFD1+SUM(Source!C1:XFD1)+SUM(Source!C:XFD)");
}

#[test]
fn move_rows_preserves_penultimate_line_ranges() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"Z1", "input":"=A1048575+SUM(A5:A1048575)"})).unwrap();
    s.execute("sheet.moveRows", json!({"rows":"2:3", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"Z1"})).unwrap()["formula"], "=A1048575+SUM(A3:A1048575)");
}

#[test]
fn move_columns_preserves_penultimate_line_ranges() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"A2", "input":"=XFC1+SUM(E1:XFC1)"})).unwrap();
    s.execute("sheet.moveColumns", json!({"cols":"B:C", "before":9})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"A2"})).unwrap()["formula"], "=XFC1+SUM(C1:XFC1)");
}

#[test]
fn move_near_sheet_edge_preserves_tail_reference_through_intermediate_source() {
    let mut s = s();
    s.execute("cell.set", json!({"cell":"Z1", "input":"=A1048576"})).unwrap();
    s.execute("sheet.moveRows", json!({"rows":"1048573:1048574", "before":1})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell":"Z1"})).unwrap()["formula"], "=A1048576");
}

#[test]
fn move_rows_carries_slicer_and_control_anchors() {
    let mut s = s();
    let anchor = gridcraft_model::Anchor { cell: CellRef::new(1, 0), mode: gridcraft_model::AnchorMode::MoveOnly, ..Default::default() };
    let sh = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
    sh.slicers.push(gridcraft_model::Slicer {
        id: 1,
        table: String::new(),
        column: String::new(),
        caption: String::new(),
        anchor,
        selected: None,
        columns: 1,
    });
    sh.controls.push(gridcraft_model::FormControl { id: 1, anchor, ..Default::default() });
    let absolute = gridcraft_model::Anchor { cell: CellRef::new(1, 2), mode: gridcraft_model::AnchorMode::Absolute, ..Default::default() };
    sh.controls.push(gridcraft_model::FormControl { id: 2, anchor: absolute, ..Default::default() });
    let before = s.doc().unwrap().wb.clone();
    s.execute("sheet.moveRows", json!({"rows":"2:3", "before":9})).unwrap();
    let moved = s.doc().unwrap().wb.clone();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.slicers[0].anchor.cell.row, 7);
    assert_eq!(sh.controls[0].anchor.cell.row, 7);
    assert_eq!(sh.controls[1].anchor.cell.row, 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().wb, *before);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().wb, *moved);
}
