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
    let deep = "FAUX+".repeat(300) + "VRAI";
    assert!(parse_input(&deep, FormulaLocale::Fr, |_| false).is_err());
}
