//! Integration tests for the date and time catalog.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationErrorKind, parse_expression, validate_call, validate_expression};

const FUNCTIONS: [(&str, usize, usize); 24] = [
    ("CALENDAR", 2, 2),
    ("CALENDARAUTO", 0, 1),
    ("DATE", 3, 3),
    ("DATEDIFF", 3, 3),
    ("DATEVALUE", 1, 1),
    ("DAY", 1, 1),
    ("EDATE", 2, 2),
    ("EOMONTH", 2, 2),
    ("HOUR", 1, 1),
    ("MINUTE", 1, 1),
    ("MONTH", 1, 1),
    ("NETWORKDAYS", 2, 4),
    ("NOW", 0, 0),
    ("QUARTER", 1, 1),
    ("SECOND", 1, 1),
    ("TIME", 3, 3),
    ("TIMEVALUE", 1, 1),
    ("TODAY", 0, 0),
    ("UTCNOW", 0, 0),
    ("UTCTODAY", 0, 0),
    ("WEEKDAY", 1, 2),
    ("WEEKNUM", 1, 2),
    ("YEAR", 1, 1),
    ("YEARFRAC", 2, 3),
];

fn date_time_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::DateAndTime).collect()
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

fn call_errors(src: &str) -> Vec<gridcraft_dax::ValidationError> {
    let expr = parse(src);
    match &expr.kind {
        ExprKind::Call(call) => validate_call(call),
        other => panic!("{src:?} is not a call: {other:?}"),
    }
}

fn has_arity_error(src: &str) -> bool {
    call_errors(src).iter().any(|error| error.kind == ValidationErrorKind::Arity)
}

#[test]
fn catalog_has_every_date_and_time_function_exactly_once_with_documented_arity() {
    let specs = date_time_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::DateAndTime);
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
    assert_eq!(lookup("EoMonth").map(|spec| spec.name), Some("EOMONTH"));
    assert_eq!(lookup("DateValue").map(|spec| spec.name), Some("DATEVALUE"));
    assert!(lookup("SUMMARY").is_none());
    assert!(lookup("DATE(").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("DAY ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in date_time_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert!(spec.min_args <= spec.max_args, "{} min <= max", spec.name);
        assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);

        let required = spec.params.iter().filter(|param| !param.optional).count();
        let optional = spec.params.iter().filter(|param| param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);
        assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);
        for param in spec.params {
            assert!(!param.name.is_empty(), "{} parameter name", spec.name);
        }

        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);
        for arity in spec.min_args..=spec.max_args {
            assert!(spec.overload_for(arity).is_some(), "{} has no overload for arity {arity}", spec.name);
        }
        let arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
        assert_eq!(arities.iter().min().copied(), Some(spec.min_args), "{} overload min", spec.name);
        assert_eq!(arities.iter().max().copied(), Some(spec.max_args), "{} overload max", spec.name);

        for overload in spec.overloads {
            assert_eq!(overload.params.len(), overload.arity(), "{} overload arity", spec.name);
        }
    }
}

#[test]
fn every_example_parses_as_an_expression() {
    for spec in date_time_catalog() {
        for example in spec.examples {
            let expr = parse(example);
            assert!(validate_expression(&expr).is_empty(), "{} example {example:?} validates", spec.name);
        }
    }
}

#[test]
fn parameter_kinds_and_return_types_are_precise() {
    assert_eq!(spec("DATE").return_type, ReturnType::DateTime);
    assert_eq!(spec("DAY").return_type, ReturnType::Integer);
    assert_eq!(spec("YEAR").return_type, ReturnType::Integer);
    assert_eq!(spec("DATEDIFF").return_type, ReturnType::Integer);
    assert_eq!(spec("NETWORKDAYS").return_type, ReturnType::Integer);
    assert_eq!(spec("CALENDAR").return_type, ReturnType::Table);
    assert_eq!(spec("CALENDARAUTO").return_type, ReturnType::Table);
    assert_eq!(spec("YEARFRAC").return_type, ReturnType::Decimal);
    assert_eq!(spec("TIMEVALUE").return_type, ReturnType::DateTime);

    assert_eq!(spec("DATEVALUE").params[0].kind, ParamKind::Text);
    assert_eq!(spec("TIMEVALUE").params[0].kind, ParamKind::Text);
    assert_eq!(spec("DATE").params[0].kind, ParamKind::Number);
    assert_eq!(spec("DAY").params[0].kind, ParamKind::Date);
    assert_eq!(spec("EDATE").params[1].kind, ParamKind::Number);
    assert_eq!(spec("NETWORKDAYS").params[3].kind, ParamKind::Table);
    assert!(spec("NETWORKDAYS").params[2].optional);
    assert!(spec("NETWORKDAYS").params[3].optional);
    assert!(spec("WEEKDAY").params[1].optional);
    assert!(spec("YEARFRAC").params[2].optional);
}

#[test]
fn volatile_and_host_flags_are_honest() {
    let volatile = ["NOW", "TODAY", "UTCNOW", "UTCTODAY"];
    for spec in date_time_catalog() {
        assert_eq!(spec.flags.volatile, volatile.contains(&spec.name), "{} volatile flag", spec.name);
        assert!(!spec.flags.iterator, "{} is not an iterator", spec.name);
        assert!(!spec.flags.not_recommended, "{} is recommended", spec.name);
        assert_eq!(spec.flags.excel_compatible, spec.name != "NETWORKDAYS", "{} excel flag", spec.name);
        assert!(!spec.is_iterator(), "{} is_iterator", spec.name);
    }
}

#[test]
fn arity_boundaries_are_enforced_for_representative_functions() {
    // Zero-argument functions reject any argument.
    assert!(!has_arity_error("NOW ()"));
    assert!(has_arity_error("NOW ( 1 )"));
    assert!(!has_arity_error("TODAY ()"));
    assert!(has_arity_error("TODAY ( 1 )"));
    assert!(!has_arity_error("UTCNOW ()"));
    assert!(!has_arity_error("UTCTODAY ()"));
    assert!(has_arity_error("UTCTODAY ( 0 )"));

    // Optional argument: CALENDARAUTO () and CALENDARAUTO ( 3 ), not two arguments.
    assert!(!has_arity_error("CALENDARAUTO ()"));
    assert!(!has_arity_error("CALENDARAUTO ( 3 )"));
    assert!(has_arity_error("CALENDARAUTO ( 3, 4 )"));

    // Fixed arity: DATE takes exactly three arguments.
    assert!(has_arity_error("DATE ( 2024, 1 )"));
    assert!(!has_arity_error("DATE ( 2024, 1, 1 )"));
    assert!(has_arity_error("DATE ( 2024, 1, 1, 1 )"));

    // Optional trailing arguments: WEEKDAY, WEEKNUM, YEARFRAC.
    assert!(has_arity_error("WEEKDAY ()"));
    assert!(!has_arity_error("WEEKDAY ( DATE ( 2024, 1, 1 ) )"));
    assert!(!has_arity_error("WEEKDAY ( DATE ( 2024, 1, 1 ), 2 )"));
    assert!(has_arity_error("WEEKDAY ( DATE ( 2024, 1, 1 ), 2, 3 )"));

    assert!(!has_arity_error("WEEKNUM ( DATE ( 2024, 1, 1 ) )"));
    assert!(!has_arity_error("WEEKNUM ( DATE ( 2024, 1, 1 ), 21 )"));
    assert!(has_arity_error("WEEKNUM ( DATE ( 2024, 1, 1 ), 21, 1 )"));

    assert!(has_arity_error("YEARFRAC ( DATE ( 2024, 1, 1 ) )"));
    assert!(!has_arity_error("YEARFRAC ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ) )"));
    assert!(!has_arity_error("YEARFRAC ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ), 1 )"));
    assert!(has_arity_error("YEARFRAC ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ), 1, 0 )"));

    // Four arities: NETWORKDAYS.
    assert!(has_arity_error("NETWORKDAYS ( DATE ( 2024, 1, 1 ) )"));
    assert!(!has_arity_error("NETWORKDAYS ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 31 ) )"));
    assert!(!has_arity_error("NETWORKDAYS ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 31 ), 1 )"));
    assert!(!has_arity_error("NETWORKDAYS ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 31 ), 1, { DATE ( 2024, 1, 15 ) } )"));
    assert!(has_arity_error("NETWORKDAYS ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 31 ), 1, { DATE ( 2024, 1, 15 ) }, 2 )"));

    // Statically decidable table parameter: the holidays argument must not be a scalar literal.
    let errors = call_errors("NETWORKDAYS ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 31 ), 1, 1 )");
    assert!(errors.iter().any(|error| error.kind == ValidationErrorKind::ArgumentKind), "scalar holidays should be rejected, got {errors:?}");
}
