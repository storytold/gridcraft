//! Committed ink is painted as the pen previews it, at any zoom.

use egui::Shape;
use egui::epaint::ColorMode;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

/// The colour of the ink stroke drawn from (40, 40) to (200, 40) sheet points, `width` sheet
/// points across: the two-point horizontal path whose width is `width` at its own scale (160 pt
/// long).
fn painted_ink(h: &egui_kittest::Harness<'static, SheetApp>, width: f32) -> Option<egui::Color32> {
    h.output().shapes.iter().find_map(|s| match &s.shape {
        Shape::Path(p) if p.points.len() == 2 && (p.points[0].y - p.points[1].y).abs() < 0.01 => {
            let z = (p.points[1].x - p.points[0].x).abs() / 160.0;
            match p.stroke.color {
                ColorMode::Solid(c) if z > 0.0 && (p.stroke.width - width * z).abs() < 0.01 => Some(c),
                _ => None,
            }
        }
        _ => None,
    })
}

#[test]
fn ink_translucency_follows_the_brush_not_the_zoom() {
    // A highlighter is translucent and a pen opaque, whatever the zoom: the committed stroke
    // keeps its brush's width, so a highlighter is not repainted opaque at 50% or a pen
    // translucent at 200%.
    for (brush, mm, zoom, translucent) in
        [("highlighter", 4.0_f32, 50, true), ("pen", 1.0, 200, false), ("highlighter", 4.0, 100, true), ("pen", 0.5, 100, false)]
    {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("view.zoom", json!({"percent": zoom})).unwrap();
        s.execute("draw.stroke", json!({"points": [[40, 40], [200, 40]], "color": "#FFFF00", "width": mm, "brush": brush})).unwrap();
        // The painted width is the brush's height in cm converted to sheet points; a highlighter
        // is twice as tall across the stroke as it is wide.
        let tool = gridcraft_engine::cmd::draw::parse_tool(brush).unwrap_or_default();
        let (_, height_cm) = gridcraft_engine::cmd::draw::ink_size_cm(tool, mm);
        let width = gridcraft_engine::cmd::draw::cm_to_px(height_cm);
        let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            crate::common::app(s),
        );
        h.run_steps(4);
        let c = painted_ink(&h, width).unwrap_or_else(|| panic!("the {brush} stroke is painted at {zoom}%"));
        assert_eq!(c.a() < 255, translucent, "{brush} at {zoom}%: {c:?}");
    }
}
