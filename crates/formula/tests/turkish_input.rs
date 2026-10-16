use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::Tr, |_| false).unwrap())
}

fn tr_eq(a: &str, b: &str) -> bool {
    FormulaLocale::Tr.canonical_name(a) == FormulaLocale::Tr.canonical_name(b)
        || a.chars().zip(b.chars()).all(|(ca, cb)| match (ca, cb) {
            ('i' | 'İ', 'i' | 'İ') => true,
            ('ı' | 'I', 'ı' | 'I') => true,
            (x, y) => x.to_ascii_uppercase() == y.to_ascii_uppercase(),
        }) && a.chars().count() == b.chars().count()
}

#[test]
fn turkish_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=topla(A1:A10)", "SUM(A1:A10)"),
        ("ORTALAMA(A1:A10)", "AVERAGE(A1:A10)"),
        ("EĞER(A1>0;\"Evet\";\"Hayır\")", "IF(A1>0,\"Evet\",\"Hayır\")"),
        ("eğer(A1>0;\"Evet\";\"Hayır\")", "IF(A1>0,\"Evet\",\"Hayır\")"),
        ("SAY(A1:A10)", "COUNT(A1:A10)"),
        ("ETOPLA(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("EĞERSAY(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("DÜŞEYARA(A2;D:E;2;YANLIŞ)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("düşeyara(A2;D:E;2;yanlış)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("BUGÜN()", "TODAY()"),
        ("=doğru", "TRUE"),
        ("=yanlış", "FALSE"),
        ("DOĞRU()", "TRUE()"),
        ("YANLIŞ()", "FALSE()"),
        ("TOPLA(1;SUM(2;3);ORTALAMA(4;6))", "SUM(1,SUM(2,3),AVERAGE(4,6))"),
        ("EĞER(DOĞRU;;)", "IF(TRUE,,)"),
        ("ÇAPRAZARA(A1;B:B;C:C)", "XLOOKUP(A1,B:B,C:C)"),
        ("çaprazara(A1;B:B;C:C)", "XLOOKUP(A1,B:B,C:C)"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn turkish_input_preserves_reference_and_array_syntax() {
    for (src, expected) in [
        ("TOPLA({1\\2;3\\4};5)", "SUM({1,2;3,4},5)"),
        ("{DOĞRU\\YANLIŞ;TRUE\\FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("SUM((A1:A3;C1:C3))", "SUM((A1:A3,C1:C3))"),
        ("\"TOPLA;YANLIŞ\"&\"a\"\"b\"", "\"TOPLA;YANLIŞ\"&\"a\"\"b\""),
        ("'TOPLA;YANLIŞ'!A1+TOPLA!B2+TOPLA!YANLIŞ", "'TOPLA;YANLIŞ'!A1+TOPLA!B2+TOPLA!YANLIŞ"),
        ("TOPLA[EĞERSAY]+YANLIŞ[EĞER]+SAY", "TOPLA[EĞERSAY]+YANLIŞ[EĞER]+SAY"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    for bad in [r"TOPLA({1;2\3})", "{TOPLA(A1)}", "{YANLIŞ;}"] {
        assert!(parse_input(bad, FormulaLocale::Tr, |_| false).is_err(), "{bad}");
    }
    assert_eq!(print(&parse("TOPLA(1,2)+YANLIŞ").unwrap()), "TOPLA(1,2)+YANLIŞ");
    assert!(parse("SUM(1;2)").is_err());
    assert!(parse("{DOĞRU}").is_err());
    let long_chain = "YANLIŞ+".repeat(300) + "DOĞRU";
    assert!(parse_input(&long_chain, FormulaLocale::Tr, |_| false).is_err());
    assert!(print_input(&parse(&long_chain).unwrap(), FormulaLocale::Tr, |_| false).is_err());
}

#[test]
fn turkish_input_preserves_defined_and_local_names() {
    let known = |name: &str| ["TOPLA", "YANLIŞ"].iter().any(|n| tr_eq(n, name));
    let expr = parse_input("TOPLA(2;3)+YANLIŞ+EĞER(DOĞRU;4;5)", FormulaLocale::Tr, known).unwrap();
    assert_eq!(print(&expr), "TOPLA(2,3)+YANLIŞ+IF(TRUE,4,5)");
    for (src, expected) in [
        ("LET(YANLIŞ;7;YANLIŞ)", "LET(YANLIŞ,7,YANLIŞ)"),
        ("LET(yanlış;YANLIŞ;EĞER(yanlış;1;2))", "LET(yanlış,FALSE,IF(yanlış,1,2))"),
        ("LET(TOPLA;LAMBDA(x;x+1);TOPLA(2))", "LET(TOPLA,LAMBDA(x,x+1),TOPLA(2))"),
        ("LET(yanlış;7;değer;YANLIŞ;değer)", "LET(yanlış,7,değer,YANLIŞ,değer)"),
        ("LAMBDA(YANLIŞ;YANLIŞ)(7)", "LAMBDA(YANLIŞ,YANLIŞ)(7)"),
        ("LAMBDA(TOPLA;TOPLA(2))(LAMBDA(x;x+1))", "LAMBDA(TOPLA,TOPLA(2))(LAMBDA(x,x+1))"),
        ("LET(YANLIŞ;7;YANLIŞ)+YANLIŞ", "LET(YANLIŞ,7,YANLIŞ)+FALSE"),
        ("LET(YANLIŞ;7;LAMBDA(x;YANLIŞ+x)(1))", "LET(YANLIŞ,7,LAMBDA(x,YANLIŞ+x)(1))"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn locale_controls_decimal_list_array_separators_and_display() {
    let tr = FormulaLocale::Tr;
    let en = FormulaLocale::En;
    let expr = parse_input("EĞER(DOĞRU;TOPLA(1,5;2,25);0)", tr, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(TRUE,SUM(1.5,2.25),0)");
    assert_eq!(print_input(&expr, tr, |_| false).unwrap(), "EĞER(DOĞRU;TOPLA(1,5;2,25);0)");
    assert_eq!(print_input(&expr, en, |_| false).unwrap(), print(&expr));
    assert!(parse_input("SUM(1;2)", en, |_| false).is_err());
    assert_eq!(print(&parse_input("TOPLA(1,2)+YANLIŞ", en, |_| false).unwrap()), "TOPLA(1,2)+YANLIŞ");
    assert!(parse_input("TOPLA(1.5;2)", tr, |_| false).is_err());
    let expr = parse("SUM({1.5,2.25;3,4})+SUM((A1:A3,C1:C3))").unwrap();
    let shown = print_input(&expr, tr, |_| false).unwrap();
    assert_eq!(print(&parse_input(&shown, tr, |_| false).unwrap()), print(&expr));
    let expr = parse("LET(TOPLA,LAMBDA(x,x+1),SUM(TOPLA(2),FALSE))").unwrap();
    let shown = print_input(&expr, tr, |n| n == "YANLIŞ").unwrap();
    assert_eq!(shown, "LET(TOPLA;LAMBDA(x;x+1);SUM(TOPLA(2);FALSE))");
    assert_eq!(print(&parse_input(&shown, tr, |n| n == "YANLIŞ").unwrap()), print(&expr));
}

#[test]
fn turkish_display_round_trips_text_containing_separators() {
    let tr = FormulaLocale::Tr;
    let expr = parse(r#"IF(A1="a,b;c",CONCAT("1.5",",",";"),SUM(0.25,A1:A3 B2))"#).unwrap();
    let shown = print_input(&expr, tr, |_| false).unwrap();
    assert_eq!(shown, r#"EĞER(A1="a,b;c";METİNBİRLEŞTİR("1.5";",";";");TOPLA(0,25;A1:A3 B2))"#);
    assert_eq!(print(&parse_input(&shown, tr, |_| false).unwrap()), print(&expr));
}
