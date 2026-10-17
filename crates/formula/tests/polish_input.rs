use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::Pl, |_| false).unwrap())
}

#[test]
fn polish_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=suma(A1:A10)", "SUM(A1:A10)"),
        ("ŚREDNIA(A1:A10)", "AVERAGE(A1:A10)"),
        ("=jeżeli(A1>0;\"Tak\";\"Nie\")", "IF(A1>0,\"Tak\",\"Nie\")"),
        ("ILE.LICZB(A1:A10)", "COUNT(A1:A10)"),
        ("SUMA.JEŻELI(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("LICZ.JEŻELI(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("WYSZUKAJ.PIONOWO(A2;D:E;2;FAŁSZ)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("X.WYSZUKAJ(A2;D:D;E:E)", "XLOOKUP(A2,D:D,E:E)"),
        ("JEŻELI.BŁĄD(1/0;0)", "IFERROR(1/0,0)"),
        ("ZŁĄCZ.TEKSTY(DŁ(A1);USUŃ.ZBĘDNE.ODSTĘPY(B1))", "CONCATENATE(LEN(A1),TRIM(B1))"),
        ("DZIŚ()", "TODAY()"),
        ("=prawda", "TRUE"),
        ("=fałsz", "FALSE"),
        ("=FAŁSZ", "FALSE"),
        ("PRAWDA()", "TRUE()"),
        ("ORAZ(LUB(PRAWDA;FAŁSZ);NIE(FAŁSZ))", "AND(OR(TRUE,FALSE),NOT(FALSE))"),
        ("SUMA(1;SUM(2;3);ŚREDNIA(4;6))", "SUM(1,SUM(2,3),AVERAGE(4,6))"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn polish_decimal_comma_arrays_and_display() {
    let pl = FormulaLocale::Pl;
    let expr = parse_input("JEŻELI(PRAWDA;SUMA(1,5;2,25);0)", pl, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(TRUE,SUM(1.5,2.25),0)");
    assert_eq!(print_input(&expr, pl, |_| false).unwrap(), "JEŻELI(PRAWDA;SUMA(1,5;2,25);0)");
    assert_eq!(input("SUMA({1\\2;3\\4};5)"), "SUM({1,2;3,4},5)");
    assert_eq!(input("{PRAWDA\\FAŁSZ}"), "{TRUE,FALSE}");
    let expr = parse(r#"IF(A1="a,b;c",VLOOKUP(0.25,D:E,2,FALSE),ROUND(A1,2))"#).unwrap();
    let shown = print_input(&expr, pl, |_| false).unwrap();
    assert_eq!(shown, r#"JEŻELI(A1="a,b;c";WYSZUKAJ.PIONOWO(0,25;D:E;2;FAŁSZ);ZAOKR(A1;2))"#);
    assert_eq!(print(&parse_input(&shown, pl, |_| false).unwrap()), print(&expr));
}

#[test]
fn polish_input_preserves_defined_names_and_files_stay_english() {
    let known = |name: &str| name.eq_ignore_ascii_case("SUMA");
    let expr = parse_input("SUMA(2;3)+JEŻELI(PRAWDA;4;5)", FormulaLocale::Pl, known).unwrap();
    assert_eq!(print(&expr), "SUMA(2,3)+IF(TRUE,4,5)");
    // File/API syntax ignores the Polish table.
    assert_eq!(print(&parse("SUMA(1,2)+FAŁSZ").unwrap()), "SUMA(1,2)+FAŁSZ");
    assert!(parse("{PRAWDA}").is_err());
    // Spanish keeps its own booleans.
    assert_eq!(print(&parse_input("=PRAWDA", FormulaLocale::Es, |_| false).unwrap()), "PRAWDA");
}

#[test]
fn polish_aliases_never_shadow_another_english_function() {
    for (en, local) in [("DATE", "DATA"), ("TEXT", "TEKST"), ("RAND", "LOS")] {
        assert_eq!(FormulaLocale::Pl.canonical_name(local), en);
        assert_eq!(FormulaLocale::Pl.function_name(en), local);
    }
    // English names typed in Polish mode still work.
    assert_eq!(input("SUM(1;2)"), "SUM(1,2)");
}
