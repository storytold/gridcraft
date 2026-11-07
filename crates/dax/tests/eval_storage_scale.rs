//! Scale test: 100k rows through the columnar substrate.
//!
//! What is measured:
//!
//! * wall-clock time of `Evaluator::new` (one materialization), of `SUMX`/`COUNTROWS`/
//!   `DISTINCTCOUNT` and of a 8-group pivot with a grand total, printed with `--nocapture` and
//!   asserted to stay under [`BUDGET`];
//! * the storage counters of the same calls, which prove the numbers were computed from columns
//!   and bound the work: a pivot cell may only touch rows of its own group plus one pass over the
//!   source row ids, so `rows_from_storage` has to stay linear in the row count (an accidental
//!   per-cell full scan of a 100k-row model across 100 groups would blow the budget and the
//!   bound below).
//!
//! Peak memory is not sampled (it is not portable across the test hosts); the bound that matters
//! is structural: the model keeps one scalar per cell, storage adds one typed value plus one null
//! bit per cell, and no path clones model rows — `rows_from_storage` counts rows served from
//! columns, and the only `Vec<Vec<Scalar>>` materialized here is the pivot's own output table
//! (9 rows).
//!
//! This is an integration-test binary: it is never compiled for wasm, so reading the clock here
//! cannot hit the trap the workspace `disallowed_methods` lint guards against.
#![allow(clippy::disallowed_methods)]

use std::time::{Duration, Instant};

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable};
use gridcraft_dax::pivot_table::{PivotLayout, PivotTable};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, FilterContext, StorageStats, parse_expression};

const ROWS: usize = 100_000;
const REGIONS: [&str; 8] = ["East", "West", "North", "South", "Centre", "Coastal", "Inland", "Metro"];
/// Generous upper bound: a debug build handles this fixture in well under a second on the
/// development machine, so 60 s only catches a pathological (quadratic or repeating-scan) change.
const BUDGET: Duration = Duration::from_secs(60);

fn amount(row: usize) -> i64 {
    (row % 1_000) as i64
}

fn region(row: usize) -> &'static str {
    REGIONS[row % REGIONS.len()]
}

fn product_key(row: usize) -> i64 {
    (row % 50) as i64 + 1
}

fn model() -> DataModel {
    let mut rows = Vec::with_capacity(ROWS);
    for row in 0..ROWS {
        rows.push(vec![
            Scalar::Integer(product_key(row)),
            Scalar::String(region(row).into()),
            Scalar::Integer(amount(row)),
            Scalar::Integer((row % 7) as i64 + 1),
        ]);
    }
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                ModelColumn { name: "Quantity".into(), data_type: Some("Int64".into()) },
            ],
            rows,
        }],
        relationships: Vec::new(),
        measures: vec![
            Measure { name: "Total Amount".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure { name: "Rows".into(), formula: "COUNTROWS(Sales)".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure {
                name: "Products".into(),
                formula: "DISTINCTCOUNT(Sales[ProductKey])".into(),
                table: Some("Sales".into()),
                format: None,
                expr: None,
            },
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn evaluate(evaluator: &Evaluator<'_>, model: &DataModel, formula: &str) -> Scalar {
    let expr = parse_expression(formula).expect("parse");
    let bound = Binder::new(model).bind_expression(&expr).expect("bind");
    evaluator.evaluate(&bound).expect("evaluate")
}

#[test]
fn hundred_thousand_rows_evaluate_within_the_budget() {
    let model = model();
    // Independent expectations from the generator, not from the DAX engine.
    let mut per_region = vec![0i64; REGIONS.len()];
    let mut total = 0i64;
    for row in 0..ROWS {
        per_region[row % REGIONS.len()] += amount(row);
        total += amount(row);
    }

    let built = Instant::now();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let build_time = built.elapsed();
    let storage = evaluator.storage().expect("the generated model materializes");
    assert!(storage.is_faithful(), "the generated columns are homogeneous and rectangular");

    let started = Instant::now();
    // sum of (row % 1000) over 100k rows = 100 blocks of 0..999 = 100 * 499_500.
    assert_eq!(evaluate(&evaluator, &model, "SUMX(Sales, Sales[Amount])"), Scalar::Decimal(total as f64));
    let sum_time = started.elapsed();

    let started = Instant::now();
    assert_eq!(evaluate(&evaluator, &model, "COUNTROWS(Sales)"), Scalar::Integer(ROWS as i64));
    let count_time = started.elapsed();

    let started = Instant::now();
    assert_eq!(evaluate(&evaluator, &model, "DISTINCTCOUNT(Sales[ProductKey])"), Scalar::Integer(50));
    let distinct_time = started.elapsed();

    let pivot = PivotTable::new("Sales by region", "Total Amount")
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular);
    let started = Instant::now();
    let (table, stats) = pivot.render_with_stats(&model).expect("render");
    let pivot_time = started.elapsed();

    // One row per region in ScalarKey order plus the grand total.
    let mut sorted: Vec<(&str, i64)> = REGIONS.iter().copied().zip(per_region.iter().copied()).collect();
    sorted.sort_by(|left, right| left.0.cmp(right.0));
    let mut expected: Vec<Vec<Scalar>> =
        sorted.iter().map(|(label, sum)| vec![Scalar::String((*label).into()), Scalar::Decimal(*sum as f64)]).collect();
    expected.push(vec![Scalar::String("Grand Total".into()), Scalar::Decimal(total as f64)]);
    assert_eq!(table.rows, expected);
    assert_eq!(table.rows.len(), REGIONS.len() + 1);

    let total_time = build_time + sum_time + count_time + distinct_time + pivot_time;
    eprintln!(
        "100k rows: build {build_time:?}, SUMX {sum_time:?}, COUNTROWS {count_time:?}, DISTINCTCOUNT {distinct_time:?}, pivot {pivot_time:?}, total {total_time:?}"
    );
    // The pivot renders through its own evaluator, so read the two counter snapshots separately.
    let direct = evaluator.storage_stats();
    eprintln!("counters for SUMX/COUNTROWS/DISTINCTCOUNT: {direct:?}");
    eprintln!("counters for the pivot render: {stats:?}");

    // Every one of these calls was answered from columns, not from a row scan.
    assert!(direct.iterators > 0, "SUMX went through storage: {direct:?}");
    assert!(direct.count_rows > 0, "COUNTROWS went through storage: {direct:?}");
    assert!(direct.aggregates > 0, "DISTINCTCOUNT went through storage: {direct:?}");
    assert!(stats.aggregates > 0, "every pivot cell went through storage: {stats:?}");
    assert_eq!(stats.pivot_group_rows, ROWS as u64, "grouping read every source row once");
    assert_eq!(evaluator.storage_stats().row_scan_fallbacks, 0);

    // Linear work per cell: 8 groups + the grand total + the grouping pass stay within a few
    // passes of the 100k rows (an accidental per-cell full scan of the model would not).
    assert!(stats.rows_from_storage < 20 * ROWS as u64, "rows served from columns must stay linear: {stats:?}");
    assert!(total_time < BUDGET, "100k rows took {total_time:?}, over the {BUDGET:?} budget");
}

/// The filters path at scale: a single-group pivot restricted to one region.
#[test]
fn filtered_pivot_at_scale_counts_one_region() {
    let model = model();
    let mut expected = 0i64;
    for row in (0..ROWS).filter(|row| row % REGIONS.len() == 0) {
        expected += amount(row);
    }
    let pivot = PivotTable::new("East only", "Total Amount")
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_filter(gridcraft_dax::pivot_table::PivotFilter::new("Sales[Region]", [Scalar::String("East".into())]))
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular);
    let started = Instant::now();
    let (table, stats) = pivot.render_with_stats(&model).expect("render");
    let elapsed = started.elapsed();
    eprintln!("100k rows, filtered pivot: {elapsed:?}, counters {stats:?}");

    assert_eq!(table.rows.len(), 2, "the East leaf plus the grand total");
    assert_eq!(table.rows[0], vec![Scalar::String("East".into()), Scalar::Decimal(expected as f64)]);
    assert_eq!(table.rows[1], vec![Scalar::String("Grand Total".into()), Scalar::Decimal(expected as f64)]);
    assert_eq!(stats.pivot_group_rows, ROWS as u64 / REGIONS.len() as u64);
    assert!(elapsed < BUDGET);
}

/// Every measure evaluates at scale through storage, without a per-measure rebuild.
#[test]
fn measures_at_scale_share_one_substrate() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let mut total = 0i64;
    for row in 0..ROWS {
        total += amount(row);
    }
    assert_eq!(evaluator.evaluate_measure("Total Amount", FilterContext::default()).expect("measure"), Scalar::Decimal(total as f64));
    assert_eq!(evaluator.evaluate_measure("Rows", FilterContext::default()).expect("measure"), Scalar::Integer(ROWS as i64));
    assert_eq!(evaluator.evaluate_measure("Products", FilterContext::default()).expect("measure"), Scalar::Integer(50));
    let stats: StorageStats = evaluator.storage_stats();
    assert!(stats.aggregates >= 2 && stats.count_rows == 1, "{stats:?}");
}
