//! Formula dialects: the words and separators of a formula language at the editor boundary.
//! Each language's own cases live in its `<language>_input.rs`; these hold for every language.
use gridcraft_core::CellError;
use gridcraft_formula::FormulaLocale::{self, Es};
use gridcraft_formula::{parse, parse_input, print, print_input};

/// Reads `src` typed in `locale` and prints it in canonical syntax.
fn canonical(src: &str, locale: FormulaLocale) -> String {
    print(&parse_input(src, locale, |_| false).unwrap_or_else(|e| panic!("{locale:?} `{src}`: {e}")))
}

/// Shows a canonical formula the way `locale` writes it.
fn shown(canonical: &str, locale: FormulaLocale) -> String {
    print_input(&parse(canonical).unwrap(), locale, |_| false).unwrap()
}

/// Every language with its own formula words (all but English).
fn localized() -> impl Iterator<Item = FormulaLocale> {
    FormulaLocale::ALL.into_iter().filter(|l| *l != FormulaLocale::En)
}

/// Errors, booleans and structured-reference specifiers read and show in the language's words.
#[test]
fn errors_booleans_and_table_specifiers_are_spelled_in_the_language() {
    for (local, english) in [
        ("SI(VERDADERO;BUSCARV(A2;D:E;2;FALSO);#N/D)", "IF(TRUE,VLOOKUP(A2,D:E,2,FALSE),#N/A)"),
        ("CONTAR.SI(Tabla1[[#Encabezados];[Col1]];\">1,5\")", "COUNTIF(Tabla1[[#Headers],[Col1]],\">1,5\")"),
        ("SUMA(Tabla1[#Todo])+SUMA(Tabla1[[#Esta fila];[a]:[b]])", "SUM(Tabla1[#All])+SUM(Tabla1[[#This Row],[a]:[b]])"),
        ("SUMA({1\\2;3,5\\4})+SI(1/0;#¡VALOR!)", "SUM({1,2;3.5,4})+IF(1/0,#VALUE!)"),
    ] {
        assert_eq!(canonical(local, Es), english, "entry");
        assert_eq!(shown(english, Es), local, "display");
    }
}

/// Showing a canonical formula in any language and reading it back gives the same formula.
#[test]
fn every_language_round_trips_canonical_formulas() {
    for formula in [
        "SUM(1.5,-2.25,A1:B2 B2:C3)",
        r#"IF(AND(TRUE,NOT(FALSE)),"a,b;c",#DIV/0!)"#,
        r#"{1.5,TRUE;#N/A,"x;y"}"#,
        "SUM((A1,B1),Sheet2!A:A)",
        "Table1[[#This Row],[a]]+SUM(Table1[#Totals])+Table1[[#Headers],[a]:[b]]",
        "LET(x,1,LAMBDA(y,x+y)(2))",
        "XLOOKUP(1,A:A,B:B,#N/A)+IFNA(#GETTING_DATA,#SPILL!)",
        "SUM(1:3)+ROWS($2:$2)",
        "CEILING.MATH(2.5)+T.TEST(A1:A3,B1:B3,2,1)",
        "BYROW(A1:B2,SUM)+GROUPBY(A1:A3,B1:B3,AVERAGE)",
        // `SUMA` is no function here; read back, it must not become the binding `SUM`.
        "LET(SUM,42,ISERROR(SUMA))",
    ] {
        let expected = print(&parse(formula).unwrap());
        for locale in FormulaLocale::ALL {
            let local = shown(formula, locale);
            assert_eq!(canonical(&local, locale), expected, "{locale:?}: {formula} shown as {local}");
        }
    }
}

/// Local input also accepts English function names, booleans and errors, in any letter case.
#[test]
fn english_words_and_any_letter_case_are_accepted() {
    assert_eq!(canonical("SUM(1;2)+suma(3)", Es), "SUM(1,2)+SUM(3)");
    assert_eq!(canonical("{verdadero\\FALSE}+#n/d+#¡valor!", Es), "{TRUE,FALSE}+#N/A+#VALUE!");
    assert_eq!(canonical("SUMA(Tabla1[[#encabezados];[a]])", Es), "SUM(Tabla1[[#Headers],[a]])");
    assert_eq!(canonical("SUMA(Tabla1[ [#Datos];[a]])", Es), "SUM(Tabla1[[#Data],[a]])");
    // English separators are not the language's syntax.
    assert!(parse_input("SUMA(1.5;2)", Es, |_| false).is_err());
}

/// A function passed by name (BYROW(…,SUM)) is translated, unless a defined name takes the spelling.
#[test]
fn functions_passed_by_name_are_spelled_in_the_language() {
    // GROUPBY(…,SUM) and BYROW(…,SUM) take a worksheet function by name, saved as `_xleta.SUM`.
    assert_eq!(canonical("BYROW(A1:B3;suma)", Es), "BYROW(A1:B3,SUM)");
    assert_eq!(shown("_xlfn.BYROW(A1:B3,_xleta.SUM)", Es), "BYROW(A1:B3;SUMA)");
    // A local boolean word stays a boolean, and a defined name with the local spelling is the name.
    assert_eq!(canonical("SI(VERDADERO;1)", Es), "IF(TRUE,1)");
    let known = |name: &str| name == "SUMA";
    assert_eq!(print(&parse_input("BYROW(A1:B3;SUMA)", Es, known).unwrap()), "BYROW(A1:B3,SUMA)");
    assert_eq!(print_input(&parse("BYROW(A1:B3,SUM)").unwrap(), Es, known).unwrap(), "BYROW(A1:B3;SUM)");
}

/// Each table is sorted by canonical name, has no two functions sharing a local name, and only
/// TRUE()/FALSE() share a boolean's spelling.
#[test]
fn language_tables_are_sorted_unique_and_unambiguous() {
    for locale in localized() {
        let table = locale.translations();
        assert!(table.windows(2).all(|w| w[0].0 < w[1].0), "{locale:?}: not sorted by canonical name");
        let mut locals: Vec<String> = table.iter().map(|(_, local)| local.to_uppercase()).collect();
        locals.sort();
        let all = locals.len();
        locals.dedup();
        assert_eq!(locals.len(), all, "{locale:?}: two functions share a local name");
        for (en, local) in table {
            assert_ne!(en, local, "{locale:?}: {en} is listed though it keeps its English name");
            assert_eq!(locale.function_name(en), *local);
            assert_eq!(locale.canonical_name(local), *en);
            // The TRUE() and FALSE() functions are spelled like the booleans; nothing else may be.
            let upper = local.to_uppercase();
            let boolean = upper == locale.boolean_name(true) || upper == locale.boolean_name(false);
            assert_eq!(boolean, matches!(*en, "TRUE" | "FALSE"), "{locale:?}: {en} = {local}");
            // Every name can be typed (dots, digits, accents, mixed scripts) and shown back.
            assert_eq!(canonical(&format!("{}(A1)", local.to_lowercase()), locale), format!("{en}(A1)"));
            assert_eq!(shown(&format!("{en}(A1)"), locale), format!("{local}(A1)"));
        }
        for e in CellError::ALL {
            assert_eq!(canonical(locale.error_name(e), locale), e.as_str(), "{locale:?}");
        }
    }
}

/// Malformed local input is a parse error. A long flat chain is not nesting (#916), but nested
/// parentheses past the limit are.
#[test]
fn malformed_localized_input_is_an_error() {
    for bad in ["#¡VALOR", "Tabla1[[#Encabezados];", "SUMA({1\\}", "{SUMA(1)}"] {
        assert!(parse_input(bad, Es, |_| false).is_err(), "{bad}");
    }
    for locale in localized() {
        let word = locale.boolean_name(true);
        let chain = format!("{word}+").repeat(300) + "1";
        assert!(parse_input(&chain, locale, |_| false).is_ok(), "{locale:?}: a flat chain is not nesting");
        let nested = format!("{}{word}{}", "(".repeat(300), ")".repeat(300));
        assert!(parse_input(&nested, locale, |_| false).is_err(), "{locale:?}");
    }
}
