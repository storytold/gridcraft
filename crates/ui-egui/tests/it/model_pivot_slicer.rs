//! The data-model PivotTable Fields pane's slicers: a filter field's members as tiles in the pane,
//! the tiles written as the PivotTable's page filter on Apply, and the timeline a date field would
//! need reported instead of faked.
//!
//! A sheet slicer object (`Slicer {table, column}` in the model) binds to a worksheet table's
//! column and writes the sheet's `AutoFilter`; a data-model field is neither, so the pane draws its
//! slicer and Apply sends the selection as `dataModel.createPivot`'s `filters` — the page field.
//! A page field may not also sit on an axis (Excel's one-field-one-area rule, enforced by
//! `check_axes`), so the report groups by one column while the slicer filters another.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

const MODEL: &str = r##"{
    "tables": [
        {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Category","data_type":"String"},{"name":"Amount","data_type":"Double"}],
         "rows":[["East","A",10],["West","B",30],["East","A",20],["North","C",40]]},
        {"name":"Calendar","columns":[{"name":"Date","data_type":"DateTime"}],"rows":[["2024-01-01"],["2024-01-02"]]}
    ],
    "relationships": [],
    "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
}"##;

/// A blank workbook with the model loaded, in a window tall enough for the whole pane: the slicer
/// card sits at the end of the pane's scroll area, and at 1200×800 it is below the fold, where a
/// synthetic click cannot reach it (a person scrolls the pane, as in Excel's field list).
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.run("dataModel.load", json!({"json": MODEL})).expect("load the model");
    crate::common::harness_with(crate::common::builder().with_size(egui::vec2(1200.0, 1500.0)), crate::common::app(session), 4)
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

/// Creates a model PivotTable at A3 grouped by `row_field` and puts the active cell inside it.
fn with_pivot(h: &mut Harness, row_field: &str) {
    h.state_mut().run("dataModel.createPivot", json!({"measure": "Total Sales", "row_fields": [row_field], "at": "A3"})).expect("create the pivot");
    h.state_mut().run("selection.set", json!({"cell": "B4"})).expect("select inside the report");
    h.run_steps(3);
}

/// Moves a field to Filters from its area chip's menu and gives it a slicer.
fn add_slicer(h: &mut Harness, caption: &str) {
    click(h, caption);
    click(h, &format!("{caption} ▾"));
    click(h, "Move to Report Filter");
    click(h, &format!("{caption} ▾"));
    click(h, "Add as Slicer");
}

/// The report's filters as the command received them.
fn filters_of(params: &Json) -> Vec<(String, Vec<String>)> {
    params["filters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|filter| {
            let field = filter["field"].as_str().unwrap_or_default().to_string();
            let allowed = filter["allowed"].as_array().into_iter().flatten().filter_map(|value| value.as_str().map(str::to_string)).collect();
            (field, allowed)
        })
        .collect()
}

#[test]
fn a_filter_field_becomes_a_slicer_and_apply_writes_the_page_filter() {
    let mut h = harness();
    with_pivot(&mut h, "Sales[Category]");
    add_slicer(&mut h, "Region");
    assert!(shown(&h, "Slicers"), "the pane lists the slicer");
    for tile in ["East", "West", "North"] {
        assert!(shown(&h, tile), "the slicer shows the member `{tile}`");
    }

    // A plain click on a tile selects only that member, as Excel's slicer does.
    click(&mut h, "East");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(filters_of(&params), vec![("Sales[Region]".to_string(), vec!["East".to_string()])]);
    assert_eq!(params["row_fields"], json!(["Sales[Category]"]), "the report keeps grouping by Category");

    // The grid shrank to the selected member: the page field above the body, then one category.
    h.run_steps(2);
    assert_eq!(cell(&h, 2, 0), "Region", "the page field's caption sits at the pivot's top-left");
    assert_eq!(cell(&h, 3, 0), "East", "the page field names the selected member");
    assert_eq!(cell(&h, 4, 0), "Row Labels", "the body starts below the page field");
    assert_eq!(cell(&h, 5, 0), "A");
    assert_eq!(cell(&h, 6, 0), "", "no other category survived the filter");
    assert!(shown(&h, "Slicers"), "the slicer stays in the pane after Apply");
}

#[test]
fn clearing_the_slicer_restores_every_row() {
    let mut h = harness();
    with_pivot(&mut h, "Sales[Category]");
    add_slicer(&mut h, "Region");
    click(&mut h, "East");
    click(&mut h, "Apply");
    assert_eq!(filters_of(&last_command(&h, "dataModel.createPivot")), vec![("Sales[Region]".to_string(), vec!["East".to_string()])]);

    // Clicking the only selected tile clears the filter, as the engine's slicer does, and the
    // Clear button does the same: every member allowed again.
    click(&mut h, "East");
    click(&mut h, "Clear Slicer Filter");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    let filter = params["filters"][0].clone();
    assert_eq!(filter["field"], json!("Sales[Region]"));
    assert!(filter.get("allowed").is_none(), "clearing the filter restores the page field's (All), not a member list");

    h.run_steps(2);
    assert_eq!(cell(&h, 2, 0), "Region");
    assert_eq!(cell(&h, 3, 0), "(All)", "the page field is back to (All)");
    assert_eq!(cell(&h, 5, 0), "A", "the body lists every category again");
    assert_eq!(cell(&h, 6, 0), "B");
    assert_eq!(cell(&h, 7, 0), "C");
}

#[test]
fn a_text_field_can_carry_a_slicer() {
    let mut h = harness();
    with_pivot(&mut h, "Sales[Region]");
    // Category is text: its members are the tiles, and the report keeps grouping by Region.
    add_slicer(&mut h, "Category");
    assert!(shown(&h, "Slicers"));

    click(&mut h, "A");
    click(&mut h, "Apply");
    let params = last_command(&h, "dataModel.createPivot");
    assert_eq!(filters_of(&params), vec![("Sales[Category]".to_string(), vec!["A".to_string()])]);
    h.run_steps(2);
    assert_eq!(cell(&h, 2, 0), "Category");
    assert_eq!(cell(&h, 3, 0), "A");
    assert_eq!(cell(&h, 5, 0), "East", "only the region with a category-A row survives");
    assert_eq!(cell(&h, 6, 0), "");
}

#[test]
fn a_date_field_reports_the_timeline_limitation_instead_of_a_slicer() {
    let mut h = harness();
    with_pivot(&mut h, "Sales[Region]");
    // The date column goes to Filters like any other field, then asks for a slicer.
    add_slicer(&mut h, "Date");
    let limitation = "`Calendar[Date]` is a date field: a timeline needs the field's date grouping, which a model PivotTable does not carry, so it has no slicer here.";
    assert!(h.query_by_label(limitation).is_some(), "the pane says why there is no slicer");
    assert!(!shown(&h, "Slicers"), "no slicer card is faked for the date field");
    assert!(shown(&h, "Date ▾"), "the filter itself stays queued");
}

/// The slicer card as it paints — the tiles above Apply — for a visual check. Runs only when
/// `GRIDCRAFT_PANE_SNAPSHOT` names a directory, like the pane's search/drag snapshot.
#[test]
fn the_slicer_card_renders() {
    let Ok(dir) = std::env::var("GRIDCRAFT_PANE_SNAPSHOT") else { return };
    let mut h = harness();
    with_pivot(&mut h, "Sales[Category]");
    add_slicer(&mut h, "Region");
    click(&mut h, "East");
    h.run_steps(2);
    std::fs::create_dir_all(&dir).expect("the snapshot directory");
    h.render().unwrap().save(std::path::Path::new(&dir).join("model_pivot_slicer.png")).unwrap();
    assert!(shown(&h, "Slicers"));
    assert!(shown(&h, "East"), "the selected tile is still listed");
}
