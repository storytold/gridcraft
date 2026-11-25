//! Conditional formatting evaluation: what extra style, data bar, colour scale or icon a cell
//! gets.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use gridcraft_core::{CellRef, RangeRef, Value, compare};
use gridcraft_model::{CfOperator, CfRule, CfValueKind, Color, PatternType, Sheet, Style, Table, Underline, Workbook, default_icon_thresholds};

/// The conditional look of one cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CfLook {
    /// Differential style (only non-default parts apply).
    pub style: Option<Style>,
    /// Fill from a colour scale (RGB).
    pub scale_fill: Option<[u8; 3]>,
    /// Data bar: fraction 0..=1 of the cell width, colour, gradient.
    pub bar: Option<(f32, [u8; 3], bool)>,
    /// Icon: set name and index (0 = lowest).
    pub icon: Option<(String, usize)>,
    pub hide_value: bool,
}

/// Precomputed statistics per rule (min/max/percentiles/top-N thresholds/duplicates), each
/// table's visible edges, and a per-sheet spatial index over conditional-format ranges.
///
/// Callers either create a fresh `CfCache` per operation/paint or bind its lifetime to
/// `Weak<Sheet>` `Arc` identity (as the grid does, so sheet edits produce a new `Arc<Sheet>`
/// and drop the cache). Index freshness relies on that invariant rather than deep-diffing
/// individual rules or values.
#[derive(Default)]
pub struct CfCache {
    /// Cached per-`(group_id, need)` statistics; `None` marks a group skipped because
    /// `MAX_TOTAL_STATS_SCAN` was already exhausted.
    stats: HashMap<(u32, StatsNeed), Option<RuleStats>>,
    table_edges: HashMap<RangeRef, crate::tables::ShownEdges>,
    stats_cells_scanned: usize,
    index: Option<SheetCfIndex>,
    no_num_fmt: Option<(usize, usize)>,
    scratch_groups: Vec<u32>,
    scratch_ranks: Vec<u32>,
}

struct RuleMeta {
    orig_idx: usize,
    group_id: u32,
    first: Option<CellRef>,
    sets_num_fmt: bool,
}
struct RangeGroup {
    /// Rule ranks into `SheetCfIndex::rules` (already sorted by `(priority, orig_idx)`).
    rule_ranks: Vec<u32>,
    has_num_fmt: bool,
}

struct IntervalNode {
    center: u32,
    is_leaf: bool,
    /// Multi-cell `(range, group_id)` entries sorted ascending by `range.start.row`.
    by_start: Vec<(RangeRef, u32)>,
    /// Multi-cell `(range, group_id)` entries sorted descending by `range.end.row` (empty for leaves).
    by_end: Vec<(RangeRef, u32)>,
    left: Option<usize>,
    right: Option<usize>,
}

/// Spatial index over one sheet's conditional formats:
/// - identical `sqref` lists across rules are grouped once (`RangeGroup`);
/// - exact 1×1 ranges live in `single_cells` (`HashMap`);
/// - multi-cell ranges live in a balanced row interval tree (`multi_tree`).
struct SheetCfIndex {
    sheet: usize,
    rule_count: usize,
    bbox: Option<RangeRef>,
    any_num_fmt: bool,
    rules: Vec<RuleMeta>,
    groups: Vec<RangeGroup>,
    single_cells: HashMap<CellRef, Vec<u32>>,
    multi_tree: Vec<IntervalNode>,
}

impl SheetCfIndex {
    fn build(sh: &Sheet, sheet: usize) -> Self {
        let mut order: Vec<(usize, u32)> = sh.cond_formats.iter().enumerate().map(|(i, cf)| (i, cf.priority)).collect();
        order.sort_by_key(|&(orig_idx, priority)| (priority, orig_idx));

        let mut rules = Vec::with_capacity(order.len());
        let mut any_num_fmt = false;
        for &(orig_idx, _) in &order {
            let sets_num_fmt = sh.cond_formats[orig_idx].rule.style().is_some_and(style_sets_num_fmt);
            any_num_fmt |= sets_num_fmt;
            rules.push(RuleMeta { orig_idx, group_id: u32::MAX, first: None, sets_num_fmt });
        }

        let mut groups: Vec<RangeGroup> = Vec::new();
        let mut group_reps: Vec<usize> = Vec::new();
        let mut buckets: HashMap<u64, Vec<(u32, usize)>> = HashMap::new();
        for (rank, meta) in rules.iter_mut().enumerate() {
            let ranges = &sh.cond_formats[meta.orig_idx].ranges;
            if ranges.is_empty() {
                continue;
            }
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            ranges.hash(&mut hasher);
            let h = hasher.finish();
            let bucket = buckets.entry(h).or_default();
            if let Some(&(gid, _)) = bucket.iter().find(|&&(_, rep)| sh.cond_formats[rep].ranges == *ranges) {
                meta.group_id = gid;
                let g = &mut groups[gid as usize];
                g.rule_ranks.push(rank as u32);
                g.has_num_fmt |= meta.sets_num_fmt;
            } else {
                let gid = groups.len() as u32;
                meta.group_id = gid;
                groups.push(RangeGroup { rule_ranks: vec![rank as u32], has_num_fmt: meta.sets_num_fmt });
                group_reps.push(meta.orig_idx);
                bucket.push((gid, meta.orig_idx));
            }
        }

        let mut bbox: Option<RangeRef> = None;
        let mut single_cells: HashMap<CellRef, Vec<u32>> = HashMap::new();
        let mut unique_multi: Vec<(RangeRef, u32)> = Vec::new();
        for (gid, &rep_idx) in group_reps.iter().enumerate() {
            let ranges = &sh.cond_formats[rep_idx].ranges;
            let mut seen_in_group = (ranges.len() > 1).then(|| HashSet::with_capacity(ranges.len()));
            let mut group_first: Option<CellRef> = None;
            for &r in ranges {
                if let Some(seen) = &mut seen_in_group
                    && !seen.insert(r)
                {
                    continue;
                }
                group_first = Some(match group_first {
                    None => r.start,
                    Some(f) => CellRef::new(f.row.min(r.start.row), f.col.min(r.start.col)),
                });
                bbox = Some(bbox.map_or(r, |b| b.union(&r)));
                if r.is_single() {
                    single_cells.entry(r.start).or_default().push(gid as u32);
                } else {
                    unique_multi.push((r, gid as u32));
                }
            }
            for &rank in &groups[gid].rule_ranks {
                rules[rank as usize].first = group_first;
            }
        }

        let multi_tree = build_interval_tree(unique_multi);

        Self { sheet, rule_count: sh.cond_formats.len(), bbox, any_num_fmt, rules, groups, single_cells, multi_tree }
    }

    fn query_point_groups(&self, c: CellRef, mut emit: impl FnMut(u32) -> bool) {
        if !self.bbox.is_some_and(|b| b.contains(c)) {
            return;
        }
        if let Some(gs) = self.single_cells.get(&c) {
            for &g in gs {
                if !emit(g) {
                    return;
                }
            }
        }
        if self.multi_tree.is_empty() {
            return;
        }
        let mut node_idx = Some(0usize);
        while let Some(ni) = node_idx {
            let node = &self.multi_tree[ni];
            if node.is_leaf {
                for &(r, g) in &node.by_start {
                    if r.contains(c) && !emit(g) {
                        return;
                    }
                }
                break;
            }
            if c.row < node.center {
                for &(r, g) in &node.by_start {
                    if r.start.row > c.row {
                        break;
                    }
                    if c.col >= r.start.col && c.col <= r.end.col && !emit(g) {
                        return;
                    }
                }
                node_idx = node.left;
            } else if c.row > node.center {
                for &(r, g) in &node.by_end {
                    if r.end.row < c.row {
                        break;
                    }
                    if c.col >= r.start.col && c.col <= r.end.col && !emit(g) {
                        return;
                    }
                }
                node_idx = node.right;
            } else {
                for &(r, g) in &node.by_start {
                    if c.col >= r.start.col && c.col <= r.end.col && !emit(g) {
                        return;
                    }
                }
                break;
            }
        }
    }

    fn matches_any(&self, c: CellRef) -> bool {
        if !self.bbox.is_some_and(|b| b.contains(c)) {
            return false;
        }
        if self.single_cells.contains_key(&c) {
            return true;
        }
        let mut hit = false;
        self.query_point_groups(c, |_| {
            hit = true;
            false
        });
        hit
    }

    fn has_num_fmt_at(&self, c: CellRef) -> bool {
        if !self.any_num_fmt || !self.bbox.is_some_and(|b| b.contains(c)) {
            return false;
        }
        let mut hit = false;
        self.query_point_groups(c, |g| {
            if self.groups[g as usize].has_num_fmt {
                hit = true;
                return false;
            }
            true
        });
        hit
    }
}

fn build_interval_tree(entries: Vec<(RangeRef, u32)>) -> Vec<IntervalNode> {
    fn rec(mut entries: Vec<(RangeRef, u32)>, nodes: &mut Vec<IntervalNode>) -> Option<usize> {
        if entries.is_empty() {
            return None;
        }
        let idx = nodes.len();
        if entries.len() <= 8 {
            entries.sort_unstable_by_key(|(r, _)| r.start.row);
            nodes.push(IntervalNode { center: 0, is_leaf: true, by_start: entries, by_end: Vec::new(), left: None, right: None });
            return Some(idx);
        }
        entries.sort_unstable_by_key(|(r, _)| r.start.row);
        let center = entries[entries.len() / 2].0.start.row;
        let mut left = Vec::new();
        let mut mid = Vec::new();
        let mut right = Vec::new();
        for item in entries {
            if item.0.end.row < center {
                left.push(item);
            } else if item.0.start.row > center {
                right.push(item);
            } else {
                mid.push(item);
            }
        }
        let mut by_end = mid.clone();
        by_end.sort_unstable_by_key(|(a, _)| std::cmp::Reverse(a.end.row));
        nodes.push(IntervalNode { center, is_leaf: false, by_start: mid, by_end, left: None, right: None });
        let l = rec(left, nodes);
        let r = rec(right, nodes);
        nodes[idx].left = l;
        nodes[idx].right = r;
        Some(idx)
    }
    let mut nodes = Vec::new();
    rec(entries, &mut nodes);
    nodes
}

#[derive(Default, Clone)]
struct RuleStats {
    sorted: Vec<f64>,
    avg: f64,
    std: f64,
    counts: HashMap<String, usize>,
}

/// Which statistics a rule reads. Scanning the rule's whole range for them costs O(range), so
/// rules that read none (cell value, formula, text, blanks, errors, dates) never scan.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum StatsNeed {
    /// Sorted numbers, their mean and deviation (top/bottom N, above average, scales, bars, icons).
    Numbers,
    /// How often each displayed text occurs (duplicate / unique values).
    Counts,
}

fn stats_need(rule: &CfRule) -> Option<StatsNeed> {
    match rule {
        CfRule::Duplicate { .. } | CfRule::Unique { .. } => Some(StatsNeed::Counts),
        // only two- and three-colour scales paint: no statistics for the others
        CfRule::ColorScale { stops } if !matches!(stops.len(), 2 | 3) => None,
        CfRule::Top10 { .. } | CfRule::AboveAverage { .. } | CfRule::ColorScale { .. } | CfRule::DataBar { .. } | CfRule::IconSet { .. } => {
            Some(StatsNeed::Numbers)
        }
        _ => None,
    }
}

impl RuleStats {
    fn build(sh: &Sheet, ranges: &[RangeRef], need: StatsNeed, limit: usize) -> (RuleStats, usize) {
        let (sorted, counts, scanned) = numbers_in(sh, ranges, need, limit);
        let n = sorted.len().max(1) as f64;
        let avg = sorted.iter().sum::<f64>() / n;
        let std = (sorted.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / n).sqrt();
        (RuleStats { sorted, avg, std, counts }, scanned)
    }
}

#[cfg(test)]
thread_local! {
    /// Cells `numbers_in` visited on this thread (tests count the work a lookup does).
    static SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Conditional-format rules `CfCache::look` evaluated on this thread.
    static RULES_EVALUATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_scanned() {
    SCANNED.with(|n| n.set(0));
}

#[cfg(test)]
pub(crate) fn scanned_count() -> usize {
    SCANNED.with(std::cell::Cell::get)
}

#[cfg(test)]
fn reset_rules_evaluated() {
    RULES_EVALUATED.with(|n| n.set(0));
}

#[cfg(test)]
fn rules_evaluated_count() -> usize {
    RULES_EVALUATED.with(std::cell::Cell::get)
}

/// Most conditional-format rules evaluated for any single cell in [`CfCache::look`]: bounds
/// hostile sheets stacking hundreds or thousands of overlapping rules (e.g. `sqref="A:XFD"`)
/// across every caller (screen paint, autofit, filters, exports, PDF) while keeping normal sheets
/// unaffected.
const MAX_CF_RULES_PER_CELL: usize = 64;

/// Most cells one rule's statistics visit, across all of its ranges together: a rule repeating
/// the same range many times must not multiply the scan.
const STATS_SCAN_LIMIT: usize = 2_000_000;
/// Aggregate cells scanned across all distinct `(group_id, need)` statistics builds in one
/// `CfCache`. Sized at 16x `STATS_SCAN_LIMIT` so legitimate multi-column workbooks never starve,
/// while hostile sheets with thousands of distinct overlapping full-column stats rules stay bounded.
const MAX_TOTAL_STATS_SCAN: usize = 16 * STATS_SCAN_LIMIT;

/// Steps through the spilled cells inside `r`, row-major: `Some(cell)` for each, and `None` for a
/// jump past the spills left or right of the columns on a row (one step per row, however many
/// there are), so unrelated spills between a narrow range's corners cost little.
fn spill_steps(sh: &Sheet, r: RangeRef) -> impl Iterator<Item = Option<CellRef>> + '_ {
    let mut cur = Some(r.start);
    std::iter::from_fn(move || {
        let at = cur.filter(|at| *at <= r.end)?;
        let (&c, _) = sh.spill.range(at..=r.end).next()?;
        if c.col < r.start.col {
            cur = Some(CellRef::new(c.row, r.start.col));
            Some(None)
        } else if c.col > r.end.col {
            cur = c.row.checked_add(1).map(|row| CellRef::new(row, r.start.col));
            Some(None)
        } else {
            cur = c.col.checked_add(1).map(|col| CellRef::new(c.row, col));
            Some(Some(c))
        }
    })
}

fn write_lowercase_display(v: &Value, buf: &mut String) {
    buf.clear();
    match v {
        Value::Text(s) => {
            if s.is_ascii() {
                buf.push_str(s);
                buf.make_ascii_lowercase();
            } else {
                buf.push_str(&s.to_lowercase());
            }
        }
        Value::Error(e) => {
            buf.push_str(e.as_str());
            buf.make_ascii_lowercase();
        }
        Value::Bool(b) => buf.push_str(if *b { "true" } else { "false" }),
        _ => {
            let d = v.display();
            if d.is_ascii() {
                buf.push_str(&d);
                buf.make_ascii_lowercase();
            } else {
                buf.push_str(&d.to_lowercase());
            }
        }
    }
}

fn count_value(v: &Value, counts: &mut HashMap<String, usize>, key: &mut String) {
    if v.is_empty() {
        return;
    }
    if let Value::Text(s) = v
        && s.bytes().all(|b| b.is_ascii() && !b.is_ascii_uppercase())
    {
        if let Some(cnt) = counts.get_mut(s.as_ref()) {
            *cnt += 1;
        } else {
            counts.insert(s.to_string(), 1);
        }
        return;
    }
    write_lowercase_display(v, key);
    if let Some(cnt) = counts.get_mut(key.as_str()) {
        *cnt += 1;
    } else {
        counts.insert(key.clone(), 1);
    }
}

fn numbers_in(sh: &Sheet, ranges: &[RangeRef], need: StatsNeed, limit: usize) -> (Vec<f64>, HashMap<String, usize>, usize) {
    let mut nums = Vec::new();
    let mut counts = HashMap::new();
    let mut key = String::new();
    let mut budget = limit.min(STATS_SCAN_LIMIT);
    let initial_budget = budget;
    let mut seen: HashSet<RangeRef> = HashSet::new();
    let used = sh.used_range();
    let has_spills = !sh.spill.is_empty();
    for r in ranges {
        if budget == 0 {
            break;
        }
        let r = match used.and_then(|u| r.intersection(&u)) {
            Some(x) => x,
            None => continue,
        };
        // The same range listed twice counts its cells once.
        if !seen.insert(r) {
            continue;
        }
        // Only stored and spilled cells hold values: blank cells of a sparse range cost nothing,
        // so a value far down a huge range is still counted. Every cell looked at is charged.
        #[cfg(test)]
        let mut scanned_here = 0usize;
        for (c, cell) in sh.cells.iter_range(r).take(budget) {
            budget -= 1;
            #[cfg(test)]
            {
                scanned_here += 1;
            }
            let v: &Value = if !cell.value.is_empty() || cell.formula.is_some() {
                &cell.value
            } else if has_spills {
                sh.spill.get(&c).unwrap_or(&Value::Empty)
            } else {
                &Value::Empty
            };
            match need {
                StatsNeed::Numbers => {
                    if let Value::Number(n) = v {
                        nums.push(*n);
                    }
                }
                StatsNeed::Counts => count_value(v, &mut counts, &mut key),
            }
        }
        if has_spills && budget > 0 {
            for c in spill_steps(sh, r).take(budget) {
                budget -= 1;
                let Some(c) = c.filter(|c| sh.cells.get(*c).is_none()) else { continue };
                #[cfg(test)]
                {
                    scanned_here += 1;
                }
                let v = sh.spill.get(&c).unwrap_or(&Value::Empty);
                match need {
                    StatsNeed::Numbers => {
                        if let Value::Number(n) = v {
                            nums.push(*n);
                        }
                    }
                    StatsNeed::Counts => count_value(v, &mut counts, &mut key),
                }
            }
        }
        #[cfg(test)]
        SCANNED.with(|n| n.set(n.get() + scanned_here));
    }
    nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (nums, counts, initial_budget - budget)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0).clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let a = sorted.get(lo).copied().unwrap_or(0.0);
    let b = sorted.get(hi).copied().unwrap_or(a);
    a + (b - a) * (rank - lo as f64)
}

fn threshold(wb: &Workbook, sheet: usize, at: CellRef, st: &RuleStats, k: &CfValueKind) -> f64 {
    let min = st.sorted.first().copied().unwrap_or(0.0);
    let max = st.sorted.last().copied().unwrap_or(0.0);
    match k {
        CfValueKind::Min => min,
        CfValueKind::Max => max,
        CfValueKind::Number(n) => *n,
        CfValueKind::Percent(p) => min + (max - min) * p / 100.0,
        CfValueKind::Percentile(p) => percentile(&st.sorted, *p),
        CfValueKind::Formula(f) => gridcraft_calc::evaluate(wb, sheet, at, f).to_number().unwrap_or(0.0),
    }
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f64 + (b[0] as f64 - a[0] as f64) * t).round() as u8,
        (a[1] as f64 + (b[1] as f64 - a[1] as f64) * t).round() as u8,
        (a[2] as f64 + (b[2] as f64 - a[2] as f64) * t).round() as u8,
    ]
}

pub use gridcraft_model::icon_count;

struct RuleEval<'a> {
    wb: &'a Workbook,
    sheet: usize,
    cell: CellRef,
    first: CellRef,
    value: &'a Value,
    text: &'a std::cell::OnceCell<String>,
    text_lower: &'a std::cell::OnceCell<String>,
    stats: &'a RuleStats,
}

impl RuleEval<'_> {
    fn text(&self) -> &str {
        self.text.get_or_init(|| self.value.display())
    }

    fn text_lower(&self) -> &str {
        self.text_lower.get_or_init(|| self.text().to_lowercase())
    }
}

fn simple_rule_style<'a>(ctx: &RuleEval<'_>, rule: &'a CfRule) -> Option<&'a Style> {
    let RuleEval { wb, sheet, cell, first, value, stats, .. } = ctx;
    match rule {
        CfRule::CellIs { op, a, b, style } => {
            let eval = |f: &str| gridcraft_calc::evaluate(wb, *sheet, *cell, &relative(f, *first, *cell));
            let av = eval(a);
            let matches = match op {
                CfOperator::Between | CfOperator::NotBetween => {
                    let bv = b.as_deref().map(eval).unwrap_or_default();
                    let (lo, hi) = if compare(&av, &bv) == std::cmp::Ordering::Greater { (bv, av) } else { (av, bv) };
                    let within = compare(value, &lo) != std::cmp::Ordering::Less && compare(value, &hi) != std::cmp::Ordering::Greater;
                    if *op == CfOperator::Between { within } else { !within }
                }
                CfOperator::Equal => compare(value, &av) == std::cmp::Ordering::Equal,
                CfOperator::NotEqual => compare(value, &av) != std::cmp::Ordering::Equal,
                CfOperator::Greater => compare(value, &av) == std::cmp::Ordering::Greater,
                CfOperator::Less => compare(value, &av) == std::cmp::Ordering::Less,
                CfOperator::GreaterOrEqual => compare(value, &av) != std::cmp::Ordering::Less,
                CfOperator::LessOrEqual => compare(value, &av) != std::cmp::Ordering::Greater,
            };
            (matches && !value.is_error()).then_some(style.as_ref())
        }
        CfRule::Expression { formula, style } => {
            gridcraft_calc::evaluate(wb, *sheet, *cell, &relative(formula, *first, *cell)).to_bool().unwrap_or(false).then_some(style.as_ref())
        }
        CfRule::ContainsText { text: needle, style } => {
            (!value.is_error() && ctx.text_lower().contains(&needle.to_lowercase())).then_some(style.as_ref())
        }
        CfRule::NotContainsText { text: needle, style } => {
            (value.is_error() || !ctx.text_lower().contains(&needle.to_lowercase())).then_some(style.as_ref())
        }
        CfRule::BeginsWith { text: needle, style } => {
            (!value.is_error() && ctx.text_lower().starts_with(&needle.to_lowercase())).then_some(style.as_ref())
        }
        CfRule::EndsWith { text: needle, style } => {
            (!value.is_error() && ctx.text_lower().ends_with(&needle.to_lowercase())).then_some(style.as_ref())
        }
        CfRule::Blanks { style } => (value.is_empty() || ctx.text().trim().is_empty()).then_some(style.as_ref()),
        CfRule::NoBlanks { style } => (!value.is_empty() && !ctx.text().trim().is_empty()).then_some(style.as_ref()),
        CfRule::Errors { style } => value.is_error().then_some(style.as_ref()),
        CfRule::NoErrors { style } => (!value.is_error()).then_some(style.as_ref()),
        CfRule::Duplicate { style } => (!value.is_empty() && stats.counts.get(ctx.text_lower()).copied().unwrap_or(0) > 1).then_some(style.as_ref()),
        CfRule::Unique { style } => (!value.is_empty() && stats.counts.get(ctx.text_lower()).copied().unwrap_or(0) == 1).then_some(style.as_ref()),
        _ => None,
    }
}

fn time_period_matches(wb: &Workbook, period: &str, serial: f64) -> bool {
    let today = gridcraft_core::date::serial_in_system(wb.date_system, gridcraft_calc::now_serial()).floor();
    let day = serial.floor();
    let date = |serial| gridcraft_core::date::datetime_from_serial(wb.date_system, serial);
    let week_start = today - date(today).map(|value| value.weekday as f64).unwrap_or(0.0);
    let (year, month) = date(today).map(|value| (value.year, value.month)).unwrap_or((2026, 1));
    let year_month = date(day).map(|value| (value.year, value.month));
    let month_matches = |offset: i32| {
        let shifted_month = month as i32 + offset;
        let shifted_year = year + (shifted_month - 1).div_euclid(12);
        let shifted_month = (shifted_month - 1).rem_euclid(12) + 1;
        year_month == Some((shifted_year, shifted_month as u32))
    };
    match period {
        "yesterday" => day == today - 1.0,
        "today" => day == today,
        "tomorrow" => day == today + 1.0,
        "last7Days" => day > today - 7.0 && day <= today,
        "lastWeek" => day >= week_start - 7.0 && day < week_start,
        "thisWeek" => day >= week_start && day < week_start + 7.0,
        "nextWeek" => day >= week_start + 7.0 && day < week_start + 14.0,
        "lastMonth" => month_matches(-1),
        "thisMonth" => month_matches(0),
        "nextMonth" => month_matches(1),
        _ => false,
    }
}

impl CfCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// [`crate::tables::shown_edges`] of `t`, found once per cache.
    pub fn table_edges(&mut self, sh: &Sheet, t: &Table) -> crate::tables::ShownEdges {
        *self.table_edges.entry(t.range).or_insert_with(|| crate::tables::shown_edges(sh, t))
    }

    fn ensure_index(&mut self, sh: &Sheet, sheet: usize) {
        let stale = self.index.as_ref().is_none_or(|idx| idx.sheet != sheet || idx.rule_count != sh.cond_formats.len());
        if stale {
            // Group IDs are reassigned whenever the index is rebuilt, so cached `(group_id, need)`
            // statistics must reset together with the index.
            self.stats.clear();
            self.stats_cells_scanned = 0;
            self.no_num_fmt = None;
            self.index = Some(SheetCfIndex::build(sh, sheet));
        }
    }

    /// True when at least one conditional formatting rule on `sheet` covers `c`.
    pub(crate) fn may_apply(&mut self, wb: &Workbook, sheet: usize, c: CellRef) -> bool {
        let Some(sh) = wb.sheet(sheet) else { return false };
        if sh.cond_formats.is_empty() {
            return false;
        }
        self.ensure_index(sh, sheet);
        self.index.as_ref().is_some_and(|idx| idx.matches_any(c))
    }

    /// Conditional look of `c`, or `None` when no rule applies. Evaluates at most
    /// [`MAX_CF_RULES_PER_CELL`] highest-priority covering rules per cell.
    pub fn look(&mut self, wb: &Workbook, sheet: usize, c: CellRef) -> Option<CfLook> {
        let sh = wb.sheet(sheet)?;
        if sh.cond_formats.is_empty() {
            return None;
        }
        self.ensure_index(sh, sheet);
        let index = self.index.as_ref()?;
        let matched_groups = &mut self.scratch_groups;
        matched_groups.clear();
        index.query_point_groups(c, |g| {
            matched_groups.push(g);
            true
        });
        if matched_groups.is_empty() {
            return None;
        }
        let rule_ranks: &[u32] = if matched_groups.len() == 1 {
            let g_ranks = &index.groups[matched_groups[0] as usize].rule_ranks;
            &g_ranks[..g_ranks.len().min(MAX_CF_RULES_PER_CELL)]
        } else {
            matched_groups.sort_unstable();
            matched_groups.dedup();
            let ranks = &mut self.scratch_ranks;
            ranks.clear();
            for &g in matched_groups.iter() {
                let g_ranks = &index.groups[g as usize].rule_ranks;
                ranks.extend_from_slice(&g_ranks[..g_ranks.len().min(MAX_CF_RULES_PER_CELL)]);
            }
            if ranks.len() > MAX_CF_RULES_PER_CELL {
                ranks.select_nth_unstable(MAX_CF_RULES_PER_CELL);
                ranks.truncate(MAX_CF_RULES_PER_CELL);
            }
            ranks.sort_unstable();
            ranks.as_slice()
        };
        let mut out = CfLook::default();
        let mut any = false;
        let v = sh.value(c);
        let text = std::cell::OnceCell::new();
        let text_lower = std::cell::OnceCell::new();
        let no_stats = RuleStats::default();
        let stats_map = &mut self.stats;
        let stats_scanned = &mut self.stats_cells_scanned;
        for &rank in rule_ranks {
            #[cfg(test)]
            RULES_EVALUATED.with(|n| n.set(n.get() + 1));
            let meta = &index.rules[rank as usize];
            let idx = meta.orig_idx;
            let cf = &sh.cond_formats[idx];
            let first = meta.first.unwrap_or(c);
            let st: &RuleStats = match stats_need(&cf.rule) {
                Some(need) => {
                    let cached = stats_map.entry((meta.group_id, need)).or_insert_with(|| {
                        let remaining = MAX_TOTAL_STATS_SCAN.saturating_sub(*stats_scanned);
                        if remaining == 0 {
                            return None;
                        }
                        let (built, scanned) = RuleStats::build(sh, &cf.ranges, need, remaining.min(STATS_SCAN_LIMIT));
                        *stats_scanned = stats_scanned.saturating_add(scanned);
                        Some(built)
                    });
                    let Some(st) = cached.as_ref() else {
                        continue;
                    };
                    st
                }
                None => &no_stats,
            };
            let context = RuleEval { wb, sheet, cell: c, first, value: &v, text: &text, text_lower: &text_lower, stats: st };
            let matched_style = simple_rule_style(&context, &cf.rule).or_else(|| match &cf.rule {
                CfRule::Top10 { bottom, percent, rank, style } => match v.as_f64() {
                    Some(n) if !st.sorted.is_empty() => {
                        let k = if *percent { ((st.sorted.len() as f64) * (*rank as f64) / 100.0).floor().max(1.0) as usize } else { *rank as usize };
                        let k = k.clamp(1, st.sorted.len());
                        let ok = if *bottom {
                            n <= st.sorted.get(k - 1).copied().unwrap_or(n)
                        } else {
                            n >= st.sorted.get(st.sorted.len() - k).copied().unwrap_or(n)
                        };
                        ok.then_some(style.as_ref())
                    }
                    _ => None,
                },
                CfRule::AboveAverage { below, equal, std_dev, style } => match v.as_f64() {
                    Some(n) if !st.sorted.is_empty() => {
                        let thr = st.avg + if *below { -1.0 } else { 1.0 } * st.std * *std_dev as f64;
                        let ok = if *below { n < thr || (*equal && n == thr) } else { n > thr || (*equal && n == thr) };
                        ok.then_some(style.as_ref())
                    }
                    _ => None,
                },
                CfRule::TimePeriod { period, style } => match v.as_f64() {
                    Some(n) if time_period_matches(wb, period, n) => Some(style.as_ref()),
                    _ => None,
                },
                CfRule::ColorScale { stops } => {
                    // Only two- and three-colour scales paint: evaluate no thresholds for others.
                    if let Some(n) = v.as_f64().filter(|_| matches!(stops.len(), 2 | 3)) {
                        let pts: Vec<(f64, [u8; 3])> = stops
                            .iter()
                            .map(|(k, col)| (threshold(wb, sheet, c, st, k), col.resolve(&wb.theme).unwrap_or([255, 255, 255])))
                            .collect();
                        let rgb = match pts.as_slice() {
                            [(a, ca), (b, cb)] => lerp(*ca, *cb, if b > a { (n - a) / (b - a) } else { 0.0 }),
                            [(a, ca), (m, cm), (b, cb)] => {
                                if n <= *m {
                                    lerp(*ca, *cm, if m > a { (n - a) / (m - a) } else { 0.0 })
                                } else {
                                    lerp(*cm, *cb, if b > m { (n - m) / (b - m) } else { 1.0 })
                                }
                            }
                            _ => [255, 255, 255],
                        };
                        if out.scale_fill.is_none() {
                            out.scale_fill = Some(rgb);
                            any = true;
                        }
                    }
                    None
                }
                CfRule::DataBar { min, max, color, gradient, show_value } => {
                    if let Some(n) = v.as_f64() {
                        let lo = threshold(wb, sheet, c, st, min);
                        let hi = threshold(wb, sheet, c, st, max);
                        let lo = if lo > 0.0 { 0.0 } else { lo };
                        let frac = if hi > lo { ((n - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 1.0 };
                        if out.bar.is_none() {
                            out.bar = Some((
                                (frac as f32) * 0.9 + 0.1 * (frac > 0.0) as u8 as f32,
                                color.resolve(&wb.theme).unwrap_or([0x63, 0x8E, 0xC6]),
                                *gradient,
                            ));
                            out.hide_value |= !show_value;
                            any = true;
                        }
                    }
                    None
                }
                CfRule::IconSet { set, thresholds, reverse, show_value } => {
                    if let Some(n) = v.as_f64() {
                        let k = icon_count(set);
                        // One threshold per icon, the first being the lower bound (as in the file);
                        // also accept the lower bound left out, else Excel's defaults.
                        let defaults;
                        let lower = match thresholds.len() {
                            n if n == k => thresholds.get(1..).unwrap_or_default(),
                            n if n + 1 == k => thresholds.as_slice(),
                            _ => {
                                defaults = default_icon_thresholds(set);
                                defaults.get(1..).unwrap_or_default()
                            }
                        };
                        let cuts: Vec<f64> = lower.iter().map(|t| threshold(wb, sheet, c, st, t)).collect();
                        let mut idx = cuts.iter().filter(|cut| n >= **cut).count();
                        if *reverse {
                            idx = k - 1 - idx.min(k - 1);
                        }
                        if out.icon.is_none() {
                            out.icon = Some((set.clone(), idx.min(k - 1)));
                            out.hide_value |= !show_value;
                            any = true;
                        }
                    }
                    None
                }
                _ => None,
            });
            if let Some(st) = matched_style {
                any = true;
                // Every matching rule applies; where two set the same part, the rule with the
                // higher priority (seen first) wins.
                match &mut out.style {
                    None => out.style = Some(st.clone()),
                    Some(prev) => layer_under(prev, st),
                }
                if cf.stop_if_true {
                    break;
                }
            }
        }
        any.then_some(out)
    }

    /// Effective number format code of `c`, honouring matching conditional formatting `<dxf><numFmt>`
    /// overrides. Zero overhead when no conditional formatting rule covering `c` specifies a `numFmt`.
    pub fn effective_num_fmt<'a>(&mut self, wb: &'a Workbook, sheet: usize, sh: &'a Sheet, c: CellRef) -> std::borrow::Cow<'a, str> {
        let base = wb.styles.get(sh.style_id(c)).num_fmt.as_str();
        if sh.cond_formats.is_empty() {
            return std::borrow::Cow::Borrowed(base);
        }
        let key = (sheet, sh.cond_formats.len());
        if self.index.as_ref().is_none_or(|idx| (idx.sheet, idx.rule_count) != key) {
            if self.no_num_fmt == Some(key) {
                return std::borrow::Cow::Borrowed(base);
            }
            if !sh.cond_formats.iter().any(|cf| cf.rule.style().is_some_and(style_sets_num_fmt)) {
                self.no_num_fmt = Some(key);
                return std::borrow::Cow::Borrowed(base);
            }
            self.ensure_index(sh, sheet);
        }
        if !self.index.as_ref().is_some_and(|idx| idx.has_num_fmt_at(c)) {
            return std::borrow::Cow::Borrowed(base);
        }
        if let Some(l) = self.look(wb, sheet, c)
            && let Some(st) = l.style
            && style_sets_num_fmt(&st)
        {
            return std::borrow::Cow::Owned(st.num_fmt.as_str().to_string());
        }
        std::borrow::Cow::Borrowed(base)
    }
}

/// Whether a differential style explicitly sets a number format override (non-General, or stated General).
pub(crate) fn style_sets_num_fmt(st: &Style) -> bool {
    st.num_fmt.as_str() != "General" || st.dxf_stated.num_fmt
}

/// Whether any conditional formatting rule on `sh` covering `c` carries a differential `numFmt`.
pub(crate) fn has_num_fmt_rule(sh: &Sheet, c: CellRef) -> bool {
    if sh.cond_formats.is_empty() {
        return false;
    }
    sh.cond_formats.iter().any(|cf| cf.rule.style().is_some_and(style_sets_num_fmt) && cf.ranges.iter().any(|r| r.contains(c)))
}

/// Adds the parts `lower` sets that `style` leaves unset (differential styles: default parts
/// are unset).
fn layer_under(style: &mut Style, lower: &Style) {
    if style.align.reading_order.is_none() {
        style.align.reading_order = lower.align.reading_order;
    }
    if style.fill.pattern == PatternType::None {
        style.fill = lower.fill;
    }
    if style.font.color == Color::Auto {
        style.font.color = lower.font.color;
    }
    style.font.bold |= lower.font.bold;
    style.font.italic |= lower.font.italic;
    style.font.strike |= lower.font.strike;
    if style.font.underline == Underline::None {
        style.font.underline = lower.font.underline;
    }
    let b = &mut style.borders;
    for (dst, src) in [
        (&mut b.left, lower.borders.left),
        (&mut b.right, lower.borders.right),
        (&mut b.top, lower.borders.top),
        (&mut b.bottom, lower.borders.bottom),
        (&mut b.diag_down, lower.borders.diag_down),
        (&mut b.diag_up, lower.borders.diag_up),
    ] {
        if dst.is_none() {
            *dst = src;
        }
    }
    if !style_sets_num_fmt(style) {
        style.num_fmt = lower.num_fmt.clone();
        style.dxf_stated.num_fmt = lower.dxf_stated.num_fmt;
    }
}

/// Shifts a rule formula written for the top-left cell of the range to `c`.
fn relative(f: &str, first: CellRef, c: CellRef) -> String {
    if first == c {
        return f.to_string();
    }
    match gridcraft_formula::parse(f.trim_start_matches('=')) {
        Ok(e) => {
            gridcraft_formula::print(&gridcraft_formula::adjust::shift_relative(e, c.row as i64 - first.row as i64, c.col as i64 - first.col as i64))
        }
        Err(_) => f.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use gridcraft_model::CondFormat;
    use serde_json::json;

    use super::*;

    #[test]
    fn reading_order_respects_conditional_rule_priority_and_explicit_context() {
        for (higher, expected) in [
            (json!({"font": {"bold": true}}), "RightToLeft"),
            (json!({"align": {"reading_order": "Context"}}), "Context"),
            (json!({"align": {"reading_order": "LeftToRight"}}), "LeftToRight"),
        ] {
            let mut s = crate::Session::new();
            s.new_workbook();
            s.execute("cell.set", json!({"cell": "A1", "input": "مرحبا 123 ABC"})).unwrap();
            for style in [json!({"align": {"reading_order": "RightToLeft"}}), higher] {
                s.execute("home.conditionalFormat", json!({"range": "A1", "rule": {"type": "noBlanks", "style": style}})).unwrap();
            }
            let wb = &s.doc().unwrap().wb;
            let (reopened, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(wb).unwrap()).unwrap();
            for wb in [wb, &reopened] {
                let style = CfCache::new().look(wb, 0, CellRef::new(0, 0)).unwrap().style.unwrap();
                assert_eq!(serde_json::to_value(style.align).unwrap()["reading_order"], expected);
            }
        }
    }

    #[test]
    fn every_matching_rule_applies() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[200], [400], [510]]})).unwrap();
        let rule = |op: &str, value: &str, preset: &str| json!({"range": "A1:A3", "rule": {"type": "cellIs", "operator": op, "value": value, "preset": preset}});
        s.execute("home.conditionalFormat", rule("greaterThan", "300", "lightRedFill")).unwrap();
        s.execute("home.conditionalFormat", rule("greaterThan", "500", "redBorder")).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style);
        assert!(look("A1").is_none());
        let only_fill = look("A2").unwrap();
        assert_eq!(only_fill.fill.pattern, PatternType::Solid);
        assert!(only_fill.borders.top.is_none());
        let both = look("A3").unwrap();
        assert_eq!(both.fill, only_fill.fill);
        assert!(!both.borders.top.is_none());
    }

    #[test]
    fn cell_value_rules_on_blanks_and_errors() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[3, "=NA()"], [null, 7], ["=NA()", null], [" ", null], [7, null]]})).unwrap();
        let rule = |range: &str, r: serde_json::Value| json!({"range": range, "rule": r});
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "cellIs", "operator": "less", "value": "5", "preset": "lightRedFill"})))
            .unwrap();
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "noBlanks", "preset": "redBorder"}))).unwrap();
        s.execute("home.conditionalFormat", rule("B1:B2", json!({"type": "cellIs", "operator": "greater", "value": "5", "preset": "lightRedFill"})))
            .unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style);
        let fill = |st: Option<Style>| st.is_some_and(|st| st.fill.pattern == PatternType::Solid);
        let border = |st: Option<Style>| st.is_some_and(|st| !st.borders.top.is_none());
        // Less than 5: 3 and the blank cell (as 0), not #N/A.
        assert_eq!([look("A1"), look("A2"), look("A3"), look("A5")].map(fill), [true, true, false, false]);
        // Greater than 5: 7, not #N/A.
        assert_eq!([look("B1"), look("B2")].map(fill), [false, true]);
        // No blanks: neither the empty cell nor the one holding a space.
        assert_eq!([look("A1"), look("A2"), look("A4")].map(border), [true, false, false]);
    }

    /// The statistics as every rule used to get them: one pass collecting both parts.
    fn all_stats_before(sh: &Sheet, ranges: &[RangeRef]) -> RuleStats {
        let mut nums = Vec::new();
        let mut counts = HashMap::new();
        for r in ranges {
            let Some(r) = sh.used_range().and_then(|u| r.intersection(&u)) else { continue };
            for c in r.iter() {
                let v = sh.value(c);
                if let Value::Number(n) = v {
                    nums.push(n);
                }
                if !v.is_empty() {
                    *counts.entry(v.display().to_lowercase()).or_insert(0) += 1;
                }
            }
        }
        nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = nums.len().max(1) as f64;
        let avg = nums.iter().sum::<f64>() / n;
        let std = (nums.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / n).sqrt();
        RuleStats { sorted: nums, avg, std, counts }
    }

    #[test]
    fn needed_statistics_equal_the_full_ones() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[3, "x"], [7, "X"], [3, null], ["=NA()", 2.5], [" ", 9], [-1, "y"]]})).unwrap();
        let d = s.doc().unwrap();
        let sh = d.wb.sheet(0).unwrap();
        for ranges in ["A1:B6", "A1:A3", "B2:B9", "D1:E4", "A1:A2"].map(|r| vec![RangeRef::parse(r).unwrap()]) {
            let full = all_stats_before(sh, &ranges);
            let (numbers, _) = RuleStats::build(sh, &ranges, StatsNeed::Numbers, STATS_SCAN_LIMIT);
            assert_eq!((numbers.sorted, numbers.avg.to_bits(), numbers.std.to_bits()), (full.sorted, full.avg.to_bits(), full.std.to_bits()));
            assert_eq!(RuleStats::build(sh, &ranges, StatsNeed::Counts, STATS_SCAN_LIMIT).0.counts, full.counts);
        }
    }

    #[test]
    fn rules_that_read_statistics_still_match() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1, "a"], [2, "A"], [3, "b"], [10, "c"], [2, null]]})).unwrap();
        let rule = |range: &str, r: serde_json::Value| json!({"range": range, "rule": r});
        s.execute("home.conditionalFormat", rule("B1:B5", json!({"type": "duplicate", "preset": "lightRedFill"}))).unwrap();
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "top10", "rank": 1, "preset": "redBorder"}))).unwrap();
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "colorScale"}))).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap());
        let fill = |l: Option<CfLook>| l.and_then(|l| l.style).is_some_and(|st| st.fill.pattern == PatternType::Solid);
        // Duplicates are case-insensitive texts: "a" and "A".
        assert_eq!(["B1", "B2", "B3", "B4", "B5"].map(|a| fill(look(a))), [true, true, false, false, false]);
        // The top value gets the border, the scale runs from the minimum to the maximum.
        assert!(look("A4").and_then(|l| l.style).is_some_and(|st| !st.borders.top.is_none()));
        assert!(look("A1").and_then(|l| l.style).is_none_or(|st| st.borders.top.is_none()));
        assert_eq!(look("A1").and_then(|l| l.scale_fill), Some([0xF8, 0x69, 0x6B]));
        assert_eq!(look("A4").and_then(|l| l.scale_fill), Some([0x63, 0xBE, 0x7B]));
    }

    /// Looking cells up does work in proportion to the cells asked for, not the rule's range: the
    /// statistics are only gathered for rules that read them, and once per rule.
    #[test]
    fn lookups_do_not_scan_the_range_for_rules_without_statistics() {
        let mut s = crate::Session::new();
        s.new_workbook();
        let rows: Vec<serde_json::Value> = (1..=10_000).map(|i| json!([i, i % 7])).collect();
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute(
            "home.conditionalFormat",
            json!({"range": "A1:B10000", "rule": {"type": "expression", "formula": "=MOD(ROW(),2)=0", "preset": "lightRedFill"}}),
        )
        .unwrap();
        s.execute(
            "home.conditionalFormat",
            json!({"range": "A1:B10000", "rule": {"type": "cellIs", "operator": "greater", "value": "5", "preset": "redBorder"}}),
        )
        .unwrap();
        s.execute("home.conditionalFormat", json!({"range": "A1:B10000", "rule": {"type": "noBlanks", "preset": "redBorder"}})).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        SCANNED.with(|n| n.set(0));
        for r in 1..=50 {
            for col in ["A", "B"] {
                cache.look(&d.wb, 0, CellRef::parse(&format!("{col}{r}")).unwrap());
            }
        }
        assert_eq!(SCANNED.with(|n| n.get()), 0);
        // A rule that does read them scans its range once, however many cells are looked up.
        s.execute("home.conditionalFormat", json!({"range": "A1:A10000", "rule": {"type": "top10", "preset": "redBorder"}})).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        SCANNED.with(|n| n.set(0));
        for r in 1..=50 {
            cache.look(&d.wb, 0, CellRef::parse(&format!("A{r}")).unwrap());
        }
        assert_eq!(SCANNED.with(|n| n.get()), 10_000);
    }

    /// A rule listing the same range many times scans it once, and never more than the shared
    /// limit however many ranges it holds.
    #[test]
    fn repeated_ranges_share_one_scan_budget() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3]]})).unwrap();
        s.execute("range.setValues", json!({"range": "XFD1048576", "values": [[4]]})).unwrap();
        let d = s.doc().unwrap();
        let sh = d.wb.sheet(0).unwrap();
        let dense = RangeRef::parse("A1:XFD1048576").unwrap();
        let small = RangeRef::parse("A1:A3").unwrap();
        SCANNED.with(|n| n.set(0));
        let (st, _) = RuleStats::build(sh, &vec![small; 1000], StatsNeed::Numbers, STATS_SCAN_LIMIT);
        assert_eq!(st.sorted, vec![1.0, 2.0, 3.0]);
        assert_eq!(SCANNED.with(|n| n.get()), 3);
        let shifted: Vec<RangeRef> = (0..50).map(|i| RangeRef::new(CellRef::new(i, 0), dense.end)).collect();
        SCANNED.with(|n| n.set(0));
        let _ = RuleStats::build(sh, &shifted, StatsNeed::Numbers, STATS_SCAN_LIMIT);
        assert!(SCANNED.with(|n| n.get()) <= STATS_SCAN_LIMIT);
    }

    /// Blank cells of a sparse range are not charged to the scan budget: a value at the end of two
    /// million-row columns still counts, so duplicates are found.
    #[test]
    fn sparse_ranges_keep_their_values_under_the_scan_budget() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[7]]})).unwrap();
        s.execute("range.setValues", json!({"range": "B1048576", "values": [[7]]})).unwrap();
        let d = s.doc().unwrap();
        let sh = d.wb.sheet(0).unwrap();
        let ranges = [RangeRef::parse("A1:A1048576").unwrap(), RangeRef::parse("B1:B1048576").unwrap()];
        SCANNED.with(|n| n.set(0));
        let (st, _) = RuleStats::build(sh, &ranges, StatsNeed::Counts, STATS_SCAN_LIMIT);
        assert_eq!(st.counts.get("7"), Some(&2));
        assert_eq!(SCANNED.with(|n| n.get()), 2);
    }

    /// Spills left and right of a narrow range, between its corners, are skipped a row at a time:
    /// they do not use the budget that the in-range spills need.
    #[test]
    fn spills_outside_a_narrow_range_do_not_use_its_scan_budget() {
        let mut s = crate::Session::new();
        s.new_workbook();
        let d = s.doc().unwrap();
        let mut sh = d.wb.sheet(0).unwrap().clone();
        for row in 0..1000 {
            for col in [0u32, 5, 6, 7] {
                sh.spill.insert(CellRef::new(row, col), Value::Number(1.0));
            }
        }
        let r = RangeRef::parse("F1:G1000").unwrap();
        let steps: Vec<_> = spill_steps(&sh, r).collect();
        assert_eq!(steps.iter().flatten().count(), 2000);
        // column A is skipped one step a row, never one per cell
        assert!(steps.len() <= 2000 + 1000, "{}", steps.len());
        let narrow = RangeRef::parse("C1:C1000").unwrap();
        assert_eq!(spill_steps(&sh, narrow).flatten().count(), 0);
    }

    /// A colour scale with an unsupported number of stops paints nothing and evaluates none of them.
    #[test]
    fn colour_scales_with_other_stop_counts_paint_nothing() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1], [5]]})).unwrap();
        let mut wb: Workbook = (*s.doc().unwrap().wb).clone();
        let stop = (CfValueKind::Formula("=1".into()), Color::default());
        let rule = |n: usize| CondFormat {
            ranges: vec![RangeRef::parse("A1:A2").unwrap()],
            rule: CfRule::ColorScale { stops: vec![stop.clone(); n] },
            priority: 1,
            stop_if_true: false,
        };
        wb.sheet_mut(0).unwrap().cond_formats.push(rule(4));
        let mut cache = CfCache::new();
        assert_eq!(cache.look(&wb, 0, CellRef::parse("A1").unwrap()).and_then(|l| l.scale_fill), None);
        wb.sheet_mut(0).unwrap().cond_formats = vec![rule(2)];
        let mut cache = CfCache::new();
        assert!(cache.look(&wb, 0, CellRef::parse("A1").unwrap()).and_then(|l| l.scale_fill).is_some());
    }

    #[test]
    fn cell_value_operator_names() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[3], [7]]})).unwrap();
        let rule = |op: &str| json!({"range": "A1:A2", "rule": {"type": "cellIs", "operator": op, "value": "5", "preset": "lightRedFill"}});
        s.execute("home.conditionalFormat", rule("lessThan")).unwrap();
        assert!(s.execute("home.conditionalFormat", rule("smaller")).is_err());
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style).is_some();
        assert_eq!([look("A1"), look("A2")], [true, false]);
    }

    #[test]
    fn rules_compare_with_cells_on_other_sheets() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "E1", "values": [["Suspended"], ["OK"], ["Terminated"], [7], [3]]})).unwrap();
        let rule = |r: serde_json::Value| json!({"range": "E1:E5", "rule": r});
        s.execute(
            "home.conditionalFormat",
            rule(json!({"type": "cellIs", "operator": "equal", "value": "='Vehicle Range'!$A$8", "preset": "redText"})),
        )
        .unwrap();
        s.execute(
            "home.conditionalFormat",
            rule(json!({"type": "cellIs", "operator": "equal", "value": "='Vehicle Range'!$A$9", "preset": "redText"})),
        )
        .unwrap();
        s.execute(
            "home.conditionalFormat",
            rule(json!({"type": "expression", "formula": "=AND(ISNUMBER(E1),E1>'Vehicle Range'!$B$1)", "preset": "yellowFill"})),
        )
        .unwrap();
        s.execute("home.insertSheet", json!({"name": "Vehicle Range"})).unwrap();
        s.execute("range.setValues", json!({"range": "'Vehicle Range'!A8", "values": [["Suspended"], ["Terminated"]]})).unwrap();
        s.execute("range.setValues", json!({"range": "'Vehicle Range'!B1", "values": [[5]]})).unwrap();
        let d = s.doc().unwrap();
        let report = d.wb.sheets.iter().position(|sh| sh.name != "Vehicle Range").unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, report, CellRef::parse(a).unwrap()).and_then(|l| l.style).unwrap_or_default();
        let looks = ["E1", "E2", "E3", "E4", "E5"].map(&mut look);
        // The cells equal to 'Vehicle Range'!A8 or A9 get the red text.
        let red_text = looks[0].font.color;
        assert_ne!(red_text, Color::Auto);
        let red = |st: &Style| st.font.color == red_text;
        let filled = |st: &Style| st.fill.pattern == PatternType::Solid;
        assert_eq!(looks.each_ref().map(red), [true, false, true, false, false]);
        assert_eq!(looks.each_ref().map(filled), [false, false, false, true, false]);
    }
    #[test]
    fn text_rules_skip_error_values() {
        let mut s = crate::Session::new();
        s.new_workbook();
        let values = json!([["=NA()", "=NA()", "=1/0", "=NA()"], ["abc", "#x", "x!", "abc"], ["xyz", "y", "y", "xyz"]]);
        s.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
        let rule = |range: &str, kind: &str, text: &str| json!({"range": range, "rule": {"type": kind, "text": text, "preset": "lightRedFill"}});
        s.execute("home.conditionalFormat", rule("A1:A3", "containsText", "a")).unwrap();
        s.execute("home.conditionalFormat", rule("B1:B3", "beginsWith", "#")).unwrap();
        s.execute("home.conditionalFormat", rule("C1:C3", "endsWith", "!")).unwrap();
        s.execute("home.conditionalFormat", rule("D1:D3", "notContainsText", "a")).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style).is_some();
        // Contains "a": abc, never #N/A although its text holds an "a".
        assert_eq!([look("A1"), look("A2"), look("A3")], [false, true, false]);
        // Begins with "#": #x, never #N/A.
        assert_eq!([look("B1"), look("B2"), look("B3")], [false, true, false]);
        // Ends with "!": x!, never #DIV/0!.
        assert_eq!([look("C1"), look("C2"), look("C3")], [false, true, false]);
        // Does not contain "a": #N/A and xyz, not abc.
        assert_eq!([look("D1"), look("D2"), look("D3")], [true, false, true]);
    }

    #[test]
    fn spatial_index_handles_interval_tree_single_cells_and_numfmt() {
        let mut wb = Workbook::new();
        let mk_fill = |rgb: u32| Style {
            fill: gridcraft_model::Fill { pattern: PatternType::Solid, fg: Color::Rgb(rgb), bg: Color::Auto },
            ..Default::default()
        };
        let mut num_style = mk_fill(0x112233);
        num_style.num_fmt = gridcraft_model::NumFmt("0.00%".into());
        let sh = wb.sheet_mut(0).unwrap();
        for r in 0..40u32 {
            sh.set_value(CellRef::new(r, 0), Value::Number(r as f64));
            sh.set_value(CellRef::new(r, 1), Value::Number(r as f64));
        }
        // 20 disjoint multi-cell row bands (forces non-leaf interval tree nodes) + 1 full-column rule
        // + 1 single-cell rule overlapping a multi-cell band + 1 numFmt rule.
        for i in 0..20u32 {
            sh.cond_formats.push(CondFormat {
                ranges: vec![RangeRef::new(CellRef::new(i * 2, 0), CellRef::new(i * 2 + 1, 1))],
                rule: CfRule::NoBlanks { style: Box::new(mk_fill(0xAA0000 + i)) },
                priority: 10 + i,
                stop_if_true: false,
            });
        }
        sh.cond_formats.push(CondFormat {
            ranges: vec![RangeRef::cols(0, 0)],
            rule: CfRule::NoBlanks { style: Box::new(mk_fill(0x00FF00)) },
            priority: 1,
            stop_if_true: false,
        });
        sh.cond_formats.push(CondFormat {
            ranges: vec![RangeRef::cell(CellRef::new(10, 1)), RangeRef::new(CellRef::new(10, 1), CellRef::new(11, 2))],
            rule: CfRule::NoBlanks { style: Box::new(num_style) },
            priority: 2,
            stop_if_true: false,
        });

        let mut cache = CfCache::new();
        // Column A gets priority-1 full-column fill (0x00FF00).
        let a10 = cache.look(&wb, 0, CellRef::new(10, 0)).unwrap().style.unwrap();
        assert_eq!(a10.fill.fg, Color::Rgb(0x00FF00));
        // B11 (row 10, col 1) gets priority-2 numFmt rule (0x112233) over row-band rule (priority 15).
        let b11 = cache.look(&wb, 0, CellRef::new(10, 1)).unwrap().style.unwrap();
        assert_eq!(b11.fill.fg, Color::Rgb(0x112233));
        assert_eq!(cache.effective_num_fmt(&wb, 0, wb.sheet(0).unwrap(), CellRef::new(10, 1)).as_ref(), "0.00%");
        assert_eq!(cache.effective_num_fmt(&wb, 0, wb.sheet(0).unwrap(), CellRef::new(10, 0)).as_ref(), "General");
        // Cell outside all ranges returns None and may_apply == false.
        assert!(!cache.may_apply(&wb, 0, CellRef::new(10, 5)));
        assert!(cache.look(&wb, 0, CellRef::new(10, 5)).is_none());
    }

    #[test]
    fn duplicate_stats_rules_share_group_cache() {
        let mut wb = Workbook::new();
        let red_fill =
            Style { fill: gridcraft_model::Fill { pattern: PatternType::Solid, fg: Color::Rgb(0xFF0000), bg: Color::Auto }, ..Default::default() };
        let sh = wb.sheet_mut(0).unwrap();
        for r in 0..100_000u32 {
            sh.set_value(CellRef::new(r, 0), Value::text(format!("k{}", r % 1000)));
        }
        // 200 duplicateValues rules sharing identical sqref A:A must share ONE scan.
        for i in 0..200u32 {
            sh.cond_formats.push(CondFormat {
                ranges: vec![RangeRef::cols(0, 0)],
                rule: CfRule::Duplicate { style: Box::new(red_fill.clone()) },
                priority: i + 1,
                stop_if_true: false,
            });
        }
        reset_scanned();
        let mut cache = CfCache::new();
        let first = cache.look(&wb, 0, CellRef::new(0, 0)).and_then(|l| l.style).expect("A1 duplicate fill");
        let last = cache.look(&wb, 0, CellRef::new(99_999, 0)).and_then(|l| l.style).expect("A100000 duplicate fill");
        assert_eq!(first.fill.fg, Color::Rgb(0xFF0000));
        assert_eq!(last.fill.fg, Color::Rgb(0xFF0000));
        assert_eq!(scanned_count(), 100_000, "200 identical-sqref duplicate rules must scan A:A exactly once");
        let lay = crate::cmd::print::layout_sheet(&wb, 0, &crate::cmd::print::PrintOptions::default(), None).unwrap();
        let mut first_page = lay.clone();
        first_page.pages.truncate(1);
        let pdf = crate::cmd::print::render_pdf(&wb, &[first_page], "", "").unwrap();
        assert_eq!(pdf.pages, 1);
    }

    #[test]
    fn distinct_column_duplicate_rules_do_not_starve_each_other() {
        let mut wb = Workbook::new();
        let red_fill =
            Style { fill: gridcraft_model::Fill { pattern: PatternType::Solid, fg: Color::Rgb(0xFF0000), bg: Color::Auto }, ..Default::default() };
        const ROWS: u32 = 700_000;
        let sh = wb.sheet_mut(0).unwrap();
        let fill_val = Value::text("fill");
        let dup_val = Value::text("dup");
        let unique_val = Value::text("only_once");
        for col in 0..3u32 {
            sh.set_value(CellRef::new(0, col), dup_val.clone());
            for r in 1..(ROWS - 2) {
                sh.set_value(CellRef::new(r, col), fill_val.clone());
            }
            sh.set_value(CellRef::new(ROWS - 2, col), unique_val.clone());
            sh.set_value(CellRef::new(ROWS - 1, col), dup_val.clone());
            sh.cond_formats.push(CondFormat {
                ranges: vec![RangeRef::cols(col, col)],
                rule: CfRule::Duplicate { style: Box::new(red_fill.clone()) },
                priority: col + 1,
                stop_if_true: false,
            });
        }
        // 3 distinct columns x 700,000 cells = 2,100,000 cells (> STATS_SCAN_LIMIT if pooled):
        // each group gets its own per-group scan limit so later columns are never starved.
        reset_scanned();
        let mut cache = CfCache::new();
        for col in 0..3u32 {
            let bottom = cache
                .look(&wb, 0, CellRef::new(ROWS - 1, col))
                .and_then(|l| l.style)
                .unwrap_or_else(|| panic!("col {col} bottom duplicate must be highlighted"));
            assert_eq!(bottom.fill.fg, Color::Rgb(0xFF0000));
            assert!(cache.look(&wb, 0, CellRef::new(ROWS - 2, col)).is_none(), "col {col} unique cell must not match duplicate rule");
        }
        assert_eq!(scanned_count(), 3 * ROWS as usize);
    }

    #[test]
    fn distinct_overlapping_stats_rules_respect_aggregate_scan_cap() {
        let mut wb = Workbook::new();
        let red_fill = Style { fill: gridcraft_model::Fill::solid(Color::Rgb(0xFF0000)), ..Style::default() };
        {
            let sh = wb.sheet_mut(0).unwrap();
            for r in 0..200u32 {
                sh.set_value(CellRef::new(r, 0), Value::Number(f64::from(r % 50)));
            }
            for i in 0..20u32 {
                sh.cond_formats.push(CondFormat {
                    ranges: vec![RangeRef::new(CellRef::new(i, 0), CellRef::new(1_048_575, 0))],
                    rule: CfRule::Duplicate { style: Box::new(red_fill.clone()) },
                    priority: i + 1,
                    stop_if_true: false,
                });
            }
        }
        // Simulate a cache already near MAX_TOTAL_STATS_SCAN: further distinct overlapping stats
        // groups must stop scanning once the aggregate cap is reached rather than scaling unboundedly.
        reset_scanned();
        let mut cache = CfCache::new();
        cache.ensure_index(wb.sheet(0).unwrap(), 0);
        cache.stats_cells_scanned = MAX_TOTAL_STATS_SCAN - 500;
        let _ = cache.look(&wb, 0, CellRef::new(25, 0));
        assert_eq!(cache.stats_cells_scanned, MAX_TOTAL_STATS_SCAN);
        assert!(scanned_count() <= 500, "aggregate cap must bound distinct overlapping groups (scanned {})", scanned_count());
    }

    #[test]
    fn exhausted_stats_cap_skips_stats_rules_without_blocking_lower_priority_rules() {
        let mut wb = Workbook::new();
        let red_fill = Style { fill: gridcraft_model::Fill::solid(Color::Rgb(0xFF0000)), ..Style::default() };
        let green_fill = Style { fill: gridcraft_model::Fill::solid(Color::Rgb(0x00FF00)), ..Style::default() };
        {
            let sh = wb.sheet_mut(0).unwrap();
            // True mean is 15.0: A1 (10.0) is below average, but 10.0 > 0.0 would falsely match if
            // exhausted stats fell back to RuleStats::default() (avg = 0.0).
            sh.set_value(CellRef::new(0, 0), Value::Number(10.0));
            sh.set_value(CellRef::new(1, 0), Value::Number(20.0));
            let range = vec![RangeRef::new(CellRef::new(0, 0), CellRef::new(1, 0))];
            sh.cond_formats.push(CondFormat {
                ranges: range.clone(),
                rule: CfRule::AboveAverage { below: false, equal: false, std_dev: 0, style: Box::new(red_fill) },
                priority: 1,
                stop_if_true: true,
            });
            sh.cond_formats.push(CondFormat {
                ranges: range,
                rule: CfRule::CellIs { op: CfOperator::Greater, a: "0".into(), b: None, style: Box::new(green_fill) },
                priority: 2,
                stop_if_true: false,
            });
        }
        // Exhausted cache: priority-1 AboveAverage must be skipped (neither matching against avg=0.0
        // nor triggering stop_if_true), so priority-2 CellIs still applies on both lookups.
        reset_scanned();
        let mut cache = CfCache::new();
        cache.ensure_index(wb.sheet(0).unwrap(), 0);
        cache.stats_cells_scanned = MAX_TOTAL_STATS_SCAN;
        let a1 = cache.look(&wb, 0, CellRef::new(0, 0)).and_then(|l| l.style).expect("priority-2 CellIs must apply to A1");
        assert_eq!(a1.fill.fg, Color::Rgb(0x00FF00));
        let a2 = cache.look(&wb, 0, CellRef::new(1, 0)).and_then(|l| l.style).expect("priority-2 CellIs must apply to A2");
        assert_eq!(a2.fill.fg, Color::Rgb(0x00FF00));
        assert_eq!(scanned_count(), 0);

        // Sanity check: a non-exhausted cache evaluates true mean (15.0): A1 gets green, A2 gets red.
        let mut fresh = CfCache::new();
        assert_eq!(fresh.look(&wb, 0, CellRef::new(0, 0)).and_then(|l| l.style).unwrap().fill.fg, Color::Rgb(0x00FF00));
        assert_eq!(fresh.look(&wb, 0, CellRef::new(1, 0)).and_then(|l| l.style).unwrap().fill.fg, Color::Rgb(0xFF0000));
    }

    #[test]
    fn duplicate_and_unique_rules_match_greek_final_sigma() {
        let mut wb = Workbook::new();
        let red_fill = Style { fill: gridcraft_model::Fill::solid(Color::Rgb(0xFF0000)), ..Style::default() };
        let blue_fill = Style { fill: gridcraft_model::Fill::solid(Color::Rgb(0x0000FF)), ..Style::default() };
        {
            let sh = wb.sheet_mut(0).unwrap();
            sh.set_value(CellRef::new(0, 0), Value::text("ΑΘΗΝΑΣ"));
            sh.set_value(CellRef::new(1, 0), Value::text("ΑΘΗΝΑΣ"));
            sh.set_value(CellRef::new(2, 0), Value::text("ΕΛΛΑΣ"));
            sh.cond_formats.push(CondFormat {
                ranges: vec![RangeRef::new(CellRef::new(0, 0), CellRef::new(2, 0))],
                rule: CfRule::Duplicate { style: Box::new(red_fill) },
                priority: 1,
                stop_if_true: false,
            });
            sh.cond_formats.push(CondFormat {
                ranges: vec![RangeRef::new(CellRef::new(0, 0), CellRef::new(2, 0))],
                rule: CfRule::Unique { style: Box::new(blue_fill) },
                priority: 2,
                stop_if_true: false,
            });
        }
        let mut cache = CfCache::new();
        assert_eq!(cache.look(&wb, 0, CellRef::new(0, 0)).and_then(|l| l.style).unwrap().fill.fg, Color::Rgb(0xFF0000));
        assert_eq!(cache.look(&wb, 0, CellRef::new(1, 0)).and_then(|l| l.style).unwrap().fill.fg, Color::Rgb(0xFF0000));
        assert_eq!(cache.look(&wb, 0, CellRef::new(2, 0)).and_then(|l| l.style).unwrap().fill.fg, Color::Rgb(0x0000FF));
    }

    #[test]
    fn fill_only_cf_rules_skip_index_and_stats_during_effective_num_fmt_and_fit_imported_rows() {
        let mut wb = Workbook::new();
        let mut wrap_style = Style::default();
        wrap_style.align.wrap = true;
        let wrap_id = wb.styles.intern(wrap_style);
        let red_fill = Style { fill: gridcraft_model::Fill::solid(Color::Rgb(0xFF0000)), ..Style::default() };
        {
            let sh = wb.sheet_mut(0).unwrap();
            for r in 0..200u32 {
                let c = CellRef::new(r, 0);
                sh.set_value(c, Value::text("line one\nline two\nline three"));
                sh.set_style(c, wrap_id);
            }
            for i in 0..500u32 {
                sh.cond_formats.push(CondFormat {
                    ranges: vec![RangeRef::cols(0, 0)],
                    rule: CfRule::Duplicate { style: Box::new(red_fill.clone()) },
                    priority: i * 2 + 1,
                    stop_if_true: false,
                });
                sh.cond_formats.push(CondFormat {
                    ranges: vec![RangeRef::cols(0, 0)],
                    rule: CfRule::Top10 { bottom: false, percent: false, rank: 10, style: Box::new(red_fill.clone()) },
                    priority: i * 2 + 2,
                    stop_if_true: false,
                });
            }
        }
        reset_scanned();
        let mut cache = CfCache::new();
        let sh = wb.sheet(0).unwrap();
        for r in 0..200u32 {
            assert_eq!(cache.effective_num_fmt(&wb, 0, sh, CellRef::new(r, 0)).as_ref(), "General");
        }
        assert!(cache.index.is_none(), "fill-only CF rules must not build SheetCfIndex during effective_num_fmt");
        assert_eq!(scanned_count(), 0, "fill-only CF rules must perform 0 stats scans during effective_num_fmt");

        let default_h = wb.sheet(0).unwrap().default_row_height;
        reset_scanned();
        crate::io::fit_imported_rows(&mut wb);
        assert_eq!(scanned_count(), 0, "fit_imported_rows with fill-only CF rules must perform 0 stats scans");
        assert!(wb.sheet(0).unwrap().rows.get(&0).and_then(|i| i.size).is_some_and(|h| h > default_h), "wrapped multi-line row must still auto-fit");

        // When a stats rule DOES set numFmt, fit_imported_rows must share one CfCache across all rows
        // (scanning the sheet's range once rather than once per row).
        let mut wb_nf = Workbook::new();
        let mut wrap_style_nf = Style::default();
        wrap_style_nf.align.wrap = true;
        let wrap_id_nf = wb_nf.styles.intern(wrap_style_nf);
        {
            let sh = wb_nf.sheet_mut(0).unwrap();
            for r in 0..100u32 {
                let c = CellRef::new(r, 0);
                sh.set_value(c, Value::Number(f64::from(r + 1)));
                sh.set_style(c, wrap_id_nf);
            }
            sh.cond_formats.push(CondFormat {
                ranges: vec![RangeRef::new(CellRef::new(0, 0), CellRef::new(99, 0))],
                rule: CfRule::Top10 {
                    bottom: false,
                    percent: false,
                    rank: 10,
                    style: Box::new(Style { num_fmt: gridcraft_model::NumFmt::new("0.0000000000000000"), ..Style::default() }),
                },
                priority: 1,
                stop_if_true: false,
            });
        }
        reset_scanned();
        crate::io::fit_imported_rows(&mut wb_nf);
        assert_eq!(scanned_count(), 100, "fit_imported_rows must share one CfCache per sheet (1 scan across all rows)");
    }

    #[test]
    fn look_bounds_overlapping_full_sheet_rules_per_cell() {
        let full_sheet = RangeRef::cols(0, gridcraft_core::MAX_COLS - 1);
        let green_fill =
            Style { fill: gridcraft_model::Fill::solid(Color::Rgb(0x00FF00)), num_fmt: gridcraft_model::NumFmt::new("0.00%"), ..Style::default() };
        let late_marker = Style {
            fill: gridcraft_model::Fill::solid(Color::Rgb(0xFF0000)),
            font: gridcraft_model::Font { bold: true, ..Default::default() },
            num_fmt: gridcraft_model::NumFmt::new("0.000%"),
            ..Style::default()
        };
        let non_matching_style = Style { fill: gridcraft_model::Fill::solid(Color::Rgb(0x0000FF)), ..Style::default() };

        // Case 1: 500 overlapping full-sheet A:XFD rules sharing one RangeGroup.
        // Priorities 1..=63 do not match; priority 64 sets green fill + 0.00%;
        // priorities 65..=500 match unconditionally with bold + red fill.
        let mut wb = Workbook::new();
        {
            let sh = wb.sheet_mut(0).unwrap();
            sh.set_value(CellRef::new(0, 0), Value::Number(0.5));
            for p in 1..=500u32 {
                let rule = if p < MAX_CF_RULES_PER_CELL as u32 {
                    CfRule::CellIs { op: CfOperator::Greater, a: "100".into(), b: None, style: Box::new(non_matching_style.clone()) }
                } else if p == MAX_CF_RULES_PER_CELL as u32 {
                    CfRule::NoBlanks { style: Box::new(green_fill.clone()) }
                } else {
                    CfRule::NoBlanks { style: Box::new(late_marker.clone()) }
                };
                sh.cond_formats.push(CondFormat { ranges: vec![full_sheet], rule, priority: p, stop_if_true: false });
            }
        }
        let mut cache = CfCache::new();
        reset_rules_evaluated();
        let st = cache.look(&wb, 0, CellRef::new(0, 0)).and_then(|l| l.style).expect("rule 64 must match");
        assert_eq!(rules_evaluated_count(), MAX_CF_RULES_PER_CELL);
        assert_eq!(st.fill.fg, Color::Rgb(0x00FF00));
        assert!(!st.font.bold, "rules past MAX_CF_RULES_PER_CELL must not be evaluated");
        assert_eq!(cache.effective_num_fmt(&wb, 0, wb.sheet(0).unwrap(), CellRef::new(0, 0)).as_ref(), "0.00%");

        // Case 2: 500 distinct overlapping shifted full-width ranges (multi-group path in CfCache::look).
        let mut wb_multi = Workbook::new();
        let target = CellRef::new(600, 0);
        {
            let sh = wb_multi.sheet_mut(0).unwrap();
            sh.set_value(target, Value::Number(0.5));
            for p in 1..=500u32 {
                let r = RangeRef::new(CellRef::new(p - 1, 0), CellRef::new(gridcraft_core::MAX_ROWS - 1, gridcraft_core::MAX_COLS - 1));
                let rule = if p < MAX_CF_RULES_PER_CELL as u32 {
                    CfRule::CellIs { op: CfOperator::Greater, a: "100".into(), b: None, style: Box::new(non_matching_style.clone()) }
                } else if p == MAX_CF_RULES_PER_CELL as u32 {
                    CfRule::NoBlanks { style: Box::new(green_fill.clone()) }
                } else {
                    CfRule::NoBlanks { style: Box::new(late_marker.clone()) }
                };
                sh.cond_formats.push(CondFormat { ranges: vec![r], rule, priority: p, stop_if_true: false });
            }
        }
        let mut cache_multi = CfCache::new();
        reset_rules_evaluated();
        let st_multi = cache_multi.look(&wb_multi, 0, target).and_then(|l| l.style).expect("rule 64 must match across groups");
        assert_eq!(rules_evaluated_count(), MAX_CF_RULES_PER_CELL);
        assert_eq!(st_multi.fill.fg, Color::Rgb(0x00FF00));
        assert!(!st_multi.font.bold, "rules past MAX_CF_RULES_PER_CELL across groups must not be evaluated");
    }
}
