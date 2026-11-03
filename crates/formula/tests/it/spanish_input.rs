use gridcraft_formula::{Expr, FormulaLocale, parse, parse_input, print, print_input};

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
        ("VNA(0,1;A1:A5)", "NPV(0.1,A1:A5)"),
        ("TIR(A1:A5;0,1)", "IRR(A1:A5,0.1)"),
        ("PAGO(0,08/12;10;10000)", "PMT(0.08/12,10,10000)"),
        ("VA(0,05;12;100)", "PV(0.05,12,100)"),
        ("VF(0,05;12;100)", "FV(0.05,12,100)"),
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
    // Real nesting past the parser's limit is still refused.
    let nested = "(".repeat(300) + "1" + &")".repeat(300);
    assert!(parse_input(&nested, FormulaLocale::Es, |_| false).is_err());
    let deep = (0..300).fold(Expr::Number(1.0), |e, _| Expr::Paren(Box::new(e)));
    assert!(print_input(&deep, FormulaLocale::Es, |_| false).is_err());
}

#[test]
fn spanish_input_accepts_flat_chains_as_long_as_excel() {
    // A flat chain nests on its left without nesting in the text. Excel accepts 4,096
    // operands (8,192 characters); Spanish entry refused more than 256 (#916). On a 1 MiB
    // stack, as on the Windows main thread.
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let long_chain = "FALSO+".repeat(1360) + "VERDADERO";
            let english = "FALSE+".repeat(1360) + "TRUE";
            assert_eq!(input(&long_chain), english);
            assert_eq!(print_input(&parse(&english).unwrap(), FormulaLocale::Es, |_| false).unwrap(), long_chain);
            let sums = vec!["SUMA(1;2)"; 800].join("+");
            assert_eq!(input(&sums), vec!["SUM(1,2)"; 800].join("+"));
        })
        .unwrap()
        .join()
        .unwrap();
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

#[test]
fn local_names_match_aliases_in_any_case_like_the_evaluator() {
    // `ſi` (long s) is `SI` case-insensitively, so the call is the LAMBDA, not IF.
    assert_eq!(input("LET(ſi;LAMBDA(x;x*2);SI(3))"), "LET(ſi,LAMBDA(x,x*2),SI(3))");
    assert!(gridcraft_formula::same_name("ß", "ẞ"));
    assert!(gridcraft_formula::same_name("dług", "DŁUG"));
    assert!(gridcraft_formula::same_name("ſi", "SI"));
    assert!(gridcraft_formula::same_name("σοφός", "ΣΟΦΌΣ"));
    // Dotless ı is its own letter outside Turkic languages.
    assert!(!gridcraft_formula::same_name("ı", "i"));
    assert!(!gridcraft_formula::same_name("ı", "I"));
    assert!(!gridcraft_formula::same_name("a", "b"));
}

#[test]
fn every_spanish_function_name_round_trips() {
    let es = FormulaLocale::Es;
    let pairs = gridcraft_formula::spanish_function_names();
    for &(en, name) in pairs {
        // Typed in Spanish (any case) it is stored in English, and it is shown in Spanish again.
        for typed in [name.to_string(), name.to_lowercase()] {
            let expr = parse_input(&format!("={typed}(1;2,5)"), es, |_| false).unwrap_or_else(|e| panic!("{typed}: {e:?}"));
            assert_eq!(print(&expr), format!("{en}(1,2.5)"), "{typed}");
            assert_eq!(print_input(&expr, es, |_| false).unwrap(), format!("{name}(1;2,5)"), "{typed}");
        }
        assert_eq!(es.function_name(en), name);
        assert_eq!(es.canonical_name(name), en);
    }
    // One row per function, and no Spanish name stands for two functions.
    let mut english: Vec<&str> = pairs.iter().map(|p| p.0).collect();
    let mut spanish: Vec<String> = pairs.iter().map(|p| p.1.to_uppercase()).collect();
    english.sort_unstable();
    spanish.sort_unstable();
    let count = pairs.len();
    english.dedup();
    spanish.dedup();
    assert_eq!((english.len(), spanish.len()), (count, count));
    // A Spanish name that is also an English one stands for the function Spanish Excel means.
    assert_eq!(input("DECIMAL(1,5;1)"), "FIXED(1.5,1)");
    assert_eq!(input("CONV.DECIMAL(\"FF\";16)"), "DECIMAL(\"FF\",16)");
    // English names still work when they are not Spanish names of something else.
    assert_eq!(input("SUM(1;2)+BUSCARX(1;A:A;B:B)+año(HOY())"), "SUM(1,2)+XLOOKUP(1,A:A,B:B)+YEAR(TODAY())");
}

#[test]
fn a_function_given_by_name_as_an_argument_is_translated() {
    let es = FormulaLocale::Es;
    assert_eq!(input("MAP({1;4};RAIZ)"), "MAP({1;4},SQRT)");
    assert_eq!(input("BYROW(A1:B2;suma)"), "BYROW(A1:B2,SUM)");
    assert_eq!(print_input(&parse("MAP({1;4},SQRT)").unwrap(), es, |_| false).unwrap(), "MAP({1;4};RAIZ)");
    // Bindings, defined names, booleans and names that are no function's keep their spelling.
    assert_eq!(input("LET(RAIZ;LAMBDA(x;x);MAP(A1:A2;RAIZ))"), "LET(RAIZ,LAMBDA(x,x),MAP(A1:A2,RAIZ))");
    assert_eq!(print(&parse_input("MAP(A1:A2;RAIZ)", es, |n| n == "RAIZ").unwrap()), "MAP(A1:A2,RAIZ)");
    assert_eq!(input("SI(FALSO;total;1)"), "IF(FALSE,total,1)");
    let shown = print_input(&parse("LET(RAIZ,1,MAP(A1:A2,SQRT))").unwrap(), es, |_| false).unwrap();
    assert_eq!(shown, "LET(RAIZ;1;MAP(A1:A2;SQRT))");
    assert_eq!(print(&parse_input(&shown, es, |_| false).unwrap()), "LET(RAIZ,1,MAP(A1:A2,SQRT))");
    // Only where a function is expected: an ordinary argument or a bare name stays a name.
    assert_eq!(input("RAIZ+1"), "RAIZ+1");
    assert_eq!(input("SUMA(RAIZ;1)"), "SUM(RAIZ,1)");
    assert_eq!(input("MAP(RAIZ;RAIZ)"), "MAP(RAIZ,SQRT)");
    assert_eq!(input("LAMBDA(x;x)(RAIZ)"), "LAMBDA(x,x)(RAIZ)");
    for (src, expected) in [
        ("REDUCE(0;A1:A3;SUMA)", "REDUCE(0,A1:A3,SUM)"),
        ("SCAN(0;A1:A3;SUMA)", "SCAN(0,A1:A3,SUM)"),
        ("BYCOL(A1:B2;MAX)", "BYCOL(A1:B2,MAX)"),
        ("MAKEARRAY(2;2;PRODUCTO)", "MAKEARRAY(2,2,PRODUCT)"),
        ("AGRUPARPOR(A1:A9;B1:B9;PROMEDIO)", "GROUPBY(A1:A9,B1:B9,AVERAGE)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    assert_eq!(print_input(&parse("SUM(SQRT,1)").unwrap(), es, |_| false).unwrap(), "SUMA(SQRT;1)");
}

#[test]
fn a_shadowed_alias_never_reads_back_as_another_function() {
    let es = FormulaLocale::Es;
    // DECIMAL (the base converter) is CONV.DECIMAL in Spanish; with CONV.DECIMAL bound, the
    // English DECIMAL would read back as FIXED, so the formula is not shown in Spanish.
    for english in ["LET(CONV.DECIMAL,1,DECIMAL(\"FF\",16)+CONV.DECIMAL)", "LET(CONV.DECIMAL,1,MAP(A1:A2,DECIMAL))"] {
        assert!(print_input(&parse(english).unwrap(), es, |_| false).is_err(), "{english}");
    }
    // FIXED keeps its English name when DECIMAL is bound, and reads back as FIXED.
    let expr = parse("LET(DECIMAL,1,FIXED(1.5,1)&DECIMAL)").unwrap();
    let shown = print_input(&expr, es, |_| false).unwrap();
    assert_eq!(shown, "LET(DECIMAL;1;FIXED(1,5;1)&DECIMAL)");
    assert_eq!(print(&parse_input(&shown, es, |_| false).unwrap()), print(&expr));
    assert_eq!(es.function_name("COVARIANCE.P"), "COVARIANZA.P");
}
