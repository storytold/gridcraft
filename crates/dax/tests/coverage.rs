//! Every catalogued DAX function must reach an evaluator path.
//!
//! The catalog is the source of truth for the 484 functions. These tests build a minimal call for
//! each spec — at *every* documented arity, using each overload's parameter kinds — bind it
//! against a small model and evaluate it. A structured "not implemented" error, or a bind
//! failure, fails the test: both mean the function has no evaluator path.
//!
//! `catalog_counts_by_category` guards the documented catalog numbers; `catalog_names_are_unique`
//! guards against a duplicate record hiding a missing function.

use gridcraft_dax::functions::{FunctionCategory, ParamKind, ReturnType, catalog};
use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Quantity".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Flag".into(), data_type: Some("Boolean".into()) },
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Decimal(10.0), Scalar::Integer(2), Scalar::Boolean(true), Scalar::DateTime(45292.0)],
                    vec![Scalar::String("West".into()), Scalar::Decimal(20.0), Scalar::Integer(3), Scalar::Boolean(false), Scalar::DateTime(45293.0)],
                    vec![Scalar::String("East".into()), Scalar::Decimal(30.0), Scalar::Integer(4), Scalar::Boolean(true), Scalar::DateTime(45294.0)],
                ],
            },
            ModelTable {
                name: "Products".into(),
                columns: vec![
                    ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Price".into(), data_type: Some("Double".into()) },
                ],
                rows: vec![vec![Scalar::String("Bike".into()), Scalar::Decimal(100.0)], vec![Scalar::String("Helmet".into()), Scalar::Decimal(25.0)]],
            },
        ],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    model
}

/// A syntactically valid argument for a catalog parameter kind.
fn placeholder(kind: ParamKind) -> &'static str {
    match kind {
        ParamKind::Column => "Sales[Amount]",
        ParamKind::Table | ParamKind::Reference => "Sales",
        ParamKind::Expression => "Sales[Amount]",
        ParamKind::Logical | ParamKind::Boolean => "TRUE",
        ParamKind::Text => "\"text\"",
        ParamKind::Date => "DATE(2024,1,1)",
        ParamKind::Function => "SUM",
        ParamKind::Number | ParamKind::Scalar | ParamKind::Any | ParamKind::Variant | ParamKind::Json => "1",
    }
}

/// Builds a minimal call with `arity` arguments taken from the matching overload (or the
/// descriptive signature when no overload matches, which the shape test rejects).
fn formula(name: &str, arity: usize) -> String {
    let spec = gridcraft_dax::functions::lookup(name).expect("catalogued");
    let kinds: Vec<ParamKind> = match spec.overload_for(arity) {
        Some(overload) => (0..arity).map(|index| overload.kind_at(index).unwrap_or(ParamKind::Any)).collect(),
        None => (0..arity).map(|index| spec.params.get(index).map(|param| param.kind).unwrap_or(ParamKind::Any)).collect(),
    };
    let args: Vec<&str> = kinds.iter().map(|kind| placeholder(*kind)).collect();
    format!("{name}({})", args.join(", "))
}

/// Every arity the catalog documents for a function, including the variadic minimum.
fn documented_arities(name: &str) -> Vec<usize> {
    let spec = gridcraft_dax::functions::lookup(name).expect("catalogued");
    let mut arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
    arities.push(spec.min_args);
    arities.sort_unstable();
    arities.dedup();
    arities
}

#[test]
fn every_catalogued_function_has_an_evaluator_path() {
    let model = model();
    let binder = Binder::new(&model);
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let mut failures = Vec::new();
    let mut checked = 0usize;

    for spec in catalog() {
        for arity in documented_arities(spec.name) {
            checked += 1;
            let source = formula(spec.name, arity);
            let expr = match parse_expression(&source) {
                Ok(expr) => expr,
                Err(error) => {
                    failures.push(format!("{}: parse error in `{source}`: {error}", spec.name));
                    continue;
                }
            };
            let bound = match binder.bind_expression(&expr) {
                Ok(bound) => bound,
                Err(error) => {
                    failures.push(format!("{}: bind error in `{source}`: {error}", spec.name));
                    continue;
                }
            };
            let result = if spec.return_type == ReturnType::Table {
                evaluator.evaluate_table(&bound).map(|table| Scalar::Integer(table.rows.len() as i64))
            } else {
                evaluator.evaluate(&bound)
            };
            if let Err(error) = result
                && error.message.contains("not implemented")
            {
                failures.push(format!("{}: `{source}` -> {error}", spec.name));
            }
        }
    }

    assert!(checked >= catalog().len(), "every function is checked at least once");
    assert!(failures.is_empty(), "{} calls have no evaluator path:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn catalog_counts_by_category() {
    // Guards the documentation numbers: 484 catalogued, split across the documented families.
    assert_eq!(catalog().len(), 484);
    for category in FunctionCategory::ALL {
        assert!(catalog().iter().any(|spec| spec.category == category), "{} has functions", category.name());
    }

    // The exact split, so a function moving family (or a silently dropped record) fails here
    // instead of hiding behind the total.
    let expected: [(FunctionCategory, usize); 15] = [
        (FunctionCategory::Aggregation, 22),
        (FunctionCategory::DateAndTime, 24),
        (FunctionCategory::Filter, 29),
        (FunctionCategory::Financial, 51),
        (FunctionCategory::Information, 114),
        (FunctionCategory::Logical, 13),
        (FunctionCategory::MathAndTrig, 49),
        (FunctionCategory::Other, 16),
        (FunctionCategory::ParentChild, 5),
        (FunctionCategory::RelationshipManagement, 4),
        (FunctionCategory::Statistical, 43),
        (FunctionCategory::TableManipulation, 35),
        (FunctionCategory::Text, 23),
        (FunctionCategory::TimeIntelligence, 45),
        (FunctionCategory::VisualCalculations, 11),
    ];
    let mut failures = Vec::new();
    for (category, count) in expected {
        let actual = catalog().iter().filter(|spec| spec.category == category).count();
        if actual != count {
            failures.push(format!("{}: {actual} catalogued, documented {count}", category.name()));
        }
    }
    assert!(failures.is_empty(), "catalog family counts drifted:\n{}", failures.join("\n"));
    assert_eq!(expected.iter().map(|(_, count)| count).sum::<usize>(), catalog().len(), "the family counts add up to the catalog");
}

#[test]
fn catalog_names_are_unique() {
    let mut names: Vec<String> = catalog().iter().map(|spec| spec.name.to_ascii_uppercase()).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(total, names.len(), "the catalog has duplicate function names");
    for spec in catalog() {
        assert!(gridcraft_dax::functions::lookup(spec.name).is_some(), "{} is not reachable through lookup", spec.name);
    }
}
