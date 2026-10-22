//! Integration tests for the Math and Trig catalog.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn math_trig_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::MathAndTrig).collect()
}

const FUNCTIONS: [(&str, usize, usize); 49] = [
    ("ABS", 1, 1),
    ("ACOS", 1, 1),
    ("ACOSH", 1, 1),
    ("ACOT", 1, 1),
    ("ACOTH", 1, 1),
    ("ASIN", 1, 1),
    ("ASINH", 1, 1),
    ("ATAN", 1, 1),
    ("ATANH", 1, 1),
    ("CEILING", 2, 2),
    ("CONVERT", 2, 2),
    ("COS", 1, 1),
    ("COSH", 1, 1),
    ("COT", 1, 1),
    ("COTH", 1, 1),
    ("CURRENCY", 1, 1),
    ("DEGREES", 1, 1),
    ("DIVIDE", 2, 3),
    ("EVEN", 1, 1),
    ("EXP", 1, 1),
    ("FACT", 1, 1),
    ("FLOOR", 2, 2),
    ("GCD", 2, 2),
    ("INT", 1, 1),
    ("ISO.CEILING", 1, 2),
    ("LCM", 2, 2),
    ("LN", 1, 1),
    ("LOG", 1, 2),
    ("LOG10", 1, 1),
    ("MOD", 2, 2),
    ("MROUND", 2, 2),
    ("ODD", 1, 1),
    ("PI", 0, 0),
    ("POWER", 2, 2),
    ("QUOTIENT", 2, 2),
    ("RADIANS", 1, 1),
    ("RAND", 0, 0),
    ("RANDBETWEEN", 2, 2),
    ("ROUND", 2, 2),
    ("ROUNDDOWN", 2, 2),
    ("ROUNDUP", 2, 2),
    ("SIGN", 1, 1),
    ("SIN", 1, 1),
    ("SINH", 1, 1),
    ("SQRT", 1, 1),
    ("SQRTPI", 1, 1),
    ("TAN", 1, 1),
    ("TANH", 1, 1),
    ("TRUNC", 1, 2),
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
fn catalog_has_every_math_trig_function_exactly_once_with_documented_arity() {
    let specs = math_trig_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::MathAndTrig);
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
    assert_eq!(lookup("iso.ceiling").map(|spec| spec.name), Some("ISO.CEILING"));
    assert_eq!(lookup("RaNd").map(|spec| spec.name), Some("RAND"));
    assert!(lookup("ABSX").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("ABS ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in math_trig_catalog() {
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
        assert!(!spec.is_iterator(), "{} is not an iterator", spec.name);
    }
}

#[test]
fn flags_mark_volatile_functions_and_none_are_recommended_against() {
    for spec in math_trig_catalog() {
        let should_be_volatile = matches!(spec.name, "RAND" | "RANDBETWEEN");
        assert_eq!(spec.flags.volatile, should_be_volatile, "{} volatile flag", spec.name);
        assert!(!spec.flags.iterator, "{} iterator flag", spec.name);
        assert!(!spec.flags.not_recommended, "{} not-recommended flag", spec.name);
        assert!(spec.flags.excel_compatible, "{} Excel compatibility", spec.name);
        assert!(spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
    }
}

#[test]
fn return_types_match_the_documented_results() {
    for (name, return_type) in [
        ("ABS", ReturnType::Numeric),
        ("ACOS", ReturnType::Decimal),
        ("ACOSH", ReturnType::Decimal),
        ("ACOT", ReturnType::Decimal),
        ("ACOTH", ReturnType::Decimal),
        ("ASIN", ReturnType::Decimal),
        ("ASINH", ReturnType::Decimal),
        ("ATAN", ReturnType::Decimal),
        ("ATANH", ReturnType::Decimal),
        ("CEILING", ReturnType::Numeric),
        ("CONVERT", ReturnType::Any),
        ("COS", ReturnType::Decimal),
        ("COSH", ReturnType::Decimal),
        ("COT", ReturnType::Decimal),
        ("COTH", ReturnType::Decimal),
        ("CURRENCY", ReturnType::Currency),
        ("DEGREES", ReturnType::Decimal),
        ("DIVIDE", ReturnType::Decimal),
        ("EVEN", ReturnType::Integer),
        ("EXP", ReturnType::Decimal),
        ("FACT", ReturnType::Decimal),
        ("FLOOR", ReturnType::Decimal),
        ("GCD", ReturnType::Integer),
        ("INT", ReturnType::Any),
        ("ISO.CEILING", ReturnType::Numeric),
        ("LCM", ReturnType::Integer),
        ("LN", ReturnType::Decimal),
        ("LOG", ReturnType::Decimal),
        ("LOG10", ReturnType::Decimal),
        ("MOD", ReturnType::Any),
        ("MROUND", ReturnType::Decimal),
        ("ODD", ReturnType::Integer),
        ("PI", ReturnType::Decimal),
        ("POWER", ReturnType::Decimal),
        ("QUOTIENT", ReturnType::Integer),
        ("RADIANS", ReturnType::Decimal),
        ("RAND", ReturnType::Decimal),
        ("RANDBETWEEN", ReturnType::Integer),
        ("ROUND", ReturnType::Decimal),
        ("ROUNDDOWN", ReturnType::Decimal),
        ("ROUNDUP", ReturnType::Decimal),
        ("SIGN", ReturnType::Integer),
        ("SIN", ReturnType::Decimal),
        ("SINH", ReturnType::Decimal),
        ("SQRT", ReturnType::Decimal),
        ("SQRTPI", ReturnType::Decimal),
        ("TAN", ReturnType::Decimal),
        ("TANH", ReturnType::Decimal),
        ("TRUNC", ReturnType::Decimal),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }
}

#[test]
fn overloads_model_the_documented_signatures() {
    assert_eq!(overload_kinds(spec("PI"), 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec("RAND"), 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec("ABS"), 1), Some(vec![ParamKind::Number]));
    assert_eq!(overload_kinds(spec("CEILING"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("DIVIDE"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("DIVIDE"), 3), Some(vec![ParamKind::Number, ParamKind::Number, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("ISO.CEILING"), 1), Some(vec![ParamKind::Number]));
    assert_eq!(overload_kinds(spec("ISO.CEILING"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("LOG"), 1), Some(vec![ParamKind::Number]));
    assert_eq!(overload_kinds(spec("LOG"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("TRUNC"), 1), Some(vec![ParamKind::Number]));
    assert_eq!(overload_kinds(spec("TRUNC"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("CONVERT"), 2), Some(vec![ParamKind::Any, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("CURRENCY"), 1), Some(vec![ParamKind::Scalar]));

    let divide = spec("DIVIDE");
    assert_eq!(divide.params[2].name, "AlternateResult");
    assert_eq!(divide.params[2].kind, ParamKind::Any);
    assert!(divide.params[2].optional);

    let iso_ceiling = spec("ISO.CEILING");
    assert_eq!(iso_ceiling.params[1].name, "Significance");
    assert!(iso_ceiling.params[1].optional);

    let trunc = spec("TRUNC");
    assert_eq!(trunc.params[1].name, "NumberOfDigits");
    assert!(trunc.params[1].optional);
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "ABS ( -1 )",
        "ACOS ( 0.5 )",
        "CEILING ( 10.2, 1 )",
        "CONVERT ( \"123\", INTEGER )",
        "CURRENCY ( 10.5 )",
        "DIVIDE ( 10, 2 )",
        "DIVIDE ( 10, 0, 0 )",
        "ISO.CEILING ( 10.2 )",
        "ISO.CEILING ( 10.7, 0.5 )",
        "LOG ( 100 )",
        "LOG ( 8, 2 )",
        "PI ()",
        "RAND ()",
        "RANDBETWEEN ( 1, 10 )",
        "SIGN ( -5 )",
        "TRUNC ( 3.14159 )",
        "TRUNC ( 3.14159, 2 )",
    ] {
        assert_valid(src);
    }

    for src in [
        "ABS ( )",
        "ABS ( 1, 2 )",
        "CEILING ( 1 )",
        "CEILING ( 1, 2, 3 )",
        "CONVERT ( 1 )",
        "DIVIDE ( 1 )",
        "DIVIDE ( 1, 2, 3, 4 )",
        "ISO.CEILING ( 1, 2, 3 )",
        "LOG ( )",
        "LOG ( 1, 2, 3 )",
        "PI ( 1 )",
        "RAND ( 1 )",
        "RANDBETWEEN ( 1 )",
        "TRUNC ( )",
        "TRUNC ( 1, 2, 3 )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_arity_boundary_is_enforced() {
    for spec in math_trig_catalog() {
        for arity in 0..=4usize {
            let arguments = vec!["1"; arity].join(", ");
            let src = format!("{} ( {arguments} )", spec.name);
            let errors = expression_errors(&src);
            let arity_errors: Vec<&ValidationError> = errors.iter().filter(|error| error.kind == ValidationErrorKind::Arity).collect();
            if arity < spec.min_args || arity > spec.max_args {
                assert_eq!(arity_errors.len(), 1, "{src:?} should report one arity error: {errors:?}");
            } else {
                assert!(arity_errors.is_empty(), "{src:?} has a documented overload and must not report an arity error: {errors:?}");
            }
        }
    }
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in math_trig_catalog() {
        assert!(!spec.examples.is_empty(), "{} has no examples", spec.name);
        for example in spec.examples {
            let expr = parse(example);
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }
}

#[test]
fn validate_call_reports_arities_and_kinds() {
    let errors = call_errors("PI ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert!(errors[0].span.is_empty());

    assert!(call_errors("ABS ( 1 )").is_empty());
    assert!(call_errors("PI ( )").is_empty());
    assert!(call_errors("CONVERT ( 1, INTEGER )").is_empty());
}

#[test]
fn nested_calls_are_checked_inside_math_expressions() {
    assert_valid("ROUND ( SQRT ( 16 ), 0 ) + PI () * POWER ( 2, 3 )");
    assert_valid("DIVIDE ( ABS ( -10 ), CEILING ( 3.1, 1 ) )");

    let errors = expression_errors("ROUND ( SQRT ( 16, 2 ), 0 )");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].function, "SQRT");
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
}
