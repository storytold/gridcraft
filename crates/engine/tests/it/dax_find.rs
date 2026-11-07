//! `dataModel.find`: the Data View's Find across model rows, scoped by table and column.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

const MODEL: &str = r#"{
    "tables": [
        {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"},{"name":"Secret","data_type":"String","hidden":true}],
         "rows":[["East",10,"x"],["West",20,"y"],["East",30,"z"]]},
        {"name":"Regions","columns":[{"name":"Region","data_type":"String"},{"name":"Manager","data_type":"String"}],
         "rows":[["East","Ana"],["West","Bo"]]}
    ],
    "relationships": [],
    "measures": []
}"#;

fn load() -> Session {
    let mut s = Session::new();
    s.execute("dataModel.load", json!({"json": MODEL})).expect("load model");
    s
}

fn find(s: &mut Session, params: Json) -> Json {
    s.execute("dataModel.find", params).expect("find")
}

fn places(result: &Json) -> Vec<(String, String, u64)> {
    result["matches"]
        .as_array()
        .expect("matches")
        .iter()
        .map(|found| {
            (
                found["table"].as_str().expect("table").to_string(),
                found["column"].as_str().expect("column").to_string(),
                found["row"].as_u64().expect("row"),
            )
        })
        .collect()
}

#[test]
fn find_searches_every_table_and_reports_where_it_matched() {
    let mut s = load();
    let result = find(&mut s, json!({"query": "east"}));
    assert_eq!(
        places(&result),
        vec![("Sales".into(), "Region".into(), 0), ("Sales".into(), "Region".into(), 2), ("Regions".into(), "Region".into(), 0),]
    );
    assert_eq!(result["matches"][0]["value"], json!("East"));
    assert_eq!(result["truncated"], json!(false));
    assert_eq!(result["query"], json!("east"));
}

#[test]
fn match_case_and_scope_narrow_the_search() {
    let mut s = load();
    // Case-sensitive matching sees the stored spelling only.
    assert_eq!(places(&find(&mut s, json!({"query": "East", "case": true}))).len(), 3);
    assert!(places(&find(&mut s, json!({"query": "east", "case": true}))).is_empty(), "the stored spelling is East, not east");
    // A table scope keeps only that table's columns.
    let result = find(&mut s, json!({"query": "east", "table": "regions"}));
    assert_eq!(places(&result), vec![("Regions".into(), "Region".into(), 0)]);
    // A column scope keeps only that column.
    let result = find(&mut s, json!({"query": "2", "table": "Sales", "column": "Amount"}));
    assert_eq!(places(&result), vec![("Sales".into(), "Amount".into(), 1)]);
    // Hidden columns are model columns: Find searches them too.
    let result = find(&mut s, json!({"query": "z", "table": "Sales", "column": "Secret"}));
    assert_eq!(places(&result), vec![("Sales".into(), "Secret".into(), 2)]);
    // Numbers are searched as the grid paints them.
    let result = find(&mut s, json!({"query": "30", "table": "Sales"}));
    assert_eq!(places(&result), vec![("Sales".into(), "Amount".into(), 2)]);
}

#[test]
fn find_rejects_bad_parameters() {
    let mut s = load();
    assert!(s.execute("dataModel.find", json!({})).is_err(), "`query` is required");
    assert!(s.execute("dataModel.find", json!({"query": "  "})).is_err(), "a blank query is no search");
    let error = s.execute("dataModel.find", json!({"query": "east", "column": "Region"})).expect_err("a column scope needs a table");
    assert!(error.to_string().contains("needs `table`"), "{error}");
    assert!(s.execute("dataModel.find", json!({"query": "east", "table": "Missing"})).is_err());
    assert!(s.execute("dataModel.find", json!({"query": "east", "table": "Sales", "column": "Missing"})).is_err());
    let mut empty = Session::new();
    assert!(empty.execute("dataModel.find", json!({"query": "east"})).is_err(), "no model, no find");
    assert!(empty.commands().iter().any(|command| command.id == "dataModel.find" && !command.enabled));
}

#[test]
fn find_caps_its_result_and_says_so() {
    let rows: Vec<Json> = (0..600).map(|row| json!([row, "needle"])).collect();
    let model = json!({
        "tables": [{"name":"T","columns":[{"name":"N","data_type":"Int64"},{"name":"Text","data_type":"String"}],"rows": rows}],
        "relationships": [],
        "measures": []
    });
    let mut s = Session::new();
    s.execute("dataModel.load", json!({"model": model})).expect("load big model");
    let result = find(&mut s, json!({"query": "needle"}));
    assert_eq!(result["matches"].as_array().expect("matches").len(), 500, "the match list is capped");
    assert_eq!(result["truncated"], json!(true), "the cap is reported, never silent");
    // A smaller search is not marked truncated.
    let result = find(&mut s, json!({"query": "needle", "table": "T", "column": "N"}));
    assert!(result["matches"].as_array().expect("matches").is_empty());
    assert_eq!(result["truncated"], json!(false));
}
