//! Integration tests for the financial catalog.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ValidationErrorKind, parse_expression, validate_expression};

fn financial_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::Financial).collect()
}

/// Every financial function in dax.guide index order, with its documented arity.
const FUNCTIONS: [(&str, usize, usize); 51] = [
    ("ACCRINT", 6, 8),
    ("ACCRINTM", 4, 5),
    ("AMORDEGRC", 6, 7),
    ("AMORLINC", 6, 7),
    ("COUPDAYBS", 3, 4),
    ("COUPDAYS", 3, 4),
    ("COUPDAYSNC", 3, 4),
    ("COUPNCD", 3, 4),
    ("COUPNUM", 3, 4),
    ("COUPPCD", 3, 4),
    ("CUMIPMT", 6, 6),
    ("CUMPRINC", 6, 6),
    ("DB", 4, 5),
    ("DDB", 4, 5),
    ("DISC", 4, 5),
    ("DOLLARDE", 2, 2),
    ("DOLLARFR", 2, 2),
    ("DURATION", 5, 6),
    ("EFFECT", 2, 2),
    ("FV", 3, 5),
    ("INTRATE", 4, 5),
    ("IPMT", 4, 6),
    ("ISPMT", 4, 4),
    ("MDURATION", 5, 6),
    ("NOMINAL", 2, 2),
    ("NPER", 3, 5),
    ("ODDFPRICE", 8, 9),
    ("ODDFYIELD", 8, 9),
    ("ODDLPRICE", 7, 8),
    ("ODDLYIELD", 7, 8),
    ("PDURATION", 3, 3),
    ("PMT", 3, 5),
    ("PPMT", 4, 6),
    ("PRICE", 6, 7),
    ("PRICEDISC", 4, 5),
    ("PRICEMAT", 5, 6),
    ("PV", 3, 5),
    ("RATE", 3, 6),
    ("RECEIVED", 4, 5),
    ("RRI", 3, 3),
    ("SLN", 3, 3),
    ("SYD", 4, 4),
    ("TBILLEQ", 3, 3),
    ("TBILLPRICE", 3, 3),
    ("TBILLYIELD", 3, 3),
    ("VDB", 5, 7),
    ("XIRR", 3, 5),
    ("XNPV", 4, 4),
    ("YIELD", 6, 7),
    ("YIELDDISC", 4, 5),
    ("YIELDMAT", 5, 6),
];

fn parse(src: &str) -> Expr {
    parse_expression(src).unwrap_or_else(|error| panic!("failed to parse `{src}`: {error}"))
}

fn spec(name: &str) -> &'static FunctionSpec {
    lookup(name).unwrap_or_else(|| panic!("{name} is missing from the catalog"))
}

fn errors(src: &str) -> Vec<gridcraft_dax::ValidationError> {
    validate_expression(&parse(src))
}

/// A parseable argument of the requested kind.
fn sample(kind: ParamKind) -> &'static str {
    match kind {
        ParamKind::Date => "DATE ( 2020, 1, 1 )",
        ParamKind::Table => "CashFlows",
        ParamKind::Boolean | ParamKind::Logical => "TRUE",
        ParamKind::Column | ParamKind::Reference => "Sales[Amount]",
        ParamKind::Text => "\"text\"",
        ParamKind::Function => "SUM",
        ParamKind::Json => "\"{}\"",
        ParamKind::Expression | ParamKind::Scalar | ParamKind::Any | ParamKind::Number | ParamKind::Variant => "1",
    }
}

fn call(name: &str, arity: usize, params: &[ParamKind]) -> String {
    let arguments: Vec<&str> = (0..arity).map(|index| sample(params[index])).collect();
    format!("{name} ( {} )", arguments.join(", "))
}

fn overload_kinds(spec: &FunctionSpec, arity: usize) -> Option<Vec<ParamKind>> {
    spec.overload_for(arity).map(|overload| overload.params.to_vec())
}

#[test]
fn catalog_has_every_financial_function_exactly_once_in_index_order() {
    let specs = financial_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len(), "financial function count changed");
    for (record, (name, min, max)) in specs.iter().zip(FUNCTIONS) {
        assert_eq!(record.name, name, "unexpected financial function order");
        assert_eq!(record.category, FunctionCategory::Financial);
        assert_eq!((record.min_args, record.max_args), (min, max), "{name} arity changed");
    }
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        for variant in [name.to_string(), name.to_ascii_lowercase(), format!("{name} ").trim().to_ascii_lowercase()] {
            let found = lookup(&variant).unwrap_or_else(|| panic!("lookup failed for {variant}"));
            assert_eq!(found.name, name);
            assert_eq!(found.category, FunctionCategory::Financial);
        }
    }
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for record in financial_catalog() {
        let name = record.name;
        assert_eq!(name, name.to_ascii_uppercase(), "{name} must be catalogued in upper case");
        assert!(record.min_args <= record.max_args, "{name} has an inverted arity range");
        assert!(!record.params.is_empty(), "{name} must document its parameters");
        assert_eq!(record.params.len(), record.max_args, "{name} parameter count must equal max_args");

        let required = record.params.iter().take_while(|parameter| !parameter.optional).count();
        assert_eq!(required, record.min_args, "{name} optional markers must form a suffix");
        assert!(record.params.iter().skip(required).all(|parameter| parameter.optional), "{name} has a required parameter after an optional one");

        // Exactly one non-variadic overload per accepted arity, matching the documented prefix.
        assert_eq!(record.overloads.len(), record.max_args - record.min_args + 1, "{name} must enumerate every accepted arity");
        for arity in record.min_args..=record.max_args {
            let matching: Vec<_> = record.overloads.iter().filter(|overload| overload.matches(arity)).collect();
            assert_eq!(matching.len(), 1, "{name} must accept arity {arity} exactly once");
            let overload = matching[0];
            assert!(!overload.variadic, "{name} uses a variadic overload");
            let expected: Vec<ParamKind> = record.params[..arity].iter().map(|parameter| parameter.kind).collect();
            assert_eq!(overload.params, expected.as_slice(), "{name} overload kinds for arity {arity}");
        }
        assert!(record.overload_for(record.min_args - 1).is_none(), "{name} accepts too few arguments");
        assert!(record.overload_for(record.max_args + 1).is_none(), "{name} accepts too many arguments");

        assert!(!record.description.is_empty(), "{name} needs a description");
        assert!(!record.examples.is_empty(), "{name} needs at least one example");
        for example in record.examples {
            assert!(example.to_ascii_uppercase().contains(name), "{name} example does not mention the function: {example}");
        }

        assert!(record.flags.excel_compatible, "{name} is an Excel financial function");
        assert!(record.flags.direct_query_limited, "{name} has DirectQuery restrictions");
        assert!(!record.flags.not_recommended, "{name} is not marked not recommended by dax.guide");
        assert_eq!(record.flags.iterator, matches!(name, "XIRR" | "XNPV"), "{name} iterator flag");
        assert_eq!(record.sugar_of, None, "{name} is not syntax sugar");
    }
}

#[test]
fn return_types_match_the_documented_values() {
    for record in financial_catalog() {
        let expected = match record.name {
            "COUPDAYBS" | "COUPDAYSNC" | "COUPNUM" => ReturnType::Integer,
            "COUPNCD" | "COUPPCD" => ReturnType::DateTime,
            _ => ReturnType::Decimal,
        };
        assert_eq!(record.return_type, expected, "{} return type", record.name);
    }
}

#[test]
fn every_catalog_example_parses_and_validates() {
    for record in financial_catalog() {
        for example in record.examples {
            let parsed = parse(example);
            let errors = validate_expression(&parsed);
            assert!(errors.is_empty(), "{} example `{example}`: {errors:?}", record.name);
        }
    }
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for (name, min, max) in FUNCTIONS {
        let record = spec(name);
        let kinds: Vec<ParamKind> = record.params.iter().map(|parameter| parameter.kind).collect();

        for arity in min..=max {
            let source = call(name, arity, &kinds);
            let errors = errors(&source);
            assert!(!errors.iter().any(|error| error.kind == ValidationErrorKind::Arity), "{name} should accept {arity} arguments: {errors:?}");
        }

        if min > 0 {
            let too_few = call(name, min - 1, &kinds);
            let errors = errors(&too_few);
            assert!(errors.iter().any(|error| error.kind == ValidationErrorKind::Arity), "{name} should reject {} arguments", min - 1);
        }

        let too_many_kinds: Vec<ParamKind> = kinds.iter().copied().chain(std::iter::repeat_n(ParamKind::Number, 1)).collect();
        let too_many = call(name, max + 1, &too_many_kinds);
        let errors = errors(&too_many);
        assert!(errors.iter().any(|error| error.kind == ValidationErrorKind::Arity), "{name} should reject {} arguments", max + 1);
    }
}

#[test]
fn every_accepted_arity_matches_its_overload_kinds() {
    for (name, min, max) in FUNCTIONS {
        let record = spec(name);
        for arity in min..=max {
            let expected: Vec<ParamKind> = record.params[..arity].iter().map(|parameter| parameter.kind).collect();
            assert_eq!(overload_kinds(record, arity).as_deref(), Some(expected.as_slice()));
        }
    }
}

#[test]
fn table_arguments_are_only_used_by_the_two_iterators() {
    let with_table: Vec<&str> = financial_catalog()
        .iter()
        .filter(|record| record.params.iter().any(|parameter| parameter.kind == ParamKind::Table))
        .map(|record| record.name)
        .collect();
    assert_eq!(with_table, ["XIRR", "XNPV"]);

    for name in with_table {
        let record = spec(name);
        assert!(record.flags.iterator, "{name} must be flagged as an iterator");
        let table = record.params[0];
        assert_eq!(table.kind, ParamKind::Table);
        assert_eq!(table.name, "Table");
    }
}
