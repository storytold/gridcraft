//! Small cell text is drawn in black and white on the pixel grid, as glyphs of its own, and larger
//! text the usual way.

use egui::{Rect, Shape, TextureId};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const TEXT: &str = "Readable";

fn harness() -> Harness {
    harness_with(|s| {
        s.execute("cell.set", json!({"cell": "B2", "input": TEXT})).unwrap();
    })
}

fn harness_with(setup: impl FnOnce(&mut Session)) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    setup(&mut s);
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

fn cell_rect(h: &Harness, cell: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap())
}

/// Glyphs drawn on the pixel grid inside `cell`, one quad each.
fn glyphs_in(h: &Harness, cell: &str) -> usize {
    let r = cell_rect(h, cell).expand(1.0);
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Mesh(m) if m.texture_id != TextureId::default() && !m.vertices.is_empty() && m.vertices.iter().all(|v| r.contains(v.pos)) => {
                Some(m.vertices.len() / 4)
            }
            _ => None,
        })
        .sum()
}

fn text_in(h: &Harness, f: impl Fn(&str) -> bool) -> bool {
    h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if f(&t.galley.job.text)))
}

fn at_half(mut h: Harness) -> Harness {
    h.state_mut().run("view.zoom", json!({"percent": 50})).unwrap();
    h.run_steps(4);
    h
}

/// A number too wide for its column shows #### at a small zoom too, never its digits.
#[test]
fn a_number_too_wide_shows_hashes_when_small() {
    let h = at_half(harness_with(|s| {
        s.execute("range.setValues", json!({"range": "A1", "values": [[123_456_789_012.0]]})).unwrap();
        s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 4})).unwrap();
    }));
    let glyphs = glyphs_in(&h, "A1");
    assert!(glyphs < 12, "not the twelve digits: {glyphs} glyphs");
    assert!(glyphs > 0 || text_in(&h, |t| t.starts_with('#')), "#### is drawn");
    assert!(!text_in(&h, |t| t.contains("1234")), "nor as text");
}

/// A repeat fill keeps its fill characters at a small zoom.
#[test]
fn a_repeat_fill_keeps_its_fill_when_small() {
    let h = at_half(harness_with(|s| {
        s.execute("range.setValues", json!({"range": "A1", "values": [[5]]})).unwrap();
        s.execute("home.formatCells", json!({"range": "A1", "style": {"num_fmt": "*-0"}})).unwrap();
        s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 14})).unwrap();
    }));
    let glyphs = glyphs_in(&h, "A1");
    assert!(glyphs > 2 || text_in(&h, |t| t.contains("--")), "the fill is drawn, not only the 5: {glyphs} glyphs");
}
