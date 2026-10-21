//! Parallel formula evaluation on native builds (WASM stays on the caller thread).

use super::graph::{Graph, HashMap, HashSet, Key, Node};
use super::pass_host::{finish_formula, spill_cells_in_range};
use crate::eval::{Area, Evaluator, Host, MAX_CELLS, binary_values};
use gridcraft_core::{Array, CellError, CellRef, RangeRef, Value};
use gridcraft_formula::{BinOp, Expr, SheetSel, UnOp};
use gridcraft_model::Workbook;

pub(super) struct FlatWave {
    pub pairs: Vec<(Key, Value)>,
    pub spills: HashMap<Key, std::sync::Arc<Array>>,
}

pub(super) struct ParallelBatch {
    pub values: HashMap<Key, Value>,
    pub spills: HashMap<Key, std::sync::Arc<Array>>,
    pub done: HashSet<Key>,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const PARALLEL_MIN: usize = 64;
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const MAX_LIGHT_PREDS: usize = 4_096;

pub(super) fn eval_flat_wave(wb: &Workbook, graph: &Graph, dirty: &[Key], now: f64, workers: Option<u8>) -> Option<FlatWave> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (wb, graph, dirty, now, workers);
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        eval_flat_wave_native(wb, graph, dirty, now, workers)
    }
}

pub(super) fn eval_independent(
    wb: &Workbook,
    graph: &Graph,
    exprs: &HashMap<Key, std::sync::Arc<Expr>>,
    dirty: &[Key],
    now: f64,
    workers: Option<u8>,
) -> Option<ParallelBatch> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (wb, graph, exprs, dirty, now, workers);
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        eval_independent_native(wb, graph, exprs, dirty, now, workers)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_count(jobs: usize, cap: Option<u8>) -> usize {
    let machine = std::thread::available_parallelism().map(|p| p.get()).unwrap_or(1).clamp(1, 8);
    let n = match cap {
        Some(c) => usize::from(c).clamp(1, 8),
        None => machine,
    };
    let n = n.min(jobs.max(1));
    if n >= 2 {
        n
    } else if cap.is_none() && cfg!(test) && jobs >= PARALLEL_MIN {
        2
    } else {
        1
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct DirtyIndex {
    set: HashSet<Key>,
    cols: HashMap<(usize, u32), Vec<u32>>,
}

#[cfg(not(target_arch = "wasm32"))]
fn dirty_index(keys: &[Key]) -> DirtyIndex {
    let mut set = HashSet::with_capacity_and_hasher(keys.len(), Default::default());
    let mut cols: HashMap<(usize, u32), Vec<u32>> = HashMap::default();
    for k in keys {
        if set.insert(*k) {
            cols.entry((k.0, k.1.col)).or_default().push(k.1.row);
        }
    }
    for rows in cols.values_mut() {
        rows.sort_unstable();
        rows.dedup();
    }
    DirtyIndex { set, cols }
}

#[cfg(not(target_arch = "wasm32"))]
fn has_dirty_precedent(index: &DirtyIndex, areas: &[Area]) -> bool {
    for area in areas {
        if area.range.is_single() {
            if index.set.contains(&(area.sheet, area.range.start)) {
                return true;
            }
        } else {
            let (c0, c1) = (area.range.start.col, area.range.end.col);
            let (r0, r1) = (area.range.start.row, area.range.end.row);
            for ((sheet, col), rows) in &index.cols {
                if *sheet != area.sheet || *col < c0 || *col > c1 {
                    continue;
                }
                let i = rows.partition_point(|r| *r < r0);
                if rows.get(i..).is_some_and(|tail| tail.iter().any(|r| *r <= r1)) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(not(target_arch = "wasm32"))]
fn precedent_keys(index: &DirtyIndex, areas: &[Area]) -> Result<Vec<Key>, ()> {
    let mut out = Vec::new();
    let mut seen: HashSet<Key> = HashSet::default();
    for area in areas {
        if area.range.is_single() {
            let p = (area.sheet, area.range.start);
            if index.set.contains(&p) && seen.insert(p) {
                out.push(p);
            }
        } else {
            let (c0, c1) = (area.range.start.col, area.range.end.col);
            let (r0, r1) = (area.range.start.row, area.range.end.row);
            for ((sheet, col), rows) in &index.cols {
                if *sheet != area.sheet || *col < c0 || *col > c1 {
                    continue;
                }
                let i = rows.partition_point(|r| *r < r0);
                for row in rows.get(i..).unwrap_or(&[]).iter().take_while(|r| **r <= r1) {
                    let p = (*sheet, CellRef::new(*row, *col));
                    if seen.insert(p) {
                        out.push(p);
                        if out.len() > MAX_LIGHT_PREDS {
                            return Err(());
                        }
                    }
                }
            }
        }
        if out.len() > MAX_LIGHT_PREDS {
            return Err(());
        }
    }
    Ok(out)
}

#[cfg(not(target_arch = "wasm32"))]
fn column_wave_is_clear(graph: &Graph, dirty: &[Key]) -> bool {
    let Some(first) = dirty.first() else { return false };
    let (sheet, col) = (first.0, first.1.col);
    for k in dirty {
        if k.0 != sheet || k.1.col != col {
            return false;
        }
        let Some(node) = graph.nodes.get(k) else { return false };
        if node.dynamic || node.expr.is_none() {
            return false;
        }
        if node.areas.iter().any(|a| a.sheet == sheet && a.range.start.col <= col && col <= a.range.end.col) {
            return false;
        }
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn call_always_scalar(name: &str) -> bool {
    matches!(
        name,
        "SUM"
            | "COUNT"
            | "AVERAGE"
            | "SUMIF"
            | "SUMIFS"
            | "COUNTIF"
            | "COUNTIFS"
            | "AVERAGEIF"
            | "AVERAGEIFS"
            | "SUMPRODUCT"
            | "MIN"
            | "MAX"
            | "MINA"
            | "MAXA"
            | "COUNTA"
            | "COUNTBLANK"
            | "AND"
            | "OR"
            | "XOR"
            | "PRODUCT"
            | "MEDIAN"
            | "AGGREGATE"
            | "SUBTOTAL"
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn expr_might_spill(expr: &Expr) -> bool {
    let mut stack = vec![expr];
    while let Some(expr) = stack.pop() {
        match expr {
            Expr::Paren(e) => stack.push(e),
            Expr::Number(_) | Expr::Bool(_) | Expr::Text(_) | Expr::Error(_) | Expr::Missing => {}
            Expr::Array(rows) => {
                let n: usize = rows.iter().map(|r| r.len()).sum();
                if n != 1 {
                    return true;
                }
                if let Some(e) = rows.first().and_then(|r| r.first()) {
                    stack.push(e);
                }
            }
            Expr::Ref(r) => {
                if !r.range().is_single() {
                    return true;
                }
            }
            Expr::Name(_) | Expr::Struct(_) | Expr::Invoke(_, _) => return true,
            Expr::Unary(op, e) => match op {
                UnOp::Neg | UnOp::Plus | UnOp::Percent => stack.push(e),
                UnOp::At => {}
                UnOp::Spill | UnOp::Trim(_) => return true,
            },
            Expr::Binary(op, a, b) => match op {
                BinOp::Range | BinOp::Union | BinOp::Intersect => return true,
                _ => {
                    stack.push(a);
                    stack.push(b);
                }
            },
            Expr::Call(name, _) => {
                if !call_always_scalar(name) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(not(target_arch = "wasm32"))]
enum SpillKind {
    Known(RangeRef),
    Unknown,
}

#[cfg(not(target_arch = "wasm32"))]
fn spill_kind(node: &Node) -> Option<SpillKind> {
    if let Some(r) = node.legacy {
        return Some(SpillKind::Known(r));
    }
    if node.expr.as_ref().is_some_and(|e| expr_might_spill(e)) {
        return Some(SpillKind::Unknown);
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn area_meets_spill(area: &Area, spiller: Key, kind: &SpillKind) -> bool {
    match kind {
        SpillKind::Known(r) => area.sheet == spiller.0 && area.range.intersects(r),
        SpillKind::Unknown => area.sheet == spiller.0 && area.range.end.row >= spiller.1.row && area.range.end.col >= spiller.1.col,
    }
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
fn eval_frozen(
    wb: &Workbook,
    exprs: &HashMap<Key, std::sync::Arc<Expr>>,
    prior: &HashMap<Key, Value>,
    spills: &HashMap<Key, std::sync::Arc<Array>>,
    spill_index: &SpillIndex,
    now: f64,
    k: Key,
    legacy: Option<RangeRef>,
) -> (Value, Option<std::sync::Arc<Array>>) {
    let Some(expr) = exprs.get(&k) else {
        return (wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default(), None);
    };
    let mut host = FrozenHost { wb, prior, spills, spill_index, now, at: k, used: HashMap::default() };
    let v = {
        let mut ev = Evaluator::new(&mut host, k.0, k.1);
        ev.value(expr)
    };
    finish_formula(wb, exprs, k, expr, v, legacy, true)
}

#[cfg(not(target_arch = "wasm32"))]
fn eval_flat_scalar(wb: &Workbook, sheet: usize, expr: &Expr) -> Option<Value> {
    eval_flat_scalar_at(wb, sheet, expr, 0)
}

#[cfg(not(target_arch = "wasm32"))]
fn eval_flat_scalar_at(wb: &Workbook, sheet: usize, expr: &Expr, depth: u32) -> Option<Value> {
    if depth > 64 {
        return None;
    }
    let next = depth + 1;
    match expr {
        Expr::Paren(e) => eval_flat_scalar_at(wb, sheet, e, next),
        Expr::Number(n) => Some(Value::number(*n)),
        Expr::Bool(b) => Some(Value::Bool(*b)),
        Expr::Text(t) => Some(Value::Text(std::sync::Arc::clone(t))),
        Expr::Error(e) => Some(Value::Error(*e)),
        Expr::Unary(op, e) => {
            let Value::Number(n) = eval_flat_scalar_at(wb, sheet, e, next)? else {
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
            Some(wb.sheet(sheet)?.value(range.start))
        }
        Expr::Binary(op, a, b) if !matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect) => {
            let x = eval_flat_scalar_at(wb, sheet, a, next)?;
            let y = eval_flat_scalar_at(wb, sheet, b, next)?;
            Some(binary_values(*op, &x, &y, wb.date_system))
        }
        Expr::Call(name, args) if matches!(name.as_str(), "SUM" | "COUNT" | "AVERAGE") => eval_flat_agg(wb, sheet, name, args, next),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fold_stored_ref(wb: &Workbook, sheet: usize, range: RangeRef, sum: &mut f64, count: &mut u64, error: &mut Option<CellError>) -> bool {
    let Some(sh) = wb.sheet(sheet) else { return false };
    if !sh.spill.is_empty() || !sh.spill_ranges.is_empty() {
        return false;
    }
    let mut accept = |cell: &gridcraft_model::Cell| -> bool {
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
        for (_, cell) in sh.cells.row(range.start.row, range.start.col, range.end.col) {
            if !accept(cell) {
                return false;
            }
        }
    } else {
        for (_, cell) in sh.cells.iter_range(range) {
            if !accept(cell) {
                return false;
            }
        }
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn eval_flat_agg(wb: &Workbook, sheet: usize, name: &str, args: &[Expr], depth: u32) -> Option<Value> {
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
            Expr::Paren(e) => match eval_flat_scalar_at(wb, sheet, e, depth + 1)? {
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
                if !fold_stored_ref(wb, sheet, r.range(), &mut sum, &mut count, &mut error) {
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
fn sheets_have_spills(wb: &Workbook) -> bool {
    wb.sheets.iter().any(|s| !s.spill.is_empty() || !s.spill_ranges.is_empty())
}

#[cfg(not(target_arch = "wasm32"))]
fn eval_flat_wave_native(wb: &Workbook, graph: &Graph, dirty: &[Key], now: f64, workers: Option<u8>) -> Option<FlatWave> {
    if dirty.len() < PARALLEL_MIN || worker_count(dirty.len(), workers) < 2 || sheets_have_spills(wb) {
        return None;
    }
    for k in dirty {
        let node = graph.nodes.get(k)?;
        if node.dynamic || node.expr.is_none() {
            return None;
        }
        if node.legacy.is_some() || node.expr.as_ref().is_some_and(|e| expr_might_spill(e)) {
            return None;
        }
    }
    if !column_wave_is_clear(graph, dirty) {
        let index = dirty_index(dirty);
        for k in dirty {
            let node = graph.nodes.get(k)?;
            if node.dynamic || node.expr.is_none() || has_dirty_precedent(&index, &node.areas) {
                return None;
            }
        }
    }
    let n_workers = worker_count(dirty.len(), workers);
    if n_workers < 2 {
        return None;
    }
    let chunk = dirty.len().div_ceil(n_workers);
    let prior: HashMap<Key, Value> = HashMap::default();
    let spills_in: HashMap<Key, std::sync::Arc<Array>> = HashMap::default();
    let spill_index = SpillIndex::build(&spills_in);
    let mut parts: Vec<Vec<(Key, Value, Option<std::sync::Arc<Array>>)>> = Vec::new();
    let mut failed = false;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for group in dirty.chunks(chunk) {
            let prior = &prior;
            let spills_in = &spills_in;
            let spill_index = &spill_index;
            handles.push(scope.spawn(move || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut scratch: Option<HashMap<Key, std::sync::Arc<Expr>>> = None;
                    let mut local = Vec::with_capacity(group.len());
                    for k in group {
                        let Some(node) = graph.nodes.get(k) else { continue };
                        let Some(expr) = node.expr.as_deref() else { continue };
                        let legacy = node.legacy;
                        let fast = eval_flat_scalar(wb, k.0, expr);
                        if let Some(v) = fast.as_ref()
                            && legacy.is_none()
                            && !matches!(v, Value::Array(_) | Value::Empty)
                        {
                            local.push((*k, v.clone(), None));
                            continue;
                        }
                        let v = match fast {
                            Some(v) => v,
                            None => {
                                let mut host = FrozenHost { wb, prior, spills: spills_in, spill_index, now, at: *k, used: HashMap::default() };
                                let mut ev = Evaluator::new(&mut host, k.0, k.1);
                                ev.value(expr)
                            }
                        };
                        let exprs = scratch.get_or_insert_with(HashMap::default);
                        let (v, sp) = finish_formula(wb, exprs, *k, expr, v, legacy, true);
                        local.push((*k, v, sp));
                    }
                    local
                }))
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(Ok(local)) => parts.push(local),
                Ok(Err(_)) | Err(_) => failed = true,
            }
        }
    });
    if failed {
        return None;
    }
    let mut pairs = Vec::with_capacity(dirty.len());
    let mut spills = HashMap::default();
    for local in parts {
        for (k, v, sp) in local {
            if let Some(a) = sp {
                spills.insert(k, a);
            }
            pairs.push((k, v));
        }
    }
    Some(FlatWave { pairs, spills })
}

#[cfg(not(target_arch = "wasm32"))]
fn eval_independent_native(
    wb: &Workbook,
    graph: &Graph,
    exprs: &HashMap<Key, std::sync::Arc<Expr>>,
    dirty: &[Key],
    now: f64,
    workers: Option<u8>,
) -> Option<ParallelBatch> {
    if dirty.len() < PARALLEL_MIN || worker_count(dirty.len(), workers) < 2 {
        return None;
    }
    let index = dirty_index(dirty);
    let mut left: HashMap<Key, u32> = HashMap::default();
    let mut succs: HashMap<Key, Vec<Key>> = HashMap::default();
    let mut heavy: HashSet<Key> = HashSet::default();
    let mut static_preds: HashMap<Key, HashSet<Key>> = HashMap::default();
    for k in dirty {
        if !exprs.contains_key(k) {
            continue;
        }
        let Some(node) = graph.nodes.get(k) else {
            heavy.insert(*k);
            continue;
        };
        if node.dynamic {
            heavy.insert(*k);
            continue;
        }
        match precedent_keys(&index, &node.areas) {
            Err(()) => {
                heavy.insert(*k);
            }
            Ok(preds) => {
                left.insert(*k, preds.len() as u32);
                static_preds.insert(*k, preds.iter().copied().collect());
                for p in preds {
                    if p != *k {
                        succs.entry(p).or_default().push(*k);
                    }
                }
            }
        }
    }
    for &r in dirty {
        if heavy.contains(&r) {
            continue;
        }
        let Some(rnode) = graph.nodes.get(&r) else { continue };
        for &s in dirty {
            if s == r {
                continue;
            }
            let Some(snode) = graph.nodes.get(&s) else { continue };
            let Some(skind) = spill_kind(snode) else { continue };
            if !rnode.areas.iter().any(|a| area_meets_spill(a, s, &skind)) {
                continue;
            }
            if heavy.contains(&s) || !left.contains_key(&s) {
                heavy.insert(r);
                left.remove(&r);
                break;
            }
            let already = static_preds.get(&r).is_some_and(|p| p.contains(&s));
            if !already {
                if let Some(n) = left.get_mut(&r) {
                    *n = n.saturating_add(1);
                }
                succs.entry(s).or_default().push(r);
            }
        }
    }
    let mut values: HashMap<Key, Value> = HashMap::default();
    let mut spills: HashMap<Key, std::sync::Arc<Array>> = HashMap::default();
    let mut done: HashSet<Key> = HashSet::default();
    loop {
        let ready: Vec<Key> = dirty.iter().copied().filter(|k| !done.contains(k) && !heavy.contains(k) && left.get(k).copied() == Some(0)).collect();
        if ready.len() < PARALLEL_MIN || worker_count(ready.len(), workers) < 2 {
            break;
        }
        let n_workers = worker_count(ready.len(), workers);
        let chunk = ready.len().div_ceil(n_workers);
        let spill_index = SpillIndex::build(&spills);
        let mut parts: Vec<Vec<(Key, Value, Option<std::sync::Arc<Array>>)>> = Vec::new();
        let mut failed = false;
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for group in ready.chunks(chunk) {
                let values = &values;
                let spills = &spills;
                let spill_index = &spill_index;
                handles.push(scope.spawn(move || {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut local = Vec::with_capacity(group.len());
                        for k in group {
                            let legacy = graph.nodes.get(k).and_then(|n| n.legacy);
                            let (v, sp) = eval_frozen(wb, exprs, values, spills, spill_index, now, *k, legacy);
                            local.push((*k, v, sp));
                        }
                        local
                    }))
                }));
            }
            for handle in handles {
                match handle.join() {
                    Ok(Ok(local)) => parts.push(local),
                    Ok(Err(_)) | Err(_) => failed = true,
                }
            }
        });
        if failed {
            return None;
        }
        for local in parts {
            for (k, v, sp) in local {
                if let Some(children) = succs.get(&k) {
                    for c in children {
                        if let Some(n) = left.get_mut(c) {
                            *n = n.saturating_sub(1);
                        }
                    }
                }
                if let Some(a) = sp {
                    spills.insert(k, a);
                }
                values.insert(k, v);
                done.insert(k);
            }
        }
    }
    if done.is_empty() { None } else { Some(ParallelBatch { values, spills, done }) }
}
