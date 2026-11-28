//! `file.exportPdf` with `fitToPage`: a wide sheet fits one page with it and paginates without it,
//! and nothing shrinks below the 10% minimum scale.

use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

fn wide_session(columns: i64) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let row: Vec<i64> = (1..=columns).collect();
    s.execute("range.setValues", json!({"range": "A1", "values": [row]})).unwrap();
    s
}

fn pages(s: &mut Session, params: Json) -> u64 {
    s.execute("file.exportPdf", params).unwrap()["pages"].as_u64().unwrap()
}

#[test]
fn fit_to_page_puts_a_wide_sheet_on_one_page_and_off_paginates() {
    // 40 default-width columns are far wider than one portrait page.
    let mut s = wide_session(40);
    let paginated = pages(&mut s, json!({}));
    assert!(paginated > 1, "without the flag the wide sheet needs several pages: {paginated}");
    assert_eq!(pages(&mut s, json!({"fitToPage": true})), 1);
    // The flag is per export: it does not change the sheet's own Page Setup.
    assert_eq!(pages(&mut s, json!({})), paginated);
    assert_eq!(pages(&mut s, json!({"fitToPage": false})), paginated);
}

#[test]
fn fit_to_page_stops_shrinking_at_ten_percent() {
    // 400 columns need more than one page even at the 10% minimum scale; the UI warns about this.
    let mut s = wide_session(400);
    assert!(pages(&mut s, json!({"fitToPage": true})) > 1);
}
