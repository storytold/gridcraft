use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

const TR: FormulaLocale = FormulaLocale::Tr;

fn input(src: &str) -> String {
    print(&parse_input(src, TR, |_| false).unwrap())
}

#[test]
fn turkish_separators_parse_argument_lists_decimals_and_arrays() {
    for (src, expected) in [
        ("=SUM(A1:A10;B1)", "SUM(A1:A10,B1)"),
        ("=IF(A1>0;1;0)", "IF(A1>0,1,0)"),
        ("=SUM(1,5)", "SUM(1.5)"),
        ("=A1*0,29*0,5", "A1*0.29*0.5"),
        ("=SUM(1,25;-2,5)", "SUM(1.25,-2.5)"),
        ("=SUM({1\\2;3\\4};5)", "SUM({1,2;3,4},5)"),
        ("=1,5+1", "1.5+1"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    // A dot is not the decimal separator here, and a semicolon is not an argument separator.
    assert!(parse_input("SUM(1.5;2)", TR, |_| false).is_err());
    assert!(parse_input("SUM(1,2,3)", TR, |_| false).is_err());
    // English syntax stays valid for English-language users of the same build.
    assert_eq!(print(&parse_input("SUM(1.2,3)", FormulaLocale::En, |_| false).unwrap()), "SUM(1.2,3)");
}

#[test]
fn turkish_display_round_trips_decimals_arrays_and_text() {
    let expr = parse(r#"IF(A1="a,b;c",CONCAT("1.5",",",";"),SUM(0.25,A1:A3))"#).unwrap();
    let shown = print_input(&expr, TR, |_| false).unwrap();
    assert_eq!(shown, r#"IF(A1="a,b;c";CONCAT("1.5";",";";");SUM(0,25;A1:A3))"#);
    assert_eq!(print(&parse_input(&shown, TR, |_| false).unwrap()), print(&expr));
    // Names and TRUE/FALSE are not translated: Turkish keeps the English spellings.
    assert_eq!(print_input(&parse("TRUE+SUM(1.5)").unwrap(), TR, |_| false).unwrap(), "TRUE+SUM(1,5)");
}
