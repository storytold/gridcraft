//! Justified and distributed text, horizontal or vertical, folds at the column width as wrapped
//! text does, without Wrap Text set, as in Excel.

use egui::Shape;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Color, HAlign, Style, StyleId, Underline, VAlign};
use serde_json::json;

/// A cell look, set on its style.
type Look = fn(&mut Style);

const TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa";

/// The grid's shapes with `TEXT` in B2 in the given look.
fn painted(look: Look) -> Vec<egui::epaint::ClippedShape> {
    let mut session = crate::support::blank_session();
    session.execute("home.columnWidth", json!({"cols": "B:B", "chars": 15})).unwrap();
    session.execute("cell.set", json!({"cell": "B2", "input": TEXT})).unwrap();
    session.execute("home.rowHeight", json!({"rows": "2:2", "height": 120})).unwrap();
    let wb = std::sync::Arc::make_mut(&mut session.doc_mut().unwrap().wb);
    let st = wb.styles.derive(StyleId::DEFAULT, look);
    wb.sheet_mut(0).unwrap().set_style(CellRef::new(1, 1), st);
    let h = crate::support::harness(session, 4);
    h.output().shapes.clone()
}

/// The rows the grid paints `TEXT` on, in B2 with the given look.
fn painted_rows(look: Look) -> usize {
    painted(look)
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

#[test]
fn underline_and_strike_run_under_every_folded_line() {
    let red = egui::Color32::from_rgb(0xC0, 0x10, 0x20);
    let looks: [(&str, Look); 2] = [
        ("wrapped", |s| {
            s.align.wrap = true;
            (s.font.underline, s.font.strike, s.font.color) = (Underline::Single, true, Color::Rgb(0xC01020));
        }),
        ("distributed", |s| {
            s.align.h = HAlign::Distributed;
            (s.font.underline, s.font.strike, s.font.color) = (Underline::Single, true, Color::Rgb(0xC01020));
        }),
    ];
    for (name, look) in looks {
        let shapes = painted(look);
        let rows = shapes
            .iter()
            .find_map(|s| match &s.shape {
                Shape::Text(t) if t.galley.job.text == TEXT => Some(t.galley.rows.len()),
                _ => None,
            })
            .expect("the text is painted");
        let lines = shapes.iter().filter(|s| matches!(&s.shape, Shape::LineSegment { stroke, .. } if stroke.color == red)).count();
        assert!(rows >= 3, "{name}: {rows} rows");
        // One underline and one strikethrough per folded line.
        assert_eq!(lines, 2 * rows, "{name}: {lines} lines for {rows} rows");
    }
}
