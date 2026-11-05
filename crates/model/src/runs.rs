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
        // Longest common byte prefix, snapped back to a UTF-8 boundary (the kept bytes are
        // identical, so boundary-ness agrees in both texts).
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
        // Longest common suffix over the remainders; zipping caps it at the shorter remainder,
        // so the kept parts never overlap.
        let old_tail = old_text.as_bytes().get(p..).unwrap_or(&[]);
        let new_tail = new_text.as_bytes().get(p..).unwrap_or(&[]);
        let mut s = 0;
        for (a, b) in old_tail.iter().rev().zip(new_tail.iter().rev()) {
            if a != b {
                break;
            }
            s += 1;
        }
        while s > 0 && !old_text.is_char_boundary(old_len - s) {
            s -= 1;
        }
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
    fn serde_roundtrip() {
        let cr = CellRuns {
            runs: vec![Run { start: 0, len: 5, font: RunFont { name: Some("Georgia".into()), size: Some(14.0), bold: Some(true), ..none_font() } }],
        };
        let json = serde_json::to_string(&cr).unwrap();
        assert_eq!(serde_json::from_str::<CellRuns>(&json).unwrap(), cr);
    }
}
