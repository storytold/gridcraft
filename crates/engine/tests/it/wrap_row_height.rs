//! #1969: a wrapped cell's row must fit WORD-wrapped lines, not the character-fit
//! estimate (words can't split, so each drawn line ends early and narrow columns
//! need more lines than width/estimate suggests).

use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::{StyleId, Workbook};
use gridcraft_engine::{DocState, Session, Trust};
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

#[test]
fn hidden_columns_do_not_inflate_automatic_row_heights() {
    for width in [0.0, 64.0] {
        let mut wb = Workbook::new();
        let wrapped = wb.styles.derive(StyleId::DEFAULT, |s| {
            s.align.wrap = true;
            s.font.size = 24.0;
        });
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(0, 0), Value::text("Visible heading"));
        sh.set_value(CellRef::new(0, 1), Value::text("Hidden heading that must not set the visible row height"));
        sh.set_style(CellRef::new(0, 1), wrapped);
        let col = sh.cols.entry(1).or_default();
        col.size = Some(width);
        col.hidden = width != 0.0;
        sh.rows.entry(0).or_default().size = Some(70.0);
        let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
        let (opened, _) = gridcraft_engine::io::open_bytes("hidden-header.xlsx", &bytes).unwrap();
        assert_eq!(opened.active().unwrap().row_height(0), 70.0, "open keeps the saved height, width={width}");

        let mut s = Session::new();
        s.add_document(DocState::new(opened, None, "Hidden header".into(), Trust::Local));
        s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
        let sh = s.active().unwrap().wb.active().unwrap();
        assert_eq!(sh.row_height(0), sh.default_row_height, "AutoFit measures only the visible heading, width={width}");
        assert_eq!(sh.value(CellRef::new(0, 1)), Value::text("Hidden heading that must not set the visible row height"));
        s.execute("home.unhideColumns", json!({"cols": "B:B"})).unwrap();
        s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
        assert!(s.active().unwrap().wb.active().unwrap().row_height(0) > 70.0, "the heading fits once its column is visible");
    }
}
