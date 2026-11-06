//! Explicit default formats must survive insertion, history and file round trips.
use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_model::StyleId;
use serde_json::json;

fn book() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn style(s: &Session, address: &str) -> StyleId {
    s.doc().unwrap().wb.active().unwrap().style_id(CellRef::parse(address).unwrap())
}

fn check_round_trips(s: &mut Session, address: &str) {
    for format in ["xlsx", "json"] {
        let saved = s.execute("file.saveBytes", json!({"format": format})).unwrap();
        let mut loaded = Session::new();
        loaded.execute("file.open", json!({"name": format!("book.{format}"), "base64": saved["base64"]})).unwrap();
        assert_eq!(style(&loaded, address), StyleId::DEFAULT, "{format} lost {address}'s explicit default format");
        assert_eq!(loaded.doc().unwrap().wb.active().unwrap().value(CellRef::parse(address).unwrap()), Value::Empty);
    }
}

#[test]
fn partial_insert_preserves_explicit_default_formats() {
    for (formatted_lines, at, shift, moved) in [("3:4", "B3", "down", "B4"), ("C:D", "C2", "right", "D2")] {
        let mut s = book();
        s.execute("cell.set", json!({"cell": "B2", "input": "7"})).unwrap();
        s.execute("home.fillColor", json!({"range": formatted_lines, "color": "#FF0000"})).unwrap();
        let inherited = style(&s, at);
        assert_ne!(inherited, StyleId::DEFAULT);
        s.execute("home.insertCells", json!({"range": at, "shift": shift})).unwrap();
        assert_eq!(style(&s, at), StyleId::DEFAULT);
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(style(&s, at), inherited);
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(style(&s, at), StyleId::DEFAULT);
        check_round_trips(&mut s, at);
        // Shifting a previously inserted empty reset must not make it inherit again.
        s.execute("home.insertCells", json!({"range": at, "shift": shift})).unwrap();
        assert_eq!(style(&s, moved), StyleId::DEFAULT);
        check_round_trips(&mut s, moved);
    }
}

#[test]
fn whole_line_insert_preserves_a_default_override_in_the_source_line() {
    for (line, command, params, at, ordinary) in
        [("2:2", "home.insertRows", json!({"rows": "3:3"}), "B3", "A3"), ("B:B", "home.insertColumns", json!({"cols": "C:C"}), "C2", "C1")]
    {
        let mut s = book();
        s.execute("home.fillColor", json!({"range": line, "color": "#FF0000"})).unwrap();
        s.execute("cell.set", json!({"cell": "B2", "input": "7"})).unwrap();
        s.execute("edit.clearFormats", json!({"range": "B2"})).unwrap();
        assert_eq!(style(&s, "B2"), StyleId::DEFAULT);
        s.execute(command, params).unwrap();
        assert_eq!(style(&s, at), StyleId::DEFAULT);
        assert_ne!(style(&s, ordinary), StyleId::DEFAULT);
        check_round_trips(&mut s, at);
    }
}

#[test]
fn implicit_default_source_clears_a_formatted_destination_line() {
    for (line, at, shift) in [("3:3", "B3", "down"), ("C:C", "C2", "right")] {
        let mut s = book();
        s.execute("home.bold", json!({"range": line})).unwrap();
        assert_ne!(style(&s, at), StyleId::DEFAULT);
        assert_eq!(s.doc().unwrap().wb.active().unwrap().cells.len(), 0);
        s.execute("home.insertCells", json!({"range": at, "shift": shift})).unwrap();
        assert_eq!(style(&s, at), StyleId::DEFAULT, "an absent source cell still supplies the default format");
        assert_eq!(s.doc().unwrap().wb.active().unwrap().cells.len(), 1, "only the necessary reset is stored");
        check_round_trips(&mut s, at);
    }
    let mut s = book();
    s.execute("home.insertCells", json!({"range": "B3:Z10000", "shift": "down"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().cells.len(), 0, "ordinary defaults must not materialize empty cells");
    s.execute("home.bold", json!({"range": "2:2"})).unwrap();
    let capped = s.execute("home.insertCells", json!({"range": "A3:XFD70", "shift": "down"})).unwrap();
    assert_eq!(capped["formattingSkipped"], true, "over one million differing styles must be refused before allocation");
    assert_eq!(capped["formattingCellLimit"], 1_000_000);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().cells.len(), 0);
}

#[test]
fn copied_cells_carry_inherited_formats_between_sheets() {
    for (source, at, line, formatted, outside) in
        [("B2:C3", "E5", "2:2", "E5", "D5"), ("2:2", "A5", "2:2", "XFD5", "A4"), ("B:B", "E1", "B:B", "E1000000", "D1")]
    {
        let mut s = book();
        s.execute("home.bold", json!({"range": line})).unwrap();
        let expected = style(&s, "B2");
        s.execute("home.rowHeight", json!({"rows": "2:2", "height": 71})).unwrap();
        s.execute("home.columnWidth", json!({"cols": "B:B", "width": 137})).unwrap();
        s.execute("edit.copy", json!({"range": source})).unwrap();
        s.execute("home.insertSheet", json!({"name": "Destination"})).unwrap();
        // Destination dimensions and unrelated formats must not come from source axes.
        s.execute("home.rowHeight", json!({"rows": "4:6", "height": 31})).unwrap();
        s.execute("home.columnWidth", json!({"cols": "D:F", "width": 89})).unwrap();
        s.execute("home.insertCopiedCells", json!({"at": at, "shift": "down"})).unwrap();
        assert_eq!(style(&s, formatted), expected, "{source} must carry inherited formatting, including blank cells");
        assert_eq!(style(&s, outside), StyleId::DEFAULT);
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.row_height(4), 31.0, "source row height must not overwrite destination metadata");
        assert_eq!(sh.col_width(4), 89.0, "source column width must not overwrite destination metadata");
        assert!(s.doc().unwrap().wb.active().unwrap().cells.len() < 40_000, "whole lines stay sparse");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(style(&s, formatted), StyleId::DEFAULT);
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(style(&s, formatted), expected);
        for format in ["xlsx", "json"] {
            let saved = s.execute("file.saveBytes", json!({"format": format})).unwrap();
            let mut loaded = Session::new();
            loaded.execute("file.open", json!({"name": format!("book.{format}"), "base64": saved["base64"]})).unwrap();
            assert_eq!(style(&loaded, formatted), expected, "{format} lost copied style");
        }
    }
    let mut s = book();
    s.execute("edit.copy", json!({"range": "B2"})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Destination"})).unwrap();
    s.execute("home.bold", json!({"range": "3:3"})).unwrap();
    s.execute("home.insertCopiedCells", json!({"at": "B3"})).unwrap();
    assert_eq!(style(&s, "B3"), StyleId::DEFAULT, "blank default source must clear destination inheritance");
    check_round_trips(&mut s, "B3");
}

#[test]
fn copied_format_limit_rolls_back_the_whole_insertion() {
    let mut s = book();
    s.execute("home.bold", json!({"range": "2:70"})).unwrap();
    s.execute("edit.copy", json!({"range": "A2:XFC70"})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Destination"})).unwrap();
    let before = s.doc().unwrap().wb.clone();
    let depth = s.doc().unwrap().undo.len();
    assert!(s.execute("home.insertCopiedCells", json!({"at": "A1"})).is_err());
    assert_eq!(s.doc().unwrap().wb, before, "an oversized formatting transfer must leave no partial insertion");
    assert_eq!(s.doc().unwrap().undo.len(), depth);
    assert!(s.clipboard.is_some(), "a refused insertion must retain its source copy");
}
