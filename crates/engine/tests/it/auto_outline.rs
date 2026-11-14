//! Data › Outline › Group › Auto Outline (`data.autoOutline`, part of #3088): the outline built
//! from a sheet's summary formulas, checked against the outline Excel builds for the same sheets
//! (the measurements in #3088).

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

/// A new workbook with `rows` written from A1.
fn sheet(rows: Json) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s
}

/// The issue's "Below" sheet: three detail rows under each of two subtotals (rows 5 and 9), a
/// total in row 10 that adds the subtotals, and a Q1 column E that sums each row's months. The
/// formulas are filled from one cell each, so they reach the sheet through relative references.
fn below() -> Session {
    let mut s = sheet(json!([
        ["Item", "Jan", "Feb", "Mar", "Q1"],
        ["a", 1, 2, 3],
        ["b", 4, 5, 6],
        ["c", 7, 8, 9],
        ["Sub1"],
        ["d", 1, 1, 1],
        ["e", 2, 2, 2],
        ["f", 3, 3, 3],
        ["Sub2"],
        ["Total"]
    ]));
    for (cell, input, fill, range) in [
        ("B5", "=SUM(B2:B4)", "edit.fillRight", "B5:D5"),
        ("B9", "=SUM(B6:B8)", "edit.fillRight", "B9:D9"),
        ("B10", "=B5+B9", "edit.fillRight", "B10:D10"),
        ("E2", "=SUM(B2:D2)", "edit.fillDown", "E2:E10"),
    ] {
        s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
        s.execute(fill, json!({"range": range})).unwrap();
    }
    s
}

/// The issue's "Above" sheet: each subtotal heads the three rows below it, and the total column
/// sits left of the months it sums.
fn above() -> Session {
    sheet(json!([
        ["Item", "Q1", "Jan", "Feb", "Mar"],
        ["Sub1", "=SUM(C2:E2)", "=SUM(C3:C5)", "=SUM(D3:D5)", "=SUM(E3:E5)"],
        ["a", "=SUM(C3:E3)", 1, 2, 3],
        ["b", "=SUM(C4:E4)", 4, 5, 6],
        ["c", "=SUM(C5:E5)", 7, 8, 9],
        ["Sub2", "=SUM(C6:E6)", "=SUM(C7:C9)", "=SUM(D7:D9)", "=SUM(E7:E9)"],
        ["d", "=SUM(C7:E7)", 1, 1, 1],
        ["e", "=SUM(C8:E8)", 2, 2, 2],
        ["f", "=SUM(C9:E9)", 3, 3, 3]
    ]))
}

/// The (0-based line, level) of every grouped row (`rows`) or column of the active sheet.
fn levels(s: &Session, rows: bool) -> Vec<(u32, u8)> {
    let sh = s.doc().unwrap().wb.active().unwrap();
    let map = if rows { &sh.rows } else { &sh.cols };
    map.iter().filter(|(_, l)| l.outline > 0).map(|(&i, l)| (i, l.outline)).collect()
}

/// The (0-based line, hidden, collapsed) of every row of the active sheet that is grouped, hidden
/// or marked collapsed.
fn row_states(s: &Session) -> Vec<(u32, bool, bool)> {
    let sh = s.doc().unwrap().wb.active().unwrap();
    sh.rows.iter().filter(|(_, l)| l.outline > 0 || l.hidden || l.collapsed).map(|(&i, l)| (i, l.hidden, l.collapsed)).collect()
}

/// Rows 2:4 and 6:8 at level 2 inside rows 2:9 at level 1, as Excel builds them for [`below`].
const BELOW_ROWS: [(u32, u8); 8] = [(1, 2), (2, 2), (3, 2), (4, 1), (5, 2), (6, 2), (7, 2), (8, 1)];
/// Columns B:D at level 1.
const MONTH_COLS: [(u32, u8); 3] = [(1, 1), (2, 1), (3, 1)];

fn value(s: &Session, a1: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a1).unwrap())
}

#[test]
fn summary_rows_below_and_a_total_column_build_excels_outline() {
    let mut s = below();
    // Summary rows set to above beforehand: the formulas put them back below.
    s.execute("data.outlineSettings", json!({"summaryBelow": false})).unwrap();
    let r = s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(r, json!({"rows": 2, "cols": 1, "summaryBelow": true, "summaryRight": true}));
    assert_eq!(levels(&s, true), BELOW_ROWS);
    assert_eq!(levels(&s, false), MONTH_COLS);
    assert!(s.doc().unwrap().wb.active().unwrap().summary_below);

    // The outline works like one grouped by hand: level 1 shows the header and the total only.
    s.execute("data.showLevels", json!({"rows": 1, "cols": 1})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!((1..=8).all(|r| sh.is_row_hidden(r)) && !sh.is_row_hidden(0) && !sh.is_row_hidden(9));
    assert!((1..=3).all(|c| sh.is_col_hidden(c)) && !sh.is_col_hidden(4));
    s.execute("data.showLevels", json!({"rows": 3, "cols": 2})).unwrap();

    // One undo step takes the outline and the setting back; redo restores both.
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert!(levels(&s, true).is_empty() && levels(&s, false).is_empty());
    assert!(!s.doc().unwrap().wb.active().unwrap().summary_below);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(levels(&s, true), BELOW_ROWS);
    assert_eq!(levels(&s, false), MONTH_COLS);
    assert!(s.doc().unwrap().wb.active().unwrap().summary_below);
}

#[test]
fn summaries_above_and_left_of_their_detail_switch_the_settings_and_survive_xlsx() {
    let mut s = above();
    let r = s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(r, json!({"rows": 1, "cols": 1, "summaryBelow": false, "summaryRight": false}));
    assert_eq!(levels(&s, true), [(2, 1), (3, 1), (4, 1), (6, 1), (7, 1), (8, 1)]);
    assert_eq!(levels(&s, false), [(2, 1), (3, 1), (4, 1)]);

    // Collapsed to level 1 and saved: the levels, the hidden detail, the marks on the summary
    // lines above and left of it and both directions come back.
    s.execute("data.showLevels", json!({"rows": 1, "cols": 1})).unwrap();
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    s.execute("file.open", json!({"name": "above.xlsx", "base64": saved["base64"]})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.summary_below && !sh.summary_right);
    assert_eq!(levels(&s, true), [(2, 1), (3, 1), (4, 1), (6, 1), (7, 1), (8, 1)]);
    assert_eq!(levels(&s, false), [(2, 1), (3, 1), (4, 1)]);
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!([2, 3, 4, 6, 7, 8].iter().all(|&r| sh.is_row_hidden(r)) && !sh.is_row_hidden(5));
    assert!(sh.rows.get(&1).is_some_and(|l| l.collapsed) && sh.rows.get(&5).is_some_and(|l| l.collapsed));
    assert!(sh.cols.get(&1).is_some_and(|l| l.collapsed));
}

#[test]
fn a_range_outlines_its_own_rows_and_columns_and_whole_lines_one_direction() {
    // A block: only the formulas in it, one level each.
    let mut s = below();
    let r = s.execute("data.autoOutline", json!({"range": "A1:E5"})).unwrap();
    assert_eq!(r["rows"], json!(1));
    assert_eq!(levels(&s, true), [(1, 1), (2, 1), (3, 1)]);
    assert_eq!(levels(&s, false), MONTH_COLS);

    // Whole rows: the same rows as the whole sheet, and no column groups.
    let mut s = below();
    s.execute("selection.set", json!({"range": "1:10"})).unwrap();
    s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(levels(&s, true), BELOW_ROWS);
    assert!(levels(&s, false).is_empty());

    // Whole columns: column groups only.
    let mut s = below();
    s.execute("data.autoOutline", json!({"range": "A:E"})).unwrap();
    assert!(levels(&s, true).is_empty());
    assert_eq!(levels(&s, false), MONTH_COLS);

    // Another sheet, named without a range: all of it.
    let mut s = below();
    s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
    s.execute("data.autoOutline", json!({"sheet": "Sheet1"})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let one = wb.sheet(wb.sheet_index("Sheet1").unwrap()).unwrap();
    assert_eq!(one.rows.iter().filter(|(_, l)| l.outline > 0).map(|(&i, l)| (i, l.outline)).collect::<Vec<_>>(), BELOW_ROWS);
    assert!(wb.active().unwrap().rows.is_empty());
}

#[test]
fn nothing_to_outline_is_an_error_and_no_undo_step() {
    let mut s = sheet(json!([["Item", "Jan"], ["a", 1], ["b", 2], ["Far", "=B2"], [null, 5], ["Both", "=B5+B7"], [null, 6]]));
    s.execute("cell.set", json!({"cell": "D1", "input": "kept"})).unwrap();
    // Values only, a formula that doesn't touch what it reads (B4 reads B2), one that reads both
    // sides of itself (B6), and a block without formulas.
    let e = s.execute("data.autoOutline", json!({})).unwrap_err().to_string();
    assert!(e.contains("no outline to create"), "{e}");
    let mut b = below();
    assert!(b.execute("data.autoOutline", json!({"range": "B2:D4"})).is_err());
    assert!(levels(&b, true).is_empty() && levels(&b, false).is_empty());
    // The failed call left nothing to undo: undo takes back the cell typed before it.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(value(&s, "D1"), Value::Empty);
    // Hostile parameters are errors, not panics.
    for p in [json!({"range": "ZZZ999999999"}), json!({"range": "A1:B2,D1:E2"}), json!({"sheet": 9}), json!({"range": "Nope!A1:B2"})] {
        assert!(b.execute("data.autoOutline", p.clone()).is_err(), "{p}");
    }
}

#[test]
fn the_outline_there_is_replaced_and_its_hidden_detail_shown() {
    let mut s = below();
    s.execute("range.setValues", json!({"range": "A12", "values": [["x"], ["y"], ["z"]]})).unwrap();
    s.execute("cell.set", json!({"cell": "G1", "input": "=SUBTOTAL(103,A12:A14)"})).unwrap();
    // Groups made by hand: rows 3:4 and 12:14, collapsed; row 16 hidden on its own.
    s.execute("data.group", json!({"rows": "3:4"})).unwrap();
    s.execute("data.group", json!({"rows": "12:14"})).unwrap();
    s.execute("data.showLevels", json!({"rows": 1})).unwrap();
    s.execute("home.hideRows", json!({"rows": "16"})).unwrap();
    assert_eq!(value(&s, "G1"), Value::Number(0.0), "SUBTOTAL(103) skips the collapsed rows");

    s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(levels(&s, true), BELOW_ROWS);
    // Rows 12:14 have no group and show again, no collapsed mark is left, and row 16 stays hidden.
    assert_eq!(row_states(&s), BELOW_ROWS.iter().map(|&(i, _)| (i, false, false)).chain([(15, true, false)]).collect::<Vec<_>>());
    assert_eq!(value(&s, "G1"), Value::Number(3.0), "the rows shown again are counted at once");

    // Running it again changes nothing and adds no undo step: one undo goes back to the groups by hand.
    s.execute("data.autoOutline", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(levels(&s, true), [(2, 1), (3, 1), (11, 1), (12, 1), (13, 1)]);
    assert_eq!(value(&s, "G1"), Value::Number(0.0));
}

#[test]
fn the_outline_follows_the_formulas_after_rows_are_inserted() {
    let mut s = below();
    s.execute("data.autoOutline", json!({})).unwrap();
    // A new row inside the first group: SUM(B2:B4) grows to SUM(B2:B5), and the total keeps
    // adding the moved subtotals.
    s.execute("home.insertRows", json!({"rows": "3:3"})).unwrap();
    s.execute("range.setValues", json!({"range": "A3", "values": [["new", 10, 10, 10]]})).unwrap();
    assert_eq!(value(&s, "B6"), Value::Number(22.0));
    assert_eq!(value(&s, "B11"), Value::Number(28.0));
    s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(levels(&s, true), [(1, 2), (2, 2), (3, 2), (4, 2), (5, 1), (6, 2), (7, 2), (8, 2), (9, 1)]);
    assert_eq!(levels(&s, false), MONTH_COLS);
}

#[test]
fn a_protected_sheet_refuses_auto_outline() {
    let mut s = below();
    s.execute("review.protectSheet", json!({})).unwrap();
    let e = s.execute("data.autoOutline", json!({})).unwrap_err().to_string();
    assert!(e.contains("protected"), "{e}");
    assert!(levels(&s, true).is_empty() && levels(&s, false).is_empty());
}

#[test]
fn subtotal_and_absolute_references_are_summaries_too() {
    // SUBTOTAL over the rows above, written with absolute and mixed references, and a total that
    // subtotals the whole block (its SUBTOTALs are skipped by SUBTOTAL, but they still nest).
    let mut s = sheet(json!([
        ["Item", "Jan"],
        ["a", 1],
        ["b", 2],
        ["c", 3],
        ["Sub1", "=SUBTOTAL(9,$B$2:$B$4)"],
        ["d", 4],
        ["e", 5],
        ["f", 6],
        ["Sub2", "=SUBTOTAL(9,B$6:B8)"],
        ["Total", "=SUBTOTAL(9,B2:B9)"]
    ]));
    let r = s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(r, json!({"rows": 2, "cols": 0, "summaryBelow": true, "summaryRight": true}));
    assert_eq!(levels(&s, true), BELOW_ROWS);
    assert!(levels(&s, false).is_empty());
    assert_eq!(value(&s, "B10"), Value::Number(21.0));
}

#[test]
fn only_references_to_the_formulas_own_sheet_count() {
    let mut s = sheet(json!([["Item", "Jan"], ["a", 1], ["b", 2], ["c", 3]]));
    s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
    s.execute("range.setValues", json!({"range": "B2", "values": [[1], [2], [3]]})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"sheet": "Sheet1", "cell": "B5", "input": "=SUM(Data!B2:B4)"})).unwrap();
    // The rows next to B5 on another sheet are not its detail here.
    let e = s.execute("data.autoOutline", json!({"sheet": "Sheet1"})).unwrap_err().to_string();
    assert!(e.contains("no outline to create"), "{e}");
    // The same range on its own sheet, named with the sheet, is.
    s.execute("cell.set", json!({"sheet": "Sheet1", "cell": "B5", "input": "=SUM(Sheet1!B2:B4)"})).unwrap();
    s.execute("data.autoOutline", json!({"sheet": "Sheet1"})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let one = wb.sheet(wb.sheet_index("Sheet1").unwrap()).unwrap();
    assert_eq!(one.rows.iter().filter(|(_, l)| l.outline > 0).map(|(&i, l)| (i, l.outline)).collect::<Vec<_>>(), [(1, 1), (2, 1), (3, 1)]);
    let data = wb.sheet(wb.sheet_index("Data").unwrap()).unwrap();
    assert!(data.rows.values().all(|l| l.outline == 0));
}

#[test]
fn merged_cells_and_rows_hidden_by_hand_keep_their_state() {
    let mut s = below();
    // A label merged down the first group's rows, and a merged cell holding a total.
    s.execute("home.mergeCells", json!({"range": "A2:A4"})).unwrap();
    s.execute("home.mergeCells", json!({"range": "F9:G9"})).unwrap();
    s.execute("cell.set", json!({"cell": "F9", "input": "=SUM(F6:G8)"})).unwrap();
    s.execute("home.hideRows", json!({"rows": "3"})).unwrap();
    s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(levels(&s, true), BELOW_ROWS);
    // Row 3 was hidden by hand, not by the outline, so it stays hidden; nothing is collapsed.
    assert_eq!(row_states(&s), BELOW_ROWS.iter().map(|&(i, _)| (i, i == 2, false)).collect::<Vec<_>>());
    // F9 reads columns F and G: no column of it is a summary, so the columns are B:D as before.
    assert_eq!(levels(&s, false), MONTH_COLS);
}

#[test]
fn nesting_stops_at_the_deepest_level_xlsx_can_hold_and_survives_a_round_trip() {
    // Each row adds one to the row above: ten summaries, each of the one before.
    let mut rows = vec![json!(["Start", 1])];
    for r in 1..=10 {
        rows.push(json!(["Next", format!("=B{r}+1")]));
    }
    let mut s = sheet(Json::Array(rows));
    let r = s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(r["rows"], json!(7), "{r}");
    let got = levels(&s, true);
    assert!(got.iter().all(|&(_, l)| (1..=7).contains(&l)), "{got:?}");
    assert_eq!(got.first(), Some(&(0, 7)));
    assert_eq!(got.last(), Some(&(9, 1)));
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    s.execute("file.open", json!({"name": "deep.xlsx", "base64": saved["base64"]})).unwrap();
    assert_eq!(levels(&s, true), got);
}

#[test]
fn summaries_over_huge_ranges_are_cut_to_the_used_cells() {
    // A heading total over the rest of the column: the detail is the rows that hold something.
    let mut s = sheet(json!([["Total", "=SUM(B2:B1048576)"], ["a", 1], ["b", 2]]));
    let r = s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(r, json!({"rows": 1, "cols": 0, "summaryBelow": false, "summaryRight": true}));
    assert_eq!(levels(&s, true), [(1, 1), (2, 1)]);

    // A total in the sheet's last row over every row above it outlines them all, in one pass.
    let mut s = sheet(json!([["Item", "Jan"], ["a", 1]]));
    s.execute("cell.set", json!({"cell": "B1048576", "input": "=SUM(B1:B1048575)"})).unwrap();
    let r = s.execute("data.autoOutline", json!({})).unwrap();
    assert_eq!(r["rows"], json!(1));
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.rows.values().filter(|l| l.outline == 1).count(), 1_048_575);
    assert!(sh.rows.get(&1_048_575).is_none_or(|l| l.outline == 0));
}
