//! Wrapped cell text starts a new row only after a space or a hyphen, as in Excel: a long URL
//! fills its row and breaks mid-word rather than after a `/` or a `.`.

use egui::Shape;
use serde_json::json;

const URL: &str = "https://example.com/aaaa/bbbb/cccc/dddd/eeee/ffff";

#[test]
fn a_wrapped_url_breaks_where_excel_breaks_it() {
    let mut session = crate::support::blank_session();
    session.execute("home.columnWidth", json!({"cols": "B:B", "chars": 12})).unwrap();
    session.execute("cell.set", json!({"cell": "B2", "input": URL})).unwrap();
    session.execute("home.wrapText", json!({"range": "B2", "on": true})).unwrap();
    session.execute("home.rowHeight", json!({"rows": "2:2", "height": 120})).unwrap();
    let h = crate::support::harness(session, 4);
    let galley = h
        .output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == URL => Some(t.galley.clone()),
            _ => None,
        })
        .expect("the URL is painted");
    let rows: Vec<String> = galley.rows.iter().map(|r| r.glyphs.iter().map(|g| g.chr).collect()).collect();
    assert!(rows.len() > 1, "the URL wraps: {rows:?}");
    assert_eq!(rows.concat(), URL);
    // The first row runs past `https://` and `example.` to the column's edge: egui's own rule
    // ended it after the `//`, then after the `.`.
    assert!(rows[0].len() > "https://".len() && rows[1] != "example.", "rows: {rows:?}");
    // Rows end where the column is full, so there are fewer of them.
    assert!(rows.len() <= 5, "rows: {rows:?}");
}
