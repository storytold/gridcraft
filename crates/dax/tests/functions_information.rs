//! Integration tests for the information catalog and its static validation.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

fn information_catalog() -> Vec<&'static FunctionSpec> {
    catalog().iter().filter(|spec| spec.category == FunctionCategory::Information).collect()
}

/// Name and documented arity, in dax.guide order. `usize::MAX` marks an open, repeatable
/// argument list.
const FUNCTIONS: [(&str, usize, usize); 114] = [
    ("COLUMNSTATISTICS", 0, 0),
    ("CONTAINS", 3, usize::MAX),
    ("CONTAINSROW", 2, usize::MAX),
    ("CONTAINSSTRING", 2, 2),
    ("CONTAINSSTRINGEXACT", 2, 2),
    ("CUSTOMDATA", 0, 0),
    ("EVALUATEANDLOG", 1, 3),
    ("HASONEFILTER", 1, 1),
    ("HASONEVALUE", 1, 1),
    ("INFO.ALTERNATEOFDEFINITIONS", 0, usize::MAX),
    ("INFO.ANNOTATIONS", 0, usize::MAX),
    ("INFO.ATTRIBUTEHIERARCHIES", 0, usize::MAX),
    ("INFO.ATTRIBUTEHIERARCHYSTORAGES", 0, usize::MAX),
    ("INFO.CALCDEPENDENCY", 0, usize::MAX),
    ("INFO.CALCULATIONGROUPS", 0, usize::MAX),
    ("INFO.CALCULATIONITEMS", 0, usize::MAX),
    ("INFO.CALENDARCOLUMNGROUPS", 0, usize::MAX),
    ("INFO.CALENDARCOLUMNREFERENCES", 0, usize::MAX),
    ("INFO.CALENDARS", 0, usize::MAX),
    ("INFO.CATALOGS", 0, usize::MAX),
    ("INFO.CHANGEDPROPERTIES", 0, usize::MAX),
    ("INFO.COLUMNPARTITIONSTORAGES", 0, usize::MAX),
    ("INFO.COLUMNPERMISSIONS", 0, usize::MAX),
    ("INFO.COLUMNS", 0, usize::MAX),
    ("INFO.COLUMNSTORAGES", 0, usize::MAX),
    ("INFO.CSDLMETADATA", 0, usize::MAX),
    ("INFO.CULTURES", 0, usize::MAX),
    ("INFO.DATACOVERAGEDEFINITIONS", 0, usize::MAX),
    ("INFO.DATASOURCES", 0, usize::MAX),
    ("INFO.DELTATABLEMETADATASTORAGES", 0, usize::MAX),
    ("INFO.DEPENDENCIES", 0, usize::MAX),
    ("INFO.DETAILROWSDEFINITIONS", 0, usize::MAX),
    ("INFO.DICTIONARYSTORAGES", 0, usize::MAX),
    ("INFO.EXCLUDEDARTIFACTS", 0, usize::MAX),
    ("INFO.EXPRESSIONS", 0, usize::MAX),
    ("INFO.EXTENDEDPROPERTIES", 0, usize::MAX),
    ("INFO.FORMATSTRINGDEFINITIONS", 0, usize::MAX),
    ("INFO.FUNCTIONS", 0, usize::MAX),
    ("INFO.GENERALSEGMENTMAPSEGMENTMETADATASTORAGES", 0, usize::MAX),
    ("INFO.GROUPBYCOLUMNS", 0, usize::MAX),
    ("INFO.HIERARCHIES", 0, usize::MAX),
    ("INFO.HIERARCHYSTORAGES", 0, usize::MAX),
    ("INFO.KPIS", 0, usize::MAX),
    ("INFO.LEVELS", 0, usize::MAX),
    ("INFO.LINGUISTICMETADATA", 0, usize::MAX),
    ("INFO.MEASURES", 0, usize::MAX),
    ("INFO.MODEL", 0, usize::MAX),
    ("INFO.OBJECTTRANSLATIONS", 0, usize::MAX),
    ("INFO.PARQUETFILESTORAGES", 0, usize::MAX),
    ("INFO.PARTITIONS", 0, usize::MAX),
    ("INFO.PARTITIONSTORAGES", 0, usize::MAX),
    ("INFO.PERSPECTIVECOLUMNS", 0, usize::MAX),
    ("INFO.PERSPECTIVEHIERARCHIES", 0, usize::MAX),
    ("INFO.PERSPECTIVEMEASURES", 0, usize::MAX),
    ("INFO.PERSPECTIVES", 0, usize::MAX),
    ("INFO.PERSPECTIVETABLES", 0, usize::MAX),
    ("INFO.PROPERTIES", 0, usize::MAX),
    ("INFO.QUERYGROUPS", 0, usize::MAX),
    ("INFO.REFRESHPOLICIES", 0, usize::MAX),
    ("INFO.RELATEDCOLUMNDETAILS", 0, usize::MAX),
    ("INFO.RELATIONSHIPINDEXSTORAGES", 0, usize::MAX),
    ("INFO.RELATIONSHIPS", 0, usize::MAX),
    ("INFO.RELATIONSHIPSTORAGES", 0, usize::MAX),
    ("INFO.ROLEMEMBERSHIPS", 0, usize::MAX),
    ("INFO.ROLES", 0, usize::MAX),
    ("INFO.SEGMENTMAPSTORAGES", 0, usize::MAX),
    ("INFO.SEGMENTSTORAGES", 0, usize::MAX),
    ("INFO.STORAGEFILES", 0, usize::MAX),
    ("INFO.STORAGEFOLDERS", 0, usize::MAX),
    ("INFO.STORAGETABLECOLUMNS", 0, usize::MAX),
    ("INFO.STORAGETABLECOLUMNSEGMENTS", 0, usize::MAX),
    ("INFO.TABLEPERMISSIONS", 0, usize::MAX),
    ("INFO.TABLES", 0, usize::MAX),
    ("INFO.TABLESTORAGES", 0, usize::MAX),
    ("INFO.USERDEFINEDFUNCTIONS", 0, usize::MAX),
    ("INFO.VARIATIONS", 0, usize::MAX),
    ("INFO.VIEW.COLUMNS", 0, 0),
    ("INFO.VIEW.MEASURES", 0, 0),
    ("INFO.VIEW.RELATIONSHIPS", 0, 0),
    ("INFO.VIEW.TABLES", 0, 0),
    ("ISAFTER", 2, usize::MAX),
    ("ISBLANK", 1, 1),
    ("ISBOOLEAN", 1, 1),
    ("ISCROSSFILTERED", 1, 1),
    ("ISDATETIME", 1, 1),
    ("ISDECIMAL", 1, 1),
    ("ISDOUBLE", 1, 1),
    ("ISEMPTY", 1, 1),
    ("ISERROR", 1, 1),
    ("ISEVEN", 1, 1),
    ("ISFILTERED", 1, 1),
    ("ISINSCOPE", 1, 1),
    ("ISINT64", 1, 1),
    ("ISLOGICAL", 1, 1),
    ("ISNONTEXT", 1, 1),
    ("ISNUMBER", 1, 1),
    ("ISNUMERIC", 1, 1),
    ("ISODD", 1, 1),
    ("ISONORAFTER", 2, usize::MAX),
    ("ISSELECTEDMEASURE", 1, usize::MAX),
    ("ISSTRING", 1, 1),
    ("ISSUBTOTAL", 1, 1),
    ("ISTEXT", 1, 1),
    ("NAMEOF", 1, 3),
    ("SELECTEDMEASURE", 0, 0),
    ("SELECTEDMEASUREFORMATSTRING", 0, 0),
    ("SELECTEDMEASURENAME", 0, 0),
    ("TABLEOF", 1, 1),
    ("TEXTCONTAINS", 2, 3),
    ("TEXTSIMILARITY", 2, 3),
    ("USERCULTURE", 0, 0),
    ("USERNAME", 0, 0),
    ("USEROBJECTID", 0, 0),
    ("USERPRINCIPALNAME", 0, 0),
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

fn call_errors(src: &str) -> Vec<ValidationError> {
    let expr = parse(src);
    match &expr.kind {
        ExprKind::Call(call) => validate_call(call),
        other => panic!("{src:?} is not a call: {other:?}"),
    }
}

fn assert_valid(src: &str) {
    let errors = validate_expression(&parse(src));
    assert!(errors.is_empty(), "{src:?} should validate, got {errors:?}");
}

fn assert_arity_error(src: &str) {
    let errors = call_errors(src);
    assert!(errors.iter().any(|error| error.kind == ValidationErrorKind::Arity), "{src:?} should report an arity error, got {errors:?}");
}

fn assert_kind_error(src: &str) {
    let errors = call_errors(src);
    assert!(
        errors.iter().any(|error| error.kind == ValidationErrorKind::ArgumentKind),
        "{src:?} should report an argument-kind error, got {errors:?}"
    );
}

#[test]
fn catalog_has_every_information_function_exactly_once_with_documented_arity() {
    let specs = information_catalog();
    assert_eq!(specs.len(), FUNCTIONS.len());

    for (name, min_args, max_args) in FUNCTIONS {
        let matches: Vec<&FunctionSpec> = specs.iter().copied().filter(|spec| spec.name == name).collect();
        assert_eq!(matches.len(), 1, "{name} should appear exactly once");
        assert_eq!(matches[0].min_args, min_args, "{name} min arity");
        assert_eq!(matches[0].max_args, max_args, "{name} max arity");
        assert_eq!(matches[0].category, FunctionCategory::Information);
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
    assert_eq!(lookup("info.catalogs").map(|spec| spec.name), Some("INFO.CATALOGS"));
    assert_eq!(lookup("IsBlank").map(|spec| spec.name), Some("ISBLANK"));
    assert!(lookup("ISBLANKED").is_none());
    assert!(lookup("").is_none());
    assert!(lookup("ISBLANK ").is_none());
}

#[test]
fn every_record_has_consistent_signature_and_metadata() {
    for spec in information_catalog() {
        assert!(!spec.description.is_empty(), "{} description", spec.name);
        assert!(!spec.examples.is_empty(), "{} examples", spec.name);
        assert!(spec.min_args <= spec.max_args, "{} arity range", spec.name);
        assert!(!spec.is_iterator(), "{} is not an iterator", spec.name);
        assert!(spec.name.chars().all(|ch| !ch.is_ascii_lowercase()), "{} should be uppercase", spec.name);

        let required = spec.params.iter().filter(|param| !param.optional).count();
        let optional = spec.params.iter().filter(|param| param.optional).count();
        assert_eq!(required, spec.min_args, "{} required params", spec.name);
        if spec.max_args != usize::MAX {
            assert_eq!(spec.params.len(), spec.max_args, "{} descriptive params", spec.name);
            assert_eq!(optional, spec.max_args - spec.min_args, "{} optional params", spec.name);
        }

        assert!(!spec.overloads.is_empty(), "{} overloads", spec.name);
        assert!(spec.overload_for(spec.min_args).is_some(), "{} overload for min arity", spec.name);
        if spec.max_args != usize::MAX {
            assert!(spec.overload_for(spec.max_args).is_some(), "{} overload for max arity", spec.name);
        } else {
            assert!(spec.overloads.iter().any(|overload| overload.variadic), "{} open overload", spec.name);
        }
    }
}

#[test]
fn every_example_parses_and_validates() {
    for spec in information_catalog() {
        for example in spec.examples {
            let expr = parse(example);
            let call = match &expr.kind {
                ExprKind::Call(call) => call,
                other => panic!("{} example {example:?} is not a call: {other:?}", spec.name),
            };
            assert_eq!(call.name.text, spec.name, "example {example:?} calls the wrong function");
            let errors = validate_expression(&expr);
            assert!(errors.is_empty(), "{} example {example:?} should validate, got {errors:?}", spec.name);
        }
    }
}

#[test]
fn return_types_match_the_documented_results() {
    for (name, return_type) in [
        ("COLUMNSTATISTICS", ReturnType::Table),
        ("CONTAINS", ReturnType::Boolean),
        ("CONTAINSROW", ReturnType::Boolean),
        ("CONTAINSSTRING", ReturnType::Boolean),
        ("CONTAINSSTRINGEXACT", ReturnType::Boolean),
        ("CUSTOMDATA", ReturnType::String),
        ("EVALUATEANDLOG", ReturnType::Variant),
        ("HASONEFILTER", ReturnType::Boolean),
        ("HASONEVALUE", ReturnType::Boolean),
        ("INFO.CATALOGS", ReturnType::Table),
        ("INFO.VIEW.TABLES", ReturnType::Table),
        ("ISBLANK", ReturnType::Boolean),
        ("ISEMPTY", ReturnType::Boolean),
        ("ISERROR", ReturnType::Boolean),
        ("NAMEOF", ReturnType::String),
        ("SELECTEDMEASURE", ReturnType::Any),
        ("SELECTEDMEASUREFORMATSTRING", ReturnType::String),
        ("SELECTEDMEASURENAME", ReturnType::String),
        ("TABLEOF", ReturnType::Table),
        ("TEXTCONTAINS", ReturnType::Boolean),
        ("TEXTSIMILARITY", ReturnType::Decimal),
        ("USERCULTURE", ReturnType::String),
        ("USERNAME", ReturnType::String),
        ("USEROBJECTID", ReturnType::String),
        ("USERPRINCIPALNAME", ReturnType::String),
    ] {
        assert_eq!(spec(name).return_type, return_type, "{name} return type");
    }
}

#[test]
fn parameter_kinds_encode_what_each_predicate_inspects() {
    assert_eq!(spec("ISBLANK").params[0].kind, ParamKind::Any);
    assert_eq!(spec("ISEMPTY").params[0].kind, ParamKind::Table);
    assert_eq!(spec("HASONEVALUE").params[0].kind, ParamKind::Column);
    assert_eq!(spec("ISINSCOPE").params[0].kind, ParamKind::Column);
    assert_eq!(spec("ISCROSSFILTERED").params[0].kind, ParamKind::Reference);
    assert_eq!(spec("ISSELECTEDMEASURE").params[0].kind, ParamKind::Reference);
    assert_eq!(spec("NAMEOF").params[0].kind, ParamKind::Reference);
    assert_eq!(spec("TABLEOF").params[0].kind, ParamKind::Reference);
    assert_eq!(spec("CONTAINS").params[1].kind, ParamKind::Column);
    assert_eq!(spec("TEXTCONTAINS").params[0].kind, ParamKind::Column);
    assert_eq!(spec("ISEVEN").params[0].kind, ParamKind::Number);
}

#[test]
fn arity_boundaries_are_enforced() {
    assert_valid("ISBLANK ( 1 )");
    assert_arity_error("ISBLANK ()");
    assert_arity_error("ISBLANK ( 1, 2 )");

    assert_valid("ISNUMBER ( 1 )");
    assert_arity_error("ISNUMBER ()");
    assert_arity_error("ISNUMBER ( 1, 2 )");

    assert_valid("ISEMPTY ( Sales )");
    assert_arity_error("ISEMPTY ()");
    assert_arity_error("ISEMPTY ( Sales, Sales )");

    assert_valid("CONTAINS ( Product, Product[Color], \"Red\" )");
    assert_valid("CONTAINS ( Sales, Sales[ProductKey], 1, Sales[Quantity], 2 )");
    assert_arity_error("CONTAINS ( Product )");
    assert_arity_error("CONTAINS ( Product, Product[Color] )");

    assert_valid("CONTAINSROW ( Sales, Sales[ProductKey] )");
    assert_arity_error("CONTAINSROW ( Sales )");

    assert_valid("ISAFTER ( Sales[Year], 2008 )");
    assert_valid("ISAFTER ( Sales[Year], 2008, 1 )");
    assert_arity_error("ISAFTER ( Sales[Year] )");

    assert_valid("ISONORAFTER ( Sales[Year], 2008 )");
    assert_arity_error("ISONORAFTER ( Sales[Year] )");

    assert_valid("EVALUATEANDLOG ( 1 )");
    assert_valid("EVALUATEANDLOG ( 1, \"label\" )");
    assert_valid("EVALUATEANDLOG ( 1, \"label\", 5 )");
    assert_arity_error("EVALUATEANDLOG ()");
    assert_arity_error("EVALUATEANDLOG ( 1, \"label\", 5, 6 )");

    assert_valid("NAMEOF ( Sales[Amount] )");
    assert_valid("NAMEOF ( Sales[Amount], \"COLUMN\" )");
    assert_valid("NAMEOF ( Sales[Amount], \"COLUMN\", \"UNESCAPED\" )");
    assert_arity_error("NAMEOF ()");
    assert_arity_error("NAMEOF ( Sales[Amount], \"COLUMN\", \"UNESCAPED\", \"FULL\" )");

    assert_valid("TEXTSIMILARITY ( Product[Product Name], \"red\" )");
    assert_valid("TEXTSIMILARITY ( Product[Product Name], \"red\", \"FUZZYMATCHING\" )");
    assert_arity_error("TEXTSIMILARITY ( Product[Product Name] )");
    assert_arity_error("TEXTSIMILARITY ( Product[Product Name], \"red\", \"FUZZYMATCHING\", 1 )");

    assert_valid("SELECTEDMEASURE ()");
    assert_arity_error("SELECTEDMEASURE ( 1 )");

    assert_valid("INFO.CATALOGS ()");
    assert_valid("INFO.CATALOGS ( \"CATALOG_NAME\" )");
    assert_valid("INFO.CATALOGS ( \"CATALOG_NAME\", \"AdventureWorks\" )");
    assert_valid("INFO.VIEW.TABLES ()");
    assert_arity_error("INFO.VIEW.TABLES ( \"Name\" )");
}

#[test]
fn column_and_table_arguments_are_checked_statically() {
    assert_kind_error("CONTAINS ( Product, 1, \"Red\" )");
    assert_kind_error("CONTAINS ( 1, Product[Color], \"Red\" )");
    assert_kind_error("HASONEFILTER ( 1 )");
    assert_kind_error("HASONEVALUE ( 1 )");
    assert_kind_error("ISINSCOPE ( 1 )");
    assert_kind_error("ISFILTERED ( 1 )");
    assert_kind_error("ISSUBTOTAL ( 1 )");
    assert_kind_error("ISEMPTY ( 1 )");
    assert_kind_error("TEXTCONTAINS ( 1, \"red\" )");
    assert_kind_error("TEXTSIMILARITY ( 1, \"red\" )");

    assert_valid("ISEMPTY ( FILTER ( Sales, Sales[Quantity] > 0 ) )");
    assert_valid("CONTAINS ( Product, Product[Color], \"Red\" )");
}
