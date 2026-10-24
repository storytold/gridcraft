use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::It, |_| false).unwrap())
}

#[test]
fn italian_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=somma(A1:A10)", "SUM(A1:A10)"),
        ("MEDIA(A1:A10)", "AVERAGE(A1:A10)"),
        ("SE(A1>0;\"Sì\";\"No\")", "IF(A1>0,\"Sì\",\"No\")"),
        ("CONTA.NUMERI(A1:A10)", "COUNT(A1:A10)"),
        ("CONTA.VALORI(A1:A10)", "COUNTA(A1:A10)"),
        ("CONTA.VUOTE(A1:A10)", "COUNTBLANK(A1:A10)"),
        ("SOMMA.SE(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("CONTA.SE(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("CONTA.PIÙ.SE(A1:A10;\">0\";B1:B10;\">1\")", "COUNTIFS(A1:A10,\">0\",B1:B10,\">1\")"),
        ("PIÙ.SE(VERO;1;FALSO;2)", "IFS(TRUE,1,FALSE,2)"),
        ("CERCA.VERT(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("OGGI()", "TODAY()"),
        ("=vero", "TRUE"),
        ("=falso", "FALSE"),
        ("VERO()", "TRUE()"),
        ("FALSO()", "FALSE()"),
        ("SOMMA(1;SUM(2;3);MEDIA(4;6))", "SUM(1,SUM(2,3),AVERAGE(4,6))"),
        ("SE(VERO;;)", "IF(TRUE,,)"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn italian_aliases_are_case_and_accent_insensitive() {
    for (src, expected) in [
        ("val.errore(A1)", "ISERROR(A1)"),
        ("VAL.ERRORE(A1)", "ISERROR(A1)"),
        ("val.vuoto(A1)", "ISBLANK(A1)"),
        ("media.più.se(A1:A9;\">0\")", "AVERAGEIFS(A1:A9,\">0\")"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn italian_distinct_rounding_and_parity_aliases() {
    for (src, expected) in [
        ("PARI(3)", "EVEN(3)"),
        ("DISPARI(3)", "ODD(3)"),
        ("ARROTONDA.PER.ECC(1,2;1)", "ROUNDUP(1.2,1)"),
        ("ARROTONDA.PER.DIF(1,2;1)", "ROUNDDOWN(1.2,1)"),
        ("ARROTONDA.ECCESSO(1,2)", "CEILING(1.2)"),
        ("ARROTONDA.DIFETTO(1,2)", "FLOOR(1.2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn italian_input_preserves_reference_and_array_syntax() {
    for (src, expected) in [
        ("SOMMA({1\\2;3\\4};5)", "SUM({1,2;3,4},5)"),
        ("{VERO\\FALSO;TRUE\\FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("SUM((A1:A3;C1:C3))", "SUM((A1:A3,C1:C3))"),
        ("\"SOMMA;FALSO\"&\"a\"\"b\"", "\"SOMMA;FALSO\"&\"a\"\"b\""),
        ("'SOMMA;FALSO'!A1+SOMMA!B2+SOMMA!FALSO", "'SOMMA;FALSO'!A1+SOMMA!B2+SOMMA!FALSO"),
        ("SOMMA[CONTA.SE]+FALSO[SE]+CONTA", "SOMMA[CONTA.SE]+FALSO[SE]+CONTA"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    for bad in [r"SOMMA({1;2\3})", "{SOMMA(A1)}", "{FALSO;}"] {
        assert!(parse_input(bad, FormulaLocale::It, |_| false).is_err(), "{bad}");
    }
}

#[test]
fn locale_controls_decimal_list_array_separators_and_display() {
    let it = FormulaLocale::It;
    let en = FormulaLocale::En;
    let expr = parse_input("SE(VERO;SOMMA(1,5;2,25);0)", it, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(TRUE,SUM(1.5,2.25),0)");
    assert_eq!(print_input(&expr, it, |_| false).unwrap(), "SE(VERO;SOMMA(1,5;2,25);0)");
    assert_eq!(print_input(&expr, en, |_| false).unwrap(), print(&expr));
    assert!(parse_input("SUM(1;2)", en, |_| false).is_err());
    assert_eq!(print(&parse_input("SOMMA(1,2)+FALSO", en, |_| false).unwrap()), "SOMMA(1,2)+FALSO");
    assert!(parse_input("SOMMA(1.5;2)", it, |_| false).is_err());
    let expr = parse("SUM({1.5,2.25;3,4})+SUM((A1:A3,C1:C3))").unwrap();
    let shown = print_input(&expr, it, |_| false).unwrap();
    assert_eq!(print(&parse_input(&shown, it, |_| false).unwrap()), print(&expr));
    // Displaying in Italian localises booleans and function names, but a bound name wins.
    let expr = parse("LET(SOMMA,LAMBDA(x,x+1),SUM(SOMMA(2),FALSE))").unwrap();
    let shown = print_input(&expr, it, |n| n == "FALSO").unwrap();
    assert_eq!(shown, "LET(SOMMA;LAMBDA(x;x+1);SUM(SOMMA(2);FALSE))");
    assert_eq!(print(&parse_input(&shown, it, |n| n == "FALSO").unwrap()), print(&expr));
}

#[test]
fn italian_input_preserves_defined_and_local_names() {
    let known = |name: &str| ["SOMMA", "FALSO"].iter().any(|n| n.eq_ignore_ascii_case(name));
    let expr = parse_input("SOMMA(2;3)+FALSO+SE(VERO;4;5)", FormulaLocale::It, known).unwrap();
    assert_eq!(print(&expr), "SOMMA(2,3)+FALSO+IF(TRUE,4,5)");
    for (src, expected) in [
        ("LET(FALSO;7;FALSO)", "LET(FALSO,7,FALSO)"),
        ("LET(falso;FALSO;SE(falso;1;2))", "LET(falso,FALSE,IF(falso,1,2))"),
        ("LET(SOMMA;LAMBDA(x;x+1);SOMMA(2))", "LET(SOMMA,LAMBDA(x,x+1),SOMMA(2))"),
        ("LET(falso;7;valore;FALSO;valore)", "LET(falso,7,valore,FALSO,valore)"),
        ("LAMBDA(FALSO;FALSO)(7)", "LAMBDA(FALSO,FALSO)(7)"),
        ("LAMBDA(SOMMA;SOMMA(2))(LAMBDA(x;x+1))", "LAMBDA(SOMMA,SOMMA(2))(LAMBDA(x,x+1))"),
        ("LET(FALSO;7;FALSO)+FALSO", "LET(FALSO,7,FALSO)+FALSE"),
        ("LET(FALSO;7;LAMBDA(x;FALSO+x)(1))", "LET(FALSO,7,LAMBDA(x,FALSO+x)(1))"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn italian_chi_square_aliases_stay_distinct() {
    for (src, expected) in [
        ("DISTRIB.CHI(1;2)", "CHIDIST(1,2)"),
        ("DISTRIB.CHI.QUAD(1;2;VERO)", "CHISQ.DIST(1,2,TRUE)"),
        ("INV.CHI(0,5;2)", "CHIINV(0.5,2)"),
        ("INV.CHI.QUAD(0,5;2)", "CHISQ.INV(0.5,2)"),
        ("TEST.CHI(A1:B2;D1:E2)", "CHITEST(A1:B2,D1:E2)"),
        ("TEST.CHI.QUAD(A1:B2;D1:E2)", "CHISQ.TEST(A1:B2,D1:E2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    // A stored CHISQ.DIST must display as DISTRIB.CHI.QUAD, not collapse to DISTRIB.CHI.
    let expr = parse("CHISQ.DIST(1,2,TRUE)").unwrap();
    assert_eq!(print_input(&expr, FormulaLocale::It, |_| false).unwrap(), "DISTRIB.CHI.QUAD(1;2;VERO)");
}
