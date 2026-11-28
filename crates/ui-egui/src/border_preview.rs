//! Original border diagrams, painted with the same strokes as worksheet cells.

use egui::{Align2, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{BorderStyle, Borders, Theme};

use crate::grid::paint_border;
use crate::theme::{self, Tokens};

/// A labelled menu button with a two-by-two diagram to distinguish inside edges
/// from the selection outline. Labels stay available to accessibility tools.
pub(crate) fn preset_button(ui: &mut Ui, label: &str, preset: &str) -> Response {
    let response = ui.add(egui::Button::new("").frame(false).min_size(vec2(254.0, 26.0)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let t = Tokens::get(ui.ctx());
    let rect = Rect::from_center_size(pos2(response.rect.left() + 16.0, response.rect.center().y), vec2(20.0, 16.0));
    paint_preset_diagram(ui.painter(), rect, preset, t.text, t.text_disabled);
    ui.painter().text(pos2(response.rect.left() + 36.0, response.rect.center().y), Align2::LEFT_CENTER, label, theme::ui_font(12.5), t.text);
    response
}

/// Paints the remembered border preset as a compact Home ribbon icon.
pub(crate) fn paint_preset_icon(painter: &egui::Painter, rect: Rect, preset: &str, color: Color32) {
    let icon_rect = rect.shrink(1.0);
    let edges = diagram_edges(icon_rect);
    let styles = preset_styles(preset);
    let diagonal = preset_diagonal_points(icon_rect, preset);
    let has_active_border = diagonal.is_some() || styles.iter().any(|&style| style != BorderStyle::None);

    if !has_active_border {
        let muted = Tokens::get(painter.ctx()).text_dim;
        for points in edges.into_iter().take(4) {
            painter.extend(egui::Shape::dotted_line(&points, muted, 3.0, 0.45));
        }
        return;
    }

    for (points, style) in edges.into_iter().zip(styles) {
        if style != BorderStyle::None {
            paint_icon_border(painter, points, style, color, icon_rect);
        }
    }

    if let Some(points) = diagonal {
        paint_icon_border(painter, points, BorderStyle::Thin, color, icon_rect);
    }
}

/// Returns the six diagram edges in top, bottom, left, right, horizontal and vertical order.
fn diagram_edges(rect: Rect) -> [[egui::Pos2; 2]; 6] {
    [
        [rect.left_top(), rect.right_top()],
        [rect.left_bottom(), rect.right_bottom()],
        [rect.left_top(), rect.left_bottom()],
        [rect.right_top(), rect.right_bottom()],
        [rect.left_center(), rect.right_center()],
        [rect.center_top(), rect.center_bottom()],
    ]
}

/// Computes parallel line offsets separated by 2.0 px, shifted inward on outer `bounds` edges.
fn double_line_offsets(points: [egui::Pos2; 2], bounds: Rect) -> (egui::Vec2, egui::Vec2) {
    let (a, b) = (points[0], points[1]);
    let horiz = (a.y - b.y).abs() < 0.5;
    let (pos, min, max) = if horiz { (a.y, bounds.top(), bounds.bottom()) } else { (a.x, bounds.left(), bounds.right()) };
    let (d0, d1) = if pos <= min + 0.5 {
        (0.0, 2.0)
    } else if pos >= max - 0.5 {
        (-2.0, 0.0)
    } else {
        (-1.0, 1.0)
    };
    if horiz { (vec2(0.0, d0), vec2(0.0, d1)) } else { (vec2(d0, 0.0), vec2(d1, 0.0)) }
}

/// Paints a single border edge at weights tuned for the compact 14x14 ribbon icon.
fn paint_icon_border(painter: &egui::Painter, points: [egui::Pos2; 2], style: BorderStyle, color: Color32, bounds: Rect) {
    match style {
        BorderStyle::Thin => {
            painter.line_segment(points, Stroke::new(1.2, color));
        }
        BorderStyle::Thick => {
            painter.line_segment(points, Stroke::new(2.4, color));
        }
        BorderStyle::Double => {
            let (off_a, off_b) = double_line_offsets(points, bounds);
            painter.line_segment([points[0] + off_a, points[1] + off_a], Stroke::new(1.0, color));
            painter.line_segment([points[0] + off_b, points[1] + off_b], Stroke::new(1.0, color));
        }
        _ => paint_border(painter, points, style, color),
    }
}

/// Uses the same edge styles for the Borders menu and its main button icon.
fn paint_preset_diagram(painter: &egui::Painter, rect: Rect, preset: &str, color: Color32, muted: Color32) {
    let edges = diagram_edges(rect);

    for (points, style) in edges.into_iter().zip(preset_styles(preset)) {
        if style == BorderStyle::None {
            painter.extend(egui::Shape::dotted_line(&points, muted, 3.0, 0.45));
        } else {
            paint_border(painter, points, style, color);
        }
    }

    // Diagonal presets use the same stroke painter as worksheet borders.
    // Unselected diagonals are left blank rather than shown as dotted guides.
    if let Some(points) = preset_diagonal_points(rect, preset) {
        paint_border(painter, points, BorderStyle::Thin, color);
    }
}

/// Border styles in top, bottom, left, right, horizontal and vertical order.
fn preset_styles(preset: &str) -> [BorderStyle; 6] {
    use BorderStyle::{Double, None, Thick, Thin};
    match preset {
        "bottom" => [None, Thin, None, None, None, None],
        "top" => [Thin, None, None, None, None, None],
        "left" => [None, None, Thin, None, None, None],
        "right" => [None, None, None, Thin, None, None],
        "all" => [Thin; 6],
        "outside" => [Thin, Thin, Thin, Thin, None, None],
        "thickOutside" => [Thick, Thick, Thick, Thick, None, None],
        "doubleBottom" => [None, Double, None, None, None, None],
        "thickBottom" => [None, Thick, None, None, None, None],
        "topBottom" => [Thin, Thin, None, None, None, None],
        "topThickBottom" => [Thin, Thick, None, None, None, None],
        "topDoubleBottom" => [Thin, Double, None, None, None, None],
        "inside" => [None, None, None, None, Thin, Thin],
        "insideHorizontal" => [None, None, None, None, Thin, None],
        "insideVertical" => [None, None, None, None, None, Thin],
        _ => [None; 6],
    }
}

/// Returns the diagonal matching a Borders command preset, if present.
fn preset_diagonal_points(rect: Rect, preset: &str) -> Option<[egui::Pos2; 2]> {
    match preset {
        "diagonalDown" => Some([rect.left_top(), rect.right_bottom()]),
        "diagonalUp" => Some([rect.left_bottom(), rect.right_top()]),
        _ => None,
    }
}

/// Preview only reads the dialog draft. A white worksheet surface keeps automatic
/// and explicit border colors faithful in both light and dark application themes.
pub(crate) fn sample(ui: &mut Ui, borders: &Borders, theme: &Theme) {
    ui.add_space(8.0);
    ui.label("Preview");
    let (area, response) = ui.allocate_exact_size(vec2(248.0, 104.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Border preview"));
    let t = Tokens::get(ui.ctx());
    ui.painter().rect_filled(area, 4.0, t.grid_bg);
    ui.painter().rect_stroke(area, 4.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let rect = area.shrink2(vec2(30.0, 20.0));
    let edges = [
        (borders.top, [rect.left_top(), rect.right_top()]),
        (borders.bottom, [rect.left_bottom(), rect.right_bottom()]),
        (borders.left, [rect.left_top(), rect.left_bottom()]),
        (borders.right, [rect.right_top(), rect.right_bottom()]),
        (borders.diag_down, [rect.left_top(), rect.right_bottom()]),
        (borders.diag_up, [rect.left_bottom(), rect.right_top()]),
    ];
    for (line, points) in edges.iter().take(4) {
        if line.is_none() {
            ui.painter().extend(egui::Shape::dotted_line(points, Color32::from_gray(190), 4.0, 0.5));
        }
    }
    for (line, points) in edges {
        if !line.is_none() {
            let color = line.color.resolve(theme).map(theme::color32).unwrap_or(Color32::BLACK);
            paint_border(ui.painter(), points, line.style, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All Borders menu presets must have the intended edge layout.
    #[test]
    fn ribbon_border_preset_styles_match_menu() {
        use BorderStyle::{Double, None as Empty, Thick, Thin};

        let cases = [
            ("bottom", [Empty, Thin, Empty, Empty, Empty, Empty]),
            ("top", [Thin, Empty, Empty, Empty, Empty, Empty]),
            ("left", [Empty, Empty, Thin, Empty, Empty, Empty]),
            ("right", [Empty, Empty, Empty, Thin, Empty, Empty]),
            ("none", [Empty; 6]),
            ("all", [Thin; 6]),
            ("outside", [Thin, Thin, Thin, Thin, Empty, Empty]),
            ("thickOutside", [Thick, Thick, Thick, Thick, Empty, Empty]),
            ("doubleBottom", [Empty, Double, Empty, Empty, Empty, Empty]),
            ("thickBottom", [Empty, Thick, Empty, Empty, Empty, Empty]),
            ("topBottom", [Thin, Thin, Empty, Empty, Empty, Empty]),
            ("topThickBottom", [Thin, Thick, Empty, Empty, Empty, Empty]),
            ("topDoubleBottom", [Thin, Double, Empty, Empty, Empty, Empty]),
            ("inside", [Empty, Empty, Empty, Empty, Thin, Thin]),
            ("insideHorizontal", [Empty, Empty, Empty, Empty, Thin, Empty]),
            ("insideVertical", [Empty, Empty, Empty, Empty, Empty, Thin]),
        ];

        for (preset, expected) in cases {
            assert_eq!(preset_styles(preset), expected, "{preset}");
        }
    }

    /// Both diagonal presets follow the engine's down/up directions.
    #[test]
    fn ribbon_border_diagonal_presets_have_correct_endpoints() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(20.0, 16.0));

        assert_eq!(preset_diagonal_points(rect, "diagonalDown"), Some([rect.left_top(), rect.right_bottom()]));
        assert_eq!(preset_diagonal_points(rect, "diagonalUp"), Some([rect.left_bottom(), rect.right_top()]));

        // Other presets must not gain an unexpected diagonal stroke.
        assert_eq!(preset_diagonal_points(rect, "none"), None);
        assert_eq!(preset_diagonal_points(rect, "inside"), None);
    }

    struct PaintedShapes {
        lines: Vec<([egui::Pos2; 2], Stroke)>,
        circles: Vec<(egui::Pos2, Color32)>,
        total_shapes: usize,
    }

    /// Flattens painted `egui::Shape` trees into line segments and dot circles for assertions.
    fn collect_shape(shape: &egui::Shape, lines: &mut Vec<([egui::Pos2; 2], Stroke)>, circles: &mut Vec<(egui::Pos2, Color32)>) {
        match shape {
            egui::Shape::LineSegment { points, stroke } => lines.push((*points, *stroke)),
            egui::Shape::Circle(c) => circles.push((c.center, c.fill)),
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    collect_shape(s, lines, circles);
                }
            }
            _ => {}
        }
    }

    /// Runs a single offscreen `egui::Context` pass and collects the emitted shapes.
    fn render_offscreen(dark: bool, paint: impl FnOnce(&egui::Painter)) -> PaintedShapes {
        let ctx = egui::Context::default();
        ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
        let mut paint = Some(paint);
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 200.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            if let Some(f) = paint.take() {
                f(ui.painter());
            }
        });
        output.textures_delta.clear();
        let mut lines = Vec::new();
        let mut circles = Vec::new();
        for clipped in &output.shapes {
            collect_shape(&clipped.shape, &mut lines, &mut circles);
        }
        PaintedShapes { lines, circles, total_shapes: output.shapes.len() }
    }

    const REPRESENTATIVE_PRESETS: [&str; 11] = [
        "none",
        "bottom",
        "thickBottom",
        "doubleBottom",
        "outside",
        "thickOutside",
        "all",
        "insideHorizontal",
        "insideVertical",
        "diagonalDown",
        "diagonalUp",
    ];

    /// Every representative preset paints non-empty shapes cleanly in both icon and diagram modes.
    #[test]
    fn offscreen_presets_render_without_panics() {
        let icon_rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(16.0, 16.0));
        let diagram_rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 16.0));
        let active = Color32::BLACK;
        let muted = Tokens::light().text_disabled;

        for preset in REPRESENTATIVE_PRESETS {
            let icon = render_offscreen(false, |p| paint_preset_icon(p, icon_rect, preset, active));
            assert!(icon.total_shapes > 0, "{preset} icon produced no shapes");

            let diagram = render_offscreen(false, |p| paint_preset_diagram(p, diagram_rect, preset, active, muted));
            assert!(diagram.total_shapes > 0, "{preset} diagram produced no shapes");
        }
    }

    /// `"none"` in the compact ribbon icon paints only the four outer dotted perimeter edges.
    #[test]
    fn paint_preset_icon_none_draws_outer_dotted_edges_only() {
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(16.0, 16.0));
        let bounds = rect.shrink(1.0);
        for dark in [false, true] {
            let expected_muted = if dark { Tokens::dark().text_dim } else { Tokens::light().text_dim };
            let painted = render_offscreen(dark, |p| paint_preset_icon(p, rect, "none", Color32::WHITE));
            assert!(painted.lines.is_empty(), "none must not draw solid border lines");
            assert!(!painted.circles.is_empty(), "none must draw outer dotted guides");
            let (mut top_dots, mut bottom_dots, mut left_dots, mut right_dots) = (0usize, 0usize, 0usize, 0usize);
            for (center, color) in &painted.circles {
                assert_eq!(*color, expected_muted, "dotted guide uses text_dim");
                let on_left = (center.x - bounds.left()).abs() < 0.01;
                let on_right = (center.x - bounds.right()).abs() < 0.01;
                let on_top = (center.y - bounds.top()).abs() < 0.01;
                let on_bottom = (center.y - bounds.bottom()).abs() < 0.01;
                let on_perimeter = on_left || on_right || on_top || on_bottom;
                assert!(on_perimeter, "dot at {center:?} must lie on outer perimeter {bounds:?}, not interior cross");
                let strictly_horiz = center.x > bounds.left() + 0.05 && center.x < bounds.right() - 0.05;
                let strictly_vert = center.y > bounds.top() + 0.05 && center.y < bounds.bottom() - 0.05;
                if on_top && strictly_horiz {
                    top_dots += 1;
                }
                if on_bottom && strictly_horiz {
                    bottom_dots += 1;
                }
                if on_left && strictly_vert {
                    left_dots += 1;
                }
                if on_right && strictly_vert {
                    right_dots += 1;
                }
            }
            assert!(
                top_dots > 0 && bottom_dots > 0 && left_dots > 0 && right_dots > 0,
                "expected non-corner dots on all 4 edges: top={top_dots}, bottom={bottom_dots}, left={left_dots}, right={right_dots}"
            );
        }
    }

    /// Active presets omit dotted guides in `paint_preset_icon`, while `paint_preset_diagram` keeps them.
    #[test]
    fn paint_preset_icon_omits_dotted_guides_while_diagram_keeps_them() {
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(16.0, 16.0));
        let bounds = rect.shrink(1.0);
        let active = Color32::from_rgb(0x24, 0x24, 0x24);

        for preset in REPRESENTATIVE_PRESETS.into_iter().filter(|&p| p != "none") {
            let icon = render_offscreen(false, |p| paint_preset_icon(p, rect, preset, active));
            assert!(icon.circles.is_empty(), "{preset} icon must omit dotted guides, got {} dots", icon.circles.len());
            assert!(!icon.lines.is_empty(), "{preset} icon must draw active border lines");
        }

        let bottom_icon = render_offscreen(false, |p| paint_preset_icon(p, rect, "bottom", active));
        assert_eq!(bottom_icon.lines.len(), 1, "bottom icon draws exactly one line segment");
        assert_eq!(bottom_icon.lines[0].0, [bounds.left_bottom(), bounds.right_bottom()]);
        assert!((bottom_icon.lines[0].1.width - 1.2).abs() < 0.01);

        let diagram_rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 16.0));
        let bottom_diagram = render_offscreen(false, |p| paint_preset_diagram(p, diagram_rect, "bottom", active, Tokens::light().text_disabled));
        assert!(!bottom_diagram.circles.is_empty(), "menu diagram keeps dotted guides for absent edges");
        assert_eq!(bottom_diagram.lines.len(), 1);
    }

    /// Thin, thick, and double strokes have distinct widths/counts and remain inside the icon bounds.
    #[test]
    fn paint_preset_icon_distinguishes_thin_thick_and_double() {
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(16.0, 16.0));
        let bounds = rect.shrink(1.0);
        let active = Color32::BLACK;

        let thin = render_offscreen(false, |p| paint_preset_icon(p, rect, "bottom", active));
        let thick = render_offscreen(false, |p| paint_preset_icon(p, rect, "thickBottom", active));
        let double = render_offscreen(false, |p| paint_preset_icon(p, rect, "doubleBottom", active));

        assert_eq!(thin.lines.len(), 1);
        assert_eq!(thick.lines.len(), 1);
        assert_eq!(double.lines.len(), 2);

        let thin_w = thin.lines[0].1.width;
        let thick_w = thick.lines[0].1.width;
        assert!(thick_w >= thin_w * 1.8, "thick ({thick_w}) must be visibly heavier than thin ({thin_w})");

        let y0 = double.lines[0].0[0].y;
        let y1 = double.lines[1].0[0].y;
        assert!((y1 - y0).abs() >= 1.9, "double lines must stay separated: y0={y0}, y1={y1}");
        for (pts, stroke) in &double.lines {
            assert!((stroke.width - 1.0).abs() < 0.01);
            assert!(pts.iter().all(|pt| bounds.expand(0.01).contains(*pt)), "double line {pts:?} stays inside {bounds:?}");
        }

        let outside = render_offscreen(false, |p| paint_preset_icon(p, rect, "outside", active));
        let thick_outside = render_offscreen(false, |p| paint_preset_icon(p, rect, "thickOutside", active));
        assert_eq!(outside.lines.len(), 4);
        assert_eq!(thick_outside.lines.len(), 4);
        assert!(thick_outside.lines[0].1.width > outside.lines[0].1.width);
    }

    /// Inside, all, and diagonal presets draw the expected line segments in `paint_preset_icon`.
    #[test]
    fn paint_preset_icon_renders_expected_inside_and_diagonal_segments() {
        let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(16.0, 16.0));
        let bounds = rect.shrink(1.0);
        let active = Color32::BLACK;

        let horiz = render_offscreen(false, |p| paint_preset_icon(p, rect, "insideHorizontal", active));
        assert_eq!(horiz.lines.len(), 1);
        assert_eq!(horiz.lines[0].0, [bounds.left_center(), bounds.right_center()]);

        let vert = render_offscreen(false, |p| paint_preset_icon(p, rect, "insideVertical", active));
        assert_eq!(vert.lines.len(), 1);
        assert_eq!(vert.lines[0].0, [bounds.center_top(), bounds.center_bottom()]);

        let down = render_offscreen(false, |p| paint_preset_icon(p, rect, "diagonalDown", active));
        assert_eq!(down.lines.len(), 1);
        assert_eq!(down.lines[0].0, [bounds.left_top(), bounds.right_bottom()]);

        let up = render_offscreen(false, |p| paint_preset_icon(p, rect, "diagonalUp", active));
        assert_eq!(up.lines.len(), 1);
        assert_eq!(up.lines[0].0, [bounds.left_bottom(), bounds.right_top()]);

        let all = render_offscreen(false, |p| paint_preset_icon(p, rect, "all", active));
        assert_eq!(all.lines.len(), 6);
    }
}
