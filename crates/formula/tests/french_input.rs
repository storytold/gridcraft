use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

#[test]
fn french_names_separators_booleans_and_arrays() {
    for (canonical, french) in [
        ("IF(TRUE,SUM(1.5,2.25),FALSE)", "SI(VRAI;SOMME(1,5;2,25);FAUX)"),
        ("VLOOKUP(A1,B:C,2,FALSE)", "RECHERCHEV(A1;B:C;2;FAUX)"),
        ("XLOOKUP(A1,B:B,C:C)", "RECHERCHEX(A1;B:B;C:C)"),
        ("STDEV.P(A1:A5)+VAR.P(A1:A5)+VARP(A1:A5)", "ECARTYPE.PEARSON(A1:A5)+VAR.P.N(A1:A5)+VAR.P(A1:A5)"),
        ("TODAY()+SUM({1.5,2;3,4})", "AUJOURDHUI()+SOMME({1,5\\2;3\\4})"),
        ("{TRUE,FALSE;FALSE,TRUE}", "{VRAI\\FAUX;FAUX\\VRAI}"),
        ("IF(TRUE,,)", "SI(VRAI;;)"),
        ("SUM((A1:A3,C1:C3))", "SOMME((A1:A3;C1:C3))"),
        ("PERCENTOF(A1,A1:A3)", "POURCENTAGEDE(A1;A1:A3)"),
        ("DROP(A1:C3,1)", "EXCLURE(A1:C3;1)"),
        ("GROUPBY(A1:A3,B1:B3,SUM)", "GROUPER.PAR(A1:A3;B1:B3;SOMME)"),
    ] {
        let expr = parse(canonical).unwrap();
        assert_eq!(print_input(&expr, FormulaLocale::Fr, |_| false).unwrap(), french);
        assert_eq!(print(&parse_input(french, FormulaLocale::Fr, |_| false).unwrap()), canonical);
    }
    assert_eq!(print(&parse_input("somme(1;2)+faux", FormulaLocale::Fr, |_| false).unwrap()), "SUM(1,2)+FALSE");
    assert!(parse("SOMME(1;2)").is_err());
    assert!(parse_input("SOMME(1.5;2)", FormulaLocale::Fr, |_| false).is_err());
    // Spanish literals must not be accepted as French array constants.
    assert!(parse_input("{VERDADERO}", FormulaLocale::Fr, |_| false).is_err());
}

#[test]
fn french_translation_preserves_user_text_names_and_scopes() {
    for canonical in [
        "LET(SOMME,LAMBDA(x,x+1),SUM(SOMME(2),FALSE))",
        "LET(FAUX,7,IF(FAUX,FAUX,TRUE))+FALSE",
        "LAMBDA(SOMME,SOMME(2))(LAMBDA(x,x+1))",
        "'SOMME;FAUX'!A1+SOMME[NB.SI]",
        "IF(A1=\"a,b;c\",CONCAT(\"SUM\",\"1.5\"),SUM(0.25,A1:A3 B2))",
        "_xludf.SUM(1.5)",
    ] {
        let expr = parse(canonical).unwrap();
        let known = |n: &str| n.eq_ignore_ascii_case("FAUX");
        let shown = print_input(&expr, FormulaLocale::Fr, known).unwrap();
        assert_eq!(print(&parse_input(&shown, FormulaLocale::Fr, known).unwrap()), print(&expr));
    }
    let deep = "NON(".repeat(300) + "VRAI" + &")".repeat(300);
    assert!(parse_input(&deep, FormulaLocale::Fr, |_| false).is_err());
}

#[test]
fn french_localization_keeps_upstream_long_chain_and_unicode_name_support() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let canonical = vec!["1"; 4096].join("+");
            let expr = parse_input(&canonical, FormulaLocale::Fr, |_| false).unwrap();
            assert_eq!(print_input(&expr, FormulaLocale::Fr, |_| false).unwrap(), canonical);
            let french = vec!["SOMME(1;2)"; 700].join("+");
            let expr = parse_input(&french, FormulaLocale::Fr, |_| false).unwrap();
            assert_eq!(print(&expr), vec!["SUM(1,2)"; 700].join("+"));
            assert_eq!(print_input(&expr, FormulaLocale::Fr, |_| false).unwrap(), french);
        })
        .unwrap()
        .join()
        .unwrap();
    let expr = parse_input("LET(ſomme;LAMBDA(x;x*2);SOMME(3))", FormulaLocale::Fr, |_| false).unwrap();
    assert_eq!(print(&expr), "LET(ſomme,LAMBDA(x,x*2),SOMME(3))");
    assert_eq!(print_input(&expr, FormulaLocale::Fr, |_| false).unwrap(), "LET(ſomme;LAMBDA(x;x*2);SOMME(3))");
}

#[test]
fn french_conditional_extrema_use_the_excel_names() {
    for (english, french) in [("MAXIFS", "MAX.SI.ENS"), ("MINIFS", "MIN.SI.ENS")] {
        assert_eq!(FormulaLocale::Fr.function_name(english), french);
        let expr = parse_input(&format!("{french}(A1:A3;B1:B3;\">0\")"), FormulaLocale::Fr, |_| false).unwrap();
        assert_eq!(print(&expr), format!("{english}(A1:A3,B1:B3,\">0\")"));
    }
}

#[test]
fn french_error_values_round_trip_without_rewriting_user_text() {
    use gridcraft_core::CellError;
    for (french, canonical) in [
        ("SI(ESTERREUR(A1);#VALEUR!;A1)", "IF(ISERROR(A1),#VALUE!,A1)"),
        ("SIERREUR(A1;#N/A)+#NOMBRE!", "IFERROR(A1,#N/A)+#NUM!"),
        ("{#NOM?\\#NUL!;#DIV/0!\\#REF!}", "{#NAME?,#NULL!;#DIV/0!,#REF!}"),
        ("#PROPAGATION!&#CALC!&#CHARGEMENT_DONNEES", "#SPILL!&#CALC!&#GETTING_DATA"),
        ("SI(VRAI;\"#VALUE!\";'#VALEUR!'!A1)", "IF(TRUE,\"#VALUE!\",'#VALEUR!'!A1)"),
        ("SOMME(A1#)", "SUM(A1#)"),
    ] {
        assert_eq!(print(&parse_input(french, FormulaLocale::Fr, |_| false).unwrap()), canonical);
        assert_eq!(print_input(&parse(canonical).unwrap(), FormulaLocale::Fr, |_| false).unwrap(), french);
    }
    for error in CellError::ALL {
        let canonical = error.as_str();
        let french = FormulaLocale::Fr.error_name(error);
        assert_eq!(FormulaLocale::Fr.parse_error(&french.to_ascii_lowercase()), Some(error));
        assert_eq!(print(&parse_input(canonical, FormulaLocale::Fr, |_| false).unwrap()), canonical);
        assert_eq!(print(&parse_input(french, FormulaLocale::Fr, |_| false).unwrap()), canonical);
        assert_eq!(print_input(&parse(canonical).unwrap(), FormulaLocale::En, |_| false).unwrap(), canonical);
        assert_eq!(print_input(&parse(canonical).unwrap(), FormulaLocale::Es, |_| false).unwrap(), canonical);
    }
    assert_eq!(FormulaLocale::Fr.error_name(CellError::Circ), "#CIRC!");
    for bad in ["#VALEUR", "#PROPAGATION", "#VALEUR!suffix", "#VALEUR!é"] {
        assert!(parse_input(bad, FormulaLocale::Fr, |_| false).is_err(), "{bad}");
    }
    assert!(parse("#VALEUR!").is_err());
    assert!(parse_input("#VALEUR!", FormulaLocale::Es, |_| false).is_err());
}
