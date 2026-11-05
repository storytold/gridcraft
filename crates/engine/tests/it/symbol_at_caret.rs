//! Insert › Symbol puts the symbol at the caret, in place of the selected text (issue #3424).

use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use serde_json::json;

fn insert(text: &str, params: serde_json::Value) -> String {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    s.execute("insert.symbol", params).unwrap();
    let d = s.doc().unwrap();
    d.wb.active().unwrap().cell(CellRef::new(0, 0)).map(|c| c.input_text()).unwrap_or_default()
}

#[test]
fn the_symbol_goes_in_at_the_caret() {
    assert_eq!(insert("HelloWorld", json!({"char": "€", "caret": 5})), "Hello€World");
}

#[test]
fn the_symbol_replaces_the_selected_text() {
    assert_eq!(insert("HelloWorld", json!({"char": "€", "caret": 2, "anchor": 7})), "He€rld");
    assert_eq!(insert("HelloWorld", json!({"char": "€", "caret": 7, "anchor": 2})), "He€rld");
}

#[test]
fn caret_positions_count_characters_and_past_the_end_is_the_end() {
    assert_eq!(insert("αβγδ", json!({"char": "€", "caret": 2})), "αβ€γδ");
    assert_eq!(insert("Hello", json!({"char": "€", "caret": 99, "anchor": 1_000_000})), "Hello€");
}

#[test]
fn without_a_caret_the_symbol_goes_at_the_end() {
    assert_eq!(insert("Hello", json!({"char": "€"})), "Hello€");
}
