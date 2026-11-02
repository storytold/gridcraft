//! Cells.

use std::sync::Arc;

use gridcraft_core::{RangeRef, Value};
use gridcraft_formula::Expr;
use serde::{Deserialize, Serialize};

use crate::python::PyCode;
use crate::style::StyleId;

/// A parsed formula. `text` is the canonical formula text without the leading `=`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Formula {
    pub text: String,
    #[serde(skip)]
    pub expr: Option<Arc<Expr>>,
    /// Legacy array formula (Ctrl+Shift+Enter) covering this range; the formula lives in the
    /// top-left cell.
    pub array: Option<RangeRef>,
    /// Read from a file that marks this formula as a dynamic-array formula (cell metadata `cm`).
    /// The writer keeps the mark, so a formula Excel evaluates as an array is never saved as a
    /// legacy formula (which Excel evaluates with implicit intersection). Formulas entered or
    /// rewritten in GridCraft start without it, and the writer decides from their shape.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dynamic: bool,
}

impl Formula {
    /// Parses `text` (with or without `=`). Unparseable formulas keep their text and evaluate to
    /// `#NAME?`.
    pub fn new(text: &str) -> Formula {
        let body = text.strip_prefix('=').unwrap_or(text);
        match gridcraft_formula::parse(body) {
            Ok(e) => Formula { text: gridcraft_formula::print(&e), expr: Some(Arc::new(e)), array: None, dynamic: false },
            Err(_) => Formula { text: body.to_string(), expr: None, array: None, dynamic: false },
        }
    }
    pub fn from_expr(e: Expr) -> Formula {
        Formula { text: gridcraft_formula::print(&e), expr: Some(Arc::new(e)), array: None, dynamic: false }
    }
    /// The parsed expression, re-parsing after deserialization.
    pub fn expr(&self) -> Option<Expr> {
        self.expr.as_deref().cloned().or_else(|| gridcraft_formula::parse(&self.text).ok())
    }
    /// The parsed expression without copying it (re-parses after deserialization).
    pub fn expr_arc(&self) -> Option<Arc<Expr>> {
        self.expr.clone().or_else(|| gridcraft_formula::parse(&self.text).ok().map(Arc::new))
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
