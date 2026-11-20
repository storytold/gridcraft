//! DAX syntax tree.
//!
//! Every expression node carries a [`Span`] in byte offsets into the source text. Statement
//! nodes (`DEFINE` entries, `EVALUATE`, `ORDER BY`, visual shapes) group expressions and names.
//! [`Expr`] equality deliberately ignores spans (`PartialEq` compares [`ExprKind`] only), which
//! makes the parser/printer round-trip invariant expressible as `parse(print(e)) == e`.

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
    /// An expression-level `VAR <name> = <expr> ... RETURN <expr>` block.
    VarReturn {
        vars: Vec<VarDecl>,
        body: Box<Expr>,
    },
}

/// One `VAR <name> = <expression>` declaration inside an [`ExprKind::VarReturn`].
///
/// The name is a plain identifier: DAX variable names cannot be quoted or bracketed. Declarations
/// are sequential, so `nameN` is visible only to `expressionN+1` and to the `RETURN` body.
#[derive(Clone, Debug, PartialEq)]
pub struct VarDecl {
    pub name: Name,
    pub expr: Expr,
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
                ExprKind::VarReturn { vars, body } => {
                    stack.push((body, depth + 1));
                    stack.extend(vars.iter().map(|var| (&var.expr, depth + 1)));
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
                ExprKind::VarReturn { vars, body } => {
                    stack.push(body);
                    stack.extend(vars.iter().rev().map(|var| &var.expr));
                }
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

/// A complete DAX query: an optional `DEFINE` block followed by one or more `EVALUATE`
/// statements.
///
/// A query always has at least one [`Evaluate`]; the parser rejects `DEFINE` without a following
/// `EVALUATE`. For the simple form `EVALUATE <expression>` the `define` field is `None` and
/// `evaluates` holds exactly one entry with no clauses.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub define: Option<Define>,
    pub evaluates: Vec<Evaluate>,
}

/// The `DEFINE` block of a query. One block applies to every `EVALUATE` statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Define {
    pub items: Vec<Definition>,
}

/// One entry of a `DEFINE` block.
#[derive(Clone, Debug, PartialEq)]
pub enum Definition {
    Measure(MeasureDef),
    Column(ColumnDef),
    Table(TableDef),
    Var(VarDef),
    Function(FunctionDef),
    MParameter(MParameterDef),
}

/// `MEASURE <table>[<measure>] = <scalar expression>`.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasureDef {
    pub name: NameRef,
    pub expr: Expr,
}

/// `COLUMN <table>[<column>] = <scalar expression>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnDef {
    pub name: NameRef,
    pub expr: Expr,
}

/// `VAR <name> = <expression>` in a `DEFINE` block. Unlike an expression-level
/// [`ExprKind::VarReturn`], a query-scoped variable has no `RETURN`.
#[derive(Clone, Debug, PartialEq)]
pub struct VarDef {
    pub name: Name,
    pub expr: Expr,
}

/// `TABLE <name> = <table expression> [WITH VISUAL SHAPE ...]`.
#[derive(Clone, Debug, PartialEq)]
pub struct TableDef {
    pub name: Name,
    pub expr: Expr,
    pub visual_shape: Option<VisualShape>,
}

/// `MPARAMETER <name> = <expression>`: a Power BI dynamic M parameter binding.
#[derive(Clone, Debug, PartialEq)]
pub struct MParameterDef {
    pub name: Name,
    pub expr: Expr,
}

/// A query-defined function: `FUNCTION <name> = (<param>, ...) => <body>`.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionDef {
    pub name: Name,
    pub params: Vec<FunctionParam>,
    pub body: Expr,
}

/// One parameter of a [`FunctionDef`].
///
/// Every annotation is optional: `p`, `p : INT64`, `p : SCALAR INT64 VAL` and `p = 1` are all
/// valid parameter declarations.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionParam {
    pub name: Name,
    pub ty: Option<ParameterType>,
    pub subtype: Option<ParameterSubtype>,
    pub passing: Option<PassingMode>,
    pub default: Option<Expr>,
}

/// The DAX user-defined-function parameter types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParameterType {
    AnyVal,
    Scalar,
    Table,
    AnyRef,
    CalendarRef,
    ColumnRef,
    MeasureRef,
    TableRef,
}

impl ParameterType {
    /// The canonical uppercase keyword.
    pub const fn keyword(self) -> &'static str {
        match self {
            ParameterType::AnyVal => "ANYVAL",
            ParameterType::Scalar => "SCALAR",
            ParameterType::Table => "TABLE",
            ParameterType::AnyRef => "ANYREF",
            ParameterType::CalendarRef => "CALENDARREF",
            ParameterType::ColumnRef => "COLUMNREF",
            ParameterType::MeasureRef => "MEASUREREF",
            ParameterType::TableRef => "TABLEREF",
        }
    }

    /// Recognises a type keyword case-insensitively.
    pub fn from_keyword(word: &str) -> Option<Self> {
        Some(match word.to_ascii_uppercase().as_str() {
            "ANYVAL" => ParameterType::AnyVal,
            "SCALAR" => ParameterType::Scalar,
            "TABLE" => ParameterType::Table,
            "ANYREF" => ParameterType::AnyRef,
            "CALENDARREF" => ParameterType::CalendarRef,
            "COLUMNREF" => ParameterType::ColumnRef,
            "MEASUREREF" => ParameterType::MeasureRef,
            "TABLEREF" => ParameterType::TableRef,
            _ => return None,
        })
    }
}

/// The scalar subtypes, which are only valid together with [`ParameterType::Scalar`] (or alone,
/// as in the documented `p : INT64` shorthand).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParameterSubtype {
    Boolean,
    DateTime,
    Decimal,
    Double,
    Int64,
    Numeric,
    String,
    Variant,
}

impl ParameterSubtype {
    pub const fn keyword(self) -> &'static str {
        match self {
            ParameterSubtype::Boolean => "BOOLEAN",
            ParameterSubtype::DateTime => "DATETIME",
            ParameterSubtype::Decimal => "DECIMAL",
            ParameterSubtype::Double => "DOUBLE",
            ParameterSubtype::Int64 => "INT64",
            ParameterSubtype::Numeric => "NUMERIC",
            ParameterSubtype::String => "STRING",
            ParameterSubtype::Variant => "VARIANT",
        }
    }

    pub fn from_keyword(word: &str) -> Option<Self> {
        Some(match word.to_ascii_uppercase().as_str() {
            "BOOLEAN" => ParameterSubtype::Boolean,
            "DATETIME" => ParameterSubtype::DateTime,
            "DECIMAL" => ParameterSubtype::Decimal,
            "DOUBLE" => ParameterSubtype::Double,
            "INT64" => ParameterSubtype::Int64,
            "NUMERIC" => ParameterSubtype::Numeric,
            "STRING" => ParameterSubtype::String,
            "VARIANT" => ParameterSubtype::Variant,
            _ => return None,
        })
    }
}

/// How a function argument is passed: `VAL` (eagerly) or `EXPR` (lazily).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassingMode {
    Val,
    Expr,
}

impl PassingMode {
    pub const fn keyword(self) -> &'static str {
        match self {
            PassingMode::Val => "VAL",
            PassingMode::Expr => "EXPR",
        }
    }

    pub fn from_keyword(word: &str) -> Option<Self> {
        match word.to_ascii_uppercase().as_str() {
            "VAL" => Some(PassingMode::Val),
            "EXPR" => Some(PassingMode::Expr),
            _ => None,
        }
    }
}

/// One `EVALUATE <table expression> [ORDER BY ...] [START AT ...]` statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Evaluate {
    pub table: Expr,
    pub order_by: Vec<OrderItem>,
    pub start_at: Vec<StartValue>,
}

/// One item of a query-level `ORDER BY` clause.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderItem {
    pub expr: Expr,
    pub direction: SortDirection,
}

/// The sort direction of an `ORDER BY` item. An omitted direction parses as
/// [`SortDirection::Asc`], and the printer writes the keyword explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub const fn keyword(self) -> &'static str {
        match self {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        }
    }

    pub const fn is_ascending(self) -> bool {
        matches!(self, SortDirection::Asc)
    }
}

/// One value of a `START AT` clause: a constant or an XMLA `@parameter` reference.
///
/// Start values are constants, not expressions. A sign in front of a numeric literal is folded
/// into the literal (`START AT -1`); everything else must be a literal or `@name`.
#[derive(Clone, Debug, PartialEq)]
pub enum StartValue {
    Literal(Literal),
    Parameter(Name),
}

/// The `WITH VISUAL SHAPE` clause of a query-defined table.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualShape {
    pub axes: Vec<Axis>,
    /// The optional `DENSIFY "<column>"` clause, always last.
    pub densify: Option<String>,
}

/// One `AXIS ROWS` / `AXIS COLUMNS` clause of a visual shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    pub kind: AxisKind,
    pub groups: Vec<AxisGroup>,
    pub order_by: Vec<AxisOrderItem>,
}

/// The axis of a visual shape. Only these two are documented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AxisKind {
    Rows,
    Columns,
}

impl AxisKind {
    pub const fn keyword(self) -> &'static str {
        match self {
            AxisKind::Rows => "ROWS",
            AxisKind::Columns => "COLUMNS",
        }
    }
}

/// One `GROUP <column>, ... TOTAL <column>` level of an axis.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisGroup {
    pub columns: Vec<NameRef>,
    pub total: NameRef,
}

/// One column of an axis `ORDER BY` clause.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisOrderItem {
    pub column: NameRef,
    pub direction: SortDirection,
}
