//! A cross-sheet cut qualifies native A1 references. Names, named LAMBDA calls, relative
//! names and structured references have other lookup rules; don't silently change them.
use super::{Request, Result, TRANSFER, Workbook, bad};
use gridcraft_formula::{Expr, SheetSel, StructItem};
use std::collections::BTreeSet;

struct Names<'a> {
    wb: &'a Workbook,
    checked: BTreeSet<(Option<usize>, usize, usize, String)>,
    shifted: bool,
}
impl Names<'_> {
    fn expression(&mut self, e: &Expr, from: usize, to: usize, definition: bool, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(bad(TRANSFER, "A defined name is nested too deeply to verify its binding."));
        }
        let mut result = Ok(());
        e.walk(&mut |e| {
            if result.is_err() {
                return;
            }
            result = match e {
                Expr::Name(n) => self.name(n, from, to, depth),
                Expr::Call(n, _) if from != to && (n.eq_ignore_ascii_case("INDIRECT") || n.eq_ignore_ascii_case("SHEET")) => {
                    Err(bad(TRANSFER, "This formula resolves its worksheet at evaluation time; it cannot safely move between sheets."))
                }
                Expr::Call(n, _) => self.name(n, from, to, depth), // named LAMBDAs resolve in the same scope
                Expr::Struct(s) if s.table.is_empty() || s.specifiers.contains(&StructItem::ThisRow) && self.shifted => {
                    Err(bad(TRANSFER, "This structured reference depends on the formula's table or row."))
                }
                Expr::Ref(r)
                    if definition
                        && ((from != to && r.sheet == SheetSel::Current) || (self.shifted && gridcraft_formula::adjust::has_relative_refs(e))) =>
                {
                    Err(bad(TRANSFER, "A defined name's reference is relative to its caller's sheet or cell."))
                }
                _ => Ok(()),
            };
        });
        result
    }
    fn name(&mut self, n: &str, from: usize, to: usize, depth: usize) -> Result<()> {
        // Mirrors calc::eval::name: qualified names resolve on the named sheet, but a
        // workbook-scoped definition still evaluates in the formula's calling sheet.
        let (a, b, bare) = if let Some((sheet, name)) = n.rsplit_once('!') {
            let Some(i) = self.wb.sheet_index(sheet) else { return Ok(()) };
            (i, i, name)
        } else {
            (from, to, n)
        };
        let before = self.wb.name(bare, a);
        let after = self.wb.name(bare, b);
        match (before, after) {
            (None, None) => Ok(()), // a built-in, lexical binding, table name, or already unbound
            (Some(l), Some(r)) if std::ptr::eq(l, r) => {
                let src = l.scope.unwrap_or(from);
                let dst = l.scope.unwrap_or(to);
                if !self.checked.insert((l.scope, src, dst, l.name.to_ascii_lowercase())) {
                    return Ok(());
                }
                let expr = gridcraft_formula::parse_name(&l.formula).map_err(|_| bad(TRANSFER, "Cannot verify a defined-name formula."))?;
                self.expression(&expr, src, dst, true, depth + 1)
            }
            _ => Err(bad(TRANSFER, "A defined name resolves differently on the destination sheet. Qualify it or use a stable workbook name.")),
        }
    }
}

pub(super) fn guard(wb: &Workbook, v: &Request) -> Result<()> {
    let sh = &wb.sheets[v.from];
    let mut names = Names { wb, checked: BTreeSet::new(), shifted: v.src.start != v.at };
    for (_, cell) in sh.cells.iter_range(v.src) {
        if v.from != v.to && cell.py.is_some() {
            return Err(bad(TRANSFER, "Python cells can resolve workbook names at runtime and cannot safely move between sheets."));
        }
        if let Some(f) = &cell.formula {
            f.with_expr(|e| names.expression(e, v.from, v.to, false, 0))
                .ok_or_else(|| bad(TRANSFER, "An unsupported formula cannot move between sheets."))??;
        }
    }
    // Validation and CF rules travel with their cells and native cut qualifies only A1 nodes.
    for f in sh
        .validations
        .iter()
        .filter(|r| r.ranges.iter().any(|a| a.intersects(&v.src)))
        .flat_map(|r| r.formulas())
        .chain(sh.cond_formats.iter().filter(|r| r.ranges.iter().any(|a| a.intersects(&v.src))).flat_map(|r| r.rule.formulas()))
    {
        if !f.is_empty() {
            let e = gridcraft_formula::parse(f.trim_start_matches('=')).map_err(|_| bad(TRANSFER, "Cannot verify a transferred rule formula."))?;
            names.expression(&e, v.from, v.to, false, 0)?;
        }
    }
    Ok(())
}

pub(super) fn guard_callers(wb: &Workbook, v: &Request) -> Result<()> {
    // A single rewritten definition cannot preserve the meanings at multiple callers of a
    // relative name. This applies even when none of those callers is in the moved block.
    // Global unqualified absolute references also lack one unambiguous host sheet.
    for n in &wb.names {
        let Ok(e) = gridcraft_formula::parse_name(&n.formula) else { continue };
        let mut ambiguous = false;
        e.walk(&mut |e| {
            if let Expr::Ref(r) = e {
                let on_source = match &r.sheet {
                    SheetSel::Current => n.scope.is_none_or(|i| i == v.from),
                    SheetSel::Named(sheet) => wb.sheet_index(sheet) == Some(v.from),
                    SheetSel::Span(first, last) => {
                        wb.sheet_index(first).zip(wb.sheet_index(last)).is_some_and(|(a, b)| (a.min(b)..=a.max(b)).contains(&v.from))
                    }
                };
                ambiguous |= on_source
                    && (gridcraft_formula::adjust::has_relative_refs(e)
                        || (n.scope.is_none() && r.sheet == SheetSel::Current && r.range().intersects(&v.src)));
            }
        });
        if ambiguous {
            return Err(bad(TRANSFER, "A defined name has a caller-relative reference that may read the moved cells."));
        }
    }
    Ok(())
}
