//! `dataModel.validate`: referential integrity and relationship cardinality over the model's own
//! rows. Read-only: the report is not stored and the model is untouched.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

const MODEL: &str = r#"{
    "tables": [
        {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"}],
         "rows":[["East",10],["West",20],["Nowhere",5]]},
        {"name":"Regions","columns":[{"name":"Region","data_type":"String"},{"name":"SortKey","data_type":"Int64"}],
         "rows":[["East",1],["West",2],["East",3]]}
    ],
    "relationships": [{"from_table":"Sales","from_column":"Region","to_table":"Regions","to_column":"Region"}],
    "measures": []
}"#;

fn load() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("dataModel.load", json!({"json": MODEL})).expect("load model");
    s
}

fn findings(report: &Json, check: &str) -> Vec<Json> {
    report["findings"].as_array().expect("findings").iter().filter(|finding| finding["check"] == check).cloned().collect()
}

#[test]
fn an_orphan_row_and_a_repeated_one_side_key_are_reported_with_their_rows() {
    let mut s = load();
    let report = s.execute("dataModel.validate", json!({})).expect("validate");
    assert_eq!(report["tables"], json!(2));
    assert_eq!(report["rows"], json!(6), "every row of every table is inspected");

    let orphans = findings(&report, "orphanRow");
    assert_eq!(orphans.len(), 1, "{report}");
    assert_eq!(orphans[0]["severity"], json!("error"));
    assert_eq!(orphans[0]["table"], json!("Sales"));
    assert_eq!(orphans[0]["column"], json!("Region"));
    assert_eq!(orphans[0]["related_table"], json!("Regions"));
    assert_eq!(orphans[0]["key"], json!("Nowhere"));
    assert_eq!(orphans[0]["rows"], json!([2]), "the row id is reported");
    assert_eq!(orphans[0]["total"], json!(1));
    assert!(orphans[0]["message"].as_str().is_some_and(|message| message.contains("Nowhere")), "{report}");

    let duplicates = findings(&report, "duplicateKey");
    assert_eq!(duplicates.len(), 1, "only the repeated one-side key is reported: {report}");
    assert_eq!(duplicates[0]["severity"], json!("error"));
    assert_eq!(duplicates[0]["table"], json!("Regions"));
    assert_eq!(duplicates[0]["key"], json!("East"));
    assert_eq!(duplicates[0]["rows"], json!([0, 2]), "both rows of the duplicate are listed");
    assert_eq!(duplicates[0]["total"], json!(2), "two rows carry the duplicate key");
    assert!(duplicates[0]["message"].as_str().is_some_and(|message| message.contains("appears in 2 row(s)")), "{report}");

    // The measured cardinality says the one side is not unique: many-to-one, not the declared
    // one-to-many. That is a warning, and the rest of the report still arrives.
    let relationships = report["relationships"].as_array().expect("relationships");
    assert_eq!(relationships.len(), 1);
    assert_eq!(relationships[0]["declared"], json!("oneToMany"));
    assert_eq!(relationships[0]["actual"], json!("manyToOne"));
    assert_eq!(relationships[0]["from_rows"], json!(3));
    assert_eq!(relationships[0]["to_rows"], json!(3));
    assert_eq!(relationships[0]["orphan_keys"], json!(1));
    assert_eq!(relationships[0]["duplicate_to_keys"], json!(1));
    let cardinality = findings(&report, "invalidCardinality");
    assert_eq!(cardinality.len(), 1, "{report}");
    assert_eq!(cardinality[0]["severity"], json!("warning"));
}

#[test]
fn a_clean_model_reports_no_findings_and_the_model_is_not_changed() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "dataModel.load",
        json!({"json": r#"{"tables":[
            {"name":"Sales","columns":[{"name":"Region"}],"rows":[["East"],["East"]]},
            {"name":"Regions","columns":[{"name":"Region"}],"rows":[["East"],["West"]]}],
            "relationships":[{"from_table":"Sales","from_column":"Region","to_table":"Regions","to_column":"Region"}]}"#}),
    )
    .expect("load model");
    let before = s.execute("dataModel.status", json!({})).expect("status");
    let report = s.execute("dataModel.validate", json!({})).expect("validate");
    assert_eq!(report["findings"], json!([]), "{report}");
    assert_eq!(report["relationships"][0]["actual"], json!("oneToMany"));
    let after = s.execute("dataModel.status", json!({})).expect("status");
    assert_eq!(before, after, "validate is read-only");
}

#[test]
fn validate_needs_a_model() {
    let mut s = Session::new();
    let error = s.execute("dataModel.validate", json!({})).expect_err("no model");
    assert!(error.to_string().contains("no data model loaded"), "{error}");
}
