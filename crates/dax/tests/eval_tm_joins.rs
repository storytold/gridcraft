//! Evaluator tests for the table-manipulation join/generation family.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Integer(10), Scalar::Integer(1)],
                    vec![Scalar::String("West".into()), Scalar::Integer(20), Scalar::Integer(2)],
                    vec![Scalar::String("East".into()), Scalar::Integer(30), Scalar::Integer(1)],
                    vec![Scalar::String("North".into()), Scalar::Integer(40), Scalar::Integer(3)],
                ],
            },
            ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(1), Scalar::String("Audio".into())],
                    vec![Scalar::Integer(2), Scalar::String("Video".into())],
                    vec![Scalar::Integer(3), Scalar::String("Audio".into())],
                ],
            },
        ],
        relationships: Vec::new(),
        measures: vec![Measure {
            name: "Sales Amount".into(),
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
    evaluator.evaluate(&bound).map_err(|error| error.message)
}

fn eval_table(src: &str) -> Result<TableValue, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.message)
}

fn table(src: &str) -> TableValue {
    eval_table(src).expect("table evaluates")
}

fn names(value: &TableValue) -> Vec<String> {
    value.columns.iter().map(|column| column.name.clone()).collect()
}

fn error(src: &str) -> String {
    eval_table(src).expect_err("expression should fail")
}

const LEFT: &str = "UNION ( ROW ( \"K\", 1, \"A\", \"a\" ), ROW ( \"K\", 2, \"A\", \"b\" ), ROW ( \"K\", 3, \"A\", \"c\" ) )";
const RIGHT: &str = "UNION ( ROW ( \"K\", 2, \"B\", \"B\" ), ROW ( \"K\", 3, \"B\", \"C\" ), ROW ( \"K\", 4, \"B\", \"D\" ) )";

#[test]
fn generate_evaluates_the_second_table_per_row() {
    let result = table("GENERATE ( Sales, ROW ( \"Tag\", Sales[Region] ) )");
    assert_eq!(names(&result), vec!["Region", "Amount", "ProductKey", "Tag"]);
    assert_eq!(result.rows.len(), 4);
    assert_eq!(result.rows[0], vec![Scalar::String("East".into()), Scalar::Integer(10), Scalar::Integer(1), Scalar::String("East".into())]);
    assert_eq!(result.rows[3], vec![Scalar::String("North".into()), Scalar::Integer(40), Scalar::Integer(3), Scalar::String("North".into())]);
}

#[test]
fn generate_drops_rows_with_an_empty_second_table() {
    let result = table("GENERATE ( Sales, FILTER ( ROW ( \"X\", 1 ), Sales[Amount] > 100 ) )");
    assert_eq!(names(&result), vec!["Region", "Amount", "ProductKey", "X"]);
    assert!(result.rows.is_empty());
}

#[test]
fn generateall_keeps_rows_with_blanks() {
    let result = table("GENERATEALL ( Sales, FILTER ( ROW ( \"X\", 1 ), Sales[Amount] > 100 ) )");
    assert_eq!(names(&result), vec!["Region", "Amount", "ProductKey", "X"]);
    assert_eq!(result.rows.len(), 4);
    assert_eq!(result.rows[0].len(), 4);
    assert_eq!(result.rows[0][3], Scalar::Blank);
}

#[test]
fn generate_rejects_duplicate_column_names() {
    let message = error("GENERATE ( Sales, ROW ( \"Region\", \"x\" ) )");
    assert!(message.contains("both tables"), "{message}");
}

#[test]
fn natural_inner_join_matches_common_columns() {
    let result = table(&format!("NATURALINNERJOIN ( {LEFT}, {RIGHT} )"));
    assert_eq!(names(&result), vec!["K", "A", "B"]);
    assert_eq!(result.rows.len(), 2);
    assert_eq!(result.rows[0], vec![Scalar::Integer(2), Scalar::String("b".into()), Scalar::String("B".into())]);
    assert_eq!(result.rows[1], vec![Scalar::Integer(3), Scalar::String("c".into()), Scalar::String("C".into())]);
}

#[test]
fn natural_left_outer_join_keeps_left_rows() {
    let result = table(&format!("NATURALLEFTOUTERJOIN ( {LEFT}, {RIGHT} )"));
    assert_eq!(names(&result), vec!["K", "A", "B"]);
    assert_eq!(result.rows.len(), 3);
    assert_eq!(result.rows[0], vec![Scalar::Integer(1), Scalar::String("a".into()), Scalar::Blank]);
    assert_eq!(result.rows[1], vec![Scalar::Integer(2), Scalar::String("b".into()), Scalar::String("B".into())]);
}

#[test]
fn natural_join_requires_a_common_column() {
    let message = error("NATURALINNERJOIN ( ROW ( \"A\", 1 ), ROW ( \"B\", 2 ) )");
    assert!(message.contains("matching name"), "{message}");
}

#[test]
fn natural_join_uses_strict_comparison() {
    // Integer 2 and Decimal 2.0 are different types in DAX, so they do not match.
    let result = table("NATURALINNERJOIN ( ROW ( \"K\", 2 ), ROW ( \"K\", 2.0 ) )");
    assert!(result.rows.is_empty());
}

#[test]
fn natural_join_usage_returns_its_table() {
    assert_eq!(eval("COUNTROWS ( NATURALJOINUSAGE ( Product ) )"), Ok(Scalar::Integer(3)));
    let result = table("NATURALJOINUSAGE ( Product )");
    assert_eq!(names(&result), vec!["ProductKey", "Category"]);
    assert_eq!(result.rows.len(), 3);
}

#[test]
fn generate_series_integer_and_boundaries() {
    let result = table("GENERATESERIES ( 1, 10, 2 )");
    assert_eq!(names(&result), vec!["Value"]);
    let values: Vec<Scalar> = result.rows.iter().map(|row| row[0].clone()).collect();
    assert_eq!(values, vec![Scalar::Integer(1), Scalar::Integer(3), Scalar::Integer(5), Scalar::Integer(7), Scalar::Integer(9)]);

    let single = table("GENERATESERIES ( 5, 5 )");
    assert_eq!(single.rows, vec![vec![Scalar::Integer(5)]]);

    let reversed = table("GENERATESERIES ( 10, 1 )");
    assert!(reversed.rows.is_empty());
    assert_eq!(names(&reversed), vec!["Value"]);
}

#[test]
fn generate_series_decimal_and_date_results() {
    let decimal = table("GENERATESERIES ( 0, 1, 0.1 )");
    assert_eq!(decimal.rows.len(), 11);
    assert!(matches!(decimal.rows[0][0], Scalar::Decimal(_)));
    let last = decimal.rows[10][0].to_number().unwrap_or(f64::NAN);
    assert!((last - 1.0).abs() < 1e-9, "{last}");

    let dates = table("GENERATESERIES ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 3 ) )");
    assert_eq!(dates.rows, vec![vec![Scalar::DateTime(45292.0)], vec![Scalar::DateTime(45293.0)], vec![Scalar::DateTime(45294.0)]]);
}

#[test]
fn generate_series_rejects_invalid_increments_and_huge_ranges() {
    assert!(error("GENERATESERIES ( 1, 5, 0 )").contains("positive"));
    assert!(error("GENERATESERIES ( 1, 5, -1 )").contains("positive"));
    assert!(error("GENERATESERIES ( 0, 100000000, 1 )").contains("too large"));
}

#[test]
fn filters_returns_direct_filter_values() {
    assert_eq!(eval("COUNTROWS ( FILTERS ( Sales[Region] ) )"), Ok(Scalar::Integer(3)));
    let direct = table("FILTERS ( Sales[Region] )");
    assert_eq!(names(&direct), vec!["Region"]);
    assert_eq!(direct.lineage, Some(0));
    assert_eq!(direct.rows[0], vec![Scalar::String("East".into())]);

    assert_eq!(eval("CALCULATE ( COUNTROWS ( FILTERS ( Sales[Region] ) ), Sales[Region] = \"East\" )"), Ok(Scalar::Integer(1)));
    // A value that does not exist in the model is not a filter.
    assert_eq!(eval("CALCULATE ( COUNTROWS ( FILTERS ( Sales[Region] ) ), Sales[Region] = \"South\" )"), Ok(Scalar::Blank));
}

#[test]
fn filters_ignores_cross_filters() {
    // Filtering Amount does not directly filter Region, so FILTERS still sees every region.
    assert_eq!(eval("CALCULATE ( COUNTROWS ( FILTERS ( Sales[Region] ) ), Sales[Amount] <> 15 )"), Ok(Scalar::Integer(3)));
}

#[test]
fn detail_rows_returns_the_measure_home_table() {
    assert_eq!(eval("COUNTROWS ( DETAILROWS ( [Sales Amount] ) )"), Ok(Scalar::Integer(4)));
    let result = table("DETAILROWS ( [Sales Amount] )");
    assert_eq!(names(&result), vec!["Region", "Amount", "ProductKey"]);
    assert_eq!(result.lineage, Some(0));
    assert_eq!(eval("CALCULATE ( COUNTROWS ( DETAILROWS ( [Sales Amount] ) ), Sales[Region] = \"East\" )"), Ok(Scalar::Integer(2)));
}

const POINTS: &str = "UNION ( \
    ROW ( \"X\", 1, \"Y\", 5 ), ROW ( \"X\", 2, \"Y\", 9 ), ROW ( \"X\", 3, \"Y\", 1 ), ROW ( \"X\", 4, \"Y\", 9 ), \
    ROW ( \"X\", 5, \"Y\", 2 ), ROW ( \"X\", 6, \"Y\", 8 ), ROW ( \"X\", 7, \"Y\", 9 ), ROW ( \"X\", 8, \"Y\", 0 ) )";

#[test]
fn sample_axis_preserves_local_min_max_per_bin() {
    let result = table(&format!("SAMPLEAXISWITHLOCALMINMAX ( 2, {POINTS}, [X], [Y], 1 )"));
    assert_eq!(names(&result), vec!["X", "Y"]);
    let axis: Vec<i64> = result
        .rows
        .iter()
        .filter_map(|row| match row[0] {
            Scalar::Integer(value) => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(axis, vec![2, 3, 7, 8]);
    // The selected rows keep the local minima and maxima: 9/1 in the first half, 9/0 in the second.
    let measures: Vec<i64> = result
        .rows
        .iter()
        .filter_map(|row| match row[1] {
            Scalar::Integer(value) => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(measures, vec![9, 1, 9, 0]);
}

#[test]
fn sample_axis_with_a_large_size_keeps_every_row() {
    let result = table(&format!("SAMPLEAXISWITHLOCALMINMAX ( 100, {POINTS}, [X], [Y], 1 )"));
    assert_eq!(result.rows.len(), 8);
}

#[test]
fn sample_axis_validates_arguments() {
    assert!(error(&format!("SAMPLEAXISWITHLOCALMINMAX ( 0, {POINTS}, [X], [Y], 1 )")).contains("positive"));
    assert!(error(&format!("SAMPLEAXISWITHLOCALMINMAX ( 2, {POINTS}, [X], [Y], 0 )")).contains("MinResolution"));
    assert!(error(&format!("SAMPLEAXISWITHLOCALMINMAX ( 2, {POINTS}, [X], [Y], 5, [Y], \"ALPHABETICAL\", 1, 1 )")).contains("MaxResolution"));
}

#[test]
fn substitute_with_index_replaces_common_columns() {
    let right = "UNION ( ROW ( \"K\", 1 ), ROW ( \"K\", 2 ), ROW ( \"K\", 3 ) )";
    // The left rows are deliberately out of key order: the result keeps that order while the
    // index follows the sorted second table.
    let shuffled = "UNION ( ROW ( \"K\", 3, \"A\", \"c\" ), ROW ( \"K\", 1, \"A\", \"a\" ), ROW ( \"K\", 2, \"A\", \"b\" ) )";
    let result = table(&format!("SUBSTITUTEWITHINDEX ( {shuffled}, \"Idx\", {right}, [K], 1 )"));
    assert_eq!(names(&result), vec!["A", "Idx"]);
    assert_eq!(
        result.rows,
        vec![
            vec![Scalar::String("c".into()), Scalar::Integer(2)],
            vec![Scalar::String("a".into()), Scalar::Integer(0)],
            vec![Scalar::String("b".into()), Scalar::Integer(1)],
        ]
    );

    let descending = table(&format!("SUBSTITUTEWITHINDEX ( {shuffled}, \"Idx\", {right}, [K], 0 )"));
    assert_eq!(
        descending.rows,
        vec![
            vec![Scalar::String("c".into()), Scalar::Integer(0)],
            vec![Scalar::String("a".into()), Scalar::Integer(2)],
            vec![Scalar::String("b".into()), Scalar::Integer(1)],
        ]
    );
}

#[test]
fn substitute_with_index_requires_common_columns() {
    let message = error("SUBSTITUTEWITHINDEX ( ROW ( \"K\", 1 ), \"Idx\", ROW ( \"J\", 2 ), [K], 1 )");
    assert!(message.contains("matching name"), "{message}");
}
