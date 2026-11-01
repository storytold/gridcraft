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

// Build imported content without commands, which would already fit its row.
fn wrapped_column_workbook(hidden: bool, width: Option<f32>) -> Workbook {
    let mut wb = Workbook::new();
    let wrapped = wb.styles.derive(StyleId::DEFAULT, |s| s.align.wrap = true);
    let sh = wb.sheet_mut(0).unwrap();
    sh.default_col_width = 80.0;
    sh.set_value(CellRef::new(0, 0), Value::text("Label"));
    sh.set_value(CellRef::new(0, 1), Value::text("Wrapped text spans lines"));
    sh.set_style(CellRef::new(0, 1), wrapped);
    let col = sh.cols.entry(1).or_default();
    col.size = width;
    col.hidden = hidden;
    sh.rows.entry(0).or_default().size = Some(20.0);
    wb
}

fn wrapped_column_session(hidden: bool, width: Option<f32>) -> Session {
    let mut s = Session::new();
    s.add_document(DocState::new(wrapped_column_workbook(hidden, width), None, "Wrapped column".into(), Trust::Local));
    s
}

#[test]
fn hidden_wrapped_columns_match_visible_stored_width_on_import() {
    let mut heights = Vec::new();
    for width in [56.0, 120.0] {
        let mut fitted = Vec::new();
        for hidden in [false, true] {
            let bytes = gridcraft_xlsx::write_xlsx(&wrapped_column_workbook(hidden, Some(width))).unwrap();
            let (opened, _) = gridcraft_engine::io::open_bytes("wrapped-column.xlsx", &bytes).unwrap();
            let sh = opened.active().unwrap();
            fitted.push(sh.row_height(0));
            assert!(sh.row_height(0) > 20.0, "hidden wrapped content must still contribute");
            assert_eq!(sh.cols.get(&1).unwrap().hidden, hidden);
        }
        assert_eq!(fitted[0], fitted[1], "hidden and visible import heights must match at width {width}");
        heights.push(fitted[0]);
    }
    assert!(heights[0] > heights[1], "the narrower stored width must produce a taller row");
}

#[test]
fn hidden_wrapped_columns_match_visible_autofit_and_edits() {
    for width in [56.0, 120.0] {
        let mut fitted = Vec::new();
        let mut edited = Vec::new();
        for hidden in [false, true] {
            let mut s = wrapped_column_session(hidden, Some(width));
            s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
            fitted.push(row_height(&mut s, 0));
            s.execute("cell.set", json!({"cell": "B1", "input": "Wrapped text spans many more lines after editing"})).unwrap();
            edited.push(row_height(&mut s, 0));
            let sh = s.doc().unwrap().wb.active().unwrap();
            assert_eq!(sh.cols.get(&1).unwrap().size, Some(width));
            assert_eq!(sh.cols.get(&1).unwrap().hidden, hidden);
        }
        assert!(fitted[0] > 20.0, "AutoFit must include the wrapped cell");
        assert_eq!(fitted[0], fitted[1], "AutoFit must remember width {width}");
        assert!(edited[0] > fitted[0], "editing must refit the wrapped content");
        assert_eq!(edited[0], edited[1], "edits must remember width {width}");
    }
}

#[test]
fn hidden_wrapped_columns_without_a_stored_width_use_the_sheet_default() {
    let mut heights = Vec::new();
    for width in [None, Some(80.0), Some(160.0)] {
        let mut s = wrapped_column_session(true, width);
        s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
        heights.push(row_height(&mut s, 0));
        assert_eq!(s.doc().unwrap().wb.active().unwrap().cols.get(&1).unwrap().size, width, "fitting must preserve an absent width");
    }
    assert!(heights[0] > 20.0, "the default-width hidden cell must contribute");
    assert_eq!(heights[0], heights[1], "no width uses the sheet's 80px default");
    assert!(heights[0] > heights[2], "the fallback must honor the sheet default rather than a fixed width");
}

#[test]
fn a_hidden_wrapped_column_beside_a_visible_one_fits_the_taller_of_the_two() {
    let alone = |width: f32| {
        let mut s = wrapped_column_session(false, Some(width));
        s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
        row_height(&mut s, 0)
    };
    let (wide, narrow) = (alone(120.0), alone(56.0));
    assert!(narrow > wide, "the narrower column needs more lines");
    for (visible, hidden, expected) in [(120.0, 56.0, narrow), (56.0, 120.0, narrow), (120.0, 120.0, wide)] {
        let mut wb = wrapped_column_workbook(false, Some(visible));
        let wrapped = wb.styles.derive(StyleId::DEFAULT, |s| s.align.wrap = true);
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(0, 2), Value::text("Wrapped text spans lines"));
        sh.set_style(CellRef::new(0, 2), wrapped);
        let col = sh.cols.entry(2).or_default();
        col.size = Some(hidden);
        col.hidden = true;
        let mut s = Session::new();
        s.add_document(DocState::new(wb, None, "Two wrapped columns".into(), Trust::Local));
        s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
        assert_eq!(row_height(&mut s, 0), expected, "visible {visible}px beside hidden {hidden}px");
    }
}

#[test]
fn hiding_a_column_after_fitting_keeps_its_wrapped_height() {
    for width in [56.0, 120.0] {
        let mut s = wrapped_column_session(false, Some(width));
        s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
        let fitted = row_height(&mut s, 0);
        s.execute("home.hideColumns", json!({"cols": "B:B"})).unwrap();
        assert_eq!(row_height(&mut s, 0), fitted, "hiding must not resize the row at width {width}");
        s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
        assert_eq!(row_height(&mut s, 0), fitted, "refitting after hiding must remember width {width}");
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert!(sh.cols.get(&1).unwrap().hidden);
        assert_eq!(sh.cols.get(&1).unwrap().size, Some(width));
    }
}

#[test]
fn explicit_zero_width_retains_the_minimum_wrap_width_whether_hidden_or_visible() {
    // An explicitly stored zero is not an absent width: retain the existing clamp,
    // including the row-height cap, consistently with a visible zero-width column.
    for hidden in [false, true] {
        let mut s = wrapped_column_session(hidden, Some(0.0));
        s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
        assert_eq!(row_height(&mut s, 0), 546.0);
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.cols.get(&1).unwrap().size, Some(0.0));
        assert_eq!(sh.cols.get(&1).unwrap().hidden, hidden);
    }
}

#[test]
fn imported_issue_2724_header_uses_its_hidden_columns_stored_width() {
    let mut wb = Workbook::new();
    let wrapped = wb.styles.derive(StyleId::DEFAULT, |s| s.align.wrap = true);
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_value(CellRef::new(7, 0), Value::text("Visible heading"));
    sh.set_value(CellRef::new(7, 46), Value::text("Hidden wrapped budget heading"));
    sh.set_style(CellRef::new(7, 46), wrapped);
    let col = sh.cols.entry(46).or_default();
    col.size = Some(64.0);
    col.hidden = true;
    sh.rows.entry(7).or_default().size = Some(70.0);
    sh.freeze = Some((8, 10));
    sh.zoom = 130;
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let (opened, _) = gridcraft_engine::io::open_bytes("hidden-header.xlsx", &bytes).unwrap();
    assert_eq!(opened.active().unwrap().row_height(7), 77.0, "stored-width wrapping must not pin the frozen header at 546px");
}
