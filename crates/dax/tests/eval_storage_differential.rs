//! Differential test of the storage-backed evaluator against documented expected values.
//!
//! The fixture is a two-table star schema (Sales many-side, Product one-side) with blanks in the
//! measure, quantity, region and key columns. Every expectation below is stated with its
//! arithmetic in the comment, and was checked independently in Excel 16 against the same rows
//! (workbook built with COM: `Sales`/`Product` sheets plus a formula summary and a real
//! `PivotTable`). The values Excel returned were: `SUM(Amount)` 280, `AVERAGE` 46.666666666666664,
//! `COUNT` 6, `COUNTA` 6, `COUNTBLANK` 1, distinct incl. blank 7, `MIN` 10, `MAX` 100,
//! `PRODUCT` 2100000000, `STDEV.S` 33.862466931200785, East 150, Hardware 190, extended price
//! 150.5, related-row count 7, West 100, North 20, blank region 10, the cross-tab key totals
//! 130/60/70/20 and the pivot's East 150 / North 20 / West 100 / (blank) 10 / Grand Total 280.
//!
//! These assertions are *documented values*, not a comparison of one evaluator path against
//! another. Each one additionally asserts the matching [`StorageStats`] counter moved, so a
//! documented value can never be produced by the row-scan fallback and still pass.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::pivot_table::{PivotFilter, PivotLayout, PivotTable};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, StorageStats, parse_expression};

// Product  ProductKey Name       Category UnitPrice
//          1          Widget     Hardware    20.00
//          2          Gadget     Hardware     5.00
//          3          Gizmo      Tools       12.50
//          4          Doohickey  Tools        0.75
//
// Sales    ProductKey Region Amount Quantity
//          1          East      100    2
//          2          East       50    5
//          1          West       30    3
//          3          West       70    1
//          1          East     (blank) (blank)
//          4          North      20    4
//          2          (blank)    10    2
fn model() -> DataModel {
    let product = |key: i64, name: &str, category: &str, price: i64| {
        vec![Scalar::Integer(key), Scalar::String(name.into()), Scalar::String(category.into()), Scalar::Currency(price)]
    };
    let sale = |key: i64, region: Scalar, amount: Scalar, quantity: Scalar| vec![Scalar::Integer(key), region, amount, quantity];
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "UnitPrice".into(), data_type: Some("Currency".into()) },
                ],
                rows: vec![
                    product(1, "Widget", "Hardware", 200_000),
                    product(2, "Gadget", "Hardware", 50_000),
                    product(3, "Gizmo", "Tools", 125_000),
                    product(4, "Doohickey", "Tools", 7_500),
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
                    sale(1, Scalar::String("East".into()), Scalar::Integer(100), Scalar::Integer(2)),
                    sale(2, Scalar::String("East".into()), Scalar::Integer(50), Scalar::Integer(5)),
                    sale(1, Scalar::String("West".into()), Scalar::Integer(30), Scalar::Integer(3)),
                    sale(3, Scalar::String("West".into()), Scalar::Integer(70), Scalar::Integer(1)),
                    sale(1, Scalar::String("East".into()), Scalar::Blank, Scalar::Blank),
                    sale(4, Scalar::String("North".into()), Scalar::Integer(20), Scalar::Integer(4)),
                    sale(2, Scalar::Blank, Scalar::Integer(10), Scalar::Integer(2)),
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
            measure("Total Amount", "SUM(Sales[Amount])", "Sales"),
            measure("Rows", "COUNTROWS(Sales)", "Sales"),
            measure("Product Count", "DISTINCTCOUNT(Sales[ProductKey])", "Sales"),
            measure("Extended Price", "SUMX(Sales, Sales[Quantity] * RELATED(Product[UnitPrice]))", "Sales"),
            measure("Sales Rows Per Product", "SUMX(Product, COUNTROWS(RELATEDTABLE(Sales)))", "Product"),
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn measure(name: &str, formula: &str, table: &str) -> Measure {
    Measure { name: name.into(), formula: formula.into(), table: Some(table.into()), format: None, expr: None }
}

fn text(value: &str) -> Scalar {
    Scalar::String(value.into())
}

/// Evaluates a scalar formula together with the storage counters it moved.
fn eval(model: &DataModel, formula: &str) -> (Scalar, StorageStats) {
    let evaluator = Evaluator::new(model).expect("evaluator");
    let expr = parse_expression(formula).expect("parse");
    let bound = Binder::new(model).bind_expression(&expr).expect("bind");
    let value = evaluator.evaluate(&bound).expect("evaluate");
    (value, evaluator.storage_stats())
}

fn eval_table(model: &DataModel, formula: &str) -> (TableValue, StorageStats) {
    let evaluator = Evaluator::new(model).expect("evaluator");
    let expr = parse_expression(formula).expect("parse");
    let bound = Binder::new(model).bind_expression(&expr).expect("bind");
    let table = evaluator.evaluate_table(&bound).expect("table");
    (table, evaluator.storage_stats())
}

fn assert_close(actual: &Scalar, expected: f64, label: &str) {
    match actual {
        Scalar::Decimal(value) => assert!((value - expected).abs() <= 1e-9, "{label}: expected {expected}, got {value}"),
        other => panic!("{label}: expected a decimal {expected}, got {other:?}"),
    }
}

#[test]
fn fixture_is_materialized_faithfully() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let storage = evaluator.storage().expect("the fixture materializes");
    assert!(storage.is_faithful(), "every fixture cell round-trips, so storage answers for the rows");
    assert!(evaluator.storage_error().is_none());
    assert_eq!(storage.tables().len(), 2);
    assert_eq!(storage.relationships().len(), 1);
}

#[test]
fn column_aggregates_match_documented_values() {
    let model = model();
    // 100 + 50 + 30 + 70 + 20 + 10 = 280 (row 5 has a blank Amount).
    let (value, stats) = eval(&model, "SUM(Sales[Amount])");
    assert_eq!(value, Scalar::Decimal(280.0));
    assert!(stats.aggregates > 0, "SUM must be served by storage, got {stats:?}");

    // 280 / 6 non-blank rows.
    let (value, stats) = eval(&model, "AVERAGE(Sales[Amount])");
    assert_eq!(value, Scalar::Decimal(280.0 / 6.0));
    assert!(stats.aggregates > 0);

    // COUNT ignores the blank Amount; COUNTA counts it as absent; COUNTBLANK is exactly one cell.
    assert_eq!(eval(&model, "COUNT(Sales[Amount])").0, Scalar::Integer(6));
    assert_eq!(eval(&model, "COUNTA(Sales[Amount])").0, Scalar::Integer(6));
    assert_eq!(eval(&model, "COUNTBLANK(Sales[Amount])").0, Scalar::Integer(1));

    // 100, 50, 30, 70, 20, 10 and BLANK are seven distinct values: DAX counts BLANK as one.
    let (value, stats) = eval(&model, "DISTINCTCOUNT(Sales[Amount])");
    assert_eq!(value, Scalar::Integer(7));
    assert!(stats.aggregates > 0);

    assert_eq!(eval(&model, "MIN(Sales[Amount])").0, Scalar::Decimal(10.0));
    assert_eq!(eval(&model, "MAX(Sales[Amount])").0, Scalar::Decimal(100.0));
    // 100 * 50 * 30 * 70 * 20 * 10 = 2,100,000,000.
    assert_eq!(eval(&model, "PRODUCT(Sales[Amount])").0, Scalar::Decimal(2_100_000_000.0));

    // Currency is read in major units: MIN is 0.75 and MAX is 20.00 (Excel agrees).
    let (value, stats) = eval(&model, "MIN(Product[UnitPrice])");
    assert_eq!(value, Scalar::Decimal(0.75));
    assert!(stats.aggregates > 0);
    assert_eq!(eval(&model, "MAX(Product[UnitPrice])").0, Scalar::Decimal(20.0));
}

#[test]
fn iterator_aggregates_match_documented_values() {
    let model = model();
    let (value, stats) = eval(&model, "SUMX(Sales, Sales[Amount])");
    assert_eq!(value, Scalar::Decimal(280.0));
    assert!(stats.iterators > 0, "a plain-column SUMX must be answered from the column, got {stats:?}");
    assert!(stats.rows_from_storage >= 7);

    let (value, stats) = eval(&model, "COUNTROWS(Sales)");
    assert_eq!(value, Scalar::Integer(7));
    assert!(stats.count_rows > 0);

    // FILTER needs a row context, so this iterator keeps the row path but must still be correct.
    // 100 + 50 + 70 = 220 are the amounts above 40.
    let (value, stats) = eval(&model, "SUMX(FILTER(Sales, Sales[Amount] > 40), Sales[Amount])");
    assert_eq!(value, Scalar::Decimal(220.0));
    assert!(stats.iterators == 0, "a FILTER table argument cannot use the plain-column fast path");
    // The FILTER itself still reads its source table from columns.
    assert!(stats.rows_from_storage >= 7);
}

#[test]
fn filter_context_aggregates_match_documented_values() {
    let model = model();
    // East rows: 100 + 50 + blank = 150.
    let (value, stats) = eval(&model, "CALCULATE(SUM(Sales[Amount]), Sales[Region] = \"East\")");
    assert_eq!(value, Scalar::Decimal(150.0));
    assert!(stats.aggregates > 0);
    assert!(stats.filter_scans > 0, "the Region filter must be a storage column filter, got {stats:?}");

    // Rows of a related table restrict this one: Hardware products are keys 1 and 2, so
    // amounts 100 + 50 + 30 + 10 = 190 (the blank amount adds nothing).
    let (value, stats) = eval(&model, "CALCULATE(SUM(Sales[Amount]), Product[Category] = \"Hardware\")");
    assert_eq!(value, Scalar::Decimal(190.0));
    assert!(stats.relationship_lookups > 0, "filter propagation must use the relationship index, got {stats:?}");
}

#[test]
fn related_joins_match_documented_values() {
    let model = model();
    // Unit price x quantity per row: 20*2 + 5*5 + 20*3 + 12.5*1 + (blank) + 0.75*4 + 5*2
    //                              = 40 + 25 + 60 + 12.5 + 3 + 10 = 150.5.
    let (value, stats) = eval(&model, "SUMX(Sales, Sales[Quantity] * RELATED(Product[UnitPrice]))");
    assert_eq!(value, Scalar::Decimal(150.5));
    assert!(stats.related > 0, "RELATED must resolve through the relationship index, got {stats:?}");

    // Sales rows per product: 3 (key 1) + 2 (key 2) + 1 (key 3) + 1 (key 4) = 7.
    let (value, stats) = eval(&model, "SUMX(Product, COUNTROWS(RELATEDTABLE(Sales)))");
    assert_eq!(value, Scalar::Decimal(7.0));
    assert!(stats.related_table > 0, "RELATEDTABLE must use the relationship index, got {stats:?}");

    // The one-sided price of a related row is available without an iterator too.
    let (value, _) = eval(&model, "SUMX(Product, Product[UnitPrice])");
    assert_eq!(value, Scalar::Decimal(38.25));
}

#[test]
fn values_and_distinct_match_documented_values() {
    let model = model();
    // Regions in first-appearance order with BLANK last: East, West, North, (blank).
    let (table, stats) = eval_table(&model, "VALUES(Sales[Region])");
    assert_eq!(table.columns.len(), 1);
    assert_eq!(table.rows, vec![vec![text("East")], vec![text("West")], vec![text("North")], vec![Scalar::Blank]]);
    assert_eq!(table.lineage, Some(1));
    assert!(stats.distinct_values > 0, "VALUES must dedupe through storage, got {stats:?}");

    // DISTINCT agrees, and DISTINCTCOUNT counts the same four values.
    let (distinct, _) = eval_table(&model, "DISTINCT(Sales[Region])");
    assert_eq!(distinct.rows, table.rows);
    assert_eq!(eval(&model, "DISTINCTCOUNT(Sales[Region])").0, Scalar::Integer(4));
}

#[test]
fn statistical_aggregate_matches_documented_values() {
    let model = model();
    // Observations 100, 50, 30, 70, 20, 10; mean 280/6.
    let mean = 280.0 / 6.0;
    let squares: f64 = [100.0, 50.0, 30.0, 70.0, 20.0, 10.0].iter().map(|value| (value - mean) * (value - mean)).sum();
    let (value, stats) = eval(&model, "STDEV.S(Sales[Amount])");
    assert_close(&value, (squares / 5.0).sqrt(), "STDEV.S");
    assert!(stats.aggregates > 0, "STDEV.S must read observations from storage, got {stats:?}");
}

#[test]
fn pivot_matches_documented_values() {
    let model = model();
    let pivot = PivotTable::new("Sales by region", "Total Amount")
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular);
    let (table, stats) = pivot.render_with_stats(&model).expect("render pivot");

    assert_eq!(table.columns.iter().map(|column| column.name.clone()).collect::<Vec<_>>(), vec!["Region".to_string(), "Total Amount".to_string()]);
    // BLANK 10, East 150, North 20, West 100; total 280.
    assert_eq!(table.rows.len(), 5);
    assert_eq!(table.rows[0], vec![Scalar::Blank, Scalar::Decimal(10.0)]);
    assert_eq!(table.rows[1], vec![text("East"), Scalar::Decimal(150.0)]);
    assert_eq!(table.rows[2], vec![text("North"), Scalar::Decimal(20.0)]);
    assert_eq!(table.rows[3], vec![text("West"), Scalar::Decimal(100.0)]);
    assert_eq!(table.rows[4], vec![text("Grand Total"), Scalar::Decimal(280.0)]);

    assert!(stats.aggregates > 0, "each pivot cell is a storage aggregate, got {stats:?}");
    assert_eq!(stats.pivot_group_rows, 7, "grouping must read the seven source rows from columns");
}

#[test]
fn filtered_pivot_matches_documented_values() {
    let model = model();
    // Keeping East and West only: 150 + 100 = 250, and BLANK/North rows are gone.
    let pivot = PivotTable::new("Sales in East and West", "Total Amount")
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_filter(PivotFilter::new("Sales[Region]", [text("East"), text("West")]))
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular);
    let (table, stats) = pivot.render_with_stats(&model).expect("render pivot");

    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0], vec![text("East"), Scalar::Decimal(150.0)]);
    assert_eq!(table.rows[1], vec![text("West"), Scalar::Decimal(100.0)]);
    assert_eq!(table.rows[2], vec![text("Grand Total"), Scalar::Decimal(250.0)]);
    // Five rows survive the filter (East x3, West x2).
    assert_eq!(stats.pivot_group_rows, 5);
}

#[test]
fn pivot_cross_tab_matches_documented_values() {
    let model = model();
    // Rows Region, columns ProductKey; cells are the amount per combination.
    let pivot = PivotTable::new("Region by product", "Total Amount")
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_column_fields(vec!["Sales[ProductKey]".into()])
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular);
    let (table, _) = pivot.render_with_stats(&model).expect("render pivot");

    let header: Vec<String> = table.columns.iter().map(|column| column.name.clone()).collect();
    assert_eq!(header, vec!["Region".to_string(), "1".to_string(), "2".to_string(), "3".to_string(), "4".to_string(), "Grand Total".to_string()]);
    assert_eq!(table.rows[0], vec![Scalar::Blank, Scalar::Blank, Scalar::Decimal(10.0), Scalar::Blank, Scalar::Blank, Scalar::Decimal(10.0)]);
    assert_eq!(
        table.rows[1],
        vec![text("East"), Scalar::Decimal(100.0), Scalar::Decimal(50.0), Scalar::Blank, Scalar::Blank, Scalar::Decimal(150.0)]
    );
    assert_eq!(table.rows[2], vec![text("North"), Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Decimal(20.0), Scalar::Decimal(20.0)]);
    assert_eq!(table.rows[3], vec![text("West"), Scalar::Decimal(30.0), Scalar::Blank, Scalar::Decimal(70.0), Scalar::Blank, Scalar::Decimal(100.0)]);
    assert_eq!(
        table.rows[4],
        vec![
            text("Grand Total"),
            Scalar::Decimal(130.0),
            Scalar::Decimal(60.0),
            Scalar::Decimal(70.0),
            Scalar::Decimal(20.0),
            Scalar::Decimal(280.0)
        ]
    );
}

#[test]
fn measures_evaluate_through_storage() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let filtered = gridcraft_dax::FilterContext::default();
    assert_eq!(evaluator.evaluate_measure("Total Amount", filtered.clone()).expect("measure"), Scalar::Decimal(280.0));
    assert_eq!(evaluator.evaluate_measure("Rows", filtered.clone()).expect("measure"), Scalar::Integer(7));
    assert_eq!(evaluator.evaluate_measure("Product Count", filtered.clone()).expect("measure"), Scalar::Integer(4));
    assert_eq!(evaluator.evaluate_measure("Extended Price", filtered.clone()).expect("measure"), Scalar::Decimal(150.5));
    assert_eq!(evaluator.evaluate_measure("Sales Rows Per Product", filtered).expect("measure"), Scalar::Decimal(7.0));
    let stats = evaluator.storage_stats();
    // Total Amount and Product Count are column aggregates; Rows is COUNTROWS; Extended Price and
    // Sales Rows Per Product resolve their RELATED/RELATEDTABLE calls through the index (their
    // derived iterator expressions stay row-based, so `iterators` is zero here).
    assert!(stats.aggregates >= 2 && stats.count_rows == 1 && stats.related > 0 && stats.related_table > 0, "{stats:?}");
}
