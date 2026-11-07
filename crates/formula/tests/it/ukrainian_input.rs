//! Ukrainian Excel has never shipped its own translated function-name table (unlike Russian):
//! every official `uk-ua` Microsoft support page for a function keeps the English name
//! ("Функція VLOOKUP", "Функція SUMIF", "Функція COUNT", "Функція NPV" - never a Ukrainian
//! word), and the separators do change (`;`, decimal `,`, `:` for array rows) - the same
//! scheme as Russian, confirmed independently for the first two, assumed rather than
//! confirmed for the array row separator specifically (no source says otherwise for
//! Ukrainian). See docs/localization-parity.md for the sourcing note. So `Uk`'s alias table
//! is empty on
//! purpose: these tests are about separators and about that table staying empty, not about
//! function-name aliases the way `russian_input.rs` and `spanish_input.rs` are.

use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::Uk, |_| false).unwrap())
}

#[test]
fn ukrainian_keeps_english_function_names_but_uses_local_separators() {
    for (src, expected) in [
        ("=SUM(A1:A10)", "SUM(A1:A10)"),
        ("IF(A1>0;\"Так\";\"Ні\")", "IF(A1>0,\"Так\",\"Ні\")"),
        ("VLOOKUP(A2;D:E;2;FALSE)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("SUM(1,5;2,25)", "SUM(1.5,2.25)"),
        ("SUM({1;2:3;4};5)", "SUM({1,2;3,4},5)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn ukrainian_display_round_trips_separators() {
    let uk = FormulaLocale::Uk;
    let expr = parse("IF(TRUE,SUM(1.5,2.25),0)").unwrap();
    let shown = print_input(&expr, uk, |_| false).unwrap();
    assert_eq!(shown, "IF(TRUE;SUM(1,5;2,25);0)");
    assert_eq!(print(&parse_input(&shown, uk, |_| false).unwrap()), print(&expr));
}

#[test]
fn ukrainian_does_not_translate_russian_function_names() {
    // The empty table is intentional, not just unpopulated: this locale layer doesn't
    // recognize "ЕСЛИ" as anything, Russian or otherwise, so it passes the name through
    // unchanged instead of canonicalizing it to IF. (Whether "ЕСЛИ" is a registered function
    // is a question for gridcraft-functions at evaluation time, not this crate.) A workbook
    // whose formulas were mangled by a Russian-locale tool needs those formulas actually
    // fixed to English - a locale flag alone can't paper over that for Ukrainian.
    let expr = parse_input("ЕСЛИ(1;2;3)", FormulaLocale::Uk, |_| false).unwrap();
    assert_eq!(print(&expr), "ЕСЛИ(1,2,3)");
}
