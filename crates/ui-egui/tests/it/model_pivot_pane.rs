//! The data-model PivotTable Fields pane: hidden until the workbook has a model PivotTable (or the
//! user reopens it), groups the model's tables, columns and measures, and Apply runs
//! `dataModel.createPivot` with the pending placements.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
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

/// A model with no measures: the pane's values area names the column alone.
const MODEL_NO_MEASURES: &str = r##"{
    "tables": [
        {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"}],
         "rows":[["East",10],["West",30],["East",20]]}
    ],
    "relationships": [],
    "measures": []
}"##;

/// A blank workbook with the model loaded (`loads_model: false` for one without it).
fn harness(loads_model: bool) -> Harness {
    harness_with(if loads_model { MODEL } else { "" })
}

/// A blank workbook with `model` loaded (an empty string loads nothing).
fn harness_with(model: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    if !model.is_empty() {
        session.run("dataModel.load", json!({"json": model})).expect("load the model");
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
    // A cell with content is in the way of a new report: the engine says where and the pane prints
    // the message where the person can read it.
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "busy"})).expect("write a cell");
    h.state_mut().run("selection.set", json!({"cell": "B2"})).expect("select the busy cell");
    h.run_steps(2);
    click(&mut h, "Show PivotTable Fields");
    click(&mut h, "Region");
    click(&mut h, "Amount");
    click(&mut h, "Apply");
    let error = "There's already data in Sheet1!B2. Do you want to replace it? (Run again with replace: true to overwrite.)";
    assert!(h.query_by_label(error).is_some(), "the engine's message is printed in the pane");
    assert!(pane_open(&h), "the pane stays open on the failed report");
    assert!(shown(&h, "Sum of Amount ▾"), "the pending placement is kept for the next try");
}

#[test]
fn a_numeric_column_dropped_in_values_is_summed_without_a_measure() {
    let mut h = harness(true);
    click(&mut h, "Show PivotTable Fields");
    click(&mut h, "Region");
    click(&mut h, "Amount");
    assert!(shown(&h, "Sum of Amount ▾"), "the column's chip names the aggregation the command applies");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["values"], json!([{"column": "Sales[Amount]", "aggregate": "Sum"}]));
    assert_eq!(params["row_fields"], json!(["Sales[Region]"]));
    h.run_steps(2);
    assert!(shown(&h, "Sum of Amount ▾"), "the applied report keeps its value chip");
    assert_eq!(cell(&h, 0, 1), "Sum of Amount", "the report's value header");
    // The model's rows are East 10, West 30, East 20: each region sums its own rows.
    assert_eq!(cell(&h, 1, 1), "30", "East sums to 30");
    assert_eq!(cell(&h, 2, 1), "30", "West sums to 30");
}

#[test]
fn the_value_chip_menu_changes_the_aggregation() {
    let mut h = harness(true);
    click(&mut h, "Show PivotTable Fields");
    click(&mut h, "Region");
    click(&mut h, "Amount");
    click(&mut h, "Sum of Amount ▾");
    click(&mut h, "Summarize by Average");
    assert!(shown(&h, "Average of Amount ▾"), "the chip follows the chosen aggregation");
    click(&mut h, "Apply");
    assert_eq!(last_command(&h, "dataModel.createPivot")["values"], json!([{"column": "Sales[Amount]", "aggregate": "Average"}]));
    h.run_steps(2);
    assert_eq!(cell(&h, 1, 1), "15", "East averages 10 and 20");
}

#[test]
fn a_model_without_measures_still_carries_a_pivot() {
    let mut h = harness_with(MODEL_NO_MEASURES);
    click(&mut h, "Show PivotTable Fields");
    click(&mut h, "Region");
    click(&mut h, "Amount");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["values"], json!([{"column": "Sales[Amount]", "aggregate": "Sum"}]));
    assert_eq!(params["measure"], json!(""), "no measure exists; the command takes the column's table");
    h.run_steps(2);
    assert_eq!(cell(&h, 0, 1), "Sum of Amount");
    assert_eq!(cell(&h, 1, 1), "30", "East sums to 30");
}

#[test]
fn moving_a_field_to_filters_keeps_every_value() {
    let mut h = harness(true);
    with_pivot(&mut h, Some("Mine"));
    click(&mut h, "Region ▾");
    click(&mut h, "Move to Report Filter");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    let filters = params["filters"].as_array().cloned().unwrap_or_default();
    assert_eq!(filters.len(), 1, "the field moved to Filters reports one filter");
    assert_eq!(filters[0]["field"], json!("Sales[Region]"));
    // Excel moves a field to Filters as `(All)`: the page field names the column and keeps every
    // value, so no member list is sent and no row is dropped before the user picks anything.
    assert!(filters[0].get("allowed").is_none(), "an untouched filter is (All), not a member list: {filters:?}");

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

// ---------------------------------------------------------------- search and drag and drop

/// Sets the field search box's query through the pane's control-channel state.
fn set_query(h: &Harness, query: &str) {
    gridcraft_ui_egui::model_pivot_pane::set_search(&h.ctx, query);
}

/// One area's drop zone, as the pane recorded it this frame.
fn area(h: &Harness, title: &str) -> egui::Rect {
    gridcraft_ui_egui::model_pivot_pane::area_rect(&h.ctx, title).unwrap_or_else(|| panic!("the `{title}` area painted a drop zone"))
}

/// A `ui.drag` from the control channel, offered batch by batch like the native frame hook does.
fn control_drag(h: &mut Harness, from: Pos2, to: Pos2) {
    assert_eq!(control(h, "ui.drag", json!({"x": from.x, "y": from.y, "toX": to.x, "toY": to.y})), json!({"ok": true, "result": null}));
    while let Some(batch) = h.state_mut().synthetic.pop_front() {
        h.input_mut().events.extend(batch);
        h.step();
    }
    h.run_steps(2);
}

/// Drags the field or chip labelled `label` onto `area`.
fn drag_to_area(h: &mut Harness, label: &str, area: egui::Rect) {
    let from = h.get_by_label(label).rect().center();
    let to = area.center();
    control_drag(h, from, to);
}

#[test]
fn search_filters_the_tree_and_clearing_restores_it() {
    let mut h = harness(true);
    with_pivot(&mut h, None);
    for label in ["Sales", "Region", "Category", "Amount", "Total Sales", "Products", "Name"] {
        assert!(shown(&h, label), "the unfiltered tree lists `{label}`");
    }
    assert!(!shown(&h, "No fields match"));

    // A field name narrows every table to the matching children, whatever the case.
    set_query(&h, "CAT");
    h.run_steps(2);
    assert!(shown(&h, "Category"), "the matching column stays");
    assert!(shown(&h, "Sales"), "its table stays as the group header");
    for label in ["Region", "Amount", "Total Sales", "Products", "Name"] {
        assert!(!shown(&h, label), "`{label}` does not match the query");
    }

    // A table name keeps that table's children even when none of them match.
    set_query(&h, "products");
    h.run_steps(2);
    assert!(shown(&h, "Products") && shown(&h, "Name"), "a matching table shows its fields");
    assert!(!shown(&h, "Region"), "the other table is gone entirely");

    // Nothing matches: the pane says so instead of an empty tree.
    set_query(&h, "zzz");
    h.run_steps(2);
    assert!(shown(&h, "No fields match"), "the pane reports an empty result");
    assert!(!shown(&h, "Products") && !shown(&h, "Name"));

    // Clearing restores the whole tree, and the placed fields are still where they were.
    set_query(&h, "");
    h.run_steps(2);
    assert!(!shown(&h, "No fields match"));
    for label in ["Sales", "Region", "Category", "Amount", "Total Sales", "Products", "Name"] {
        assert!(shown(&h, label), "clearing the box restores `{label}`");
    }
    assert!(shown(&h, "Region ▾") && shown(&h, "Total Sales ▾"), "the report's own placements are untouched");
    h.check_accessibility();
}

#[test]
fn dragging_a_column_from_the_tree_to_rows_sends_it_on_apply() {
    let mut h = harness(true);
    click(&mut h, "Show PivotTable Fields");
    assert!(pane_open(&h));
    assert!(!shown(&h, "Category ▾"), "nothing is placed yet");

    let rows = area(&h, "Rows");
    drag_to_area(&mut h, "Category", rows);
    assert!(shown(&h, "Category ▾"), "the drop placed the column in Rows");

    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["row_fields"], json!(["Sales[Category]"]), "the dragged column travels as a field reference");
    assert_eq!(params["column_fields"], json!([]));
    assert_eq!(params["replace"], json!(false), "a new report is still new");
}

#[test]
fn dragging_between_rows_and_columns_moves_the_field() {
    let mut h = harness(true);
    with_pivot(&mut h, Some("Mine"));
    assert!(shown(&h, "Region ▾"), "the report's row field is placed");

    let columns = area(&h, "Columns");
    drag_to_area(&mut h, "Region ▾", columns);
    assert!(shown(&h, "Region ▾"), "the chip still exists, now in Columns");

    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["row_fields"], json!([]), "the field left Rows");
    assert_eq!(params["column_fields"], json!(["Sales[Region]"]), "and landed in Columns");
    assert_eq!(params["name"], json!("Mine"), "the report it belongs to is unchanged");
}

#[test]
fn dragging_a_measure_from_the_tree_to_values_adds_it() {
    let mut h = harness(true);
    click(&mut h, "Show PivotTable Fields");
    assert!(!shown(&h, "Total Sales ▾"));

    let values = area(&h, "Values");
    drag_to_area(&mut h, "Total Sales", values);
    assert!(shown(&h, "Total Sales ▾"), "the measure sits in Σ Values");

    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["values"], json!(["Total Sales"]), "the measure travels in `values`");
    assert_eq!(params["measure"], json!("Total Sales"));
}

#[test]
fn dragging_within_rows_reorders_the_placed_fields() {
    let mut h = harness(true);
    with_pivot(&mut h, Some("Mine"));
    click(&mut h, "Category");
    assert!(shown(&h, "Category ▾"));

    // Drop Region below Category, still inside the Rows zone: the place is the gap, not the end.
    let rows = area(&h, "Rows");
    let below = pos2(rows.center().x, h.get_by_label("Category ▾").rect().center().y + 8.0);
    assert!(rows.contains(below), "the drop point is inside Rows");
    let from = h.get_by_label("Region ▾").rect().center();
    control_drag(&mut h, from, below);

    click(&mut h, "Apply");
    assert_eq!(
        last_command(&h, "dataModel.createPivot")["row_fields"],
        json!(["Sales[Category]", "Sales[Region]"]),
        "the dragged field takes the gap it was dropped in"
    );
}

#[test]
fn dragging_a_placed_field_out_of_the_areas_removes_it() {
    let mut h = harness(true);
    with_pivot(&mut h, Some("Mine"));
    assert!(shown(&h, "Region ▾"));
    let header = h.get_by_label("PivotTable Fields").rect().center();
    let from = h.get_by_label("Region ▾").rect().center();
    control_drag(&mut h, from, header);
    assert!(!shown(&h, "Region ▾"), "dropping outside every area removes the field");
    click(&mut h, "Category");
    click(&mut h, "Apply");
    assert_eq!(last_command(&h, "dataModel.createPivot")["row_fields"], json!(["Sales[Category]"]));
}

#[test]
fn the_tick_box_path_still_places_fields() {
    let mut h = harness(true);
    click(&mut h, "Show PivotTable Fields");
    click(&mut h, "Region");
    click(&mut h, "Total Sales");
    assert!(shown(&h, "Region ▾") && shown(&h, "Total Sales ▾"));
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(params["row_fields"], json!(["Sales[Region]"]));
    assert_eq!(params["values"], json!(["Total Sales"]));
}

/// The pane's control-channel state: the query a control request sets, and the drop zones a
/// caller can aim a `ui.drag` at.
#[test]
fn the_pane_exposes_the_search_query_and_its_drop_zones() {
    let mut h = harness(true);
    with_pivot(&mut h, None);
    set_query(&h, "reg");
    h.run_steps(2);
    assert_eq!(gridcraft_ui_egui::model_pivot_pane::search(&h.ctx), "reg", "the pane reports the query it renders");
    assert!(shown(&h, "Region") && !shown(&h, "Category"));
    for title in ["Filters", "Columns", "Rows", "Values"] {
        assert!(area(&h, title).is_positive(), "the `{title}` area reports its drop zone");
    }
}

#[test]
fn the_search_box_and_a_drag_highlight_render() {
    let Ok(dir) = std::env::var("GRIDCRAFT_PANE_SNAPSHOT") else { return };
    let mut h = harness(true);
    click(&mut h, "Show PivotTable Fields");
    set_query(&h, "reg");
    h.run_steps(2);

    // A drag in flight: pressed on the tree row, moved over Rows, not yet released.
    let from = h.get_by_label("Region").rect().center();
    let to = area(&h, "Rows").center();
    h.input_mut().events.extend([
        Event::PointerMoved(from),
        Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.step();
    for i in 1..=8 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 8.0)));
        h.step();
    }

    std::fs::create_dir_all(&dir).expect("the snapshot directory");
    h.render().unwrap().save(std::path::Path::new(&dir).join("model_pivot_search_drag.png")).unwrap();

    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(shown(&h, "Region ▾"), "the drag still lands after the snapshot");
}

#[test]
fn typing_in_the_search_box_filters_the_tree() {
    let mut h = harness(true);
    with_pivot(&mut h, None);
    // The box is a normal single-line text edit: focus it and type, as Tab and the keyboard would.
    h.ctx.memory_mut(|memory| memory.request_focus(gridcraft_ui_egui::model_pivot_pane::search_id()));
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("cat".into()));
    h.run_steps(2);
    assert_eq!(gridcraft_ui_egui::model_pivot_pane::search(&h.ctx), "cat", "the box holds what was typed");
    assert!(shown(&h, "Category"), "the typed query filters the tree");
    assert!(!shown(&h, "Region") && !shown(&h, "Name"));
    // Clearing the box restores the whole tree.
    set_query(&h, "");
    h.run_steps(2);
    assert!(shown(&h, "Region") && shown(&h, "Name"), "the cleared tree is whole again");
}

#[test]
fn dragging_a_column_onto_filters_makes_it_a_page_filter() {
    let mut h = harness(true);
    click(&mut h, "Show PivotTable Fields");
    let filters = area(&h, "Filters");
    drag_to_area(&mut h, "Region", filters);
    assert!(shown(&h, "Region ▾"), "the dragged column sits in Filters");
    // The report needs a measure; tick one so Apply can write the report.
    click(&mut h, "Total Sales");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    let entries = params["filters"].as_array().cloned().unwrap_or_default();
    assert_eq!(entries.len(), 1, "a dropped filter field reports one filter");
    assert_eq!(entries[0]["field"], json!("Sales[Region]"));
    assert!(entries[0].get("allowed").is_none(), "the drop makes the page field (All), like the menu does: {entries:?}");
}

#[test]
fn opening_the_pane_outside_every_report_starts_a_new_one() {
    let mut h = harness(true);
    with_pivot(&mut h, Some("Mine"));
    // Move the active cell outside the report: the pane closes to its strip.
    h.state_mut().run("selection.set", json!({"cell": "H20"})).expect("select outside");
    h.run_steps(2);
    assert!(!pane_open(&h), "the pane hides outside its report");
    assert!(shown(&h, "Show PivotTable Fields"));
    // One click opens it, unbound: Apply must create a report, not rewrite the existing one.
    click(&mut h, "Show PivotTable Fields");
    h.run_steps(1);
    assert!(pane_open(&h), "a single click opens the pane");
    click(&mut h, "Category");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert!(params.get("name").is_none(), "an unbound pane sends no name: {params:?}");
    assert_eq!(params["replace"], json!(false), "a new report does not replace anything");

    // That Apply wrote a report at the active cell; the binding must not outlive the selection.
    // Close the pane, leave every report (the first Apply wrote one at H20), reopen from the
    // strip, Apply: a new report, not a rewrite.
    click(&mut h, "Close");
    h.state_mut().run("selection.set", json!({"cell": "K50"})).expect("select outside");
    h.run_steps(2);
    assert!(!pane_open(&h), "the closed pane hides behind its strip");
    assert!(shown(&h, "Show PivotTable Fields"));
    click(&mut h, "Show PivotTable Fields");
    assert!(pane_open(&h), "the strip brings the pane back");
    click(&mut h, "Category");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert!(params.get("name").is_none(), "a reopened pane sends no name: {params:?}");
    assert_eq!(params["replace"], json!(false), "the reopened pane does not rewrite the report it wrote before");
}
