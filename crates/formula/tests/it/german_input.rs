use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::De, |_| false).unwrap())
}

fn display(canonical: &str) -> String {
    print_input(&parse(canonical).unwrap(), FormulaLocale::De, |_| false).unwrap()
}

#[test]
fn german_names_and_separators() {
    for (src, expected) in [
        ("=summe(A1:A10)", "SUM(A1:A10)"),
        ("WENN(A1>0;\"Ja\";\"Nein\")", "IF(A1>0,\"Ja\",\"Nein\")"),
        ("SVERWEIS(A2;D:E;2;FALSCH)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("MITTELWERTWENNS(A1:A9;B1:B9;\">0\")", "AVERAGEIFS(A1:A9,B1:B9,\">0\")"),
        ("BETA.VERT(0,5;2;3;WAHR)", "BETA.DIST(0.5,2,3,TRUE)"),
        ("RENDITEFÄLL(A1;A2;A3;A4;A5)", "YIELDMAT(A1,A2,A3,A4,A5)"),
        ("SUMME(1,5;2,25)", "SUM(1.5,2.25)"),
        ("=wahr", "TRUE"),
        ("FALSCH()", "FALSE()"),
        // Names German Excel keeps in English stay as typed.
        ("MAX(1;2)+INDEX(A1:B2;1;2)", "MAX(1,2)+INDEX(A1:B2,1,2)"),
        // Spanish words mean nothing in German.
        ("SUMA(1;2)+VERDADERO", "SUMA(1,2)+VERDADERO"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn german_arrays_use_a_point_between_columns() {
    for (src, expected) in
        [("SUMME({1.2;3.4};5)", "SUM({1,2;3,4},5)"), ("{1,5.2,25}", "{1.5,2.25}"), ("{WAHR.FALSCH;TRUE.FALSE}", "{TRUE,FALSE;TRUE,FALSE}")]
    {
        assert_eq!(input(src), expected, "{src}");
    }
    assert!(parse_input("{SUMME(A1)}", FormulaLocale::De, |_| false).is_err());
}

#[test]
fn german_display_of_canonical_formulas() {
    for (canonical, expected) in [
        ("IF(A1>0,SUM(1.5,2),FALSE)", "WENN(A1>0;SUMME(1,5;2);FALSCH)"),
        ("VLOOKUP(A2,D:E,2,TRUE)", "SVERWEIS(A2;D:E;2;WAHR)"),
        ("{1.5,2;3,4}", "{1,5.2;3.4}"),
        ("MAX(1,2)", "MAX(1;2)"),
    ] {
        assert_eq!(display(canonical), expected, "{canonical}");
        assert_eq!(input(&display(canonical)), canonical, "{canonical} round trip");
    }
}
