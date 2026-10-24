//! Every function the engine knows has a French name, so French entry never falls back to
//! English for a built-in (a missing row would let `=SOMME(…)` work but `=NEWFN(…)` show its
//! English name). A function added later fails here until it has its French row.
use gridcraft_engine::{calc::SPECIAL_FUNCTIONS, formula::FormulaLocale};

#[test]
/// Every registered function and special form has a French row.
fn every_known_function_has_a_french_name() {
    let table = FormulaLocale::Fr.aliases();
    let has = |name: &str| table.iter().any(|(en, _)| en.eq_ignore_ascii_case(name));
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
/// No function has two French names, and no French name stands for two functions.
fn french_names_are_unique_both_ways() {
    let table = FormulaLocale::Fr.aliases();
    for (i, (en, fr)) in table.iter().enumerate() {
        for (en2, fr2) in &table[i + 1..] {
            assert!(!en.eq_ignore_ascii_case(en2), "two rows for {en}");
            assert!(!fr.eq_ignore_ascii_case(fr2), "{fr} names both {en} and {en2}");
        }
    }
}
