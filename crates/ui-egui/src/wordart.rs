//! Insert › Text › WordArt: the style gallery. Previews are drawn in code from the engine's
//! preset palette (`gridcraft_engine::cmd::wordart::STYLES`); picking one runs `insert.wordArt`.

use egui::{Align2, Color32, FontId, Painter, Rect, Sense, Ui, vec2};
use gridcraft_engine::cmd::wordart::{STYLES, WordArtStyle};
use gridcraft_engine::model::Theme;
use serde_json::json;

use crate::{SheetApp, theme};

/// Paints "Abc" in `style` centred in `r` (fill, outline and shadow as the sheet object has them).
pub(crate) fn paint_sample(p: &Painter, r: Rect, style: &WordArtStyle, theme: &Theme) {
    let px = 24.0;
    let family = theme::cell_family("", style.bold, style.italic);
    let font = FontId::new(px, family);
    let c = r.center();
    let col = |c: gridcraft_engine::model::Color| c.resolve(theme).map(theme::color32).unwrap_or(Color32::BLACK);
    if let Some(sh) = style.shadow {
        p.text(c + vec2(1.5, 1.5), Align2::CENTER_CENTER, "Abc", font.clone(), col(sh));
    }
    if let Some((oc, w)) = style.outline {
        let d = (w * 96.0 / 72.0 / 2.0).clamp(0.5, 3.0);
        for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
            p.text(c + vec2(dx * d, dy * d), Align2::CENTER_CENTER, "Abc", font.clone(), col(oc));
        }
    }
    p.text(c, Align2::CENTER_CENTER, "Abc", font, col(style.fill));
}

/// The gallery popup content: two rows of previews; a click inserts that style.
pub(crate) fn gallery(app: &mut SheetApp, ui: &mut Ui) {
    let theme = app.session.active().map(|d| d.wb.theme.clone()).unwrap_or_default();
    ui.set_width(5.0 * 72.0);
    egui::Grid::new("wordart_gallery").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        for (i, style) in STYLES.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(68.0, 44.0), Sense::click());
            let t = theme::Tokens::get(ui.ctx());
            ui.painter().rect_filled(r, 4.0, if resp.hovered() { t.hover } else { Color32::WHITE });
            ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(1.0, t.input_border), egui::StrokeKind::Inside);
            paint_sample(&ui.painter().with_clip_rect(r), r, style, &theme);
            if resp.on_hover_text(style.name).clicked() {
                app.run_or_alert("insert.wordArt", json!({"style": style.name}));
                ui.close();
            }
            if (i + 1) % 5 == 0 {
                ui.end_row();
            }
        }
    });
}
