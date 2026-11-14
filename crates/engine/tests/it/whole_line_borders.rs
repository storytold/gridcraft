//! A whole row or column's borders are its line format, never a cell per row (#2484).
//!
//! Excel stores a whole-column border as the column's default style, merged into the cells that
//! exist. GridCraft wrote a styled cell for every row of the column: 1,048,576 of them, a 47 MB
//! sheet part whose element count made the reader call the sheet damaged and drop what followed,
//! so `Sales!C11`, a `SUBTOTAL` over the table, came back `#REF!` after a save and reload while
//! the in-memory sheet had the right total. Found by the command-sequence fuzzer on the `sales`
//! sample after `home.drawBorder` on the whole column `F:F`.

use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::BorderStyle;
use gridcraft_engine::{Session, io};
use serde_json::json;

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(at(cell))
}

/// The left border `home.drawBorder` puts on the whole column `F` of the `sales` sample, whose
/// table runs from `B4` to `I11` with the `SUBTOTAL`s of its totals row in row 11.
#[test]
fn a_whole_column_border_keeps_the_table_subtotal_through_a_round_trip() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    s.execute("home.drawBorder", json!({"range": "F:F", "edge": "left", "color": "#1F4E79"})).unwrap();
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        assert!(sh.cols.get(&5).and_then(|i| i.style).is_some(), "the border is the column's line style");
        assert!(sh.cells.len() < 200, "not a cell per row of the column: {} cells", sh.cells.len());
    }
    let before = value(&s, "C11");
    assert_eq!(before, Value::Number(304_820.0), "the in-memory total");
    // The file holds the sample's cells, not a million, and reads back the total -- where it used
    // to be #REF!, the reader having dropped the table past its element cap.
    let bytes = {
        let doc = s.doc().unwrap();
        io::save_bytes(&doc.wb, "border.xlsx").unwrap()
    };
    assert!(bytes.len() < 200_000, "no cell per row of the column in the file: {} bytes", bytes.len());
    let (back, _) = io::open_bytes("border.xlsx", &bytes).unwrap();
    let sh = back.sheet(0).unwrap();
    assert_eq!(sh.value(at("C11")), before, "the total survives the round trip");
    assert!(sh.cols.get(&5).and_then(|i| i.style).is_some(), "and so does the border");
}

/// A whole row works the same way.
#[test]
fn a_whole_row_border_is_the_row_format_too() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    s.execute("home.drawBorder", json!({"range": "3:3", "edge": "top", "color": "#1F4E79"})).unwrap();
    let doc = s.doc().unwrap();
    let sh = doc.wb.active().unwrap();
    assert!(sh.rows.get(&2).and_then(|i| i.style).is_some(), "the border is the row's line style");
    assert!(sh.cells.len() < 200, "not a cell per column of the row: {} cells", sh.cells.len());
}

/// A border of an ordinary range still styles its cells one by one, edges of the range included.
#[test]
fn an_ordinary_range_still_styles_its_cells_one_by_one() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["x"], ["y"], ["z"]]})).unwrap();
    s.execute("home.drawBorder", json!({"range": "A1:A2", "edge": "left"})).unwrap();
    let doc = s.doc().unwrap();
    let sh = doc.wb.active().unwrap();
    let left = |c: &str| doc.wb.styles.get(sh.style_id(at(c))).borders.left.style;
    assert_ne!(left("A1"), BorderStyle::None, "the selected cell takes the border");
    assert_ne!(left("A2"), BorderStyle::None, "the last row of the range too");
    assert_eq!(left("A3"), BorderStyle::None, "the cell below keeps none");
    assert!(sh.cols.get(&0).and_then(|i| i.style).is_none(), "an ordinary range is not a line format");
}

/// The edges Excel drops for a whole line, and the cell it materialises: the left edge of a whole
/// row lands on its first cell (created when the sheet row is empty), while a whole row's right
/// edge and a whole column's bottom edge are never stored at all.
#[test]
fn whole_line_edges_follow_excels_store_or_drop_decisions() {
    let mut s = Session::new();
    s.new_workbook();
    // A whole row's left edge: A3 does not exist yet, Excel writes it anyway.
    s.execute("home.borders", json!({"range": "3:3", "preset": "left"})).unwrap();
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        let a3 = sh.cells.get(at("A3")).expect("the row's first cell is materialised, as Excel writes it");
        assert_ne!(doc.wb.styles.get(a3.style).borders.left.style, BorderStyle::None, "and it holds the left edge");
        assert!(sh.rows.get(&2).and_then(|i| i.style).is_none(), "the row's line itself takes nothing");
        assert!(sh.cells.len() < 10, "only that one cell: {} cells", sh.cells.len());
    }
    // A whole row's right edge: Excel stores nothing, even on a present boundary cell.
    s.execute("cell.set", json!({"cell": "XFD3", "input": "end"})).unwrap();
    s.execute("home.borders", json!({"range": "3:3", "preset": "right"})).unwrap();
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        let xfd3 = sh.cells.get(at("XFD3")).expect("the cell we wrote is still there");
        assert_eq!(doc.wb.styles.get(xfd3.style).borders.right.style, BorderStyle::None, "Excel drops a whole row's right edge");
    }
    // A whole column's bottom edge: Excel stores nothing either, F1048576 present or not.
    s.execute("cell.set", json!({"cell": "F1048576", "input": "end"})).unwrap();
    s.execute("home.borders", json!({"range": "F:F", "preset": "bottom"})).unwrap();
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        let last = sh.cells.get(at("F1048576")).expect("the cell we wrote is still there");
        assert_eq!(doc.wb.styles.get(last.style).borders.bottom.style, BorderStyle::None, "Excel drops a whole column's bottom edge");
        assert!(sh.cols.get(&5).and_then(|i| i.style).is_none(), "and the column's line takes nothing either");
    }
    // The mirror case Excel does keep: a whole column's top edge on a present F1.
    s.execute("cell.set", json!({"cell": "F1", "input": "hdr"})).unwrap();
    s.execute("home.borders", json!({"range": "F:F", "preset": "top"})).unwrap();
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        let f1 = sh.cells.get(at("F1")).expect("the cell we wrote is still there");
        assert_ne!(doc.wb.styles.get(f1.style).borders.top.style, BorderStyle::None, "Excel keeps a whole column's top edge on F1");
        assert!(sh.cells.len() < 10, "and materialises nothing else: {} cells", sh.cells.len());
    }
}

/// Excel draws a whole row's left edge on the first cell of *every* selected row -- not only the
/// first row's -- while a preset whose left edge belongs to the rows' lines stores no cell at all.
#[test]
fn a_whole_row_left_border_reaches_every_selected_row() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.borders", json!({"range": "3:10", "preset": "left"})).unwrap();
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        for row in 3..=10u32 {
            let idx = row - 1;
            let cell = sh.cells.get(CellRef::new(idx, 0)).unwrap_or_else(|| panic!("A{row}, the first cell of row {row}, holds its left edge"));
            assert_ne!(doc.wb.styles.get(cell.style).borders.left.style, BorderStyle::None, "A{row} left");
            assert!(sh.rows.get(&idx).and_then(|i| i.style).is_none(), "row {row}'s line takes no left edge");
        }
        assert!(sh.cells.len() <= 8, "only those first cells: {} cells", sh.cells.len());
    }
    // `all` puts the left edge on every row's line instead, and stores no cell for it.
    s.execute("home.borders", json!({"range": "20:25", "preset": "all"})).unwrap();
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        for row in 20..=25u32 {
            let idx = row - 1;
            assert!(sh.cells.get(CellRef::new(idx, 0)).is_none(), "preset all writes no A{row}");
            let st = sh.rows.get(&idx).and_then(|i| i.style).expect("the row's line is styled");
            let b = &doc.wb.styles.get(st).borders;
            assert_ne!(b.left.style, BorderStyle::None, "row {row} left, on its line");
            assert_ne!(b.right.style, BorderStyle::None, "row {row} right");
            assert_ne!(b.top.style, BorderStyle::None, "row {row} top");
            assert_ne!(b.bottom.style, BorderStyle::None, "row {row} bottom");
        }
        assert!(sh.cells.len() <= 8, "and no cell was written: {} cells", sh.cells.len());
    }
}

/// The rows' line entries and their first cells are both bounded, so a whole-row selection too
/// large for the reader's element cap is refused whole, before any border is applied.
#[test]
fn a_whole_row_selection_past_the_reader_cap_is_refused_whole() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A5", "values": [["x"]]})).unwrap();
    let err = s.execute("home.borders", json!({"range": "1:100001", "preset": "left"})).unwrap_err().to_string();
    assert!(err.contains("selection too large"), "{err}");
    let doc = s.doc().unwrap();
    let sh = doc.wb.active().unwrap();
    assert!(sh.rows.get(&2).and_then(|i| i.style).is_none(), "row 3 was styled");
    assert!(sh.cells.get(at("A3")).is_none(), "no first cell was materialised");
    assert_eq!(doc.wb.styles.get(sh.style_id(at("A5"))).borders.left.style, BorderStyle::None, "A5 untouched");
}

/// The ranges of one selection are judged together, before any border is applied: each can sit
/// under the per-range cap while the selection as a whole projects past what the sheet may hold,
/// and a selection under the cap still applies its first cells as usual.
#[test]
fn a_selection_of_whole_row_ranges_is_judged_as_a_whole() {
    let mut s = Session::new();
    s.new_workbook();
    // Ten disjoint ranges, each exactly at the per-range cap: together ten times the cap.
    let many = (0..10).map(|i| format!("{}:{}", i * 100_000 + 1, (i + 1) * 100_000)).collect::<Vec<_>>().join(",");
    let err = s.execute("home.borders", json!({"range": many, "preset": "left"})).unwrap_err().to_string();
    assert!(err.contains("selection too large"), "{err}");
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        assert!(sh.cells.is_empty(), "no first cell was materialised: {} cells", sh.cells.len());
        assert!(sh.rows.get(&0).and_then(|i| i.style).is_none(), "row 1's line was styled");
    }
    // The same shape under the cap goes through, one first cell per selected row.
    s.execute("home.borders", json!({"range": "3:10,20:25", "preset": "left"})).unwrap();
    let doc = s.doc().unwrap();
    let sh = doc.wb.active().unwrap();
    for (lo, hi) in [(3u32, 10u32), (20, 25)] {
        for row in lo..=hi {
            let cell = sh.cells.get(CellRef::new(row - 1, 0)).unwrap_or_else(|| panic!("A{row} holds its left edge"));
            assert_ne!(doc.wb.styles.get(cell.style).borders.left.style, BorderStyle::None, "A{row} left");
        }
    }
    assert!(sh.cells.len() <= 14, "only those first cells: {} cells", sh.cells.len());
}

/// The cells a sheet already holds count toward the same reader cap: a sheet built up to it gains
/// no whole-row first cells, while the same borders on a roomy sheet apply.
#[test]
fn a_whole_row_left_border_respects_the_cells_the_sheet_already_holds() {
    let mut s = Session::new();
    s.new_workbook();
    // Every column but the last one: a range that spans all columns formats rows' lines instead
    // of cells, and would materialise nothing.
    s.execute("home.bold", json!({"range": "A1:XFC62"})).unwrap();
    {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        assert!(sh.cells.len() >= 1_000_000, "the sheet is at the reader's cap: {} cells", sh.cells.len());
    }
    let err = s.execute("home.borders", json!({"range": "63:70", "preset": "left"})).unwrap_err().to_string();
    assert!(err.contains("selection too large"), "{err}");
    let doc = s.doc().unwrap();
    let sh = doc.wb.active().unwrap();
    assert!(sh.cells.get(CellRef::new(62, 0)).is_none(), "A63 was materialised");
    assert!(sh.rows.get(&62).and_then(|i| i.style).is_none(), "row 63's line was styled");
}

/// A border line can be drawn from either side: clearing a whole column clears the facing edge
/// its neighbour's line holds, without a cell per row of the neighbour either.
#[test]
fn clearing_a_whole_column_clears_the_facing_edge_of_its_neighbour() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "B2", "values": [["x"]]})).unwrap();
    s.execute("home.drawBorder", json!({"range": "B:B", "edge": "right"})).unwrap();
    let line_right = |s: &Session, col: u32| {
        let doc = s.doc().unwrap();
        let sh = doc.wb.active().unwrap();
        sh.cols.get(&col).and_then(|i| i.style).map(|st| doc.wb.styles.get(st).borders.right.style != BorderStyle::None)
    };
    assert_eq!(line_right(&s, 1), Some(true), "the border is on B's line");
    assert_eq!(line_right(&s, 2), None, "C's line holds nothing yet");
    s.execute("home.borders", json!({"range": "C:C", "preset": "none"})).unwrap();
    assert_eq!(line_right(&s, 1), Some(false), "clearing C clears the facing edge of B's line");
    let doc = s.doc().unwrap();
    assert!(doc.wb.active().unwrap().cells.len() < 50, "no cell per row of the neighbour");
}
