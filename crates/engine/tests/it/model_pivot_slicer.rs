//! A data-model PivotTable's page filter — the state the field-list pane's slicer drives.
//!
//! The pane's slicer tiles are not engine state: they edit the pending `filters` of
//! `dataModel.createPivot`, whose `allowed` list is the PivotTable's page field. These tests pin
//! the semantics that makes real: one selected member narrows the report, every member selected
//! restores it, a text column carries the selection, and `dataModel.pivotFields` hands the
//! selection back so the pane's tiles can show it again.
//!
//! A page field may not also sit on an axis (`pivot_model::check_axes`), which is Excel's rule too
//! — one field, one area — so the report groups by one column while the slicer filters another.

use gridcraft_core::CellRef;
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

const MODEL: &str = r##"{
    "tables": [{"name":"Sales",
                "columns":[{"name":"Region","data_type":"String"},{"name":"Category","data_type":"String"},
                           {"name":"Amount","data_type":"Double"}],
                "rows":[["East","A",10],["West","B",30],["East","A",20],["North","C",40]]}],
    "relationships": [],
    "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales","format":"#,##0"}]
}"##;

/// A session holding [`MODEL`].
fn load() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("dataModel.load", json!({"json": MODEL})).expect("load model");
    s
}

fn create(s: &mut Session, params: Json) -> Json {
    s.execute("dataModel.createPivot", params).expect("create pivot")
}

fn cell(s: &Session, row: u32, col: u32) -> String {
    s.doc().expect("doc").wb.sheet(0).expect("sheet").value(CellRef::new(row, col)).display()
}

/// The sheet as a grid of display strings, `rows` by `cols`, from `(row, col)`.
fn grid(s: &Session, row: u32, col: u32, rows: u32, cols: u32) -> Vec<Vec<String>> {
    (0..rows).map(|r| (0..cols).map(|c| cell(s, row + r, col + c)).collect()).collect()
}

/// The report's body starts below its page fields: two rows per filter, as the writer lays them out.
fn body_row(filters: usize) -> u32 {
    2 + (filters * 2) as u32
}

#[test]
fn one_selected_member_is_a_page_filter_and_shrinks_the_report() {
    let mut s = load();
    // No filter: every category, so a header row and three report rows.
    let reply = create(&mut s, json!({"measure": "Total Sales", "row_fields": ["Sales[Category]"]}));
    assert_eq!(reply["rows"], json!(4));
    assert_eq!(grid(&s, 2, 0, 4, 2), vec![vec!["Row Labels", "Total Sales"], vec!["A", "30"], vec!["B", "30"], vec!["C", "40"],]);

    // One tile on — the slicer's selection — travels as the filter field's `allowed` list. The
    // filtered column is not on an axis, as the writer requires.
    let reply = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Category]"],
            "filters": [{"field": "Sales[Region]", "allowed": ["East"]}],
            "name": reply["name"],
            "replace": true,
        }),
    );
    assert_eq!(reply["rows"], json!(2), "only the selected member's rows survive");
    // The page field takes two rows above the body: its caption, then the selected item.
    assert_eq!(grid(&s, 2, 0, 2, 1), vec![vec!["Region"], vec!["East"]]);
    let body = body_row(1);
    assert_eq!(grid(&s, body, 0, 3, 2), vec![vec!["Row Labels", "Total Sales"], vec!["A", "30"], vec!["", ""],]);
}

#[test]
fn every_member_selected_restores_every_row() {
    let mut s = load();
    let reply = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Category]"],
            "filters": [{"field": "Sales[Region]", "allowed": ["East"]}],
        }),
    );
    let name = reply["name"].clone();

    // "Clear Slicer Filter": every member of the field is allowed again, so the report is whole.
    let reply = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Category]"],
            "filters": [{"field": "Sales[Region]", "allowed": ["East", "West", "North"]}],
            "name": name,
            "replace": true,
        }),
    );
    assert_eq!(reply["rows"], json!(4), "the header and every category are back");
    let body = body_row(1);
    assert_eq!(grid(&s, body, 0, 4, 2), vec![vec!["Row Labels", "Total Sales"], vec!["A", "30"], vec!["B", "30"], vec!["C", "40"],]);
}

#[test]
fn a_text_column_can_carry_the_selection() {
    let mut s = load();
    // A slicer is not a numeric widget: a text column's members are the tiles.
    let reply = create(
        &mut s,
        json!({"measure": "Total Sales", "row_fields": ["Sales[Region]"], "filters": [{"field": "Sales[Category]", "allowed": ["A"]}]}),
    );
    assert_eq!(reply["rows"], json!(2), "only the region with a category-A row survives");
    assert_eq!(grid(&s, 2, 0, 2, 1), vec![vec!["Category"], vec!["A"]]);
    let body = body_row(1);
    assert_eq!(grid(&s, body, 0, 3, 2), vec![vec!["Row Labels", "Total Sales"], vec!["East", "30"], vec!["", ""],]);
}

#[test]
fn the_field_list_hands_the_selection_back_for_the_slicer_tiles() {
    let mut s = load();
    let reply = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Category]"],
            "filters": [{"field": "Sales[Region]", "allowed": ["East"]}],
        }),
    );
    let name = reply["name"].as_str().expect("the command names the pivot").to_string();
    let answer = s.execute("dataModel.pivotFields", json!({"name": name})).expect("read the field list");
    let selected = &answer["selected"];
    assert_eq!(selected["filters"][0]["field"], json!("Sales[Region]"));
    assert_eq!(selected["filters"][0]["allowed"], json!(["East"]), "the pane's tiles read the selection here");
    // The read-back `at` is the report body, below the page field; the pane subtracts the page rows
    // (`filters * 2`) to apply to the PivotTable's top-left again.
    assert_eq!(selected["at"], json!("A5"));
    assert_eq!(body_row(1), 4, "one filter puts the body four rows below the pivot's top-left");
}
