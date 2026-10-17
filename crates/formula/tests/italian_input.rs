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
        ("SOMMA.SE(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("somma.più.se(A1:A3;B1:B3;\">0\")", "SUMIFS(A1:A3,B1:B3,\">0\")"),
        ("CERCA.VERT(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("SE.ERRORE(1/0;0)", "IFERROR(1/0,0)"),
        ("E(VERO;O(FALSO;NON(FALSO)))", "AND(TRUE,OR(FALSE,NOT(FALSE)))"),
        ("OGGI()", "TODAY()"),
        ("=vero", "TRUE"),
        ("=falso", "FALSE"),
        ("SOMMA(1;SUM(2;3);MEDIA(4;6))", "SUM(1,SUM(2,3),AVERAGE(4,6))"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn italian_input_uses_decimal_commas_and_array_backslashes() {
    let it = FormulaLocale::It;
    let expr = parse_input("SE(VERO;SOMMA(1,5;2,25);0)", it, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(TRUE,SUM(1.5,2.25),0)");
    assert_eq!(print_input(&expr, it, |_| false).unwrap(), "SE(VERO;SOMMA(1,5;2,25);0)");
    assert_eq!(input("SOMMA({1\\2;3\\4};5)"), "SUM({1,2;3,4},5)");
    assert_eq!(input("{VERO\\FALSO}"), "{TRUE,FALSE}");
    assert!(parse_input("SOMMA(1.5;2)", it, |_| false).is_err());
    // Spanish words are not Italian aliases, and Italian ones are not Spanish.
    assert_eq!(input("SUMA(1;2)"), "SUMA(1,2)");
    assert_eq!(print(&parse_input("SOMMA(1;2)", FormulaLocale::Es, |_| false).unwrap()), "SOMMA(1,2)");
}

#[test]
fn italian_display_round_trips_and_keeps_defined_names() {
    let it = FormulaLocale::It;
    let expr = parse(r#"IF(A1="a,b;c",IFERROR(VLOOKUP(A1,B:C,2,FALSE),0.5),SUMIFS(A:A,B:B,">0"))"#).unwrap();
    let shown = print_input(&expr, it, |_| false).unwrap();
    assert_eq!(shown, r#"SE(A1="a,b;c";SE.ERRORE(CERCA.VERT(A1;B:C;2;FALSO);0,5);SOMMA.PIÙ.SE(A:A;B:B;">0"))"#);
    assert_eq!(print(&parse_input(&shown, it, |_| false).unwrap()), print(&expr));
    let known = |name: &str| name.eq_ignore_ascii_case("SOMMA");
    assert_eq!(print(&parse_input("SOMMA(2;3)+MEDIA(1;2)", it, known).unwrap()), "SOMMA(2,3)+AVERAGE(1,2)");
    assert_eq!(print_input(&parse("SUM(1,2)").unwrap(), it, known).unwrap(), "SUM(1;2)");
}
