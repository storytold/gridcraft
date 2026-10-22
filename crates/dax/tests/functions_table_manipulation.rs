//! Integration tests for the table manipulation catalog and its static validation.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn table_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::TableManipulation).collect()
}

const FUNCTIONS: [(&str, usize, usize); 35] = [
    ("ADDCOLUMNS", 3, usize::MAX),
    ("ADDMISSINGITEMS", 1, usize::MAX),
    ("CROSSJOIN", 1, usize::MAX),
    ("CURRENTGROUP", 0, 0),
    ("DATATABLE", 3, usize::MAX),
    ("DETAILROWS", 1, 1),
    ("DISTINCT", 1, 1),
    ("EXCEPT", 2, 2),
    ("FILTERS", 1, 1),
    ("GENERATE", 2, 2),
    ("GENERATEALL", 2, 2),
    ("GENERATESERIES", 2, 3),
    ("GROUPBY", 1, usize::MAX),
    ("IGNORE", 1, 1),
    ("INTERSECT", 2, 2),
    ("NATURALINNERJOIN", 2, 2),
    ("NATURALJOINUSAGE", 1, 1),
    ("NATURALLEFTOUTERJOIN", 2, 2),
    ("NONVISUAL", 1, 1),
    ("ROLLUP", 1, usize::MAX),
    ("ROLLUPADDISSUBTOTAL", 2, usize::MAX),
    ("ROLLUPGROUP", 1, usize::MAX),
    ("ROLLUPISSUBTOTAL", 2, usize::MAX),
    ("ROW", 2, usize::MAX),
    ("SAMPLEAXISWITHLOCALMINMAX", 5, usize::MAX),
    ("SELECTCOLUMNS", 1, usize::MAX),
    ("SUBSTITUTEWITHINDEX", 4, usize::MAX),
    ("SUMMARIZE", 1, usize::MAX),
    ("SUMMARIZECOLUMNS", 0, usize::MAX),
    ("TOPN", 2, usize::MAX),
    ("TOPNPERLEVEL", 6, 6),
    ("TOPNSKIP", 3, usize::MAX),
    ("TREATAS", 2, usize::MAX),
    ("UNION", 2, usize::MAX),
    ("VALUES", 1, 1),
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

fn kinds_at(spec: &FunctionSpec, arity: usize) -> Option<Vec<ParamKind>> {
    spec.overload_for(arity).map(|overload| (0..arity).filter_map(|index| overload.kind_at(index)).collect())
}

fn arity_errors(src: &str) -> Vec<ValidationError> {
    expression_errors(src).into_iter().filter(|error| error.kind == ValidationErrorKind::Arity).collect()
}

#[test]
fn catalog_has_every_table_manipulation_function_exactly_once_in_index_order() {
    let specs = table_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for ((name, min_args, max_args), spec) in FUNCTIONS.iter().zip(specs.iter()) {
        assert_eq!(spec.name, *name, "catalog order follows the dax.guide index");
        assert_eq!(spec.min_args, *min_args, "{name} min arity");
        assert_eq!(spec.max_args, *max_args, "{name} max arity");
        assert_eq!(spec.category, FunctionCategory::TableManipulation);
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
    assert_eq!(lookup("CurrentGroup").map(|spec| spec.name), Some("CURRENTGROUP"));
    assert_eq!(lookup("summarizeColumns").map(|spec| spec.name), Some("SUMMARIZECOLUMNS"));
    assert!(lookup("SUMMARISE").is_none());
    assert!(lookup("ROW ").is_none());
    assert!(lookup("").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in table_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);
        assert_eq!(spec.sugar_of, None, "{} has no syntax sugar", spec.name);
        assert_eq!(spec.category, FunctionCategory::TableManipulation);

        let required = spec.params.iter().filter(|param| !param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);

        if spec.max_args == usize::MAX {
            assert!(spec.overloads.iter().any(|overload| overload.variadic), "{} unbounded arity needs a variadic overload", spec.name);
            for arity in spec.min_args..=spec.min_args + 10 {
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
    for name in ["IGNORE", "ROLLUP", "ROLLUPADDISSUBTOTAL", "ROLLUPGROUP", "ROLLUPISSUBTOTAL"] {
        assert_eq!(spec(name).return_type, ReturnType::Blank, "{name} returns no value");
    }

    for spec in table_catalog() {
        if !matches!(spec.name, "IGNORE" | "ROLLUP" | "ROLLUPADDISSUBTOTAL" | "ROLLUPGROUP" | "ROLLUPISSUBTOTAL") {
            assert_eq!(spec.return_type, ReturnType::Table, "{} return type", spec.name);
        }

        let is_iterator =
            matches!(spec.name, "ADDCOLUMNS" | "GENERATE" | "GENERATEALL" | "SELECTCOLUMNS" | "SUBSTITUTEWITHINDEX" | "TOPN" | "TOPNSKIP");
        assert_eq!(spec.flags.iterator, is_iterator, "{} iterator flag", spec.name);
        assert_eq!(spec.is_iterator(), is_iterator, "{} is_iterator", spec.name);

        let is_excel_compatible = !matches!(spec.name, "NATURALJOINUSAGE" | "SAMPLEAXISWITHLOCALMINMAX" | "TOPNPERLEVEL");
        assert_eq!(spec.flags.excel_compatible, is_excel_compatible, "{} Excel compatibility", spec.name);

        assert!(!spec.flags.volatile, "{} is not volatile", spec.name);
        assert!(!spec.flags.not_recommended, "{} is not function-level not recommended", spec.name);
        assert!(spec.flags.direct_query_limited, "{} carries the DirectQuery restrictions", spec.name);
    }
}

#[test]
fn overloads_model_the_documented_signatures() {
    assert_eq!(overload_kinds(spec("ADDCOLUMNS"), 3), Some(vec![ParamKind::Table, ParamKind::Text, ParamKind::Expression]));
    assert!(spec("ADDCOLUMNS").overload_for(5).is_some_and(|overload| overload.variadic));

    assert_eq!(overload_kinds(spec("ADDMISSINGITEMS"), 1), Some(vec![ParamKind::Table]));
    assert!(spec("ADDMISSINGITEMS").overload_for(4).is_some_and(|overload| overload.variadic));

    assert_eq!(overload_kinds(spec("CROSSJOIN"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(kinds_at(spec("CROSSJOIN"), 3), Some(vec![ParamKind::Table; 3]));

    assert_eq!(overload_kinds(spec("DATATABLE"), 3), Some(vec![ParamKind::Text, ParamKind::Text, ParamKind::Any]));

    assert_eq!(overload_kinds(spec("DISTINCT"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(spec("DISTINCT").overloads.len(), 2);
    assert_eq!(spec("DISTINCT").overloads[1].params, &[ParamKind::Table]);

    assert_eq!(overload_kinds(spec("GENERATESERIES"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("GENERATESERIES"), 3), Some(vec![ParamKind::Number, ParamKind::Number, ParamKind::Number]));

    assert_eq!(overload_kinds(spec("GROUPBY"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(overload_kinds(spec("GROUPBY"), 2), Some(vec![ParamKind::Table, ParamKind::Column]));
    assert!(spec("GROUPBY").overload_for(6).is_some_and(|overload| overload.variadic));

    assert_eq!(overload_kinds(spec("ROLLUPADDISSUBTOTAL"), 2), Some(vec![ParamKind::Column, ParamKind::Text]));
    assert!(spec("ROLLUPADDISSUBTOTAL").overload_for(5).is_some_and(|overload| overload.variadic));

    // The variadic model repeats the last kind, so a fourth `ROW` argument reports `Any`.
    assert_eq!(kinds_at(spec("ROW"), 4), Some(vec![ParamKind::Text, ParamKind::Any, ParamKind::Any, ParamKind::Any]));

    assert_eq!(overload_kinds(spec("SELECTCOLUMNS"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(overload_kinds(spec("SELECTCOLUMNS"), 2), Some(vec![ParamKind::Table, ParamKind::Any]));

    assert_eq!(
        overload_kinds(spec("SUBSTITUTEWITHINDEX"), 4),
        Some(vec![ParamKind::Table, ParamKind::Text, ParamKind::Table, ParamKind::Expression])
    );

    assert_eq!(overload_kinds(spec("SUMMARIZE"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(overload_kinds(spec("SUMMARIZE"), 2), Some(vec![ParamKind::Table, ParamKind::Reference]));

    assert_eq!(overload_kinds(spec("SUMMARIZECOLUMNS"), 0), Some(vec![]));
    assert!(spec("SUMMARIZECOLUMNS").overload_for(5).is_some_and(|overload| overload.variadic));

    assert_eq!(overload_kinds(spec("TOPN"), 2), Some(vec![ParamKind::Number, ParamKind::Table]));
    assert_eq!(overload_kinds(spec("TOPN"), 3), Some(vec![ParamKind::Number, ParamKind::Table, ParamKind::Expression]));

    assert_eq!(
        overload_kinds(spec("TOPNPERLEVEL"), 6),
        Some(vec![ParamKind::Number, ParamKind::Table, ParamKind::Table, ParamKind::Table, ParamKind::Table, ParamKind::Text,])
    );

    assert_eq!(overload_kinds(spec("TOPNSKIP"), 3), Some(vec![ParamKind::Number, ParamKind::Number, ParamKind::Table]));
    assert!(spec("TOPNSKIP").overload_for(6).is_some_and(|overload| overload.variadic));

    assert_eq!(overload_kinds(spec("TREATAS"), 2), Some(vec![ParamKind::Table, ParamKind::Column]));
    assert_eq!(kinds_at(spec("TREATAS"), 3), Some(vec![ParamKind::Table, ParamKind::Column, ParamKind::Column]));

    assert_eq!(overload_kinds(spec("UNION"), 2), Some(vec![ParamKind::Table, ParamKind::Table]));
    assert_eq!(kinds_at(spec("UNION"), 4), Some(vec![ParamKind::Table; 4]));

    assert_eq!(overload_kinds(spec("VALUES"), 1), Some(vec![ParamKind::Column]));
    assert_eq!(spec("VALUES").overloads[1].params, &[ParamKind::Table]);
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "CURRENTGROUP ( )",
        "DISTINCT ( Sales[Region] )",
        "DISTINCT ( Sales )",
        "EXCEPT ( Sales, SalesArchive )",
        "INTERSECT ( Sales, SalesArchive )",
        "UNION ( Sales, SalesArchive )",
        "UNION ( Sales, SalesArchive, SalesForecast )",
        "CROSSJOIN ( Product, Store )",
        "CROSSJOIN ( Product, Store, 'Date' )",
        "GENERATE ( Product, RELATEDTABLE ( Sales ) )",
        "GENERATEALL ( Product, RELATEDTABLE ( Sales ) )",
        "GENERATESERIES ( 1, 10 )",
        "GENERATESERIES ( 1, 10, 2 )",
        "SUMMARIZECOLUMNS ( )",
        "SUMMARIZECOLUMNS ( Sales[Region], \"Amount\", SUM ( Sales[Amount] ) )",
        "TOPN ( 10, Sales )",
        "TOPN ( 10, Sales, Sales[Amount], DESC )",
        "TOPNPERLEVEL ( 10, Sales, Levels, Expanded, Boundaries, \"Restart\" )",
        "TOPNSKIP ( 10, 20, Sales )",
        "TOPNSKIP ( 10, 20, Sales, Sales[Amount], DESC )",
        "VALUES ( Sales[Region] )",
        "VALUES ( Store )",
    ] {
        assert_valid(src);
    }

    for src in [
        "CURRENTGROUP ( 1 )",
        "ADDCOLUMNS ( Sales, \"Amount\" )",
        "ADDMISSINGITEMS ( )",
        "CROSSJOIN ( )",
        "DATATABLE ( \"Name\", STRING )",
        "DETAILROWS ( )",
        "DISTINCT ( )",
        "EXCEPT ( Sales )",
        "FILTERS ( )",
        "GENERATE ( Sales )",
        "GENERATEALL ( Sales )",
        "GENERATESERIES ( 1 )",
        "GENERATESERIES ( 1, 10, 2, 3 )",
        "GROUPBY ( )",
        "IGNORE ( )",
        "INTERSECT ( Sales )",
        "NATURALINNERJOIN ( Sales )",
        "NATURALJOINUSAGE ( )",
        "NATURALLEFTOUTERJOIN ( Sales )",
        "NONVISUAL ( )",
        "ROLLUP ( )",
        "ROLLUPADDISSUBTOTAL ( Sales[Country] )",
        "ROLLUPGROUP ( )",
        "ROLLUPISSUBTOTAL ( Sales[Country] )",
        "ROW ( \"Value\" )",
        "SAMPLEAXISWITHLOCALMINMAX ( 100, Sales, Sales[Order Date], Sales[Amount] )",
        "SELECTCOLUMNS ( )",
        "SUBSTITUTEWITHINDEX ( Sales, \"Index\", Products )",
        "SUMMARIZE ( )",
        "TOPN ( 10 )",
        "TOPNPERLEVEL ( 10, Sales, Levels, Expanded, Boundaries )",
        "TOPNSKIP ( 10, 20 )",
        "TREATAS ( Sales[Region] )",
        "UNION ( Sales )",
        "VALUES ( )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_arity_boundary_is_enforced() {
    for spec in table_catalog() {
        if spec.max_args == usize::MAX {
            if spec.min_args > 0 {
                let src = format!("{} ( )", spec.name);
                assert_eq!(arity_errors(&src).len(), 1, "{src:?} is below the minimum arity");
            }
            for arity in spec.min_args..=spec.min_args + 12 {
                let arguments = vec!["1"; arity].join(", ");
                let src = format!("{} ( {arguments} )", spec.name);
                assert!(arity_errors(&src).is_empty(), "{src:?} has a documented overload");
            }
            continue;
        }

        for arity in 0..=spec.max_args + 1 {
            let arguments = vec!["1"; arity].join(", ");
            let src = format!("{} ( {arguments} )", spec.name);
            if arity < spec.min_args || arity > spec.max_args {
                assert_eq!(arity_errors(&src).len(), 1, "{src:?} should report one arity error");
            } else {
                assert!(arity_errors(&src).is_empty(), "{src:?} has a documented overload");
            }
        }
    }
}

#[test]
fn column_and_table_parameters_are_statically_checked() {
    for src in [
        "ADDCOLUMNS ( 1, \"Amount\", 1 )",
        "CROSSJOIN ( 1 )",
        "DISTINCT ( 1 )",
        "EXCEPT ( Sales, 1 )",
        "FILTERS ( Sales )",
        "FILTERS ( 1 )",
        "GENERATE ( Sales, 1 )",
        "NATURALINNERJOIN ( 1, Sales )",
        "NONVISUAL ( 1 )",
        "SELECTCOLUMNS ( 1, \"Amount\", 1 )",
        "SUBSTITUTEWITHINDEX ( Sales, \"Index\", 1, Sales[ProductKey] )",
        "TREATAS ( 1, Sales[Region] )",
        "UNION ( 1, Sales )",
        "VALUES ( 1 )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "ADDCOLUMNS ( Sales, \"Amount\", Sales[Amount] )",
        "DISTINCT ( Sales[Region] )",
        "FILTERS ( Sales[Region] )",
        "GENERATE ( Sales, RELATEDTABLE ( Sales ) )",
        "NONVISUAL ( TREATAS ( { 1 }, Sales[Region] ) )",
        "SELECTCOLUMNS ( Sales, \"Amount\", Sales[Amount] )",
        "SUBSTITUTEWITHINDEX ( Sales, \"Index\", DISTINCT ( Sales[ProductKey] ), Sales[ProductKey] )",
        "TREATAS ( { 1, 2 }, Sales[Region] )",
        "UNION ( Sales, SalesArchive )",
        "VALUES ( Sales[Region] )",
        "VALUES ( Sales )",
    ] {
        assert_valid(src);
    }

    let errors = assert_invalid_kind("FILTERS ( 1 )", ValidationErrorKind::ArgumentKind);
    let message = &errors[0].message;
    assert!(message.contains("FILTERS"), "{message}");
    assert!(message.contains("column reference"), "{message}");
    assert!(message.contains("literal"), "{message}");
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in table_catalog() {
        assert!(!spec.examples.is_empty(), "{} has no examples", spec.name);
        for example in spec.examples {
            let expr = parse(example);
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }
}

#[test]
fn companion_functions_are_valid_only_in_their_documented_position() {
    for src in [
        "GROUPBY ( Sales, Sales[Region], \"Amount\", SUMX ( CURRENTGROUP (), Sales[Amount] ) )",
        "SUMMARIZE ( Sales, ROLLUP ( Sales[Country], Sales[City] ), \"Amount\", SUM ( Sales[Amount] ) )",
        "SUMMARIZE ( Sales, ROLLUPGROUP ( Sales[Country], Sales[City] ), \"Amount\", SUM ( Sales[Amount] ) )",
        "SUMMARIZECOLUMNS ( ROLLUPADDISSUBTOTAL ( Sales[Country], \"IsSubtotal\" ), \"Amount\", SUM ( Sales[Amount] ) )",
        "SUMMARIZECOLUMNS ( Sales[Region], IGNORE ( [Sales Amount] ) )",
        "SUMMARIZECOLUMNS ( Sales[Region], NATURALJOINUSAGE ( Product ) )",
        "ADDMISSINGITEMS ( Sales[Country], SUMMARIZECOLUMNS ( Sales[Country], \"Amount\", SUM ( Sales[Amount] ) ), ROLLUPISSUBTOTAL ( Sales[Country], \"IsSubtotal\" ) )",
    ] {
        assert_valid(src);
    }
}

#[test]
fn unknown_functions_are_ignored_but_nested_calls_are_checked() {
    assert!(expression_errors("FOO ( 1, 2, 3 )").is_empty());
    assert!(call_errors("FOO ( 1, 2, 3 )").is_empty());

    assert!(expression_errors("CALCULATE ( SUM ( [Sales Amount] ), ROWS )").is_empty());

    let errors = expression_errors("SUMMARIZECOLUMNS ( Sales[Region], \"Amount\", FILTERS ( 1 ) )");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].function, "FILTERS");
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
}

#[test]
fn validate_call_reports_arities_and_kinds() {
    let errors = call_errors("CURRENTGROUP ( )");
    assert!(errors.is_empty());

    let errors = call_errors("CURRENTGROUP ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert!(errors[0].span.is_empty());

    let errors = call_errors("VALUES ( 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
    assert!(!errors[0].span.is_empty());

    assert!(call_errors("VALUES ( Sales[Region] )").is_empty());
    assert!(call_errors("VALUES ( Store )").is_empty());
    assert!(call_errors("SUMMARIZECOLUMNS ( )").is_empty());
}
