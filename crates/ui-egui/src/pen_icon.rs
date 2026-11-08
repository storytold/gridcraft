//! Draw-tab pen tiles: each brush's tip silhouette and a short stroke swatch, drawn in code in the
//! pen's own colour. A textured pen's effect is clipped into its tip, so a tile reads as the brush
//! it selects (original artwork; no raster assets).

use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::theme::Tokens;

/// The brush kinds of Excel's Add menu, in menu order (their `InkTool` JSON names).
pub const TOOLS: [&str; 5] = ["pen", "highlighter", "fountainPen", "brushPen", "pencil"];

/// The eight pen textures, in Excel's palette order (their `InkEffect` JSON names).
pub const EFFECTS: [&str; 8] = ["rainbow", "galaxy", "lava", "ocean", "roseGold", "gold", "silver", "bronze"];

/// A brush kind's label in the gallery and its menus.
pub fn tool_label(tool: &str) -> &'static str {
    match tool {
        "pencil" => "Pencil",
        "highlighter" => "Highlighter",
        "fountainPen" => "Fountain Pen",
        "brushPen" => "Brush Pen",
        _ => "Pen",
    }
}

/// A texture's label in the gallery and its menus.
pub fn effect_label(effect: &str) -> &'static str {
    match effect {
        "rainbow" => "Rainbow",
        "galaxy" => "Galaxy",
        "lava" => "Lava",
        "ocean" => "Ocean",
        "roseGold" => "Rose Gold",
        "gold" => "Gold",
        "silver" => "Silver",
        "bronze" => "Bronze",
        _ => "Texture",
    }
}

/// The base colour of a texture (the colour Excel stores as the stroke's `color`). The fallback is
/// the pen blue used for an unknown name.
pub fn effect_base(effect: &str) -> Color32 {
    match effect {
        "rainbow" => Color32::from_rgb(0xE8, 0x3A, 0x6B),
        "galaxy" => Color32::from_rgb(0x5B, 0x3E, 0xA8),
        "lava" => Color32::from_rgb(0xD9, 0x4F, 0x1E),
        "ocean" => Color32::from_rgb(0x1B, 0x7F, 0xB5),
        "roseGold" => Color32::from_rgb(0xC9, 0x8A, 0x7A),
        "gold" => Color32::from_rgb(0xC9, 0x9A, 0x2E),
        "silver" => Color32::from_rgb(0x9A, 0xA3, 0xAD),
        "bronze" => Color32::from_rgb(0x9A, 0x67, 0x3A),
        _ => Color32::from_rgb(0x1F, 0x5F, 0xC9),
    }
}

/// The colours a texture's bands run through, brightest first. A solid pen has only its own.
pub fn effect_colors(effect: &str) -> &'static [Color32] {
    const RAINBOW: [Color32; 5] = [
        Color32::from_rgb(0xE8, 0x3A, 0x3A),
        Color32::from_rgb(0xE8, 0xA5, 0x2E),
        Color32::from_rgb(0x4C, 0xAF, 0x4C),
        Color32::from_rgb(0x2E, 0x7F, 0xD0),
        Color32::from_rgb(0x8A, 0x4C, 0xC9),
    ];
    const GALAXY: [Color32; 4] = [
        Color32::from_rgb(0x2A, 0x1B, 0x5E),
        Color32::from_rgb(0x5B, 0x3E, 0xA8),
        Color32::from_rgb(0xA6, 0x6B, 0xD8),
        Color32::from_rgb(0xE8, 0xC7, 0xF4),
    ];
    const LAVA: [Color32; 4] = [
        Color32::from_rgb(0x5C, 0x14, 0x0C),
        Color32::from_rgb(0xB8, 0x2E, 0x14),
        Color32::from_rgb(0xE8, 0x6A, 0x1E),
        Color32::from_rgb(0xF6, 0xC0, 0x4A),
    ];
    const OCEAN: [Color32; 4] = [
        Color32::from_rgb(0x0B, 0x2F, 0x4C),
        Color32::from_rgb(0x14, 0x62, 0x8C),
        Color32::from_rgb(0x2E, 0xA3, 0xB8),
        Color32::from_rgb(0x9A, 0xE0, 0xD8),
    ];
    const ROSE_GOLD: [Color32; 3] = [Color32::from_rgb(0x8A, 0x4A, 0x44), Color32::from_rgb(0xC9, 0x8A, 0x7A), Color32::from_rgb(0xF0, 0xC9, 0xB0)];
    const GOLD: [Color32; 3] = [Color32::from_rgb(0x7A, 0x55, 0x12), Color32::from_rgb(0xC9, 0x9A, 0x2E), Color32::from_rgb(0xF2, 0xD5, 0x7A)];
    const SILVER: [Color32; 3] = [Color32::from_rgb(0x5E, 0x66, 0x6E), Color32::from_rgb(0x9A, 0xA3, 0xAD), Color32::from_rgb(0xE2, 0xE7, 0xEC)];
    const BRONZE: [Color32; 3] = [Color32::from_rgb(0x5A, 0x38, 0x1C), Color32::from_rgb(0x9A, 0x67, 0x3A), Color32::from_rgb(0xD2, 0xA3, 0x6E)];
    match effect {
        "rainbow" => &RAINBOW,
        "galaxy" => &GALAXY,
        "lava" => &LAVA,
        "ocean" => &OCEAN,
        "roseGold" => &ROSE_GOLD,
        "gold" => &GOLD,
        "silver" => &SILVER,
        "bronze" => &BRONZE,
        _ => &[],
    }
}

/// The colour a textured swatch shows at its centre (its base when unnamed).
pub fn effect_mid(effect: &str) -> Color32 {
    let bands = effect_colors(effect);
    bands.get(bands.len() / 2).copied().unwrap_or_else(|| effect_base(effect))
}

fn lerp(a: Pos2, b: Pos2, t: f32) -> Pos2 {
    pos2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// Paints the tip glyph of `tool` in `rect`: a round blob for a pen, a flat narrow point for a
/// pencil, a wide translucent chisel for a highlighter, a nib for a fountain pen and a bristle fan
/// for a brush pen. A textured pen gets the effect's bands clipped into the tip.
pub fn paint_tip(p: &Painter, rect: Rect, tool: &str, color: Color32, effect: Option<&str>) {
    let s = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    match tool {
        "pencil" => {
            // A narrow hexagonal barrel with a flat, slightly angled lead.
            let body = Rect::from_center_size(r.center(), vec2(s * 0.34, s * 0.82));
            p.rect_filled(body, s * 0.06, color);
            p.rect_stroke(body, s * 0.06, Stroke::new(1.0, color.gamma_multiply(0.55)), egui::StrokeKind::Inside);
            let lead = Rect::from_center_size(pos2(r.center().x, r.bottom() - s * 0.18), vec2(s * 0.34, s * 0.26));
            p.add(egui::Shape::convex_polygon(
                vec![lead.left_top(), lead.right_top(), pos2(lead.right(), lead.bottom() - s * 0.02), pos2(lead.left(), lead.bottom() - s * 0.02)],
                Color32::from_gray(60),
                Stroke::NONE,
            ));
            p.line_segment(
                [pos2(body.center().x, body.top() + s * 0.1), pos2(body.center().x, body.bottom() - s * 0.22)],
                Stroke::new(1.0, Color32::from_white_alpha(70)),
            );
        }
        "highlighter" => {
            // A wide, translucent chisel tip.
            let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 150);
            let body = Rect::from_center_size(r.center(), vec2(s * 0.78, s * 0.34));
            p.rect_filled(body, s * 0.05, fill);
            p.rect_stroke(body, s * 0.05, Stroke::new(1.0, color.gamma_multiply(0.7)), egui::StrokeKind::Inside);
            let lip = Rect::from_min_size(pos2(body.left(), body.bottom() - s * 0.06), vec2(body.width(), s * 0.14));
            p.add(egui::Shape::convex_polygon(
                vec![lip.left_top(), lip.right_top(), pos2(lip.right() - s * 0.08, lip.bottom()), pos2(lip.left(), lip.bottom() - s * 0.04)],
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 210),
                Stroke::NONE,
            ));
        }
        "fountainPen" => {
            // A nib: a gold-edged triangle with a slit and breather hole.
            let top = pos2(r.center().x, r.top() + s * 0.08);
            let left = pos2(r.center().x - s * 0.30, r.bottom() - s * 0.12);
            let right = pos2(r.center().x + s * 0.30, r.bottom() - s * 0.12);
            p.add(egui::Shape::convex_polygon(vec![top, right, left], color, Stroke::new(1.0, color.gamma_multiply(0.5))));
            p.line_segment(
                [pos2(r.center().x, r.top() + s * 0.2), pos2(r.center().x, r.bottom() - s * 0.2)],
                Stroke::new(1.2, Color32::from_gray(235)),
            );
            p.circle_filled(pos2(r.center().x, r.top() + s * 0.34), s * 0.05, Color32::from_gray(235));
        }
        "brushPen" => {
            // A bristle fan: a rounded triangle with separate hairs.
            let top = pos2(r.center().x, r.top() + s * 0.1);
            let left = pos2(r.center().x - s * 0.30, r.bottom() - s * 0.1);
            let right = pos2(r.center().x + s * 0.30, r.bottom() - s * 0.1);
            p.add(egui::Shape::convex_polygon(vec![top, right, left], color, Stroke::new(1.0, color.gamma_multiply(0.5))));
            for i in 0..4 {
                let t = 0.2 + i as f32 * 0.2;
                p.line_segment([lerp(top, left, t), lerp(right, left, t)], Stroke::new(0.8, color.gamma_multiply(0.65)));
            }
            p.circle_filled(lerp(top, pos2(r.center().x, r.bottom() - s * 0.1), 0.35), s * 0.10, color.gamma_multiply(1.2));
        }
        _ => {
            // A round pen tip with a pressed core.
            p.circle_filled(r.center(), s * 0.32, color);
            p.circle_filled(r.center(), s * 0.16, color.gamma_multiply(0.65));
            p.circle_stroke(r.center(), s * 0.32, Stroke::new(1.0, color.gamma_multiply(0.5)));
        }
    }
    if let Some(effect) = effect {
        paint_texture(p, r, tool, effect);
    }
}

/// Clips an effect's bands into the tip's silhouette, so a textured pen reads as textured.
fn paint_texture(p: &Painter, r: Rect, tool: &str, effect: &str) {
    let bands = effect_colors(effect);
    if bands.is_empty() {
        return;
    }
    let clip = match tool {
        "pencil" => Rect::from_center_size(r.center(), vec2(r.width() * 0.34, r.height() * 0.82)),
        "highlighter" => Rect::from_center_size(r.center(), vec2(r.width() * 0.78, r.height() * 0.34)),
        "fountainPen" | "brushPen" => Rect::from_min_max(
            pos2(r.left() + r.width() * 0.2, r.top() + r.height() * 0.1),
            pos2(r.right() - r.width() * 0.2, r.bottom() - r.height() * 0.1),
        ),
        _ => Rect::from_center_size(r.center(), vec2(r.width() * 0.64, r.height() * 0.64)),
    };
    let cp = p.with_clip_rect(clip.intersect(p.clip_rect()));
    let n = bands.len();
    let stripe = clip.height() / n as f32;
    for (i, band) in bands.iter().enumerate() {
        let y = clip.top() + stripe * i as f32;
        // Diagonal bands read as texture rather than a plain gradient.
        let skew = clip.width() * 0.25;
        cp.add(egui::Shape::convex_polygon(
            vec![
                pos2(clip.left(), y),
                pos2(clip.right(), y),
                pos2(clip.right() - skew, y + stripe * 1.4),
                pos2(clip.left() - skew, y + stripe * 1.4),
            ],
            Color32::from_rgba_unmultiplied(band.r(), band.g(), band.b(), 220),
            Stroke::NONE,
        ));
    }
}

/// Paints a short stroke swatch for `tool` in `rect`: a round line for a pen, a flat one for a
/// pencil, a wide translucent band for a highlighter, a pressure-tapered stroke for the pens.
/// A textured swatch carries the effect's bands along the stroke.
pub fn paint_swatch(p: &Painter, rect: Rect, tool: &str, color: Color32, effect: Option<&str>) {
    let (w, h) = (rect.width(), rect.height());
    let y = rect.center().y;
    let (x0, x1) = (rect.left() + w * 0.06, rect.right() - w * 0.06);
    let thick = match tool {
        "highlighter" => (h * 0.55).max(4.0),
        "pencil" => (h * 0.22).max(1.5),
        _ => (h * 0.34).max(2.0),
    };
    let bands = effect.map(effect_colors).unwrap_or(&[]);
    let color_at = |t: f32| -> Color32 {
        if bands.is_empty() {
            return color;
        }
        let i = ((t * bands.len() as f32) as usize).min(bands.len() - 1);
        bands.get(i).copied().unwrap_or(color)
    };
    let press = |t: f32| -> f32 {
        match tool {
            "fountainPen" | "brushPen" => 0.35 + 0.65 * (std::f32::consts::PI * t).sin(),
            "pencil" | "highlighter" => 1.0,
            _ => 0.7 + 0.3 * (std::f32::consts::PI * t).sin(),
        }
    };
    let alpha = if tool == "highlighter" { 120 } else { 255 };
    let steps = 20;
    let curve = |t: f32| -> Pos2 { pos2(x0 + (x1 - x0) * t, y - (std::f32::consts::PI * t).sin() * h * 0.16) };
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let tm = (t0 + t1) / 2.0;
        let c = color_at(tm);
        let c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha);
        let width = (thick * press(tm)).max(1.0);
        p.line_segment([curve(t0), curve(t1)], Stroke::new(width, c));
    }
}

/// One pen tile of the Drawing Tools gallery: the tool's tip and stroke in its own colour, an
/// accent frame when selected, and a chevron at the foot that opens the pen's dropdown.
pub struct PenTile<'a> {
    /// The gallery pen's id, which keys the tile's widgets.
    pub id: u32,
    pub tool: &'a str,
    pub color: Color32,
    pub effect: Option<&'a str>,
    pub tip: &'a str,
    pub selected: bool,
}

/// Paints a pen tile and returns its (body, dropdown arrow) responses. The body selects the pen;
/// the arrow opens its dropdown.
pub fn pen_tile(ui: &mut Ui, tile: PenTile<'_>) -> (Response, Response) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(38.0, 72.0), Sense::hover());
    let split = rect.bottom() - 13.0;
    let body = Rect::from_min_max(rect.min, pos2(rect.right(), split));
    let arrow = Rect::from_min_max(pos2(rect.left(), split), rect.max);
    let id = ui.scope_id().with(("pen_tile", tile.id));
    let main = ui.interact(body, id.with("body"), Sense::click());
    let chevron = ui.interact(arrow, id.with("arrow"), Sense::click());
    if main.hovered() || chevron.hovered() {
        ui.painter().rect_filled(rect, 5.0, t.hover);
    }
    if main.is_pointer_button_down_on() {
        ui.painter().rect_filled(body, 5.0, t.pressed);
    }
    if chevron.is_pointer_button_down_on() {
        ui.painter().rect_filled(arrow, 5.0, t.pressed);
    }
    let p = ui.painter();
    paint_tip(p, Rect::from_center_size(pos2(rect.center().x, rect.top() + 22.0), vec2(28.0, 28.0)), tile.tool, tile.color, tile.effect);
    paint_swatch(p, Rect::from_center_size(pos2(rect.center().x, rect.top() + 44.0), vec2(28.0, 12.0)), tile.tool, tile.color, tile.effect);
    crate::icons::paint(p, Rect::from_center_size(arrow.center(), vec2(10.0, 10.0)), crate::icons::Icon::Chevron, t.text_dim);
    if tile.selected {
        p.rect_stroke(rect, 5.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
    }
    // Each tile is its own pen ("Galaxy pen", "Red pen", …), so the tool's generic name would
    // leave the six default pens and every custom one indistinguishable to a screen reader.
    let label = tile.tip;
    main.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    chevron.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), format!("{label} options")));
    let tip = tile.tip.to_string();
    (main.on_hover_text(tip.clone()), chevron.on_hover_text(tip))
}

/// The effect swatch of the 8 textured colours: a small tile with the texture's bands.
pub fn paint_effect_swatch(p: &Painter, rect: Rect, effect: &str) {
    let bands = effect_colors(effect);
    if bands.is_empty() {
        p.rect_filled(rect, 2.0, effect_base(effect));
        return;
    }
    let n = bands.len();
    let stripe = rect.height() / n as f32;
    for (i, band) in bands.iter().enumerate() {
        let r = Rect::from_min_max(pos2(rect.left(), rect.top() + stripe * i as f32), pos2(rect.right(), rect.top() + stripe * (i + 1) as f32 + 0.5));
        p.rect_filled(r, 0.0, *band);
    }
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, Color32::from_gray(160)), StrokeKind::Inside);
}
