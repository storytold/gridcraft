//! Integration tests for the parent-child catalog and its static validation.

use gridcraft_dax::functions::{BlankPolicy, FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::types::{DataType, InferredType, infer_type};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn parent_child_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::ParentChild).collect()
}

const FUNCTIONS: [(&str, usize, usize); 5] =
    [("PATH", 2, 2), ("PATHCONTAINS", 2, 2), ("PATHITEM", 2, 3), ("PATHITEMREVERSE", 2, 3), ("PATHLENGTH", 1, 1)];

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
fn catalog_has_every_parent_child_function_exactly_once_in_index_order() {
    let specs = parent_child_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());
    assert_eq!(specs.len(), 5);

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::ParentChild, "{name} category");
    }

    let catalogued: std::collections::BTreeSet<&str> = specs.iter().map(|spec| spec.name).collect();
    let documented: std::collections::BTreeSet<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(catalogued, documented);

    let ordered: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
    let expected: Vec<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(ordered, expected, "dax.guide index order");
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        assert_eq!(lookup(name).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_lowercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_uppercase()).map(|spec| spec.name), Some(name));
    }
    assert_eq!(lookup("PathLength").map(|spec| spec.name), Some("PATHLENGTH"));
    assert!(lookup("PATHLEN").is_none());
    assert!(lookup("PATHITEMS").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("PATH ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in parent_child_catalog() {
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
        assert_eq!(spec.blank_policy, BlankPolicy::Unspecified, "{} blank policy", spec.name);
        assert!(spec.params.iter().all(|param| !param.name.is_empty()), "{} parameter names", spec.name);
    }
}

#[test]
fn return_type_metadata() {
    for (name, return_type) in [
        ("PATH", ReturnType::String),
        ("PATHCONTAINS", ReturnType::Boolean),
        ("PATHITEM", ReturnType::Any),
        ("PATHITEMREVERSE", ReturnType::Any),
        ("PATHLENGTH", ReturnType::Integer),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }
}

#[test]
fn return_types_reach_static_inference() {
    let inferred = |src: &str| infer_type(&parse(src)).unwrap_or_else(|error| panic!("{src:?} inference failed: {error}"));

    assert_eq!(inferred("PATH ( Account[AccountKey], Account[ParentAccountKey] )"), InferredType::Data(DataType::String));
    assert_eq!(inferred("PATHCONTAINS ( Account[AccountPath], 2 )"), InferredType::Data(DataType::Boolean));
    assert_eq!(inferred("PATHLENGTH ( Account[AccountPath] )"), InferredType::Data(DataType::Integer));
    // The result of PATHITEM follows the optional Type argument, so it stays unknown.
    assert_eq!(inferred("PATHITEM ( Account[AccountPath], 1 )"), InferredType::Unknown);
    assert_eq!(inferred("PATHITEMREVERSE ( Account[AccountPath], 1, INTEGER )"), InferredType::Unknown);
}

#[test]
fn compatibility_flags_are_honest() {
    for spec in parent_child_catalog() {
        assert!(!spec.flags.iterator, "{} is not an iterator", spec.name);
        assert_eq!(spec.is_iterator(), spec.flags.iterator, "{} is_iterator", spec.name);
        assert!(!spec.flags.volatile, "{} is not volatile", spec.name);
        assert!(!spec.flags.not_recommended, "{} is recommended", spec.name);
        assert!(spec.flags.excel_compatible, "{} should be Excel compatible", spec.name);
        assert!(spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
    }
}

#[test]
fn overloads_model_the_documented_signatures() {
    assert_eq!(overload_kinds(spec("PATH"), 2), Some(vec![ParamKind::Column, ParamKind::Column]));
    assert_eq!(overload_kinds(spec("PATHCONTAINS"), 2), Some(vec![ParamKind::Text, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("PATHITEM"), 2), Some(vec![ParamKind::Text, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("PATHITEM"), 3), Some(vec![ParamKind::Text, ParamKind::Number, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("PATHITEMREVERSE"), 2), Some(vec![ParamKind::Text, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("PATHITEMREVERSE"), 3), Some(vec![ParamKind::Text, ParamKind::Number, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("PATHLENGTH"), 1), Some(vec![ParamKind::Text]));

    for name in ["PATHITEM", "PATHITEMREVERSE"] {
        let spec = spec(name);
        assert_eq!(spec.params.len(), 3);
        assert_eq!(spec.params[0].name, "Path");
        assert_eq!(spec.params[1].name, "Position");
        assert_eq!(spec.params[2].name, "Type");
        assert!(!spec.params[0].optional);
        assert!(!spec.params[1].optional);
        assert!(spec.params[2].optional);
    }

    let path = spec("PATH");
    assert_eq!(path.params[0].name, "ID_ColumnName");
    assert_eq!(path.params[1].name, "Parent_ColumnName");
    assert_eq!(path.params[0].kind, ParamKind::Column);
    assert_eq!(path.params[1].kind, ParamKind::Column);

    let contains = spec("PATHCONTAINS");
    assert_eq!(contains.params[0].name, "Path");
    assert_eq!(contains.params[0].kind, ParamKind::Text);
    assert_eq!(contains.params[1].name, "Item");
    assert_eq!(contains.params[1].kind, ParamKind::Any);

    let length = spec("PATHLENGTH");
    assert_eq!(length.params[0].name, "Path");
    assert_eq!(length.params[0].kind, ParamKind::Text);
    assert!(!length.params[0].optional);
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "PATH ( Account[AccountKey], Account[ParentAccountKey] )",
        "PATH ( Employee[EmployeeKey], Employee[ParentEmployeeKey] )",
        "PATHCONTAINS ( Account[AccountPath], 2 )",
        "PATHCONTAINS ( \"1|24|2\", \"24\" )",
        "PATHITEM ( Account[AccountPath], 1 )",
        "PATHITEM ( Account[AccountPath], 2, INTEGER )",
        "PATHITEM ( Account[AccountPath], 2, TEXT )",
        "PATHITEM ( Account[AccountPath], 2, 0 )",
        "PATHITEM ( Account[AccountPath], 2, 1 )",
        "PATHITEMREVERSE ( Account[AccountPath], 1 )",
        "PATHITEMREVERSE ( Account[AccountPath], 2, INTEGER )",
        "PATHITEMREVERSE ( Account[AccountPath], 2, TEXT )",
        "PATHLENGTH ( Account[AccountPath] )",
        "PATHLENGTH ( \"123|456|789|764\" )",
    ] {
        assert_valid(src);
    }

    for src in [
        "PATH ( )",
        "PATH ( Account[AccountKey] )",
        "PATH ( Account[AccountKey], Account[ParentAccountKey], Account[AccountKey] )",
        "PATHCONTAINS ( )",
        "PATHCONTAINS ( Account[AccountPath] )",
        "PATHCONTAINS ( Account[AccountPath], 2, TEXT )",
        "PATHITEM ( )",
        "PATHITEM ( Account[AccountPath] )",
        "PATHITEM ( Account[AccountPath], 1, TEXT, 2 )",
        "PATHITEMREVERSE ( )",
        "PATHITEMREVERSE ( Account[AccountPath] )",
        "PATHITEMREVERSE ( Account[AccountPath], 1, INTEGER, 2 )",
        "PATHLENGTH ( )",
        "PATHLENGTH ( Account[AccountPath], 2 )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::Arity);
    }
}

#[test]
fn error_messages_name_the_function_and_expected_arity() {
    let errors = expression_errors("PATH ( Account[AccountKey] )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].function, "PATH");
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert!(errors[0].message.contains("PATH"), "{}", errors[0].message);
    assert!(errors[0].message.contains('2'), "{}", errors[0].message);
    assert!(!errors[0].span.is_empty(), "validate_expression should report the call span");

    let expr = parse("PATHLENGTH ( )");
    let ExprKind::Call(call) = &expr.kind else { panic!("PATHLENGTH ( ) is not a call") };
    let errors = validate_call(call);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].span.is_empty(), "validate_call has no call span");
}

#[test]
fn path_arguments_must_be_column_references() {
    assert_invalid_kind("PATH ( 1, Account[ParentAccountKey] )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("PATH ( Account[AccountKey], \"1\" )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("PATH ( Account[AccountKey] + 1, Account[ParentAccountKey] )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("PATH ( Account[AccountKey], Account[ParentAccountKey] & \"\" )", ValidationErrorKind::ArgumentKind);

    // Text-only parameters are not statically checked, so literals and column references both pass.
    assert_valid("PATHCONTAINS ( \"1|24|2\", 2 )");
    assert_valid("PATHITEM ( Account[AccountPath], 1 )");
    assert_valid("PATHLENGTH ( \"1|24|2\" )");
}

#[test]
fn nested_calls_are_validated_through_unknown_functions() {
    assert_valid("PATHCONTAINS ( PATH ( Account[AccountKey], Account[ParentAccountKey] ), 2 )");
    assert_valid("PATHLENGTH ( PATH ( Account[AccountKey], Account[ParentAccountKey] ) )");

    let errors = assert_invalid_kind("FOO ( PATH ( 1, Account[ParentAccountKey] ) )", ValidationErrorKind::ArgumentKind);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].function, "PATH");
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in parent_child_catalog() {
        for example in spec.examples {
            let expr = parse(example);
            if let ExprKind::Call(call) = &expr.kind {
                assert!(!call.args.is_empty(), "{} example should have arguments", spec.name);
            }
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} failed validation: {errors:?}", spec.name);
        }
    }

    // A concrete end-to-end expression from the dax.guide page.
    let expr = parse("PATHCONTAINS ( PATH ( Account[AccountKey], Account[ParentAccountKey] ), 2 )");
    assert_eq!(gridcraft_dax::print_expression(&expr), "PATHCONTAINS(PATH(Account[AccountKey], Account[ParentAccountKey]), 2)");
}
