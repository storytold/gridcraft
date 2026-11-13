use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

/// Brazilian Portuguese entry is stored as canonical English and shown back in Portuguese:
/// names, `;` and `,` separators, `\` array columns, booleans and table specifiers.
#[test]
fn portuguese_formulas_store_english_and_show_portuguese() {
    let pt = FormulaLocale::Pt;
    for (typed, stored, shown) in [
        ("=SOMA(1;2,5)", "SUM(1,2.5)", "SOMA(1;2,5)"),
        ("=média(A1:A3)", "AVERAGE(A1:A3)", "MÉDIA(A1:A3)"),
        ("SE(verdadeiro;{1\\2;3\\4};FALSO)", "IF(TRUE,{1,2;3,4},FALSE)", "SE(VERDADEIRO;{1\\2;3\\4};FALSO)"),
        ("PROCV(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)", "PROCV(A2;D:E;2;FALSO)"),
        ("SOMA(T[[#Cabeçalhos];[Qtd]:[Preço]])", "SUM(T[[#Headers],[Qtd]:[Preço]])", "SOMA(T[[#Cabeçalhos];[Qtd]:[Preço]])"),
        ("CONT.SES(T[#Tudo];T[#dados];T[#Totais])", "COUNTIFS(T[#All],T[#Data],T[#Totals])", "CONT.SES(T[#Tudo];T[#Dados];T[#Totais])"),
        ("T[[#Esta linha];[Qtd]]", "T[@Qtd]", "T[@Qtd]"),
        // A function without a Portuguese name keeps its English one.
        ("MINHAFUNCAO(1;2)", "MINHAFUNCAO(1,2)", "MINHAFUNCAO(1;2)"),
    ] {
        let expr = parse_input(typed, pt, |_| false).unwrap();
        assert_eq!(print(&expr), stored, "{typed}");
        assert_eq!(print_input(&parse(stored).unwrap(), pt, |_| false).unwrap(), shown, "{stored}");
    }
    // File and API syntax stays English.
    assert!(parse("SOMA(1;2)").is_err());
    assert_eq!(print(&parse("VERDADEIRO").unwrap()), "VERDADEIRO");
}
