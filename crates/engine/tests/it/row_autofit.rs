//! Whole-row and whole-column formatting must re-fit row heights (issue #833).

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::{DocState, Session, Trust};
use gridcraft_model::{StyleId, Workbook};
use serde_json::json;

/// Makes the first sheet writable so tests can set row metadata without triggering commands.
fn sheet_mut(s: &mut gridcraft_engine::Session) -> &mut gridcraft_engine::model::Sheet {
    std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap()
}

/// Whole-row font changes fit the row, and undo restores its previous height.
#[test]
fn whole_row_font_format_grows_row_height_and_undoes() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A5", "values": [["plain big bold"]]})).unwrap();
    let before = s.doc().unwrap().wb.active().unwrap().row_height(4);
    s.execute("home.fontSize", json!({"range": "5:5", "size": 20})).unwrap();
    let after = s.doc().unwrap().wb.active().unwrap().row_height(4);
    assert!(after > before + 10.0, "20pt whole-row format should grow the row: before={before}, after={after}");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().row_height(4), before, "undo must restore the height");
}

/// Whole-column formatting fits populated rows without materializing every empty row.
#[test]
fn whole_column_font_format_grows_only_rows_with_cells() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a"]]})).unwrap();
    s.execute("range.setValues", json!({"range": "A50", "values": [["b"]]})).unwrap();
    s.execute("home.fontSize", json!({"range": "A:A", "size": 20})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.row_height(0) > 30.0 && sh.row_height(49) > 30.0, "rows with cells grow");
    assert!(sh.rows.len() <= 3, "full-column format must fit only rows that have cells, rows map has {} entries", sh.rows.len());
}

/// An explicit row height takes precedence over automatic fitting after a font change.
#[test]
fn whole_row_format_does_not_resize_custom_height_rows() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A5", "values": [["plain big bold"]]})).unwrap();
    {
        let sh = sheet_mut(&mut s);
        let e = sh.rows.entry(4).or_default();
        e.custom = true;
        e.size = Some(25.0);
    }
    s.execute("home.fontSize", json!({"range": "5:5", "size": 20})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().row_height(4), 25.0, "custom-height rows keep their height");
}

/// Formatting may fit a hidden row's content but must preserve its hidden state.
#[test]
fn whole_row_format_keeps_hidden_rows_hidden() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A5", "values": [["plain big bold"]]})).unwrap();
    let before = s.doc().unwrap().wb.active().unwrap().row_height(4);
    sheet_mut(&mut s).rows.entry(4).or_default().hidden = true;
    s.execute("home.fontSize", json!({"range": "5:5", "size": 20})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    let info = sh.rows.get(&4).unwrap();
    assert!(info.size.unwrap_or(before) > before + 10.0, "hidden row still grows to its font: before={before}, size={:?}", info.size);
    assert!(info.hidden, "formatting a hidden row must not unhide it");
}

/// Builds an unfitted row with a wrapped B1 cell and controlled width, visibility and height.
/// Direct model setup avoids command-triggered fitting before the operation under test.
fn wrapped_column_workbook(hidden: bool, width: Option<f32>, custom_height: bool) -> Workbook {
    let mut wb = Workbook::new();
    let wrapped = wb.styles.derive(StyleId::DEFAULT, |style| style.align.wrap = true);
    let sh = wb.sheet_mut(0).unwrap();
    sh.default_col_width = 80.0;
    sh.set_value(CellRef::new(0, 0), Value::text("Label"));
    sh.set_value(CellRef::new(0, 1), Value::text("Wrapped text spans lines"));
    sh.set_style(CellRef::new(0, 1), wrapped);
    let col = sh.cols.entry(1).or_default();
    col.hidden = hidden;
    col.size = width;
    let row = sh.rows.entry(0).or_default();
    row.size = Some(20.0);
    row.custom = custom_height;
    wb
}

/// Imports the synthetic XLSX through file.open to exercise fitting during real document loading.
fn open_wrapped_column(hidden: bool, width: Option<f32>, custom_height: bool) -> Session {
    let bytes = gridcraft_xlsx::write_xlsx(&wrapped_column_workbook(hidden, width, custom_height)).unwrap();
    let mut s = Session::new();
    s.execute("file.open", json!({"name": "wrapped-column.xlsx", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    s
}

/// Import uses hidden columns' stored or default wrap width while preserving visibility.
#[test]
fn imported_wrapped_rows_use_hidden_columns_stored_width() {
    let mut heights = Vec::new();
    for width in [None, Some(56.0), Some(80.0), Some(120.0)] {
        let visible = open_wrapped_column(false, width, false);
        let hidden = open_wrapped_column(true, width, false);
        let expected = visible.doc().unwrap().wb.active().unwrap().row_height(0);
        let sh = hidden.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.row_height(0), expected, "hiding a column must not change the imported row's wrap width: {width:?}");
        assert!(expected > 20.0, "the wrapped hidden cell must still contribute to the row height: {width:?}");
        assert!(sh.cols.get(&1).unwrap().hidden, "fitting must not unhide the column");
        heights.push(expected);
    }
    assert_eq!(heights[0], heights[2], "an absent stored width must use the sheet default");
    assert!(heights[1] > heights[3], "a narrow stored width must wrap onto more lines than a wide one");
}

/// Explicit AutoFit includes hidden text, releases custom height and restores it on undo.
#[test]
fn explicit_row_autofit_uses_hidden_columns_stored_or_default_width() {
    let mut heights = Vec::new();
    for width in [None, Some(56.0), Some(80.0), Some(120.0)] {
        let mut fitted = Vec::new();
        for hidden in [false, true] {
            let mut s = Session::new();
            s.add_document(DocState::new(wrapped_column_workbook(hidden, width, true), None, "Wrapped column".into(), Trust::Local));
            assert_eq!(s.doc().unwrap().wb.active().unwrap().row_height(0), 20.0);
            s.execute("home.autofitRowHeight", json!({"rows": "1:1"})).unwrap();
            let sh = s.doc().unwrap().wb.active().unwrap();
            fitted.push(sh.row_height(0));
            assert!(sh.row_height(0) > 20.0, "AutoFit must include the wrapped cell, including when hidden");
            assert!(!sh.rows.get(&0).unwrap().custom, "explicit AutoFit releases the custom height");
            assert_eq!(sh.cols.get(&1).unwrap().hidden, hidden);
            assert_eq!(sh.cols.get(&1).unwrap().size, width, "AutoFit must not change the stored column width");
            s.execute("edit.undo", json!({})).unwrap();
            let sh = s.doc().unwrap().wb.active().unwrap();
            assert_eq!(sh.row_height(0), 20.0, "undo restores the explicit row height");
            assert!(sh.rows.get(&0).unwrap().custom);
            assert_eq!(sh.cols.get(&1).unwrap().hidden, hidden);
        }
        assert_eq!(fitted[0], fitted[1], "hidden and visible columns must have the same AutoFit wrap width: {width:?}");
        heights.push(fitted[0]);
    }
    assert_eq!(heights[0], heights[2], "an absent stored width must use the sheet default");
    assert!(heights[1] > heights[3], "AutoFit must use the stored width when present");
}

/// Opening a workbook preserves custom row heights whether the wrapped column is hidden or visible.
#[test]
fn imported_wrapped_hidden_columns_keep_custom_row_heights() {
    for hidden in [false, true] {
        let s = open_wrapped_column(hidden, Some(56.0), true);
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.row_height(0), 20.0, "import must preserve an explicit custom height");
        assert!(sh.rows.get(&0).unwrap().custom);
        assert_eq!(sh.cols.get(&1).unwrap().hidden, hidden);
    }
}
