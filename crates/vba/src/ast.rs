//! The expression/statement tree for the supported VBA subset.

/// A runtime value. VBA is dynamically typed (`Variant`); this is deliberately small — no dates,
/// no arrays, no objects beyond the `Range`/`Cells` access baked into [`crate::interp`].
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Empty,
    Number(f64),
    Str(String),
    Bool(bool),
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Value::Empty => false,
            Value::Number(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Bool(b) => *b,
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Bool(true) => -1.0, // VBA's own convention (True == -1 numerically).
            Value::Bool(false) => 0.0,
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
            Value::Empty => 0.0,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Value::Empty => String::new(),
            Value::Number(n) => crate::interp::format_number(*n),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// A place a value can be read from or written to: a plain variable, or `.Value` on a cell
/// reference (`Range("A1")` / `Cells(r, c)`, optionally qualified by a sheet).
#[derive(Debug, Clone)]
pub enum Place {
    Var(String),
    Cell { sheet: Option<Box<Expr>>, cell: Box<CellExpr> },
}

/// How a cell is addressed: by A1 text (`Range("A1")`, possibly a variable holding text) or by
/// 1-based row/column (`Cells(row, col)`).
#[derive(Debug, Clone)]
pub enum CellExpr {
    A1(Box<Expr>),
    RowCol(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Value),
    Var(String),
    Unary(UnOp, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    /// Reads a cell's `.Value` (`Range("A1").Value`, `Cells(1,1).Value`, or bare `Range("A1")`,
    /// which VBA treats as `.Value` too).
    CellRead {
        sheet: Option<Box<Expr>>,
        cell: Box<CellExpr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Dim(String),
    Assign(Place, Expr),
    If { branches: Vec<(Expr, Vec<Stmt>)>, else_body: Vec<Stmt> },
    For { var: String, start: Expr, end: Expr, step: Option<Expr>, body: Vec<Stmt> },
    MsgBox(Expr),
}
