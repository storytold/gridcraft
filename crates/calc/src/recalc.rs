//! Dependency graph and recalculation.
//!
//! The graph maps every formula cell to its static precedents (resolved areas). A reverse index
//! answers "which formulas read this cell?": single-cell precedents in a hash map, ranges as row
//! intervals per column (wide ranges per sheet), each an interval index so a lookup costs
//! O(log n + matches) however many ranges share the column. After an edit, the dirty set is the
//! transitive closure of dependents; it is evaluated in topological order, with on-demand
//! evaluation as a safety net for dynamic references. Cycles produce `#CIRC!` unless iterative
//! calculation is on: then a formula read while it is being evaluated gives its previous value,
//! and the formulas that read one are evaluated again until the loop settles
//! (`PassHost::iterate_loops`).

use std::collections::VecDeque;

use gridcraft_core::{Array, CellError, CellRef, RangeRef, Value};
use gridcraft_formula::Expr;
use gridcraft_model::{CalcMode, Workbook};

use crate::eval::{Evaluator, Host, NameCache, precedents_with, stack_position};

mod graph;
mod pass_host;

#[cfg(test)]
use graph::Intervals;
pub use graph::{FxHasher, Graph, Key};
use graph::{HashMap, HashSet, Node};
pub use pass_host::thread_count;
use pass_host::{PassHost, parallel_min, spills_as_before};

/// Calculation state kept beside a workbook.
#[derive(Clone, Debug)]
pub struct Calc {
    pub graph: Graph,
    rng: u64,
    /// Fixed "now" for deterministic tests; `None` = system clock.
    pub fixed_now: Option<f64>,
    /// Cells whose formulas have errors from cycles.
    pub circular: Vec<Key>,
    pub last_recalc_cells: usize,
    /// Areas the formulas of the graph hold from names, so the budget on them holds across edits
    /// as well as within one build (see [`NameCache`]), and what each formula holding some was
    /// charged, given back when the formula is resolved again or removed.
    name_areas: usize,
    name_charges: HashMap<Key, usize>,
    /// Levels with fewer formulas than this are calculated on the main thread (see
    /// [`PARALLEL_MIN`]); tests lower it to put every level on worker threads.
    pub(crate) parallel_min: usize,
    /// Levels calculated on worker threads in the last recalculation.
    pub worker_levels: usize,
    /// Levels whose worker threads could not be started or failed, calculated on the main
    /// thread instead, in the last recalculation.
    pub worker_fallbacks: usize,
    /// Formulas a worker thread handed back in the last recalculation: they read a formula of
    /// their own level that wasn't done (INDIRECT, OFFSET, a spill range) or drew a random
    /// number, and the main thread calculated them once the level was merged.
    pub handed_back: usize,
    /// Levels in which formulas after an array laid out differently from what the workbook
    /// stores (or lost) were calculated again on the main thread, rather than merged from the
    /// workers, in the last recalculation (see [`PassHost::run_level`]).
    pub relaid_levels: usize,
    /// Makes every worker thread fail, to test the fallback.
    #[cfg(test)]
    pub(crate) fail_workers: bool,
    /// Formulas a worker takes at a time, to spread small workbooks over the workers.
    #[cfg(test)]
    pub(crate) chunk: Option<usize>,
    /// Formulas of threaded levels the main thread calculated at their turn, in the last
    /// recalculation (see [`PassHost::run_level`]).
    #[cfg(test)]
    pub(crate) level_formulas_on_main: usize,
    /// Of those, the ones whose worker result was thrown away because an array before them was
    /// laid out differently from what the workbook stores.
    #[cfg(test)]
    pub(crate) level_formulas_redone: usize,
    /// Calculation threads regardless of the settings, the environment and the processors
    /// present, so a test means what it says on any machine.
    #[cfg(test)]
    pub(crate) threads: Option<usize>,
    /// Caps the planner's index work, to test the row-major fallback.
    #[cfg(test)]
    pub(crate) plan_budget: Option<usize>,
}

impl Default for Calc {
    fn default() -> Self {
        Calc {
            graph: Graph::default(),
            rng: 0x9E37_79B9_7F4A_7C15,
            fixed_now: None,
            circular: vec![],
            last_recalc_cells: 0,
            name_areas: 0,
            name_charges: HashMap::default(),
            parallel_min: parallel_min(),
            worker_levels: 0,
            worker_fallbacks: 0,
            handed_back: 0,
            relaid_levels: 0,
            #[cfg(test)]
            fail_workers: false,
            #[cfg(test)]
            chunk: None,
            #[cfg(test)]
            level_formulas_on_main: 0,
            #[cfg(test)]
            level_formulas_redone: 0,
            #[cfg(test)]
            threads: None,
            #[cfg(test)]
            plan_budget: None,
        }
    }
}

/// Pops one finished component off Tarjan's stack, labelling its nodes with `comp_index`. The
/// component is everything up to and including `root`.
fn pop_component(stack: &mut Vec<usize>, on_stack: &mut [bool], comp: &mut [usize], comp_index: usize, root: usize) {
    while let Some(w) = stack.pop() {
        on_stack[w] = false;
        comp[w] = comp_index;
        if w == root {
            break;
        }
    }
}

/// Strongly connected components of a graph given as adjacency lists (Tarjan, iterative): each
/// node's component index, and the number of nodes in each component.
fn components(edges: &[Vec<usize>]) -> (Vec<usize>, Vec<usize>) {
    let n = edges.len();
    let (mut low, mut num) = (vec![0usize; n], vec![usize::MAX; n]);
    let (mut on_stack, mut stack, mut comp) = (vec![false; n], Vec::new(), vec![usize::MAX; n]);
    let (mut next, mut comps) = (0usize, 0usize);
    for root in 0..n {
        if num[root] != usize::MAX {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        num[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(top) = call.last_mut() {
            let v = top.0;
            if let Some(&w) = edges.get(v).and_then(|e| e.get(top.1)) {
                top.1 += 1;
                if num[w] == usize::MAX {
                    num[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(num[w]);
                }
                continue;
            }
            call.pop();
            if let Some(&(u, _)) = call.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == num[v] {
                pop_component(&mut stack, &mut on_stack, &mut comp, comps, v);
                comps += 1;
            }
        }
    }
    let mut size = vec![0usize; comps];
    for &c in &comp {
        if let Some(s) = size.get_mut(c) {
            *s += 1;
        }
    }
    (comp, size)
}

impl Calc {
    pub fn new() -> Self {
        Self::default()
    }

    fn now(&self) -> f64 {
        if let Some(n) = self.fixed_now {
            return n;
        }
        now_serial()
    }

    /// Rebuilds the graph from scratch.
    pub fn rebuild(&mut self, wb: &Workbook) {
        self.graph = Graph::default();
        let mut circ = Vec::new();
        let mut names = NameCache::default();
        self.name_charges.clear();
        for (si, sheet) in wb.sheets.iter().enumerate() {
            for (c, cell) in sheet.cells.iter() {
                if let Some(f) = &cell.formula
                    && let Some(e) = f.expr()
                {
                    let before = names.stored();
                    let (areas, dynamic) = precedents_with(wb, si, &e, &mut names);
                    let charge = names.stored().saturating_sub(before);
                    if charge > 0 {
                        self.name_charges.insert((si, c), charge);
                    }
                    self.graph.insert((si, c), Node { areas, dynamic });
                    if cell.value == Value::Error(CellError::Circ) {
                        circ.push((si, c));
                    }
                }
            }
        }
        self.graph.settle();
        self.name_areas = names.stored();
        self.circular = if wb.calc.iterative { Vec::new() } else { self.loops_among(&circ) };
    }

    /// The circular references of a rebuilt workbook (after undo, redo or a rollback, which put
    /// back cells without calculating them), from the formulas whose cached value is `#CIRC!`:
    /// the listed cells still on a loop, and one cell of each loop none of them is on.
    fn loops_among(&self, circ: &[Key]) -> Vec<Key> {
        if circ.is_empty() {
            return Vec::new();
        }
        let n = circ.len();
        let index: HashMap<Key, usize> = circ.iter().enumerate().map(|(i, k)| (*k, i)).collect();
        // Edges from a cell to the `#CIRC!` formulas that read it; loops are the same either way.
        let mut deps = Vec::new();
        let edges: Vec<Vec<usize>> = circ
            .iter()
            .map(|k| {
                deps.clear();
                self.graph.dependents(k.0, k.1, &mut deps);
                let mut e: Vec<usize> = deps.iter().filter_map(|d| index.get(d).copied()).collect();
                e.sort_unstable();
                e.dedup();
                e
            })
            .collect();
        // Loop members: nodes in a component of more than one, or with an edge to themselves.
        let (comp, size) = components(&edges);
        let on_loop = |i: usize| comp.get(i).and_then(|c| size.get(*c)).is_some_and(|s| *s > 1) || edges.get(i).is_some_and(|e| e.contains(&i));
        // A formula with dynamic references (INDIRECT, OFFSET) may close a loop the graph can't see.
        let mut out: Vec<Key> =
            self.circular.iter().copied().filter(|k| index.get(k).is_some_and(|&i| on_loop(i) || self.graph.dynamic.contains(k))).collect();
        let mut covered: HashSet<usize> = out.iter().filter_map(|k| index.get(k)).filter_map(|&i| comp.get(i).copied()).collect();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| circ.get(i).map(|(s, c)| (*s, c.row, c.col)));
        for i in order {
            if let (Some(&c), Some(&k)) = (comp.get(i), circ.get(i))
                && on_loop(i)
                && covered.insert(c)
            {
                out.push(k);
            }
        }
        out
    }

    /// Recalculates every formula (F9 / Ctrl+Alt+F9, and after loading).
    pub fn recalc_all(&mut self, wb: &mut Workbook) {
        self.rebuild(wb);
        // Spills are laid out again from the formulas where they are now (a structural edit moves
        // or deletes the formulas, not the values they spilled).
        for i in 0..wb.sheets.len() {
            if wb.sheet(i).is_some_and(|s| !s.spill_ranges.is_empty() || !s.spill.is_empty())
                && let Some(sh) = wb.sheet_mut(i)
            {
                sh.spill.clear();
                sh.spill_ranges.clear();
            }
        }
        let all: Vec<Key> = self.graph.nodes.keys().copied().collect();
        self.run(wb, all, true);
    }

    /// Updates the graph for cells whose content changed and recalculates what depends on them.
    pub fn cells_changed(&mut self, wb: &mut Workbook, changed: &[Key]) {
        let mut cleared: Vec<(usize, RangeRef)> = Vec::new();
        // The budget on areas taken from names counts on from what the graph's formulas hold: an
        // edited formula gives its charge back before it is resolved again, so editing or refilling
        // the same readers doesn't spend it.
        let mut names = NameCache::with_stored(self.name_areas);
        for &k in changed {
            if let Some(old) = self.name_charges.remove(&k) {
                names.give_back(old);
            }
            let formula = wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.clone());
            match formula.and_then(|f| f.expr()) {
                Some(e) => {
                    let before = names.stored();
                    let (areas, dynamic) = precedents_with(wb, k.0, &e, &mut names);
                    let charge = names.stored().saturating_sub(before);
                    if charge > 0 {
                        self.name_charges.insert(k, charge);
                    }
                    self.graph.insert(k, Node { areas, dynamic });
                }
                None => {
                    self.graph.remove(k);
                    // A formula replaced by a constant (or cleared) takes its spilled values with it.
                    if let Some(sh) = wb.sheet_mut(k.0)
                        && let Some(r) = sh.spill_ranges.remove(&k.1)
                    {
                        for c in r.iter() {
                            sh.spill.remove(&c);
                        }
                        cleared.push((k.0, r));
                    }
                    // A formula that doesn't parse evaluates to #NAME?.
                    if let Some(cell) = wb.sheet_mut(k.0).and_then(|s| s.cells.get_mut(k.1))
                        && cell.formula.is_some()
                    {
                        cell.value = Value::Error(CellError::Name);
                    }
                }
            }
        }
        self.name_areas = names.stored();
        self.graph.settle();
        if wb.calc.mode == CalcMode::Manual {
            // Only the edited formulas themselves are evaluated.
            let own: Vec<Key> = changed.iter().copied().filter(|k| self.graph.nodes.contains_key(k)).collect();
            self.run(wb, own, false);
            return;
        }
        let mut seeds: Vec<Key> = changed.to_vec();
        let mut emptied: Vec<Key> = Vec::new();
        // Typing into (or clearing) a cell a dynamic array spills over (or would spill over)
        // re-evaluates the anchor, which may now be blocked or unblocked.
        for &k in changed {
            if let Some(sh) = wb.sheet(k.0) {
                for (anchor, r) in &sh.spill_ranges {
                    if *anchor != k.1 && r.contains(k.1) {
                        seeds.push((k.0, *anchor));
                    }
                }
                for (c, cell) in sh.cells.iter_range(RangeRef::new(CellRef::new(k.1.row.saturating_sub(64), k.1.col.saturating_sub(64)), k.1)) {
                    if c != k.1 && cell.value == Value::Error(CellError::Spill) {
                        seeds.push((k.0, c));
                    }
                }
                if !sh.cells.has(k.1) {
                    match emptied.iter_mut().find(|x| x.0 == k.0) {
                        Some(x) => x.1 = CellRef::new(x.1.row.max(k.1.row), x.1.col.max(k.1.col)),
                        None => emptied.push(k),
                    }
                }
            }
        }
        // An emptied cell can unblock an anchor any distance above or to the left of it.
        for (si, end) in emptied {
            for &(s, c) in self.graph.nodes.keys() {
                if s == si && c.row <= end.row && c.col <= end.col && wb.sheet(s).is_some_and(|sh| sh.value(c) == Value::Error(CellError::Spill)) {
                    seeds.push((s, c));
                }
            }
        }
        // Spill areas of changed anchors also change.
        for &k in changed {
            if let Some(r) = wb.sheet(k.0).and_then(|s| s.spill_ranges.get(&k.1)) {
                seeds.extend(r.iter().map(|c| (k.0, c)));
            }
        }
        for (si, r) in &cleared {
            seeds.extend(r.iter().map(|c| (*si, c)));
        }
        let t0 = prof_now();
        let dirty = self.dirty_closure(wb, &seeds);
        let t1 = prof_now();
        let n = dirty.len();
        self.run(wb, dirty, false);
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            eprintln!("recalc: {} seeds, closure {} cells {:.1} ms, run {:.1} ms", seeds.len(), n, t1 - t0, prof_now() - t1);
        }
    }

    fn dirty_closure(&self, _wb: &Workbook, seeds: &[Key]) -> Vec<Key> {
        let mut seen: HashSet<Key> = HashSet::default();
        let mut queue: VecDeque<Key> = VecDeque::new();
        for k in seeds {
            if self.graph.nodes.contains_key(k) && seen.insert(*k) {
                queue.push_back(*k);
            }
            let mut deps = Vec::new();
            self.graph.dependents(k.0, k.1, &mut deps);
            for d in deps {
                if seen.insert(d) {
                    queue.push_back(d);
                }
            }
        }
        for k in &self.graph.dynamic {
            if seen.insert(*k) {
                queue.push_back(*k);
            }
        }
        let mut deps = Vec::new();
        while let Some(k) = queue.pop_front() {
            deps.clear();
            self.graph.dependents(k.0, k.1, &mut deps);
            for d in deps.drain(..) {
                if seen.insert(d) {
                    queue.push_back(d);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Evaluates the dirty set and writes results (and spills) back.
    fn run(&mut self, wb: &mut Workbook, mut dirty: Vec<Key>, full: bool) {
        self.last_recalc_cells = 0;
        self.worker_levels = 0;
        self.worker_fallbacks = 0;
        self.handed_back = 0;
        self.relaid_levels = 0;
        #[cfg(test)]
        {
            self.level_formulas_redone = 0;
        }
        #[cfg(test)]
        {
            self.level_formulas_on_main = 0;
        }
        let mut dynamic_again = !self.graph.dynamic.is_empty();
        for _round in 0..8 {
            if dirty.is_empty() {
                break;
            }
            // Row-major order is a good topological guess; on-demand evaluation fixes the rest.
            dirty.sort_by_key(|(s, c)| (*s, c.row, c.col));
            let mut exprs: HashMap<Key, std::sync::Arc<Expr>> = HashMap::with_capacity_and_hasher(dirty.len(), Default::default());
            for k in &dirty {
                if let Some(e) = wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.as_ref()).and_then(|f| f.expr_arc()) {
                    exprs.insert(*k, e);
                }
            }
            let now = self.now();
            let mut rng = self.rng;
            let tp = prof_now();
            // Iterative calculation reads previous values as it goes: one thread keeps that simple.
            #[cfg(test)]
            let threads = if wb.calc.iterative { 1 } else { self.threads.unwrap_or_else(|| thread_count(&wb.calc)) };
            #[cfg(not(test))]
            let threads = if wb.calc.iterative { 1 } else { thread_count(&wb.calc) };
            // Planned on one thread too, so the order the formulas are calculated in (which
            // cell of a cycle shows `#CIRC!`, the sequence RAND draws in, how many rounds a
            // chain of spills takes) is the same whatever the machine and the setting.
            #[cfg(test)]
            let plan = self.graph.levels_within(wb, &dirty, self.plan_budget);
            #[cfg(not(test))]
            let plan = self.graph.levels(wb, &dirty);
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some()
                && let Some(p) = &plan
            {
                eprintln!(
                    "  plan: {} levels over {} formulas, {} behind cycles, {} index work, {} threads, {:.1} ms",
                    p.bounds.len(),
                    p.leveled,
                    p.order.len() - p.leveled,
                    p.work(),
                    threads,
                    prof_now() - tp
                );
            }
            let te = prof_now();
            let (results, spills, cycles) = {
                let mut host = PassHost::new(wb, &exprs, Some(&self.graph), &mut rng, now);
                host.iterate = wb.calc.iterative;
                #[cfg(test)]
                {
                    host.fail_workers = self.fail_workers;
                    if let Some(n) = self.chunk {
                        host.chunk = n;
                    }
                }
                match &plan {
                    Some(plan) => {
                        for level in plan.levels() {
                            host.run_level(level, threads, self.parallel_min);
                        }
                        for &k in plan.rest() {
                            host.calculate(k);
                        }
                    }
                    None => {
                        for k in &dirty {
                            host.calculate(*k);
                        }
                    }
                }
                if host.iterate && !host.members.is_empty() {
                    let order: Vec<Key> = match &plan {
                        Some(p) => p.levels().flatten().copied().chain(p.rest().iter().copied()).collect(),
                        None => dirty.clone(),
                    };
                    let passes = host.iterate_loops(&order, wb.calc.max_iterations, wb.calc.max_change);
                    if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                        eprintln!("  iterative calculation: {} passes over {} formulas", passes, host.tainted.len());
                    }
                }
                self.worker_levels += host.worker_levels;
                self.worker_fallbacks += host.fallbacks;
                self.handed_back += host.handed_back;
                self.relaid_levels += host.relaid_levels;
                #[cfg(test)]
                {
                    self.level_formulas_redone += host.redone;
                }
                #[cfg(test)]
                {
                    self.level_formulas_on_main += host.on_main;
                }
                (host.results, host.spills, host.cycles)
            };
            self.rng = rng;
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  eval {} formulas {:.1} ms", results.len(), prof_now() - te);
            }
            let tw = prof_now();
            self.last_recalc_cells += results.len();
            // The loops this round finds replace the ones listed for the formulas it evaluated.
            if !self.circular.is_empty() {
                let evaluated: HashSet<Key> = dirty.iter().copied().collect();
                self.circular.retain(|k| !evaluated.contains(k));
            }
            self.circular.extend(cycles);
            let (mut spill_changes, settled_spill_moved) = self.write_back(wb, &dirty, results, &spills);
            // Formulas with references only known while evaluating (INDIRECT, OFFSET) may have read
            // a spill area before it was laid out: they are evaluated once more. A planned round
            // can lay out a spill after a formula of an earlier level read its area, so they are
            // evaluated again after a planned round in which the spill of a formula outside their
            // set changed (a volatile array lays out new values every time it runs, and would
            // send them round again whatever they read).
            if (dynamic_again || (plan.is_some() && settled_spill_moved)) && !spills.is_empty() && !self.graph.dynamic.is_empty() {
                dynamic_again = false;
                spill_changes.extend(self.graph.dynamic.iter().copied());
            }
            if full {
                // Everything was already evaluated once; spill dependents need another pass.
            }
            if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                eprintln!("  write-back {:.1} ms", prof_now() - tw);
            }
            spill_changes.sort_by_key(|(s, c)| (*s, c.row, c.col));
            spill_changes.dedup();
            if spill_changes.is_empty() {
                break;
            }
            dirty = self.dirty_closure(wb, &spill_changes);
        }
        // Loops among formulas the run did not evaluate stay listed while they are still formulas.
        // Iterative calculation reports none.
        if wb.calc.iterative {
            self.circular.clear();
        } else {
            self.circular.retain(|k| self.graph.nodes.contains_key(k));
        }
    }

    fn write_back(
        &mut self,
        wb: &mut Workbook,
        dirty: &[Key],
        results: HashMap<Key, Value>,
        spills: &HashMap<Key, std::sync::Arc<Array>>,
    ) -> (Vec<Key>, bool) {
        let mut spill_changes = Vec::new();
        // Whether the spill of a formula outside the dynamic set (INDIRECT, OFFSET, volatile)
        // changed.
        let mut settled_spill_moved = false;
        let mut freed = Vec::new();
        for (key, value) in results {
            let Some(sheet) = wb.sheet_mut(key.0) else { continue };
            if let Some(cell) = sheet.cells.get_mut(key.1)
                && cell.value != value
            {
                cell.value = value;
            }
        }
        for key in dirty {
            let new = spills.get(key);
            let Some(sheet) = wb.sheet_mut(key.0) else { continue };
            let old = sheet.spill_ranges.get(&key.1).copied();
            if old.is_none() && new.is_none() {
                continue;
            }
            // An unchanged spill does not alter any values its dependents read.
            if let (Some(range), Some(array)) = (old, new)
                && spills_as_before(sheet, key.1, range, array)
            {
                continue;
            }
            settled_spill_moved |= !self.graph.dynamic.contains(key);
            if let Some(range) = old {
                for cell in range.iter() {
                    sheet.spill.remove(&cell);
                }
                sheet.spill_ranges.remove(&key.1);
            }
            let mut changed_range = old;
            if let Some(array) = new {
                let end = CellRef::new(key.1.row.saturating_add(array.rows as u32 - 1), key.1.col.saturating_add(array.cols as u32 - 1));
                let range = RangeRef::new(key.1, end);
                let blocked = spill_is_blocked(sheet, key.1, range, end);
                if blocked {
                    if let Some(cell) = sheet.cells.get_mut(key.1) {
                        cell.value = Value::Error(CellError::Spill);
                    }
                    let mut blocked_dependents = Vec::new();
                    self.graph.dependents_of_range(key.0, range, &mut blocked_dependents);
                    spill_changes.extend(blocked_dependents.into_iter().filter(|dependent| dependent != key));
                } else {
                    fill_spill(sheet, key.1, range, array);
                    changed_range = Some(changed_range.map_or(range, |previous| previous.union(&range)));
                }
            }
            if let Some(range) = changed_range {
                let mut dependents = Vec::new();
                self.graph.dependents_of_range(key.0, range, &mut dependents);
                let freed_old_range = match (old, sheet.spill_ranges.get(&key.1)) {
                    (Some(previous), Some(current)) => !(current.contains(previous.start) && current.contains(previous.end)),
                    (_, None) => true,
                    (None, Some(_)) => false,
                };
                spill_changes.extend(dependents.into_iter().filter(|dependent| dependent != key || freed_old_range));
            }
            if let Some(previous) = old
                && !sheet.spill_ranges.get(&key.1).is_some_and(|current| current.contains(previous.start) && current.contains(previous.end))
            {
                freed.push((key.0, previous));
            }
        }
        for (sheet_index, range) in freed {
            for &(sheet, cell) in self.graph.nodes.keys() {
                let was_blocked_cell = cell.row <= range.end.row
                    && cell.col <= range.end.col
                    && wb.sheet(sheet).is_some_and(|sheet| sheet.value(cell) == Value::Error(CellError::Spill));
                if sheet == sheet_index && was_blocked_cell {
                    spill_changes.push((sheet, cell));
                }
            }
        }
        (spill_changes, settled_spill_moved)
    }
}

fn spill_is_blocked(sheet: &gridcraft_model::Sheet, anchor: CellRef, range: RangeRef, end: CellRef) -> bool {
    end.row >= gridcraft_core::MAX_ROWS
        || end.col >= gridcraft_core::MAX_COLS
        || range.iter().any(|cell| cell != anchor && (sheet.cells.has(cell) || sheet.spill.contains_key(&cell)))
        || sheet.merges.iter().any(|merge| merge.intersects(&range))
        || sheet.tables.iter().any(|table| table.range.contains(anchor))
}

fn fill_spill(sheet: &mut gridcraft_model::Sheet, anchor: CellRef, range: RangeRef, array: &Array) {
    for (index, cell) in range.iter().enumerate() {
        if cell != anchor {
            sheet.spill.insert(cell, array.data.get(index).cloned().unwrap_or_default());
        }
    }
    sheet.spill_ranges.insert(anchor, range);
}

/// Current date-time as a serial (1900 system), from the system clock (browser clock on wasm).
/// UTC; the local offset isn't available without platform calls. Falls back to a fixed date only
/// when no clock is available.
pub fn now_serial() -> f64 {
    gridcraft_core::date::unix_now_ms().map_or(46302.5, gridcraft_core::date::serial_from_unix_ms)
}

/// Evaluates a formula in the context of a cell without storing it (conditional formats, data
/// validation, Evaluate Formula, Name Manager previews, the CLI `eval`).
pub fn evaluate(wb: &Workbook, sheet: usize, at: CellRef, formula: &str) -> Value {
    let body = formula.strip_prefix('=').unwrap_or(formula);
    let Ok(expr) = gridcraft_formula::parse(body) else { return Value::Error(CellError::Name) };
    evaluate_expr(wb, sheet, at, &expr)
}

pub fn evaluate_expr(wb: &Workbook, sheet: usize, at: CellRef, expr: &Expr) -> Value {
    let exprs = HashMap::default();
    let mut rng = 0x1234_5678_9ABC_DEF0u64;
    let mut host = PassHost::new(wb, &exprs, None, &mut rng, now_serial());
    let mut ev = Evaluator::new(&mut host, sheet, at);
    ev.value(expr)
}

fn prof_now() -> f64 {
    gridcraft_core::date::unix_now_ms().unwrap_or(0.0)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, Instant};

    use gridcraft_core::{CellRef, RangeRef, Value};
    use gridcraft_model::{Cell, Formula, Workbook};

    use super::{Calc, HashMap, Host, Intervals, Key, PassHost};

    /// xorshift64 for repeatable pseudo-random cases.
    fn rng(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn found(iv: &Intervals<u32>, a: u32, b: u32) -> Vec<u32> {
        let mut v = Vec::new();
        iv.query(a, b, &mut |t| v.push(*t));
        v.sort_unstable();
        v
    }

    #[test]
    fn intervals_match_a_linear_scan() {
        let mut iv: Intervals<u32> = Intervals::default();
        let mut model: Vec<(u32, u32, u32)> = Vec::new();
        let mut s = 0x9E37_79B9_7F4A_7C15;
        for step in 0..20_000u32 {
            let roll = rng(&mut s) % 10;
            if roll < 6 || model.is_empty() {
                let r0 = (rng(&mut s) % 500) as u32;
                let r1 = r0 + (rng(&mut s) % 40) as u32;
                if !model.contains(&(r0, r1, step)) {
                    iv.insert(r0, r1, step);
                    model.push((r0, r1, step));
                }
            } else {
                let i = (rng(&mut s) % model.len() as u64) as usize;
                let (r0, r1, t) = model.swap_remove(i);
                iv.remove(r0, r1, t);
                if roll == 9 {
                    // Removed and put back before the next merge.
                    iv.insert(r0, r1, t);
                    model.push((r0, r1, t));
                }
            }
            if rng(&mut s).is_multiple_of(50) {
                iv.settle();
            }
            if step.is_multiple_of(97) {
                let a = (rng(&mut s) % 560) as u32;
                let b = a + (rng(&mut s) % 30) as u32;
                let mut want: Vec<u32> = model.iter().filter(|(r0, r1, _)| *r0 <= b && *r1 >= a).map(|(_, _, t)| *t).collect();
                want.sort_unstable();
                assert_eq!(found(&iv, a, b), want, "rows {a}..={b} at step {step}");
            }
        }
        iv.settle();
        for row in 0..560 {
            let mut want: Vec<u32> = model.iter().filter(|(r0, r1, _)| *r0 <= row && row <= *r1).map(|(_, _, t)| *t).collect();
            want.sort_unstable();
            assert_eq!(found(&iv, row, row), want, "row {row}");
        }
        for (r0, r1, t) in std::mem::take(&mut model) {
            iv.remove(r0, r1, t);
        }
        iv.settle();
        assert!(iv.is_empty());
    }

    #[test]
    fn interval_queries_cost_log_n_plus_matches() {
        // 40k one-row ranges (like =SUM(E1:J1) down a column) and 1k ranges over everything:
        // a point query visits O(log n) tree nodes per match, not every range in the column.
        let n = 40_000u32;
        let mut iv: Intervals<u32> = Intervals::default();
        for r in 0..n {
            iv.insert(r, r, r);
        }
        for t in 0..1_000 {
            iv.insert(0, n, n + t);
        }
        iv.settle();
        let mut worst = 0;
        for row in (0..n).step_by(7) {
            let mut k = 0;
            let visited = iv.query(row, row, &mut |_| k += 1);
            assert_eq!(k, 1_001);
            worst = worst.max(visited);
        }
        // 1,001 matches in a tree of depth 16: well under 4 nodes per match plus the path.
        assert!(worst < 4 * 1_001 + 64, "visited {worst} nodes");
        let mut k = 0;
        let visited = iv.query(n + 1, n + 5, &mut |_| k += 1);
        assert_eq!(k, 0);
        assert!(visited < 64, "visited {visited} nodes for no match");
    }

    /// `rows` rows of six numbers (E:J) and `=SUM(E{r}:J{r})` in K.
    fn sum_rows(rows: u32) -> Workbook {
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            for r in 0..rows {
                for c in 4..10 {
                    sh.cells.set(CellRef::new(r, c), Cell::value(Value::Number(f64::from(r % 97 + c))));
                }
                sh.cells.set(CellRef::new(r, 10), Cell::formula(Formula::new(&format!("=SUM(E{0}:J{0})", r + 1))));
            }
        }
        wb
    }

    /// Opening (full recalc), changing every value of a column, and re-entering every formula.
    fn open_and_bulk_edit(rows: u32) -> Duration {
        let mut wb = sum_rows(rows);
        let mut calc = Calc::new();
        let started = gridcraft_core::date::unix_now_ms().unwrap_or(0.0);
        calc.recalc_all(&mut wb);
        let values: Vec<Key> = (0..rows).map(|r| (0, CellRef::new(r, 4))).collect();
        if let Some(sh) = wb.sheet_mut(0) {
            for (_, c) in &values {
                sh.cells.set(*c, Cell::value(Value::Number(1.0)));
            }
        }
        calc.cells_changed(&mut wb, &values);
        let formulas: Vec<Key> = (0..rows).map(|r| (0, CellRef::new(r, 10))).collect();
        calc.cells_changed(&mut wb, &formulas);
        let ended = gridcraft_core::date::unix_now_ms().unwrap_or(started);
        let elapsed = Duration::from_secs_f64((ended - started).max(0.0) / 1000.0);
        let last = wb.sheet(0).map(|s| s.value(CellRef::new(rows - 1, 10)));
        let r = rows - 1;
        assert_eq!(last, Some(Value::Number(f64::from(1 + (5..10).map(|c| r % 97 + c).sum::<u32>()))));
        elapsed
    }

    #[test]
    fn range_formulas_recalc_and_bulk_edit_correctly() {
        // The values behind the timing test below, checked on every run.
        open_and_bulk_edit(500);
    }

    #[test]
    #[ignore = "timing-sensitive; run with --ignored in release"]
    fn recalc_with_range_formulas_scales_linearly() {
        // Each row's SUM reads a range in the same columns. Recalculating on open used to walk
        // every row per range read (the used range), and finding a cell's dependents scanned
        // every range in its column: 4x the rows took 16x the time. Linear is 4x; allow 8x.
        let best = |rows| (0..3).map(|_| open_and_bulk_edit(rows)).min().unwrap_or_default();
        let small = best(5_000);
        let large = best(20_000);
        assert!(large < small * 8 + Duration::from_millis(100), "5k rows {small:?}, 20k rows {large:?}");
    }

    /// A thousand formulas in row 1 (`=SEQUENCE(2)+$DLM$1` with `arrays`, else `=1+$DLM$1`) and
    /// 20,000 readers of two-cell blocks in rows 11 to 1,010 that none of the arrays reach.
    fn arrays_beside_readers(arrays: bool) -> Workbook {
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            let anchor = if arrays { "=SEQUENCE(2)+$DLM$1" } else { "=1+$DLM$1" };
            for col in 0..1_000 {
                sh.cells.set(CellRef::new(0, col), Cell::formula(Formula::new(anchor)));
            }
            for row in 10..1_010 {
                for col in 0..20 {
                    let block = RangeRef::new(CellRef::new(row, 100 + 2 * col), CellRef::new(row, 101 + 2 * col));
                    sh.cells.set(CellRef::new(row, col), Cell::formula(Formula::new(&format!("=SUM({})+$DLM$1", block.a1()))));
                }
            }
        }
        wb
    }

    /// The fastest of three edits of `at` after opening `wb`, on `threads` threads.
    #[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
    fn edit_time(wb: &Workbook, at: CellRef, threads: usize) -> Duration {
        (0..3)
            .map(|i| {
                let mut wb = wb.clone();
                let mut calc = Calc::new();
                calc.threads = Some(threads);
                calc.parallel_min = 512;
                calc.recalc_all(&mut wb);
                if let Some(sh) = wb.sheet_mut(0) {
                    sh.cells.set(at, Cell::value(Value::Number(f64::from(10 + i))));
                }
                let t = Instant::now();
                calc.cells_changed(&mut wb, &[(0, at)]);
                t.elapsed()
            })
            .min()
            .unwrap_or_default()
    }

    #[test]
    #[ignore = "timing-sensitive; run with --ignored in release"]
    fn range_reads_beside_many_arrays_walk_only_the_arrays_that_reach_them() {
        // Each range read lays the pass's arrays over the block: each array is checked once and
        // only the part reaching the block is walked. Walking every cell of every array, as the
        // parent commit did, took about 17 times the edit beside no arrays (8-core laptop, on
        // power), and sorting all of them on every read 35 times; this takes about 5.
        let dlm = CellRef::parse("DLM1").unwrap();
        let none = edit_time(&arrays_beside_readers(false), dlm, 1);
        let many = edit_time(&arrays_beside_readers(true), dlm, 1);
        eprintln!("one edit, one thread: beside no arrays {none:?}, beside a thousand {many:?}");
        assert!(many < none * 10 + Duration::from_millis(20), "beside no arrays {none:?}, beside a thousand {many:?}");
    }

    #[test]
    #[ignore = "timing-sensitive; run with --ignored in release"]
    fn arrays_with_new_values_in_their_place_cost_eight_threads_no_more_than_one() {
        // A thousand arrays with new values in the area they had: none of the readers in their
        // level reads them, so the workers' results stand and the level costs one worker pass,
        // with a dynamic formula after the arrays too (it alone is calculated again). Running
        // the workers again after each such array took eight threads to about 1.25 times one
        // thread's time, and to 1.7 times with the dynamic formula.
        let dlm = CellRef::parse("DLM1").unwrap();
        let mut with_dynamic = arrays_beside_readers(true);
        if let Some(sh) = with_dynamic.sheet_mut(0) {
            sh.cells.set(CellRef::new(5_000, 0), Cell::formula(Formula::new("=INDIRECT(\"A1\")+$DLM$1")));
        }
        for (what, wb) in [("no dynamic formula", arrays_beside_readers(true)), ("a dynamic formula after them", with_dynamic)] {
            let one = edit_time(&wb, dlm, 1);
            let eight = edit_time(&wb, dlm, 8);
            eprintln!("one edit, {what}: one thread {one:?}, eight threads {eight:?}");
            assert!(eight < one + one / 10 + Duration::from_millis(10), "{what}: one thread {one:?}, eight threads {eight:?}");
        }
    }

    #[test]
    #[ignore = "timing-sensitive; run with --ignored in release"]
    #[allow(clippy::disallowed_methods)] // a native-only timing test: the clock is read off wasm
    fn new_arrays_at_the_top_of_a_wide_level_cost_eight_threads_no_more_than_one() {
        // Opening lays every array out anew. 200 arrays above 100,000 formulas of the same level
        // that don't read them: the formulas keep their worker results. Running the workers
        // again over the rest of the level after each new array, eight times, then finishing it
        // on the main thread, took eight threads to about 2.2 times one thread's time.
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            for col in 0..200 {
                sh.cells.set(CellRef::new(0, col), Cell::formula(Formula::new("=SEQUENCE(2)+$ZZ$1")));
            }
            for row in 10..10_010 {
                for col in 0..10 {
                    sh.cells.set(CellRef::new(row, col), Cell::formula(Formula::new("=ROW()*2+$ZZ$1")));
                }
            }
        }
        let open = |threads: usize| {
            (0..3)
                .map(|_| {
                    let mut wb = wb.clone();
                    let mut calc = Calc::new();
                    calc.threads = Some(threads);
                    calc.parallel_min = 512;
                    let t = Instant::now();
                    calc.recalc_all(&mut wb);
                    t.elapsed()
                })
                .min()
                .unwrap_or_default()
        };
        let (one, eight) = (open(1), open(8));
        eprintln!("opening 200 arrays above 100,000 formulas: one thread {one:?}, eight threads {eight:?}");
        assert!(eight < one + one / 10 + Duration::from_millis(10), "one thread {one:?}, eight threads {eight:?}");
    }

    #[test]
    fn two_arrays_over_one_cell_read_as_the_one_anchored_first() {
        // Two 3x3 arrays of the pass, the second anchored one row and one column after the
        // first, overlap over four cells. The write-back keeps the array anchored first in
        // row-major order, so a read of one cell and a read of a block get the first array's
        // values there and the second's where only it reaches, whatever order the pass's map
        // holds them in: over several pairs of anchors the map yields the later one first for
        // some of them, which the test checks it has met.
        let wb = Workbook::new();
        let exprs = HashMap::default();
        let array =
            |base: f64| std::sync::Arc::new(gridcraft_core::Array::new(3, 3, (0..9).map(|i| Value::Number(base + f64::from(i))).collect()).unwrap());
        let area = |a: CellRef| RangeRef::new(a, CellRef::new(a.row + 2, a.col + 2));
        let mut later_first = 0;
        for (row, col) in [(0, 0), (1, 1), (2, 2), (1, 0), (0, 1), (0, 3), (4, 0), (4, 4), (7, 2), (3, 6)] {
            let first = CellRef::new(row, col);
            let later = CellRef::new(row + 1, col + 1);
            let mut rng = 1u64;
            let mut host = PassHost::new(&wb, &exprs, None, &mut rng, 0.0);
            host.spills.insert((0, first), array(0.0));
            host.spills.insert((0, later), array(100.0));
            if host.spills.keys().next() == Some(&(0, later)) {
                later_first += 1;
            }
            let want = |c: CellRef| {
                let (f, l) = (area(first), area(later));
                if f.contains(c) {
                    Value::Number(f64::from((c.row - first.row) * 3 + c.col - first.col))
                } else if l.contains(c) {
                    Value::Number(100.0 + f64::from((c.row - later.row) * 3 + c.col - later.col))
                } else {
                    Value::Empty
                }
            };
            let around = RangeRef::new(first, CellRef::new(later.row + 3, later.col + 3));
            for c in around.iter() {
                assert_eq!(host.cell_value(0, c), want(c), "anchors {} and {}: {}", first.a1(), later.a1(), c.a1());
            }
            // A block leaves out the anchor of the array it reads (that cell is the formula's), and
            // the later anchor lies under the first array, so it reads the first array's value.
            let parts = [around, RangeRef::new(later, CellRef::new(later.row + 1, later.col + 1)), area(later), RangeRef::new(first, first)];
            for block in parts {
                let values = host.range_values(0, block);
                let cells: Vec<Value> = block.iter().map(|c| if c == first { Value::Empty } else { want(c) }).collect();
                assert_eq!(values, cells, "anchors {} and {}: block {}", first.a1(), later.a1(), block.a1());
            }
        }
        assert!(later_first > 0, "the map yielded the later anchor first for none of the pairs, so the pick wasn't tested");
    }

    #[test]
    fn range_dependents_after_edits() {
        // Formulas that read ranges, wide ranges and whole columns, found again after edits.
        let mut wb = Workbook::new();
        let mut calc = Calc::new();
        let set = |wb: &mut Workbook, calc: &mut Calc, at: &str, input: &str| {
            let c = CellRef::parse(at).unwrap();
            let cell = if input.starts_with('=') { Cell::formula(Formula::new(input)) } else { Cell::value(Value::Number(input.parse().unwrap())) };
            wb.sheet_mut(0).unwrap().cells.set(c, cell);
            calc.cells_changed(wb, &[(0, c)]);
        };
        let num = |wb: &Workbook, at: &str| {
            let v = wb.sheet(0).unwrap().value(CellRef::parse(at).unwrap());
            v.as_f64().unwrap_or_else(|| panic!("{at} = {v:?}"))
        };
        set(&mut wb, &mut calc, "A1", "1");
        set(&mut wb, &mut calc, "B2", "2");
        set(&mut wb, &mut calc, "CZ3", "3");
        set(&mut wb, &mut calc, "DA5", "=SUM(A1:B2)");
        set(&mut wb, &mut calc, "DA6", "=SUM(A1:CZ3)");
        set(&mut wb, &mut calc, "DA7", "=SUM(B:B)");
        set(&mut wb, &mut calc, "DA8", "=SUM(2:2)");
        assert_eq!((num(&wb, "DA5"), num(&wb, "DA6"), num(&wb, "DA7"), num(&wb, "DA8")), (3.0, 6.0, 2.0, 2.0));
        set(&mut wb, &mut calc, "B2", "20");
        assert_eq!((num(&wb, "DA5"), num(&wb, "DA6"), num(&wb, "DA7"), num(&wb, "DA8")), (21.0, 24.0, 20.0, 20.0));
        // Re-entering a formula with another range drops the old one.
        set(&mut wb, &mut calc, "DA5", "=SUM(A1:A2)");
        set(&mut wb, &mut calc, "B2", "5");
        assert_eq!(num(&wb, "DA5"), 1.0);
        let mut deps = Vec::new();
        calc.graph.dependents(0, CellRef::parse("B2").unwrap(), &mut deps);
        deps.sort_unstable();
        let at = |s: &str| (0, CellRef::parse(s).unwrap());
        assert_eq!(deps, vec![at("DA6"), at("DA7"), at("DA8")]);
        deps.clear();
        calc.graph.dependents_of_range(0, RangeRef::new(CellRef::parse("A1").unwrap(), CellRef::parse("A9").unwrap()), &mut deps);
        deps.sort_unstable();
        deps.dedup();
        assert_eq!(deps, vec![at("DA5"), at("DA6"), at("DA8")]);
        set(&mut wb, &mut calc, "CZ3", "30");
        assert_eq!(num(&wb, "DA6"), 36.0);
        // Ranges on another sheet.
        wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
        set(&mut wb, &mut calc, "DA9", "=SUM(Data!A1:A3)+SUM(Data!A:CZ)");
        assert_eq!(num(&wb, "DA9"), 0.0);
        wb.sheet_mut(1).unwrap().cells.set(CellRef::parse("A2").unwrap(), Cell::value(Value::Number(4.0)));
        calc.cells_changed(&mut wb, &[(1, CellRef::parse("A2").unwrap())]);
        assert_eq!(num(&wb, "DA9"), 8.0);
        deps.clear();
        calc.graph.dependents(1, CellRef::parse("A2").unwrap(), &mut deps);
        assert_eq!(deps, vec![at("DA9"), at("DA9")]);
    }

    /// A formula at the nesting limit reading thousands of formulas not calculated yet suspends the
    /// evaluations in progress once, not once per formula read.
    #[test]
    fn wide_read_at_the_nesting_limit_suspends_once() {
        const WIDTH: u32 = 3000;
        let mut wb = Workbook::new();
        let mut exprs = HashMap::default();
        let mut put = |wb: &mut Workbook, at: String, text: String| {
            let at = CellRef::parse(&at).unwrap();
            wb.sheet_mut(0).unwrap().cells.set(at, Cell::formula(Formula::new(&text)));
            exprs.insert((0, at), std::sync::Arc::new(gridcraft_formula::parse(&text[1..]).unwrap()));
        };
        // A1 reads A2, ..., A64 (the 64th formula deep) reads a wide range of formulas.
        for r in 1..64 {
            put(&mut wb, format!("A{r}"), format!("=A{}+1", r + 1));
        }
        put(&mut wb, "A64".to_string(), format!("=SUM(B1:B{WIDTH})"));
        for r in 1..=WIDTH {
            put(&mut wb, format!("B{r}"), format!("={r}*2"));
        }
        let mut rng = 1u64;
        let mut host = PassHost::new(&wb, &exprs, None, &mut rng, 0.0);
        host.calculate((0, CellRef::parse("A1").unwrap()));
        let total = (WIDTH * (WIDTH + 1)) as f64;
        assert_eq!(host.results.get(&(0, CellRef::parse("A1").unwrap())), Some(&Value::Number(total + 63.0)));
        assert!(host.cycles.is_empty());
        assert!(host.suspensions <= 3, "{} suspensions", host.suspensions);
    }

    #[test]
    fn a_workers_copied_block_is_dropped_by_its_own_spill() {
        let mut wb = Workbook::new();
        let mut exprs = HashMap::default();
        let anchor = CellRef::parse("A2").unwrap();
        wb.sheet_mut(0).unwrap().cells.set(anchor, Cell::formula(Formula::new("=SEQUENCE(1,200)")));
        exprs.insert((0, anchor), std::sync::Arc::new(gridcraft_formula::parse("SEQUENCE(1,200)").unwrap()));
        let mut rng = 1u64;
        let host = PassHost::new(&wb, &exprs, None, &mut rng, 0.0);
        let mut worker_rng = 1u64;
        let mut w = host.worker(&mut worker_rng, 1 << 20);
        let block = RangeRef::parse("B2:CW2").unwrap();
        // The first read only notes the (empty) block; the second copies it, and the copy
        // serves the reads after that.
        assert!(w.range_values(0, block).iter().all(Value::is_empty));
        assert_eq!((w.ranges.len(), w.seen.len(), w.cached_bytes), (0, 1, 0));
        assert!(w.range_values(0, block).iter().all(Value::is_empty));
        assert_eq!((w.ranges.len(), w.cached_bytes), (1, 100 * std::mem::size_of::<Value>()));
        // The worker's own spill lands in it: the copy goes, the next read sees the array.
        w.blocked = false;
        w.compute((0, anchor));
        assert!(!w.blocked && w.spills.contains_key(&(0, anchor)));
        assert!(w.ranges.is_empty() && w.cached_bytes == 0);
        let fresh = w.range_values(0, block);
        assert_eq!((fresh.first(), fresh.last()), (Some(&Value::Number(2.0)), Some(&Value::Number(101.0))));
        // A block holding a formula not done yet: the read is handed back, nothing is copied.
        let reader = CellRef::parse("D1").unwrap();
        let mut second_book = Workbook::new();
        second_book.sheet_mut(0).unwrap().cells.set(reader, Cell::formula(Formula::new("=5")));
        let mut second_exprs = HashMap::default();
        second_exprs.insert((0, reader), std::sync::Arc::new(gridcraft_formula::parse("5").unwrap()));
        let mut second_rng = 1u64;
        let second_host = PassHost::new(&second_book, &second_exprs, None, &mut second_rng, 0.0);
        let mut second_worker_rng = 1u64;
        let mut second_worker = second_host.worker(&mut second_worker_rng, 1 << 20);
        second_worker.depth = 1;
        let _ = second_worker.range_values(0, RangeRef::parse("B1:CW1").unwrap());
        let _ = second_worker.range_values(0, RangeRef::parse("B1:CW1").unwrap());
        assert!(second_worker.blocked && second_worker.ranges.is_empty());
        // A block of long text past the worker's share of the byte budget is not copied.
        let mut third_book = Workbook::new();
        for col in 0..100u32 {
            third_book.sheet_mut(0).unwrap().cells.set(CellRef::new(0, col), Cell::value(Value::Text("x".repeat(20_000).into())));
        }
        let third_exprs = HashMap::default();
        let mut third_rng = 1u64;
        let third_host = PassHost::new(&third_book, &third_exprs, None, &mut third_rng, 0.0);
        let mut third_worker_rng = 1u64;
        let mut third_worker = third_host.worker(&mut third_worker_rng, 1 << 20);
        let wide = RangeRef::parse("A1:CV1").unwrap();
        let _ = third_worker.range_values(0, wide);
        let _ = third_worker.range_values(0, wide);
        assert!(third_worker.ranges.is_empty() && third_worker.cached_bytes == 0);
    }

    /// For tests of `PassHost::visit_range`, which can't be reached from outside this module: the
    /// values it hands over for `range` while the formulas in `pending` (cell, formula) have not been
    /// calculated yet, or `None` when it declines the block.
    pub(crate) fn probe_visit_range(wb: &Workbook, pending: &[(&str, &str)], range: RangeRef) -> Option<Vec<Value>> {
        let mut exprs = HashMap::default();
        for (at, formula) in pending {
            let expr = gridcraft_formula::parse(formula.strip_prefix('=').unwrap_or(formula)).ok()?;
            exprs.insert((0, CellRef::parse(at)?), std::sync::Arc::new(expr));
        }

        // A cell in `pending` is calculated the first time something reads it, which is the state
        // of a real pass partway through; `visit_range` has to cope with the formulas it reads
        // being calculated under it.
        let mut rng = 1u64;
        let mut host = PassHost::new(wb, &exprs, None, &mut rng, 0.0);

        let mut seen = Vec::new();
        host.visit_range(0, range, &mut |v| {
            seen.push(v.clone());
            true
        })
        .then_some(seen)
    }
}
