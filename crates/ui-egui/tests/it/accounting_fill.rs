//! Repeat fill (`*` in a number format, as in the Accounting format): a number that fits is drawn
//! across its cell, and one too wide for its column shows #### instead of losing its leading digits.

use egui::Shape;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const ACCOUNTING: &str = r#"_-* #,##0.00\ "CHF"_-;\-* #,##0.00\ "CHF"_-;_-* "-"??\ "CHF"_-;_-@_-"#;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1:B2", "values": [[344.75, 344.75], [2509, 2509]]})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1:B2", "style": {"num_fmt": ACCOUNTING}})).unwrap();
    // Column A is too narrow for 2,509.00 CHF; column B fits both.
    s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 7})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "B:B", "chars": 20})).unwrap();
    s.execute("selection.set", json!({"range": "H20"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

/// Painted text shapes whose text satisfies `f`: (text, left x, width).
fn texts(h: &Harness, f: impl Fn(&str) -> bool) -> Vec<(String, f32, f32)> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if f(&t.galley.job.text) => Some((t.galley.job.text.clone(), t.pos.x, t.galley.size().x)),
            _ => None,
        })
        .collect()
}

#[test]
fn too_wide_fill_number_shows_hashes_not_a_clipped_value() {
    let h = harness();
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let a2 = geo.cell_rect(sheet, CellRef::parse("A2").unwrap());
    let b2 = geo.cell_rect(sheet, CellRef::parse("B2").unwrap());
    // A2 is drawn as #### inside its cell.
    let hashes = texts(&h, |t| !t.is_empty() && t.chars().all(|c| c == '#'));
    assert!(hashes.iter().any(|(_, x, w)| *x >= a2.left() && x + w <= a2.right() + 0.5), "A2 shows ####: {hashes:?}");
    // The only 2,509.00 drawn is B2's, whole and inside column B.
    let values = texts(&h, |t| t.contains("509.00"));
    assert_eq!(values.len(), 1, "{values:?}");
    let (text, x, w) = &values[0];
    assert!(text.contains("2,509.00 CHF"), "{text:?}");
    assert!(*x >= b2.left() && x + w <= b2.right() + 0.5, "B2's value stays in its cell: {values:?}");
}

#[test]
fn fill_spreads_a_fitting_number_across_its_cell() {
    let h = harness();
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let b1 = geo.cell_rect(sheet, CellRef::parse("B1").unwrap());
    let drawn = texts(&h, |t| t.contains("344.75 CHF"));
    let (_, x, w) = drawn.iter().find(|(_, x, _)| *x >= b1.left()).expect("B1 is drawn");
    // The fill pushes the value to the right edge (less the trailing `_-` space).
    assert!(b1.right() - (x + w) < 6.0, "B1 ends at its right edge: {drawn:?} in {b1:?}");
    assert!(*x - b1.left() < 6.0, "the fill starts at the left edge: {drawn:?} in {b1:?}");
}
