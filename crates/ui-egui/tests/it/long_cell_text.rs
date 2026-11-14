//! Very long cell text paints without asking the GPU for more than it allows. The whole string
//! used to be laid out and tessellated every frame: a 1.1 MB CSV field built a 335 MB vertex
//! buffer, over wgpu's 256 MiB maximum, and the app aborted on open.
//!
//! Bounds keep it small: the grid lays out a one-line cell's first 1,024 characters (far wider
//! than a screen) and a wrapped cell's first 32,767 (a cell's maximum), and a CSV import cuts a
//! field to a cell's 32,767 characters, which also bounds the formula bar.

use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::Workbook;
use gridcraft_engine::{DocState, Session};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Characters the grid lays out of a one-line cell, and of a wrapped one.
const LINE_CHARS: usize = 1024;
const CELL_CHARS: usize = 32_767;
/// wgpu's default `max_buffer_size`.
const MAX_BUFFER_BYTES: usize = 256 << 20;
/// One frame of a sheet with a long cell stays far below the buffer limit.
const FRAME_BUDGET: usize = MAX_BUFFER_BYTES / 16;

/// The shape of the real file: one log line repeated into a 1.1 MB field.
fn long_text() -> String {
    "Connection Error: hresult=0x80040408, message=Could not start the service.\n".repeat(15_000)
}

fn harness(wb: Workbook, setup: impl FnOnce(&mut Session)) -> Harness {
    let mut session = Session::new();
    session.add_document(DocState::new(wb, None, "audit".into(), gridcraft_engine::Trust::FromFile));
    setup(&mut session);
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(3);
    h
}

fn longest_text_shape(shape: &egui::Shape) -> usize {
    match shape {
        egui::Shape::Text(s) => s.galley.text().chars().count(),
        egui::Shape::Vec(shapes) => shapes.iter().map(longest_text_shape).max().unwrap_or(0),
        _ => 0,
    }
}

/// Bytes of vertices and indices the last frame tessellates into.
fn frame_bytes(h: &Harness) -> usize {
    h.ctx
        .tessellate(h.output().shapes.clone(), h.ctx.pixels_per_point())
        .iter()
        .map(|p| match &p.primitive {
            egui::epaint::Primitive::Mesh(m) => {
                m.vertices.len() * std::mem::size_of::<egui::epaint::Vertex>() + m.indices.len() * std::mem::size_of::<u32>()
            }
            egui::epaint::Primitive::Callback(_) => 0,
        })
        .sum()
}

fn painted_starting_with(h: &Harness, opening: &str) -> bool {
    fn walk(shape: &egui::Shape, opening: &str) -> bool {
        match shape {
            egui::Shape::Text(t) => t.galley.text().starts_with(opening),
            egui::Shape::Vec(shapes) => shapes.iter().any(|s| walk(s, opening)),
            _ => false,
        }
    }
    h.output().shapes.iter().any(|s| walk(&s.shape, opening))
}

#[test]
fn a_million_character_cell_paints_its_first_characters_within_the_gpu_buffer_limit() {
    // Put in directly: an import would cut it, and the grid must hold its bound on its own.
    let text = long_text();
    assert!(text.chars().count() > 1_000_000);
    for wrap in [false, true] {
        let mut wb = Workbook::new();
        wb.sheet_mut(0).unwrap().set_value(CellRef::new(1, 1), Value::text(text.clone()));
        let h = harness(wb, |s| {
            if wrap {
                s.execute("home.formatCells", json!({"range": "B2", "style": {"align": {"wrap": true}}})).unwrap();
            }
        });
        let longest = h.output().shapes.iter().map(|s| longest_text_shape(&s.shape)).max().unwrap_or(0);
        let most = if wrap { CELL_CHARS } else { LINE_CHARS };
        assert!(longest <= most, "wrap {wrap}: a text shape lays out {longest} characters");
        assert!(painted_starting_with(&h, "Connection Error: hr"), "wrap {wrap}: the cell's text is not painted");
        let bytes = frame_bytes(&h);
        assert!(bytes < FRAME_BUDGET, "wrap {wrap}: one frame tessellates {bytes} bytes");
    }
}

#[test]
fn the_formula_bar_shows_an_imported_long_field_within_the_gpu_buffer_limit() {
    let csv = format!("id,message\n1,\"{}\"\n", long_text());
    let (wb, warnings) = gridcraft_engine::io::open_bytes("audit.csv", csv.as_bytes()).unwrap();
    assert!(warnings.iter().any(|w| w.contains("B2") && w.contains("32767")), "{warnings:?}");
    let h = harness(wb, |s| {
        s.execute("selection.set", json!({"cell": "B2"})).unwrap();
    });
    // The bar shows the cell's (cut) text: the whole of it, not the grid's one-line 1,024.
    let longest = h.output().shapes.iter().map(|s| longest_text_shape(&s.shape)).max().unwrap_or(0);
    assert!(longest > LINE_CHARS && longest <= CELL_CHARS, "the formula bar lays out {longest} characters");
    let bytes = frame_bytes(&h);
    assert!(bytes < FRAME_BUDGET, "one frame tessellates {bytes} bytes");
}
