//! Quick Analysis › Totals: appends totals (or per-cell percentages / running totals) next to a
//! block of numbers as ordinary formulas, in one undoable edit. The popup in the UI
//! (`ui.quickAnalysis`) is a thin front end for this command; the control channel, CLI and MCP
//! call it directly. The layout is GridCraft's own: aggregates go in the row below the block (or
//! the column to its right), per-cell results in a block of the same shape next to it.

use std::collections::BTreeSet;

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use gridcraft_model::{Color, NumFmt, Sparkline, SparklineKind};
use serde_json::{Value as Json, json};

use super::*;

const CMD: &str = "analysis.addTotal";
const SPARK: &str = "analysis.addSparklines";
/// Most sparklines one call may add.
const MAX_SPARKLINES: u64 = 10_000;
/// Most cells one call may write (a per-cell block on a huge selection is refused).
const MAX_OUTPUT_CELLS: u64 = 200_000;
/// Largest block (after clipping to the used range) the command scans for numbers.
const MAX_SCAN_CELLS: u64 = 5_000_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "analysis.addTotal",
            "Add Totals",
            [],
            None,
            "{function?: sum|average|count|min|max|percentTotal|runningTotal (default sum), direction?: columns|rows (default columns: one result per column, below the block; rows: one per row, right of it; percentTotal and runningTotal write a block of the same shape to the right (columns) or below (rows)), range?: \"A1:C9\" (default: the selection; limited to the used range), overwrite?: bool (else non-empty output cells are refused)} → {function, direction, output, cells}. Quick Analysis › Totals. A header row (columns) or label column (rows) is skipped; columns without numbers get no total. One undo step.",
            has_doc,
            add_total
        ),
        cmd!(
            "analysis.addSparklines",
            "Add Sparklines",
            [],
            None,
            "{type?: line|column|winLoss (default line), direction?: rows|columns (default rows: one sparkline per row, in the column right of the block; columns: one per column, in the row below), markers?: bool, range?: \"A1:F9\" (default: the selection; limited to the used range), overwrite?: bool} → {output, sparklines}. Quick Analysis › Sparklines. A header row (columns) or label column (rows) is skipped; lines without numbers get none. One undo step.",
            has_doc,
            add_sparklines
        ),
    ]
}

#[derive(Clone, Copy, PartialEq)]
enum Func {
    Aggregate(&'static str),
    Percent,
    Running,
}

fn parse_func(name: &str) -> Result<Func> {
    Ok(match name.to_ascii_lowercase().replace(['_', '-', ' ', '%'], "").as_str() {
        "sum" => Func::Aggregate("SUM"),
        "average" | "avg" => Func::Aggregate("AVERAGE"),
        "count" => Func::Aggregate("COUNT"),
        "min" => Func::Aggregate("MIN"),
        "max" => Func::Aggregate("MAX"),
        "percenttotal" | "percent" => Func::Percent,
        "runningtotal" | "running" => Func::Running,
        _ => return Err(bad(CMD, format!("unknown function `{name}` (use sum, average, count, min, max, percentTotal or runningTotal)"))),
    })
}

/// The block a Quick Analysis result is computed from.
struct Block {
    sheet: usize,
    /// The selection clipped to the used range.
    range: RangeRef,
    /// `range` without its header row (columns) or label column (rows).
    data: RangeRef,
    /// The columns (or, `by_rows`, the rows) of `data` that hold numbers.
    lines: BTreeSet<u32>,
    by_rows: bool,
}

impl Block {
    fn cell_at(&self, line: u32, i: u32) -> CellRef {
        if self.by_rows { CellRef::new(line, i) } else { CellRef::new(i, line) }
    }
    /// First and last position along a line of `data`.
    fn span_of_line(&self) -> (u32, u32) {
        if self.by_rows { (self.data.start.col, self.data.end.col) } else { (self.data.start.row, self.data.end.row) }
    }
    fn line_range(&self, line: u32, from: u32, to: u32) -> RangeRef {
        RangeRef::new(self.cell_at(line, from), self.cell_at(line, to))
    }
}

fn block(s: &Session, p: &Json, by_rows: bool) -> Result<Block> {
    let sheet = target_sheet(s, p)?;
    let sel = match str_param(p, "range") {
        Some(_) => target_range(s, p)?,
        None => s.doc()?.selection.current(),
    };
    if sel.is_single() {
        return Err(bad(CMD, "select at least two cells"));
    }
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    // Whole rows / columns are limited to what holds data.
    let used = sh.cells.used_range().ok_or_else(|| EngineError::Other("There is no data to analyse.".into()))?;
    let r = sel.intersection(&used).ok_or_else(|| EngineError::Other("There is no data to analyse in the selection.".into()))?;
    if r.count() > MAX_SCAN_CELLS {
        return Err(bad(CMD, "selection too large to analyse"));
    }
    let is_num = |c: CellRef| sh.value(c).is_number();
    let is_text = |c: CellRef| sh.value(c).is_text();
    // Header row (columns) / label column (rows): text only, with at least one entry.
    let lead: Vec<CellRef> = if by_rows {
        (r.start.row..=r.end.row).map(|row| CellRef::new(row, r.start.col)).collect()
    } else {
        (r.start.col..=r.end.col).map(|c| CellRef::new(r.start.row, c)).collect()
    };
    let longer_than_one = if by_rows { r.width() > 1 } else { r.height() > 1 };
    let has_label = longer_than_one && lead.iter().any(|c| is_text(*c)) && !lead.iter().any(|c| is_num(*c));
    let data = if by_rows {
        RangeRef::new(CellRef::new(r.start.row, r.start.col + u32::from(has_label)), r.end)
    } else {
        RangeRef::new(CellRef::new(r.start.row + u32::from(has_label), r.start.col), r.end)
    };
    let mut lines: BTreeSet<u32> = BTreeSet::new();
    for (c, _) in sh.cells.iter_range(data) {
        if is_num(c) {
            lines.insert(if by_rows { c.row } else { c.col });
        }
    }
    if lines.is_empty() {
        return Err(EngineError::Other("The selection holds no numbers.".into()));
    }
    Ok(Block { sheet, range: r, data, lines, by_rows })
}

fn add_total(s: &mut Session, p: &Json) -> Result<Json> {
    let func = parse_func(str_param(p, "function").unwrap_or("sum"))?;
    let by_rows = match str_param(p, "direction") {
        None | Some("columns") => false,
        Some("rows") => true,
        Some(other) => return Err(bad(CMD, format!("direction must be columns or rows, not `{other}`"))),
    };
    let overwrite = bool_param(p, "overwrite").unwrap_or(false);
    let b = block(s, p, by_rows)?;
    let (r, sheet) = (b.range, b.sheet);
    let (lo, hi) = b.span_of_line();
    let out_cells = match func {
        Func::Aggregate(_) => b.lines.len() as u64,
        _ => b.lines.len() as u64 * (u64::from(hi - lo) + 1),
    };
    if out_cells > MAX_OUTPUT_CELLS {
        return Err(bad(CMD, "selection too large for this result"));
    }
    // `$` pins a part of a reference like a hand-written formula would.
    let abs = |c: CellRef, row: bool, col: bool| {
        let a = c.a1();
        let letters: String = a.chars().take_while(|ch| ch.is_ascii_alphabetic()).collect();
        let digits = &a[letters.len()..];
        format!("{}{letters}{}{digits}", if col { "$" } else { "" }, if row { "$" } else { "" })
    };
    // (target, formula) pairs.
    let mut writes: Vec<(CellRef, String)> = Vec::with_capacity(out_cells as usize);
    let (height, width) = (r.height(), r.width());
    for &line in &b.lines {
        match func {
            Func::Aggregate(name) => {
                let target = if by_rows { CellRef::new(line, r.end.col.saturating_add(1)) } else { CellRef::new(r.end.row.saturating_add(1), line) };
                writes.push((target, format!("={name}({})", b.line_range(line, lo, hi).a1())));
            }
            Func::Percent | Func::Running => {
                for i in lo..=hi {
                    let src = b.cell_at(line, i);
                    let target = if by_rows {
                        CellRef::new(src.row.saturating_add(height), src.col)
                    } else {
                        CellRef::new(src.row, src.col.saturating_add(width))
                    };
                    let formula = if func == Func::Percent {
                        // The summed block is locked on the axis the formula is filled along.
                        let (a, z) = (b.cell_at(line, lo), b.cell_at(line, hi));
                        format!("={}/SUM({}:{})", src.a1(), abs(a, !by_rows, by_rows), abs(z, !by_rows, by_rows))
                    } else {
                        format!("=SUM({}:{})", abs(b.cell_at(line, lo), !by_rows, by_rows), src.a1())
                    };
                    writes.push((target, formula));
                }
            }
        }
    }
    // Everything has to fit on the sheet and may not overwrite data.
    if writes.iter().any(|(t, _)| t.row >= MAX_ROWS || t.col >= MAX_COLS) {
        return Err(EngineError::Other("There's no room next to the selection for the result.".into()));
    }
    let sh = s.doc()?.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    if !overwrite && writes.iter().any(|(t, _)| !sh.value(*t).is_empty() || sh.cells.get(*t).is_some_and(|c| c.formula.is_some())) {
        return Err(EngineError::Other("The cells next to the selection already hold data; clear them first.".into()));
    }
    let out = writes.iter().fold(None::<RangeRef>, |acc, (t, _)| Some(acc.map_or(RangeRef::cell(*t), |a| a.union(&RangeRef::cell(*t)))));
    let percent = func == Func::Percent;
    let count = writes.len();
    edit(s, |cx| {
        for (t, f) in &writes {
            let cell = super::edit::input_to_cell(f, sheet, *t, &mut cx.wb)?.unwrap_or_default();
            cx.sheet_mut(sheet)?.set_cell(*t, cell);
            if percent {
                let cur = cx.wb.sheet(sheet).map(|sh| sh.style_id(*t)).unwrap_or_default();
                let new = cx.wb.styles.derive(cur, |st| st.num_fmt = NumFmt::new("0.0%"));
                cx.sheet_mut(sheet)?.set_style(*t, new);
            }
            cx.touch(sheet, *t);
        }
        Ok(())
    })?;
    let name = match func {
        Func::Aggregate(n) => n.to_ascii_lowercase(),
        Func::Percent => "percentTotal".into(),
        Func::Running => "runningTotal".into(),
    };
    Ok(json!({"function": name, "direction": if by_rows { "rows" } else { "columns" }, "output": out.map(|o| o.a1()), "cells": count}))
}

fn add_sparklines(s: &mut Session, p: &Json) -> Result<Json> {
    let by_rows = match str_param(p, "direction") {
        None | Some("rows") => true,
        Some("columns") => false,
        Some(other) => return Err(bad(SPARK, format!("direction must be rows or columns, not `{other}`"))),
    };
    let kind = match str_param(p, "type") {
        None | Some("line") => SparklineKind::Line,
        Some("column") => SparklineKind::Column,
        Some("winLoss") => SparklineKind::WinLoss,
        Some(other) => return Err(bad(SPARK, format!("type must be line, column or winLoss, not `{other}`"))),
    };
    let markers = bool_param(p, "markers").unwrap_or(false);
    let overwrite = bool_param(p, "overwrite").unwrap_or(false);
    let b = block(s, p, by_rows)?;
    if b.lines.len() as u64 > MAX_SPARKLINES {
        return Err(bad(SPARK, "selection too large for this result"));
    }
    let (lo, hi) = b.span_of_line();
    let r = b.range;
    let sparks: Vec<(CellRef, String)> = b
        .lines
        .iter()
        .map(|&line| {
            let at = if by_rows { CellRef::new(line, r.end.col.saturating_add(1)) } else { CellRef::new(r.end.row.saturating_add(1), line) };
            (at, b.line_range(line, lo, hi).a1())
        })
        .collect();
    if sparks.iter().any(|(t, _)| t.row >= MAX_ROWS || t.col >= MAX_COLS) {
        return Err(EngineError::Other("There's no room next to the selection for the sparklines.".into()));
    }
    let sheet = b.sheet;
    let sh = s.doc()?.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    if !overwrite && sparks.iter().any(|(t, _)| !sh.value(*t).is_empty() || sh.sparklines.iter().any(|x| x.cell == *t)) {
        return Err(EngineError::Other("The cells next to the selection already hold data; clear them first.".into()));
    }
    let out = sparks.iter().fold(None::<RangeRef>, |acc, (t, _)| Some(acc.map_or(RangeRef::cell(*t), |a| a.union(&RangeRef::cell(*t)))));
    let count = sparks.len();
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        for (cell, source) in sparks {
            sh.sparklines.retain(|x| x.cell != cell);
            sh.sparklines.push(Sparkline { cell, source, kind, color: Color::Theme(4, 0), markers });
        }
        Ok(())
    })?;
    Ok(json!({"output": out.map(|o| o.a1()), "sparklines": count}))
}
