//! Integration tests for the visual calculation catalog and its static validation.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn visual_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::VisualCalculations).collect()
}

const FUNCTIONS: [(&str, usize, usize); 11] = [
    ("COLLAPSE", 1, usize::MAX),
    ("COLLAPSEALL", 1, 2),
    ("EXPAND", 1, usize::MAX),
    ("EXPANDALL", 1, 2),
    ("FIRST", 1, 5),
    ("LAST", 1, 5),
    ("MOVINGAVERAGE", 2, 7),
    ("NEXT", 1, 6),
    ("PREVIOUS", 1, 6),
    ("RANGE", 1, 6),
    ("RUNNINGSUM", 1, 5),
];

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
fn catalog_has_every_visual_calculation_function_exactly_once_in_index_order() {
    let specs = visual_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for ((name, min_args, max_args), spec) in FUNCTIONS.iter().zip(specs.iter()) {
        assert_eq!(spec.name, *name, "catalog order follows the dax.guide index");
        assert_eq!(spec.min_args, *min_args, "{name} min arity");
        assert_eq!(spec.max_args, *max_args, "{name} max arity");
        assert_eq!(spec.category, FunctionCategory::VisualCalculations);
    }

    let catalogued: std::collections::BTreeSet<&str> = specs.iter().map(|spec| spec.name).collect();
    let documented: std::collections::BTreeSet<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(catalogued, documented);
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        assert_eq!(lookup(name).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_lowercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_uppercase()).map(|spec| spec.name), Some(name));
    }
    assert_eq!(lookup("rUnNiNgSuM").map(|spec| spec.name), Some("RUNNINGSUM"));
    assert!(lookup("COLLAPSE_ALL").is_none());
    assert!(lookup("RUNNINGSUM ").is_none());
    assert!(lookup("").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in visual_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);

        let required = spec.params.iter().filter(|param| !param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);

        if spec.max_args == usize::MAX {
            // COLLAPSE and EXPAND accept a repeatable list of columns, so the descriptive
            // signature records one column slot and the overload is marked variadic.
            assert!(matches!(spec.name, "COLLAPSE" | "EXPAND"), "{} unbounded arity", spec.name);
            assert!(spec.overloads.iter().any(|overload| overload.variadic), "{} variadic overload", spec.name);
            assert!(spec.params.iter().any(|param| param.name == "Column" && param.optional), "{} repeatable column", spec.name);
            for arity in spec.min_args..=12 {
                assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
            }
            continue;
        }

        assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);
        let optional = spec.params.iter().filter(|param| param.optional).count();
        assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);

        for arity in spec.min_args..=spec.max_args {
            assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
        }
        let arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
        assert_eq!(arities.iter().min().copied(), Some(spec.min_args), "{} overload min", spec.name);
        assert_eq!(arities.iter().max().copied(), Some(spec.max_args), "{} overload max", spec.name);
        assert!(spec.overloads.iter().all(|overload| !overload.variadic), "{} should use fixed overloads", spec.name);
    }
}

#[test]
fn return_types_and_flags_are_honest() {
    for (name, return_type) in [
        ("COLLAPSE", ReturnType::Any),
        ("COLLAPSEALL", ReturnType::Any),
        ("EXPAND", ReturnType::Any),
        ("EXPANDALL", ReturnType::Any),
        ("FIRST", ReturnType::Any),
        ("LAST", ReturnType::Any),
        ("MOVINGAVERAGE", ReturnType::Numeric),
        ("NEXT", ReturnType::Any),
        ("PREVIOUS", ReturnType::Any),
        ("RANGE", ReturnType::Table),
        ("RUNNINGSUM", ReturnType::Numeric),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }

    for spec in visual_catalog() {
        // Visual calculations exist only in Power BI; no other host or optimisation flag applies.
        assert!(!spec.flags.excel_compatible, "{} is not Excel compatible", spec.name);
        assert!(!spec.flags.iterator, "{} is not an iterator", spec.name);
        assert!(!spec.flags.volatile, "{} is not volatile", spec.name);
        assert!(!spec.flags.not_recommended, "{} should not be marked not recommended", spec.name);
        assert!(!spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
        assert!(!spec.is_iterator(), "{} is_iterator", spec.name);
        assert_eq!(spec.sugar_of, None, "{} has no syntax sugar", spec.name);
    }
}

#[test]
fn overloads_model_the_documented_signatures() {
    assert_eq!(overload_kinds(spec("COLLAPSE"), 1), Some(vec![ParamKind::Any]));
    assert_eq!(overload_kinds(spec("COLLAPSE"), 2), Some(vec![ParamKind::Any, ParamKind::Any]));
    assert!(spec("COLLAPSE").overload_for(5).is_some_and(|overload| overload.variadic));

    assert_eq!(overload_kinds(spec("COLLAPSEALL"), 1), Some(vec![ParamKind::Any]));
    assert_eq!(overload_kinds(spec("COLLAPSEALL"), 2), Some(vec![ParamKind::Any, ParamKind::Any]));

    assert_eq!(overload_kinds(spec("EXPANDALL"), 1), Some(vec![ParamKind::Any]));
    assert_eq!(overload_kinds(spec("EXPANDALL"), 2), Some(vec![ParamKind::Any, ParamKind::Any]));
    assert!(spec("EXPAND").overload_for(6).is_some_and(|overload| overload.variadic));

    assert_eq!(overload_kinds(spec("FIRST"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("LAST"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("FIRST"), 5), Some(vec![ParamKind::Column, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]));

    assert_eq!(overload_kinds(spec("MOVINGAVERAGE"), 2), Some(vec![ParamKind::Column, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("MOVINGAVERAGE"), 3), Some(vec![ParamKind::Column, ParamKind::Number, ParamKind::Boolean]));
    assert_eq!(overload_kinds(spec("MOVINGAVERAGE"), 7).map(|kinds| kinds.len()), Some(7));

    assert_eq!(overload_kinds(spec("NEXT"), 2), Some(vec![ParamKind::Column, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("PREVIOUS"), 2), Some(vec![ParamKind::Column, ParamKind::Number]));

    assert_eq!(overload_kinds(spec("RANGE"), 1), Some(vec![ParamKind::Number]));
    assert_eq!(overload_kinds(spec("RANGE"), 2), Some(vec![ParamKind::Number, ParamKind::Boolean]));

    assert_eq!(overload_kinds(spec("RUNNINGSUM"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("RUNNINGSUM"), 5).map(|kinds| kinds.len()), Some(5));
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        // COLLAPSE / EXPAND: navigation-only and expression forms.
        "COLLAPSE ( ROWS )",
        "COLLAPSE ( ROWS, 2 )",
        "COLLAPSE ( [Sales Amount], ROWS )",
        "COLLAPSE ( [Sales Amount], ROWS, 2 )",
        "COLLAPSE ( [Sales Amount], [Month] )",
        "EXPAND ( ROWS )",
        "EXPAND ( [Sales Amount], ROWS, 2 )",
        "EXPANDALL ( [Sales Amount], COLUMNS )",
        "COLLAPSEALL ( ROWS )",
        "COLLAPSEALL ( [Sales Amount], ROWS )",
        // Row retrieval functions with every optional argument.
        "FIRST ( [Sales Amount] )",
        "FIRST ( [Sales Amount], ROWS, [Month], DEFAULT, NONE )",
        "LAST ( [Sales Amount], ROWS, [Month], FIRST, LOWESTPARENT )",
        "NEXT ( [Sales Amount] )",
        "NEXT ( [Sales Amount], 2, ROWS, [Month], LAST, HIGHESTPARENT )",
        "PREVIOUS ( [Sales Amount] )",
        "PREVIOUS ( [Sales Amount], 1, ROWS, [Month], DEFAULT, -1 )",
        // Aggregate-style visual calculations.
        "MOVINGAVERAGE ( [Sales Amount], 6 )",
        "MOVINGAVERAGE ( [Sales Amount], 6, TRUE, ROWS, [Month], DEFAULT, NONE )",
        "RANGE ( 6 )",
        "RANGE ( -5, TRUE, ROWS, [Month], DEFAULT, NONE )",
        "RUNNINGSUM ( [Sales Amount] )",
        "RUNNINGSUM ( [Sales Amount], ROWS, [Month], DEFAULT, NONE )",
    ] {
        assert_valid(src);
    }

    for src in [
        "COLLAPSE ( )",
        "COLLAPSEALL ( )",
        "EXPAND ( )",
        "EXPANDALL ( )",
        "FIRST ( )",
        "LAST ( )",
        "MOVINGAVERAGE ( [Sales Amount] )",
        "NEXT ( )",
        "PREVIOUS ( )",
        "RANGE ( )",
        "RUNNINGSUM ( )",
        "COLLAPSEALL ( [Sales Amount], ROWS, 1 )",
        "EXPANDALL ( [Sales Amount], ROWS, 1 )",
        "FIRST ( [Sales Amount], ROWS, [Month], DEFAULT, NONE, 1 )",
        "LAST ( [Sales Amount], ROWS, [Month], DEFAULT, NONE, 1 )",
        "MOVINGAVERAGE ( [Sales Amount], 6, TRUE, ROWS, [Month], DEFAULT, NONE, 1 )",
        "NEXT ( [Sales Amount], 1, ROWS, [Month], DEFAULT, NONE, 1 )",
        "PREVIOUS ( [Sales Amount], 1, ROWS, [Month], DEFAULT, NONE, 1 )",
        "RANGE ( 6, TRUE, ROWS, [Month], DEFAULT, NONE, 1 )",
        "RUNNINGSUM ( [Sales Amount], ROWS, [Month], DEFAULT, NONE, 1 )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_arity_boundary_is_enforced() {
    for spec in visual_catalog() {
        if spec.max_args == usize::MAX {
            // A zero-argument call is the only rejected arity for a variadic signature.
            let errors = assert_invalid_kind(&format!("{} ( )", spec.name), ValidationErrorKind::Arity);
            assert_eq!(errors.len(), 1, "{} () should report one arity error", spec.name);
            for arity in spec.min_args..=20usize {
                let arguments = vec!["1"; arity].join(", ");
                let src = format!("{} ( {arguments} )", spec.name);
                let arity_errors: Vec<ValidationError> =
                    expression_errors(&src).into_iter().filter(|error| error.kind == ValidationErrorKind::Arity).collect();
                assert!(arity_errors.is_empty(), "{src:?} must not report an arity error: {arity_errors:?}");
            }
            continue;
        }

        for arity in 0..=spec.max_args + 1 {
            let arguments = vec!["1"; arity].join(", ");
            let src = format!("{} ( {arguments} )", spec.name);
            let arity_errors: Vec<ValidationError> =
                expression_errors(&src).into_iter().filter(|error| error.kind == ValidationErrorKind::Arity).collect();
            if arity < spec.min_args || arity > spec.max_args {
                assert_eq!(arity_errors.len(), 1, "{src:?} should report one arity error: {arity_errors:?}");
            } else {
                assert!(arity_errors.is_empty(), "{src:?} has a documented overload and must not report an arity error: {arity_errors:?}");
            }
        }
    }
}

#[test]
fn column_parameters_must_be_bracketed_references() {
    for src in ["FIRST ( 1 )", "LAST ( 1 )", "NEXT ( 1 )", "PREVIOUS ( 1 )", "RUNNINGSUM ( 1 )", "MOVINGAVERAGE ( 1, 6 )"] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "FIRST ( [Sales Amount] )",
        "LAST ( Sales[Amount] )",
        "NEXT ( 'Sales'[Amount] )",
        "PREVIOUS ( [Amount] )",
        "RUNNINGSUM ( Sales[Amount] )",
        "MOVINGAVERAGE ( [Sales Amount], 6 )",
    ] {
        assert_valid(src);
    }

    // RANGE returns a table and takes only numbers, flags and names, so its first argument is
    // not statically checked as a column.
    assert_valid("RANGE ( 1 )");
    assert_valid("RANGE ( -1 )");

    let errors = assert_invalid_kind("FIRST ( 1 )", ValidationErrorKind::ArgumentKind);
    let message = &errors[0].message;
    assert!(message.contains("FIRST"), "{message}");
    assert!(message.contains("column reference"), "{message}");
    assert!(message.contains("literal"), "{message}");
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in visual_catalog() {
        assert!(!spec.examples.is_empty(), "{} has no examples", spec.name);
        for example in spec.examples {
            let expr = parse(example);
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }
}

#[test]
fn unknown_functions_are_ignored_but_nested_calls_are_checked() {
    assert!(expression_errors("FOO ( 1, 2, 3 )").is_empty());
    assert!(call_errors("FOO ( 1, 2, 3 )").is_empty());

    assert!(expression_errors("CALCULATE ( SUM ( [Sales Amount] ), ROWS )").is_empty());
    assert!(expression_errors("CALCULATE ( SUM ( [Sales Amount] ), RANGE ( -5, TRUE, ROWS ) )").is_empty());

    let errors = expression_errors("COLLAPSEALL ( RUNNINGSUM ( 1 ), ROWS )");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].function, "RUNNINGSUM");
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
}

#[test]
fn validate_call_reports_arities_and_kinds() {
    let errors = call_errors("FIRST ( )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert!(errors[0].span.is_empty());

    let errors = call_errors("RUNNINGSUM ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
    assert!(!errors[0].span.is_empty());

    assert!(call_errors("RUNNINGSUM ( [Sales Amount] )").is_empty());
    assert!(call_errors("COLLAPSE ( ROWS )").is_empty());
    assert!(call_errors("RANGE ( 6 )").is_empty());
}
