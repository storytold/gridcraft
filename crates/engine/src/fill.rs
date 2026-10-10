//! Fill: copy, AutoFill series (numbers, dates, weekdays, months, text with numbers, custom
//! lists), Series dialog and Flash Fill.

use std::sync::Arc;

use gridcraft_core::date::{MONTHS, WEEKDAYS, datetime_from_serial, serial_from_ymd};
use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Cell, Formula};

use crate::Result;
use crate::cmd::Ctx;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillMode {
    Series,
    Copy,
    Formats,
    ValuesOnly,
}

/// Built-in custom lists.
fn lists() -> Vec<Vec<String>> {
    let short_days: Vec<String> = WEEKDAYS.iter().map(|d| d.get(..3).unwrap_or(d).to_string()).collect();
    let short_months: Vec<String> = MONTHS.iter().map(|m| m.get(..3).unwrap_or(m).to_string()).collect();
    vec![
        short_days,
        WEEKDAYS.iter().map(|s| s.to_string()).collect(),
        short_months,
        MONTHS.iter().map(|s| s.to_string()).collect(),
        vec!["Q1".into(), "Q2".into(), "Q3".into(), "Q4".into()],
    ]
}

/// Fills `target` (which contains `src` at one edge) from the pattern in `src`.
pub fn fill(cx: &mut Ctx, sheet: usize, src: RangeRef, target: RangeRef, mode: FillMode) -> Result<()> {
    let custom = cx.wb.custom_lists.clone();
    let Some(sh) = cx.wb.sheet(sheet) else { return Ok(()) };
    // Direction: down/up if target extends rows, else right/left.
    let vertical = target.height() != src.height() || target.width() == src.width();
    let forward = if vertical {
        target.end.row > src.end.row || target.start.row == src.start.row
    } else {
        target.end.col > src.end.col || target.start.col == src.start.col
    };
    let lanes: Vec<u32> = if vertical { (src.start.col..=src.end.col).collect() } else { (src.start.row..=src.end.row).collect() };
    let src_len = if vertical { src.height() } else { src.width() };
    let mut writes: Vec<(CellRef, Option<Cell>)> = Vec::new();
    let mut pictures = Vec::new();
    for lane in lanes {
        let src_cells: Vec<(CellRef, Option<Cell>)> = (0..src_len)
            .map(|i| {
                let c = if vertical { CellRef::new(src.start.row + i, lane) } else { CellRef::new(lane, src.start.col + i) };
                (c, sh.cell(c).cloned())
            })
            .collect();
        let targets: Vec<CellRef> = if vertical {
            let rows: Vec<u32> =
                if forward { (src.end.row + 1..=target.end.row).collect() } else { (target.start.row..src.start.row).rev().collect() };
            rows.into_iter().map(|r| CellRef::new(r, lane)).collect()
        } else {
            let cols: Vec<u32> =
                if forward { (src.end.col + 1..=target.end.col).collect() } else { (target.start.col..src.start.col).rev().collect() };
            cols.into_iter().map(|c| CellRef::new(lane, c)).collect()
        };
        let pattern = if mode == FillMode::Series {
            Pattern::detect(
                &src_cells,
                &custom,
                cx.wb.date_system,
                cx.wb.styles.get(src_cells.first().and_then(|c| c.1.as_ref()).map(|c| c.style).unwrap_or_default()).num_fmt.as_str(),
            )
        } else {
            Pattern::Copy
        };
        for (k, t) in targets.iter().enumerate() {
            let step = k as i64 + 1;
            let idx = k % src_cells.len().max(1);
            let (sc, scell) = src_cells.get(if forward { idx } else { src_cells.len() - 1 - idx }).cloned().unwrap_or((src.start, None));
            let mut cell = scell.clone().unwrap_or_default();
            match mode {
                FillMode::Formats => {
                    let mut existing = sh.cell(*t).cloned().unwrap_or_default();
                    existing.style = cell.style;
                    writes.push((*t, Some(existing)));
                    continue;
                }
                FillMode::ValuesOnly | FillMode::Series | FillMode::Copy => {
                    let style = if mode == FillMode::ValuesOnly { sh.cell(*t).map(|c| c.style).unwrap_or_default() } else { cell.style };
                    if let Some(f) = &cell.formula {
                        if let Some(e) = f.expr() {
                            let shifted = gridcraft_formula::adjust::shift_relative(e, t.row as i64 - sc.row as i64, t.col as i64 - sc.col as i64);
                            cell.formula = Some(Arc::new(Formula::from_expr(shifted)));
                            cell.value = Value::Empty;
                        }
                    } else if let Some(v) = pattern.value(step, forward, src_cells.len()) {
                        cell.value = v;
                    }
                    cell.style = style;
                }
            }
            if let Some(picture) = sh.cell_pictures.get(&sc) {
                pictures.push((*t, picture.clone()));
            }
            writes.push((*t, if cell.is_blank() { None } else { Some(cell) }));
        }
    }
    let sh = cx.sheet_mut(sheet)?;
    for (c, cell) in &writes {
        match cell {
            Some(x) if mode == FillMode::Formats => sh.cells.set(*c, x.clone()),
            Some(x) => sh.set_cell(*c, x.clone()),
            None => {
                sh.remove_cell(*c);
            }
        }
    }
    sh.cell_pictures.extend(pictures);
    for (c, _) in writes {
        cx.touch(sheet, c);
    }
    Ok(())
}

#[derive(Clone, Debug)]
enum Pattern {
    Copy,
    /// Linear numeric series: last value and step.
    Linear {
        last: f64,
        first: f64,
        step: f64,
    },
    /// Text with a trailing (or leading) number: `Item 1`, `Q1`.
    TextNum {
        prefix: String,
        suffix: String,
        last: i64,
        first: i64,
        step: i64,
        width: usize,
    },
    /// A position in a list (days, months, custom).
    List {
        list: Vec<String>,
        last: usize,
        first: usize,
        step: i64,
        upper: bool,
        lower: bool,
    },
    /// Dates stepping by months (same day each month) or years.
    Months {
        last: f64,
        first: f64,
        months: i64,
    },
}

impl Pattern {
    fn detect(cells: &[(CellRef, Option<Cell>)], custom: &[Vec<String>], sys: gridcraft_core::DateSystem, fmt: &str) -> Pattern {
        let vals: Vec<Value> = cells.iter().map(|(_, c)| c.as_ref().map(|c| c.value.clone()).unwrap_or_default()).collect();
        if cells.iter().any(|(_, c)| c.as_ref().is_some_and(|c| c.formula.is_some())) {
            return Pattern::Copy;
        }
        // Numbers.
        if !vals.is_empty() && vals.iter().all(|v| v.is_number()) {
            let nums: Vec<f64> = vals.iter().filter_map(Value::as_f64).collect();
            let first = nums.first().copied().unwrap_or(0.0);
            let last = nums.last().copied().unwrap_or(0.0);
            let is_date = fmt.contains('d') || fmt.contains('y') || (fmt.contains('m') && !fmt.contains('h'));
            if nums.len() == 1 {
                // A single number copies; a single date increments by a day.
                return if is_date { Pattern::Linear { last, first, step: 1.0 } } else { Pattern::Copy };
            }
            // Dates that differ by whole months.
            if is_date && nums.len() >= 2 {
                let d0 = datetime_from_serial(sys, first);
                let d1 = datetime_from_serial(sys, nums.get(1).copied().unwrap_or(first));
                if let (Some(a), Some(b)) = (d0, d1)
                    && a.day == b.day
                    && (a.month != b.month || a.year != b.year)
                {
                    let months = (b.year as i64 - a.year as i64) * 12 + b.month as i64 - a.month as i64;
                    return Pattern::Months { last, first, months };
                }
            }
            // Least-squares step for 2+ values (Excel uses a linear trend).
            let n = nums.len() as f64;
            let mean_x = (n - 1.0) / 2.0;
            let mean_y = nums.iter().sum::<f64>() / n;
            let mut num = 0.0;
            let mut den = 0.0;
            for (i, y) in nums.iter().enumerate() {
                num += (i as f64 - mean_x) * (y - mean_y);
                den += (i as f64 - mean_x).powi(2);
            }
            let step = if den == 0.0 { 0.0 } else { num / den };
            let fitted_last = mean_y + step * (n - 1.0 - mean_x);
            let fitted_first = mean_y - step * mean_x;
            return Pattern::Linear { last: fitted_last, first: fitted_first, step };
        }
        // Text.
        if !vals.is_empty() && vals.iter().all(|v| v.is_text()) {
            let texts: Vec<String> = vals.iter().filter_map(|v| v.as_text().map(str::to_string)).collect();
            let all_lists: Vec<Vec<String>> = custom.iter().cloned().chain(lists()).collect();
            for list in &all_lists {
                let idx: Vec<Option<usize>> = texts.iter().map(|t| list.iter().position(|l| l.eq_ignore_ascii_case(t))).collect();
                if idx.iter().all(Option::is_some) {
                    let idx: Vec<usize> = idx.into_iter().flatten().collect();
                    let first = idx.first().copied().unwrap_or(0);
                    let last = idx.last().copied().unwrap_or(0);
                    let step = if idx.len() >= 2 { idx[1] as i64 - idx[0] as i64 } else { 1 };
                    let t0 = texts.first().cloned().unwrap_or_default();
                    let upper = t0.chars().all(|c| !c.is_lowercase()) && t0.chars().any(char::is_alphabetic) && t0.len() > 1;
                    let lower = t0.chars().all(|c| !c.is_uppercase());
                    return Pattern::List { list: list.clone(), last, first, step, upper, lower };
                }
            }
            // Text with a number at the end (or start).
            let split: Vec<Option<(String, i64, String, usize)>> = texts.iter().map(|t| split_num(t)).collect();
            if split.iter().all(Option::is_some) {
                let parts: Vec<(String, i64, String, usize)> = split.into_iter().flatten().collect();
                if parts.iter().all(|p| p.0 == parts[0].0 && p.2 == parts[0].2) {
                    let first = parts.first().map(|p| p.1).unwrap_or(0);
                    let last = parts.last().map(|p| p.1).unwrap_or(0);
                    let step = if parts.len() >= 2 { parts[1].1 - parts[0].1 } else { 1 };
                    return Pattern::TextNum { prefix: parts[0].0.clone(), suffix: parts[0].2.clone(), last, first, step, width: parts[0].3 };
                }
            }
        }
        Pattern::Copy
    }

    /// The value `k` steps beyond the source (forward) or before it (backward).
    fn value(&self, k: i64, forward: bool, _len: usize) -> Option<Value> {
        let sign = if forward { 1 } else { -1 };
        match self {
            Pattern::Copy => None,
            Pattern::Linear { last, first, step } => {
                let base = if forward { *last } else { *first };
                Some(Value::number(round15(base + step * (k * sign) as f64)))
            }
            Pattern::TextNum { prefix, suffix, last, first, step, width } => {
                let base = if forward { *last } else { *first };
                let n = base + step * k * sign;
                let n = n.abs();
                Some(Value::text(format!("{prefix}{n:0width$}{suffix}", width = *width)))
            }
            Pattern::List { list, last, first, step, upper, lower } => {
                let len = list.len() as i64;
                if len == 0 {
                    return None;
                }
                let base = if forward { *last } else { *first } as i64;
                let i = (base + step * k * sign).rem_euclid(len) as usize;
                let s = list.get(i)?.clone();
                Some(Value::text(if *upper {
                    s.to_uppercase()
                } else if *lower && !s.chars().next().is_some_and(char::is_uppercase) {
                    s.to_lowercase()
                } else {
                    s
                }))
            }
            Pattern::Months { last, first, months } => {
                let base = if forward { *last } else { *first };
                let d = datetime_from_serial(gridcraft_core::DateSystem::D1900, base)?;
                let m = d.month as i64 + months * k * sign;
                let s = serial_from_ymd(gridcraft_core::DateSystem::D1900, d.year as i64, m, d.day as i64)?;
                Some(Value::number(s))
            }
        }
    }
}

fn round15(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let mag = x.abs().log10().floor() as i32;
    let p = 10f64.powi(14 - mag);
    if !p.is_finite() || p == 0.0 { x } else { (x * p).round() / p }
}

/// `Item 007` → ("Item ", 7, "", 3); `3rd` style leading numbers → ("", 3, "rd", 1).
fn split_num(t: &str) -> Option<(String, i64, String, usize)> {
    let chars: Vec<char> = t.chars().collect();
    let end = chars.iter().rposition(|c| c.is_ascii_digit())?;
    let mut start = end;
    while start > 0 && chars.get(start - 1).is_some_and(|c| c.is_ascii_digit()) {
        start -= 1;
    }
    let digits: String = chars.get(start..=end)?.iter().collect();
    let n: i64 = digits.parse().ok()?;
    let prefix: String = chars.get(..start)?.iter().collect();
    let suffix: String = chars.get(end + 1..)?.iter().collect();
    let width = if digits.starts_with('0') && digits.len() > 1 { digits.len() } else { 1 };
    Some((prefix, n, suffix, width))
}

/// Home › Fill › Series.
pub fn series(cx: &mut Ctx, sheet: usize, r: RangeRef, rows: bool, kind: &str, step: f64, stop: Option<f64>, unit: &str) -> Result<()> {
    let sys = cx.wb.date_system;
    let Some(sh) = cx.wb.sheet(sheet) else { return Ok(()) };
    let lanes: Vec<u32> = if rows { (r.start.row..=r.end.row).collect() } else { (r.start.col..=r.end.col).collect() };
    let len = if rows { r.width() } else { r.height() };
    let mut writes = Vec::new();
    for lane in lanes {
        let first_cell = if rows { CellRef::new(lane, r.start.col) } else { CellRef::new(r.start.row, lane) };
        let Some(start) = sh.value(first_cell).as_f64() else { continue };
        let style = sh.style_id(first_cell);
        let mut v = start;
        for i in 1..len {
            v = match kind {
                "growth" => v * step,
                "date" => {
                    let d = datetime_from_serial(sys, v);
                    match (unit, d) {
                        ("month", Some(d)) => serial_from_ymd(sys, d.year as i64, d.month as i64 + step as i64, d.day as i64).unwrap_or(v),
                        ("year", Some(d)) => serial_from_ymd(sys, d.year as i64 + step as i64, d.month as i64, d.day as i64).unwrap_or(v),
                        ("weekday", Some(_)) => {
                            let mut n = v;
                            let mut left = step.abs() as i64;
                            while left > 0 {
                                n += step.signum();
                                if datetime_from_serial(sys, n).is_some_and(|d| d.weekday != 0 && d.weekday != 6) {
                                    left -= 1;
                                }
                            }
                            n
                        }
                        _ => v + step,
                    }
                }
                _ => v + step,
            };
            if let Some(s) = stop
                && ((step >= 0.0 && v > s) || (step < 0.0 && v < s))
            {
                break;
            }
            let c = if rows { CellRef::new(lane, r.start.col + i) } else { CellRef::new(r.start.row + i, lane) };
            writes.push((c, Cell { value: Value::number(round15(v)), formula: None, style }));
        }
    }
    let sh = cx.sheet_mut(sheet)?;
    for (c, cell) in &writes {
        sh.set_cell(*c, cell.clone());
    }
    for (c, _) in writes {
        cx.touch(sheet, c);
    }
    Ok(())
}

/// Flash Fill: learns a transformation from the examples typed in the column of `at` (rows
/// above it with values) from the column(s) to the left, and fills the remaining rows.
/// Supported transformations: a token (word) of the source by index, concatenations of tokens
/// with literal separators, upper/lower/proper case, and fixed-position substrings.
pub fn flash_fill(cx: &mut Ctx, sheet: usize, at: CellRef) -> Result<usize> {
    let Some(sh) = cx.wb.sheet(sheet) else { return Ok(0) };
    if at.col == 0 {
        return Ok(0);
    }
    let src_col = at.col - 1;
    // Rows of the data region in the source column.
    let mut top = at.row;
    while top > 0 && !sh.value(CellRef::new(top - 1, src_col)).is_empty() {
        top -= 1;
    }
    let mut bottom = at.row;
    while bottom + 1 < gridcraft_core::MAX_ROWS && !sh.value(CellRef::new(bottom + 1, src_col)).is_empty() && bottom - top < 1_000_000 {
        bottom += 1;
    }
    // Header row: skip a first row whose target is text and source looks like a header? Keep simple.
    let mut examples = Vec::new();
    let mut todo = Vec::new();
    for r in top..=bottom {
        let src = sh.value(CellRef::new(r, src_col)).display();
        let dst = sh.value(CellRef::new(r, at.col));
        if dst.is_empty() {
            todo.push((r, src));
        } else {
            examples.push((src, dst.display()));
        }
    }
    if examples.is_empty() {
        return Ok(0);
    }
    let Some(program) = learn(&examples) else { return Ok(0) };
    let mut n = 0;
    let style = sh.style_id(CellRef::new(top, at.col));
    let mut writes = Vec::new();
    for (r, src) in todo {
        if let Some(out) = program.apply(&src) {
            writes.push((CellRef::new(r, at.col), out));
            n += 1;
        }
    }
    let sh = cx.sheet_mut(sheet)?;
    for (c, out) in &writes {
        sh.set_cell(*c, Cell { value: Value::text(out.as_str()), formula: None, style });
    }
    for (c, _) in writes {
        cx.touch(sheet, c);
    }
    Ok(n)
}

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Lit(String),
    /// Token index (negative from the end) with a case transform.
    Tok(i32, Case),
    /// First character of a token.
    Initial(i32, Case),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Case {
    Same,
    Upper,
    Lower,
    Proper,
}

#[derive(Clone, Debug)]
struct Program(Vec<Piece>);

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|t| !t.is_empty()).map(str::to_string).collect()
}

fn apply_case(s: &str, c: Case) -> String {
    match c {
        Case::Same => s.to_string(),
        Case::Upper => s.to_uppercase(),
        Case::Lower => s.to_lowercase(),
        Case::Proper => {
            let mut out = String::new();
            for (i, ch) in s.chars().enumerate() {
                if i == 0 {
                    out.extend(ch.to_uppercase());
                } else {
                    out.extend(ch.to_lowercase());
                }
            }
            out
        }
    }
}

impl Program {
    fn apply(&self, src: &str) -> Option<String> {
        let toks = tokens(src);
        let get = |i: i32| -> Option<&String> {
            if i >= 0 { toks.get(i as usize) } else { toks.len().checked_sub(i.unsigned_abs() as usize).and_then(|j| toks.get(j)) }
        };
        let mut out = String::new();
        for p in &self.0 {
            match p {
                Piece::Lit(l) => out.push_str(l),
                Piece::Tok(i, c) => out.push_str(&apply_case(get(*i)?, *c)),
                Piece::Initial(i, c) => out.push_str(&apply_case(&get(*i)?.chars().next()?.to_string(), *c)),
            }
        }
        Some(out)
    }
}

/// Finds a program consistent with every example.
fn learn(examples: &[(String, String)]) -> Option<Program> {
    let (src, dst) = examples.first()?;
    let candidates = synthesize(src, dst);
    candidates.into_iter().find(|p| examples.iter().all(|(s, d)| p.apply(s).as_deref() == Some(d.as_str())))
}

/// Candidate programs for one example, by greedy segmentation of the output into token matches
/// and literals (tries several index conventions).
fn synthesize(src: &str, dst: &str) -> Vec<Program> {
    let toks = tokens(src);
    let n = toks.len() as i32;
    let mut out = Vec::new();
    for prefer_neg in [false, true] {
        let mut pieces = Vec::new();
        let mut rest = dst;
        let mut guard = 0;
        while !rest.is_empty() && guard < 64 {
            guard += 1;
            // Longest token (with any case) that prefixes `rest`.
            let mut best: Option<(usize, Piece)> = None;
            for (i, t) in toks.iter().enumerate() {
                let idx = if prefer_neg { i as i32 - n } else { i as i32 };
                for c in [Case::Same, Case::Upper, Case::Lower, Case::Proper] {
                    let cand = apply_case(t, c);
                    if !cand.is_empty() && rest.starts_with(&cand) && best.as_ref().is_none_or(|(l, _)| cand.len() > *l) {
                        best = Some((cand.len(), Piece::Tok(idx, c)));
                    }
                }
            }
            if best.as_ref().is_none_or(|(l, _)| *l <= 1) {
                // Initial letters (e.g. "J. Smith").
                for (i, t) in toks.iter().enumerate() {
                    let idx = if prefer_neg { i as i32 - n } else { i as i32 };
                    if let Some(ch) = t.chars().next() {
                        for c in [Case::Same, Case::Upper, Case::Lower] {
                            let cand = apply_case(&ch.to_string(), c);
                            if rest.starts_with(&cand) && best.is_none() {
                                best = Some((cand.len(), Piece::Initial(idx, c)));
                            }
                        }
                    }
                }
            }
            match best {
                Some((len, piece)) => {
                    pieces.push(piece);
                    rest = rest.get(len..).unwrap_or("");
                }
                None => {
                    let ch = rest.chars().next().unwrap_or(' ');
                    match pieces.last_mut() {
                        Some(Piece::Lit(l)) => l.push(ch),
                        _ => pieces.push(Piece::Lit(ch.to_string())),
                    }
                    rest = rest.get(ch.len_utf8()..).unwrap_or("");
                }
            }
        }
        out.push(Program(pieces));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_programs() {
        let p = learn(&[("John Smith".into(), "Smith, John".into())]).unwrap();
        assert_eq!(p.apply("Ada Lovelace").as_deref(), Some("Lovelace, Ada"));
        let p = learn(&[("jane doe".into(), "JD".into())]).unwrap();
        assert_eq!(p.apply("alan turing").as_deref(), Some("AT"));
        let p = learn(&[("alice@example.com".into(), "alice".into())]).unwrap();
        assert_eq!(p.apply("bob@example.com").as_deref(), Some("bob"));
    }

    #[test]
    fn split_numbers() {
        assert_eq!(split_num("Item 7"), Some(("Item ".into(), 7, "".into(), 1)));
        assert_eq!(split_num("Q003x"), Some(("Q".into(), 3, "x".into(), 3)));
        assert_eq!(split_num("abc"), None);
    }
}
