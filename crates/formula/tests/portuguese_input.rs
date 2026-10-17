use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::PtBr, |_| false).unwrap())
}

#[test]
fn portuguese_aliases_and_separators_use_canonical_engine_syntax() {
    for (src, expected) in [
        ("=SOMA(A1:A10)", "SUM(A1:A10)"),
        ("MÉDIA(A1:A10)", "AVERAGE(A1:A10)"),
        ("SE(A1>0;\"Sim\";\"Não\")", "IF(A1>0,\"Sim\",\"Não\")"),
        ("CONT.SE(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("PROCV(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("PROCX(A2;D:D;E:E;\"Não encontrado\")", "XLOOKUP(A2,D:D,E:E,\"Não encontrado\")"),
        ("HOJE()", "TODAY()"),
        ("=VERDADEIRO", "TRUE"),
        ("=FALSO", "FALSE"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn portuguese_decimal_arrays_and_display_round_trip() {
    let locale = FormulaLocale::PtBr;
    let expr = parse_input("SE(VERDADEIRO;SOMA({1,5\\2,25;3\\4});0)", locale, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(TRUE,SUM({1.5,2.25;3,4}),0)");
    let shown = print_input(&expr, locale, |_| false).unwrap();
    assert_eq!(shown, "SE(VERDADEIRO;SOMA({1,5\\2,25;3\\4});0)");
    assert_eq!(print(&parse_input(&shown, locale, |_| false).unwrap()), print(&expr));

    let canonical = parse("SUM(XLOOKUP(A1,D:D,E:E),AVERAGE(A1:A3),FALSE)").unwrap();
    assert_eq!(print_input(&canonical, locale, |_| false).unwrap(), "SOMA(PROCX(A1;D:D;E:E);MÉDIA(A1:A3);FALSO)");
}

#[test]
fn portuguese_aliases_do_not_capture_defined_or_lambda_names() {
    let known = |name: &str| ["SOMA", "FALSO"].iter().any(|n| n.eq_ignore_ascii_case(name));
    let expr = parse_input("LET(SOMA;LAMBDA(x;x+1);SOMA(2))+FALSO+SE(VERDADEIRO;4;5)", FormulaLocale::PtBr, known).unwrap();
    assert_eq!(print(&expr), "LET(SOMA,LAMBDA(x,x+1),SOMA(2))+FALSO+IF(TRUE,4,5)");
}
