//! Integration tests for the aggregation catalog and its static validation.

use gridcraft_dax::functions::{BlankPolicy, FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn aggregation_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::Aggregation).collect()
}

const FUNCTIONS: [(&str, usize, usize); 22] = [
    ("APPROXIMATEDISTINCTCOUNT", 1, 1),
    ("AVERAGE", 1, 1),
    ("AVERAGEA", 1, 1),
    ("AVERAGEX", 2, 2),
    ("COUNT", 1, 1),
    ("COUNTA", 1, 1),
    ("COUNTAX", 2, 2),
    ("COUNTBLANK", 1, 1),
    ("COUNTROWS", 0, 1),
    ("COUNTX", 2, 2),
    ("DISTINCTCOUNT", 1, 1),
    ("DISTINCTCOUNTNOBLANK", 1, 1),
    ("MAX", 1, 2),
    ("MAXA", 1, 1),
    ("MAXX", 2, 3),
    ("MIN", 1, 2),
    ("MINA", 1, 1),
    ("MINX", 2, 3),
    ("PRODUCT", 1, 1),
    ("PRODUCTX", 2, 2),
    ("SUM", 1, 1),
    ("SUMX", 2, 2),
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
fn catalog_has_every_aggregation_function_exactly_once_with_documented_arity() {
    let specs = aggregation_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::Aggregation);
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
    assert_eq!(lookup("SuM").map(|spec| spec.name), Some("SUM"));
    assert!(lookup("SUMM").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("SUM ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in aggregation_catalog() {
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

        if let Some(target) = spec.sugar_of {
            assert_ne!(target, spec.name, "{} is sugar of itself", spec.name);
            assert!(lookup(target).is_some(), "{} sugar target {target} is missing", spec.name);
        }
    }
}

#[test]
fn iterator_and_sugar_metadata() {
    let iterators = ["AVERAGEX", "COUNTAX", "COUNTX", "MAXX", "MINX", "PRODUCTX", "SUMX"];
    for spec in aggregation_catalog() {
        assert_eq!(spec.flags.iterator, iterators.contains(&spec.name), "{} iterator flag", spec.name);
        assert_eq!(spec.is_iterator(), spec.flags.iterator, "{} is_iterator", spec.name);
    }

    for (name, target) in [
        ("AVERAGE", "AVERAGEX"),
        ("COUNT", "COUNTX"),
        ("COUNTA", "COUNTAX"),
        ("MAX", "MAXX"),
        ("MIN", "MINX"),
        ("PRODUCT", "PRODUCTX"),
        ("SUM", "SUMX"),
        ("DISTINCTCOUNTNOBLANK", "DISTINCTCOUNT"),
    ] {
        assert_eq!(spec(name).sugar_of, Some(target), "{name} sugar");
    }

    for name in [
        "APPROXIMATEDISTINCTCOUNT",
        "AVERAGEA",
        "AVERAGEX",
        "COUNTAX",
        "COUNTBLANK",
        "COUNTROWS",
        "COUNTX",
        "DISTINCTCOUNT",
        "MAXA",
        "MAXX",
        "MINA",
        "MINX",
        "PRODUCTX",
        "SUMX",
    ] {
        assert_eq!(spec(name).sugar_of, None, "{name} is not pure sugar");
    }
}

#[test]
fn blank_policy_metadata() {
    for (name, policy) in [
        ("APPROXIMATEDISTINCTCOUNT", BlankPolicy::BlankWhenEmpty),
        ("AVERAGE", BlankPolicy::Unspecified),
        ("AVERAGEA", BlankPolicy::Unspecified),
        ("AVERAGEX", BlankPolicy::BlankWhenEmpty),
        ("COUNT", BlankPolicy::BlankWhenEmpty),
        ("COUNTA", BlankPolicy::BlankWhenEmpty),
        ("COUNTAX", BlankPolicy::BlankWhenEmpty),
        ("COUNTBLANK", BlankPolicy::Unspecified),
        ("COUNTROWS", BlankPolicy::BlankWhenEmpty),
        ("COUNTX", BlankPolicy::BlankWhenEmpty),
        ("DISTINCTCOUNT", BlankPolicy::BlankWhenEmpty),
        ("DISTINCTCOUNTNOBLANK", BlankPolicy::Unspecified),
        ("MAX", BlankPolicy::Unspecified),
        ("MAXA", BlankPolicy::Unspecified),
        ("MAXX", BlankPolicy::Unspecified),
        ("MIN", BlankPolicy::Unspecified),
        ("MINA", BlankPolicy::Unspecified),
        ("MINX", BlankPolicy::Unspecified),
        ("PRODUCT", BlankPolicy::BlankWhenEmpty),
        ("PRODUCTX", BlankPolicy::BlankWhenEmpty),
        ("SUM", BlankPolicy::BlankWhenEmpty),
        ("SUMX", BlankPolicy::BlankWhenEmpty),
    ] {
        assert_eq!(spec(name).blank_policy, policy, "{name} blank policy");
    }
}

#[test]
fn return_type_metadata() {
    for (name, return_type) in [
        ("APPROXIMATEDISTINCTCOUNT", ReturnType::Integer),
        ("AVERAGE", ReturnType::Numeric),
        ("AVERAGEA", ReturnType::Numeric),
        ("AVERAGEX", ReturnType::Numeric),
        ("COUNT", ReturnType::Integer),
        ("COUNTA", ReturnType::Integer),
        ("COUNTAX", ReturnType::Integer),
        ("COUNTBLANK", ReturnType::Integer),
        ("COUNTROWS", ReturnType::Integer),
        ("COUNTX", ReturnType::Integer),
        ("DISTINCTCOUNT", ReturnType::Integer),
        ("DISTINCTCOUNTNOBLANK", ReturnType::Integer),
        ("MAX", ReturnType::Any),
        ("MAXA", ReturnType::Any),
        ("MAXX", ReturnType::Any),
        ("MIN", ReturnType::Any),
        ("MINA", ReturnType::Any),
        ("MINX", ReturnType::Any),
        ("PRODUCT", ReturnType::Decimal),
        ("PRODUCTX", ReturnType::Decimal),
        ("SUM", ReturnType::Numeric),
        ("SUMX", ReturnType::Numeric),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }
}

#[test]
fn compatibility_flags_are_honest() {
    assert!(!spec("APPROXIMATEDISTINCTCOUNT").flags.excel_compatible);
    assert!(!spec("DISTINCTCOUNTNOBLANK").flags.excel_compatible);
    assert!(spec("COUNTROWS").flags.excel_compatible);
    for spec in aggregation_catalog() {
        if spec.name != "APPROXIMATEDISTINCTCOUNT" && spec.name != "DISTINCTCOUNTNOBLANK" {
            assert!(spec.flags.excel_compatible, "{} should be Excel compatible", spec.name);
        }
        assert!(spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
    }

    let not_recommended: Vec<&str> = aggregation_catalog().iter().filter(|spec| spec.flags.not_recommended).map(|spec| spec.name).collect();
    assert_eq!(not_recommended, ["AVERAGEA"]);

    // COUNTROWS () is in the catalog even though Excel's embedded engine rejects that form.
    assert_valid("COUNTROWS ( )");
    assert_eq!(spec("COUNTROWS").min_args, 0);
}

#[test]
fn overloads_model_the_documented_signatures() {
    assert_eq!(overload_kinds(spec("MAX"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("MAX"), 2), Some(vec![ParamKind::Scalar, ParamKind::Scalar]));
    assert_eq!(overload_kinds(spec("MIN"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("MIN"), 2), Some(vec![ParamKind::Scalar, ParamKind::Scalar]));
    assert_eq!(overload_kinds(spec("COUNTROWS"), 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec("COUNTROWS"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(overload_kinds(spec("MAXX"), 2), Some(vec![ParamKind::Table, ParamKind::Expression]));
    assert_eq!(overload_kinds(spec("MAXX"), 3), Some(vec![ParamKind::Table, ParamKind::Expression, ParamKind::Logical]));
    assert_eq!(overload_kinds(spec("MINX"), 3), Some(vec![ParamKind::Table, ParamKind::Expression, ParamKind::Logical]));

    let maxx = spec("MAXX");
    assert_eq!(maxx.params.len(), 3);
    assert_eq!(maxx.params[2].name, "Variant");
    assert_eq!(maxx.params[2].kind, ParamKind::Logical);
    assert!(maxx.params[2].optional);
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "SUM ( Sales[Amount] )",
        "COUNTROWS ( )",
        "COUNTROWS ( Sales )",
        "MAX ( 1, 2 )",
        "MAX ( Sales[Amount] )",
        "MIN ( 3, 5 )",
        "MAXX ( Sales, Sales[Amount] )",
        "MAXX ( Sales, Sales[Amount], TRUE )",
        "MINX ( Sales, Sales[Amount] )",
        "MINX ( Sales, Sales[Amount], FALSE )",
        "AVERAGEX ( Sales, Sales[Amount] )",
        "COUNTAX ( Sales, Sales[Flag] )",
        "COUNTX ( Sales, Sales[Quantity] )",
        "PRODUCTX ( Sales, Sales[Quantity] )",
        "SUMX ( Sales, Sales[Amount] )",
        "DISTINCTCOUNT ( Sales[Region] )",
    ] {
        assert_valid(src);
    }

    for src in [
        "SUM ( )",
        "SUM ( Sales[Amount], Sales[Amount] )",
        "COUNTROWS ( Sales, Sales )",
        "MAX ( )",
        "MAX ( 1, 2, 3 )",
        "MIN ( 1, 2, 3 )",
        "MAXX ( Sales )",
        "MAXX ( Sales, Sales[Amount], TRUE, FALSE )",
        "MINX ( Sales )",
        "MINX ( Sales, Sales[Amount], TRUE, FALSE )",
        "AVERAGEX ( Sales )",
        "AVERAGEX ( Sales, Sales[Amount], TRUE )",
        "COUNTAX ( Sales )",
        "COUNTX ( Sales )",
        "PRODUCTX ( Sales )",
        "SUMX ( Sales )",
        "DISTINCTCOUNT ( )",
        "COUNT ( )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_arity_boundary_is_enforced() {
    for spec in aggregation_catalog() {
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
fn column_only_arguments_must_be_bracketed_references() {
    for src in [
        "SUM ( 1 )",
        "AVERAGE ( 1 + 1 )",
        "COUNT ( \"x\" )",
        "COUNTA ( TRUE )",
        "COUNTBLANK ( 1 )",
        "DISTINCTCOUNT ( 1 )",
        "DISTINCTCOUNTNOBLANK ( 1 )",
        "APPROXIMATEDISTINCTCOUNT ( 1 )",
        "PRODUCT ( 1 )",
        "MAXA ( 1 )",
        "MINA ( 1 )",
        "MAX ( 1 )",
        "MIN ( 1 )",
        "SUM ( -1 )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "SUM ( Sales[Amount] )",
        "SUM ( [Amount] )",
        "AVERAGE ( 'Sales'[Amount] )",
        "COUNT ( Sales[Text] )",
        "DISTINCTCOUNT ( Sales[Region] )",
        "APPROXIMATEDISTINCTCOUNT ( Sales[ProductKey] )",
        "PRODUCT ( Sales[Quantity] )",
        "MAX ( Sales[Amount] )",
        "MIN ( Sales[Amount] )",
        "MAXA ( Sales[Flag] )",
        "MINA ( Sales[Flag] )",
    ] {
        assert_valid(src);
    }

    let errors = assert_invalid_kind("SUM ( 1 )", ValidationErrorKind::ArgumentKind);
    let message = &errors[0].message;
    assert!(message.contains("SUM"), "{message}");
    assert!(message.contains("column reference"), "{message}");
    assert!(message.contains("literal"), "{message}");
}

#[test]
fn table_arguments_reject_obvious_scalars_but_accept_dynamic_tables() {
    for src in [
        "COUNTROWS ( 1 )",
        "COUNTROWS ( \"Sales\" )",
        "COUNTROWS ( -1 )",
        "COUNTROWS ( 1 + 1 )",
        "COUNTROWS ( Sales[Amount] )",
        "SUMX ( 1, Sales[Amount] )",
        "AVERAGEX ( 1 + 1, Sales[Amount] )",
        "MAXX ( TRUE, Sales[Amount] )",
        "MINX ( 2 * 2, Sales[Amount] )",
        "COUNTAX ( \"x\", Sales[Flag] )",
        "COUNTX ( FALSE, Sales[Quantity] )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "COUNTROWS ( Sales )",
        "COUNTROWS ( DISTINCT ( Sales[Region] ) )",
        "COUNTROWS ( VALUES ( Sales[Region] ) )",
        "COUNTROWS ( FILTER ( Sales, Sales[Amount] > 0 ) )",
        "COUNTROWS ( { 1, 2 } )",
        "SUMX ( FILTER ( Sales, Sales[Amount] > 0 ), Sales[Amount] )",
        "SUMX ( { 1, 2, 3 }, [Value] )",
        "AVERAGEX ( ALL ( Sales ), [Amount] )",
        "MAXX ( Sales, Sales[Amount] )",
        "MINX ( RELATEDTABLE ( ResellerSales ), [Sales Amount] )",
    ] {
        assert_valid(src);
    }
}

#[test]
fn max_and_min_model_both_overloads() {
    assert_valid("MAX ( 1, 2 )");
    assert_valid("MAX ( 3, 5 )");
    assert_valid("MAX ( Sales[Amount] )");
    assert_valid("MAX ( 10000, [Sales Amount] )");
    assert_valid("MIN ( 3, 5 )");
    assert_valid("MIN ( Sales[Net Price] )");
    assert_valid("MIN ( 5000000, [Sales Amount] )");

    // One argument is the column-only overload, not a scalar.
    assert_invalid_kind("MAX ( 1 )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("MIN ( 1 )", ValidationErrorKind::ArgumentKind);

    // Three arguments match no overload.
    assert_invalid_kind("MAX ( 1, 2, 3 )", ValidationErrorKind::Arity);
    assert_invalid_kind("MIN ( 1, 2, 3 )", ValidationErrorKind::Arity);
}

#[test]
fn countrows_accepts_zero_or_one_table_argument() {
    for src in ["COUNTROWS ( )", "COUNTROWS ( Sales )", "COUNTROWS ( DISTINCT ( Sales[Region] ) )"] {
        assert_valid(src);
    }
    assert_invalid_kind("COUNTROWS ( Sales, Sales )", ValidationErrorKind::Arity);
    assert_invalid_kind("COUNTROWS ( 1 )", ValidationErrorKind::ArgumentKind);

    let spec = spec("COUNTROWS");
    assert_eq!((spec.min_args, spec.max_args), (0, 1));
    assert_eq!(overload_kinds(spec, 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec, 1), Some(vec![ParamKind::Table]));
}

#[test]
fn maxx_and_minx_accept_the_optional_variant_argument() {
    for src in [
        "MAXX ( Sales, Sales[Amount] )",
        "MAXX ( Sales, Sales[Amount], TRUE )",
        "MAXX ( Customer, 1 * Customer[LargeFamily] )",
        "MINX ( Sales, Sales[Amount] )",
        "MINX ( Sales, Sales[Amount], FALSE )",
        "MINX ( Customer, 1 * Customer[NoCars] )",
    ] {
        assert_valid(src);
    }
    assert_invalid_kind("MAXX ( Sales )", ValidationErrorKind::Arity);
    assert_invalid_kind("MAXX ( Sales, Sales[Amount], TRUE, FALSE )", ValidationErrorKind::Arity);
    assert_invalid_kind("MINX ( Sales )", ValidationErrorKind::Arity);
    assert_invalid_kind("MINX ( Sales, Sales[Amount], TRUE, FALSE )", ValidationErrorKind::Arity);
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in aggregation_catalog() {
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
    assert!(expression_errors("CALCULATE ( SUM ( Sales[Amount] ), Sales[Region] = \"East\" )").is_empty());
    assert!(call_errors("FOO ( 1, 2, 3 )").is_empty());

    let errors = expression_errors("CALCULATE ( SUM ( 1 ) )");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].function, "SUM");
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
}

#[test]
fn validate_call_reports_arities_and_kinds() {
    let errors = call_errors("MAX ( 1, 2, 3 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    // `Call` has no own span; the expression validator supplies it.
    assert!(errors[0].span.is_empty());

    let errors = call_errors("SUM ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
    assert!(!errors[0].span.is_empty());

    assert!(call_errors("SUM ( Sales[Amount] )").is_empty());
    assert!(call_errors("COUNTROWS ( )").is_empty());
}

#[test]
fn validate_expression_reports_every_error_with_a_span() {
    let src = "SUM ( 1 ) + AVERAGE ( 2 ) + MAX ( 3 )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 3, "{errors:?}");
    for error in &errors {
        assert!(!error.span.is_empty(), "{error:?}");
        assert!(error.span.end <= src.len(), "{error:?}");
    }
    let arguments: Vec<&str> = errors.iter().map(|error| &src[error.span.start..error.span.end]).collect();
    assert_eq!(arguments, ["1", "2", "3"]);

    let src = "MAX ( 1, 2, 3 )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert_eq!(&src[errors[0].span.start..errors[0].span.end], src);

    let src = "COUNTROWS ( 1 )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 1);
    assert_eq!(&src[errors[0].span.start..errors[0].span.end], "1");
}
