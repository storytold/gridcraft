use gridcraft_formula::{Expr, FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::Ru, |_| false).unwrap())
}

#[test]
fn russian_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=сумм(A1:A10)", "SUM(A1:A10)"),
        ("СРЗНАЧ(A1:A10)", "AVERAGE(A1:A10)"),
        ("ЕСЛИ(A1>0;\"Да\";\"Нет\")", "IF(A1>0,\"Да\",\"Нет\")"),
        ("СЧЁТ(A1:A10)", "COUNT(A1:A10)"),
        ("СУММЕСЛИ(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("СЧЁТЕСЛИ(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("ВПР(A2;D:E;2;ЛОЖЬ)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("СЕГОДНЯ()", "TODAY()"),
        ("ЧПС(0,1;A1:A5)", "NPV(0.1,A1:A5)"),
        ("ВСД(A1:A5;0,1)", "IRR(A1:A5,0.1)"),
        ("ПЛТ(0,08/12;10;10000)", "PMT(0.08/12,10,10000)"),
        ("ПС(0,05;12;100)", "PV(0.05,12,100)"),
        ("БС(0,05;12;100)", "FV(0.05,12,100)"),
        ("=истина", "TRUE"),
        ("=ложь", "FALSE"),
        ("ИСТИНА()", "TRUE()"),
        ("ЛОЖЬ()", "FALSE()"),
        ("СУММ(1;SUM(2;3);СРЗНАЧ(4;6))", "SUM(1,SUM(2,3),AVERAGE(4,6))"),
        ("ЕСЛИ(ИСТИНА;;)", "IF(TRUE,,)"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

/// Excel is case-insensitive about function and boolean names, Cyrillic included. The locale
/// table lookup used to use `eq_ignore_ascii_case`, which doesn't fold Cyrillic case at all
/// (it only worked for Spanish's plain-ASCII words) - this exercises the fix (`same_name`).
#[test]
fn russian_aliases_match_case_insensitively_like_the_evaluator() {
    for src in ["если(истина;1;2)", "Если(Истина;1;2)", "ЕСЛИ(ИСТИНА;1;2)", "еСлИ(иСтИнА;1;2)"] {
        assert_eq!(input(src), "IF(TRUE,1,2)", "{src}");
    }
}

#[test]
fn russian_input_preserves_reference_and_array_syntax() {
    for (src, expected) in [
        ("СУММ({1;2:3;4};5)", "SUM({1,2;3,4},5)"),
        ("{ИСТИНА;ЛОЖЬ:TRUE;FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("SUM((A1:A3;C1:C3))", "SUM((A1:A3,C1:C3))"),
        ("\"СУММ;ЛОЖЬ\"&\"a\"\"b\"", "\"СУММ;ЛОЖЬ\"&\"a\"\"b\""),
        ("'СУММ;ЛОЖЬ'!A1+СУММ!B2+СУММ!ЛОЖЬ", "'СУММ;ЛОЖЬ'!A1+СУММ!B2+СУММ!ЛОЖЬ"),
        ("СУММ[СЧЁТЕСЛИ]+ЛОЖЬ[ЕСЛИ]+СЧЁТ", "СУММ[СЧЁТЕСЛИ]+ЛОЖЬ[ЕСЛИ]+СЧЁТ"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    for bad in [r"СУММ({1;2:3})", "{СУММ(A1)}", "{ЛОЖЬ;}"] {
        assert!(parse_input(bad, FormulaLocale::Ru, |_| false).is_err(), "{bad}");
    }
    assert_eq!(print(&parse("СУММ(1,2)+ЛОЖЬ").unwrap()), "СУММ(1,2)+ЛОЖЬ");
    assert!(parse("SUM(1;2)").is_err());
    assert!(parse("{ИСТИНА}").is_err());
    // Real nesting past the parser's limit is still refused.
    let nested = "(".repeat(300) + "1" + &")".repeat(300);
    assert!(parse_input(&nested, FormulaLocale::Ru, |_| false).is_err());
    let deep = (0..300).fold(Expr::Number(1.0), |e, _| Expr::Paren(Box::new(e)));
    assert!(print_input(&deep, FormulaLocale::Ru, |_| false).is_err());
}

#[test]
fn russian_input_accepts_flat_chains_as_long_as_excel() {
    // A flat chain nests on its left without nesting in the text; see spanish_input.rs for why
    // this needs its own stack.
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let long_chain = "ЛОЖЬ+".repeat(1360) + "ИСТИНА";
            let english = "FALSE+".repeat(1360) + "TRUE";
            assert_eq!(input(&long_chain), english);
            assert_eq!(print_input(&parse(&english).unwrap(), FormulaLocale::Ru, |_| false).unwrap(), long_chain);
            let sums = vec!["СУММ(1;2)"; 800].join("+");
            assert_eq!(input(&sums), vec!["SUM(1,2)"; 800].join("+"));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn russian_input_preserves_defined_and_local_names() {
    let known = |name: &str| ["СУММ", "ЛОЖЬ"].iter().any(|n| gridcraft_formula::same_name(n, name));
    let expr = parse_input("СУММ(2;3)+ЛОЖЬ+ЕСЛИ(ИСТИНА;4;5)", FormulaLocale::Ru, known).unwrap();
    assert_eq!(print(&expr), "СУММ(2,3)+ЛОЖЬ+IF(TRUE,4,5)");
    for (src, expected) in [
        ("LET(ЛОЖЬ;7;ЛОЖЬ)", "LET(ЛОЖЬ,7,ЛОЖЬ)"),
        ("LET(ложь;ЛОЖЬ;ЕСЛИ(ложь;1;2))", "LET(ложь,FALSE,IF(ложь,1,2))"),
        ("LET(СУММ;LAMBDA(x;x+1);СУММ(2))", "LET(СУММ,LAMBDA(x,x+1),СУММ(2))"),
        ("LET(ложь;7;значение;ЛОЖЬ;значение)", "LET(ложь,7,значение,ЛОЖЬ,значение)"),
        ("LAMBDA(ЛОЖЬ;ЛОЖЬ)(7)", "LAMBDA(ЛОЖЬ,ЛОЖЬ)(7)"),
        ("LAMBDA(СУММ;СУММ(2))(LAMBDA(x;x+1))", "LAMBDA(СУММ,СУММ(2))(LAMBDA(x,x+1))"),
        ("LET(ЛОЖЬ;7;ЛОЖЬ)+ЛОЖЬ", "LET(ЛОЖЬ,7,ЛОЖЬ)+FALSE"),
        ("LET(ЛОЖЬ;7;LAMBDA(x;ЛОЖЬ+x)(1))", "LET(ЛОЖЬ,7,LAMBDA(x,ЛОЖЬ+x)(1))"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn locale_controls_decimal_list_array_separators_and_display() {
    let ru = FormulaLocale::Ru;
    let en = FormulaLocale::En;
    let expr = parse_input("ЕСЛИ(ИСТИНА;СУММ(1,5;2,25);0)", ru, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(TRUE,SUM(1.5,2.25),0)");
    assert_eq!(print_input(&expr, ru, |_| false).unwrap(), "ЕСЛИ(ИСТИНА;СУММ(1,5;2,25);0)");
    assert_eq!(print_input(&expr, en, |_| false).unwrap(), print(&expr));
    assert!(parse_input("SUM(1;2)", en, |_| false).is_err());
    assert_eq!(print(&parse_input("СУММ(1,2)+ЛОЖЬ", en, |_| false).unwrap()), "СУММ(1,2)+ЛОЖЬ");
    assert!(parse_input("СУММ(1.5;2)", ru, |_| false).is_err());
    let expr = parse("SUM({1.5,2.25;3,4})+SUM((A1:A3,C1:C3))").unwrap();
    let shown = print_input(&expr, ru, |_| false).unwrap();
    assert_eq!(print(&parse_input(&shown, ru, |_| false).unwrap()), print(&expr));
    let expr = parse("LET(СУММ,LAMBDA(x,x+1),SUM(СУММ(2),FALSE))").unwrap();
    let shown = print_input(&expr, ru, |n| n == "ЛОЖЬ").unwrap();
    assert_eq!(shown, "LET(СУММ;LAMBDA(x;x+1);SUM(СУММ(2);FALSE))");
    assert_eq!(print(&parse_input(&shown, ru, |n| n == "ЛОЖЬ").unwrap()), print(&expr));
}

#[test]
fn russian_display_round_trips_text_containing_separators() {
    let ru = FormulaLocale::Ru;
    let expr = parse(r#"IF(A1="a,b;c",CONCAT("1.5",",",";"),SUM(0.25,A1:A3 B2))"#).unwrap();
    let shown = print_input(&expr, ru, |_| false).unwrap();
    assert_eq!(shown, r#"ЕСЛИ(A1="a,b;c";CONCAT("1.5";",";";");СУММ(0,25;A1:A3 B2))"#);
    assert_eq!(print(&parse_input(&shown, ru, |_| false).unwrap()), print(&expr));
}

/// `;` is the array *column* separator in Russian, same character as the ordinary argument
/// separator - `:` is the row separator instead (confirmed against Microsoft's own ru-ru
/// documentation), the opposite pairing from Spanish's `\`/`;`. A plain range's `:` outside an
/// array (A1:A3) is untouched either way - only `array_depth > 0` triggers the translation.
#[test]
fn russian_array_literal_uses_semicolon_for_columns_and_colon_for_rows() {
    let ru = FormulaLocale::Ru;
    let expr = parse("SUM({1,2;3,4})+SUM((A1:A3,C1:C3))").unwrap();
    let shown = print_input(&expr, ru, |_| false).unwrap();
    assert_eq!(shown, "СУММ({1;2:3;4})+СУММ((A1:A3;C1:C3))");
    assert_eq!(print(&parse_input(&shown, ru, |_| false).unwrap()), print(&expr));
}
