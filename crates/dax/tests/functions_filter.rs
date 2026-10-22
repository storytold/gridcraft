//! Integration tests for the filter catalog.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn filter_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::Filter).collect()
}

const FUNCTIONS: [(&str, usize, usize); 29] = [
    ("ALL", 0, usize::MAX),
    ("ALLCROSSFILTERED", 1, 1),
    ("ALLEXCEPT", 2, usize::MAX),
    ("ALLNOBLANKROW", 1, usize::MAX),
    ("ALLSELECTED", 0, usize::MAX),
    ("ALLSELECTEDAPPLY", 1, 1),
    ("ALLSELECTEDREMOVE", 1, usize::MAX),
    ("ALWAYSAPPLY", 1, 1),
    ("CALCULATE", 1, usize::MAX),
    ("CALCULATETABLE", 1, usize::MAX),
    ("DEPENDON", 2, usize::MAX),
    ("FILTER", 2, 2),
    ("FILTERCLUSTER", 4, usize::MAX),
    ("GROUPCROSSAPPLY", 2, usize::MAX),
    ("GROUPCROSSAPPLYTABLE", 2, usize::MAX),
    ("INDEX", 1, 7),
    ("KEEPFILTERS", 1, 1),
    ("LOOKUP", 3, usize::MAX),
    ("LOOKUPVALUE", 3, usize::MAX),
    ("LOOKUPWITHTOTALS", 3, usize::MAX),
    ("MATCHBY", 0, usize::MAX),
    ("NONFILTER", 1, 1),
    ("OFFSET", 1, 7),
    ("ORDERBY", 0, usize::MAX),
    ("PARTITIONBY", 0, usize::MAX),
    ("REMOVEFILTERS", 0, usize::MAX),
    ("SELECTEDVALUE", 1, 2),
    ("SHADOWCLUSTER", 1, 1),
    ("WINDOW", 2, 10),
];

const EXCEL_COMPATIBLE: [&str; 12] = [
    "ALL",
    "ALLCROSSFILTERED",
    "ALLEXCEPT",
    "ALLNOBLANKROW",
    "ALLSELECTED",
    "CALCULATE",
    "CALCULATETABLE",
    "FILTER",
    "KEEPFILTERS",
    "LOOKUPVALUE",
    "REMOVEFILTERS",
    "SELECTEDVALUE",
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

fn arity_errors(src: &str) -> Vec<ValidationError> {
    expression_errors(src).into_iter().filter(|error| error.kind == ValidationErrorKind::Arity).collect()
}

#[test]
fn catalog_has_every_filter_function_exactly_once_in_dax_guide_order() {
    let specs = filter_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for (index, (name, min_args, max_args)) in FUNCTIONS.iter().enumerate() {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == *name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(specs[index].name, *name, "catalog order at {index}");
        assert_eq!(matches[0].min_args, *min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, *max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::Filter);
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
    assert_eq!(lookup("FiLtEr").map(|spec| spec.name), Some("FILTER"));
    assert_eq!(lookup("allselected").map(|spec| spec.name), Some("ALLSELECTED"));
    assert!(lookup("FILTERX").is_none());
    assert!(lookup("FILTER ").is_none());
    assert!(lookup("").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in filter_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        for example in spec.examples {
            assert!(!example.is_empty(), "{} has an empty example", spec.name);
            assert!(example.to_ascii_uppercase().starts_with(spec.name), "{} example {example:?} must call the function", spec.name);
        }

        let required = spec.params.iter().filter(|param| !param.optional).count();
        let optional = spec.params.iter().filter(|param| param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);

        if spec.max_args == usize::MAX {
            assert!(spec.params.len() >= spec.min_args, "{} descriptive params", spec.name);
            assert!(spec.overloads.iter().any(|overload| overload.variadic), "{} unbounded arity needs a variadic overload", spec.name);
        } else {
            assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);
            assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);
            assert_eq!(spec.overloads.iter().map(|overload| overload.arity()).max(), Some(spec.max_args), "{} overload max", spec.name);
        }

        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);
        assert_eq!(spec.overloads.iter().map(|overload| overload.arity()).min(), Some(spec.min_args), "{} overload min", spec.name);
        let probe = std::cmp::min(spec.max_args, spec.min_args + 4);
        for arity in spec.min_args..=probe {
            assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
        }
        assert!(spec.overload_for(spec.min_args).is_some(), "{} minimum arity overload", spec.name);
        if spec.min_args > 0 {
            assert!(spec.overload_for(spec.min_args - 1).is_none(), "{} accepts too few arguments", spec.name);
        }
        if spec.max_args != usize::MAX {
            assert!(spec.overload_for(spec.max_args + 1).is_none(), "{} accepts too many arguments", spec.name);
        }
    }
}

#[test]
fn return_types_and_iterator_flags_are_documented() {
    for (name, return_type) in [
        ("ALL", ReturnType::Table),
        ("ALLCROSSFILTERED", ReturnType::Table),
        ("ALLEXCEPT", ReturnType::Table),
        ("ALLNOBLANKROW", ReturnType::Table),
        ("ALLSELECTED", ReturnType::Table),
        ("ALLSELECTEDAPPLY", ReturnType::Table),
        ("ALLSELECTEDREMOVE", ReturnType::Table),
        ("ALWAYSAPPLY", ReturnType::Table),
        ("CALCULATE", ReturnType::Any),
        ("CALCULATETABLE", ReturnType::Table),
        ("DEPENDON", ReturnType::Table),
        ("FILTER", ReturnType::Table),
        ("FILTERCLUSTER", ReturnType::Table),
        ("GROUPCROSSAPPLY", ReturnType::Table),
        ("GROUPCROSSAPPLYTABLE", ReturnType::Table),
        ("INDEX", ReturnType::Table),
        ("KEEPFILTERS", ReturnType::Any),
        ("LOOKUP", ReturnType::Any),
        ("LOOKUPVALUE", ReturnType::Any),
        ("LOOKUPWITHTOTALS", ReturnType::Any),
        ("MATCHBY", ReturnType::Any),
        ("NONFILTER", ReturnType::Table),
        ("OFFSET", ReturnType::Table),
        ("ORDERBY", ReturnType::Any),
        ("PARTITIONBY", ReturnType::Any),
        ("REMOVEFILTERS", ReturnType::Any),
        ("SELECTEDVALUE", ReturnType::Any),
        ("SHADOWCLUSTER", ReturnType::Table),
        ("WINDOW", ReturnType::Table),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }

    for spec in filter_catalog() {
        assert_eq!(spec.flags.iterator, spec.name == "FILTER", "{} iterator flag", spec.name);
        assert_eq!(spec.is_iterator(), spec.flags.iterator, "{} is_iterator", spec.name);
        assert!(!spec.flags.volatile, "{} must not be volatile", spec.name);
        assert!(!spec.flags.not_recommended, "{} must not be marked not recommended", spec.name);
        assert!(spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
        assert_eq!(spec.flags.excel_compatible, EXCEL_COMPATIBLE.contains(&spec.name), "{} Excel compatibility", spec.name);
    }
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in filter_catalog() {
        for example in spec.examples {
            let expr = parse(example);
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }
}

#[test]
fn parameter_kinds_track_the_dax_guide_signatures() {
    assert_eq!(overload_kinds(spec("FILTER"), 2), Some(vec![ParamKind::Table, ParamKind::Logical]));
    assert_eq!(overload_kinds(spec("ALL"), 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec("ALL"), 1), Some(vec![ParamKind::Reference]));
    assert_eq!(overload_kinds(spec("ALL"), 2), Some(vec![ParamKind::Reference, ParamKind::Column]));
    // Variadic overloads report their declared prefix; the last kind repeats.
    assert_eq!(overload_kinds(spec("ALLEXCEPT"), 3), Some(vec![ParamKind::Table, ParamKind::Column]));
    assert_eq!(spec("ALLEXCEPT").overload_for(3).and_then(|overload| overload.kind_at(2)), Some(ParamKind::Column));
    assert_eq!(overload_kinds(spec("SELECTEDVALUE"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("SELECTEDVALUE"), 2), Some(vec![ParamKind::Column, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("CALCULATE"), 1), Some(vec![ParamKind::Expression]));
    assert_eq!(overload_kinds(spec("CALCULATE"), 3), Some(vec![ParamKind::Expression, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("CALCULATETABLE"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(overload_kinds(spec("INDEX"), 1), Some(vec![ParamKind::Number]));
    assert_eq!(overload_kinds(spec("INDEX"), 3), Some(vec![ParamKind::Number, ParamKind::Table, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("WINDOW"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("WINDOW"), 5), Some(vec![ParamKind::Number, ParamKind::Any, ParamKind::Number, ParamKind::Any, ParamKind::Table]));
    assert_eq!(overload_kinds(spec("LOOKUP"), 3), Some(vec![ParamKind::Any, ParamKind::Column, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("LOOKUPVALUE"), 3), Some(vec![ParamKind::Column, ParamKind::Column, ParamKind::Any]));
    assert_eq!(overload_kinds(spec("PARTITIONBY"), 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec("PARTITIONBY"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(overload_kinds(spec("MATCHBY"), 0), Some(Vec::new()));
    assert_eq!(overload_kinds(spec("MATCHBY"), 2), Some(vec![ParamKind::Column]));

    // The descriptive signature keeps the documented parameter names and optional markers.
    let window = spec("WINDOW");
    let names: Vec<&str> = window.params.iter().map(|param| param.name).collect();
    assert_eq!(names, ["From", "FromType", "To", "ToType", "Relation", "OrderBy", "Blanks", "PartitionBy", "MatchBy", "Reset"]);
    assert!(window.params[1].optional && !window.params[2].optional);
    assert_eq!(spec("FILTER").params[1].name, "FilterExpression");
    assert_eq!(spec("ALLEXCEPT").params[0].kind, ParamKind::Table);
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "ALL ( )",
        "ALL ( Sales )",
        "ALL ( Sales[Region], Sales[Country] )",
        "ALLCROSSFILTERED ( Sales )",
        "ALLEXCEPT ( Sales, Sales[Region] )",
        "ALLNOBLANKROW ( Customer )",
        "ALLSELECTED ( )",
        "ALLSELECTED ( Sales[Region] )",
        "CALCULATE ( [Sales Amount], ALL ( Sales ) )",
        "CALCULATETABLE ( Sales, Sales[Region] = \"East\" )",
        "FILTER ( Sales, Sales[Amount] > 0 )",
        "SELECTEDVALUE ( Product[Color] )",
        "SELECTEDVALUE ( Product[Color], \"Multiple colors\" )",
        "KEEPFILTERS ( Product[Color] = \"Red\" )",
        "LOOKUP ( [Sales Amount], 'Date'[Year], 2018, \"Inferred\" )",
        "LOOKUPVALUE ( Product[Color], Product[ProductKey], 1, \"Unknown\" )",
        "LOOKUPWITHTOTALS ( [Sales Amount], [Year], 2018 )",
        "INDEX ( 1, Sales, ORDERBY ( Sales[OrderDate] ) )",
        "INDEX ( -1, ALLSELECTED ( Sales ) )",
        "OFFSET ( -1, Sales )",
        "WINDOW ( 1, ABS, 1, ABS, Sales )",
        "WINDOW ( -1, REL, 1, REL, Sales, ORDERBY ( Sales[OrderDate] ), KEEP, PARTITIONBY ( Sales[Region] ) )",
        "MATCHBY ( Sales[OrderKey] )",
        "ORDERBY ( Sales[Amount], DESC )",
        "PARTITIONBY ( Sales[Region] )",
        "REMOVEFILTERS ( Sales[Region] )",
        "DEPENDON ( Sales, Sales[OrderDate] )",
        "NONFILTER ( VALUES ( Product[Color] ) )",
        "SHADOWCLUSTER ( VALUES ( Product[Color] ) )",
        "ALWAYSAPPLY ( FILTER ( VALUES ( Product[Color] ), FALSE ) )",
        "ALLSELECTEDAPPLY ( VALUES ( Product[Color] ) )",
        "ALLSELECTEDREMOVE ( VALUES ( Product[Color] ), Product[Color] )",
        "GROUPCROSSAPPLY ( 'Date'[Year], ALL ( 'Date' ), \"Sales\", [Sales Amount] )",
        "GROUPCROSSAPPLYTABLE ( 'Date'[Year], ALL ( 'Date' ), \"Colors\", VALUES ( 'Product'[Color] ) )",
        "FILTERCLUSTER ( Product[Category], ALL ( Product ), \"\", ALL ( Sales ) )",
    ] {
        assert_valid(src);
    }

    for src in [
        "ALLCROSSFILTERED ( )",
        "ALLCROSSFILTERED ( Sales, Sales )",
        "ALLEXCEPT ( Sales )",
        "ALLNOBLANKROW ( )",
        "ALLSELECTEDAPPLY ( )",
        "ALWAYSAPPLY ( Sales, Sales )",
        "CALCULATE ( )",
        "CALCULATETABLE ( )",
        "DEPENDON ( Sales )",
        "FILTER ( Sales )",
        "FILTER ( Sales, TRUE, FALSE )",
        "FILTERCLUSTER ( Product[Category], ALL ( Product ), \"\" )",
        "GROUPCROSSAPPLY ( 'Date'[Year] )",
        "GROUPCROSSAPPLYTABLE ( 'Date'[Year] )",
        "INDEX ( )",
        "INDEX ( 1, Sales, ORDERBY ( Sales[Amount] ), KEEP, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[OrderKey] ), NONE, 1 )",
        "KEEPFILTERS ( )",
        "KEEPFILTERS ( TRUE, FALSE )",
        "LOOKUP ( [Sales Amount], 'Date'[Year] )",
        "LOOKUPVALUE ( Product[Color], Product[ProductKey] )",
        "LOOKUPWITHTOTALS ( [Sales Amount], [Year] )",
        "NONFILTER ( )",
        "OFFSET ( )",
        "SELECTEDVALUE ( )",
        "SELECTEDVALUE ( Product[Color], \"x\", \"y\" )",
        "SHADOWCLUSTER ( )",
        "WINDOW ( 1 )",
        "WINDOW ( 1, ABS, 1, ABS, Sales, ORDERBY ( Sales[OrderDate] ), KEEP, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[OrderKey] ), NONE, 1 )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_arity_boundary_is_enforced() {
    for spec in filter_catalog() {
        let arities: Vec<usize> = if spec.max_args == usize::MAX {
            let mut probe = vec![spec.min_args, spec.min_args + 1, spec.min_args + 5];
            if spec.min_args > 0 {
                probe.push(spec.min_args - 1);
            }
            probe
        } else {
            (0..=spec.max_args).collect()
        };
        for arity in arities {
            let arguments = vec!["1"; arity].join(", ");
            let src = format!("{} ( {arguments} )", spec.name);
            let errors = arity_errors(&src);
            if arity < spec.min_args || arity > spec.max_args {
                assert_eq!(errors.len(), 1, "{src:?} should report one arity error: {errors:?}");
            } else {
                assert!(errors.is_empty(), "{src:?} has a documented overload and must not report an arity error: {errors:?}");
            }
        }
    }
}

#[test]
fn statically_decidable_kinds_are_enforced() {
    for src in [
        "FILTER ( 1, TRUE )",
        "FILTER ( Sales[Amount], TRUE )",
        "ALLCROSSFILTERED ( 1 )",
        "ALLCROSSFILTERED ( Sales[Region] )",
        "SELECTEDVALUE ( 1 )",
        "SELECTEDVALUE ( \"Color\", \"Unknown\" )",
        "ALLEXCEPT ( Sales, 1 )",
        "PARTITIONBY ( 1 )",
        "MATCHBY ( \"OrderKey\" )",
        "INDEX ( 1, 2 )",
        "OFFSET ( 1, 2 )",
        "WINDOW ( 1, ABS, 1, ABS, 2 )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "FILTER ( Sales, TRUE )",
        "ALLCROSSFILTERED ( 'Product' )",
        "SELECTEDVALUE ( 'Product'[Color] )",
        "ALLEXCEPT ( Sales, Sales[Region] )",
        "PARTITIONBY ( Sales[Region] )",
        "MATCHBY ( Sales[OrderKey] )",
        "INDEX ( 1, ALLSELECTED ( Sales ) )",
    ] {
        assert_valid(src);
    }

    let errors = assert_invalid_kind("FILTER ( 1, TRUE )", ValidationErrorKind::ArgumentKind);
    assert!(errors[0].message.contains("FILTER"), "{errors:?}");
    assert!(errors[0].message.contains("table expression"), "{errors:?}");
}

#[test]
fn validate_call_reports_arities_and_kinds() {
    let errors = call_errors("FILTER ( Sales )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert!(errors[0].span.is_empty());

    let errors = call_errors("ALLCROSSFILTERED ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
    assert!(!errors[0].span.is_empty());

    assert!(call_errors("FILTER ( Sales, Sales[Amount] > 0 )").is_empty());
    assert!(call_errors("ALL ( )").is_empty());
    assert!(call_errors("CALCULATE ( [Sales Amount], ALL ( Sales ) )").is_empty());
}

#[test]
fn validate_expression_reports_nested_filter_calls() {
    let src = "CALCULATE ( [Sales Amount], FILTER ( 1, TRUE ) )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].function, "FILTER");
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
    assert_eq!(&src[errors[0].span.start..errors[0].span.end], "1");

    let src = "FILTER ( Sales, Sales[Amount] > 0 )";
    assert!(expression_errors(src).is_empty());
}
