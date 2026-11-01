//! Pasting a table copied from Google Sheets keeps what Sheets records in the clipboard HTML: typed
//! numbers and dates with their formats, formulas (written in R1C1) and cell formatting.
//!
//! The HTML below is written by hand in the shape Sheets uses (`google-sheets-html-origin`,
//! `data-sheets-root`, `data-sheets-value`, `data-sheets-numberformat`, `data-sheets-formula`,
//! inline `style`), not captured from a real clipboard.

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::display::cell_text;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn sheets_table(rows: &str) -> String {
    format!(
        "<meta charset='utf-8'><google-sheets-html-origin><style type=\"text/css\"><!--td {{border: 1px solid #cccccc;}}--></style>\
         <table xmlns=\"http://www.w3.org/1999/xhtml\" cellspacing=\"0\" cellpadding=\"0\" dir=\"ltr\" border=\"1\" data-sheets-root=\"1\">\
         <colgroup><col width=\"100\"/><col width=\"100\"/><col width=\"100\"/></colgroup><tbody>{rows}</tbody></table>"
    )
}

fn at(cell: &str) -> CellRef {
    CellRef::parse(cell).unwrap()
}

fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().cell(at(cell)).map(|c| c.value.clone()).unwrap_or(Value::Empty)
}

fn shown(s: &Session, cell: &str) -> String {
    let d = s.doc().unwrap();
    cell_text(&d.wb, d.wb.active().unwrap(), at(cell))
}

fn formula(s: &Session, cell: &str) -> Option<String> {
    s.doc().unwrap().wb.active().unwrap().cell(at(cell)).and_then(|c| c.formula.as_ref().map(|f| f.text.clone()))
}

fn style(s: &Session, cell: &str) -> gridcraft_engine::model::Style {
    let d = s.doc().unwrap();
    d.wb.styles.get(d.wb.active().unwrap().style_id(at(cell))).clone()
}

const HEADER: &str =
    "overflow:hidden;padding:2px 3px 2px 3px;vertical-align:bottom;background-color:#ff0000;font-weight:bold;color:#ffffff;text-align:center;";

#[test]
fn a_table_from_sheets_keeps_dates_numbers_formulas_and_colours() {
    let mut s = session();
    let html = sheets_table(&format!(
        "<tr><td style=\"{HEADER}\" data-sheets-value=\"{{&quot;1&quot;:2,&quot;2&quot;:&quot;Date&quot;}}\">Date</td>\
         <td style=\"{HEADER}\" data-sheets-value=\"{{&quot;1&quot;:2,&quot;2&quot;:&quot;Price&quot;}}\">Price</td>\
         <td style=\"{HEADER}\" data-sheets-value=\"{{&quot;1&quot;:2,&quot;2&quot;:&quot;Double&quot;}}\">Double</td></tr>\
         <tr><td style=\"text-align:right;\" data-sheets-value=\"{{&quot;1&quot;:3,&quot;3&quot;:46323}}\" data-sheets-numberformat=\"{{&quot;1&quot;:2,&quot;2&quot;:&quot;dd/mm/yyyy&quot;,&quot;3&quot;:1}}\">28/10/2026</td>\
         <td data-sheets-value=\"{{&quot;1&quot;:3,&quot;3&quot;:10.5}}\" data-sheets-numberformat=\"{{&quot;1&quot;:2,&quot;2&quot;:&quot;0.00&quot;,&quot;3&quot;:1}}\">10.50</td>\
         <td style=\"font-style:italic;text-decoration:underline line-through;\" data-sheets-value=\"{{&quot;1&quot;:3,&quot;3&quot;:21}}\" data-sheets-formula=\"=RC[-1]*2\">21.00</td></tr>\
         <tr><td></td><td></td><td data-sheets-value=\"{{&quot;1&quot;:3,&quot;3&quot;:21}}\" data-sheets-formula=\"=SUM(R2C2:R[-1]C[-1])\">10.50</td></tr>"
    ));
    let text = "Date\tPrice\tDouble\n28/10/2026\t10.50\t21.00\n\t\t10.50\n";
    s.execute("edit.paste", json!({"at": "A1", "text": text, "html": html})).unwrap();

    // The header keeps its fill, white bold text and centring.
    let head = style(&s, "A1");
    assert_eq!(head.fill, gridcraft_engine::model::Fill::solid(gridcraft_engine::model::Color::Rgb(0xFF0000)));
    assert_eq!(head.font.color, gridcraft_engine::model::Color::Rgb(0xFFFFFF));
    assert!(head.font.bold);
    assert_eq!(head.align.h, gridcraft_engine::model::HAlign::Center);
    assert_eq!(value(&s, "A1"), Value::text("Date"));

    // The date is the serial Sheets recorded and shows in the format it had.
    assert_eq!(value(&s, "A2"), Value::number(46323.0));
    assert_eq!(shown(&s, "A2"), "28/10/2026");
    assert_eq!(style(&s, "A2").align.h, gridcraft_engine::model::HAlign::Right);
    assert_eq!(value(&s, "B2"), Value::number(10.5));
    assert_eq!(shown(&s, "B2"), "10.50");

    // R1C1 formulas land as A1 formulas relative to the pasted position.
    assert_eq!(formula(&s, "C2").as_deref(), Some("B2*2"));
    assert_eq!(formula(&s, "C3").as_deref(), Some("SUM($B$2:B2)"));
    let c2 = style(&s, "C2");
    assert!(c2.font.italic && c2.font.strike);
    assert_eq!(c2.font.underline, gridcraft_engine::model::Underline::Single);
}

#[test]
fn formulas_follow_the_cell_when_pasted_elsewhere() {
    let mut s = session();
    let html = sheets_table("<tr><td data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:3}\" data-sheets-formula=\"=R[-1]C[-2]+R5C1\">3</td></tr>");
    s.execute("edit.paste", json!({"at": "D10", "text": "3", "html": html})).unwrap();
    assert_eq!(formula(&s, "D10").as_deref(), Some("B9+$A$5"));
}

#[test]
fn a_typed_value_wins_over_how_the_text_parses() {
    // Sheets in a dd/mm/yyyy locale shows 1 February 2026; en-US input would read it as 2 January.
    let mut s = session();
    let html = sheets_table(
        "<tr><td data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:46054}\" data-sheets-numberformat=\"{&quot;1&quot;:2,&quot;2&quot;:&quot;dd/mm/yyyy&quot;,&quot;3&quot;:1}\">01/02/2026</td></tr>",
    );
    s.execute("edit.paste", json!({"at": "A1", "text": "01/02/2026", "html": html})).unwrap();
    assert_eq!(value(&s, "A1"), Value::number(46054.0));
    assert_eq!(shown(&s, "A1"), "01/02/2026");
}

#[test]
fn an_empty_coloured_cell_keeps_its_fill() {
    let mut s = session();
    let html = sheets_table("<tr><td style=\"background-color:rgb(255, 242, 204);\"></td><td>x</td></tr>");
    s.execute("edit.paste", json!({"at": "A1", "text": "\tx", "html": html})).unwrap();
    assert_eq!(style(&s, "A1").fill, gridcraft_engine::model::Fill::solid(gridcraft_engine::model::Color::Rgb(0xFFF2CC)));
    assert_eq!(value(&s, "B1"), Value::text("x"));
    assert_eq!(style(&s, "B1"), gridcraft_engine::model::Style::default());
}

#[test]
fn a_formula_that_cannot_be_read_leaves_the_value() {
    // Whole-row references and references off the sheet stay as the value Sheets showed.
    let mut s = session();
    let html = sheets_table(
        "<tr><td data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:7}\" data-sheets-formula=\"=SUM(R2:R3)\" style=\"font-weight:bold\">7</td>\
         <td data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:8}\" data-sheets-formula=\"=R[-1]C[0]\">8</td></tr>",
    );
    s.execute("edit.paste", json!({"at": "A1", "text": "7\t8", "html": html})).unwrap();
    assert_eq!((formula(&s, "A1"), value(&s, "A1")), (None, Value::number(7.0)));
    assert!(style(&s, "A1").font.bold, "formatting still applies");
    assert_eq!((formula(&s, "B1"), value(&s, "B1")), (None, Value::number(8.0)), "row 0 minus 1 is off the sheet");
}

#[test]
fn inline_styles_of_other_sources_are_not_applied() {
    // Only Sheets tables are read for formatting; any other page's inline CSS is left alone.
    let mut s = session();
    let html = "<table><tr><td style=\"background-color:#ff0000;font-weight:bold\">a</td></tr></table>";
    s.execute("edit.paste", json!({"at": "A1", "text": "a", "html": html})).unwrap();
    assert_eq!(style(&s, "A1"), gridcraft_engine::model::Style::default());
}

#[test]
fn hostile_sheets_attributes_do_not_crash_paste() {
    let mut s = session();
    for attrs in [
        "data-sheets-value=\"{\" data-sheets-numberformat=\"[\" data-sheets-formula=\"=\"",
        "data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:1e999}\" data-sheets-formula=\"=R[99999999999999999999]C[0]\"",
        "data-sheets-formula=\"=R1048577C1+R1C16385+RC[-1\" style=\"color:rgb(1,2\"",
        "data-sheets-numberformat=\"{&quot;2&quot;:&quot;[[[&quot;}\" data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:5}\"",
        "data-sheets-formula=\"='unterminated\" style=\"background:#12\"",
    ] {
        let html = sheets_table(&format!("<tr><td {attrs}>5</td></tr>"));
        s.execute("edit.paste", json!({"at": "A1", "text": "5", "html": html})).unwrap();
    }
}

#[test]
fn a_formula_with_a_sheets_only_function_keeps_the_value() {
    // QUERY, IMPORTRANGE and ARRAYFORMULA don't exist here: the formula would show #NAME?.
    let mut s = session();
    let html = sheets_table(
        "<tr><td data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:7}\" data-sheets-formula=\"=IMPORTRANGE(&quot;url&quot;,&quot;A1&quot;)+1\" style=\"font-weight:bold\">7</td>\
         <td data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:9}\" data-sheets-formula=\"=IF(RC[-1]&gt;1,SUM(RC[-1],2),0)\">9</td></tr>",
    );
    s.execute("edit.paste", json!({"at": "A1", "text": "7\t9", "html": html})).unwrap();
    assert_eq!((formula(&s, "A1"), value(&s, "A1")), (None, Value::number(7.0)));
    assert!(style(&s, "A1").font.bold);
    assert_eq!(formula(&s, "B1").as_deref(), Some("IF(A1>1,SUM(A1,2),0)"), "functions GridCraft has, special forms included, stay formulas");
}

#[test]
fn a_boolean_cell_pastes_as_a_boolean() {
    // Sheets records a checkbox or TRUE/FALSE as type 4; the text is "TRUE"/"FALSE".
    let mut s = session();
    let html = sheets_table(
        "<tr><td data-sheets-value=\"{&quot;1&quot;:4,&quot;4&quot;:1}\" style=\"text-align:center;\">TRUE</td>\
         <td data-sheets-value=\"{&quot;1&quot;:4,&quot;4&quot;:0}\">FALSE</td>\
         <td data-sheets-value=\"{&quot;1&quot;:4,&quot;4&quot;:1}\" data-sheets-formula=\"=RC[-2]=RC[-2]\">TRUE</td></tr>",
    );
    s.execute("edit.paste", json!({"at": "A1", "text": "TRUE\tFALSE\tTRUE", "html": html})).unwrap();
    assert_eq!(value(&s, "A1"), Value::Bool(true));
    assert_eq!(value(&s, "B1"), Value::Bool(false));
    assert_eq!(shown(&s, "A1"), "TRUE");
    assert_eq!(style(&s, "A1").align.h, gridcraft_engine::model::HAlign::Center);
    assert_eq!(formula(&s, "C1").as_deref(), Some("A1=A1"));
    assert_eq!(value(&s, "C1"), Value::Bool(true));
}

#[test]
fn html_that_shows_other_text_than_the_pasted_field_changes_nothing() {
    // HTML left over from another copy: the cell shows 2 and holds =1+1, the pasted text says 99.
    let mut s = session();
    let html = sheets_table(
        "<tr><td data-sheets-value=\"{&quot;1&quot;:3,&quot;3&quot;:2}\" data-sheets-formula=\"=1+1\" style=\"background-color:#ff0000;font-weight:bold\">2</td></tr>",
    );
    s.execute("edit.paste", json!({"at": "A1", "text": "99", "html": html})).unwrap();
    assert_eq!((formula(&s, "A1"), value(&s, "A1")), (None, Value::number(99.0)));
    assert_eq!(style(&s, "A1"), gridcraft_engine::model::Style::default());
}
