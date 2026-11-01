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

#[test]
fn polish_names_beyond_the_common_functions() {
    for (src, expected) in [
        ("FILTRUJ(A1:B9;A1:A9>0)", "FILTER(A1:B9,A1:A9>0)"),
        ("SORTUJ.WEDŁUG(A1:A9;B1:B9)", "SORTBY(A1:A9,B1:B9)"),
        ("PODZIEL.TEKST(A1;\",\")", "TEXTSPLIT(A1,\",\")"),
        ("STOS.PION(A1:A2;B1:B2)", "VSTACK(A1:A2,B1:B2)"),
        ("BD.SUMA(A1:C9;\"Kwota\";E1:E2)", "DSUM(A1:C9,\"Kwota\",E1:E2)"),
        ("REGLINX.ETS(A1;B1:B9;C1:C9)", "FORECAST.ETS(A1,B1:B9,C1:C9)"),
        ("ROZKŁAD.NORMALNY.S(1)", "NORMSDIST(1)"),
        ("DATA.RÓŻNICA(A1;B1;\"d\")", "DATEDIF(A1,B1,\"d\")"),
        ("PRZEŁĄCZ(A1;1;\"a\";\"b\")", "SWITCH(A1,1,\"a\",\"b\")"),
        ("WIERSZAMI(A1:B2;LAMBDA(r;SUMA(r)))", "BYROW(A1:B2,LAMBDA(r,SUM(r)))"),
        // Functions Polish Excel keeps in English.
        ("MAX(COS(1);MOD(5;2))", "MAX(COS(1),MOD(5,2))"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn polish_error_values() {
    for (pl, en) in [
        ("#ZERO!", "#NULL!"),
        ("#DZIEL/0!", "#DIV/0!"),
        ("#ARG!", "#VALUE!"),
        ("#ADR!", "#REF!"),
        ("#NAZWA?", "#NAME?"),
        ("#LICZBA!", "#NUM!"),
        ("#N/D", "#N/A"),
        ("#POBIERANIE_DANYCH", "#GETTING_DATA"),
        ("#ROZLANIE!", "#SPILL!"),
        ("#OBL!", "#CALC!"),
    ] {
        let expr = parse_input(&format!("JEŻELI.BŁĄD({pl};0)"), FormulaLocale::Pl, |_| false).unwrap();
        assert_eq!(print(&expr), format!("IFERROR({en},0)"));
        assert_eq!(print_input(&expr, FormulaLocale::Pl, |_| false).unwrap(), format!("JEŻELI.BŁĄD({pl};0)"));
    }
    // `#N/D!` (Microsoft's ERROR.TYPE page), lower case and English spellings are accepted too.
    assert_eq!(input("=#n/d!"), "#N/A");
    assert_eq!(input("=#dziel/0!"), "#DIV/0!");
    assert_eq!(input("=#DIV/0!"), "#DIV/0!");
    // Other languages keep the English spellings.
    assert!(parse_input("=#ARG!", FormulaLocale::Es, |_| false).is_err());
}

#[test]
fn polish_table_specifiers_and_separator() {
    for (pl, en) in [
        ("Tabela1[#Wszystko]", "Tabela1[#All]"),
        ("Tabela1[#dane]", "Tabela1[#Data]"),
        ("Tabela1[[#Nagłówki];[Kwota]]", "Tabela1[[#Headers],[Kwota]]"),
        ("Tabela1[[#Sumy];[Kwota]:[Cena]]", "Tabela1[[#Totals],[Kwota]:[Cena]]"),
        ("Tabela1[[#Ten wiersz];[Kwota]]", "Tabela1[@Kwota]"),
        ("Tabela1[[Kwota;netto]]", "Tabela1[Kwota;netto]"),
    ] {
        assert_eq!(input(&format!("SUMA({pl})")), format!("SUM({en})"), "{pl}");
    }
    let expr = parse("SUM(Tabela1[[#Headers],[Kwota]],Tabela1[#Data])").unwrap();
    assert_eq!(print_input(&expr, FormulaLocale::Pl, |_| false).unwrap(), "SUMA(Tabela1[[#Nagłówki];[Kwota]];Tabela1[#Dane])");
    // A column named with the separator round-trips.
    for src in ["SUM(Tabela1[Kwota;netto])", "SUM(Tabela1[@Kwota;netto])"] {
        let shown = print_input(&parse(src).unwrap(), FormulaLocale::Pl, |_| false).unwrap();
        assert_eq!(input(&shown), src, "{shown}");
    }
}

#[test]
fn polish_bindings_match_non_ascii_case() {
    // `FAŁSZ` refers to the LET name `fałsz`, not to the boolean.
    assert_eq!(input("LET(fałsz;7;FAŁSZ+1)"), "LET(fałsz,7,FAŁSZ+1)");
    assert_eq!(input("LAMBDA(już;JUŻ*2)(3)"), "LAMBDA(już,JUŻ*2)(3)");
    let expr = parse("LET(fałsz,7,FAŁSZ+1)").unwrap();
    assert_eq!(print_input(&expr, FormulaLocale::Pl, |_| false).unwrap(), "LET(fałsz;7;FAŁSZ+1)");
}
