//! Integration tests for the visual-calculation evaluator.
//!
//! Visual functions need a visual row; the integration surface drives them through
//! `ADDCOLUMNS`, whose row iteration provides the innermost row frame that the evaluator treats
//! as the visual row. Expected values are computed by hand from the three-row `Sales` fixture.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "Month".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
            ],
            rows: vec![
                vec![Scalar::String("Jan".into()), Scalar::Integer(10)],
                vec![Scalar::String("Feb".into()), Scalar::Integer(20)],
                vec![Scalar::String("Mar".into()), Scalar::Integer(30)],
            ],
        }],
        relationships: vec![],
        measures: vec![Measure {
            name: "Total Amount".into(),
            formula: "SUM(Sales[Amount])".into(),
            table: Some("Sales".into()),
            format: None,
            expr: None,
        }],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn eval(src: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn eval_table(src: &str) -> Result<TableValue, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.to_string())
}

/// Evaluates `ADDCOLUMNS(Sales, "<column>", <expression>)` and returns the raw values of the new
/// column so navigation-only calls can be asserted as keywords.
fn visual_scalars(expression: &str) -> Vec<Scalar> {
    let source = format!("ADDCOLUMNS(Sales, \"Result\", {expression})");
    let table = eval_table(&source).unwrap_or_else(|error| panic!("{source} failed: {error}"));
    let index = table.column_index("Result").expect("Result column");
    table.rows.iter().filter_map(|row| row.get(index).cloned()).collect()
}

/// Evaluates `ADDCOLUMNS(Sales, "<column>", <expression>)` and returns the values of the new
/// column, using `None` for BLANK.
fn visual_column(expression: &str) -> Vec<Option<f64>> {
    let source = format!("ADDCOLUMNS(Sales, \"Result\", {expression})");
    let table = eval_table(&source).unwrap_or_else(|error| panic!("{source} failed: {error}"));
    let index = table.column_index("Result").expect("Result column");
    table.rows.iter().map(|row| row.get(index).and_then(|value| if value.is_blank() { None } else { value.to_number() })).collect()
}

#[test]
fn running_sum_accumulates_from_the_first_row() {
    assert_eq!(visual_column("RUNNINGSUM(Sales[Amount])"), vec![Some(10.0), Some(30.0), Some(60.0)]);
}

#[test]
fn running_sum_accepts_a_measure() {
    assert_eq!(visual_column("RUNNINGSUM([Total Amount])"), vec![Some(10.0), Some(30.0), Some(60.0)]);
}

#[test]
fn first_and_last_return_the_axis_boundaries() {
    assert_eq!(visual_column("FIRST(Sales[Amount])"), vec![Some(10.0), Some(10.0), Some(10.0)]);
    assert_eq!(visual_column("LAST(Sales[Amount])"), vec![Some(30.0), Some(30.0), Some(30.0)]);
    // The optional axis argument is a layout hint and does not change the result.
    assert_eq!(visual_column("FIRST(Sales[Amount], ROWS)"), vec![Some(10.0), Some(10.0), Some(10.0)]);
    assert_eq!(visual_column("LAST(Sales[Amount], ROWS)"), vec![Some(30.0), Some(30.0), Some(30.0)]);
}

#[test]
fn previous_and_next_navigate_one_row() {
    // Row 0 has no previous row and row 2 has no next row, so they are BLANK.
    assert_eq!(visual_column("PREVIOUS(Sales[Amount])"), vec![None, Some(10.0), Some(20.0)]);
    assert_eq!(visual_column("NEXT(Sales[Amount])"), vec![Some(20.0), Some(30.0), None]);
}

#[test]
fn previous_and_next_honor_steps_and_boundaries() {
    assert_eq!(visual_column("PREVIOUS(Sales[Amount], 2)"), vec![None, None, Some(10.0)]);
    assert_eq!(visual_column("NEXT(Sales[Amount], 2)"), vec![Some(30.0), None, None]);
    // Steps past either end stay BLANK rather than wrapping around the axis.
    assert_eq!(visual_column("PREVIOUS(Sales[Amount], 9)"), vec![None, None, None]);
    assert_eq!(visual_column("NEXT(Sales[Amount], 9)"), vec![None, None, None]);
}

#[test]
fn navigation_functions_pass_text_values_through() {
    let jan = Scalar::String("Jan".into());
    let feb = Scalar::String("Feb".into());
    let mar = Scalar::String("Mar".into());
    assert_eq!(visual_scalars("FIRST(Sales[Month])"), vec![jan.clone(), jan.clone(), jan.clone()]);
    assert_eq!(visual_scalars("LAST(Sales[Month])"), vec![mar.clone(), mar.clone(), mar.clone()]);
    assert_eq!(visual_scalars("PREVIOUS(Sales[Month])"), vec![Scalar::Blank, jan.clone(), feb.clone()]);
    assert_eq!(visual_scalars("NEXT(Sales[Month])"), vec![feb, mar, Scalar::Blank]);
}

#[test]
fn previous_and_next_reject_non_positive_steps() {
    for expression in ["PREVIOUS(Sales[Amount], 0)", "NEXT(Sales[Amount], -1)"] {
        let error = eval_table(&format!("ADDCOLUMNS(Sales, \"Result\", {expression})")).expect_err("steps must be positive");
        assert!(error.contains("at least 1"), "{expression}: {error}");
    }
}

#[test]
fn moving_average_uses_a_window_ending_at_the_current_row() {
    // Windows: [10], [10, 20], [20, 30].
    assert_eq!(visual_column("MOVINGAVERAGE(Sales[Amount], 2)"), vec![Some(10.0), Some(15.0), Some(25.0)]);
    assert_eq!(visual_column("MOVINGAVERAGE(Sales[Amount], 1)"), vec![Some(10.0), Some(20.0), Some(30.0)]);
}

#[test]
fn moving_average_can_exclude_the_current_row() {
    // Previous two rows only: none, [10], [10, 20].
    assert_eq!(visual_column("MOVINGAVERAGE(Sales[Amount], 2, FALSE)"), vec![None, Some(10.0), Some(15.0)]);
    // Previous three rows only: none, [10], [10, 20].
    assert_eq!(visual_column("MOVINGAVERAGE(Sales[Amount], 3, FALSE)"), vec![None, Some(10.0), Some(15.0)]);
}

#[test]
fn moving_average_rejects_a_bad_window_size() {
    let error = eval_table("ADDCOLUMNS(Sales, \"Result\", MOVINGAVERAGE(Sales[Amount], 0))").expect_err("window must be positive");
    assert!(error.contains("WindowSize must be at least 1"), "{error}");
    let error = eval_table("ADDCOLUMNS(Sales, \"Result\", MOVINGAVERAGE(Sales[Amount], 2, \"nope\"))").expect_err("boolean expected");
    assert!(error.contains("Boolean"), "{error}");
}

#[test]
fn range_selects_rows_after_a_non_negative_step() {
    // RANGE(1) with the default includeCurrent selects the current and following row.
    assert_eq!(visual_column("COUNTROWS(RANGE(1))"), vec![Some(2.0), Some(2.0), Some(1.0)]);
    assert_eq!(visual_column("COUNTROWS(RANGE(0))"), vec![Some(1.0), Some(1.0), Some(1.0)]);
}

#[test]
fn range_selects_rows_before_a_negative_step() {
    // RANGE(-1) selects the previous row and the current row.
    assert_eq!(visual_column("COUNTROWS(RANGE(-1))"), vec![Some(1.0), Some(2.0), Some(2.0)]);
    assert_eq!(visual_column("COUNTROWS(RANGE(-2))"), vec![Some(1.0), Some(2.0), Some(3.0)]);
}

#[test]
fn range_can_exclude_the_current_row() {
    // In the core evaluator COUNTROWS of an empty table is BLANK, so the out-of-range row here
    // is `None` rather than zero.
    assert_eq!(visual_column("COUNTROWS(RANGE(1, FALSE))"), vec![Some(1.0), Some(1.0), None]);
    assert_eq!(visual_column("COUNTROWS(RANGE(-1, FALSE))"), vec![None, Some(1.0), Some(1.0)]);
}

#[test]
fn range_rows_carry_the_axis_values() {
    // Previous row plus current: 10, 10+20, 20+30.
    assert_eq!(visual_column("SUMX(RANGE(-1), Sales[Amount])"), vec![Some(10.0), Some(30.0), Some(50.0)]);
    assert_eq!(visual_column("SUMX(RANGE(-1, FALSE), Sales[Amount])"), vec![None, Some(10.0), Some(20.0)]);
    assert_eq!(visual_column("SUMX(RANGE(0), Sales[Amount])"), vec![Some(10.0), Some(20.0), Some(30.0)]);
}

#[test]
fn range_requires_a_visual_frame_and_a_table_context() {
    let error = eval_table("RANGE(1)").expect_err("no visual frame");
    assert!(error.contains("visual frame"), "{error}");
    // In scalar position the table-returning function reports what it is instead of running.
    let error = eval("RANGE(1)").expect_err("RANGE is a table");
    assert!(error.contains("returns a table"), "{error}");
}

#[test]
fn collapse_and_expand_are_identity_layout_hints() {
    for expression in [
        "COLLAPSE(Sales[Amount])",
        "COLLAPSE(Sales[Amount], ROWS)",
        "COLLAPSE(Sales[Amount], ROWS, 2)",
        "COLLAPSE(Sales[Amount], Sales[Month])",
        "COLLAPSEALL(Sales[Amount])",
        "COLLAPSEALL(Sales[Amount], ROWS)",
        "EXPAND(Sales[Amount])",
        "EXPAND(Sales[Amount], ROWS)",
        "EXPAND(Sales[Amount], ROWS, 2)",
        "EXPANDALL(Sales[Amount])",
        "EXPANDALL(Sales[Amount], COLUMNS)",
    ] {
        assert_eq!(visual_column(expression), vec![Some(10.0), Some(20.0), Some(30.0)], "{expression}");
    }
}

#[test]
fn collapse_and_expand_navigation_only_calls_keep_the_axis_keyword() {
    assert_eq!(visual_scalars("COLLAPSE(ROWS, 1)"), vec![Scalar::String("ROWS".into()); 3]);
    assert_eq!(visual_scalars("COLLAPSE(COLUMNS)"), vec![Scalar::String("COLUMNS".into()); 3]);
    assert_eq!(visual_scalars("EXPAND(ROWS, 2)"), vec![Scalar::String("ROWS".into()); 3]);
    assert_eq!(visual_scalars("COLLAPSEALL(ROWS)"), vec![Scalar::String("ROWS".into()); 3]);
    assert_eq!(visual_scalars("EXPANDALL(COLUMNS)"), vec![Scalar::String("COLUMNS".into()); 3]);
}

#[test]
fn documented_axis_and_clause_keywords_bind() {
    let running = vec![Some(10.0), Some(30.0), Some(60.0)];
    assert_eq!(visual_column("RUNNINGSUM(Sales[Amount], ROWS)"), running);
    assert_eq!(visual_column("RUNNINGSUM(Sales[Amount], ROWS, KEEP, DEFAULT, NONE)"), running);

    let moving = vec![Some(10.0), Some(15.0), Some(25.0)];
    assert_eq!(visual_column("MOVINGAVERAGE(Sales[Amount], 2, TRUE, ROWS)"), moving);
    assert_eq!(visual_column("MOVINGAVERAGE(Sales[Amount], 2, TRUE, ROWS, KEEP, DEFAULT, NONE)"), moving);
    assert_eq!(visual_column("MOVINGAVERAGE(Sales[Amount], 2, TRUE, COLUMNS, KEEP, LAST, LOWESTPARENT)"), moving);

    assert_eq!(visual_column("PREVIOUS(Sales[Amount], 1, ROWS, KEEP, DEFAULT, NONE)"), vec![None, Some(10.0), Some(20.0)]);
    assert_eq!(visual_column("NEXT(Sales[Amount], 1, ROWS, KEEP, DEFAULT, NONE)"), vec![Some(20.0), Some(30.0), None]);
    assert_eq!(visual_column("COUNTROWS(RANGE(-1, TRUE, ROWS, KEEP, DEFAULT, LOWESTPARENT))"), vec![Some(1.0), Some(2.0), Some(2.0)]);
    assert_eq!(visual_column("COUNTROWS(RANGE(12, FALSE, ROWS, KEEP))"), vec![Some(2.0), Some(1.0), None]);
}

#[test]
fn collapse_and_expand_keep_a_measure_value() {
    assert_eq!(visual_column("COLLAPSE([Total Amount])"), vec![Some(10.0), Some(20.0), Some(30.0)]);
    assert_eq!(visual_column("EXPANDALL([Total Amount])"), vec![Some(10.0), Some(20.0), Some(30.0)]);
}

#[test]
fn visual_functions_require_a_visual_frame() {
    for source in [
        "RUNNINGSUM(Sales[Amount])",
        "MOVINGAVERAGE(Sales[Amount], 2)",
        "FIRST(Sales[Amount])",
        "LAST(Sales[Amount])",
        "PREVIOUS(Sales[Amount])",
        "NEXT(Sales[Amount])",
        "COLLAPSE(Sales[Amount])",
        "COLLAPSEALL(Sales[Amount])",
        "EXPAND(Sales[Amount])",
        "EXPANDALL(Sales[Amount])",
    ] {
        let error = eval(source).expect_err("no visual frame");
        assert!(error.contains("visual frame"), "{source}: {error}");
    }
    let error = eval_table("RANGE(1)").expect_err("no visual frame");
    assert!(error.contains("visual frame"), "RANGE: {error}");
}

#[test]
fn visual_functions_require_a_model_row() {
    // A table constructor has no lineage, so its rows cannot provide a visual axis.
    let error = eval_table("ADDCOLUMNS({1, 2}, \"Result\", RUNNINGSUM(Sales[Amount]))").expect_err("no model row");
    assert!(error.contains("visual frame"), "{error}");
    let error = eval_table("ADDCOLUMNS({1, 2}, \"Result\", COLLAPSE(Sales[Amount]))").expect_err("no model row");
    assert!(error.contains("visual frame"), "{error}");
}

#[test]
fn aggregate_windows_skip_blank_values() {
    // Replace the second row's amount with BLANK and verify the running window ignores it.
    let mut model = model();
    model.tables[0].rows[1][1] = Scalar::Blank;
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("ADDCOLUMNS(Sales, \"Result\", MOVINGAVERAGE(Sales[Amount], 2))").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    let table = evaluator.evaluate_table(&bound).expect("table");
    let index = table.column_index("Result").expect("Result column");
    let values: Vec<Option<f64>> =
        table.rows.iter().map(|row| row.get(index).and_then(|value| if value.is_blank() { None } else { value.to_number() })).collect();
    // Row 0: [10] -> 10. Row 1: [10, BLANK] -> 10. Row 2: [BLANK, 30] -> 30.
    assert_eq!(values, vec![Some(10.0), Some(10.0), Some(30.0)]);
}
