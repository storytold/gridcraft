//! Formatting a selection while an AutoFilter hides rows changes only the visible cells, as in
//! Excel: rows the filter hid keep their format and show it unchanged once the filter is cleared.
//! Rows hidden by hand are still formatted.

use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Color, Fill, Style};
use serde_json::json;

/// A1:B6 with a filter on Score > 4: rows 3, 5 and 6 (scores 1, 2, 3) are filtered out.
fn filtered() -> Session {
    filtered_after(|_| {})
}

/// [`filtered`], running `before` on the sheet before the filter hides any row.
fn filtered_after(before: impl FnOnce(&mut Session)) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    before(&mut s);
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Score"], ["a", 5], ["b", 1], ["c", 7], ["d", 2], ["e", 3]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1:B6"})).unwrap();
    s.execute("data.filter", json!({})).unwrap();
    let r = s.execute("data.filterBy", json!({"column": "B", "custom": {"op": ">", "value": "4"}})).unwrap();
    assert_eq!(r["hiddenRows"], 3);
    s
}

fn style(s: &Session, a: &str) -> Style {
    let wb = &s.doc().unwrap().wb;
    let sh = wb.active().unwrap();
    wb.styles.get(sh.style_id(CellRef::parse(a).unwrap())).clone()
}

fn yellow() -> Fill {
    Fill::solid(Color::from_hex("#FFFF00").unwrap())
}

fn fill(s: &Session, a: &str) -> Fill {
    style(s, a).fill
}

#[test]
fn fill_color_skips_filtered_rows() {
    let mut s = filtered();
    s.execute("home.fillColor", json!({"range": "A2:B6", "color": "#FFFF00"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    for a in ["A2", "B2", "A4", "B4"] {
        assert_eq!(fill(&s, a), yellow(), "{a} was visible");
    }
    for a in ["A3", "B3", "A5", "B5", "A6", "B6"] {
        assert_eq!(fill(&s, a), Fill::default(), "{a} was filtered out");
    }
}

#[test]
fn fill_color_on_whole_columns_keeps_filtered_rows_unchanged() {
    let mut s = filtered_after(|s| {
        s.execute("home.fillColor", json!({"range": "C3", "color": "#FF0000"})).unwrap();
    });
    s.execute("home.fillColor", json!({"range": "B:C", "color": "#FFFF00"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    // Visible rows, inside and below the filter range, take the column format.
    for a in ["B1", "B2", "C4", "B7", "C100"] {
        assert_eq!(fill(&s, a), yellow(), "{a} was visible");
    }
    // Filtered-out rows keep what they had, whether a stored cell or the column's old default.
    assert_eq!(fill(&s, "B3"), Fill::default());
    assert_eq!(fill(&s, "C3"), Fill::solid(Color::from_hex("#FF0000").unwrap()));
    assert_eq!(fill(&s, "C5"), Fill::default());
    assert_eq!(fill(&s, "B6"), Fill::default());
    // Columns outside the selection are untouched.
    assert_eq!(fill(&s, "A2"), Fill::default());
}

#[test]
fn fill_color_on_whole_rows_skips_filtered_rows() {
    let mut s = filtered();
    s.execute("home.fillColor", json!({"range": "2:6", "color": "#FFFF00"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    for a in ["A2", "Z2", "B4", "XFD4"] {
        assert_eq!(fill(&s, a), yellow(), "{a} was visible");
    }
    for a in ["A3", "Z3", "B5", "XFD6"] {
        assert_eq!(fill(&s, a), Fill::default(), "{a} was filtered out");
    }
}

#[test]
fn other_formats_on_the_same_path_skip_filtered_rows() {
    let mut s = filtered();
    s.execute("home.bold", json!({"range": "A2:A6", "on": true})).unwrap();
    s.execute("home.numberFormat", json!({"range": "B2:B6", "code": "0.00"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    assert!(style(&s, "A2").font.bold);
    assert!(!style(&s, "A3").font.bold);
    assert_eq!(style(&s, "B4").num_fmt.as_str(), "0.00");
    assert_eq!(style(&s, "B5").num_fmt.as_str(), "General");
}

#[test]
fn rows_hidden_by_hand_are_still_formatted() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.hideRows", json!({"rows": "3:4"})).unwrap();
    s.execute("home.fillColor", json!({"range": "A2:A5", "color": "#FFFF00"})).unwrap();
    for a in ["A2", "A3", "A4", "A5"] {
        assert_eq!(fill(&s, a), yellow(), "{a}");
    }
}

#[test]
fn undo_restores_the_whole_column_format() {
    let mut s = filtered();
    s.execute("home.fillColor", json!({"range": "B:B", "color": "#FFFF00"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    for a in ["B2", "B3", "B7"] {
        assert_eq!(fill(&s, a), Fill::default(), "{a}");
    }
}

#[test]
fn whole_columns_keep_the_old_column_formats_on_filtered_rows() {
    let red = || Fill::solid(Color::from_hex("#FF0000").unwrap());
    let green = || Fill::solid(Color::from_hex("#00FF00").unwrap());
    let mut s = filtered_after(|s| {
        s.execute("home.fillColor", json!({"range": "C:C", "color": "#FF0000"})).unwrap();
        s.execute("home.fillColor", json!({"range": "E:E", "color": "#00FF00"})).unwrap();
    });
    s.execute("home.fillColor", json!({"range": "B:D", "color": "#FFFF00"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    for a in ["B2", "C2", "D4", "C7"] {
        assert_eq!(fill(&s, a), yellow(), "{a} was visible");
    }
    // Filtered rows: the formatted column keeps its old format, the unformatted ones none, and
    // the columns outside the selection theirs.
    assert_eq!(fill(&s, "C5"), red());
    assert_eq!(fill(&s, "D5"), Fill::default());
    assert_eq!(fill(&s, "A5"), Fill::default());
    assert_eq!(fill(&s, "E5"), green());
    // A later whole-column format still reaches the rows that were filtered out.
    s.execute("home.fillColor", json!({"range": "F:F", "color": "#FFFF00"})).unwrap();
    assert_eq!(fill(&s, "F5"), yellow());
}

#[test]
fn the_whole_sheet_skips_filtered_rows() {
    let mut s = filtered();
    s.execute("home.fillColor", json!({"range": "A1:XFD1048576", "color": "#FFFF00"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    for a in ["A1", "B2", "XFD4", "C1000"] {
        assert_eq!(fill(&s, a), yellow(), "{a} was visible");
    }
    for a in ["A3", "B5", "XFD6"] {
        assert_eq!(fill(&s, a), Fill::default(), "{a} was filtered out");
    }
}

/// A row format outranks a column format, so a whole-column format reaches a formatted row's
/// cells too: filling column B after making row 1 bold gives a bold, filled B1.
#[test]
fn whole_columns_reach_rows_with_a_row_format() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.bold", json!({"range": "1:1", "on": true})).unwrap();
    s.execute("home.fillColor", json!({"range": "B:B", "color": "#FFFF00"})).unwrap();
    assert_eq!(fill(&s, "B1"), yellow());
    assert!(style(&s, "B1").font.bold);
    assert_eq!(fill(&s, "A1"), Fill::default());
    assert!(style(&s, "A1").font.bold);
    s.execute("home.italic", json!({"range": "A1:XFD1048576", "on": true})).unwrap();
    assert!(style(&s, "Z1").font.bold && style(&s, "Z1").font.italic);
}

#[test]
fn filtered_rows_keep_their_format_through_xlsx() {
    let mut s = filtered_after(|s| {
        s.execute("home.fillColor", json!({"range": "C:C", "color": "#FF0000"})).unwrap();
    });
    s.execute("home.fillColor", json!({"range": "B:D", "color": "#FFFF00"})).unwrap();
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sh = wb.active().unwrap();
    let fill = |a: &str| wb.styles.get(sh.style_id(CellRef::parse(a).unwrap())).fill;
    assert_eq!(fill("B2"), yellow());
    assert_eq!(fill("D100"), yellow());
    assert_eq!(fill("C5"), Fill::solid(Color::from_hex("#FF0000").unwrap()));
    assert_eq!(fill("D5"), Fill::default());
}

/// After a whole-sheet format every column has a format of its own. A later whole-column format
/// with a long filter keeps the filtered rows' cells in the selected columns only, so it neither
/// runs into the size limit nor stores cells in the columns it didn't touch.
#[test]
fn a_whole_column_format_after_a_whole_sheet_format_stays_in_its_columns() {
    let white = || Fill::solid(Color::from_hex("#FFFFFF").unwrap());
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.fillColor", json!({"range": "A1:XFD1048576", "color": "#FFFFFF"})).unwrap();
    let values: Vec<_> = std::iter::once(json!(["Score"])).chain((1..=600).map(|n| json!([n]))).collect();
    s.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
    s.execute("selection.set", json!({"range": "A1:A601"})).unwrap();
    s.execute("data.filter", json!({})).unwrap();
    let r = s.execute("data.filterBy", json!({"column": "A", "custom": {"op": ">", "value": "300"}})).unwrap();
    assert_eq!(r["hiddenRows"], 300);
    s.execute("home.fillColor", json!({"range": "B:B", "color": "#FFFF00"})).unwrap();
    // A whole-sheet format again: the filtered rows keep the old one without a cell per column.
    s.execute("home.bold", json!({"range": "A1:XFD1048576", "on": true})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    assert_eq!(fill(&s, "B2"), white());
    assert!(!style(&s, "B2").font.bold);
    assert_eq!(fill(&s, "B302"), yellow());
    assert!(style(&s, "Z302").font.bold);
    assert_eq!(fill(&s, "Z2"), white());
    assert!(!style(&s, "Z2").font.bold);
    let doc = s.doc().unwrap();
    let sh = doc.wb.active().unwrap();
    assert!(sh.cells.get(CellRef::parse("C2").unwrap()).is_none(), "no cell stored outside the selected column");
    assert!(sh.cells.get(CellRef::parse("Z2").unwrap()).is_none(), "no cell per column for the whole sheet");
}

#[test]
fn borders_skip_filtered_rows() {
    let mut s = filtered_after(|s| {
        s.execute("home.borders", json!({"range": "D3:E5", "preset": "all"})).unwrap();
    });
    s.execute("home.borders", json!({"range": "A2:B6", "preset": "all"})).unwrap();
    // Removing D4:E4's borders leaves the edges stored on the filtered rows above and below.
    s.execute("home.borders", json!({"range": "D4:E4", "preset": "none"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    let thin = |a: &str| style(&s, a).borders;
    for a in ["A2", "B4"] {
        assert_ne!(thin(a), Default::default(), "{a} was visible");
    }
    for a in ["A3", "B5", "A6"] {
        assert_eq!(thin(a), Default::default(), "{a} was filtered out");
    }
    assert_eq!(thin("D4"), Default::default());
    assert_ne!(thin("D3").bottom, Default::default());
    assert_ne!(thin("D5").top, Default::default());
}

/// A blank cell can't hold the default style, so a filtered row whose columns had the default
/// keeps it only under a default row format, even when nearly every column had another one.
#[test]
fn a_column_that_had_the_default_keeps_it_on_filtered_rows() {
    let white = || Fill::solid(Color::from_hex("#FFFFFF").unwrap());
    let mut s = filtered_after(|s| {
        s.execute("home.fillColor", json!({"range": "A1:XFD1048576", "color": "#FFFFFF"})).unwrap();
        s.execute("home.fillColor", json!({"range": "C:C", "color": "auto"})).unwrap();
    });
    s.execute("home.bold", json!({"range": "A1:XFD1048576", "on": true})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    assert_eq!(fill(&s, "C5"), Fill::default());
    assert_eq!(fill(&s, "A5"), white());
    assert!(!style(&s, "C5").font.bold);
    assert!(style(&s, "C4").font.bold);
}

#[test]
fn the_border_outline_follows_the_visible_rows() {
    let mut s = filtered();
    // Row 6, the selection's last, is filtered out: the bottom edge goes on row 4.
    s.execute("home.borders", json!({"range": "A2:A6", "preset": "outside"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    let b = |a: &str| style(&s, a).borders;
    assert_ne!(b("A2").top, Default::default());
    assert_ne!(b("A4").bottom, Default::default());
    assert_eq!(b("A2").bottom, Default::default());
    assert_eq!(b("A6"), Default::default());
}

#[test]
fn format_painter_skips_filtered_rows() {
    let mut s = filtered();
    s.execute("home.fillColor", json!({"range": "D1", "color": "#FFFF00"})).unwrap();
    s.execute("selection.set", json!({"range": "D1"})).unwrap();
    s.execute("edit.formatPainter", json!({"sticky": true})).unwrap();
    s.execute("edit.formatPainter", json!({"apply": "A2:A6"})).unwrap();
    s.execute("edit.formatPainter", json!({"apply": "2:6"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    for a in ["A2", "A4", "Z2", "Z4"] {
        assert_eq!(fill(&s, a), yellow(), "{a} was visible");
    }
    for a in ["A3", "A5", "A6", "Z3", "Z6"] {
        assert_eq!(fill(&s, a), Fill::default(), "{a} was filtered out");
    }
}

#[test]
fn redo_skips_filtered_rows_again() {
    let mut s = filtered();
    s.execute("home.fillColor", json!({"range": "A2:B6", "color": "#FFFF00"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(fill(&s, "A2"), Fill::default());
    s.execute("edit.redo", json!({})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    assert_eq!(fill(&s, "A2"), yellow());
    assert_eq!(fill(&s, "A3"), Fill::default());
}

#[test]
fn each_area_of_a_multi_area_selection_skips_filtered_rows() {
    let mut s = filtered();
    s.execute("home.fillColor", json!({"range": "A2:A3,A4:B6", "color": "#FFFF00"})).unwrap();
    s.execute("data.clearFilter", json!({})).unwrap();
    for a in ["A2", "A4", "B4"] {
        assert_eq!(fill(&s, a), yellow(), "{a} was visible");
    }
    for a in ["A3", "A5", "B5", "B6"] {
        assert_eq!(fill(&s, a), Fill::default(), "{a} was filtered out");
    }
}
