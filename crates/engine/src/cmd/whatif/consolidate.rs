use super::group_key::GroupKey;
use super::{
    Cell, CellRef, EngineError, HashMap, Json, RangeRef, Result, Session, StyleId, Value, bad, bool_param, edit, json, parse_range, str_param,
};

// ---------------------------------------------------------------- consolidate

#[derive(Clone, Copy, Debug, Default)]
struct Agg {
    count: u64,
    nums: u64,
    sum: f64,
    product: f64,
    min: f64,
    max: f64,
}

impl Agg {
    fn add(&mut self, v: &Value) {
        if v.is_empty() {
            return;
        }
        self.count += 1;
        if let Value::Number(n) = v {
            if self.nums == 0 {
                self.min = *n;
                self.max = *n;
                self.product = 1.0;
            }
            self.nums += 1;
            self.sum += n;
            self.product *= n;
            self.min = self.min.min(*n);
            self.max = self.max.max(*n);
        }
    }
    fn result(&self, func: &str) -> Value {
        match func {
            "count" => Value::number(self.count as f64),
            "countNums" | "countnums" => Value::number(self.nums as f64),
            "average" => {
                if self.nums == 0 {
                    Value::Error(gridcraft_core::CellError::Div0)
                } else {
                    Value::number(self.sum / self.nums as f64)
                }
            }
            "max" => Value::number(if self.nums == 0 { 0.0 } else { self.max }),
            "min" => Value::number(if self.nums == 0 { 0.0 } else { self.min }),
            "product" => Value::number(if self.nums == 0 { 0.0 } else { self.product }),
            _ => Value::number(self.sum),
        }
    }
}

/// A row/column key: a position or a label (matched case-insensitively).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Pos(u32),
    Label(GroupKey),
}

fn label_value(value: Value) -> Option<Value> {
    match value.scalar() {
        Value::Empty | Value::Array(_) => None,
        // Keep Consolidate's existing trimming rule for text categories. An empty
        // string returned by a formula is a value, while an unset cell has no label.
        Value::Text(text) if !text.is_empty() => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| Value::text(trimmed))
        }
        value => Some(value),
    }
}

const MAX_CONSOLIDATE_CELLS: u64 = 5_000_000;

/// The cells a consolidated table writes: every row key meets every column key, plus a label for
/// each key on the sides that have labels.
fn output_cells(rows: usize, cols: usize, top: bool, left: bool) -> u64 {
    let (r, c) = (rows as u64, cols as u64);
    r.saturating_mul(c).saturating_add(if top { c } else { 0 }).saturating_add(if left { r } else { 0 })
}

#[cfg(test)]
#[test]
fn the_output_limit_counts_label_cells() {
    // A million row keys and four column keys are 4 million cells, 5 million with the row labels.
    assert_eq!(output_cells(1_000_000, 4, false, false), 4_000_000);
    assert_eq!(output_cells(1_000_000, 4, false, true), 5_000_000);
    assert!(output_cells(1_000_000, 5, false, true) > MAX_CONSOLIDATE_CELLS);
    assert_eq!(output_cells(3, 2, true, true), 6 + 2 + 3);
    assert_eq!(output_cells(usize::MAX, usize::MAX, true, true), u64::MAX);
}

pub(super) fn consolidate(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "data.consolidate";
    let func = str_param(p, "function").unwrap_or("sum").to_string();
    if !["sum", "count", "countNums", "countnums", "average", "max", "min", "product"].contains(&func.as_str()) {
        return Err(bad(CMD, format!("unknown function `{func}`")));
    }
    let top = bool_param(p, "topRow").unwrap_or(false);
    let left = bool_param(p, "leftColumn").unwrap_or(false);
    let d = s.doc()?;
    let srcs: Vec<String> = p
        .get("sources")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .or_else(|| str_param(p, "sources").map(|t| t.split(';').map(str::to_string).collect()))
        .unwrap_or_default();
    if srcs.is_empty() {
        return Err(bad(CMD, "give at least one source range in `sources`"));
    }
    if srcs.len() > 255 {
        return Err(bad(CMD, "too many sources (255 max)"));
    }
    let (dsi, dest) = match str_param(p, "destination") {
        Some(t) => {
            let (si, r) = parse_range(&d.wb, t, CMD)?;
            (si, r.start)
        }
        None => (d.wb.active_sheet(), d.selection.active),
    };
    // Keys in first-seen order, each with its index: a lookup is one probe, not a scan.
    let mut row_keys: Vec<Key> = Vec::new();
    let mut col_keys: Vec<Key> = Vec::new();
    let mut row_index: HashMap<Key, usize> = HashMap::new();
    let mut col_index: HashMap<Key, usize> = HashMap::new();
    let mut row_labels: HashMap<usize, Value> = HashMap::new();
    let mut col_labels: HashMap<usize, Value> = HashMap::new();
    let mut aggs: HashMap<(usize, usize), Agg> = HashMap::new();
    let mut total: u64 = 0;
    let index_of = |keys: &mut Vec<Key>, index: &mut HashMap<Key, usize>, k: Key| -> usize {
        *index.entry(k).or_insert_with_key(|k| {
            keys.push(k.clone());
            keys.len() - 1
        })
    };
    for t in &srcs {
        let (si, r) = parse_range(&d.wb, t, CMD)?;
        total = total.saturating_add(r.count());
        if total > MAX_CONSOLIDATE_CELLS {
            return Err(bad(CMD, "the sources are too large"));
        }
        let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
        let r0 = r.start.row + top as u32;
        let c0 = r.start.col + left as u32;
        if r0 > r.end.row || c0 > r.end.col {
            continue;
        }
        // Column keys of this source.
        let mut cols: Vec<(u32, usize)> = Vec::new();
        for col in c0..=r.end.col {
            let ci = if top {
                let Some(label) = label_value(sh.value(CellRef::new(r.start.row, col))) else {
                    continue;
                };
                let ci = index_of(&mut col_keys, &mut col_index, Key::Label(GroupKey::from_value(&label)));
                col_labels.entry(ci).or_insert(label);
                ci
            } else {
                index_of(&mut col_keys, &mut col_index, Key::Pos(col - c0))
            };
            cols.push((col, ci));
        }
        for row in r0..=r.end.row {
            let ri = if left {
                let Some(label) = label_value(sh.value(CellRef::new(row, r.start.col))) else {
                    continue;
                };
                let ri = index_of(&mut row_keys, &mut row_index, Key::Label(GroupKey::from_value(&label)));
                row_labels.entry(ri).or_insert(label);
                ri
            } else {
                index_of(&mut row_keys, &mut row_index, Key::Pos(row - r0))
            };
            for (col, ci) in &cols {
                aggs.entry((ri, *ci)).or_default().add(&sh.value(CellRef::new(row, *col)));
            }
        }
    }
    let (out_r0, out_c0) = (dest.row + top as u32, dest.col + left as u32);
    if out_r0 as u64 + row_keys.len() as u64 > gridcraft_core::MAX_ROWS as u64
        || out_c0 as u64 + col_keys.len() as u64 > gridcraft_core::MAX_COLS as u64
    {
        return Err(EngineError::Other("The consolidated data doesn't fit below and right of the destination.".into()));
    }
    if output_cells(row_keys.len(), col_keys.len(), top, left) > MAX_CONSOLIDATE_CELLS {
        return Err(bad(CMD, "the consolidated table would be too large"));
    }
    let mut cells: Vec<(CellRef, Value)> = Vec::new();
    if top {
        for ci in 0..col_keys.len() {
            cells.push((CellRef::new(dest.row, out_c0 + ci as u32), col_labels.remove(&ci).unwrap_or_default()));
        }
    }
    if left {
        for ri in 0..row_keys.len() {
            cells.push((CellRef::new(out_r0 + ri as u32, dest.col), row_labels.remove(&ri).unwrap_or_default()));
        }
    }
    for ri in 0..row_keys.len() {
        for ci in 0..col_keys.len() {
            let v = aggs.get(&(ri, ci)).map(|a| if a.count == 0 { Value::Empty } else { a.result(&func) }).unwrap_or_default();
            cells.push((CellRef::new(out_r0 + ri as u32, out_c0 + ci as u32), v));
        }
    }
    let end = CellRef::new(
        (out_r0 + row_keys.len() as u32).saturating_sub(1).max(dest.row),
        (out_c0 + col_keys.len() as u32).saturating_sub(1).max(dest.col),
    );
    edit(s, |cx| {
        for (c, v) in cells {
            let sh = cx.sheet_mut(dsi)?;
            let style = sh.style_id(c);
            if v.is_empty() && style == StyleId::DEFAULT {
                sh.remove_cell(c);
            } else {
                sh.set_cell(c, Cell { value: v, formula: None, style, py: None });
            }
            cx.touch(dsi, c);
        }
        Ok(())
    })?;
    Ok(json!({"range": RangeRef::new(dest, end).a1(), "rows": row_keys.len(), "columns": col_keys.len()}))
}
