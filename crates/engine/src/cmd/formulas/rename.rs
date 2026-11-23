//! Renaming a defined name (Name Manager › Edit…) without changing what any formula means.
//!
//! Every place a formula lives is read twice with [`gridcraft_calc::resolve`], the rules the
//! evaluator reads names by: with the names as they are, and with the edited name. The uses that
//! read the edited name are renamed, and the edit is made only if afterwards every use in every
//! formula reads exactly what it read before. Otherwise it is refused and nothing changes. A
//! rename can therefore never send a use to another name, a LET or LAMBDA variable, a table or
//! a function, nor make another use read the renamed name. Names built from text while
//! calculating (`INDIRECT("Old")`) can't be followed.

use std::collections::BTreeSet;

use gridcraft_calc::resolve::{self, Site, Use};
use gridcraft_core::CellRef;
use gridcraft_formula::Expr;
use gridcraft_model::{CfRule, CfValueKind, DefinedName, Formula, ValidationKind, Workbook};

use crate::{EngineError, Result};

/// A place that holds a formula, other than a cell, a name, a PivotTable source or Python code.
#[derive(Clone, Copy)]
enum Holder {
    /// A conditional format's formula: (sheet, rule, slot in [`rule_formulas`] order).
    Cf(usize, usize, usize),
    /// A validation's first (`false`) or second formula: (sheet, rule).
    Dv(usize, usize, bool),
    /// A chart's source block: (sheet, chart).
    Source(usize, usize),
    /// A chart series' values, categories, name or bubble sizes (0–3): (sheet, chart, series).
    Series(usize, usize, usize, u8),
    /// A form control's linked cell: (sheet, control).
    Control(usize, usize),
    /// A sparkline's source: (sheet, sparkline).
    Sparkline(usize, usize),
}

impl Holder {
    fn sheet(self) -> usize {
        match self {
            Holder::Cf(s, ..)
            | Holder::Dv(s, ..)
            | Holder::Source(s, _)
            | Holder::Series(s, ..)
            | Holder::Control(s, _)
            | Holder::Sparkline(s, _) => s,
        }
    }
    fn what(self) -> &'static str {
        match self {
            Holder::Cf(..) => "a conditional formatting rule",
            Holder::Dv(..) => "a data validation rule",
            Holder::Source(..) | Holder::Series(..) => "a chart",
            Holder::Control(..) => "a form control",
            Holder::Sparkline(..) => "a sparkline",
        }
    }
}

/// A conditional format's formulas, in a fixed order.
fn rule_formulas(rule: &mut CfRule) -> Vec<&mut String> {
    fn value(v: &mut CfValueKind) -> Option<&mut String> {
        match v {
            CfValueKind::Formula(f) => Some(f),
            _ => None,
        }
    }
    match rule {
        CfRule::CellIs { a, b, .. } => std::iter::once(a).chain(b.as_mut()).collect(),
        CfRule::Expression { formula, .. } => vec![formula],
        CfRule::ColorScale { stops } => stops.iter_mut().filter_map(|(v, _)| value(v)).collect(),
        CfRule::DataBar { min, max, .. } => [min, max].into_iter().filter_map(value).collect(),
        CfRule::IconSet { thresholds, .. } => thresholds.iter_mut().filter_map(value).collect(),
        _ => Vec::new(),
    }
}

fn text_mut(wb: &mut Workbook, h: Holder) -> Option<&mut String> {
    let sh = wb.sheet_mut(h.sheet())?;
    match h {
        Holder::Cf(_, r, k) => sh.cond_formats.get_mut(r).and_then(|cf| rule_formulas(&mut cf.rule).into_iter().nth(k)),
        Holder::Dv(_, r, second) => sh.validations.get_mut(r).and_then(|dv| if second { dv.f2.as_mut() } else { Some(&mut dv.f1) }),
        Holder::Source(_, c) => sh.charts.get_mut(c).and_then(|ch| ch.source.as_mut()),
        Holder::Series(_, c, i, field) => sh.charts.get_mut(c).and_then(|ch| ch.series.get_mut(i)).and_then(|se| match field {
            0 => Some(&mut se.values),
            1 => se.categories.as_mut(),
            2 => se.name.as_mut(),
            _ => se.bubble_sizes.as_mut(),
        }),
        Holder::Control(_, c) => sh.controls.get_mut(c).and_then(|c| c.link.as_mut()),
        Holder::Sparkline(_, i) => sh.sparklines.get_mut(i).map(|sp| &mut sp.source),
    }
}

/// Every formula a [`Holder`] holds in `wb`, with its text.
fn holders(wb: &Workbook) -> Vec<(Holder, String)> {
    let mut out = Vec::new();
    for (si, sh) in wb.sheets.iter().enumerate() {
        for (r, cf) in sh.cond_formats.iter().enumerate() {
            let mut rule = cf.rule.clone();
            out.extend(rule_formulas(&mut rule).into_iter().enumerate().map(|(k, f)| (Holder::Cf(si, r, k), f.clone())));
        }
        for (r, dv) in sh.validations.iter().enumerate() {
            // A literal list (`"a,b"`, `Open,Closed`) is no formula.
            let formula = match dv.kind {
                ValidationKind::Any => false,
                ValidationKind::List => !crate::cmd::data::is_literal_list(dv.f1.trim()),
                _ => true,
            };
            if formula {
                out.push((Holder::Dv(si, r, false), dv.f1.clone()));
            }
            if let Some(f2) = &dv.f2 {
                out.push((Holder::Dv(si, r, true), f2.clone()));
            }
        }
        for (c, ch) in sh.charts.iter().enumerate() {
            out.extend(ch.source.iter().map(|t| (Holder::Source(si, c), t.clone())));
            for (i, se) in ch.series.iter().enumerate() {
                out.push((Holder::Series(si, c, i, 0), se.values.clone()));
                for (field, t) in [(1, &se.categories), (2, &se.name), (3, &se.bubble_sizes)] {
                    out.extend(t.iter().map(|t| (Holder::Series(si, c, i, field), t.clone())));
                }
            }
        }
        for (i, c) in sh.controls.iter().enumerate() {
            out.extend(c.link.iter().map(|t| (Holder::Control(si, i), t.clone())));
        }
        for (i, sp) in sh.sparklines.iter().enumerate() {
            out.push((Holder::Sparkline(si, i), sp.source.clone()));
        }
    }
    out
}

/// A formula written as text, with or without a leading `=`: its tree, and the `=` to keep.
fn parse_text(t: &str) -> Option<(Expr, &'static str)> {
    let body = t.trim_start_matches('=');
    let eq = if body.len() < t.len() { "=" } else { "" };
    gridcraft_formula::parse(body).ok().map(|e| (e, eq))
}

/// The xl("…") references in Python code.
fn python_refs(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    gridcraft_model::python::rewrite_xl(code, |r| {
        out.push(r.to_string());
        None
    });
    out
}

/// The sheets each workbook-level name's definition is read on: those whose formulas (or
/// names read there) use it. The evaluator reads a workbook-level name's definition on the sheet
/// that uses it, and a sheet-level one's on its own sheet. A workbook-level name nothing uses is
/// checked on every sheet, as any sheet may come to use it.
fn name_hosts(wb: &Workbook, renamed: usize) -> Vec<Vec<usize>> {
    let mut hosts: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); wb.names.len()];
    let mut work: Vec<(Expr, usize)> = Vec::new();
    for (si, sh) in wb.sheets.iter().enumerate() {
        work.extend(sh.cells.iter().filter_map(|(_, c)| c.formula.as_ref()?.expr()).map(|e| (e, si)));
        for py in sh.cells.iter().filter_map(|(_, c)| c.py.as_ref()) {
            work.extend(python_refs(&py.code).iter().filter_map(|r| gridcraft_formula::parse(r).ok()).map(|e| (e, si)));
        }
        for pt in &sh.pivots {
            if let Some(d) = crate::pivot::source_name(wb, &pt.source, si).filter(|&d| d != renamed)
                && let Some(set) = hosts.get_mut(d)
                && let Some(e) = wb.names.get(d).filter(|n| n.scope.is_none()).and_then(|n| gridcraft_formula::parse_name(&n.formula).ok())
                && set.insert(si)
            {
                work.push((e, si));
            }
        }
    }
    work.extend(holders(wb).into_iter().filter_map(|(h, t)| parse_text(&t).map(|(e, _)| (e, h.sheet()))));
    for (i, n) in wb.names.iter().enumerate() {
        if i != renamed
            && let Some(s) = n.scope
            && let Ok(e) = gridcraft_formula::parse_name(&n.formula)
        {
            work.push((e, s));
        }
    }
    // Each (name, sheet) is added once, so this ends after at most names × sheets steps.
    while let Some((e, h)) = work.pop() {
        for u in resolve::uses(wb, h, &e) {
            let Use::Defined(d) = u else { continue };
            if d == renamed || wb.names.get(d).is_none_or(|n| n.scope.is_some()) {
                continue;
            }
            if let Some(set) = hosts.get_mut(d)
                && set.insert(h)
                && let Some(e) = wb.names.get(d).and_then(|n| gridcraft_formula::parse_name(&n.formula).ok())
            {
                work.push((e, h));
            }
        }
    }
    let all: Vec<usize> = (0..wb.sheets.len()).collect();
    wb.names
        .iter()
        .zip(hosts)
        .map(|(n, set)| match n.scope {
            Some(s) => vec![s],
            None if set.is_empty() => all.clone(),
            None => set.into_iter().collect(),
        })
        .collect()
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
pub(super) fn rename(wb: &Workbook, index: usize, def: DefinedName) -> Result<Workbook> {
    let new = def.name.clone();
    let mut after = wb.clone();
    match after.names.get_mut(index) {
        Some(n) => *n = def,
        None => return Err(EngineError::Other("no such name to edit".into())),
    }
    let refuse = |place: String| {
        EngineError::Other(format!(
            "The name can't be changed to {new}: {place} would then read something else (another name, a LET or LAMBDA variable, a table or a function), or stop finding it."
        ))
    };
    let hosts = name_hosts(wb, index);
    let r = Renamer { before: wb, after: &after, index, new: &new };
    let sheet_name = |si: usize| wb.sheet(si).map(|s| gridcraft_formula::quote_sheet(&s.name)).unwrap_or_default();

    let mut cells: Vec<(usize, CellRef, Formula)> = Vec::new();
    let mut texts: Vec<(Holder, String)> = Vec::new();
    let mut names: Vec<(usize, String)> = Vec::new();
    let mut pivots: Vec<(usize, usize, String)> = Vec::new();
    let mut python: Vec<(usize, CellRef, String)> = Vec::new();

    for (si, sh) in wb.sheets.iter().enumerate() {
        for (c, cell) in sh.cells.iter() {
            if let Some(f) = &cell.formula
                && let Some(e) = f.expr()
            {
                match r.rewrite(&e, &[si]) {
                    Ok(Some(e)) => cells.push((si, c, Formula { array: f.array, dynamic: f.dynamic, ..Formula::from_expr(e) })),
                    Ok(None) => {}
                    Err(()) => return Err(refuse(format!("the formula in {}!{}", sheet_name(si), c.a1()))),
                }
            }
            if let Some(py) = &cell.py {
                let mut failed = false;
                let code = gridcraft_model::python::rewrite_xl(&py.code, |reference| {
                    let e = gridcraft_formula::parse(reference).ok()?;
                    match r.rewrite(&e, &[si]) {
                        Ok(e) => e.map(|e| gridcraft_formula::print(&e)),
                        Err(()) => {
                            failed = true;
                            None
                        }
                    }
                });
                if failed {
                    return Err(refuse(format!("the Python code in {}!{}", sheet_name(si), c.a1())));
                }
                if let Some(code) = code {
                    crate::cmd::edit::check_python_fits(
                        &gridcraft_model::PyCode { code: code.clone(), object: py.object, ..Default::default() },
                        &sh.name,
                        c,
                    )?;
                    python.push((si, c, code));
                }
            }
        }
        // A PivotTable reads its source by its own rules (a table first, then a range, then a name).
        for (pi, pt) in sh.pivots.iter().enumerate() {
            let read = crate::pivot::source_name(wb, &pt.source, si);
            let text = if read == Some(index) {
                let eq = if pt.source.trim_start().starts_with('=') { "=" } else { "" };
                format!("{eq}{new}")
            } else {
                pt.source.clone()
            };
            if crate::pivot::source_name(&after, &text, si) != read {
                return Err(refuse(format!("the source of the PivotTable {} on {}", pt.name, sheet_name(si))));
            }
            if text != pt.source {
                pivots.push((si, pi, text));
            }
        }
    }
    for (h, t) in holders(wb) {
        let Some((e, eq)) = parse_text(&t) else { continue };
        match r.rewrite(&e, &[h.sheet()]) {
            Ok(Some(e)) => texts.push((h, format!("{eq}{}", gridcraft_formula::print(&e)))),
            Ok(None) => {}
            Err(()) => return Err(refuse(format!("{} on {}", h.what(), sheet_name(h.sheet())))),
        }
    }
    for (i, n) in wb.names.iter().enumerate() {
        // The renamed name's own definition is the one the edit gives.
        if i == index {
            continue;
        }
        let Ok(e) = gridcraft_formula::parse_name(&n.formula) else { continue };
        match r.rewrite(&e, hosts.get(i).map_or(&[][..], Vec::as_slice)) {
            Ok(Some(e)) => names.push((i, gridcraft_formula::print(&e))),
            Ok(None) => {}
            Err(()) => return Err(refuse(format!("the name {}", n.name))),
        }
    }

    for (si, c, f) in cells {
        if let Some(cell) = after.sheet_mut(si).and_then(|sh| sh.cells.get_mut(c)) {
            cell.formula = Some(std::sync::Arc::new(f));
        }
    }
    for (si, c, code) in python {
        if let Some(py) = after.sheet_mut(si).and_then(|sh| sh.cells.get_mut(c)).and_then(|cell| cell.py.as_mut()) {
            std::sync::Arc::make_mut(py).code = code;
        }
    }
    for (si, pi, text) in pivots {
        if let Some(pt) = after.sheet_mut(si).and_then(|sh| sh.pivots.get_mut(pi)) {
            pt.source = text;
        }
    }
    for (h, t) in texts {
        if let Some(slot) = text_mut(&mut after, h) {
            *slot = t;
        }
    }
    for (i, t) in names {
        if let Some(n) = after.names.get_mut(i) {
            n.formula = t;
        }
    }
    Ok(after)
}
