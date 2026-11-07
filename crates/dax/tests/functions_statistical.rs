//! Integration tests for the statistical catalog.

use gridcraft_dax::functions::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

/// Every statistical function in dax.guide index order, with its documented arity range.
/// `usize::MAX` marks the open-ended signatures of LINEST, LINESTX and SAMPLE.
const FUNCTIONS: [(&str, usize, usize); 43] = [
    ("BETA.DIST", 4, 6),
    ("BETA.INV", 3, 5),
    ("CHISQ.DIST", 3, 3),
    ("CHISQ.DIST.RT", 2, 2),
    ("CHISQ.INV", 2, 2),
    ("CHISQ.INV.RT", 2, 2),
    ("COMBIN", 2, 2),
    ("COMBINA", 2, 2),
    ("CONFIDENCE.NORM", 3, 3),
    ("CONFIDENCE.T", 3, 3),
    ("EXPON.DIST", 3, 3),
    ("GEOMEAN", 1, 1),
    ("GEOMEANX", 2, 2),
    ("LINEST", 2, usize::MAX),
    ("LINESTX", 3, usize::MAX),
    ("MEDIAN", 1, 1),
    ("MEDIANX", 2, 2),
    ("NORM.DIST", 4, 4),
    ("NORM.INV", 3, 3),
    ("NORM.S.DIST", 2, 2),
    ("NORM.S.INV", 1, 1),
    ("PERCENTILE.EXC", 2, 2),
    ("PERCENTILE.INC", 2, 2),
    ("PERCENTILEX.EXC", 3, 3),
    ("PERCENTILEX.INC", 3, 3),
    ("PERMUT", 2, 2),
    ("POISSON.DIST", 3, 3),
    ("RANK.EQ", 2, 3),
    ("RANKX", 2, 5),
    ("SAMPLE", 3, usize::MAX),
    ("STDEV.P", 1, 1),
    ("STDEV.S", 1, 1),
    ("STDEVX.P", 2, 2),
    ("STDEVX.S", 2, 2),
    ("T.DIST", 3, 3),
    ("T.DIST.2T", 2, 2),
    ("T.DIST.RT", 2, 2),
    ("T.INV", 2, 2),
    ("T.INV.2T", 2, 2),
    ("VAR.P", 1, 1),
    ("VAR.S", 1, 1),
    ("VARX.P", 2, 2),
    ("VARX.S", 2, 2),
];

fn statistical_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::Statistical).collect()
}

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

fn argument(kind: ParamKind) -> &'static str {
    match kind {
        ParamKind::Column => "Sales[Amount]",
        ParamKind::Table => "Sales",
        ParamKind::Expression => "1",
        ParamKind::Scalar => "1",
        ParamKind::Logical => "TRUE",
        ParamKind::Any => "1",
        ParamKind::Date => "DATE ( 2020, 1, 1 )",
        ParamKind::Text => "\"text\"",
        ParamKind::Number => "1",
        ParamKind::Boolean => "TRUE",
        ParamKind::Function => "1",
        ParamKind::Reference => "Sales",
        ParamKind::Json => "\"{}\"",
        ParamKind::Variant => "1",
    }
}

/// Builds a call with `arity` arguments whose kinds follow the first matching overload.
fn call_with_arity(name: &str, arity: usize) -> String {
    let overload = spec(name).overload_for(arity).unwrap_or_else(|| panic!("{name} has no overload for {arity} arguments"));
    let args: Vec<&str> = (0..arity).map(|index| argument(overload.kind_at(index).expect("kind for arity"))).collect();
    format!("{name} ( {} )", args.join(", "))
}

/// Builds a call with `arity` arguments taken from the minimum-arity overload, for probes that
/// intentionally have too few arguments for the function.
fn call_with_prefix(name: &str, arity: usize) -> String {
    let spec = spec(name);
    let overload = spec.overload_for(spec.min_args).expect("minimum overload");
    let args: Vec<&str> = (0..arity).map(|index| argument(overload.kind_at(index).expect("kind for prefix"))).collect();
    format!("{name} ( {} )", args.join(", "))
}

/// Builds a call with one argument more than the maximum-arity overload.
fn call_with_extra(name: &str) -> String {
    let spec = spec(name);
    let overload = spec.overload_for(spec.max_args).expect("maximum overload");
    let mut args: Vec<&str> = (0..spec.max_args).map(|index| argument(overload.kind_at(index).expect("kind for max"))).collect();
    args.push("1");
    format!("{name} ( {} )", args.join(", "))
}

fn call_errors(src: &str) -> Vec<ValidationError> {
    let expr = parse(src);
    match &expr.kind {
        ExprKind::Call(call) => validate_call(call),
        other => panic!("{src:?} is not a call: {other:?}"),
    }
}

#[test]
fn catalog_has_every_statistical_function_once_in_index_order() {
    let specs = statistical_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    let catalogued: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
    let documented: Vec<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(catalogued, documented);

    for (index, (name, min_args, max_args)) in FUNCTIONS.into_iter().enumerate() {
        let spec = specs[index];
        assert_eq!(spec.name, name);
        assert_eq!(spec.min_args, min_args, "{name} min arity");
        assert_eq!(spec.max_args, max_args, "{name} max arity");
        assert_eq!(spec.category, FunctionCategory::Statistical, "{name} category");
    }
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        assert_eq!(lookup(name).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_lowercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_uppercase()).map(|spec| spec.name), Some(name));
    }
    assert_eq!(lookup("medianx").map(|spec| spec.name), Some("MEDIANX"));
    assert!(lookup("MEDIANXX").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("MEDIAN ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in statistical_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);

        let open_ended = spec.max_args == usize::MAX;
        if open_ended {
            // The descriptive signature names the fixed prefix and the optional trailing argument.
            assert!(spec.params.len() < spec.max_args, "{} open-ended params", spec.name);
            assert!(spec.params.iter().any(|param| !param.optional), "{} required params", spec.name);
        } else {
            assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);
            let required = spec.params.iter().filter(|param| !param.optional).count();
            let optional = spec.params.iter().filter(|param| param.optional).count();
            assert_eq!(required, spec.min_args, "{} required params", spec.name);
            assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);
        }

        for arity in spec.min_args..=spec.min_args.min(spec.max_args) {
            assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
        }
        if !open_ended {
            for arity in spec.min_args..=spec.max_args {
                assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
            }
            let arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
            assert_eq!(arities.iter().min().copied(), Some(spec.min_args), "{} overload min", spec.name);
            assert_eq!(arities.iter().max().copied(), Some(spec.max_args), "{} overload max", spec.name);
        }
        assert!(spec.overload_for(spec.min_args + 5).is_some() || !open_ended, "{} open-ended overload", spec.name);
    }
}

#[test]
fn open_ended_signatures_accept_extra_arguments() {
    for (name, min_args) in [("LINEST", 2), ("LINESTX", 3), ("SAMPLE", 3)] {
        let call = call_with_arity(name, min_args);
        assert_valid(&call);
        let extended = call_with_arity(name, min_args + 4);
        assert_valid(&extended);
        assert_eq!(spec(name).max_args, usize::MAX);
    }

    assert_invalid_kind("LINEST ( Sales[Amount] )", ValidationErrorKind::Arity);
    assert_invalid_kind("LINESTX ( Sales, Sales[Amount] )", ValidationErrorKind::Arity);
    assert_invalid_kind("SAMPLE ( 10, Sales )", ValidationErrorKind::Arity);
}

#[test]
fn overloads_model_the_documented_signatures() {
    let kinds = |name: &str, arity: usize| -> Option<Vec<ParamKind>> { spec(name).overload_for(arity).map(|overload| overload.params.to_vec()) };

    assert_eq!(kinds("BETA.DIST", 4), Some(vec![ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Logical]));
    assert_eq!(
        kinds("BETA.DIST", 6),
        Some(vec![ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Logical, ParamKind::Number, ParamKind::Number,])
    );
    assert_eq!(kinds("BETA.INV", 3), Some(vec![ParamKind::Number, ParamKind::Number, ParamKind::Number]));
    assert_eq!(kinds("GEOMEAN", 1), Some(vec![ParamKind::Column]));
    assert_eq!(kinds("GEOMEANX", 2), Some(vec![ParamKind::Table, ParamKind::Expression]));
    assert_eq!(kinds("MEDIAN", 1), Some(vec![ParamKind::Column]));
    assert_eq!(kinds("PERCENTILEX.INC", 3), Some(vec![ParamKind::Table, ParamKind::Expression, ParamKind::Number]));
    assert_eq!(kinds("RANKX", 5), Some(vec![ParamKind::Table, ParamKind::Expression, ParamKind::Scalar, ParamKind::Scalar, ParamKind::Scalar]));
    assert_eq!(kinds("RANK.EQ", 2), Some(vec![ParamKind::Scalar, ParamKind::Column]));
    assert_eq!(kinds("LINEST", 2), Some(vec![ParamKind::Column, ParamKind::Column]));
    assert_eq!(kinds("LINEST", 3), Some(vec![ParamKind::Column, ParamKind::Column, ParamKind::Any]));
    assert_eq!(kinds("LINESTX", 3), Some(vec![ParamKind::Table, ParamKind::Expression, ParamKind::Expression]));
    assert_eq!(kinds("SAMPLE", 3), Some(vec![ParamKind::Number, ParamKind::Table, ParamKind::Expression]));
}

#[test]
fn return_type_metadata() {
    let decimals = [
        "BETA.DIST",
        "BETA.INV",
        "CHISQ.DIST",
        "CHISQ.DIST.RT",
        "CHISQ.INV",
        "CHISQ.INV.RT",
        "CONFIDENCE.NORM",
        "CONFIDENCE.T",
        "EXPON.DIST",
        "GEOMEAN",
        "GEOMEANX",
        "NORM.DIST",
        "NORM.INV",
        "NORM.S.DIST",
        "NORM.S.INV",
        "PERMUT",
        "POISSON.DIST",
        "STDEV.P",
        "STDEV.S",
        "STDEVX.P",
        "STDEVX.S",
        "T.DIST",
        "T.DIST.2T",
        "T.DIST.RT",
        "T.INV",
        "T.INV.2T",
        "VAR.P",
        "VAR.S",
        "VARX.P",
        "VARX.S",
    ];
    let integers = ["COMBIN", "COMBINA", "RANK.EQ", "RANKX"];
    let tables = ["LINEST", "LINESTX", "SAMPLE"];
    let variants = ["MEDIAN", "MEDIANX", "PERCENTILE.EXC", "PERCENTILE.INC", "PERCENTILEX.EXC", "PERCENTILEX.INC"];

    for name in decimals {
        assert_eq!(spec(name).return_type, ReturnType::Decimal, "{name} return type");
    }
    for name in integers {
        assert_eq!(spec(name).return_type, ReturnType::Integer, "{name} return type");
    }
    for name in tables {
        assert_eq!(spec(name).return_type, ReturnType::Table, "{name} return type");
    }
    for name in variants {
        assert_eq!(spec(name).return_type, ReturnType::Variant, "{name} return type");
    }

    assert_eq!(decimals.len() + integers.len() + tables.len() + variants.len(), FUNCTIONS.len());
}

#[test]
fn iterator_and_compatibility_flags_are_honest() {
    let iterators =
        ["GEOMEANX", "LINESTX", "MEDIANX", "PERCENTILEX.EXC", "PERCENTILEX.INC", "RANKX", "SAMPLE", "STDEVX.P", "STDEVX.S", "VARX.P", "VARX.S"];

    for spec in statistical_catalog() {
        assert_eq!(spec.flags.iterator, iterators.contains(&spec.name), "{} iterator flag", spec.name);
        assert_eq!(spec.is_iterator(), spec.flags.iterator, "{} is_iterator", spec.name);
        assert!(!spec.flags.volatile, "{} should not be volatile", spec.name);
        assert!(!spec.flags.not_recommended, "{} should not be marked not recommended", spec.name);
        assert!(spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
        assert_eq!(spec.flags.excel_compatible, !matches!(spec.name, "LINEST" | "LINESTX"), "{} Excel compatibility", spec.name);
    }

    assert_eq!(spec("BETA.DIST").flags, FunctionFlags::new(false, false, false, true, true), "distribution functions use the standard flags");
    assert_eq!(spec("LINEST").flags, FunctionFlags::new(false, false, false, false, true));
    assert_eq!(spec("LINESTX").flags, FunctionFlags::new(true, false, false, false, true));
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in statistical_catalog() {
        for example in spec.examples {
            assert_valid(example);
        }
    }
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for spec in statistical_catalog() {
        let minimum = call_with_arity(spec.name, spec.min_args);
        assert!(call_errors(&minimum).is_empty(), "{} minimum call {minimum:?}", spec.name);

        if spec.min_args > 0 {
            let too_few = call_with_prefix(spec.name, spec.min_args - 1);
            let errors = call_errors(&too_few);
            assert!(errors.iter().any(|error| error.kind == ValidationErrorKind::Arity), "{} should reject {too_few:?}", spec.name);
        }

        if spec.max_args != usize::MAX {
            let too_many = call_with_extra(spec.name);
            let errors = call_errors(&too_many);
            assert!(errors.iter().any(|error| error.kind == ValidationErrorKind::Arity), "{} should reject {too_many:?}", spec.name);
        }
    }
}

#[test]
fn documented_arity_ranges_are_enforced_for_key_functions() {
    assert_valid("BETA.DIST ( 0.5, 3.5, 4, TRUE )");
    assert_valid("BETA.DIST ( 0.5, 3.5, 4, TRUE, 0 )");
    assert_valid("BETA.DIST ( 0.5, 3.5, 4, TRUE, 0, 1 )");
    assert_invalid_kind("BETA.DIST ( 0.5, 3.5, 4 )", ValidationErrorKind::Arity);
    assert_invalid_kind("BETA.DIST ( 0.5, 3.5, 4, TRUE, 0, 1, 2 )", ValidationErrorKind::Arity);

    assert_valid("RANKX ( Sales, Sales[Amount] )");
    assert_valid("RANKX ( Sales, Sales[Amount], 42 )");
    assert_valid("RANKX ( Sales, Sales[Amount], 42, 0 )");
    assert_valid("RANKX ( Sales, Sales[Amount], 42, 0, Dense )");
    assert_invalid_kind("RANKX ( Sales )", ValidationErrorKind::Arity);
    assert_invalid_kind("RANKX ( Sales, Sales[Amount], 42, 0, Dense, 1 )", ValidationErrorKind::Arity);

    assert_valid("RANK.EQ ( 42, Sales[Quantity] )");
    assert_valid("RANK.EQ ( 42, Sales[Quantity], 1 )");
    assert_invalid_kind("RANK.EQ ( 42, Sales[Quantity], 1, 0 )", ValidationErrorKind::Arity);
}

#[test]
fn column_and_table_arguments_are_checked() {
    assert_invalid_kind("GEOMEAN ( 1 )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("MEDIAN ( Sales[Amount] + 1 )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("STDEV.P ( \"text\" )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("STDEVX.P ( 1, 1 )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("LINEST ( 1, Sales[Amount] )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("RANKX ( 1, Sales[Amount] )", ValidationErrorKind::ArgumentKind);
    assert_invalid_kind("RANK.EQ ( 1, 1 )", ValidationErrorKind::ArgumentKind);

    assert_valid("STDEVX.P ( FILTER ( Sales, Sales[Amount] > 0 ), Sales[Amount] )");
    assert_valid("LINESTX ( Sales, Sales[Amount], Sales[Quantity] * 2 )");
    assert_valid("SAMPLE ( 10, DISTINCT ( Sales[Amount] ), Sales[Amount] )");
}
