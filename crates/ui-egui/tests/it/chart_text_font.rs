//! Chart text is drawn in the workbook theme's body (minor) font, as Excel draws every chart text
//! that names no font of its own, not in the interface font.

use egui::{FontFamily, Shape};
use gridcraft_engine::Session;
use serde_json::json;

/// The font families of the texts painted for `wanted` strings right of column C, where the
/// chart is: the same strings in the source cells, columns A and B, are cell text.
fn families(h: &crate::common::Harness, wanted: &[&str]) -> Vec<(String, FontFamily)> {
    let chart_left = h.state().grid.rect.unwrap().left() + 3.0 * 64.0 + 20.0;
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if wanted.contains(&t.galley.job.text.as_str()) && t.pos.x > chart_left => {
                t.galley.job.sections.first().map(|sec| (t.galley.job.text.clone(), sec.format.font_id.family.clone()))
            }
            _ => None,
        })
        .collect()
}

fn chart_session(minor: &str) -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Quarter", "North"], ["Q1", 10], ["Q2", 30]]})).unwrap();
    session.execute("insert.chart", json!({"range": "A1:B3", "type": "column", "at": "D2", "width": 360, "height": 220})).unwrap();
    let doc = session.active_mut().unwrap();
    std::sync::Arc::make_mut(&mut doc.wb).theme.minor_font = minor.into();
    session
}

#[test]
fn chart_text_takes_the_theme_body_font() {
    for minor in ["Calibri", "Arial", "Times New Roman"] {
        let h = crate::support::harness(chart_session(minor), 4);
        let found = families(&h, &["Q1", "Q2", "North"]);
        assert!(!found.is_empty(), "{minor}: the chart's labels are painted");
        let expected = gridcraft_ui_egui::theme::cell_family(minor, false, false);
        for (text, family) in &found {
            assert_eq!(*family, expected, "{minor}: {text} is drawn in the theme's body font");
        }
    }
    // A serif body font isn't the interface font: the families really differ.
    assert_ne!(gridcraft_ui_egui::theme::cell_family("Times New Roman", false, false), gridcraft_ui_egui::theme::ui_font(10.0).family);
}
