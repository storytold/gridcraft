//! DAX syntax tree.
//!
//! Every node carries a [`Span`] in byte offsets into the source text. [`Expr`] equality
//! deliberately ignores spans (`PartialEq` compares [`ExprKind`] only), which makes the
//! parser/printer round-trip invariant expressible as `parse(print(e)) == e`.

/// A half-open byte range `start..end` into the source text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// An empty span at byte offset 0.
    pub const fn empty() -> Self {
        Span { start: 0, end: 0 }
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

    /// The smallest span covering both operands.
    pub fn to(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
}

/// A `dt"..."` date/time literal.
///
/// Components are validated when the literal is lexed. The `has_time` flag distinguishes
/// `dt"2023-05-19"` from `dt"2023-05-19T00:00:00"`, which are different literal forms even
/// though they denote the same serial value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DateTimeLiteral {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub has_time: bool,
}

impl DateTimeLiteral {
    /// Canonical `YYYY-MM-DD` / `YYYY-MM-DDThh:mm:ss` text, without the `dt` prefix or quotes.
    pub fn canonical(self) -> String {
        format!("{self}")
    }
}

impl std::fmt::Display for DateTimeLiteral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)?;
        if self.has_time {
            write!(f, "T{:02}:{:02}:{:02}", self.hour, self.minute, self.second)?;
        }
        Ok(())
    }
}

/// A DAX literal value.
///
/// Integer and Decimal are distinct literal kinds (not one numeric kind): `42` is an
/// [`Literal::Integer`] and `42.0`, `42e0` are [`Literal::Decimal`] values. The sign of a
/// negative number is the unary `-` operator, never part of the literal.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    /// A 64-bit signed integer literal such as `42`.
    Integer(i64),
    /// An IEEE-754 double literal written with a decimal point or exponent: `3.14`, `.5`, `1e3`.
    Decimal(f64),
    /// A `dt"..."` literal.
    DateTime(DateTimeLiteral),
    String(String),
    Boolean(bool),
}

/// How an identifier was written, so the printer can reproduce it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameStyle {
    /// `Sales`
    Bare,
    /// `'Sales Table'`
    Quoted,
    /// `[Amount]`
    Bracketed,
}

/// A single identifier, preserving its source spelling and quoting style.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Name {
    /// Text with quoting removed and doubled quote/bracket escapes resolved.
    pub text: String,
    pub style: NameStyle,
}

impl Name {
    pub fn bare(text: impl Into<String>) -> Self {
        Name { text: text.into(), style: NameStyle::Bare }
    }

    pub fn quoted(text: impl Into<String>) -> Self {
        Name { text: text.into(), style: NameStyle::Quoted }
    }

    pub fn bracketed(text: impl Into<String>) -> Self {
        Name { text: text.into(), style: NameStyle::Bracketed }
    }
}

/// A name, optionally table-qualified: `'Sales'[Amount]`, `Sales[Amount]` or `[Amount]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NameRef {
    /// Table part of a column/measure reference.
    pub table: Option<Name>,
    pub name: Name,
}

impl NameRef {
    pub fn unqualified(name: Name) -> Self {
        NameRef { table: None, name }
    }

    pub fn qualified(table: Name, name: Name) -> Self {
        NameRef { table: Some(table), name }
    }
}

/// A function call. The name keeps the spelling used in the source (`Sum`, `SUM`).
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub name: Name,
    pub args: Vec<Expr>,
}

/// Prefix operators. `Not` is the keyword operator, not a function call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Plus,
    Neg,
    Not,
}

impl UnaryOp {
    /// Binding power, higher binds tighter (DAX precedence levels 3 and 8).
    pub const fn precedence(self) -> u8 {
        match self {
            UnaryOp::Plus | UnaryOp::Neg => 80,
            UnaryOp::Not => 30,
        }
    }

    pub const fn symbol(self) -> &'static str {
        match self {
            UnaryOp::Plus => "+",
            UnaryOp::Neg => "-",
            UnaryOp::Not => "NOT",
        }
    }
}

/// Binary operators, in the precedence order of <https://dax.guide/operators/>.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    StrictEq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    In,
}

impl BinaryOp {
    pub const fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Pow => "^",
            BinaryOp::Concat => "&",
            BinaryOp::Eq => "=",
            BinaryOp::StrictEq => "==",
            BinaryOp::Ne => "<>",
            BinaryOp::Lt => "<",
            BinaryOp::Gt => ">",
            BinaryOp::Le => "<=",
            BinaryOp::Ge => ">=",
            BinaryOp::And => "&&",
            BinaryOp::Or => "||",
            BinaryOp::In => "IN",
        }
    }

    /// Binding power, higher binds tighter. The values encode the ten precedence levels
    /// from DAX Guide: parentheses/calls/`IN`, `^`, unary sign, `* /`, `+ -`, `&`,
    /// comparisons, `NOT`, `&&`, `||`.
    pub const fn precedence(self) -> u8 {
        match self {
            BinaryOp::In => 100,
            BinaryOp::Pow => 90,
            BinaryOp::Mul | BinaryOp::Div => 70,
            BinaryOp::Add | BinaryOp::Sub => 60,
            BinaryOp::Concat => 50,
            BinaryOp::Eq | BinaryOp::StrictEq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => 40,
            BinaryOp::And => 20,
            BinaryOp::Or => 10,
        }
    }

    /// DAX binary operators associate to the left, except `^`, which is right-associative
    /// (verified against Excel: `2^3^2` is 512).
    pub const fn is_left_associative(self) -> bool {
        !matches!(self, BinaryOp::Pow)
    }

    pub const fn is_right_associative(self) -> bool {
        matches!(self, BinaryOp::Pow)
    }

    pub const fn is_comparison(self) -> bool {
        matches!(self, BinaryOp::Eq | BinaryOp::StrictEq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge)
    }

    pub const fn is_logical(self) -> bool {
        matches!(self, BinaryOp::And | BinaryOp::Or)
    }
}

/// The shape of an expression, without its span.
#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Literal(Literal),
    /// A bare, quoted or table-qualified name.
    Name(NameRef),
    /// `SUM(1, 2)`.
    Call(Call),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// A table constructor `{ 1, 2 }`. Rows are [`ExprKind::Tuple`] values.
    Table(Vec<Expr>),
    /// A row constructor / parenthesised comma list `(1, "a")`.
    Tuple(Vec<Expr>),
}

/// A DAX expression with its source span.
#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Expr { kind, span }
    }

    pub fn literal(value: Literal, span: Span) -> Self {
        Expr::new(ExprKind::Literal(value), span)
    }

    /// The tree depth (a leaf is 1), computed iteratively so it is safe on any shape the
    /// parser accepts. The parser uses this to bound the AST that recursive `Debug`,
    /// `PartialEq` and drop glue would otherwise walk.
    pub fn depth(&self) -> usize {
        let mut max = 0;
        let mut stack = vec![(self, 1usize)];
        while let Some((node, depth)) = stack.pop() {
            max = max.max(depth);
            match &node.kind {
                ExprKind::Call(call) => stack.extend(call.args.iter().map(|e| (e, depth + 1))),
                ExprKind::Unary { expr, .. } => stack.push((expr, depth + 1)),
                ExprKind::Binary { left, right, .. } => {
                    stack.push((right, depth + 1));
                    stack.push((left, depth + 1));
                }
                ExprKind::Table(items) | ExprKind::Tuple(items) => {
                    stack.extend(items.iter().map(|e| (e, depth + 1)));
                }
                ExprKind::Literal(_) | ExprKind::Name(_) => {}
            }
        }
        max
    }

    /// Visits every node depth-first, parents before children. Iterative, so deeply nested
    /// (but within the parser limits) trees cannot overflow the stack.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Expr)) {
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            f(node);
            match &node.kind {
                ExprKind::Call(call) => stack.extend(call.args.iter().rev()),
                ExprKind::Unary { expr, .. } => stack.push(expr),
                ExprKind::Binary { left, right, .. } => {
                    stack.push(right);
                    stack.push(left);
                }
                ExprKind::Table(items) | ExprKind::Tuple(items) => stack.extend(items.iter().rev()),
                ExprKind::Literal(_) | ExprKind::Name(_) => {}
            }
        }
    }
}

/// Spans do not participate in equality: reparsing printed text produces the same AST.
impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

/// `EVALUATE <table expression>`. The statements PR extends this type.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub evaluate: Expr,
}
