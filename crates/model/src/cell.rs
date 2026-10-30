//! Cells.

use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_core::{RangeRef, Value};
use gridcraft_formula::Expr;
use serde::{Deserialize, Serialize};

use crate::style::StyleId;

/// A namespace explicitly supplied by an imported formula, independent of function support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileFunctionPrefix {
    Xlfn,
    XlfnXlws,
}

impl FileFunctionPrefix {
    /// Add only the recorded namespace to the existing function name, never replace the call.
    pub fn apply(self, name: &str) -> String {
        match self {
            Self::Xlfn => format!("_xlfn.{name}"),
            Self::XlfnXlws => format!("_xlfn._xlws.{name}"),
        }
    }
}

/// A parsed formula. `text` is the canonical formula text without the leading `=`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Formula {
    pub text: String,
    /// Imported namespaces in lexical occurrence order per normalized function name. `None`
    /// keeps a bare call distinct from tagged calls. The enum cannot inject another name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub file_functions: BTreeMap<String, Vec<Option<FileFunctionPrefix>>>,
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
            Ok(e) => {
                Formula { text: gridcraft_formula::print(&e), expr: Some(Arc::new(e)), array: None, dynamic: false, file_functions: BTreeMap::new() }
            }
            Err(_) => Formula { text: body.to_string(), expr: None, array: None, dynamic: false, file_functions: BTreeMap::new() },
        }
    }
    pub fn from_expr(e: Expr) -> Formula {
        Formula { text: gridcraft_formula::print(&e), expr: Some(Arc::new(e)), array: None, dynamic: false, file_functions: BTreeMap::new() }
    }
    /// Rewrite an expression while keeping imported call namespaces only when call counts and
    /// local LET/LAMBDA bindings still match. Array/dynamic metadata stays under the caller's
    /// existing policy, as with [`Self::from_expr`].
    pub fn rewrite_expr(&self, e: Expr) -> Formula {
        if self.file_functions.is_empty() {
            return Self::from_expr(e);
        }
        let Some(old) = self.expr_arc() else { return Self::from_expr(e) };
        if (has_local_bindings(&old) || has_local_bindings(&e)) && binding_shape(&old) != binding_shape(&e) {
            return Self::from_expr(e);
        }
        let mut file_functions = BTreeMap::new();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        e.walk(&mut |node| {
            if let Expr::Call(name, _) = node {
                let count = counts.entry(name.to_ascii_uppercase()).or_default();
                *count = count.saturating_add(1);
            }
        });
        for (name, count) in counts {
            if let Some(prefixes) = self.file_functions.get(&name)
                && count == prefixes.len()
            {
                file_functions.insert(name, prefixes.clone());
            }
        }
        Formula { file_functions, ..Self::from_expr(e) }
    }
    /// Apply the same namespace and binding checks to a formula entered through the editor.
    pub fn rewrite_input_expr(&self, e: Expr) -> Formula {
        self.rewrite_expr(e)
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

/// Whether the expression can change which function a bare call names through a local binding.
fn has_local_bindings(expr: &Expr) -> bool {
    let mut found = false;
    expr.walk(&mut |node| {
        found |= matches!(node, Expr::Call(name, _) if name == "LET" || name == "LAMBDA");
    });
    found
}

/// A value-independent tree signature that keeps call positions and local declaration names.
fn binding_shape(expr: &Expr) -> Vec<String> {
    let mut shape = Vec::new();
    expr.walk(&mut |node| {
        shape.push(match node {
            Expr::Call(name, args) => {
                let last = args.len().saturating_sub(1);
                let declarations = if name == "LET" || name == "LAMBDA" {
                    args.iter()
                        .enumerate()
                        .filter(|(i, _)| *i < last && (name == "LAMBDA" || i % 2 == 0))
                        .map(|(_, arg)| match arg {
                            Expr::Name(name) => gridcraft_formula::fold_name(name),
                            _ => "?".into(),
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                format!("call:{}:{}:{declarations:?}", name.to_ascii_uppercase(), args.len())
            }
            Expr::Binary(op, _, _) => format!("binary:{op:?}"),
            Expr::Unary(op, _) => format!("unary:{op:?}"),
            Expr::Paren(_) => "paren".into(),
            Expr::Invoke(_, args) => format!("invoke:{}", args.len()),
            Expr::Array(rows) => format!("array:{:?}", rows.iter().map(Vec::len).collect::<Vec<_>>()),
            _ => "value".into(),
        });
    });
    shape
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

#[cfg(test)]
mod prefix_tests {
    use super::*;

    /// Old serialized formulas have no prefix map; new metadata survives JSON and reference edits.
    #[test]
    fn file_function_prefixes_are_optional_and_survive_json_and_rewrites() {
        let old: Formula = serde_json::from_str(r#"{"text":"SYNTHETICFUTURE(A1)","array":null}"#).unwrap();
        assert!(old.file_functions.is_empty());
        let mut formula = old;
        formula.file_functions.insert("SYNTHETICFUTURE".into(), vec![Some(FileFunctionPrefix::Xlfn)]);
        let saved = serde_json::to_string(&formula).unwrap();
        let restored: Formula = serde_json::from_str(&saved).unwrap();
        let shifted = gridcraft_formula::adjust::shift_relative(restored.expr().unwrap(), 1, 0);
        let rewritten = restored.rewrite_expr(shifted);
        assert_eq!(rewritten.text, "SYNTHETICFUTURE(A2)");
        assert_eq!(rewritten.file_functions, formula.file_functions);
    }

    /// A direct structural rewrite cannot carry a local occurrence's bare marker to a global call.
    #[test]
    fn direct_rewrite_discards_namespaces_when_let_bindings_change() {
        let mut formula = Formula::new("XLOOKUP(1,A:A,B:B)+LET(XLOOKUP,1,XLOOKUP(2,A:A,B:B))");
        formula.file_functions.insert("XLOOKUP".into(), vec![Some(FileFunctionPrefix::Xlfn), None]);
        let e = formula.expr().unwrap().map(&mut |node| match node {
            Expr::Name(name) if name == "XLOOKUP" => Expr::Name("Renamed".into()),
            other => other,
        });
        assert!(formula.rewrite_expr(e.clone()).file_functions.is_empty());
        assert!(formula.rewrite_input_expr(e).file_functions.is_empty());
    }
}
