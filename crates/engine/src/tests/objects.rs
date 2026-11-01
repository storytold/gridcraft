//! Floating objects of every kind: found, moved, listed, stacked and deleted by `{kind, id}`.

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, AnchorMode, ObjKind, ObjRef, Sheet};
use serde_json::json;

use super::s;
use crate::Session;

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

fn sheet(s: &Session, i: usize) -> &Sheet {
    s.doc().unwrap().wb.sheet(i).unwrap()
}

fn id_of(v: &serde_json::Value, key: &str) -> u32 {
    v[key].as_u64().unwrap() as u32
}

/// A table with one object of each kind on the first sheet: a chart, a picture, a shape, a
/// slicer and a form control, in `ObjKind::ALL` order.
fn every_kind(s: &mut Session) -> [ObjRef; 5] {
    s.execute("range.setValues", json!({"range": "A1", "values": [["Region", "Qty"], ["East", 3], ["West", 5], ["East", 2], ["North", 8]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B5", "name": "Sales"})).unwrap();
    let chart = id_of(&s.execute("insert.chart", json!({"range": "B1:B5", "type": "column", "at": "D2"})).unwrap(), "chart");
    let image = super::add_image(s, "D18", AnchorMode::MoveAndSize);
    let shape = id_of(&s.execute("insert.shape", json!({"kind": "ellipse", "at": "H18"})).unwrap(), "shape");
    s.execute("selection.set", json!({"cell": "A2"})).unwrap();
    let slicer = id_of(&s.execute("insert.slicer", json!({"column": "Region", "at": "L2"})).unwrap(), "slicer");
    let control = id_of(&s.execute("insert.slider", json!({"at": "P18", "link": "$B$7"})).unwrap(), "control");
    [
        ObjRef::new(ObjKind::Chart, chart),
        ObjRef::new(ObjKind::Image, image),
        ObjRef::new(ObjKind::Shape, shape),
        ObjRef::new(ObjKind::Slicer, slicer),
        ObjRef::new(ObjKind::Control, control),
    ]
}

#[test]
fn every_kind_is_found_moved_and_deleted_by_kind_and_id() {
    let mut s = s();
    let objs = every_kind(&mut s);
    for o in objs {
        assert_eq!(sheet(&s, 0).object_ref(o.id), Some(o), "the id finds its kind");
        assert_eq!(ObjKind::parse(o.kind.name()), Some(o.kind));
        s.execute("object.move", json!({"kind": o.kind.name(), "id": o.id, "at": "M20", "width": 50, "height": 30})).unwrap();
        let a = sheet(&s, 0).object_anchor(o).unwrap();
        assert_eq!((a.cell, a.width, a.height), (at("M20"), 50.0, 30.0));
        s.execute("object.setAnchorMode", json!({"kind": o.kind.name(), "id": o.id, "mode": "absolute"})).unwrap();
        assert_eq!(sheet(&s, 0).object_anchor(o).unwrap().mode, AnchorMode::Absolute);
    }
    // A kind that does not match the id, an unknown id or kind, and a missing kind are refused.
    assert!(s.execute("object.delete", json!({"kind": "slicer", "id": objs[0].id})).is_err());
    assert!(s.execute("object.delete", json!({"kind": "chart", "id": 9999})).is_err());
    assert!(s.execute("object.delete", json!({"kind": "widget", "id": objs[0].id})).is_err());
    assert!(s.execute("object.delete", json!({"id": objs[0].id})).is_err(), "kind is required");
    for (n, o) in objs.into_iter().enumerate() {
        let out = s.execute("object.delete", json!({"kind": o.kind.name(), "id": o.id})).unwrap();
        assert_eq!(out["deleted"], json!({"kind": o.kind.name(), "id": o.id}));
        assert!(!sheet(&s, 0).has_object(o));
        for later in &objs[n + 1..] {
            assert!(sheet(&s, 0).has_object(*later), "deleting {o:?} leaves {later:?}");
        }
    }
    // One undo step per delete brings the control back.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(sheet(&s, 0).has_object(objs[4]));
}

#[test]
fn selection_pane_lists_and_stacks_slicers() {
    let mut s = s();
    let objs = every_kind(&mut s);
    let second = id_of(&s.execute("insert.slicer", json!({"column": "Qty", "at": "P2"})).unwrap(), "slicer");
    let list = s.execute("arrange.selectionPane", json!({})).unwrap();
    let items = list.as_array().unwrap();
    let kinds: Vec<&str> = items.iter().map(|o| o["kind"].as_str().unwrap()).collect();
    for k in ObjKind::ALL {
        assert!(kinds.contains(&k.name()), "{k:?} listed in {kinds:?}");
    }
    assert_eq!(kinds.last(), Some(&"control"), "form controls stack on top");
    assert_eq!(kinds.get(kinds.len() - 2), Some(&"slicer"), "slicers stack above charts, pictures and shapes");
    let names: Vec<&str> = items.iter().map(|o| o["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Region") && names.contains(&"Qty"), "{names:?}");
    assert!(names.contains(&format!("Slider {}", objs[4].id).as_str()), "{names:?}");
    assert_eq!(sheet(&s, 0).slicers.first().map(|c| c.id), Some(objs[3].id));
    s.execute("arrange.sendBackward", json!({"kind": "slicer", "id": second, "toBack": true})).unwrap();
    assert_eq!(sheet(&s, 0).slicers.first().map(|c| c.id), Some(second));
}

#[test]
fn hostile_object_params_are_refused() {
    let mut s = s();
    let objs = every_kind(&mut s);
    for p in [
        json!({}),
        json!({"kind": "chart"}),
        json!({"kind": "chart", "id": -1}),
        json!({"kind": "chart", "id": u64::MAX}),
        json!({"kind": 7, "id": objs[0].id}),
        json!({"kind": null, "id": objs[0].id}),
    ] {
        for cmd in ["object.delete", "object.move", "object.setAnchorMode"] {
            assert!(s.execute(cmd, p.clone()).is_err(), "{cmd} {p}");
        }
    }
    let o = objs[3];
    s.execute("object.move", json!({"kind": o.kind.name(), "id": o.id, "dx": 1e300, "dy": -1e300, "width": 1e300, "height": -5})).unwrap();
    let a = sheet(&s, 0).object_anchor(o).unwrap();
    assert!(a.dx.is_finite() && a.dy.is_finite() && a.width <= 10000.0 && a.height >= 0.0, "{a:?}");
}

fn cell_of(s: &Session, o: ObjRef) -> String {
    sheet(s, 0).object_anchor(o).unwrap().cell.a1()
}

#[test]
fn every_kind_follows_row_and_column_edits_and_sorts() {
    let mut s = s();
    let objs = every_kind(&mut s);
    // Anchors: chart D2, picture D18, shape H18, slicer L2, slider P18.
    s.execute("object.setAnchorMode", json!({"kind": "shape", "id": objs[2].id, "mode": "absolute"})).unwrap();
    let cells = |s: &Session| objs.map(|o| cell_of(s, o));
    assert_eq!(cells(&s), ["D2", "D18", "H18", "L2", "P18"]);
    s.execute("home.insertRows", json!({"rows": "10:11"})).unwrap();
    assert_eq!(cells(&s), ["D2", "D20", "H18", "L2", "P20"], "rows inserted above move the objects below them; an absolute shape stays");
    s.execute("home.insertColumns", json!({"cols": "C:C"})).unwrap();
    assert_eq!(cells(&s), ["E2", "E20", "H18", "M2", "Q20"]);
    s.execute("home.deleteColumns", json!({"cols": "C:C"})).unwrap();
    s.execute("home.deleteRows", json!({"rows": "10:11"})).unwrap();
    assert_eq!(cells(&s), ["D2", "D18", "H18", "L2", "P18"]);
    // Sorting moves a picture anchored inside the sorted block with its row.
    s.execute("range.setValues", json!({"range": "T1", "values": [["k"], [3], [1], [2]]})).unwrap();
    let pic = ObjRef::new(ObjKind::Image, super::add_image(&mut s, "T2", AnchorMode::MoveAndSize));
    s.execute("selection.set", json!({"range": "T1:T4"})).unwrap();
    s.execute("data.sortAscending", json!({"header": true, "column": "T"})).unwrap();
    assert_eq!(cell_of(&s, pic), "T4", "the row holding 3 sorts last");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(cell_of(&s, pic), "T2");
}

/// A picture at `cell` with anchor `mode`, offset `(dx, dy)` and size `(width, height)`, added
/// straight to the model like `add_image`, so a sort that follows is the only step to undo.
fn picture(s: &mut Session, cell: &str, mode: AnchorMode, dx: f32, dy: f32, width: f32, height: f32) -> ObjRef {
    let id = super::add_image(s, cell, mode);
    let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
    if let Some(img) = wb.sheet_mut(0).unwrap().images.iter_mut().find(|i| i.id == id) {
        img.anchor = Anchor { dx, dy, width, height, ..img.anchor };
    }
    ObjRef::new(ObjKind::Image, id)
}

/// The anchor of `o` as (cell, dx, dy, width, height).
fn anchor_of(s: &Session, o: ObjRef) -> (String, f32, f32, f32, f32) {
    let a = sheet(s, 0).object_anchor(o).unwrap();
    (a.cell.a1(), a.dx, a.dy, a.width, a.height)
}

/// Sorting rows under a picture follows its anchor mode, as Excel's "Move and size with cells",
/// "Move but don't size with cells" and "Don't move or size with cells" do.
#[test]
fn sorting_rows_under_a_picture_follows_its_anchor_mode() {
    let mut s = s();
    // Rows 2..5 hold 3, 1, 4, 2; sorted ascending they read 1, 2, 3, 4, so row 2 moves to row 4,
    // row 3 to row 2, row 4 to row 5 and row 5 to row 3. Default rows are 20 pt tall.
    s.execute("range.setValues", json!({"range": "A1", "values": [["k"], [3], [1], [4], [2]]})).unwrap();
    // Three pictures inside one row each (3 pt down, 12 pt tall), one per anchor mode.
    let sized = picture(&mut s, "A2", AnchorMode::MoveAndSize, 5.0, 3.0, 30.0, 12.0);
    let moved = picture(&mut s, "A3", AnchorMode::MoveOnly, 5.0, 3.0, 30.0, 12.0);
    let pinned = picture(&mut s, "A4", AnchorMode::Absolute, 5.0, 3.0, 30.0, 12.0);
    // A "move and size" picture spanning rows 2 and 3: its top is 10 pt into row 2 (y 30) and
    // its bottom 15 pt into row 3 (y 55). The sort puts row 3 above row 2.
    let tall = picture(&mut s, "A2", AnchorMode::MoveAndSize, 5.0, 10.0, 30.0, 25.0);
    let before: Vec<_> = [sized, moved, pinned, tall].iter().map(|&o| anchor_of(&s, o)).collect();
    s.execute("selection.set", json!({"range": "A1:A5"})).unwrap();
    s.execute("data.sortAscending", json!({"header": true, "column": "A"})).unwrap();
    assert_eq!(
        anchor_of(&s, sized),
        ("A4".into(), 5.0, 3.0, 30.0, 12.0),
        "move and size: travels with row 2 to row 4, re-fitted to the same 20 pt row"
    );
    assert_eq!(anchor_of(&s, moved), ("A2".into(), 5.0, 3.0, 30.0, 12.0), "move only: travels with row 3 to row 2 at its own size");
    assert_eq!(anchor_of(&s, pinned), ("A4".into(), 5.0, 3.0, 30.0, 12.0), "absolute: stays while the cells move under it");
    // The tall picture's bottom row now sorts above its top row, so there is nothing to stretch
    // it between: it travels with its top-left row and keeps its size instead of collapsing.
    assert_eq!(anchor_of(&s, tall), ("A4".into(), 5.0, 10.0, 30.0, 25.0), "two rows swapped: moves with its top row at its old size");
    s.execute("edit.undo", json!({})).unwrap();
    let after: Vec<_> = [sized, moved, pinned, tall].iter().map(|&o| anchor_of(&s, o)).collect();
    assert_eq!(after, before, "undo restores every anchor");
}

/// A sort by columns follows the anchor mode the same way.
#[test]
fn sorting_columns_under_a_picture_follows_its_anchor_mode() {
    let mut s = s();
    // Row 10 holds a header in A and 3, 1, 2 in B..D; sorted ascending, column B moves to D,
    // C to B and D to C. Default columns are 64 pt wide.
    s.execute("range.setValues", json!({"range": "A10", "values": [["k", 3, 1, 2]]})).unwrap();
    let sized = picture(&mut s, "B10", AnchorMode::MoveAndSize, 5.0, 3.0, 30.0, 12.0);
    let moved = picture(&mut s, "C10", AnchorMode::MoveOnly, 5.0, 3.0, 30.0, 12.0);
    let pinned = picture(&mut s, "D10", AnchorMode::Absolute, 5.0, 3.0, 30.0, 12.0);
    // A "move and size" picture spanning columns B and C: from x 104 (40 pt into B) to x 154
    // (26 pt into C). The sort puts column C before column B.
    let wide = picture(&mut s, "B10", AnchorMode::MoveAndSize, 40.0, 3.0, 50.0, 12.0);
    let before: Vec<_> = [sized, moved, pinned, wide].iter().map(|&o| anchor_of(&s, o)).collect();
    s.execute("data.sort", json!({"range": "A10:D10", "header": true, "orientation": "columns", "keys": [{"row": 10}]})).unwrap();
    assert_eq!(anchor_of(&s, sized), ("D10".into(), 5.0, 3.0, 30.0, 12.0), "move and size: travels with column B to D at the same width");
    assert_eq!(anchor_of(&s, moved), ("B10".into(), 5.0, 3.0, 30.0, 12.0), "move only: travels with column C to B at its own size");
    assert_eq!(anchor_of(&s, pinned), ("D10".into(), 5.0, 3.0, 30.0, 12.0), "absolute: stays");
    assert_eq!(anchor_of(&s, wide), ("D10".into(), 40.0, 3.0, 50.0, 12.0), "two columns swapped: moves with its left column at its old size");
    s.execute("edit.undo", json!({})).unwrap();
    let after: Vec<_> = [sized, moved, pinned, wide].iter().map(|&o| anchor_of(&s, o)).collect();
    assert_eq!(after, before, "undo restores every anchor");
}

#[test]
fn slicers_follow_inserted_rows_like_other_objects() {
    let mut s = s();
    let objs = every_kind(&mut s);
    s.execute("home.insertRows", json!({"rows": "1:2"})).unwrap();
    assert_eq!(cell_of(&s, objs[3]), "L4");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(cell_of(&s, objs[3]), "L2");
}

#[test]
fn chart_series_follow_cut_moves_renames_and_sheet_deletes() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[3], [5], [2], [8]]})).unwrap();
    let chart = id_of(&s.execute("insert.chart", json!({"range": "A1:A4", "type": "column"})).unwrap(), "chart");
    let values = |s: &Session| sheet(s, 0).charts.iter().find(|c| c.id == chart).map(|c| c.series[0].values.clone()).unwrap_or_default();
    s.execute("edit.cut", json!({"range": "A1:A4"})).unwrap();
    s.execute("edit.paste", json!({"at": "F2"})).unwrap();
    assert!(values(&s).contains("$F$2:$F$5"), "a cut-move takes the series with the cells: {}", values(&s));
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap().charts[0].series[0].values = "Other!$A$1:$A$3".into();
    s.execute("sheet.rename", json!({"sheet": 1, "name": "Inputs"})).unwrap();
    assert_eq!(values(&s), "Inputs!$A$1:$A$3");
    s.execute("home.deleteSheet", json!({"sheet": 1})).unwrap();
    assert!(values(&s).contains("#REF!"), "a series into a deleted sheet becomes #REF!: {}", values(&s));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(values(&s), "Inputs!$A$1:$A$3");
}

#[test]
fn a_copied_sheet_gives_its_objects_fresh_ids() {
    let mut s = s();
    let objs = every_kind(&mut s);
    s.execute("sheet.move", json!({"sheet": 0, "to": 1, "copy": true})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let ids_of = |sh: &Sheet| -> Vec<u32> {
        sh.charts
            .iter()
            .map(|c| c.id)
            .chain(sh.images.iter().map(|c| c.id))
            .chain(sh.shapes.iter().map(|c| c.id))
            .chain(sh.slicers.iter().map(|c| c.id))
            .chain(sh.controls.iter().map(|c| c.id))
            .collect()
    };
    let mut ids: Vec<u32> = wb.sheets.iter().flat_map(|sh| ids_of(sh)).collect();
    assert_eq!(ids.len(), 2 * objs.len());
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 2 * objs.len(), "every object id stays unique across the workbook");
    let copy = sheet(&s, 1);
    for (o, id) in objs.iter().zip(ids_of(copy)) {
        assert_eq!(copy.object_ref(id).map(|c| c.kind), Some(o.kind));
    }
    assert!(s.doc().unwrap().wb.next_object_id() > *ids.last().unwrap_or(&0));
}

#[test]
fn bubble_sizes_follow_edits_like_the_other_series_references() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1, 3, 10], [2, 5, 20], [3, 2, 30]]})).unwrap();
    let chart = id_of(&s.execute("insert.chart", json!({"range": "A1:B3", "type": "bubble"})).unwrap(), "chart");
    std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap().charts.iter_mut().find(|c| c.id == chart).unwrap().series[0]
        .bubble_sizes = Some("Sheet1!$C$1:$C$3".into());
    let sizes = |s: &Session| sheet(s, 0).charts.iter().find(|c| c.id == chart).and_then(|c| c.series[0].bubble_sizes.clone()).unwrap_or_default();
    s.execute("home.insertRows", json!({"rows": "1:1"})).unwrap();
    assert_eq!(sizes(&s), "Sheet1!$C$2:$C$4", "an inserted row moves the bubble sizes with their cells");
    s.execute("edit.cut", json!({"range": "C2:C4"})).unwrap();
    s.execute("edit.paste", json!({"at": "F2"})).unwrap();
    assert_eq!(sizes(&s), "Sheet1!$F$2:$F$4", "a cut-move takes the bubble sizes with the cells");
    s.execute("sheet.rename", json!({"sheet": 0, "name": "Data"})).unwrap();
    assert_eq!(sizes(&s), "Data!$F$2:$F$4", "a renamed sheet is renamed in the bubble sizes");
}

/// A file can carry an object with the largest id. New objects and a copied sheet's objects then
/// take free ids instead of the largest one again, so `{kind, id}` still names one object.
#[test]
fn object_ids_stay_unique_when_a_file_used_the_largest_id() {
    let mut s = s();
    let first = id_of(&s.execute("insert.shape", json!({"kind": "rectangle", "at": "B2"})).unwrap(), "shape");
    // The file's shape carries the largest id (set on the model, as a loaded file would).
    std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap().shapes.iter_mut().find(|o| o.id == first).unwrap().id = u32::MAX;
    let second = id_of(&s.execute("insert.shape", json!({"kind": "ellipse", "at": "D2"})).unwrap(), "shape");
    let third = id_of(&s.execute("insert.shape", json!({"kind": "triangle", "at": "F2"})).unwrap(), "shape");
    assert!(second != u32::MAX && third != u32::MAX && second != third, "new shapes take free ids, not the largest again: {second}, {third}");
    s.execute("sheet.move", json!({"sheet": 0, "to": 1, "copy": true})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let mut ids: Vec<u32> = wb.sheets.iter().flat_map(|sh| sh.shapes.iter().map(|o| o.id)).collect();
    assert_eq!(ids.len(), 6);
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 6, "every shape id is distinct across both sheets");
}
