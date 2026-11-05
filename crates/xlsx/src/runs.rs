//! Shared-string rich-text runs: assembling a parsed `<si>` (text + run parts) into the model's
//! [`CellRuns`] side-table payload, under the per-cell and per-workbook caps (issue #774).

use std::sync::Arc;

use gridcraft_model::{CellRuns, MAX_RUNS_PER_CELL, Run, RunFont};

use crate::styles::read_run_font;
use crate::xml::{El, si_parts};

/// A shared-string item: the plain text every cell sharing the item stores, and its rich-text
/// runs when the item has any worth keeping.
pub struct Si {
    pub text: Arc<str>,
    pub runs: Option<CellRuns>,
}

impl Si {
    /// Parses one `<si>`: the text as [`crate::xml::rich_text`] gives it, plus the runs when at
    /// least one carries an `<rPr>` delta, they fit [`MAX_RUNS_PER_CELL`] and `budget`, and they
    /// line up with the text. Over a cap (or misaligned) the runs are dropped and the text kept;
    /// the budget only shrinks by runs actually kept.
    pub fn parse(si: &El, palette: &[u32], budget: &mut u64) -> Si {
        let (text, parts) = si_parts(si);
        let runs = cell_runs(&text, &parts, palette, budget);
        Si { text: Arc::from(text), runs }
    }
}

/// Spends `n` units of the workbook run budget when they all fit: `true` with the budget
/// reduced by `n`, `false` with it untouched. The one place the cliff is enforced, so
/// parse-time charging (each `<si>` built once) and attachment-time charging (each cell a
/// shared item's runs are stored for) cannot drift apart.
pub fn runs_fits(budget: &mut u64, n: usize) -> bool {
    // usize → u64 never fails on supported platforms; u64::MAX never fits a real budget.
    let n = u64::try_from(n).unwrap_or(u64::MAX);
    match budget.checked_sub(n) {
        Some(left) => {
            *budget = left;
            true
        }
        None => false,
    }
}

/// Assembles the parsed run parts, enforcing the caps. Never an error: hostile input loses its
/// runs and keeps its text.
fn cell_runs(text: &str, parts: &[crate::xml::SiRun<'_>], palette: &[u32], budget: &mut u64) -> Option<CellRuns> {
    // Runs without a single <rPr> carry no formatting information.
    if parts.is_empty() || !parts.iter().any(|p| p.rpr.is_some()) {
        return None;
    }
    if parts.len() > MAX_RUNS_PER_CELL {
        return None;
    }
    let runs: Vec<Run> = parts
        .iter()
        .map(|p| Run {
            start: p.start,
            len: p.len,
            font: match p.rpr {
                Some(rpr) => read_run_font(rpr, palette),
                None => RunFont { name: None, size: None, bold: None, italic: None, underline: None, strike: None, color: None, vert: None },
            },
        })
        .collect();
    let cr = CellRuns { runs };
    // The budget only shrinks by runs actually kept.
    if !cr.valid_for(text) || !runs_fits(budget, cr.runs.len()) {
        return None;
    }
    Some(cr)
}
