//! Rich-text runs for a cell: per-range font deltas over a plain text value (issue #774).
//! The cell's `Value::Text` stays a plain string; the runs live in a side-table and only
//! describe how to draw ranges of it.

use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::style::{Color, Font, Underline, VertAlign};

/// Upper bound on runs stored for one cell. Enforced where runs enter the model (xlsx import,
/// editing commands); `CellRuns::valid_for` only checks geometry.
pub const MAX_RUNS_PER_CELL: usize = 8_192;

/// A font delta: `None` fields inherit the cell style's font via [`RunFont::merged`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunFont {
    pub name: Option<String>,
    pub size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<Underline>,
    pub strike: Option<bool>,
    pub color: Option<Color>,
    pub vert: Option<VertAlign>,
}

impl Eq for RunFont {}
impl Hash for RunFont {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.name.hash(h);
        self.size.map(f32::to_bits).hash(h);
        (self.bold, self.italic, self.underline, self.strike, self.color, self.vert).hash(h);
    }
}

impl RunFont {
    /// This delta applied over `base`: `None` fields take `base`'s value.
    pub fn merged(&self, base: &Font) -> Font {
        Font {
            name: self.name.clone().unwrap_or_else(|| base.name.clone()),
            size: self.size.unwrap_or(base.size),
            bold: self.bold.unwrap_or(base.bold),
            italic: self.italic.unwrap_or(base.italic),
            underline: self.underline.unwrap_or(base.underline),
            strike: self.strike.unwrap_or(base.strike),
            color: self.color.unwrap_or(base.color),
            vert: self.vert.unwrap_or(base.vert),
            charset: base.charset,
            family: base.family,
            minor: base.minor,
        }
    }
}

/// One run: `len` bytes of the cell's text starting at byte `start`, drawn with `font`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub start: u32,
    pub len: u32,
    pub font: RunFont,
}

impl Hash for Run {
    fn hash<H: Hasher>(&self, h: &mut H) {
        (self.start, self.len).hash(h);
        self.font.hash(h);
    }
}

/// The runs of one cell, covering its text value (see [`CellRuns::valid_for`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellRuns {
    pub runs: Vec<Run>,
}

impl Hash for CellRuns {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.runs.hash(h);
    }
}

/// Error from [`CellRuns::format_range`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunFormatError {
    /// The result would need more than [`MAX_RUNS_PER_CELL`] runs.
    Cap,
}

/// `delta` field-wise over `base`: every `Some` field of `delta` wins, `None` keeps `base`'s.
fn overlay(base: &RunFont, delta: &RunFont) -> RunFont {
    RunFont {
        name: delta.name.clone().or_else(|| base.name.clone()),
        size: delta.size.or(base.size),
        bold: delta.bold.or(base.bold),
        italic: delta.italic.or(base.italic),
        underline: delta.underline.or(base.underline),
        strike: delta.strike.or(base.strike),
        color: delta.color.or(base.color),
        vert: delta.vert.or(base.vert),
    }
}

/// The longest common byte prefix `p` (snapped back to a UTF-8 boundary: the kept bytes are
/// identical, so boundary-ness agrees in both texts) and the longest common byte suffix `s`
/// of the remainders past it (zipping caps it at the shorter remainder, so the kept parts
/// never overlap). `old_text` and `new_text` then differ only inside `[p, old_len - s)` versus
/// `[p, new_len - s)`. Never panics.
fn common_prefix_suffix(old_text: &str, new_text: &str) -> (usize, usize) {
    let mut p = 0;
    for (a, b) in old_text.as_bytes().iter().zip(new_text.as_bytes()) {
        if a != b {
            break;
        }
        p += 1;
    }
    while p > 0 && !old_text.is_char_boundary(p) {
        p -= 1;
    }
    let old_tail = old_text.as_bytes().get(p..).unwrap_or(&[]);
    let new_tail = new_text.as_bytes().get(p..).unwrap_or(&[]);
    let mut s = 0;
    for (a, b) in old_tail.iter().rev().zip(new_tail.iter().rev()) {
        if a != b {
            break;
        }
        s += 1;
    }
    while s > 0 && !old_text.is_char_boundary(old_text.len() - s) {
        s -= 1;
    }
    (p, s)
}

impl CellRuns {
    /// True when the runs are in order, non-overlapping, on UTF-8 boundaries and concatenate to
    /// exactly `text`. An empty run list is never valid.
    pub fn valid_for(&self, text: &str) -> bool {
        if self.runs.is_empty() {
            return false;
        }
        let mut total: usize = 0;
        for run in &self.runs {
            if run.start as usize != total {
                return false;
            }
            let end = match total.checked_add(run.len as usize) {
                Some(end) => end,
                None => return false,
            };
            if text.get(total..end).is_none() {
                return false;
            }
            total = end;
        }
        total == text.len()
    }

    /// The text of run `i`: `None` when the index is out of bounds or the run's byte range is
    /// out of bounds or not on UTF-8 boundaries.
    pub fn run_text<'a>(&self, text: &'a str, i: usize) -> Option<&'a str> {
        let run = self.runs.get(i)?;
        let start = run.start as usize;
        let end = start.checked_add(run.len as usize)?;
        text.get(start..end)
    }

    /// Formats byte `range` of the cell's text by overlaying `delta` field-wise on the runs it
    /// covers, splitting runs that straddle the range edges (the pieces keep the parent font)
    /// and coalescing neighbours that end up equal. Returns `Ok(None)` when the result is flat —
    /// an empty list, or one run covering all of `text` with an all-`None` font — and
    /// [`RunFormatError::Cap`] when the result would exceed [`MAX_RUNS_PER_CELL`]. Never panics:
    /// pieces are plain byte arithmetic, so a misuse (range off the text or its boundaries)
    /// just reassembles the runs.
    pub fn format_range(&self, text: &str, range: std::ops::Range<usize>, delta: &RunFont) -> Result<Option<CellRuns>, RunFormatError> {
        let mut runs: Vec<Run> = Vec::with_capacity(self.runs.len() + 2);
        for old in &self.runs {
            let rs = old.start as usize;
            let re = rs.saturating_add(old.len as usize);
            let from = range.start.clamp(rs, re);
            let to = range.end.clamp(from, re);
            if from > rs {
                runs.push(Run { start: old.start, len: (from - rs) as u32, font: old.font.clone() });
            }
            if to > from {
                runs.push(Run { start: from as u32, len: (to - from) as u32, font: overlay(&old.font, delta) });
            }
            if re > to {
                runs.push(Run { start: to as u32, len: (re - to) as u32, font: old.font.clone() });
            }
        }
        let mut coalesced: Vec<Run> = Vec::with_capacity(runs.len());
        for r in runs {
            match coalesced.last_mut() {
                Some(last) if last.font == r.font && last.start as usize + last.len as usize == r.start as usize => {
                    last.len = last.len.saturating_add(r.len);
                }
                _ => coalesced.push(r),
            }
        }
        let flat = match coalesced.as_slice() {
            [] => true,
            [one] => one.start == 0 && one.len as usize == text.len() && one.font == RunFont::default(),
            _ => false,
        };
        if flat {
            return Ok(None);
        }
        if coalesced.len() > MAX_RUNS_PER_CELL {
            return Err(RunFormatError::Cap);
        }
        Ok(Some(CellRuns { runs: coalesced }))
    }

    /// Re-derives the runs after a text edit: `self` is valid for `old_text`, the result for
    /// `new_text`. The edit reduces to a kept common byte prefix, a kept common byte suffix,
    /// and the middle — deleted from `old_text`, inserted into `new_text`: covered run
    /// portions are dropped, and the inserted bytes take the font of the surviving run they
    /// land in (the preceding run at a run boundary, the first run at the text start).
    /// Returns `None` when the result is flat — empty (`new_text` empty, or a full retype with
    /// no surviving run to inherit a font from), or one all-default run covering all of
    /// `new_text` — or defensively when the pieces cannot be kept valid for `new_text` (e.g.
    /// `self` was not actually valid for `old_text`). Never panics.
    pub fn edited(&self, old_text: &str, new_text: &str) -> Option<CellRuns> {
        if new_text.is_empty() {
            return None;
        }
        let old_len = old_text.len();
        let new_len = new_text.len();
        let (p, s) = common_prefix_suffix(old_text, new_text);
        let new_mid = (new_len - s).saturating_sub(p); // bytes inserted into the middle of new
        let k2_old = old_len - s; // kept-suffix start, old coordinates (deleted: p..k2_old)
        let k2_new = new_len - s; // kept-suffix start, new coordinates
        let shift = k2_new as i64 - k2_old as i64;
        // Kept pieces: each old run contributes its head (before the middle) and its tail
        // (after the middle, shifted into new coordinates); the covered middle is dropped.
        let mut pieces: Vec<Run> = Vec::with_capacity(self.runs.len() + 1);
        for old in &self.runs {
            let rs = old.start as usize;
            let re = rs.saturating_add(old.len as usize);
            let head_end = re.min(p);
            if head_end > rs {
                pieces.push(Run { start: old.start, len: (head_end - rs) as u32, font: old.font.clone() });
            }
            let tail_start = rs.max(k2_old);
            if re > tail_start {
                let start = (tail_start as i64).checked_add(shift).and_then(|start| u32::try_from(start).ok());
                if let Some(start) = start {
                    pieces.push(Run { start, len: (re - tail_start) as u32, font: old.font.clone() });
                }
            }
        }
        // The inserted middle takes the font of the run it lands in: the last surviving run
        // starting before `p` (the preceding run at a boundary), or the first run at `p == 0`.
        if new_mid > 0 {
            let split = pieces.iter().rposition(|r| (r.start as usize) < p);
            // The `?` fires only when nothing survives (full retype) — flat.
            let font = pieces.get(split.unwrap_or(0))?.font.clone();
            pieces.insert(split.map_or(0, |i| i + 1), Run { start: p as u32, len: new_mid as u32, font });
        }
        let mut coalesced: Vec<Run> = Vec::with_capacity(pieces.len());
        for r in pieces {
            match coalesced.last_mut() {
                Some(last) if last.font == r.font && (last.start as usize).saturating_add(last.len as usize) == r.start as usize => {
                    last.len = last.len.saturating_add(r.len);
                }
                _ => coalesced.push(r),
            }
        }
        let flat = match coalesced.as_slice() {
            [] => true,
            [one] => one.start == 0 && one.len as usize == new_text.len() && one.font == RunFont::default(),
            _ => false,
        };
        if flat {
            return None;
        }
        let edited = CellRuns { runs: coalesced };
        if edited.valid_for(new_text) { Some(edited) } else { None }
    }

    /// Re-derives the runs after a series fill: `self` is valid for `old_text`, the result for
    /// `new_text`, which is `old_text` with one contiguous byte region replaced (the renumbered
    /// part of the fill). Runs before and after the region keep their fonts — the tail shifted
    /// by the region's length change — and the region's new bytes take the font of the run
    /// covering offset `p` in `old_text` at its start: a run starting exactly at `p` wins over
    /// the one ending there, so a renumbered digit keeps its own font where
    /// [`CellRuns::edited`] would hand the inserted bytes to the preceding run. Byte ranges of
    /// `new_text` no piece covers (a region starting past every run, or a `self` that was not
    /// valid for `old_text`) get a default-font run, so the result concatenates to `new_text`.
    /// Returns `None` when the result is flat — empty, or one all-default run covering all of
    /// `new_text` — or would exceed [`MAX_RUNS_PER_CELL`], and defensively when the pieces
    /// cannot be kept valid for `new_text` (e.g. they overlap). Never panics.
    pub fn filled(&self, old_text: &str, new_text: &str) -> Option<CellRuns> {
        if old_text == new_text {
            return Some(self.clone());
        }
        let old_len = old_text.len();
        let new_len = new_text.len();
        let (p, s) = common_prefix_suffix(old_text, new_text);
        let m = (new_len - s).saturating_sub(p); // bytes replacing the old middle
        let k2_old = old_len - s; // kept-suffix start, old coordinates (replaced: p..k2_old)
        let k2_new = new_len - s; // kept-suffix start, new coordinates
        let shift = k2_new as i64 - k2_old as i64;
        // Kept pieces: each old run contributes its head (before the region) and its tail
        // (after the region, shifted into new coordinates); the covered middle is dropped.
        let mut pieces: Vec<Run> = Vec::with_capacity(self.runs.len() + 1);
        for old in &self.runs {
            let rs = old.start as usize;
            let re = rs.saturating_add(old.len as usize);
            let head_end = re.min(p);
            if head_end > rs {
                pieces.push(Run { start: old.start, len: (head_end - rs) as u32, font: old.font.clone() });
            }
            let tail_start = rs.max(k2_old);
            if re > tail_start {
                let start = (tail_start as i64).checked_add(shift).and_then(|start| u32::try_from(start).ok());
                if let Some(start) = start {
                    pieces.push(Run { start, len: (re - tail_start) as u32, font: old.font.clone() });
                }
            }
        }
        // The replaced region takes the font of the run covering offset `p` at its start —
        // the number's own run, not the preceding one; with no covering run (the region
        // starts past every run) the default-font gap fill below covers it.
        if m > 0 {
            let font = self
                .runs
                .iter()
                .find(|r| (r.start as usize) <= p && p < (r.start as usize).saturating_add(r.len as usize))
                .map(|r| r.font.clone())
                .unwrap_or_default();
            let split = pieces.iter().rposition(|r| (r.start as usize) < p);
            pieces.insert(split.map_or(0, |i| i + 1), Run { start: p as u32, len: m as u32, font });
        }
        // Walk the pieces in start order, giving every uncovered byte range of `new_text` a
        // default-font run so the result concatenates to `new_text` (`valid_for` demands a
        // first run at 0); overlapping or out-of-order pieces cannot be repaired.
        let mut spans: Vec<Run> = Vec::with_capacity(pieces.len() + 1);
        let mut cursor = 0usize;
        for piece in pieces {
            let start = piece.start as usize;
            if start < cursor {
                return None;
            }
            if start > cursor {
                spans.push(Run { start: cursor as u32, len: (start - cursor) as u32, font: RunFont::default() });
            }
            cursor = start.saturating_add(piece.len as usize);
            spans.push(piece);
        }
        if cursor > new_len {
            return None;
        }
        if cursor < new_len {
            spans.push(Run { start: cursor as u32, len: (new_len - cursor) as u32, font: RunFont::default() });
        }
        let mut coalesced: Vec<Run> = Vec::with_capacity(spans.len());
        for r in spans {
            match coalesced.last_mut() {
                Some(last) if last.font == r.font && (last.start as usize).saturating_add(last.len as usize) == r.start as usize => {
                    last.len = last.len.saturating_add(r.len);
                }
                _ => coalesced.push(r),
            }
        }
        let flat = match coalesced.as_slice() {
            [] => true,
            [one] => one.start == 0 && one.len as usize == new_text.len() && one.font == RunFont::default(),
            _ => false,
        };
        if flat || coalesced.len() > MAX_RUNS_PER_CELL {
            return None;
        }
        let filled = CellRuns { runs: coalesced };
        if filled.valid_for(new_text) { Some(filled) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none_font() -> RunFont {
        RunFont { name: None, size: None, bold: None, italic: None, underline: None, strike: None, color: None, vert: None }
    }

    fn run(start: u32, len: u32) -> Run {
        Run { start, len, font: none_font() }
    }

    #[test]
    fn contiguous_valid() {
        let cr = CellRuns { runs: vec![run(0, 5), run(5, 4)] };
        assert!(cr.valid_for("black red"));
        assert_eq!(cr.run_text("black red", 0), Some("black"));
        assert_eq!(cr.run_text("black red", 1), Some(" red"));
        assert_eq!(cr.run_text("black red", 2), None);
    }

    #[test]
    fn gap_invalid() {
        let cr = CellRuns { runs: vec![run(0, 2), run(3, 1)] };
        assert!(!cr.valid_for("abcd"));
    }

    #[test]
    fn overlap_invalid() {
        let cr = CellRuns { runs: vec![run(0, 3), run(2, 2)] };
        assert!(!cr.valid_for("abcd"));
    }

    #[test]
    fn wrong_total_invalid() {
        let cr = CellRuns { runs: vec![run(0, 2)] };
        assert!(!cr.valid_for("abcd"));
    }

    #[test]
    fn non_boundary_invalid() {
        // "é" is two bytes; the first run cuts through the character.
        let cr = CellRuns { runs: vec![run(0, 1), run(1, 1)] };
        assert!(!cr.valid_for("é"));
    }

    #[test]
    fn empty_invalid() {
        let cr = CellRuns { runs: vec![] };
        assert!(!cr.valid_for("abc"));
        assert!(!cr.valid_for(""));
    }

    #[test]
    fn merged_overrides() {
        let base = Font { name: "Georgia".into(), size: 14.0, italic: true, ..Font::default() };
        let delta = RunFont { bold: Some(true), color: Some(Color::rgb(255, 0, 0)), ..none_font() };
        let m = delta.merged(&base);
        assert!(m.bold);
        assert_eq!(m.color, Color::rgb(255, 0, 0));
        assert_eq!(m.name, "Georgia");
        assert_eq!(m.size, 14.0);
        assert!(m.italic);
    }

    #[test]
    fn merged_inherits_all() {
        let base = Font { name: "Georgia".into(), size: 14.0, bold: true, ..Font::default() };
        assert_eq!(none_font().merged(&base), base);
    }

    fn run_font(start: u32, len: u32, font: RunFont) -> Run {
        Run { start, len, font }
    }

    fn bold_font() -> RunFont {
        RunFont { bold: Some(true), ..none_font() }
    }

    fn red_font() -> RunFont {
        RunFont { color: Some(Color::rgb(255, 0, 0)), ..none_font() }
    }

    fn green_font() -> RunFont {
        RunFont { color: Some(Color::rgb(0, 128, 0)), ..none_font() }
    }

    #[test]
    fn format_overrides_only_given_fields() {
        // "black red": bold on "black" survives; the red overlay on "red" (bytes 6..9) sets
        // only `color`.
        let cr = CellRuns { runs: vec![run_font(0, 5, bold_font()), run(5, 4)] };
        let out = cr.format_range("black red", 6..9, &red_font()).unwrap();
        assert_eq!(out, Some(CellRuns { runs: vec![run_font(0, 5, bold_font()), run(5, 1), run_font(6, 3, red_font())] }));
    }

    #[test]
    fn format_splits_inside_run() {
        // "abcdef": colouring bytes 2..4 inside the bold run splits it; both halves and the
        // covered middle keep the parent's bold.
        let cr = CellRuns { runs: vec![run_font(0, 6, bold_font())] };
        let out = cr.format_range("abcdef", 2..4, &red_font()).unwrap();
        assert_eq!(
            out,
            Some(CellRuns {
                runs: vec![
                    run_font(0, 2, bold_font()),
                    run_font(2, 2, RunFont { color: Some(Color::rgb(255, 0, 0)), ..bold_font() }),
                    run_font(4, 2, bold_font())
                ]
            })
        );
    }

    #[test]
    fn format_coalesces_neighbors() {
        // The bold delta over two adjacent bold runs yields a single run.
        let cr = CellRuns { runs: vec![run_font(0, 2, bold_font()), run_font(2, 2, bold_font())] };
        let out = cr.format_range("abcd", 0..4, &bold_font()).unwrap();
        assert_eq!(out, Some(CellRuns { runs: vec![run_font(0, 4, bold_font())] }));
    }

    #[test]
    fn format_whole_text_empty_delta_is_flat() {
        let cr = CellRuns { runs: vec![run(0, 9)] };
        assert_eq!(cr.format_range("black red", 0..9, &none_font()).unwrap(), None);
    }

    #[test]
    fn format_partial_empty_delta_is_noop() {
        let cr = CellRuns { runs: vec![run_font(0, 2, bold_font()), run(2, 2)] };
        let out = cr.format_range("abcd", 1..3, &none_font()).unwrap();
        assert_eq!(out, Some(cr));
    }

    #[test]
    fn format_cap_errors() {
        // 8 192 one-byte runs (at the cap, alternating fonts so they can't coalesce) plus a
        // two-byte tail run over "a".repeat(8194); splitting the tail would exceed the cap.
        let mut runs = Vec::with_capacity(MAX_RUNS_PER_CELL + 1);
        for i in 0..MAX_RUNS_PER_CELL as u32 {
            runs.push(run_font(i, 1, if i % 2 == 0 { bold_font() } else { none_font() }));
        }
        runs.push(run_font(MAX_RUNS_PER_CELL as u32, 2, bold_font()));
        let cr = CellRuns { runs };
        let text = "a".repeat(MAX_RUNS_PER_CELL + 2);
        assert!(cr.valid_for(&text));
        let out = cr.format_range(&text, (MAX_RUNS_PER_CELL + 1)..(MAX_RUNS_PER_CELL + 2), &red_font());
        assert_eq!(out, Err(RunFormatError::Cap));
    }

    #[test]
    fn format_multibyte_boundaries() {
        // "é ok è" is 8 bytes: é = 0..2, " ok " = 2..6, è = 6..8; every range stays on the
        // characters' byte boundaries.
        let cr = CellRuns { runs: vec![run_font(0, 8, bold_font())] };
        let out = cr.format_range("é ok è", 2..6, &red_font()).unwrap();
        assert_eq!(
            out,
            Some(CellRuns {
                runs: vec![
                    run_font(0, 2, bold_font()),
                    run_font(2, 4, RunFont { color: Some(Color::rgb(255, 0, 0)), ..bold_font() }),
                    run_font(6, 2, bold_font())
                ]
            })
        );
        assert!(out.unwrap().valid_for("é ok è"));
        let out = cr.format_range("é ok è", 6..8, &red_font()).unwrap();
        assert_eq!(
            out,
            Some(CellRuns { runs: vec![run_font(0, 6, bold_font()), run_font(6, 2, RunFont { color: Some(Color::rgb(255, 0, 0)), ..bold_font() })] })
        );
        assert!(out.unwrap().valid_for("é ok è"));
    }

    fn black_red_green() -> CellRuns {
        // "black red green": red on "red" (bytes 6..9), green on "green" (bytes 10..15).
        CellRuns { runs: vec![run(0, 6), run_font(6, 3, red_font()), run(9, 1), run_font(10, 5, green_font())] }
    }

    #[test]
    fn edited_insert_before_run() {
        // Typing "x" at byte 0: the first run absorbs it; the red run shifts to 7..10.
        let out = black_red_green().edited("black red green", "xblack red green");
        assert_eq!(out, Some(CellRuns { runs: vec![run(0, 7), run_font(7, 3, red_font()), run(10, 1), run_font(11, 5, green_font())] }));
    }

    #[test]
    fn edited_insert_inside_run() {
        // Typing "x" at byte 7 (inside "red"): the inserted byte joins the red run ("rxed").
        let out = black_red_green().edited("black red green", "black rxed green");
        assert_eq!(out, Some(CellRuns { runs: vec![run(0, 6), run_font(6, 4, red_font()), run(10, 1), run_font(11, 5, green_font())] }));
    }

    #[test]
    fn edited_insert_at_run_boundary() {
        // Typing "x" at byte 6, between "black " and "red": the inserted bytes join the
        // *preceding* run (byte 6 is that run's end), so red stays 3 bytes, shifted to 7..10.
        let out = black_red_green().edited("black red green", "black xred green");
        assert_eq!(out, Some(CellRuns { runs: vec![run(0, 7), run_font(7, 3, red_font()), run(10, 1), run_font(11, 5, green_font())] }));
    }

    #[test]
    fn edited_insert_after_run() {
        // Typing at the very end: runs unchanged except the trailing green run grows.
        let out = black_red_green().edited("black red green", "black red greenx");
        assert_eq!(out, Some(CellRuns { runs: vec![run(0, 6), run_font(6, 3, red_font()), run(9, 1), run_font(10, 6, green_font())] }));
    }

    #[test]
    fn edited_delete_spans_runs() {
        // Deleting bytes 3..12 ("ck red gr"): covered portions are gone; "bla" meets the
        // green tail ("een") at the delete point.
        let out = black_red_green().edited("black red green", "blaeen");
        assert_eq!(out, Some(CellRuns { runs: vec![run(0, 3), run_font(3, 3, green_font())] }));
    }

    #[test]
    fn edited_paste_over_selection() {
        // Replacing "black red" (bytes 0..9) with "B&W": one run in the first run's font
        // (paste plus kept space coalesce), green tail preserved.
        let out = black_red_green().edited("black red green", "B&W green");
        assert_eq!(out, Some(CellRuns { runs: vec![run(0, 4), run_font(4, 5, green_font())] }));
    }

    #[test]
    fn edited_delete_all_is_none() {
        assert_eq!(black_red_green().edited("black red green", ""), None);
    }

    #[test]
    fn edited_retype_full() {
        // No common prefix or suffix: nothing survives to keep formatting — flat.
        let cr = CellRuns { runs: vec![run_font(0, 3, bold_font())] };
        assert_eq!(cr.edited("abc", "xyz"), None);
    }

    #[test]
    fn edited_ime_multibyte() {
        // IME inserts "é" (two bytes) mid-run; the red run absorbs both bytes.
        let out = black_red_green().edited("black red green", "black réed green");
        assert_eq!(out, Some(CellRuns { runs: vec![run(0, 6), run_font(6, 5, red_font()), run(11, 1), run_font(12, 5, green_font())] }));
        assert!(out.unwrap().valid_for("black réed green"));
    }

    #[test]
    fn edited_result_always_valid() {
        // Property over the shapes above: the result is always `None` or valid for the new text.
        let shapes = [
            ("black red green", "xblack red green"),
            ("black red green", "black rxed green"),
            ("black red green", "black xred green"),
            ("black red green", "black red greenx"),
            ("black red green", "blaeen"),
            ("black red green", "B&W green"),
            ("black red green", ""),
            ("black red green", "black réed green"),
            ("abc", "xyz"),
        ];
        for (old, new) in shapes {
            if let Some(out) = black_red_green().edited(old, new) {
                assert!(out.valid_for(new), "edited({old:?} → {new:?}) produced runs not valid for the new text");
            }
        }
    }

    #[test]
    fn filled_changed_digit_keeps_its_font() {
        // "Item 1" with a bold "1" -> "Item 2": the bold stays on the digit.
        let runs = CellRuns {
            runs: vec![
                Run { start: 0, len: 5, font: RunFont::default() },
                Run { start: 5, len: 1, font: RunFont { bold: Some(true), ..RunFont::default() } },
            ],
        };
        let out = runs.filled("Item 1", "Item 2").unwrap();
        assert!(out.valid_for("Item 2"));
        assert_eq!(
            out.runs,
            vec![
                Run { start: 0, len: 5, font: RunFont::default() },
                Run { start: 5, len: 1, font: RunFont { bold: Some(true), ..RunFont::default() } },
            ]
        );
    }

    #[test]
    fn filled_widening_number_covers_the_growth() {
        // "Item 9" -> "Item 10": the bold covers both digits.
        let runs = CellRuns { runs: vec![Run { start: 5, len: 1, font: RunFont { bold: Some(true), ..RunFont::default() } }] };
        let out = runs.filled("Item 9", "Item 10").unwrap();
        assert!(out.valid_for("Item 10"));
        assert_eq!(out.runs.len(), 2, "implicit default head + bold digits");
        assert_eq!(out.runs[1].len, 2);
    }

    #[test]
    fn filled_prefix_and_suffix_runs_stay_anchored() {
        // Leading number: "5 kg" -> "6 kg" with a red "kg": the red stays on "kg".
        let runs = CellRuns {
            runs: vec![
                Run { start: 0, len: 2, font: RunFont::default() },
                Run { start: 2, len: 2, font: RunFont { color: Some(Color::Rgb(0xFF0000)), ..RunFont::default() } },
            ],
        };
        let out = runs.filled("5 kg", "6 kg").unwrap();
        assert_eq!(out.runs[1], Run { start: 2, len: 2, font: RunFont { color: Some(Color::Rgb(0xFF0000)), ..RunFont::default() } });
    }

    #[test]
    fn filled_flat_result_is_none() {
        let runs = CellRuns { runs: vec![Run { start: 0, len: 3, font: RunFont::default() }] };
        assert!(runs.filled("Mon", "Tue").is_none(), "a single all-default run covering the text is flat");
    }

    #[test]
    fn filled_identical_text_clones() {
        let runs = CellRuns { runs: vec![Run { start: 0, len: 5, font: RunFont { italic: Some(true), ..RunFont::default() } }] };
        assert_eq!(runs.filled("hello", "hello").unwrap(), runs);
    }

    #[test]
    fn filled_result_always_valid() {
        // Property over hostile inputs: the result is always `None` or valid for the new text.
        // Multibyte texts (kept prefix, replaced middle, kept suffix), a `self` that was never
        // valid for `old_text` (a gap, overlapping runs, a run past the text end), an empty
        // `new_text`, an empty `old_text`, and texts at the run-cap boundary: exactly
        // `MAX_RUNS_PER_CELL` alternating runs that a fill re-bases to exactly the cap (the
        // over-cap refusal is pinned by the explicit assert below).
        let cap = MAX_RUNS_PER_CELL as u32;
        let at_cap = CellRuns { runs: (0..cap).map(|i| run_font(i, 1, if i % 2 == 0 { bold_font() } else { none_font() })).collect() };
        let old_cap = "a".repeat(cap as usize);
        let shapes: Vec<(CellRuns, String, String)> = vec![
            (black_red_green(), "black red green".into(), "black red green".into()),
            (black_red_green(), "black red green".into(), String::new()),
            (black_red_green(), String::new(), "black red green".into()),
            // Multibyte kept prefix and suffix, and a replaced middle that spans the 2-byte
            // `è` ("1 ère" -> "2 ème": the middle [0,5) covers "1 èr").
            (CellRuns { runs: vec![run(0, 6), run_font(6, 1, bold_font())] }, "café 1".into(), "café 10".into()),
            (CellRuns { runs: vec![run(0, 1), run_font(1, 5, red_font())] }, "1 ère".into(), "2 ème".into()),
            (CellRuns { runs: vec![run_font(0, 2, bold_font()), run(2, 4)] }, "10 €".into(), "100 €".into()),
            // A `self` that is not valid for `old_text`: a gap at byte 2, runs that overlap,
            // and a run reaching past the end of the text.
            (CellRuns { runs: vec![run_font(0, 2, bold_font()), run(3, 1)] }, "abcd".into(), "abxd".into()),
            (CellRuns { runs: vec![run(0, 3), run(2, 2)] }, "abcd".into(), "abcdx".into()),
            (CellRuns { runs: vec![run_font(0, 10, bold_font())] }, "abc".into(), "abcd".into()),
            // At the cap: changing the last byte keeps exactly `cap` runs.
            (at_cap, old_cap.clone(), format!("{}b", "a".repeat((cap - 1) as usize))),
        ];
        for (cr, old, new) in &shapes {
            if let Some(out) = cr.filled(old, new) {
                assert!(out.valid_for(new), "filled({old:?} → {new:?}) produced runs not valid for the new text");
            }
        }
        // One run past the cap: the appended 'b' sits beyond every run, so it takes the
        // default font and cannot coalesce with the bold last run — the re-based result would
        // need `cap + 1` runs, and `filled` refuses it rather than go over the cap.
        let last_bold_at_cap = CellRuns { runs: (0..cap).map(|i| run_font(i, 1, if i % 2 == 1 { bold_font() } else { none_font() })).collect() };
        assert!(last_bold_at_cap.filled(&old_cap, &format!("{old_cap}b")).is_none());
    }

    #[test]
    fn serde_roundtrip() {
        let cr = CellRuns {
            runs: vec![Run { start: 0, len: 5, font: RunFont { name: Some("Georgia".into()), size: Some(14.0), bold: Some(true), ..none_font() } }],
        };
        let json = serde_json::to_string(&cr).unwrap();
        assert_eq!(serde_json::from_str::<CellRuns>(&json).unwrap(), cr);
    }
}
