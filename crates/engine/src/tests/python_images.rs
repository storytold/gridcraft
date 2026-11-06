//! Python cells that give pictures, on the fake worker: what a cell keeps, its return type, and what
//! commands give agents. `apps/gridcraft-cli/tests/it/python_cells.rs` runs real matplotlib.

use serde_json::json;

use gridcraft_core::{CellError, Value};

use super::python_run::{c, picture_bytes, py, runs, session, settle};
use crate::Session;

fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(c(cell))
}

fn python(s: &Session, cell: &str) -> gridcraft_model::PyCode {
    s.doc().unwrap().wb.active().unwrap().cell(c(cell)).unwrap().py.as_deref().unwrap().clone()
}

fn in_cell(s: &Session, cell: &str) -> bool {
    s.doc().unwrap().wb.active().unwrap().picture_at(c(cell)).is_some()
}

#[test]
fn a_picture_with_no_choice_is_a_python_object_named_image() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "picture 40 30");
    settle(&mut s);
    assert_eq!(value(&s, "A1"), Value::text("Image"));
    let py1 = python(&s, "A1");
    assert!(py1.shows_object() && !py1.object, "in effect, not chosen");
    assert!(!in_cell(&s, "A1"), "a plot isn't drawn inside a small cell");
    let image = py1.image().unwrap();
    assert_eq!((image.width, image.height, image.density), (40, 30, 1));
    assert_eq!(image.picture.mime, "image/png");
    assert!(py1.to_formula().ends_with(",1)"), "the file says Python Object: {}", py1.to_formula());
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["python"]["output"], "object");
}

#[test]
fn a_cell_with_no_choice_goes_back_to_spilling_when_it_stops_giving_a_picture() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "picture 40 30");
    settle(&mut s);
    py(&mut s, "A1", "spill 2 1");
    settle(&mut s);
    assert!(!python(&s, "A1").shows_object());
    assert_eq!(value(&s, "A2"), Value::Number(20.0));
}

#[test]
fn choosing_excel_value_draws_it_in_the_cell_without_running_and_undo_takes_it_back() {
    let (mut s, h) = session();
    py(&mut s, "A1", "picture 40 30");
    settle(&mut s);
    let ran = runs(&h).len();
    let r = s.execute("python.output", json!({"cell": "A1", "mode": "value"})).unwrap();
    assert_eq!(r["output"], "value");
    settle(&mut s);
    assert_eq!(runs(&h).len(), ran, "the picture is already there");
    assert!(in_cell(&s, "A1"));
    assert!(python(&s, "A1").to_formula().ends_with(",0)"));
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert!(!in_cell(&s, "A1"));
    assert!(python(&s, "A1").shows_object());
    s.execute("edit.redo", json!({})).unwrap();
    settle(&mut s);
    assert!(in_cell(&s, "A1"));
}

#[test]
fn the_shortcut_with_no_mode_turns_a_picture_into_an_excel_value_first() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "picture 40 30");
    settle(&mut s);
    assert_eq!(s.execute("python.output", json!({"cell": "A1"})).unwrap()["output"], "value");
    settle(&mut s);
    assert!(in_cell(&s, "A1"));
    assert_eq!(s.execute("python.output", json!({"cell": "A1"})).unwrap()["output"], "object");
    settle(&mut s);
    assert!(!in_cell(&s, "A1"));
    assert_eq!(value(&s, "A1"), Value::text("Image"));
    assert!(python(&s, "A1").object);
}

#[test]
fn excel_value_chosen_first_draws_in_the_cell_when_it_arrives() {
    let (mut s, _h) = session();
    s.execute("python.set", json!({"cell": "A1", "code": "picture 40 30", "output": "value"})).unwrap();
    settle(&mut s);
    assert!(in_cell(&s, "A1"));
    assert_eq!(value(&s, "A1"), Value::text("Image"), "what a formula that reads it sees");
}

#[test]
fn a_new_result_replaces_the_picture_and_a_number_takes_it_away() {
    let (mut s, _h) = session();
    s.execute("python.set", json!({"cell": "A1", "code": "picture 40 30", "output": "value"})).unwrap();
    settle(&mut s);
    py(&mut s, "A1", "picture 8 6");
    settle(&mut s);
    assert_eq!(python(&s, "A1").image().unwrap().width, 8);
    assert!(in_cell(&s, "A1"));
    py(&mut s, "A1", "ret 5");
    settle(&mut s);
    assert!(!in_cell(&s, "A1"));
    assert_eq!(value(&s, "A1"), Value::Number(5.0));
}

#[test]
fn a_failure_leaves_no_picture_in_the_cell() {
    let (mut s, _h) = session();
    s.execute("python.set", json!({"cell": "A1", "code": "picture 40 30", "output": "value"})).unwrap();
    settle(&mut s);
    py(&mut s, "A1", "fail");
    settle(&mut s);
    assert!(!in_cell(&s, "A1"));
    assert_eq!(value(&s, "A1"), Value::Error(CellError::Python));
}

#[test]
fn convert_to_normal_cell_keeps_the_picture_it_shows() {
    let (mut s, _h) = session();
    s.execute("python.set", json!({"cell": "A1", "code": "picture 40 30", "output": "value"})).unwrap();
    settle(&mut s);
    s.execute("python.clear", json!({"cell": "A1"})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let sh = wb.active().unwrap();
    assert!(sh.cell(c("A1")).unwrap().py.is_none());
    assert!(sh.cell_pictures.contains_key(&c("A1")));
    s.execute("edit.undo", json!({})).unwrap();
    assert!(python(&s, "A1").image().is_some());
    assert!(!s.doc().unwrap().wb.active().unwrap().cell_pictures.contains_key(&c("A1")));
}

#[test]
fn python_image_and_save_image_give_the_bytes() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "picture 40 30 2");
    py(&mut s, "A2", "ret 1");
    settle(&mut s);
    let r = s.execute("python.image", json!({"cell": "A1"})).unwrap();
    assert_eq!(
        (&r["cell"], &r["mime"], &r["width"], &r["height"], &r["density"]),
        (&json!("A1"), &json!("image/png"), &json!(20), &json!(15), &json!(2))
    );
    let bytes = crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
    assert_eq!(bytes, picture_bytes(40, 30));
    assert!(s.execute("python.image", json!({"cell": "A2"})).unwrap_err().to_string().contains("no picture"));
    assert!(s.execute("python.image", json!({"cell": "C9"})).unwrap_err().to_string().contains("isn't a Python cell"));
    assert!(s.execute("python.saveImage", json!({"cell": "A1"})).is_err(), "a path is needed, and no dialog opens");
    let dir = std::env::temp_dir().join(format!("gridcraft-py-image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plot");
    let saved = s.execute("python.saveImage", json!({"cell": "A1", "path": path.to_str().unwrap()})).unwrap();
    let target = saved["path"].as_str().unwrap().to_string();
    assert!(target.ends_with("plot.png"));
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
    let named = dir.join("named.PNG");
    let saved = s.execute("python.saveImage", json!({"cell": "A1", "path": named.to_str().unwrap()})).unwrap();
    assert!(saved["path"].as_str().unwrap().ends_with("named.PNG"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_workbook_as_json_keeps_the_picture() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "picture 40 30 2");
    settle(&mut s);
    let saved = s.execute("file.saveBytes", json!({"format": "json"})).unwrap();
    let bytes = crate::io::base64_decode(saved["base64"].as_str().unwrap()).unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let text = doc.to_string();
    assert!(text.contains("\"density\":2"), "{}", &text[..text.len().min(400)]);
    let back: gridcraft_model::Workbook = serde_json::from_slice(&bytes).unwrap();
    let py1 = back.sheets[0].cell(c("A1")).unwrap().py.as_deref().unwrap().clone();
    assert_eq!(py1.image().unwrap().picture.data, picture_bytes(40, 30));
}

#[test]
fn a_cell_that_changes_an_earlier_figure_runs_again_from_that_cell() {
    let (mut s, h) = session();
    set(&mut s, "C1", "1");
    py(&mut s, "A1", "picture 40 30");
    py(&mut s, "A2", "ret 2");
    // A3 reads C1, and says it changed the figure of A1 (sheet 0, row 0, column 0).
    py(&mut s, "A3", "touch 0 0 0 xl(\"C1\")");
    settle(&mut s);
    let before = runs(&h).len();
    set(&mut s, "C1", "2");
    settle(&mut s);
    assert_eq!(
        runs(&h).split_off(before),
        ["picture 40 30", "ret 2", "touch 0 0 0 xl(\"C1\")"],
        "from the figure's cell, not from the one whose input changed"
    );
    // A cell after them all is no reason to go back.
    py(&mut s, "A4", "ret 4");
    settle(&mut s);
    let before = runs(&h).len();
    py(&mut s, "A4", "ret 5");
    settle(&mut s);
    assert_eq!(runs(&h).split_off(before), ["ret 5"]);
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

// ---- Linked floating pictures

fn images(s: &Session, sheet: usize) -> usize {
    s.doc().unwrap().wb.sheets[sheet].images.len()
}

fn image_bytes(s: &Session, sheet: usize) -> Vec<Vec<u8>> {
    s.doc().unwrap().wb.sheets[sheet].images.iter().map(|o| (*o.data).clone()).collect()
}

/// A workbook whose A1 gave a picture, shown over the cells at D1.
fn with_linked_picture() -> (Session, gridcraft_python::fake::FakeHandle) {
    let (mut s, h) = session();
    py(&mut s, "A1", "picture 40 30");
    settle(&mut s);
    let r = s.execute("python.displayOverCells", json!({"cell": "A1", "at": "D1"})).unwrap();
    assert_eq!(r["source"], "Sheet1!$A$1");
    assert_eq!(images(&s, 0), 1);
    (s, h)
}

/// `edit`, then Undo and Redo: the picture goes, comes back with the code, and goes again.
fn gone_and_back(s: &mut Session, what: &str) {
    assert_eq!(images(s, 0), 0, "{what}");
    s.execute("edit.undo", json!({})).unwrap();
    settle(s);
    assert_eq!(images(s, 0), 1, "{what}: undo");
    assert_eq!(python(s, "A1").code, "picture 40 30", "{what}: undo");
    assert!(python(s, "A1").image().is_some(), "{what}: undo");
    s.execute("edit.redo", json!({})).unwrap();
    settle(s);
    assert_eq!(images(s, 0), 0, "{what}: redo");
}

#[test]
fn a_linked_picture_shows_the_cells_picture_and_changes_with_it() {
    let (mut s, _h) = with_linked_picture();
    assert_eq!(image_bytes(&s, 0), [picture_bytes(40, 30)]);
    py(&mut s, "A1", "picture 20 30");
    settle(&mut s);
    assert_eq!(image_bytes(&s, 0), [picture_bytes(20, 30)], "the code changed, and so did the picture");
    let o = s.doc().unwrap().wb.sheets[0].images[0].clone();
    assert_eq!(o.anchor.width, 40.0, "the width stays");
    assert_eq!(o.anchor.height, 60.0, "the height follows the proportions");
    assert_eq!(o.anchor.cell, c("D1"));
}

#[test]
fn a_picture_needs_a_picture_to_show() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "ret 1");
    settle(&mut s);
    assert!(s.execute("python.displayOverCells", json!({"cell": "A1"})).is_err());
    assert!(s.execute("python.displayOverCells", json!({"cell": "C3"})).is_err());
    assert!(s.execute("python.createReference", json!({})).is_err());
    assert!(s.execute("python.createReference", json!({"id": 99})).is_err());
    assert_eq!(images(&s, 0), 0);
}

#[test]
fn create_reference_makes_another_linked_picture() {
    let (mut s, _h) = with_linked_picture();
    let id = s.doc().unwrap().wb.sheets[0].images[0].id;
    s.execute("python.createReference", json!({"id": id})).unwrap();
    assert_eq!(images(&s, 0), 2);
    py(&mut s, "A1", "picture 20 30");
    settle(&mut s);
    assert_eq!(image_bytes(&s, 0), [picture_bytes(20, 30), picture_bytes(20, 30)]);
    s.execute("insert.picture", json!({"base64": crate::io::base64_encode(&picture_bytes(2, 2)), "at": "H1"})).unwrap();
    let plain = s.doc().unwrap().wb.sheets[0].images[2].id;
    assert!(s.execute("python.createReference", json!({"id": plain})).is_err(), "not linked");
}

#[test]
fn clearing_the_source_deletes_the_picture_and_undo_brings_both_back() {
    let (mut s, _h) = with_linked_picture();
    s.execute("edit.clearContents", json!({"range": "A1"})).unwrap();
    settle(&mut s);
    gone_and_back(&mut s, "cleared");
}

#[test]
fn a_value_over_the_source_deletes_the_picture() {
    let (mut s, _h) = with_linked_picture();
    s.execute("cell.set", json!({"cell": "A1", "input": "5"})).unwrap();
    settle(&mut s);
    gone_and_back(&mut s, "a value");
}

#[test]
fn convert_to_normal_cell_deletes_the_picture() {
    let (mut s, _h) = with_linked_picture();
    s.execute("python.clear", json!({"cell": "A1"})).unwrap();
    settle(&mut s);
    gone_and_back(&mut s, "converted");
}

#[test]
fn deleting_the_row_or_the_column_deletes_the_picture() {
    let (mut s, _h) = with_linked_picture();
    s.execute("home.deleteRows", json!({"rows": "1:1"})).unwrap();
    settle(&mut s);
    gone_and_back(&mut s, "row");
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    s.execute("home.deleteColumns", json!({"cols": "A:A"})).unwrap();
    settle(&mut s);
    gone_and_back(&mut s, "column");
}

#[test]
fn deleting_the_sources_sheet_deletes_pictures_on_other_sheets() {
    let (mut s, _h) = session();
    s.execute("home.insertSheet", json!({"name": "Plots"})).unwrap();
    let plots = s.doc().unwrap().wb.active_sheet();
    assert_eq!(plots, 1);
    s.execute("python.set", json!({"sheet": "Sheet1", "cell": "A1", "code": "picture 40 30"})).unwrap();
    settle(&mut s);
    let r = s.execute("python.displayOverCells", json!({"sheet": "Sheet1", "cell": "A1", "at": "B2"})).unwrap();
    assert_eq!(r["source"], "Sheet1!$A$1");
    assert_eq!(images(&s, 1), 1, "it is on the active sheet");
    s.execute("home.deleteSheet", json!({"sheet": "Sheet1"})).unwrap();
    settle(&mut s);
    assert_eq!(images(&s, 0), 0, "the sheet that is left is Plots");
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(images(&s, 1), 1);
    assert_eq!(s.doc().unwrap().wb.sheets[0].cell(c("A1")).unwrap().py.as_deref().unwrap().code, "picture 40 30");
}

#[test]
fn code_that_stops_giving_a_picture_deletes_it_and_undo_brings_both_back() {
    let (mut s, _h) = with_linked_picture();
    let steps = s.doc().unwrap().undo.len();
    py(&mut s, "A1", "ret 5");
    assert_eq!(images(&s, 0), 1, "the result isn't here yet");
    settle(&mut s);
    assert_eq!(value(&s, "A1"), Value::Number(5.0));
    assert_eq!(s.doc().unwrap().undo.len(), steps + 1, "the deletion belongs to the edit");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(images(&s, 0), 1, "at once, with the old code and result");
    assert_eq!(python(&s, "A1").code, "picture 40 30");
    settle(&mut s);
    assert_eq!(value(&s, "A1"), Value::text("Image"));
    assert_eq!(images(&s, 0), 1, "and running that code again keeps it");
    assert_eq!(image_bytes(&s, 0), [picture_bytes(40, 30)]);
    s.execute("edit.redo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(images(&s, 0), 0);
    assert_eq!(value(&s, "A1"), Value::Number(5.0));
}

#[test]
fn a_change_to_what_the_source_reads_counts_too() {
    let (mut s, _h) = session();
    s.execute("cell.set", json!({"cell": "C1", "input": "1"})).unwrap();
    // The fake worker's `picturewhen`: a picture while the input is 1, else a number.
    py(&mut s, "A1", "picturewhen xl(\"C1\")");
    settle(&mut s);
    s.execute("python.displayOverCells", json!({"cell": "A1", "at": "D1"})).unwrap();
    assert_eq!(images(&s, 0), 1);
    let steps = s.doc().unwrap().undo.len();
    s.execute("cell.set", json!({"cell": "C1", "input": "2"})).unwrap();
    settle(&mut s);
    assert_eq!(images(&s, 0), 0);
    assert_eq!(s.doc().unwrap().undo.len(), steps + 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(images(&s, 0), 1);
    assert_eq!(value(&s, "C1"), Value::Number(1.0));
}

#[test]
fn a_run_with_no_edit_behind_it_makes_its_own_step_when_it_deletes() {
    let (mut s, _h) = session();
    // The fake worker's `pictureonce`: a picture on its first run in a worker, then a number.
    py(&mut s, "A1", "pictureonce");
    settle(&mut s);
    s.execute("python.displayOverCells", json!({"cell": "A1", "at": "D1"})).unwrap();
    s.execute("cell.set", json!({"cell": "F9", "input": "1"})).unwrap();
    let steps = s.doc().unwrap().undo.len();
    s.execute("python.run", json!({"cell": "A1"})).unwrap();
    settle(&mut s);
    assert_eq!(images(&s, 0), 0);
    assert_eq!(value(&s, "A1"), Value::Number(1.0));
    assert_eq!(s.doc().unwrap().undo.len(), steps + 1);
    assert_eq!(s.doc().unwrap().undo.last().unwrap().label, "Run Python Cell");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(images(&s, 0), 1, "the picture and the earlier result, and nothing else");
    assert_eq!(value(&s, "A1"), Value::text("Image"));
    assert_eq!(value(&s, "F9"), Value::Number(1.0));
}

#[test]
fn a_run_that_doesnt_finish_deletes_nothing_and_says_so() {
    let (mut s, _h) = with_linked_picture();
    for code in ["fail", "die", "garbage"] {
        py(&mut s, "A1", code);
        settle(&mut s);
        assert_eq!(images(&s, 0), 1, "{code}");
        assert_eq!(image_bytes(&s, 0), [picture_bytes(40, 30)], "{code}: its last image");
        let wb = &s.doc().unwrap().wb;
        assert!(crate::python::links::problem(wb, &wb.sheets[0].images[0]).is_some(), "{code}: marked");
    }
    py(&mut s, "A1", "picture 20 30");
    settle(&mut s);
    let wb = &s.doc().unwrap().wb;
    assert_eq!(crate::python::links::problem(wb, &wb.sheets[0].images[0]), None, "the next picture clears it");
    assert_eq!(image_bytes(&s, 0), [picture_bytes(20, 30)]);
}

#[test]
fn the_link_follows_the_source() {
    let (mut s, _h) = with_linked_picture();
    let source = |s: &Session| s.doc().unwrap().wb.sheets[0].images[0].python_source.clone().unwrap();
    s.execute("home.insertRows", json!({"rows": "1:2"})).unwrap();
    settle(&mut s);
    assert_eq!(source(&s), "Sheet1!$A$3");
    s.execute("home.insertColumns", json!({"cols": "A:A"})).unwrap();
    settle(&mut s);
    assert_eq!(source(&s), "Sheet1!$B$3");
    s.execute("sheet.rename", json!({"name": "My Data"})).unwrap();
    assert_eq!(source(&s), "'My Data'!$B$3");
    settle(&mut s);
    assert_eq!(images(&s, 0), 1);
    py(&mut s, "B3", "picture 20 30");
    settle(&mut s);
    assert_eq!(image_bytes(&s, 0), [picture_bytes(20, 30)], "it still shows that cell");
    // A cut and paste of the cell moves the link.
    s.execute("selection.set", json!({"range": "B3"})).unwrap();
    s.execute("edit.cut", json!({})).unwrap();
    s.execute("edit.paste", json!({"at": "E5"})).unwrap();
    settle(&mut s);
    assert_eq!(source(&s), "'My Data'!$E$5");
    assert_eq!(images(&s, 0), 1);
}

#[test]
fn deleting_the_picture_leaves_the_cell() {
    let (mut s, _h) = with_linked_picture();
    let id = s.doc().unwrap().wb.sheets[0].images[0].id;
    s.execute("object.delete", json!({"kind": "image", "id": id})).unwrap();
    assert_eq!(images(&s, 0), 0);
    assert_eq!(python(&s, "A1").code, "picture 40 30");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(images(&s, 0), 1);
}

// ---- Figures whose size is up to GridCraft

fn resize(s: &mut Session, width: f64, height: f64) {
    let id = s.doc().unwrap().wb.sheets[0].images[0].id;
    s.execute("object.move", json!({"kind": "image", "id": id, "width": width, "height": height})).unwrap();
    settle(s);
}

fn figure_with_picture(code: &str) -> (Session, gridcraft_python::fake::FakeHandle) {
    let (mut s, h) = session();
    py(&mut s, "A1", code);
    settle(&mut s);
    s.execute("python.displayOverCells", json!({"cell": "A1", "at": "D1"})).unwrap();
    settle(&mut s);
    (s, h)
}

fn shown(s: &Session) -> (u32, u32) {
    let o = &s.doc().unwrap().wb.sheets[0].images[0];
    let i = image::load_from_memory(&o.data).unwrap();
    (i.width(), i.height())
}

#[test]
fn resizing_a_free_figure_draws_it_again_without_running_the_cell() {
    let (mut s, h) = figure_with_picture("figure 400 300");
    assert_eq!(shown(&s), (400, 300), "at its natural size there is nothing to do");
    let ran = runs(&h).len();
    let steps = s.doc().unwrap().undo.len();
    resize(&mut s, 800.0, 200.0);
    assert_eq!(shown(&s), (800, 200));
    assert_eq!(runs(&h).len(), ran, "no cell ran");
    assert_eq!(s.doc().unwrap().undo.len(), steps + 1, "the resize is the one step");
    // Undo brings the old size and the old drawing back, with nothing to draw.
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(shown(&s), (400, 300));
    s.execute("edit.redo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(shown(&s), (800, 200));
}

#[test]
fn a_figure_stops_at_its_floor_and_the_picture_is_scaled_below_it() {
    let (mut s, _h) = figure_with_picture("figure 400 300");
    resize(&mut s, 100.0, 100.0);
    assert_eq!(shown(&s), (200, 150), "half its natural size");
    let o = s.doc().unwrap().wb.sheets[0].images[0].clone();
    assert_eq!((o.anchor.width, o.anchor.height), (100.0, 100.0), "it shows at the size it was given");
    resize(&mut s, 100.0, 400.0);
    assert_eq!(shown(&s), (200, 400), "each side on its own");
}

#[test]
fn a_figure_placed_smaller_than_natural_keeps_its_text_the_same_size_when_resized() {
    let (mut s, _h) = figure_with_picture("figure 1280 960");
    let o = s.doc().unwrap().wb.sheets[0].images[0].clone();
    assert_eq!(o.anchor.width, 640.0, "placed at most 640 wide");
    assert_eq!(shown(&s), (1280, 960));
    resize(&mut s, 800.0, 480.0);
    assert_eq!(shown(&s), (1600, 960), "one unit of the picture still stands for two of the figure");
}

#[test]
fn copies_at_different_sizes_each_get_their_own_drawing() {
    let (mut s, _h) = figure_with_picture("figure 400 300");
    let id = s.doc().unwrap().wb.sheets[0].images[0].id;
    s.execute("python.createReference", json!({"id": id})).unwrap();
    let second = s.doc().unwrap().wb.sheets[0].images[1].id;
    s.execute("object.move", json!({"kind": "image", "id": id, "width": 800.0, "height": 300.0})).unwrap();
    s.execute("object.move", json!({"kind": "image", "id": second, "width": 400.0, "height": 600.0})).unwrap();
    settle(&mut s);
    let sizes: Vec<(u32, u32)> = s.doc().unwrap().wb.sheets[0]
        .images
        .iter()
        .map(|o| {
            let i = image::load_from_memory(&o.data).unwrap();
            (i.width(), i.height())
        })
        .collect();
    assert_eq!(sizes, [(800, 300), (400, 600)]);
}

#[test]
fn a_new_result_keeps_the_shape_of_a_free_figures_picture_and_redraws_it() {
    let (mut s, _h) = figure_with_picture("figure 400 300");
    resize(&mut s, 800.0, 200.0);
    py(&mut s, "A1", "figure 402 300");
    settle(&mut s);
    let o = s.doc().unwrap().wb.sheets[0].images[0].clone();
    assert_eq!((o.anchor.width, o.anchor.height), (800.0, 200.0));
    assert_eq!(shown(&s), (800, 200));
}

#[test]
fn a_figure_of_fixed_size_or_another_picture_just_scales() {
    let (mut s, h) = figure_with_picture("picture 400 300");
    resize(&mut s, 800.0, 200.0);
    assert_eq!(shown(&s), (400, 300), "the same bytes, stretched");
    assert!(h.requests().iter().all(|r| !matches!(r, gridcraft_python::protocol::Request::Render { .. })));
}

#[test]
fn a_figure_that_cant_be_drawn_again_stays_as_it_is() {
    let (mut s, _h) = session();
    py(&mut s, "A1", "set no_render 1");
    py(&mut s, "A2", "figure 400 300");
    settle(&mut s);
    s.execute("python.displayOverCells", json!({"cell": "A2", "at": "D1"})).unwrap();
    resize(&mut s, 800.0, 200.0);
    assert_eq!(shown(&s), (400, 300));
    assert_eq!(value(&s, "A2"), Value::text("Image"), "no error on the cell");
    assert_eq!(s.python_diagnostic(0, c("A2")).map(|d| d.kind.id), None);
    // It isn't tried again and again.
    assert!(!s.python_busy());
}

#[test]
fn a_worker_lost_while_it_draws_makes_no_cell_error() {
    let (mut s, h) = session();
    py(&mut s, "A1", "set die_on_render 1");
    py(&mut s, "A2", "figure 400 300");
    settle(&mut s);
    s.execute("python.displayOverCells", json!({"cell": "A2", "at": "D1"})).unwrap();
    settle(&mut s);
    let started = h.starts();
    resize(&mut s, 800.0, 200.0);
    assert_eq!(shown(&s), (400, 300), "it stays as it was");
    assert_eq!(value(&s, "A2"), Value::text("Image"));
    assert_eq!(s.python_diagnostic(0, c("A2")).map(|d| d.kind.id), None);
    assert_eq!(h.starts(), started, "nothing starts a worker for this");
    assert!(!s.python_busy(), "and it isn't tried again and again");
    // The next cell to run finds a fresh worker, and the cells before it run again in it.
    py(&mut s, "A3", "ret 3");
    settle(&mut s);
    assert_eq!(value(&s, "A3"), Value::Number(3.0));
    assert!(h.starts() > started);
}

#[test]
fn with_no_worker_the_picture_stretches_and_nothing_starts_one() {
    let (mut s, h) = figure_with_picture("figure 400 300");
    s.set_python_workbook_enabled(false);
    settle(&mut s);
    let started = h.starts();
    resize(&mut s, 800.0, 200.0);
    assert_eq!(shown(&s), (400, 300));
    assert_eq!(h.starts(), started);
    assert!(!s.python_busy());
}

#[test]
fn excel_value_fills_its_cell_at_or_above_the_floor() {
    let (mut s, _h) = session();
    s.execute("python.set", json!({"cell": "A1", "code": "figure 400 300", "output": "value"})).unwrap();
    settle(&mut s);
    let drawn = |s: &Session| {
        let wb = &s.doc().unwrap().wb;
        let i = image::load_from_memory(&wb.sheets[0].picture_at(c("A1")).unwrap().data).unwrap();
        (i.width(), i.height())
    };
    assert_eq!(drawn(&s), (400, 300), "below the floor it stays the natural picture");
    s.execute("home.columnWidth", json!({"cols": "A:A", "width": 500})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1:1", "height": 200})).unwrap();
    settle(&mut s);
    let wb = &s.doc().unwrap().wb;
    let (w, h) = (wb.sheets[0].col_width(0).round() as u32, wb.sheets[0].row_height(0).round() as u32);
    assert_eq!(drawn(&s), (w, h), "the cell's own size");
    assert_eq!(python(&s, "A1").image().unwrap().width, 400, "the result itself is unchanged");
}
