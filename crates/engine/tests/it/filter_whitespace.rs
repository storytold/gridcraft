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

/// Legacy saved criteria with whitespace-only `<filter val="   "/>` and `blanks: false` migrate on load and update row visibility immediately (#2943).
#[test]
fn restored_whitespace_only_criterion_keeps_rows_visible_on_reapply_2943() {
    use gridcraft_engine::cmd::data::{filter_choices, filter_choices_params};
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
                [null],
                ["   "],
                ["Card"]
            ]
        }),
    )
    .unwrap();

    // Simulate a legacy saved workbook whose filter stored `values: ["   "]` with `blanks: false`,
    // and where the genuinely empty row (row 1 = A2) was hidden by the pre-fix blank branch.
    {
        let wb = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb);
        let sh = wb.sheet_mut(0).unwrap();
        sh.autofilter = Some(AutoFilter {
            range: gridcraft_core::RangeRef::parse("A1:A4").unwrap(),
            criteria: vec![(0, FilterCriterion::Values { values: vec!["   ".into()], blanks: false, identities: vec![] })],
        });
        sh.rows.entry(1).or_default().hidden = true;
        sh.rows.entry(3).or_default().hidden = true;
    }

    // 1. XLSX save + reopen migrates `<filter val="   "/>` AND reapplies immediately on open so
    //    the genuinely empty row A2 is visible before any user `data.reapply` command.
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (reopened, _) = gridcraft_engine::io::open_bytes("legacy_filter.xlsx", &bytes).unwrap();
    {
        let sh = reopened.sheet(0).unwrap();
        assert!(!sh.is_row_hidden(1), "genuinely empty row must be visible immediately on open");
        assert!(!sh.is_row_hidden(2), "whitespace-only row must be visible immediately on open");
        assert!(sh.is_row_hidden(3), "non-matching row must remain hidden on open");
    }
    s.active_mut().unwrap().wb = std::sync::Arc::new(reopened);

    // 2. Direct in-memory legacy criterion (`blanks: false`, `values: ["   "]`) also keeps blank rows visible on reapply.
    {
        let wb = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb);
        wb.sheet_mut(0).unwrap().autofilter = Some(AutoFilter {
            range: gridcraft_core::RangeRef::parse("A1:A4").unwrap(),
            criteria: vec![(0, FilterCriterion::Values { values: vec!["   ".into()], blanks: false, identities: vec![] })],
        });
    }
    s.execute("data.reapply", json!({})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "in-memory legacy empty row must stay visible after reapply");
    assert!(!sh.is_row_hidden(2), "in-memory legacy whitespace-only row must stay visible after reapply");
    assert!(sh.is_row_hidden(3), "non-matching row must be hidden after reapply");

    // 3. Verify Show Formulas mode: distinct qualified labels, preserved checklist identity on Apply,
    //    single label for ordinary whitespace variants, and literal `(Blanks)` vs `'(Blanks)'` separation.
    let mut s2 = Session::new();
    s2.new_workbook();
    s2.execute("cell.set", json!({"cell": "A1", "input": "Name"})).unwrap();
    s2.execute("cell.set", json!({"cell": "A2", "input": "=\" Card \""})).unwrap();
    s2.execute(
        "range.setValues",
        json!({
            "range": "A3",
            "parse": false,
            "values": [[" =\" Card \" "], ["Card"], [" Card "], ["(Blanks)"], ["'(Blanks)'"], [" "]]
        }),
    )
    .unwrap();
    s2.execute("formulas.showFormulas", json!({"on": true})).unwrap();

    // With no active criterion, ordinary whitespace variants `"Card"` and `" Card "` merge into one `"Card"` label,
    // while formula `=" Card "` and literal `" =\" Card \" "` have distinct qualified labels.
    s2.execute("data.filter", json!({"range": "A1:A8"})).unwrap();
    let initial_choices = filter_choices(&s2.doc().unwrap().wb, 0, 0);
    assert_eq!(initial_choices.iter().filter(|c| c.label == "Card").count(), 1, "{initial_choices:?}");
    let formula_like_labels: Vec<_> = initial_choices.iter().filter(|c| c.label.starts_with("=\" Card \"")).map(|c| c.label.as_str()).collect();
    assert_eq!(formula_like_labels.len(), 2, "{initial_choices:?}");
    assert_ne!(formula_like_labels[0], formula_like_labels[1], "colliding identities must have distinct labels: {initial_choices:?}");

    // Filtering by `"Card"` checks `"Card"` and formula `=" Card "`, leaving literal `" =\" Card \" "` unchecked.
    s2.execute("data.filterBy", json!({"column": "A", "values": ["Card"], "blanks": false})).unwrap();
    let choices = filter_choices(&s2.doc().unwrap().wb, 0, 0);
    let formula_entry = choices.iter().find(|c| c.label == "=\" Card \" (result: Card)").expect("formula choice");
    let literal_entry = choices.iter().find(|c| c.label == "=\" Card \" (text)").expect("literal choice");
    assert!(formula_entry.checked, "{choices:?}");
    assert!(!literal_entry.checked, "{choices:?}");

    // Clicking Apply without changing the checks keeps the literal `" =\" Card \" "` row hidden.
    let unchanged_checks: Vec<_> = choices.iter().map(|c| (c, c.checked)).collect();
    s2.execute("data.filterBy", filter_choices_params("A", &unchanged_checks)).unwrap();
    let sh2 = s2.doc().unwrap().wb.active().unwrap();
    assert!(!sh2.is_row_hidden(1), "formula row A2 stays visible");
    assert!(sh2.is_row_hidden(2), "literal row A3 stays hidden");
    assert!(!sh2.is_row_hidden(3), "Card row A4 stays visible");
    assert!(!sh2.is_row_hidden(4), "whitespace Card row A5 stays visible");

    // Selecting ONLY the formula checkbox leaves the literal row hidden (and vice versa).
    let only_formula: Vec<_> = choices.iter().map(|c| (c, c.label == "=\" Card \" (result: Card)")).collect();
    s2.execute("data.filterBy", filter_choices_params("A", &only_formula)).unwrap();
    let sh2 = s2.doc().unwrap().wb.active().unwrap();
    assert!(!sh2.is_row_hidden(1), "formula row A2 visible");
    assert!(sh2.is_row_hidden(2), "literal row A3 hidden");
    assert!(sh2.is_row_hidden(3), "Card row A4 hidden");

    let only_literal: Vec<_> = choices.iter().map(|c| (c, c.label == "=\" Card \" (text)")).collect();
    s2.execute("data.filterBy", filter_choices_params("A", &only_literal)).unwrap();
    let sh2 = s2.doc().unwrap().wb.active().unwrap();
    assert!(sh2.is_row_hidden(1), "formula row A2 hidden");
    assert!(!sh2.is_row_hidden(2), "literal row A3 visible");
    assert!(sh2.is_row_hidden(3), "Card row A4 hidden");

    // Literal `(Blanks)`, literal `'(Blanks)'`, and the actual `(Blanks)` toggle all have distinct labels and values.
    assert!(choices.iter().any(|c| c.label == "(Blanks)" && c.value.is_none() && c.identity.is_none()), "{choices:?}");
    assert!(choices.iter().any(|c| c.label == "'(Blanks)'" && c.value.as_deref() == Some("(Blanks)")), "{choices:?}");
    assert!(choices.iter().any(|c| c.label == "''(Blanks)''" && c.value.as_deref() == Some("'(Blanks)'")), "{choices:?}");
}

/// Nonblank cells formatted to display only spaces stay selectable in both dropdown and selected-cell actions (#2943).
#[test]
fn nonblank_space_formatted_cells_stay_selectable_2943() {
    use gridcraft_core::CellRef;
    use gridcraft_engine::cmd::data::{filter_by_cell_params, filter_choices, filter_choices_params};

    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "Val"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "42"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "A2", "format": "\" \""})).unwrap();
    s.execute("range.setValues", json!({"range": "A3", "parse": false, "values": [[null], ["   "], ["Card"]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A5"})).unwrap();

    // 1. Dropdown checklist: selecting only the space-formatted `42` entry keeps row A2 visible and hides blanks.
    let choices = filter_choices(&s.doc().unwrap().wb, 0, 0);
    let num_choice = choices.iter().find(|c| c.value.as_deref() == Some("42")).expect("numeric 42 choice");
    assert_eq!(num_choice.label, "42");
    let only_42: Vec<_> = choices.iter().map(|c| (c, c.value.as_deref() == Some("42"))).collect();
    s.execute("data.filterBy", filter_choices_params("A", &only_42)).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "space-formatted 42 row A2 must stay visible");
    assert!(sh.is_row_hidden(2), "empty row A3 must be hidden");
    assert!(sh.is_row_hidden(3), "whitespace text row A4 must be hidden");
    assert!(sh.is_row_hidden(4), "Card row A5 must be hidden");

    // 2. Selected-cell action (`filter_by_cell_params`): on A2 (`42` formatted as `" "`) selects `42`, not blanks;
    //    on A4 (`"   "` text) selects blanks, not `42`.
    s.execute("data.clearFilter", json!({})).unwrap();
    let params_a2 = {
        let d = s.doc().unwrap();
        filter_by_cell_params(&d.wb, d.wb.active().unwrap(), CellRef::parse("A2").unwrap(), "A")
    };
    s.execute("data.filterBy", params_a2).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "A2 selected-cell filter keeps 42 visible");
    assert!(sh.is_row_hidden(2), "A2 selected-cell filter hides empty row");
    assert!(sh.is_row_hidden(3), "A2 selected-cell filter hides whitespace text row");

    let params_a4 = {
        let d = s.doc().unwrap();
        filter_by_cell_params(&d.wb, d.wb.active().unwrap(), CellRef::parse("A4").unwrap(), "A")
    };
    s.execute("data.filterBy", params_a4).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.is_row_hidden(1), "A4 selected-cell filter hides 42");
    assert!(!sh.is_row_hidden(2), "A4 selected-cell filter keeps empty row visible");
    assert!(!sh.is_row_hidden(3), "A4 selected-cell filter keeps whitespace text row visible");

    // 3. Formatted-blank number `42` vs formatted-blank text `"42"` (`;;;`) keep distinct qualified labels,
    //    and a >11-digit General number (`1234567890123`) keeps its checklist identity across Show Formulas toggles.
    let mut s3 = Session::new();
    s3.new_workbook();
    s3.execute("cell.set", json!({"cell": "A1", "input": "Val"})).unwrap();
    s3.execute("cell.set", json!({"cell": "A2", "input": "42"})).unwrap();
    s3.execute("home.numberFormat", json!({"range": "A2", "format": "\" \""})).unwrap();
    s3.execute("range.setValues", json!({"range": "A3", "parse": false, "values": [["42"]]})).unwrap();
    s3.execute("home.numberFormat", json!({"range": "A3", "format": ";;;"})).unwrap();
    s3.execute("cell.set", json!({"cell": "A4", "input": "1234567890123"})).unwrap();
    s3.execute("data.filter", json!({"range": "A1:A4"})).unwrap();

    let c3 = filter_choices(&s3.doc().unwrap().wb, 0, 0);
    assert!(c3.iter().any(|c| c.label == "42 (formatted blank, number)"), "{c3:?}");
    assert!(c3.iter().any(|c| c.label == "42 (formatted blank, text)"), "{c3:?}");

    let only_big: Vec<_> = c3.iter().map(|c| (c, c.value.as_deref() == Some("1.23457E+12"))).collect();
    s3.execute("data.filterBy", filter_choices_params("A", &only_big)).unwrap();
    s3.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s3.execute("data.reapply", json!({})).unwrap();
    assert!(!s3.doc().unwrap().wb.active().unwrap().is_row_hidden(3), "1234567890123 stays visible across Show Formulas toggle");
}

/// Checklist selections survive number-format changes, Show Formulas toggles, and XLSX save/reopen (#2943).
#[test]
fn checklist_identity_survives_format_change_show_formulas_and_xlsx_2943() {
    use gridcraft_engine::cmd::data::{filter_choices, filter_choices_params};

    // 1. Numeric `42` selected under General stays selected after formatting as `" "` and running `data.reapply`,
    //    without matching literal text `"42"`.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "Val"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "42"})).unwrap();
    s.execute("range.setValues", json!({"range": "A3", "parse": false, "values": [["42"], ["Other"]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();

    let c1 = filter_choices(&s.doc().unwrap().wb, 0, 0);
    let only_num: Vec<_> = c1.iter().map(|c| (c, c.label == "42 (number)")).collect();
    s.execute("data.filterBy", filter_choices_params("A", &only_num)).unwrap();
    s.execute("home.numberFormat", json!({"range": "A2", "format": "\" \""})).unwrap();
    s.execute("data.reapply", json!({})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "numeric 42 must stay visible after formatting as space");
    assert!(sh.is_row_hidden(2), "literal text '42' must stay hidden");
    assert!(sh.is_row_hidden(3), "Other row must stay hidden");

    // 2. Show Formulas ON -> select only formula `="Card"` -> turn Show Formulas OFF ->
    //    checklist keeps `="Card"` checked and `"Card"`/`" Card "` merged & unchecked -> Apply keeps literal hidden.
    let mut sf = Session::new();
    sf.new_workbook();
    sf.execute("cell.set", json!({"cell": "A1", "input": "Name"})).unwrap();
    sf.execute("cell.set", json!({"cell": "A2", "input": "=\" Card \""})).unwrap();
    sf.execute("range.setValues", json!({"range": "A3", "parse": false, "values": [["Card"], [" Card "], ["Other"]]})).unwrap();
    sf.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    sf.execute("data.filter", json!({"range": "A1:A5"})).unwrap();

    let on_choices = filter_choices(&sf.doc().unwrap().wb, 0, 0);
    let only_formula: Vec<_> = on_choices.iter().map(|c| (c, c.label.starts_with("=\" Card \""))).collect();
    sf.execute("data.filterBy", filter_choices_params("A", &only_formula)).unwrap();

    // Save + reopen XLSX while formula identity is active, then `data.reapply` must still hide literal `"Card"`.
    let bytes = gridcraft_xlsx::write_xlsx(&sf.doc().unwrap().wb).unwrap();
    let (reopened, _) = gridcraft_engine::io::open_bytes("formula_identity.xlsx", &bytes).unwrap();
    sf.active_mut().unwrap().wb = std::sync::Arc::new(reopened);
    sf.execute("data.reapply", json!({})).unwrap();
    {
        let sh = sf.doc().unwrap().wb.active().unwrap();
        assert!(!sh.is_row_hidden(1), "formula row A2 stays visible after XLSX reopen + reapply");
        assert!(sh.is_row_hidden(2), "literal Card row A3 stays hidden after XLSX reopen + reapply");
        assert!(sh.is_row_hidden(3), "whitespace Card row A4 stays hidden after XLSX reopen + reapply");
    }

    // Turn Show Formulas OFF: dropdown keeps formula `=" Card "` checked and literal `"Card"` + `" Card "` merged & unchecked.
    sf.execute("formulas.showFormulas", json!({"on": false})).unwrap();
    let off_choices = filter_choices(&sf.doc().unwrap().wb, 0, 0);
    assert_eq!(off_choices.len(), 3, "expected formula Card, merged literal Card, and Other: {off_choices:?}");
    assert_eq!(off_choices.iter().filter(|c| c.checked).count(), 1, "{off_choices:?}");

    let unchanged_off: Vec<_> = off_choices.iter().map(|c| (c, c.checked)).collect();
    sf.execute("data.filterBy", filter_choices_params("A", &unchanged_off)).unwrap();
    let sh = sf.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "formula row A2 stays visible after Apply with Show Formulas off");
    assert!(sh.is_row_hidden(2), "literal Card row A3 stays hidden after Apply with Show Formulas off");
    assert!(sh.is_row_hidden(3), "whitespace Card row A4 stays hidden after Apply with Show Formulas off");
    assert!(sh.is_row_hidden(4), "Other row A5 stays hidden");
}

/// Migration preserves manually hidden rows and respects sibling side-by-side table filters (#2943).
#[test]
fn migration_preserves_manual_hidden_rows_and_sibling_table_filters_2943() {
    use gridcraft_core::RangeRef;
    use gridcraft_model::{AutoFilter, FilterCriterion, Table, TableColumn, TotalsFn};

    // 1. Manually hidden `"Card"` row stays hidden when `["   ", "Card"]` migrates on open,
    //    while genuinely empty row excluded by old filter becomes visible.
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [["Name"], [null], ["Card"], ["Card"], ["Other"]]
        }),
    )
    .unwrap();
    {
        let wb = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb);
        let sh = wb.sheet_mut(0).unwrap();
        sh.autofilter = Some(AutoFilter {
            range: RangeRef::parse("A1:A5").unwrap(),
            criteria: vec![(0, FilterCriterion::Values { values: vec!["   ".into(), "Card".into()], blanks: false, identities: vec![] })],
        });
        // Row 1 (A2 = null) was hidden by old blank branch; Row 2 (A3 = "Card") was hidden manually;
        // Row 3 (A4 = "Card") is visible; Row 4 (A5 = "Other") was hidden by filter.
        sh.rows.entry(1).or_default().hidden = true;
        sh.rows.entry(2).or_default().hidden = true;
        sh.rows.entry(4).or_default().hidden = true;
    }
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (reopened, _) = gridcraft_engine::io::open_bytes("manual_hidden.xlsx", &bytes).unwrap();
    let sh = reopened.sheet(0).unwrap();
    assert!(!sh.is_row_hidden(1), "empty row A2 excluded by old filter becomes visible");
    assert!(sh.is_row_hidden(2), "manually hidden matching Card row A3 must remain hidden");
    assert!(!sh.is_row_hidden(3), "visible Card row A4 remains visible");
    assert!(sh.is_row_hidden(4), "non-matching Other row A5 remains hidden");

    // 2. Two side-by-side tables sharing worksheet rows: migrating Table1 must not unhide a row excluded by Table2.
    let mut st = Session::new();
    st.new_workbook();
    st.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [
                ["Item", "Region"],
                [null, "West"],
                [null, "East"],
                ["Card", "East"]
            ]
        }),
    )
    .unwrap();
    {
        let wb = std::sync::Arc::make_mut(&mut st.active_mut().unwrap().wb);
        let sh = wb.sheet_mut(0).unwrap();
        let col =
            |name: &str| TableColumn { name: name.into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None };
        sh.tables.push(Table {
            id: 1,
            name: "LeftTable".into(),
            range: RangeRef::parse("A1:A4").unwrap(),
            header_row: true,
            totals_row: false,
            columns: vec![col("Item")],
            style: "TableStyleMedium2".into(),
            banded_rows: true,
            banded_cols: false,
            first_col: false,
            last_col: false,
            filter_button: true,
            filter: vec![(0, FilterCriterion::Values { values: vec!["   ".into()], blanks: false, identities: vec![] })],
            table_type: None,
            connection_id: None,
        });
        sh.tables.push(Table {
            id: 2,
            name: "RightTable".into(),
            range: RangeRef::parse("B1:B4").unwrap(),
            header_row: true,
            totals_row: false,
            columns: vec![col("Region")],
            style: "TableStyleMedium2".into(),
            banded_rows: true,
            banded_cols: false,
            first_col: false,
            last_col: false,
            filter_button: true,
            filter: vec![(0, FilterCriterion::Values { values: vec!["East".into()], blanks: false, identities: vec![] })],
            table_type: None,
            connection_id: None,
        });
        sh.rows.entry(1).or_default().hidden = true;
        sh.rows.entry(2).or_default().hidden = true;
        sh.rows.entry(3).or_default().hidden = true;
    }
    let tbytes = gridcraft_xlsx::write_xlsx(&st.doc().unwrap().wb).unwrap();
    let (treopened, _) = gridcraft_engine::io::open_bytes("two_tables.xlsx", &tbytes).unwrap();
    let tsh = treopened.sheet(0).unwrap();
    assert!(tsh.is_row_hidden(1), "row 1 (West) is still excluded by RightTable and must stay hidden");
    assert!(!tsh.is_row_hidden(2), "row 2 (empty + East) passes both tables after migration and becomes visible");
    assert!(tsh.is_row_hidden(3), "row 3 (Card + East) is excluded by LeftTable blanks filter and stays hidden");
}

/// Table slicers merge surrounding-whitespace variants into a single tile (#2943).
#[test]
fn slicer_groups_whitespace_variants_into_single_tile_2943() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [["Name"], [" Card "], ["Card"], ["Other"], ["   "]]
        }),
    )
    .unwrap();
    s.execute("insert.table", json!({"range": "A1:A5", "name": "Items"})).unwrap();
    let res = s.execute("insert.slicer", json!({"table": "Items", "column": "Name"})).unwrap();
    let id = res["slicer"].as_u64().unwrap() as u32;

    let wb = &s.doc().unwrap().wb;
    let sl = wb.active().unwrap().slicers.iter().find(|x| x.id == id).unwrap();
    let tiles = gridcraft_engine::cmd::slicer::slicer_items(wb, 0, sl);
    assert_eq!(tiles, vec![("Card".to_string(), true), ("Other".to_string(), true)]);

    s.execute("slicer.toggle", json!({"slicer": id, "value": "Card"})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "' Card ' row stays visible");
    assert!(!sh.is_row_hidden(2), "'Card' row stays visible");
    assert!(sh.is_row_hidden(3), "'Other' row hidden");
    assert!(sh.is_row_hidden(4), "whitespace-only row hidden");
}

/// Table slicers merge non-ASCII Unicode case variants into a single tile and keep both visible (#2943).
#[test]
fn slicer_groups_unicode_case_variants_into_single_tile_2943() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "parse": false,
            "values": [["School"], ["ÉCOLE"], ["école"], ["autre"]]
        }),
    )
    .unwrap();
    s.execute("insert.table", json!({"range": "A1:A4", "name": "Schools"})).unwrap();
    let res = s.execute("insert.slicer", json!({"table": "Schools", "column": "School"})).unwrap();
    let id = res["slicer"].as_u64().unwrap() as u32;

    let wb = &s.doc().unwrap().wb;
    let sl = wb.active().unwrap().slicers.iter().find(|x| x.id == id).unwrap();
    let tiles = gridcraft_engine::cmd::slicer::slicer_items(wb, 0, sl);
    // Unicode case variants 'ÉCOLE' and 'école' must share one tile.
    assert_eq!(tiles.len(), 2, "must produce 2 tiles: ÉCOLE and autre, but was {tiles:?}");
    assert!(tiles.iter().any(|(t, on)| (t == "ÉCOLE" || t == "école") && *on));
    assert!(tiles.iter().any(|(t, on)| t == "autre" && *on));

    // Selecting the shared tile keeps both 'ÉCOLE' and 'école' visible.
    let tile_name = tiles.iter().find(|(t, _)| t == "ÉCOLE" || t == "école").unwrap().0.clone();
    s.execute("slicer.toggle", json!({"slicer": id, "value": tile_name})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "ÉCOLE row stays visible");
    assert!(!sh.is_row_hidden(2), "école row stays visible");
    assert!(sh.is_row_hidden(3), "autre row hidden");
}
