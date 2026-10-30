//! The Power Pivot Data View's sort/filter state: `dataModel.view` and the view-state commands.
//! View state changes what the window shows, never the model, and `dataModel.load` resets it.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

const MODEL: &str = r#"{
    "tables": [
        {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"},{"name":"Secret","data_type":"String","hidden":true}],
         "rows":[["East",10,"x"],["West",30,"y"],["East",20,"z"]]},
        {"name":"Regions","columns":[{"name":"Region","data_type":"String"},{"name":"SortKey","data_type":"Int64"}],
         "rows":[["East",2],["West",1]]}
    ],
    "relationships": [{"from_table":"Sales","from_column":"Region","to_table":"Regions","to_column":"Region"}],
    "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
}"#;

fn load() -> Session {
    let mut s = Session::new();
    s.execute("dataModel.load", json!({"json": MODEL})).expect("load model");
    s
}

fn rows_of(s: &mut Session, params: Json) -> Json {
    s.execute("dataModel.view", params).expect("view")
}

fn amounts(view: &Json) -> Vec<f64> {
    view["rows"].as_array().expect("rows").iter().map(|row| row[1].as_f64().expect("number")).collect()
}

fn row_numbers(view: &Json) -> Vec<u64> {
    view["row_numbers"].as_array().expect("row numbers").iter().map(|row| row.as_u64().expect("index")).collect()
}

#[test]
fn view_hides_hidden_columns_and_reports_the_view_state() {
    let mut s = load();
    let view = rows_of(&mut s, json!({"table": "sales"}));
    assert_eq!(view["table"], json!("Sales"), "the reply keeps the model's spelling");
    assert_eq!(view["columns"], json!(["Region", "Amount"]), "the hidden column is out of the grid");
    assert_eq!(view["hidden"], json!(["Secret"]));
    assert_eq!(view["column_indexes"], json!([0, 1]));
    assert_eq!(view["text_rows"][1], json!(["West", "30"]), "the grid paints the model's text, not a JSON number");
    assert_eq!(row_numbers(&view), vec![0, 1, 2]);
    assert_eq!(view["sort"], Json::Null);
    assert_eq!(view["filters"], json!([]));
    assert_eq!(view["view_state"], json!(true), "the reply says it is view state, not model data");
    assert_eq!(view["total_rows"], json!(3));

    // Show Hidden is a window setting: the same query with the toggle on returns the column.
    let view = rows_of(&mut s, json!({"table": "Sales", "show_hidden": true}));
    assert_eq!(view["columns"], json!(["Region", "Amount", "Secret"]));
}

#[test]
fn sorting_orders_the_view_but_never_the_model() {
    let mut s = load();
    let reply = s.execute("dataModel.sortTable", json!({"table": "Sales", "column": "Amount", "descending": true})).expect("sort");
    assert_eq!(reply["view_state"], json!(true));
    assert_eq!(reply["sort"]["column"], json!("Amount"));
    assert_eq!(reply["sort"]["descending"], json!(true));
    assert_eq!(amounts(&rows_of(&mut s, json!({"table": "Sales"}))), vec![30.0, 20.0, 10.0]);
    assert_eq!(row_numbers(&rows_of(&mut s, json!({"table": "Sales"}))), vec![1, 2, 0], "the gutter keeps the model row numbers");
    assert_eq!(s.execute("dataModel.status", json!({})).expect("status")["tables"][0]["rows"], json!(3));

    // The model itself is untouched: a table expression still returns the stored row order.
    let table = s.execute("dataModel.evaluate", json!({"formula": "Sales", "table": true})).expect("table");
    assert_eq!(table["rows"][0][1], json!(10));

    s.execute("dataModel.clearSort", json!({"table": "Sales"})).expect("clear sort");
    assert_eq!(row_numbers(&rows_of(&mut s, json!({"table": "Sales"}))), vec![0, 1, 2]);
    // Clearing a sort that is not set is a no-op, not an error.
    s.execute("dataModel.clearSort", json!({"table": "Sales"})).expect("clear sort twice");
}

#[test]
fn filters_keep_the_allowed_values_per_column() {
    let mut s = load();
    s.execute("dataModel.setFilter", json!({"table": "Sales", "column": "Region", "values": ["East"]})).expect("filter");
    let filtered = rows_of(&mut s, json!({"table": "Sales"}));
    assert_eq!(row_numbers(&filtered), vec![0, 2]);
    assert_eq!(filtered["filters"], json!([{"column": "Region", "values": ["East"]}]));
    assert_eq!(filtered["visible_rows"], json!(2));

    // A second column's filter is ANDed with the first.
    s.execute("dataModel.setFilter", json!({"table": "Sales", "column": "Amount", "values": ["20"]})).expect("filter amount");
    assert_eq!(row_numbers(&rows_of(&mut s, json!({"table": "Sales"}))), vec![2]);

    // An empty value list clears that column's filter.
    s.execute("dataModel.setFilter", json!({"table": "Sales", "column": "Amount", "values": []})).expect("clear column filter");
    assert_eq!(row_numbers(&rows_of(&mut s, json!({"table": "Sales"}))), vec![0, 2]);

    s.execute("dataModel.clearFilters", json!({"table": "Sales"})).expect("clear filters");
    let all = rows_of(&mut s, json!({"table": "Sales"}));
    assert_eq!(row_numbers(&all), vec![0, 1, 2]);
    assert_eq!(all["filters"], json!([]));

    // Filter values are the text the grid paints, so a number filter matches its painted text.
    s.execute("dataModel.setFilter", json!({"table": "Sales", "column": "Amount", "values": ["30"]})).expect("numeric text filter");
    assert_eq!(row_numbers(&rows_of(&mut s, json!({"table": "Sales"}))), vec![1]);
}

#[test]
fn sort_by_column_orders_through_a_relationship() {
    let mut s = load();
    let reply = s
        .execute("dataModel.sortByColumn", json!({"table": "sales", "by_table": "regions", "by_column": "sortkey", "descending": false}))
        .expect("sort by column");
    assert_eq!(reply["sort"]["column"], json!("Region"), "the key column is resolved from the relationship");
    assert_eq!(reply["sort"]["by_table"], json!("Regions"));
    assert_eq!(reply["sort"]["by_column"], json!("SortKey"));
    // West sorts first (SortKey 1), then the two East rows in their stored order.
    assert_eq!(row_numbers(&rows_of(&mut s, json!({"table": "Sales"}))), vec![1, 0, 2]);
    // Descending reverses it; the equal East rows keep their relative order (stable sort).
    s.execute("dataModel.sortByColumn", json!({"table": "Sales", "by_table": "Regions", "by_column": "SortKey", "descending": true}))
        .expect("descending");
    assert_eq!(row_numbers(&rows_of(&mut s, json!({"table": "Sales"}))), vec![0, 2, 1]);
}

#[test]
fn sort_by_column_needs_a_relationship_and_disambiguates_its_key() {
    let mut s = load();
    let error = s
        .execute("dataModel.sortByColumn", json!({"table": "Sales", "by_table": "Sales", "by_column": "Amount"}))
        .expect_err("a table cannot sort by itself");
    assert!(error.to_string().contains("related table"), "{error}");
    let error = s
        .execute("dataModel.sortByColumn", json!({"table": "Regions", "by_table": "Sales", "by_column": "Amount", "column": "SortKey"}))
        .expect_err("no relationship joins Regions and Sales on SortKey");
    assert!(error.to_string().contains("no relationship"), "{error}");
    // With `column` omitted the single existing relationship is used in the other direction too.
    s.execute("dataModel.sortByColumn", json!({"table": "Regions", "by_table": "Sales", "by_column": "Amount"})).expect("reverse direction sort");

    // Two relationships between the same tables need `column` to choose the key.
    let two_links = r#"{
        "tables": [
            {"name":"A","columns":[{"name":"K1"},{"name":"K2"}],"rows":[[1,10],[2,20]]},
            {"name":"B","columns":[{"name":"K1"},{"name":"K2"},{"name":"Label"}],"rows":[[1,10,"a"],[2,20,"b"]]}
        ],
        "relationships": [
            {"from_table":"A","from_column":"K1","to_table":"B","to_column":"K1"},
            {"from_table":"A","from_column":"K2","to_table":"B","to_column":"K2"}
        ],
        "measures": []
    }"#;
    let mut s = Session::new();
    s.execute("dataModel.load", json!({"json": two_links})).expect("load");
    let error = s.execute("dataModel.sortByColumn", json!({"table": "A", "by_table": "B", "by_column": "Label"})).expect_err("ambiguous key");
    assert!(error.to_string().contains("more than one relationship"), "{error}");
    s.execute("dataModel.sortByColumn", json!({"table": "A", "by_table": "B", "by_column": "Label", "column": "K2"})).expect("chosen key");
    assert_eq!(row_numbers(&rows_of(&mut s, json!({"table": "A"}))), vec![0, 1]);
}

#[test]
fn loading_a_model_resets_the_view_state() {
    let mut s = load();
    s.execute("dataModel.sortTable", json!({"table": "Sales", "column": "Amount", "descending": true})).expect("sort");
    s.execute("dataModel.setFilter", json!({"table": "Sales", "column": "Region", "values": ["East"]})).expect("filter");
    s.execute("dataModel.load", json!({"json": MODEL})).expect("reload");
    let fresh = rows_of(&mut s, json!({"table": "Sales"}));
    assert_eq!(fresh["sort"], Json::Null, "a new model starts unsorted");
    assert_eq!(fresh["filters"], json!([]), "a new model starts unfiltered");
    assert_eq!(row_numbers(&fresh), vec![0, 1, 2]);
}

#[test]
fn view_commands_reject_unknown_tables_columns_and_bad_values() {
    let mut s = load();
    assert!(s.execute("dataModel.view", json!({"table": "Missing"})).is_err());
    assert!(s.execute("dataModel.sortTable", json!({"table": "Sales", "column": "Missing"})).is_err());
    assert!(s.execute("dataModel.setFilter", json!({"table": "Sales", "column": "Missing", "values": ["East"]})).is_err());
    let error = s.execute("dataModel.setFilter", json!({"table": "Sales", "column": "Region", "values": [1, 2]})).expect_err("values are text");
    assert!(error.to_string().contains("array of strings"), "{error}");
    assert!(s.execute("dataModel.clearFilters", json!({"table": "Missing"})).is_err());

    // A model-less session has the commands disabled, not merely failing at run time.
    let mut empty = Session::new();
    assert!(empty.execute("dataModel.view", json!({"table": "Sales"})).is_err());
    assert!(empty.commands().iter().any(|command| command.id == "dataModel.view" && !command.enabled));
    assert!(empty.commands().iter().any(|command| command.id == "dataModel.sortTable" && !command.enabled));
    assert!(s.commands().iter().any(|command| command.id == "dataModel.view" && command.enabled));
}
