//! Integration tests for the relationship management catalog and its static validation.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::types::{InferredType, infer_type};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn relationship_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::RelationshipManagement).collect()
}

const FUNCTIONS: [(&str, usize, usize); 4] = [("CROSSFILTER", 3, 3), ("RELATED", 1, 1), ("RELATEDTABLE", 1, 1), ("USERELATIONSHIP", 2, 2)];

fn parse(src: &str) -> Expr {
    match parse_expression(src) {
        Ok(expr) => expr,
        Err(error) => panic!("{src:?} did not parse: {error}"),
    }
}

fn spec(name: &str) -> &'static FunctionSpec {
    match lookup(name) {
        Some(spec) => spec,
        None => panic!("{name} is missing from the catalog"),
    }
}

fn expression_errors(src: &str) -> Vec<ValidationError> {
    validate_expression(&parse(src))
}

fn call_errors(src: &str) -> Vec<ValidationError> {
    let expr = parse(src);
    match &expr.kind {
        ExprKind::Call(call) => validate_call(call),
        other => panic!("{src:?} is not a call: {other:?}"),
    }
}

fn assert_valid(src: &str) {
    let errors = expression_errors(src);
    assert!(errors.is_empty(), "{src:?} should validate, got {errors:?}");
}

fn assert_invalid_kind(src: &str, kind: ValidationErrorKind) -> Vec<ValidationError> {
    let errors = expression_errors(src);
    assert!(!errors.is_empty(), "{src:?} should be rejected");
    assert!(errors.iter().any(|error| error.kind == kind), "{src:?} should report {kind:?}, got {errors:?}");
    errors
}

fn overload_kinds(spec: &FunctionSpec, arity: usize) -> Option<Vec<ParamKind>> {
    spec.overload_for(arity).map(|overload| overload.params.to_vec())
}

#[test]
fn catalog_has_every_relationship_function_exactly_once_with_documented_arity() {
    let specs = relationship_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::RelationshipManagement);
    }

    let catalogued: std::collections::BTreeSet<&str> = specs.iter().map(|spec| spec.name).collect();
    let documented: std::collections::BTreeSet<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(catalogued, documented);

    // dax.guide lists the functions in this order.
    let order: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
    assert_eq!(order, ["CROSSFILTER", "RELATED", "RELATEDTABLE", "USERELATIONSHIP"]);
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        assert_eq!(lookup(name).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_lowercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_uppercase()).map(|spec| spec.name), Some(name));
    }
    assert_eq!(lookup("Related").map(|spec| spec.name), Some("RELATED"));
    assert!(lookup("RELATEDTABLES").is_none());
    assert!(lookup("CROSS_FILTER").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("RELATED ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in relationship_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);

        let required = spec.params.iter().filter(|param| !param.optional).count();
        let optional = spec.params.iter().filter(|param| param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);
        assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);

        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);
        for arity in spec.min_args..=spec.max_args {
            assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
        }
        let arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
        assert_eq!(arities.iter().min().copied(), Some(spec.min_args), "{} overload min", spec.name);
        assert_eq!(arities.iter().max().copied(), Some(spec.max_args), "{} overload max", spec.name);
        assert_eq!(spec.sugar_of, None, "{} is not syntax sugar", spec.name);
    }
}

#[test]
fn return_type_metadata() {
    for (name, return_type) in
        [("CROSSFILTER", ReturnType::Any), ("RELATED", ReturnType::Any), ("RELATEDTABLE", ReturnType::Table), ("USERELATIONSHIP", ReturnType::Any)]
    {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }
}

#[test]
fn flags_are_honest() {
    for spec in relationship_catalog() {
        assert!(!spec.flags.iterator, "{} is not an iterator", spec.name);
        assert!(!spec.flags.volatile, "{} is not volatile", spec.name);
        assert!(!spec.flags.not_recommended, "{} is recommended", spec.name);
        assert!(spec.flags.excel_compatible, "{} should be Excel compatible", spec.name);
        assert!(spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
        assert!(!spec.is_iterator(), "{} is_iterator", spec.name);
    }
}

#[test]
fn overloads_model_the_documented_signatures() {
    assert_eq!(overload_kinds(spec("CROSSFILTER"), 3), Some(vec![ParamKind::Column, ParamKind::Column, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("RELATED"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("RELATEDTABLE"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(overload_kinds(spec("USERELATIONSHIP"), 2), Some(vec![ParamKind::Column, ParamKind::Column]));

    let crossfilter = spec("CROSSFILTER");
    assert_eq!(crossfilter.params[0].name, "LeftColumnName");
    assert_eq!(crossfilter.params[1].name, "RightColumnName");
    assert_eq!(crossfilter.params[2].name, "CrossFilterType");
    assert!(crossfilter.params.iter().all(|param| !param.optional));

    let userelationship = spec("USERELATIONSHIP");
    assert_eq!(userelationship.params[0].name, "ColumnName1");
    assert_eq!(userelationship.params[1].name, "ColumnName2");

    assert_eq!(spec("RELATED").params[0].name, "ColumnName");
    assert_eq!(spec("RELATEDTABLE").params[0].name, "Table");
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey], BOTH )",
        "CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey], 0 )",
        "RELATED ( 'Product'[List Price] )",
        "RELATED ( [Amount] )",
        "RELATEDTABLE ( Sales )",
        "USERELATIONSHIP ( Sales[Delivery Date], 'Date'[Date] )",
    ] {
        assert_valid(src);
    }

    for src in [
        "CROSSFILTER ( )",
        "CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey] )",
        "CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey], BOTH, NONE )",
        "RELATED ( )",
        "RELATED ( Sales[Amount], Sales[Quantity] )",
        "RELATEDTABLE ( )",
        "RELATEDTABLE ( Sales, Sales )",
        "USERELATIONSHIP ( Sales[Delivery Date] )",
        "USERELATIONSHIP ( Sales[Delivery Date], 'Date'[Date], 1 )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_arity_boundary_is_enforced() {
    for spec in relationship_catalog() {
        for arity in 0..=4usize {
            let arguments = vec!["[Value]"; arity].join(", ");
            let src = format!("{} ( {arguments} )", spec.name);
            let errors = expression_errors(&src);
            let arity_errors: Vec<&ValidationError> = errors.iter().filter(|error| error.kind == ValidationErrorKind::Arity).collect();
            if arity < spec.min_args || arity > spec.max_args {
                assert_eq!(arity_errors.len(), 1, "{src:?} should report one arity error: {errors:?}");
            } else if spec.name != "RELATEDTABLE" {
                // RELATEDTABLE needs a table expression, but `[Value]` is a column reference.
                assert!(arity_errors.is_empty(), "{src:?} has a documented overload and must not report an arity error: {errors:?}");
            }
        }
    }
}

#[test]
fn column_arguments_must_be_bracketed_references() {
    for src in [
        "RELATED ( 1 )",
        "RELATED ( \"Category\" )",
        "RELATED ( Sales )",
        "RELATED ( Sales[Amount] + 1 )",
        "CROSSFILTER ( 1, Customer[CustomerKey], BOTH )",
        "CROSSFILTER ( Sales[CustomerKey], 2, BOTH )",
        "CROSSFILTER ( \"Sales\", \"Customer\", BOTH )",
        "USERELATIONSHIP ( 1, 2 )",
        "USERELATIONSHIP ( Sales[Delivery Date], 'Date' )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "RELATED ( Sales[Amount] )",
        "RELATED ( [Amount] )",
        "RELATED ( 'Product'[List Price] )",
        "CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey], BOTH )",
        "USERELATIONSHIP ( Sales[Delivery Date], 'Date'[Date] )",
    ] {
        assert_valid(src);
    }

    let errors = assert_invalid_kind("RELATED ( 1 )", ValidationErrorKind::ArgumentKind);
    let message = &errors[0].message;
    assert!(message.contains("RELATED"), "{message}");
    assert!(message.contains("column reference"), "{message}");
    assert!(message.contains("literal"), "{message}");
}

#[test]
fn relatedtable_requires_a_table_expression() {
    for src in ["RELATEDTABLE ( 1 )", "RELATEDTABLE ( \"Sales\" )", "RELATEDTABLE ( -1 )", "RELATEDTABLE ( Sales[Amount] )"] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }
    for src in [
        "RELATEDTABLE ( Sales )",
        "RELATEDTABLE ( InternetSales )",
        "RELATEDTABLE ( DISTINCT ( Sales[Region] ) )",
        "COUNTROWS ( RELATEDTABLE ( Sales ) )",
    ] {
        assert_valid(src);
    }
}

#[test]
fn crossfilter_accepts_every_documented_direction_argument() {
    for direction in ["NONE", "ONEWAY", "BOTH", "ONEWAY_RIGHTFILTERSLEFT", "ONEWAY_LEFTFILTERSRIGHT", "0", "1", "2", "3", "4"] {
        let src = format!("CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey], {direction} )");
        assert_valid(&src);
    }
}

#[test]
fn filter_modifiers_have_no_static_value_type() {
    // CROSSFILTER and USERELATIONSHIP return no value, so static inference leaves them unknown
    // instead of inventing a scalar or table type.
    for src in [
        "CROSSFILTER ( Sales[CustomerKey], Customer[CustomerKey], BOTH )",
        "USERELATIONSHIP ( Sales[Delivery Date], 'Date'[Date] )",
        "RELATED ( 'Product'[List Price] )",
    ] {
        assert_eq!(infer_type(&parse(src)), Ok(InferredType::Unknown), "{src:?}");
    }
    assert_eq!(infer_type(&parse("RELATEDTABLE ( Sales )")), Ok(InferredType::Table));
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in relationship_catalog() {
        assert!(!spec.examples.is_empty(), "{} has no examples", spec.name);
        for example in spec.examples {
            let expr = parse(example);
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }
}

#[test]
fn nested_calls_report_every_argument_error() {
    let errors = expression_errors("CALCULATE ( [Sales Amount], CROSSFILTER ( 1, 2, BOTH ) )");
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors.iter().all(|error| error.function == "CROSSFILTER"));
    assert!(errors.iter().all(|error| error.kind == ValidationErrorKind::ArgumentKind));

    let src = "CALCULATE ( [Sales Amount], CROSSFILTER ( Sales[CustomerKey], 2, BOTH ) )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(&src[errors[0].span.start..errors[0].span.end], "2");

    assert!(expression_errors("CALCULATE ( [Sales Amount], USERELATIONSHIP ( Sales[Delivery Date], 'Date'[Date] ) )").is_empty());
    assert!(expression_errors("SUMX ( Sales, Sales[Quantity] * RELATED ( 'Product'[List Price] ) )").is_empty());
}

#[test]
fn validate_call_reports_arities_and_kinds() {
    let errors = call_errors("RELATED ( )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].function, "RELATED");
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert!(errors[0].span.is_empty());

    let errors = call_errors("RELATED ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
    assert!(!errors[0].span.is_empty());

    assert!(call_errors("RELATED ( Sales[Amount] )").is_empty());
    assert!(call_errors("USERELATIONSHIP ( Sales[Delivery Date], 'Date'[Date] )").is_empty());
    assert!(call_errors("RELATEDTABLE ( Sales )").is_empty());
}
