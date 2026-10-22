//! Pivot field buttons use the surface color for the active theme.

use egui::{Color32, RawInput, Rect, Shape, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, theme};
use serde_json::json;

fn render_pivot_field_buttons(dark: bool) -> Vec<Shape> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", 100], ["West", 80]]})).unwrap();
    session
        .execute("insert.pivotTable", json!({"source": "Sheet1!A1:B3", "destination": "Sheet1!D3", "rows": ["Region"], "values": ["Sales"]}))
        .unwrap();
    session.execute("selection.set", json!({"cell": "D3"})).unwrap();

    let mut app = SheetApp::new(session, Default::default());
    app.ui.dark = dark;
    let ctx = egui::Context::default();
    let mut output = egui::FullOutput::default();
    for _ in 0..6 {
        output = ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), ..Default::default() }, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        });
        output.textures_delta.clear();
    }
    output.shapes.into_iter().map(|shape| shape.shape).collect()
}

fn field_button_fill(shapes: &[Shape], label: &str) -> Color32 {
    let text = shapes
        .iter()
        .find_map(|shape| match shape {
            Shape::Text(text) if text.galley.text() == label => Some(text),
            _ => None,
        })
        .unwrap_or_else(|| panic!("pivot field button {label:?} was not painted"));
    let text_rect = text.galley.rect.translate(text.pos.to_vec2());
    shapes
        .iter()
        .filter_map(|shape| match shape {
            Shape::Rect(rect) if rect.rect.contains(text_rect.center()) => Some(rect),
            _ => None,
        })
        .min_by(|a, b| a.rect.area().total_cmp(&b.rect.area()))
        .unwrap_or_else(|| panic!("pivot field button {label:?} has no painted background"))
        .fill
}

#[test]
fn pivot_field_button_surfaces_follow_the_active_theme() {
    for (dark, expected) in [(false, theme::Tokens::light().ribbon), (true, theme::Tokens::dark().ribbon)] {
        let shapes = render_pivot_field_buttons(dark);
        assert_eq!(field_button_fill(&shapes, "Region ▾"), expected, "dark mode: {dark}");
    }
}
