use super::*;

/// A1:A8 with a header, dates over two years, a text and a blank, filtered.
fn dated_session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let rows = [["When"], ["2025-12-24"], ["2026-03-05"], ["2026-03-20"], ["2026-04-01"], ["n/a"], [""], ["2026-03-05"]];
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A8"})).unwrap();
    s
}

fn tree(nodes: &[DateNode]) -> Vec<String> {
    let mut out = vec![];
    for n in nodes {
        let kids = tree(&n.children);
        out.push(if kids.is_empty() {
            format!("{}{}", n.label, if n.checked { "" } else { "-" })
        } else {
            format!("{}[{}]", n.label, kids.join(" "))
        });
    }
    out
}

#[test]
fn filter_list_groups_dates_by_year_month_and_day() {
    let s = dated_session();
    let list = filter_list(&s.doc().unwrap().wb, 0, 0);
    assert_eq!(tree(&list.dates), ["2025[December[24]]", "2026[March[05 20] April[01]]"]);
    assert_eq!(list.values, [("n/a".to_string(), true), ("(Blanks)".to_string(), true)]);
}

#[test]
fn filter_by_dates_shows_the_checked_months_and_days() {
    let mut s = dated_session();
    s.execute("data.filterBy", json!({"column": "A", "dates": [{"year": 2026, "month": 3}, {"year": 2025, "month": 12, "day": 24}]})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let sh = wb.active().unwrap();
    let hidden: Vec<u32> = (1..8).filter(|r| sh.is_row_hidden(*r)).collect();
    assert_eq!(hidden, [4, 5, 6], "April, the text and the blank are hidden");
    let list = filter_list(wb, 0, 0);
    assert_eq!(tree(&list.dates), ["2025[December[24]]", "2026[March[05 20] April[01-]]"]);
    assert_eq!(list.values, [("n/a".to_string(), false), ("(Blanks)".to_string(), false)]);
    // A text value next to the dates: both stay in the filter, as Excel keeps them, and a row
    // shows when it matches either.
    s.execute("data.filterBy", json!({"column": "A", "values": ["n/a"], "dates": [{"year": 2025}]})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    let hidden: Vec<u32> = (1..8).filter(|r| sh.is_row_hidden(*r)).collect();
    assert_eq!(hidden, [2, 3, 4, 6, 7]);
    let crit = sh.autofilter.as_ref().and_then(|a| a.criteria.first()).map(|(_, c)| c.clone());
    let y2025 = DateGroupItem { grouping: DateGrouping::Year, year: 2025, month: 0, day: 0, hour: 0, minute: 0, second: 0 };
    assert_eq!(crit, Some(FilterCriterion::Values { values: vec!["n/a".into()], blanks: false, dates: vec![y2025] }));
}

#[test]
fn checked_date_items_use_the_widest_fully_checked_groups() {
    let s = dated_session();
    let mut list = filter_list(&s.doc().unwrap().wb, 0, 0);
    // Uncheck 20 March 2026.
    if let Some(day) = list.dates.get_mut(1).and_then(|y| y.children.get_mut(0)).and_then(|m| m.children.get_mut(1)) {
        day.checked = false;
    }
    list.refresh("");
    let items = checked_date_items(&list.dates);
    let g = |grouping, year, month, day| DateGroupItem { grouping, year, month, day, hour: 0, minute: 0, second: 0 };
    assert_eq!(items, [g(DateGrouping::Year, 2025, 0, 0), g(DateGrouping::Day, 2026, 3, 5), g(DateGrouping::Month, 2026, 4, 0)]);
}

#[test]
fn a_search_limits_the_boxes_and_the_applied_dates_to_what_it_shows() {
    let s = dated_session();
    let mut list = filter_list(&s.doc().unwrap().wb, 0, 0);
    list.refresh("march");
    let counts = |l: &FilterList| l.dates.iter().map(|n| (n.shown, n.shown_on, n.leaves)).collect::<Vec<_>>();
    assert_eq!(counts(&list), [(0, 0, 1), (2, 2, 3)], "2025 is hidden, 2026 shows its two March days");
    // Unchecking 2026 during the search leaves April, which the search hides, checked.
    list.dates[1].set_shown("march", false);
    list.refresh("march");
    assert_eq!(counts(&list), [(0, 0, 1), (2, 0, 3)]);
    list.refresh("");
    assert_eq!(counts(&list), [(1, 1, 1), (3, 1, 3)]);
    // Apply under the search covers the shown, checked dates only: March, not the whole year or 2025.
    list.dates[1].set_shown("march", true);
    list.refresh("march");
    let march = DateGroupItem { grouping: DateGrouping::Month, year: 2026, month: 3, day: 0, hour: 0, minute: 0, second: 0 };
    assert_eq!(checked_date_items(&list.dates), [march]);
}

#[test]
fn dates_follow_the_second_shown_and_durations_stay_values() {
    let mut s = Session::new();
    s.new_workbook();
    // A hair before midnight shows the next day; `[h]:mm` durations are not dates even past 24 hours.
    s.execute("range.setValues", json!({"range": "A1", "values": [["When", "Took"], [46086.99999999, 0.5], [46086, 1.25]]})).unwrap();
    s.execute("home.numberFormat", json!({"range": "A2:A3", "code": "yyyy-mm-dd"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "B2:B3", "code": "[h]:mm"})).unwrap();
    s.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
    let wb = &s.doc().unwrap().wb;
    assert_eq!(tree(&filter_list(wb, 0, 0).dates), ["2026[March[05 06]]"]);
    let took = filter_list(wb, 0, 1);
    assert!(took.dates.is_empty());
    assert_eq!(took.values.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["12:00", "30:00"]);
}

#[test]
fn a_selection_of_values_and_dates_keeps_both_and_shows_either() {
    let mut s = dated_session();
    s.execute("home.numberFormat", json!({"range": "A2:A8", "code": "d mmm yyyy"})).unwrap();
    s.execute("data.filterBy", json!({"column": "A", "values": ["n/a"], "dates": [{"year": 2026, "month": 3}]})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let sh = wb.active().unwrap();
    let crit = sh.autofilter.as_ref().and_then(|a| a.criteria.first()).map(|(_, c)| c.clone());
    // Excel keeps a text ticked together with a month in one filter and shows the rows of either
    // (observed in Excel 365): so does GridCraft, and the dropdown shows both ticked.
    let march = DateGroupItem { grouping: DateGrouping::Month, year: 2026, month: 3, day: 0, hour: 0, minute: 0, second: 0 };
    assert_eq!(crit, Some(FilterCriterion::Values { values: vec!["n/a".into()], blanks: false, dates: vec![march] }));
    let hidden: Vec<u32> = (1..8).filter(|r| sh.is_row_hidden(*r)).collect();
    assert_eq!(hidden, [1, 4, 6], "December, April and the blank are hidden");
    let mut list = filter_list(wb, 0, 0);
    list.refresh("");
    assert!(list.values.iter().any(|(t, on)| t == "n/a" && *on), "{:?}", list.values);
    assert_eq!(checked_date_items(&list.dates), [march]);
    // Through XLSX the filter keeps both kinds.
    let bytes = crate::io::save_bytes(wb, "mixed.xlsx").unwrap();
    let (opened, warnings) = crate::io::open_bytes("mixed.xlsx", &bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let crit = opened.sheet(0).and_then(|sh| sh.autofilter.as_ref()).and_then(|a| a.criteria.first()).map(|(_, c)| c.clone());
    assert_eq!(crit, Some(FilterCriterion::Values { values: vec!["n/a".into()], blanks: false, dates: vec![march] }));
}

#[test]
fn a_date_cell_is_not_matched_by_its_serial_number() {
    let mut s = Session::new();
    s.new_workbook();
    // A2 is the plain number 46086, A3 the date with that serial (2026-03-05).
    s.execute("range.setValues", json!({"range": "A1", "values": [["When"], [46086], ["2026-03-05"]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A3"})).unwrap();
    s.execute("data.filterBy", json!({"column": "A", "values": ["46086"], "dates": [{"year": 2025}]})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.is_row_hidden(1), "the number passes by its value");
    assert!(sh.is_row_hidden(2), "the date is not in 2025, whatever its serial");
}

#[test]
fn filter_by_ignores_null_values_and_rejects_a_values_that_is_not_a_list() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["n"], [1], [5], [9]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
    let r = s.execute("data.filterBy", json!({"column": "A", "values": null, "dates": null, "custom": {"op": ">", "value": "4"}})).unwrap();
    assert_eq!(r["hiddenRows"], json!(1), "the custom filter applies");
    let err = s.execute("data.filterBy", json!({"column": "A", "values": "5"})).unwrap_err();
    assert!(err.to_string().contains("`values` must be a list"), "{err}");
}

#[test]
fn blanks_alone_keeps_the_empty_cells() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["n"], [1], [null], [3]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
    let r = s.execute("data.filterBy", json!({"column": "A", "blanks": true})).unwrap();
    assert_eq!(r["hiddenRows"], json!(2));
    // With another kind of filter, `blanks` does not take over.
    let r = s.execute("data.filterBy", json!({"column": "A", "blanks": true, "custom": {"op": ">", "value": "2"}})).unwrap();
    assert_eq!(r["hiddenRows"], json!(2), "the custom filter keeps 3 only");
}

#[test]
fn times_add_hour_minute_and_second_levels() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["At"], ["2026-03-05 08:30"], ["2026-03-05 14:00:15"]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A3"})).unwrap();
    let list = filter_list(&s.doc().unwrap().wb, 0, 0);
    assert_eq!(tree(&list.dates), ["2026[March[05[08[:30[:00]] 14[:00[:15]]]]]"]);
    s.execute("data.filterBy", json!({"column": "A", "dates": [{"year": 2026, "month": 3, "day": 5, "hour": 14}]})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(sh.is_row_hidden(1));
    assert!(!sh.is_row_hidden(2));
}

#[test]
fn date_groups_ignore_fields_below_their_grouping() {
    // A year item that still carries a month (from a session file or an agent) covers the whole year.
    let item = DateGroupItem { grouping: DateGrouping::Year, year: 2026, month: 3, day: 9, hour: 0, minute: 0, second: 0 };
    let april = gridcraft_core::date::DateTime { year: 2026, month: 4, day: 1, ..Default::default() };
    assert!(item.contains(&april));
    assert!(DateSet::new(&[item]).contains(&april));
    assert!(!DateSet::new(&[item]).contains(&gridcraft_core::date::DateTime { year: 2025, month: 4, day: 1, ..Default::default() }));
}

#[test]
fn dates_on_the_first_day_of_the_1904_system_are_grouped() {
    let mut s = Session::new();
    s.new_workbook();
    Arc::make_mut(&mut s.doc_mut().unwrap().wb).date_system = gridcraft_core::DateSystem::D1904;
    s.execute("range.setValues", json!({"range": "A1", "values": [["When"], [0.25], [0.5], [1.0]]})).unwrap();
    s.execute("home.numberFormat", json!({"range": "A2:A4", "code": "yyyy-mm-dd hh:mm"})).unwrap();
    s.execute("range.setValues", json!({"range": "B1", "values": [["Time"], [0.25]]})).unwrap();
    s.execute("home.numberFormat", json!({"range": "B2", "code": "h:mm"})).unwrap();
    s.execute("data.filter", json!({"range": "A1:B4"})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let list = filter_list(wb, 0, 0);
    assert_eq!(tree(&list.dates), ["1904[January[01[06[:00[:00]] 12[:00[:00]]] 02[00[:00[:00]]]]]"]);
    assert!(list.values.is_empty(), "{:?}", list.values);
    // A time-only format below 1 is a time, not a date.
    let times = filter_list(wb, 0, 1);
    assert!(times.dates.is_empty());
    assert_eq!(times.values.first().map(|(t, _)| t.as_str()), Some("6:00"));
}

#[test]
fn every_text_of_a_date_stays_searchable() {
    // One day shown in nine formats: the dropdown's search must find the ninth text too.
    let codes = ["yyyy-mm-dd", "d/m/yyyy", "m/d/yyyy", "dd.mm.yyyy", "d mmm yyyy", "mmm d, yyyy", "dddd d", "yyyy/mm/dd", "d mmmm yyyy"];
    let mut s = Session::new();
    s.new_workbook();
    let mut rows = vec![json!(["When"])];
    rows.extend(codes.iter().map(|_| json!(["2026-03-05"])));
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    for (i, code) in codes.iter().enumerate() {
        s.execute("home.numberFormat", json!({"range": format!("A{}", i + 2), "code": code})).unwrap();
    }
    s.execute("data.filter", json!({"range": format!("A1:A{}", codes.len() + 1)})).unwrap();
    let list = filter_list(&s.doc().unwrap().wb, 0, 0);
    let day = &list.dates[0].children[0].children[0];
    // The labels from the year down, then one line per distinct text.
    assert_eq!(day.search.lines().count(), codes.len() + 1, "{:?}", day.search);
    assert!(day.matches("5 march 2026"), "{:?}", day.search);
}

#[test]
fn filter_by_rejects_malformed_dates() {
    let mut s = dated_session();
    for bad in [
        json!([{"month": 3}]),
        json!([{"year": 2026, "day": 5}]),
        json!([{"year": 2026, "month": 13}]),
        json!([{"year": 2026, "months": 3}]),
        json!([{"year": 2026, "month": 0}]),
        json!([{"year": 2026, "month": 3, "day": 0}]),
        json!({"year": 2026}),
        json!([3]),
    ] {
        let err = s.execute("data.filterBy", json!({"column": "A", "dates": bad.clone()})).unwrap_err();
        assert!(err.to_string().contains("dates"), "{bad}: {err}");
    }
}

#[test]
fn filter_blanks_include_empty_text_and_formula_results() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["id", "f"], ["e", null], ["ef", "=\"\""], ["sp", " "], ["z", 0]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:B5"})).unwrap();
    let r = s.execute("data.filterBy", json!({"column": "B", "values": [], "blanks": true})).unwrap();
    assert_eq!(r["hiddenRows"], json!(2));
    let vals = filter_list(&s.doc().unwrap().wb, 0, 1).values;
    // The `=""` result joins (Blanks) instead of showing as its own empty entry.
    assert!(!vals.iter().any(|(t, _)| t.is_empty()), "{vals:?}");
    let blanks: Vec<_> = vals.into_iter().filter(|(t, _)| t == "(Blanks)").collect();
    assert_eq!(blanks, vec![("(Blanks)".to_string(), true)]);
}

#[test]
fn subtotal_honours_header_false() {
    // #131: with header:false the first data row was treated as a header and left out of its
    // group's subtotal and the grand total.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["East", 10], ["East", 20], ["West", 30], ["West", 40]]})).unwrap();
    s.execute("data.subtotal", json!({"range": "A1:B4", "groupBy": "A", "function": "sum", "columns": ["B"], "header": false})).unwrap();
    let get = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap();
    assert_eq!(get(&mut s, "A1")["value"], json!("East"));
    assert_eq!(get(&mut s, "A3")["text"], json!("East Total"));
    assert_eq!(get(&mut s, "B3")["value"], json!(30.0), "East's subtotal counts both East rows");
    assert_eq!(get(&mut s, "A7")["text"], json!("Grand Total"));
    assert_eq!(get(&mut s, "B7")["value"], json!(100.0), "the grand total counts every data row");
}

#[test]
fn subtotal_puts_summary_rows_below_their_detail() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", 10], ["East", 20], ["West", 30]]})).unwrap();
    s.execute("data.outlineSettings", json!({"summaryBelow": false})).unwrap();
    s.execute("data.subtotal", json!({"range": "A1:B4", "groupBy": "A", "columns": ["B"]})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap().clone();
    assert!(sh.summary_below, "the totals sit below their groups");
    // East's detail is rows 2–3 and its total row 4: collapsing level 1 keeps the total showing.
    s.execute("data.showLevels", json!({"rows": 1})).unwrap();
    let hidden = |s: &Session, row: u32| s.doc().unwrap().wb.active().unwrap().is_row_hidden(row);
    assert!(hidden(&s, 1) && hidden(&s, 2), "East's detail rows are hidden");
    assert!(!hidden(&s, 3), "East Total stays visible");
    // One undo takes the subtotals and the setting back.
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!(!sh.summary_below);
    assert_eq!(sh.value(CellRef::new(3, 0)).display(), "West");
}

#[test]
fn filter_by_huge_numeric_column_is_rejected_not_overflowed() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "B1", "values": [["Name", "Qty"], ["a", 1], ["b", 2]]})).unwrap();
    s.execute("data.filter", json!({"range": "B1:C3"})).unwrap();
    for col in [json!(2), json!(u64::from(u32::MAX)), json!(u64::MAX)] {
        let err = s.execute("data.filterBy", json!({"column": col, "values": ["a"]})).unwrap_err();
        assert!(err.to_string().contains("column outside the filter range"), "{col}: {err}");
    }
    let ok = s.execute("data.filterBy", json!({"column": 0, "values": ["a"]})).unwrap();
    assert_eq!(ok["hiddenRows"], json!(1));
}

#[test]
fn filter_by_inside_a_table_installs_the_autofilter_and_keeps_the_button() {
    for sel in [json!({"cell": "A1"}), json!({"cell": "A2"}), json!({"range": "A1:A3"})] {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["H"], ["a"], ["b"]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A3", "header": true})).unwrap();
        s.execute("selection.set", sel.clone()).unwrap();
        s.execute("data.filterBy", json!({"column": "A", "values": ["a"]})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.autofilter.as_ref().map(|a| a.range.a1()), Some("A1:A3".to_string()), "{sel}");
        assert!(sh.tables[0].filter_button, "{sel}");
        assert!(!sh.is_row_hidden(1), "{sel}");
        assert!(sh.is_row_hidden(2), "{sel}");
        assert_eq!(sh.value(CellRef::new(2, 0)).display(), "b", "{sel}");
    }
}

#[test]
fn autofilter_range_grows_when_a_value_is_entered_in_the_row_below() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["d", 4], ["b", 2]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
    let range = |s: &Session| s.doc().unwrap().wb.active().unwrap().autofilter.as_ref().map(|a| a.range.a1());
    s.execute("cell.set", json!({"cell": "A4", "input": "e"})).unwrap();
    assert_eq!(range(&s), Some("A1:B4".to_string()));
    s.execute("range.setValues", json!({"range": "A5", "values": [["f", 6]]})).unwrap();
    assert_eq!(range(&s), Some("A1:B5".to_string()));
    // A row typed after a blank row does not grow it.
    s.execute("cell.set", json!({"cell": "A7", "input": "x"})).unwrap();
    assert_eq!(range(&s), Some("A1:B5".to_string()));
    // A cell outside the filter's columns does not either.
    s.execute("cell.set", json!({"cell": "C6", "input": "x"})).unwrap();
    assert_eq!(range(&s), Some("A1:B5".to_string()));
    // Empty text is a blank entry, not a value.
    s.execute("range.setValues", json!({"range": "A6", "parse": false, "values": [[""]]})).unwrap();
    assert_eq!(range(&s), Some("A1:B5".to_string()));
    // A formula and Ctrl+Enter (range.fill) are entries too.
    s.execute("cell.set", json!({"cell": "B6", "input": "=1+1"})).unwrap();
    assert_eq!(range(&s), Some("A1:B6".to_string()));
    s.execute("range.fill", json!({"range": "A7:B7", "input": "g"})).unwrap();
    assert_eq!(range(&s), Some("A1:B7".to_string()));
}

#[test]
fn list_validation_without_formula1_is_an_error() {
    let mut s = Session::new();
    s.new_workbook();
    assert!(s.execute("data.validation", json!({"range": "A1:A3", "type": "list", "source": "\"a,b,c\""})).is_err());
    assert!(s.execute("data.validation", json!({"range": "A1:A3", "type": "list"})).is_err());
    assert!(s.doc().unwrap().wb.active().unwrap().validations.is_empty());
    s.execute("data.validation", json!({"range": "A1:A3", "type": "list", "formula1": "\"a,b,c\""})).unwrap();
    assert_eq!(s.execute("data.validate", json!({"cell": "A1", "input": "a"})).unwrap()["ok"], json!(true));
}

#[test]
fn partial_validation_edits_keep_the_rest_of_the_old_rule() {
    let mut s = Session::new();
    s.new_workbook();
    let rule = |range: &str, lo: &str, hi: &str| json!({"range": range, "type": "whole", "operator": "between", "formula1": lo, "formula2": hi});
    let ok =
        |s: &mut Session, cell: &str, input: &str| s.execute("data.validate", json!({"cell": cell, "input": input})).unwrap()["ok"] == json!(true);
    s.execute("data.validation", rule("B1:B3", "1", "10")).unwrap();
    s.execute("data.validation", json!({"range": "B2", "clear": true})).unwrap();
    assert!(ok(&mut s, "B2", "999"));
    assert!(!ok(&mut s, "B1", "999"));
    assert!(!ok(&mut s, "B3", "999"));
    s.execute("data.validation", rule("B2:B4", "100", "200")).unwrap();
    for cell in ["B2", "B3", "B4"] {
        assert!(ok(&mut s, cell, "150"), "{cell}");
        assert!(!ok(&mut s, cell, "5"), "{cell}");
    }
    assert!(ok(&mut s, "B1", "5"));
    assert!(!ok(&mut s, "B1", "150"));
}

#[test]
fn showing_detail_gives_zero_width_columns_their_width_back() {
    // #1714: templates hide grouped columns as `width="0" hidden="1"`; + must show them.
    let mut s = Session::new();
    s.new_workbook();
    let hidden = LineInfo { size: Some(0.0), hidden: true, outline: 1, ..LineInfo::default() };
    let sh = Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
    for c in 2..=4 {
        sh.cols.insert(c, hidden);
    }
    sh.cols.insert(5, LineInfo { collapsed: true, ..LineInfo::default() });
    s.execute("data.showDetail", json!({"cols": "C:E", "level": 1})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    for c in 2..=4 {
        assert_eq!(sh.col_width(c), sh.default_col_width, "column {c}");
    }
    // Hiding again hides them; the level buttons show zero-width columns too.
    s.execute("data.hideDetail", json!({"cols": "C:E", "level": 1})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().col_width(2), 0.0);
    Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap().cols.insert(3, hidden);
    s.execute("data.showLevels", json!({"cols": 2})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!((sh.col_width(2), sh.col_width(3)), (sh.default_col_width, sh.default_col_width));
}

#[test]
fn removing_an_autofilter_keeps_manually_hidden_and_collapsed_rows() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["H"], ["a"], ["b"], ["c"]]})).unwrap();
    s.execute("range.setValues", json!({"range": "A10", "values": [["z"]]})).unwrap();
    s.execute("data.group", json!({"rows": "2:4"})).unwrap();
    s.execute("data.hideDetail", json!({"rows": "2:4"})).unwrap();
    s.execute("home.hideRows", json!({"rows": "10:10"})).unwrap();
    let hidden = |s: &Session| -> Vec<u32> {
        let sh = s.doc().unwrap().wb.active().unwrap();
        sh.rows.iter().filter(|(_, l)| l.hidden).map(|(r, _)| *r).collect()
    };
    let before = hidden(&s);
    assert_eq!(before, [1, 2, 3, 9]);
    s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().autofilter.is_none());
    assert_eq!(hidden(&s), before);
    assert_eq!(row_state(&s, 4..=4), [(false, true)]);
}

#[test]
fn table_dropdown_setup_outside_the_table_keeps_manually_hidden_rows() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["H"], ["a"], ["b"], ["c"]]})).unwrap();
    s.execute("range.setValues", json!({"range": "A10", "values": [["z"]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:A4", "header": true})).unwrap();
    s.execute("home.hideRows", json!({"rows": "3:3"})).unwrap();
    s.execute("home.hideRows", json!({"rows": "10:10"})).unwrap();
    s.execute("selection.set", json!({"range": "E1"})).unwrap();
    // The table filter dropdown installs an AutoFilter on demand by toggling `data.filter` twice.
    s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.rows.iter().filter(|(_, l)| l.hidden).map(|(r, _)| *r).collect::<Vec<_>>(), [2, 9]);
}

/// Rows 5:8 grouped, with 6:7 grouped again inside (0-based 4..=7 and 5..=6).
fn nested_rows() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("data.group", json!({"rows": "5:8"})).unwrap();
    s.execute("data.group", json!({"rows": "6:7"})).unwrap();
    s
}

/// (hidden, collapsed) of 0-based rows.
fn row_state(s: &Session, rows: std::ops::RangeInclusive<u32>) -> Vec<(bool, bool)> {
    let sh = s.doc().unwrap().wb.active().unwrap();
    rows.map(|r| sh.rows.get(&r).map(|l| (l.hidden, l.collapsed)).unwrap_or_default()).collect()
}

#[test]
fn hide_detail_marks_the_summary_collapsed_and_show_detail_keeps_inner_groups_collapsed() {
    let mut s = nested_rows();
    s.execute("data.hideDetail", json!({"rows": "6:7", "level": 2})).unwrap();
    assert_eq!(row_state(&s, 4..=8), [(false, false), (true, false), (true, false), (false, true), (false, false)]);
    s.execute("data.hideDetail", json!({"rows": "5:8", "level": 1})).unwrap();
    assert_eq!(row_state(&s, 4..=8), [(true, false), (true, false), (true, false), (true, true), (false, true)]);
    s.execute("data.showDetail", json!({"rows": "5:8", "level": 1})).unwrap();
    assert_eq!(row_state(&s, 4..=8), [(false, false), (true, false), (true, false), (false, true), (false, false)]);
    s.execute("data.showDetail", json!({"rows": "6:7", "level": 2})).unwrap();
    assert_eq!(row_state(&s, 4..=8), [(false, false); 5]);
    // Each step is one undoable command.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(row_state(&s, 5..=7), [(true, false), (true, false), (false, true)]);
}

#[test]
fn hide_detail_on_the_summary_row_collapses_the_group_above_it() {
    let mut s = nested_rows();
    s.execute("data.hideDetail", json!({"rows": "9:9"})).unwrap();
    assert_eq!(row_state(&s, 4..=8), [(true, false), (true, false), (true, false), (true, false), (false, true)]);
}

#[test]
fn show_levels_collapses_and_expands_to_a_level() {
    let mut s = nested_rows();
    s.execute("data.showLevels", json!({"rows": 1})).unwrap();
    assert_eq!(row_state(&s, 4..=8), [(true, false), (true, false), (true, false), (true, true), (false, true)]);
    s.execute("data.showLevels", json!({"rows": 2})).unwrap();
    assert_eq!(row_state(&s, 4..=8), [(false, false), (true, false), (true, false), (false, true), (false, false)]);
    s.execute("data.showLevels", json!({"rows": 3})).unwrap();
    assert_eq!(row_state(&s, 4..=8), [(false, false); 5]);
    // Columns are separate; no level at all is an error rather than a guess.
    s.execute("data.group", json!({"cols": "C:E"})).unwrap();
    s.execute("data.showLevels", json!({"cols": 1})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!((2..=4).all(|c| sh.cols.get(&c).is_some_and(|l| l.hidden)));
    assert!(sh.cols.get(&5).is_some_and(|l| l.collapsed));
    assert_eq!(row_state(&s, 4..=4), [(false, false)]);
    assert!(s.execute("data.showLevels", json!({})).is_err());
}

#[test]
fn group_by_columns_for_a_block_of_cells() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
    s.execute("data.group", json!({"by": "cols"})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert!((1..=2).all(|c| sh.cols.get(&c).is_some_and(|l| l.outline == 1)));
    assert!(sh.rows.values().all(|l| l.outline == 0));
}

#[test]
fn group_shortcuts_are_the_windows_ones() {
    let sc = |id: &str| specs().into_iter().find(|c| c.id == id).and_then(|c| c.shortcut);
    assert_eq!(sc("data.group"), Some("Alt+Shift+Right"));
    assert_eq!(sc("data.ungroup"), Some("Alt+Shift+Left"));
}

#[test]
fn summary_rows_above_the_detail_take_the_collapsed_mark() {
    let mut s = nested_rows();
    let r = s.execute("data.outlineSettings", json!({"summaryBelow": false})).unwrap();
    assert_eq!(r, json!({"summaryBelow": false, "summaryRight": true}));
    // Row 4 (index 3) is the outer group's summary, row 5 the inner one's.
    s.execute("data.hideDetail", json!({"rows": "5:8", "level": 1})).unwrap();
    assert_eq!(row_state(&s, 3..=8), [(false, true), (true, false), (true, false), (true, false), (true, false), (false, false)]);
    s.execute("data.showDetail", json!({"rows": "5:8", "level": 1})).unwrap();
    assert_eq!(row_state(&s, 3..=8), [(false, false); 6]);
    // Hide Detail on the summary row collapses the group below it.
    s.execute("data.hideDetail", json!({"rows": "4:4"})).unwrap();
    assert_eq!(row_state(&s, 3..=4), [(false, true), (true, false)]);
    s.execute("data.showLevels", json!({"rows": 1})).unwrap();
    assert_eq!(row_state(&s, 3..=8), [(false, true), (true, true), (true, false), (true, false), (true, false), (false, false)]);
    // Ungrouping the outer group clears its mark on the row above.
    s.execute("data.showLevels", json!({"rows": 3})).unwrap();
    s.execute("data.hideDetail", json!({"rows": "5:8", "level": 1})).unwrap();
    s.execute("data.ungroup", json!({"rows": "5:8"})).unwrap();
    assert_eq!(row_state(&s, 3..=3), [(false, false)]);
}

#[test]
fn changing_the_summary_direction_moves_collapsed_marks() {
    let mut s = nested_rows();
    s.execute("data.group", json!({"cols": "C:E"})).unwrap();
    s.execute("data.hideDetail", json!({"rows": "5:8", "level": 1})).unwrap();
    s.execute("data.hideDetail", json!({"cols": "C:E"})).unwrap();
    s.execute("data.outlineSettings", json!({"summaryBelow": false, "summaryRight": false})).unwrap();
    let col_marked = |s: &Session, c: u32| s.doc().unwrap().wb.active().unwrap().cols.get(&c).is_some_and(|l| l.collapsed);
    assert_eq!(row_state(&s, 3..=8).iter().map(|r| r.1).collect::<Vec<_>>(), [true, false, false, false, false, false]);
    assert!(col_marked(&s, 1) && !col_marked(&s, 5));
    s.execute("data.outlineSettings", json!({"summaryBelow": true})).unwrap();
    assert_eq!(row_state(&s, 3..=8).iter().map(|r| r.1).collect::<Vec<_>>(), [false, false, false, false, false, true]);
    assert!(col_marked(&s, 1), "columns keep their direction");
    // One undo step per change; no params leaves the settings as they are.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(row_state(&s, 3..=3)[0].1);
    assert_eq!(s.execute("data.outlineSettings", json!({})).unwrap(), json!({"summaryBelow": false, "summaryRight": false}));
    // Nested groups that end on one row share its mark: it moves to the outer group only.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("data.group", json!({"rows": "5:8"})).unwrap();
    s.execute("data.group", json!({"rows": "6:8"})).unwrap();
    s.execute("data.showLevels", json!({"rows": 1})).unwrap();
    s.execute("data.outlineSettings", json!({"summaryBelow": false})).unwrap();
    assert_eq!(row_state(&s, 3..=8).iter().map(|r| r.1).collect::<Vec<_>>(), [true, false, false, false, false, false]);
    s.execute("data.showDetail", json!({"rows": "4:4"})).unwrap();
    assert_eq!(row_state(&s, 3..=8), [(false, false); 6]);
    // With only the inner group collapsed, its mark goes to row 5, and the outer group's
    // Hide and Show Detail keep it collapsed.
    s.execute("data.outlineSettings", json!({"summaryBelow": true})).unwrap();
    s.execute("data.hideDetail", json!({"rows": "6:8", "level": 2})).unwrap();
    s.execute("data.outlineSettings", json!({"summaryBelow": false})).unwrap();
    assert_eq!(row_state(&s, 3..=8), [(false, false), (false, true), (true, false), (true, false), (true, false), (false, false)]);
    s.execute("data.hideDetail", json!({"rows": "5:8", "level": 1})).unwrap();
    s.execute("data.showDetail", json!({"rows": "5:8", "level": 1})).unwrap();
    assert_eq!(row_state(&s, 3..=8), [(false, false), (false, true), (true, false), (true, false), (true, false), (false, false)]);
    // A group on the first row has no row above it to mark.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("data.group", json!({"rows": "1:3"})).unwrap();
    s.execute("data.outlineSettings", json!({"summaryBelow": false})).unwrap();
    s.execute("data.hideDetail", json!({"rows": "1:3"})).unwrap();
    assert_eq!(row_state(&s, 0..=3), [(true, false), (true, false), (true, false), (false, false)]);
}

#[test]
fn column_sort_rejects_row_key_zero_without_panicking() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    for row in [0u64, 1 << 32] {
        let params = json!({"range": "A1:B2", "header": false, "orientation": "columns", "keys": [{"row": row}]});
        let err = s.execute("data.sort", params).unwrap_err();
        assert!(matches!(err, EngineError::BadParams { .. }), "row {row}: {err:?}");
    }
}

#[test]
fn single_row_and_column_parameters() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
    s.execute("home.insertRows", json!({"rows": "2"})).unwrap();
    s.execute("home.insertColumns", json!({"cols": "B"})).unwrap();
    let v = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
    assert_eq!(v(&mut s, "C3"), json!(4.0));
    s.execute("home.hideRows", json!({"rows": "1"})).unwrap();
    s.execute("data.group", json!({"cols": "C"})).unwrap();
    assert!(s.execute("home.deleteRows", json!({"rows": "0"})).is_err());
    s.execute("home.deleteRows", json!({"rows": "2"})).unwrap();
    s.execute("home.deleteColumns", json!({"cols": "B"})).unwrap();
    assert_eq!(v(&mut s, "B2"), json!(4.0));
}

#[test]
fn t2c_fixed_widths_are_positions_in_any_order_without_duplicates() {
    let cases: [(Json, [&str; 4]); 5] = [
        (json!([6, 3]), ["abc", "def", "ghij", ""]),
        (json!([3, 3]), ["abc", "defghij", "", ""]),
        (json!([0]), ["abcdefghij", "", "", ""]),
        (json!([20]), ["abcdefghij", "", "", ""]),
        (json!([7, 0, 3, 7, 10]), ["abc", "defg", "hij", ""]),
    ];
    for (widths, want) in cases {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "abcdefghij"})).unwrap();
        s.execute("data.textToColumns", json!({"range": "A1:A1", "fixedWidths": widths})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap();
        let got: Vec<String> = (0..4).map(|c| sh.value(CellRef::new(0, c)).display()).collect();
        assert_eq!(got, want, "{widths}");
    }
}

#[test]
fn split() {
    assert_eq!(split_delimited("a,b,,c", &[','], false, '"'), ["a", "b", "", "c"]);
    assert_eq!(split_delimited("a,b,,c", &[','], true, '"'), ["a", "b", "c"]);
    assert_eq!(split_delimited("\"x,y\",z", &[','], false, '"'), ["x,y", "z"]);
}

#[test]
fn remove_duplicates_without_a_valid_column_is_refused() {
    // `columns: []` (or only unusable names) used to compare nothing and delete every row
    // but the first.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a"], ["b"], ["c"]]})).unwrap();
    for columns in [json!([]), json!(["ZZZ"])] {
        let err = s.execute("data.removeDuplicates", json!({"range": "A1:A3", "columns": columns})).unwrap_err();
        assert!(err.to_string().contains("columns"), "{err}");
    }
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::text("a"));
    assert_eq!(sh.value(CellRef::new(2, 0)), Value::text("c"));
}

#[test]
fn a_list_typed_without_quotes_is_saved_as_a_quoted_list() {
    // In a file an inline list is a string literal (`"a,b"`); unquoted it reads as a formula.
    // References, names and formulas stay unquoted, and every rule survives a save and reopen.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "K1:K3", "values": [["Red"], ["Green"], ["Blue"]]})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Colors", "refersTo": "=Sheet1!$K$1:$K$3"})).unwrap();
    let rules = [
        ("A1", "Open,In progress,Closed", r#""Open,In progress,Closed""#),
        ("B1", r#""Open,In progress,Closed""#, r#""Open,In progress,Closed""#),
        ("C1", " 1,2,3 ", r#""1,2,3""#),
        ("D1", r#"Say "hi",Bye"#, r#""Say ""hi"",Bye""#),
        ("E1", "=$K$1:$K$3", "$K$1:$K$3"),
        ("F1", "$K$1:$K$3", "$K$1:$K$3"),
        ("G1", "=Colors", "Colors"),
        ("H1", "Sheet1!$K$1:$K$3", "Sheet1!$K$1:$K$3"),
        ("I1", "=OFFSET($K$1,0,0,3,1)", "OFFSET($K$1,0,0,3,1)"),
    ];
    for (cell, source, _) in rules {
        s.execute("data.validation", json!({"range": cell, "type": "list", "formula1": source})).unwrap();
    }
    let accepted = [
        ("A1", "In progress"),
        ("B1", "Closed"),
        ("C1", "2"),
        ("D1", r#"Say "hi""#),
        ("E1", "Green"),
        ("F1", "Blue"),
        ("G1", "Red"),
        ("H1", "Green"),
    ];
    let check = |wb: &Workbook| {
        for (cell, input) in accepted {
            let at = CellRef::parse(cell).unwrap();
            assert!(check_validation(wb, 0, at, input).is_none(), "{cell} refuses {input}");
            assert!(check_validation(wb, 0, at, "Pending").is_some(), "{cell} accepts Pending");
        }
    };
    let wb = s.doc().unwrap().wb.clone();
    check(&wb);

    let sheet_xml = |wb: &Workbook| {
        let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut xml).unwrap();
        xml
    };
    let xml = sheet_xml(&wb);
    for (cell, _, saved) in rules {
        assert!(xml.contains(&format!(r#"sqref="{cell}"><formula1>{saved}</formula1>"#)), "{cell}: {saved} missing from {xml}");
    }
    let (reopened, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
    check(&reopened);
    assert_eq!(sheet_xml(&reopened), xml, "a second save writes the same rules");
}

#[test]
fn a_rule_with_an_unknown_type_operator_or_error_style_is_refused() {
    // A typo used to give a rule that validates nothing (`type="none"`) or the wrong check,
    // with no error.
    let mut s = Session::new();
    s.new_workbook();
    for params in [
        json!({"range": "A1", "type": "lst", "formula1": "\"a,b\""}),
        json!({"range": "A1", "type": "whole", "operator": "greaterthan", "formula1": "1"}),
        json!({"range": "A1", "type": "whole", "operator": ">", "formula1": "1"}),
        json!({"range": "A1", "type": "list", "formula1": "\"a,b\"", "errorStyle": "warn"}),
    ] {
        let e = s.execute("data.validation", params.clone()).unwrap_err().to_string();
        assert!(e.contains("unknown"), "{params}: {e}");
    }
    assert!(s.doc().unwrap().wb.active().unwrap().validations.is_empty());
    for params in [
        json!({"range": "A1", "type": "any"}),
        json!({"range": "A2", "type": "whole", "operator": "greaterThan", "formula1": "1"}),
        json!({"range": "A3", "type": "list", "formula1": "\"a,b\"", "errorStyle": "warning"}),
    ] {
        s.execute("data.validation", params).unwrap();
    }
    let sh = s.doc().unwrap().wb.active().unwrap().clone();
    assert_eq!(sh.validations.len(), 3);
    assert!(check_validation(&s.doc().unwrap().wb, 0, CellRef::parse("A2").unwrap(), "0").is_some());
}
