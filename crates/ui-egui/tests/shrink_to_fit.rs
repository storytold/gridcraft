//! Shrink to fit (#375): too-wide content is drawn in a smaller font inside its cell.

use egui::Shape;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    let shrink = json!({"align": {"shrink": true}});
    s.execute("cell.set", json!({"cell": "A1", "input": "Executive Producer"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": shrink})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "Control Producer"})).unwrap();
    // Column A's width set by hand (to the default), so the numbers formatted below keep it.
    s.execute("home.columnWidth", json!({"cols": "A:A", "width": 64})).unwrap();
    s.execute("cell.set", json!({"cell": "A4", "input": "123456789012"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A4", "style": {"num_fmt": "#,##0", "align": {"shrink": true}}})).unwrap();
    s.execute("cell.set", json!({"cell": "A5", "input": "123456789012"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A5", "style": {"num_fmt": "#,##0"}})).unwrap();
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

/// The painted text shape whose text is `text`: (font size, width, left x).
fn text_shape(h: &Harness, text: &str) -> Option<(f32, f32, f32)> {
    h.output().shapes.iter().find_map(|s| match &s.shape {
        Shape::Text(t) if t.galley.job.text == text => {
            let size = t.galley.job.sections.first().map(|s| s.format.font_id.size)?;
            Some((size, t.galley.size().x, t.pos.x))
        }
        _ => None,
    })
}

#[test]
fn shrink_to_fit_text_and_numbers_fit_their_cell() {
    let h = harness();
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let a1 = geo.cell_rect(sheet, CellRef::parse("A1").unwrap());
    let (small, w, x) = text_shape(&h, "Executive Producer").expect("A1 is drawn");
    let (normal, cw, _) = text_shape(&h, "Control Producer").expect("A2 is drawn");
    assert!(small < normal, "A1 is drawn smaller ({small} vs {normal})");
    assert!(cw > a1.width(), "the control is wider than the column");
    assert!(x >= a1.left() && x + w <= a1.right(), "A1 fits inside column A");
    // A number shrinks instead of turning into ####; its unshrunk twin still shows ####.
    let (num, nw, _) = text_shape(&h, "123,456,789,012").expect("A4 shows the number");
    assert!(num < normal && nw <= a1.width());
    assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if t.galley.job.text.starts_with("##"))));
}
