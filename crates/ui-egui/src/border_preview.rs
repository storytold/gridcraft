//! Original border diagrams, painted with the same strokes as worksheet cells.

use egui::{Align2, Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{BorderStyle, Borders, Theme};

use crate::grid::paint_border;
use crate::theme::{self, Tokens};

/// A labelled menu button with a two-by-two diagram to distinguish inside edges
/// from the selection outline. Labels stay available to accessibility tools.
pub(crate) fn preset_button(ui: &mut Ui, label: &str, preset: &str) -> Response {
    let response = ui.add(egui::Button::new("").frame(false).min_size(vec2(254.0, 26.0)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    let t = Tokens::get(ui.ctx());
    let rect = Rect::from_center_size(pos2(response.rect.left() + 16.0, response.rect.center().y), vec2(20.0, 16.0));
    let edges = [
        [rect.left_top(), rect.right_top()],
        [rect.left_bottom(), rect.right_bottom()],
        [rect.left_top(), rect.left_bottom()],
        [rect.right_top(), rect.right_bottom()],
        [rect.left_center(), rect.right_center()],
        [rect.center_top(), rect.center_bottom()],
    ];
    use BorderStyle::{Double, None, Thick, Thin};
    let styles = match preset {
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
        _ => [None; 6],
    };
    for (points, style) in edges.into_iter().zip(styles) {
        if style == None {
            ui.painter().extend(egui::Shape::dotted_line(&points, t.text_disabled, 3.0, 0.45));
        } else {
            paint_border(ui.painter(), points, style, t.text);
        }
    }
    ui.painter().text(pos2(response.rect.left() + 36.0, response.rect.center().y), Align2::LEFT_CENTER, label, theme::ui_font(12.5), t.text);
    response
}

/// Preview only reads the dialog draft. A white worksheet surface keeps automatic
/// and explicit border colors faithful in both light and dark application themes.
pub(crate) fn sample(ui: &mut Ui, borders: &Borders, theme: &Theme) {
    ui.add_space(8.0);
    ui.label("Preview");
    let (area, response) = ui.allocate_exact_size(vec2(248.0, 104.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Border preview"));
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
