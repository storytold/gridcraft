//! Integration tests for the "Other" category catalog and its static validation.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn other_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::Other).collect()
}

const FUNCTIONS: [(&str, usize, usize); 16] = [
    ("BITLSHIFT", 2, 2),
    ("BITRSHIFT", 2, 2),
    ("BLANK", 0, 0),
    ("EARLIER", 1, 2),
    ("EARLIEST", 1, 1),
    ("ERROR", 1, 1),
    ("EXTERNALMEASURE", 1, 3),
    ("HASH", 1, usize::MAX),
    ("INFO.STORAGETABLES", 0, usize::MAX),
    ("ISATLEVEL", 1, 1),
    ("ISCURRENCY", 1, 1),
    ("ISINTEGER", 1, 1),
    ("KEYWORDMATCH", 2, 2),
    ("RANK", 0, 7),
    ("ROWNUMBER", 0, 6),
    ("SAMPLECARTESIANPOINTSBYCOVER", 4, 7),
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
fn catalog_has_every_other_function_exactly_once_with_documented_arity() {
    let specs = other_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::Other);
    }

    let catalogued: std::collections::BTreeSet<&str> = specs.iter().map(|spec| spec.name).collect();
    let documented: std::collections::BTreeSet<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(catalogued, documented);

    // Index order from dax.guide/functions/other/.
    let names: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
    assert_eq!(names, FUNCTIONS.iter().map(|(name, _, _)| *name).collect::<Vec<_>>());
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        assert_eq!(lookup(name).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_lowercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_uppercase()).map(|spec| spec.name), Some(name));
    }
    assert_eq!(lookup("eArLiEr").map(|spec| spec.name), Some("EARLIER"));
    assert_eq!(lookup("info.storagetables").map(|spec| spec.name), Some("INFO.STORAGETABLES"));
    assert_eq!(lookup("Info.StorageTables").map(|spec| spec.name), Some("INFO.STORAGETABLES"));
    assert!(lookup("EARLY").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("HASH ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in other_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert!(spec.min_args <= spec.max_args, "{} arity range", spec.name);

        let required = spec.params.iter().filter(|param| !param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);

        if spec.max_args == usize::MAX {
            assert!(spec.overloads.iter().any(|overload| overload.variadic), "{} needs a variadic overload", spec.name);
        } else {
            assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);
            let optional = spec.params.iter().filter(|param| param.optional).count();
            assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);
        }

        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);
        let upper = if spec.max_args == usize::MAX { spec.min_args.saturating_add(8) } else { spec.max_args };
        for arity in spec.min_args..=upper {
            assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
        }

        let arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
        assert_eq!(arities.iter().min().copied(), Some(spec.min_args), "{} overload min", spec.name);
        if spec.max_args != usize::MAX {
            assert_eq!(arities.iter().max().copied(), Some(spec.max_args), "{} overload max", spec.name);
        }

        assert!(spec.sugar_of.is_none(), "{} is not syntax sugar", spec.name);
    }
}

#[test]
fn return_type_metadata() {
    for (name, return_type) in [
        ("BITLSHIFT", ReturnType::Integer),
        ("BITRSHIFT", ReturnType::Integer),
        ("BLANK", ReturnType::Blank),
        ("EARLIER", ReturnType::Any),
        ("EARLIEST", ReturnType::Any),
        ("ERROR", ReturnType::Variant),
        ("EXTERNALMEASURE", ReturnType::Any),
        ("HASH", ReturnType::Integer),
        ("INFO.STORAGETABLES", ReturnType::Table),
        ("ISATLEVEL", ReturnType::Boolean),
        ("ISCURRENCY", ReturnType::Boolean),
        ("ISINTEGER", ReturnType::Boolean),
        ("KEYWORDMATCH", ReturnType::Boolean),
        ("RANK", ReturnType::Integer),
        ("ROWNUMBER", ReturnType::Integer),
        ("SAMPLECARTESIANPOINTSBYCOVER", ReturnType::Table),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }
}

#[test]
fn compatibility_flags_are_honest() {
    let not_recommended: Vec<&str> = other_catalog().iter().filter(|spec| spec.flags.not_recommended).map(|spec| spec.name).collect();
    assert_eq!(not_recommended, ["EARLIER", "EARLIEST"]);

    for name in
        ["EXTERNALMEASURE", "HASH", "INFO.STORAGETABLES", "ISATLEVEL", "ISCURRENCY", "ISINTEGER", "RANK", "ROWNUMBER", "SAMPLECARTESIANPOINTSBYCOVER"]
    {
        assert!(!spec(name).flags.excel_compatible, "{name} is not an Excel function");
    }
    for name in ["BITLSHIFT", "BITRSHIFT", "BLANK", "EARLIER", "EARLIEST", "ERROR", "KEYWORDMATCH"] {
        assert!(spec(name).flags.excel_compatible, "{name} is available in Excel");
    }

    for name in ["HASH", "INFO.STORAGETABLES", "ISATLEVEL", "KEYWORDMATCH", "SAMPLECARTESIANPOINTSBYCOVER"] {
        assert!(!spec(name).flags.direct_query_limited, "{name} DirectQuery note");
    }
    for name in ["BITLSHIFT", "BITRSHIFT", "BLANK", "EARLIER", "EARLIEST", "ERROR", "EXTERNALMEASURE", "ISCURRENCY", "ISINTEGER", "RANK", "ROWNUMBER"]
    {
        assert!(spec(name).flags.direct_query_limited, "{name} DirectQuery note");
    }

    for spec in other_catalog() {
        assert!(!spec.flags.iterator, "{} is not an iterator", spec.name);
        assert!(!spec.flags.volatile, "{} is not volatile", spec.name);
        assert!(!spec.is_iterator(), "{} is_iterator", spec.name);
    }
}

#[test]
fn overloads_model_the_documented_signatures() {
    assert_eq!(overload_kinds(spec("BITLSHIFT"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("BLANK"), 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec("EARLIER"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("EARLIER"), 2), Some(vec![ParamKind::Column, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("EXTERNALMEASURE"), 3), Some(vec![ParamKind::Text, ParamKind::Text, ParamKind::Text]));

    let hash = spec("HASH");
    assert_eq!(hash.min_args, 1);
    let hash_overload = hash.overloads[0];
    assert!(hash_overload.variadic);
    assert_eq!(hash_overload.kind_at(0), Some(ParamKind::Any));
    assert_eq!(hash_overload.kind_at(12), Some(ParamKind::Any));

    let info = spec("INFO.STORAGETABLES");
    assert_eq!(info.min_args, 0);
    assert_eq!(info.overload_for(0).map(|overload| overload.arity()), Some(0));
    assert_eq!(overload_kinds(info, 1), Some(vec![ParamKind::Text]));
    let pair = info.overload_for(2);
    assert!(pair.is_some_and(|overload| overload.variadic));
    assert_eq!(pair.map(|overload| overload.kind_at(3)), Some(Some(ParamKind::Any)));

    let rank = spec("RANK");
    assert_eq!(rank.params.len(), 7);
    assert_eq!(rank.params[0].name, "Ties");
    assert_eq!(rank.params[1].name, "Relation");
    assert_eq!(rank.params[1].kind, ParamKind::Table);
    assert!(rank.params.iter().all(|param| param.optional));
    assert_eq!(overload_kinds(rank, 3), Some(vec![ParamKind::Text, ParamKind::Table, ParamKind::Any]));
    assert_eq!(
        overload_kinds(rank, 7),
        Some(vec![ParamKind::Text, ParamKind::Table, ParamKind::Any, ParamKind::Text, ParamKind::Any, ParamKind::Any, ParamKind::Any])
    );

    let rownumber = spec("ROWNUMBER");
    assert_eq!(rownumber.params.len(), 6);
    assert_eq!(rownumber.params[0].name, "Relation");
    assert!(rownumber.params.iter().all(|param| param.optional));
    assert_eq!(overload_kinds(rownumber, 2), Some(vec![ParamKind::Table, ParamKind::Any]));
    assert_eq!(
        overload_kinds(rownumber, 6),
        Some(vec![ParamKind::Table, ParamKind::Any, ParamKind::Text, ParamKind::Any, ParamKind::Any, ParamKind::Any])
    );

    let sample = spec("SAMPLECARTESIANPOINTSBYCOVER");
    assert_eq!(sample.params.len(), 7);
    assert_eq!(overload_kinds(sample, 4), Some(vec![ParamKind::Number, ParamKind::Table, ParamKind::Column, ParamKind::Column]));
    assert_eq!(
        overload_kinds(sample, 7),
        Some(vec![
            ParamKind::Number,
            ParamKind::Table,
            ParamKind::Column,
            ParamKind::Column,
            ParamKind::Column,
            ParamKind::Number,
            ParamKind::Number
        ])
    );
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "BITLSHIFT ( 4, 2 )",
        "BITRSHIFT ( 8, 2 )",
        "BLANK ()",
        "EARLIER ( Customer[Yearly Income] )",
        "EARLIER ( Customer[Yearly Income], 1 )",
        "EARLIEST ( Customer[Yearly Income] )",
        "ERROR ( \"boom\" )",
        "EXTERNALMEASURE ( \"Remote Sales\" )",
        "EXTERNALMEASURE ( \"Remote Sales\", \"DOUBLE\" )",
        "EXTERNALMEASURE ( \"Remote Sales\", \"DOUBLE\", \"RemoteModel\" )",
        "HASH ( Sales[Amount] )",
        "HASH ( Sales[Amount], Sales[Quantity], Sales[Region] )",
        "INFO.STORAGETABLES ()",
        "INFO.STORAGETABLES ( \"DATABASE_NAME\" )",
        "INFO.STORAGETABLES ( \"DATABASE_NAME\", \"Sales\" )",
        "INFO.STORAGETABLES ( \"DATABASE_NAME\", \"Sales\", \"CUBE_NAME\", \"Model\" )",
        "ISATLEVEL ( 'Date'[Year] )",
        "ISCURRENCY ( Sales[Amount] )",
        "ISINTEGER ( Sales[Quantity] )",
        "KEYWORDMATCH ( \"bike\", \"Mountain Bike\" )",
        "RANK ()",
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ) )",
        "ROWNUMBER ()",
        "ROWNUMBER ( Sales, ORDERBY ( Sales[Amount] ) )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[Longitude], Sales[Latitude] )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[X], Sales[Y], Sales[Radius], 4, 0.1 )",
    ] {
        assert_valid(src);
    }

    for src in [
        "BITLSHIFT ( 4 )",
        "BITLSHIFT ( 4, 2, 1 )",
        "BITRSHIFT ( 8 )",
        "BITRSHIFT ( 8, 2, 1 )",
        "BLANK ( 1 )",
        "EARLIER ( )",
        "EARLIER ( Customer[Yearly Income], 1, 2 )",
        "EARLIEST ( )",
        "EARLIEST ( Customer[Yearly Income], 1 )",
        "ERROR ( )",
        "ERROR ( \"a\", \"b\" )",
        "EXTERNALMEASURE ( )",
        "EXTERNALMEASURE ( \"a\", \"b\", \"c\", \"d\" )",
        "HASH ( )",
        "ISATLEVEL ( )",
        "ISATLEVEL ( 'Date'[Year], 'Date'[Month] )",
        "ISCURRENCY ( )",
        "ISINTEGER ( 1, 2 )",
        "KEYWORDMATCH ( \"bike\" )",
        "KEYWORDMATCH ( \"bike\", \"Bike\", \"Mount\" )",
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ), DEFAULT, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Id] ), 0, 1 )",
        "ROWNUMBER ( Sales, ORDERBY ( Sales[Amount] ), DEFAULT, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Id] ), 0, 1 )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[X] )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[X], Sales[Y], Sales[Radius], 4, 0.1, 2 )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_arity_boundary_is_enforced() {
    for spec in other_catalog() {
        for arity in 0..=9usize {
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
        "EARLIER ( 1 )",
        "EARLIEST ( \"x\" )",
        "ISATLEVEL ( 1 )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, 1, Sales[Y] )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[X], 1 )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[X], Sales[Y], 1 )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "EARLIER ( Customer[Yearly Income] )",
        "EARLIEST ( Customer[Yearly Income] )",
        "ISATLEVEL ( 'Date'[Year] )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[X], Sales[Y] )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[X], Sales[Y], Sales[Radius] )",
    ] {
        assert_valid(src);
    }
}

#[test]
fn table_arguments_reject_obvious_scalars_but_accept_dynamic_tables() {
    for src in [
        "RANK ( DENSE, 1, ORDERBY ( Sales[Amount] ) )",
        "RANK ( DENSE, \"Sales\", ORDERBY ( Sales[Amount] ) )",
        "RANK ( DENSE, Sales[Amount], ORDERBY ( Sales[Amount] ) )",
        "ROWNUMBER ( 1, ORDERBY ( Sales[Amount] ) )",
        "ROWNUMBER ( 1 + 1, ORDERBY ( Sales[Amount] ) )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, 1, Sales[X], Sales[Y] )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, TRUE, Sales[X], Sales[Y] )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales[Amount], Sales[X], Sales[Y] )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ) )",
        "RANK ( DENSE, FILTER ( Sales, Sales[Amount] > 0 ), ORDERBY ( Sales[Amount] ) )",
        "RANK ( DENSE, { 1, 2 }, ORDERBY ( [Value] ) )",
        "ROWNUMBER ( DISTINCT ( Sales[Region] ), ORDERBY ( Sales[Region] ) )",
        "ROWNUMBER ( VALUES ( Sales[Region] ), ORDERBY ( Sales[Region] ) )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, FILTER ( Sales, Sales[Amount] > 0 ), Sales[X], Sales[Y] )",
        "SAMPLECARTESIANPOINTSBYCOVER ( 1000, { 1, 2 }, [X], [Y] )",
    ] {
        assert_valid(src);
    }
}

#[test]
fn variadic_functions_accept_repeatable_arguments() {
    for src in [
        "HASH ( Sales[Amount] )",
        "HASH ( Sales[Amount], Sales[Quantity] )",
        "HASH ( Sales[Amount], Sales[Quantity], Sales[Region], Sales[OrderDate], 1 )",
        "INFO.STORAGETABLES ()",
        "INFO.STORAGETABLES ( \"A\" )",
        "INFO.STORAGETABLES ( \"A\", \"B\" )",
        "INFO.STORAGETABLES ( \"A\", \"B\", \"C\", \"D\", \"E\" )",
    ] {
        assert_valid(src);
    }
    assert_invalid_kind("HASH ( )", ValidationErrorKind::Arity);
}

#[test]
fn rank_and_rownumber_keep_their_optional_clauses() {
    for src in [
        "RANK ( )",
        "RANK ( DENSE )",
        "RANK ( SKIP, Sales )",
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount], DESC ) )",
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount], DESC ), FIRST )",
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount], DESC ), LAST, PARTITIONBY ( Sales[Region] ) )",
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount], DESC ), LAST, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Id] ) )",
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount], DESC ), LAST, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Id] ), 0 )",
        "ROWNUMBER ( )",
        "ROWNUMBER ( Sales )",
        "ROWNUMBER ( Sales, ORDERBY ( Sales[Amount], ASC ) )",
        "ROWNUMBER ( Sales, ORDERBY ( Sales[Amount], ASC ), DEFAULT, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Id] ), HIGHESTPARENT )",
    ] {
        assert_valid(src);
    }
    assert_invalid_kind(
        "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ), DEFAULT, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Id] ), 0, 1 )",
        ValidationErrorKind::Arity,
    );
    assert_invalid_kind(
        "ROWNUMBER ( Sales, ORDERBY ( Sales[Amount] ), DEFAULT, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Id] ), 0, 1 )",
        ValidationErrorKind::Arity,
    );
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in other_catalog() {
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
    let errors = call_errors("BLANK ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    // `Call` has no own span; the expression validator supplies it.
    assert!(errors[0].span.is_empty());

    let errors = call_errors("EARLIER ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
    assert!(!errors[0].span.is_empty());

    assert!(call_errors("BLANK ()").is_empty());
    assert!(call_errors("INFO.STORAGETABLES ()").is_empty());
}

#[test]
fn validate_expression_reports_every_error_with_a_span() {
    let src = "EARLIER ( 1 ) + EARLIEST ( 2 ) + ISATLEVEL ( 3 )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 3, "{errors:?}");
    for error in &errors {
        assert!(!error.span.is_empty(), "{error:?}");
        assert!(error.span.end <= src.len(), "{error:?}");
    }
    let arguments: Vec<&str> = errors.iter().map(|error| &src[error.span.start..error.span.end]).collect();
    assert_eq!(arguments, ["1", "2", "3"]);

    let src = "BLANK ( 1 )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert_eq!(&src[errors[0].span.start..errors[0].span.end], src);
}
