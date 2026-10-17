//! Right-to-left text (Hebrew, Arabic) in cells and text fields, in Unicode bidirectional display
//! order (UAX #9).
//!
//! egui shapes each run of one font face in the direction of that run's script and places the runs
//! left to right in logical order; `LayoutJob::append` also merges a section into the previous one
//! of the same format. So with plain egui calls:
//! - a Hebrew cell drawn by one face (Arial, Segoe UI, David) is reversed as a whole, digits and
//!   Latin words included: "קובץ 2026" showed as "6202 ץבוק";
//! - a cell whose letters and spaces come from different faces (the Hebrew fallback face and the
//!   cell font) showed its words left to right, in the wrong order;
//! - in a text field the caret sat as if the line ran left to right, so Backspace after a Hebrew
//!   word deleted the space on its other side.
//!
//! [`layout`] lays text out one section per stretch of one face and one direction
//! ([`editor_pieces`]), already in the order that makes egui's shaping come out right. Every
//! character is laid out exactly once, and the glyphs are put back into reading order at their
//! screen positions ([`to_logical_order`], with GridCraft's epaint patch), so the caret, clicks,
//! Backspace and Delete follow the text the user sees. [`singleline`] and [`multiline`] are
//! `TextEdit`s with that layout; in a field reading right to left the Left and Right arrows move
//! the way they point, as in Excel ([`rtl_arrows`]).

use std::collections::HashMap;
use std::sync::Arc;

use unicode_bidi::{BidiInfo, Level};

/// One stretch of a line as egui must get it (see [`editor_pieces`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The characters, in the order egui's shaper must receive them.
    pub text: String,
    /// The index of the format they are drawn with.
    pub format: usize,
    /// The line's character indices this piece shows, in screen order (left to right).
    pub shown: Vec<usize>,
    /// The piece's characters read right to left (an odd embedding level).
    pub rtl: bool,
}

/// A line (logical order, one format index per character) as the stretches egui must lay out, one
/// section each and in this order, so that it shows the Unicode bidirectional display order. Each
/// piece holds one face (`face(format, c)` names it) at one embedding level, already in the order
/// that makes the shaper's reversal come out right; a bracket is mirrored where the shaper won't.
///
/// Every character is kept exactly once and in no other piece (bidi controls stay, undrawn), so the
/// galley has one glyph per character. `None` for a line without right-to-left script, which egui
/// already draws right. `rtl` is the paragraph direction when one is known (a right-to-left
/// sheet); `None` takes it from the first strong character.
pub fn editor_pieces(line: &[(char, usize)], rtl: Option<bool>, mut face: impl FnMut(usize, char) -> usize) -> Option<Vec<Piece>> {
    if !line.iter().any(|(c, _)| is_rtl_script(*c)) {
        return None;
    }
    let text: String = line.iter().map(|(c, _)| *c).collect();
    // The logical index of the character starting at each byte.
    let mut at_byte = vec![usize::MAX; text.len()];
    for (i, (byte, _)) in text.char_indices().enumerate() {
        if let Some(slot) = at_byte.get_mut(byte) {
            *slot = i;
        }
    }
    let info = BidiInfo::new(&text, rtl.map(|r| if r { Level::rtl() } else { Level::ltr() }));
    let mut key = |i: usize| line.get(i).map(|&(c, f)| (f, face(f, c)));
    let mut pieces = Vec::new();
    for para in &info.paragraphs {
        // unicode-bidi indexes the first level of a line, so an empty one would panic.
        if para.range.is_empty() {
            continue;
        }
        let (levels, runs) = info.visual_runs(para, para.range.clone());
        for run in runs {
            let odd = levels.get(run.start).is_some_and(Level::is_rtl);
            let Some(piece) = text.get(run.clone()) else { continue };
            let mut shown: Vec<usize> = piece.char_indices().filter_map(|(b, _)| at_byte.get(run.start + b).copied()).collect();
            if odd {
                shown.reverse();
            }
            // Split the run where the format or the face changes: egui starts a new run there.
            let mut start = 0;
            while start < shown.len() {
                let first = shown.get(start).and_then(|&i| key(i));
                let mut end = start + 1;
                while end < shown.len() && shown.get(end).and_then(|&i| key(i)) == first {
                    end += 1;
                }
                let indices = shown.get(start..end).unwrap_or_default().to_vec();
                let chars: Vec<char> = indices.iter().filter_map(|&i| line.get(i).map(|p| p.0)).collect();
                let format = first.map_or(0, |(f, _)| f);
                pieces.push(Piece { text: for_shaper(&chars, odd), format, shown: indices, rtl: odd });
                start = end;
            }
        }
    }
    Some(pieces)
}

/// Appends one [`Piece`] to `job` as a section of its own. `LayoutJob::append` merges a section
/// into the previous one of the same format, which would hand two pieces to the shaper as one run.
pub fn append_piece(job: &mut egui::text::LayoutJob, text: &str, format: egui::TextFormat) {
    let start = job.text.len();
    job.text.push_str(text);
    let range = egui::text::ByteIndex(start)..egui::text::ByteIndex(job.text.len());
    job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: range, format });
}

/// One line of a laid-out text whose glyphs [`to_logical_order`] puts back into reading order: the
/// line's first character (in the whole text) and, for each glyph on screen from left to right,
/// the character it shows and whether that character reads right to left.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineOrder {
    pub start: usize,
    pub shown: Vec<(usize, bool)>,
}

/// Puts the glyphs of each row laid out from [`editor_pieces`] back into reading order, each at its
/// place on screen and marked right to left where it reads so, so egui's caret and clicks follow
/// the text (`Glyph::rtl`, GridCraft's epaint patch). `text` is the laid-out text in logical order.
/// A line egui wrapped onto several rows keeps drawing order.
pub fn to_logical_order(galley: &mut Arc<egui::Galley>, text: &str, lines: &[LineOrder]) {
    if lines.is_empty() {
        return;
    }
    let chars: Vec<char> = text.chars().collect();
    let g = Arc::make_mut(galley);
    let mut row_start = 0usize;
    for placed in &mut g.rows {
        let count = placed.row.glyphs.len();
        let this_start = row_start;
        row_start = row_start.saturating_add(count).saturating_add(usize::from(placed.ends_with_newline));
        let Some(line) = lines.iter().find(|l| l.start == this_start && l.shown.len() == count) else { continue };
        let row = Arc::make_mut(&mut placed.row);
        let mut logical: Vec<Option<egui::epaint::text::Glyph>> = vec![None; count];
        for (glyph, &(index, rtl)) in row.glyphs.iter().zip(&line.shown) {
            let Some(slot) = index.checked_sub(line.start).and_then(|i| logical.get_mut(i)) else { return };
            let mut glyph = *glyph;
            glyph.rtl = rtl;
            if let Some(&c) = chars.get(index) {
                glyph.chr = c;
            }
            *slot = Some(glyph);
        }
        // Only a complete permutation replaces the row; anything else keeps drawing order.
        if let Some(glyphs) = logical.into_iter().collect::<Option<Vec<_>>>() {
            row.glyphs = glyphs;
        }
    }
}

/// Appends `sections` (logical order) to `job`: a line holding right-to-left script in its display
/// order (see [`editor_pieces`]), other lines as they are. Every character is appended once.
/// Returns the screen order of each right-to-left line, for [`to_logical_order`].
pub fn append_in_display_order(
    ctx: &egui::Context,
    job: &mut egui::text::LayoutJob,
    sections: &[(&str, egui::TextFormat)],
    direction: Option<bool>,
) -> Vec<LineOrder> {
    let push = |job: &mut egui::text::LayoutJob, s: &str, format: usize| {
        if let Some((_, f)) = sections.get(format) {
            job.append(s, 0.0, f.clone());
        }
    };
    if !sections.iter().any(|(s, _)| has_rtl(s)) {
        for (i, (s, _)) in sections.iter().enumerate() {
            push(job, s, i);
        }
        return Vec::new();
    }
    let chars: Vec<(char, usize)> = sections.iter().enumerate().flat_map(|(i, (s, _))| s.chars().map(move |c| (c, i))).collect();
    let faces = faces_of(ctx, sections, &chars);
    let mut orders = Vec::new();
    let mut offset = 0usize;
    for line in chars.split_inclusive(|(c, _)| *c == '\n') {
        let start = offset;
        offset = offset.saturating_add(line.len());
        let (body, newline) = match line.split_last() {
            Some((last, body)) if last.0 == '\n' => (body, Some(last.1)),
            _ => (line, None),
        };
        match editor_pieces(body, direction, |f, c| faces.get(&(f, c)).copied().unwrap_or(usize::MAX)) {
            Some(pieces) => {
                let shown = pieces.iter().flat_map(|p| p.shown.iter().map(move |&i| (start.saturating_add(i), p.rtl))).collect();
                orders.push(LineOrder { start, shown });
                for piece in pieces {
                    if let Some((_, format)) = sections.get(piece.format) {
                        append_piece(job, &piece.text, format.clone());
                    }
                }
            }
            None => {
                // Unchanged: consecutive characters of one format stay one section.
                for group in body.chunk_by(|a, b| a.1 == b.1) {
                    let s: String = group.iter().map(|p| p.0).collect();
                    push(job, &s, group.first().map_or(0, |p| p.1));
                }
            }
        }
        if let Some(format) = newline {
            push(job, "\n", format);
        }
    }
    orders
}

/// Lays out `sections` (logical order) with the settings of `job` (wrap width, alignment; its own
/// text and sections are replaced): right-to-left lines in display order, the glyphs in reading
/// order. `direction` as for [`editor_pieces`].
pub fn layout(
    ctx: &egui::Context,
    mut job: egui::text::LayoutJob,
    sections: &[(&str, egui::TextFormat)],
    direction: Option<bool>,
) -> Arc<egui::Galley> {
    job.text.clear();
    job.sections.clear();
    let orders = append_in_display_order(ctx, &mut job, sections, direction);
    let mut galley = ctx.fonts_mut(|f| f.layout_job(job));
    if !orders.is_empty() {
        let text: String = sections.iter().map(|(s, _)| *s).collect();
        to_logical_order(&mut galley, &text, &orders);
    }
    galley
}

/// One line of `text` in one font and colour, as `Painter::layout_no_wrap` lays it out but in
/// display order.
pub fn layout_no_wrap(ctx: &egui::Context, text: &str, font: egui::FontId, color: egui::Color32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    layout(ctx, job, &[(text, egui::TextFormat::simple(font, color))], None)
}

/// `Painter::text` with right-to-left script in display order: one line of `text` anchored at
/// `pos`. Returns where it was painted.
pub fn paint_text(p: &egui::Painter, pos: egui::Pos2, anchor: egui::Align2, text: &str, font: egui::FontId, color: egui::Color32) -> egui::Rect {
    let galley = layout_no_wrap(p.ctx(), text, font, color);
    let rect = anchor.anchor_size(pos, galley.size());
    p.galley(rect.min, galley, color);
    rect
}

/// Cell text in one font and colour, for painting: `wrap` is the wrap width (`None`: one line per
/// line of the text) and `halign` the alignment of the rows. Text with right-to-left script breaks
/// into rows in reading order (where egui wraps the logical text, so a long Hebrew cell starts on
/// its first row) and each row is then put in display order with its paragraph's direction.
pub fn layout_cell(
    ctx: &egui::Context,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    wrap: Option<f32>,
    halign: egui::Align,
) -> Arc<egui::Galley> {
    let format = egui::TextFormat::simple(font, color);
    let mut job = egui::text::LayoutJob { halign, ..Default::default() };
    job.wrap.max_width = wrap.unwrap_or(f32::INFINITY);
    if !has_rtl(text) {
        job.append(text, 0.0, format);
        return ctx.fonts_mut(|f| f.layout_job(job));
    }
    let rows = match wrap {
        Some(width) => wrap_words(text, width, |s| ctx.fonts_mut(|f| f.layout_no_wrap(s.to_string(), format.font_id.clone(), color).size().x)),
        None => text.split('\n').map(|l| (l.to_string(), paragraph_rtl(l))).collect(),
    };
    // Each row is laid out as its own line, in its paragraph's direction.
    job.wrap.max_width = f32::INFINITY;
    for (i, (row, rtl)) in rows.iter().enumerate() {
        if i > 0 {
            job.append("\n", 0.0, format.clone());
        }
        let line: Vec<(char, usize)> = row.chars().map(|c| (c, 0)).collect();
        let faces = faces_of(ctx, &[(row.as_str(), format.clone())], &line);
        match editor_pieces(&line, *rtl, |f, c| faces.get(&(f, c)).copied().unwrap_or(usize::MAX)) {
            Some(pieces) => pieces.iter().for_each(|p| append_piece(&mut job, &p.text, format.clone())),
            None => job.append(row, 0.0, format.clone()),
        }
    }
    ctx.fonts_mut(|f| f.layout_job(job))
}

/// One line of `text` in `format` in display order, with the paragraph direction `direction`
/// (`None`: from the first strong character).
pub fn line(ctx: &egui::Context, text: &str, format: egui::TextFormat, direction: Option<bool>) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    job.break_on_newline = false;
    layout(ctx, job, &[(text, format)], direction)
}

/// A file name or path as the interface shows it, as a file manager does: the folders and the
/// name's stem each read in their own direction (a Hebrew or Arabic one right to left, numbers and
/// Latin words in their places inside it), in order from the left, and the separators and the
/// extension stay where they are, so "קובץ (1).docx" shows its words from the right with "(1)"
/// at their left and ".docx" at the end. Plain egui reversed a name starting with a Hebrew letter
/// as a whole: it showed as "xcod.(1) ץבוק". For painting (the glyphs stay in display order).
pub fn file_name(ctx: &egui::Context, text: &str, font: egui::FontId, color: egui::Color32) -> Arc<egui::Galley> {
    let format = egui::TextFormat::simple(font, color);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    job.break_on_newline = false;
    if !has_rtl(text) {
        job.append(text, 0.0, format);
        return ctx.fonts_mut(|f| f.layout_job(job));
    }
    let chars: Vec<(char, usize)> = text.chars().map(|c| (c, 0)).collect();
    let faces = faces_of(ctx, &[(text, format.clone())], &chars);
    for (part, isolate) in path_parts(text) {
        let line: Vec<(char, usize)> = part.chars().map(|c| (c, 0)).collect();
        let pieces = if isolate { editor_pieces(&line, None, |f, c| faces.get(&(f, c)).copied().unwrap_or(usize::MAX)) } else { None };
        match pieces {
            Some(pieces) => pieces.iter().for_each(|p| append_piece(&mut job, &p.text, format.clone())),
            None => append_piece(&mut job, part, format.clone()),
        }
    }
    ctx.fonts_mut(|f| f.layout_job(job))
}

/// `text` (a path or file name) as its parts in order, each marked when it reads in its own
/// direction: folder names and the file's stem are, the separators and a short extension are not.
fn path_parts(text: &str) -> Vec<(&str, bool)> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(['/', '\\']) {
        if i > 0 {
            parts.push((rest.get(..i).unwrap_or_default(), true));
        }
        parts.push((rest.get(i..i + 1).unwrap_or_default(), false));
        rest = rest.get(i + 1..).unwrap_or_default();
    }
    // The extension: a dot and up to eight ASCII letters or digits at the end.
    match rest.rfind('.') {
        Some(dot)
            if dot > 0
                && rest.len() - dot <= 9
                && rest.get(dot + 1..).is_some_and(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric())) =>
        {
            parts.push((rest.get(..dot).unwrap_or_default(), true));
            parts.push((rest.get(dot..).unwrap_or_default(), false));
        }
        _ if !rest.is_empty() => parts.push((rest, true)),
        _ => {}
    }
    parts
}

/// A window title: the file name `name` as [`file_name`] shows it, then `suffix` (" •",
/// " — Edited") after it, centred on `center`. Returns where it was painted.
pub fn paint_title(p: &egui::Painter, center: egui::Pos2, name: &str, suffix: &str, font: egui::FontId, color: egui::Color32) -> egui::Rect {
    let name_g = file_name(p.ctx(), name, font.clone(), color);
    let suffix_g = p.ctx().fonts_mut(|f| f.layout_no_wrap(suffix.to_string(), font, color));
    let size = egui::vec2(name_g.size().x + suffix_g.size().x, name_g.size().y.max(suffix_g.size().y));
    let rect = egui::Align2::CENTER_CENTER.anchor_size(center, size);
    let suffix_x = rect.min.x + name_g.size().x;
    p.galley(rect.min, name_g, color);
    p.galley(egui::pos2(suffix_x, rect.min.y), suffix_g, color);
    rect
}

/// [`file_name`] cut with "…" to `max_width` when it is wider (its end goes first).
pub fn file_name_truncated(ctx: &egui::Context, text: &str, font: egui::FontId, color: egui::Color32, max_width: f32) -> Arc<egui::Galley> {
    let whole = file_name(ctx, text, font.clone(), color);
    if whole.size().x <= max_width || !max_width.is_finite() {
        return whole;
    }
    let chars: Vec<char> = text.chars().collect();
    let with_ellipsis = |n: usize| {
        let mut s: String = chars.get(..n).unwrap_or_default().iter().collect::<String>().trim_end().to_string();
        s.push('…');
        s
    };
    let (mut fits, mut too_wide) = (0usize, chars.len());
    while too_wide - fits > 1 {
        let mid = fits + (too_wide - fits) / 2;
        if file_name(ctx, &with_ellipsis(mid), font.clone(), color).size().x <= max_width {
            fits = mid;
        } else {
            too_wide = mid;
        }
    }
    file_name(ctx, &with_ellipsis(fits), font, color)
}

/// `Painter::text` for a file name or path ([`file_name`]). Returns where it was painted.
pub fn paint_file_name(p: &egui::Painter, pos: egui::Pos2, anchor: egui::Align2, text: &str, font: egui::FontId, color: egui::Color32) -> egui::Rect {
    let galley = file_name(p.ctx(), text, font, color);
    let rect = anchor.anchor_size(pos, galley.size());
    p.galley(rect.min, galley, color);
    rect
}

/// One line of `text` in `format`, cut with "…" in reading order to `max_width` when it is wider:
/// the longest start of the text that fits with the ellipsis, laid out in display order with the
/// paragraph direction `direction` (`None`: from the first strong character; `Some(false)` for a
/// file name, see [`file_name`]). egui's own elision cuts the reordered text, so a Hebrew name lost
/// its start and kept its end.
pub fn truncated(ctx: &egui::Context, text: &str, format: egui::TextFormat, max_width: f32, direction: Option<bool>) -> Arc<egui::Galley> {
    let whole = line(ctx, text, format.clone(), direction);
    if whole.size().x <= max_width || !max_width.is_finite() {
        return whole;
    }
    let chars: Vec<char> = text.chars().collect();
    let with_ellipsis = |n: usize| {
        let mut s: String = chars.get(..n).unwrap_or_default().iter().collect::<String>().trim_end().to_string();
        s.push('…');
        s
    };
    // The longest prefix that fits, by bisection (at least nothing but the ellipsis).
    let (mut fits, mut too_wide) = (0usize, chars.len());
    while too_wide - fits > 1 {
        let mid = fits + (too_wide - fits) / 2;
        if line(ctx, &with_ellipsis(mid), format.clone(), direction).size().x <= max_width {
            fits = mid;
        } else {
            too_wide = mid;
        }
    }
    line(ctx, &with_ellipsis(fits), format, direction)
}

/// One paragraph (no line break inside) of a multi-line field, as egui's `TextEdit` must get it:
/// `sections` (logical order) wrapped to `width` between words in reading order ([`wrap_ranges`];
/// egui's own wrapping looks for spaces in glyph order, reversed in a right-to-left run, and broke
/// Hebrew words in the middle), each row in display order with the paragraph's direction
/// (`direction`, else its first strong character) and its glyphs in reading order, so the caret,
/// clicks and Backspace follow the text on wrapped rows too. A right-to-left paragraph's rows sit
/// against the right edge (`width`). `None` for a paragraph without right-to-left script that
/// isn't set right to left: egui lays that out right itself.
pub fn paragraph(ctx: &egui::Context, sections: &[(&str, egui::TextFormat)], width: f32, direction: Option<bool>) -> Option<Arc<egui::Galley>> {
    let text: String = sections.iter().map(|(s, _)| *s).collect();
    let rtl = direction.or_else(|| paragraph_rtl(&text));
    if !has_rtl(&text) && rtl != Some(true) {
        return None;
    }
    // Each character's section.
    let owner: Vec<usize> = sections.iter().enumerate().flat_map(|(i, (s, _))| s.chars().map(move |_| i)).collect();
    let chars: Vec<char> = text.chars().collect();
    // The sections covering the characters `range`, clipped to it.
    let clipped = |range: std::ops::Range<usize>| -> Vec<(String, egui::TextFormat)> {
        let mut out: Vec<(String, egui::TextFormat)> = Vec::new();
        let mut last = usize::MAX;
        for i in range {
            let (Some(&c), Some(&f)) = (chars.get(i), owner.get(i)) else { continue };
            match out.last_mut() {
                Some((s, _)) if f == last => s.push(c),
                _ => {
                    if let Some((_, format)) = sections.get(f) {
                        out.push((c.to_string(), format.clone()));
                        last = f;
                    }
                }
            }
        }
        out
    };
    let measure = |range: std::ops::Range<usize>| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        for (s, format) in clipped(range) {
            job.append(&s, 0.0, format);
        }
        ctx.fonts_mut(|f| f.layout_job(job)).size().x
    };
    let rows: Vec<Arc<egui::Galley>> = wrap_ranges(&chars, width, measure)
        .into_iter()
        .map(|range| {
            let parts = clipped(range);
            let refs: Vec<(&str, egui::TextFormat)> = parts.iter().map(|(s, f)| (s.as_str(), f.clone())).collect();
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = f32::INFINITY;
            layout(ctx, job, &refs, rtl)
        })
        .collect();
    let mut whole = egui::text::LayoutJob::default();
    whole.wrap.max_width = width;
    for (s, format) in sections {
        whole.append(s, 0.0, format.clone());
    }
    let mut galley = egui::Galley::concat(Arc::new(whole), &rows, ctx.pixels_per_point());
    // The rows are one paragraph: soft breaks, not line breaks. A right-to-left one's rows end at
    // the right edge.
    let mut rect = egui::Rect::NOTHING;
    let mut mesh = egui::Rect::NOTHING;
    for placed in &mut galley.rows {
        placed.ends_with_newline = false;
        // The text ends at the right edge; a row's trailing space, at its left end in a
        // right-to-left row, may hang past the left edge, as in a word processor.
        if rtl == Some(true) && width.is_finite() {
            placed.pos.x = width - placed.row.size.x;
        }
        rect |= egui::Rect::from_min_size(placed.pos, placed.row.size);
        mesh |= placed.row.visuals.mesh_bounds.translate(placed.pos.to_vec2());
    }
    if !galley.rows.is_empty() {
        galley.rect = rect;
        galley.mesh_bounds = mesh;
    }
    Some(Arc::new(galley))
}

/// A paragraph's characters broken into rows (character ranges, every character in exactly one)
/// that `measure` finds no wider than `width`. Rows break after a space, in reading order; a word
/// wider than a row breaks between characters. `measure` gets a row without its trailing spaces.
pub fn wrap_ranges(chars: &[char], width: f32, mut measure: impl FnMut(std::ops::Range<usize>) -> f32) -> Vec<std::ops::Range<usize>> {
    let trimmed = |r: std::ops::Range<usize>| {
        let mut end = r.end;
        while end > r.start && chars.get(end - 1) == Some(&' ') {
            end -= 1;
        }
        r.start..end
    };
    // Words: each ends after its spaces.
    let mut words: Vec<std::ops::Range<usize>> = Vec::new();
    let mut start = 0;
    for i in 0..chars.len() {
        if chars.get(i) == Some(&' ') && chars.get(i + 1).is_some_and(|c| *c != ' ') {
            words.push(start..i + 1);
            start = i + 1;
        }
    }
    words.push(start..chars.len());
    let mut rows = Vec::new();
    let mut line = 0..0;
    for word in words {
        if line.is_empty() || measure(trimmed(line.start..word.end)) <= width {
            line = line.start..word.end;
        } else {
            rows.push(line.clone());
            line = word;
        }
        // A word wider than a row: the longest prefix that fits (at least one character) per row,
        // found by bisection so a long word costs few measurements.
        while line.len() > 1 && measure(trimmed(line.clone())) > width {
            let (mut fits, mut too_wide) = (1usize, line.len());
            while too_wide - fits > 1 {
                let mid = fits + (too_wide - fits) / 2;
                if measure(trimmed(line.start..line.start + mid)) <= width {
                    fits = mid;
                } else {
                    too_wide = mid;
                }
            }
            rows.push(line.start..line.start + fits);
            line = line.start + fits..line.end;
        }
    }
    rows.push(line);
    rows
}

/// `text` broken into rows that `measure` finds no wider than `width`, each with its paragraph's
/// direction (see [`wrap_ranges`]). egui's own wrapping can't be used for right-to-left text: it
/// looks for spaces in glyph order, which is reversed in a right-to-left run, and broke Hebrew
/// words in the middle.
pub fn wrap_words(text: &str, width: f32, mut measure: impl FnMut(&str) -> f32) -> Vec<(String, Option<bool>)> {
    let mut rows = Vec::new();
    for paragraph in text.split('\n') {
        let rtl = paragraph_rtl(paragraph);
        let chars: Vec<char> = paragraph.chars().collect();
        let piece = |r: std::ops::Range<usize>| chars.get(r).unwrap_or_default().iter().collect::<String>();
        for range in wrap_ranges(&chars, width, |r| measure(&piece(r))) {
            rows.push((piece(range), rtl));
        }
    }
    rows
}

/// The direction of a paragraph: that of its first strong character (UAX #9 rules P2 and P3),
/// `None` without one.
fn paragraph_rtl(text: &str) -> Option<bool> {
    text.chars().find_map(|c| if is_rtl_script(c) && c.is_alphabetic() { Some(true) } else { c.is_alphabetic().then_some(false) })
}

/// Which face of its format's font family egui draws each of `chars` with (the family's first font
/// that maps it, as egui picks it; `usize::MAX` for none), keyed by format and character.
fn faces_of(ctx: &egui::Context, sections: &[(&str, egui::TextFormat)], chars: &[(char, usize)]) -> HashMap<(usize, char), usize> {
    use skrifa::MetadataProvider as _;
    ctx.fonts(|fonts| {
        let defs = fonts.definitions();
        let families: Vec<Vec<Option<skrifa::FontRef<'_>>>> = sections
            .iter()
            .map(|(_, format)| {
                defs.families.get(&format.font_id.family).map_or_else(Vec::new, |names| {
                    names
                        .iter()
                        .map(|name| defs.font_data.get(name).and_then(|data| skrifa::FontRef::from_index(&data.font, data.index).ok()))
                        .collect()
                })
            })
            .collect();
        let mut out = HashMap::new();
        for &(c, format) in chars {
            // egui draws a tab and thin spaces with the space glyph.
            let probe = if matches!(c, '\t' | '\u{2009}' | '\u{202F}') { ' ' } else { c };
            out.entry((format, c)).or_insert_with(|| {
                families
                    .get(format)
                    .and_then(|faces| {
                        faces.iter().position(|face| face.as_ref().is_some_and(|f| f.charmap().map(probe).is_some_and(|g| g.to_u32() != 0)))
                    })
                    .unwrap_or(usize::MAX)
            });
        }
        out
    })
}

/// A single-line `TextEdit` that lays right-to-left text out in display order with the caret
/// following it (see the module documentation); a field reading right to left is aligned to the
/// right. It takes the font and colours of a plain `TextEdit`, so don't combine it with `.font()`,
/// `.text_color()`, `.horizontal_align()` or `.layouter()`. Call [`rtl_arrows`] before showing it.
pub fn singleline(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    let align = if reads_rtl(text.as_str().chars()) { egui::Align::Max } else { egui::Align::Min };
    // `field_singleline` is a function item, a zero-sized value: leaking its box allocates nothing,
    // and gives the `'static` mutable reference the builder wants.
    egui::TextEdit::singleline(text).horizontal_align(align).layouter(Box::leak(Box::new(field_singleline)))
}

/// [`singleline`] for a multi-line field. Each line is a paragraph with its own direction: one
/// holding right-to-left script wraps in reading order and sits against the right edge
/// ([`paragraph`]), the others as egui lays them out.
pub fn multiline(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::multiline(text).layouter(Box::leak(Box::new(field_multiline)))
}

fn field_singleline(ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap_width: f32) -> Arc<egui::Galley> {
    field_layout(ui, text.as_str(), None)
}

fn field_multiline(ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32) -> Arc<egui::Galley> {
    field_layout(ui, text.as_str(), Some(wrap_width))
}

/// What egui's own `TextEdit` layouter does (its font, colour and alignment), with right-to-left
/// lines in display order. `wrap` is `None` for a single line.
fn field_layout(ui: &egui::Ui, text: &str, wrap: Option<f32>) -> Arc<egui::Galley> {
    let font_id = egui::FontSelection::Default.resolve(ui.style());
    let color = ui.visuals().override_text_color.unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());
    let format = egui::TextFormat::simple(font_id, color);
    let mut job = egui::text::LayoutJob::default();
    match wrap {
        Some(width) if has_rtl(text) => {
            // Paragraph by paragraph, joined as egui joins lines.
            let ctx = ui.ctx();
            let parts: Vec<Arc<egui::Galley>> = text
                .split('\n')
                .map(|line| {
                    paragraph(ctx, &[(line, format.clone())], width, None).unwrap_or_else(|| {
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = width;
                        job.append(line, 0.0, format.clone());
                        ctx.fonts_mut(|f| f.layout_job(job))
                    })
                })
                .collect();
            job.wrap.max_width = width;
            job.append(text, 0.0, format);
            return Arc::new(egui::Galley::concat(Arc::new(job), &parts, ctx.pixels_per_point()));
        }
        Some(width) => job.wrap.max_width = width,
        None => job.break_on_newline = false,
    }
    job.halign = if reads_rtl(text.chars()) { egui::Align::RIGHT } else { egui::Align::LEFT };
    layout(ui.ctx(), job, &[(text, format)], None)
}

/// In the text field `id` (`None`: the next widget's automatic id, as a `TextEdit` without `.id()`
/// gets), while it has focus and `text` reads right to left, swaps this frame's Left and Right
/// arrow keys, so egui's logical caret movement goes the way the arrow points. Call it before the
/// field is shown.
pub fn rtl_arrows(ui: &egui::Ui, id: Option<egui::Id>, text: &str) {
    let id = id.unwrap_or_else(|| ui.next_auto_id());
    if ui.memory(|m| m.has_focus(id)) && reads_rtl(text.chars()) {
        ui.input_mut(|i| swap_arrows(&mut i.events));
    }
}

/// Swaps the Left and Right arrow keys (with any modifiers) in `events`.
pub fn swap_arrows(events: &mut [egui::Event]) {
    for event in events {
        if let egui::Event::Key { key, .. } = event {
            *key = match *key {
                egui::Key::ArrowLeft => egui::Key::ArrowRight,
                egui::Key::ArrowRight => egui::Key::ArrowLeft,
                other => other,
            };
        }
    }
}

/// `shown` (one face, one level, in screen order) as the shaper must get it. The shaper takes a
/// run's direction from its first character of a strong script and reverses a right-to-left run,
/// mirroring its brackets; `odd` says whether the characters are right to left.
fn for_shaper(shown: &[char], odd: bool) -> String {
    // Arabic-Indic digits are Arabic script to the shaper, though they read left to right.
    let strong = |c: &char| if is_rtl_script(*c) && (c.is_alphabetic() || c.is_numeric()) { Some(true) } else { c.is_alphabetic().then_some(false) };
    let first = shown.iter().find_map(strong);
    let last = shown.iter().rev().find_map(strong);
    // Given as shown, the run must shape left to right; given reversed, right to left. A piece
    // starting right to left and ending left to right can't be either; it is left as shown.
    let reversed = first == Some(true) && last == Some(true);
    let shaped_rtl = first == Some(true);
    let mirror = |c: char| if odd != shaped_rtl { mirrored(c) } else { c };
    if reversed { shown.iter().rev().map(|&c| mirror(c)).collect() } else { shown.iter().map(|&c| mirror(c)).collect() }
}

/// Whether `chars` read right to left as a whole: more right-to-left letters than left-to-right
/// ones, so the order they come in doesn't change the answer.
pub fn reads_rtl(chars: impl IntoIterator<Item = char>) -> bool {
    let (mut rtl, mut ltr) = (0usize, 0usize);
    for c in chars {
        if is_rtl_script(c) && c.is_alphabetic() {
            rtl = rtl.saturating_add(1);
        } else if c.is_alphabetic() {
            ltr = ltr.saturating_add(1);
        }
    }
    rtl > ltr
}

/// Whether `text` holds any right-to-left script.
pub fn has_rtl(text: &str) -> bool {
    text.chars().any(is_rtl_script)
}

/// Characters of the right-to-left scripts' blocks: letters, marks and the scripts' own
/// punctuation, all drawn by the script's face.
fn is_rtl_script(c: char) -> bool {
    matches!(u32::from(c), 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFC | 0x1_0800..=0x1_0FFF | 0x1_E800..=0x1_EFFF)
}

fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        other => other,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// What egui shows for `pieces`: each is one run of one face, shaped in the direction of its
    /// first strong-script character, so a right-to-left one comes out reversed and mirrored.
    fn shown_by_egui(pieces: &[Piece]) -> String {
        let strong =
            |c: char| if is_rtl_script(c) && (c.is_alphabetic() || c.is_numeric()) { Some(true) } else { c.is_alphabetic().then_some(false) };
        pieces
            .iter()
            .map(|p| if p.text.chars().find_map(strong) == Some(true) { p.text.chars().rev().map(mirrored).collect() } else { p.text.clone() })
            .collect()
    }

    fn one_format(s: &str) -> Vec<(char, usize)> {
        s.chars().map(|c| (c, 0)).collect()
    }

    /// Hebrew in face 1 (an installed fallback), everything else in face 0 (the cell font).
    fn split_faces(_: usize, c: char) -> usize {
        usize::from(is_rtl_script(c))
    }

    #[test]
    fn lines_show_numbers_and_latin_left_to_right_inside_hebrew() {
        for (logical, screen) in [
            ("קידוד ישן 1255", "1255 ןשי דודיק"),
            ("שלום עולם", "םלוע םולש"),
            ("סה\"כ 12,500 ₪", "₪ 12,500 כ\"הס"),
            ("(שלום)", "(םולש)"),
            ("מחיר: 12.50", "12.50 :ריחמ"),
            ("Q1 שלום", "Q1 םולש"),
        ] {
            let pieces = editor_pieces(&one_format(logical), None, |_, _| 0).unwrap();
            assert_eq!(shown_by_egui(&pieces), screen, "{logical}");
            let pieces = editor_pieces(&one_format(logical), None, split_faces).unwrap();
            assert_eq!(shown_by_egui(&pieces), screen, "{logical}, Hebrew from another face");
        }
    }

    #[test]
    fn a_right_to_left_sheet_reads_mixed_cells_right_to_left() {
        // On a right-to-left sheet a cell starting with a Latin word still reads right to left.
        let pieces = editor_pieces(&one_format("Q1 שלום"), Some(true), |_, _| 0).unwrap();
        assert_eq!(shown_by_egui(&pieces), "םולש Q1");
    }

    #[test]
    fn pieces_keep_every_character_and_its_format() {
        let line: Vec<(char, usize)> = "שלום".chars().map(|c| (c, 0)).chain(" עולם 2026".chars().map(|c| (c, 1))).collect();
        let pieces = editor_pieces(&line, None, |_, _| 0).unwrap();
        assert_eq!(shown_by_egui(&pieces), "2026 םלוע םולש");
        for piece in &pieces {
            for c in piece.text.chars() {
                assert!(line.iter().any(|&(l, f)| l == c && f == piece.format), "{c:?} in format {}", piece.format);
            }
        }
        let mut given: Vec<char> = line.iter().map(|p| p.0).collect();
        let mut laid: Vec<char> = pieces.iter().flat_map(|p| p.text.chars()).collect();
        given.sort_unstable();
        laid.sort_unstable();
        assert_eq!(given, laid, "one glyph per character, so the caret indices still fit");
    }

    #[test]
    fn lines_without_rtl_are_left_alone_and_odd_ones_never_panic() {
        for s in ["", "1255 abc", "\u{200F}", "a (b)"] {
            assert_eq!(editor_pieces(&one_format(s), None, |_, _| 0), None, "{s:?}");
        }
        for s in ["\u{05B0}", "\u{05D0}\u{2029}\u{2029}", "\u{202E}\u{05D0}1", "\u{05D0}\u{200F}\u{200E}\u{2067}x", "\u{0661}\u{0662} abc"] {
            let line = one_format(s);
            let pieces = editor_pieces(&line, None, |f, c| f + usize::from(c.is_ascii())).unwrap();
            assert_eq!(pieces.iter().map(|p| p.text.chars().count()).sum::<usize>(), line.len(), "{s:?}");
        }
    }

    #[test]
    fn wrapped_cells_break_between_words_in_reading_order() {
        // Every character one unit wide.
        let width = |s: &str| s.chars().count() as f32;
        let rows = wrap_words("שלום עולם גדול ויפה", 10.0, width);
        let texts: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["שלום עולם ", "גדול ויפה"], "the first words on the first row, no word cut");
        assert!(rows.iter().all(|(_, rtl)| *rtl == Some(true)));
        // A word wider than a row breaks between its characters; paragraphs keep their own direction.
        let rows = wrap_words("אבגדהוזחטיכלמנ\nHello עולם", 5.0, width);
        let texts: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(texts, ["אבגדה", "וזחטי", "כלמנ", "Hello ", "עולם"]);
        assert_eq!(rows.last().map(|r| r.1), Some(Some(false)), "the second paragraph starts with a Latin word");
        // Nothing is lost, and odd widths never loop.
        for width_units in [0.0, 1.0, f32::NAN, f32::INFINITY] {
            let rows = wrap_words("שלום  עולם", width_units, width);
            assert_eq!(rows.iter().map(|(t, _)| t.as_str()).collect::<String>(), "שלום  עולם", "{width_units}");
        }
    }

    #[test]
    fn text_reads_right_to_left_by_its_letters() {
        assert!(reads_rtl("שם הקובץ 2".chars()));
        assert!(reads_rtl("2 קפלן".chars()));
        assert!(!reads_rtl("Invoice 2026 שלום".chars()));
        assert!(!reads_rtl("1255".chars()));
        assert!(!reads_rtl("".chars()));
    }

    #[test]
    fn arrows_swap_and_nothing_else_does() {
        let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT };
        let mut events = vec![key(egui::Key::ArrowLeft), key(egui::Key::ArrowRight), key(egui::Key::Backspace), egui::Event::Text("ש".into())];
        swap_arrows(&mut events);
        assert_eq!(events, vec![key(egui::Key::ArrowRight), key(egui::Key::ArrowLeft), key(egui::Key::Backspace), egui::Event::Text("ש".into())]);
    }

    /// A Hebrew face installed on this machine, if any (the fallback face in a real run).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn installed_hebrew_face() -> Option<Arc<egui::FontData>> {
        let candidates: &[&str] = if cfg!(windows) {
            &[r"C:\Windows\Fonts\arial.ttf", r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\tahoma.ttf"]
        } else if cfg!(target_os = "macos") {
            &["/System/Library/Fonts/SFHebrew.ttf", "/System/Library/Fonts/Supplemental/Arial.ttf"]
        } else {
            &[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
            ]
        };
        candidates.iter().find_map(|p| std::fs::read(p).ok()).map(|b| Arc::new(egui::FontData::from_owned(b)))
    }

    /// A context whose `hebrew-only` family is one installed face drawing Hebrew and digits, as a
    /// cell font such as Arial or David does; `None` without such a face.
    #[cfg(not(target_arch = "wasm32"))]
    fn hebrew_context() -> Option<(egui::Context, egui::TextFormat)> {
        let face = installed_hebrew_face()?;
        let mut defs = egui::FontDefinitions::default();
        defs.font_data.insert("hebrew".into(), face);
        defs.families.insert(egui::FontFamily::Name("hebrew-only".into()), vec!["hebrew".into()]);
        let ctx = egui::Context::default();
        ctx.set_fonts(defs);
        // Fonts set before a pass apply from it.
        ctx.run_ui(egui::RawInput::default(), |_| {}).drop_without_applying_deltas();
        let format = egui::TextFormat { font_id: egui::FontId::new(13.0, egui::FontFamily::Name("hebrew-only".into())), ..Default::default() };
        Some((ctx, format))
    }

    fn glyph_text(galley: &egui::Galley) -> String {
        galley.rows.iter().flat_map(|row| row.glyphs.iter()).map(|g| g.chr).collect()
    }

    /// With real egui and one face drawing both the Hebrew and the digits, a plain layout reverses
    /// "1255"; [`layout`] draws it in reading order with one glyph per character.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn egui_draws_cells_in_reading_order() {
        let Some((ctx, format)) = hebrew_context() else {
            eprintln!("skipping egui_draws_cells_in_reading_order: no installed Hebrew face");
            return;
        };
        let text = "קידוד ישן 1255";
        let mut plain = egui::text::LayoutJob::default();
        plain.append(text, 0.0, format.clone());
        let plain = ctx.fonts_mut(|f| f.layout_job(plain));
        // Drawing order: glyph characters left to right as egui placed them.
        let drawn = |g: &egui::Galley| {
            let mut glyphs: Vec<_> = g.rows.iter().flat_map(|r| r.glyphs.iter()).filter(|g| g.advance_width > 0.0).copied().collect();
            glyphs.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
            glyphs.iter().map(|g| g.chr).collect::<String>()
        };
        assert_eq!(drawn(&plain), "5521 ןשי דודיק", "a plain layout: the bug this fixes");
        let fixed = layout(&ctx, egui::text::LayoutJob::default(), &[(text, format)], None);
        assert_eq!(glyph_text(&fixed), text, "glyphs in reading order, one per character");
        // On screen, left to right: the number, then the words read from the right.
        let mut on_screen: Vec<_> = fixed.rows.iter().flat_map(|r| r.glyphs.iter()).filter(|g| g.advance_width > 0.0).copied().collect();
        on_screen.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
        let shown: String = on_screen.iter().map(|g| g.chr).collect();
        assert_eq!(shown, "1255 ןשי דודיק");
    }

    /// A long Hebrew paragraph wraps between words in reading order, its first words on the first
    /// row at the right edge, one glyph per character, and the caret at the start of the text sits
    /// at the right end of the first row.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn egui_wraps_hebrew_paragraphs_in_reading_order() {
        use egui::text::CCursor;
        let Some((ctx, format)) = hebrew_context() else {
            eprintln!("skipping egui_wraps_hebrew_paragraphs_in_reading_order: no installed Hebrew face");
            return;
        };
        let text = "שלום עולם זה טקסט ארוך מאוד שעובר לשורה הבאה 2026";
        let width = ctx.fonts_mut(|f| f.layout_no_wrap("שלום עולם זה טקסט".into(), format.font_id.clone(), egui::Color32::WHITE)).size().x + 2.0;
        let galley = paragraph(&ctx, &[(text, format)], width, None).expect("Hebrew");
        assert!(galley.rows.len() >= 3, "{} rows", galley.rows.len());
        assert_eq!(glyph_text(&galley), text, "glyphs in reading order, one per character");
        assert!(galley.rows.iter().all(|r| !r.ends_with_newline), "one paragraph: soft breaks only");
        let first: String = galley.rows[0].row.glyphs.iter().map(|g| g.chr).collect();
        assert!(text.starts_with(&first) && first.starts_with("שלום עולם"), "{first:?}");
        assert!(first.trim_end().chars().last().is_some_and(|c| c != ' '));
        for row in &galley.rows {
            let words: String = row.row.glyphs.iter().map(|g| g.chr).collect();
            assert!(text.split(' ').any(|w| words.trim().starts_with(w)), "a row starts on a word boundary: {words:?}");
            assert!(
                (row.pos.x + row.row.size.x - width).abs() < 1.0,
                "rows end at the right edge: {} + {} vs {width}, {words:?}",
                row.pos.x,
                row.row.size.x
            );
        }
        let start = galley.pos_from_cursor(CCursor::new(0));
        assert!((start.min.x - width).abs() < 1.5, "the caret before the first letter is at the right end ({})", start.min.x);
    }

    /// A Hebrew file name reads its words right to left with "(1)" at their left and keeps its
    /// extension at the end; in a path the folders keep their order.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn egui_shows_hebrew_file_names_with_the_extension_at_the_end() {
        let Some((ctx, format)) = hebrew_context() else {
            eprintln!("skipping egui_shows_hebrew_file_names_with_the_extension_at_the_end: no installed Hebrew face");
            return;
        };
        // Characters in screen order, left to right (brackets as typed: the shaper mirrors them).
        let screen = |g: &egui::Galley| {
            let mut glyphs: Vec<_> = g.rows.iter().flat_map(|r| r.glyphs.iter()).filter(|gl| gl.advance_width > 0.0).copied().collect();
            glyphs.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
            glyphs.iter().map(|gl| gl.chr).filter(|c| !matches!(c, '(' | ')')).collect::<String>()
        };
        let font = format.font_id.clone();
        let g = file_name(&ctx, "דף קשר ב2 (1).docx", font.clone(), egui::Color32::WHITE);
        assert_eq!(screen(&g), "1 2ב רשק ףד.docx", "the stem from the right, \"(1)\" at its left, the extension at the end");
        let g = file_name(&ctx, r"F:\Downloads\קובץ חדש.pdf", font.clone(), egui::Color32::WHITE);
        assert_eq!(screen(&g), r"F:\Downloads\שדח ץבוק.pdf");
        let g = file_name(&ctx, "Report 2026.xlsx", font.clone(), egui::Color32::WHITE);
        assert_eq!(screen(&g), "Report 2026.xlsx", "Latin names are untouched");
        // Cut at the end, never panicking.
        let long = file_name(&ctx, "דף קשר ב2 (1).docx", font.clone(), egui::Color32::WHITE);
        let cut = file_name_truncated(&ctx, "דף קשר ב2 (1).docx", font.clone(), egui::Color32::WHITE, long.size().x * 0.6);
        assert!(cut.size().x <= long.size().x * 0.6 + 0.5 && screen(&cut).contains('…'));
        assert_eq!(screen(&file_name_truncated(&ctx, "קובץ.docx", font, egui::Color32::WHITE, 1.0)), "…");
        for odd in ["", "\\", "a\\", ".docx", "קובץ.", r"\\server\תיקייה\"] {
            let _ = file_name(&ctx, odd, format.font_id.clone(), egui::Color32::WHITE);
        }
    }

    /// Deleting the space after a Hebrew word deleted the one on its other side, as the caret was
    /// placed as if the line ran left to right. With the glyphs back in reading order (GridCraft's
    /// epaint patch), the caret before the first letter is at the right edge, the caret after
    /// "שלום" is on that word's left side, and a click there lands after its last letter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn egui_caret_follows_hebrew_reading_order() {
        use egui::text::CCursor;
        let Some((ctx, format)) = hebrew_context() else {
            eprintln!("skipping egui_caret_follows_hebrew_reading_order: no installed Hebrew face");
            return;
        };
        let text = "שלום עולם 2026";
        let galley = layout(&ctx, egui::text::LayoutJob::default(), &[(text, format)], None);
        let row = &galley.rows[0].row;
        assert_eq!(row.glyphs.iter().map(|g| g.chr).collect::<String>(), text, "glyphs in reading order");
        let x = |i: usize| galley.pos_from_cursor(CCursor::new(i)).min.x;
        let right = row.glyphs.iter().map(|g| g.max_x()).fold(f32::MIN, f32::max);
        let left = row.glyphs.iter().map(|g| g.pos.x).fold(f32::MAX, f32::min);
        assert!((x(0) - right).abs() < 0.5, "before ש: the right edge ({} vs {right})", x(0));
        let mem = row.glyphs[3];
        assert!((x(4) - mem.pos.x).abs() < 0.5, "after שלום: left of ם ({} vs {})", x(4), mem.pos.x);
        let two = row.glyphs[10];
        assert!(!two.rtl && (x(10) - two.pos.x).abs() < 0.5, "2026 reads left to right inside");
        assert!(x(text.chars().count()) < x(4) && x(text.chars().count()) >= left);
        let y = galley.rows[0].pos.y + row.size.y / 2.0;
        assert_eq!(galley.cursor_from_pos(egui::vec2(mem.pos.x + 1.0, y)).index.0, 4, "a click left of ם puts the caret after it");
        assert_eq!(galley.cursor_from_pos(egui::vec2(mem.max_x() - 1.0, y)).index.0, 3, "and right of it, before it");
    }
}
