//! Cross-category invariants for the complete DAX function catalog.

use std::collections::BTreeMap;

use gridcraft_dax::functions::{FunctionCategory, ReturnType, catalog, lookup};
use gridcraft_dax::parse_expression;
use gridcraft_dax::types::{DataType, InferredType, infer_type};

#[test]
fn every_category_has_functions_and_names_are_unique() {
    for category in FunctionCategory::ALL {
        let count = catalog().iter().filter(|spec| spec.category == category).count();
        assert!(count > 0, "{} has no functions", category.name());
    }

    let mut names = BTreeMap::new();
    for spec in catalog() {
        if let Some(previous) = names.insert(spec.name, spec.category) {
            panic!("{} appears in both {} and {}", spec.name, previous.name(), spec.category.name());
        }
    }
    assert_eq!(names.len(), catalog().len());
}

#[test]
fn records_have_consistent_signatures_and_parseable_examples() {
    for spec in catalog() {
        assert!(!spec.name.is_empty(), "empty name");
        assert!(!spec.description.is_empty(), "{} has no description", spec.name);
        assert!(!spec.examples.is_empty(), "{} has no examples", spec.name);
        assert!(spec.min_args <= spec.max_args, "{} has min > max", spec.name);

        let variadic = spec.overloads.iter().any(|overload| overload.variadic);
        if variadic {
            assert!(
                spec.overloads.iter().any(|overload| overload.matches(spec.min_args)),
                "{} has no variadic overload for min arity {}",
                spec.name,
                spec.min_args
            );
            assert!(
                spec.overloads.iter().any(|overload| overload.matches(spec.max_args)),
                "{} has no overload for max arity {}",
                spec.name,
                spec.max_args
            );
        } else {
            assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);
            let required = spec.params.iter().filter(|param| !param.optional).count();
            let optional = spec.params.iter().filter(|param| param.optional).count();
            assert_eq!(required, spec.min_args, "{} required params", spec.name);
            assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);
            for arity in spec.min_args..=spec.max_args {
                assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
            }
        }

        for example in spec.examples {
            assert!(parse_expression(example).is_ok(), "{} example does not parse: {example}", spec.name);
        }
        assert!(lookup(&spec.name.to_ascii_lowercase()).is_some(), "{} lookup", spec.name);
    }
}

#[test]
fn catalogued_zero_argument_functions_expose_their_return_type() {
    for spec in catalog().iter().filter(|spec| spec.min_args == 0) {
        let source = format!("{}()", spec.name);
        let expr = parse_expression(&source).unwrap_or_else(|error| panic!("{source}: {error}"));
        let expected = match spec.return_type {
            ReturnType::Integer => InferredType::Data(DataType::Integer),
            ReturnType::Numeric | ReturnType::Decimal => InferredType::Data(DataType::Decimal),
            ReturnType::Currency => InferredType::Data(DataType::Currency),
            ReturnType::Boolean => InferredType::Data(DataType::Boolean),
            ReturnType::DateTime => InferredType::Data(DataType::DateTime),
            ReturnType::String => InferredType::Data(DataType::String),
            ReturnType::Variant => InferredType::Data(DataType::Variant),
            ReturnType::Table => InferredType::Table,
            ReturnType::Blank => InferredType::Blank,
            ReturnType::Any => InferredType::Unknown,
        };
        assert_eq!(infer_type(&expr), Ok(expected), "{} return type", spec.name);
    }
}
