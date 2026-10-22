//! Integration tests for the logical catalog and its static validation.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn logical_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::Logical).collect()
}

/// Every function on <https://dax.guide/functions/logical/>, in index order. `usize::MAX`
/// marks a variadic maximum arity.
const FUNCTIONS: [(&str, usize, usize); 13] = [
    ("AND", 2, 2),
    ("BITAND", 2, 2),
    ("BITOR", 2, 2),
    ("BITXOR", 2, 2),
    ("COALESCE", 2, usize::MAX),
    ("FALSE", 0, 0),
    ("IF", 2, 3),
    ("IF.EAGER", 2, 3),
    ("IFERROR", 2, 2),
    ("NOT", 1, 1),
    ("OR", 2, 2),
    ("SWITCH", 3, usize::MAX),
    ("TRUE", 0, 0),
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
fn catalog_has_every_logical_function_exactly_once_in_index_order() {
    let specs = logical_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());
    assert_eq!(specs.len(), 13);

    for (spec, (name, min_args, max_args)) in specs.iter().zip(FUNCTIONS) {
        assert_eq!(spec.name, name, "dax.guide index order");
        assert_eq!(spec.min_args, min_args, "{name} min arity");
        assert_eq!(spec.max_args, max_args, "{name} max arity");
        assert_eq!(spec.category, FunctionCategory::Logical, "{name} category");
    }

    let catalogued: std::collections::BTreeSet<&str> = specs.iter().map(|spec| spec.name).collect();
    let documented: std::collections::BTreeSet<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(catalogued, documented);
    assert_eq!(catalogued.len(), specs.len(), "every name appears exactly once");
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        assert_eq!(lookup(name).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_lowercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_uppercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(name).map(|spec| spec.category), Some(FunctionCategory::Logical));
    }
    assert_eq!(lookup("if.Eager").map(|spec| spec.name), Some("IF.EAGER"));
    assert_eq!(lookup("and").map(|spec| spec.name), Some("AND"));
    assert!(lookup("IF.EAGER ").is_none());
    assert!(lookup("IFEAGER").is_none());
    assert!(lookup("").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in logical_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);
        assert_eq!(spec.sugar_of, None, "{} is not syntax sugar", spec.name);
        assert!(!spec.flags.iterator, "{} is not an iterator", spec.name);
        assert_eq!(spec.is_iterator(), spec.flags.iterator, "{} is_iterator", spec.name);
        assert!(!spec.flags.volatile, "{} is not volatile", spec.name);

        let required = spec.params.iter().filter(|param| !param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);
        assert!(spec.params.len() >= spec.min_args, "{} descriptive params", spec.name);

        let variadic = spec.overloads.iter().any(|overload| overload.variadic);
        assert_eq!(variadic, spec.max_args == usize::MAX, "{} variadic arity", spec.name);
        if spec.max_args != usize::MAX {
            assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);
            let optional = spec.params.iter().filter(|param| param.optional).count();
            assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);
            for arity in spec.min_args..=spec.max_args {
                assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
            }
            let arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
            assert_eq!(arities.iter().min().copied(), Some(spec.min_args), "{} overload min", spec.name);
            assert_eq!(arities.iter().max().copied(), Some(spec.max_args), "{} overload max", spec.name);
        } else {
            let overload = spec.overload_for(spec.min_args).expect("variadic minimum arity");
            assert!(overload.variadic, "{} minimum arity is variadic", spec.name);
            assert!(spec.overload_for(spec.min_args + 7).is_some(), "{} accepts long calls", spec.name);
        }
    }
}

#[test]
fn documented_parameter_kinds_and_overloads() {
    assert_eq!(overload_kinds(spec("AND"), 2), Some(vec![ParamKind::Logical, ParamKind::Logical]));
    assert_eq!(overload_kinds(spec("OR"), 2), Some(vec![ParamKind::Logical, ParamKind::Logical]));
    assert_eq!(overload_kinds(spec("NOT"), 1), Some(vec![ParamKind::Logical]));
    assert_eq!(overload_kinds(spec("BITAND"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("BITOR"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("BITXOR"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));

    assert_eq!(overload_kinds(spec("IF"), 2), Some(vec![ParamKind::Logical, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("IF"), 3), Some(vec![ParamKind::Logical, ParamKind::Any, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("IF.EAGER"), 2), Some(vec![ParamKind::Logical, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("IF.EAGER"), 3), Some(vec![ParamKind::Logical, ParamKind::Any, ParamKind::Any]));
    assert_eq!(spec("IF").params[2].name, "ResultIfFalse");
    assert!(spec("IF").params[2].optional);
    assert!(spec("IF.EAGER").params[2].optional);

    assert_eq!(overload_kinds(spec("IFERROR"), 2), Some(vec![ParamKind::Any, ParamKind::Any]));
    assert_eq!(spec("IFERROR").params[0].name, "Value");
    assert_eq!(spec("IFERROR").params[1].name, "ValueIfError");

    assert_eq!(overload_kinds(spec("COALESCE"), 2), Some(vec![ParamKind::Any, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("COALESCE"), 5), Some(vec![ParamKind::Any, ParamKind::Any]));
    assert_eq!(spec("COALESCE").overload_for(5).map(|overload| overload.kind_at(4)), Some(Some(ParamKind::Any)));
    let coalesce = spec("COALESCE");
    assert!(coalesce.overload_for(2).is_some_and(|overload| overload.variadic));
    assert_eq!(coalesce.params.iter().map(|param| param.name).collect::<Vec<_>>(), ["Value1", "Value2"]);

    assert_eq!(overload_kinds(spec("SWITCH"), 3), Some(vec![ParamKind::Any, ParamKind::Any, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("SWITCH"), 6), Some(vec![ParamKind::Any, ParamKind::Any, ParamKind::Any]));
    assert_eq!(spec("SWITCH").overload_for(6).map(|overload| overload.kind_at(5)), Some(Some(ParamKind::Any)));
    let switch = spec("SWITCH");
    assert!(switch.overload_for(3).is_some_and(|overload| overload.variadic));
    assert_eq!(switch.params.len(), 4);
    assert_eq!(switch.params[3].name, "Else");
    assert!(switch.params[3].optional);

    assert_eq!(overload_kinds(spec("TRUE"), 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec("FALSE"), 0), Some(Vec::new()));
    assert!(spec("TRUE").params.is_empty());
    assert!(spec("FALSE").params.is_empty());
}

#[test]
fn return_types_and_flags_are_documented() {
    for (name, return_type) in [
        ("AND", ReturnType::Boolean),
        ("BITAND", ReturnType::Integer),
        ("BITOR", ReturnType::Integer),
        ("BITXOR", ReturnType::Integer),
        ("COALESCE", ReturnType::Any),
        ("FALSE", ReturnType::Boolean),
        ("IF", ReturnType::Any),
        ("IF.EAGER", ReturnType::Any),
        ("IFERROR", ReturnType::Any),
        ("NOT", ReturnType::Boolean),
        ("OR", ReturnType::Boolean),
        ("SWITCH", ReturnType::Any),
        ("TRUE", ReturnType::Boolean),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }

    for spec in logical_catalog() {
        assert!(spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
        let engine_only = ["BITAND", "BITOR", "BITXOR", "COALESCE", "IF.EAGER"].contains(&spec.name);
        assert_eq!(spec.flags.excel_compatible, !engine_only, "{} Excel compatibility", spec.name);
        let discouraged = ["IF.EAGER", "IFERROR"].contains(&spec.name);
        assert_eq!(spec.flags.not_recommended, discouraged, "{} not-recommended flag", spec.name);
    }

    assert!(spec("IFERROR").flags.not_recommended);
    assert!(spec("IF.EAGER").flags.not_recommended);
    assert!(!spec("IF").flags.not_recommended, "IF itself is the recommended conditional");
}

#[test]
fn every_example_parses_and_validates() {
    for spec in logical_catalog() {
        for (index, example) in spec.examples.iter().enumerate() {
            let expr = parse_expression(example).unwrap_or_else(|error| panic!("{} example {index} {example:?} did not parse: {error}", spec.name));
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }

    // The examples named by the catalog contract are present verbatim.
    assert!(spec("IF").examples.contains(&"IF ( Sales[Amount] > 0, 1, 0 )"));
    assert!(spec("SWITCH").examples.contains(&"SWITCH ( [X], 1, \"a\", 2, \"b\", \"other\" )"));
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "AND ( TRUE, FALSE )",
        "BITAND ( 5, 3 )",
        "BITOR ( 5, 3 )",
        "BITXOR ( 5, 3 )",
        "COALESCE ( 1, 2 )",
        "COALESCE ( 1, 2, 3, 4, 5 )",
        "FALSE ()",
        "IF ( TRUE, 1 )",
        "IF ( TRUE, 1, 0 )",
        "IF.EAGER ( TRUE, 1 )",
        "IF.EAGER ( TRUE, 1, 0 )",
        "IFERROR ( 1 / 0, 0 )",
        "NOT ( TRUE )",
        "OR ( TRUE, FALSE )",
        "SWITCH ( 1, 1, \"a\" )",
        "SWITCH ( 1, 1, \"a\", 2, \"b\" )",
        "SWITCH ( 1, 1, \"a\", 2, \"b\", \"other\" )",
        "SWITCH ( TRUE (), Sales[Amount] > 1000, \"Large\", \"Small\" )",
        "TRUE ()",
    ] {
        assert_valid(src);
    }

    for src in [
        "AND ( )",
        "AND ( TRUE )",
        "AND ( TRUE, FALSE, TRUE )",
        "BITAND ( 5 )",
        "BITAND ( 5, 3, 1 )",
        "BITOR ( 5 )",
        "BITOR ( 5, 3, 1 )",
        "BITXOR ( 5 )",
        "BITXOR ( 5, 3, 1 )",
        "COALESCE ( )",
        "COALESCE ( 1 )",
        "FALSE ( TRUE )",
        "IF ( TRUE )",
        "IF ( TRUE, 1, 0, 2 )",
        "IF.EAGER ( TRUE )",
        "IF.EAGER ( TRUE, 1, 0, 2 )",
        "IFERROR ( 1 )",
        "IFERROR ( 1, 2, 3 )",
        "OR ( )",
        "OR ( TRUE )",
        "OR ( TRUE, FALSE, TRUE )",
        "SWITCH ( )",
        "SWITCH ( 1 )",
        "SWITCH ( 1, 1 )",
        "TRUE ( TRUE )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_finite_arity_boundary_is_enforced() {
    for spec in logical_catalog().iter().filter(|spec| spec.max_args != usize::MAX && spec.name != "NOT") {
        for arity in 0..=spec.max_args + 1 {
            let arguments = vec!["1"; arity].join(", ");
            let src = format!("{} ( {arguments} )", spec.name);
            let errors = expression_errors(&src);
            let arity_errors: Vec<&ValidationError> = errors.iter().filter(|error| error.kind == ValidationErrorKind::Arity).collect();
            if arity < spec.min_args || arity > spec.max_args {
                assert_eq!(arity_errors.len(), 1, "{src:?} should report one arity error: {errors:?}");
            } else {
                assert!(arity_errors.is_empty(), "{src:?} has a documented overload: {errors:?}");
            }
        }
    }

    for name in ["COALESCE", "SWITCH"] {
        let spec = spec(name);
        for arity in 0..spec.min_args {
            let arguments = vec!["1"; arity].join(", ");
            let src = format!("{} ( {arguments} )", spec.name);
            assert_invalid_kind(&src, ValidationErrorKind::Arity);
        }
        for arity in [spec.min_args, spec.min_args + 1, spec.min_args + 4, 17] {
            let arguments = vec!["1"; arity].join(", ");
            let src = format!("{} ( {arguments} )", spec.name);
            let errors = expression_errors(&src);
            assert!(errors.iter().all(|error| error.kind != ValidationErrorKind::Arity), "{src:?} is inside the variadic range: {errors:?}");
        }
    }
}

#[test]
fn not_is_an_operator_and_has_no_zero_argument_syntax() {
    // The lexer classifies NOT as an operator, so `NOT ( )` is invalid syntax and longer
    // argument lists parse as a tuple operand rather than a call.
    assert!(parse_expression("NOT ( )").is_err());
    assert_valid("NOT ( TRUE )");
    let tuple_operand = parse("NOT ( TRUE, FALSE )");
    assert!(matches!(&tuple_operand.kind, ExprKind::Unary { .. }), "{:?}", tuple_operand.kind);
    assert!(expression_errors("NOT ( TRUE, FALSE )").is_empty());

    // The catalog record still documents the function form and rejects other arities when a
    // call node is built through the API.
    assert_eq!((spec("NOT").min_args, spec("NOT").max_args), (1, 1));
    let call = gridcraft_dax::ast::Call { name: gridcraft_dax::ast::Name::bare("NOT"), args: vec![parse("TRUE"), parse("FALSE")] };
    let errors = validate_call(&call);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
}

#[test]
fn validate_call_reports_arity_without_expression_spans() {
    let errors = call_errors("IF ( TRUE )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert_eq!(errors[0].function, "IF");
    assert_eq!(errors[0].span.start, errors[0].span.end);

    assert!(call_errors("IFERROR ( 1 / 0, 0 )").is_empty());
    assert!(call_errors("SWITCH ( [X], 1, \"a\", \"other\" )").is_empty());
}

#[test]
fn boolean_combinators_reject_known_non_logical_arity_but_accept_expressions() {
    // Logical arguments are not statically decidable, so any expression is accepted; only the
    // count is enforced. These calls all have the documented arity.
    for src in [
        "AND ( Sales[Flag], 1 = 1 )",
        "OR ( [A] > 0, [B] < 0 )",
        "NOT ( [A] = \"x\" )",
        "IF ( [A] > 0, [B] )",
        "SWITCH ( [A], \"x\", 1, \"y\", 2 )",
        "COALESCE ( [A], [B], \"fallback\" )",
    ] {
        assert_valid(src);
    }
}
