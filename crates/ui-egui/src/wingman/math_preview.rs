//! Read-only transcript equations; no shape, workbook command, link or TeX interpreter is created.
use crate::{equation, theme};
use egui::{Color32, Role, Sense, TextStyle, Ui, WidgetInfo, vec2};
use gridcraft_engine::math::{self, Face, Item, MathList};
use std::{collections::HashMap, sync::Arc};

type Layout = Result<Arc<MathList>, &'static str>;
const TOO_LARGE: &str = "This equation is too large to draw.";

// One entry per distinct source, shared by inline and display math. Cache failures too: a
// malformed math block elsewhere in the chat must not restart a parser on every repaint.
#[derive(Default)]
struct Layouts {
    background: Option<Color32>,
    entries: HashMap<Box<str>, (bool, Layout)>,
}

impl egui::cache::CacheTrait for Layouts {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn update(&mut self) {
        self.entries.retain(|_, (used, _)| std::mem::take(used));
    }
}

impl Layouts {
    fn get(&mut self, src: &str, bg: Color32) -> Layout {
        if src.len() > math::MAX_SOURCE_BYTES {
            return Err(TOO_LARGE);
        }
        if self.background != Some(bg) {
            self.entries.clear();
            self.background = Some(bg);
        }
        if let Some((used, result)) = self.entries.get_mut(src) {
            *used = true;
            return result.clone();
        }
        if self.entries.len() >= 128 {
            return Err("Too many equations to preview.");
        }
        let result = prepare(src, bg).map(Arc::new);
        self.entries.insert(src.into(), (true, result.clone()));
        result
    }
}

fn prepare(src: &str, bg: Color32) -> Result<MathList, &'static str> {
    // This is the workbook's guarded layout entry point (size, nesting, macro and matrix caps).
    let mut list = math::layout(src).map_err(|_| "This equation could not be laid out.")?;
    if list.items.is_empty() || list.width <= 0.0 || list.total_height() <= 0.0 {
        return Err("This equation has no visible content.");
    }
    // A workbook object can be inspected at any zoom. A chat pane is much smaller: bound the
    // glyph atlas and polygon work separately, including source that hides a large glyph's box.
    if list.items.len() > 1024 {
        return Err(TOO_LARGE);
    }
    let mut commands = 0;
    for item in &mut list.items {
        let ink = match item {
            Item::Glyph(g) => {
                let size = if matches!(g.face, Face::Large(_)) { g.height + g.depth } else { g.size };
                if size > 12.0 {
                    return Err(TOO_LARGE);
                }
                &mut g.color
            }
            Item::Rule(rule) => &mut rule.color,
            Item::Path(path) => {
                commands += path.commands.len();
                if path.commands.len() > 192 || commands > 2048 {
                    return Err(TOO_LARGE);
                }
                &mut path.color
            }
        };
        if ink.is_some_and(|[r, g, b]| theme::contrast(Color32::from_rgb(r, g, b), bg) < 4.5) {
            *ink = None; // Fall back to the theme's text colour, including in high contrast.
        }
    }
    Ok(list)
}

/// Display math occupies its own row, centred at the transcript's text scale.
pub(super) fn show(ui: &mut Ui, src: &str) -> Result<(), &'static str> {
    draw(ui, src, true)
}

/// A natural-width, read-only widget for an inline run in a horizontal wrapping layout.
pub(super) fn inline(ui: &mut Ui, src: &str) -> Result<(), &'static str> {
    draw(ui, src, false)
}

fn draw(ui: &mut Ui, src: &str, display: bool) -> Result<(), &'static str> {
    let bg = ui.visuals().panel_fill;
    let list = ui.memory_mut(|m| m.caches.cache::<Layouts>().get(src, bg))?;
    let body = TextStyle::Body.resolve(ui.style()).size;
    let em = body * if display { 1.25 } else { 1.0 };
    let pad = body * if display { 0.5 } else { 0.12 };
    let available = (if display { ui.available_width() } else { ui.max_rect().width() }).min(body * 100.0);
    let height_limit = body * if display { 14.0 } else { 5.0 };
    let scale = em.min((available - 2.0 * pad) / list.width).min((height_limit - 2.0 * pad) / list.total_height());
    // A long line must never expand the dock, nor appear to succeed as a strip of unreadable dots.
    if scale < body * 0.8 {
        return Err("This equation does not fit in the preview.");
    }
    let ink_size = vec2(list.width, list.total_height()) * scale;
    let width = if display { available } else { (ink_size.x + 2.0 * pad).min(available) };
    let height = (ink_size.y + 2.0 * pad).max(ui.text_style_height(&TextStyle::Body));
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Math, true, src));
    response.on_hover_text(src);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, bg);
        equation::paint_at(&painter, rect.center() - ink_size / 2.0, &list, ui.visuals().text_color(), scale);
    }
    Ok(())
}
