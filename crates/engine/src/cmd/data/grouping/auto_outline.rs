//! Data ▸ Outline ▸ Group ▸ Auto Outline: builds a sheet's outline from its summary formulas.
//!
//! A formula is a summary of the rows next to it when every cell it reads on its own sheet, in its
//! own column, lies on one side of it and the nearest of them touches it: `=SUM(B2:B4)` in B5 sums
//! rows 2–4 below them, `=SUM(B3:B5)` in B2 above them. Columns work the same way within a row
//! (`=SUM(B2:D2)` in E2). A summary that reads other summaries (`=B5+B9` in B10) takes in their
//! detail too, which makes the outer level. Each detail run becomes a group, and a line's outline
//! level is the number of groups around it, at most [`outline::MAX_LEVEL`].

use std::collections::{BTreeMap, BTreeSet};

use super::{
    CellRef, EngineError, Json, LineInfo, RangeRef, Result, Session, edit, json, move_marks, outline, refuse_unless_outline_allowed, set_hidden,
    str_param, target_sheet,
};
use gridcraft_model::{Sheet, Workbook};

/// Rows (summary rows below or above their detail) or columns (right or left of it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Rows,
    Cols,
}

impl Axis {
    /// The line a cell sits on along this axis, and its position across it.
    fn split(self, c: CellRef) -> (u32, u32) {
        match self {
            Axis::Rows => (c.row, c.col),
            Axis::Cols => (c.col, c.row),
        }
    }
    /// A range's first and last line along this axis, and its first and last position across it.
    fn spans(self, r: &RangeRef) -> ((u32, u32), (u32, u32)) {
        match self {
            Axis::Rows => ((r.start.row, r.end.row), (r.start.col, r.end.col)),
            Axis::Cols => ((r.start.col, r.end.col), (r.start.row, r.end.row)),
        }
    }
}

/// The outline Auto Outline found on one axis: each group's first and last detail line with its
/// level, and whether the summary lines come after their detail (below / right).
#[derive(Clone, Debug, Default, PartialEq)]
struct AxisOutline {
    /// (first line, last line, level), in line order and not overlapping.
    runs: Vec<(u32, u32, u8)>,
    summary_after: bool,
}

/// The summary formulas of one axis: summary line → the farthest detail line a formula on it
/// reads, for summaries after their detail and for summaries before it, with how many formula
/// cells point each way.
#[derive(Default)]
struct Summaries {
    after: BTreeMap<u32, u32>,
    before: BTreeMap<u32, u32>,
    votes_after: usize,
    votes_before: usize,
}

/// Collects the summary formulas in `area` along `axis`, in one pass over the area's stored cells.
/// Only references on the formula's own sheet count (relative, absolute or naming the sheet), and
/// only those crossing the formula's own column (rows) or row (columns): together they must lie on
/// one side of the formula and touch it. The detail is cut off at the edge of `area`.
fn summaries(wb: &Workbook, si: usize, sh: &Sheet, area: RangeRef, axis: Axis) -> Summaries {
    let (first, last) = axis.spans(&area).0;
    let mut out = Summaries::default();
    for (at, cell) in sh.cells.iter_range(area) {
        let Some(f) = cell.formula.as_ref() else { continue };
        let (line, across) = axis.split(at);
        let Some(areas) = f.with_expr(|e| gridcraft_calc::eval::precedents(wb, si, e).0) else { continue };
        let mut read: Option<(u32, u32)> = None;
        for a in areas.iter().filter(|a| a.sheet == si) {
            let ((lo, hi), (c0, c1)) = axis.spans(&a.range);
            if c0 <= across && across <= c1 {
                read = Some(read.map_or((lo, hi), |(l, h)| (l.min(lo), h.max(hi))));
            }
        }
        let Some((lo, hi)) = read else { continue };
        if hi.checked_add(1) == Some(line) && hi >= first {
            let far = out.after.entry(line).or_insert(lo.max(first));
            *far = (*far).min(lo.max(first));
            out.votes_after += 1;
        } else if line.checked_add(1) == Some(lo) && lo <= last {
            let far = out.before.entry(line).or_insert(hi.min(last));
            *far = (*far).max(hi.min(last));
            out.votes_before += 1;
        }
    }
    out
}

/// Each summary line's detail run, inner summaries first: a run that reaches a line which is
/// itself a summary takes in that summary's detail, so runs nest and never cross. `after` says the
/// summaries sit after their detail; `found` maps each summary line to the farthest line it reads.
/// A run jumps from one inner summary to the next with a map lookup, so the work grows with the
/// number of summaries, not with the number of lines they cover.
fn detail_runs(found: &BTreeMap<u32, u32>, after: bool) -> Vec<(u32, u32)> {
    // Summary line → the far end of its run.
    let mut far_end: BTreeMap<u32, u32> = BTreeMap::new();
    let order: Vec<(u32, u32)> =
        if after { found.iter().map(|(&l, &f)| (l, f)).collect() } else { found.iter().rev().map(|(&l, &f)| (l, f)).collect() };
    for (line, mut far) in order {
        // The detail line next to the summary.
        let Some(mut near) = (if after { line.checked_sub(1) } else { line.checked_add(1) }) else { continue };
        loop {
            // The nearest summary already placed between `near` and `far`: its run is inside this one.
            let inner = match after {
                true if far <= near => far_end.range(far..=near).next_back(),
                false if near <= far => far_end.range(near..=far).next(),
                _ => None,
            };
            let Some((_, &end)) = inner else { break };
            far = if after { far.min(end) } else { far.max(end) };
            match if after { end.checked_sub(1) } else { end.checked_add(1) } {
                Some(n) => near = n,
                None => break,
            }
        }
        far_end.insert(line, far);
    }
    far_end.into_iter().filter_map(|(line, far)| if after { Some((far, line.checked_sub(1)?)) } else { Some((line.checked_add(1)?, far)) }).collect()
}

/// The outline levels nested runs give: (first, last, level) for every stretch of lines inside at
/// least one run, deeper than [`outline::MAX_LEVEL`] counted as that level.
fn levels(runs: &[(u32, u32)]) -> Vec<(u32, u32, u8)> {
    let mut delta: BTreeMap<u32, i64> = BTreeMap::new();
    for &(a, b) in runs {
        *delta.entry(a).or_default() += 1;
        *delta.entry(b.saturating_add(1)).or_default() -= 1;
    }
    let mut out: Vec<(u32, u32, u8)> = Vec::new();
    let mut depth = 0i64;
    let mut points = delta.into_iter().peekable();
    while let Some((at, d)) = points.next() {
        depth += d;
        let Some(&(next, _)) = points.peek() else { break };
        if depth > 0 {
            let level = u8::try_from(depth).unwrap_or(u8::MAX).min(outline::MAX_LEVEL);
            match out.last_mut() {
                // Levels past the deepest one merge into it.
                Some(last) if last.2 == level && last.1.checked_add(1) == Some(at) => last.1 = next - 1,
                _ => out.push((at, next - 1, level)),
            }
        }
    }
    out
}

/// The outline Auto Outline builds on one axis, or `None` when no formula in `area` summarises
/// the lines next to it. More summaries after their detail than before it put the summaries after
/// (below / right); more before put them before; a tie keeps the sheet's setting.
fn axis_outline(wb: &Workbook, si: usize, sh: &Sheet, area: RangeRef, axis: Axis) -> Option<AxisOutline> {
    let found = summaries(wb, si, sh, area, axis);
    let currently_after = !outline::summary_before(sh, axis == Axis::Rows);
    let after = match found.votes_after.cmp(&found.votes_before) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => currently_after,
    };
    let chosen = if after { &found.after } else { &found.before };
    if chosen.is_empty() {
        return None;
    }
    Some(AxisOutline { runs: levels(&detail_runs(chosen, after)), summary_after: after })
}

/// `lines` with the outline between `first` and `last` replaced by `runs`. Lines that lose their
/// level are shown again (a zero size gets its size back), except rows a filter hides; collapsed
/// marks in and next to the stretch go, except those of groups outside it, which move to their
/// new summary line when the direction changes (`was_before` → `before`). Lines left with nothing
/// to say are dropped, so replacing an outline over whole rows leaves no entry per row.
fn replace_outline(
    lines: &BTreeMap<u32, LineInfo>,
    (first, last): (u32, u32),
    runs: &[(u32, u32, u8)],
    (was_before, before): (bool, bool),
    filtered: &dyn Fn(u32) -> bool,
) -> BTreeMap<u32, LineInfo> {
    let mut map = lines.clone();
    // Marks that belong to groups wholly outside the stretch stay.
    let outside: BTreeSet<u32> = outline::groups_with(&map, was_before)
        .into_iter()
        .filter(|g| g.has_summary() && (g.end < first || g.start > last) && map.get(&g.summary()).is_some_and(|e| e.collapsed))
        .map(|g| g.summary())
        .collect();
    for (&i, e) in map.range_mut(first..=last) {
        if e.outline > 0 {
            e.outline = 0;
            if e.hidden && !filtered(i) {
                set_hidden(e, false);
            }
        }
    }
    for (&i, e) in map.range_mut(first.saturating_sub(1)..=last.saturating_add(1)) {
        if e.collapsed && !outside.contains(&i) {
            e.collapsed = false;
        }
    }
    for &(a, b, level) in runs {
        for i in a..=b {
            map.entry(i).or_default().outline = level;
        }
    }
    if was_before != before {
        move_marks(&mut map, was_before, !before);
    }
    let empty: Vec<u32> =
        map.range(first.saturating_sub(1)..=last.saturating_add(1)).filter(|(_, e)| **e == LineInfo::default()).map(|(&i, _)| i).collect();
    for i in empty {
        map.remove(&i);
    }
    map
}

/// What Auto Outline works on: the sheet and, for a block of cells, that block; `None` means the
/// whole sheet (one cell selected or named, every cell, or another sheet without `range`).
fn auto_outline_scope(s: &Session, p: &Json) -> Result<(usize, Option<RangeRef>)> {
    let si = target_sheet(s, p)?;
    let d = s.doc()?;
    if let Some(t) = str_param(p, "range")
        && let (Some(name), _) = crate::cmd::split_sheet(t)
        && d.wb.sheet_index(&name) != Some(si)
    {
        return Err(crate::cmd::bad("data.autoOutline", format!("`{t}` names a sheet other than `sheet`")));
    }
    let ranges = if p.get("range").is_some() || si == d.wb.active_sheet() { crate::cmd::target_ranges(s, p)? } else { Vec::new() };
    if ranges.len() > 1 {
        return Err(EngineError::Other("Auto Outline can't be used on more than one range at a time.".into()));
    }
    let scope = ranges.first().copied().filter(|r| !r.is_single() && !(r.is_full_rows() && r.is_full_cols()));
    Ok((si, scope))
}

/// Data ▸ Outline ▸ Group ▸ Auto Outline. With one cell selected it outlines the whole sheet; with
/// a block of cells, the rows and columns of that block; whole rows give row groups only and whole
/// columns column groups only. The outline already there is replaced, and the summary direction
/// (Outline Settings) follows the formulas found. Nothing to outline is an error; one undo step.
pub(in crate::cmd::data) fn auto_outline(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, scope) = auto_outline_scope(s, p)?;
    let rows = scope.is_none_or(|r| !r.is_full_cols() || r.is_full_rows());
    let cols = scope.is_none_or(|r| !r.is_full_rows() || r.is_full_cols());
    for (on, is_rows) in [(rows, true), (cols, false)] {
        if on {
            refuse_unless_outline_allowed(s, si, is_rows)?;
        }
    }
    let d = s.doc()?;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let area = match (scope, sh.used_range()) {
        (Some(r), Some(used)) => r.intersection(&used),
        (None, used) => used,
        (Some(_), None) => None,
    };
    let found = |axis: Axis, on: bool| if on { area.and_then(|a| axis_outline(&d.wb, si, sh, a, axis)) } else { None };
    let (row_outline, col_outline) = (found(Axis::Rows, rows), found(Axis::Cols, cols));
    if row_outline.is_none() && col_outline.is_none() {
        return Err(EngineError::Other(
            "There is no outline to create: no formula here sums the rows or columns next to it (Data › Outline › Group › Auto Outline).".into(),
        ));
    }
    let max_row = gridcraft_core::MAX_ROWS - 1;
    let max_col = gridcraft_core::MAX_COLS - 1;
    let row_lines = scope.map_or((0, max_row), |r| (r.start.row, r.end.row));
    let col_lines = scope.map_or((0, max_col), |r| (r.start.col, r.end.col));
    let below = row_outline.as_ref().map_or(sh.summary_below, |o| o.summary_after);
    let right = col_outline.as_ref().map_or(sh.summary_right, |o| o.summary_after);
    // `Sheet::is_row_filtered`, with the filter's rows worked out once.
    let filter_spans = sh.filter_row_spans();
    let filtered = |row: u32| filter_spans.iter().any(|&(first, last)| row >= first && row <= last);
    let runs = |o: &Option<AxisOutline>| o.as_ref().map_or(Vec::new(), |o| o.runs.clone());
    let new_rows = rows.then(|| replace_outline(&sh.rows, row_lines, &runs(&row_outline), (!sh.summary_below, !below), &filtered));
    let new_cols = cols.then(|| replace_outline(&sh.cols, col_lines, &runs(&col_outline), (!sh.summary_right, !right), &|_| false));
    let level = |o: &Option<AxisOutline>| o.as_ref().and_then(|o| o.runs.iter().map(|r| r.2).max()).unwrap_or(0);
    let result = json!({"rows": level(&row_outline), "cols": level(&col_outline), "summaryBelow": below, "summaryRight": right});
    // Running it again on an unchanged sheet changes nothing, and adds no undo step.
    let unchanged = new_rows.as_ref().is_none_or(|m| *m == sh.rows)
        && new_cols.as_ref().is_none_or(|m| *m == sh.cols)
        && below == sh.summary_below
        && right == sh.summary_right;
    if unchanged {
        return Ok(result);
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        if let Some(m) = new_rows {
            sh.rows = m;
        }
        if let Some(m) = new_cols {
            sh.cols = m;
        }
        sh.summary_below = below;
        sh.summary_right = right;
        // Lines shown again change what SUBTOTAL(10x) and AGGREGATE skip.
        cx.structural = true;
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_runs_take_in_the_detail_of_the_summaries_they_read() {
        // Rows 5 and 9 sum the three rows above them; row 10 reads rows 5 and 9 (0-based 4, 8, 9).
        let found = BTreeMap::from([(4, 1), (8, 5), (9, 4)]);
        assert_eq!(detail_runs(&found, true), vec![(1, 3), (5, 7), (1, 8)]);
        assert_eq!(levels(&detail_runs(&found, true)), vec![(1, 3, 2), (4, 4, 1), (5, 7, 2), (8, 8, 1)]);
        // The same above the detail: rows 1 and 5 head 2–4 and 6–8, row 0 reads rows 1 and 5.
        let found = BTreeMap::from([(0, 5), (1, 4), (5, 8)]);
        assert_eq!(detail_runs(&found, false), vec![(1, 8), (2, 4), (6, 8)]);
        assert_eq!(levels(&detail_runs(&found, false)), vec![(1, 1, 1), (2, 4, 2), (5, 5, 1), (6, 8, 2)]);
    }

    #[test]
    fn a_running_total_nests_no_deeper_than_the_deepest_level() {
        // Each row reads the one above it: ten nested runs, all of them from row 1.
        let found: BTreeMap<u32, u32> = (2..12).map(|l| (l, l - 1)).collect();
        let runs = detail_runs(&found, true);
        assert_eq!(runs.len(), 10);
        assert!(runs.iter().all(|&(a, _)| a == 1), "{runs:?}");
        let lv = levels(&runs);
        assert_eq!(lv.first(), Some(&(1, 4, outline::MAX_LEVEL)), "{lv:?}");
        assert_eq!(lv.last(), Some(&(10, 10, 1)), "{lv:?}");
        assert!(lv.windows(2).all(|w| w[0].1 + 1 == w[1].0), "{lv:?}");
    }

    #[test]
    fn replacing_shows_lost_detail_but_keeps_filtered_rows_and_marks_outside() {
        let line = |outline, hidden, collapsed, size| LineInfo { outline, hidden, collapsed, size, ..LineInfo::default() };
        // Line 0 is a collapsed group of its own (its mark on line 1); lines 2–3 a collapsed group
        // (mark on 4) whose line 2 has zero size and whose line 3 a filter hides as well.
        let lines = BTreeMap::from([
            (0, line(1, true, false, None)),
            (1, line(0, false, true, None)),
            (2, line(1, true, false, Some(0.0))),
            (3, line(1, true, false, None)),
            (4, line(0, false, true, None)),
        ]);
        let got = replace_outline(&lines, (2, 5), &[(5, 5, 1)], (false, false), &|i| i == 3);
        let want = BTreeMap::from([
            (0, line(1, true, false, None)),
            (1, line(0, false, true, None)),
            (3, line(0, true, false, None)),
            (5, line(1, false, false, None)),
        ]);
        assert_eq!(got, want);
    }

    #[test]
    fn runs_at_the_edges_of_the_sheet_do_not_overflow() {
        let last = gridcraft_core::MAX_ROWS - 1;
        assert_eq!(detail_runs(&BTreeMap::from([(last, 0)]), true), vec![(0, last - 1)]);
        assert_eq!(detail_runs(&BTreeMap::from([(0, last)]), false), vec![(1, last)]);
        assert_eq!(levels(&[(0, last)]), vec![(0, last, 1)]);
    }
}
