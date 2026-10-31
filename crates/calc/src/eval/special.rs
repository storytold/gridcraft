use std::collections::{HashMap, HashSet};

use super::{Area, BinOp, Ev, Expr, Reference, SheetSel, UnOp, Value, Workbook, stack_position};
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
/// hidden names its commands rewrite without one, so the resolution can't go stale. Names with
/// relative references (evaluated relative to the using cell), unknown names, unparsable names,
/// sheet-scoped names used from another sheet, cycles between names and chains deeper than the
/// bound stay dynamic. A LAMBDA name calling itself is not a cycle here: the call reads what its
/// body reads.
///
/// Resolves names through a fresh [`NameCache`]; a graph build resolving many formulas shares one
/// through [`precedents_with`].
pub fn precedents(wb: &Workbook, sheet: usize, e: &Expr) -> (Vec<Area>, bool) {
    precedents_with(wb, sheet, e, &mut NameCache::default())
}

/// [`precedents`] with the defined names resolved through `names`, shared across the formulas of
/// one graph build.
pub fn precedents_with(wb: &Workbook, sheet: usize, e: &Expr, names: &mut NameCache) -> (Vec<Area>, bool) {
    let mut out = Vec::new();
    let mut dynamic = false;
    Walk { wb, top: stack_position(), names, from_names: 0, overflowed: false, taken: vec![HashSet::new()] }.collect(
        sheet,
        e,
        &mut Vec::new(),
        &mut out,
        &mut dynamic,
        Depth::default(),
    );
    // Each area once: a formula mentioning a name or a range k times depends on it once.
    let mut distinct = HashSet::new();
    out.retain(|a| distinct.insert(*a));
    (out, dynamic)
}

/// Names referring to names are followed this many levels before the formula is called dynamic.
const NAME_DEPTH: usize = 8;

/// Most areas one defined name contributes, and most a formula takes from the names it mentions,
/// before the name or the formula is called dynamic. A name's areas are stored in the graph for
/// every formula reading it, so a name of many single cells, or one summing many such names, would
/// otherwise make each short formula reading it as large as all of them; dynamic, as every name was
/// before names were resolved, is always a correct answer. A formula's own references don't count.
const MAX_NAME_AREAS: usize = 1024;

/// Most areas the formulas of one graph build may take from names, all together. A name of
/// [`MAX_NAME_AREAS`] areas read from a whole column of cells would store them once per cell (a
/// million readers: a thousand million areas); past this budget, about 50 MB of them, the further
/// readers are dynamic. A workbook of tens of thousands of LAMBDA calls over a named range stores a
/// few areas per reader, far below it. The names a build resolves keep their own areas too, as many
/// again at most: a definition that is only another name (`copy_k = wide_name`) costs three bytes of
/// text but copies all the areas of the name it repeats, so past this budget the further names are
/// dynamic, and a workbook of many such names costs no more than this.
const MAX_BUILD_NAME_AREAS: usize = 1 << 20;

/// Most definition text one graph build parses to resolve names, in bytes. A workbook-wide name is
/// resolved for each sheet that uses it (its unqualified references point at that sheet), so a
/// thousand long names used from two hundred sheets would parse 1.6 GB of definitions; past this
/// budget, about half a second of parsing, the further names are dynamic. A workbook of LAMBDA names
/// used from a few sheets parses a few kilobytes.
const MAX_BUILD_NAME_TEXT: usize = 4 << 20;

/// Nesting the precedent walk follows in total, through names included: the parser's own bound
/// for one formula, counted the way the parser counts (a left operand, a postfix `%` or `#`, an
/// invoked result and an array constant's elements sit at their expression's level; a right
/// operand, an argument and a parenthesis one deeper; the walk's count is one less than the
/// parser's depth, the formula's own level), so
/// no single formula is cut, while a chain of names that together nest past it is. The native
/// stack the walk may use is bounded on top of this, by `WALK_STACK_BUDGET` and `WALK_PARSE_BUDGET`.
const MAX_NESTING: usize = 256;

/// Native stack, in bytes, the precedent walk may use below `precedents_with`, however the parser
/// counted the nesting: the nesting count follows the parser shape by shape, but a shape the parser
/// counts differently (a trim reference costs it nothing and the walk one or two frames a link)
/// must still fit a 1 MiB thread (the Windows main thread), with room for the frames above the
/// walk. Past it the formula is dynamic. In the macOS test build no formula within the length limit
/// gets there, a trim-reference chain at that limit included; the budget bounds the walk by the
/// stack it measures, whatever a frame costs on other targets. Measured like [`STACK_BUDGET`], with
/// [`stack_position`].
const WALK_STACK_BUDGET: usize = 768 * 1024;
/// Stack the walk may have used when it meets a defined name, whose definition it parses on top of
/// its deepest frame: a definition nested to the parser's limit needs about 565 KiB in the test
/// profile and 500 KiB in release, so this and that together stay within the budget above. Met
/// deeper down, the name stays dynamic for that formula.
const WALK_PARSE_BUDGET: usize = 192 * 1024;

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
        let mut by_name = HashMap::new();
        for (i, n) in wb.names.iter().enumerate() {
            by_name.entry((n.name.to_ascii_lowercase(), n.scope)).or_insert(i);
        }
        NameIndex { by_name }
    }

    /// Where in `wb.names` the name `name` seen from `sheet` is, as [`Workbook::name`] finds it: one
    /// scoped to that sheet first, then a workbook-wide one.
    pub fn find(&self, name: &str, sheet: usize) -> Option<usize> {
        let lower = name.to_ascii_lowercase();
        self.by_name.get(&(lower.clone(), Some(sheet))).or_else(|| self.by_name.get(&(lower, None))).copied()
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
/// per use. A name met again from its own
/// definition (a recursive LAMBDA) adds nothing; met again from deeper down the chain it is a
/// cycle, and the names on it are dynamic. A chain cut by [`NAME_DEPTH`] is
/// dynamic from the cut up to the formula, and the cache keeps that answer for those names even
/// where a shorter chain reaches them later: dynamic is always a correct answer, only a slower one.
#[derive(Default)]
pub struct NameCache {
    /// Keyed by the name, its scope and the sheet its unqualified references resolve against.
    entries: HashMap<NameKey, NameEntry>,
    /// The workbook's names by lower-cased name and scope, built at the first lookup.
    index: Option<NameIndex>,
    /// Areas handed to the formulas of the build from the names they mention, counted against
    /// [`MAX_BUILD_NAME_AREAS`].
    stored: usize,
    /// Characters of definitions parsed in the build, counted against [`MAX_BUILD_NAME_TEXT`].
    parsed: usize,
    /// Areas kept in `entries` for the names resolved in the build, counted against
    /// [`MAX_BUILD_NAME_AREAS`].
    cached: usize,
}

impl NameCache {
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
    Resolved {
        areas: Vec<Area>,
        dynamic: bool,
    },
}

/// Adds `range` to `out` on the sheet(s) `r` names, or on `sheet` for an unqualified reference;
/// a sheet that doesn't exist contributes nothing (evaluation reports #REF!).
fn push_area(wb: &Workbook, sheet: usize, out: &mut Vec<Area>, r: &Reference, range: RangeRef) {
    match &r.sheet {
        SheetSel::Current => out.push(Area { sheet, range }),
        SheetSel::Named(n) => {
            if let Some(i) = wb.sheet_index(n) {
                out.push(Area { sheet: i, range });
            }
        }
        SheetSel::Span(a, b) => {
            if let (Some(i), Some(j)) = (wb.sheet_index(a), wb.sheet_index(b)) {
                for s in i.min(j)..=i.max(j) {
                    out.push(Area { sheet: s, range });
                }
            }
        }
    }
}

/// What a function call reads beyond its arguments' own references: SUMIF and AVERAGEIF the
/// cells of the sum range they really read (see `resized_sum_range`), dynamic when the criteria
/// range's size isn't known statically; INDIRECT, OFFSET and the volatile functions, read at
/// evaluation (dynamic).
fn call_reads(wb: &Workbook, sheet: usize, n: &str, args: &[Expr], out: &mut Vec<Area>, dynamic: &mut bool) {
    if resizes_sum_range(n, args.len()) {
        match (args.first(), args.get(2)) {
            (Some(Expr::Ref(c)), Some(Expr::Ref(s))) => {
                let (crit, sum) = (c.range(), s.range());
                push_area(wb, sheet, out, s, resized_sum_range(sum, crit.height(), crit.width()));
            }
            (_, Some(Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Array(_) | Expr::Missing)) => {}
            _ => *dynamic = true,
        }
    }
    if matches!(n, "INDIRECT" | "OFFSET" | "NOW" | "TODAY" | "RAND" | "RANDBETWEEN" | "RANDARRAY" | "CELL" | "INFO") {
        *dynamic = true;
    }
}

/// One box per sheet around `areas`: what a range with a computed end covers.
fn bounding_boxes(areas: Vec<Area>) -> Vec<Area> {
    let mut boxes: Vec<Area> = Vec::new();
    for area in areas {
        match boxes.iter_mut().find(|x| x.sheet == area.sheet) {
            Some(x) => x.range = x.range.union(&area.range),
            None => boxes.push(area),
        }
    }
    boxes
}

/// The precedent walk over one formula: what stays fixed while it recurses.
struct Walk<'w, 'n> {
    wb: &'w Workbook,
    /// `stack_position()` where the walk started, for the stack budgets.
    top: usize,
    names: &'n mut NameCache,
    /// Areas the formula has taken from the names it mentions itself, counted against
    /// [`MAX_NAME_AREAS`].
    from_names: usize,
    /// Whether the name being resolved went past [`MAX_NAME_AREAS`]: it then stores none.
    overflowed: bool,
    /// The names whose areas each list being collected already holds (the formula's, a
    /// definition's, a computed range's; innermost last): a name mentioned again adds nothing.
    taken: Vec<HashSet<NameKey>>,
}

/// How deep the walk is: `names` counts the defined names entered on the way here (bounded by
/// [`NAME_DEPTH`]), `nest` the nesting the parser would count (bounded by [`MAX_NESTING`]).
#[derive(Clone, Copy, Default)]
struct Depth {
    names: usize,
    nest: usize,
}

impl Depth {
    /// One nesting level deeper: a right operand, an argument, a parenthesis.
    fn deeper(self) -> Self {
        Depth { nest: self.nest + 1, ..self }
    }
    /// Into a defined name's definition, which is also one nesting level deeper.
    fn into_name(self) -> Self {
        Depth { names: self.names + 1, nest: self.nest + 1 }
    }
}

/// A LET/LAMBDA parameter, lower-cased as names compare. (An optional parameter written `[x]`
/// parses as a structured reference, not a name, so it never reaches the walk as one.)
fn bound_name(n: &str) -> String {
    n.to_ascii_lowercase()
}

impl Walk<'_, '_> {
    /// Adds the precedents a defined name contributes when used from `sheet` to `out`, resolving it
    /// through `names` the first time and looking it up after that.
    fn name(&mut self, sheet: usize, n: &str, out: &mut Vec<Area>, dynamic: &mut bool, at: Depth) {
        let wb = self.wb;
        let (scope_sheet, bare) = match n.split_once('!') {
            Some((s, b)) => match wb.sheet_index(&gridcraft_core::unquote_sheet(s)) {
                Some(i) => (i, b),
                None => {
                    *dynamic = true;
                    return;
                }
            },
            None => (sheet, n),
        };
        let index = self.names.index.get_or_insert_with(|| NameIndex::new(wb));
        let Some(def) = index.find(bare, scope_sheet).and_then(|i| wb.names.get(i)) else {
            // Not a defined name: a table name (data body known only at evaluation) or a typo
            // (#NAME?). Either way, evaluate every time.
            *dynamic = true;
            return;
        };
        // A sheet-scoped name used from another sheet (`Sheet1!F`): a LAMBDA it yields runs its body
        // against the calling cell's sheet, not the scope sheet its references are resolved against
        // here, so the cells it reads aren't known statically.
        if def.scope.is_some_and(|s| s != sheet) {
            *dynamic = true;
            return;
        }
        let name_sheet = def.scope.unwrap_or(sheet);
        let key = (def.name.to_ascii_lowercase(), def.scope, name_sheet);
        // Looked up before the depth bound: a hit doesn't recurse.
        match self.names.entries.get(&key) {
            // A direct self-call (a recursive LAMBDA): it reads what the body being collected reads,
            // nothing more.
            Some(NameEntry::Resolving { depth: entered }) if entered.saturating_add(1) == at.names => return,
            Some(NameEntry::Resolving { .. }) => {
                // A longer cycle (A→B→A): B's entry lacks A's areas, so the names on it are dynamic.
                *dynamic = true;
                return;
            }
            Some(NameEntry::Resolved { areas, dynamic: d }) => {
                let (n, d) = (areas.len(), *d);
                *dynamic |= d;
                // Mentioned again where its areas were already added: they would only be removed
                // again as repeats, so nothing is added or counted.
                if self.taken.last().is_some_and(|t| t.contains(&key)) {
                    return;
                }
                if !self.may_add(out.len(), n, at) {
                    *dynamic = true;
                } else if let Some(NameEntry::Resolved { areas, .. }) = self.names.entries.get(&key) {
                    out.extend_from_slice(areas);
                    if let Some(t) = self.taken.last_mut() {
                        t.insert(key);
                    }
                }
                return;
            }
            None => {}
        }
        let parsed = self.names.parsed.saturating_add(def.formula.len());
        if at.names >= NAME_DEPTH
            || stack_position().abs_diff(self.top) > WALK_PARSE_BUDGET
            || parsed > MAX_BUILD_NAME_TEXT
            || self.names.cached >= MAX_BUILD_NAME_AREAS
        {
            *dynamic = true;
            return;
        }
        self.names.parsed = parsed;
        let mut areas = Vec::new();
        let mut d = false;
        match gridcraft_formula::parse_name(&def.formula) {
            Ok(expr) if !gridcraft_formula::adjust::has_relative_refs(&expr) => {
                self.names.entries.insert(key.clone(), NameEntry::Resolving { depth: at.names });
                let outer = std::mem::replace(&mut self.overflowed, false);
                self.taken.push(HashSet::new());
                self.collect(name_sheet, &expr, &mut Vec::new(), &mut areas, &mut d, at.into_name());
                self.taken.pop();
                if std::mem::replace(&mut self.overflowed, outer) {
                    areas.clear();
                }
                // Stored once each: a definition mentioning a range k times contributes it once.
                let mut distinct = HashSet::new();
                areas.retain(|a| distinct.insert(*a));
            }
            _ => d = true,
        }
        if areas.len() > MAX_NAME_AREAS {
            areas = Vec::new();
            d = true;
        }
        let cached = self.names.cached.saturating_add(areas.len());
        if cached > MAX_BUILD_NAME_AREAS {
            areas = Vec::new();
            d = true;
        } else {
            self.names.cached = cached;
        }
        *dynamic |= d;
        if self.may_add(out.len(), areas.len(), at) {
            out.extend_from_slice(&areas);
            if let Some(t) = self.taken.last_mut() {
                t.insert(key.clone());
            }
        } else {
            *dynamic = true;
        }
        self.names.entries.insert(key, NameEntry::Resolved { areas, dynamic: d });
    }

    /// Whether `n` more areas from a name may join the `held` areas collected so far: inside another
    /// name's definition, while that name stays within [`MAX_NAME_AREAS`] (past it, it is dynamic
    /// anyway, so collecting stops there rather than after the definition), and for a name the
    /// formula mentions itself, as [`Walk::may_take`] counts them.
    fn may_add(&mut self, held: usize, n: usize, at: Depth) -> bool {
        if at.names > 0 {
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

    /// Walks `e`, adding every static reference to `out` and setting `dynamic` for what can't be
    /// resolved now. `bound` holds the LAMBDA parameters and LET variables in scope, which are not
    /// name lookups; `at` says how deep the walk is.
    fn collect(&mut self, sheet: usize, e: &Expr, bound: &mut Vec<String>, out: &mut Vec<Area>, dynamic: &mut bool, at: Depth) {
        let wb = self.wb;
        if at.nest >= MAX_NESTING || stack_position().abs_diff(self.top) > WALK_STACK_BUDGET {
            // Deeper than the parser lets one formula nest, or than the stack is given, so only a
            // chain of names gets here: what it reads is known at evaluation instead.
            *dynamic = true;
            return;
        }
        match e {
            Expr::Ref(r) => push_area(wb, sheet, out, r, r.range()),
            Expr::Name(n) => {
                if !bound.contains(&bound_name(n)) {
                    self.name(sheet, n, out, dynamic, at);
                }
            }
            Expr::Struct(_) => *dynamic = true,
            // An array constant's elements are read by the parser without a level of their own.
            Expr::Array(rows) => {
                for x in rows.iter().flatten() {
                    self.collect(sheet, x, bound, out, dynamic, at);
                }
            }
            Expr::Unary(UnOp::Percent, _) => {
                // Postfix: the parser wraps the operand without a level of its own, as often as the
                // nesting left allows; unwound here the same way.
                let mut node = e;
                while let Expr::Unary(UnOp::Percent, inner) = node {
                    node = inner;
                }
                self.collect(sheet, node, bound, out, dynamic, at);
            }
            // A trim reference is the range operator with a modifier: its operands sit where a
            // range's do. The spill operator (`A1#`) is a postfix on a reference, at its level.
            Expr::Unary(UnOp::Trim(_) | UnOp::Spill, a) => self.collect(sheet, a, bound, out, dynamic, at),
            Expr::Unary(_, a) | Expr::Paren(a) => self.collect(sheet, a, bound, out, dynamic, at.deeper()),
            // A range ending in a function's result (`A1:INDEX(B1:B9,n)`) covers the cells between
            // the references inside it, not only those references. A left spine of such ranges
            // (`X:(B1):(C1)`) is unwound like an operator chain: the parser has each left operand at
            // its expression's level and each right operand one deeper.
            Expr::Binary(BinOp::Range, a, b) if !matches!((&**a, &**b), (Expr::Ref(_), Expr::Ref(_))) => {
                let mut inner = Vec::new();
                let mut rights = vec![&**b];
                let mut node = &**a;
                while let Expr::Binary(BinOp::Range, x, y) = node
                    && !matches!((&**x, &**y), (Expr::Ref(_), Expr::Ref(_)))
                {
                    rights.push(&**y);
                    node = x;
                }
                // The range's own list: a name already added to `out` still counts towards its box.
                self.taken.push(HashSet::new());
                self.collect(sheet, node, bound, &mut inner, dynamic, at);
                for r in rights.into_iter().rev() {
                    self.collect(sheet, r, bound, &mut inner, dynamic, at.deeper());
                }
                self.taken.pop();
                out.extend(bounding_boxes(inner));
            }
            Expr::Binary(..) => {
                // Unwind the left spine of a long operator chain (`=A1+A2+…`, 4,096 operands within
                // Excel's formula limit) into a list, so the walk recurses once per right operand
                // and not once per operator. A range operator with a computed end stays a node of
                // its own (the arm above).
                let mut rights = Vec::new();
                let mut node = e;
                while let Expr::Binary(op, a, b) = node
                    && !(*op == BinOp::Range && !matches!((&**a, &**b), (Expr::Ref(_), Expr::Ref(_))))
                {
                    rights.push(&**b);
                    node = a;
                }
                // The spine's bottom sits where the parser parsed it, at this level: the nesting
                // counted here then matches the parser's on every path.
                self.collect(sheet, node, bound, out, dynamic, at);
                for b in rights.into_iter().rev() {
                    self.collect(sheet, b, bound, out, dynamic, at.deeper());
                }
            }
            Expr::Invoke(..) => {
                // Invoking a result, perhaps again (`f(x)(y)`): the parser wraps the callee without a
                // level of its own and parses each argument list one deeper.
                let mut lists = Vec::new();
                let mut node = e;
                while let Expr::Invoke(f, args) = node {
                    lists.push(args);
                    node = f;
                }
                self.collect(sheet, node, bound, out, dynamic, at);
                for args in lists.into_iter().rev() {
                    for a in args {
                        self.collect(sheet, a, bound, out, dynamic, at.deeper());
                    }
                }
            }
            Expr::Call(n, args) => {
                call_reads(wb, sheet, n, args, out, dynamic);
                match n.as_str() {
                    // Parameters are bound inside the body only; the body is the last argument.
                    "LAMBDA" if !args.is_empty() => {
                        let mut added = 0;
                        for p in &args[..args.len() - 1] {
                            if let Expr::Name(pn) = p {
                                bound.push(bound_name(pn));
                                added += 1;
                            }
                        }
                        if let Some(body) = args.last() {
                            self.collect(sheet, body, bound, out, dynamic, at.deeper());
                        }
                        bound.truncate(bound.len().saturating_sub(added));
                    }
                    // Each LET variable is bound for the values after it and for the result.
                    "LET" if args.len() >= 3 => {
                        let mut added = 0;
                        for pair in args[..args.len() - 1].chunks(2) {
                            if let [Expr::Name(vn), value] = pair {
                                self.collect(sheet, value, bound, out, dynamic, at.deeper());
                                bound.push(bound_name(vn));
                                added += 1;
                            } else if let [other] = pair {
                                self.collect(sheet, other, bound, out, dynamic, at.deeper());
                            }
                        }
                        if let Some(result) = args.last() {
                            self.collect(sheet, result, bound, out, dynamic, at.deeper());
                        }
                        bound.truncate(bound.len().saturating_sub(added));
                    }
                    _ => {
                        for a in args {
                            self.collect(sheet, a, bound, out, dynamic, at.deeper());
                        }
                        // Not a built-in: a defined name holding a LAMBDA (its body's references are
                        // this cell's precedents too), or unknown (#NAME?, evaluated every time).
                        if gridcraft_functions::lookup(n).is_none() && !is_special(n) && !bound.contains(&bound_name(n)) {
                            self.name(sheet, n, out, dynamic, at);
                        }
                    }
                }
            }
            Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Missing => {}
        }
    }
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
