//! Cell text placed where Excel puts it (GDI on Windows).
//!
//! egui's line is the `hhea` ascent + descent + line gap, with the gap under the text and no room
//! above the ascender. GDI's line is the OS/2 win ascent + win descent, whose extra room is above
//! the ascender. Placed by the galley's height, cell text sat about 3 px high in a 20 px row
//! (Calibri 11 at 100%). [`text_y`] places the first baseline by the win metrics instead.

use std::cell::RefCell;
use std::collections::HashMap;

use egui::{FontDefinitions, FontFamily, FontId, Galley, Rect};
use gridcraft_engine::model::VAlign;

/// A font's win ascent and win descent, in points.
#[derive(Clone, Copy, Debug)]
pub(super) struct Win {
    pub ascent: f32,
    pub descent: f32,
}

/// The win metrics of the first face of `font`'s family (`None` when the face has no OS/2 table).
pub(super) fn win_metrics(ctx: &egui::Context, font: &FontId) -> Option<Win> {
    thread_local! {
        static PER_EM: RefCell<HashMap<FontFamily, Option<(f32, f32)>>> = RefCell::new(HashMap::new());
    }
    let cached = PER_EM.with(|c| c.borrow().get(&font.family).copied());
    let per_em = cached.unwrap_or_else(|| {
        let v = ctx.fonts(|f| win_per_em(f.definitions(), &font.family));
        PER_EM.with(|c| c.borrow_mut().insert(font.family.clone(), v));
        v
    });
    per_em.map(|(a, d)| Win { ascent: a * font.size, descent: d * font.size })
}

/// The family's first face's win ascent and descent as fractions of the font size.
pub(super) fn win_per_em(defs: &FontDefinitions, family: &FontFamily) -> Option<(f32, f32)> {
    use skrifa::raw::TableProvider;
    let fd = defs.families.get(family).and_then(|keys| keys.first()).and_then(|k| defs.font_data.get(k))?;
    let font = skrifa::FontRef::from_index((*fd.font).as_ref(), fd.index).ok()?;
    let upem = font.head().ok()?.units_per_em() as f32;
    let os2 = font.os2().ok()?;
    if upem == 0.0 || os2.us_win_ascent() == 0 {
        return None;
    }
    let k = fd.tweak.scale / upem;
    Some((os2.us_win_ascent() as f32 * k, os2.us_win_descent() as f32 * k))
}

/// How far below its top a galley's first baseline sits.
pub(super) fn first_baseline(galley: &Galley) -> Option<f32> {
    let row = galley.rows.first()?;
    row.row.glyphs.first().map(|g| row.pos.y + g.pos.y)
}

/// The top of `galley` (the cell text in `font`) placed by [`text_y`], or `None` when the font has
/// no win metrics or the galley no glyph, for the caller's own placement.
pub(super) fn gdi_text_y(ctx: &egui::Context, font: &FontId, galley: &Galley, valign: VAlign, rect: Rect, z: f32) -> Option<f32> {
    let win = win_metrics(ctx, font)?;
    let ascent = first_baseline(galley)?;
    Some(text_y(valign, rect, galley.rows.len(), ascent, win, z))
}

/// The top of a galley of `lines` lines, whose first baseline is `ascent` below its top, put where
/// GDI puts the same text in `rect`: a block of `lines` win lines at the top, the middle, or half
/// a pixel above the bottom.
pub(super) fn text_y(valign: VAlign, rect: Rect, lines: usize, ascent: f32, win: Win, z: f32) -> f32 {
    let block = lines.max(1) as f32 * (win.ascent + win.descent);
    let top = match valign {
        VAlign::Top => rect.top(),
        VAlign::Center | VAlign::Justify | VAlign::Distributed => rect.center().y - block / 2.0,
        VAlign::Bottom => rect.bottom() - 0.5 * z - block,
    };
    top + win.ascent - ascent
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{FontData, pos2};
    use std::sync::Arc;

    // Calibri 11 at 100%: egui ascent 11, win ascent 13.96 and win descent 3.94 (points).
    const WIN: Win = Win { ascent: 1950.0 / 2048.0 * 44.0 / 3.0, descent: 550.0 / 2048.0 * 44.0 / 3.0 };
    const ASCENT: f32 = 11.0;

    fn baseline(v: VAlign, rect: Rect) -> f32 {
        text_y(v, rect, 1, ASCENT, WIN, 1.0) + ASCENT
    }

    #[test]
    fn an_unknown_family_has_no_win_metrics() {
        assert_eq!(win_per_em(&FontDefinitions::empty(), &FontFamily::Name("nope".into())), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn calibri_win_metrics_come_from_its_os2_table() {
        let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\calibri.ttf") else { return };
        let mut defs = FontDefinitions::empty();
        defs.font_data.insert("calibri".into(), Arc::new(FontData::from_owned(bytes)));
        defs.families.insert(FontFamily::Name("c".into()), vec!["calibri".into()]);
        let (a, d) = win_per_em(&defs, &FontFamily::Name("c".into())).unwrap();
        assert!((a - 1950.0 / 2048.0).abs() < 1e-5 && (d - 550.0 / 2048.0).abs() < 1e-5, "{a} {d}");
    }

    #[test]
    fn the_baseline_sits_where_gdi_puts_it_in_a_20_px_row() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 20.0));
        // Bottom: the win descent and half a pixel above the bottom.
        assert!((rect.bottom() - baseline(VAlign::Bottom, rect) - (WIN.descent + 0.5)).abs() < 1e-4);
        // Center: the win line in the middle, so the baseline is about 5 px above the bottom.
        let center = baseline(VAlign::Center, rect);
        assert!((center - (10.0 - (WIN.ascent + WIN.descent) / 2.0 + WIN.ascent)).abs() < 1e-4);
        assert!((rect.bottom() - center - 5.0).abs() < 0.1, "{center}");
        // Top: the win ascent below the top.
        assert!((baseline(VAlign::Top, rect) - WIN.ascent).abs() < 1e-4);
    }

    #[test]
    fn wrapped_lines_center_as_one_block() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 60.0));
        let one = text_y(VAlign::Center, rect, 1, ASCENT, WIN, 1.0);
        let two = text_y(VAlign::Center, rect, 2, ASCENT, WIN, 1.0);
        assert!((one - two - (WIN.ascent + WIN.descent) / 2.0).abs() < 1e-4);
    }
}
