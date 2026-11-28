//! What recalculates when a defined name changes without a structural edit.
//!
//! A formula reading a name depends on what the name resolves to (its range, or the references
//! in its LAMBDA body), and the dependency graph builds those edges from the definitions. The
//! name commands are structural edits and recalculate everything; the hidden names that store
//! solver models, scenarios, custom views, scripts and the watch list are rewritten by their own
//! commands without that, and a cell can still read them. Then the formulas that mention a changed
//! name, directly or through a name whose definition mentions one, are re-resolved and
//! recalculated like edited cells, and so are the dynamic formulas (a formula can reach a name, or
//! a name reading it, through INDIRECT's text, which mentions no name); in manual mode they are
//! re-resolved only. The
//! rest of the workbook isn't: saving a custom view in a workbook of 50K formulas must not cost a
//! full recalculation.
//!
//! The readers are searched by a formula's text before its tree: the parser keeps each name as
//! written in the text, so a formula whose text doesn't contain an affected name can't mention
//! one, and testing the text is cheaper than walking a tree that has to be parsed first.
//!
//! A mention is matched by the name's text alone, ASCII case aside, with any sheet prefix dropped
//! and the scope of the changed name ignored. That must stay at least as wide as the names the
//! precedent walk resolves (`Walk::name` in the calc crate's `eval/special.rs`): a reader this
//! misses keeps a stale value.

use std::collections::{HashMap, HashSet};

use gridcraft_calc::{Calc, CallGives, Key};
use gridcraft_formula::{BinOp, Expr, UnOp};
use gridcraft_model::{CalcMode, DefinedName, Workbook};

/// Recalculates after an edit that may have changed defined names: `edited` are the cells whose
/// content changed (empty when only names did), `old` the workbook before the edit, which `wb`
/// was copied from. The names are compared only when their revision moved since `old`
/// ([`Workbook::names_revision`]): an edit that wrote no name leaves it, and the comparison of every
/// definition is skipped. In automatic mode, when the names differ, the readers of the changed names join the edited cells
/// and the calculation runs even when there are none while a changed name, or a name reading one
/// through any chain of names, may be a reference before or after, since it also evaluates the
/// dynamic formulas, which can reach such a name through INDIRECT's text. A changed name that can't
/// give a reference either time (the Watch Window list, a saved custom view, the scenario list:
/// text, text joined, or a definition too long to parse), nor any name reading it, gives INDIRECT
/// no reference either time ([`may_be_references`]): with no reader and no edited cell nothing is
/// calculated, and the volatile formulas keep their values, as after a change no formula reads.
/// In manual mode the readers' values wait for the next calculation, as edited cells'
/// dependents do (F9, Shift+F9 and leaving manual mode, for Automatic or Automatic except tables,
/// rebuild the graph and calculate everything), and only the edited cells are calculated; but the
/// readers are resolved again at once, so the graph follows the new definitions for Trace
/// Precedents and Dependents and for the what-if trials (Goal Seek, Scenario Manager, Solver),
/// which calculate a copy of it in automatic mode. Returns whether a calculation ran that
/// evaluated at least one formula, so a caller that adds no undo step knows to move the revision.
pub(crate) fn recalc_after_name_changes(calc: &mut Calc, wb: &mut Workbook, old: &Workbook, edited: Vec<Key>) -> bool {
    // A write may leave the names equal, so a moved revision is compared in full.
    let changed = wb.names_revision() != old.names_revision() && {
        #[cfg(test)]
        NAME_COMPARISONS.with(|count| count.set(count.get().saturating_add(1)));
        wb.names != old.names
    };
    let plan = changed.then(|| {
        let old: Vec<&DefinedName> = old.names.iter().collect();
        plan_after(wb, &old)
    });
    recalc_planned(calc, wb, plan, edited)
}

/// A command's rewrite of one hidden name: every definition named exactly `name` dropped, and at
/// most one appended at the end of the list (the Watch Window list, `stash_set`).
pub(crate) struct Rewrite {
    pub(crate) name: &'static str,
    /// The definitions dropped, with their places in the list before the rewrite, in order.
    pub(crate) removed: Vec<(usize, DefinedName)>,
}

impl Rewrite {
    /// The definitions of `names` that a rewrite of `name` drops, taken before it does.
    pub(crate) fn before(names: &[DefinedName], name: &'static str) -> Rewrite {
        Rewrite { name, removed: names.iter().enumerate().filter(|(_, n)| n.name == name).map(|(i, n)| (i, n.clone())).collect() }
    }
}

/// [`recalc_after_name_changes`] after `rewrite`, with the list before it rebuilt from the list
/// now and the definitions it dropped: the other definitions are borrowed, not copied, since a
/// workbook's definitions can hold tens of megabytes and a Watch Window change rewrites one name.
pub(crate) fn recalc_after_rewrite(calc: &mut Calc, wb: &mut Workbook, rewrite: Rewrite) -> bool {
    let plan = {
        let (kept, appended) = match wb.names.split_last() {
            Some((last, rest)) if last.name == rewrite.name => (rest, Some(last)),
            _ => (&wb.names[..], None),
        };
        // Unchanged only when the one definition dropped from the end came back as it was.
        let unchanged = match (rewrite.removed.as_slice(), appended) {
            ([(at, old)], Some(new)) => *at == kept.len() && old == new,
            ([], None) => true,
            _ => false,
        };
        (!unchanged).then(|| {
            let mut kept = kept.iter();
            let mut removed = rewrite.removed.iter().peekable();
            let total = kept.len().saturating_add(removed.len());
            let old: Vec<&DefinedName> = (0..total)
                .filter_map(|i| if removed.peek().is_some_and(|(at, _)| *at == i) { removed.next().map(|(_, n)| n) } else { kept.next() })
                .collect();
            plan_after(wb, &old)
        })
    };
    let evaluated = recalc_planned(calc, wb, plan, Vec::new());
    // As a commit does after its calculation: a slider or spin button linked to a recalculated
    // cell moves with it.
    if evaluated {
        super::controls::sync_linked_values(wb);
    }
    evaluated
}

/// The readers of the names that differ between `old` and the workbook's, and whether the dynamic
/// formulas must be calculated as well (never in manual mode, where the readers are only resolved
/// again).
fn plan_after(wb: &Workbook, old: &[&DefinedName]) -> (Vec<Key>, bool) {
    let changed = changed_names(old.iter().copied(), &wb.names);
    let (readers, affected) = name_readers(wb, &changed);
    let reachable_by_text = wb.calc.mode != CalcMode::Manual && {
        let new: Vec<&DefinedName> = wb.names.iter().collect();
        [old, &new].iter().any(|names| may_be_references(names, &affected))
    };
    (readers, reachable_by_text)
}

/// Calculates `edited` and the readers `plan` found (or, in manual mode, resolves those again),
/// with the dynamic formulas when the plan says so: see [`recalc_after_name_changes`].
fn recalc_planned(calc: &mut Calc, wb: &mut Workbook, plan: Option<(Vec<Key>, bool)>, edited: Vec<Key>) -> bool {
    let mut dirty = edited;
    let mut reachable_by_text = false;
    if let Some((readers, reachable)) = plan {
        if wb.calc.mode == CalcMode::Manual {
            calc.resolve_again(wb, &readers);
        } else {
            dirty.extend(readers);
            reachable_by_text = reachable;
        }
    }
    if dirty.is_empty() && !reachable_by_text {
        return false;
    }
    calc.cells_changed(wb, &dirty);
    calc.last_recalc_cells > 0
}

/// The names (lower-cased) whose definition differs between `old` and `new`, including names
/// added or removed, and names of one scope spelled twice (ASCII case aside) whose definitions
/// changed places: a formula reads the first, so moving one behind the other (a Watch Window
/// change rewrites its name at the end) changes what the formula reads with no definition changed.
/// Scope is ignored in the result: a mention is matched by name alone, which can only recalculate a
/// reader more than necessary.
fn changed_names<'a>(old: impl IntoIterator<Item = &'a DefinedName>, new: impl IntoIterator<Item = &'a DefinedName>) -> HashSet<String> {
    // Each definition with its place among those of its name and scope. The key borrows the
    // definition's text: a workbook's definitions can hold tens of megabytes, which would
    // otherwise be copied twice for a Watch Window change.
    fn keys<'a>(names: impl IntoIterator<Item = &'a DefinedName>) -> HashSet<(String, Option<usize>, usize, &'a str)> {
        let mut places: HashMap<(String, Option<usize>), usize> = HashMap::new();
        names
            .into_iter()
            .map(|n| {
                let name = n.name.to_ascii_lowercase();
                let place = places.entry((name.clone(), n.scope)).or_insert(0);
                let key = (name, n.scope, *place, n.formula.as_str());
                *place = place.saturating_add(1);
                key
            })
            .collect()
    }
    let (olds, news) = (keys(old), keys(new));
    olds.symmetric_difference(&news).map(|(name, ..)| name.clone()).collect()
}

/// Whether a definition in `names` of one of `affected` (lower-cased, scope ignored) may evaluate
/// to a reference. INDIRECT turns anything else into `#REF!`, so a name that can't give one gives
/// the dynamic formulas the same `#REF!` before and after, and needs no pass over them. The
/// judgement is made on each definition's tree ([`may_give_reference`]).
fn may_be_references(names: &[&DefinedName], affected: &HashSet<String>) -> bool {
    names.iter().filter(|n| affected.contains(&n.name.to_ascii_lowercase())).any(|n| may_give_reference(&n.formula))
}

/// Whether the definition `formula` may evaluate to a reference, judged by its tree,
/// conservatively: a shape the evaluator (`Evaluator::eval` and `Evaluator::call` in the calc
/// crate) may turn into a reference counts as one that may, and some that it never does count so
/// too (`@` turns a reference into a value, and the range operator needs both operands to be
/// references):
/// - a definition that doesn't parse can't: `Evaluator::name` gives `#NAME?` for it;
/// - literals, arrays of them, and operators that compute a value (arithmetic, a text join, a
///   comparison, negation, a percent) can't, whatever their operands;
/// - a reference, a structured reference, an invoked expression (`LAMBDA(x,x)(A1)`), and a call
///   of a function [`gridcraft_calc::call_gives`] says may give one can;
/// - parentheses, the range, union and intersection operators, `@`, a spill reference and a trim
///   reference can when an operand can;
/// - a name can, with a sheet prefix or without, whatever its definitions: `Evaluator::name` looks
///   the name up as written among the caller's LET variables and LAMBDA parameters before the
///   defined names, and a caller of INDIRECT can bind it to a reference (`alias` defined as
///   `edge`, read by `LET(edge,$B$2,INDIRECT("alias"))`; `LET(Sheet1!edge,...)` binds `Sheet1!edge`);
/// - a call by a name that isn't a function can, whatever the name's definitions: the caller can
///   bind the name to a LAMBDA.
fn may_give_reference(formula: &str) -> bool {
    let Ok(e) = gridcraft_formula::parse_name(formula) else { return false };
    // An explicit stack: a long chain of operators must not recurse once per operand.
    let mut pending = vec![&e];
    while let Some(x) = pending.pop() {
        match x {
            Expr::Ref(_) | Expr::Struct(_) | Expr::Invoke(..) | Expr::Name(_) => return true,
            Expr::Paren(y) | Expr::Unary(UnOp::At | UnOp::Spill | UnOp::Trim(_), y) => pending.push(y),
            Expr::Binary(BinOp::Range | BinOp::Union | BinOp::Intersect, a, b) => {
                pending.push(a);
                pending.push(b);
            }
            Expr::Call(f, _) => match gridcraft_calc::call_gives(f) {
                CallGives::Value => {}
                CallGives::MayBeReference | CallGives::ByName => return true,
            },
            Expr::Number(_)
            | Expr::Text(_)
            | Expr::Bool(_)
            | Expr::Error(_)
            | Expr::Array(_)
            | Expr::Missing
            | Expr::Unary(UnOp::Neg | UnOp::Plus | UnOp::Percent, _)
            | Expr::Binary(..) => {}
        }
    }
    false
}

/// Calls `f` with each name `e` mentions, as a name or as a call, without a sheet prefix and as
/// written. A LAMBDA parameter or LET variable with the same name counts as a mention too: that
/// only recalculates a formula that didn't need it.
fn for_each_mention(e: &Expr, mut f: impl FnMut(&str)) {
    e.walk(&mut |x| {
        if let Expr::Name(n) | Expr::Call(n, _) = x {
            f(n.rsplit('!').next().unwrap_or(n));
        }
    });
}

/// The formulas that read any of `changed` names, directly or through another name whose
/// definition mentions one: what a change to those names can affect. Names referring to names
/// are followed through an index of which names mention which, each name at most once. Returned
/// with those names (lower-cased): `changed` and the names reaching one of them through any chain
/// of names, as far as the index got before its budget ran out (every formula is a reader then).
fn name_readers(wb: &Workbook, changed: &HashSet<String>) -> (Vec<Key>, HashSet<String>) {
    if changed.is_empty() {
        return (Vec::new(), HashSet::new());
    }
    // Which names each name's definition mentions, turned around: for every name, the names that
    // mention it. A definition is parsed into it only when its text may mention a name newly found
    // affected (as formulas are screened below, and while no more than `MAX_SCREENED_NAMES` names
    // are affected; past that, every definition not parsed yet is), each at most once: a name-only
    // update (a Watch Window change) then parses the few definitions that can mention the changed
    // names, not all of them. A definition screened out mentions none of the names affected so
    // far, so the index holds every reader of those. Past its budget, every formula is taken as a
    // reader.
    let mut mentioned_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut mentioned = HashSet::new();
    let (mut held, budget) = (0usize, index_budget());
    let mut parsed = vec![false; wb.names.len()];
    // Whether each definition holds an ASCII capital, found the first time it is screened: one
    // without is searched as it is in every round, with no lower-cased copy (see `mentions`).
    let mut capitals: Vec<Option<bool>> = vec![None; wb.names.len()];
    let mut affected = changed.clone();
    // The names found affected since the definitions were last screened.
    let mut fresh = changed.clone();
    while !fresh.is_empty() {
        let screen = affected.len() <= MAX_SCREENED_NAMES;
        for ((n, done), capital) in wb.names.iter().zip(parsed.iter_mut()).zip(capitals.iter_mut()) {
            if *done || (screen && !mentions(&n.formula, &fresh, *capital.get_or_insert_with(|| has_capital(&n.formula)))) {
                continue;
            }
            *done = true;
            #[cfg(test)]
            DEFINITIONS_PARSED.with(|count| count.set(count.get().saturating_add(1)));
            let Ok(e) = gridcraft_formula::parse_name(&n.formula) else { continue };
            let name = n.name.to_ascii_lowercase();
            for_each_mention(&e, |m| {
                mentioned.insert(m.to_ascii_lowercase());
            });
            for m in mentioned.drain() {
                // The owning name's copy, and the mentioned name's own copy the first time it is a key.
                let mut cost = std::mem::size_of::<String>().saturating_add(name.len());
                if !mentioned_by.contains_key(&m) {
                    cost = cost.saturating_add(std::mem::size_of::<String>().saturating_add(m.len()));
                }
                held = held.saturating_add(cost);
                if held > budget {
                    return (every_formula(wb), affected);
                }
                mentioned_by.entry(m).or_default().push(name.clone());
            }
        }
        // Unscreened, the definitions parsed now may mention any name affected so far.
        let mut queue: Vec<String> = if screen { fresh.drain().collect() } else { affected.iter().cloned().collect() };
        fresh.clear();
        while let Some(n) = queue.pop() {
            for reader in mentioned_by.get(&n).into_iter().flatten() {
                if affected.insert(reader.clone()) {
                    fresh.insert(reader.clone());
                    queue.push(reader.clone());
                }
            }
        }
        if !screen {
            // Every definition is in the index: nothing more can be found.
            break;
        }
    }
    // A formula's text is the source of its tree (printed from it, or the text it was parsed
    // from), and the parser keeps each name it reads as written there, so a formula whose text
    // doesn't contain an affected name (ASCII case aside, as names resolve) can't mention one
    // and its tree isn't walked. With many affected names the text tests could cost more than
    // the walks, so every tree is walked then.
    let screen = affected.len() <= MAX_SCREENED_NAMES;
    let mut out = Vec::new();
    for (si, sheet) in wb.sheets.iter().enumerate() {
        for (c, cell) in sheet.cells.iter() {
            if let Some(f) = cell.formula.as_ref()
                && (!screen || text_may_mention(&f.text, &affected))
                && let Some(e) = f.expr_arc()
            {
                let mut found = false;
                for_each_mention(&e, |m| {
                    if !found && affected.contains(&m.to_ascii_lowercase()) {
                        found = true;
                    }
                });
                if found {
                    out.push((si, c));
                }
            }
        }
    }
    (out, affected)
}

/// Every formula in the workbook: the readers [`name_readers`] gives past [`MAX_INDEX_BYTES`].
fn every_formula(wb: &Workbook) -> Vec<Key> {
    let mut out = Vec::new();
    for (si, sheet) in wb.sheets.iter().enumerate() {
        for (c, cell) in sheet.cells.iter() {
            if cell.formula.is_some() {
                out.push((si, c));
            }
        }
    }
    out
}

/// Most bytes the copies of names in [`name_readers`]'s index of which names mention which may
/// take, each counted as a `String` and its text. A definition can mention about a thousand names
/// within the formula length limit, and a file can carry any number of definitions with names of
/// any length, so the index could otherwise take gigabytes on a name-only update (a Watch Window
/// change). Past this budget every formula is taken as a reader and recalculated: always correct,
/// only slower. A workbook of thousands of names that mention a few names each holds a few
/// hundred kilobytes.
const MAX_INDEX_BYTES: usize = 32 << 20;

#[cfg(test)]
thread_local! {
    /// [`MAX_INDEX_BYTES`] on this thread, which a test lowers to build an index at the budget.
    static INDEX_BUDGET: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_INDEX_BYTES) };
}

#[cfg(test)]
thread_local! {
    /// Definitions [`name_readers`] parsed on this thread, which a test reads.
    static DEFINITIONS_PARSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
thread_local! {
    /// Times [`recalc_after_name_changes`] compared the names in full on this thread, which a test
    /// reads.
    static NAME_COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(not(test))]
fn index_budget() -> usize {
    MAX_INDEX_BYTES
}

#[cfg(test)]
fn index_budget() -> usize {
    INDEX_BUDGET.with(std::cell::Cell::get)
}

/// Most affected names [`name_readers`] tests each formula's text, and each definition's, for before
/// walking its tree.
const MAX_SCREENED_NAMES: usize = 32;

/// Whether `text` contains one of `names` (lower-cased), ASCII case aside: whether a formula with
/// this text can mention one of them. An empty name can't be ruled out.
fn text_may_mention(text: &str, names: &HashSet<String>) -> bool {
    mentions(text, names, true)
}

/// Whether `text` holds an ASCII capital. Every byte is looked at, with no early return. A
/// definition without one is searched as it is, with no lower-cased copy: the first round pays
/// this pass over the text in place of the copy, and the rounds after it save the copy.
fn has_capital(text: &str) -> bool {
    text.bytes().fold(false, |found, b| found | b.is_ascii_uppercase())
}

/// [`text_may_mention`], told whether `text` may hold an ASCII capital: one that holds none is its
/// own lower-cased copy and is searched as it is.
fn mentions(text: &str, names: &HashSet<String>, capitals: bool) -> bool {
    // A name longer than the text can't be in it, and needs no copy of the text.
    if names.iter().all(|n| n.len() > text.len()) {
        return false;
    }
    // The text is lower-cased once and each name found with `str::contains`, whose search takes
    // time linear in the text. Comparing the name at each place its first letter matches would
    // cost the text's length times the name's on a text that nearly matches everywhere (a run of
    // one letter, names of that letter ending in another), minutes for one Watch Window change.
    // The copy is one formula's text, dropped before the next formula is screened.
    let lower = if capitals { std::borrow::Cow::Owned(text.to_ascii_lowercase()) } else { std::borrow::Cow::Borrowed(text) };
    names.iter().any(|n| n.len() <= lower.len() && lower.contains(n.as_str()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use gridcraft_core::{CellRef, Value};
    use serde_json::json;

    use super::{DEFINITIONS_PARSED, INDEX_BUDGET, MAX_INDEX_BYTES, NAME_COMPARISONS};
    use crate::Session;

    /// A word for `k` made of letters only (`aa`, `ab`, …), for names that must not read as cells.
    fn letters(k: usize) -> String {
        let (high, low) = (k / 26 % 26, k % 26);
        [high, low].iter().map(|&i| char::from(b'a' + u8::try_from(i).unwrap())).collect()
    }

    #[test]
    fn a_commit_that_writes_no_name_compares_no_names() {
        // A commit compares the names before and after only when their revision moved: a cell edit
        // writes no name and skips the comparison, which for a workbook of many long definitions
        // would cost more than the edit. A command that writes a name compares them.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("formulas.defineName", json!({"name": "Rate", "refersTo": "=0.5"})).unwrap();
        s.execute("cell.set", json!({"cell": "A1", "input": "=Rate*2"})).unwrap();
        let compared = || NAME_COMPARISONS.with(std::cell::Cell::get);
        let before = compared();
        for row in 1..=5 {
            s.execute("cell.set", json!({"cell": format!("B{row}"), "input": row.to_string()})).unwrap();
        }
        s.execute("range.setValues", json!({"range": "C1:C2", "values": [[1], [2]]})).unwrap();
        s.execute("home.bold", json!({"range": "B1:B5"})).unwrap();
        assert_eq!(compared(), before, "edits that write no name compare none");
        s.execute("view.customViews", json!({"save": "first"})).unwrap();
        assert_eq!(compared(), before + 1, "saving a view writes a name, and the names are compared");
        s.execute("cell.set", json!({"cell": "B6", "input": "6"})).unwrap();
        assert_eq!(compared(), before + 1, "and the next cell edit compares none again");
    }

    #[test]
    fn a_watch_window_change_parses_no_definition_that_cannot_mention_its_name() {
        // Two hundred long definitions that mention no name: adding to the Watch Window rewrites
        // its hidden name only, and none of them can mention it, so none is parsed. The reader of
        // the watch list's name still follows it.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
        s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
        let long = format!("={}", vec!["1"; 2_000].join("+"));
        for k in 0..200 {
            s.execute("formulas.defineName", json!({"name": format!("filler_{}", letters(k)), "refersTo": long})).unwrap();
        }
        s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
        DEFINITIONS_PARSED.with(|n| n.set(0));
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
        assert_eq!(DEFINITIONS_PARSED.with(std::cell::Cell::get), 0, "definitions parsed");
        let shown = s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("A1").unwrap());
        assert_eq!(shown, Value::Number("[\"Sheet1!B1\"]".len() as f64));
    }

    #[test]
    fn a_chain_of_names_is_followed_through_the_text_screen() {
        // `outer_name` reads `middle_name`, which reads the watch list's name: `middle_name` is
        // screened in by the changed name, `outer_name` by `middle_name` once that is found
        // affected, and B1 reading `outer_name` follows the change. The filler is parsed by
        // neither.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
        s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
        s.execute("formulas.defineName", json!({"name": "middle_name", "refersTo": "=LEN(_sc_watch)"})).unwrap();
        s.execute("formulas.defineName", json!({"name": "outer_name", "refersTo": "=middle_name+1"})).unwrap();
        s.execute("formulas.defineName", json!({"name": "filler_name", "refersTo": "=7"})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": "=outer_name"})).unwrap();
        DEFINITIONS_PARSED.with(|n| n.set(0));
        s.execute("formulas.watchWindow", json!({"add": "Sheet1!C1"})).unwrap();
        assert_eq!(DEFINITIONS_PARSED.with(std::cell::Cell::get), 2, "the middle and outer definitions");
        let shown = s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("B1").unwrap());
        assert_eq!(shown, Value::Number("[\"Sheet1!C1\"]".len() as f64 + 1.0));
    }

    #[test]
    fn a_near_miss_spelling_is_screened_in_but_is_not_a_mention() {
        // `near_miss` reads `_sc_watched`, whose text holds the changed name `_sc_watch`: its
        // definition is parsed, and found not to mention it, so its reader is not one; the reader
        // of `_sc_watch` itself is.
        let mut wb = gridcraft_model::Workbook::new();
        for (name, formula) in [("_sc_watched", "5"), ("near_miss", "_sc_watched+1")] {
            wb.names_mut().push(gridcraft_model::DefinedName {
                name: name.into(),
                scope: None,
                formula: formula.into(),
                comment: String::new(),
                hidden: false,
            });
        }
        let sheet = wb.sheet_mut(0).unwrap();
        for (at, formula) in [("A1", "=LEN(_sc_watch)"), ("C1", "=near_miss")] {
            sheet.cells.set(CellRef::parse(at).unwrap(), gridcraft_model::Cell::formula(gridcraft_model::Formula::new(formula)));
        }
        let changed: HashSet<String> = ["_sc_watch".to_string()].into_iter().collect();
        DEFINITIONS_PARSED.with(|n| n.set(0));
        let (readers, _) = super::name_readers(&wb, &changed);
        assert_eq!(DEFINITIONS_PARSED.with(std::cell::Cell::get), 1, "near_miss is screened in");
        assert_eq!(readers, vec![(0, CellRef::parse("A1").unwrap())]);
    }

    #[test]
    fn past_its_budget_the_index_of_mentions_gives_way_to_every_formula() {
        // `Alias` mentions two names (`LEN` and the watch list's name), so the index holds two
        // copies of its name and one of each name it mentions. With exactly that budget only the
        // two readers of the watch list's name are recalculated; one byte under it, every formula
        // is, and the values are the same. The readers are entered once the name exists (an empty
        // list, after a first add and remove), so they read it directly (one naming a name that
        // doesn't exist yet is dynamic and recalculated anyway) and the search must find them.
        let string = std::mem::size_of::<String>();
        let held = 2 * (string + "alias".len()) + (string + "len".len()) + (string + "_sc_watch".len());
        for (budget, recalculated) in [(held, 2), (held - 1, 7)] {
            INDEX_BUDGET.with(|b| b.set(budget));
            let mut s = Session::new();
            s.new_workbook();
            s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
            s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
            s.execute("formulas.defineName", json!({"name": "Alias", "refersTo": "=LEN(_sc_watch)"})).unwrap();
            s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
            s.execute("cell.set", json!({"cell": "B1", "input": "=Alias"})).unwrap();
            for r in 1..=5 {
                s.execute("cell.set", json!({"cell": format!("C{r}"), "input": format!("=ROW()*{r}")})).unwrap();
            }
            s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
            let shown = |at: &str| s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(at).unwrap());
            let length = Value::Number("[\"Sheet1!B1\"]".len() as f64);
            assert_eq!((shown("A1"), shown("B1")), (length.clone(), length), "budget {budget}");
            for r in 1..=5 {
                assert_eq!(shown(&format!("C{r}")), Value::Number(f64::from(r * r)), "budget {budget}");
            }
            assert_eq!(s.doc().unwrap().calc.last_recalc_cells, recalculated, "budget {budget}");
        }
        INDEX_BUDGET.with(|b| b.set(MAX_INDEX_BYTES));
    }

    #[test]
    fn long_mentioned_names_count_towards_the_budget_of_the_index_of_mentions() {
        // `Bridge`, a short name, mentions three long names besides `LEN` and the watch list's
        // name: the index holds five copies of `bridge` and one of each mentioned name, the long
        // ones taking most of the budget. At exactly that budget only the two readers of the watch
        // list's name are recalculated; one byte under it, every formula is, and the values are the
        // same. The readers are entered once the name exists (an empty list, after a first add and
        // remove), so they read it directly and the search must find them.
        let string = std::mem::size_of::<String>();
        let long: Vec<String> = ["First", "Second", "Third"].iter().map(|w| format!("Mentioned{w}_{}", "padding".repeat(32))).collect();
        let held = 5 * (string + "bridge".len())
            + (string + "len".len())
            + (string + "_sc_watch".len())
            + long.iter().map(|n| string + n.len()).sum::<usize>();
        for (budget, recalculated) in [(held, 2), (held - 1, 7)] {
            INDEX_BUDGET.with(|b| b.set(budget));
            let mut s = Session::new();
            s.new_workbook();
            s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
            s.execute("formulas.watchWindow", json!({"remove": "Sheet1!B1"})).unwrap();
            for n in &long {
                s.execute("formulas.defineName", json!({"name": n, "refersTo": "=1"})).unwrap();
            }
            let refers_to = format!("=LEN(_sc_watch)+0*({})", long.join("+"));
            s.execute("formulas.defineName", json!({"name": "Bridge", "refersTo": refers_to})).unwrap();
            s.execute("cell.set", json!({"cell": "A1", "input": "=LEN(_sc_watch)"})).unwrap();
            s.execute("cell.set", json!({"cell": "B1", "input": "=Bridge"})).unwrap();
            for r in 1..=5 {
                s.execute("cell.set", json!({"cell": format!("C{r}"), "input": format!("=ROW()*{r}")})).unwrap();
            }
            s.execute("formulas.watchWindow", json!({"add": "Sheet1!B1"})).unwrap();
            let shown = |at: &str| s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(at).unwrap());
            let length = Value::Number("[\"Sheet1!B1\"]".len() as f64);
            assert_eq!((shown("A1"), shown("B1")), (length.clone(), length), "budget {budget}");
            for r in 1..=5 {
                assert_eq!(shown(&format!("C{r}")), Value::Number(f64::from(r * r)), "budget {budget}");
            }
            assert_eq!(s.doc().unwrap().calc.last_recalc_cells, recalculated, "budget {budget}");
        }
        INDEX_BUDGET.with(|b| b.set(MAX_INDEX_BYTES));
    }

    #[test]
    fn the_changed_names_are_those_whose_definitions_differ_or_change_places() {
        // What `changed_names` returns for each kind of edit, lower-cased: an unchanged list, a
        // changed definition, a name added or removed, the same definition moved to another
        // scope, two spellings of one name changing places, and two names of different scopes
        // changing places, which changes what neither reads.
        let name = |name: &str, scope: Option<usize>, formula: &str| gridcraft_model::DefinedName {
            name: name.into(),
            scope,
            formula: formula.into(),
            comment: String::new(),
            hidden: false,
        };
        let changed = |list: &[&str]| list.iter().map(|n| n.to_string()).collect::<HashSet<String>>();
        let base = [name("Rate", None, "0.07"), name("Total", None, "Sheet1!$A$1")];
        let cases: Vec<(&str, Vec<gridcraft_model::DefinedName>, HashSet<String>)> = vec![
            ("unchanged", base.to_vec(), changed(&[])),
            ("definition changed", vec![name("Rate", None, "0.08"), base[1].clone()], changed(&["rate"])),
            ("name added", vec![base[0].clone(), base[1].clone(), name("Extra", None, "1")], changed(&["extra"])),
            ("name removed", vec![base[1].clone()], changed(&["rate"])),
            ("scope changed", vec![name("Rate", Some(0), "0.07"), base[1].clone()], changed(&["rate"])),
            ("case only", vec![name("RATE", None, "0.07"), base[1].clone()], changed(&[])),
            ("names reordered", vec![base[1].clone(), base[0].clone()], changed(&[])),
        ];
        for (label, new, expected) in cases {
            assert_eq!(super::changed_names(&base, &new), expected, "{label}");
        }
        let spellings = [name("Rate", None, "0.07"), name("RATE", None, "0.5")];
        let swapped = [spellings[1].clone(), spellings[0].clone()];
        assert_eq!(super::changed_names(&spellings, &swapped), changed(&["rate"]), "two spellings changed places");
        let scoped = [name("Rate", None, "0.07"), name("Rate", Some(0), "0.5")];
        let swapped = [scoped[1].clone(), scoped[0].clone()];
        assert_eq!(super::changed_names(&scoped, &swapped), changed(&[]), "two scopes changed places");
    }

    #[test]
    fn the_text_screen_finds_a_name_anywhere_in_the_text_ascii_case_aside() {
        // Texts that nearly match everywhere: a run of `a` and names of `a` ending in another
        // letter. A name is found where it stands anywhere in the text, inside a longer word
        // included, in either ASCII case; not where only its start matches, nor where it is
        // longer than the text. Letters outside ASCII keep their case.
        let run = "a".repeat(600);
        let names = |list: &[&str]| list.iter().map(|n| n.to_string()).collect::<HashSet<String>>();
        let cases: Vec<(String, HashSet<String>, bool)> = vec![
            (run.clone(), names(&["aaab"]), false),
            (format!("{run}b"), names(&["aaab"]), true),
            (format!("{}B", run.to_ascii_uppercase()), names(&["aaab"]), true),
            (format!("=SUM({run}c)"), names(&["aaab", "aaac"]), true),
            (format!("=SUM({run}c)"), names(&["aaab", "aaad"]), false),
            (run.clone(), names(&[&"a".repeat(600)]), true),
            (run.clone(), names(&[&"a".repeat(601)]), false),
            ("=Édge+1".to_string(), names(&["édge"]), false),
            ("=édge+1".to_string(), names(&["édge"]), true),
            (String::new(), names(&[""]), true),
            ("=1".to_string(), names(&[""]), true),
        ];
        for (text, names, found) in cases {
            let shown: String = text.chars().take(12).collect();
            assert_eq!(super::text_may_mention(&text, &names), found, "{shown}... ({} bytes) for {names:?}", text.len());
            let capital = super::has_capital(&text);
            assert_eq!(super::mentions(&text, &names, capital), found, "{shown}... without a copy when it has no capital");
        }
    }

    #[test]
    fn a_definition_may_give_a_reference_by_its_tree() {
        // What `may_be_references` answers for `changed_name` with each definition, beside the
        // names it mentions: a definition that doesn't parse, literals and operators computing a
        // value can't give a reference; references, calls of functions that can return one, and
        // invoked expressions can; a mentioned name can, with a sheet prefix or without and
        // whatever its definitions (text, a chain of names, a cycle), since a caller of INDIRECT
        // can bind it as written to a reference, and a name joined to text can't.
        let name = |name: &str, formula: &str| gridcraft_model::DefinedName {
            name: name.into(),
            scope: None,
            formula: formula.into(),
            comment: String::new(),
            hidden: false,
        };
        const WORDS: [&str; 9] = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
        // `changed_name` reads `link_one`, which reads `link_two`, and so on: `links` names in
        // all, the last defined as `end`.
        let chain = |links: usize, end: &str| -> Vec<gridcraft_model::DefinedName> {
            let mut list = vec![name("changed_name", "link_one")];
            for k in 0..links {
                let next = if k + 1 == links { end.to_string() } else { format!("link_{}", WORDS[k + 1]) };
                list.push(name(&format!("link_{}", WORDS[k]), &next));
            }
            list
        };
        let alone = |formula: &str| vec![name("changed_name", formula)];
        let with = |formula: &str, others: &[(&str, &str)]| {
            let mut list = alone(formula);
            list.extend(others.iter().map(|(n, f)| name(n, f)));
            list
        };
        let long_list = format!("\"{}\"", "x".repeat(9_000));
        let cases: Vec<(&str, Vec<gridcraft_model::DefinedName>, bool)> = vec![
            ("a number", alone("5"), false),
            ("text joined", alone("\"first part\"&\"second part\""), false),
            ("a definition that doesn't parse", alone(&long_list), false),
            ("arithmetic over a reference", alone("Sheet1!$A$1+1"), false),
            ("a negated reference", alone("-Sheet1!$A$1"), false),
            ("a function of a reference that gives a value", alone("LEN(Sheet1!$A$1)"), false),
            ("an array of literals", alone("{1,2;3,4}"), false),
            ("a reference", alone("Sheet1!$A$1"), true),
            ("a reference in parentheses", alone("((Sheet1!$A$1))"), true),
            ("a structured reference", alone("Sales[Amount]"), true),
            ("a union", alone("(Sheet1!$A$1,Sheet1!$B$2)"), true),
            ("a spill reference", alone("Sheet1!$A$1#"), true),
            ("INDIRECT", alone("INDIRECT(\"Sheet1!A1\")"), true),
            ("IF over literals", alone("IF(TRUE,1,2)"), true),
            ("an invoked LAMBDA", alone("LAMBDA(cell,cell)(Sheet1!$A$1)"), true),
            ("a name defined as text, which a caller can bind to a reference", with("other_name", &[("other_name", "\"text\"")]), true),
            ("a name joined to text", with("other_name&\"\"", &[("other_name", "Sheet1!$A$1")]), false),
            ("a name that can", with("other_name", &[("other_name", "Sheet1!$A$1")]), true),
            ("a name in another case defined as text, which a caller can bind", with("OTHER_NAME", &[("other_name", "\"text\"")]), true),
            ("a prefixed name defined as text, which a caller can bind as written", with("Sheet1!other_name", &[("other_name", "\"text\"")]), true),
            ("a name in another case with a sheet prefix that can", with("Sheet1!OTHER_NAME", &[("other_name", "Sheet1!$A$1")]), true),
            ("a name defined twice, one that can", with("other_name", &[("other_name", "5"), ("other_name", "Sheet1!$A$1")]), true),
            ("a name not defined", alone("missing_name"), true),
            ("a called LAMBDA name", with("picker(1)", &[("picker", "LAMBDA(row,INDEX(Sheet1!$A:$A,row))")]), true),
            ("a called name defined as text, which a caller can bind to a LAMBDA", with("picker(1)", &[("picker", "\"text\"")]), true),
            ("a cycle", with("loop_first", &[("loop_first", "loop_second"), ("loop_second", "loop_first")]), true),
            ("eight names away, text, through a first name a caller can bind", chain(8, "\"text\""), true),
            ("eight names away, a reference", chain(8, "Sheet1!$A$1"), true),
            ("nine names away, text", chain(9, "\"text\""), true),
            ("nine names away, a reference", chain(9, "Sheet1!$A$1"), true),
        ];
        let affected: HashSet<String> = ["changed_name".to_string()].into_iter().collect();
        let wrong: Vec<&str> = cases
            .iter()
            .filter(|(_, names, may)| super::may_be_references(&names.iter().collect::<Vec<_>>(), &affected) != *may)
            .map(|(label, ..)| *label)
            .collect();
        assert!(wrong.is_empty(), "judged wrongly: {wrong:?}");
    }
}
