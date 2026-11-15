use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::{Area, BinOp, Ev, Expr, Reference, SheetSel, UnOp, Value, Workbook, may_be_cut};
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};

/// SUMIF and AVERAGEIF with a sum range: Excel sizes the sum range like the criteria range.
pub(super) fn resizes_sum_range(name: &str, args: usize) -> bool {
    args == 3 && matches!(name, "SUMIF" | "AVERAGEIF")
}

/// Height and width of a single-area reference or an array argument.
pub(super) fn ev_shape(ev: &Ev) -> Option<(u32, u32)> {
    match ev {
        Ev::R(areas) => match areas.as_slice() {
            [a] => Some((a.range.height(), a.range.width())),
            _ => None,
        },
        Ev::V(Value::Array(a)) => Some((u32::try_from(a.rows).unwrap_or(u32::MAX), u32::try_from(a.cols).unwrap_or(u32::MAX))),
        Ev::V(_) => Some((1, 1)),
        Ev::L(_) => None,
    }
}

/// The range SUMIF/AVERAGEIF add up: from the sum range's top-left cell, as many rows and columns
/// as the criteria range (`height` × `width`), cut off at the sheet's edge.
pub fn resized_sum_range(sum: RangeRef, height: u32, width: u32) -> RangeRef {
    let row = sum.start.row.saturating_add(height.saturating_sub(1)).min(MAX_ROWS - 1);
    let col = sum.start.col.saturating_add(width.saturating_sub(1)).min(MAX_COLS - 1);
    RangeRef::new(sum.start, CellRef::new(row, col))
}

/// Collects the static references of an expression with their sheets resolved; `dynamic` is
/// set when the formula uses INDIRECT/OFFSET/tables (or a name that can't be resolved now) whose
/// targets can change between recalcs.
///
/// Defined names are resolved here, not treated as dynamic: a name holding a range contributes
/// that range, a name holding a LAMBDA contributes the references inside its body (its
/// parameters, and LET variables, are bound names, not lookups), names referring to names follow
/// the chain to a bounded depth. A column of cells calling one LAMBDA name is then an ordinary
/// static dependency, re-evaluated when its inputs change and only then. Changing a name's
/// definition is a structural edit (full rebuild), and the engine re-resolves the readers of the
/// hidden names its commands rewrite without one, in manual mode too (their values then wait for
/// the next calculation, their precedents don't), so the graph follows the definitions. Names with
/// relative references (evaluated relative to the using cell), unknown names, unparsable names,
/// sheet-scoped names used from another sheet, cycles between names and chains deeper than the
/// bound stay dynamic. A LAMBDA name calling itself is not a cycle here: the call reads what its
/// body reads. The evaluator reads a definition with the caller's LET variables and LAMBDA
/// parameters in scope, so a name whose definition leaves free a name the caller binds is dynamic.
///
/// The cases that stay dynamic, in one list (docs/target-app-parity.md points here):
/// - INDIRECT, OFFSET and the volatile functions (`call_reads`), in a formula or a name;
/// - tables (structured references, and a name that is not defined, as a table name is);
/// - names with relative references, and unparsable names;
/// - a sheet prefix naming no sheet, and a sheet-scoped name used from another sheet;
/// - cycles between names, and chains deeper than [`NAME_DEPTH`];
/// - a name given to SUMIF or AVERAGEIF as a range (the sum range is sized from the criteria
///   range, known statically only for plain references);
/// - a spill reference (`A1#`) in an operand of the range operator, directly or through a name;
/// - a LET variable, LAMBDA parameter or recursive self-call in an operand of the range operator;
/// - a name whose definition leaves free a name the caller binds;
/// - a formula mentioning a name whose own array covers cells it reads, while it does (decided
///   when the array is laid out, not here: `Calc::mark_spill_reader` in `recalc.rs`);
/// - a database function, in a formula or in a name's definition (its computed criteria read
///   cells shifted from the criteria cells' references, see `computed_criterion`);
/// - GETPIVOTDATA given anything but a plain reference into a report, and in a name's definition
///   (`call_reads`);
/// - a formula that holds a name, a LET variable, a LAMBDA parameter or a call to a function not
///   built in, and whose areas (its references, the boxes of its range operands, the areas of the
///   names it reads) include one the evaluator may cut to the sheet's used range ([`may_be_cut`]:
///   whole columns or rows, or a block over 16,384 cells running to the sheet's edge, or over
///   1,048,576 cells): what the cut array holds (its shape, its blanks) depends on cells outside the
///   area, which the graph has no edge for. A formula holding such an area and no name is static
///   (`=ROWS(A:A+0)` misses the used range growing). This follows `read_range` and `trim_lines`;
///   TRIMRANGE cuts every reference by its own rule, but its result depends only on the cells
///   inside the area;
/// - a formula that holds a name, a LET variable, a LAMBDA parameter or a call to a function not
///   built in, and calls SUBTOTAL or AGGREGATE, itself or in a definition it reads: these two
///   charge the sizes left after cutting each reference argument to the used range against a budget
///   of `MAX_CELLS` cells per call, so past it their result depends on cells no reference names.
///   Arguments can repeat (a name, a LET variable, a recursive LAMBDA's union), so the walk does not
///   try to bound them. With plain references only, such a call is static, as before;
/// - past the graph build's bounds: [`MAX_PRECEDENT_NODES`], [`MAX_NAME_AREAS`],
///   [`MAX_BUILD_NAME_AREAS`], [`MAX_BUILD_FREE_NAMES`], [`MAX_BUILD_FOLDED_FREE_NAMES`] and
///   [`MAX_BUILD_NAME_TEXT`].
///
/// Resolves names through a fresh [`NameCache`]; a graph build resolving many formulas shares one
/// through [`precedents_with`].
pub fn precedents(wb: &Workbook, sheet: usize, e: &Expr) -> (Vec<Area>, bool) {
    precedents_with(wb, sheet, e, &mut NameCache::default())
}

/// [`precedents`] with the defined names resolved through `names`, shared across the formulas of
/// one graph build.
pub fn precedents_with(wb: &Workbook, sheet: usize, e: &Expr, names: &mut NameCache) -> (Vec<Area>, bool) {
    // Room for the formula's list and a few nested ones (a range's operands, a name's
    // definition), without growing.
    let mut taken = Vec::with_capacity(4);
    taken.push(Taken::default());
    let mut walk = Walk {
        wb,
        names,
        nodes: 0,
        from_names: 0,
        overflowed: false,
        taken,
        database: false,
        frames: Vec::new(),
        spill: false,
        names_met: false,
        budgeted: false,
    };
    let (mut out, mut dynamic) = walk.collect(sheet, e, 0);
    // A database function's computed criteria read cells no reference names (see
    // `computed_criterion`): a formula calling one is dynamic.
    dynamic |= walk.database;
    // SUBTOTAL and AGGREGATE cut every reference argument to the used range and charge what is left
    // against a budget of `MAX_CELLS` cells per call: past it, their result depends on cells no
    // reference names. A name, LET variable or LAMBDA parameter can hand a call the same areas any
    // number of times, so with one the formula is dynamic, as it was before names were resolved
    // statically; with plain references only it stays static, as it always was.
    dynamic |= walk.names_met && walk.budgeted;
    // Each area once: a formula mentioning a name or a range k times depends on it once.
    remove_repeats(&mut out);
    // An area the evaluator may cut, in a formula that holds a name: the formulas that were dynamic
    // before names were resolved statically. The formula's list holds its references, the boxes of
    // its range operands (each covering the references and names' areas inside it, and cut where one
    // of them is: a box only grows) and the areas of the names it reads, those of the names their
    // definitions read included. A name whose areas the walk drops (past a bound) is dynamic, and so
    // is the formula reading it. This is the one place the rule is applied.
    if walk.names_met && !dynamic {
        dynamic = out.iter().any(|a| may_be_cut(&a.range));
    }
    (out, dynamic)
}

/// Lists of up to this many areas, or boxes, are searched by comparison rather than through a set
/// or an index: most formulas hold a handful of areas, and a set costs an allocation and a hash per
/// area.
pub(crate) const SCANNED_UP_TO: usize = 16;

/// Removes the repeats from `list`, keeping the first of each area in order.
fn remove_repeats(list: &mut Vec<Area>) {
    if list.len() <= SCANNED_UP_TO {
        // Each area is compared with those kept before it, which move to the front.
        let mut kept = 0;
        for i in 0..list.len() {
            let Some(&area) = list.get(i) else { break };
            if !list.get(..kept).is_some_and(|before| before.contains(&area)) {
                if let Some(slot) = list.get_mut(kept) {
                    *slot = area;
                }
                kept += 1;
            }
        }
        list.truncate(kept);
        return;
    }
    let mut distinct: HashSet<Area, std::hash::BuildHasherDefault<crate::recalc::FxHasher>> =
        HashSet::with_capacity_and_hasher(list.len(), Default::default());
    list.retain(|a| distinct.insert(*a));
}

/// Names referring to names are followed this many levels before the formula is called dynamic.
const NAME_DEPTH: usize = 8;

/// Most expression nodes one precedent walk visits, before giving up and marking the formula
/// dynamic. Every node of the formula counts one, and so does every node of the name definitions the
/// walk parses (a name the cache already holds costs nothing), except an inner link of a chain of one
/// shape, which counts with the chain's outermost node: a parenthesis directly inside a parenthesis,
/// a `%` directly on a `%`, an operator other than a range as the left operand of another, a range as
/// the left operand of a range, and an invoked result invoked again. The walk's explicit stack holds
/// at most the children of the nodes visited, plus two steps for each, so the bound limits its
/// memory too. The same flag (`link` on [`Step::Visit`]) decides that a chain of ranges shares one
/// box list, so a change to this counting rule must not change that.
const MAX_PRECEDENT_NODES: usize = 1_000_000;

/// Most areas one defined name contributes, and most a formula takes from the names it mentions,
/// before the name or the formula is called dynamic. A name's areas are stored in the graph for
/// every formula reading it, so a name of many single cells, or one summing many such names, would
/// otherwise make each short formula reading it as large as all of them; dynamic is always a correct
/// answer. A formula's own references don't count. In a definition, a span of sheets
/// (`First:Last!A1`) counts one area per sheet at each mention, and a range operand the boxes it
/// closes into, one per sheet at each mention, against the list they join (the definition's, or an
/// enclosing range operand's): the list as it stands, and where that passes this bound, the list
/// without its repeats (see `fit`).
const MAX_NAME_AREAS: usize = 1024;

/// Most areas the formulas of one graph build may take from names, all together, each area counted
/// once whatever its size. A name of [`MAX_NAME_AREAS`] areas read from a whole column of cells
/// would store them once per cell (a million readers: a thousand million areas); past this budget
/// (a million areas of 24 bytes, about 25 MB) the further readers are dynamic. The graph indexes a
/// range of up to 64 columns once per column, as it does for a formula's own references, so the
/// memory of that index is not what this budget bounds. A workbook of tens of thousands of LAMBDA
/// calls over a named range stores a few areas per reader, far below it. The names a build
/// resolves keep their own areas too, as many again at most: a definition that is only another
/// name (`copy_k = wide_name`) costs three bytes of text but copies all the areas of the name it
/// repeats, so past this budget the further names are dynamic, and a workbook of many such names
/// costs no more than this.
const MAX_BUILD_NAME_AREAS: usize = 1 << 20;

/// Most free names (left unbound by a definition) the names one graph build resolves keep, all
/// together; past it the further names are dynamic. A summary keeps each as a number standing for
/// its spelling, which the build holds once ([`NameCache`]'s `spellings`), so a long name kept by
/// many names costs its length once.
const MAX_BUILD_FREE_NAMES: usize = 1 << 20;

/// Most free names one graph build copies from the summary of a name into the definition using it,
/// all together, counted before each copy (a copy skipped because the definition can't keep its
/// free names, see `Frame::past_free_budget`, counts as made); past it the definition that would
/// copy is dynamic. A name leaves free those of the names it uses too, so names sharing their free
/// names copy the shared ones once per use: two hundred names each summing the same three hundred
/// names, used by each of a thousand names, copy 60 million. The other budgets don't bound this:
/// [`MAX_BUILD_FREE_NAMES`] counts the free names a definition keeps, not the copies that made
/// them, and a name using another of free names it already holds keeps no more for each copy.
/// This budget, about 268 million copies, is large enough that the copies of the three-tier shapes
/// measured, up to three hundred leaves, two hundred children and twelve hundred parents (72
/// million copies), fit within it, and small enough that the copying a hostile file can force takes
/// seconds, not minutes. Whether the formulas reading such a shape stay static depends on the other
/// bounds too: the largest of those shapes holds about 5 MiB of definitions, past
/// [`MAX_BUILD_NAME_TEXT`], so some of its readers are dynamic.
const MAX_BUILD_FOLDED_FREE_NAMES: usize = 1 << 28;

#[cfg(test)]
thread_local! {
    /// [`MAX_BUILD_FOLDED_FREE_NAMES`] on this thread, which a test lowers to build at the budget.
    pub(crate) static FOLD_BUDGET: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_BUILD_FOLDED_FREE_NAMES) };
}

#[cfg(not(test))]
fn fold_budget() -> usize {
    MAX_BUILD_FOLDED_FREE_NAMES
}

#[cfg(test)]
fn fold_budget() -> usize {
    FOLD_BUDGET.with(std::cell::Cell::get)
}

/// Most definition text one graph build parses to resolve names, in bytes. A workbook-wide name is
/// resolved for each sheet that uses it (its unqualified references point at that sheet), so a
/// thousand long names used from two hundred sheets would parse 1.6 GB of definitions; past this
/// budget the further names are dynamic. A workbook of LAMBDA names used from a few sheets parses a
/// few kilobytes.
const MAX_BUILD_NAME_TEXT: usize = 4 << 20;

/// A name as the walk resolves it: lower-cased, its scope, and the sheet its unqualified
/// references resolve against.
type NameKey = (String, Option<usize>, usize);

/// The defined names of a workbook by lower-cased name and scope, built once for a graph build or a
/// recalculation pass: [`Workbook::name`] scans every name at every lookup, so a build of many
/// formulas over many names cost formulas × names.
pub struct NameIndex {
    by_name: HashMap<(String, Option<usize>), usize>,
}

impl NameIndex {
    pub fn new(wb: &Workbook) -> Self {
        #[cfg(test)]
        NAME_INDEXES_BUILT.with(|n| n.set(n.get().saturating_add(1)));
        let mut by_name = HashMap::new();
        for (i, n) in wb.names.iter().enumerate() {
            by_name.entry((n.name.to_ascii_lowercase(), n.scope)).or_insert(i);
        }
        NameIndex { by_name }
    }

    /// Where in `wb.names` the name `name` seen from `sheet` is, as [`Workbook::name`] finds it: one
    /// scoped to that sheet first, then a workbook-wide one. Its unindexed twin is
    /// [`Workbook::name_position`], which must find the same name.
    pub fn find(&self, name: &str, sheet: usize) -> Option<usize> {
        let lower = name.to_ascii_lowercase();
        self.by_name.get(&(lower.clone(), Some(sheet))).or_else(|| self.by_name.get(&(lower, None))).copied()
    }
}

/// Name lookups of one graph build or one recalculation pass answered by a scan of the workbook's
/// names ([`Workbook::name_position`]) before [`NameLookup`] builds a [`NameIndex`] at the next one.
/// A speed switch, not a limit: the scan and the index find the same name, so no result depends on
/// it. Measured 2026-11-18 (release build, four-core 2011 iMac), ten one-cell edits of a formula
/// reading one name: with the index built at the first lookup they took 0.4 ms with 50 names,
/// 10.7 ms with 2,000 and 1.6 to 2.8 s with 200,000 (a scan at every lookup: 0.09, 0.18 and 28 ms);
/// with the first 16 lookups scanned, 0.15 ms, 0.33 ms and about 60 ms, while a full recalculation
/// of 20,000 formulas over 50 names stayed within noise.
pub(crate) const NAME_LOOKUPS_SCANNED: usize = 16;

#[cfg(test)]
thread_local! {
    /// [`NameIndex`]es built on this thread, which a test reads; a level's workers add theirs to
    /// the main thread's after the level.
    pub(crate) static NAME_INDEXES_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Finds defined names for one graph build or one recalculation pass: by a scan of the workbook's
/// names for the first [`NAME_LOOKUPS_SCANNED`] lookups, then through a [`NameIndex`] built at the
/// next one. An edit that looks up a few names in a workbook of many doesn't pay for an index of
/// all of them, and a build or pass that looks up many doesn't scan the list at each.
#[derive(Default)]
pub struct NameLookup {
    index: Option<NameIndex>,
    scans: usize,
}

impl NameLookup {
    /// Where in `wb.names` the name `name` seen from `sheet` is (see [`NameIndex::find`]).
    pub fn find(&mut self, wb: &Workbook, name: &str, sheet: usize) -> Option<usize> {
        if let Some(index) = &self.index {
            return index.find(name, sheet);
        }
        if self.scans < NAME_LOOKUPS_SCANNED {
            self.scans += 1;
            return wb.name_position(name, sheet);
        }
        self.index.insert(NameIndex::new(wb)).find(name, sheet)
    }

    /// The index, once built.
    pub fn index(&self) -> Option<&NameIndex> {
        self.index.as_ref()
    }

    /// Before `lookups` lookups made where [`NameLookup::find`] can't be called (by the worker
    /// threads of a level, which read [`NameLookup::index`]): counted as scans while they fit in
    /// what is left of [`NAME_LOOKUPS_SCANNED`], else the index is built now, once, for them to
    /// share.
    pub fn expect(&mut self, wb: &Workbook, lookups: usize) {
        if self.index.is_some() {
            return;
        }
        if lookups <= NAME_LOOKUPS_SCANNED.saturating_sub(self.scans) {
            self.scans += lookups;
        } else {
            self.index = Some(NameIndex::new(wb));
        }
    }
}

/// What each defined name contributes to the formulas using it, resolved once per graph build.
///
/// The workbook's names don't change while a graph is built, so a name read from thousands of
/// cells, or mentioned many times inside another name's definition, is resolved the first time
/// and looked up after that, once per sheet it is used from (unqualified references point at that
/// sheet). The work of a build is then linear in the size of the definitions times the sheets using
/// them, however often they are referenced, and bounded by [`MAX_BUILD_NAME_TEXT`]; followed afresh
/// at every mention, a chain of names each mentioning the previous one k times would cost k^depth
/// per use. A name met again from its own definition (a recursive LAMBDA) adds nothing; met again
/// from deeper down the chain it is a cycle, and the names on it are dynamic. A chain cut by
/// [`NAME_DEPTH`] is dynamic from the cut up to the formula, and the cache keeps that answer for
/// those names even where a shorter chain reaches them later: dynamic is always a correct answer,
/// only a slower one.
#[derive(Default)]
pub struct NameCache {
    /// Keyed by the name, its scope and the sheet its unqualified references resolve against.
    entries: HashMap<NameKey, NameEntry>,
    /// Finds the workbook's names (see [`NameLookup`]).
    lookup: NameLookup,
    /// Areas handed to the formulas of the build from the names they mention, counted against
    /// [`MAX_BUILD_NAME_AREAS`].
    stored: usize,
    /// Characters of definitions parsed in the build, counted against [`MAX_BUILD_NAME_TEXT`].
    parsed: usize,
    /// Areas kept in `entries` for the names resolved in the build, counted against
    /// [`MAX_BUILD_NAME_AREAS`].
    cached: usize,
    /// Free names kept in `entries`, counted against [`MAX_BUILD_FREE_NAMES`].
    free_kept: usize,
    /// The spelling (case-folded) of each free name the build met in a definition, and of each LET
    /// variable and LAMBDA parameter it bound, numbered: the summaries, the definitions being
    /// resolved and the bound names in scope hold the numbers. It grows with the text of the
    /// formulas and definitions the build reads, each spelling once.
    spellings: HashMap<String, usize>,
    /// Free names copied from the summaries of names into the definitions using them, counted
    /// against [`MAX_BUILD_FOLDED_FREE_NAMES`].
    folded: usize,
}

impl NameCache {
    /// The number standing for the spelling `name`, numbered the first time it is met.
    fn spelling(&mut self, name: String) -> usize {
        let next = self.spellings.len();
        *self.spellings.entry(name).or_insert(next)
    }

    /// Free names copied into definitions so far (see [`MAX_BUILD_FOLDED_FREE_NAMES`]).
    #[cfg(test)]
    pub(crate) fn folded(&self) -> usize {
        self.folded
    }

    /// Bytes the free names kept in the build hold: their numbers in the summaries, and each
    /// spelling once (those of bound names included).
    #[cfg(test)]
    pub(crate) fn free_name_bytes(&self) -> usize {
        let spelled: usize = self.spellings.keys().map(String::len).sum();
        spelled.saturating_add(self.free_kept.saturating_mul(std::mem::size_of::<usize>()))
    }

    /// A cache whose budget on areas taken from names counts on from `stored`.
    pub fn with_stored(stored: usize) -> Self {
        NameCache { stored, ..Default::default() }
    }

    /// Areas handed to formulas from names so far, the count [`NameCache::with_stored`] takes.
    pub fn stored(&self) -> usize {
        self.stored
    }

    /// Gives back what a formula was charged, before it is resolved again or removed.
    pub fn give_back(&mut self, areas: usize) {
        self.stored = self.stored.saturating_sub(areas);
    }
}

enum NameEntry {
    /// Being resolved further up the current chain, entered at `depth`: met again from its own
    /// definition it is a direct self-call, from deeper down a cycle.
    Resolving {
        depth: usize,
    },
    Resolved(Summary),
}

/// What a defined name brings to the formulas using it, whichever use resolved it: each use
/// applies its own context ([`Walk::apply`]).
struct Summary {
    /// Shared with the range operands holding the name as its boxes (see [`Deferred`]).
    areas: Rc<[Area]>,
    /// One box per sheet around `areas` ([`bounding_boxes`]), made at the name's first use in a
    /// range's operand: all a range operand takes from the name for its own box. At most as many
    /// as `areas`.
    boxes: Option<Vec<Area>>,
    dynamic: bool,
    /// The names the definition leaves free, those of the names it uses included (folded), as the
    /// numbers of their spellings, sorted. A dynamic name keeps none: every formula using it is
    /// dynamic whatever its caller binds.
    free: Vec<usize>,
    /// Whether the definition holds a spill reference, those of the names it uses included.
    spill: bool,
    /// Whether the definition calls SUBTOTAL or AGGREGATE, those of the names it uses included.
    budgeted: bool,
}

/// Adds `range` to `out` on the sheet(s) `r` names, or on `sheet` for an unqualified reference; a
/// sheet that doesn't exist contributes nothing (evaluation reports #REF!). A span of sheets adds
/// one area per sheet, at each mention: where they don't [`fit`] in `out` within `most` it adds
/// none and returns false, so repeating a span in a definition never takes `out` past `most` areas.
/// The boxes a range operand closes into are counted the same way against the list they join
/// (`Walk::collect`). `distinct` is what `out`'s [`Taken`] knows of its repeats.
fn push_area(
    wb: &Workbook,
    sheet: usize,
    out: &mut Vec<Area>,
    mut distinct: Option<&mut Distinct>,
    r: &Reference,
    range: RangeRef,
    most: usize,
) -> bool {
    match &r.sheet {
        SheetSel::Current => out.push(Area { sheet, range }),
        SheetSel::Named(n) => {
            if let Some(i) = wb.sheet_index(n) {
                out.push(Area { sheet: i, range });
            }
        }
        SheetSel::Span(a, b) => {
            if let (Some(i), Some(j)) = (wb.sheet_index(a), wb.sheet_index(b)) {
                let sheets = i.abs_diff(j).saturating_add(1);
                if !fit(out, distinct.as_deref_mut(), sheets, most) {
                    return false;
                }
                // A span whose areas the list already holds without repeats would add only repeats,
                // which the next removal drops: they are counted, not added.
                let span = (i.min(j), i.max(j), range);
                if let Some(d) = distinct {
                    if d.spans.contains(&span) {
                        d.repeats = d.repeats.saturating_add(sheets);
                        return true;
                    }
                    d.spans_added.push(span);
                }
                for s in i.min(j)..=i.max(j) {
                    out.push(Area { sheet: s, range });
                }
            }
        }
    }
    true
}

/// Whether `extra` more areas fit in `list` within `most`, the `extra` counted in full whether or
/// not `list` holds them. Where they would not, the repeats are removed from `list` (its first of
/// each kept, in order) and it is counted again: repeats are removed from a definition's areas
/// after collecting anyway, so a reference written many times before a span of sheets counts as it
/// does after the span, and a span of up to half `most` sheets, repeated alone in a definition,
/// fits however often it is written. Only the areas added since the last removal are looked at
/// (`distinct` holds the ones before, which have no repeats; without it the whole list is): a
/// definition repeating a span of 512 sheets 700 times would otherwise pass over a full list of
/// 1,024 areas at each mention. `distinct` holds no more areas than the list held at a removal,
/// at most `most` after a count that fits plus the areas added since. Past a count that fails
/// even so, the name is dynamic and stores no areas, and the walk adds no more references or boxes
/// to it (`Walk::collect`). A range operand's list may hold names as their boxes (see `Walk::apply`):
/// it is counted with their areas, which are put back in their place before repeats are removed.
fn fit(list: &mut Vec<Area>, distinct: Option<&mut Distinct>, extra: usize, most: usize) -> bool {
    let mut spare = Distinct::default();
    let distinct = match distinct {
        Some(d) => d,
        None => &mut spare,
    };
    if list.len().saturating_add(distinct.repeats).saturating_add(distinct.hidden).saturating_add(extra) <= most {
        return true;
    }
    distinct.expand(list);
    let added = list.split_off(distinct.clean.min(list.len()));
    #[cfg(test)]
    AREAS_SIFTED.with(|n| n.set(n.get().saturating_add(added.len())));
    list.extend(added.into_iter().filter(|a| distinct.seen.insert(*a)));
    distinct.clean = list.len();
    distinct.repeats = 0;
    let Distinct { spans, spans_added, .. } = distinct;
    spans.extend(spans_added.drain(..));
    list.len().saturating_add(extra) <= most
}

#[cfg(test)]
thread_local! {
    /// Areas [`fit`] looked at on this thread to remove repeats, which a test reads.
    pub(crate) static AREAS_SIFTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The first `clean` areas of a list being collected, which hold no repeats, and the set of them:
/// [`fit`] removes repeats from the areas after them only. `spans` are the spans of sheets (first
/// sheet, last sheet, range) whose areas are all among those, and `spans_added` those added since
/// the last removal; a span of `spans` mentioned again only counts its areas in `repeats`, the
/// repeats the list would hold had they been added, which every count includes and the next
/// removal clears. The spans kept are at most those written in the definition. `deferred` are the
/// names a range operand's list holds as their boxes, in the order they joined it, all after
/// `clean`, and `hidden` the areas they would add beyond their boxes, which every count includes
/// until [`Distinct::expand`] puts their areas back.
#[derive(Default)]
struct Distinct {
    seen: HashSet<Area, std::hash::BuildHasherDefault<crate::recalc::FxHasher>>,
    clean: usize,
    repeats: usize,
    spans: HashSet<(usize, usize, RangeRef), std::hash::BuildHasherDefault<crate::recalc::FxHasher>>,
    spans_added: Vec<(usize, usize, RangeRef)>,
    deferred: Vec<Deferred>,
    hidden: usize,
}

/// A name in a range operand's list as its boxes: the `boxes` areas from `at` stand for `areas`.
struct Deferred {
    at: usize,
    boxes: usize,
    areas: Rc<[Area]>,
}

impl Distinct {
    /// Puts the areas of the names `list` holds as their boxes back in place of the boxes: the list
    /// is then what it would be had the names added their areas.
    fn expand(&mut self, list: &mut Vec<Area>) {
        let Some(first) = self.deferred.first() else { return };
        let mut at = first.at.min(list.len());
        let mut tail = list.split_off(at).into_iter();
        for d in self.deferred.drain(..) {
            list.extend(tail.by_ref().take(d.at.saturating_sub(at)));
            let _boxes = tail.by_ref().take(d.boxes).count();
            list.extend_from_slice(&d.areas);
            at = d.at.saturating_add(d.boxes);
        }
        list.extend(tail);
        self.hidden = 0;
    }
}

/// What a list being collected already holds (see `Walk::taken`).
#[derive(Default)]
struct Taken {
    /// The names whose areas it holds: a name mentioned again adds nothing.
    names: HashSet<NameKey>,
    /// Its areas without repeats, as far as [`fit`] has removed them.
    areas: Distinct,
}

/// Most areas a list being collected may hold after a span of sheets, or the boxes of a range
/// operand, are added to it (see `fit`): in a name's definition (`depth` > 0) [`MAX_NAME_AREAS`],
/// past which the name is dynamic anyway; a formula's own references don't count.
fn most_areas(depth: usize) -> usize {
    if depth > 0 { MAX_NAME_AREAS } else { usize::MAX }
}

/// What a function call reads beyond its arguments' own references: SUMIF and AVERAGEIF the
/// cells of the sum range they really read (see `resized_sum_range`), dynamic when the criteria
/// range's size isn't known statically; GETPIVOTDATA the report its reference points into;
/// INDIRECT, OFFSET and the volatile functions, read at evaluation (dynamic). `in_definition`: the
/// call sits in a name's definition, not in the formula itself.
// The list's repeats (`distinct`) travel beside the list itself, which makes one argument more.
#[allow(clippy::too_many_arguments)]
fn call_reads(
    wb: &Workbook,
    sheet: usize,
    n: &str,
    args: &[Expr],
    in_definition: bool,
    out: &mut Vec<Area>,
    distinct: Option<&mut Distinct>,
    dynamic: &mut bool,
) {
    // GETPIVOTDATA reads the report cells, so it depends on the whole report its reference points
    // into (pivot commands recalculate everything). Any other reference is dynamic, and so is a
    // call in a name's definition: the report would be looked up on the sheet that resolved the
    // name, and a name's summary doesn't depend on the use that resolved it.
    if n == "GETPIVOTDATA" {
        let report = match args.get(1) {
            Some(Expr::Ref(r)) if !in_definition => {
                let si = match &r.sheet {
                    SheetSel::Current => Some(sheet),
                    SheetSel::Named(name) => wb.sheet_index(name),
                    SheetSel::Span(..) => None,
                };
                si.and_then(|i| wb.sheet(i))
                    .and_then(|sh| sh.pivots.iter().find_map(|p| p.last_range.filter(|lr| lr.intersects(&r.range()))))
                    .map(|lr| (r, lr))
            }
            _ => None,
        };
        match report {
            Some((r, lr)) => *dynamic |= !push_area(wb, sheet, out, None, r, lr, usize::MAX),
            None => *dynamic = true,
        }
    }
    if resizes_sum_range(n, args.len()) {
        match (args.first(), args.get(2)) {
            (Some(Expr::Ref(c)), Some(Expr::Ref(s))) => {
                let (crit, sum) = (c.range(), s.range());
                let most = if in_definition { MAX_NAME_AREAS } else { usize::MAX };
                *dynamic |= !push_area(wb, sheet, out, distinct, s, resized_sum_range(sum, crit.height(), crit.width()), most);
            }
            (_, Some(Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Array(_) | Expr::Missing)) => {}
            _ => *dynamic = true,
        }
    }
    if matches!(n, "INDIRECT" | "OFFSET" | "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" | "CELL" | "INFO") {
        *dynamic = true;
    }
}

#[cfg(test)]
thread_local! {
    /// Looks [`bounding_boxes`] took on this thread to place the areas it was given, which a test
    /// reads: one per box compared while the boxes are scanned, one per probe of the index after.
    pub(crate) static BOXES_LOOKED_AT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts one look of [`bounding_boxes`] (see [`BOXES_LOOKED_AT`]).
fn looked() {
    #[cfg(test)]
    BOXES_LOOKED_AT.with(|n| n.set(n.get().saturating_add(1)));
}

/// One box per sheet around `areas`, in the order the sheets first appear: what a range with a
/// computed end covers. Past a few boxes, each area finds its sheet's box through an index: a span
/// of sheets in an operand (`(First:Last!A1):A2`) brings one area per sheet, and a scan of the
/// boxes made so far would cost the square of the number of sheets. Fewer than
/// [`SCANNED_UP_TO`] boxes are scanned, which costs less than building the index.
fn bounding_boxes(areas: &[Area]) -> Vec<Area> {
    let mut boxes: Vec<Area> = Vec::new();
    // Holds every box once there are `SCANNED_UP_TO` of them.
    let mut by_sheet: HashMap<usize, usize, std::hash::BuildHasherDefault<crate::recalc::FxHasher>> = HashMap::default();
    for &area in areas {
        let at = if boxes.len() < SCANNED_UP_TO {
            boxes.iter().position(|b| {
                looked();
                b.sheet == area.sheet
            })
        } else {
            looked();
            by_sheet.get(&area.sheet).copied()
        };
        match at.and_then(|i| boxes.get_mut(i)) {
            Some(x) => x.range = x.range.union(&area.range),
            None => {
                boxes.push(area);
                if boxes.len() == SCANNED_UP_TO {
                    by_sheet.extend(boxes.iter().enumerate().map(|(i, b)| (b.sheet, i)));
                } else if boxes.len() > SCANNED_UP_TO {
                    by_sheet.insert(area.sheet, boxes.len().saturating_sub(1));
                }
            }
        }
    }
    boxes
}

/// The precedent walk over one formula and the definitions of the names it resolves. Two rules
/// hold throughout:
///
/// 1. Anything the walk can't resolve, or meets past one of its bounds, makes the formula dynamic.
///    That is always a correct answer, only a slower one: the formula recalculates every time.
/// 2. A name's [`Summary`] doesn't depend on the use that resolved it: the caller's bound names
///    and whether the use sits in a range's operand are applied at each use, from the summary.
///    Where the order of resolution matters (a chain cut at [`NAME_DEPTH`], a cycle), the summary
///    errs towards dynamic, by rule 1.
struct Walk<'w, 'n> {
    wb: &'w Workbook,
    names: &'n mut NameCache,
    /// Nodes visited so far, counted against [`MAX_PRECEDENT_NODES`].
    nodes: usize,
    /// Areas the formula has taken from the names it mentions itself, counted against
    /// [`MAX_NAME_AREAS`].
    from_names: usize,
    /// Whether the name being resolved went past [`MAX_NAME_AREAS`]: it then stores none.
    overflowed: bool,
    /// What each list being collected already holds (the formula's, a definition's, a computed
    /// range's; innermost last): the names whose areas it holds, and its areas without repeats.
    taken: Vec<Taken>,
    /// Whether the walk met a database function, whose computed criteria it can't follow: the
    /// formula is then dynamic (see [`precedents_with`]).
    database: bool,
    /// The definitions being resolved, innermost last.
    frames: Vec<Frame>,
    /// Whether the definition being resolved (or the formula) met a spill reference so far, those
    /// of the names it uses included: a name holding one, met in a range's operand, is dynamic.
    spill: bool,
    /// Whether the walk met a name: a defined name, a LET variable or LAMBDA parameter (bound or
    /// used), or a call to a function not built in.
    names_met: bool,
    /// Whether the formula calls SUBTOTAL or AGGREGATE, itself or in a definition it reads (the
    /// summaries hand it on), and for a definition being resolved, whether it does.
    budgeted: bool,
}

/// A definition being resolved: what it leaves to its callers so far.
#[derive(Default)]
struct Frame {
    /// The names the definition leaves free, those of the names it uses included (folded), as the
    /// numbers of their spellings, hashed by the cheap hasher the graph uses for cell keys: copying
    /// them is most of a build's work on names that share many free names.
    free: HashSet<usize, std::hash::BuildHasherDefault<crate::recalc::FxHasher>>,
    /// The names bound where the definition calls itself, each once, as the numbers of their
    /// spellings: a definition can call itself a thousand times under two thousand parameters.
    bound_at_self_calls: HashSet<usize, std::hash::BuildHasherDefault<crate::recalc::FxHasher>>,
    /// The [`Bound::generation`] of the definition's bound names at its last self-call, whose
    /// names `bound_at_self_calls` holds already.
    self_call_generation: Option<u64>,
    /// Whether `free` would already take the build past [`MAX_BUILD_FREE_NAMES`] were it kept: the
    /// definition will keep none, unless it is dynamic for another reason (then it keeps none
    /// either), so no more are copied into it.
    past_free_budget: bool,
}

#[cfg(test)]
thread_local! {
    /// Free names copied into the definitions being resolved on this thread, which a test reads.
    pub(crate) static FREE_NAMES_COPIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Bound names compared on this thread with a name looked up or with a name's free names, which
    /// a test reads: one per hash lookup or per name taken from the smaller side.
    pub(crate) static BOUND_NAMES_COMPARED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts `n` bound names compared (see [`BOUND_NAMES_COMPARED`]).
fn compared(n: usize) {
    #[cfg(test)]
    BOUND_NAMES_COMPARED.with(|c| c.set(c.get().saturating_add(n)));
    #[cfg(not(test))]
    let _ = n;
}

/// The LAMBDA parameters and LET variables in scope, as the numbers of their spellings
/// ([`NameCache`]'s `spellings`): a stack of them, innermost last, which the scopes push and pop,
/// and how often each spelling stands on it, so that a name bound in two nested scopes stays bound
/// when the inner one ends. Whether a name is bound costs one hash, not one comparison per bound
/// name: a LAMBDA can have over two hundred parameters and a body mentioning a name hundreds of
/// times.
#[derive(Default)]
struct Bound {
    stack: Vec<usize>,
    counts: HashMap<usize, usize, std::hash::BuildHasherDefault<crate::recalc::FxHasher>>,
    /// Moves on at every name bound or unbound: while it stays the same, so do the names bound.
    generation: u64,
}

impl Bound {
    fn push(&mut self, spelling: usize) {
        self.stack.push(spelling);
        let count = self.counts.entry(spelling).or_insert(0);
        *count = count.saturating_add(1);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Ends the scope of the last `count` names bound.
    fn pop(&mut self, count: usize) {
        for _ in 0..count {
            let Some(spelling) = self.stack.pop() else { return };
            self.generation = self.generation.wrapping_add(1);
            if let Some(left) = self.counts.get_mut(&spelling) {
                *left = left.saturating_sub(1);
                if *left == 0 {
                    self.counts.remove(&spelling);
                }
            }
        }
    }

    fn contains(&self, spelling: usize) -> bool {
        compared(1);
        self.counts.contains_key(&spelling)
    }

    /// The spellings bound, each once.
    fn spellings(&self) -> impl Iterator<Item = usize> + '_ {
        self.counts.keys().copied()
    }

    /// How many spellings are bound, each counted once.
    fn distinct(&self) -> usize {
        self.counts.len()
    }
}

/// Where a name is used: `depth` counts the defined names entered on the way there (bounded by
/// [`NAME_DEPTH`]). `in_range` says the use sits inside an operand of a range operator, whose box
/// would have to cover what a LET variable or LAMBDA parameter there reads: the formula is dynamic
/// instead.
#[derive(Clone, Copy)]
struct Site {
    depth: usize,
    in_range: bool,
}

/// A LET/LAMBDA parameter, case-folded as the evaluator compares names (`same_name`, in any
/// script). (An optional parameter written `[x]` parses as a structured reference, not a name, so
/// it never reaches the walk as one.)
fn bound_name(n: &str) -> String {
    gridcraft_formula::fold_name(n)
}

impl Walk<'_, '_> {
    /// Adds the precedents a defined name contributes when used from `sheet` to `out`, resolving it
    /// through `names` the first time and looking it up after that. The evaluator reads a definition
    /// with the caller's LET variables and LAMBDA parameters (`bound`) in scope, so a name the
    /// definition leaves free is the caller's variable where the caller binds one so named: the
    /// formula is then dynamic. The engine's `name_readers` must match every name this resolves.
    fn name(&mut self, sheet: usize, name: &str, bound: &Bound, out: &mut Vec<Area>, dynamic: &mut bool, site: Site) {
        let wb = self.wb;
        // Split as the evaluator splits it: the parser stores the sheet unquoted, so it is used
        // as is and split at the last `!`.
        let (scope_sheet, bare) = match name.rsplit_once('!') {
            Some((sheet_name, bare)) => match wb.sheet_index(sheet_name) {
                Some(i) => (i, bare),
                None => {
                    *dynamic = true;
                    return;
                }
            },
            None => (sheet, name),
        };
        let Some(definition) = self.names.lookup.find(wb, bare, scope_sheet).and_then(|i| wb.names.get(i)) else {
            // Not a defined name: a table name (data body known only at evaluation) or a typo
            // (#NAME?). Either way, evaluate every time.
            *dynamic = true;
            return;
        };
        // A sheet-scoped name used from another sheet (`Sheet1!F`): a LAMBDA it yields runs its body
        // against the calling cell's sheet, not the scope sheet its references are resolved against
        // here, so the cells it reads aren't known statically.
        if definition.scope.is_some_and(|s| s != sheet) {
            *dynamic = true;
            return;
        }
        let name_sheet = definition.scope.unwrap_or(sheet);
        let key = (definition.name.to_ascii_lowercase(), definition.scope, name_sheet);
        // Looked up before the depth bound: a hit doesn't recurse.
        match self.names.entries.get(&key) {
            // A direct self-call (a recursive LAMBDA): it reads what the body being collected reads,
            // nothing more. In a range's operand, its result's cells are not known: dynamic. The
            // names bound here are checked against the definition's free names once they are all
            // known. A self-call under the same bound names as the last one adds none: the
            // definition's self-calls all sit in its own walk, under one `Bound`.
            Some(NameEntry::Resolving { depth }) if depth.saturating_add(1) == site.depth => {
                *dynamic |= site.in_range;
                if let Some(frame) = self.frames.last_mut()
                    && frame.self_call_generation != Some(bound.generation)
                {
                    compared(bound.distinct());
                    frame.bound_at_self_calls.extend(bound.spellings());
                    frame.self_call_generation = Some(bound.generation);
                }
                return;
            }
            Some(NameEntry::Resolving { .. }) => {
                // A longer cycle (A→B→A): B's entry lacks A's areas, so the names on it are dynamic.
                *dynamic = true;
                return;
            }
            Some(NameEntry::Resolved(_)) => return self.apply(&key, bound, out, dynamic, site),
            None => {}
        }
        let parsed = self.names.parsed.saturating_add(definition.formula.len());
        if site.depth >= NAME_DEPTH || parsed > MAX_BUILD_NAME_TEXT || self.names.cached >= MAX_BUILD_NAME_AREAS {
            *dynamic = true;
            return;
        }
        self.names.parsed = parsed;
        let summary = self.resolve(&key, &definition.formula, name_sheet, site.depth);
        self.names.entries.insert(key.clone(), NameEntry::Resolved(summary));
        self.apply(&key, bound, out, dynamic, site);
    }

    /// Resolves the definition `formula` of the name `key`, entered at `depth`, against
    /// `name_sheet`, within the build's budgets.
    fn resolve(&mut self, key: &NameKey, formula: &str, name_sheet: usize, depth: usize) -> Summary {
        let mut summary = Summary { areas: Rc::default(), boxes: None, dynamic: true, free: Vec::new(), spill: false, budgeted: false };
        let mut areas = Vec::new();
        let mut past_free_budget = false;
        let outer_spill = std::mem::replace(&mut self.spill, false);
        let outer_budgeted = std::mem::replace(&mut self.budgeted, false);
        match gridcraft_formula::parse_name(formula) {
            Ok(expr) if !gridcraft_formula::adjust::has_relative_refs(&expr) => {
                self.names.entries.insert(key.clone(), NameEntry::Resolving { depth });
                let outer_overflowed = std::mem::replace(&mut self.overflowed, false);
                self.taken.push(Taken::default());
                self.frames.push(Frame::default());
                (areas, summary.dynamic) = self.collect(name_sheet, &expr, depth + 1);
                self.taken.pop();
                let frame = self.frames.pop().unwrap_or_default();
                // Where it calls itself with one of its free names bound, the call reads that.
                summary.dynamic |= frame.bound_at_self_calls.iter().any(|n| frame.free.contains(n));
                // Counted before they are collected and sorted: past the budget they are dropped.
                past_free_budget =
                    !summary.dynamic && (frame.past_free_budget || self.names.free_kept.saturating_add(frame.free.len()) > MAX_BUILD_FREE_NAMES);
                if !summary.dynamic && !past_free_budget {
                    summary.free = frame.free.into_iter().collect();
                    summary.free.sort_unstable();
                }
                if std::mem::replace(&mut self.overflowed, outer_overflowed) {
                    areas.clear();
                }
                // Stored once each: a definition mentioning a range k times contributes it once.
                let mut distinct = HashSet::new();
                areas.retain(|a| distinct.insert(*a));
            }
            _ => {}
        }
        if areas.len() > MAX_NAME_AREAS {
            areas = Vec::new();
            summary.free = Vec::new();
            summary.dynamic = true;
        }
        let cached = self.names.cached.saturating_add(areas.len());
        let free_kept = self.names.free_kept.saturating_add(summary.free.len());
        if cached > MAX_BUILD_NAME_AREAS || free_kept > MAX_BUILD_FREE_NAMES || past_free_budget {
            areas = Vec::new();
            summary.free = Vec::new();
            summary.dynamic = true;
        } else {
            self.names.cached = cached;
            self.names.free_kept = free_kept;
        }
        summary.areas = areas.into();
        summary.spill = std::mem::replace(&mut self.spill, outer_spill);
        summary.budgeted = std::mem::replace(&mut self.budgeted, outer_budgeted);
        summary
    }

    /// Applies the summary of the resolved name `key` at one use: the use is dynamic where the name
    /// is, where the caller binds a name the definition leaves free, or where a name holding a spill
    /// reference sits in a range's operand; the definition being resolved around the use (if any)
    /// takes on the name's free names and spill reference; and the name's areas join `out` (in a
    /// range's operand, its boxes standing for them), within the budgets of [`Walk::may_add`], unless
    /// `out` holds them already.
    fn apply(&mut self, key: &NameKey, bound: &Bound, out: &mut Vec<Area>, dynamic: &mut bool, site: Site) {
        let Some(NameEntry::Resolved(summary)) = self.names.entries.get(key) else {
            return;
        };
        // A name leaving nothing free (a plain range) can't be shadowed. Otherwise the smaller side
        // is looked up in the other, at every use, before a repeated use returns below: a LET can
        // bind over a hundred names around hundreds of uses of a name leaving one free.
        let shadowed = !summary.free.is_empty()
            && bound.distinct() > 0
            && if bound.distinct() <= summary.free.len() {
                compared(bound.distinct());
                bound.spellings().any(|n| summary.free.binary_search(&n).is_ok())
            } else {
                summary.free.iter().any(|&n| bound.contains(n))
            };
        *dynamic |= summary.dynamic || shadowed || (summary.spill && site.in_range);
        self.spill |= summary.spill;
        self.budgeted |= summary.budgeted;
        let count = summary.areas.len();
        // Mentioned again where its areas were already added: they would only be removed again as
        // repeats, so nothing is added or counted. Nor are its free names passed on again: at that
        // first use, in the same definition, they were passed on, or the definition was dynamic
        // already (or became so past the copy budget) and keeps none.
        if self.taken.last().is_some_and(|t| t.names.contains(key)) {
            return;
        }
        // A definition already dynamic keeps no free names (see [`Summary`]), so none are copied
        // into it; the copy is charged before it is made. Nor are any copied into a definition whose
        // free names would already take the build past what it keeps (it ends with at least as many
        // as either side holds now, and the build keeps no fewer later): those copies are charged
        // as if made, so the copy budget runs out where it would have.
        if !*dynamic && let Some(frame) = self.frames.last_mut() {
            let folded = self.names.folded.saturating_add(summary.free.len());
            if folded > fold_budget() {
                *dynamic = true;
            } else {
                self.names.folded = folded;
                let least = frame.free.len().max(summary.free.len());
                frame.past_free_budget |= self.names.free_kept.saturating_add(least) > MAX_BUILD_FREE_NAMES;
                if !frame.past_free_budget {
                    #[cfg(test)]
                    FREE_NAMES_COPIED.with(|n| n.set(n.get().saturating_add(summary.free.len())));
                    frame.free.extend(summary.free.iter().copied());
                }
            }
        }
        // The repeats a span left uncounted in `out`, and the areas of the names it holds as their
        // boxes (see `Distinct`), count as held.
        let held = out.len().saturating_add(self.taken.last().map_or(0, |t| t.areas.repeats.saturating_add(t.areas.hidden)));
        if !self.may_add(held, count, site) {
            *dynamic = true;
            return;
        }
        if let Some(NameEntry::Resolved(summary)) = self.names.entries.get_mut(key) {
            let Summary { areas, boxes, .. } = summary;
            // In a range's operand (`out` is the operand's own list, which closes into one box per
            // sheet) the name's boxes stand for its areas, which give the same boxes: a name used in
            // many range operands is not copied, nor passed over, at each. Its areas are counted as
            // held all the same, and put back where a count would fail (`fit`).
            let boxes = if site.in_range { Some(&*boxes.get_or_insert_with(|| bounding_boxes(areas))) } else { None };
            match (boxes, self.taken.last_mut()) {
                (Some(boxes), Some(t)) if boxes.len() < areas.len() => {
                    t.areas.deferred.push(Deferred { at: out.len(), boxes: boxes.len(), areas: Rc::clone(areas) });
                    t.areas.hidden = t.areas.hidden.saturating_add(areas.len().saturating_sub(boxes.len()));
                    out.extend_from_slice(boxes);
                }
                _ => out.extend_from_slice(areas),
            }
        }
        if let Some(t) = self.taken.last_mut() {
            t.names.insert(key.clone());
        }
    }

    /// Whether `n` more areas from a name may join the `held` areas collected so far: inside another
    /// name's definition, while that name stays within [`MAX_NAME_AREAS`] (past it, it is dynamic
    /// anyway, so collecting stops there rather than after the definition), and for a name the
    /// formula mentions itself, as [`Walk::may_take`] counts them.
    fn may_add(&mut self, held: usize, n: usize, site: Site) -> bool {
        if site.depth > 0 {
            let fits = held.saturating_add(n) <= MAX_NAME_AREAS;
            self.overflowed |= !fits;
            fits
        } else {
            self.may_take(n)
        }
    }

    /// Whether the formula may take `n` more areas from a name it mentions itself, counting them
    /// when it may: at most [`MAX_NAME_AREAS`] for the formula, and [`MAX_BUILD_NAME_AREAS`] for all
    /// the formulas of the graph build, since a name's areas are stored again for every reader.
    fn may_take(&mut self, n: usize) -> bool {
        self.from_names = self.from_names.saturating_add(n);
        let stored = self.names.stored.saturating_add(n);
        if self.from_names > MAX_NAME_AREAS || stored > MAX_BUILD_NAME_AREAS {
            return false;
        }
        self.names.stored = stored;
        true
    }

    /// Walks `root`, a formula or (`depth` > 0) the definition of the name entered last, and returns
    /// its static references and whether it is dynamic: whether anything in it can't be resolved
    /// now. The walk keeps its own stack of [`Step`]s, so its native stack use doesn't grow with
    /// the expression's nesting; it recurses only into a name's definition, through
    /// [`Walk::name`], which [`NAME_DEPTH`] bounds.
    fn collect<'e>(&mut self, sheet: usize, root: &'e Expr, depth: usize) -> (Vec<Area>, bool) {
        let wb = self.wb;
        // The areas of `root`, and one list for each range operator whose operands are being
        // visited (innermost last), which closes into the enclosing list as bounding boxes.
        let mut out = Vec::new();
        let mut ranges: Vec<Vec<Area>> = Vec::new();
        let mut state =
            Collecting { pending: vec![Step::Visit { expr: root, in_range: false, link: false }], bound: Bound::default(), dynamic: false };
        while let Some(step) = state.pending.pop() {
            let (e, in_range, link) = match step {
                Step::Visit { expr, in_range, link } => (expr, in_range, link),
                Step::Close => {
                    self.taken.pop();
                    if let Some(inner) = ranges.pop() {
                        // The boxes count against the list they join as a span of sheets does
                        // (`push_area`): the operand's list counted its own spans only, so a range
                        // operand repeated in a definition would otherwise collect without bound
                        // before repeats are removed.
                        let boxes = bounding_boxes(&inner);
                        let list = ranges.last_mut().unwrap_or(&mut out);
                        if self.overflowed || !fit(list, self.taken.last_mut().map(|t| &mut t.areas), boxes.len(), most_areas(depth)) {
                            state.dynamic = true;
                            self.overflowed = true;
                        } else {
                            list.extend(boxes);
                        }
                    }
                    continue;
                }
                Step::Bind(variable) => {
                    self.names_met = true;
                    state.bound.push(self.names.spelling(bound_name(variable)));
                    continue;
                }
                Step::Unbind(count) => {
                    state.bound.pop(count);
                    continue;
                }
                Step::Lookup { name, in_range } => {
                    self.names_met = true;
                    // Folded once, for the bound names and the definition's free names. Every bound
                    // name has a number, so a spelling without one isn't bound; with nothing bound,
                    // none is looked up.
                    let folded = (state.bound.distinct() > 0 || !self.frames.is_empty()).then(|| bound_name(name));
                    let spelling = folded.as_ref().filter(|_| state.bound.distinct() > 0).and_then(|f| self.names.spellings.get(f).copied());
                    if !spelling.is_some_and(|n| state.bound.contains(n)) {
                        // In a definition, a name it doesn't bind is left free to its callers.
                        if let Some(folded) = folded
                            && !self.frames.is_empty()
                        {
                            let spelling = spelling.unwrap_or_else(|| self.names.spelling(folded));
                            if let Some(frame) = self.frames.last_mut() {
                                frame.free.insert(spelling);
                            }
                        }
                        let list = ranges.last_mut().unwrap_or(&mut out);
                        self.name(sheet, name, &state.bound, list, &mut state.dynamic, Site { depth, in_range });
                    } else {
                        state.dynamic |= in_range;
                    }
                    continue;
                }
            };
            // Past the node bound the answer is conservative: the formula recalculates every time.
            if !link {
                self.nodes = self.nodes.saturating_add(1);
                if self.nodes > MAX_PRECEDENT_NODES {
                    state.dynamic = true;
                    continue;
                }
            }
            let visit = |expr: &'e Expr, link: bool| Step::Visit { expr, in_range, link };
            match e {
                // Past `overflowed` the name stores no areas (and is dynamic already), so none is
                // added: a span failing to `fit` again would remove repeats again.
                Expr::Ref(_) if self.overflowed => {}
                Expr::Ref(r) => {
                    let distinct = self.taken.last_mut().map(|t| &mut t.areas);
                    if !push_area(wb, sheet, ranges.last_mut().unwrap_or(&mut out), distinct, r, r.range(), most_areas(depth)) {
                        state.dynamic = true;
                        self.overflowed = true;
                    }
                }
                // Looked up next, before any other step.
                Expr::Name(n) => state.pending.push(Step::Lookup { name: n, in_range }),
                Expr::Struct(_) => state.dynamic = true,
                Expr::Array(rows) => state.pending.extend(rows.iter().flatten().rev().map(|x| visit(x, false))),
                Expr::Unary(UnOp::Percent, a) => state.pending.push(visit(a, matches!(**a, Expr::Unary(UnOp::Percent, _)))),
                // The spill operator (`A1#`): in a range's operand the spill's extent, not the anchor
                // cell, sets the box, so the formula is dynamic.
                Expr::Unary(UnOp::Spill, a) => {
                    self.spill = true;
                    state.dynamic |= in_range;
                    state.pending.push(visit(a, false));
                }
                Expr::Paren(a) => state.pending.push(visit(a, matches!(**a, Expr::Paren(_)))),
                Expr::Unary(_, a) => state.pending.push(visit(a, false)),
                // A range ending in a function's result (`A1:INDEX(B1:B9,n)`), or a chain of plain
                // references the parser can't fold into one (`A1:B2:C3`), covers the cells between the
                // references inside it, not only those references. A range whose left operand is a
                // range (`X:(B1):(C1)`) is one list: its operands, the left one's included, fill it, and
                // it closes once. A trim reference (`A1:.B9`) is the range operator with a modifier.
                Expr::Binary(BinOp::Range, a, b) => {
                    if !link {
                        // The range's own list: a name already added to the enclosing list still counts
                        // towards its box.
                        ranges.push(Vec::new());
                        self.taken.push(Taken::default());
                        state.pending.push(Step::Close);
                    }
                    state.pending.push(Step::Visit { expr: b, in_range: true, link: false });
                    state.pending.push(Step::Visit { expr: a, in_range: true, link: matches!(**a, Expr::Binary(BinOp::Range, ..)) });
                }
                Expr::Binary(_, a, b) => {
                    state.pending.push(visit(b, false));
                    state.pending.push(visit(a, matches!(&**a, Expr::Binary(op, ..) if *op != BinOp::Range)));
                }
                Expr::Invoke(callee, args) => {
                    state.pending.extend(args.iter().rev().map(|x| visit(x, false)));
                    state.pending.push(visit(callee, matches!(**callee, Expr::Invoke(..))));
                }
                Expr::Call(n, args) => self.visit_call(sheet, Site { depth, in_range }, n, args, ranges.last_mut().unwrap_or(&mut out), &mut state),
                Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Missing => {}
            }
        }
        (out, state.dynamic)
    }

    /// Queues a call's steps: a LAMBDA body, its parameters bound until an `Unbind`; LET values,
    /// each then a `Bind`, the result and an `Unbind`; else the arguments and, last, a `Lookup` of a
    /// callee not built in, so its areas count against [`MAX_NAME_AREAS`] after theirs.
    fn visit_call<'e>(&mut self, sheet: usize, site: Site, n: &'e str, args: &'e [Expr], list: &mut Vec<Area>, state: &mut Collecting<'e>) {
        let visit = |expr: &'e Expr, link: bool| Step::Visit { expr, in_range: site.in_range, link };
        call_reads(self.wb, sheet, n, args, site.depth > 0, list, self.taken.last_mut().map(|t| &mut t.areas), &mut state.dynamic);
        // A database function makes the formula dynamic (see `precedents_with`), and inside a
        // name's definition the name too, for every formula reading it. Their names all start
        // with D (the parser upper-cases calls), so other calls aren't looked up.
        if n.starts_with('D') && gridcraft_functions::lookup(n).is_some_and(|f| f.category == gridcraft_functions::Category::Database) {
            self.database = true;
            state.dynamic |= site.depth > 0;
        }
        // Their budget on cut sizes (see `precedents_with`); the parser upper-cases calls, as the
        // evaluator's dispatch reads them.
        self.budgeted |= matches!(n, "SUBTOTAL" | "AGGREGATE");
        match (n, args.split_last()) {
            // Parameters are bound inside the body only; the body is the last argument.
            ("LAMBDA", Some((body, parameters))) => {
                let mut added = 0;
                for p in parameters {
                    if let Expr::Name(pn) = p {
                        state.bound.push(self.names.spelling(bound_name(pn)));
                        added += 1;
                    }
                }
                self.names_met |= added > 0;
                state.pending.push(Step::Unbind(added));
                state.pending.push(visit(body, false));
            }
            // Each LET variable is bound for the values after it and for the result.
            ("LET", Some((result, pairs))) if args.len() >= 3 => {
                let added = pairs.chunks(2).filter(|pair| matches!(pair, [Expr::Name(_), _])).count();
                state.pending.push(Step::Unbind(added));
                state.pending.push(visit(result, false));
                for pair in pairs.chunks(2).rev() {
                    match pair {
                        [Expr::Name(variable), value] => {
                            state.pending.push(Step::Bind(variable));
                            state.pending.push(visit(value, false));
                        }
                        [other] => state.pending.push(visit(other, false)),
                        _ => {}
                    }
                }
            }
            _ => {
                // Not a built-in: a defined name holding a LAMBDA (its body's references are
                // this cell's precedents too), or unknown (#NAME?, evaluated every time),
                // looked up after the arguments.
                if gridcraft_functions::lookup(n).is_none() && !is_special(n) {
                    state.pending.push(Step::Lookup { name: n, in_range: site.in_range });
                }
                state.pending.extend(args.iter().rev().map(|x| visit(x, false)));
            }
        }
    }
}

/// The state of one [`Walk::collect`] loop, which a visited call also changes.
struct Collecting<'e> {
    /// The steps still to run.
    pending: Vec<Step<'e>>,
    /// The LAMBDA parameters and LET variables in scope, which are not name lookups.
    bound: Bound,
    /// Whether anything met so far can't be resolved now.
    dynamic: bool,
}

/// One entry of [`Walk::collect`]'s explicit stack. Steps run last in, first out: a node queues its
/// children in reverse, so they run left to right, and a step queued before them runs after all of
/// them.
enum Step<'e> {
    /// Visit an expression and queue what it holds.
    Visit {
        expr: &'e Expr,
        /// Inside an operand of a range operator (see [`Site`]).
        in_range: bool,
        /// An inner link of a chain of one shape. It means two things: the node is not counted
        /// against [`MAX_PRECEDENT_NODES`] (it counts with the chain's outermost node), and, for a
        /// range that is the left operand of a range, no list of its own is opened and closed: its
        /// operands fill the parent range's list.
        link: bool,
    },
    /// The operands of a range operator have been visited: the list they filled closes into the
    /// enclosing one, one box per sheet.
    Close,
    /// A LET variable's value has been visited: the variable is bound for what comes after it.
    Bind(&'e str),
    /// The scope of the last `count` LET variables or LAMBDA parameters ends.
    Unbind(usize),
    /// Look up a name met in the expression, or the callee of a call to a function that isn't built
    /// in once its arguments have been visited: a LET variable or LAMBDA parameter in scope, or a
    /// defined name ([`Walk::name`]).
    Lookup { name: &'e str, in_range: bool },
}

fn is_special(n: &str) -> bool {
    matches!(
        n,
        "IF" | "IFS"
            | "IFERROR"
            | "IFNA"
            | "CHOOSE"
            | "SWITCH"
            | "ROW"
            | "COLUMN"
            | "ROWS"
            | "COLUMNS"
            | "AREAS"
            | "ISREF"
            | "ISFORMULA"
            | "FORMULATEXT"
            | "SHEET"
            | "SHEETS"
            | "OFFSET"
            | "INDIRECT"
            | "INDEX"
            | "CELL"
            | "TRIMRANGE"
            | "GETPIVOTDATA"
            | "SUBTOTAL"
            | "AGGREGATE"
            | "TEXT"
            | "LET"
            | "LAMBDA"
            | "ISOMITTED"
            | "MAP"
            | "REDUCE"
            | "SCAN"
            | "BYROW"
            | "BYCOL"
            | "MAKEARRAY"
    )
}

/// Names of functions the evaluator implements itself (merged into the function list for UI).
pub const SPECIAL_FUNCTIONS: &[&str] = &[
    "IF",
    "IFS",
    "IFERROR",
    "IFNA",
    "CHOOSE",
    "SWITCH",
    "ROW",
    "COLUMN",
    "ROWS",
    "COLUMNS",
    "AREAS",
    "ISREF",
    "ISFORMULA",
    "FORMULATEXT",
    "SHEET",
    "SHEETS",
    "OFFSET",
    "INDIRECT",
    "INDEX",
    "CELL",
    "TRIMRANGE",
    "GETPIVOTDATA",
    "SUBTOTAL",
    "AGGREGATE",
    "TEXT",
    "LET",
    "LAMBDA",
    "ISOMITTED",
    "MAP",
    "REDUCE",
    "SCAN",
    "BYROW",
    "BYCOL",
    "MAKEARRAY",
];

pub fn is_known_function(n: &str) -> bool {
    is_special(&n.to_ascii_uppercase()) || gridcraft_functions::lookup(n).is_some()
}
