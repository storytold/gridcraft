//! Every icon paints at both ribbon sizes in both themes, and its drawing stays inside its box.
//!
//! The set is walked through [`icons::ALL`]; a variant added to the enum without a place there
//! would go unpainted, so the paint and name tests both start from that list.

use egui::{RawInput, Rect, Shape, pos2, vec2};
use gridcraft_ui_egui::icons::{self, Icon};
use gridcraft_ui_egui::theme;

/// A context with the deterministic bundled font set and GridCraft's visuals installed.
fn context(dark: bool) -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(theme::pinned_font_definitions());
    theme::apply(&ctx, dark);
    let mut first = ctx.run_ui(RawInput::default(), |_ui| {});
    first.textures_delta.clear(); // nothing consumes the atlas in a headless test
    ctx
}

/// Paints `icon` into a `size`×`size` box at (40, 40) and returns the shapes it produced.
fn painted(ctx: &egui::Context, icon: Icon, size: f32) -> Vec<Shape> {
    let rect = Rect::from_min_size(pos2(40.0, 40.0), vec2(size, size));
    let mut output = ctx.run_ui(RawInput::default(), |ui| {
        icons::paint(ui.painter(), rect, icon, theme::Tokens::get(ui.ctx()).text);
    });
    output.textures_delta.clear();
    output.shapes.into_iter().map(|shape| shape.shape).collect()
}

#[test]
fn every_icon_paints_at_16_and_32_px_in_both_themes() {
    for dark in [false, true] {
        let ctx = context(dark);
        for size in [16.0, 32.0] {
            for &icon in icons::ALL {
                let shapes = painted(&ctx, icon, size);
                assert!(!shapes.is_empty(), "{icon:?} painted nothing at {size} px (dark: {dark})");
                for shape in &shapes {
                    let b = shape.visual_bounding_rect();
                    assert!(b.is_finite(), "{icon:?} painted a non-finite shape at {size} px (dark: {dark}): {shape:?}");
                }
            }
        }
    }
}

#[test]
fn every_icon_stays_inside_its_box() {
    // The box's strokes are inside its edges and the widest stroke is 2.3 px at 32 px, so one
    // pixel of slack covers antialiasing without hiding a drawing that bleeds out of the grid.
    for dark in [false, true] {
        let ctx = context(dark);
        for size in [16.0, 32.0] {
            let rect = Rect::from_min_size(pos2(40.0, 40.0), vec2(size, size));
            for &icon in icons::ALL {
                for shape in painted(&ctx, icon, size) {
                    let b = shape.visual_bounding_rect();
                    if b == Rect::NOTHING {
                        continue;
                    }
                    assert!(rect.expand(1.0).contains_rect(b), "{icon:?} paints outside its {size} px box (dark: {dark}): {b:?} not within {rect:?}");
                }
            }
        }
    }
}

#[test]
fn every_icon_paints_something_at_both_sizes() {
    // A second, coarser frame check: the whole set in one pass, so a dropped variant cannot hide
    // behind a neighbour's shapes.
    for dark in [false, true] {
        let ctx = context(dark);
        for size in [16.0, 32.0] {
            let mut output = ctx.run_ui(RawInput::default(), |ui| {
                for (i, &icon) in icons::ALL.iter().enumerate() {
                    let x = 8.0 + (i % 16) as f32 * (size + 6.0);
                    let y = 8.0 + (i / 16) as f32 * (size + 6.0);
                    let rect = Rect::from_min_size(pos2(x, y), vec2(size, size));
                    icons::paint(ui.painter(), rect, icon, theme::Tokens::get(ui.ctx()).text);
                }
            });
            output.textures_delta.clear();
            let shapes: Vec<Shape> = output.shapes.into_iter().map(|shape| shape.shape).collect();
            assert!(shapes.len() >= icons::ALL.len(), "dark: {dark}, {size} px: {} shapes for {} icons", shapes.len(), icons::ALL.len());
        }
    }
}

#[test]
fn every_icon_is_listed_once() {
    let mut seen = std::collections::HashSet::new();
    for &icon in icons::ALL {
        assert!(seen.insert(icon), "{icon:?} is listed twice in icons::ALL");
    }
    // Debug formatting is the only handle on a variant besides its position, so the count also
    // catches a duplicate that a debug name would hide.
    let names: std::collections::HashSet<String> = icons::ALL.iter().map(|icon| format!("{icon:?}")).collect();
    assert_eq!(names.len(), icons::ALL.len(), "two variants share a name");
}
