//! Regression tests for Excel-compatible default chart styles.

use gridcraft_engine::Session;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({
            "range": "A1",
            "values": [
                ["Quarter", "North", "South"],
                ["Q1", "10", "30"],
                ["Q2", "30", "10"]
            ]
        }),
    )
    .unwrap();
    s
}

/// Newly inserted charts use Excel's blue default style; explicit styles survive XLSX.
#[test]
fn new_charts_default_to_excel_style_two_and_keep_explicit_styles() {
    use std::io::Read;

    for kind in ["column", "line"] {
        let mut s = session();

        s.execute("insert.chart", json!({"range": "A1:C3", "type": kind})).unwrap();

        for expected in [2_u32, 9_u32] {
            let wb = &s.doc().unwrap().wb;
            assert_eq!(wb.active().unwrap().charts[0].style, expected, "{kind}: in-memory style");

            let bytes = gridcraft_engine::io::save_bytes(wb, "chart.xlsx").unwrap();

            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();

            let mut xml = String::new();
            archive.by_name("xl/charts/chart1.xml").unwrap().read_to_string(&mut xml).unwrap();

            assert!(xml.contains(&format!(r#"<c:style val="{expected}"/>"#)), "{kind}: expected style {expected} in {xml}");

            let (reopened, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();

            assert_eq!(reopened.active().unwrap().charts[0].style, expected, "{kind}: style after XLSX round-trip");

            if expected == 2 {
                s.execute("chart.set", json!({"style": 9})).unwrap();
            }
        }
    }
}

/// Forecast Sheet charts use Excel style 2 in memory and after XLSX export.
#[test]
fn forecast_sheet_chart_uses_excel_style_two_in_xlsx() {
    use std::io::Read;

    let mut s = Session::new();
    s.new_workbook();

    let mut rows = vec![json!(["Month", "Sales"])];
    let season = [3.0, -1.0, 4.0, -6.0];

    for t in 1..=16 {
        rows.push(json!([t, 10.0 + 2.0 * t as f64 + season[(t - 1) % 4]]));
    }

    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();

    s.execute("selection.set", json!({"range": "A1:B17"})).unwrap();

    s.execute("data.forecastSheet", json!({"periods": 4, "statistics": true})).unwrap();

    let wb = &s.doc().unwrap().wb;

    let chart = wb.active().unwrap().charts.first().unwrap();

    assert_eq!(chart.style, 2, "Forecast Sheet chart must use Excel style 2");

    let bytes = gridcraft_engine::io::save_bytes(wb, "forecast.xlsx").unwrap();

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();

    let mut xml = String::new();

    archive.by_name("xl/charts/chart1.xml").unwrap().read_to_string(&mut xml).unwrap();

    assert!(xml.contains(r#"<c:style val="2"/>"#), "Forecast Sheet XML must contain style 2: {xml}");

    let (reopened, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();

    assert_eq!(reopened.active().unwrap().charts[0].style, 2, "Forecast Sheet style must survive XLSX round-trip");
}
