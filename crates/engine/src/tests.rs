use gridcraft_core::{CellRef, Value};
use serde_json::json;

use crate::Session;

fn s() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn v(s: &Session, a: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a).unwrap())
}

#[test]
fn entering_empty_cells_preserves_whole_sheet_formatting() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "existing"})).unwrap();
    s.execute("selection.set", json!({"range": "A1:XFD1048576"})).unwrap();
    s.execute("home.alignCenter", json!({})).unwrap();
    let border = gridcraft_model::BorderLine { style: gridcraft_model::BorderStyle::Thin, color: gridcraft_model::Color::Auto };
    s.execute("home.formatCells", json!({"style": {"borders": {"top": border, "bottom": border, "left": border, "right": border}}})).unwrap();
    let expected = {
        let d = s.doc().unwrap();
        d.wb.styles.get(d.wb.active().unwrap().style_id(CellRef::parse("B1").unwrap())).clone()
    };
    assert_eq!(expected.align.h, gridcraft_model::HAlign::Center);
    for (cell, input) in [("A1", "edited"), ("B1", "new"), ("C1", "=1+2"), ("XFD1048576", "edge")] {
        s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.wb.styles.get(d.wb.active().unwrap().style_id(CellRef::parse(cell).unwrap())), &expected, "{cell}");
    }
    s.execute("range.setValues", json!({"range": "D1", "values": [["text", 7, true]]})).unwrap();
    for cell in ["D1", "E1", "F1"] {
        let d = s.doc().unwrap();
        assert_eq!(d.wb.styles.get(d.wb.active().unwrap().style_id(CellRef::parse(cell).unwrap())), &expected, "{cell}");
    }
    s.execute("edit.undo", json!({})).unwrap();
    assert!(v(&s, "D1").is_empty());
    s.execute("edit.redo", json!({})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.wb.styles.get(d.wb.active().unwrap().style_id(CellRef::parse("D1").unwrap())), &expected);
    assert_eq!(d.wb.active().unwrap().cells.len(), 7);
    let (reopened, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&d.wb).unwrap()).unwrap();
    for cell in ["A1", "B1", "C1", "D1", "E1", "F1", "G1", "XFD1048576"] {
        assert_eq!(
            reopened.styles.get(reopened.active().unwrap().style_id(CellRef::parse(cell).unwrap())),
            &expected,
            "{cell} after XLSX round trip"
        );
    }
}

#[test]
fn entry_and_text_paste_use_inherited_formats_and_cell_overrides() {
    let mut s = s();
    s.execute("home.bold", json!({"range": "B:B", "on": true})).unwrap();
    s.execute("home.formatCells", json!({"range": "3:3", "style": {"num_fmt": "@", "font": {"italic": true}}})).unwrap();
    s.execute("home.fontColor", json!({"range": "B3", "color": "#FF0000"})).unwrap();
    let expected = {
        let d = s.doc().unwrap();
        d.wb.styles.get(d.wb.active().unwrap().style_id(CellRef::parse("B3").unwrap())).clone()
    };
    s.execute("cell.set", json!({"cell": "B3", "input": "=1+2"})).unwrap();
    s.execute("cell.set", json!({"cell": "C3", "input": "00123"})).unwrap();
    s.execute("edit.paste", json!({"at": "D3", "text": "=2+3\t00456"})).unwrap();
    assert_eq!(v(&s, "B3"), Value::text("=1+2"));
    assert_eq!(v(&s, "C3"), Value::text("00123"));
    assert_eq!(v(&s, "D3"), Value::text("=2+3"));
    assert_eq!(v(&s, "E3"), Value::text("00456"));
    s.execute("cell.set", json!({"cell": "B4", "input": "12%"})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(d.wb.styles.get(sh.style_id(CellRef::parse("B3").unwrap())), &expected);
    for cell in ["C3", "D3", "E3"] {
        let st = d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap()));
        assert!(st.font.italic);
        assert!(!st.font.bold, "row style takes precedence over column style");
        assert_eq!(st.num_fmt.as_str(), "@");
    }
    let percent = d.wb.styles.get(sh.style_id(CellRef::parse("B4").unwrap()));
    assert!(percent.font.bold);
    assert_ne!(percent.num_fmt.as_str(), "General");
}

#[test]
fn enter_values_and_formulas() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "10"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "32"})).unwrap();
    s.execute("cell.set", json!({"cell": "A3", "input": "=sum(a1:a2"})).unwrap();
    assert_eq!(v(&s, "A3"), Value::Number(42.0));
    assert!(s.execute("cell.set", json!({"cell": "A4", "input": "=1+*2"})).is_err());
    s.execute("cell.set", json!({"cell": "B1", "input": "12%"})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(crate::display::cell_text(&d.wb, sh, CellRef::parse("B1").unwrap()), "12%");
}

#[test]
fn undo_redo() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(1.0));
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(2.0));
    assert!(s.doc().unwrap().is_dirty());
}

#[test]
fn no_border_clears_only_shared_edges_and_undo_restores_them() {
    use gridcraft_core::RangeRef;
    use gridcraft_model::{BorderLine, Borders};

    for (range, neighbors) in [
        ("B2", &[("B1", "bottom"), ("B3", "top"), ("A2", "right"), ("C2", "left")][..]),
        (
            "B2:C3",
            &[("B1", "bottom"), ("C1", "bottom"), ("B4", "top"), ("C4", "top"), ("A2", "right"), ("A3", "right"), ("D2", "left"), ("D3", "left")][..],
        ),
    ] {
        let mut s = s();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12], [13, 14, 15, 16]]})).unwrap();
        s.execute("cell.set", json!({"cell": "D4", "input": "=SUM(A1:C1)"})).unwrap();
        s.execute("home.bold", json!({"range": "A1:D4", "on": true})).unwrap();
        for preset in ["all", "diagonalDown"] {
            s.execute("home.borders", json!({"range": "A1:D4", "preset": preset, "style": "double", "color": "#CC3344"})).unwrap();
        }
        let before: Vec<_> = RangeRef::parse("A1:D4").unwrap().iter().map(|c| s.execute("cell.get", json!({"cell": c.a1()})).unwrap()).collect();
        s.execute("home.borders", json!({"range": range, "preset": "none"})).unwrap();
        let selected = RangeRef::parse(range).unwrap();
        for original in &before {
            let address = original["cell"].as_str().unwrap();
            let mut expected = original.clone();
            if selected.contains(CellRef::parse(address).unwrap()) {
                expected["style"]["borders"] = json!(Borders::default());
            } else if let Some((_, edge)) = neighbors.iter().find(|(cell, _)| *cell == address) {
                expected["style"]["borders"][*edge] = json!(BorderLine::default());
            }
            assert_eq!(s.execute("cell.get", json!({"cell": address})).unwrap(), expected, "clearing {range}, cell {address}");
        }
        s.execute("edit.undo", json!({})).unwrap();
        for original in before {
            assert_eq!(s.execute("cell.get", json!({"cell": original["cell"]})).unwrap(), original);
        }
    }
}

#[test]
fn no_border_handles_sheet_corners_without_creating_out_of_bounds_cells() {
    for (range, corner, neighbors) in [
        ("A1:B2", "A1", [("B1", "left"), ("A2", "top")]),
        ("XFC1048575:XFD1048576", "XFD1048576", [("XFC1048576", "right"), ("XFD1048575", "bottom")]),
    ] {
        let mut s = s();
        s.execute("home.borders", json!({"range": range, "preset": "all"})).unwrap();
        s.execute("home.borders", json!({"range": corner, "preset": "none"})).unwrap();
        for (neighbor, edge) in neighbors {
            assert_eq!(s.execute("cell.get", json!({"cell": neighbor})).unwrap()["style"]["borders"][edge]["style"], "None");
        }
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert!(sh.cells.iter().all(|(c, _)| c.is_valid()));
        assert!(sh.cells.iter().count() <= 4);
    }
}

#[test]
fn borders_respect_sheet_formatting_protection() {
    let mut s = s();
    s.execute("home.borders", json!({"range": "A1:B2", "preset": "all"})).unwrap();
    s.execute("review.protectSheet", json!({})).unwrap();
    let before = s.doc().unwrap().wb.clone();
    let undo_count = s.doc().unwrap().undo.len();
    for preset in ["none", "left"] {
        assert!(s.execute("home.borders", json!({"range": "A1", "preset": preset})).is_err());
        assert_eq!(s.doc().unwrap().wb.styles, before.styles);
        assert_eq!(s.doc().unwrap().wb.active().unwrap().cells, before.active().unwrap().cells);
        assert_eq!(s.doc().unwrap().undo.len(), undo_count);
    }
    s.execute("review.protectSheet", json!({"formatCells": true})).unwrap();
    s.execute("home.borders", json!({"range": "A1", "preset": "none"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "B1"})).unwrap()["style"]["borders"]["left"]["style"], "None");
}

#[test]
fn copy_paste_shifts_formulas() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=A1+B1"})).unwrap();
    s.execute("edit.copy", json!({"range": "C1"})).unwrap();
    s.execute("selection.set", json!({"range": "C2"})).unwrap();
    s.execute("edit.paste", json!({})).unwrap();
    assert_eq!(v(&s, "C2"), Value::Number(7.0));
    s.execute("edit.cut", json!({"range": "A1"})).unwrap();
    s.execute("selection.set", json!({"range": "E5"})).unwrap();
    s.execute("edit.paste", json!({})).unwrap();
    assert_eq!(v(&s, "E5"), Value::Number(1.0));
    let f = s.execute("cell.get", json!({"cell": "C1"})).unwrap();
    assert_eq!(f["formula"], "=E5+B1");
}

#[test]
fn insert_delete_rows_adjust() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=SUM(A1:A3)"})).unwrap();
    s.execute("home.insertRows", json!({"rows": "2:2"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "B1"})).unwrap()["formula"], "=SUM(A1:A4)");
    s.execute("cell.set", json!({"cell": "A2", "input": "10"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Number(16.0));
    s.execute("home.deleteRows", json!({"rows": "1:1"})).unwrap();
    assert_eq!(v(&s, "B1"), Value::Empty);
    assert_eq!(v(&s, "A1"), Value::Number(10.0));
}

#[test]
fn inserting_or_deleting_cells_cancels_copy_mode() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
    s.execute("edit.cut", json!({"range": "3:3"})).unwrap();
    s.execute("home.insertRows", json!({"rows": "3:3"})).unwrap();
    assert!(s.clipboard.is_none());
    // The cut data is now in row 4; a paste must not move the new blank row 3 instead.
    assert!(s.execute("edit.paste", json!({"at": "A10"})).is_err());
    assert_eq!(v(&s, "A4"), Value::Number(3.0));
    s.execute("edit.copy", json!({"range": "A:A"})).unwrap();
    s.execute("home.deleteColumns", json!({"cols": "B:B"})).unwrap();
    assert!(s.clipboard.is_none());
    s.execute("edit.copy", json!({"range": "A1"})).unwrap();
    s.execute("home.insertCells", json!({"range": "A1", "shift": "down"})).unwrap();
    assert!(s.clipboard.is_none());
    s.execute("edit.copy", json!({"range": "A2"})).unwrap();
    s.execute("home.deleteCells", json!({"range": "A1", "shift": "up"})).unwrap();
    assert!(s.clipboard.is_none());
}

#[test]
fn fill_series() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [3]]})).unwrap();
    s.execute("edit.autoFill", json!({"source": "A1:A2", "target": "A1:A6"})).unwrap();
    assert_eq!(v(&s, "A6"), Value::Number(11.0));
    s.execute("cell.set", json!({"cell": "B1", "input": "Mon"})).unwrap();
    s.execute("edit.autoFill", json!({"source": "B1", "target": "B1:B3"})).unwrap();
    assert_eq!(v(&s, "B3"), Value::from("Wed"));
    s.execute("cell.set", json!({"cell": "C1", "input": "Item 9"})).unwrap();
    s.execute("edit.autoFill", json!({"source": "C1", "target": "C1:C2"})).unwrap();
    assert_eq!(v(&s, "C2"), Value::from("Item 10"));
}

#[test]
fn insert_delete_cells_adjust() {
    let formula = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["formula"].clone();
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3], [4], [5]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=SUM(A1:A5)"})).unwrap();
    s.execute("cell.set", json!({"cell": "C2", "input": "=A4*10"})).unwrap();
    s.execute("cell.set", json!({"cell": "C3", "input": "=SUM(A1:B5)"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Fourth", "refersTo": "=Sheet1!$A$4"})).unwrap();
    s.execute(
        "home.conditionalFormat",
        json!({"range": "A4:A5", "rule": {"type": "cellIs", "operator": "greater", "value": "3", "preset": "redText"}}),
    )
    .unwrap();
    s.execute("data.validation", json!({"range": "A5", "type": "whole", "operator": "greater", "formula1": "0"})).unwrap();
    s.execute("insert.chart", json!({"range": "A1:A5", "type": "line"})).unwrap();
    // Insert A3 shifting down: ranges across the insertion grow, references below move.
    s.execute("home.insertCells", json!({"range": "A3", "shift": "down"})).unwrap();
    assert_eq!(formula(&mut s, "C1"), "=SUM(A1:A6)");
    assert_eq!(formula(&mut s, "C2"), "=A5*10");
    assert_eq!(formula(&mut s, "C3"), "=SUM(A1:B5)");
    assert_eq!(v(&s, "C2"), Value::Number(40.0));
    let sh = s.doc().unwrap().wb.active().unwrap().clone();
    assert_eq!(s.doc().unwrap().wb.names[0].formula, "Sheet1!$A$5");
    assert_eq!(sh.cond_formats[0].ranges[0].a1(), "A5:A6");
    assert_eq!(sh.validations[0].ranges[0].a1(), "A6");
    assert_eq!(sh.charts[0].series[0].values, "Sheet1!$A$1:$A$6");
    // Delete A5 (the 4) shifting up: references to it become #REF!, those below move up.
    s.execute("home.deleteCells", json!({"range": "A5", "shift": "up"})).unwrap();
    assert_eq!(formula(&mut s, "C1"), "=SUM(A1:A5)");
    assert_eq!(formula(&mut s, "C2"), "=#REF!*10");
    assert_eq!(s.doc().unwrap().wb.names[0].formula, "#REF!");
    assert_eq!(v(&s, "C1"), Value::Number(11.0));
    let sh = s.doc().unwrap().wb.active().unwrap().clone();
    assert_eq!(sh.cond_formats[0].ranges[0].a1(), "A5");
    assert_eq!(sh.validations[0].ranges[0].a1(), "A5");
    assert_eq!(sh.charts[0].series[0].values, "Sheet1!$A$1:$A$5");
    // Shifting right and left.
    s.execute("range.setValues", json!({"range": "E1", "values": [[1, 2, 3]]})).unwrap();
    s.execute("cell.set", json!({"cell": "E3", "input": "=G1+SUM(E1:G1)"})).unwrap();
    s.execute("home.insertCells", json!({"range": "F1", "shift": "right"})).unwrap();
    assert_eq!(formula(&mut s, "E3"), "=H1+SUM(E1:H1)");
    s.execute("home.deleteCells", json!({"range": "E1:F1", "shift": "left"})).unwrap();
    assert_eq!(formula(&mut s, "E3"), "=F1+SUM(E1:F1)");
    assert_eq!(v(&s, "E3"), Value::Number(8.0));
}

#[test]
fn sort_and_filter() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Score"], ["b", 2], ["c", 3], ["a", 1]]})).unwrap();
    s.execute("selection.set", json!({"cell": "B2"})).unwrap();
    s.execute("data.sortDescending", json!({})).unwrap();
    assert_eq!(v(&s, "A2"), Value::from("c"));
    assert_eq!(v(&s, "A1"), Value::from("Name"));
    s.execute("data.filter", json!({})).unwrap();
    let r = s.execute("data.filterBy", json!({"column": "B", "custom": {"op": ">", "value": "1"}})).unwrap();
    assert_eq!(r["hiddenRows"], 1);
}

/// A sheet with locked data in A1:B3, unlocked cells in D1:D3 and row 10, then protected.
fn protected(extra: serde_json::Value) -> Session {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=A1+D1"})).unwrap();
    s.execute("range.setValues", json!({"range": "D1", "values": [[10], [20], [30]]})).unwrap();
    s.execute("home.lockCell", json!({"range": "D1:D3", "on": false})).unwrap();
    s.execute("home.lockCell", json!({"range": "10:10", "on": false})).unwrap();
    s.execute("review.protectSheet", extra).unwrap();
    s
}

#[test]
fn protection_refuses_edits_of_locked_cells() {
    let refused = |s: &mut Session, id: &str, p: serde_json::Value| {
        let before = s.doc().unwrap().wb.clone();
        let e = s.execute(id, p).expect_err(id).to_string();
        assert!(e.contains("protected sheet"), "{id}: {e}");
        assert_eq!(*s.doc().unwrap().wb, *before, "{id} changed the workbook");
    };
    let mut s = protected(json!({}));
    refused(&mut s, "cell.set", json!({"cell": "A1", "input": "9"}));
    refused(&mut s, "edit.clearContents", json!({"range": "A1:A3"}));
    refused(&mut s, "edit.clearAll", json!({"range": "A1"}));
    refused(&mut s, "range.setValues", json!({"range": "A2", "values": [[9]]}));
    refused(&mut s, "range.fill", json!({"range": "A5:A6", "input": "x"}));
    refused(&mut s, "edit.fillDown", json!({"range": "A1:A3"}));
    refused(&mut s, "edit.fillRight", json!({"range": "A1:B1"}));
    refused(&mut s, "edit.fillUp", json!({"range": "A1:A3"}));
    refused(&mut s, "edit.fillLeft", json!({"range": "A1:B1"}));
    refused(&mut s, "edit.autoFill", json!({"source": "A1", "target": "A1:A5"}));
    s.execute("edit.copy", json!({"range": "D1"})).unwrap();
    refused(&mut s, "edit.paste", json!({"at": "A2"}));
    refused(&mut s, "edit.paste", json!({"at": "A2", "text": "x\ty"}));
    refused(&mut s, "home.deleteRows", json!({"rows": "2:2"}));
    refused(&mut s, "home.deleteColumns", json!({"cols": "A:A"}));
    refused(&mut s, "home.insertRows", json!({"rows": "1:1"}));
    refused(&mut s, "home.deleteCells", json!({"range": "A1", "shift": "up"}));
    refused(&mut s, "home.insertCells", json!({"range": "A1", "shift": "down"}));
    refused(&mut s, "edit.replace", json!({"what": "2", "with": "5"}));
    refused(&mut s, "data.goalSeek", json!({"set": "B1", "to": 50, "changing": "A1"}));
    // Unlocked cells stay editable.
    s.execute("edit.paste", json!({"at": "D2"})).unwrap();
    assert_eq!(v(&s, "D2"), Value::Number(10.0));
    s.execute("range.setValues", json!({"range": "D2", "values": [[5]]})).unwrap();
    s.execute("edit.fillDown", json!({"range": "D1:D3"})).unwrap();
    assert_eq!(v(&s, "D3"), Value::Number(10.0));
    s.execute("edit.clearContents", json!({"range": "D3"})).unwrap();
    assert_eq!(v(&s, "D3"), Value::Empty);
    s.execute("data.goalSeek", json!({"set": "B1", "to": 50, "changing": "D1"})).unwrap();
    assert!((v(&s, "B1").as_f64().unwrap() - 50.0).abs() < 0.001);
    s.execute("edit.replace", json!({"what": "49", "with": "48", "wholeCell": true})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(1.0));
}

#[test]
fn protection_allows_permitted_row_and_column_edits() {
    let mut s = protected(json!({"insertRows": true, "deleteRows": true, "insertColumns": true}));
    // Inserting shifts locked cells, which the protection allows.
    s.execute("home.insertRows", json!({"rows": "1:1"})).unwrap();
    assert_eq!(v(&s, "A2"), Value::Number(1.0));
    s.execute("home.insertColumns", json!({"cols": "A:A"})).unwrap();
    // Deleting a row with locked cells isn't; deleting an unlocked row is.
    assert!(s.execute("home.deleteRows", json!({"rows": "2:2"})).is_err());
    s.execute("home.deleteRows", json!({"rows": "11:11"})).unwrap();
    assert!(s.execute("home.deleteColumns", json!({"cols": "Z:Z"})).is_err());
}

#[test]
fn hidden_formulas_stay_hidden_on_protected_sheets() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "=SEQUENCE(3)*7"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=1+1"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": {"protection": {"locked": true, "hidden": true}}})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    let text = |s: &Session, a: &str| {
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        crate::display::cell_text(&d.wb, sh, CellRef::parse(a).unwrap())
    };
    assert_eq!(text(&s, "A1"), "=SEQUENCE(3)*7");
    assert_eq!(s.execute("edit.find", json!({"what": "SEQUENCE", "all": true})).unwrap()["count"], 1);
    s.execute("review.protectSheet", json!({})).unwrap();
    assert_eq!(text(&s, "A1"), "7");
    assert_eq!(text(&s, "B1"), "=1+1");
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert!(crate::display::formula_hidden(&d.wb, sh, CellRef::parse("A2").unwrap()));
    assert!(!crate::display::formula_hidden(&d.wb, sh, CellRef::parse("B1").unwrap()));
    assert!(s.execute("edit.find", json!({"what": "SEQUENCE", "all": true})).is_err());
    let r = s.execute("edit.find", json!({"what": "7", "all": true})).unwrap();
    assert_eq!(r["results"][0]["value"], "7");
}

#[test]
fn formatting_and_styles() {
    let mut s = s();
    s.execute("selection.set", json!({"range": "A1:B2"})).unwrap();
    s.execute("home.bold", json!({})).unwrap();
    s.execute("home.fillColor", json!({"color": "#FFFF00"})).unwrap();
    s.execute("home.mergeCenter", json!({})).unwrap();
    let c = s.execute("cell.get", json!({"cell": "B2"})).unwrap();
    assert_eq!(c["style"]["font"]["bold"], true);
    assert_eq!(c["merge"], "A1:B2");
    s.execute("home.cellStyle", json!({"range": "D1", "name": "Good"})).unwrap();
}

#[test]
fn spill_references_roundtrip_through_xlsx() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "B1", "input": "=SEQUENCE(4)"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=sum(b1#)"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Seq", "refersTo": "=Sheet1!$B$1#"})).unwrap();
    s.execute("cell.set", json!({"cell": "C2", "input": "=MAX(Seq)"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "C1"})).unwrap()["formula"], "=SUM(B1#)");
    assert_eq!((v(&s, "C1"), v(&s, "C2")), (Value::Number(10.0), Value::Number(4.0)));
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let b64 = r["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "x.xlsx", "base64": b64})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "C1"})).unwrap()["formula"], "=SUM(B1#)");
    assert_eq!(s.doc().unwrap().wb.names[0].formula, "Sheet1!$B$1#");
    assert_eq!((v(&s, "C1"), v(&s, "C2")), (Value::Number(10.0), Value::Number(4.0)));
}

#[test]
fn tables_and_structured_refs() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Item", "Qty"], ["x", 2], ["y", 5]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B3"})).unwrap();
    s.execute("cell.set", json!({"cell": "D1", "input": "=SUM(Table1[Qty])"})).unwrap();
    assert_eq!(v(&s, "D1"), Value::Number(7.0));
    s.execute("table.totalRow", json!({"on": true, "table": "Table1"})).unwrap();
    assert_eq!(v(&s, "B4"), Value::Number(7.0));
}

#[test]
fn create_names_from_selection() {
    let names = |s: &Session| -> Vec<(String, String)> {
        let mut v: Vec<_> = s.doc().unwrap().wb.names.iter().map(|n| (n.name.clone(), n.formula.clone())).collect();
        v.sort();
        v
    };
    let named = |n: &str, f: &str| (n.to_string(), f.to_string());
    let mut s = s();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [["", "Jan", "Feb", ""], ["North", 1, 2, "N"], ["South", 3, 4, "S"], ["", "First", "Second", ""]]}),
    )
    .unwrap();
    s.execute("formulas.createFromSelection", json!({"range": "A1:D4", "top": false, "bottom": true, "right": true})).unwrap();
    assert_eq!(
        names(&s),
        [named("First", "Sheet1!$B$1:$B$3"), named("N", "Sheet1!$A$2:$C$2"), named("S", "Sheet1!$A$3:$C$3"), named("Second", "Sheet1!$C$1:$C$3")]
    );
    // Existing names are kept unless replacing is asked for.
    let r = s.execute("formulas.createFromSelection", json!({"range": "A1:C3", "top": true, "left": true})).unwrap();
    assert_eq!(r["created"], 4);
    let r = s.execute("formulas.createFromSelection", json!({"range": "B1:C3", "top": true})).unwrap();
    assert_eq!((r["created"].clone(), r["skipped"].clone()), (json!(0), json!(["Jan", "Feb"])));
    s.execute("cell.set", json!({"cell": "B1", "input": "N"})).unwrap();
    let r = s.execute("formulas.createFromSelection", json!({"range": "B1:B3", "replace": true})).unwrap();
    assert_eq!(r["created"], 1);
    assert!(names(&s).contains(&named("N", "Sheet1!$B$2:$B$3")));
    assert!(names(&s).contains(&named("North", "Sheet1!$B$2:$C$2")));
}

#[test]
fn names_that_look_like_references_are_refused() {
    let mut s = s();
    for bad in ["R1C1", "r2", "C3", "RC", "R", "c", "rc12", "A1", "XFD1048576", "1st"] {
        assert!(s.execute("formulas.defineName", json!({"name": bad, "refersTo": "=1"})).is_err(), "{bad}");
    }
    for good in ["R1C1X", "Rate", "RCx", "Rx", "_R1", "Cost"] {
        s.execute("formulas.defineName", json!({"name": good, "refersTo": "=1"})).unwrap();
    }
}

#[test]
fn lambda_names_keep_their_case_in_calls() {
    let mut s = s();
    s.execute("formulas.defineName", json!({"name": "Double", "refersTo": "=LAMBDA(x,x*2)"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Quad", "refersTo": "=LAMBDA(x,Double(Double(x)))"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=double(4)+Quad(1)+sum(1)"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["formula"], "=Double(4)+Quad(1)+SUM(1)");
    assert_eq!(v(&s, "A1"), Value::Number(13.0));
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let b64 = r["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "x.xlsx", "base64": b64})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["formula"], "=Double(4)+Quad(1)+SUM(1)");
    let quad = s.doc().unwrap().wb.names.iter().find(|n| n.name == "Quad").unwrap().formula.clone();
    assert_eq!(quad, "LAMBDA(x,Double(Double(x)))");
    assert_eq!(v(&s, "A1"), Value::Number(13.0));
}

#[test]
fn sheets() {
    let mut s = s();
    s.execute("home.insertSheet", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "5"})).unwrap();
    s.execute("sheet.rename", json!({"name": "Data"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=Data!A1*2"})).unwrap();
    assert_eq!(v(&s, "A1"), Value::Number(10.0));
    s.execute("sheet.rename", json!({"sheet": "Data", "name": "My Data"})).unwrap();
    assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["formula"], "='My Data'!A1*2");
}

#[test]
fn sheet_operations_keep_names_on_their_sheets() {
    let names = |s: &Session| -> Vec<(String, Option<String>, String)> {
        let wb = &s.doc().unwrap().wb;
        let mut v: Vec<_> =
            wb.names.iter().map(|n| (n.name.clone(), n.scope.and_then(|i| wb.sheet(i)).map(|s| s.name.clone()), n.formula.clone())).collect();
        v.sort();
        v
    };
    let named = |n: &str, scope: Option<&str>, f: &str| (n.to_string(), scope.map(str::to_string), f.to_string());
    let mut s = s();
    s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Notes"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Rate", "refersTo": "=Data!$A$1", "scope": "Data"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Rate", "refersTo": "=Notes!$B$1", "scope": "Notes"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Total", "refersTo": "=Data!$C$1"})).unwrap();
    // Moving Data (index 1) to the end: its names go with it.
    s.execute("sheet.move", json!({"sheet": "Data", "to": 2})).unwrap();
    assert_eq!(names(&s), [named("Rate", Some("Data"), "Data!$A$1"), named("Rate", Some("Notes"), "Notes!$B$1"), named("Total", None, "Data!$C$1")]);
    // Copying Data to the front copies its sheet-level names, referring to the copy.
    s.execute("sheet.move", json!({"sheet": "Data", "to": 0, "copy": true})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets[0].name, "Data (2)");
    assert_eq!(
        names(&s),
        [
            named("Rate", Some("Data"), "Data!$A$1"),
            named("Rate", Some("Data (2)"), "'Data (2)'!$A$1"),
            named("Rate", Some("Notes"), "Notes!$B$1"),
            named("Total", None, "Data!$C$1")
        ]
    );
    // Deleting Data removes its names; names referring to it become #REF!.
    s.execute("home.deleteSheet", json!({"sheet": "Data"})).unwrap();
    assert_eq!(
        names(&s),
        [named("Rate", Some("Data (2)"), "'Data (2)'!$A$1"), named("Rate", Some("Notes"), "Notes!$B$1"), named("Total", None, "#REF!")]
    );
    // Deleting a name in one scope keeps the others.
    s.execute("formulas.deleteName", json!({"name": "rate", "scope": "Notes"})).unwrap();
    assert_eq!(names(&s), [named("Rate", Some("Data (2)"), "'Data (2)'!$A$1"), named("Total", None, "#REF!")]);
    assert!(s.execute("formulas.deleteName", json!({"name": "Total", "scope": "Notes"})).is_err());
    s.execute("formulas.deleteName", json!({"name": "Total", "scope": "Workbook"})).unwrap();
    assert_eq!(names(&s).len(), 1);
}

#[test]
fn xlsx_roundtrip_through_engine() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a", 1], ["b", 2]]})).unwrap();
    s.execute("cell.set", json!({"cell": "B3", "input": "=SUM(B1:B2)"})).unwrap();
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let b64 = r["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "x.xlsx", "base64": b64})).unwrap();
    assert_eq!(v(&s, "B3"), Value::Number(3.0));
}

#[test]
fn samples_build() {
    for (name, _) in crate::sample::SAMPLES {
        let wb = crate::sample::build(name).unwrap_or_else(|| panic!("sample {name}"));
        assert!(!wb.sheets[0].cells.is_empty());
    }
}

#[test]
fn every_command_survives_empty_params() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    for spec in crate::command_specs() {
        if matches!(spec.id, "file.close" | "file.open" | "file.save" | "file.saveAs" | "file.exportCsv" | "file.exportHtml" | "insert.picture") {
            continue;
        }
        let _ = s.execute(spec.id, json!({}));
        let _ = s.execute(spec.id, json!([1, 2]));
        let _ = s.execute(spec.id, json!({"range": "ZZZ999999999", "cell": "", "sheet": 99}));
    }
}

#[test]
fn parity_counts() {
    let (done, total) = crate::catalog::parity();
    assert!(total > 250);
    assert!(done > 150, "parity {done}/{total}");
}

#[test]
fn switch_row_column_on_a_chart_read_from_a_file() {
    let mut s = s();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [["", "North", "South", "East", "West"], ["Mon", 10, 14, 9, 12], ["Tue", 12, 15, 11, 14], ["Wed", 14, 16, 10, 11], ["Thu", 11, 13, 8, 9], ["Fri", 9, 12, 7, 11], ["Sat", 11, 15, 10, 14]]}),
    )
    .unwrap();
    s.execute("insert.chart", json!({"type": "column", "range": "A1:E7"})).unwrap();
    let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let b64 = r["base64"].as_str().unwrap().to_string();
    s.execute("file.open", json!({"name": "x.xlsx", "base64": b64})).unwrap();
    let chart = |s: &Session| s.doc().unwrap().wb.active().unwrap().charts[0].clone();
    assert_eq!((chart(&s).source, chart(&s).series.len()), (None, 4));
    s.execute("chart.switchRowColumn", json!({})).unwrap();
    let c = chart(&s);
    assert!(c.by_rows);
    assert_eq!(c.source.as_deref(), Some("Sheet1!A1:E7"));
    assert_eq!(c.series.len(), 6);
    assert_eq!(c.series[0].name.as_deref(), Some("Sheet1!$A$2"));
    assert_eq!(c.series[0].categories.as_deref(), Some("Sheet1!$B$1:$E$1"));
    assert_eq!(c.series[0].values, "Sheet1!$B$2:$E$2");
    s.execute("chart.switchRowColumn", json!({})).unwrap();
    assert_eq!(chart(&s).series.len(), 4);
}

#[test]
fn ink_strokes_and_ink_to_shape() {
    let mut s = s();
    // A rough closed box.
    let pts: Vec<[f64; 2]> = vec![[100.0, 100.0], [200.0, 102.0], [201.0, 180.0], [99.0, 181.0], [101.0, 104.0]];
    s.execute("draw.stroke", json!({"points": pts})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap().clone();
    assert_eq!(sh.shapes.len(), 1);
    assert_eq!(sh.shapes[0].kind, gridcraft_model::ShapeKind::Ink);
    let r = s.execute("draw.inkToShape", json!({})).unwrap();
    assert_eq!(r["kind"], "Rectangle");
    // An open stroke becomes a line.
    s.execute("draw.stroke", json!({"points": [[0.0, 0.0], [50.0, 10.0], [120.0, 30.0]]})).unwrap();
    assert_eq!(s.execute("draw.inkToShape", json!({})).unwrap()["kind"], "Line");
    assert!(s.execute("draw.stroke", json!({"points": [[1.0, 1.0]]})).is_err());
}
