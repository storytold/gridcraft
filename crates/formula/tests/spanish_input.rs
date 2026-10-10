use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::Es, |_| false).unwrap())
}

#[test]
fn spanish_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=suma(A1:A10)", "SUM(A1:A10)"),
        ("PROMEDIO(A1:A10)", "AVERAGE(A1:A10)"),
        ("SI(A1>0;\"Sí\";\"No\")", "IF(A1>0,\"Sí\",\"No\")"),
        ("CONTAR(A1:A10)", "COUNT(A1:A10)"),
        ("SUMAR.SI(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("CONTAR.SI(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("BUSCARV(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("HOY()", "TODAY()"),
        ("=verdadero", "TRUE"),
        ("=falso", "FALSE"),
        ("VERDADERO()", "TRUE()"),
        ("FALSO()", "FALSE()"),
        ("SUMA(1;SUM(2;3);PROMEDIO(4;6))", "SUM(1,SUM(2,3),AVERAGE(4,6))"),
        ("SI(VERDADERO;;)", "IF(TRUE,,)"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn spanish_input_preserves_reference_and_array_syntax() {
    for (src, expected) in [
        ("SUMA({1\\2;3\\4};5)", "SUM({1,2;3,4},5)"),
        ("{VERDADERO\\FALSO;TRUE\\FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("SUM((A1:A3;C1:C3))", "SUM((A1:A3,C1:C3))"),
        ("\"SUMA;FALSO\"&\"a\"\"b\"", "\"SUMA;FALSO\"&\"a\"\"b\""),
        ("'SUMA;FALSO'!A1+SUMA!B2+SUMA!FALSO", "'SUMA;FALSO'!A1+SUMA!B2+SUMA!FALSO"),
        ("SUMA[CONTAR.SI]+FALSO[SI]+CONTAR", "SUMA[CONTAR.SI]+FALSO[SI]+CONTAR"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    for bad in [r"SUMA({1;2\3})", "{SUMA(A1)}", "{FALSO;}"] {
        assert!(parse_input(bad, FormulaLocale::Es, |_| false).is_err(), "{bad}");
    }
    assert_eq!(print(&parse("SUMA(1,2)+FALSO").unwrap()), "SUMA(1,2)+FALSO");
    assert!(parse("SUM(1;2)").is_err());
    assert!(parse("{VERDADERO}").is_err());
    // Pratt parsing can produce a deep left-hand AST without deep parser recursion.
    let long_chain = "FALSO+".repeat(300) + "VERDADERO";
    assert!(parse_input(&long_chain, FormulaLocale::Es, |_| false).is_err());
    assert!(print_input(&parse(&long_chain).unwrap(), FormulaLocale::Es, |_| false).is_err());
}

#[test]
fn spanish_input_preserves_defined_and_local_names() {
    let known = |name: &str| ["SUMA", "FALSO"].iter().any(|n| n.eq_ignore_ascii_case(name));
    let expr = parse_input("SUMA(2;3)+FALSO+SI(VERDADERO;4;5)", FormulaLocale::Es, known).unwrap();
    assert_eq!(print(&expr), "SUMA(2,3)+FALSO+IF(TRUE,4,5)");
    for (src, expected) in [
        ("LET(FALSO;7;FALSO)", "LET(FALSO,7,FALSO)"),
        ("LET(falso;FALSO;SI(falso;1;2))", "LET(falso,FALSE,IF(falso,1,2))"),
        ("LET(SUMA;LAMBDA(x;x+1);SUMA(2))", "LET(SUMA,LAMBDA(x,x+1),SUMA(2))"),
        ("LET(falso;7;valor;FALSO;valor)", "LET(falso,7,valor,FALSO,valor)"),
        ("LAMBDA(FALSO;FALSO)(7)", "LAMBDA(FALSO,FALSO)(7)"),
        ("LAMBDA(SUMA;SUMA(2))(LAMBDA(x;x+1))", "LAMBDA(SUMA,SUMA(2))(LAMBDA(x,x+1))"),
        ("LET(FALSO;7;FALSO)+FALSO", "LET(FALSO,7,FALSO)+FALSE"),
        ("LET(FALSO;7;LAMBDA(x;FALSO+x)(1))", "LET(FALSO,7,LAMBDA(x,FALSO+x)(1))"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn locale_controls_decimal_list_array_separators_and_display() {
    let es = FormulaLocale::Es;
    let en = FormulaLocale::En;
    let expr = parse_input("SI(VERDADERO;SUMA(1,5;2,25);0)", es, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(TRUE,SUM(1.5,2.25),0)");
    assert_eq!(print_input(&expr, es, |_| false).unwrap(), "SI(VERDADERO;SUMA(1,5;2,25);0)");
    assert_eq!(print_input(&expr, en, |_| false).unwrap(), print(&expr));
    assert!(parse_input("SUM(1;2)", en, |_| false).is_err());
    assert_eq!(print(&parse_input("SUMA(1,2)+FALSO", en, |_| false).unwrap()), "SUMA(1,2)+FALSO");
    assert!(parse_input("SUMA(1.5;2)", es, |_| false).is_err());
    let expr = parse("SUM({1.5,2.25;3,4})+SUM((A1:A3,C1:C3))").unwrap();
    let shown = print_input(&expr, es, |_| false).unwrap();
    assert_eq!(print(&parse_input(&shown, es, |_| false).unwrap()), print(&expr));
    let expr = parse("LET(SUMA,LAMBDA(x,x+1),SUM(SUMA(2),FALSE))").unwrap();
    let shown = print_input(&expr, es, |n| n == "FALSO").unwrap();
    assert_eq!(shown, "LET(SUMA;LAMBDA(x;x+1);SUM(SUMA(2);FALSE))");
    assert_eq!(print(&parse_input(&shown, es, |n| n == "FALSO").unwrap()), print(&expr));
}

#[test]
fn spanish_display_round_trips_text_containing_separators() {
    let es = FormulaLocale::Es;
    let expr = parse(r#"IF(A1="a,b;c",CONCAT("1.5",",",";"),SUM(0.25,A1:A3 B2))"#).unwrap();
    let shown = print_input(&expr, es, |_| false).unwrap();
    assert_eq!(shown, r#"SI(A1="a,b;c";CONCAT("1.5";",";";");SUMA(0,25;A1:A3 B2))"#);
    assert_eq!(print(&parse_input(&shown, es, |_| false).unwrap()), print(&expr));
}
