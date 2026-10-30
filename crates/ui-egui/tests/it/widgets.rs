//! Ribbon widget paint tests, using egui's bundled fonts on every platform.

use egui::{FontDefinitions, FontFamily, Rect, Shape, pos2, vec2};
use gridcraft_ui_egui::{icons::Icon, theme, widgets};

fn render_big_button(label: &str, dropdown: bool) -> (Rect, Vec<Shape>) {
    let ctx = egui::Context::default();
    let mut fonts = FontDefinitions::default();
    let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.insert(FontFamily::Name(theme::UI.into()), proportional);
    ctx.set_fonts(fonts);
    let mut rect = Rect::NOTHING;
    let mut output = ctx
        .run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 200.0))), ..Default::default() }, |ui| {
            rect = widgets::big_button(ui, Icon::CondFormat, label, "test button", dropdown).rect
        });
    output.textures_delta.clear();
    (rect, output.shapes.into_iter().map(|shape| shape.shape).collect())
}

#[test]
fn big_dropdown_paints_no_caret_and_centres_its_label() {
    // Buttons with a menu show it on click; the ribbon draws no caret beside their labels.
    for label in ["Paste", "Conditional\nFormatting", "Longer first line\nGo"] {
        let (rect, shapes) = render_big_button(label, true);
        let texts: Vec<_> = shapes.iter().filter_map(|shape| if let Shape::Text(text) = shape { Some(text) } else { None }).collect();
        assert_eq!(texts.iter().map(|text| text.galley.job.text.as_str()).collect::<Vec<_>>(), label.split('\n').collect::<Vec<_>>());
        assert!(
            !shapes.iter().any(|shape| matches!(shape, Shape::Path(path) if path.points.iter().all(|p| p.y > rect.top() + 30.0))),
            "no caret under the icon"
        );
        for text in texts {
            let r = text.galley.rect.translate(text.pos.to_vec2());
            assert!((r.center().x - rect.center().x).abs() < 0.5, "labels are centred");
        }
    }
}

#[test]
fn big_button_without_dropdown_keeps_centered_labels_and_width() {
    for label in ["Paste", "Conditional\nFormatting"] {
        let (rect, shapes) = render_big_button(label, false);
        assert!(
            !shapes.iter().any(|shape| matches!(shape, Shape::Path(path) if path.points.iter().all(|p| p.y > rect.top() + 36.0))),
            "plain button must not paint an arrow"
        );
        let mut widest: f32 = 0.0;
        for shape in shapes {
            if let Shape::Text(text) = shape {
                let text_rect = text.galley.rect.translate(text.pos.to_vec2());
                assert!((text_rect.center().x - rect.center().x).abs() < 0.01);
                widest = widest.max(text_rect.width());
            }
        }
        assert!((rect.width() - (widest + 12.0).max(48.0)).abs() < 0.01);
    }
}
