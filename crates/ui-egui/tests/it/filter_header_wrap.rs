//! A wrapped header under an AutoFilter dropdown wraps across its whole column, as in Excel
//! (checked in Excel: a wrapped header exactly as wide as its text stays one line with the
//! dropdown).

use egui::Shape;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

const HEADER: &str = "Agreement signed with the owner";

/// How many lines the header is painted on, with column A `width` pixels wide.
fn header_lines(width: Option<f32>, filtered: bool) -> usize {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": HEADER})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "x"})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    if filtered {
        s.execute("data.filter", json!({"range": "A1:A2"})).unwrap();
    }
    // Away from A1, so the formula bar doesn't show the header too.
    s.execute("selection.set", json!({"range": "H20"})).unwrap();
    let d = s.doc_mut().unwrap();
    let sh = std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut d.wb).sheets[0]);
    sh.cols.entry(0).or_default().size = Some(width.unwrap_or(600.0));
    let e = sh.rows.entry(0).or_default();
    (e.size, e.custom) = (Some(60.0), true);
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 600.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h.output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == HEADER => Some(t.galley.rows.len()),
            _ => None,
        })
        .expect("the header is painted")
}

#[test]
fn a_filtered_header_wraps_across_its_whole_column() {
    // The narrowest column (in steps of 2 px) the header fits on one line without a filter.
    let width = (40..400).map(|w| w as f32 * 2.0).find(|w| header_lines(Some(*w), false) == 1).expect("the header fits somewhere");
    assert!(header_lines(Some(width - 2.0), false) > 1);
    // With a filter it still fits: the text runs under the dropdown.
    assert_eq!(header_lines(Some(width), true), 1);
}
