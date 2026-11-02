//! #1969: a wrapped cell's row must fit WORD-wrapped lines, not the character-fit
//! estimate (words can't split, so each drawn line ends early and narrow columns
//! need more lines than width/estimate suggests).

use gridcraft_engine::Session;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn row_height(s: &mut Session, row: u32) -> f32 {
    s.doc().unwrap().wb.active().unwrap().row_height(row)
}

#[test]
fn wrapped_row_fits_word_wrapped_lines() {
    let mut s = session();
    // Column fits ~10 characters; six-character words mean only one word per line.
    s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 10})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "aaaaaa bbbbbb cccccc dddddd"})).unwrap();
    let plain = row_height(&mut s, 0);
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let wrapped = row_height(&mut s, 0);
    // Word wrap: 4 lines (one word each). The old estimate: ceil(28/10) = 3 lines.
    // Line height at 11pt is ~19.8, so 4 lines need ~79 vs ~59 for 3.
    assert!(wrapped >= plain + 3.0 * 19.0, "4 word-wrapped lines must grow the row by ~4 line heights: plain={plain}, wrapped={wrapped}");
}

#[test]
fn issue_repro_nasa_sentence() {
    let mut s = session();
    s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 12})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "Since the 1960s, NASA has left hardware across the solar system\u{2013}on the Moon, on Mars, and in deep space."}))
        .unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let h = row_height(&mut s, 0);
    // Excel shows this at ~10 lines in a 12-char column; the character estimate undercounts.
    assert!(h >= 9.0 * 19.0, "NASA sentence in a 12-char column needs ~10 lines, got height {h}");
}

/// A front end whose font draws every character 20 points wide, far wider than the estimate.
fn wide_font(s: &mut Session) {
    s.text_width = Some(std::sync::Arc::new(|text: &str, _font: &gridcraft_engine::model::Font, _exact: bool| text.chars().count() as f32 * 20.0));
}

#[test]
fn wrapped_row_is_measured_in_the_front_ends_font() {
    let mut s = session();
    wide_font(&mut s);
    s.execute("home.columnWidth", json!({"cols": "A:A", "width": 100})).unwrap();
    // The grid wraps at 100 - 5 = 95 points: "aaaa bbbb" measures 180, so each word takes a
    // line. The estimate fits two of these words per line (two lines).
    s.execute("cell.set", json!({"cell": "A1", "input": "aaaa bbbb cccc dddd"})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let h = row_height(&mut s, 0);
    assert!(h >= 4.0 * 19.0, "4 measured lines need ~4 line heights, got {h}");
}

#[test]
fn an_unusable_measure_falls_back_to_the_estimate() {
    let mut s = session();
    s.text_width = Some(std::sync::Arc::new(|_: &str, _: &gridcraft_engine::model::Font, _: bool| f32::NAN));
    s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 10})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "aaaaaa bbbbbb cccccc dddddd"})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let h = row_height(&mut s, 0);
    assert!(h.is_finite() && h >= 4.0 * 19.0, "NaN widths must not shrink or poison the row, got {h}");
}

#[test]
fn rows_refit_when_the_fonts_change_but_keep_custom_heights() {
    let mut s = session();
    s.execute("home.columnWidth", json!({"cols": "A:A", "width": 100})).unwrap();
    s.execute("range.setValues", json!({"range": "A1:A2", "values": [["aaaa bbbb cccc dddd"], ["aaaa bbbb cccc dddd"]]})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1:A2", "on": true})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "2", "height": 20})).unwrap();
    let estimated = row_height(&mut s, 0);
    let undo_len = s.doc().unwrap().undo.len();
    // The front end's fonts arrive after the workbook was sized by estimate.
    wide_font(&mut s);
    s.refit_wrapped_rows();
    assert!(row_height(&mut s, 0) >= 4.0 * 19.0 && row_height(&mut s, 0) > estimated, "the wrapped row grows to the measured lines");
    assert_eq!(row_height(&mut s, 1), 20.0, "a custom height is kept");
    assert_eq!(s.doc().unwrap().undo.len(), undo_len, "a refit is not an edit");
}

#[test]
fn undo_restores_the_saved_heights_without_refitting() {
    // Refitting every wrapped row on each undo made undo of a one-cell edit take seconds on a
    // big sheet (#1973 review); undo restores what it saved, as Excel does.
    let mut s = session();
    s.execute("home.columnWidth", json!({"cols": "A:A", "width": 100})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "aaaa bbbb cccc dddd"})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let estimated = row_height(&mut s, 0);
    s.execute("cell.set", json!({"cell": "C1", "input": "x"})).unwrap();
    wide_font(&mut s);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(row_height(&mut s, 0), estimated, "undo puts back the height it saved");
}

#[test]
fn a_line_may_end_after_a_hyphen_but_not_an_en_dash() {
    // The grid wraps at 100 points, plus the half point egui allows. As egui breaks lines, a
    // hyphen ends "aa-", "bbbbb" fills the next line and " c" takes a third. After an en dash
    // there is no break: "aa–bb" | "bbb c".
    for (text, lines) in [("aa-bbbbb c", 3.0), ("aa\u{2013}bbbbb c", 2.0)] {
        let mut s = session();
        wide_font(&mut s);
        s.execute("home.columnWidth", json!({"cols": "A:A", "width": 105})).unwrap();
        s.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
        s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
        let h = row_height(&mut s, 0);
        assert!((lines * 19.0..(lines + 1.0) * 19.0).contains(&h), "{text:?}: {lines} lines, got {h}");
    }
}

#[test]
fn a_word_wider_than_the_column_breaks_between_characters() {
    let mut s = session();
    wide_font(&mut s);
    // Wraps at 90 points: four 20-point characters a line, so nine take three lines (the
    // word's width over the column's would say two).
    s.execute("home.columnWidth", json!({"cols": "A:A", "width": 95})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "aaaaaaaaa"})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let h = row_height(&mut s, 0);
    assert!(h >= 3.0 * 19.0, "three lines, got {h}");
}
