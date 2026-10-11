//! Formula syntax tree.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use serde::{Deserialize, Serialize};

/// One end of a reference, with `$` anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Anchor {
    pub row: u32,
    pub col: u32,
    pub row_abs: bool,
    pub col_abs: bool,
}

impl Anchor {
    pub fn cell(&self) -> CellRef {
        CellRef::new(self.row, self.col)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RefKind {
    Cell(Anchor),
    Range(Anchor, Anchor),
    /// Whole rows `3:5` (row, abs) pairs.
    Rows(u32, bool, u32, bool),
    /// Whole columns `A:C`.
    Cols(u32, bool, u32, bool),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SheetSel {
    /// Unqualified: the formula's own sheet.
    Current,
    Named(String),
    /// 3-D span `Sheet1:Sheet3`.
    Span(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reference {
    pub sheet: SheetSel,
    pub kind: RefKind,
}

impl Reference {
    pub fn range(&self) -> RangeRef {
        match &self.kind {
            RefKind::Cell(a) => RangeRef::cell(a.cell()),
            RefKind::Range(a, b) => RangeRef::new(a.cell(), b.cell()),
            RefKind::Rows(r0, _, r1, _) => RangeRef::rows((*r0).min(*r1), (*r0).max(*r1)),
            RefKind::Cols(c0, _, c1, _) => RangeRef::cols((*c0).min(*c1), (*c0).max(*c1)),
        }
    }
    pub fn sheet_name(&self) -> Option<&str> {
        match &self.sheet {
            SheetSel::Named(s) => Some(s),
            _ => None,
        }
    }
}

/// Structured reference `Table1[[#Headers],[Col1]:[Col3]]`, `[@Col]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StructRef {
    /// Empty for `[@Col]` inside the table itself.
    pub table: String,
    pub specifiers: Vec<StructItem>,
    pub col_start: Option<String>,
    pub col_end: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StructItem {
    All,
    Data,
    Headers,
    Totals,
    ThisRow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnOp {
    Neg,
    Plus,
    Percent,
    /// Implicit intersection `@`.
    At,
    /// Spill range operator `A1#`: the whole spill range of the dynamic array anchored at the
    /// cell (`ANCHORARRAY` in files).
    Spill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    /// `:` between two references.
    Range,
    /// `,` inside parentheses.
    Union,
    /// space between two references.
    Intersect,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
            BinOp::Concat => "&",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::Range => ":",
            BinOp::Union => ",",
            BinOp::Intersect => " ",
        }
    }
    /// Binding power (higher binds tighter).
    pub fn precedence(&self) -> u8 {
        match self {
            BinOp::Range => 90,
            BinOp::Intersect => 85,
            BinOp::Union => 80,
            BinOp::Pow => 50,
            BinOp::Mul | BinOp::Div => 40,
            BinOp::Add | BinOp::Sub => 30,
            BinOp::Concat => 20,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => 10,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Expr {
    Number(f64),
    Text(Arc<str>),
    Bool(bool),
    Error(CellError),
    /// Array constant `{1,2;3,4}` (rows of literal expressions).
    Array(Vec<Vec<Expr>>),
    Ref(Reference),
    /// Defined name, table name or LET/LAMBDA parameter.
    Name(String),
    Struct(StructRef),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// Function call; the name is upper-case without `_xlfn.`.
    Call(String, Vec<Expr>),
    /// Calling the result of an expression, e.g. `LAMBDA(x,x+1)(2)`.
    Invoke(Box<Expr>, Vec<Expr>),
    /// An omitted argument `F(1,,3)`.
    Missing,
    Paren(Box<Expr>),
}

/// Left-nested chains (`1+1+…`, `A1%%%`, `f(1)(2)…`) can be arbitrarily deep, so cloning and
/// comparing follow the left spine with a loop; only right operands and arguments recurse, and
/// those are bounded by the parser's nesting limit.
impl Clone for Expr {
    fn clone(&self) -> Expr {
        enum Up {
            Binary(BinOp, Expr),
            Percent,
            Invoke(Vec<Expr>),
        }
        let mut ups: Vec<Up> = Vec::new();
        let mut cur = self;
        let mut node = loop {
            match cur {
                Expr::Binary(op, a, b) => {
                    ups.push(Up::Binary(*op, (**b).clone()));
                    cur = a;
                }
                Expr::Unary(UnOp::Percent, x) => {
                    ups.push(Up::Percent);
                    cur = x;
                }
                Expr::Invoke(c, args) => {
                    ups.push(Up::Invoke(args.clone()));
                    cur = c;
                }
                Expr::Number(n) => break Expr::Number(*n),
                Expr::Text(t) => break Expr::Text(t.clone()),
                Expr::Bool(b) => break Expr::Bool(*b),
                Expr::Error(e) => break Expr::Error(*e),
                Expr::Array(rows) => break Expr::Array(rows.clone()),
                Expr::Ref(r) => break Expr::Ref(r.clone()),
                Expr::Name(n) => break Expr::Name(n.clone()),
                Expr::Struct(s) => break Expr::Struct(s.clone()),
                Expr::Unary(op, x) => break Expr::Unary(*op, x.clone()),
                Expr::Call(n, args) => break Expr::Call(n.clone(), args.clone()),
                Expr::Missing => break Expr::Missing,
                Expr::Paren(x) => break Expr::Paren(x.clone()),
            }
        };
        while let Some(up) = ups.pop() {
            node = match up {
                Up::Binary(op, b) => Expr::Binary(op, Box::new(node), Box::new(b)),
                Up::Percent => Expr::Unary(UnOp::Percent, Box::new(node)),
                Up::Invoke(args) => Expr::Invoke(Box::new(node), args),
            };
        }
        node
    }
}

impl PartialEq for Expr {
    fn eq(&self, other: &Expr) -> bool {
        enum Later<'a> {
            Operand(&'a Expr, &'a Expr),
            Args(&'a [Expr], &'a [Expr]),
        }
        let mut later: Vec<Later<'_>> = Vec::new();
        let (mut a, mut b) = (self, other);
        loop {
            match (a, b) {
                (Expr::Binary(o1, l1, r1), Expr::Binary(o2, l2, r2)) => {
                    if o1 != o2 {
                        return false;
                    }
                    later.push(Later::Operand(r1, r2));
                    (a, b) = (l1, l2);
                }
                (Expr::Unary(UnOp::Percent, x), Expr::Unary(UnOp::Percent, y)) => (a, b) = (x, y),
                (Expr::Invoke(c1, a1), Expr::Invoke(c2, a2)) => {
                    later.push(Later::Args(a1, a2));
                    (a, b) = (c1, c2);
                }
                (Expr::Number(x), Expr::Number(y)) if x != y => return false,
                (Expr::Text(x), Expr::Text(y)) if x != y => return false,
                (Expr::Bool(x), Expr::Bool(y)) if x != y => return false,
                (Expr::Error(x), Expr::Error(y)) if x != y => return false,
                (Expr::Array(x), Expr::Array(y)) if x != y => return false,
                (Expr::Ref(x), Expr::Ref(y)) if x != y => return false,
                (Expr::Name(x), Expr::Name(y)) if x != y => return false,
                (Expr::Struct(x), Expr::Struct(y)) if x != y => return false,
                (Expr::Unary(o1, x), Expr::Unary(o2, y)) if o1 != o2 || x != y => return false,
                (Expr::Call(n1, x), Expr::Call(n2, y)) if n1 != n2 || x != y => return false,
                (Expr::Paren(x), Expr::Paren(y)) if x != y => return false,
                (Expr::Number(_), Expr::Number(_))
                | (Expr::Text(_), Expr::Text(_))
                | (Expr::Bool(_), Expr::Bool(_))
                | (Expr::Error(_), Expr::Error(_))
                | (Expr::Array(_), Expr::Array(_))
                | (Expr::Ref(_), Expr::Ref(_))
                | (Expr::Name(_), Expr::Name(_))
                | (Expr::Struct(_), Expr::Struct(_))
                | (Expr::Unary(..), Expr::Unary(..))
                | (Expr::Call(..), Expr::Call(..))
                | (Expr::Paren(_), Expr::Paren(_))
                | (Expr::Missing, Expr::Missing) => break,
                _ => return false,
            }
        }
        later.into_iter().all(|l| match l {
            Later::Operand(x, y) => x == y,
            Later::Args(x, y) => x == y,
        })
    }
}

impl Expr {
    /// Visits every node depth-first (parents before children, operands left to right).
    ///
    /// Left-nested chains (`1+1+…`, `A1%%%`, `f(1)(2)…`) can be arbitrarily deep, so the left
    /// spine is followed with a loop; only right operands and arguments recurse, and those are
    /// bounded by the parser's nesting limit.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Expr)) {
        enum Later<'a> {
            Operand(&'a Expr),
            Args(&'a [Expr]),
        }
        let mut later: Vec<Later<'a>> = Vec::new();
        let mut cur = self;
        loop {
            f(cur);
            match cur {
                Expr::Binary(_, a, b) => {
                    later.push(Later::Operand(b));
                    cur = a;
                }
                Expr::Unary(UnOp::Percent, x) => cur = x,
                Expr::Invoke(c, args) => {
                    later.push(Later::Args(args));
                    cur = c;
                }
                Expr::Array(rows) => {
                    rows.iter().flatten().for_each(|e| e.walk(f));
                    break;
                }
                Expr::Unary(_, e) | Expr::Paren(e) => {
                    e.walk(f);
                    break;
                }
                Expr::Call(_, args) => {
                    args.iter().for_each(|e| e.walk(f));
                    break;
                }
                _ => break,
            }
        }
        while let Some(l) = later.pop() {
            match l {
                Later::Operand(e) => e.walk(f),
                Later::Args(args) => args.iter().for_each(|e| e.walk(f)),
            }
        }
    }
    /// Rewrites every node bottom-up. The left spine is handled iteratively (see [`Expr::walk`]).
    pub fn map(self, f: &mut dyn FnMut(Expr) -> Expr) -> Expr {
        enum Up {
            Binary(BinOp, Expr),
            Percent,
            Invoke(Vec<Expr>),
        }
        let mut ups: Vec<Up> = Vec::new();
        let mut cur = self;
        let mut e = loop {
            match cur {
                Expr::Binary(op, a, b) => {
                    ups.push(Up::Binary(op, *b));
                    cur = *a;
                }
                Expr::Unary(UnOp::Percent, x) => {
                    ups.push(Up::Percent);
                    cur = *x;
                }
                Expr::Invoke(c, args) => {
                    ups.push(Up::Invoke(args));
                    cur = *c;
                }
                Expr::Array(rows) => break Expr::Array(rows.into_iter().map(|r| r.into_iter().map(|e| e.map(f)).collect()).collect()),
                Expr::Unary(op, e) => break Expr::Unary(op, Box::new(e.map(f))),
                Expr::Paren(e) => break Expr::Paren(Box::new(e.map(f))),
                Expr::Call(n, args) => break Expr::Call(n, args.into_iter().map(|e| e.map(f)).collect()),
                other => break other,
            }
        };
        e = f(e);
        while let Some(up) = ups.pop() {
            e = match up {
                Up::Binary(op, b) => {
                    let b = b.map(f);
                    Expr::Binary(op, Box::new(e), Box::new(b))
                }
                Up::Percent => Expr::Unary(UnOp::Percent, Box::new(e)),
                Up::Invoke(args) => Expr::Invoke(Box::new(e), args.into_iter().map(|a| a.map(f)).collect()),
            };
            e = f(e);
        }
        e
    }
    /// All references in the formula.
    pub fn references(&self) -> Vec<&Reference> {
        let mut v = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Ref(r) = e {
                v.push(r);
            }
        });
        v
    }
    /// Function names used (upper-case).
    pub fn functions(&self) -> Vec<&str> {
        let mut v = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Call(n, _) = e {
                v.push(n.as_str());
            }
        });
        v
    }
}

pub(crate) fn clamp_anchor(row: i64, col: i64) -> Option<(u32, u32)> {
    if (0..MAX_ROWS as i64).contains(&row) && (0..MAX_COLS as i64).contains(&col) { Some((row as u32, col as u32)) } else { None }
}
