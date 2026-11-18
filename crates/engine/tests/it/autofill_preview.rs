//! Fill-handle prediction agrees with the committed engine fill without changing state.

use gridcraft_engine::{
    Session,
    core::{CellRef, RangeRef, Value},
    fill::{FillMode, preview_cell},
};

#[test]
fn preview_preserves_custom_list_precedence_and_mixed_text_patterns() {
    use gridcraft_engine::model::Cell;
    use std::sync::Arc;

    let mut s = Session::new();
    s.new_workbook();
    let doc = s.doc_mut().unwrap();
    let mut wb = (*doc.wb).clone();
    wb.custom_lists = vec![vec!["Monday".into(), "Reviewday".into()], vec!["Monday".into(), "Wrongday".into()]];
    for row in 0..4096 {
        let text = match row {
            0 => "Monday",
            1 => "JAN",
            2 => "Q4",
            3 => "Product 007",
            _ => "On backorder",
        };
        wb.sheet_mut(0).unwrap().set_cell(CellRef::new(row, 0), Cell { value: Value::text(text), ..Default::default() });
    }
    doc.wb = Arc::new(wb);
    let before = doc.wb.clone();
    let (revision, undo) = (doc.revision, doc.undo.len());
    let src = RangeRef::parse("A1:A4096").unwrap();
    let target = RangeRef::parse("A1:A4101").unwrap();
    let expected = ["Reviewday", "FEB", "Q1", "Product 008", "On backorder"];
    for (offset, text) in expected.iter().enumerate() {
        let at = CellRef::new(4096 + offset as u32, 0);
        assert_eq!(preview_cell(&doc.wb, 0, src, target, at, FillMode::Series).unwrap().value, Value::text(*text));
    }
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb));
    assert_eq!((s.doc().unwrap().revision, s.doc().unwrap().undo.len()), (revision, undo));
    s.execute("edit.autoFill", serde_json::json!({"source": "A1:A4096", "target": "A1:A4101"})).unwrap();
    for (offset, text) in expected.iter().enumerate() {
        assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::new(4096 + offset as u32, 0)), Value::text(*text));
    }
    assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
}

#[test]
fn preview_and_fill_preserve_python_code_and_its_cached_value() {
    use gridcraft_engine::model::{Cell, PyCode};
    use std::sync::Arc;

    for (mode, fill_mode) in [
        ("series", FillMode::Series),
        ("alternate", FillMode::Alternate),
        ("copy", FillMode::Copy),
        ("values", FillMode::ValuesOnly),
        ("formats", FillMode::Formats),
        ("extend", FillMode::Extend),
    ] {
        for (source, target, at) in [("B1", "B1:B4", "B4"), ("B4", "B1:B4", "B1"), ("B2", "B2:E2", "E2"), ("E2", "B2:E2", "B2")] {
            let mut s = Session::new();
            s.new_workbook();
            let src = RangeRef::parse(source).unwrap();
            let target_range = RangeRef::parse(target).unwrap();
            let at = CellRef::parse(at).unwrap();
            let original = Cell { value: Value::Number(7.0), py: Some(Arc::new(PyCode::new("xl('A1') * 2"))), ..Default::default() };
            let destination = Cell { value: Value::Number(99.0), py: Some(Arc::new(PyCode::new("destination = 99"))), ..Default::default() };
            // A loaded Python cell can hold cached output without a Python runtime.
            let doc = s.doc_mut().unwrap();
            let mut wb = (*doc.wb).clone();
            wb.sheet_mut(0).unwrap().set_cell(src.start, original.clone());
            wb.sheet_mut(0).unwrap().set_cell(at, destination.clone());
            doc.wb = Arc::new(wb);
            let before = doc.wb.clone();
            let (revision, undo) = (doc.revision, doc.undo.len());
            let expected = if fill_mode == FillMode::Formats { &destination } else { &original };
            let preview = preview_cell(&doc.wb, 0, src, target_range, at, fill_mode).unwrap();
            assert_eq!(&preview, expected, "{source} -> {target} ({mode})");
            let doc = s.doc().unwrap();
            assert!(Arc::ptr_eq(&before, &doc.wb), "prediction is read-only");
            assert_eq!((doc.revision, doc.undo.len()), (revision, undo));
            s.execute("edit.autoFill", serde_json::json!({"source": source, "target": target, "mode": mode})).unwrap();
            let doc = s.doc().unwrap();
            assert_eq!(doc.wb.active().unwrap().cell(at), Some(expected), "{source} -> {target} ({mode})");
            assert_eq!(doc.wb.active().unwrap().cell(src.start), Some(&original));
            assert_eq!(doc.undo.len(), undo + 1, "release makes one undo step");
            s.execute("edit.undo", serde_json::json!({})).unwrap();
            assert_eq!(s.doc().unwrap().wb.active().unwrap().cell(at), Some(&destination));
        }
    }
}

#[test]
fn preview_matches_committed_fill_in_both_directions_and_modes() {
    for (values, source, target, at, mode, expected) in [
        (serde_json::json!([[1], [2], [3]]), "B1:B3", "B1:B6", "B6", "series", "6"),
        (serde_json::json!([[1], [4], [4]]), "B1:B3", "B1:B6", "B6", "series", "9"),
        (serde_json::json!([[1], [2], [3]]), "B4:B6", "B1:B6", "B1", "series", "-2"),
        (serde_json::json!([[1, 2, 3]]), "B2:D2", "B2:G2", "G2", "series", "6"),
        (serde_json::json!([[1, 2, 3]]), "E2:G2", "B2:G2", "B2", "series", "-2"),
        (serde_json::json!([[1], [2], [3]]), "B1:B3", "B1:B5", "B5", "alternate", "2"),
        (serde_json::json!([[5]]), "B1", "B1:B4", "B4", "series", "5"),
        (serde_json::json!([[5]]), "B1", "B1:B4", "B4", "alternate", "8"),
        (serde_json::json!([["Item 007"]]), "B1", "B1:B4", "B4", "series", "Item 010"),
        (serde_json::json!([["Item 007"]]), "B4", "B1:B4", "B1", "series", "Item 004"),
        (serde_json::json!([["Item 007"]]), "B2", "B2:E2", "E2", "series", "Item 010"),
        (serde_json::json!([["Item 007"]]), "E2", "B2:E2", "B2", "series", "Item 004"),
        (serde_json::json!([["Monday"]]), "B1", "B1:B4", "B4", "series", "Thursday"),
        (serde_json::json!([["Product 1"], ["On backorder"]]), "B1:B2", "B1:B5", "B5", "series", "Product 3"),
        (serde_json::json!([["1/31/2026"]]), "B1", "B1:B4", "B4", "series", "2/3/2026"),
        (serde_json::json!([["1/31/2026"]]), "B4", "B1:B4", "B1", "series", "1/28/2026"),
        (serde_json::json!([["1/31/2026"]]), "B2", "B2:E2", "E2", "series", "2/3/2026"),
        (serde_json::json!([["1/31/2026"]]), "E2", "B2:E2", "B2", "series", "1/28/2026"),
        (serde_json::json!([["1/31/2026"], ["3/31/2026"]]), "B1:B2", "B1:B4", "B4", "series", "7/31/2026"),
        (serde_json::json!([["=A1+$A$1"]]), "B1", "B1:B4", "B4", "series", "=A4+$A$1"),
        (serde_json::json!([["=A4+$A$1"]]), "B4", "B1:B4", "B1", "series", "=A1+$A$1"),
        (serde_json::json!([["=A2+$A$1"]]), "B2", "B2:E2", "E2", "series", "=D2+$A$1"),
        (serde_json::json!([["=D2+$A$1"]]), "E2", "B2:E2", "B2", "series", "=A2+$A$1"),
    ] {
        let mut s = Session::new();
        s.new_workbook();
        let src = RangeRef::parse(source).unwrap();
        let target_range = RangeRef::parse(target).unwrap();
        let at = CellRef::parse(at).unwrap();
        s.execute("range.setValues", serde_json::json!({"range": src.start.a1(), "values": values})).unwrap();
        let d = s.doc().unwrap();
        let revision = d.revision;
        let undo = d.undo.len();
        let fill_mode = if mode == "alternate" { FillMode::Alternate } else { FillMode::Series };
        let preview = preview_cell(&d.wb, 0, src, target_range, at, fill_mode).unwrap();
        let text = if preview.formula.is_some() {
            preview.input_text()
        } else {
            gridcraft_engine::display::value_text_as(&d.wb, &preview.value, d.wb.styles.get(preview.style).num_fmt.as_str())
        };
        assert_eq!(text, expected, "{source} -> {target} ({mode})");
        assert_eq!((s.doc().unwrap().revision, s.doc().unwrap().undo.len()), (revision, undo));
        s.execute("edit.autoFill", serde_json::json!({"source": source, "target": target, "mode": mode})).unwrap();
        let committed = s.doc().unwrap().wb.active().unwrap().cell(at).unwrap();
        assert_eq!(preview.formula, committed.formula);
        if preview.formula.is_none() {
            assert_eq!(preview.value, committed.value);
        }
        assert_eq!(preview.style, committed.style);
    }
}

#[test]
fn preview_bounds_work_by_source_lane_instead_of_drag_distance() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", serde_json::json!({"range": "A1", "values": [[1, 10], [2, 20]]})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let src = RangeRef::parse("A1:B2").unwrap();
    let target = RangeRef::parse("A1:B1048576").unwrap();
    let at = CellRef::parse("B1048576").unwrap();
    assert_eq!(preview_cell(wb, 0, src, target, at, FillMode::Series).unwrap().value, Value::number(10_485_760.0));
    let huge_source = RangeRef::parse("A1:A1048575").unwrap();
    assert!(
        preview_cell(wb, 0, huge_source, RangeRef::parse("A1:A1048576").unwrap(), CellRef::parse("A1048576").unwrap(), FillMode::Series).is_none()
    );
    assert!(preview_cell(wb, 0, src, src, src.start, FillMode::Series).is_none());
    assert!(preview_cell(wb, 0, RangeRef::parse("A1:F2").unwrap(), RangeRef::parse("A1:F1048576").unwrap(), at, FillMode::Series).is_none());
}

#[test]
fn preview_rejects_rectangles_that_the_fill_command_refuses() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", serde_json::json!({"cell": "A1", "input": "5"})).unwrap();
    for (source, target, at) in [("A1:E1", "A1:E1000001", "A1000001"), ("A1:XFD400", "A1:XFD401", "A401")] {
        let (src, target_range, at) = (RangeRef::parse(source).unwrap(), RangeRef::parse(target).unwrap(), CellRef::parse(at).unwrap());
        assert!(preview_cell(&s.doc().unwrap().wb, 0, src, target_range, at, FillMode::Series).is_none(), "{source} -> {target}");
        assert!(s.execute("edit.autoFill", serde_json::json!({"source": source, "target": target})).is_err());
    }
}
