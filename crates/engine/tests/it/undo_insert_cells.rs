use gridcraft_engine::Session;
use serde_json::json;

fn c4(s: &Session) -> String {
    format!("{:?}", s.doc().unwrap().wb.active().unwrap().cell(gridcraft_core::CellRef::parse("C4").unwrap()))
}

/// Fuzz seed 7, shrunk. Borders on a whole column used to write a cell per row, so one undo step
/// outgrew the history's byte budget and the steps before it were dropped.
#[test]
fn whole_column_borders_keep_every_undo_step() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "budget"})).unwrap();
    let start = c4(&s);
    s.execute("home.borders", json!({"preset": "outside", "range": "F:F"})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().cells.len() < 10_000);
    s.execute("home.insertCells", json!({"range": "C15:G22", "shift": "column"})).unwrap();
    s.execute("home.insertCells", json!({"range": "H3:I3", "shift": "column"})).unwrap();
    assert_eq!(s.doc().unwrap().undo.len(), 3);
    for _ in 0..3 {
        s.execute("edit.undo", json!({})).unwrap();
    }
    assert_eq!(c4(&s), start);
}

#[test]
fn whole_column_outside_borders_apply_to_the_column_and_its_ends() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.execute("home.borders", json!({"preset": "outside", "range": "B:C"})).unwrap();
    let sh = s.doc().unwrap().wb.clone();
    let sh = sh.active().unwrap();
    let styles = &s.doc().unwrap().wb.styles;
    let border = |id| styles.get(id).borders;
    let col = |c: u32| border(sh.cols.get(&c).and_then(|i| i.style).unwrap());
    assert!(col(1).left.style != Default::default() && col(1).right.style == Default::default() && col(1).top.style == Default::default());
    assert!(col(2).right.style != Default::default() && col(2).left.style == Default::default());
    let top = border(sh.style_id(gridcraft_core::CellRef::parse("B1").unwrap()));
    assert!(top.top.style != Default::default() && top.left.style != Default::default());
    let bottom = border(sh.style_id(gridcraft_core::CellRef::new(1_048_575, 2)));
    assert!(bottom.bottom.style != Default::default() && bottom.right.style != Default::default());
}

fn border_of(s: &Session, a1: &str) -> gridcraft_model::Borders {
    let wb = &s.doc().unwrap().wb;
    wb.styles.get(wb.active().unwrap().style_id(gridcraft_core::CellRef::parse(a1).unwrap())).borders
}

#[test]
fn whole_column_none_clears_the_facing_edge_of_a_neighbour_the_line_never_visits() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.execute("home.borders", json!({"preset": "right", "range": "A500"})).unwrap();
    assert!(border_of(&s, "A500").right.style != Default::default());
    s.execute("home.borders", json!({"preset": "none", "range": "B:B"})).unwrap();
    assert!(border_of(&s, "A500").right.style == Default::default());
}

#[test]
fn whole_column_borders_reach_blank_cells_of_styled_rows_and_leave_inner_ends_alone() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.execute("home.borders", json!({"preset": "bottom", "range": "5:5"})).unwrap();
    s.execute("home.borders", json!({"preset": "outside", "range": "B:B"})).unwrap();
    let b5 = border_of(&s, "B5");
    assert!(b5.left.style != Default::default() && b5.right.style != Default::default() && b5.bottom.style != Default::default());
    // A blank end cell takes only its own edges, not the column's outer ones twice over.
    let b2 = border_of(&s, "B2");
    assert!(b2.top.style == Default::default() && b2.left.style != Default::default());
}

#[test]
fn whole_row_none_clears_the_facing_edge_of_a_blank_cell_that_inherits_it_from_a_column() {
    let mut s = Session::new();
    s.execute("file.new", json!({})).unwrap();
    s.execute("home.borders", json!({"preset": "all", "range": "B:B"})).unwrap();
    assert!(border_of(&s, "B4").bottom.style != Default::default());
    s.execute("home.borders", json!({"preset": "none", "range": "5:5"})).unwrap();
    assert!(border_of(&s, "B4").bottom.style == Default::default());
}
