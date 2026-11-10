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

/// One painted line segment: its width and colour in screen pixels, and its length.
struct Seg {
    width: f32,
    color: egui::Color32,
    len: f32,
}

/// The line segments painted for the sheet, with their width and colour.
fn painted_segments(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<Seg> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::LineSegment { points, stroke } => Some(Seg { width: stroke.width, color: stroke.color, len: (points[1] - points[0]).length() }),
            _ => None,
        })
        .collect()
}

fn harness_for(stroke: serde_json::Value) -> egui_kittest::Harness<'static, SheetApp> {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("draw.stroke", stroke).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

#[test]
fn every_brush_branch_paints_with_its_own_colour_and_width() {
    use gridcraft_engine::model::InkTool;
    let px = gridcraft_engine::cmd::draw::cm_to_px;
    let width_of = |brush: &str, mm: f32| {
        let tool = gridcraft_engine::cmd::draw::parse_tool(brush).unwrap_or(InkTool::Pen);
        let (w, h) = gridcraft_engine::cmd::draw::ink_size_cm(tool, mm);
        (px(w), px(h))
    };
    let same_rgb = |c: &egui::Color32, b: &egui::Color32| c.r() == b.r() && c.g() == b.g() && c.b() == b.b();
    // The zoom the harness paints at, from the painted length of a stroke drawn 160 sheet points
    // along. Every expected width below is in sheet points, so it is scaled by this once.
    let zoom = |segs: &[Seg], sheet_len: f32| segs.iter().map(|s| s.len).sum::<f32>() / sheet_len;

    // A solid pencil: one segment per point pair, flat, at 0.8x its width and 215 alpha.
    let h =
        harness_for(json!({"points": [[40, 40], [80, 40], [120, 40], [160, 40], [200, 40]], "brush": "pencil", "width": 2.0, "color": "#333333"}));
    let segs: Vec<Seg> = painted_segments(&h).into_iter().filter(|s| s.color.a() == 215).collect();
    assert!(segs.len() >= 4, "a pencil paints per-segment: {}", segs.len());
    let z = zoom(&segs, 160.0);
    assert!(z > 0.0);
    let (w, _) = width_of("pencil", 2.0);
    for seg in &segs {
        assert!((seg.width - (w * 0.8).max(1.0) * z).abs() < 0.02, "pencil width {} vs {}", seg.width, (w * 0.8).max(1.0) * z);
    }

    // A solid fountain pen: pressure-tapered, so the segments vary and none is wider than the brush.
    let h = harness_for(
        json!({"points": [[40, 40], [80, 40], [120, 40], [160, 40], [200, 40]], "brush": "fountainPen", "width": 2.0, "color": "#1F3864"}),
    );
    let nib = egui::Color32::from_rgb(0x1F, 0x38, 0x64);
    let segs: Vec<Seg> = painted_segments(&h).into_iter().filter(|s| same_rgb(&s.color, &nib)).collect();
    assert!(segs.len() >= 4, "a tapered stroke is drawn per segment: {}", segs.len());
    let z = zoom(&segs, 160.0);
    let (fw, _) = width_of("fountainPen", 2.0);
    assert!(segs.iter().all(|s| s.width <= fw * z + 0.01), "no segment is wider than the nib");
    assert!(segs.iter().any(|s| s.width < fw * z - 0.01), "the ends taper");

    // A textured highlighter: translucent bands, not the pen colour.
    let h = harness_for(
        json!({"points": [[40, 40], [80, 40], [120, 40], [160, 40], [200, 40]], "brush": "highlighter", "width": 4.0, "effect": "ocean"}),
    );
    let bands = gridcraft_ui_egui::pen_icon::effect_colors("ocean");
    // egui stores Colour32 premultiplied, so a translucent band is compared at its own alpha.
    let tinted = |b: &egui::Color32| egui::Color32::from_rgba_unmultiplied(b.r(), b.g(), b.b(), 115);
    let segs: Vec<Seg> = painted_segments(&h).into_iter().filter(|s| s.color.a() == 115 && bands.iter().any(|b| tinted(b) == s.color)).collect();
    assert!(segs.len() >= 4, "a textured band is drawn per segment: {}", segs.len());
    let z = zoom(&segs, 160.0);
    let (_, hh) = width_of("highlighter", 4.0);
    assert!(segs.iter().all(|s| (s.width - hh.max(2.0) * z).abs() < 0.02), "a highlighter is `height_cm` across");

    // A textured brush pen and a textured pen: the texture's bands, opaque, in the brush's width.
    for (brush, mm, effect) in [("brushPen", 1.0, "lava"), ("pen", 1.0, "gold")] {
        let h = harness_for(json!({"points": [[40, 40], [80, 40], [120, 40], [160, 40], [200, 40]], "brush": brush, "width": mm, "effect": effect}));
        let bands = gridcraft_ui_egui::pen_icon::effect_colors(effect);
        let segs: Vec<Seg> = painted_segments(&h).into_iter().filter(|s| s.color.a() == 255 && bands.iter().any(|b| same_rgb(b, &s.color))).collect();
        assert!(!segs.is_empty(), "{brush} paints bands");
        let z = zoom(&segs, 160.0);
        let (bw, _) = width_of(brush, mm);
        assert!(segs.iter().all(|s| s.width <= bw * z + 0.01), "{brush}: bands are no wider than its brush");
    }
}

#[test]
fn a_one_point_trace_paints_a_dot() {
    // An InkML trace with a single point exists in the model; it used to be skipped entirely.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("draw.stroke", json!({"points": [[40, 40], [200, 40]], "width": 1.0})).unwrap();
    // Give the stroke one point, as an imported single-point trace would.
    {
        let d = s.doc_mut().unwrap();
        let mut wb = (*d.wb).clone();
        let sh = wb.sheet_mut(0).unwrap();
        sh.shapes[0].text = "0,0".into();
        d.wb = std::sync::Arc::new(wb);
    }
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    assert!(
        h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Circle(c) if c.radius > 0.0 && c.fill != egui::Color32::TRANSPARENT)),
        "a one-point stroke paints a dab"
    );
}
