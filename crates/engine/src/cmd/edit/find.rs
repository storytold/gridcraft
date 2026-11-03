use super::cell::input_to_cell;
use super::*;

// ---------------------------------------------------------------- find & replace

fn matches(hay: &str, what: &str, case: bool, whole: bool) -> bool {
    let (h, w) = if case { (hay.to_string(), what.to_string()) } else { (hay.to_lowercase(), what.to_lowercase()) };
    let has_wild = w.contains(['*', '?']);
    if has_wild {
        let pat = if whole { w } else { format!("*{w}*") };
        return wildcard(&h, &pat);
    }
    if whole { h == w } else { h.contains(&w) }
}

/// `*`/`?` wildcard match with `~` escapes.
pub(crate) fn wildcard(text: &str, pat: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    let mut p: Vec<(char, bool)> = Vec::new();
    let mut it = pat.chars();
    while let Some(c) = it.next() {
        if c == '~' {
            if let Some(n) = it.next() {
                p.push((n, true));
            }
        } else {
            p.push((c, false));
        }
    }
    let (mut ti, mut pi) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == ('?', false) || (p[pi].0 == t[ti] && p[pi] != ('*', false))) {
            ti += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == ('*', false) {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == ('*', false) {
        pi += 1;
    }
    pi == p.len()
}

pub(super) fn find(s: &mut Session, p: &Json) -> Result<Json> {
    let what = str_param(p, "what").unwrap_or("").to_string();
    if what.is_empty() {
        return Err(bad("edit.find", "missing `what`"));
    }
    let case = bool_param(p, "matchCase").unwrap_or(false);
    let whole = bool_param(p, "wholeCell").unwrap_or(false);
    let values = str_param(p, "lookIn") == Some("values");
    let by_cols = bool_param(p, "byColumns").unwrap_or(false);
    let all = bool_param(p, "all").unwrap_or(false);
    let workbook = str_param(p, "within") == Some("workbook");
    s.last_find = Some(p.clone());
    let d = s.doc()?;
    let sheets: Vec<usize> = if workbook { (0..d.wb.sheets.len()).collect() } else { vec![d.wb.active_sheet()] };
    let mut hits: Vec<(usize, CellRef, String)> = Vec::new();
    for si in sheets {
        let Some(sh) = d.wb.sheet(si) else { continue };
        let mut cells: Vec<(CellRef, String)> = sh
            .cells
            .iter()
            .filter_map(|(c, cell)| {
                let text = if values || cell.formula.is_none() || crate::display::formula_hidden(&d.wb, sh, c) {
                    crate::display::cell_text(&d.wb, sh, c)
                } else {
                    cell.input_text()
                };
                matches(&text, &what, case, whole).then_some((c, text))
            })
            .collect();
        if by_cols {
            cells.sort_by_key(|(c, _)| (c.col, c.row));
        }
        hits.extend(cells.into_iter().map(|(c, t)| (si, c, t)));
    }
    if hits.is_empty() {
        return Err(EngineError::Other(format!("We couldn't find what you were looking for: {what}")));
    }
    if all {
        let list: Vec<Json> =
            hits.iter().map(|(si, c, t)| json!({"sheet": d.wb.sheet(*si).map(|s| s.name.clone()), "cell": c.a1(), "value": t})).collect();
        return Ok(json!({"count": list.len(), "results": list}));
    }
    // Next hit after the active cell.
    let cur = (d.wb.active_sheet(), d.selection.active);
    let key = |si: usize, c: CellRef| if by_cols { (si, c.col, c.row) } else { (si, c.row, c.col) };
    let next = hits.iter().find(|(si, c, _)| key(*si, *c) > key(cur.0, cur.1)).or_else(|| hits.first()).cloned();
    let Some((si, c, t)) = next else { return Err(EngineError::Other("not found".into())) };
    let dd = s.doc_mut()?;
    dd.wb_switch_sheet(si);
    dd.selection = crate::Selection::at(c);
    Ok(json!({"cell": c.a1(), "sheet": si, "value": t, "count": hits.len()}))
}

pub(super) fn replace(s: &mut Session, p: &Json) -> Result<Json> {
    let what = str_param(p, "what").unwrap_or("").to_string();
    let with = str_param(p, "with").unwrap_or("").to_string();
    if what.is_empty() {
        return Err(bad("edit.replace", "missing `what`"));
    }
    let case = bool_param(p, "matchCase").unwrap_or(false);
    let whole = bool_param(p, "wholeCell").unwrap_or(false);
    let all = bool_param(p, "all").unwrap_or(true);
    let workbook = str_param(p, "within") == Some("workbook");
    let active = s.doc()?.selection.active;
    let n = edit(s, |cx| {
        let sheets: Vec<usize> = if workbook { (0..cx.wb.sheets.len()).collect() } else { vec![cx.wb.active_sheet()] };
        let mut n = 0;
        for si in sheets {
            let cells: Vec<(CellRef, String)> =
                cx.wb.sheet(si).map(|sh| sh.cells.iter().map(|(c, _)| (c, sh.input_text(c))).collect()).unwrap_or_default();
            for (c, text) in cells {
                if !all && c != active {
                    continue;
                }
                if !matches(&text, &what, case, whole) {
                    continue;
                }
                let new = if whole {
                    with.clone()
                } else if case {
                    text.replace(&what, &with)
                } else {
                    replace_ci(&text, &what, &with)
                };
                let cell = input_to_cell(&new, si, c, &mut cx.wb).unwrap_or_else(|_| Some(Cell::value(Value::text(new.as_str()))));
                let sh = cx.sheet_mut(si)?;
                match cell {
                    Some(cell) => sh.set_cell(c, cell),
                    None => {
                        sh.remove_cell(c);
                    }
                }
                cx.touch(si, c);
                n += 1;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"replaced": n}))
}

/// Whether case-folding `s` maps every char to exactly one char of the same UTF-8 length, so
/// byte positions in the folded text still line up with the original's character boundaries.
fn folds_in_place(s: &str) -> bool {
    s.chars().all(|c| {
        let mut folded = c.to_lowercase();
        let first = folded.next();
        folded.next().is_none() && first.is_some_and(|l| l.len_utf8() == c.len_utf8())
    })
}

pub(super) fn replace_ci(text: &str, what: &str, with: &str) -> String {
    let lower = text.to_lowercase();
    let w = what.to_lowercase();
    // Slicing `text` at positions found in `lower` is only sound when folding kept every byte
    // boundary: a length check alone misses foldings that keep the total length while shifting
    // boundaries (U+212A KELVIN SIGN shrinks 3→1 bytes, U+0130 grows 2→3), where the tail used
    // to be dropped. Those fall back to the case-sensitive replace, as length changes already do.
    if lower.len() != text.len() || w.is_empty() || !folds_in_place(text) || !folds_in_place(what) {
        return text.replace(what, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(pos) = lower.get(i..).and_then(|s| s.find(&w)) {
        out.push_str(text.get(i..i + pos).unwrap_or(""));
        out.push_str(with);
        i += pos + w.len();
    }
    out.push_str(text.get(i..).unwrap_or(""));
    out
}
