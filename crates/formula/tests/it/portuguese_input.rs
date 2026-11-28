use gridcraft_formula::{Expr, FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::PtBr, |_| false).unwrap())
}

#[test]
fn portuguese_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=soma(A1:A10)", "SUM(A1:A10)"),
        ("SOMA(A1:A10)", "SUM(A1:A10)"),
        ("MEDIA(A1:A10)", "AVERAGE(A1:A10)"),
        ("MÉDIA(A1:A10)", "AVERAGE(A1:A10)"),
        ("SE(A1>0;\"Sim\";\"Não\")", "IF(A1>0,\"Sim\",\"Não\")"),
        ("CONT.NUM(A1:A10)", "COUNT(A1:A10)"),
        ("CONT.VALORES(A1:A10)", "COUNTA(A1:A10)"),
        ("SOMASE(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("CONT.SE(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("PROCV(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("PROCH(A2;D1:E10;2;FALSO)", "HLOOKUP(A2,D1:E10,2,FALSE)"),
        ("PROCX(A2;D:D;E:E)", "XLOOKUP(A2,D:D,E:E)"),
        ("CORRESP(A2;D:D;0)", "MATCH(A2,D:D,0)"),
        ("INDICE(D:E;2;1)", "INDEX(D:E,2,1)"),
        ("ÍNDICE(D:E;2;1)", "INDEX(D:E,2,1)"),
        ("HOJE()", "TODAY()"),
        ("AGORA()", "NOW()"),
        ("TEMPO(12;30;0)", "TIME(12,30,0)"),
        ("HORA(0,5)", "HOUR(0.5)"),
        ("SIN(0,5)", "SIN(0.5)"),
        ("=verdadeiro", "TRUE"),
        ("=falso", "FALSE"),
        ("VERDADEIRO()", "TRUE()"),
        ("FALSO()", "FALSE()"),
        ("SOMA(1;SUM(2;3);MEDIA(4;6))", "SUM(1,SUM(2,3),AVERAGE(4,6))"),
        ("SE(VERDADEIRO;;)", "IF(TRUE,,)"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn portuguese_input_preserves_reference_and_array_syntax() {
    for (src, expected) in [
        ("SOMA({1\\2;3\\4};5)", "SUM({1,2;3,4},5)"),
        ("{VERDADEIRO\\FALSO;TRUE\\FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("SOMA((A1:A3;C1:C3))", "SUM((A1:A3,C1:C3))"),
        ("\"SOMA;FALSO\"&\"a\"\"b\"", "\"SOMA;FALSO\"&\"a\"\"b\""),
        ("'SOMA;FALSO'!A1+SOMA!B2+SOMA!FALSO", "'SOMA;FALSO'!A1+SOMA!B2+SOMA!FALSO"),
        ("SOMA[CONT.SE]+FALSO[SE]+CONT.NUM", "SOMA[CONT.SE]+FALSO[SE]+CONT.NUM"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    for bad in [r"SOMA({1;2\3})", "{SOMA(A1)}", "{FALSO;}"] {
        assert!(parse_input(bad, FormulaLocale::PtBr, |_| false).is_err(), "{bad}");
    }
    assert_eq!(print(&parse("SOMA(1,2)+FALSO").unwrap()), "SOMA(1,2)+FALSO");
    assert!(parse("SUM(1;2)").is_err());
    assert!(parse("{VERDADEIRO}").is_err());
    let nested = "(".repeat(300) + "1" + &")".repeat(300);
    assert!(parse_input(&nested, FormulaLocale::PtBr, |_| false).is_err());
    let deep = (0..300).fold(Expr::Number(1.0), |e, _| Expr::Paren(Box::new(e)));
    assert!(print_input(&deep, FormulaLocale::PtBr, |_| false).is_err());
}

#[test]
fn portuguese_input_accepts_flat_chains_as_long_as_excel() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let long_chain = "FALSO+".repeat(1360) + "VERDADEIRO";
            let english = "FALSE+".repeat(1360) + "TRUE";
            assert_eq!(input(&long_chain), english);
            assert_eq!(print_input(&parse(&english).unwrap(), FormulaLocale::PtBr, |_| false).unwrap(), long_chain);
            let sums = vec!["SOMA(1;2)"; 800].join("+");
            assert_eq!(input(&sums), vec!["SUM(1,2)"; 800].join("+"));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn portuguese_input_preserves_defined_and_local_names() {
    let known = |name: &str| ["SOMA", "FALSO"].iter().any(|n| n.eq_ignore_ascii_case(name));
    let expr = parse_input("SOMA(2;3)+FALSO+SE(VERDADEIRO;4;5)", FormulaLocale::PtBr, known).unwrap();
    assert_eq!(print(&expr), "SOMA(2,3)+FALSO+IF(TRUE,4,5)");
    for (src, expected) in [
        ("LET(FALSO;7;FALSO)", "LET(FALSO,7,FALSO)"),
        ("LET(falso;FALSO;SE(falso;1;2))", "LET(falso,FALSE,IF(falso,1,2))"),
        ("LET(SOMA;LAMBDA(x;x+1);SOMA(2))", "LET(SOMA,LAMBDA(x,x+1),SOMA(2))"),
        ("LET(falso;7;valor;FALSO;valor)", "LET(falso,7,valor,FALSO,valor)"),
        ("LAMBDA(FALSO;FALSO)(7)", "LAMBDA(FALSO,FALSO)(7)"),
        ("LAMBDA(SOMA;SOMA(2))(LAMBDA(x;x+1))", "LAMBDA(SOMA,SOMA(2))(LAMBDA(x,x+1))"),
        ("LET(FALSO;7;FALSO)+FALSO", "LET(FALSO,7,FALSO)+FALSE"),
        ("LET(FALSO;7;LAMBDA(x;FALSO+x)(1))", "LET(FALSO,7,LAMBDA(x,FALSO+x)(1))"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn portuguese_display_round_trips() {
    let pt = FormulaLocale::PtBr;
    let expr = parse_input("SE(VERDADEIRO;SOMA(1,5;2,25);0)", pt, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(TRUE,SUM(1.5,2.25),0)");
    assert_eq!(print_input(&expr, pt, |_| false).unwrap(), "SE(VERDADEIRO;SOMA(1,5;2,25);0)");

    let expr = parse("VLOOKUP(A1,B1:C10,2,FALSE)").unwrap();
    assert_eq!(print_input(&expr, pt, |_| false).unwrap(), "PROCV(A1;B1:C10;2;FALSO)");

    // TIME and HOUR stay distinct through input, display, and re-entry.
    let time_expr = parse_input("TEMPO(12;30;0)", pt, |_| false).unwrap();
    assert_eq!(print(&time_expr), "TIME(12,30,0)");
    assert_eq!(print_input(&time_expr, pt, |_| false).unwrap(), "TEMPO(12;30;0)");
    assert_eq!(print(&parse_input(&print_input(&time_expr, pt, |_| false).unwrap(), pt, |_| false).unwrap()), "TIME(12,30,0)");

    let hour_expr = parse_input("HORA(0,5)", pt, |_| false).unwrap();
    assert_eq!(print(&hour_expr), "HOUR(0.5)");
    assert_eq!(print_input(&hour_expr, pt, |_| false).unwrap(), "HORA(0,5)");
    assert_eq!(print(&parse_input(&print_input(&hour_expr, pt, |_| false).unwrap(), pt, |_| false).unwrap()), "HOUR(0.5)");
}
