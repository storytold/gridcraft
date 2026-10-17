use gridcraft_core::{CellRef, RangeRef};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

const SORTS: [(&str, bool); 4] = [("data.sortAscending", false), ("data.sortDescending", true), ("data.sort", false), ("data.sort", true)];

fn fixture(bold_header: bool) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [
            ["Summary", "Total", 6],
            ["Item", "Rank", "Detail"],
            ["third", 3, "three"],
            ["first", 1, "one"],
            ["sixth", 6, "six"],
            ["second", 2, "two"],
            ["fifth", 5, "five"],
            ["fourth", 4, "four"]
        ]}),
    )
    .unwrap();
    if bold_header {
        s.execute("home.bold", json!({"range": "A2:C2"})).unwrap();
    }
    s.execute("data.filter", json!({"range": "A2:C8"})).unwrap();
    s
}

fn values(s: &Session, range: &str) -> Vec<Vec<String>> {
    let r = RangeRef::parse(range).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    (r.start.row..=r.end.row).map(|row| (r.start.col..=r.end.col).map(|col| sh.value(CellRef::new(row, col)).display()).collect()).collect()
}

fn reordered(before: &[Vec<String>], order: &[usize]) -> Vec<Vec<String>> {
    order.iter().map(|&i| before[i].clone()).collect()
}

fn params(command: &str, descending: bool, column: &str) -> Json {
    if command == "data.sort" {
        json!({"keys": [{"column": column, "order": if descending { "desc" } else { "asc" }}]})
    } else {
        json!({"column": column})
    }
}

#[test]
fn autofilter_sort_preserves_summary_and_plain_or_bold_header_and_is_undoable() {
    for bold in [false, true] {
        for selected in ["A1", "B2"] {
            for (command, descending) in SORTS {
                let mut s = fixture(bold);
                s.execute("selection.set", json!({"cell": selected})).unwrap();
                s.active_mut().unwrap().undo.clear();
                let before = values(&s, "A1:C8");
                let filter = s.doc().unwrap().wb.active().unwrap().autofilter.clone();
                let result = s.execute(command, params(command, descending, "B")).unwrap();
                let order = if descending { [0, 1, 4, 6, 7, 2, 5, 3] } else { [0, 1, 3, 5, 2, 7, 6, 4] };
                assert_eq!(result["sorted"], "A3:C8", "{command}, descending={descending}, bold={bold}, selected={selected}");
                assert_eq!(values(&s, "A1:C8"), reordered(&before, &order));
                assert_eq!(s.doc().unwrap().wb.active().unwrap().autofilter, filter);
                assert_eq!(s.doc().unwrap().undo.len(), 1);
                s.execute("edit.undo", json!({})).unwrap();
                assert_eq!(values(&s, "A1:C8"), before);
                assert_eq!(s.doc().unwrap().wb.active().unwrap().autofilter, filter);
            }
        }
    }
}

#[test]
fn plain_all_text_autofilter_header_is_preserved_without_type_or_format_hints() {
    for (command, descending) in SORTS {
        let mut s = fixture(false);
        s.execute("range.setValues", json!({"range": "B3", "values": [["c"], ["a"], ["f"], ["b"], ["e"], ["d"]]})).unwrap();
        s.execute("selection.set", json!({"cell": "B2"})).unwrap();
        let before = values(&s, "A1:C8");
        let result = s.execute(command, params(command, descending, "B")).unwrap();
        let order = if descending { [0, 1, 4, 6, 7, 2, 5, 3] } else { [0, 1, 3, 5, 2, 7, 6, 4] };
        assert_eq!(result["sorted"], "A3:C8");
        assert_eq!(values(&s, "A1:C8"), reordered(&before, &order));
    }
}

#[test]
fn explicit_body_range_and_deliberate_block_selection_are_not_expanded_to_autofilter() {
    for explicit_range in [false, true] {
        for (command, descending) in SORTS {
            let mut s = fixture(true);
            let before = values(&s, "A1:C8");
            let mut p = params(command, descending, "B");
            if explicit_range {
                s.execute("selection.set", json!({"cell": "B2"})).unwrap();
                p["range"] = json!("A3:C8");
            } else {
                s.execute("selection.set", json!({"range": "A4:C8"})).unwrap();
            }
            let result = s.execute(command, p).unwrap();
            let order = match (explicit_range, descending) {
                (true, false) => [0, 1, 3, 5, 2, 7, 6, 4],
                (true, true) => [0, 1, 4, 6, 7, 2, 5, 3],
                (false, false) => [0, 1, 2, 3, 5, 7, 6, 4],
                (false, true) => [0, 1, 2, 4, 6, 7, 5, 3],
            };
            assert_eq!(result["sorted"], if explicit_range { "A3:C8" } else { "A4:C8" });
            assert_eq!(values(&s, "A1:C8"), reordered(&before, &order));
        }
    }
}

#[test]
fn quick_sort_honors_explicit_header_false_even_for_a_bold_autofilter_header() {
    for (command, descending) in &SORTS[..2] {
        let mut s = fixture(true);
        let before = values(&s, "A1:C8");
        let result = s.execute(command, json!({"range": "A2:C8", "column": "B", "header": false})).unwrap();
        let order = if *descending { [0, 1, 4, 6, 7, 2, 5, 3] } else { [0, 3, 5, 2, 7, 6, 4, 1] };
        assert_eq!(result["sorted"], "A2:C8");
        assert_eq!(values(&s, "A1:C8"), reordered(&before, &order));
    }
}

#[test]
fn sorting_an_unrelated_region_does_not_capture_the_sheets_autofilter() {
    for (command, descending) in SORTS {
        let mut s = fixture(true);
        let filtered_before = values(&s, "A1:C8");
        s.execute(
            "range.setValues",
            json!({"range": "E1", "values": [["Other", "Rank", "Detail"], ["third", 3, "three"], ["first", 1, "one"], ["second", 2, "two"]]}),
        )
        .unwrap();
        s.execute("selection.set", json!({"cell": "F1"})).unwrap();
        let before = values(&s, "E1:G4");
        let result = s.execute(command, params(command, descending, "F")).unwrap();
        let order = if descending { [0, 1, 3, 2] } else { [0, 2, 3, 1] };
        assert_eq!(result["sorted"], "E2:G4");
        assert_eq!(values(&s, "E1:G4"), reordered(&before, &order));
        assert_eq!(values(&s, "A1:C8"), filtered_before);
    }
}
