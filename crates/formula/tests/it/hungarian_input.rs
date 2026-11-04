use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

#[test]
fn hungarian_entry_round_trips_numbers_booleans_arrays_and_references() {
    for (localized, canonical) in [
        ("ha(igaz;átlag(1,5;2,5);0)", "IF(TRUE,AVERAGE(1.5,2.5),0)"),
        ("SZUM({1\\2;3\\4};5)", "SUM({1,2;3,4},5)"),
        ("{IGAZ\\HAMIS;TRUE\\FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("FKERES(A2;D:E;2;HAMIS)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("HAHIBA(GYÖK(-1);\"hiba;1,5\")", "IFERROR(SQRT(-1),\"hiba;1,5\")"),
        ("SZUM('Árvíztűrő'!A1:A3)", "SUM(Árvíztűrő!A1:A3)"),
        ("SZUM[ÁTLAG]+\"SZUM;HAMIS\"", "SZUM[ÁTLAG]+\"SZUM;HAMIS\""),
        ("LET(HAMIS;7;HA(HAMIS;1;2))", "LET(HAMIS,7,IF(HAMIS,1,2))"),
        ("LAMBDA(SZUM;SZUM(2))(LAMBDA(x;x+1))", "LAMBDA(SZUM,SZUM(2))(LAMBDA(x,x+1))"),
    ] {
        let parsed = parse_input(localized, FormulaLocale::Hu, |_| false).unwrap();
        assert_eq!(print(&parsed), canonical);
        let shown = print_input(&parsed, FormulaLocale::Hu, |_| false).unwrap();
        assert_eq!(parse_input(&shown, FormulaLocale::Hu, |_| false).unwrap(), parsed);
    }
}

#[test]
fn hungarian_names_and_file_syntax_remain_distinct() {
    let known = |name: &str| ["SZUM", "HAMIS"].contains(&name);
    let expr = parse("SUM(SZUM(1),FALSE)+HAMIS").unwrap();
    let shown = print_input(&expr, FormulaLocale::Hu, known).unwrap();
    assert_eq!(shown, "SUM(SZUM(1);FALSE)+HAMIS");
    assert_eq!(parse_input(&shown, FormulaLocale::Hu, known).unwrap(), expr);
    assert_eq!(print(&parse("SZUM(1,2)+HAMIS").unwrap()), "SZUM(1,2)+HAMIS");
    assert!(parse("{IGAZ}").is_err());
    assert!(parse_input("{VERDADERO}", FormulaLocale::Hu, |_| false).is_err());
    assert!(parse_input("{IGAZ}", FormulaLocale::Es, |_| false).is_err());
    assert!(parse_input("SZUM(1.5;2)", FormulaLocale::Hu, |_| false).is_err());
    let deep = "HA(IGAZ;".repeat(300) + "0" + &";0)".repeat(300);
    assert!(parse_input(&deep, FormulaLocale::Hu, |_| false).is_err());
}

#[test]
fn hungarian_flat_chains_and_case_folded_bindings_preserve_main_behavior() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let localized = "HAMIS+".repeat(1360) + "IGAZ";
            let canonical = "FALSE+".repeat(1360) + "TRUE";
            let parsed = parse_input(&localized, FormulaLocale::Hu, |_| false).unwrap();
            assert_eq!(print(&parsed), canonical);
            assert_eq!(print_input(&parsed, FormulaLocale::Hu, |_| false).unwrap(), localized);
            let parsed = parse_input("LET(átlag;LAMBDA(x;x+1);ÁTLAG(2))", FormulaLocale::Hu, |_| false).unwrap();
            assert_eq!(print(&parsed), "LET(átlag,LAMBDA(x,x+1),ÁTLAG(2))");
        })
        .unwrap()
        .join()
        .unwrap();
}
