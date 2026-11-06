//! Small cell text is drawn in black and white on the pixel grid, as glyphs of its own, and larger
//! text the usual way.

use egui::{Shape, TextureId};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const TEXT: &str = "Readable";

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "B2", "input": TEXT})).unwrap();
    s.execute("selection.set", json!({"range": "D8"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

fn as_text_shape(h: &Harness) -> bool {
    h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if t.galley.job.text == TEXT))
}

/// Meshes on a texture of their own, not egui's font atlas: the glyphs drawn on the pixel grid.
fn glyph_meshes(h: &Harness) -> usize {
    h.output().shapes.iter().filter(|s| matches!(&s.shape, Shape::Mesh(m) if m.texture_id != TextureId::default() && !m.vertices.is_empty())).count()
}

#[test]
fn small_text_is_drawn_on_the_pixel_grid_and_larger_text_as_usual() {
    let mut h = harness();
    assert!(as_text_shape(&h), "at 100% the cell's text is an ordinary text shape");
    let before = glyph_meshes(&h);
    h.state_mut().run("view.zoom", json!({"percent": 50})).unwrap();
    h.run_steps(4);
    assert!(!as_text_shape(&h), "at 50% the cell's text is small enough for the pixel grid");
    assert!(glyph_meshes(&h) > before, "its glyphs are drawn as a mesh of their own");
    h.state_mut().run("view.zoom", json!({"percent": 100})).unwrap();
    h.run_steps(4);
    assert!(as_text_shape(&h), "back at 100% it is drawn the usual way again");
}
