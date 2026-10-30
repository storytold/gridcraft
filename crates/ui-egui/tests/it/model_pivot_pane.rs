//! The data-model PivotTable Fields pane: hidden until the workbook has a model PivotTable (or the
//! user reopens it), groups the model's tables, columns and measures, and Apply runs
//! `dataModel.createPivot` with the pending placements.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

const MODEL: &str = r##"{
    "tables": [
        {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Category","data_type":"String"},{"name":"Amount","data_type":"Double"}],
         "rows":[["East","A",10],["West","B",30],["East","A",20]]},
        {"name":"Products","columns":[{"name":"Name","data_type":"String"}],"rows":[["A"],["B"]]}
    ],
    "relationships": [],
    "measures": [
        {"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales","format":"#,##0"},
        {"name":"Order Count","formula":"COUNTROWS(Sales)","table":"Sales"}
    ]
}"##;

/// A blank workbook with the model loaded (`loads_model: false` for one without it).
fn harness(loads_model: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    if loads_model {
        session.run("dataModel.load", json!({"json": MODEL})).expect("load the model");
    }
    crate::support::harness(session, 4)
}

/// The pane's frame is open: its title is painted.
fn pane_open(h: &Harness) -> bool {
    h.query_by_label("PivotTable Fields").is_some()
}

fn shown(h: &Harness, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn click(h: &mut Harness, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
}

/// The last command the pane ran.
fn last_command(h: &Harness, id: &str) -> Json {
    h.state()
        .session
        .journal
        .iter()
        .rev()
        .find(|(command, _)| command == id)
        .map(|(_, params)| params.clone())
        .unwrap_or_else(|| panic!("`{id}` never ran"))
}

fn cell(h: &Harness, row: u32, col: u32) -> String {
    h.state().session.active().expect("doc").wb.active().expect("sheet").value(CellRef::new(row, col)).display()
}

/// Creates a model PivotTable and puts the active cell inside it.
fn with_pivot(h: &mut Harness, name: Option<&str>) -> Json {
    let mut params = json!({"measure": "Total Sales", "row_fields": ["Sales[Region]"], "at": "A3"});
    if let Some(name) = name {
        params["name"] = json!(name);
    }
    let reply = h.state_mut().run("dataModel.createPivot", params).expect("create the pivot");
    h.state_mut().run("selection.set", json!({"cell": "B4"})).expect("select inside the report");
    h.run_steps(3);
    reply
}

#[test]
fn the_pane_stays_hidden_without_a_model_pivot() {
    let mut h = harness(true);
    h.run_steps(2);
    assert!(!pane_open(&h), "a data model alone does not show the pane");
    assert!(!shown(&h, "Rows"), "no area lists are painted");
    assert!(!shown(&h, "Apply"), "nothing to apply yet");

    // A workbook with no model at all has nothing to show either — not even the reopen strip.
    let h = harness(false);
    assert!(!pane_open(&h));
    assert!(!shown(&h, "Show PivotTable Fields"));
}

#[test]
fn the_pane_lists_the_model_tables_columns_and_measures() {
    let mut h = harness(true);
    with_pivot(&mut h, None);
    assert!(pane_open(&h), "the active cell is inside a model PivotTable");
    for label in ["Sales", "Region", "Category", "Amount", "Total Sales", "Order Count", "Products", "Name"] {
        assert!(shown(&h, label), "the checklist lists `{label}`");
    }
    for label in ["Rows", "Columns", "Filters", "Values"] {
        assert!(shown(&h, label), "the `{label}` area is painted");
    }
    assert!(shown(&h, "Region ▾"), "the pivot's row field sits in Rows");
    assert!(shown(&h, "Total Sales ▾"), "the pivot's measure sits in Values");
    assert!(h.state().dialog.is_none(), "the pane is not a modal dialog");
    h.check_accessibility();
}

#[test]
fn ticking_fields_then_apply_runs_the_command_and_shows_the_report() {
    let mut h = harness(true);
    // No PivotTable yet: the reopen strip opens the pane and Apply creates one.
    assert!(!shown(&h, "Apply"));
    click(&mut h, "Show PivotTable Fields");
    assert!(pane_open(&h), "the reopen strip brings the pane back");
    assert!(shown(&h, "Apply"));
    assert!(shown(&h, "Refresh"));

    click(&mut h, "Region");
    click(&mut h, "Total Sales");
    assert!(shown(&h, "Region ▾"), "ticking a text column queues it in Rows");
    assert!(shown(&h, "Total Sales ▾"), "ticking a measure queues it in Values");

    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["row_fields"], json!(["Sales[Region]"]), "the ticked column travels as a field reference");
    assert_eq!(params["values"], json!(["Total Sales"]), "the ticked measure travels in `values`");
    assert_eq!(params["measure"], json!("Total Sales"), "the first value is the pivot's measure");
    assert_eq!(params["layout"], json!("compact"));
    assert_eq!(params["grand_totals"], json!(false));
    assert_eq!(params["replace"], json!(false), "a new report never overwrites the cells under it");
    assert!(params.get("name").is_none(), "a new report lets the engine name it");

    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert!(pane_open(&h), "the pane stays open on the report it made");
    assert!(shown(&h, "Region ▾"), "the pane now shows the report's own row field");
    assert!(shown(&h, "Total Sales ▾"), "and its measure");
    assert_eq!(cell(&h, 0, 0), "Row Labels", "the report is on the sheet");
}

/// One control-channel call, as an agent's JSON line would arrive.
fn control(h: &mut Harness, method: &str, params: Json) -> Json {
    match gridcraft_ui_egui::control::handle(h.state_mut(), &egui::Context::default(), method, &params) {
        gridcraft_ui_egui::control::Outcome::Done(reply) => reply,
        gridcraft_ui_egui::control::Outcome::Screenshot { .. } => panic!("{method} asked for a screenshot"),
    }
}

/// A `ui.click` from the control channel. The app queues one synthetic batch per pass and the
/// native frame hook offers them; the harness has no hook, so this offers them the same way.
fn control_click(h: &mut Harness, pos: egui::Pos2) {
    assert_eq!(control(h, "ui.click", json!({"x": pos.x, "y": pos.y})), json!({"ok": true, "result": null}));
    while let Some(batch) = h.state_mut().synthetic.pop_front() {
        h.input_mut().events.extend(batch);
        h.step();
    }
    h.run_steps(2);
}

#[test]
fn the_control_channel_opens_the_pane_places_fields_and_applies() {
    let mut h = harness(true);
    let tab = h.get_by_label("Show PivotTable Fields").rect().center();
    control_click(&mut h, tab);
    assert!(pane_open(&h), "the click on the strip opened the pane");
    for label in ["Region", "Total Sales"] {
        let at = h.get_by_label(label).rect().center();
        control_click(&mut h, at);
    }
    assert!(shown(&h, "Region ▾") && shown(&h, "Total Sales ▾"), "both clicks placed their field");
    let apply = h.get_by_label("Apply").rect().center();
    control_click(&mut h, apply);
    let journal = control(&mut h, "engine.journal", json!({}));
    let entries = journal["result"].as_array().cloned().unwrap_or_default();
    let ran = entries.iter().rev().find(|entry| entry["command"] == json!("dataModel.createPivot")).expect("Apply is in the journal");
    assert_eq!(ran["params"]["row_fields"], json!(["Sales[Region]"]));
    assert_eq!(ran["params"]["values"], json!(["Total Sales"]));
}

#[test]
fn applying_to_an_existing_pivot_keeps_its_place_and_refreshes_the_pane() {
    let mut h = harness(true);
    with_pivot(&mut h, Some("Mine"));
    assert!(pane_open(&h));
    assert!(shown(&h, "Region ▾"));
    click(&mut h, "Category");
    assert!(shown(&h, "Category ▾"), "the new row field is queued");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["name"], json!("Mine"), "the pane applies to the report it is bound to");
    assert_eq!(params["replace"], json!(true), "replacing a report reuses its place");
    assert_eq!(params["sheet"], json!("Sheet1"));
    assert_eq!(params["at"], json!("A3"));
    assert_eq!(params["row_fields"], json!(["Sales[Region]", "Sales[Category]"]));
    h.run_steps(2);
    assert!(pane_open(&h));
    assert!(shown(&h, "Region ▾") && shown(&h, "Category ▾"), "the pane re-reads the report it just wrote");
    assert_eq!(cell(&h, 2, 0), "Row Labels", "the report still starts at A3");
}

#[test]
fn a_command_error_surfaces_in_the_pane() {
    let mut h = harness(true);
    with_pivot(&mut h, Some("Mine"));
    // A numeric model column is not a measure, so the values area cannot sum it: the engine says
    // why and the pane prints the message where the person can read it.
    click(&mut h, "Total Sales");
    click(&mut h, "Amount");
    assert!(shown(&h, "Amount ▾"), "a numeric column queued in Values");
    click(&mut h, "Apply");
    let error = "no measure named `Sales[Amount]` in the data model";
    assert!(h.query_by_label(error).is_some(), "the engine's message is printed in the pane");
    assert!(pane_open(&h), "the pane stays open on the failed report");
    assert!(shown(&h, "Amount ▾"), "the pending placement is kept for the next try");
}

#[test]
fn moving_a_field_to_filters_fills_its_allowed_values() {
    let mut h = harness(true);
    with_pivot(&mut h, Some("Mine"));
    click(&mut h, "Region ▾");
    click(&mut h, "Move to Report Filter");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    let filters = params["filters"].as_array().cloned().unwrap_or_default();
    assert_eq!(filters.len(), 1, "the field moved to Filters reports one filter");
    assert_eq!(filters[0]["field"], json!("Sales[Region]"));
    let mut allowed: Vec<String> =
        filters[0]["allowed"].as_array().into_iter().flatten().filter_map(|value| value.as_str().map(str::to_string)).collect();
    allowed.sort();
    assert_eq!(allowed, ["East", "West"], "the filter's members come from the model");

    // A filtered report writes its page rows above the body, and the pane asks for the PivotTable's
    // top-left again, so a second Apply does not walk the report down the sheet.
    h.run_steps(2);
    h.state_mut().run("selection.set", json!({"cell": "B6"})).expect("select the filtered report");
    h.run_steps(2);
    click(&mut h, "Apply");
    assert_eq!(last_command(&h, "dataModel.createPivot")["at"], json!("A3"), "the report keeps its top-left cell");
}

#[test]
fn apply_passes_every_measure_in_the_values_area() {
    let mut h = harness(true);
    click(&mut h, "Show PivotTable Fields");
    click(&mut h, "Region");
    click(&mut h, "Total Sales");
    click(&mut h, "Order Count");
    assert!(shown(&h, "Σ Values"), "two measures title the area Σ Values");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["values"], json!(["Total Sales", "Order Count"]), "both measures travel in `values`");
    assert_eq!(params["measure"], json!("Total Sales"), "the first measure groups the rows");
    assert_eq!(params["row_fields"], json!(["Sales[Region]"]));
    h.run_steps(2);
    assert!(shown(&h, "Total Sales ▾") && shown(&h, "Order Count ▾"), "both measures sit in Σ Values afterwards");
}
