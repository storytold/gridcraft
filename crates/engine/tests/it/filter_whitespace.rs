use gridcraft_engine::Session;
use serde_json::json;

/// Value-list and equality criteria ignore leading and trailing spaces (#2911).
#[test]
fn autofilter_ignores_surrounding_spaces_2911() {
    let mut s = Session::new();
    s.new_workbook();

    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [
                ["Name"],
                ["Card"],
                ["Card "],
                [" Card"],
                ["  Card  "],
                ["card"],
                ["CARD"],
                ["Cardx"],
                ["Ca rd"],
                [" "],
                [""]
            ]
        }),
    )
    .unwrap();

    s.execute("data.filter", json!({"range": "A1:A11"})).unwrap();

    for value in ["Card", " Card", "Card "] {
        let result = s.execute("data.filterBy", json!({"column": "A", "values": [value]})).unwrap();

        assert_eq!(result["hiddenRows"], json!(4), "{value:?}");

        let sh = s.doc().unwrap().wb.active().unwrap();

        for row in 1..=6 {
            assert!(!sh.is_row_hidden(row), "row {row} should be visible for {value:?}");
        }

        for row in 7..=10 {
            assert!(sh.is_row_hidden(row), "row {row} should be hidden for {value:?}");
        }
    }

    for value in ["Card", " Card", "Card "] {
        let result = s
            .execute(
                "data.filterBy",
                json!({
                    "column": "A",
                    "custom": {"op": "=", "value": value}
                }),
            )
            .unwrap();

        assert_eq!(result["hiddenRows"], json!(4), "{value:?}");

        let sh = s.doc().unwrap().wb.active().unwrap();

        for row in 1..=6 {
            assert!(!sh.is_row_hidden(row), "{value:?}: row {row}");
        }

        for row in 7..=10 {
            assert!(sh.is_row_hidden(row), "{value:?}: row {row}");
        }
    }
}

/// Wildcard custom equality trims surrounding spaces before matching (#2911).
#[test]
fn filter_whitespace_wildcard_2911() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "parse":false,
               "values":[["Name"], [" Card "], ["Cardx"], ["Other"]]}),
    )
    .unwrap();
    s.execute("data.filter", json!({"range":"A1:A4"})).unwrap();

    let r = s.execute("data.filterBy", json!({"column":"A", "custom":{"op":"=", "value":"Card*"}})).unwrap();
    assert_eq!(r["hiddenRows"], json!(1));

    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1));
    assert!(!sh.is_row_hidden(2));
    assert!(sh.is_row_hidden(3));
}

/// Custom `=` with spaces matches empty and whitespace-only cells as in Excel (#2911).
#[test]
fn filter_whitespace_custom_blank_distinction_2911() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "parse":false,
               "values":[["Name"], [null], [""], [" "], ["   "], ["Card"]]}),
    )
    .unwrap();
    s.execute("data.filter", json!({"range":"A1:A6"})).unwrap();

    let r = s.execute("data.filterBy", json!({"column":"A", "custom":{"op":"=", "value":" "}})).unwrap();
    assert_eq!(r["hiddenRows"], json!(1));

    let sh = s.doc().unwrap().wb.active().unwrap();
    for row in 1..=4 {
        assert!(!sh.is_row_hidden(row), "row {row} should be visible");
    }
    assert!(sh.is_row_hidden(5));
}

/// Custom `<>` trims surrounding spaces before excluding matching text (#2911).
#[test]
fn filter_whitespace_custom_not_equal_2911() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "parse":false,
               "values":[["Name"], [" Card "], ["Card"], ["Other"]]}),
    )
    .unwrap();
    s.execute("data.filter", json!({"range":"A1:A4"})).unwrap();

    let r = s.execute("data.filterBy", json!({"column":"A", "custom":{"op":"<>", "value":"Card"}})).unwrap();
    assert_eq!(r["hiddenRows"], json!(2));

    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.is_row_hidden(1));
    assert!(sh.is_row_hidden(2));
    assert!(!sh.is_row_hidden(3));
}

/// Empty and whitespace-only custom `=` and `<>` criteria match Excel Desktop behavior (#2911).
#[test]
fn filter_whitespace_empty_criteria_regression_2911() {
    let mut s = Session::new();
    s.new_workbook();

    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [
                ["Name"],
                [null],
                [""],
                [" "],
                ["   "],
                ["Card"]
            ]
        }),
    )
    .unwrap();

    s.execute("data.filter", json!({"range": "A1:A6"})).unwrap();

    for (op, criterion, expected_visible) in [("=", "", vec![1, 2, 3, 4]), ("=", " ", vec![1, 2, 3, 4]), ("<>", "", vec![5]), ("<>", " ", vec![5])] {
        s.execute(
            "data.filterBy",
            json!({
                "column": "A",
                "custom": {
                    "op": op,
                    "value": criterion
                }
            }),
        )
        .unwrap();

        let sh = s.doc().unwrap().wb.active().unwrap();

        let visible: Vec<_> = (1..=5).filter(|&row| !sh.is_row_hidden(row)).collect();

        assert_eq!(visible, expected_visible, "op={op:?}, criterion={criterion:?}");
    }
}

/// Selecting `(Blanks)` in the AutoFilter dropdown includes whitespace-only text cells (#2943).
#[test]
fn filter_whitespace_dropdown_blanks_excel_2943() {
    let mut s = Session::new();
    s.new_workbook();

    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [
                ["Name"],
                [null],
                [""],
                [" "],
                ["   "],
                ["Card"]
            ]
        }),
    )
    .unwrap();

    s.execute("data.filter", json!({"range":"A1:A6"})).unwrap();

    let result = s
        .execute(
            "data.filterBy",
            json!({
                "column": "A",
                "values": [],
                "blanks": true
            }),
        )
        .unwrap();

    assert_eq!(result["hiddenRows"], json!(1));

    let sh = s.doc().unwrap().wb.active().unwrap();

    for row in 1..=4 {
        assert!(!sh.is_row_hidden(row), "Excel (Blanks) should include row {row}");
    }

    assert!(sh.is_row_hidden(5));
}

/// Filtering by a selected whitespace-only cell (`values: ["   "]` with `blanks` omitted) keeps blank rows visible (#2943).
#[test]
fn filter_by_selected_whitespace_cell_keeps_blanks_visible_2943() {
    let mut s = Session::new();
    s.new_workbook();

    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [
                ["Name"],
                [null],
                [""],
                [" "],
                ["   "],
                ["Card"]
            ]
        }),
    )
    .unwrap();

    s.execute("data.filter", json!({"range": "A1:A6"})).unwrap();

    // Simulates "Filter by Selected Cell's Value" (`values: [text]`, no `blanks` key).
    let result = s.execute("data.filterBy", json!({"column": "A", "values": ["   "]})).unwrap();
    assert_eq!(result["hiddenRows"], json!(1));

    let sh = s.doc().unwrap().wb.active().unwrap();
    for row in 1..=4 {
        assert!(!sh.is_row_hidden(row), "whitespace/blank row {row} must stay visible");
    }
    assert!(sh.is_row_hidden(5));

    // Explicit `blanks: false` from the dropdown overrides whitespace inference and hides blank/whitespace rows.
    let result = s.execute("data.filterBy", json!({"column": "A", "values": ["Card", "   "], "blanks": false})).unwrap();
    assert_eq!(result["hiddenRows"], json!(4));
    let sh = s.doc().unwrap().wb.active().unwrap();
    for row in 1..=4 {
        assert!(sh.is_row_hidden(row), "row {row} should be hidden when blanks: false");
    }
    assert!(!sh.is_row_hidden(5));
}

/// Legacy saved criteria with whitespace-only `<filter val="   "/>` and `blanks: false` migrate on load and keep rows visible on reapply (#2943).
#[test]
fn restored_whitespace_only_criterion_keeps_rows_visible_on_reapply_2943() {
    use gridcraft_engine::cmd::data::{FilterChoice, filter_choices};
    use gridcraft_model::{AutoFilter, FilterCriterion};

    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [
                ["Name"],
                ["   "],
                ["Card"]
            ]
        }),
    )
    .unwrap();

    // Simulate a legacy saved workbook whose filter stored `values: ["   "]` with `blanks: false`.
    {
        let wb = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb);
        wb.sheet_mut(0).unwrap().autofilter = Some(AutoFilter {
            range: gridcraft_core::RangeRef::parse("A1:A3").unwrap(),
            criteria: vec![(0, FilterCriterion::Values { values: vec!["   ".into()], blanks: false })],
        });
    }
    // 1. Direct in-memory legacy criterion (`blanks: false`, `values: ["   "]`) keeps rows visible on reapply.
    s.execute("data.reapply", json!({})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "in-memory legacy whitespace-only row must stay visible after reapply");
    assert!(sh.is_row_hidden(2), "non-matching row must be hidden after reapply");

    // 2. XLSX save + reopen migrates `<filter val="   "/>` and keeps rows visible on reapply.
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (reopened, _) = gridcraft_engine::io::open_bytes("legacy_filter.xlsx", &bytes).unwrap();
    s.active_mut().unwrap().wb = std::sync::Arc::new(reopened);

    s.execute("data.reapply", json!({})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "restored whitespace-only row must stay visible after reapply");
    assert!(sh.is_row_hidden(2), "non-matching row must be hidden after reapply");

    // Also verify Show Formulas mode and literal `(Blanks)` vs `'(Blanks)'` dropdown separation.
    let mut s2 = Session::new();
    s2.new_workbook();
    s2.execute("cell.set", json!({"cell": "A1", "input": "Name"})).unwrap();
    s2.execute("cell.set", json!({"cell": "A2", "input": "=\" Card \""})).unwrap();
    s2.execute("range.setValues", json!({"range": "A3", "parse": false, "values": [[" =\" Card \" "], ["(Blanks)"], ["'(Blanks)'"], [" "]]}))
        .unwrap();
    s2.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s2.execute("data.filterBy", json!({"column": "A", "values": ["Card"], "blanks": false})).unwrap();

    let choices = filter_choices(&s2.doc().unwrap().wb, 0, 0);
    // Formula `=" Card "` and literal ` =" Card " ` stay separate under Show Formulas; only the formula row is checked.
    let formula_like: Vec<_> = choices.iter().filter(|c| c.label.contains("=\" Card \"")).collect();
    assert_eq!(formula_like.len(), 2, "formula and literal text must keep distinct filter identities: {choices:?}");
    assert_eq!(formula_like.iter().filter(|c| c.checked).count(), 1, "only the matching formula row is checked: {choices:?}");

    // Literal `(Blanks)`, literal `'(Blanks)'`, and the actual `(Blanks)` toggle all have distinct labels and values.
    assert!(choices.contains(&FilterChoice { label: "(Blanks)".into(), value: None, checked: false }), "{choices:?}");
    assert!(choices.contains(&FilterChoice { label: "'(Blanks)'".into(), value: Some("(Blanks)".into()), checked: false }), "{choices:?}");
    assert!(choices.contains(&FilterChoice { label: "''(Blanks)''".into(), value: Some("'(Blanks)'".into()), checked: false }), "{choices:?}");
}
