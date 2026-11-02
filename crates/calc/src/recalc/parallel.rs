//! Parallel formula evaluation on native builds (WASM stays on the caller thread).

use super::graph::{Graph, HashMap, HashSet, Key};
use super::pass_host::{finish_formula, spill_cells_in_range};
use crate::eval::{Evaluator, Host, MAX_CELLS};
use gridcraft_core::{Array, CellRef, RangeRef, Value};
use gridcraft_formula::Expr;
use gridcraft_model::Workbook;
use std::thread::ThreadId;

const CHUNK: usize = 32;

pub(super) struct EvalLevelOut {
    pub values: HashMap<Key, Value>,
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
    exprs: &'a HashMap<Key, std::sync::Arc<Expr>>,
    prior: &'a HashMap<Key, Value>,
    spills: &'a HashMap<Key, std::sync::Arc<Array>>,
    spill_index: &'a SpillIndex,
    now: f64,
    pending_parallel: &'a HashSet<Key>,
}

#[cfg(not(target_arch = "wasm32"))]
struct WorkerOut {
    values: HashMap<Key, Value>,
    spills: HashMap<Key, std::sync::Arc<Array>>,
    deferred: Vec<Key>,
    threads: HashMap<Key, ThreadId>,
}

#[cfg(not(target_arch = "wasm32"))]
fn eval_frozen(inp: &FrozenIn<'_>, k: Key, legacy: Option<RangeRef>) -> Result<(Value, Option<std::sync::Arc<Array>>), ()> {
    let Some(expr) = inp.exprs.get(&k) else {
        return Ok((inp.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default(), None));
    };
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
    Ok(finish_formula(inp.wb, inp.exprs, k, expr, v, legacy, true))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_level(
    wb: &Workbook,
    graph: &Graph,
    exprs: &HashMap<Key, std::sync::Arc<Expr>>,
    keys: &[Key],
    prior: &HashMap<Key, Value>,
    spills: &HashMap<Key, std::sync::Arc<Array>>,
    now: f64,
    threads: usize,
    pending: &HashSet<Key>,
) -> Option<EvalLevelOut> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (wb, graph, exprs, keys, prior, spills, now, threads, pending);
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        eval_level_native(wb, graph, exprs, keys, prior, spills, now, threads, pending)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn eval_level_native(
    wb: &Workbook,
    graph: &Graph,
    exprs: &HashMap<Key, std::sync::Arc<Expr>>,
    keys: &[Key],
    prior: &HashMap<Key, Value>,
    spills: &HashMap<Key, std::sync::Arc<Array>>,
    now: f64,
    threads: usize,
    pending: &HashSet<Key>,
) -> Option<EvalLevelOut> {
    if keys.is_empty() {
        return Some(EvalLevelOut { values: HashMap::default(), spills: HashMap::default(), deferred: Vec::new(), threads: HashMap::default() });
    }
    let spill_index = SpillIndex::build(spills);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let work = || {
        let mut out = WorkerOut { values: HashMap::default(), spills: HashMap::default(), deferred: Vec::new(), threads: HashMap::default() };
        let inp = FrozenIn { wb, exprs, prior, spills, spill_index: &spill_index, now, pending_parallel: pending };
        loop {
            let i = next.fetch_add(CHUNK, std::sync::atomic::Ordering::Relaxed);
            let Some(chunk) = keys.get(i..keys.len().min(i.saturating_add(CHUNK))) else { break };
            if chunk.is_empty() {
                break;
            }
            for &k in chunk {
                let legacy = graph.nodes.get(&k).and_then(|n| n.legacy);
                match eval_frozen(&inp, k, legacy) {
                    Ok((v, sp)) => {
                        if let Some(a) = sp {
                            out.spills.insert(k, a);
                        }
                        out.values.insert(k, v);
                        out.threads.insert(k, std::thread::current().id());
                    }
                    Err(()) => out.deferred.push(k),
                }
            }
        }
        out
    };
    let mut outs: Vec<WorkerOut> = Vec::new();
    let mut failed = false;
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads.min(keys.len().div_ceil(CHUNK)) {
            match std::thread::Builder::new().name("gridcraft-calc".into()).spawn_scoped(scope, work) {
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
    let mut values = HashMap::default();
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
