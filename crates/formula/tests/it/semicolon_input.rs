//! Ukrainian (and other decimal-comma) entry: English names, `;` between arguments, decimal comma.
use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

const UK: FormulaLocale = FormulaLocale::Semicolon;

fn input(src: &str) -> String {
    print(&parse_input(src, UK, |_| false).unwrap())
}

fn shown(canonical: &str) -> String {
    print_input(&parse(canonical).unwrap(), UK, |_| false).unwrap()
}

#[test]
fn separators_are_localized_and_names_stay_english() {
    for (src, expected) in [
        ("SUM(1,5;2)", "SUM(1.5,2)"),
        ("IF(A1>0,5;\"так\";\"ні\")", "IF(A1>0.5,\"так\",\"ні\")"),
        ("VLOOKUP(A2;D:E;2;FALSE)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("PMT(0,08/12;10;10000)", "PMT(0.08/12,10,10000)"),
        ("ROUND(,5;0)", "ROUND(0.5,0)"),
        ("SUM({1\\2;3\\4})", "SUM({1,2;3,4})"),
        ("LAMBDA(x;x*1,5)(2)", "LAMBDA(x,x*1.5)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    // Spanish names and booleans are not aliases here: they stay names.
    assert_eq!(input("SUMA(1;2)"), "SUMA(1,2)");
    assert_eq!(input("VERDADERO"), "VERDADERO");
    // An English comma between arguments is a decimal comma here.
    assert_eq!(input("SUM(1,5)"), "SUM(1.5)");
}

#[test]
fn display_round_trips_through_entry() {
    for canonical in ["SUM(1.5,2)", "IF(TRUE,ROUND(A1*0.21,2),\"1,5\")", "SUM({1,2;3,4})", "TEXT(A1,\"0.00\")"] {
        let text = shown(canonical);
        assert_eq!(input(&text), canonical, "{canonical} shown as {text}");
    }
    assert_eq!(shown("SUM(1.5,2)"), "SUM(1,5;2)");
    assert_eq!(shown("IF(TRUE,1,0)"), "IF(TRUE;1;0)");
    assert_eq!(shown("SUM({1,2;3,4})"), "SUM({1\\2;3\\4})");
    // Text literals keep their commas and points.
    assert_eq!(shown("TEXT(A1,\"0.00\")"), "TEXT(A1;\"0.00\")");
}

#[test]
fn hostile_input_is_an_error_not_a_panic() {
    for bad in ["SUM(1;", ";;;", "{1\\", "1,,5", "SUM(1;2", "\\", ",", "1,5,5"] {
        let _ = parse_input(bad, UK, |_| false);
    }
    let nested = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
    assert!(parse_input(&nested, UK, |_| false).is_err());
}
