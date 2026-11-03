use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

const PT: FormulaLocale = FormulaLocale::PtBr;

fn input(src: &str) -> String {
    print(&parse_input(src, PT, |_| false).unwrap())
}

#[test]
fn portuguese_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=soma(A1:A10)", "SUM(A1:A10)"),
        ("MÉDIA(A1:A10)", "AVERAGE(A1:A10)"),
        ("média(A1:A10)", "AVERAGE(A1:A10)"),
        ("SE(A1>0;\"Sim\";\"Não\")", "IF(A1>0,\"Sim\",\"Não\")"),
        ("CONT.NÚM(A1:A10)", "COUNT(A1:A10)"),
        ("CONT.VALORES(A1:A10)", "COUNTA(A1:A10)"),
        ("SOMASE(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("SOMASES(C1:C9;A1:A9;\"x\";B1:B9;\">0\")", "SUMIFS(C1:C9,A1:A9,\"x\",B1:B9,\">0\")"),
        ("CONT.SE(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("CONT.SES(A1:A9;\"x\")", "COUNTIFS(A1:A9,\"x\")"),
        ("SOMARPRODUTO(A1:A3;B1:B3)", "SUMPRODUCT(A1:A3,B1:B3)"),
        ("PROCV(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("PROCH(A2;D1:F2;2;0)", "HLOOKUP(A2,D1:F2,2,0)"),
        ("PROCX(A2;D:D;E:E)", "XLOOKUP(A2,D:D,E:E)"),
        ("ÍNDICE(A1:B9;CORRESP(5;A1:A9;0);2)", "INDEX(A1:B9,MATCH(5,A1:A9,0),2)"),
        ("ARRED(A1*1,1;2)", "ROUND(A1*1.1,2)"),
        ("ARREDONDAR.PARA.CIMA(A1;0)+ARREDONDAR.PARA.BAIXO(A1;0)", "ROUNDUP(A1,0)+ROUNDDOWN(A1,0)"),
        ("TRUNCAR(A1)", "TRUNC(A1)"),
        ("MÁXIMO(A1:A3)-MÍNIMO(A1:A3)", "MAX(A1:A3)-MIN(A1:A3)"),
        ("E(A1>0;OU(B1;NÃO(C1)))", "AND(A1>0,OR(B1,NOT(C1)))"),
        ("SEERRO(1/0;0)", "IFERROR(1/0,0)"),
        ("CONCATENAR(\"a\";\"b\")", "CONCATENATE(\"a\",\"b\")"),
        ("HOJE()+AGORA()", "TODAY()+NOW()"),
        ("VPL(0,1;A1:A5)", "NPV(0.1,A1:A5)"),
        ("TIR(A1:A5;0,1)", "IRR(A1:A5,0.1)"),
        ("PGTO(0,08/12;10;10000)", "PMT(0.08/12,10,10000)"),
        ("VP(0,05;12;100)+VF(0,05;12;100)", "PV(0.05,12,100)+FV(0.05,12,100)"),
        ("=verdadeiro", "TRUE"),
        ("=falso", "FALSE"),
        ("VERDADEIRO()", "TRUE()"),
        // English names still work, with Portuguese separators.
        ("SUM(1;SOMA(2;3))", "SUM(1,SUM(2,3))"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn portuguese_does_not_read_spanish_aliases() {
    // `VERDADERO` and `SUMA` are Spanish: in Portuguese they stay names.
    assert_eq!(input("SUMA(1;2)+VERDADERO"), "SUMA(1,2)+VERDADERO");
    assert!(parse_input("{VERDADERO}", PT, |_| false).is_err());
}

#[test]
fn portuguese_arrays_references_and_text_are_preserved() {
    for (src, expected) in [
        ("SOMA({1\\2;3\\4};5)", "SUM({1,2;3,4},5)"),
        ("{VERDADEIRO\\FALSO;TRUE\\FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("\"SOMA;FALSO\"&\"a\"", "\"SOMA;FALSO\"&\"a\""),
        ("'SOMA;FALSO'!A1+SOMA!B2", "'SOMA;FALSO'!A1+SOMA!B2"),
        ("E1+E2", "E1+E2"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    assert!(parse_input("SOMA(1.5;2)", PT, |_| false).is_err());
}

#[test]
fn portuguese_display_round_trips() {
    let expr = parse(r#"IF(AND(A1>0,TRUE),SUM({1.5,2;3,4},0.25),AVERAGE(MAX(A1:A3),"a;b"))"#).unwrap();
    let shown = print_input(&expr, PT, |_| false).unwrap();
    assert_eq!(shown, r#"SE(E(A1>0;VERDADEIRO);SOMA({1,5\2;3\4};0,25);MÉDIA(MÁXIMO(A1:A3);"a;b"))"#);
    assert_eq!(print(&parse_input(&shown, PT, |_| false).unwrap()), print(&expr));
}

#[test]
fn portuguese_input_preserves_defined_and_local_names() {
    let known = |name: &str| ["SOMA", "FALSO"].iter().any(|n| n.eq_ignore_ascii_case(name));
    let expr = parse_input("SOMA(2;3)+FALSO+SE(VERDADEIRO;4;5)", PT, known).unwrap();
    assert_eq!(print(&expr), "SOMA(2,3)+FALSO+IF(TRUE,4,5)");
    assert_eq!(input("LET(SOMA;LAMBDA(x;x+1);SOMA(2))"), "LET(SOMA,LAMBDA(x,x+1),SOMA(2))");
    assert_eq!(input("LET(falso;7;falso)+FALSO"), "LET(falso,7,falso)+FALSE");
    // A defined name `SOMA` keeps SUM shown in English so the formula's meaning never changes.
    let expr = parse("SUM(1,2)").unwrap();
    assert_eq!(print_input(&expr, PT, known).unwrap(), "SUM(1;2)");
}
