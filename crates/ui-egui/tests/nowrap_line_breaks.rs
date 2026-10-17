//! Line breaks with Wrap Text off (#1604): Excel ignores them and draws the whole string on one line.

use egui::Shape;
use gridcraft_engine::Session;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, gridcraft_ui_egui::SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "Active\n\nUnits: LF 2x10\r\nBand: 60-20000 Hz"})).unwrap();
    // Typing a break turns Wrap Text on; a file can still store the text with it off.
    s.execute("home.formatCells", json!({"range": "A1", "style": {"align": {"wrap": false}}})).unwrap();
    s.execute("cell.set", json!({"cell": "A3", "input": "Wrapped\nText"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A3", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("selection.set", json!({"range": "H20"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut gridcraft_ui_egui::SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        gridcraft_ui_egui::SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

fn painted(h: &Harness, text: &str) -> bool {
    h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if t.galley.job.text == text))
}

#[test]
fn unwrapped_cell_draws_every_line_on_one_line() {
    let h = harness();
    assert!(painted(&h, "ActiveUnits: LF 2x10Band: 60-20000 Hz"), "A1 draws the whole string without its breaks");
    assert!(!painted(&h, "Active"), "A1 is not cut at its first break");
    // Wrap Text on keeps the breaks.
    assert!(painted(&h, "Wrapped\nText"));
}
