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

    #[test]
    fn serde_roundtrip() {
        let cr = CellRuns {
            runs: vec![Run { start: 0, len: 5, font: RunFont { name: Some("Georgia".into()), size: Some(14.0), bold: Some(true), ..none_font() } }],
        };
        let json = serde_json::to_string(&cr).unwrap();
        assert_eq!(serde_json::from_str::<CellRuns>(&json).unwrap(), cr);
    }
}
