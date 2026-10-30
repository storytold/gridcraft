//! Every function the engine knows has a French name, so French entry never falls back to
//! English for a built-in by omission (a missing row would let `=SOMME(…)` work but show a new
//! function's English name). A function added later fails here until it has its French row, or
//! joins the names French Excel spells as in English.
use gridcraft_engine::{calc::SPECIAL_FUNCTIONS, formula::FormulaLocale};

/// Functions French Excel spells as in English, so they have no row in the French table.
const SAME_IN_FRENCH: &[&str] = &[
    "ABS",
    "ACOS",
    "ACOSH",
    "ACOT",
    "ACOTH",
    "AMORDEGRC",
    "AMORLINC",
    "ASC",
    "ASIN",
    "ASINH",
    "ATAN",
    "ATAN2",
    "ATANH",
    "AVERAGEA",
    "BAHTTEXT",
    "BASE",
    "BESSELI",
    "BESSELJ",
    "BESSELK",
    "BESSELY",
    "BYCOL",
    "BYROW",
    "CHISQ.TEST",
    "CODE",
    "COMBIN",
    "COMBINA",
    "CONCAT",
    "CONVERT",
    "COS",
    "COSH",
    "COT",
    "COTH",
    "CSC",
    "CSCH",
    "DATE",
    "DATEDIF",
    "DB",
    "DBCS",
    "DDB",
    "DECIMAL",
    "DELTA",
    "ERF",
    "ERFC",
    "EUROCONVERT",
    "EXACT",
    "EXP",
    "F.TEST",
    "FACT",
    "FACTDOUBLE",
    "FISHER",
    "GAMMA",
    "GAUSS",
    "INDEX",
    "INDIRECT",
    "ISOMITTED",
    "ISPMT",
    "JIS",
    "LAMBDA",
    "LENB",
    "LET",
    "LN",
    "LOG",
    "LOG10",
    "MAKEARRAY",
    "MAP",
    "MAX",
    "MAXA",
    "MIN",
    "MINA",
    "MINUTE",
    "MOD",
    "MODE",
    "N",
    "NA",
    "PEARSON",
    "PERMUTATIONA",
    "PHI",
    "PI",
    "QUARTILE",
    "QUOTIENT",
    "RADIANS",
    "REDUCE",
    "REGEXEXTRACT",
    "REGEXREPLACE",
    "REGEXTEST",
    "REPT",
    "SCAN",
    "SEC",
    "SECH",
    "SEQUENCE",
    "SIN",
    "SINH",
    "STDEVA",
    "STDEVPA",
    "SYD",
    "T",
    "T.TEST",
    "TAN",
    "TANH",
    "TRANSPOSE",
    "TRIMRANGE",
    "TYPE",
    "UNICODE",
    "UNIQUE",
    "USDOLLAR",
    "VAR",
    "VAR.S",
    "VARA",
    "VARPA",
    "VDB",
    "Z.TEST",
];

#[test]
fn every_known_function_has_a_french_name() {
    let table = FormulaLocale::Fr.function_table();
    let has = |name: &str| table.iter().any(|(en, _)| en.eq_ignore_ascii_case(name)) || SAME_IN_FRENCH.iter().any(|n| n.eq_ignore_ascii_case(name));
    let mut missing: Vec<String> = gridcraft_functions::all()
        .iter()
        .map(|spec| spec.name.to_string())
        .chain(SPECIAL_FUNCTIONS.iter().map(|name| name.to_string()))
        .filter(|name| !has(name))
        .collect();
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "functions without a French name: {missing:?}");
}

#[test]
fn names_spelled_alike_have_no_french_row() {
    let table = FormulaLocale::Fr.function_table();
    for name in SAME_IN_FRENCH {
        assert!(!table.iter().any(|(en, _)| en.eq_ignore_ascii_case(name)), "{name} is in both lists");
    }
}
