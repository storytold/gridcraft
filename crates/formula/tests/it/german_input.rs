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

#[test]
fn german_error_values() {
    for (src, expected) in [
        ("WENNFEHLER(#WERT!;0)", "IFERROR(#VALUE!,0)"),
        ("ISTNV(#nv)", "ISNA(#N/A)"),
        ("{#BEZUG!.#ZAHL!;#DIV/0!.#NAME?}", "{#REF!,#NUM!;#DIV/0!,#NAME?}"),
        ("#DATEN_ABRUFEN", "#GETTING_DATA"),
        ("#ÜBERLAUF!", "#SPILL!"),
        ("#CALC!", "#CALC!"),
        // The canonical spelling is still understood.
        ("WENNFEHLER(#VALUE!;#N/A)", "IFERROR(#VALUE!,#N/A)"),
        // Text and sheet names are not error values.
        ("\"#WERT!\"&A1", "\"#WERT!\"&A1"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    for (canonical, expected) in [
        ("IFERROR(#VALUE!,#N/A)", "WENNFEHLER(#WERT!;#NV)"),
        ("{#REF!,#NUM!;#NULL!,#SPILL!}", "{#BEZUG!.#ZAHL!;#NULL!.#ÜBERLAUF!}"),
        ("#GETTING_DATA", "#DATEN_ABRUFEN"),
        ("#CALC!", "#CALC!"),
    ] {
        assert_eq!(display(canonical), expected, "{canonical}");
        assert_eq!(input(&display(canonical)), canonical, "{canonical} round trip");
    }
}

#[test]
fn other_languages_keep_canonical_error_values() {
    let es = print_input(&parse("IFERROR(#VALUE!,#N/A)").unwrap(), FormulaLocale::Es, |_| false).unwrap();
    assert_eq!(es, "IFERROR(#VALUE!;#N/A)");
    assert!(parse("#WERT!").is_err());
    assert!(parse_input("#WERT!", FormulaLocale::Es, |_| false).is_err());
}
