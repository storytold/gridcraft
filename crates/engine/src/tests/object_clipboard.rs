//! Copy, cut and paste of floating pictures, on the fake worker: a pasted copy stays linked to the
//! Python cell, and nothing crosses workbooks.

use std::sync::Arc;

use serde_json::json;

use super::python_run::{c, picture_bytes, py, session, settle};
use crate::{OTHER_WORKBOOK, Session};

fn images(s: &Session, sheet: usize) -> Vec<gridcraft_model::Image> {
    s.doc().unwrap().wb.sheets[sheet].images.clone()
}

fn bytes(s: &Session, sheet: usize) -> Vec<Vec<u8>> {
    images(s, sheet).iter().map(|o| (*o.data).clone()).collect()
}

/// A workbook whose A1 gave a picture, shown over the cells at D1, and the picture's id.
fn linked() -> (Session, gridcraft_python::fake::FakeHandle, u32) {
    let (mut s, h) = session();
    py(&mut s, "A1", "picture 40 30");
    settle(&mut s);
    s.execute("python.displayOverCells", json!({"cell": "A1", "at": "D1"})).unwrap();
    let id = images(&s, 0)[0].id;
    (s, h, id)
}

fn error(s: &mut Session, command: &str, params: serde_json::Value) -> String {
    let e = s.execute(command, params).unwrap_err().to_string();
    assert!(s.ui_requests.is_empty(), "{command} opens no dialog");
    e
}

#[test]
fn a_copy_changes_nothing_in_the_workbook() {
    let (mut s, _h, id) = linked();
    let (wb, steps) = (s.doc().unwrap().wb.clone(), s.doc().unwrap().undo.len());
    assert_eq!(s.execute("object.copy", json!({"kind": "image", "id": id})).unwrap()["image"], id);
    assert!(Arc::ptr_eq(&wb, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), steps);
}

#[test]
fn a_copied_and_a_cut_picture_change_with_their_source() {
    for command in ["object.copy", "object.cut"] {
        let (mut s, _h, id) = linked();
        s.execute(command, json!({"id": id})).unwrap();
        let pasted = s.execute("object.paste", json!({"at": "F1"})).unwrap();
        assert_eq!(pasted["linked"], true, "{command}");
        if command == "object.copy" {
            assert_ne!(pasted["image"], id, "a new id");
        }
        py(&mut s, "A1", "picture 20 30");
        settle(&mut s);
        for picture in bytes(&s, 0) {
            assert_eq!(picture, picture_bytes(20, 30), "{command}");
        }
        assert_eq!(images(&s, 0).len(), if command == "object.copy" { 2 } else { 1 });
    }
}

#[test]
fn a_copy_on_another_sheet_changes_too() {
    let (mut s, _h, id) = linked();
    s.execute("object.copy", json!({"id": id})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Plots"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active_sheet(), 1);
    assert_eq!(s.execute("object.paste", json!({"at": "B2"})).unwrap()["linked"], true);
    let copy = images(&s, 1)[0].clone();
    assert_eq!(copy.python_source.as_deref(), Some("Sheet1!$A$1"), "the reference names its sheet, so it needs no rewriting");
    assert_eq!(copy.anchor.cell, c("B2"));
    s.execute("python.set", json!({"sheet": "Sheet1", "cell": "A1", "code": "picture 20 30"})).unwrap();
    settle(&mut s);
    assert_eq!(bytes(&s, 1), [picture_bytes(20, 30)]);
}

#[test]
fn a_cut_can_be_undone_and_redone_and_its_paste_repeated() {
    let (mut s, _h, id) = linked();
    let steps = s.doc().unwrap().undo.len();
    s.execute("object.cut", json!({"id": id})).unwrap();
    assert!(images(&s, 0).is_empty());
    assert_eq!(s.doc().unwrap().undo.len(), steps + 1, "one step");
    s.execute("edit.undo", json!({})).unwrap();
    let back = images(&s, 0);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].python_source.as_deref(), Some("Sheet1!$A$1"), "with its link");
    s.execute("edit.redo", json!({})).unwrap();
    assert!(images(&s, 0).is_empty());

    let steps = s.doc().unwrap().undo.len();
    let first = s.execute("object.paste", json!({"at": "C3"})).unwrap();
    let second = s.execute("object.paste", json!({"at": "E5"})).unwrap();
    assert_ne!(first["image"], second["image"]);
    assert_eq!(s.doc().unwrap().undo.len(), steps + 2, "a step each");
    let cells: Vec<_> = images(&s, 0).iter().map(|o| o.anchor.cell).collect();
    assert_eq!(cells, [c("C3"), c("E5")]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(images(&s, 0).len(), 1);
}

#[test]
fn a_paste_goes_to_the_active_cell_and_keeps_the_size() {
    let (mut s, _h, id) = linked();
    s.execute("object.move", json!({"kind": "image", "id": id, "dx": 7.0, "width": 100.0, "height": 75.0})).unwrap();
    s.execute("object.copy", json!({"id": id})).unwrap();
    s.execute("selection.set", json!({"cell": "G9"})).unwrap();
    s.execute("object.paste", json!({})).unwrap();
    let copy = images(&s, 0)[1].clone();
    assert_eq!(copy.anchor.cell, c("G9"));
    assert_eq!((copy.anchor.dx, copy.anchor.width, copy.anchor.height), (0.0, 100.0, 75.0));
    assert!(s.execute("object.paste", json!({"at": "not a cell"})).is_err());
    assert_eq!(images(&s, 0).len(), 2);
}

#[test]
fn a_picture_with_no_link_copies_as_it_is() {
    let (mut s, _h) = session();
    s.execute("insert.picture", json!({"base64": crate::io::base64_encode(&picture_bytes(4, 4)), "at": "A1"})).unwrap();
    let id = images(&s, 0)[0].id;
    s.execute("object.copy", json!({"id": id})).unwrap();
    assert_eq!(s.execute("object.paste", json!({"at": "B2"})).unwrap()["linked"], false);
    assert_eq!(bytes(&s, 0)[1], picture_bytes(4, 4));
}

#[test]
fn a_paste_into_another_workbook_is_refused_and_changes_nothing() {
    for command in ["object.copy", "object.cut"] {
        let (mut s, _h, id) = linked();
        s.execute(command, json!({"id": id})).unwrap();
        s.new_workbook();
        let (wb, undo, redo) = {
            let d = s.doc().unwrap();
            (d.wb.clone(), d.undo.len(), d.redo.len())
        };
        assert_eq!(error(&mut s, "object.paste", json!({})), OTHER_WORKBOOK, "{command}");
        assert_eq!(error(&mut s, "object.paste", json!({"at": "C3"})), OTHER_WORKBOOK, "{command}");
        let d = s.doc().unwrap();
        assert!(Arc::ptr_eq(&wb, &d.wb), "{command}");
        assert_eq!((d.undo.len(), d.redo.len()), (undo, redo), "{command}");
        // The workbook it came from can still paste it.
        s.set_active(0);
        assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], true, "{command}");
    }
}

#[test]
fn a_paste_after_its_workbook_closed_is_refused() {
    let (mut s, _h, id) = linked();
    s.execute("object.copy", json!({"id": id})).unwrap();
    s.new_workbook();
    s.set_active(0);
    s.execute("file.close", json!({"force": true})).unwrap();
    assert_eq!(s.documents().len(), 1);
    assert_eq!(error(&mut s, "object.paste", json!({})), OTHER_WORKBOOK);
}

#[test]
fn deleting_the_source_deletes_the_copies_and_undo_brings_them_all_back() {
    let (mut s, _h, id) = linked();
    s.execute("object.copy", json!({"id": id})).unwrap();
    s.execute("object.paste", json!({"at": "F1"})).unwrap();
    s.execute("object.paste", json!({"at": "F9"})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Plots"})).unwrap();
    s.execute("object.paste", json!({"at": "B2"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    assert_eq!((images(&s, 0).len(), images(&s, 1).len()), (3, 1));
    let steps = s.doc().unwrap().undo.len();
    s.execute("edit.clearContents", json!({"range": "A1"})).unwrap();
    settle(&mut s);
    assert_eq!((images(&s, 0).len(), images(&s, 1).len()), (0, 0));
    assert_eq!(s.doc().unwrap().undo.len(), steps + 1, "one step");
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((images(&s, 0).len(), images(&s, 1).len()), (3, 1), "the source and every copy");
    assert_eq!(s.doc().unwrap().wb.sheets[0].cell(c("A1")).unwrap().py.as_deref().unwrap().code, "picture 40 30");
    s.execute("edit.redo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!((images(&s, 0).len(), images(&s, 1).len()), (0, 0));
}

#[test]
fn other_kinds_and_bad_calls_are_errors_and_open_nothing() {
    let (mut s, _h) = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["x"], [1], [2]]})).unwrap();
    let chart = s.execute("insert.chart", json!({"range": "A1:A3"})).unwrap()["chart"].as_u64().unwrap();
    let shape = s.execute("insert.shape", json!({"kind": "ellipse", "at": "F1"})).unwrap()["shape"].as_u64().unwrap();
    for (kind, id) in [("chart", chart), ("shape", shape), ("slicer", 1), ("equation", 1), ("control", 1)] {
        for command in ["object.copy", "object.cut"] {
            let e = error(&mut s, command, json!({"kind": kind, "id": id}));
            assert_eq!(e, "Only pictures can be copied for now.", "{command} {kind}");
        }
    }
    assert_eq!(s.doc().unwrap().wb.sheets[0].charts.len(), 1, "the chart stays");
    assert_eq!(s.doc().unwrap().wb.sheets[0].shapes.len(), 1, "the shape stays");
    for command in ["object.copy", "object.cut"] {
        assert!(error(&mut s, command, json!({})).contains("missing `id`"));
        assert!(error(&mut s, command, json!({"id": 9999})).contains("no such picture"));
        assert!(error(&mut s, command, json!({"kind": "widget", "id": 1})).contains("kind must be"));
        assert!(error(&mut s, command, json!({"id": 1, "sheet": 99})).contains("no such sheet"));
    }
    assert_eq!(error(&mut s, "object.paste", json!({})), "Nothing to paste.", "the object clipboard is empty");
    assert!(s.object_clipboard.is_none());
}

#[test]
fn the_sheet_parameter_names_where_the_picture_is() {
    let (mut s, _h, id) = linked();
    s.execute("home.insertSheet", json!({"name": "Plots"})).unwrap();
    assert!(s.execute("object.cut", json!({"id": id})).is_err(), "it isn't on the active sheet");
    s.execute("object.cut", json!({"id": id, "sheet": "Sheet1"})).unwrap();
    assert!(images(&s, 0).is_empty());
    s.execute("object.paste", json!({"at": "B2"})).unwrap();
    assert_eq!(images(&s, 1).len(), 1);
}

#[test]
fn a_protected_sheet_keeps_its_pictures_and_takes_none() {
    let (mut s, _h, id) = linked();
    s.execute("object.copy", json!({"id": id})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    s.execute("object.copy", json!({"id": id})).unwrap();
    assert!(error(&mut s, "object.cut", json!({"id": id})).contains("protected"));
    assert!(error(&mut s, "object.paste", json!({})).contains("protected"));
    assert_eq!(images(&s, 0).len(), 1);
}

// ---- The link while the picture waits on the clipboard

#[test]
fn a_cut_picture_shows_what_its_source_gives_now() {
    let (mut s, _h, id) = linked();
    s.execute("object.cut", json!({"id": id})).unwrap();
    py(&mut s, "A1", "picture 20 30");
    settle(&mut s);
    assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], true);
    assert_eq!(bytes(&s, 0), [picture_bytes(20, 30)]);
    let o = images(&s, 0)[0].clone();
    assert_eq!((o.anchor.width, o.anchor.height), (40.0, 60.0), "the width stays, and the height follows the proportions");
}

#[test]
fn a_picture_on_the_clipboard_follows_its_source() {
    let (mut s, _h, id) = linked();
    s.execute("object.cut", json!({"id": id})).unwrap();
    s.execute("home.insertRows", json!({"rows": "1:2"})).unwrap();
    s.execute("home.insertColumns", json!({"cols": "A:A"})).unwrap();
    s.execute("sheet.rename", json!({"name": "My Data"})).unwrap();
    settle(&mut s);
    assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], true);
    assert_eq!(images(&s, 0)[0].python_source.as_deref(), Some("'My Data'!$B$3"));
}

#[test]
fn undo_takes_the_link_back_with_the_edit() {
    let (mut s, _h, id) = linked();
    s.execute("object.cut", json!({"id": id})).unwrap();
    s.execute("home.insertRows", json!({"rows": "1:2"})).unwrap();
    settle(&mut s);
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    s.execute("object.paste", json!({"at": "F1"})).unwrap();
    assert_eq!(images(&s, 0)[0].python_source.as_deref(), Some("Sheet1!$A$1"));
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(images(&s, 0)[0].python_source.as_deref(), Some("Sheet1!$A$1"));
    // The other way: Redo brings the moved reference back.
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("home.insertRows", json!({"rows": "1:2"})).unwrap();
    settle(&mut s);
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.redo", json!({})).unwrap();
    settle(&mut s);
    s.execute("object.paste", json!({"at": "F1"})).unwrap();
    assert_eq!(images(&s, 0)[0].python_source.as_deref(), Some("Sheet1!$A$3"));
}

#[test]
fn undo_to_before_the_copy_never_links_to_another_cell() {
    let (mut s, _h, id) = linked();
    py(&mut s, "A2", "picture 8 8");
    settle(&mut s);
    s.execute("home.insertRows", json!({"rows": "1:1"})).unwrap();
    settle(&mut s);
    s.execute("object.copy", json!({"id": id})).unwrap();
    assert_eq!(images(&s, 0)[0].python_source.as_deref(), Some("Sheet1!$A$2"));
    // Back before the insert, the copy's text names the cell below the source, which has other code.
    s.execute("edit.undo", json!({})).unwrap();
    settle(&mut s);
    assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], false);
    assert_eq!(bytes(&s, 0).last().unwrap(), &picture_bytes(40, 30));
    // The same goes for an edit from there.
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "H9", "input": "1"})).unwrap();
    assert_eq!(s.execute("object.paste", json!({"at": "F2"})).unwrap()["linked"], false);
}

#[test]
fn with_no_source_left_a_paste_arrives_without_a_link() {
    for gone in
        [json!({"command": "edit.clearContents", "params": {"range": "A1"}}), json!({"command": "home.deleteRows", "params": {"rows": "1:1"}})]
    {
        let (mut s, _h, id) = linked();
        s.execute("object.copy", json!({"id": id})).unwrap();
        s.execute(gone["command"].as_str().unwrap(), gone["params"].clone()).unwrap();
        settle(&mut s);
        assert!(images(&s, 0).is_empty(), "{gone}: the one on the sheet went with its source");
        assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], false, "{gone}");
        settle(&mut s);
        assert_eq!(bytes(&s, 0), [picture_bytes(40, 30)], "{gone}: the picture that was asked for stays");
        assert_eq!(images(&s, 0)[0].python_source, None, "{gone}");
    }
}

#[test]
fn a_source_that_stopped_giving_a_picture_leaves_no_link_and_one_that_failed_keeps_it() {
    let (mut s, _h, id) = linked();
    s.execute("object.copy", json!({"id": id})).unwrap();
    py(&mut s, "A1", "ret 5");
    settle(&mut s);
    assert!(images(&s, 0).is_empty());
    assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], false);

    let (mut s, _h, id) = linked();
    s.execute("object.copy", json!({"id": id})).unwrap();
    py(&mut s, "A1", "fail");
    settle(&mut s);
    assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], true);
    assert_eq!(bytes(&s, 0)[1], picture_bytes(40, 30), "its last image");
    py(&mut s, "A1", "picture 20 30");
    settle(&mut s);
    assert_eq!(bytes(&s, 0)[1], picture_bytes(20, 30), "the next picture reaches it");
}

#[test]
fn a_paste_while_the_source_runs_gets_the_result() {
    let (mut s, _h, id) = linked();
    s.execute("object.copy", json!({"id": id})).unwrap();
    py(&mut s, "A1", "picture 20 30");
    assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], true, "it can still give a picture");
    settle(&mut s);
    assert_eq!(bytes(&s, 0)[1], picture_bytes(20, 30));
}

#[test]
fn one_clipboard_at_a_time() {
    let (mut s, _h, id) = linked();
    s.execute("edit.copy", json!({"range": "A1"})).unwrap();
    s.execute("object.copy", json!({"id": id})).unwrap();
    assert!(s.clipboard.is_none(), "the picture replaces the cells");
    s.execute("edit.copy", json!({"range": "A1"})).unwrap();
    assert!(s.object_clipboard.is_none(), "the cells replace the picture");
    assert_eq!(error(&mut s, "object.paste", json!({})), "Nothing to paste.");
    s.execute("selection.set", json!({"cell": "C9"})).unwrap();
    s.execute("edit.paste", json!({})).unwrap();
}

#[test]
fn a_new_copy_doesnt_take_the_link_of_the_last_one() {
    let (mut s, _h, id) = linked();
    py(&mut s, "B1", "picture 8 8");
    settle(&mut s);
    s.execute("python.displayOverCells", json!({"cell": "B1", "at": "D9"})).unwrap();
    let other = images(&s, 0)[1].id;
    s.execute("object.copy", json!({"id": id})).unwrap();
    s.execute("cell.set", json!({"cell": "H9", "input": "1"})).unwrap();
    s.execute("object.copy", json!({"id": other})).unwrap();
    s.execute("object.paste", json!({"at": "F1"})).unwrap();
    assert_eq!(images(&s, 0)[2].python_source.as_deref(), Some("Sheet1!$B$1"));
    s.execute("cell.set", json!({"cell": "H9", "input": "2"})).unwrap();
    s.execute("object.paste", json!({"at": "F2"})).unwrap();
    assert_eq!(images(&s, 0)[3].python_source.as_deref(), Some("Sheet1!$B$1"));
}

#[test]
fn a_macro_check_leaves_the_clipboard_alone() {
    let (mut s, _h, id) = linked();
    s.execute("object.copy", json!({"id": id})).unwrap();
    super::macros::put(
        &mut s,
        json!({"name": "Clip", "steps": [{"command": "object.cut", "params": {"id": id}}, {"command": "edit.copy", "params": {"range": "A1"}}]}),
    );
    let check = s.execute("macro.check", json!({"name": "Clip", "inputs": {}})).unwrap();
    assert_eq!(check["footprint"]["objectsRemoved"], 1, "its steps ran");
    assert!(s.clipboard.is_none());
    assert!(s.object_clipboard.is_some());
    s.execute("cell.set", json!({"cell": "H9", "input": "1"})).unwrap();
    assert_eq!(s.execute("object.paste", json!({"at": "F1"})).unwrap()["linked"], true);
}
