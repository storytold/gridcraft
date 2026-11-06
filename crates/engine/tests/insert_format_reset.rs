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
