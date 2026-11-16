//! What recalculates when a defined name changes without a structural edit.
//!
//! A formula reading a name depends on what the name resolves to (its range, or the references
//! in its LAMBDA body), and the dependency graph builds those edges from the definitions. The
//! name commands are structural edits and recalculate everything; the hidden names that store
//! solver models, scenarios, custom views, scripts and the watch list are rewritten by their own
//! commands without that, and a cell can still read them. Then the formulas that mention a changed
//! name, directly or through a name whose definition mentions one, are re-resolved and
//! recalculated like edited cells, and so are the dynamic formulas (a formula can reach a name
//! through INDIRECT's text, which mentions no name); in manual mode they are re-resolved only. The
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

use gridcraft_calc::{Calc, Key};
use gridcraft_formula::Expr;
use gridcraft_model::{CalcMode, DefinedName, Workbook};

/// Recalculates after an edit that may have changed defined names: `edited` are the cells whose
/// content changed (empty when only names did), `old_names` the names before the edit. In
/// automatic mode, when the names differ, the readers of the changed names join the edited cells
/// and the calculation runs even when there are none, since it also evaluates the dynamic
/// formulas. In manual mode the readers' values wait for the next calculation, as edited cells'
/// dependents do (F9, Shift+F9 and leaving manual mode, for Automatic or Automatic except tables,
/// rebuild the graph and calculate everything), and only the edited cells are calculated; but the
/// readers are resolved again at once, so the graph follows the new definitions for Trace
/// Precedents and Dependents and for the what-if trials (Goal Seek, Scenario Manager, Solver),
/// which calculate a copy of it in automatic mode. Returns whether a calculation ran that
/// evaluated at least one formula, so a caller that adds no undo step knows to move the revision.
pub(crate) fn recalc_after_name_changes(calc: &mut Calc, wb: &mut Workbook, old_names: &[DefinedName], edited: Vec<Key>) -> bool {
    let manual = wb.calc.mode == CalcMode::Manual;
    let names_differ = wb.names != old_names;
    let names_changed = !manual && names_differ;
    let mut dirty = edited;
    if names_differ {
        let readers = name_readers(wb, &changed_names(old_names, &wb.names));
        if manual {
            calc.resolve_again(wb, &readers);
        } else {
            dirty.extend(readers);
        }
    }
    if dirty.is_empty() && !names_changed {
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
fn changed_names(old: &[DefinedName], new: &[DefinedName]) -> HashSet<String> {
    // Each definition with its place among those of its name and scope. The key borrows the
    // definition's text: a workbook's definitions can hold tens of megabytes, which would
    // otherwise be copied twice for a Watch Window change.
    fn keys(names: &[DefinedName]) -> HashSet<(String, Option<usize>, usize, &str)> {
        let mut places: HashMap<(String, Option<usize>), usize> = HashMap::new();
        names
            .iter()
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
/// are followed through an index of which names mention which, each name at most once.
fn name_readers(wb: &Workbook, changed: &HashSet<String>) -> Vec<Key> {
    if changed.is_empty() {
        return Vec::new();
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
    let mut affected = changed.clone();
    // The names found affected since the definitions were last screened.
    let mut fresh = changed.clone();
    while !fresh.is_empty() {
        let screen = affected.len() <= MAX_SCREENED_NAMES;
        for (n, done) in wb.names.iter().zip(parsed.iter_mut()) {
            if *done || (screen && !text_may_mention(&n.formula, &fresh)) {
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
                    return every_formula(wb);
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
    out
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
    // A name longer than the text can't be in it, and needs no copy of the text.
    if names.iter().all(|n| n.len() > text.len()) {
        return false;
    }
    // The text is lower-cased once and each name found with `str::contains`, whose search takes
    // time linear in the text. Comparing the name at each place its first letter matches would
    // cost the text's length times the name's on a text that nearly matches everywhere (a run of
    // one letter, names of that letter ending in another), minutes for one Watch Window change.
    // The copy is one formula's text, dropped before the next formula is screened.
    let lower = text.to_ascii_lowercase();
    names.iter().any(|n| n.len() <= lower.len() && lower.contains(n.as_str()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use gridcraft_core::{CellRef, Value};
    use serde_json::json;

    use super::{DEFINITIONS_PARSED, INDEX_BUDGET, MAX_INDEX_BYTES};
    use crate::Session;

    /// A word for `k` made of letters only (`aa`, `ab`, …), for names that must not read as cells.
    fn letters(k: usize) -> String {
        let (high, low) = (k / 26 % 26, k % 26);
        [high, low].iter().map(|&i| char::from(b'a' + u8::try_from(i).unwrap())).collect()
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
            wb.names.push(gridcraft_model::DefinedName {
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
        let readers = super::name_readers(&wb, &changed);
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
        }
    }
}
