use super::*;

use gridcraft_core::{MAX_COLS, MAX_ROWS, RangeRef};
use gridcraft_model::{Anchor, AnchorMode, Sheet};

fn assert_move_refused(s: &mut Session, command: &str, params: serde_json::Value, message: &str) {
    let before = s.doc().unwrap().wb.clone();
    let undo = s.doc().unwrap().undo.len();
    let err = s.execute(command, params).unwrap_err();
    assert!(err.to_string().contains(message), "{err}");
    assert_eq!(*s.doc().unwrap().wb, *before);
    assert_eq!(s.doc().unwrap().undo.len(), undo);
}

fn move_command(rows: bool) -> &'static str {
    if rows { "sheet.moveRows" } else { "sheet.moveColumns" }
}

fn move_params(rows: bool, from: u32, count: u32, before: u32) -> serde_json::Value {
    if rows {
        json!({"rows":RangeRef::rows(from, from + count - 1).a1(), "before":before})
    } else {
        json!({"cols":RangeRef::cols(from, from + count - 1).a1(), "before":before})
    }
}

fn axis_cell(rows: bool, line: u32) -> CellRef {
    if rows { CellRef::new(line, 0) } else { CellRef::new(0, line) }
}

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
    let undo = rows.doc().unwrap().undo.len();
    let err = rows.execute("sheet.moveRows", json!({"rows":"1048576:1048576", "before":0})).unwrap_err();
    assert!(err.to_string().contains("cannot be shifted off of the worksheet"));
    assert_eq!(*rows.doc().unwrap().wb, *before);
    assert_eq!(rows.doc().unwrap().undo.len(), undo);

    let mut cols = s();
    cols.execute("cell.set", json!({"cell":"XFD1", "input":"keep column"})).unwrap();
    let before = cols.doc().unwrap().wb.clone();
    let undo = cols.doc().unwrap().undo.len();
    let err = cols.execute("sheet.moveColumns", json!({"cols":"XFD:XFD", "before":0})).unwrap_err();
    assert!(err.to_string().contains("cannot be shifted off of the worksheet"));
    assert_eq!(*cols.doc().unwrap().wb, *before);
    assert_eq!(cols.doc().unwrap().undo.len(), undo);
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
        let undo = rows.doc().unwrap().undo.len();
        let err = rows.execute("sheet.moveRows", json!({"rows":"2:2", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*rows.doc().unwrap().wb, *before);
        assert_eq!(rows.doc().unwrap().undo.len(), undo);
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
        let undo = cols.doc().unwrap().undo.len();
        let err = cols.execute("sheet.moveColumns", json!({"cols":"B:B", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*cols.doc().unwrap().wb, *before);
        assert_eq!(cols.doc().unwrap().undo.len(), undo);
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
        let undo = rows.doc().unwrap().undo.len();
        let err = rows.execute("sheet.moveRows", json!({"rows":"2:2", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*rows.doc().unwrap().wb, *before);
        assert_eq!(rows.doc().unwrap().undo.len(), undo);
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
        let undo = cols.doc().unwrap().undo.len();
        let err = cols.execute("sheet.moveColumns", json!({"cols":"B:B", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*cols.doc().unwrap().wb, *before);
        assert_eq!(cols.doc().unwrap().undo.len(), undo);
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
        let undo = rows.doc().unwrap().undo.len();
        let err = rows.execute("sheet.moveRows", json!({"rows":"2:2", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*rows.doc().unwrap().wb, *before);
        assert_eq!(rows.doc().unwrap().undo.len(), undo);
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
        let undo = cols.doc().unwrap().undo.len();
        let err = cols.execute("sheet.moveColumns", json!({"cols":"B:B", "count":count, "before":0})).unwrap_err();
        assert!(err.to_string().contains("cannot be shifted off of the worksheet"), "{err}");
        assert_eq!(*cols.doc().unwrap().wb, *before);
        assert_eq!(cols.doc().unwrap().undo.len(), undo);
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
    let undo = s.doc().unwrap().undo.len();
    assert!(s.execute("sheet.moveRows", json!({"sheet":"Protected", "rows":"2:3", "before":4})).unwrap_err().to_string().contains("protected"));
    assert_eq!(*s.doc().unwrap().wb, *before);
    assert_eq!(s.doc().unwrap().undo.len(), undo);
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
    assert_move_refused(&mut s, "sheet.moveRows", json!({"rows":"2:2", "before":4}), "array");
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

// Isolate each tail store: an unrelated cell at the edge would make every fixture pass even
// if its metadata guard were removed. Spill-range ends also contribute to used_range().
const TAIL_KINDS: &[&str] = &[
    "line",
    "picture",
    "merge",
    "conditional format",
    "edit range",
    "validation",
    "table",
    "autofilter",
    "comment",
    "hyperlink",
    "sparkline",
    "print area",
    "pivot anchor",
    "pivot range",
    "spill cell",
    "spill anchor",
    "spill range",
    "cell",
];

fn add_tail(sh: &mut Sheet, kind: &str, rows: bool, tail: u32) {
    use gridcraft_model::{
        AutoFilter, CfRule, Comment, CondFormat, EditRange, Hyperlink, LineInfo, PivotTable, Sparkline, SparklineKind, Table, TableColumn, TotalsFn,
        Validation,
    };
    let cell = axis_cell(rows, tail);
    // Two cells across the other axis, wholly at the requested tail line.
    let end = if rows { CellRef::new(tail, 1) } else { CellRef::new(1, tail) };
    let range = RangeRef::new(cell, end);
    match kind {
        "line" => {
            (if rows { &mut sh.rows } else { &mut sh.cols }).insert(tail, LineInfo { hidden: true, ..Default::default() });
        }
        "picture" => {
            sh.cell_pictures
                .insert(cell, std::sync::Arc::new(gridcraft_model::CellPicture { data: vec![1], mime: "image/png".into(), alt: "tail".into() }));
        }
        "merge" => sh.merges.push(range),
        "conditional format" => sh.cond_formats.push(CondFormat {
            ranges: vec![range],
            rule: CfRule::Expression { formula: "TRUE".into(), style: Box::default() },
            priority: 1,
            stop_if_true: false,
        }),
        "edit range" => sh.edit_ranges.push(EditRange { title: "Tail".into(), ranges: vec![range], password: None, security_descriptor: None }),
        "validation" => sh.validations.push(Validation { ranges: vec![range], ..Default::default() }),
        "table" => sh.tables.push(Table {
            id: 1,
            name: "Tail".into(),
            range,
            header_row: false,
            totals_row: false,
            columns: (0..range.width())
                .map(|i| TableColumn {
                    name: format!("Column{i}"),
                    totals: TotalsFn::None,
                    totals_label: None,
                    formula: None,
                    unique_name: None,
                    xml: None,
                })
                .collect(),
            style: String::new(),
            banded_rows: false,
            banded_cols: false,
            first_col: false,
            last_col: false,
            filter_button: false,
            filter: vec![],
            table_type: None,
            connection_id: None,
        }),
        "autofilter" => sh.autofilter = Some(AutoFilter { range, criteria: vec![] }),
        "comment" => {
            sh.comments.insert(
                cell,
                Comment {
                    author: "Test".into(),
                    text: "Tail".into(),
                    replies: vec![],
                    threaded: false,
                    resolved: false,
                    visible: false,
                    dates: vec![],
                    note_box: None,
                },
            );
        }
        "hyperlink" => {
            sh.hyperlinks.insert(cell, Hyperlink { target: "A1".into(), tooltip: None, external: false });
        }
        "sparkline" => {
            sh.sparklines.push(Sparkline { cell, source: "J11:J12".into(), kind: SparklineKind::Line, color: Default::default(), markers: false })
        }
        "print area" => sh.print.print_area = Some(range),
        "pivot anchor" => sh.pivots.push(PivotTable { anchor: cell, ..Default::default() }),
        "pivot range" => sh.pivots.push(PivotTable { anchor: CellRef::new(10, 10), last_range: Some(range), ..Default::default() }),
        "spill cell" => {
            sh.spill.insert(cell, Value::Number(7.0));
        }
        // Keep the range below the edge to isolate the spill_ranges key from used_range().
        "spill anchor" => {
            sh.spill_ranges.insert(cell, RangeRef::cell(CellRef::new(10, 10)));
        }
        "spill range" => {
            sh.spill_ranges.insert(CellRef::new(10, 10), range);
        }
        "cell" => {
            sh.cells.set(cell, gridcraft_model::Cell { value: Value::Number(7.0), ..Default::default() });
        }
        _ => panic!("unknown fixture {kind}"),
    }
}

#[test]
fn move_lines_refuses_each_tail_store_without_workbook_or_undo_changes() {
    for rows in [true, false] {
        for count in [1, 2] {
            let limit = if rows { MAX_ROWS } else { MAX_COLS };
            for kind in TAIL_KINDS {
                let mut s = s();
                s.execute("cell.set", json!({"cell":"C3", "input":"existing undo entry"})).unwrap();
                let sh = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
                add_tail(sh, kind, rows, limit - count);
                let before = s.doc().unwrap().wb.clone();
                let undo = s.doc().unwrap().undo.len();
                let result = s.execute(move_command(rows), move_params(rows, 1, count, 0));
                assert!(result.is_err(), "rows={rows}, count={count}, kind={kind}");
                assert!(result.unwrap_err().to_string().contains("cannot be shifted off of the worksheet"), "{kind}");
                assert_eq!(*s.doc().unwrap().wb, *before, "rows={rows}, count={count}, kind={kind}");
                assert_eq!(s.doc().unwrap().undo.len(), undo, "{kind}");
            }
        }
    }
}

#[test]
fn move_lines_accepts_each_tail_store_just_below_the_overflow_threshold() {
    for rows in [true, false] {
        for count in [1, 2] {
            let limit = if rows { MAX_ROWS } else { MAX_COLS };
            for kind in TAIL_KINDS {
                let mut s = s();
                s.execute("cell.set", json!({"cell":axis_cell(rows, 1).a1(), "input":"moved"})).unwrap();
                add_tail(std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap(), kind, rows, limit - count - 1);
                let before = s.doc().unwrap().wb.clone();
                let undo = s.doc().unwrap().undo.len();
                let result = s.execute(move_command(rows), move_params(rows, 1, count, 0));
                assert!(result.is_ok(), "rows={rows}, count={count}, kind={kind}: {result:?}");
                assert_eq!(v(&s, "A1"), Value::Text("moved".into()), "{kind}");
                assert_eq!(s.doc().unwrap().undo.len(), undo + 1, "{kind}");
                // The structural recalc legitimately rebuilds synthetic spill caches. For all
                // persistent metadata, moving the marker back must leave the tail unchanged.
                let after = s.doc().unwrap().wb.clone();
                if !kind.starts_with("spill") {
                    let mut expected = (*before).clone();
                    let sh = expected.sheet_mut(0).unwrap();
                    let marker = sh.cells.get(axis_cell(rows, 1)).unwrap().clone();
                    sh.cells.remove(axis_cell(rows, 1));
                    sh.cells.set(CellRef::new(0, 0), marker);
                    assert_eq!(*after, expected, "rows={rows}, count={count}, kind={kind}");
                }
                s.execute("edit.undo", json!({})).unwrap();
                assert_eq!(*s.doc().unwrap().wb, *before, "{kind}");
                s.execute("edit.redo", json!({})).unwrap();
                assert_eq!(*s.doc().unwrap().wb, *after, "{kind}");
            }
        }
    }
}

fn array_session(rows: bool) -> Session {
    let mut s = s();
    s.execute("selection.set", json!({"range":if rows {"B3:B5"} else {"C2:E2"}})).unwrap();
    s.execute("cell.set", json!({"cell":if rows {"B3"} else {"C2"}, "input":if rows {"=ROW(A3:A5)"} else {"=COLUMN(C1:E1)"}, "array":true})).unwrap();
    s
}

#[test]
fn move_lines_rejects_splitting_each_array_boundary_on_both_axes() {
    for rows in [true, false] {
        // Array occupies [2,4]. Isolate from, exclusive end and before, at an inner
        // boundary and at hi; the other two boundaries stay outside the array.
        for (from, count, before) in [(3, 3, 8), (4, 2, 8), (0, 3, 8), (0, 4, 8), (7, 1, 3), (7, 1, 4)] {
            let mut s = array_session(rows);
            assert_move_refused(&mut s, move_command(rows), move_params(rows, from, count, before), "array formula");
        }
    }
}

#[test]
fn move_lines_accepts_array_edges_and_moves_the_whole_formula() {
    for rows in [true, false] {
        // from==lo, end==lo and before==lo are valid; hi+1 is also outside.
        for (from, count, before, final_lo) in [(2, 3, 8, 5), (0, 2, 8, 0), (7, 1, 2, 3), (7, 1, 5, 2), (2, 3, 0, 0)] {
            let mut s = array_session(rows);
            let original = s.doc().unwrap().wb.clone();
            let undo = s.doc().unwrap().undo.len();
            s.execute(move_command(rows), move_params(rows, from, count, before)).unwrap();
            let sh = s.doc().unwrap().wb.active().unwrap();
            let at = if rows { CellRef::new(final_lo, 1) } else { CellRef::new(1, final_lo) };
            let end = if rows { CellRef::new(final_lo + 2, 1) } else { CellRef::new(1, final_lo + 2) };
            let input = RangeRef::new(axis_cell(rows, final_lo), axis_cell(rows, final_lo + 2));
            let f = sh.cells.get(at).unwrap().formula.as_ref().unwrap();
            assert_eq!(f.array, Some(RangeRef::new(at, end)));
            assert_eq!(f.text, format!("{}({})", if rows { "ROW" } else { "COLUMN" }, input.a1()));
            assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
            let moved = s.doc().unwrap().wb.clone();
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(*s.doc().unwrap().wb, *original);
            s.execute("edit.redo", json!({})).unwrap();
            assert_eq!(*s.doc().unwrap().wb, *moved);
        }
    }
}

#[test]
fn move_lines_checks_anchor_far_corner_and_absolute_exemption() {
    for rows in [true, false] {
        let limit = if rows { MAX_ROWS } else { MAX_COLS };
        for mode in [AnchorMode::MoveOnly, AnchorMode::MoveAndSize, AnchorMode::Absolute] {
            for start_at_edge in [false, true] {
                let mut s = s();
                let sh = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
                let line = if start_at_edge { limit - 1 } else { limit - 2 };
                let size = if rows { sh.default_row_height } else { sh.default_col_width };
                let anchor = Anchor {
                    cell: axis_cell(rows, line),
                    mode,
                    width: if rows { 1.0 } else { size * 1.5 },
                    height: if rows { size * 1.5 } else { 1.0 },
                    ..Default::default()
                };
                // Verify the fixture: the far corner reaches the last valid line even
                // when the start is safe. Geometry lookup clamps beyond the grid edge.
                let far = if rows { sh.row_at(sh.row_top(line) + anchor.height as f64) } else { sh.col_at(sh.col_left(line) + anchor.width as f64) };
                assert_eq!(far, limit - 1);
                sh.controls.push(gridcraft_model::FormControl { id: 1, anchor, ..Default::default() });
                if mode == AnchorMode::Absolute {
                    let before = s.doc().unwrap().wb.clone();
                    let undo = s.doc().unwrap().undo.len();
                    s.execute(move_command(rows), move_params(rows, 1, 1, 0)).unwrap();
                    assert_eq!(s.doc().unwrap().wb.active().unwrap().controls[0].anchor, anchor);
                    assert_eq!(*s.doc().unwrap().wb, *before);
                    assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
                } else {
                    assert_move_refused(&mut s, move_command(rows), move_params(rows, 1, 1, 0), "cannot be shifted off of the worksheet");
                }
            }
        }
    }
}

#[test]
fn move_columns_carries_slicer_and_control_anchors() {
    let mut s = s();
    let anchor = Anchor { cell: CellRef::new(0, 1), mode: AnchorMode::MoveOnly, ..Default::default() };
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
    let absolute = Anchor { mode: AnchorMode::Absolute, ..anchor };
    sh.controls.push(gridcraft_model::FormControl { id: 2, anchor: absolute, ..Default::default() });
    let before = s.doc().unwrap().wb.clone();
    s.execute("sheet.moveColumns", json!({"cols":"B:C", "before":9})).unwrap();
    let after = s.doc().unwrap().wb.clone();
    let sh = after.active().unwrap();
    assert_eq!(sh.slicers[0].anchor.cell.col, 7);
    assert_eq!(sh.controls[0].anchor.cell.col, 7);
    assert_eq!(sh.controls[1].anchor, absolute);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().wb, *before);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().wb, *after);
}

#[test]
fn move_lines_checks_destination_insert_and_source_delete_protection_separately() {
    for rows in [true, false] {
        // Insertion is governed by sheet permission, not the destination cells' locks.
        for (insert, delete, unlock_source, allowed) in
            [(false, true, true, false), (true, false, true, false), (true, true, false, false), (true, true, true, true)]
        {
            let mut s = s();
            s.execute("cell.set", json!({"cell":axis_cell(rows, 1).a1(), "input":"move me"})).unwrap();
            if unlock_source {
                s.execute("home.lockCell", json!({"range":if rows {"2:2"} else {"B:B"}, "on":false})).unwrap();
            }
            s.execute("review.protectSheet", json!({"insertRows":insert, "insertColumns":insert, "deleteRows":delete, "deleteColumns":delete}))
                .unwrap();
            let params = move_params(rows, 1, 1, 4);
            if allowed {
                s.execute(move_command(rows), params).unwrap();
                assert_eq!(v(&s, &axis_cell(rows, 3).a1()), Value::Text("move me".into()));
            } else {
                assert_move_refused(&mut s, move_command(rows), params, "protected");
            }
        }
    }
}

#[test]
fn move_lines_preserves_external_line_metadata() {
    for rows in [true, false] {
        let mut s = s();
        let sh = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        let infos = if rows { &mut sh.rows } else { &mut sh.cols };
        for line in [0, 1, 2, 4, 6] {
            infos.insert(line, gridcraft_model::LineInfo { hidden: line % 2 == 0, size: Some(31.0 + line as f32), ..Default::default() });
        }
        let original = infos.clone();
        s.execute(move_command(rows), move_params(rows, 2, 1, 5)).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        let infos = if rows { &sh.rows } else { &sh.cols };
        for line in [0, 1, 6] {
            assert_eq!(infos.get(&line), original.get(&line));
        }
        assert_eq!(infos.get(&4), original.get(&2));
        assert_eq!(infos.get(&3), original.get(&4));
        assert!(!infos.contains_key(&2));
    }
}

#[test]
fn move_lines_handles_empty_sheets_start_and_past_used_range() {
    for rows in [true, false] {
        for populated in [false, true] {
            for before in [0, 10] {
                let mut s = s();
                if populated {
                    s.execute("cell.set", json!({"cell":axis_cell(rows, 2).a1(), "input":"marker"})).unwrap();
                }
                let original = s.doc().unwrap().wb.clone();
                let undo = s.doc().unwrap().undo.len();
                s.execute(move_command(rows), move_params(rows, 2, 1, before)).unwrap();
                assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
                let sh = s.doc().unwrap().wb.active().unwrap();
                if populated {
                    assert_eq!(sh.value(axis_cell(rows, if before == 0 { 0 } else { 9 })), Value::Text("marker".into()));
                    assert_eq!(sh.value(axis_cell(rows, 2)), Value::Empty);
                } else {
                    assert_eq!(*s.doc().unwrap().wb, *original);
                }
                s.execute("edit.undo", json!({})).unwrap();
                assert_eq!(*s.doc().unwrap().wb, *original);
            }
        }
    }
}

#[test]
fn move_lines_refuses_tail_hidden_state_created_by_public_commands() {
    for rows in [true, false] {
        let mut s = s();
        if rows {
            s.execute("home.hideRows", json!({"rows":"1048576:1048576"})).unwrap();
        } else {
            s.execute("home.hideColumns", json!({"cols":"XFD:XFD"})).unwrap();
        }
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert!(if rows { sh.rows.get(&(MAX_ROWS - 1)).unwrap().hidden } else { sh.cols.get(&(MAX_COLS - 1)).unwrap().hidden });
        assert!(sh.used_range().is_none(), "the metadata, not a tail cell, must trigger refusal");
        assert_move_refused(&mut s, move_command(rows), move_params(rows, 1, 1, 0), "cannot be shifted off of the worksheet");
    }
}
