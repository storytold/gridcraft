//! Cells.

use std::borrow::Cow;
use std::sync::Arc;

use gridcraft_core::{RangeRef, Value};
use gridcraft_formula::Expr;
use serde::{Deserialize, Serialize, Serializer};

use crate::python::PyCode;
use crate::style::StyleId;

/// Shared formula template: a master [`Expr`] shifted by `(dr, dc)` without cloning the AST
/// or eagerly printing the formula text.
#[derive(Clone, Debug)]
struct SharedFormula {
    base: Arc<Expr>,
    dr: i32,
    dc: i32,
}

#[derive(Clone, Debug)]
enum FormulaRepr {
    Owned { text: String, expr: Option<Arc<Expr>> },
    Shared(SharedFormula),
}

/// A parsed formula. Its text is the canonical formula text without the leading `=`.
#[derive(Clone, Debug, Deserialize)]
#[serde(from = "SavedFormula")]
pub struct Formula {
    repr: FormulaRepr,
    /// Legacy array formula (Ctrl+Shift+Enter) covering this range; the formula lives in the
    /// top-left cell.
    pub array: Option<RangeRef>,
    /// Read from a file that marks this formula as a dynamic-array formula (cell metadata `cm`).
    /// The writer keeps the mark, so a formula Excel evaluates as an array is never saved as a
    /// legacy formula (which Excel evaluates with implicit intersection). Formulas entered or
    /// rewritten in GridCraft start without it, and the writer decides from their shape.
    pub dynamic: bool,
}

impl PartialEq for Formula {
    fn eq(&self, other: &Self) -> bool {
        if self.array != other.array || self.dynamic != other.dynamic {
            return false;
        }
        match (&self.repr, &other.repr) {
            (FormulaRepr::Owned { text: a, expr: ea }, FormulaRepr::Owned { text: b, expr: eb }) => a == b && ea == eb,
            (FormulaRepr::Shared(a), FormulaRepr::Shared(b)) if Arc::ptr_eq(&a.base, &b.base) && a.dr == b.dr && a.dc == b.dc => true,
            _ => self.has_cached_expr() == other.has_cached_expr() && self.text() == other.text(),
        }
    }
}

#[derive(Serialize)]
struct BorrowedSavedFormula<'a> {
    text: Cow<'a, str>,
    array: Option<RangeRef>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    dynamic: bool,
}

impl Serialize for Formula {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BorrowedSavedFormula { text: self.text(), array: self.array, dynamic: self.dynamic }.serialize(serializer)
    }
}

impl Formula {
    /// Parses `text` (with or without `=`). Unparseable formulas keep their text and evaluate to
    /// `#NAME?`.
    pub fn new(text: &str) -> Formula {
        let body = text.strip_prefix('=').unwrap_or(text);
        match gridcraft_formula::parse(body) {
            Ok(e) => Formula::from_expr(e),
            Err(_) => Formula { repr: FormulaRepr::Owned { text: body.to_string(), expr: None }, array: None, dynamic: false },
        }
    }
    pub fn from_expr(e: Expr) -> Formula {
        Formula { repr: FormulaRepr::Owned { text: gridcraft_formula::print(&e), expr: Some(Arc::new(e)) }, array: None, dynamic: false }
    }
    /// A formula sharing a parsed master expression `base` shifted by `(dr, dc)` without cloning
    /// the AST or allocating formula text.
    pub fn from_shared(base: Arc<Expr>, dr: i32, dc: i32) -> Formula {
        Formula { repr: FormulaRepr::Shared(SharedFormula { base, dr, dc }), array: None, dynamic: false }
    }
    /// A formula keeping raw `text` verbatim with no cached AST (used for unparsed/raw-text tests).
    pub fn unparsed(text: impl Into<String>) -> Formula {
        Formula { repr: FormulaRepr::Owned { text: text.into(), expr: None }, array: None, dynamic: false }
    }
    /// Replaces the shared master expression while keeping `(dr, dc)` and all flags.
    pub fn with_shared_base(&self, base: Arc<Expr>) -> Option<Formula> {
        match &self.repr {
            FormulaRepr::Shared(s) => {
                Some(Formula { repr: FormulaRepr::Shared(SharedFormula { base, dr: s.dr, dc: s.dc }), array: self.array, dynamic: self.dynamic })
            }
            FormulaRepr::Owned { .. } => None,
        }
    }
    /// The formula's canonical text (without leading `=`). Zero-copy for owned formulas; formatted
    /// on demand for shared formulas.
    pub fn text(&self) -> Cow<'_, str> {
        match &self.repr {
            FormulaRepr::Owned { text, .. } => Cow::Borrowed(text.as_str()),
            FormulaRepr::Shared(s) => Cow::Owned(gridcraft_formula::print_shifted(&s.base, i64::from(s.dr), i64::from(s.dc))),
        }
    }
    /// Heap bytes occupied by the formula's own text buffer (`0` for shared formulas).
    pub fn owned_text_len(&self) -> usize {
        match &self.repr {
            FormulaRepr::Owned { text, .. } => text.len(),
            FormulaRepr::Shared(_) => 0,
        }
    }
    /// Whether this formula holds a shared template `(base, dr, dc)`.
    pub fn is_shared(&self) -> bool {
        matches!(self.repr, FormulaRepr::Shared(_))
    }
    /// Returns `(&base, dr, dc)` without cloning or allocating, for both owned parsed formulas
    /// (`dr = 0, dc = 0`) and shared formulas. `None` when unparsed.
    pub fn expr_shifted(&self) -> Option<(&Arc<Expr>, i32, i32)> {
        match &self.repr {
            FormulaRepr::Owned { expr: Some(e), .. } => Some((e, 0, 0)),
            FormulaRepr::Owned { expr: None, .. } => None,
            FormulaRepr::Shared(s) => Some((&s.base, s.dr, s.dc)),
        }
    }
    /// Whether a parsed AST (owned or shared) is cached on this formula.
    pub fn has_cached_expr(&self) -> bool {
        matches!(self.repr, FormulaRepr::Owned { expr: Some(_), .. } | FormulaRepr::Shared(_))
    }
    /// Drops the cached AST (materializing text first if shared). Used by tests that verify
    /// re-parsing fallback behavior.
    pub fn drop_cached_expr(&mut self) {
        let text = self.text().into_owned();
        self.repr = FormulaRepr::Owned { text, expr: None };
    }
    /// Returns a copy with `array` set.
    pub fn with_array(mut self, array: Option<RangeRef>) -> Formula {
        self.array = array;
        self
    }
    /// Returns a copy with `dynamic` set.
    pub fn with_dynamic(mut self, dynamic: bool) -> Formula {
        self.dynamic = dynamic;
        self
    }
    /// This formula with `e` as its expression (moved, copied or adjusted). It stays dynamic if it was; `array` is the caller's to set.
    pub fn rewritten(&self, e: Expr) -> Formula {
        Formula::from_expr(e).with_dynamic(self.dynamic)
    }
    /// The parsed expression, re-parsing when no tree is cached.
    pub fn expr(&self) -> Option<Expr> {
        match &self.repr {
            FormulaRepr::Owned { text, expr } => expr.as_deref().cloned().or_else(|| gridcraft_formula::parse(text).ok()),
            FormulaRepr::Shared(s) => Some(if s.dr == 0 && s.dc == 0 {
                s.base.as_ref().clone()
            } else {
                gridcraft_formula::adjust::shift_relative(s.base.as_ref().clone(), i64::from(s.dr), i64::from(s.dc))
            }),
        }
    }
    /// The parsed expression without copying it for owned formulas (materializes a shifted tree
    /// for shared formulas; re-parses when no tree is cached).
    pub fn expr_arc(&self) -> Option<Arc<Expr>> {
        match &self.repr {
            FormulaRepr::Owned { text, expr } => expr.clone().or_else(|| gridcraft_formula::parse(text).ok().map(Arc::new)),
            FormulaRepr::Shared(s) if s.dr == 0 && s.dc == 0 => Some(Arc::clone(&s.base)),
            FormulaRepr::Shared(s) => {
                Some(Arc::new(gridcraft_formula::adjust::shift_relative(s.base.as_ref().clone(), i64::from(s.dr), i64::from(s.dc))))
            }
        }
    }
    /// Runs `f` on the parsed expression, borrowing the cached tree or, when none is cached,
    /// a freshly parsed/shifted one that is dropped afterwards.
    pub fn with_expr<R>(&self, f: impl FnOnce(&Expr) -> R) -> Option<R> {
        match &self.repr {
            FormulaRepr::Owned { expr: Some(e), .. } => Some(f(e)),
            FormulaRepr::Owned { text, expr: None } => gridcraft_formula::parse(text).ok().map(|e| f(&e)),
            FormulaRepr::Shared(s) if s.dr == 0 && s.dc == 0 => Some(f(&s.base)),
            FormulaRepr::Shared(s) => {
                let shifted = gridcraft_formula::adjust::shift_relative(s.base.as_ref().clone(), i64::from(s.dr), i64::from(s.dc));
                Some(f(&shifted))
            }
        }
    }
}

/// A formula as a saved workbook holds it: the tree is not saved. Reading one parses its text
/// once and caches the tree, so a workbook opened from JSON does not parse every formula again on
/// each recalculation. The text is kept as saved, and text that doesn't parse caches nothing, as
/// [`Formula::new`] does.
#[derive(Deserialize)]
struct SavedFormula {
    text: String,
    array: Option<RangeRef>,
    #[serde(default)]
    dynamic: bool,
}

impl From<SavedFormula> for Formula {
    fn from(f: SavedFormula) -> Formula {
        let expr = gridcraft_formula::parse(&f.text).ok().map(Arc::new);
        Formula { repr: FormulaRepr::Owned { text: f.text, expr }, array: f.array, dynamic: f.dynamic }
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
    /// Constant value, or the cached result of the formula or Python code.
    pub value: Value,
    pub formula: Option<Arc<Formula>>,
    pub style: StyleId,
    /// Python code, for a Python cell (which has no formula).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub py: Option<Arc<PyCode>>,
}

impl Cell {
    pub fn value(v: Value) -> Cell {
        Cell { value: v, formula: None, style: StyleId::DEFAULT, py: None }
    }
    pub fn formula(f: Formula) -> Cell {
        Cell { value: Value::Empty, formula: Some(Arc::new(f)), style: StyleId::DEFAULT, py: None }
    }
    /// A value cell with this style.
    pub fn styled(v: Value, style: StyleId) -> Cell {
        Cell { value: v, formula: None, style, py: None }
    }
    /// No value, no formula, no Python code and the default style: storing it is pointless.
    pub fn is_blank(&self) -> bool {
        self.value.is_empty() && self.formula.is_none() && self.style == StyleId::DEFAULT && self.py.is_none()
    }
    /// Whether the cell holds content (a value, a formula or Python code), formats aside.
    pub fn has_content(&self) -> bool {
        !self.value.is_empty() || self.formula.is_some() || self.py.is_some()
    }
    /// Removes the content (value, formula and Python code), keeping the style.
    pub fn clear_content(&mut self) {
        self.value = Value::Empty;
        self.formula = None;
        self.py = None;
    }
    /// What the formula bar shows: a formula, a Python cell's code, or the value.
    pub fn input_text(&self) -> String {
        if let Some(py) = &self.py {
            return py.code.clone();
        }
        if let Some(f) = &self.formula {
            return format!("={}", f.text());
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
            Some(f) if f.array.is_some() => format!("{{={}}}", f.text()),
            _ => self.input_text(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::CellRef;

    #[test]
    fn formulas_read_from_json_keep_their_text_and_cache_the_tree() -> Result<(), serde_json::Error> {
        let mut array = Formula::new("=A1:B2*2");
        array.array = Some(RangeRef::new(CellRef::new(0, 2), CellRef::new(1, 3)));
        array.dynamic = true;
        for f in [Formula::new("=SUM(A1:A3)+'My Sheet'!B2"), Formula::new("=LET(x,A1*2,x+1)"), array] {
            let back: Formula = serde_json::from_str(&serde_json::to_string(&f)?)?;
            assert!(back.has_cached_expr(), "{}: the tree is cached after reading", f.text());
            assert_eq!(back, f);
        }
        // Saved text is kept as written, even when the canonical print differs, and its tree is
        // the one the text parses to.
        let back: Formula = serde_json::from_str(r#"{"text": "sum( a1 , 2 )", "array": null}"#)?;
        assert_eq!(back.text(), "sum( a1 , 2 )");
        assert!(!back.dynamic);
        assert_eq!(back.expr(), gridcraft_formula::parse("sum( a1 , 2 )").ok());
        assert!(back.has_cached_expr());
        // Text that doesn't parse caches nothing, as `Formula::new` does.
        let bad = Formula::new("=SUM(");
        let back: Formula = serde_json::from_str(&serde_json::to_string(&bad)?)?;
        assert_eq!(back, bad);
        assert!(!back.has_cached_expr());
        Ok(())
    }

    #[test]
    fn a_workbook_read_from_json_has_every_formula_tree() -> Result<(), Box<dyn std::error::Error>> {
        let mut wb = crate::Workbook::new();
        let mut array = Formula::new("=A1:A3*2");
        array.array = Some(RangeRef::new(CellRef::new(0, 3), CellRef::new(2, 3)));
        let sheet = wb.sheet_mut(0).ok_or("no sheet")?;
        sheet.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(1.0)));
        sheet.cells.set(CellRef::new(0, 1), Cell::formula(Formula::new("=SUM(A1:A3)")));
        sheet.cells.set(CellRef::new(1, 1), Cell::formula(Formula::new("=IF(A1>0,\"yes\",\"no\")")));
        sheet.cells.set(CellRef::new(2, 1), Cell::formula(Formula::new("=SUM(")));
        sheet.cells.set(CellRef::new(0, 3), Cell::formula(array));
        let json = serde_json::to_string(&wb)?;
        let back: crate::Workbook = serde_json::from_str(&json)?;
        let (before, after) = (wb.sheet(0).ok_or("no sheet")?, back.sheet(0).ok_or("no sheet")?);
        let mut formulas = 0;
        for (c, cell) in before.cells.iter() {
            let (Some(f), Some(read)) = (&cell.formula, after.cell(c).and_then(|cell| cell.formula.as_ref())) else { continue };
            assert_eq!(read, f, "{c:?}: text, tree and flags as saved");
            assert_eq!(read.has_cached_expr(), gridcraft_formula::parse(&f.text()).is_ok(), "{c:?}: every formula that parses has its tree");
            formulas += 1;
        }
        assert_eq!(formulas, 4);
        Ok(())
    }

    #[test]
    fn shared_formula_shares_ast_and_roundtrips_json_and_equality() -> Result<(), Box<dyn std::error::Error>> {
        let base = Arc::new(gridcraft_formula::parse("IF(A1>0,B1*$C$1,0)")?);
        let mut formulas = Vec::with_capacity(500);
        for dr in 0..500 {
            formulas.push(Formula::from_shared(Arc::clone(&base), dr, 0));
        }
        assert_eq!(Arc::strong_count(&base), 501, "all 500 formulas share one master AST");
        for (dr, f) in formulas.iter().enumerate() {
            let expected = Formula::new(&format!("=IF(A{r}>0,B{r}*$C$1,0)", r = dr + 1));
            assert_eq!(f.text(), expected.text());
            assert_eq!(*f, expected);
            assert_eq!(f.owned_text_len(), 0);
        }
        let oob = Formula::from_shared(Arc::clone(&base), -1, 0);
        assert_eq!(oob.text(), "IF(#REF!>0,#REF!*$C$1,0)");
        let json = serde_json::to_string(&formulas[10])?;
        let back: Formula = serde_json::from_str(&json)?;
        assert_eq!(back, formulas[10]);
        assert_eq!(back.text(), "IF(A11>0,B11*$C$1,0)");
        Ok(())
    }
}
