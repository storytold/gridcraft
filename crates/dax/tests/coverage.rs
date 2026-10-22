//! Every catalogued DAX function must reach an evaluator path.
//!
//! The catalog is the source of truth for the 484 functions. This test builds a minimal call for
//! each spec (one argument per parameter, using the parameter kind), binds it against a small
//! model and evaluates it. A structured "not implemented" error, or a bind failure, fails the
//! test: both mean the function has no evaluator path.

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

/// Builds a minimal call with `arity` arguments taken from the first matching overload.
fn formula(name: &str, min_args: usize) -> String {
    let spec = gridcraft_dax::functions::lookup(name).expect("catalogued");
    let params = spec.overloads.iter().find(|overload| overload.params.len() == min_args).map(|overload| overload.params).unwrap_or(&[]);
    let args: Vec<String> = (0..min_args).map(|index| placeholder(params.get(index).copied().unwrap_or(ParamKind::Any)).to_string()).collect();
    format!("{name}({})", args.join(", "))
}

#[test]
fn every_catalogued_function_has_an_evaluator_path() {
    let model = model();
    let binder = Binder::new(&model);
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let mut failures = Vec::new();

    for spec in catalog() {
        let source = formula(spec.name, spec.min_args);
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

    assert!(failures.is_empty(), "{} functions have no evaluator path:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn catalog_counts_by_category() {
    // Guards the documentation numbers: 484 catalogued, split across the documented families.
    assert_eq!(catalog().len(), 484);
    for category in FunctionCategory::ALL {
        assert!(catalog().iter().any(|spec| spec.category == category), "{} has functions", category.name());
    }
}
