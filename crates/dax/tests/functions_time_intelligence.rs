//! Integration tests for the time intelligence catalog and its static validation.

use gridcraft_dax::functions::{BlankPolicy, FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn time_intelligence_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::TimeIntelligence).collect()
}

/// Every function on <https://dax.guide/functions/time-intelligence/>, in index order.
const FUNCTIONS: [(&str, usize, usize); 45] = [
    ("CLOSINGBALANCEMONTH", 2, 3),
    ("CLOSINGBALANCEQUARTER", 2, 3),
    ("CLOSINGBALANCEWEEK", 2, 3),
    ("CLOSINGBALANCEYEAR", 2, 4),
    ("DATEADD", 3, 5),
    ("DATESBETWEEN", 3, 3),
    ("DATESINPERIOD", 4, 5),
    ("DATESMTD", 1, 1),
    ("DATESQTD", 1, 1),
    ("DATESWTD", 1, 1),
    ("DATESYTD", 1, 2),
    ("ENDOFMONTH", 1, 1),
    ("ENDOFQUARTER", 1, 1),
    ("ENDOFWEEK", 1, 1),
    ("ENDOFYEAR", 1, 2),
    ("FIRSTDATE", 1, 1),
    ("FIRSTNONBLANK", 2, 2),
    ("FIRSTNONBLANKVALUE", 2, 2),
    ("LASTDATE", 1, 1),
    ("LASTNONBLANK", 2, 2),
    ("LASTNONBLANKVALUE", 2, 2),
    ("NEXTDAY", 1, 1),
    ("NEXTMONTH", 1, 1),
    ("NEXTQUARTER", 1, 1),
    ("NEXTWEEK", 1, 1),
    ("NEXTYEAR", 1, 2),
    ("OPENINGBALANCEMONTH", 2, 3),
    ("OPENINGBALANCEQUARTER", 2, 3),
    ("OPENINGBALANCEWEEK", 2, 3),
    ("OPENINGBALANCEYEAR", 2, 4),
    ("PARALLELPERIOD", 3, 3),
    ("PREVIOUSDAY", 1, 1),
    ("PREVIOUSMONTH", 1, 1),
    ("PREVIOUSQUARTER", 1, 1),
    ("PREVIOUSWEEK", 1, 1),
    ("PREVIOUSYEAR", 1, 2),
    ("SAMEPERIODLASTYEAR", 1, 1),
    ("STARTOFMONTH", 1, 1),
    ("STARTOFQUARTER", 1, 1),
    ("STARTOFWEEK", 1, 1),
    ("STARTOFYEAR", 1, 2),
    ("TOTALMTD", 2, 3),
    ("TOTALQTD", 2, 3),
    ("TOTALWTD", 2, 3),
    ("TOTALYTD", 2, 4),
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
fn catalog_has_every_time_intelligence_function_exactly_once_in_index_order() {
    let specs = time_intelligence_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    let catalogued: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
    let documented: Vec<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(catalogued, documented, "the catalog must follow the dax.guide index order");

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::TimeIntelligence);
    }
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        assert_eq!(lookup(name).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_lowercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_uppercase()).map(|spec| spec.name), Some(name));
    }
    assert_eq!(lookup("dAtEsBeTwEeN").map(|spec| spec.name), Some("DATESBETWEEN"));
    assert_eq!(lookup("TOTALYTD").map(|spec| spec.name), Some("TOTALYTD"));
    assert!(lookup("TOTALYTDX").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("DATESYTD ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in time_intelligence_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);

        let required = spec.params.iter().filter(|param| !param.optional).count();
        let optional = spec.params.iter().filter(|param| param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);
        assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);

        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);
        for arity in spec.min_args..=spec.max_args {
            let kinds = overload_kinds(spec, arity);
            assert!(kinds.is_some(), "{} has no overload for arity {arity}", spec.name);
            assert_eq!(kinds.map(|kinds| kinds.len()), Some(arity), "{} overload {arity} kinds", spec.name);
        }
        let arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
        assert_eq!(arities.iter().min().copied(), Some(spec.min_args), "{} overload min", spec.name);
        assert_eq!(arities.iter().max().copied(), Some(spec.max_args), "{} overload max", spec.name);

        assert_eq!(spec.sugar_of, None, "{} has no catalogued sugar link", spec.name);
        assert_eq!(spec.blank_policy, BlankPolicy::Unspecified, "{} blank policy", spec.name);
        assert!(!spec.flags.volatile, "{} is not volatile", spec.name);

        // Every time intelligence function requires a date/calendar or source-column argument.
        if matches!(spec.name, "FIRSTNONBLANK" | "FIRSTNONBLANKVALUE" | "LASTNONBLANK" | "LASTNONBLANKVALUE") {
            assert_eq!(spec.params[0].kind, ParamKind::Column, "{} source column", spec.name);
            assert_eq!(spec.params[1].kind, ParamKind::Expression, "{} iterator expression", spec.name);
        } else {
            let has_date = spec.params.iter().any(|param| param.kind == ParamKind::Date || param.name == "Calendar");
            assert!(has_date, "{} has no date or calendar parameter", spec.name);
        }
    }
}

#[test]
fn iterator_flags_are_honest() {
    let iterators = ["FIRSTNONBLANK", "FIRSTNONBLANKVALUE", "LASTNONBLANK", "LASTNONBLANKVALUE"];
    for spec in time_intelligence_catalog() {
        assert_eq!(spec.flags.iterator, iterators.contains(&spec.name), "{} iterator flag", spec.name);
        assert_eq!(spec.is_iterator(), spec.flags.iterator, "{} is_iterator", spec.name);
    }
}

#[test]
fn return_type_metadata() {
    let scalar_any = [
        "CLOSINGBALANCEMONTH",
        "CLOSINGBALANCEQUARTER",
        "CLOSINGBALANCEWEEK",
        "CLOSINGBALANCEYEAR",
        "FIRSTNONBLANKVALUE",
        "LASTNONBLANKVALUE",
        "OPENINGBALANCEMONTH",
        "OPENINGBALANCEQUARTER",
        "OPENINGBALANCEWEEK",
        "OPENINGBALANCEYEAR",
        "TOTALMTD",
        "TOTALQTD",
        "TOTALWTD",
        "TOTALYTD",
    ];
    for spec in time_intelligence_catalog() {
        let expected = if scalar_any.contains(&spec.name) { ReturnType::Any } else { ReturnType::Table };
        assert_eq!(spec.return_type, expected, "{} return type", spec.name);
    }

    assert_eq!(spec("DATESBETWEEN").return_type, ReturnType::Table);
    assert_eq!(spec("TOTALYTD").return_type, ReturnType::Any);
    assert_eq!(spec("FIRSTNONBLANK").return_type, ReturnType::Table);
    assert_eq!(spec("FIRSTNONBLANKVALUE").return_type, ReturnType::Any);
}

#[test]
fn compatibility_flags_are_honest() {
    let week_or_calendar =
        ["CLOSINGBALANCEWEEK", "DATESWTD", "ENDOFWEEK", "NEXTWEEK", "OPENINGBALANCEWEEK", "PREVIOUSWEEK", "STARTOFWEEK", "TOTALWTD"];
    for spec in time_intelligence_catalog() {
        assert_eq!(spec.flags.excel_compatible, !week_or_calendar.contains(&spec.name), "{} Excel compatibility", spec.name);
        assert!(spec.flags.direct_query_limited, "{} DirectQuery note", spec.name);
        assert!(!spec.flags.not_recommended, "{} is recommended on dax.guide", spec.name);
    }
}

#[test]
fn overloads_model_the_documented_signatures() {
    assert_eq!(overload_kinds(spec("DATEADD"), 3), Some(vec![ParamKind::Date, ParamKind::Number, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("DATEADD"), 4), Some(vec![ParamKind::Date, ParamKind::Number, ParamKind::Text, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("DATEADD"), 5), Some(vec![ParamKind::Date, ParamKind::Number, ParamKind::Text, ParamKind::Text, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("DATESINPERIOD"), 4), Some(vec![ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Text]));
    assert_eq!(
        overload_kinds(spec("DATESINPERIOD"), 5),
        Some(vec![ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Text, ParamKind::Text])
    );
    assert_eq!(overload_kinds(spec("DATESBETWEEN"), 3), Some(vec![ParamKind::Date, ParamKind::Date, ParamKind::Date]));
    assert_eq!(overload_kinds(spec("DATESYTD"), 1), Some(vec![ParamKind::Date]));
    assert_eq!(overload_kinds(spec("DATESYTD"), 2), Some(vec![ParamKind::Date, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("TOTALYTD"), 4), Some(vec![ParamKind::Expression, ParamKind::Date, ParamKind::Any, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("CLOSINGBALANCEWEEK"), 2), Some(vec![ParamKind::Expression, ParamKind::Table]));
    assert_eq!(overload_kinds(spec("FIRSTNONBLANK"), 2), Some(vec![ParamKind::Column, ParamKind::Expression]));

    let totalytd = spec("TOTALYTD");
    assert_eq!(totalytd.params.len(), 4);
    assert_eq!(totalytd.params[3].name, "YearEndDate");
    assert!(totalytd.params[3].optional);
    assert_eq!(totalytd.params[3].kind, ParamKind::Text);

    let dateadd = spec("DATEADD");
    assert_eq!(dateadd.params[3].name, "Extension");
    assert_eq!(dateadd.params[4].name, "Truncation");
    assert!(dateadd.params[3].optional && dateadd.params[4].optional);
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "CLOSINGBALANCEMONTH ( [Sales Amount], 'Date'[Date] )",
        "CLOSINGBALANCEYEAR ( [Sales Amount], 'Date'[Date], ALL ( Sales ), \"6/30/2008\" )",
        "DATEADD ( 'Date'[Date], -1, YEAR )",
        "DATEADD ( Gregorian, 1, MONTH, EXTENDING, ANCHORED )",
        "DATESBETWEEN ( 'Date'[Date], DATE ( 2008, 8, 1 ), DATE ( 2008, 8, 31 ) )",
        "DATESINPERIOD ( 'Date'[Date], MAX ( 'Date'[Date] ), -12, MONTH )",
        "DATESINPERIOD ( Gregorian, DATE ( 2008, 2, 29 ), -1, MONTH, ENDALIGNED )",
        "DATESMTD ( 'Date'[Date] )",
        "DATESQTD ( 'Date'[Date] )",
        "DATESWTD ( Gregorian )",
        "DATESYTD ( 'Date'[Date] )",
        "DATESYTD ( 'Date'[Date], \"6/30/2008\" )",
        "ENDOFMONTH ( 'Date'[Date] )",
        "ENDOFQUARTER ( 'Date'[Date] )",
        "ENDOFWEEK ( Gregorian )",
        "ENDOFYEAR ( 'Date'[Date] )",
        "ENDOFYEAR ( 'Date'[Date], \"6/30/2008\" )",
        "FIRSTDATE ( 'Date'[Date] )",
        "FIRSTNONBLANK ( 'Date'[Date], [Sales Amount] )",
        "FIRSTNONBLANKVALUE ( 'Date'[Date], [Sales Amount] )",
        "LASTDATE ( 'Date'[Date] )",
        "LASTNONBLANK ( 'Date'[Date], [Sales Amount] )",
        "LASTNONBLANKVALUE ( 'Date'[Date], [Sales Amount] )",
        "NEXTDAY ( 'Date'[Date] )",
        "NEXTMONTH ( 'Date'[Date] )",
        "NEXTQUARTER ( 'Date'[Date] )",
        "NEXTWEEK ( Gregorian )",
        "NEXTYEAR ( 'Date'[Date] )",
        "NEXTYEAR ( 'Date'[Date], \"6/30/2008\" )",
        "OPENINGBALANCEMONTH ( [Sales Amount], 'Date'[Date] )",
        "OPENINGBALANCEWEEK ( [Sales Amount], Gregorian )",
        "OPENINGBALANCEYEAR ( [Sales Amount], 'Date'[Date], ALL ( Sales ), \"6/30/2008\" )",
        "PARALLELPERIOD ( 'Date'[Date], -1, YEAR )",
        "PREVIOUSDAY ( 'Date'[Date] )",
        "PREVIOUSMONTH ( 'Date'[Date] )",
        "PREVIOUSQUARTER ( 'Date'[Date] )",
        "PREVIOUSWEEK ( Gregorian )",
        "PREVIOUSYEAR ( 'Date'[Date] )",
        "SAMEPERIODLASTYEAR ( 'Date'[Date] )",
        "STARTOFMONTH ( 'Date'[Date] )",
        "STARTOFQUARTER ( 'Date'[Date] )",
        "STARTOFWEEK ( Gregorian )",
        "STARTOFYEAR ( 'Date'[Date] )",
        "TOTALMTD ( SUM ( Sales[Amount] ), 'Date'[Date] )",
        "TOTALQTD ( SUM ( Sales[Amount] ), 'Date'[Date], Sales[Region] = \"US\" )",
        "TOTALWTD ( SUM ( Sales[Amount] ), Gregorian )",
        "TOTALYTD ( SUM ( Sales[Amount] ), 'Date'[Date] )",
    ] {
        assert_valid(src);
    }

    for src in [
        "CLOSINGBALANCEMONTH ( [Sales Amount] )",
        "CLOSINGBALANCEMONTH ( [Sales Amount], 'Date'[Date], TRUE, FALSE )",
        "DATEADD ( 'Date'[Date], -1 )",
        "DATEADD ( 'Date'[Date], -1, YEAR, EXTENDING, ANCHORED, PRECISE )",
        "DATESBETWEEN ( 'Date'[Date], DATE ( 2008, 8, 1 ) )",
        "DATESBETWEEN ( 'Date'[Date], DATE ( 2008, 8, 1 ), DATE ( 2008, 8, 2 ), DATE ( 2008, 8, 3 ) )",
        "DATESINPERIOD ( 'Date'[Date], MAX ( 'Date'[Date] ), -12 )",
        "DATESINPERIOD ( 'Date'[Date], MAX ( 'Date'[Date] ), -12, MONTH, ENDALIGNED, PRECISE )",
        "DATESMTD ( )",
        "DATESMTD ( 'Date'[Date], 'Date'[Date] )",
        "DATESYTD ( )",
        "DATESYTD ( 'Date'[Date], \"6/30/2008\", TRUE )",
        "FIRSTDATE ( )",
        "FIRSTNONBLANK ( 'Date'[Date] )",
        "FIRSTNONBLANK ( 'Date'[Date], [Sales Amount], TRUE )",
        "NEXTYEAR ( 'Date'[Date], \"6/30/2008\", TRUE )",
        "TOTALYTD ( SUM ( Sales[Amount] ) )",
        "TOTALYTD ( SUM ( Sales[Amount] ), 'Date'[Date], TRUE, \"6/30/2008\", FALSE )",
        "TOTALWTD ( SUM ( Sales[Amount] ) )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }

    // `DATESMTD ( 'Date'[Date] )` is valid; keep it out of the invalid list.
    assert!(expression_errors("DATESMTD ( 'Date'[Date] )").is_empty());
}

#[test]
fn every_documented_arity_boundary_is_enforced() {
    for spec in time_intelligence_catalog() {
        for arity in 0..=6usize {
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
        "FIRSTNONBLANK ( 1, [Sales Amount] )",
        "FIRSTNONBLANK ( \"Date\", [Sales Amount] )",
        "FIRSTNONBLANKVALUE ( 1 + 1, [Sales Amount] )",
        "LASTNONBLANK ( TRUE, [Sales Amount] )",
        "LASTNONBLANKVALUE ( -1, [Sales Amount] )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "FIRSTNONBLANK ( 'Date'[Date], [Sales Amount] )",
        "FIRSTNONBLANK ( [Date], [Sales Amount] )",
        "FIRSTNONBLANKVALUE ( Sales[ProductKey], [Sales Amount] )",
        "LASTNONBLANK ( 'Date'[Date], [Sales Amount] )",
        "LASTNONBLANKVALUE ( Sales[ProductKey], [Sales Amount] )",
    ] {
        assert_valid(src);
    }
}

#[test]
fn calendar_arguments_reject_date_columns_but_accept_calendars() {
    for src in [
        "DATESWTD ( 'Date'[Date] )",
        "ENDOFWEEK ( 'Date'[Date] )",
        "NEXTWEEK ( 'Date'[Date] )",
        "PREVIOUSWEEK ( 'Date'[Date] )",
        "STARTOFWEEK ( 'Date'[Date] )",
        "CLOSINGBALANCEWEEK ( [Sales Amount], 'Date'[Date] )",
        "OPENINGBALANCEWEEK ( [Sales Amount], 'Date'[Date] )",
        "TOTALWTD ( [Sales Amount], 'Date'[Date] )",
        "ENDOFWEEK ( 1 )",
        "TOTALWTD ( [Sales Amount], 1 + 1 )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "DATESWTD ( Gregorian )",
        "ENDOFWEEK ( Gregorian )",
        "NEXTWEEK ( Gregorian )",
        "PREVIOUSWEEK ( Gregorian )",
        "STARTOFWEEK ( Gregorian )",
        "CLOSINGBALANCEWEEK ( [Sales Amount], Gregorian )",
        "OPENINGBALANCEWEEK ( [Sales Amount], Gregorian, Sales[Region] = \"US\" )",
        "TOTALWTD ( SUM ( Sales[Amount] ), Gregorian )",
    ] {
        assert_valid(src);
    }
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for spec in time_intelligence_catalog() {
        assert!(!spec.examples.is_empty(), "{} has no examples", spec.name);
        for example in spec.examples {
            let expr = parse(example);
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }
}

#[test]
fn descriptions_name_the_window_and_the_date_table_caveat() {
    for spec in time_intelligence_catalog() {
        let description = spec.description.to_ascii_lowercase();
        assert!(
            description.contains("date table") || description.contains("calendar"),
            "{} description must state the date table or calendar requirement",
            spec.name
        );
        assert!(
            description.contains("window")
                || description.contains("month")
                || description.contains("year")
                || description.contains("quarter")
                || description.contains("week")
                || description.contains("day"),
            "{} description must state the time window",
            spec.name
        );
    }
}

#[test]
fn unknown_functions_are_ignored_but_nested_calls_are_checked() {
    assert!(expression_errors("FOO ( 1, 2, 3 )").is_empty());
    assert!(expression_errors("CALCULATE ( TOTALYTD ( SUM ( Sales[Amount] ), 'Date'[Date] ) )").is_empty());
    assert!(call_errors("FOO ( 1, 2, 3 )").is_empty());

    let errors = expression_errors("CALCULATE ( FIRSTNONBLANK ( 1, [Sales Amount] ) )");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].function, "FIRSTNONBLANK");
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
}

#[test]
fn validate_call_reports_arities_and_kinds() {
    let errors = call_errors("DATEADD ( 'Date'[Date], -1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    // `Call` has no own span; the expression validator supplies it.
    assert!(errors[0].span.is_empty());

    let errors = call_errors("FIRSTNONBLANK ( 1, [Sales Amount] )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::ArgumentKind);
    assert!(!errors[0].span.is_empty());

    assert!(call_errors("DATESYTD ( 'Date'[Date] )").is_empty());
    assert!(call_errors("TOTALYTD ( SUM ( Sales[Amount] ), 'Date'[Date] )").is_empty());
}

#[test]
fn validate_expression_reports_every_error_with_a_span() {
    let src = "FIRSTNONBLANK ( 1, [Sales Amount] ) + LASTNONBLANK ( 2, [Sales Amount] )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 2, "{errors:?}");
    for error in &errors {
        assert!(!error.span.is_empty(), "{error:?}");
        assert!(error.span.end <= src.len(), "{error:?}");
    }
    let arguments: Vec<&str> = errors.iter().map(|error| &src[error.span.start..error.span.end]).collect();
    assert_eq!(arguments, ["1", "2"]);

    let src = "TOTALYTD ( SUM ( Sales[Amount] ), 'Date'[Date], TRUE, \"6/30/2008\", FALSE )";
    let errors = expression_errors(src);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert_eq!(&src[errors[0].span.start..errors[0].span.end], src);
}
