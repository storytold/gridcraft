use gridcraft_engine::brush::{Preset, Style};
use gridcraft_engine::model::ObjKind;

use super::controls::{act, control_keytip};
use super::{Icon, SheetApp, Stroke, StrokeKind, Tokens, Ui, big_split_button, json};

const PRESETS: [(Preset, &str); 4] =
    [(Preset::HardRound, "Hard round"), (Preset::SoftRound, "Soft round"), (Preset::Chalk, "Chalk"), (Preset::Highlighter, "Highlighter")];

pub(super) fn controls(app: &mut SheetApp, ui: &mut Ui) {
    let (main, arrow) = big_split_button(ui, Icon::Pen, "PhotoCraft\nBrush", "Draw with the PhotoCraft brush engine");
    control_keytip(app, ui, &main, "draw.brush");
    if app.session.draw_tool == "brush" {
        ui.painter().rect_stroke(main.rect.union(arrow.rect), 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if main.clicked() {
        act(app, "draw.brush", json!({}));
    }
    egui::Popup::menu(&arrow).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| options(app, ui));
}

fn options(app: &mut SheetApp, ui: &mut Ui) {
    ui.set_min_width(215.0);
    let mut style = app.session.draw_brush;
    // A second memory-backed popup would replace this menu in egui.
    ui.horizontal_wrapped(|ui| {
        for (preset, label) in PRESETS {
            if ui.selectable_label(style.preset == preset, label).clicked() && style.preset != preset {
                style = Style::for_preset(preset);
            }
        }
    });
    ui.collapsing("Color", |ui| {
        let [r, g, b] = style.color;
        let mut color = egui::Color32::from_rgb(r, g, b);
        if egui::color_picker::color_picker_color32(ui, &mut color, egui::color_picker::Alpha::Opaque) {
            style.color = [color.r(), color.g(), color.b()];
        }
    });
    ui.add(egui::Slider::new(&mut style.size, 1.0..=64.0).text("Size"));
    ui.add(egui::Slider::new(&mut style.hardness, 0.0..=1.0).text("Hardness"));
    ui.add(egui::Slider::new(&mut style.opacity, 0.01..=1.0).text("Opacity"));
    ui.add(egui::Slider::new(&mut style.flow, 0.01..=1.0).text("Flow"));
    ui.add(egui::Slider::new(&mut style.smoothing, 0.0..=1.0).text("Smoothing"));
    ui.checkbox(&mut style.pressure_size, "Pressure controls size");
    if style != app.session.draw_brush {
        // Keep the object selection mode while adjusting the settings for a selected stroke.
        let on = app.session.draw_tool == "brush";
        act(app, "draw.brush", json!({"on": on, "style": style}));
    }
    let selected = app.selected_object.filter(|o| o.kind == ObjKind::Image).and_then(|o| {
        app.session
            .active()?
            .wb
            .active()?
            .images
            .iter()
            .find(|pic| pic.id == o.id && gridcraft_engine::brush::has_source(&pic.data))
            .map(|pic| pic.id)
    });
    if let Some(id) = selected {
        ui.separator();
        if ui.button("Apply to selected stroke").clicked() {
            act(app, "draw.brushEdit", json!({"id": id, "style": style}));
            ui.close();
        }
    }
}
