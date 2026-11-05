//! Rich-text runs → multi-segment [`LayoutJob`] (issue #774): each run of a cell paints in its
//! own merged font and colour, instead of the whole cell text in one flat format. Pure
//! construction only — laying the job out is the grid's job (`Painter::layout_job`).

use egui::Color32;
use egui::text::{LayoutJob, TextFormat};
use gridcraft_engine::model::{CellRuns, Color, Font, RunFont, Theme, Underline};

use crate::theme;

/// The colour a run paints in: an inherited (`None`) colour takes the cell's resolved text
/// colour, `Auto` is black (Excel's automatic text), and an explicit colour is resolved against
/// the workbook theme with a black fallback (like the grid's `col32`).
pub fn run_color(rf: &RunFont, base_color: Color32, theme: &Theme) -> Color32 {
    match rf.color {
        None => base_color,
        Some(Color::Auto) => Color32::BLACK,
        Some(c) => c.resolve(theme).map(theme::color32).unwrap_or(Color32::BLACK),
    }
}

/// Build the multi-segment layout job for a cell's rich-text runs: one section per run, in the
/// run's font merged over the cell style's (`RunFont::merged`). `None` when there are no runs or
/// they don't concatenate to exactly `text` ([`CellRuns::valid_for`]) — the caller then lays the
/// text out flat, in one format.
///
/// `z` is the UI zoom (font sizes are points → pixels); `wrap` is `Some(width)` for wrapped
/// cells (breaking like `LayoutJob::simple`) and `None` for a single row that never wraps (like
/// `Painter::layout_no_wrap`).
pub fn job(text: &str, runs: &CellRuns, base: &Font, base_color: Color32, theme: &Theme, z: f32, wrap: Option<f32>) -> Option<LayoutJob> {
    if !runs.valid_for(text) {
        return None;
    }
    let mut j = LayoutJob::default();
    for run in &runs.runs {
        let start = run.start as usize;
        let end = start.checked_add(run.len as usize)?;
        let slice = text.get(start..end)?;
        let merged = run.font.merged(base);
        let family = theme::cell_family(&merged.name, merged.bold, merged.italic);
        let size = merged.size * 96.0 / 72.0 * z;
        let color = run_color(&run.font, base_color, theme);
        j.append(slice, 0.0, TextFormat::simple(egui::FontId::new(size, family), color));
    }
    match wrap {
        Some(w) => j.wrap.max_width = w,
        None => {
            j.wrap.max_width = f32::INFINITY;
            j.break_on_newline = false;
        }
    }
    Some(j)
}

/// A copy of `job` limited to its first line (the text before the first `\n`), for single-line
/// cell rendering: sections starting past the cut are dropped, the straddling one is clipped, so
/// the sections still concatenate to the text.
pub fn first_line(job: &LayoutJob) -> LayoutJob {
    let Some(cut) = job.text.find('\n') else { return job.clone() };
    let mut j = job.clone();
    j.text.truncate(cut);
    j.sections.retain(|s| s.byte_range.start.0 < cut);
    if let Some(last) = j.sections.last_mut() {
        last.byte_range.end.0 = last.byte_range.end.0.min(cut);
    }
    j
}

/// One run's underline/strike segment, in galley-local coordinates.
pub struct RunLine {
    /// Left and right edge of the run's glyphs in the row.
    pub x0: f32,
    pub x1: f32,
    /// The run's merged underline style (`Underline::None` when only the strike applies).
    pub underline: Underline,
    pub strike: bool,
    pub color: Color32,
}

/// Per-run underline/strike segments over one laid-out row of a runs job (the per-run
/// counterpart of the grid's flat underline/strike lines). `glyphs` holds `(x, advance_width)`
/// of each glyph of the row — galley-local, row offset included — and `text` is the job text
/// those glyphs spell, one glyph per char in order (true for the grid's single-row cell
/// galleys). Runs reaching past the end of `text` (a first-line clip) are clamped to it; runs
/// whose merged font has neither decoration, or that left no glyphs, are omitted.
pub fn decoration_lines(glyphs: &[(f32, f32)], text: &str, runs: &CellRuns, base: &Font, base_color: Color32, theme: &Theme) -> Vec<RunLine> {
    let mut out = Vec::new();
    for run in &runs.runs {
        let merged = run.font.merged(base);
        if merged.underline == Underline::None && !merged.strike {
            continue;
        }
        // `valid_for` put every run start on a char boundary of the full text; `text` here is a
        // prefix of it, so the clamped offsets are boundaries too. A byte offset maps to its
        // glyph by counting the chars before it (one glyph per char).
        let start_b = (run.start as usize).min(text.len());
        let end_b = match (run.start as usize).checked_add(run.len as usize) {
            Some(end) => end.min(text.len()),
            None => text.len(),
        };
        let c0 = text.get(..start_b).map_or(0, |s| s.chars().count());
        let c1 = text.get(..end_b).map_or(c0, |s| s.chars().count());
        let (mut x0, mut x1) = (f32::INFINITY, f32::NEG_INFINITY);
        for (x, adv) in glyphs.get(c0..c1).into_iter().flatten() {
            x0 = x0.min(*x);
            x1 = x1.max(x + adv);
        }
        if x0.is_finite() && x1 > x0 {
            out.push(RunLine { x0, x1, underline: merged.underline, strike: merged.strike, color: run_color(&run.font, base_color, theme) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::model::{Run, Theme};

    fn none_font() -> RunFont {
        RunFont { name: None, size: None, bold: None, italic: None, underline: None, strike: None, color: None, vert: None }
    }

    fn run(start: u32, len: u32, font: RunFont) -> Run {
        Run { start, len, font }
    }

    /// "black red green": [0..5] inherits the base colour, [5..9] is automatic, [9..15] green.
    fn three_runs() -> CellRuns {
        CellRuns {
            runs: vec![
                run(0, 5, none_font()),
                run(5, 4, RunFont { color: Some(Color::Auto), ..none_font() }),
                run(9, 6, RunFont { color: Some(Color::Rgb(0x00B050)), ..none_font() }),
            ],
        }
    }

    #[test]
    fn job_none_for_invalid() {
        let base = Font::default();
        let theme = Theme::default();
        // Covers only a prefix of the text.
        let short = CellRuns { runs: vec![run(0, 5, none_font())] };
        assert!(job("black red green", &short, &base, Color32::BLACK, &theme, 1.0, None).is_none());
        // No runs at all.
        let empty = CellRuns { runs: vec![] };
        assert!(job("black red green", &empty, &base, Color32::BLACK, &theme, 1.0, Some(50.0)).is_none());
    }

    #[test]
    fn job_segments_and_colors() {
        let base = Font::default();
        let theme = Theme::default();
        let text = "black red green";
        let red = Color32::from_rgb(255, 0, 0);
        let j = job(text, &three_runs(), &base, red, &theme, 1.0, None).unwrap();
        assert_eq!(j.text, text);
        assert_eq!(j.sections.len(), 3);
        assert_eq!(j.sections[0].format.color, red);
        assert_eq!(j.sections[1].format.color, Color32::BLACK);
        assert_eq!(j.sections[2].format.color, Color32::from_rgb(0x00, 0xB0, 0x50));
        // The sections concatenate to the text.
        assert_eq!(j.sections[0].byte_range.start.0, 0);
        assert_eq!(j.sections[0].byte_range.end.0, 5);
        assert_eq!(j.sections[1].byte_range.end.0, 9);
        assert_eq!(j.sections[2].byte_range.end.0, text.len());
    }

    #[test]
    fn bold_run_overrides_family() {
        let base = Font::default();
        let theme = Theme::default();
        let runs = CellRuns { runs: vec![run(0, 4, RunFont { bold: Some(true), ..none_font() })] };
        let j = job("bold", &runs, &base, Color32::BLACK, &theme, 1.0, None).unwrap();
        assert_eq!(j.sections[0].format.font_id.family, theme::cell_family(&base.name, true, false));
        assert_ne!(j.sections[0].format.font_id.family, theme::cell_family(&base.name, false, false));
    }

    /// A run's font family reaches the layout job (issue #774, comment 6131667259: Wingdings
    /// runs lost their font on round-trip). Rendering the Wingdings glyph itself needs the font
    /// installed; this asserts the family the run's glyphs would be laid out in.
    #[test]
    fn wingdings_run_family_reaches_job() {
        let base = Font::default();
        let theme = Theme::default();
        // "è ok": the Wingdings `è` is 2 bytes, " ok" is 3 — in a monospace-named font.
        let runs = CellRuns {
            runs: vec![
                run(0, 2, RunFont { name: Some("Wingdings".into()), ..none_font() }),
                run(2, 3, RunFont { name: Some("Courier New".into()), ..none_font() }),
            ],
        };
        let j = job("è ok", &runs, &base, Color32::BLACK, &theme, 1.0, None).unwrap();
        assert_eq!(j.sections[0].format.font_id.family, theme::cell_family("Wingdings", false, false));
        assert_eq!(j.sections[1].format.font_id.family, theme::cell_family("Courier New", false, false));
        // `cell_family` buckets names, and a monospace name lands on a different family than
        // the base font — so the per-run name genuinely drives the section's family.
        assert_ne!(j.sections[1].format.font_id.family, theme::cell_family(&base.name, false, false));
    }

    #[test]
    fn wrapped_job_honors_width() {
        let base = Font::default();
        let theme = Theme::default();
        let runs = three_runs();
        let j = job("black red green", &runs, &base, Color32::BLACK, &theme, 1.0, Some(50.0)).unwrap();
        assert_eq!(j.wrap.max_width, 50.0);
        // A no-wrap job lays out a single row of unbounded width.
        let j = job("black red green", &runs, &base, Color32::BLACK, &theme, 1.0, None).unwrap();
        assert_eq!(j.wrap.max_width, f32::INFINITY);
    }

    #[test]
    fn first_line_clips_sections() {
        let base = Font::default();
        let theme = Theme::default();
        let runs = CellRuns { runs: vec![run(0, 5, none_font())] };
        let j = job("ab\ncd", &runs, &base, Color32::BLACK, &theme, 1.0, None).unwrap();
        let fl = first_line(&j);
        assert_eq!(fl.text, "ab");
        assert_eq!(fl.sections.len(), 1);
        assert_eq!(fl.sections[0].byte_range.start.0, 0);
        assert_eq!(fl.sections[0].byte_range.end.0, 2);
        // A job with no newline is kept as is.
        let runs = CellRuns { runs: vec![run(0, 2, none_font())] };
        let j = job("ab", &runs, &base, Color32::BLACK, &theme, 1.0, None).unwrap();
        assert_eq!(first_line(&j).text, "ab");
    }

    #[test]
    fn decoration_lines_per_run() {
        // "abc": the first char is underlined (inherited colour), the second is plain, the third
        // is struck in green.
        let base = Font::default();
        let theme = Theme::default();
        let runs = CellRuns {
            runs: vec![
                run(0, 1, RunFont { underline: Some(Underline::Single), ..none_font() }),
                run(1, 1, none_font()),
                run(2, 1, RunFont { strike: Some(true), color: Some(Color::Rgb(0x00B050)), ..none_font() }),
            ],
        };
        let glyphs = vec![(0.0, 10.0), (10.0, 10.0), (20.0, 10.0)];
        let lines = decoration_lines(&glyphs, "abc", &runs, &base, Color32::from_rgb(255, 0, 0), &theme);
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].x0, lines[0].x1), (0.0, 10.0));
        assert_eq!(lines[0].underline, Underline::Single);
        assert!(!lines[0].strike);
        assert_eq!(lines[0].color, Color32::from_rgb(255, 0, 0));
        assert_eq!((lines[1].x0, lines[1].x1), (20.0, 30.0));
        assert_eq!(lines[1].underline, Underline::None);
        assert!(lines[1].strike);
        assert_eq!(lines[1].color, Color32::from_rgb(0x00, 0xB0, 0x50));
    }

    #[test]
    fn decoration_lines_clamp_past_text_end() {
        // A first-line clip shortens the text to "ab"; a run over all five original bytes is
        // clamped to it.
        let runs = CellRuns { runs: vec![run(0, 5, RunFont { underline: Some(Underline::Single), ..none_font() })] };
        let glyphs = vec![(0.0, 10.0), (10.0, 10.0)];
        let lines = decoration_lines(&glyphs, "ab", &runs, &Font::default(), Color32::BLACK, &Theme::default());
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].x0, lines[0].x1), (0.0, 20.0));
    }
}
