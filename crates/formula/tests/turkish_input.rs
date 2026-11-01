use gridcraft_formula::{FormulaLocale, TURKISH, parse, parse_input, print, print_input, turkish_eq_ignore_case};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::Tr, |_| false).unwrap())
}

fn tr_eq(a: &str, b: &str) -> bool {
    FormulaLocale::Tr.canonical_name(a) == FormulaLocale::Tr.canonical_name(b) || turkish_eq_ignore_case(a, b)
}

#[test]
fn turkish_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=topla(A1:A10)", "SUM(A1:A10)"),
        ("ORTALAMA(A1:A10)", "AVERAGE(A1:A10)"),
        ("EĞER(A1>0;\"Evet\";\"Hayır\")", "IF(A1>0,\"Evet\",\"Hayır\")"),
        ("SAY(A1:A10)", "COUNT(A1:A10)"),
        ("BAĞ_DEĞ_SAY(A1:A10)", "COUNT(A1:A10)"),
        ("bağ_değ_say(A1:A10)", "COUNT(A1:A10)"),
        ("BAĞDEĞSAY(A1:A10)", "COUNT(A1:A10)"),
        ("STDSAPMA(A1:A10)", "STDEV(A1:A10)"),
        ("VARYANS(A1:A10)", "VAR(A1:A10)"),
        ("NORMDAĞ(1;2;3;DOĞRU)", "NORMDIST(1,2,3,TRUE)"),
        ("BİRLEŞTİR(\"a\";\"b\")", "CONCATENATE(\"a\",\"b\")"),
        ("DEVRESEL_ÖDEME(0,05/12;60;-10000)", "PMT(0.05/12,60,-10000)"),
        ("BD(0,05/12;60;-200)", "PV(0.05/12,60,-200)"),
        ("GD(0,05/12;60;-200)", "FV(0.05/12,60,-200)"),
        ("TAKSİT_SAYISI(0,05/12;-200;10000)", "NPER(0.05/12,-200,10000)"),
        ("FAİZ_ORANI(60;-200;10000)", "RATE(60,-200,10000)"),
        ("İÇ_VERİM_ORANI(A1:A5)", "IRR(A1:A5)"),
        ("NBD(0,1;A1:A5)", "NPV(0.1,A1:A5)"),
        ("AİÇVERİMORANI(A1:A5;B1:B5)", "XIRR(A1:A5,B1:B5)"),
        ("ÇAPRAZİÇVERİMORANI(A1:A5;B1:B5)", "XIRR(A1:A5,B1:B5)"),
        ("ANBD(0,1;A1:A5;B1:B5)", "XNPV(0.1,A1:A5,B1:B5)"),
        ("ÇAPRAZNBD(0,1;A1:A5;B1:B5)", "XNPV(0.1,A1:A5,B1:B5)"),
        ("ETOPLA(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("EĞERSAY(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("DÜŞEYARA(A2;D:E;2;YANLIŞ)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("düşeyara(A2;D:E;2;yanlış)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("BUGÜN()", "TODAY()"),
        ("=doğru", "TRUE"),
        ("=yanlış", "FALSE"),
        ("DOĞRU()", "TRUE()"),
        ("YANLIŞ()", "FALSE()"),
        ("KARMAŞIK(3;4)", "COMPLEX(3,4)"),
        ("SANÇIKAR(\"3+4i\";\"1+2i\")", "IMSUM(\"3+4i\",\"1+2i\")"),
        ("SANTOPLA(\"3+4i\";\"1+2i\")", "IMSUB(\"3+4i\",\"1+2i\")"),
        ("DİZİOLUŞTUR(2;3)", "MAKEARRAY(2,3)"),
        ("NSAT(3,14159;2)", "TRUNC(3.14159,2)"),
        ("TAMSAYI_YAP(3,14159;2)", "TRUNC(3.14159,2)"),
        ("DOT(A1:A5;B1:B5)", "LINEST(A1:A5,B1:B5)"),
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
        ("TOPLA({1;2\\3;4};5)", "SUM({1,2;3,4},5)"),
        ("{DOĞRU;YANLIŞ\\TRUE;FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("SATIRSAY({1;2;3})", "ROWS({1,2,3})"),
        ("SATIRSAY({1\\2\\3})", "ROWS({1;2;3})"),
        ("SUM((A1:A3;C1:C3))", "SUM((A1:A3,C1:C3))"),
        ("\"TOPLA;YANLIŞ\"&\"a\"\"b\"", "\"TOPLA;YANLIŞ\"&\"a\"\"b\""),
        ("'TOPLA;YANLIŞ'!A1+TOPLA!B2+TOPLA!YANLIŞ", "'TOPLA;YANLIŞ'!A1+TOPLA!B2+TOPLA!YANLIŞ"),
        ("TOPLA[EĞERSAY]+YANLIŞ[EĞER]+SAY", "TOPLA[EĞERSAY]+YANLIŞ[EĞER]+SAY"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    for bad in [r"TOPLA({1\2;3})", "{TOPLA(A1)}", "{YANLIŞ;}"] {
        assert!(parse_input(bad, FormulaLocale::Tr, |_| false).is_err(), "{bad}");
    }
    assert_eq!(print(&parse("TOPLA(1,2)+YANLIŞ").unwrap()), "TOPLA(1,2)+YANLIŞ");
    assert!(parse("SUM(1;2)").is_err());
    assert!(parse("{DOĞRU}").is_err());
    // Real nesting past the parser's limit is still refused.
    let nested = "(".repeat(300) + "1" + &")".repeat(300);
    assert!(parse_input(&nested, FormulaLocale::Tr, |_| false).is_err());
    let deep = (0..300).fold(gridcraft_formula::Expr::Number(1.0), |e, _| gridcraft_formula::Expr::Paren(Box::new(e)));
    assert!(print_input(&deep, FormulaLocale::Tr, |_| false).is_err());
}

#[test]
fn turkish_input_accepts_flat_chains_as_long_as_excel() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let long_chain = "YANLIŞ+".repeat(1160) + "DOĞRU";
            let english = "FALSE+".repeat(1160) + "TRUE";
            assert_eq!(input(&long_chain), english);
            assert_eq!(print_input(&parse(&english).unwrap(), FormulaLocale::Tr, |_| false).unwrap(), long_chain);
            let sums = vec!["TOPLA(1;2)"; 700].join("+");
            assert_eq!(input(&sums), vec!["SUM(1,2)"; 700].join("+"));
        })
        .unwrap()
        .join()
        .unwrap();
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
    assert_eq!(shown, r#"EĞER(A1="a,b;c";ARALIKBİRLEŞTİR("1.5";",";";");TOPLA(0,25;A1:A3 B2))"#);
    assert_eq!(print(&parse_input(&shown, tr, |_| false).unwrap()), print(&expr));
}

#[test]
fn turkish_error_literals_parse_and_display() {
    let tr = FormulaLocale::Tr;
    for (tr_err, canonical) in [
        ("#BOŞ!", "#NULL!"),
        ("#SAYI/0!", "#DIV/0!"),
        ("#DEĞER!", "#VALUE!"),
        ("#BAŞV!", "#REF!"),
        ("#AD?", "#NAME?"),
        ("#SAYI!", "#NUM!"),
        ("#YOK", "#N/A"),
        ("#VERİ_ALINIYOR", "#GETTING_DATA"),
        ("#TAŞMA!", "#SPILL!"),
        ("#HESAP!", "#CALC!"),
        ("#DÖNGÜ!", "#CIRC!"),
    ] {
        let expr = parse_input(tr_err, tr, |_| false).unwrap();
        assert_eq!(print(&expr), canonical);
        let shown = print_input(&expr, tr, |_| false).unwrap();
        assert_eq!(shown, tr_err);

        // Lowercase Turkish error literals (including ı/i byte-length differences) must parse
        let lower_err = tr.lowercase(tr_err);
        let expr_lower = parse_input(&lower_err, tr, |_| false).unwrap();
        assert_eq!(print(&expr_lower), canonical, "lowercase error literal: {lower_err}");

        // Also ensure canonical literals parse in TR locale
        let expr_canon = parse_input(canonical, tr, |_| false).unwrap();
        assert_eq!(print(&expr_canon), canonical);
    }
    assert_eq!(tr.uppercase("iç_verim_oranı"), "İÇ_VERİM_ORANI");
    assert_eq!(tr.lowercase("İÇ_VERİM_ORANI"), "iç_verim_oranı");
}

#[test]
fn turkish_case_folding_and_dotless_i() {
    let tr = FormulaLocale::Tr;

    // Direct canonical_name resolution tests
    for (name, expected) in [
        // Dotless ı typed on Turkish keyboard for English names with 'I':
        ("ıserror", "ISERROR"),
        ("iserror", "ISERROR"),
        ("ISERROR", "ISERROR"),
        ("ıferror", "IFERROR"),
        ("iferror", "IFERROR"),
        ("mın", "MIN"),
        ("min", "MIN"),
        ("MIN", "MIN"),
        ("MİN", "MIN"),
        ("ındex", "INDEX"),
        ("index", "INDEX"),
        ("INDEX", "INDEX"),
        ("İNDEX", "INDEX"),
        ("ısblank", "ISBLANK"),
        ("isblank", "ISBLANK"),
        ("ısnumber", "ISNUMBER"),
        ("ısna", "ISNA"),
        // Turkish localized names typed with dotless ı or dotted i/İ:
        ("ehatalıysa", "ISERROR"),
        ("ehataliysa", "ISERROR"),
        ("EHATALIYSA", "ISERROR"),
        ("eğerhata", "IFERROR"),
        ("EĞERHATA", "IFERROR"),
        ("eğersay", "COUNTIF"),
        ("EĞERSAY", "COUNTIF"),
        ("düşeyara", "VLOOKUP"),
        ("DÜŞEYARA", "VLOOKUP"),
        ("indis", "INDEX"),
        ("ındıs", "INDEX"),
        ("İNDİS", "INDEX"),
        ("INDIS", "INDEX"),
    ] {
        assert_eq!(tr.canonical_name(name), expected, "canonical_name({name})");
    }

    // Full formula input parse & print round-trip tests
    for (src, expected) in [
        ("=ıserror(A1)", "ISERROR(A1)"),
        ("=iserror(A1)", "ISERROR(A1)"),
        ("=ISERROR(A1)", "ISERROR(A1)"),
        ("=ehatalıysa(A1)", "ISERROR(A1)"),
        ("=ehataliysa(A1)", "ISERROR(A1)"),
        ("=EHATALIYSA(A1)", "ISERROR(A1)"),
        ("=ıferror(A1;0)", "IFERROR(A1,0)"),
        ("=iferror(A1;0)", "IFERROR(A1,0)"),
        ("=eğerhata(A1;0)", "IFERROR(A1,0)"),
        ("=EĞERHATA(A1;0)", "IFERROR(A1,0)"),
        ("=mın(A1:A10)", "MIN(A1:A10)"),
        ("=min(A1:A10)", "MIN(A1:A10)"),
        ("=MİN(A1:A10)", "MIN(A1:A10)"),
        ("=ındex(A1:B2;1;1)", "INDEX(A1:B2,1,1)"),
        ("=index(A1:B2;1;1)", "INDEX(A1:B2,1,1)"),
        ("=indis(A1:B2;1;1)", "INDEX(A1:B2,1,1)"),
        ("=ındıs(A1:B2;1;1)", "INDEX(A1:B2,1,1)"),
        ("=İNDİS(A1:B2;1;1)", "INDEX(A1:B2,1,1)"),
        ("=eğersay(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("=düşeyara(A2;D:E;2;yanlış)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("=ısblank(A1)", "ISBLANK(A1)"),
        ("=ısnumber(A1)", "ISNUMBER(A1)"),
        ("=ısna(A1)", "ISNA(A1)"),
    ] {
        assert_eq!(input(src), expected, "input formula test failed for: {src}");
    }

    // Full round-trip: parsed expression printed in TR locale displays standard Turkish uppercase,
    // and re-parsing that display produces the identical canonical expression.
    for (src, canonical_expected, tr_display_expected) in [
        ("=ıserror(A1)", "ISERROR(A1)", "EHATALIYSA(A1)"),
        ("=iserror(A1)", "ISERROR(A1)", "EHATALIYSA(A1)"),
        ("=ehataliysa(A1)", "ISERROR(A1)", "EHATALIYSA(A1)"),
        ("=ıferror(A1;0)", "IFERROR(A1,0)", "EĞERHATA(A1;0)"),
        ("=mın(A1:A10)", "MIN(A1:A10)", "MİN(A1:A10)"),
        ("=ındex(A1:B2;1;1)", "INDEX(A1:B2,1,1)", "İNDİS(A1:B2;1;1)"),
        ("=ındıs(A1:B2;1;1)", "INDEX(A1:B2,1,1)", "İNDİS(A1:B2;1;1)"),
        ("=eğersay(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")", "EĞERSAY(A1:A10;\">0\")"),
        ("=düşeyara(A2;D:E;2;yanlış)", "VLOOKUP(A2,D:E,2,FALSE)", "DÜŞEYARA(A2;D:E;2;YANLIŞ)"),
    ] {
        let parsed = parse_input(src, tr, |_| false).unwrap();
        assert_eq!(print(&parsed), canonical_expected);
        let shown = print_input(&parsed, tr, |_| false).unwrap();
        assert_eq!(shown, tr_display_expected);
        let reparsed = parse_input(&shown, tr, |_| false).unwrap();
        assert_eq!(print(&reparsed), canonical_expected);
    }
}

#[test]
fn turkish_translation_has_no_collisions_and_all_round_trip() {
    let tr = FormulaLocale::Tr;
    assert_eq!(TURKISH.len(), 507, "expected 507 translated functions in TURKISH table");

    // 1. Verify no two distinct English functions share one Turkish name (exact collision check)
    let mut tr_to_en = std::collections::HashMap::new();
    for &(en, localized) in TURKISH {
        if let Some(prev_en) = tr_to_en.insert(localized, en) {
            panic!("Collision in TURKISH table: '{localized}' is mapped from both '{prev_en}' and '{en}'");
        }
    }

    // 2. Verify no collisions even under Turkish case-folding (locale-aware comparison)
    for (i, &(en1, tr1)) in TURKISH.iter().enumerate() {
        for &(en2, tr2) in &TURKISH[i + 1..] {
            assert!(!turkish_eq_ignore_case(tr1, tr2), "Turkish case-folding collision: '{tr1}' ({en1}) vs '{tr2}' ({en2})");
        }
    }

    // 3. Verify every function in TURKISH round-trips: canonical_name(function_name(en)) == en
    for &(en, _) in TURKISH {
        let localized = tr.function_name(en);
        let roundtrip = tr.canonical_name(localized);
        assert_eq!(roundtrip, en, "Round-trip failed for function: '{en}' -> '{localized}' -> '{roundtrip}'");

        // Lowercase localized with Turkish locale (e.g. İ -> i, I -> ı)
        let lower_tr = tr.lowercase(localized);
        assert_eq!(tr.canonical_name(&lower_tr), en, "Lowercase TR round-trip failed: '{en}' -> '{localized}' -> '{lower_tr}'");

        // Lowercase English with Turkish locale (e.g. I -> ı)
        let lower_en_tr = tr.lowercase(en);
        assert_eq!(tr.canonical_name(&lower_en_tr), en, "Lowercase EN (Turkish locale) round-trip failed: '{en}' -> '{lower_en_tr}'");

        // Lowercase English with ASCII lowercase (e.g. I -> i)
        let lower_en_ascii = en.to_ascii_lowercase();
        assert_eq!(tr.canonical_name(&lower_en_ascii), en, "ASCII lowercase EN round-trip failed: '{en}' -> '{lower_en_ascii}'");
    }
}
