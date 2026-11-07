//! Power Pivot window wiring: every command the window offers runs through its `dataModel.*`
//! engine command, and the window's controls reflect the engine's real state.

use egui::Key;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

const MODEL: &str = r#"{
    "tables": [
        {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"},{"name":"Secret","data_type":"String","hidden":true}],
         "rows":[["East",10,"x"],["West",30,"y"],["East",20,"z"]]},
        {"name":"Regions","columns":[{"name":"Region","data_type":"String"},{"name":"SortKey","data_type":"Int64"}],
         "rows":[["East",2],["West",1]]},
        {"name":"Calendar","columns":[{"name":"Date","data_type":"DateTime"}],"rows":[["2024-01-01"],["2024-01-02"],["2024-01-03"]]}
    ],
    "relationships": [{"from_table":"Sales","from_column":"Region","to_table":"Regions","to_column":"Region"}],
    "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
}"#;

fn open() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.run("dataModel.load", json!({"json": MODEL})).expect("load model");
    let mut h = crate::support::harness(session, 4);
    h.state_mut().open_dialog("powerPivot", json!({}));
    h.run_steps(3);
    h
}

fn values(h: &Harness) -> Json {
    let d = h.state().dialog.as_ref().expect("the Power Pivot dialog is open");
    Json::Object(d.values.clone())
}

fn seen(h: &Harness, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

/// Clicks the last button with this label: the dialog paints over the sheet's ribbon, so labels
/// such as "Home" exist twice and the window's copy comes later.
fn click(h: &mut Harness, label: &str) -> bool {
    let Some(node) = h.query_all_by_role_and_label(Role::Button, label).last() else {
        return false;
    };
    node.click_accesskit();
    h.run_steps(2);
    true
}

/// Clicks the last node with this label whatever its role (combo box options are not buttons).
fn click_any(h: &mut Harness, label: &str) -> bool {
    let Some(node) = h.query_all_by_label(label).last() else {
        return false;
    };
    node.click_accesskit();
    h.run_steps(2);
    true
}

fn click_first(h: &mut Harness, label: &str) -> bool {
    let Some(node) = h.query_all_by_role_and_label(Role::Button, label).next() else {
        return false;
    };
    node.click_accesskit();
    h.run_steps(2);
    true
}

fn set_value(h: &mut Harness, key: &str, value: Json) {
    h.state_mut().dialog.as_mut().expect("dialog").values.insert(key.into(), value);
    h.run_steps(1);
}

/// Types a DAX formula into the Data View formula bar and commits it with Enter, as Excel's
/// formula bar does (the Formatting field is an earlier text input, so the bar's is the last).
fn enter_formula(h: &mut Harness, formula: &str) {
    set_value(h, "formula", json!(formula));
    let field = h.query_all_by_role(Role::TextInput).last().expect("the formula bar has a text field").rect();
    h.query_all_by_role(Role::TextInput).find(|node| node.rect() == field).expect("formula field").focus();
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

#[test]
fn formatting_group_formats_the_selected_measure_through_the_engine() {
    let mut h = open();
    // Select the measure in the Calculation Area: the Formatting group wakes up.
    assert!(click(&mut h, "Total Sales"), "the Calculation Area lists the measure");
    assert_eq!(values(&h)["measureIndex"], json!(0));

    assert!(click(&mut h, "Currency format"), "the currency icon is live with a measure selected");
    {
        let model = values(&h)["model"].clone();
        assert_eq!(model["measures"][0]["format"], json!("$#,##0.00"));
        assert_eq!(values(&h)["error"], json!(""));
    }

    // The format field is the text input holding the field's value (the formula bar's holds a DAX
    // expression, and the app's own inputs come earlier in the tree).
    set_value(&mut h, "formatInput", json!("0.0"));
    let field = h.query_all_by_role(Role::TextInput).find(|node| node.value().as_deref() == Some("0.0")).expect("the Formatting format field").rect();
    h.query_all_by_role(Role::TextInput).find(|node| node.rect() == field).expect("format field").focus();
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(values(&h)["model"]["measures"][0]["format"], json!("0.0"), "the field applies on Enter");

    assert!(click(&mut h, "Increase Decimal"), "increase decimals is live");
    assert_eq!(values(&h)["model"]["measures"][0]["format"], json!("0.00"));
    assert!(click(&mut h, "Decrease Decimal"), "decrease decimals is live");
    assert_eq!(values(&h)["model"]["measures"][0]["format"], json!("0.0"));
    assert!(click(&mut h, "Percent format"));
    assert_eq!(values(&h)["model"]["measures"][0]["format"], json!("0.00%"));

    // The formatted value is what the formula bar shows.
    set_value(&mut h, "implicitMeasure", json!(""));
    enter_formula(&mut h, "[Total Sales]");
    assert_eq!(values(&h)["resultKind"], json!("Measure"));
    assert_eq!(values(&h)["resultText"], json!("6000.00%"), "evaluate returns the formatted display");
}

#[test]
fn data_type_and_summarize_by_controls_edit_model_metadata() {
    let mut h = open();
    assert!(click(&mut h, "Amount"), "select the Amount column in the grid");
    assert!(click(&mut h, "Double"), "the Data Type control shows the column's type");
    assert!(click_any(&mut h, "Decimal"), "choose a new model type");
    assert_eq!(values(&h)["model"]["tables"][0]["columns"][1]["type"], json!("Decimal"), "the data type is stored in the model");

    // A rejected type change keeps the old type and shows the engine's reason.
    assert!(click(&mut h, "Region"), "select the text column");
    assert!(click(&mut h, "String"), "the Data Type control follows the selection");
    assert!(click_any(&mut h, "Int64"), "text cannot become a whole number");
    assert!(seen(&h, "East"), "the error names the offending value");
    assert_eq!(values(&h)["model"]["tables"][0]["columns"][0]["type"], json!("String"), "a rejected change leaves the type");

    // Summarize By is model metadata for the selected column.
    assert!(click(&mut h, "Amount"));
    assert!(click(&mut h, "Advanced"));
    assert!(click(&mut h, "Default"), "the Summarize By control shows the type default");
    assert!(click_any(&mut h, "Average"));
    assert_eq!(values(&h)["model"]["tables"][0]["columns"][1]["summarize_by"], json!("Average"));

    // Show Implicit Measures exposes the derived measures and evaluates them.
    assert!(click(&mut h, "Show Implicit Measures"));
    assert_eq!(values(&h)["showImplicit"], json!(true));
    assert!(seen(&h, "Average of Amount"), "the Calculation Area lists the implicit measure");
    set_value(&mut h, "implicitMeasure", json!("Average of Amount"));
    enter_formula(&mut h, "[Average of Amount]");
    assert_eq!(values(&h)["resultText"], json!("20"));
}

#[allow(clippy::cognitive_complexity)] // Walks every Data View command the test covers.
#[test]
fn sort_filter_find_and_hide_wire_the_data_view_to_engine_commands() {
    let mut h = open();
    assert!(click(&mut h, "Region"), "select the Region column");
    assert!(click(&mut h, "Sort Z to A"), "the ribbon sorts the view");
    {
        let data = values(&h)["tableData"].clone();
        assert_eq!(data["rows"][0][0], json!("West"), "descending puts West first: {data}");
        assert_eq!(data["sort"]["column"], json!("Region"));
    }
    assert!(click(&mut h, "Clear Sort"));
    assert_eq!(values(&h)["tableData"]["rows"][0][0], json!("East"));

    // The header menu filters through `dataModel.setFilter`; Clear All Filters clears it.
    assert!(click_first(&mut h, "▾"), "the first header filter menu opens");
    assert!(click(&mut h, "West"), "choose one allowed value");
    {
        let data = values(&h)["tableData"].clone();
        assert_eq!(data["visible_rows"], json!(1), "{data}");
        assert_eq!(data["rows"][0][0], json!("West"));
        assert_eq!(data["filters"], json!([{"column": "Region", "values": ["West"]}]));
    }
    assert!(click(&mut h, "Clear All Filters"));
    assert_eq!(values(&h)["tableData"]["visible_rows"], json!(3));

    // Find searches the model and jumps the grid to the row it found.
    assert!(click(&mut h, "Find"), "the Find command opens its overlay");
    set_value(&mut h, "findQuery", json!("West"));
    assert!(click(&mut h, "Find"), "the overlay's Find runs dataModel.find");
    assert!(seen(&h, "Sales · Region · row 2 · West"), "the match names table, column and row");
    assert!(click(&mut h, "Sales · Region · row 2 · West"));
    {
        let v = values(&h);
        assert_eq!(v["overlay"], json!(""));
        assert_eq!(v["findRow"], json!(1));
        assert_eq!(v["findColumnIndex"], json!(0));
    }

    // Show Hidden re-reads the grid with the hidden column visible.
    assert_eq!(values(&h)["tableData"]["columns"], json!(["Region", "Amount"]), "the hidden column is not in the grid");
    assert!(click(&mut h, "Show Hidden"));
    assert_eq!(values(&h)["showHidden"], json!(true));
    assert_eq!(values(&h)["tableData"]["columns"], json!(["Region", "Amount", "Secret"]));
    assert!(seen(&h, "Secret"), "the grid now paints the hidden column");

    // Design ▸ Columns hides and shows the selected column (`dataModel.setColumnHidden`). The
    // model already marks Secret hidden, so the control offers Unhide; Show Hidden was a view
    // setting and did not change the metadata.
    assert!(click(&mut h, "Secret"), "select the now-visible column");
    assert!(click(&mut h, "Design"));
    assert!(click(&mut h, "Unhide Column"), "the control follows the model's hidden flag");
    assert_eq!(values(&h)["model"]["tables"][0]["columns"][2]["hidden"], json!(false), "unhide is model metadata");
    assert!(seen(&h, "Hide Column"), "the control flips once the column is visible");
    assert!(click(&mut h, "Hide Column"));
    assert_eq!(values(&h)["model"]["tables"][0]["columns"][2]["hidden"], json!(true));
}

#[test]
fn date_table_and_sort_by_column_wire_relationships_and_validation() {
    let mut h = open();
    // Mark the Calendar table's Date column as its date table.
    assert!(click(&mut h, "Calendar"), "select the Calendar table tab");
    assert!(click(&mut h, "Date"), "select the date column");
    assert!(click(&mut h, "Design"));
    // Validate runs `dataModel.validate` and reports the counts over the fixture model.
    assert!(click(&mut h, "Validate"), "the Relationships group offers Validate");
    assert_eq!(values(&h)["validateReport"]["findings"], json!([]), "the fixture model is clean: {:?}", values(&h)["validateReport"]);
    assert!(seen(&h, "Validate: 0 error(s), 0 warning(s)"), "the window shows what the engine found");
    assert!(seen(&h, "No integrity problems found."));
    assert!(click(&mut h, "Mark as Date Table"), "the mark is live for a DateTime column");
    assert_eq!(values(&h)["model"]["tables"][2]["date_column"], json!("Date"));
    assert!(seen(&h, "Date column: Date"));
    assert!(click(&mut h, "Unmark"));
    assert!(values(&h)["model"]["tables"][2]["date_column"].is_null());

    // A text column cannot be marked: the control is present but disabled, so the table stays
    // unmarked (the engine command's own refusal is covered by the engine tests).
    assert!(click(&mut h, "Home"));
    assert!(click(&mut h, "Sales"));
    assert!(click(&mut h, "Region"));
    assert!(click(&mut h, "Design"));
    assert!(seen(&h, "Mark as Date Table") || seen(&h, "Mark as\nDate Table"), "the disabled control is still shown");
    assert!(values(&h)["model"]["tables"][0]["date_column"].is_null(), "Sales was not marked");

    // Sort by Column goes through the Sales↔Regions relationship: West (SortKey 1) comes first.
    assert!(click(&mut h, "Home"));
    assert!(click(&mut h, "Sort by Column"), "the relationship makes Sort by Column available");
    assert!(click(&mut h, "Sort"));
    {
        let v = values(&h);
        assert_eq!(v["overlay"], json!(""), "Sort closes the dialog on success");
        let data = v["tableData"].clone();
        assert_eq!(data["sort"]["by_table"], json!("Regions"));
        assert_eq!(data["sort"]["by_column"], json!("SortKey"));
        assert_eq!(data["rows"][0][0], json!("West"));
    }

    // AutoSum adds a real SUM measure for the selected numeric column.
    assert!(click(&mut h, "Amount"));
    assert!(click(&mut h, "AutoSum"));
    let measures = values(&h)["model"]["measures"].as_array().cloned().unwrap_or_default();
    let added = measures.iter().find(|measure| measure["name"] == json!("Sum of Amount")).expect("AutoSum added a measure");
    assert_eq!(added["formula"], json!("SUM(Sales[Amount])"));
}
