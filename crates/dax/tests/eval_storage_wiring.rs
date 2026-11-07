//! Wiring test: evaluation really reads the columnar `ModelStorage`, not just the model rows.
//!
//! `Evaluator::storage_stats` counts calls *served by* storage. Each test takes a snapshot, runs
//! one formula, and asserts the counter that formula must move went up; `row_scan_fallbacks`
//! covers the opposite direction. A formula that quietly fell back to the row scan leaves its
//! counter at zero, so these assertions fail if the wiring is ever removed.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::pivot_table::{PivotLayout, PivotTable};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, FilterContext, StorageStats, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "UnitPrice".into(), data_type: Some("Currency".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(1), Scalar::String("Hardware".into()), Scalar::Currency(200_000)],
                    vec![Scalar::Integer(2), Scalar::String("Tools".into()), Scalar::Currency(50_000)],
                ],
            },
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Quantity".into(), data_type: Some("Int64".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(1), Scalar::String("East".into()), Scalar::Integer(10), Scalar::Integer(2)],
                    vec![Scalar::Integer(1), Scalar::String("West".into()), Scalar::Integer(20), Scalar::Integer(3)],
                    vec![Scalar::Integer(2), Scalar::String("East".into()), Scalar::Integer(5), Scalar::Integer(1)],
                ],
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "ProductKey".into(),
            to_table: "Product".into(),
            to_column: "ProductKey".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: vec![
            Measure { name: "Total Amount".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure { name: "Rows".into(), formula: "COUNTROWS(Sales)".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure {
                name: "Extended".into(),
                formula: "SUMX(Sales, Sales[Quantity] * RELATED(Product[UnitPrice]))".into(),
                table: Some("Sales".into()),
                format: None,
                expr: None,
            },
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn evaluate(evaluator: &Evaluator<'_>, model: &DataModel, formula: &str) -> (Scalar, StorageStats) {
    let expr = parse_expression(formula).expect("parse");
    let bound = Binder::new(model).bind_expression(&expr).expect("bind");
    let value = evaluator.evaluate(&bound).expect("evaluate");
    (value, evaluator.storage_stats())
}

fn evaluate_table(evaluator: &Evaluator<'_>, model: &DataModel, formula: &str) -> (gridcraft_dax::TableValue, StorageStats) {
    let expr = parse_expression(formula).expect("parse");
    let bound = Binder::new(model).bind_expression(&expr).expect("bind");
    let table = evaluator.evaluate_table(&bound).expect("table");
    (table, evaluator.storage_stats())
}

#[test]
fn sumx_runs_over_a_storage_column() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    assert!(evaluator.storage().expect("storage").is_faithful());

    let (value, stats) = evaluate(&evaluator, &model, "SUMX(Sales, Sales[Amount])");
    assert_eq!(value, Scalar::Decimal(35.0));
    assert_eq!(stats.iterators, 1, "SUMX over a plain column is one storage iterator: {stats:?}");
    assert_eq!(stats.aggregates, 1);
    assert_eq!(stats.rows_from_storage, 3);
    assert_eq!(stats.row_scan_fallbacks, 0);
}

#[test]
fn countrows_uses_the_filtered_row_id_set() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let (value, stats) = evaluate(&evaluator, &model, "COUNTROWS(Sales)");
    assert_eq!(value, Scalar::Integer(3));
    assert_eq!(stats.count_rows, 1, "{stats:?}");
    assert_eq!(stats.rows_from_storage, 3);

    // A filter becomes a storage column scan, and the count follows from the row ids.
    let mut filtered = FilterContext::default();
    filtered.set(1, 1, [gridcraft_dax::ScalarKey::String("East".into())]);
    let expr = parse_expression("COUNTROWS(Sales)").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate_with(&bound, filtered).expect("evaluate"), Scalar::Integer(2));
    let stats = evaluator.storage_stats();
    assert_eq!(stats.count_rows, 2);
    assert_eq!(stats.filter_scans, 1, "{stats:?}");
}

#[test]
fn related_resolves_through_the_relationship_index() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    // 20*2 + 20*3 + 5*1 = 40 + 60 + 5 = 105; the derived expression keeps the iterator row-based,
    // but every RELATED call is an index probe.
    let (value, stats) = evaluate(&evaluator, &model, "SUMX(Sales, Sales[Quantity] * RELATED(Product[UnitPrice]))");
    assert_eq!(value, Scalar::Decimal(105.0));
    assert_eq!(stats.related, 3, "one index lookup per row: {stats:?}");
    assert_eq!(stats.relationship_lookups, 3);
    assert_eq!(stats.iterators, 0, "a derived expression cannot use the plain-column iterator");
}

#[test]
fn relatedtable_resolves_through_the_relationship_index() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    // Product 1 has two sales rows, product 2 has one: 2 + 1 = 3.
    let (value, stats) = evaluate(&evaluator, &model, "SUMX(Product, COUNTROWS(RELATEDTABLE(Sales)))");
    assert_eq!(value, Scalar::Decimal(3.0));
    assert_eq!(stats.related_table, 2, "one join per product row: {stats:?}");

    // Filtered related rows: West only, so product 1 has one row and product 2 none.
    let mut filtered = FilterContext::default();
    filtered.set(1, 1, [gridcraft_dax::ScalarKey::String("West".into())]);
    let expr = parse_expression("SUMX(Product, COUNTROWS(RELATEDTABLE(Sales)))").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate_with(&bound, filtered).expect("evaluate"), Scalar::Decimal(1.0));
}

#[test]
fn values_and_distinct_dedupe_through_storage() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let (table, stats) = evaluate_table(&evaluator, &model, "VALUES(Sales[Region])");
    assert_eq!(table.rows, vec![vec![Scalar::String("East".into())], vec![Scalar::String("West".into())]]);
    assert_eq!(stats.distinct_values, 1, "{stats:?}");
    assert_eq!(stats.rows_from_storage, 3);

    let (distinct, stats) = evaluate_table(&evaluator, &model, "DISTINCT(Sales[Region])");
    assert_eq!(distinct.rows, table.rows);
    assert_eq!(stats.distinct_values, 2);
}

#[test]
fn filter_propagation_uses_the_relationship_index() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    // Hardware products are only product 1, so sales rows 1 and 2: 10 + 20 = 30.
    let (value, stats) = evaluate(&evaluator, &model, "CALCULATE(SUM(Sales[Amount]), Product[Category] = \"Hardware\")");
    assert_eq!(value, Scalar::Decimal(30.0));
    assert!(stats.relationship_lookups > 0, "propagation must probe the index: {stats:?}");
    assert_eq!(stats.aggregates, 1);
}

#[test]
fn pivot_cells_and_grouping_are_served_by_storage() {
    let model = model();
    let pivot = PivotTable::new("Sales by region", "Total Amount")
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular);
    let (table, stats) = pivot.render_with_stats(&model).expect("render");

    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0], vec![Scalar::String("East".into()), Scalar::Decimal(15.0)]);
    assert_eq!(table.rows[1], vec![Scalar::String("West".into()), Scalar::Decimal(20.0)]);
    assert_eq!(table.rows[2], vec![Scalar::String("Grand Total".into()), Scalar::Decimal(35.0)]);

    assert_eq!(stats.pivot_group_rows, 3, "grouping reads the source rows from columns: {stats:?}");
    assert!(stats.aggregates > 0, "every cell is a storage aggregate: {stats:?}");
    assert_eq!(stats.row_scan_fallbacks, 0);
}

#[test]
fn measures_share_one_substrate_across_evaluations() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    assert_eq!(evaluator.evaluate_measure("Total Amount", FilterContext::default()).expect("measure"), Scalar::Decimal(35.0));
    assert_eq!(evaluator.evaluate_measure("Rows", FilterContext::default()).expect("measure"), Scalar::Integer(3));
    assert_eq!(evaluator.evaluate_measure("Extended", FilterContext::default()).expect("measure"), Scalar::Decimal(105.0));
    let stats = evaluator.storage_stats();
    assert_eq!(stats.aggregates, 1);
    assert_eq!(stats.count_rows, 1);
    assert_eq!(stats.related, 3);
    // The substrate is built once in `Evaluator::new` and shared: its table ids are the model
    // positions, so repeated evaluation never rebuilds it.
    assert_eq!(evaluator.storage().expect("storage").total_rows(), 5);
}
