//! Justified and distributed text, horizontal or vertical, folds at the column width as wrapped
//! text does, without Wrap Text set, as in Excel.

use egui::Shape;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{HAlign, Style, StyleId, VAlign};
use serde_json::json;

/// A cell look, set on its style.
type Look = fn(&mut Style);

const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa";

/// The rows the grid paints `TEXT` on, in B2 with the given look.
fn painted_rows(look: Look) -> usize {
    let mut session = crate::support::blank_session();
    session.execute("home.columnWidth", json!({"cols": "B:B", "chars": 15})).unwrap();
    session.execute("cell.set", json!({"cell": "B2", "input": TEXT})).unwrap();
    session.execute("home.rowHeight", json!({"rows": "2:2", "height": 120})).unwrap();
    let wb = std::sync::Arc::make_mut(&mut session.doc_mut().unwrap().wb);
    let st = wb.styles.derive(StyleId::DEFAULT, look);
    wb.sheet_mut(0).unwrap().set_style(CellRef::new(1, 1), st);
    let h = crate::support::harness(session, 4);
    h.output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == TEXT => Some(t.galley.rows.len()),
            _ => None,
        })
        .expect("the text is painted")
}

#[test]
fn justified_and_distributed_text_folds_like_wrapped_text() {
    let wrapped = painted_rows(|s| s.align.wrap = true);
    assert!(wrapped >= 3, "wrapped text folds onto {wrapped} rows");
    let looks: [(&str, Look); 4] = [
        ("justify", |s| s.align.h = HAlign::Justify),
        ("distributed", |s| s.align.h = HAlign::Distributed),
        ("vertical justify", |s| s.align.v = VAlign::Justify),
        ("vertical distributed", |s| s.align.v = VAlign::Distributed),
    ];
    for (name, look) in looks {
        assert_eq!(painted_rows(look), wrapped, "{name} text folds as wrapped text does");
    }
}
