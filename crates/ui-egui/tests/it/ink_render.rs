//! Committed ink is painted as the pen previews it, at any zoom.

use egui::Shape;
use egui::epaint::ColorMode;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

/// The colour of the ink stroke drawn from (40, 40) to (200, 40) sheet points, `width` points
/// wide: the two-point horizontal path whose width is `width` at its own scale (160 pt long).
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
fn ink_translucency_follows_the_pen_not_the_zoom() {
    // A highlighter (over 8 pt) is translucent and a pen isn't, whatever the zoom: the committed
    // stroke used its width on screen, so at 50% a 12 pt highlighter turned opaque and at 200% a
    // 6 pt pen turned translucent, unlike the pen's preview.
    for (width, zoom, translucent) in [(12.0_f32, 50, true), (6.0, 200, false), (12.0, 100, true), (6.0, 100, false)] {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("view.zoom", json!({"percent": zoom})).unwrap();
        s.execute("draw.stroke", json!({"points": [[40, 40], [200, 40]], "color": "#FFFF00", "width": width})).unwrap();
        let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            crate::common::app(s),
        );
        h.run_steps(4);
        let c = painted_ink(&h, width).unwrap_or_else(|| panic!("the {width} pt stroke is painted at {zoom}%"));
        assert_eq!(c.a() < 255, translucent, "{width} pt at {zoom}%: {c:?}");
    }
}
