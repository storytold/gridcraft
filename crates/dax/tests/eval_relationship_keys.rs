//! Relationship joins across numeric variants.
//!
//! `Scalar::strict_eq` joins `Integer(1)` with `Decimal(1.0)`, so every path that answers a
//! relationship — filter propagation, `RELATED`, `RELATEDTABLE` and the integrity report — must
//! agree with it. The model here stores a decimal key on the one side and an integer key on the
//! many side, which a per-variant key index would treat as unrelated.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::storage::ModelStorage;
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, IntegrityCheck, parse_expression, validate_model};

fn model() -> DataModel {
    DataModel {
        tables: vec![
            ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                ],
                // The one side spells the key as a decimal.
                rows: vec![
                    vec![Scalar::Decimal(1.0), Scalar::String("Widget".into())],
                    vec![Scalar::Decimal(2.0), Scalar::String("Gadget".into())],
                ],
            },
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                ],
                // The many side stores the same keys as integers.
                rows: vec![vec![Scalar::Integer(1), Scalar::Integer(10)], vec![Scalar::Integer(2), Scalar::Integer(20)]],
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
        measures: Vec::new(),
    }
}

fn eval(src: &str) -> Scalar {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression(src).expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    evaluator.evaluate(&bound).expect("evaluate")
}

#[test]
fn filter_propagation_joins_an_integer_fact_key_to_a_decimal_dimension_key() {
    // The filter is set on the decimal one side and must reach the integer many side.
    assert_eq!(eval("CALCULATE(COUNTROWS(Sales), Product[ProductKey] = 1.0)"), Scalar::Integer(1));
    assert_eq!(eval("CALCULATE(SUM(Sales[Amount]), Product[ProductKey] = 2.0)"), Scalar::Decimal(20.0));
}

#[test]
fn related_and_relatedtable_join_across_numeric_variants() {
    // `RELATED` walks from the integer many-side key to the decimal one-side row.
    assert_eq!(eval("SUMX(Sales, RELATED(Product[ProductKey]))"), Scalar::Decimal(3.0));
    // `RELATEDTABLE` walks the other way: each decimal dimension key finds its integer facts.
    assert_eq!(eval("SUMX(Product, COUNTROWS(RELATEDTABLE(Sales)))"), Scalar::Decimal(2.0));
}

#[test]
fn the_integrity_report_reports_no_orphan_for_equal_keys_in_different_variants() {
    let model = model();
    let report = validate_model(&model);
    assert!(
        report.findings.iter().all(|finding| finding.check != IntegrityCheck::OrphanRow),
        "equal numeric keys must not be reported as orphans: {:?}",
        report.findings
    );
    // The storage-backed entry point must reach the same conclusion.
    let storage = ModelStorage::from_model(&model).expect("storage");
    let report = storage.validate();
    assert!(
        report.findings.iter().all(|finding| finding.check != IntegrityCheck::OrphanRow),
        "the storage path must not report orphans either: {:?}",
        report.findings
    );
}
