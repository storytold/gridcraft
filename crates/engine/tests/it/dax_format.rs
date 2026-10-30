//! Measure formatting: `dataModel.setFormat` and the format applied to `dataModel.evaluate` and
//! `dataModel.pivot` output.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

const MODEL: &str = r#"{
    "tables": [{"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"}],
                "rows":[["East",10],["West",20],["East",30]]}],
    "relationships": [],
    "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
}"#;

fn load() -> Session {
    let mut s = Session::new();
    s.execute("dataModel.load", json!({"json": MODEL})).expect("load model");
    s
}

fn measure(status: &Json, name: &str) -> Json {
    status["measures"].as_array().expect("measures").iter().find(|measure| measure["name"] == name).cloned().expect("measure")
}

fn evaluate(s: &mut Session) -> Json {
    s.execute("dataModel.evaluate", json!({"measure": "Total Sales"})).expect("evaluate")
}

#[test]
fn presets_set_the_format_and_the_displayed_value() {
    let mut s = load();
    let status = s.execute("dataModel.setFormat", json!({"name": "Total Sales", "preset": "currency"})).expect("currency");
    assert_eq!(measure(&status, "Total Sales")["format"], json!("$#,##0.00"), "the preset is stored as a format code");
    let value = evaluate(&mut s);
    assert_eq!(value["value"].as_f64(), Some(60.0), "the numeric value stays exact for agents");
    assert_eq!(value["display"], json!("$60.00"), "the display is the formatted value");
    assert_eq!(value["format"], json!("$#,##0.00"));

    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "preset": "percent"})).expect("percent");
    assert_eq!(evaluate(&mut s)["display"], json!("6000.00%"));
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "preset": "comma"})).expect("comma");
    assert_eq!(evaluate(&mut s)["display"], json!("60.00"));
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "preset": "general"})).expect("general");
    assert!(
        measure(&s.execute("dataModel.status", json!({})).expect("status"), "Total Sales")["format"].is_null(),
        "General is the absence of a format"
    );
    assert_eq!(evaluate(&mut s)["display"], json!("60"));
}

#[test]
fn the_format_field_accepts_excel_format_codes() {
    let mut s = load();
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "format": "#,##0.0"})).expect("format");
    assert_eq!(evaluate(&mut s)["display"], json!("60.0"));
    let status = s.execute("dataModel.setFormat", json!({"name": "Total Sales", "format": null})).expect("clear");
    assert!(measure(&status, "Total Sales")["format"].is_null());
    assert_eq!(evaluate(&mut s)["display"], json!("60"));
    // A blank format string clears, like the measure dialog's empty Format field.
    let status = s.execute("dataModel.setFormat", json!({"name": "Total Sales", "format": "   "})).expect("blank clears");
    assert!(measure(&status, "Total Sales")["format"].is_null());
}

#[test]
fn increase_and_decrease_decimal_walk_the_format_code() {
    let mut s = load();
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "preset": "currency"})).expect("currency");
    let status = s.execute("dataModel.setFormat", json!({"name": "Total Sales", "decimals": 1})).expect("increase");
    assert_eq!(measure(&status, "Total Sales")["format"], json!("$#,##0.000"));
    assert_eq!(evaluate(&mut s)["display"], json!("$60.000"));

    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "decimals": -2})).expect("decrease");
    assert_eq!(
        measure(&s.execute("dataModel.status", json!({})).expect("status"), "Total Sales")["format"],
        json!("$#,##0.0"),
        "two fewer places leaves one decimal"
    );
    assert_eq!(evaluate(&mut s)["display"], json!("$60.0"));
    // At zero decimals a further decrease changes nothing, and the decimal point goes with the
    // last place, as Excel does.
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "decimals": -1})).expect("decrease to zero");
    assert_eq!(measure(&s.execute("dataModel.status", json!({})).expect("status"), "Total Sales")["format"], json!("$#,##0"));
    assert_eq!(evaluate(&mut s)["display"], json!("$60"));
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "decimals": -1})).expect("decrease at zero");
    assert_eq!(measure(&s.execute("dataModel.status", json!({})).expect("status"), "Total Sales")["format"], json!("$#,##0"));

    // A percent format keeps its sign and decimal places.
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "format": "0.00%"})).expect("percent code");
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "decimals": 1})).expect("increase percent");
    assert_eq!(measure(&s.execute("dataModel.status", json!({})).expect("status"), "Total Sales")["format"], json!("0.000%"));
    assert_eq!(evaluate(&mut s)["display"], json!("6000.000%"));
}

#[test]
fn pivot_output_carries_the_formatted_grid() {
    let mut s = load();
    s.execute("dataModel.setFormat", json!({"name": "Total Sales", "format": "0.0"})).expect("format");
    let pivot = s.execute("dataModel.pivot", json!({"measure": "Total Sales", "rows": ["Sales[Region]"]})).expect("pivot");
    assert_eq!(pivot["format"], json!("0.0"));
    let rows = pivot["rows"].as_array().expect("rows");
    let display = pivot["display_rows"].as_array().expect("display rows");
    assert_eq!(rows.len(), display.len());
    assert_eq!(display[0][1], json!("40.0"), "the East group shows its formatted value");
    assert_eq!(rows[0][1], json!(40.0), "the numeric grid is unchanged");
}

#[test]
fn formatting_rejects_unknown_measures_and_bad_parameters() {
    let mut s = load();
    let error = s.execute("dataModel.setFormat", json!({"name": "Missing", "format": "0.0"})).expect_err("unknown measure");
    assert!(error.to_string().contains("unknown measure"), "{error}");
    let error = s.execute("dataModel.setFormat", json!({"name": "Total Sales", "preset": "scientific"})).expect_err("unknown preset");
    assert!(error.to_string().contains("unknown format preset"), "{error}");
    let error = s.execute("dataModel.setFormat", json!({"name": "Total Sales", "decimals": 50})).expect_err("too many decimals");
    assert!(error.to_string().contains("at most 10"), "{error}");
    assert!(s.execute("dataModel.setFormat", json!({"name": "Total Sales"})).is_err(), "a format, preset or decimal change is required");
    assert!(s.execute("dataModel.setFormat", json!({"name": "Total Sales", "format": 3})).is_err(), "a format is text or null");
    assert!(s.execute("dataModel.setFormat", json!({"name": "Total Sales", "format": &"0".repeat(200)})).is_err(), "absurd codes are rejected");
    assert!(
        measure(&s.execute("dataModel.status", json!({})).expect("status"), "Total Sales")["format"].is_null(),
        "a rejected setFormat leaves the format untouched"
    );

    // Formatting is a measure operation: a column is not a measure, and the model has no such measure.
    assert!(s.execute("dataModel.setFormat", json!({"name": "Amount", "format": "0.0"})).is_err());
}
