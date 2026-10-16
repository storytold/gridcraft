use gridcraft_engine::Session;
use serde_json::json;

fn session(setup: serde_json::Value) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("pageLayout.pageSetup", setup).unwrap();
    s
}

fn print(s: &Session) -> gridcraft_model::PrintSettings {
    s.doc().unwrap().wb.active().unwrap().print.clone()
}

#[test]
fn inserting_lines_moves_print_titles() {
    let mut s = session(json!({"titleRows": "1:1", "titleCols": "A:A"}));
    s.execute("home.insertRows", json!({"rows": "1:1"})).unwrap();
    s.execute("home.insertColumns", json!({"cols": "A:A"})).unwrap();
    assert_eq!(print(&s).title_rows, Some((1, 1)));
    assert_eq!(print(&s).title_cols, Some((1, 1)));
}

#[test]
fn inserting_lines_moves_page_breaks() {
    let mut s = session(json!({"rowBreaks": [10], "colBreaks": ["D"]}));
    s.execute("home.insertRows", json!({"rows": "1:2"})).unwrap();
    s.execute("home.insertColumns", json!({"cols": "A:A"})).unwrap();
    assert_eq!(print(&s).row_breaks, vec![11]);
    assert_eq!(print(&s).col_breaks, vec![4]);
}

#[test]
fn deleting_lines_moves_or_drops_print_titles() {
    let mut s = session(json!({"titleRows": "3:4", "titleCols": "C:C"}));
    s.execute("home.deleteRows", json!({"rows": "1:1"})).unwrap();
    s.execute("home.deleteColumns", json!({"cols": "A:A"})).unwrap();
    assert_eq!(print(&s).title_rows, Some((1, 2)));
    assert_eq!(print(&s).title_cols, Some((1, 1)));
    s.execute("home.deleteRows", json!({"rows": "2:3"})).unwrap();
    s.execute("home.deleteColumns", json!({"cols": "B:B"})).unwrap();
    assert_eq!(print(&s).title_rows, None);
    assert_eq!(print(&s).title_cols, None);
}

#[test]
fn titles_pushed_off_the_sheet_are_dropped_or_cut() {
    let mut s = session(json!({"titleRows": "1048576:1048576", "titleCols": "XFC:XFD"}));
    s.execute("home.insertRows", json!({"rows": "1:1"})).unwrap();
    s.execute("home.insertColumns", json!({"cols": "A:A"})).unwrap();
    assert_eq!(print(&s).title_rows, None);
    assert_eq!(print(&s).title_cols, Some((16383, 16383)));
}

#[test]
fn deleting_lines_moves_page_breaks() {
    let mut s = session(json!({"rowBreaks": [10, 20], "colBreaks": ["D", "F"]}));
    s.execute("home.deleteRows", json!({"rows": "1:2"})).unwrap();
    s.execute("home.deleteColumns", json!({"cols": "A:A"})).unwrap();
    assert_eq!(print(&s).row_breaks, vec![7, 17]);
    assert_eq!(print(&s).col_breaks, vec![2, 4]);
}
