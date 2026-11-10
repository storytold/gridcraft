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

/// A highlighter's translucency belongs to the brush and does not change with the zoom.
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

/// An effect's bands follow the base stroke's shape: pencil-width on a Pencil, tapered on a
/// Fountain Pen — never a full-width overlay that hides the brush.
#[test]
fn effect_bands_follow_the_brush_shape() {
    // A tapered base narrows to 0.05×width at low pressure; the overlay must narrow with it.
    for (brush, narrow) in [("pencil", false), ("fountainPen", true)] {
        let mut s = Session::new();
        s.new_workbook();
        s.execute(
            "draw.stroke",
            json!({"points": [[40, 80], [120, 80], [200, 80]], "color": "#1F5FC9", "width": 2.0, "brush": brush, "effect": "rainbow", "pressure": [2000, 32767, 2000]}),
        )
        .unwrap();
        let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            crate::common::app(s),
        );
        h.run_steps(4);
        // Tessellation turns segments into small paths: the 2-point ones along the stroke are
        // the effect bands; the base is a longer path and the grid paints at ~1px.
        let kinds: Vec<String> = h
            .output()
            .shapes
            .iter()
            .map(|s| match &s.shape {
                Shape::Path(p) => format!("path#{} w={:.1}", p.points.len(), p.stroke.width),
                _ => "other".into(),
            })
            .collect();
        let widths: Vec<f32> = h
            .output()
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::Path(p) if p.points.len() == 2 && (p.points[0].y - p.points[1].y).abs() < 3.0 && p.stroke.width > 1.5 => Some(p.stroke.width),
                _ => None,
            })
            .collect();
        assert!(!widths.is_empty(), "the {brush} effect paints band segments: {kinds:?} (brush={brush})");
        if narrow {
            // The bands must take more than one width along the stroke: the swell's ends are
            // much narrower than its middle, while an overlay paints one width everywhere.
            let mut sorted = widths.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            sorted.dedup_by(|a, b| (*a - *b).abs() < 0.05);
            assert!(sorted.len() > 1, "a tapered effect varies with the base instead of overlaying one width (brush={brush}): {widths:?}");
        } else {
            // The effect must match the pencil body, not the pen width the old overlay used: a
            // 2 mm pencil body is 0.8 × its pen width, so bands sit well under full width.
            let full = gridcraft_engine::cmd::draw::cm_to_px(2.0 / 10.0);
            assert!(
                widths.iter().all(|w| *w < full * 0.9),
                "a pencil effect paints at the pencil body width, not the pen width ({full:.1}): {widths:?}"
            );
        }
    }
}
