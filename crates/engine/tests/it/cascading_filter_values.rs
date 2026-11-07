use std::sync::Arc;

use gridcraft_engine::{
    Session,
    cmd::data::{filter_selection, filter_values},
    core::{CellRef, Value},
    model::{Cell, Color, FilterCriterion, PyCode},
};
use serde_json::json;

#[test]
fn python_cells_offer_cached_results_without_exposing_or_changing_code() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Company", "Result"], ["East", 7], ["East", null], ["West", 99]]})).unwrap();
    let sh = Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
    for (at, value) in [("B2", Value::Number(7.0)), ("B3", Value::Empty), ("B4", Value::Number(99.0))] {
        // A loaded workbook can keep Python code alongside a cached or empty result.
        sh.set_cell(CellRef::parse(at).unwrap(), Cell { value, py: Some(Arc::new(PyCode::new("secret_input * 2"))), ..Default::default() });
    }
    s.execute("data.filter", json!({"range": "A1:B4"})).unwrap();
    s.execute("data.filterBy", json!({"column": "A", "values": ["East"]})).unwrap();
    let doc = s.doc().unwrap();
    let (before, revision, undo) = (doc.wb.clone(), doc.revision, doc.undo.len());
    assert_eq!(filter_values(&doc.wb, 0, 1), vec![("7".into(), true), ("(Blanks)".into(), true)]);
    assert_eq!(
        filter_selection(&doc.wb, 0, 1, &[("7".into(), false), ("(Blanks)".into(), false)]),
        vec![("7".into(), false), ("99".into(), true), ("(Blanks)".into(), false)],
        "eligible edits overlay the cached results; a hidden result keeps its checkbox"
    );
    let doc = s.doc().unwrap();
    assert!(Arc::ptr_eq(&before, &doc.wb), "neither query mutates or evaluates the workbook");
    assert_eq!((doc.revision, doc.undo.len()), (revision, undo));
    assert_eq!(doc.wb.active().unwrap().cell(CellRef::parse("B3").unwrap()).unwrap().py.as_ref().unwrap().code, "secret_input * 2");
}

#[test]
fn slicer_criteria_outside_the_sheet_filters_columns_are_still_applied() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name"], ["one"], ["two"]]})).unwrap();
    s.execute("range.setValues", json!({"range": "F1", "values": [["Region"], ["East"], ["West"]]})).unwrap();
    s.execute("insert.table", json!({"range": "F1:F3", "name": "Other", "header": true})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A3"})).unwrap();
    let id = s.execute("insert.slicer", json!({"table": "Other", "column": "Region"})).unwrap()["slicer"].as_u64().unwrap();

    s.execute("slicer.toggle", json!({"slicer": id, "value": "East"})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.autofilter.as_ref().unwrap().range.a1(), "A1:A3");
    assert!(!sh.is_row_hidden(1));
    assert!(sh.is_row_hidden(2));
    assert_eq!(filter_values(&s.doc().unwrap().wb, 0, 0), vec![("one".into(), true)]);

    s.execute("data.reapply", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().is_row_hidden(2));
    s.execute("slicer.clear", json!({"slicer": id})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1));
    assert!(!sh.is_row_hidden(2));
    assert_eq!(filter_values(&s.doc().unwrap().wb, 0, 0), vec![("one".into(), true), ("two".into(), true)]);
}

#[test]
fn choices_use_the_same_custom_and_statistical_criteria_as_filtering() {
    for (criterion, expected, mid_checked) in [
        (json!({"custom": {"op": ">=", "value": 20, "op2": "<", "value2": 100}}), vec!["mid", "upper"], true),
        (json!({"custom": {"op": "=", "value": 10, "op2": "=", "value2": 100, "and": false}}), vec!["high", "low"], false),
        (json!({"custom": {"op": "=", "value": "n?*"}}), vec!["text"], false),
        (json!({"top": {"count": 2}}), vec!["high", "upper"], false),
        (json!({"top": {"count": 50, "percent": true, "bottom": true}}), vec!["low", "mid"], true),
        (json!({"aboveAverage": true}), vec!["high"], false),
        (json!({"aboveAverage": false}), vec!["low", "mid", "upper"], true),
    ] {
        let mut s = Session::new();
        s.new_workbook();
        s.execute(
            "range.setValues",
            json!({"range": "A1", "values": [
                ["Name", "Amount"], ["low", 10], ["mid", 20], ["upper", 30], ["high", 100], ["blank", null], ["text", "n/a"]
            ]}),
        )
        .unwrap();
        s.execute("data.filter", json!({"range": "A1:B7"})).unwrap();
        let mut params = criterion.clone();
        params["column"] = json!("B");
        s.execute("data.filterBy", params).unwrap();
        let actual: Vec<_> = filter_values(&s.doc().unwrap().wb, 0, 0).into_iter().map(|(v, _)| v).collect();
        assert_eq!(actual, expected, "{criterion}");
        // The current column's own criterion must not narrow its alternatives.
        s.execute("data.filterBy", json!({"column": "A", "values": ["mid"]})).unwrap();
        let actual: Vec<_> = filter_values(&s.doc().unwrap().wb, 0, 0).into_iter().map(|(v, _)| v).collect();
        assert_eq!(actual, expected, "{criterion}, with the current column filtered");
        assert_eq!(
            filter_values(&s.doc().unwrap().wb, 0, 1),
            vec![("20".into(), mid_checked)],
            "the numeric column's criterion sets checkboxes without excluding alternatives: {criterion}"
        );
    }
}

#[test]
fn statistical_thresholds_use_the_original_range_not_other_columns_subset() {
    for (criterion, names, expected) in [
        (json!({"top": {"count": 2}}), vec!["low", "mid", "upper"], vec!["upper"]),
        (json!({"top": {"count": 50, "percent": true}}), vec!["low", "mid", "upper"], vec!["upper"]),
        (json!({"top": {"count": 2, "bottom": true}}), vec!["mid", "upper", "high"], vec!["mid"]),
        (json!({"top": {"count": 50, "percent": true, "bottom": true}}), vec!["mid", "upper", "high"], vec!["mid"]),
        (json!({"aboveAverage": true}), vec!["low", "mid", "upper"], vec![]),
        (json!({"aboveAverage": false}), vec!["low", "mid", "upper"], vec!["low", "mid", "upper"]),
    ] {
        let mut s = Session::new();
        s.new_workbook();
        s.execute(
            "range.setValues",
            json!({"range": "A1", "values": [
                ["Name", "Amount", "Job"], ["low", 10, "low"], ["mid", 20, "mid"], ["upper", 30, "upper"], ["high", 100, "high"]
            ]}),
        )
        .unwrap();
        s.execute("data.filter", json!({"range": "A1:C5"})).unwrap();
        s.execute("data.filterBy", json!({"column": "A", "values": names})).unwrap();
        let mut params = criterion.clone();
        params["column"] = json!("B");
        s.execute("data.filterBy", params).unwrap();
        // A and B both remain active when enumerating C. Subset statistics produce different jobs.
        let actual: Vec<_> = filter_values(&s.doc().unwrap().wb, 0, 2).into_iter().map(|(v, _)| v).collect();
        assert_eq!(actual, expected, "{criterion}");
    }
}

#[test]
fn choices_match_other_columns_but_keep_the_current_columns_unchecked_values() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "B3", "values": [
            ["Company", "Job", "Status"],
            ["East", "Alpha", "Open"],
            ["East", "Beta", "Open"],
            ["East", "Closed job", "Closed"],
            ["West", "Other company", "Open"],
            ["East", null, "Open"],
            ["East", "=\"\"", "Open"],
            ["East", "Outside range", "Open"]
        ]}),
    )
    .unwrap();
    s.execute("data.filter", json!({"range": "B3:D9"})).unwrap();
    s.execute("data.filterBy", json!({"column": "B", "values": ["East"]})).unwrap();
    s.execute("data.filterBy", json!({"column": "D", "values": ["Open"]})).unwrap();
    s.execute("data.filterBy", json!({"column": "C", "values": ["Alpha"]})).unwrap();

    let choices = |s: &Session| filter_values(&s.doc().unwrap().wb, 0, 2);
    let expected =
        |beta: bool, blanks: bool| -> Vec<(String, bool)> { vec![("Alpha".into(), !beta), ("Beta".into(), beta), ("(Blanks)".into(), blanks)] };
    assert_eq!(choices(&s), expected(false, false));
    s.execute("data.filterBy", json!({"column": "C", "values": ["Beta"], "blanks": true})).unwrap();
    assert_eq!(choices(&s), expected(true, true));
    s.execute("data.filterBy", json!({"column": "C", "clear": true})).unwrap();
    assert_eq!(choices(&s), vec![("Alpha".into(), true), ("Beta".into(), true), ("(Blanks)".into(), true)]);
    // Manual visibility is not a filter criterion, even when every eligible row is hidden.
    s.execute("home.hideRows", json!({"rows": "4:10"})).unwrap();
    assert_eq!(choices(&s), vec![("Alpha".into(), true), ("Beta".into(), true), ("(Blanks)".into(), true)]);
    assert!(filter_values(&s.doc().unwrap().wb, 0, 0).is_empty());
    assert!(filter_values(&s.doc().unwrap().wb, 0, 4).is_empty());
}

#[test]
fn a_header_only_table_has_no_data_values_or_blank_choice() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "B3", "values": [["Company", "Job"]]})).unwrap();
    s.execute("insert.table", json!({"range": "B3:C3", "header": true})).unwrap();
    s.execute("data.filter", json!({"range": "B3:C3"})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert!(sh.tables[0].header_row);
    assert_eq!(sh.tables[0].range.height(), 1);
    assert_eq!(sh.autofilter.as_ref().unwrap().range.a1(), "B3:C3");
    for col in [1, 2] {
        assert!(filter_values(&d.wb, 0, col).is_empty(), "headers are not values or blank data rows");
    }
}

#[test]
fn choices_respect_blank_and_formatted_value_criteria_on_other_columns() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [
            ["Name", "Value"], ["empty", null], ["formula", "=\"\""], ["spaces", " "], ["number", 12], ["other", 30]
        ]}),
    )
    .unwrap();
    s.execute("home.numberFormat", json!({"range": "B5:B6", "code": "0.00"})).unwrap();
    s.execute("data.filter", json!({"range": "A1:B6"})).unwrap();
    s.execute("data.filterBy", json!({"column": "B", "values": [], "blanks": true})).unwrap();
    assert_eq!(filter_values(&s.doc().unwrap().wb, 0, 0), vec![("empty".into(), true), ("formula".into(), true)]);
    for value in ["12", "12.00"] {
        s.execute("data.filterBy", json!({"column": "B", "values": [value]})).unwrap();
        assert_eq!(filter_values(&s.doc().unwrap().wb, 0, 0), vec![("number".into(), true)]);
    }
    s.execute("data.filterBy", json!({"column": "B", "values": ["absent"]})).unwrap();
    assert!(filter_values(&s.doc().unwrap().wb, 0, 0).is_empty());
}

#[test]
fn choices_match_color_criteria_and_do_not_infer_icon_matches_from_hidden_rows() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Color"], ["red", 1], ["plain", 2]]})).unwrap();
    s.execute("home.fillColor", json!({"range": "B2", "color": "#FF0000"})).unwrap();
    s.execute("home.fontColor", json!({"range": "B2", "color": "#FF0000"})).unwrap();
    s.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
    for criterion in [FilterCriterion::FillColor(Color::Rgb(0xFF0000)), FilterCriterion::FontColor(Color::Rgb(0xFF0000))] {
        let sh = Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        sh.autofilter.as_mut().unwrap().criteria = vec![(1, criterion)];
        s.execute("data.reapply", json!({})).unwrap();
        assert_eq!(filter_values(&s.doc().unwrap().wb, 0, 0), vec![("red".into(), true)]);
        assert_eq!(filter_selection(&s.doc().unwrap().wb, 0, 1, &[]), vec![("1".into(), true), ("2".into(), false)]);
    }
    let sh = Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
    sh.autofilter.as_mut().unwrap().criteria = vec![(1, FilterCriterion::Icon { set: "3Arrows".into(), id: Some(0) })];
    s.execute("data.reapply", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().is_row_hidden(2), "reapply preserves imported icon visibility");
    assert_eq!(
        filter_values(&s.doc().unwrap().wb, 0, 0),
        vec![("plain".into(), true), ("red".into(), true)],
        "unsupported icon criteria cannot distinguish manual and current-column hiding"
    );
}

#[test]
fn querying_a_table_backed_sheet_filter_is_read_only_and_preserves_xlsx_criteria() {
    for table in [false, true] {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "B3", "values": [["Company", "Job"], ["East", "Alpha"], ["East", "Beta"], ["West", "Other"]]}))
            .unwrap();
        if table {
            s.execute("insert.table", json!({"range": "B3:C6", "header": true})).unwrap();
        }
        s.execute("selection.set", json!({"cell": "B3"})).unwrap();
        s.execute("data.filterBy", json!({"column": "B", "values": ["East"]})).unwrap();
        let before = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
        assert_eq!(filter_values(&s.doc().unwrap().wb, 0, 2), vec![("Alpha".into(), true), ("Beta".into(), true)]);
        assert_eq!(before, gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap());
        let (reopened, _) = gridcraft_xlsx::read_xlsx(&before).unwrap();
        let sh = reopened.active().unwrap();
        let criteria = if table { &sh.tables[0].filter } else { &sh.autofilter.as_ref().unwrap().criteria };
        assert_eq!(criteria, &vec![(0, FilterCriterion::Values { values: vec!["East".into()], blanks: false })]);
        assert!(!sh.is_row_hidden(3) && !sh.is_row_hidden(4) && sh.is_row_hidden(5));
    }
}

#[test]
fn an_unrelated_tables_criteria_do_not_restrict_the_sheet_filter_choices() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "B3", "values": [["Company", "Job"], ["East", "Alpha"], ["East", "Beta"], ["West", "Other"]]}))
        .unwrap();
    s.execute("range.setValues", json!({"range": "F3", "values": [["Company", "Job"], ["West", "Unrelated"], ["East", "Separate"]]})).unwrap();
    s.execute("insert.table", json!({"range": "F3:G5", "header": true})).unwrap();
    let sh = Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
    sh.tables[0].filter = vec![(0, FilterCriterion::Values { values: vec!["West".into()], blanks: false })];
    s.execute("selection.set", json!({"cell": "B3"})).unwrap();
    s.execute("data.filter", json!({"range": "B3:C6"})).unwrap();
    s.execute("data.filterBy", json!({"column": "B", "values": ["East"]})).unwrap();
    assert_eq!(filter_values(&s.doc().unwrap().wb, 0, 2), vec![("Alpha".into(), true), ("Beta".into(), true)]);
}

#[test]
fn choices_are_bounded_at_the_last_sheet_row_and_empty_for_missing_data() {
    let mut s = Session::new();
    s.new_workbook();
    assert!(filter_values(&s.doc().unwrap().wb, 0, 0).is_empty());
    assert!(filter_values(&s.doc().unwrap().wb, 99, 0).is_empty());
    s.execute("range.setValues", json!({"range": "B1048574", "values": [["Company", "Job"], ["West", "Before last"], ["East", "Last East"]]}))
        .unwrap();
    s.execute("data.filter", json!({"range": "B1048574:C1048576"})).unwrap();
    s.execute("data.filterBy", json!({"column": "B", "values": ["East"]})).unwrap();
    assert_eq!(filter_values(&s.doc().unwrap().wb, 0, 2), vec![("Last East".into(), true)]);
}

#[test]
fn merging_hidden_choices_preserves_the_active_criterion() {
    for (criterion, checked) in [
        (json!({"values": ["7"], "blanks": true}), [false, true, false, false, true]),
        (json!({"custom": {"op": ">=", "value": 7}}), [false, true, true, true, false]),
        (json!({"top": {"count": 2}}), [false, false, true, true, false]),
        (json!({"aboveAverage": true}), [false, false, false, true, false]),
        (json!({"aboveAverage": false}), [true, true, true, false, false]),
    ] {
        let mut s = Session::new();
        s.new_workbook();
        s.execute(
            "range.setValues",
            json!({"range": "A1", "values": [["Region", "Amount"], ["East", 1], ["East", 9], ["West", 7], ["West", 20], ["West", null]]}),
        )
        .unwrap();
        s.execute("home.numberFormat", json!({"range": "B2:B6", "code": "0.00"})).unwrap();
        s.execute("data.filter", json!({"range": "A1:B6"})).unwrap();
        s.execute("data.filterBy", json!({"column": "A", "values": ["East"]})).unwrap();
        let mut params = criterion.clone();
        params["column"] = json!("B");
        s.execute("data.filterBy", params).unwrap();
        let doc = s.doc().unwrap();
        let choices = filter_values(&doc.wb, 0, 1);
        let selection = filter_selection(&doc.wb, 0, 1, &choices);
        let expected: Vec<_> =
            ["1.00", "7.00", "9.00", "20.00", "(Blanks)"].into_iter().zip(checked).map(|(label, on)| (label.to_string(), on)).collect();
        assert_eq!(selection, expected, "unchanged eligible choices must preserve hidden matches: {criterion}");
        let values: Vec<_> = selection.iter().filter(|(label, on)| *on && label != "(Blanks)").map(|(label, _)| label.clone()).collect();
        s.execute("data.filterBy", json!({"column": "B", "values": values, "blanks": checked[4]})).unwrap();
        s.execute("data.filterBy", json!({"column": "A", "clear": true})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        for (row, on) in [checked[0], checked[2], checked[1], checked[3], checked[4]].into_iter().enumerate() {
            assert_eq!(!sh.is_row_hidden(row as u32 + 1), on, "row after Apply and clearing the other filter: {criterion}");
        }
    }
}

#[test]
fn large_values_criteria_preserve_ascii_case_and_blank_matching() {
    use gridcraft_engine::core::RangeRef;
    use gridcraft_engine::model::AutoFilter;
    let mut s = Session::new();
    s.new_workbook();
    let sh = Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
    let mut selected = Vec::new();
    for row in 1..=5_000 {
        sh.set_cell(CellRef::new(row, 0), Cell { value: Value::Text(format!("item-{row}").into()), ..Default::default() });
        sh.set_cell(CellRef::new(row, 1), Cell { value: Value::Number(f64::from(row)), ..Default::default() });
        selected.push(format!("ITEM-{row}"));
    }
    selected.extend(["ITEM-1".into(), "Ä".into()]);
    for (row, value, label) in [
        (5_001, Value::Text("Ä".into()), "unicode selected"),
        (5_002, Value::Text("ä".into()), "unicode excluded"),
        (5_003, Value::Empty, "blank row"),
    ] {
        sh.set_cell(CellRef::new(row, 0), Cell { value, ..Default::default() });
        sh.set_cell(CellRef::new(row, 1), Cell { value: Value::Text(label.into()), ..Default::default() });
    }
    sh.autofilter = Some(AutoFilter {
        range: RangeRef::new(CellRef::new(0, 0), CellRef::new(5_003, 1)),
        criteria: vec![(0, FilterCriterion::Values { values: selected, blanks: true })],
    });
    let choices = filter_values(&s.doc().unwrap().wb, 0, 1);
    let mut expected: Vec<_> = (1..=5_000).map(|n| (n.to_string(), true)).collect();
    expected.extend([("blank row".into(), true), ("unicode selected".into(), true)]);
    assert_eq!(choices, expected);
}
