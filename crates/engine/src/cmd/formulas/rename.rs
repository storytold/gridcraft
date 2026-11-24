//! Renaming a defined name (Name Manager › Edit…) without changing what any formula means.
//!
//! Every reference the workbook holds is visited once, by the same walk every reference-moving
//! edit uses ([`Workbook::rewrite_references`], plus Python cells' `xl()` references), and each is
//! read twice with [`gridcraft_calc::resolve`], the rules the evaluator reads names by: with the
//! names as they are, and with the edited name. The uses that read the edited name are renamed,
//! and the edit is made only if afterwards every use in every formula reads exactly what it read
//! before. Otherwise it is refused and nothing changes. A rename can therefore never send a use to
//! another name, a LET or LAMBDA variable, a table or a function, nor make another use read the
//! renamed name. Names built from text while calculating (`INDIRECT("Old")`) can't be followed.

use std::cell::RefCell;
use std::collections::BTreeSet;

use gridcraft_calc::resolve::{self, Site, Use};
use gridcraft_formula::Expr;
use gridcraft_model::{DefinedName, RefPlace, Workbook};

use crate::{EngineError, Result};

/// A reference `wb` holds: its tree, its holder (`None` for Python code) and the sheet it is read
/// on (a defined name's scope).
type Reference = (Expr, Option<RefPlace>, Option<usize>);

/// Every reference `wb` holds. Visited, not changed.
fn references(wb: &Workbook) -> Result<Vec<Reference>> {
    let seen: RefCell<Vec<Reference>> = RefCell::new(Vec::new());
    let mut copy = wb.clone();
    copy.rewrite_references(&|host, e| {
        seen.borrow_mut().push((e.clone(), Some(host.place), host.sheet));
        e
    });
    let sheet_of = |name: &str| wb.sheets.iter().position(|s| s.name == name);
    crate::cmd::edit::rewrite_python_refs(&mut copy, &|host, e| {
        seen.borrow_mut().push((e.clone(), None, sheet_of(host)));
        e
    })?;
    Ok(seen.into_inner())
}

/// The sheets each workbook-level name's definition is read on: those whose formulas (or
/// names read there) use it. The evaluator reads a workbook-level name's definition on the sheet
/// that uses it, and a sheet-level one's on its own sheet. A workbook-level name nothing uses is
/// checked on every sheet, as any sheet may come to use it.
fn name_hosts(wb: &Workbook, renamed: usize) -> Result<Vec<Vec<usize>>> {
    let mut hosts: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); wb.names.len()];
    let workbook_def = |d: usize| wb.names.get(d).filter(|n| n.scope.is_none()).and_then(|n| gridcraft_formula::parse_name(&n.formula).ok());
    let mut work: Vec<(Expr, usize)> = Vec::new();
    for (e, place, sheet) in references(wb)? {
        let Some(si) = sheet else { continue };
        match place {
            // A workbook-level name's own definition is read where it is used: reached below.
            Some(RefPlace::Name(i)) if i == renamed => {}
            // A PivotTable reads its source by its own rules (a table first, then a range, then a
            // name): the name it reads is read on its sheet.
            Some(RefPlace::Pivot(pi)) => {
                let Some(pt) = wb.sheet(si).and_then(|sh| sh.pivots.get(pi)) else { continue };
                if let Some(d) = crate::pivot::source_name(wb, &pt.source, si).filter(|&d| d != renamed)
                    && let Some(def) = workbook_def(d)
                    && hosts.get_mut(d).is_some_and(|set| set.insert(si))
                {
                    work.push((def, si));
                }
            }
            _ => work.push((e, si)),
        }
    }
    // Each (name, sheet) is added once, so this ends after at most names × sheets steps.
    while let Some((e, h)) = work.pop() {
        for u in resolve::uses(wb, h, &e) {
            let Use::Defined(d) = u else { continue };
            if d == renamed {
                continue;
            }
            if let Some(def) = workbook_def(d)
                && hosts.get_mut(d).is_some_and(|set| set.insert(h))
            {
                work.push((def, h));
            }
        }
    }
    let all: Vec<usize> = (0..wb.sheets.len()).collect();
    Ok(wb
        .names
        .iter()
        .zip(hosts)
        .map(|(n, set)| match n.scope {
            Some(s) => vec![s],
            None if set.is_empty() => all.clone(),
            None => set.into_iter().collect(),
        })
        .collect())
}

struct Renamer<'a> {
    before: &'a Workbook,
    after: &'a Workbook,
    index: usize,
    new: &'a str,
}

impl Renamer<'_> {
    /// `e`, read on each of `hosts`, with every use of the renamed name renamed: `Ok(None)` when
    /// it has none. `Err(())` when, renamed, some use would read something other than before.
    fn rewrite(&self, e: &Expr, hosts: &[usize]) -> std::result::Result<Option<Expr>, ()> {
        let before: Vec<Vec<Use>> = hosts.iter().map(|&h| resolve::uses(self.before, h, e)).collect();
        let renamed = |k: usize| before.iter().any(|b| b.get(k) == Some(&Use::Defined(self.index)));
        let mut out = e.clone();
        let mut any = false;
        let mut k = 0usize;
        resolve::visit(&mut out, &mut |site, _| {
            if renamed(k) {
                any = true;
                match site {
                    Site::Name(n) => {
                        *n = match n.rsplit_once('!') {
                            Some((sheet, _)) => format!("{sheet}!{}", self.new),
                            None => self.new.to_string(),
                        }
                    }
                    Site::Call(n) => *n = self.new.to_string(),
                }
            }
            k += 1;
        });
        // Every use, renamed or not, must read what it read before, on every sheet it is read on.
        if hosts.iter().zip(&before).any(|(&h, b)| resolve::uses(self.after, h, &out) != *b) {
            return Err(());
        }
        Ok(any.then_some(out))
    }
}

/// Renames the defined name at `index` in `wb` to `def` (its new name, scope and definition),
/// with every formula that uses it: the workbook as it is after, or the reason the rename is
/// refused. The renamed name keeps its place in the list.
/// Renames the defined name at `index` in `wb` to `def` (its new name, scope and definition),
/// with every formula that uses it: the workbook as it is after, or the reason the rename is
/// refused. The renamed name keeps its place in the list.
pub(super) fn rename(wb: &Workbook, index: usize, def: DefinedName) -> Result<Workbook> {
    let new = def.name.clone();
    // The names as they are after the edit, for reading uses.
    let mut after = wb.clone();
    match after.names.get_mut(index) {
        Some(n) => *n = def.clone(),
        None => return Err(EngineError::Other("no such name to edit".into())),
    }
    let hosts = name_hosts(wb, index)?;
    let r = Renamer { before: wb, after: &after, index, new: &new };
    let sheet_name = |si: usize| wb.sheet(si).map(|s| gridcraft_formula::quote_sheet(&s.name)).unwrap_or_default();
    // The first place that would read something else, named as the refusal says it.
    let refused: RefCell<Option<String>> = RefCell::new(None);
    let refuse_at = |place: String| {
        refused.borrow_mut().get_or_insert(place);
    };
    let follow = |e: Expr, hosts: &[usize], place: &dyn Fn() -> String| match r.rewrite(&e, hosts) {
        Ok(renamed) => renamed.unwrap_or(e),
        Err(()) => {
            refuse_at(place());
            e
        }
    };

    // Visited while the old name is still defined, so a chart series named by it is still read
    // as a reference; the edited definition goes in afterwards.
    let mut out = wb.clone();
    out.rewrite_references(&|host, e| {
        let si = host.sheet.unwrap_or_default();
        match host.place {
            // The renamed name's own definition is the one the edit gives.
            RefPlace::Name(i) if i == index => e,
            RefPlace::Name(i) => {
                let label = || wb.names.get(i).map(|n| format!("the name {}", n.name)).unwrap_or_default();
                follow(e, hosts.get(i).map_or(&[][..], Vec::as_slice), &label)
            }
            // A PivotTable reads its source by its own rules (a table first, then a range, then a
            // name), not a formula's: the source must read the same before and after.
            RefPlace::Pivot(pi) => {
                let Some(pt) = wb.sheet(si).and_then(|sh| sh.pivots.get(pi)) else { return e };
                let read = crate::pivot::source_name(wb, &pt.source, si);
                let renamed = read == Some(index);
                let text = if renamed { new.clone() } else { pt.source.trim_start_matches('=').to_string() };
                if crate::pivot::source_name(&after, &text, si) != read {
                    refuse_at(format!("the source of the PivotTable {} on {}", pt.name, sheet_name(si)));
                    return e;
                }
                if renamed { Expr::Name(new.clone()) } else { e }
            }
            RefPlace::Cell(c) => follow(e, &[si], &|| format!("the formula in {}!{}", sheet_name(si), c.a1())),
            RefPlace::CondFormat(_) => follow(e, &[si], &|| format!("a conditional formatting rule on {}", sheet_name(si))),
            RefPlace::Validation(_) => follow(e, &[si], &|| format!("a data validation rule on {}", sheet_name(si))),
            RefPlace::Chart(_) => follow(e, &[si], &|| format!("a chart on {}", sheet_name(si))),
            RefPlace::Sparkline(_) => follow(e, &[si], &|| format!("a sparkline on {}", sheet_name(si))),
            RefPlace::Control(_) => follow(e, &[si], &|| format!("a form control on {}", sheet_name(si))),
        }
    });
    let sheet_of = |name: &str| wb.sheets.iter().position(|s| s.name == name);
    crate::cmd::edit::rewrite_python_refs(&mut out, &|host, e| match sheet_of(host) {
        Some(si) => follow(e, &[si], &|| format!("the Python code on {}", sheet_name(si))),
        None => e,
    })?;
    if let Some(place) = refused.into_inner() {
        return Err(EngineError::Other(format!(
            "The name can't be changed to {new}: {place} would then read something else (another name, a LET or LAMBDA variable, a table or a function), or stop finding it."
        )));
    }
    if let Some(slot) = out.names.get_mut(index) {
        *slot = def;
    }
    Ok(out)
}
