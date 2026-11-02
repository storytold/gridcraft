//! Parallel formula evaluation on native builds (WASM stays on the caller thread).

use super::graph::{Graph, HashMap, HashSet, Key};
use super::pass_host::{finish_formula, spill_cells_in_range};
use crate::eval::{Evaluator, Host, MAX_CELLS, binary_values};
use gridcraft_core::{Array, CellError, CellRef, RangeRef, Value};
use gridcraft_formula::{BinOp, Expr, SheetSel, UnOp};
use gridcraft_model::Workbook;
use std::thread::ThreadId;

pub(super) struct EvalLevelOut {
    pub values: Vec<(Key, Value)>,
    pub spills: HashMap<Key, std::sync::Arc<Array>>,
    pub deferred: Vec<Key>,
    pub threads: HashMap<Key, ThreadId>,
}

#[cfg(not(target_arch = "wasm32"))]
struct SpillCover {
    anchor: Key,
    start_row: u32,
    end_row: u32,
    start_col: u32,
    array: std::sync::Arc<Array>,
}

#[cfg(not(target_arch = "wasm32"))]
struct SpillIndex {
    by_col: HashMap<(usize, u32), Vec<SpillCover>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SpillIndex {
    fn build(spills: &HashMap<Key, std::sync::Arc<Array>>) -> Self {
        let mut by_col: HashMap<(usize, u32), Vec<SpillCover>> = HashMap::default();
        for (anchor, arr) in spills {
            let end_row = anchor.1.row.saturating_add((arr.rows as u32).saturating_sub(1));
            for cc in 0..arr.cols {
                let col = anchor.1.col.saturating_add(cc as u32);
                by_col.entry((anchor.0, col)).or_default().push(SpillCover {
                    anchor: *anchor,
                    start_row: anchor.1.row,
                    end_row,
                    start_col: anchor.1.col,
                    array: std::sync::Arc::clone(arr),
                });
            }
        }
        for v in by_col.values_mut() {
            v.sort_by_key(|c| (c.start_row, c.start_col, c.anchor.1.row, c.anchor.1.col));
        }
        SpillIndex { by_col }
    }

    fn spill_value(&self, sheet: usize, c: CellRef) -> Option<Value> {
        let list = self.by_col.get(&(sheet, c.col))?;
        let hi = list.partition_point(|cover| cover.start_row <= c.row);
        let mut best: Option<&SpillCover> = None;
        for cover in &list[..hi] {
            if c.row <= cover.end_row && c.col >= cover.start_col && c != cover.anchor.1 {
                best = match best {
                    None => Some(cover),
                    Some(b) if (cover.anchor.1.row, cover.anchor.1.col) > (b.anchor.1.row, b.anchor.1.col) => Some(cover),
                    other => other,
                };
            }
        }
        let cover = best?;
        let dr = (c.row - cover.anchor.1.row) as usize;
        let dc = (c.col - cover.anchor.1.col) as usize;
        cover.array.get(dr, dc).cloned()
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct FrozenHost<'a> {
    wb: &'a Workbook,
    prior: &'a HashMap<Key, Value>,
    spills: &'a HashMap<Key, std::sync::Arc<Array>>,
    spill_index: &'a SpillIndex,
    now: f64,
    at: Key,
    used: HashMap<usize, Option<RangeRef>>,
    pending_parallel: &'a HashSet<Key>,
    deferred: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Host for FrozenHost<'_> {
    fn workbook(&self) -> &Workbook {
        self.wb
    }

    fn spill_range(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        let k = (sheet, anchor);
        if self.prior.contains_key(&k) || self.spills.contains_key(&k) {
            let arr = self.spills.get(&k)?;
            let end = CellRef::new(
                anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)),
                anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)),
            );
            return Some(RangeRef::new(anchor, end));
        }
        self.wb.sheet(sheet)?.spill_ranges.get(&anchor).copied()
    }

    fn cell_value(&mut self, sheet: usize, c: CellRef) -> Value {
        let k = (sheet, c);
        if self.pending_parallel.contains(&k) && !self.prior.contains_key(&k) {
            self.deferred = true;
            return self.wb.sheet(sheet).map(|s| s.value(c)).unwrap_or_default();
        }
        if let Some(v) = self.prior.get(&k) {
            return v.clone();
        }
        if let Some(v) = self.spill_index.spill_value(sheet, c) {
            return v;
        }
        self.wb.sheet(sheet).map(|s| s.value(c)).unwrap_or_default()
    }

    fn range_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        let (h, w) = (range.height() as usize, range.width() as usize);
        let Some(len) = h.checked_mul(w) else { return Vec::new() };
        if len as u64 > MAX_CELLS {
            return Vec::new();
        }
        let mut out = vec![Value::Empty; len];
        let Some(sh) = self.wb.sheet(sheet) else { return out };
        for (c, cell) in sh.cells.iter_range(range) {
            let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
            let value = if cell.formula.is_some() { self.cell_value(sheet, c) } else { cell.value.clone() };
            if let Some(slot) = out.get_mut(i) {
                *slot = value;
            }
        }
        for (c, v) in sh.spill.range(range.start..=range.end) {
            if range.contains(*c) {
                let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
                if let Some(slot) = out.get_mut(i)
                    && slot.is_empty()
                {
                    *slot = v.clone();
                }
            }
        }
        for (anchor, arr) in self.spills {
            if anchor.0 != sheet {
                continue;
            }
            for r in 0..arr.rows {
                for cc in 0..arr.cols {
                    let c = CellRef::new(anchor.1.row.saturating_add(r as u32), anchor.1.col.saturating_add(cc as u32));
                    if c != anchor.1 && range.contains(c) {
                        let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
                        if let Some(slot) = out.get_mut(i) {
                            *slot = arr.get(r, cc).cloned().unwrap_or_default();
                        }
                    }
                }
            }
        }
        out
    }

    fn visit_range(&mut self, sheet: usize, range: RangeRef, f: &mut dyn FnMut(&Value) -> bool) -> bool {
        let wb = self.wb;
        let Some(sh) = wb.sheet(sheet) else { return false };
        if self.spills.is_empty() && sh.spill.is_empty() && range.height() == 1 && range.width() <= 32 {
            let mut vals: [Value; 32] = std::array::from_fn(|_| Value::Empty);
            let mut n = 0usize;
            let mut plain = true;
            for (_, cell) in sh.cells.row(range.start.row, range.start.col, range.end.col) {
                if cell.formula.is_some() || n >= vals.len() {
                    plain = false;
                    break;
                }
                vals[n] = cell.value.clone();
                n += 1;
            }
            if plain {
                for v in vals.iter().take(n) {
                    if !f(v) {
                        return true;
                    }
                }
                return true;
            }
        }
        let Some(extras) = spill_cells_in_range(sheet, range, &sh.spill, self.spills) else {
            return false;
        };
        let mut i = 0;
        for (c, cell) in sh.cells.iter_range(range) {
            while i < extras.len() && extras[i].0 < c {
                if !f(&extras[i].1) {
                    return true;
                }
                i += 1;
            }
            let go = if cell.formula.is_some() {
                if i < extras.len() && extras[i].0 == c {
                    i += 1;
                }
                let v = self.cell_value(sheet, c);
                f(&v)
            } else if i < extras.len() && extras[i].0 == c {
                let v = extras[i].1.clone();
                i += 1;
                f(&v)
            } else {
                f(&cell.value)
            };
            if !go {
                return true;
            }
        }
        while i < extras.len() {
            if !f(&extras[i].1) {
                return true;
            }
            i += 1;
        }
        true
    }

    fn used_range(&mut self, sheet: usize) -> Option<RangeRef> {
        let wb = self.wb;
        *self.used.entry(sheet).or_insert_with(|| wb.sheet(sheet).and_then(|s| s.used_range()))
    }

    fn now_serial(&self) -> f64 {
        self.now
    }

    fn random(&mut self) -> f64 {
        let mut x = 0x9E37_79B9_7F4A_7C15u64
            ^ ((self.at.0 as u64) << 32)
            ^ (self.at.1.row as u64).wrapping_mul(0x1000_0001)
            ^ (self.at.1.col as u64).wrapping_mul(0x9E37);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct FrozenIn<'a> {
    wb: &'a Workbook,
    graph: &'a Graph,
    prior: &'a HashMap<Key, Value>,
    spills: &'a HashMap<Key, std::sync::Arc<Array>>,
    spill_index: &'a SpillIndex,
    now: f64,
    pending_parallel: &'a HashSet<Key>,
    in_pass: &'a HashSet<Key>,
}

#[cfg(not(target_arch = "wasm32"))]
struct WorkerOut {
    values: Vec<(Key, Value)>,
    spills: HashMap<Key, std::sync::Arc<Array>>,
    deferred: Vec<Key>,
    threads: HashMap<Key, ThreadId>,
}

#[cfg(not(target_arch = "wasm32"))]
fn eval_flat_scalar(wb: &Workbook, sheet: usize, expr: &Expr, prior: &HashMap<Key, Value>, pending_parallel: &HashSet<Key>) -> Option<Value> {
    eval_flat_scalar_at(wb, sheet, expr, 0, prior, pending_parallel)
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn eval_flat_scalar_at(
    wb: &Workbook,
    sheet: usize,
    expr: &Expr,
    depth: u32,
    prior: &HashMap<Key, Value>,
    pending_parallel: &HashSet<Key>,
) -> Option<Value> {
    if depth > 64 {
        return None;
    }
    let next = depth + 1;
    match expr {
        Expr::Paren(e) => eval_flat_scalar_at(wb, sheet, e, next, prior, pending_parallel),
        Expr::Number(n) => Some(Value::number(*n)),
        Expr::Bool(b) => Some(Value::Bool(*b)),
        Expr::Text(t) => Some(Value::Text(std::sync::Arc::clone(t))),
        Expr::Error(e) => Some(Value::Error(*e)),
        Expr::Unary(op, e) => {
            let Value::Number(n) = eval_flat_scalar_at(wb, sheet, e, next, prior, pending_parallel)? else {
                return None;
            };
            match op {
                UnOp::Neg => Some(Value::number(-n)),
                UnOp::Plus => Some(Value::number(n)),
                UnOp::Percent => Some(Value::number(n / 100.0)),
                UnOp::At | UnOp::Spill | UnOp::Trim(_) => None,
            }
        }
        Expr::Ref(r) => {
            if !matches!(r.sheet, SheetSel::Current) {
                return None;
            }
            let range = r.range();
            if !range.is_single() {
                return None;
            }
            let key = (sheet, range.start);
            if pending_parallel.contains(&key) && !prior.contains_key(&key) {
                return None;
            }
            if let Some(v) = prior.get(&key) {
                return Some(v.clone());
            }
            Some(wb.sheet(sheet)?.value(range.start))
        }
        Expr::Binary(op, a, b) if !matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect) => {
            let x = eval_flat_scalar_at(wb, sheet, a, next, prior, pending_parallel)?;
            let y = eval_flat_scalar_at(wb, sheet, b, next, prior, pending_parallel)?;
            Some(binary_values(*op, &x, &y, wb.date_system))
        }
        Expr::Call(name, args) if matches!(name.as_str(), "SUM" | "COUNT" | "AVERAGE") => {
            eval_flat_agg(wb, sheet, name, args, next, prior, pending_parallel)
        }
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
#[allow(clippy::too_many_arguments)]
fn fold_stored_ref(
    wb: &Workbook,
    sheet: usize,
    range: RangeRef,
    sum: &mut f64,
    count: &mut u64,
    error: &mut Option<CellError>,
    prior: &HashMap<Key, Value>,
    pending_parallel: &HashSet<Key>,
) -> bool {
    let Some(sh) = wb.sheet(sheet) else { return false };
    if !sh.spill.is_empty() || !sh.spill_ranges.is_empty() {
        return false;
    }
    let mut accept = |cell: &gridcraft_model::Cell, c: CellRef| -> bool {
        if cell.formula.is_some() && (prior.contains_key(&(sheet, c)) || pending_parallel.contains(&(sheet, c))) {
            return false;
        }
        match &cell.value {
            Value::Number(n) => {
                *sum += *n;
                *count = count.saturating_add(1);
                true
            }
            Value::Error(e) => {
                if error.is_none() {
                    *error = Some(*e);
                }
                true
            }
            Value::Array(_) => false,
            _ => true,
        }
    };
    if range.height() == 1 {
        let row = range.start.row;
        for (col, cell) in sh.cells.row(row, range.start.col, range.end.col) {
            if !accept(cell, CellRef::new(row, col)) {
                return false;
            }
        }
    } else {
        for (c, cell) in sh.cells.iter_range(range) {
            if !accept(cell, c) {
                return false;
            }
        }
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
#[allow(clippy::too_many_arguments)]
fn eval_flat_agg(
    wb: &Workbook,
    sheet: usize,
    name: &str,
    args: &[Expr],
    depth: u32,
    prior: &HashMap<Key, Value>,
    pending_parallel: &HashSet<Key>,
) -> Option<Value> {
    if depth > 64 {
        return None;
    }
    if args.is_empty() {
        return Some(Value::Error(CellError::Value));
    }
    let mut sum = 0.0;
    let mut count = 0u64;
    let mut error: Option<CellError> = None;
    for a in args {
        match a {
            Expr::Paren(e) => match eval_flat_scalar_at(wb, sheet, e, depth + 1, prior, pending_parallel)? {
                Value::Number(n) => {
                    sum += n;
                    count = count.saturating_add(1);
                }
                Value::Error(e) => {
                    if error.is_none() {
                        error = Some(e);
                    }
                }
                Value::Bool(_) | Value::Text(_) | Value::Empty | Value::Array(_) => return None,
            },
            Expr::Number(n) => match Value::number(*n) {
                Value::Number(n) => {
                    sum += n;
                    count = count.saturating_add(1);
                }
                Value::Error(e) => {
                    if error.is_none() {
                        error = Some(e);
                    }
                }
                _ => return None,
            },
            Expr::Ref(r) if matches!(r.sheet, SheetSel::Current) => {
                if !fold_stored_ref(wb, sheet, r.range(), &mut sum, &mut count, &mut error, prior, pending_parallel) {
                    return None;
                }
            }
            _ => return None,
        }
    }
    let value = if name != "COUNT"
        && let Some(e) = error
    {
        Value::Error(e)
    } else if name == "COUNT" {
        Value::number(count as f64)
    } else if name == "AVERAGE" {
        if count == 0 { Value::Error(CellError::Div0) } else { Value::number(sum / count as f64) }
    } else {
        Value::number(sum)
    };
    Some(value)
}

#[cfg(not(target_arch = "wasm32"))]
fn eval_frozen(inp: &FrozenIn<'_>, k: Key, legacy: Option<RangeRef>) -> Result<(Value, Option<std::sync::Arc<Array>>), ()> {
    let Some(expr) = inp.graph.nodes.get(&k).and_then(|n| n.expr.as_ref()) else {
        return Ok((inp.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default(), None));
    };
    if legacy.is_none()
        && let Some(v) = eval_flat_scalar(inp.wb, k.0, expr, inp.prior, inp.pending_parallel)
        && !v.is_empty()
        && !matches!(v, Value::Array(_))
    {
        return Ok((v, None));
    }
    let mut host = FrozenHost {
        wb: inp.wb,
        prior: inp.prior,
        spills: inp.spills,
        spill_index: inp.spill_index,
        now: inp.now,
        at: k,
        used: HashMap::default(),
        pending_parallel: inp.pending_parallel,
        deferred: false,
    };
    let v = {
        let mut ev = Evaluator::new(&mut host, k.0, k.1);
        ev.value(expr)
    };
    if host.deferred {
        return Err(());
    }
    Ok(finish_formula(inp.wb, inp.in_pass, k, expr, v, legacy, true))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_level(
    wb: &Workbook,
    graph: &Graph,
    keys: &[Key],
    prior: &HashMap<Key, Value>,
    spills: &HashMap<Key, std::sync::Arc<Array>>,
    now: f64,
    threads: usize,
    pending: &HashSet<Key>,
    in_pass: &HashSet<Key>,
) -> Option<EvalLevelOut> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (wb, graph, keys, prior, spills, now, threads, pending, in_pass);
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        eval_level_native(wb, graph, keys, prior, spills, now, threads, pending, in_pass)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn eval_level_native(
    wb: &Workbook,
    graph: &Graph,
    keys: &[Key],
    prior: &HashMap<Key, Value>,
    spills: &HashMap<Key, std::sync::Arc<Array>>,
    now: f64,
    threads: usize,
    pending: &HashSet<Key>,
    in_pass: &HashSet<Key>,
) -> Option<EvalLevelOut> {
    if keys.is_empty() {
        return Some(EvalLevelOut { values: Vec::new(), spills: HashMap::default(), deferred: Vec::new(), threads: HashMap::default() });
    }
    let spill_index = SpillIndex::build(spills);
    let spill_ref = &spill_index;
    let pieces = threads.clamp(1, keys.len());
    let width = keys.len().div_ceil(pieces);
    let slices: Vec<&[Key]> = keys.chunks(width).collect();
    let mut outs: Vec<WorkerOut> = Vec::new();
    let mut failed = false;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in slices {
            match std::thread::Builder::new().name("gridcraft-calc".into()).spawn_scoped(scope, move || {
                let mut out = WorkerOut { values: Vec::new(), spills: HashMap::default(), deferred: Vec::new(), threads: HashMap::default() };
                let inp = FrozenIn { wb, graph, prior, spills, spill_index: spill_ref, now, pending_parallel: pending, in_pass };
                for &k in chunk {
                    let legacy = graph.nodes.get(&k).and_then(|n| n.legacy);
                    match eval_frozen(&inp, k, legacy) {
                        Ok((v, sp)) => {
                            if let Some(a) = sp {
                                out.spills.insert(k, a);
                            }
                            out.values.push((k, v));
                            #[cfg(test)]
                            out.threads.insert(k, std::thread::current().id());
                        }
                        Err(()) => out.deferred.push(k),
                    }
                }
                out
            }) {
                Ok(h) => handles.push(h),
                Err(_) => break,
            }
        }
        failed = handles.is_empty();
        for h in handles {
            match h.join() {
                Ok(out) => outs.push(out),
                Err(_) => failed = true,
            }
        }
    });
    if failed {
        return None;
    }
    let mut values = Vec::new();
    let mut spill_out = HashMap::default();
    let mut deferred = Vec::new();
    let mut thread_map = HashMap::default();
    for out in outs {
        values.extend(out.values);
        spill_out.extend(out.spills);
        deferred.extend(out.deferred);
        thread_map.extend(out.threads);
    }
    Some(EvalLevelOut { values, spills: spill_out, deferred, threads: thread_map })
}
