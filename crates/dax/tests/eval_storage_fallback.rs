//! Non-faithful and unmaterializable models: storage must step aside, never change a result.
//!
//! `ModelStorage` is lossless only when every table is rectangular and every stored value equals
//! the model scalar. These tests cover the three ways that can fail — ragged rows, a promoted
//! column representation, and a model [`ModelStorage::from_model`] rejects outright — and assert
//! that evaluation keeps its original (row-scan) DAX results and reports the fallback.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::storage::StorageError;
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn eval(model: &DataModel, formula: &str) -> Scalar {
    let evaluator = Evaluator::new(model).expect("evaluator");
    let expr = parse_expression(formula).expect("parse");
    let bound = Binder::new(model).bind_expression(&expr).expect("bind");
    evaluator.evaluate(&bound).expect("evaluate")
}

fn single_table(name: &str, columns: &[(&str, &str)], rows: Vec<Vec<Scalar>>) -> DataModel {
    DataModel {
        tables: vec![ModelTable {
            name: name.into(),
            columns: columns.iter().map(|(name, kind)| ModelColumn { name: (*name).into(), data_type: Some((*kind).into()) }).collect(),
            rows,
        }],
        ..DataModel::default()
    }
}

#[test]
fn ragged_rows_keep_the_row_scan_semantics() {
    // Row 0 only supplies A; the missing B cell is a row the columnar copy would blank-extend.
    let model =
        single_table("Sales", &[("A", "Int64"), ("B", "Int64")], vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(2), Scalar::Integer(3)]]);
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let storage = evaluator.storage().expect("the ragged model still materializes");
    assert!(!storage.is_faithful(), "a ragged row cannot round-trip");

    let expr = parse_expression("SUM(Sales[B])").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    // `TableValue::column` skips a row shorter than the requested column, so only row 1 counts.
    assert_eq!(evaluator.evaluate(&bound).expect("evaluate"), Scalar::Decimal(3.0));
    assert_eq!(eval(&model, "COUNTROWS(Sales)"), Scalar::Integer(2));
    assert!(evaluator.storage_stats().row_scan_fallbacks > 0, "{:?}", evaluator.storage_stats());
}

#[test]
fn promoted_columns_keep_the_row_scan_semantics() {
    // Booleans mixed with an integer are promoted to dictionary-encoded text by the storage
    // inference; DAX still reads TRUE as 1 in a numeric aggregate, so storage must not answer.
    let model = single_table("Mixed", &[("Value", "Boolean")], vec![vec![Scalar::Boolean(true)], vec![Scalar::Integer(2)], vec![Scalar::Blank]]);
    let evaluator = Evaluator::new(&model).expect("evaluator");
    assert!(!evaluator.storage().expect("storage").is_faithful());

    let expr = parse_expression("SUM(Mixed[Value])").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate(&bound).expect("evaluate"), Scalar::Decimal(3.0));
    assert_eq!(eval(&model, "COUNT(Mixed[Value])"), Scalar::Integer(1), "COUNT ignores booleans");
    assert_eq!(eval(&model, "COUNTA(Mixed[Value])"), Scalar::Integer(2));
    assert!(evaluator.storage_stats().row_scan_fallbacks > 0, "{:?}", evaluator.storage_stats());
}

#[test]
fn unmaterializable_models_still_evaluate() {
    // A relationship naming a missing table makes `ModelStorage::from_model` fail. That is not an
    // evaluation error: the row-scan path is the documented fallback and the reason is reported.
    let mut model = single_table("Sales", &[("Amount", "Int64")], vec![vec![Scalar::Integer(4)], vec![Scalar::Integer(6)]]);
    model.relationships.push(Relationship {
        from_table: "Sales".into(),
        from_column: "Amount".into(),
        to_table: "Ghost".into(),
        to_column: "Amount".into(),
        bidirectional: false,
        inactive: false,
    });
    let evaluator = Evaluator::new(&model).expect("evaluator still builds");
    assert!(evaluator.storage().is_none());
    assert_eq!(evaluator.storage_error(), Some(&StorageError::UnknownTable { table: "Ghost".into() }));

    let expr = parse_expression("SUM(Sales[Amount])").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate(&bound).expect("evaluate"), Scalar::Decimal(10.0));
    assert!(evaluator.storage_stats().row_scan_fallbacks > 0);
}

#[test]
fn countblank_counts_empty_text_in_both_paths() {
    // DAX COUNTBLANK counts an empty string as blank. The storage fast path reads that from the
    // string dictionary, so pin it against the documented row-path rule.
    let model = single_table(
        "Notes",
        &[("Text", "String")],
        vec![vec![Scalar::String(String::new())], vec![Scalar::String("x".into())], vec![Scalar::Blank]],
    );
    let evaluator = Evaluator::new(&model).expect("evaluator");
    assert!(evaluator.storage().expect("storage").is_faithful());
    assert_eq!(eval(&model, "COUNTBLANK(Notes[Text])"), Scalar::Integer(2));
    assert_eq!(eval(&model, "COUNTA(Notes[Text])"), Scalar::Integer(2));
    assert_eq!(eval(&model, "DISTINCTCOUNT(Notes[Text])"), Scalar::Integer(3));
}

#[test]
fn mixed_numeric_columns_promote_but_keep_their_sum() {
    // Integers and decimals mix into one decimal column, which cannot round-trip. The sum is the
    // same either way, so this pins the value while the fallback keeps the representation honest.
    let model = single_table(
        "Money",
        &[("Amount", "Double")],
        vec![vec![Scalar::Integer(1)], vec![Scalar::Decimal(0.5)], vec![Scalar::Blank], vec![Scalar::Integer(3)]],
    );
    let evaluator = Evaluator::new(&model).expect("evaluator");
    assert!(!evaluator.storage().expect("storage").is_faithful());
    assert_eq!(eval(&model, "SUM(Money[Amount])"), Scalar::Decimal(4.5));
    assert_eq!(eval(&model, "COUNT(Money[Amount])"), Scalar::Integer(3));
    assert_eq!(eval(&model, "COUNTBLANK(Money[Amount])"), Scalar::Integer(1));
}

#[test]
fn pivot_over_a_non_faithful_model_groups_rows() {
    // Row 1 has no Amount cell, so the model cannot be stored faithfully and the pivot must go
    // back to grouping model rows. The blank amount makes the West cell BLANK, not zero.
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
            ],
            rows: vec![vec![Scalar::String("East".into()), Scalar::Integer(10)], vec![Scalar::String("West".into())]],
        }],
        measures: vec![gridcraft_dax::Measure {
            name: "Total".into(),
            formula: "SUM(Sales[Amount])".into(),
            table: Some("Sales".into()),
            format: None,
            expr: None,
        }],
        ..DataModel::default()
    };
    model.parse_formulas().expect("model parses");

    let pivot = gridcraft_dax::pivot_table::PivotTable::new("By region", "Total")
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_grand_totals(true)
        .with_layout(gridcraft_dax::pivot_table::PivotLayout::Tabular);
    let (table, stats) = pivot.render_with_stats(&model).expect("render");

    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0], vec![Scalar::String("East".into()), Scalar::Decimal(10.0)]);
    assert_eq!(table.rows[1], vec![Scalar::String("West".into()), Scalar::Blank]);
    assert_eq!(table.rows[2], vec![Scalar::String("Grand Total".into()), Scalar::Decimal(10.0)]);
    assert_eq!(stats.pivot_group_rows, 0, "grouping fell back to the model rows: {stats:?}");
    assert!(stats.row_scan_fallbacks > 0);
}
