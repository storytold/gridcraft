//! Model metadata: hidden columns, implicit measures, data types, date tables and default
//! summarization. The metadata travels in the model JSON and is read at `dataModel.load`.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

const MODEL: &str = r#"{
    "tables": [
        {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"},{"name":"Secret","data_type":"String","hidden":true}],
         "rows":[["East",10,"x"],["West",20,"y"],["East",30,"z"]]},
        {"name":"Calendar","columns":[{"name":"Date","data_type":"DateTime"}],"rows":[["2024-01-01"],["2024-01-02"],["2024-01-03"]]},
        {"name":"Gap","columns":[{"name":"Date","data_type":"DateTime"}],"rows":[["2024-01-01"],["2024-01-03"]]},
        {"name":"Duplicate","columns":[{"name":"Date","data_type":"DateTime"}],"rows":[["2024-01-01"],["2024-01-01"]]},
        {"name":"Unsorted","columns":[{"name":"Date","data_type":"DateTime"}],"rows":[["2024-01-02"],["2024-01-01"]]},
        {"name":"Empty","columns":[{"name":"Date","data_type":"DateTime"}],"rows":[]}
    ],
    "relationships": [],
    "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
}"#;

fn load() -> Session {
    let mut s = Session::new();
    s.execute("dataModel.load", json!({"json": MODEL})).expect("load model");
    s
}

fn current(s: &mut Session) -> Json {
    s.execute("dataModel.status", json!({})).expect("status")
}

fn column(status: &Json, table: &str, column: &str) -> Json {
    status["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .find(|entry| entry["name"] == table)
        .and_then(|entry| entry["columns"].as_array())
        .expect("columns")
        .iter()
        .find(|entry| entry["name"] == column)
        .cloned()
        .expect("column")
}

fn table(status: &Json, name: &str) -> Json {
    status["tables"].as_array().expect("tables").iter().find(|entry| entry["name"] == name).cloned().expect("table")
}

fn implicit_names(status: &Json) -> Vec<String> {
    status["implicit_measures"]
        .as_array()
        .expect("implicit measures")
        .iter()
        .map(|measure| measure["name"].as_str().expect("name").to_string())
        .collect()
}

#[test]
fn status_reports_hidden_columns_date_tables_and_summarization_defaults() {
    let mut s = load();
    let status = current(&mut s);
    assert_eq!(column(&status, "Sales", "Region")["hidden"], json!(false));
    assert_eq!(column(&status, "Sales", "Secret")["hidden"], json!(true), "the model file's hidden column is read");
    assert!(column(&status, "Sales", "Amount")["summarize_by"].is_null(), "an absent summarization is null");
    assert!(table(&status, "Calendar")["date_column"].is_null(), "an unmarked table has no date column");
    assert_eq!(status["implicit_measures"], json!([]), "implicit measures are off by default");
    assert_eq!(status["view"], json!({"show_hidden": false, "show_implicit_measures": false}));
}

#[test]
fn implicit_measures_follow_the_toggle_type_and_summarization() {
    let mut s = load();
    s.execute("dataModel.setShowImplicitMeasures", json!({"show": true})).expect("show implicit");
    let status = current(&mut s);
    let names = implicit_names(&status);
    assert!(names.contains(&"Sum of Amount".to_string()), "{names:?}");
    assert!(names.contains(&"Count of Region".to_string()), "text columns count rows: {names:?}");
    assert!(names.contains(&"Count of Date".to_string()), "date columns count rows: {names:?}");
    assert!(!names.contains(&"Sum of Secret".to_string()), "a hidden column has no implicit measure");
    assert_eq!(status["view"]["show_implicit_measures"], json!(true));

    let value = s.execute("dataModel.evaluate", json!({"measure": "Sum of Amount"})).expect("evaluate implicit");
    assert_eq!(value["value"].as_f64(), Some(60.0));
    assert_eq!(value["implicit"], json!(true));
    assert_eq!(value["aggregate"], json!("Sum"));
    assert_eq!(value["formula"], json!("SUM(Sales[Amount])"));
    let value = s.execute("dataModel.evaluate", json!({"measure": "Count of Region"})).expect("evaluate count");
    assert_eq!(value["value"].as_f64(), Some(3.0));
    assert_eq!(value["formula"], json!("COUNTROWS(Sales)"));

    // The model's own Summarize By overrides the type default.
    s.execute("dataModel.setSummarizeBy", json!({"table": "Sales", "column": "Amount", "summarize_by": "average"})).expect("summarize");
    assert_eq!(column(&current(&mut s), "Sales", "Amount")["summarize_by"], json!("Average"));
    let value = s.execute("dataModel.evaluate", json!({"measure": "Average of Amount"})).expect("average implicit");
    assert_eq!(value["value"].as_f64(), Some(20.0));

    // A declared type change changes the default summarization.
    s.execute("dataModel.setColumnType", json!({"table": "Sales", "column": "Amount", "data_type": "String"})).expect("to text");
    s.execute("dataModel.setSummarizeBy", json!({"table": "Sales", "column": "Amount", "summarize_by": null})).expect("reset summary");
    assert!(implicit_names(&current(&mut s)).contains(&"Count of Amount".to_string()));

    s.execute("dataModel.setShowImplicitMeasures", json!({"show": false})).expect("hide implicit");
    assert_eq!(current(&mut s)["implicit_measures"], json!([]), "hiding the implicit measures empties the list");
    assert!(s.execute("dataModel.evaluate", json!({"measure": "Sum of Amount"})).is_err(), "hidden implicit measures do not resolve");
}

#[test]
fn show_hidden_and_hide_a_column() {
    let mut s = load();
    s.execute("dataModel.setColumnHidden", json!({"table": "Sales", "column": "Region"})).expect("hide");
    assert_eq!(column(&current(&mut s), "Sales", "Region")["hidden"], json!(true));
    let view = s.execute("dataModel.view", json!({"table": "Sales"})).expect("view");
    assert_eq!(view["columns"], json!(["Amount"]), "a hidden column leaves the grid");
    assert_eq!(view["hidden"], json!(["Region", "Secret"]));
    let view = s.execute("dataModel.view", json!({"table": "Sales", "show_hidden": true})).expect("view hidden");
    assert_eq!(view["columns"], json!(["Region", "Amount", "Secret"]));

    s.execute("dataModel.setShowHidden", json!({"show": true})).expect("show hidden toggle");
    assert_eq!(current(&mut s)["view"]["show_hidden"], json!(true));
    let view = s.execute("dataModel.view", json!({"table": "Sales"})).expect("view");
    assert_eq!(view["columns"], json!(["Region", "Amount", "Secret"]), "the stored toggle drives the grid");

    s.execute("dataModel.setColumnHidden", json!({"table": "Sales", "column": "Region", "hidden": false})).expect("unhide");
    assert_eq!(column(&current(&mut s), "Sales", "Region")["hidden"], json!(false));
    assert!(s.execute("dataModel.setColumnHidden", json!({"table": "Sales", "column": "Missing"})).is_err());
    assert!(s.execute("dataModel.setShowHidden", json!({})).is_err(), "`show` is required");
}

#[test]
fn changing_a_column_type_validates_every_value_and_is_atomic() {
    let mut s = load();
    // Every value of Amount is a number, so text accepts it.
    let status = s.execute("dataModel.setColumnType", json!({"table": "sales", "column": "amount", "data_type": "string"})).expect("to text");
    assert_eq!(column(&status, "Sales", "Amount")["type"], json!("String"));

    // Region holds text, so Int64 rejects it, naming the offending row, and changes nothing.
    let error =
        s.execute("dataModel.setColumnType", json!({"table": "Sales", "column": "Region", "data_type": "Int64"})).expect_err("text is not Int64");
    let message = error.to_string();
    assert!(message.contains("row 1"), "{message}");
    assert!(message.contains("East"), "{message}");
    assert_eq!(column(&current(&mut s), "Sales", "Region")["type"], json!("String"), "a rejected type change leaves the type");

    // A numeric column accepts every numeric spelling.
    let status = s.execute("dataModel.setColumnType", json!({"table": "Sales", "column": "Amount", "data_type": "Number"})).expect("Number alias");
    assert_eq!(column(&status, "Sales", "Amount")["type"], json!("Double"), "aliases store the canonical name");

    // ISO dates convert to DateTime; a text column that is not dates does not.
    s.execute("dataModel.setColumnType", json!({"table": "Calendar", "column": "Date", "data_type": "String"})).expect("dates as text");
    s.execute("dataModel.setColumnType", json!({"table": "Calendar", "column": "Date", "data_type": "DateTime"})).expect("text back to dates");
    assert!(s.execute("dataModel.setColumnType", json!({"table": "Sales", "column": "Region", "data_type": "DateTime"})).is_err());

    // Booleans accept only TRUE/FALSE (or 0/1).
    let booleans = r#"{
        "tables": [{"name":"Flags","columns":[{"name":"Flag","data_type":"Int64"}],"rows":[[0],[1],[2]]}],
        "relationships": [], "measures": []
    }"#;
    let mut s = Session::new();
    s.execute("dataModel.load", json!({"json": booleans})).expect("load");
    let error =
        s.execute("dataModel.setColumnType", json!({"table": "Flags", "column": "Flag", "data_type": "Boolean"})).expect_err("2 is no boolean");
    assert!(error.to_string().contains("row 3"), "{error}");
    s.execute("dataModel.setColumnType", json!({"table": "Flags", "column": "Flag", "data_type": "Whole Number"})).expect("alias");

    assert!(s.execute("dataModel.setColumnType", json!({"table": "Flags", "column": "Flag", "data_type": "complex"})).is_err(), "unknown type");
    assert!(s.execute("dataModel.setColumnType", json!({"table": "Missing", "column": "Flag", "data_type": "Int64"})).is_err());
}

#[test]
fn mark_and_unmark_a_date_table_after_proving_the_column() {
    let mut s = load();
    let status = s.execute("dataModel.markDateTable", json!({"table": "calendar", "column": "date"})).expect("mark");
    assert_eq!(table(&status, "Calendar")["date_column"], json!("Date"), "the mark keeps the column's spelling");
    let status = s.execute("dataModel.unmarkDateTable", json!({"table": "Calendar"})).expect("unmark");
    assert!(table(&status, "Calendar")["date_column"].is_null());
    assert!(s.execute("dataModel.unmarkDateTable", json!({"table": "Calendar"})).is_err(), "already unmarked");

    for (table_name, expected) in [("Gap", "not contiguous"), ("Duplicate", "repeats"), ("Empty", "no rows")] {
        let error = s.execute("dataModel.markDateTable", json!({"table": table_name, "column": "Date"})).expect_err("invalid date column");
        assert!(error.to_string().contains(expected), "{table_name}: {error}");
    }
    // Row order does not matter to Excel: a contiguous, unique set marks whatever order it is in.
    let status =
        s.execute("dataModel.markDateTable", json!({"table": "Unsorted", "column": "Date"})).expect("an unsorted but contiguous column marks");
    assert_eq!(table(&status, "Unsorted")["date_column"], json!("Date"));
    s.execute("dataModel.unmarkDateTable", json!({"table": "Unsorted"})).expect("unmark unsorted");
    let error = s.execute("dataModel.markDateTable", json!({"table": "Sales", "column": "Region"})).expect_err("text is not a date column");
    assert!(error.to_string().contains("not a date column"), "{error}");
    assert!(s.execute("dataModel.markDateTable", json!({"table": "Calendar", "column": "Missing"})).is_err());

    // A marked date column cannot silently change to a non-date type.
    s.execute("dataModel.markDateTable", json!({"table": "Calendar", "column": "Date"})).expect("mark again");
    let error =
        s.execute("dataModel.setColumnType", json!({"table": "Calendar", "column": "Date", "data_type": "String"})).expect_err("unmark first");
    assert!(error.to_string().contains("unmark"), "{error}");
    s.execute("dataModel.unmarkDateTable", json!({"table": "Calendar"})).expect("unmark");
    s.execute("dataModel.setColumnType", json!({"table": "Calendar", "column": "Date", "data_type": "String"})).expect("now allowed");
}

#[test]
fn summarize_by_rejects_unknown_names_and_resets_to_the_default() {
    let mut s = load();
    assert!(s.execute("dataModel.setSummarizeBy", json!({"table": "Sales", "column": "Region", "summarize_by": "median"})).is_err());
    let status = s.execute("dataModel.setSummarizeBy", json!({"table": "Sales", "column": "Region", "summarize_by": "Count"})).expect("count");
    assert_eq!(column(&status, "Sales", "Region")["summarize_by"], json!("Count"));
    let status = s.execute("dataModel.setSummarizeBy", json!({"table": "Sales", "column": "Region", "summarize_by": null})).expect("reset");
    assert!(column(&status, "Sales", "Region")["summarize_by"].is_null());
    assert!(s.execute("dataModel.setSummarizeBy", json!({"table": "Sales", "column": "Region", "summarize_by": 3})).is_err());
    assert!(s.execute("dataModel.setSummarizeBy", json!({"table": "Sales", "column": "Region"})).is_err());
}

#[test]
fn a_model_document_without_metadata_lists_defaults_and_still_loads() {
    let old = r#"{
        "tables": [{"name":"Old","columns":[{"name":"A","data_type":"Double"},{"name":"B","data_type":"String"}],"rows":[[1,"x"]]}],
        "relationships": [],
        "measures": []
    }"#;
    let mut s = Session::new();
    let status = s.execute("dataModel.load", json!({"json": old})).expect("an old document loads unchanged");
    assert_eq!(column(&status, "Old", "A")["hidden"], json!(false));
    assert!(column(&status, "Old", "A")["summarize_by"].is_null());
    assert!(table(&status, "Old")["date_column"].is_null());
    assert_eq!(status["implicit_measures"], json!([]));
    // The model itself is exactly what the document said.
    assert_eq!(s.execute("dataModel.evaluate", json!({"formula": "SUM(Old[A])"})).expect("eval")["value"], json!(1.0));
}

#[test]
fn the_metadata_commands_are_disabled_without_a_model() {
    let s = Session::new();
    for id in [
        "dataModel.setColumnHidden",
        "dataModel.setColumnType",
        "dataModel.setSummarizeBy",
        "dataModel.markDateTable",
        "dataModel.unmarkDateTable",
        "dataModel.setShowHidden",
        "dataModel.setShowImplicitMeasures",
        "dataModel.setFormat",
    ] {
        assert!(s.commands().iter().any(|command| command.id == id && !command.enabled), "{id} is greyed out until a model loads");
    }
}
