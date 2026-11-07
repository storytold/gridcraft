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
            rect = widgets::big_button(ui, Icon::More, label, "test button", dropdown).rect
        });
    output.textures_delta.clear();
    (rect, output.shapes.into_iter().map(|shape| shape.shape).collect())
}

#[test]
fn big_dropdown_paints_chevron_beside_final_label_line() {
    for label in ["Paste", "Conditional\nFormatting", "Longer first line\nGo"] {
        let (rect, shapes) = render_big_button(label, true);
        let texts: Vec<_> = shapes.iter().filter_map(|shape| if let Shape::Text(text) = shape { Some(text) } else { None }).collect();
        assert_eq!(texts.iter().map(|text| text.galley.job.text.as_str()).collect::<Vec<_>>(), label.split('\n').collect::<Vec<_>>());
        let paths: Vec<_> = shapes.iter().filter_map(|shape| if let Shape::Path(path) = shape { Some(path) } else { None }).collect();
        assert_eq!(paths.len(), 1, "dropdown indicator must be painted independently of font glyphs");
        let chevron = paths[0];
        assert_eq!(chevron.points.len(), 3);
        assert!(chevron.points[0].x < chevron.points[1].x && chevron.points[1].x < chevron.points[2].x);
        assert!(chevron.points[1].y > chevron.points[0].y && chevron.points[1].y > chevron.points[2].y);
        let last = texts.last().unwrap();
        let last_rect = last.galley.rect.translate(last.pos.to_vec2());
        assert!(chevron.points[0].x > last_rect.right(), "arrow must not overlap the label");
        assert!((chevron.points[1].y - last_rect.center().y).abs() < 3.0, "arrow must align with the final line");
        assert!(rect.right() - chevron.points[2].x >= 6.0, "button must reserve space for the arrow and padding");
        assert!(last_rect.left() - rect.left() >= 6.0);
        assert!(((last_rect.left() + chevron.points[2].x) / 2.0 - rect.center().x).abs() < 2.0, "label and arrow must be centered together");
        for text in texts.iter().take(texts.len() - 1) {
            let text_rect = text.galley.rect.translate(text.pos.to_vec2());
            assert!((text_rect.center().x - rect.center().x).abs() < 0.01, "earlier lines remain centered");
        }
    }
}

#[test]
fn big_button_without_dropdown_keeps_centered_labels_and_width() {
    for label in ["Paste", "Conditional\nFormatting"] {
        let (rect, shapes) = render_big_button(label, false);
        assert!(!shapes.iter().any(|shape| matches!(shape, Shape::Path(_))), "plain button must not paint an arrow");
        let mut widest: f32 = 0.0;
        for shape in shapes {
            if let Shape::Text(text) = shape {
                let text_rect = text.galley.rect.translate(text.pos.to_vec2());
                assert!((text_rect.center().x - rect.center().x).abs() < 0.01);
                widest = widest.max(text_rect.width());
            }
        }
        assert!((rect.width() - (widest + 12.0).max(44.0)).abs() < 0.01);
    }
}
