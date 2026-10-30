//! Values shown in Spain's and Ukraine's formats; format codes and TEXT() stay en-US.

use gridcraft_core::{DateSystem, Region, Value};
use gridcraft_numfmt::{NumberFormat, format_value, format_value_in, text_function};

fn show(v: f64, code: &str, region: Region) -> String {
    format_value_in(&Value::Number(v), &NumberFormat::parse(code), DateSystem::D1900, region).text
}

fn date(y: i64, m: i64, d: i64) -> f64 {
    gridcraft_core::date::serial_from_ymd(DateSystem::D1900, y, m, d).unwrap()
}

#[test]
fn numbers_use_the_regions_separators() {
    let (es, uk) = (Region::EsEs, Region::UkUa);
    assert_eq!(show(1234.5, "#,##0.00", es), "1.234,50");
    assert_eq!(show(1234.5, "#,##0.00", uk), "1\u{a0}234,50");
    assert_eq!(show(1234567.891, "#,##0", es), "1.234.568");
    assert_eq!(show(1234.5, "General", es), "1234,5");
    assert_eq!(show(0.125, "0.0%", es), "12,5%");
    assert_eq!(show(1500.0, "0.00E+00", uk), "1,50E+03");
    assert_eq!(show(12.5, "#,##0.00 \"€\"", es), "12,50 €");
    assert_eq!(show(-12.5, "#,##0.00 \"€\";[Red]-#,##0.00 \"€\"", es), "-12,50 €");
    assert_eq!(show(1.5, "# ?/?", es), "1 1/2");
    // Literal text in a code keeps its dots and commas.
    assert_eq!(show(2.5, "0.0\" kg, aprox.\"", es), "2,5 kg, aprox.");
    // en-US is unchanged.
    assert_eq!(show(1234.5, "#,##0.00", Region::EnUs), "1,234.50");
    assert_eq!(format_value(&Value::Number(1234.5), &NumberFormat::parse("#,##0.00"), DateSystem::D1900).text, "1,234.50");
}

#[test]
fn the_short_date_follows_the_region() {
    let d = date(2026, 11, 5);
    assert_eq!(show(d, "m/d/yyyy", Region::EnUs), "11/5/2026");
    assert_eq!(show(d, "m/d/yyyy", Region::EsEs), "05/11/2026");
    assert_eq!(show(d, "m/d/yyyy", Region::UkUa), "05.11.2026");
    assert_eq!(show(d + 0.5, "m/d/yyyy h:mm", Region::UkUa), "05.11.2026 12:00");
    // An explicit code is shown as written.
    assert_eq!(show(d, "yyyy-mm-dd", Region::EsEs), "2026-11-05");
    assert_eq!(show(d, "mm/dd/yyyy", Region::EsEs), "11/05/2026");
}

#[test]
fn text_function_results_do_not_depend_on_the_region() {
    assert_eq!(text_function(&Value::Number(1234.5), "#,##0.00", DateSystem::D1900).unwrap(), "1,234.50");
    assert_eq!(text_function(&Value::Number(date(2026, 11, 5)), "m/d/yyyy", DateSystem::D1900).unwrap(), "11/5/2026");
}
