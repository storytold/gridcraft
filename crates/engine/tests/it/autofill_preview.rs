//! Fill-handle prediction agrees with the committed engine fill without changing state.

use gridcraft_engine::{
    Session,
    core::{CellRef, RangeRef, Value},
    fill::{FillMode, preview_cell},
};

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
        (serde_json::json!([["Monday"]]), "B1", "B1:B4", "B4", "series", "Thursday"),
        (serde_json::json!([["Product 1"], ["On backorder"]]), "B1:B2", "B1:B5", "B5", "series", "Product 3"),
        (serde_json::json!([["1/31/2026"]]), "B1", "B1:B4", "B4", "series", "2/3/2026"),
        (serde_json::json!([["1/31/2026"], ["3/31/2026"]]), "B1:B2", "B1:B4", "B4", "series", "7/31/2026"),
        (serde_json::json!([["=A1+$A$1"]]), "B1", "B1:B4", "B4", "series", "=A4+$A$1"),
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
