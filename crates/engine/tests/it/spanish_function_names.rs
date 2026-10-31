//! Spanish formula entry knows every built-in function (Tinta Calculo, stage 1).

use gridcraft_engine::cmd::formulas::function_list;
use gridcraft_engine::display::cell_text;
use gridcraft_engine::formula::{FormulaLocale, spanish_function_names};
use serde_json::json;

#[test]
fn every_listed_function_has_a_spanish_name() {
    let missing: Vec<String> = function_list()
        .iter()
        .filter_map(|f| f["name"].as_str())
        .filter(|name| !spanish_function_names().iter().any(|(en, _)| en == name))
        .map(str::to_string)
        .collect();
    assert!(missing.is_empty(), "functions without a Spanish name: {missing:?}");
    // Every row names a function the app has, so the table holds no stale names.
    let listed: Vec<String> = function_list().iter().filter_map(|f| f["name"].as_str().map(str::to_string)).collect();
    let extra: Vec<&str> = spanish_function_names().iter().map(|p| p.0).filter(|en| !listed.iter().any(|n| n == en)).collect();
    assert!(extra.is_empty(), "Spanish names for unknown functions: {extra:?}");
}

#[test]
fn spanish_formulas_compute_like_english_ones() {
    use gridcraft_engine::formula::{parse_input, print};
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    let cases = [
        ("SUMAR.SI.CONJUNTO({10\\20\\30};{1\\2\\1};1)", "40"),
        ("AÑO(FECHA(2026;11;3))", "2026"),
        ("REDONDEAR(PROMEDIO(1;2;4);2)", "2.33"),
        ("SI.ERROR(1/0;\"sin datos\")", "sin datos"),
        ("MAYUSC(IZQUIERDA(\"tinta\";3))", "TIN"),
        ("DIAS.LAB(FECHA(2026;11;2);FECHA(2026;11;6))", "5"),
    ];
    for (row, (spanish, expected)) in cases.iter().enumerate() {
        let expr = parse_input(spanish, FormulaLocale::Es, |_| false).unwrap();
        s.execute("cell.set", json!({"cell": format!("A{}", row + 1), "input": format!("={}", print(&expr))})).unwrap();
        let d = s.doc().unwrap();
        let shown = cell_text(&d.wb, d.wb.active().unwrap(), gridcraft_engine::core::CellRef::new(row as u32, 0));
        assert_eq!(shown, *expected, "{spanish}");
    }
}
