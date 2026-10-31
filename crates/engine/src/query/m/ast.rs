//! The M expression tree.

use std::rc::Rc;

#[derive(Debug)]
pub enum Expr {
    Null,
    Bool(bool),
    Num(f64),
    Text(Rc<str>),
    /// A variable, another query, or a library name (`Table.AddColumn`, `#date`).
    Ident(Rc<str>),
    List(Vec<ListItem>),
    /// A record literal; its fields see each other, like a `let`.
    Record(Rc<[(Rc<str>, Rc<Expr>)]>),
    Let(Rc<[(Rc<str>, Rc<Expr>)]>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// A function (`(x) => …`, and `each …`, which is `(_) => …`).
    Func(Rc<FuncDef>),
    Call(Box<Expr>, Vec<Expr>),
    /// `x[Field]`; no target is `_[Field]` (the implicit form inside `each`).
    Field {
        target: Option<Box<Expr>>,
        name: Rc<str>,
        optional: bool,
    },
    /// `x[[A], [B]]`.
    Project {
        target: Option<Box<Expr>>,
        names: Vec<Rc<str>>,
        optional: bool,
    },
    /// `x{0}`, `x{[Name = "Sales"]}`.
    Item {
        target: Box<Expr>,
        key: Box<Expr>,
        optional: bool,
    },
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `try x`, `try x otherwise y`, `try x catch (e) => y`.
    Try {
        body: Box<Expr>,
        otherwise: Option<Box<Expr>>,
        catch: Option<Rc<FuncDef>>,
    },
    Error(Box<Expr>),
    Meta(Box<Expr>, Box<Expr>),
    Type(Rc<TypeExpr>),
    Is(Box<Expr>, Rc<TypeExpr>),
    As(Box<Expr>, Rc<TypeExpr>),
    /// `...`: Expression.Error "Not Implemented".
    NotImplemented,
}

#[derive(Debug)]
pub enum ListItem {
    One(Expr),
    /// `a..b`.
    Range(Expr, Expr),
}

#[derive(Debug)]
pub struct FuncDef {
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Debug)]
pub struct Param {
    pub name: Rc<str>,
    pub optional: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Plus,
    Minus,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Coalesce,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Concat,
}

/// A type as written after `type`, `is` and `as`.
#[derive(Debug)]
pub enum TypeExpr {
    /// `text`, `number`, `any`, `null`…
    Prim(&'static str),
    Nullable(Box<TypeExpr>),
    List(Box<TypeExpr>),
    Record(Vec<(Rc<str>, TypeExpr)>),
    Table(Vec<(Rc<str>, TypeExpr)>),
    Function,
    /// An expression giving a type value (`Int64.Type`, a variable holding a type).
    Expr(Box<Expr>),
}

/// The primitive type names.
pub const PRIMITIVE_TYPES: &[&str] = &[
    "any",
    "anynonnull",
    "binary",
    "date",
    "datetime",
    "datetimezone",
    "duration",
    "function",
    "list",
    "logical",
    "none",
    "null",
    "number",
    "record",
    "table",
    "text",
    "time",
    "type",
];
