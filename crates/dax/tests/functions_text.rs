//! Integration tests for the text catalog and its static validation.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn text_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::Text).collect()
}

/// Every function on <https://dax.guide/functions/text/>, in index order. `usize::MAX` marks a
/// variadic maximum arity.
const FUNCTIONS: [(&str, usize, usize); 23] = [
    ("COMBINEVALUES", 3, usize::MAX),
    ("CONCATENATE", 2, 2),
    ("CONCATENATEX", 2, 5),
    ("EXACT", 2, 2),
    ("FIND", 2, 4),
    ("FIXED", 1, 3),
    ("FORMAT", 2, 3),
    ("LEFT", 1, 2),
    ("LEN", 1, 1),
    ("LOWER", 1, 1),
    ("MID", 3, 3),
    ("REPLACE", 4, 4),
    ("REPT", 2, 2),
    ("RIGHT", 1, 2),
    ("SEARCH", 2, 4),
    ("SUBSTITUTE", 3, 4),
    ("TOCSV", 1, 4),
    ("TOJSON", 1, 2),
    ("TRIM", 1, 1),
    ("UNICHAR", 1, 1),
    ("UNICODE", 1, 1),
    ("UPPER", 1, 1),
    ("VALUE", 1, 1),
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
fn catalog_has_every_text_function_exactly_once_in_index_order() {
    let specs = text_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    let names: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
    let expected: Vec<&str> = FUNCTIONS.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(names, expected, "text functions must follow the dax.guide index order");

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::Text);
    }
}

#[test]
fn lookup_is_case_insensitive() {
    for (name, _, _) in FUNCTIONS {
        assert_eq!(lookup(name).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_lowercase()).map(|spec| spec.name), Some(name));
        assert_eq!(lookup(&name.to_ascii_uppercase()).map(|spec| spec.name), Some(name));
    }
    assert_eq!(lookup("Left").map(|spec| spec.name), Some("LEFT"));
    assert_eq!(lookup("toJson").map(|spec| spec.name), Some("TOJSON"));
    assert!(lookup("TEXT").is_none());
    assert!(lookup("LEFTT").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("LEFT ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in text_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);

        let variadic = spec.overloads.iter().any(|overload| overload.variadic);
        assert_eq!(variadic, spec.max_args == usize::MAX, "{} variadic arity", spec.name);

        if spec.max_args != usize::MAX {
            assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);
            let required = spec.params.iter().filter(|param| !param.optional).count();
            let optional = spec.params.iter().filter(|param| param.optional).count();
            assert_eq!(required, spec.min_args, "{} required params", spec.name);
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
            assert_eq!(spec.params.len(), spec.min_args, "{} variadic descriptive params", spec.name);
        }

        assert!(!spec.flags.volatile, "{} volatile flag", spec.name);
    }
}

#[test]
fn documented_parameter_kinds_and_overloads() {
    let combinevalues = spec("COMBINEVALUES");
    assert_eq!(overload_kinds(combinevalues, 3), Some(vec![ParamKind::Text, ParamKind::Any, ParamKind::Any]));
    assert_eq!(
        combinevalues.overload_for(6).map(|overload| overload.kind_at(5)),
        Some(Some(ParamKind::Any)),
        "the last COMBINEVALUES operand repeats"
    );
    assert_eq!(combinevalues.params.iter().map(|param| param.name).collect::<Vec<_>>(), ["Delimiter", "Expression1", "Expression2"]);

    assert_eq!(overload_kinds(spec("CONCATENATE"), 2), Some(vec![ParamKind::Text, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("EXACT"), 2), Some(vec![ParamKind::Text, ParamKind::Text]));

    assert_eq!(overload_kinds(spec("CONCATENATEX"), 2), Some(vec![ParamKind::Table, ParamKind::Expression]));
    assert_eq!(overload_kinds(spec("CONCATENATEX"), 3), Some(vec![ParamKind::Table, ParamKind::Expression, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("CONCATENATEX"), 4), Some(vec![ParamKind::Table, ParamKind::Expression, ParamKind::Text, ParamKind::Expression]));
    assert_eq!(
        overload_kinds(spec("CONCATENATEX"), 5),
        Some(vec![ParamKind::Table, ParamKind::Expression, ParamKind::Text, ParamKind::Expression, ParamKind::Logical,])
    );
    let concatenatex = spec("CONCATENATEX");
    assert!(concatenatex.overloads.iter().all(|overload| !overload.variadic), "CONCATENATEX uses fixed overloads");
    assert_eq!(concatenatex.params[2].name, "Delimiter");
    assert_eq!(concatenatex.params[3].name, "OrderBy_Expression");
    assert_eq!(concatenatex.params[4].name, "Order");
    assert!(concatenatex.params[2..].iter().all(|param| param.optional));

    for name in ["FIND", "SEARCH"] {
        assert_eq!(overload_kinds(spec(name), 2), Some(vec![ParamKind::Text, ParamKind::Text]));
        assert_eq!(overload_kinds(spec(name), 3), Some(vec![ParamKind::Text, ParamKind::Text, ParamKind::Number]));
        assert_eq!(overload_kinds(spec(name), 4), Some(vec![ParamKind::Text, ParamKind::Text, ParamKind::Number, ParamKind::Any]));
        assert_eq!(spec(name).params[3].name, "NotFoundValue");
        assert!(spec(name).params[3].optional);
    }

    assert_eq!(overload_kinds(spec("FIXED"), 1), Some(vec![ParamKind::Number]));
    assert_eq!(overload_kinds(spec("FIXED"), 2), Some(vec![ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("FIXED"), 3), Some(vec![ParamKind::Number, ParamKind::Number, ParamKind::Boolean]));

    assert_eq!(overload_kinds(spec("FORMAT"), 2), Some(vec![ParamKind::Any, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("FORMAT"), 3), Some(vec![ParamKind::Any, ParamKind::Text, ParamKind::Text]));
    assert_eq!(spec("FORMAT").params[2].name, "LocaleName");

    for name in ["LEFT", "RIGHT"] {
        assert_eq!(overload_kinds(spec(name), 1), Some(vec![ParamKind::Text]));
        assert_eq!(overload_kinds(spec(name), 2), Some(vec![ParamKind::Text, ParamKind::Number]));
        assert_eq!(spec(name).params[1].name, "NumberOfCharacters");
    }

    assert_eq!(overload_kinds(spec("LEN"), 1), Some(vec![ParamKind::Text]));
    assert_eq!(overload_kinds(spec("LOWER"), 1), Some(vec![ParamKind::Text]));
    assert_eq!(overload_kinds(spec("UPPER"), 1), Some(vec![ParamKind::Text]));
    assert_eq!(overload_kinds(spec("TRIM"), 1), Some(vec![ParamKind::Text]));
    assert_eq!(overload_kinds(spec("VALUE"), 1), Some(vec![ParamKind::Text]));
    assert_eq!(overload_kinds(spec("UNICODE"), 1), Some(vec![ParamKind::Text]));
    assert_eq!(overload_kinds(spec("UNICHAR"), 1), Some(vec![ParamKind::Number]));

    assert_eq!(overload_kinds(spec("MID"), 3), Some(vec![ParamKind::Text, ParamKind::Number, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("REPLACE"), 4), Some(vec![ParamKind::Text, ParamKind::Number, ParamKind::Number, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("REPT"), 2), Some(vec![ParamKind::Text, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("SUBSTITUTE"), 3), Some(vec![ParamKind::Text, ParamKind::Text, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("SUBSTITUTE"), 4), Some(vec![ParamKind::Text, ParamKind::Text, ParamKind::Text, ParamKind::Number]));
    assert_eq!(spec("SUBSTITUTE").params[3].name, "InstanceNumber");

    assert_eq!(overload_kinds(spec("TOCSV"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(overload_kinds(spec("TOCSV"), 2), Some(vec![ParamKind::Table, ParamKind::Number]));
    assert_eq!(overload_kinds(spec("TOCSV"), 3), Some(vec![ParamKind::Table, ParamKind::Number, ParamKind::Text]));
    assert_eq!(overload_kinds(spec("TOCSV"), 4), Some(vec![ParamKind::Table, ParamKind::Number, ParamKind::Text, ParamKind::Boolean]));
    assert_eq!(overload_kinds(spec("TOJSON"), 1), Some(vec![ParamKind::Table]));
    assert_eq!(overload_kinds(spec("TOJSON"), 2), Some(vec![ParamKind::Table, ParamKind::Number]));
}

#[test]
fn return_types_and_flags_are_documented() {
    for (name, return_type) in [
        ("COMBINEVALUES", ReturnType::String),
        ("CONCATENATE", ReturnType::String),
        ("CONCATENATEX", ReturnType::String),
        ("EXACT", ReturnType::Boolean),
        ("FIND", ReturnType::Integer),
        ("FIXED", ReturnType::String),
        ("FORMAT", ReturnType::String),
        ("LEFT", ReturnType::String),
        ("LEN", ReturnType::Integer),
        ("LOWER", ReturnType::String),
        ("MID", ReturnType::String),
        ("REPLACE", ReturnType::String),
        ("REPT", ReturnType::String),
        ("RIGHT", ReturnType::String),
        ("SEARCH", ReturnType::Integer),
        ("SUBSTITUTE", ReturnType::String),
        ("TOCSV", ReturnType::String),
        ("TOJSON", ReturnType::String),
        ("TRIM", ReturnType::String),
        ("UNICHAR", ReturnType::String),
        ("UNICODE", ReturnType::Integer),
        ("UPPER", ReturnType::String),
        ("VALUE", ReturnType::Decimal),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }

    for spec in text_catalog() {
        assert_eq!(spec.flags.iterator, spec.name == "CONCATENATEX", "{} iterator flag", spec.name);
        assert!(!spec.flags.not_recommended, "{} is recommended by dax.guide", spec.name);

        let engine_only = ["TOCSV", "TOJSON"].contains(&spec.name);
        assert_eq!(spec.flags.excel_compatible, !engine_only, "{} Excel compatibility", spec.name);

        let dq_restricted = ["CONCATENATEX", "FIXED", "FORMAT", "REPLACE", "TOCSV", "TOJSON"].contains(&spec.name);
        assert_eq!(spec.flags.direct_query_limited, dq_restricted, "{} DirectQuery flag", spec.name);
    }

    assert!(spec("CONCATENATEX").is_iterator());
    assert!(!spec("CONCATENATE").is_iterator());
}

#[test]
fn descriptions_state_the_operation_and_the_case_or_locale_caveat() {
    assert!(spec("EXACT").description.contains("case"));
    assert!(spec("FIND").description.contains("case-sensitive"));
    assert!(spec("FIND").description.contains("accent-sensitive"));
    assert!(spec("FIND").description.contains("wildcards are not interpreted"));
    assert!(spec("SEARCH").description.contains("ignores case"));
    assert!(spec("SEARCH").description.contains("wildcards"));
    assert!(spec("SUBSTITUTE").description.contains("case-sensitive"));
    assert!(spec("LOWER").description.contains("locale-sensitive"));
    assert!(spec("UPPER").description.contains("locale-sensitive"));
    assert!(spec("TRIM").description.contains("non-breaking"));
    assert!(spec("REPT").description.contains("32,767"));
    assert!(spec("FIXED").description.contains("locale"));
    assert!(spec("FORMAT").description.contains("locale"));
    assert!(spec("VALUE").description.contains("locale"));
    assert!(spec("CONCATENATEX").description.contains("unspecified"));
    assert!(spec("TOJSON").description.contains("unspecified"));
    assert!(spec("TOCSV").description.contains("Not available in Excel"));
    assert!(spec("TOJSON").description.contains("DirectQuery calculated columns"));
    assert!(spec("COMBINEVALUES").description.contains("delimiter"));
    assert!(spec("COMBINEVALUES").description.contains("DirectQuery"));
}

#[test]
fn every_example_parses_and_validates() {
    for spec in text_catalog() {
        for (index, example) in spec.examples.iter().enumerate() {
            let expr = parse_expression(example).unwrap_or_else(|error| panic!("{} example {index} {example:?} did not parse: {error}", spec.name));
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }
}

#[test]
fn arity_boundaries_accept_and_reject_the_right_calls() {
    for src in [
        "COMBINEVALUES ( \" | \", Customer[Customer Name], Customer[Customer Code] )",
        "COMBINEVALUES ( \"-\", \"Contoso\", \"2020\", \"Q1\" )",
        "CONCATENATE ( \"Contoso\", \" 2020\" )",
        "CONCATENATEX ( Sales, Sales[Product] )",
        "CONCATENATEX ( Sales, Sales[Product], \", \" )",
        "CONCATENATEX ( Sales, Sales[Product], \", \", Sales[Amount] )",
        "CONCATENATEX ( Sales, Sales[Product], \", \", Sales[Amount], TRUE )",
        "EXACT ( \"SQLBI\", \"sqlbi\" )",
        "FIND ( \"Red\", \"Contoso 2G MP3 Player E200 Red\" )",
        "FIND ( \"Red\", \"Contoso 2G MP3 Player E200 Red\", 1 )",
        "FIND ( \"Red\", \"Contoso 2G MP3 Player E200 Red\", 1, BLANK () )",
        "FIXED ( 384518.16 )",
        "FIXED ( 384518.16, 2 )",
        "FIXED ( 384518.16, 2, FALSE )",
        "FORMAT ( 0.742, \"Percent\" )",
        "FORMAT ( 1234.567, \"$#,0.00\", \"en-US\" )",
        "LEFT ( \"DAX is so cool!\" )",
        "LEFT ( \"DAX is so cool!\", 3 )",
        "LEN ( \"DAX is so cool!\" )",
        "LOWER ( \"DAX is SO cool!\" )",
        "MID ( \"DAX is so cool!\", 11, 4 )",
        "REPLACE ( \"DAX is so cool!\", 11, 4, \"fantastic\" )",
        "REPT ( \"?\", 3 )",
        "RIGHT ( \"DAX is so cool!\" )",
        "RIGHT ( \"DAX is so cool!\", 5 )",
        "SEARCH ( \"red\", \"Contoso 2G MP3 Player E200 Red\" )",
        "SEARCH ( \"red\", \"Contoso 2G MP3 Player E200 Red\", 1 )",
        "SEARCH ( \"red\", \"Contoso 2G MP3 Player E200 Red\", 1, BLANK () )",
        "SUBSTITUTE ( \"Contoso 2G MP3 Player E200 Red\", \"MP3 Player\", \"Zune-like player\" )",
        "SUBSTITUTE ( \"Contoso 2G MP3 Player E200 Red\", \"2\", \"?\", 1 )",
        "TOCSV ( Customer )",
        "TOCSV ( Customer, 3 )",
        "TOCSV ( Customer, 3, \",\" )",
        "TOCSV ( Customer, 3, \",\", FALSE )",
        "TOJSON ( Customer )",
        "TOJSON ( Customer, 3 )",
        "TRIM ( \"  DAX is          awesome !!!    \" )",
        "UNICHAR ( 9473 )",
        "UNICODE ( \"A\" )",
        "UPPER ( \"dax is so cool!\" )",
        "VALUE ( \"123.45678\" )",
    ] {
        assert_valid(src);
    }

    for src in [
        "COMBINEVALUES ( )",
        "COMBINEVALUES ( \",\" )",
        "COMBINEVALUES ( \",\", \"a\" )",
        "CONCATENATE ( )",
        "CONCATENATE ( \"a\" )",
        "CONCATENATE ( \"a\", \"b\", \"c\" )",
        "CONCATENATEX ( )",
        "CONCATENATEX ( Sales )",
        "CONCATENATEX ( Sales, Sales[Product], \", \", Sales[Amount], TRUE, FALSE )",
        "EXACT ( \"a\" )",
        "EXACT ( \"a\", \"b\", \"c\" )",
        "FIND ( \"a\" )",
        "FIND ( \"a\", \"b\", 1, BLANK (), 2 )",
        "FIXED ( )",
        "FIXED ( 1, 2, FALSE, TRUE )",
        "FORMAT ( 1 )",
        "FORMAT ( 1, \"x\", \"en-US\", TRUE )",
        "LEFT ( )",
        "LEFT ( \"a\", 1, 2 )",
        "LEN ( )",
        "LEN ( \"a\", \"b\" )",
        "LOWER ( )",
        "LOWER ( \"a\", \"b\" )",
        "MID ( \"a\", 1 )",
        "MID ( \"a\", 1, 2, 3 )",
        "REPLACE ( \"a\", 1, 2 )",
        "REPLACE ( \"a\", 1, 2, \"b\", 3 )",
        "REPT ( \"a\" )",
        "REPT ( \"a\", 2, 3 )",
        "RIGHT ( )",
        "RIGHT ( \"a\", 1, 2 )",
        "SEARCH ( \"a\" )",
        "SEARCH ( \"a\", \"b\", 1, BLANK (), 2 )",
        "SUBSTITUTE ( \"a\", \"b\" )",
        "SUBSTITUTE ( \"a\", \"b\", \"c\", 1, 2 )",
        "TOCSV ( )",
        "TOCSV ( Customer, 1, \",\", TRUE, FALSE )",
        "TOJSON ( )",
        "TOJSON ( Customer, 1, 2 )",
        "TRIM ( )",
        "TRIM ( \"a\", \"b\" )",
        "UNICHAR ( )",
        "UNICHAR ( 1, 2 )",
        "UNICODE ( )",
        "UNICODE ( \"a\", \"b\" )",
        "UPPER ( )",
        "UPPER ( \"a\", \"b\" )",
        "VALUE ( )",
        "VALUE ( \"1\", \"2\" )",
    ] {
        let errors = assert_invalid_kind(src, ValidationErrorKind::Arity);
        assert_eq!(errors.len(), 1, "{src:?} should report exactly one arity error: {errors:?}");
    }
}

#[test]
fn every_documented_finite_arity_boundary_is_enforced() {
    for spec in text_catalog().iter().filter(|spec| spec.max_args != usize::MAX) {
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

    let combinevalues = spec("COMBINEVALUES");
    for arity in 0..combinevalues.min_args {
        let arguments = vec!["\"x\""; arity].join(", ");
        let src = format!("COMBINEVALUES ( {arguments} )");
        assert_invalid_kind(&src, ValidationErrorKind::Arity);
    }
    for arity in [combinevalues.min_args, combinevalues.min_args + 1, combinevalues.min_args + 14] {
        let arguments = vec!["\"x\""; arity].join(", ");
        let src = format!("COMBINEVALUES ( {arguments} )");
        let errors = expression_errors(&src);
        assert!(errors.iter().all(|error| error.kind != ValidationErrorKind::Arity), "{src:?} is inside the variadic range: {errors:?}");
    }
}

#[test]
fn validate_call_reports_arity_without_expression_spans() {
    let errors = call_errors("MID ( \"a\", 1 )");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ValidationErrorKind::Arity);
    assert_eq!(errors[0].function, "MID");
    assert_eq!(errors[0].span.start, errors[0].span.end);

    assert!(call_errors("CONCATENATEX ( Sales, Sales[Product], \", \" )").is_empty());
    assert!(call_errors("TOJSON ( Customer, 3 )").is_empty());
    assert!(call_errors("FIND ( \"a\", \"b\", 1, BLANK () )").is_empty());
}

#[test]
fn table_parameters_reject_obvious_scalars_but_accept_dynamic_tables() {
    for src in [
        "TOCSV ( 1 )",
        "TOCSV ( \"Customer\" )",
        "TOCSV ( 1 + 1, 3 )",
        "TOJSON ( -1 )",
        "TOJSON ( TRUE )",
        "CONCATENATEX ( 1, Sales[Product] )",
        "CONCATENATEX ( \"Sales\", Sales[Product] )",
    ] {
        assert_invalid_kind(src, ValidationErrorKind::ArgumentKind);
    }

    for src in [
        "TOCSV ( Customer )",
        "TOCSV ( FILTER ( Customer, Customer[CountryRegion] = \"Asia\" ), 5 )",
        "TOJSON ( Customer )",
        "TOJSON ( VALUES ( Customer[CountryRegion] ) )",
        "CONCATENATEX ( Customer, Customer[Customer Name] )",
        "CONCATENATEX ( VALUES ( Customer[CountryRegion] ), Customer[CountryRegion], \", \" )",
    ] {
        assert_valid(src);
    }
}

#[test]
fn text_parameters_are_not_statically_restricted() {
    // Text and number positions accept any expression because their static type is not
    // decidable; only Column and Table positions are checked.
    for src in [
        "LEN ( Sales[Amount] )",
        "LOWER ( 42 )",
        "LEFT ( Sales[Product], Sales[Quantity] )",
        "FIND ( Sales[Code], Sales[Name] )",
        "REPT ( \"-\", Sales[Quantity] )",
        "FORMAT ( Sales[Amount], Sales[FormatString] )",
        "VALUE ( Sales[Amount] )",
        "UNICHAR ( Sales[Quantity] )",
    ] {
        assert_valid(src);
    }
}
