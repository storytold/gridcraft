//! Cells.

use std::sync::Arc;

use gridcraft_core::{RangeRef, Value};
use gridcraft_formula::Expr;
use serde::{Deserialize, Serialize};

use crate::style::StyleId;

/// A parsed formula. `text` is the canonical formula text without the leading `=`.
///
/// Formulas copied or filled from one formula share its parsed expression (Excel's shared
/// formulas): `expr` is the expression as parsed at one cell, and `offset` is how far this
/// formula's cell is from that one; the relative parts of its references are moved by it.
/// [`Formula::expr`] gives the expression as it reads at this cell; the calculation engine reads
/// the shared one and the offset ([`Formula::parsed`]) and moves references as it resolves them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Formula {
    pub text: String,
    #[serde(skip)]
    expr: Option<Arc<Expr>>,
    /// (rows, columns) from the cell `expr` was parsed at.
    #[serde(skip)]
    offset: (i32, i32),
    /// Legacy array formula (Ctrl+Shift+Enter) covering this range; the formula lives in the
    /// top-left cell.
    pub array: Option<RangeRef>,
}

/// The text says what a formula is at its cell; whether its expression is shared doesn't matter.
impl PartialEq for Formula {
    fn eq(&self, other: &Formula) -> bool {
        self.text == other.text && self.array == other.array
    }
}

impl Formula {
    /// Parses `text` (with or without `=`). Unparseable formulas keep their text and evaluate to
    /// `#NAME?`.
    pub fn new(text: &str) -> Formula {
        let body = text.strip_prefix('=').unwrap_or(text);
        match gridcraft_formula::parse(body) {
            Ok(e) => Formula::from_expr(e),
            Err(_) => Formula { text: body.to_string(), expr: None, offset: (0, 0), array: None },
        }
    }
    pub fn from_expr(e: Expr) -> Formula {
        Formula { text: gridcraft_formula::print(&e), expr: Some(Arc::new(e)), offset: (0, 0), array: None }
    }
    /// This formula copied to a cell `dr` rows and `dc` columns away (copy, fill): it shares the
    /// parsed expression, and only its text is worked out. References pushed off the sheet
    /// become `#REF!`, as Excel does. Not an array formula.
    pub fn moved(&self, dr: i64, dc: i64) -> Formula {
        let Some(expr) = self.expr.clone().or_else(|| gridcraft_formula::parse(&self.text).ok().map(Arc::new)) else {
            return Formula { array: None, ..self.clone() };
        };
        let (base_r, base_c) = if self.expr.is_some() { self.offset } else { (0, 0) };
        let (r, c) = (i64::from(base_r).saturating_add(dr), i64::from(base_c).saturating_add(dc));
        let (Ok(r32), Ok(c32)) = (i32::try_from(r), i32::try_from(c)) else {
            // Further than any sheet: parse the moved formula on its own.
            return Formula::from_expr(gridcraft_formula::adjust::shift_relative((*expr).clone(), r, c));
        };
        Formula { text: gridcraft_formula::print_shifted(&expr, r, c), expr: Some(expr), offset: (r32, c32), array: None }
    }
    /// [`Formula::moved`] when the caller already has the copy's text (what `moved` would print):
    /// a file reader that just compared it. `None` when the expression isn't parsed here or the
    /// offset is beyond any sheet.
    pub fn moved_with_text(&self, dr: i64, dc: i64, text: String) -> Option<Formula> {
        let expr = Arc::clone(self.expr.as_ref()?);
        let r = i32::try_from(i64::from(self.offset.0).checked_add(dr)?).ok()?;
        let c = i32::try_from(i64::from(self.offset.1).checked_add(dc)?).ok()?;
        Some(Formula { text, expr: Some(expr), offset: (r, c), array: None })
    }
    /// The parsed expression shared with the formulas copied from it, and this formula's offset
    /// from it (re-parsing after deserialization, at offset 0).
    pub fn parsed(&self) -> Option<(Arc<Expr>, (i32, i32))> {
        match &self.expr {
            Some(e) => Some((Arc::clone(e), self.offset)),
            None => gridcraft_formula::parse(&self.text).ok().map(|e| (Arc::new(e), (0, 0))),
        }
    }
    /// [`Formula::parsed`] without touching the shared expression's reference count (calculation
    /// threads read it for every formula of a filled block); `None` when it must be re-parsed.
    pub fn parsed_ref(&self) -> Option<(&Expr, (i32, i32))> {
        self.expr.as_deref().map(|e| (e, self.offset))
    }
    /// The parsed expression as it reads at this formula's cell.
    pub fn expr(&self) -> Option<Expr> {
        let (e, (r, c)) = self.parsed()?;
        Some(if (r, c) == (0, 0) { (*e).clone() } else { gridcraft_formula::adjust::shift_relative((*e).clone(), r.into(), c.into()) })
    }
    /// [`Formula::expr`] without copying it when it isn't shared at an offset.
    pub fn expr_arc(&self) -> Option<Arc<Expr>> {
        let (e, (r, c)) = self.parsed()?;
        Some(if (r, c) == (0, 0) { e } else { Arc::new(gridcraft_formula::adjust::shift_relative((*e).clone(), r.into(), c.into())) })
    }
}

/// Shares formulas that are the same relative to their cells (`=A1*2` in B1 and `=A2*2` in B2):
/// the first of each kind is kept, the others become copies of it ([`Formula::moved`]). For
/// formulas made one by one: rewritten after a structural edit, or loaded from a file that
/// doesn't mark its shared formulas.
#[derive(Debug, Default)]
pub struct FormulaSharer {
    /// R1C1 text (the same for formulas that are the same relative to their cells) → the first
    /// such formula and where it is.
    first: std::collections::HashMap<usize, std::collections::HashMap<String, (gridcraft_core::CellRef, Arc<Formula>)>>,
}

impl FormulaSharer {
    /// `f`, at `at` on sheet `sheet`, sharing the expression of an earlier formula of the same
    /// kind on the same sheet. Its text is kept.
    pub fn share(&mut self, sheet: usize, at: gridcraft_core::CellRef, f: Formula) -> Arc<Formula> {
        let key = FormulaSharer::key(at, &f);
        self.share_keyed(sheet, at, f, key)
    }

    /// What formulas that are the same relative to their cells have in common (`None`: not
    /// shared, e.g. array formulas). Pure, so callers can work it out for many formulas at once
    /// and then [`FormulaSharer::share_keyed`] them in order.
    pub fn key(at: gridcraft_core::CellRef, f: &Formula) -> Option<String> {
        if f.array.is_some() {
            return None;
        }
        let e = f.expr_arc()?;
        Some(gridcraft_formula::print_r1c1(&e, at))
    }

    /// For a formula already in a cell: the shared copy to put there instead, or `None` when the
    /// cell keeps its own (the first of its kind with its expression parsed, or not shareable).
    /// Nothing is copied for formulas that stay.
    pub fn share_existing(&mut self, sheet: usize, at: gridcraft_core::CellRef, f: &Arc<Formula>, key: Option<String>) -> Option<Arc<Formula>> {
        let key = key?;
        let firsts = self.first.entry(sheet).or_default();
        match firsts.get(key.as_str()) {
            Some((origin, first)) => {
                let shared = first.expr.as_ref()?;
                let (dr, dc) =
                    (i32::try_from(i64::from(at.row) - i64::from(origin.row)).ok()?, i32::try_from(i64::from(at.col) - i64::from(origin.col)).ok()?);
                let offset = (first.offset.0.saturating_add(dr), first.offset.1.saturating_add(dc));
                if f.expr.as_ref().is_some_and(|e| Arc::ptr_eq(e, shared)) && f.offset == offset {
                    return None; // Already a copy of it.
                }
                Some(Arc::new(Formula { text: f.text.clone(), expr: Some(Arc::clone(shared)), offset, array: None }))
            }
            None => {
                if f.expr.is_some() {
                    firsts.insert(key, (at, Arc::clone(f)));
                    None
                } else {
                    // Read from JSON: parse it once, here.
                    let mut parsed = Formula::new(&f.text);
                    parsed.array = f.array;
                    let parsed = Arc::new(parsed);
                    firsts.insert(key, (at, Arc::clone(&parsed)));
                    Some(parsed)
                }
            }
        }
    }

    /// [`FormulaSharer::share`] with the key worked out already.
    pub fn share_keyed(&mut self, sheet: usize, at: gridcraft_core::CellRef, f: Formula, key: Option<String>) -> Arc<Formula> {
        let Some(key) = key else { return Arc::new(f) };
        let firsts = self.first.entry(sheet).or_default();
        // A formula read from JSON has its text only: parse it once here, so the formulas sharing
        // it (and it) aren't parsed again on every recalculation.
        let f = if f.expr.is_none() {
            let mut parsed = Formula::new(&f.text);
            parsed.array = f.array;
            parsed
        } else {
            f
        };
        match firsts.get(key.as_str()) {
            Some((origin, first)) => match (
                &first.expr,
                i32::try_from(i64::from(at.row) - i64::from(origin.row)),
                i32::try_from(i64::from(at.col) - i64::from(origin.col)),
            ) {
                (Some(shared), Ok(dr), Ok(dc)) => {
                    let (r, c) = (first.offset.0.saturating_add(dr), first.offset.1.saturating_add(dc));
                    Arc::new(Formula { text: f.text, expr: Some(Arc::clone(shared)), offset: (r, c), array: None })
                }
                _ => Arc::new(f),
            },
            None => {
                let f = Arc::new(f);
                firsts.insert(key, (at, Arc::clone(&f)));
                f
            }
        }
    }
}

/// A static picture stored as cell content, independent of floating drawing anchors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellPicture {
    /// Encoded PNG/JPEG bytes, base64 in JSON. Sheet entries share the payload through Arc.
    #[serde(with = "crate::b64")]
    pub data: Vec<u8>,
    pub mime: String,
    pub alt: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// Constant value, or the cached result of the formula.
    pub value: Value,
    pub formula: Option<Arc<Formula>>,
    pub style: StyleId,
}

impl Cell {
    pub fn value(v: Value) -> Cell {
        Cell { value: v, formula: None, style: StyleId::DEFAULT }
    }
    pub fn formula(f: Formula) -> Cell {
        Cell { value: Value::Empty, formula: Some(Arc::new(f)), style: StyleId::DEFAULT }
    }
    /// No value, no formula and the default style: storing it is pointless.
    pub fn is_blank(&self) -> bool {
        self.value.is_empty() && self.formula.is_none() && self.style == StyleId::DEFAULT
    }
    /// What the formula bar shows.
    pub fn input_text(&self) -> String {
        if let Some(f) = &self.formula {
            return format!("={}", f.text);
        }
        match &self.value {
            Value::Text(t) => {
                // Text that would parse as something else is shown with its apostrophe by Excel
                // only when entered that way; we show it plainly.
                t.to_string()
            }
            v => v.display(),
        }
    }
    /// What the formula bar shows when not editing: [`input_text`](Self::input_text), with a
    /// legacy array formula in braces (`{=A1:A2*2}`).
    pub fn bar_text(&self) -> String {
        match &self.formula {
            Some(f) if f.array.is_some() => format!("{{={}}}", f.text),
            _ => self.input_text(),
        }
    }
}
