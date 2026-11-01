//! DAX data types, literal classification, static inference and operator result tables.
//!
//! The eight scalar types are [`DataType`]'s variants, matching
//! <https://dax.guide/datatypes/>: Binary, Boolean, Currency, DateTime, Decimal, Integer,
//! String and Variant. [`PseudoType`] adds `BLANK` and `Table`, which Microsoft documents as DAX
//! types but dax.guide does not list among the eight. They are inference results, not members of
//! [`DataType`].
//!
//! # Parser-level versus evaluator-level
//!
//! This module is static. It classifies literals, infers types for expressions built from
//! literals and operators, and reports clearly incompatible operands from the documented
//! operator tables. It never evaluates an expression.
//!
//! The following belong to the evaluator and are deliberately absent:
//!
//! - text-to-number, text-to-date and text-to-Boolean parsing, and the conversion errors when
//!   text is not numeric (`"22" + 22` succeeds only when the text parses);
//! - BLANK substitution, the BLANK truth tables, `0/BLANK` (NaN) and `5/BLANK` (Infinity);
//! - Currency rounding/truncation and the fixed-point versus floating-point rules for `*` and
//!   `/`;
//! - integer overflow at runtime (literal overflow is a lexer error);
//! - IEEE-754 precision and equality (`0.1 + 0.2 = 0.3` can be false);
//! - string collation, case folding and trailing-space trimming;
//! - date normalization in `DATE`/`TIME`, date subtraction at runtime, and Variant dispatch.
//!
//! # Documented conflicts
//!
//! The published rules are not one consistent lattice; this module preserves the operator
//! tables instead of "fixing" them:
//!
//! - `*` is documented as `Integer * DateTime -> Integer` and `Currency * DateTime ->
//!   Currency`, although no DateTime row exists in the source table. The table is reproduced
//!   cell for cell; a DateTime on the left of `*` is reported as a type error.
//! - Subtraction of two DateTimes is documented as DateTime, while engines commonly produce a
//!   day count. The documented cell is used and the discrepancy is noted in [`TypeMetadata`].
//! - Microsoft's comparison pages both say text is not compared with numbers (an error) and
//!   that a Boolean > String > numeric type rank makes such comparisons succeed. The operator
//!   reference's "error" rule is modelled: comparing String with a numeric type is a
//!   [`TypeError`]. String comparisons are case-insensitive and accent-sensitive at runtime.
//! - Currency and Integer endpoint documentation disagree. The ranges recorded in
//!   [`TypeMetadata`] name the disagreement instead of picking a silent winner.
//! - Excel's embedded engine rejects `==`; dax.guide documents it and this crate parses it.
//!   Strict equality is still modelled here: `==` does not coerce BLANK.
//! - `dt"..."` literals are a Power BI/SSAS feature (August 2021 onwards) and are not supported
//!   by every engine version, despite being documented DAX syntax.

use std::fmt;

use crate::ast::{BinaryOp, Call, Expr, ExprKind, Literal, UnaryOp};

/// The eight scalar DAX data types, in dax.guide's listing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DataType {
    Binary,
    Boolean,
    Currency,
    DateTime,
    Decimal,
    Integer,
    String,
    Variant,
}

impl DataType {
    /// Every scalar type, in dax.guide's listing order.
    pub const ALL: [DataType; 8] = [
        DataType::Binary,
        DataType::Boolean,
        DataType::Currency,
        DataType::DateTime,
        DataType::Decimal,
        DataType::Integer,
        DataType::String,
        DataType::Variant,
    ];

    /// The type's canonical display name.
    pub const fn name(self) -> &'static str {
        match self {
            DataType::Binary => "Binary",
            DataType::Boolean => "Boolean",
            DataType::Currency => "Currency",
            DataType::DateTime => "DateTime",
            DataType::Decimal => "Decimal",
            DataType::Integer => "Integer",
            DataType::String => "String",
            DataType::Variant => "Variant",
        }
    }

    /// Representation, literal availability, range, precision and notes.
    pub const fn metadata(self) -> TypeMetadata {
        match self {
            DataType::Binary => TypeMetadata {
                name: "Binary",
                representation: "opaque byte sequence; not usable as a DAX expression value",
                literal_syntax: None,
                range: "up to 67,108,864 bytes per value in SSAS tabular; unsupported in Power BI models",
                precision: "opaque bytes",
                notes: "A model-only type. No DAX operator or function produces or consumes Binary, so it is inert for tokenizing, parsing and typing.",
            },
            DataType::Boolean => TypeMetadata {
                name: "Boolean",
                representation: "TRUE or FALSE, nullable",
                literal_syntax: Some("TRUE | FALSE (case-insensitive); TRUE() and FALSE() are equivalent calls"),
                range: "TRUE, FALSE",
                precision: "exact",
                notes: "TRUE is 1 and FALSE is 0 when a numeric operator needs a number (TRUE + 1 = 2), but comparisons only pair Boolean with Boolean. BLANK is coerced to FALSE for =, not for ==.",
            },
            DataType::Currency => TypeMetadata {
                name: "Currency",
                representation: "64-bit signed integer scaled by 10,000 (fixed four decimal places)",
                literal_syntax: None,
                range: "about -922,337,203,685,477.5807 to 922,337,203,685,477.5807; Power BI, SSAS and Excel documentation disagree at the endpoints",
                precision: "four decimal places, 19 significant digits",
                notes: "No literal. Produced by Currency columns, CURRENCY() and CONVERT(..., CURRENCY) (where DECIMAL is an alias for Currency), plus aggregation over Currency columns. Addition and subtraction keep fixed-point rules; multiplication and division may yield a floating-point result.",
            },
            DataType::DateTime => TypeMetadata {
                name: "DateTime",
                representation: "double: whole days since 1899-12-30, fraction is the time of day",
                literal_syntax: Some("dt\"YYYY-MM-DD\", dt\"YYYY-MM-DDThh:mm:ss\" or dt\"YYYY-MM-DD hh:mm:ss\""),
                range: "serial 0 is 1899-12-30; maximum is 9999-12-31 00:00:00. Engine support before 1900-03-01 is documented as unreliable, and minimum-date documentation conflicts",
                precision: "Power BI stores time at 1/300 second (3.33 ms)",
                notes: "dt literals were introduced in Power BI Desktop (August 2021) and later in AS/Power Pivot, so not every engine version accepts them. Locale-sensitive text parsing uses DATEVALUE/TIMEVALUE and belongs to the evaluator. Two dates subtracted are documented as DateTime even though engines commonly return a day count.",
            },
            DataType::Decimal => TypeMetadata {
                name: "Decimal",
                representation: "IEEE-754 64-bit double (TOM Double / SQL Float)",
                literal_syntax: Some("digits with a decimal point or exponent: 3.14, .5, 1., 1e3; the sign is the unary operator"),
                range: "-1.79e308 to 1.79e308; NaN and Infinity can result from operations but have no literals",
                precision: "approximately 15-17 significant digits; values are approximate",
                notes: "Division always yields Decimal, including Integer/Integer. Equality is sensitive to binary representation, so compare with tolerance where appropriate. Integer or Currency arithmetic that promotes yields Decimal.",
            },
            DataType::Integer => TypeMetadata {
                name: "Integer",
                representation: "64-bit signed integer (Int64)",
                literal_syntax: Some("digit sequence such as 42; the sign is the unary operator"),
                range: "-9,223,372,036,854,775,808 to 9,223,372,036,854,775,807 (i64). SSAS documents a narrower range because the endpoints are reserved as null sentinels",
                precision: "exact",
                notes: "There is no integer division: / always yields Decimal. Literal overflow is a lexer error; runtime overflow is an evaluator concern. The literal i64::MIN cannot be written, because the sign is a unary operator and its unsigned magnitude overflows.",
            },
            DataType::String => TypeMetadata {
                name: "String",
                representation: "UTF-16 Unicode text",
                literal_syntax: Some("\"text\" with doubled quotes, e.g. \"Say \"\"hi\"\"\""),
                range: "engine-dependent; about 32,000 characters in Power BI and larger in the Excel Data Model",
                precision: "Unicode characters; trailing spaces are trimmed by the engine, leading spaces are significant",
                notes: "The empty string \"\" is not BLANK. Comparison is case-insensitive and accent-sensitive in the engine. Text is not implicitly compared with numbers (the engine errors), but numeric text participates in arithmetic when it parses.",
            },
            DataType::Variant => TypeMetadata {
                name: "Variant",
                representation: "runtime-tagged scalar: Boolean, Integer, Currency, Decimal, DateTime, String or BLANK",
                literal_syntax: None,
                range: "the union of the scalar types",
                precision: "depends on the runtime type",
                notes: "Produced by expressions with mixed-type branches (IF, SWITCH, COALESCE) and some measures. Not a column type in a regular table, and Microsoft's syntax page states DAX does not support Variant; dax.guide and SSAS clarify that expression-level Variant exists. No literal and no operators of its own.",
            },
        }
    }

    /// Integer, Currency and Decimal are the numeric types (Boolean coerces to 1/0 in numeric
    /// operators but is not itself numeric).
    pub const fn is_numeric(self) -> bool {
        matches!(self, DataType::Integer | DataType::Currency | DataType::Decimal)
    }

    /// Whether a literal of this type can be written in DAX.
    pub const fn has_literal(self) -> bool {
        self.metadata().literal_syntax.is_some()
    }
}

/// `BLANK` and `Table` are DAX types in Microsoft's documentation but not members of
/// dax.guide's eight-type list. They are inference results, never [`DataType`] values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoType {
    Blank,
    Table,
}

impl PseudoType {
    pub const ALL: [PseudoType; 2] = [PseudoType::Blank, PseudoType::Table];

    pub const fn name(self) -> &'static str {
        match self {
            PseudoType::Blank => "BLANK",
            PseudoType::Table => "Table",
        }
    }

    pub const fn metadata(self) -> TypeMetadata {
        match self {
            PseudoType::Blank => TypeMetadata {
                name: "BLANK",
                representation: "the absence of a value; distinct from 0, \"\" and FALSE",
                literal_syntax: None,
                range: "not applicable",
                precision: "not applicable",
                notes: "There is no BLANK literal: BLANK() is a zero-argument function. Polymorphic: BLANK()+5 = 5, BLANK()*5 = BLANK, BLANK()=0 is TRUE. Do not silently treat BLANK as Integer; coercion depends on the operator and the other operand.",
            },
            PseudoType::Table => TypeMetadata {
                name: "Table",
                representation: "a set of rows and columns; non-scalar",
                literal_syntax: Some("table constructor { value, ... } or { (value, ...), ... }, plus table references"),
                range: "not applicable",
                precision: "not applicable",
                notes: "DAX has no row-value type: ROW(...) returns a one-row table. A single-column single-row table may be used as a scalar in an expression context; that is a type-checking special case, not a literal rule.",
            },
        }
    }
}

/// One row of type documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeMetadata {
    /// Display name (`Integer`, `BLANK`, `Table`).
    pub name: &'static str,
    /// Internal or model representation.
    pub representation: &'static str,
    /// The literal syntax, or `None` when the type has no literal.
    pub literal_syntax: Option<&'static str>,
    /// Documented range, including conflicting endpoints where they exist.
    pub range: &'static str,
    /// Documented precision.
    pub precision: &'static str,
    /// Behaviour a static type checker cannot model.
    pub notes: &'static str,
}

/// The result of static type inference.
///
/// [`InferredType::Unknown`] is not a DAX type: it means the expression's type depends on the
/// model, a column, a measure or a function catalog. It is distinct from
/// [`InferredType::Data(DataType::Variant)`], which is a real expression-level DAX type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InferredType {
    Data(DataType),
    Blank,
    Table,
    Unknown,
}

impl InferredType {
    pub const fn name(self) -> &'static str {
        match self {
            InferredType::Data(data_type) => data_type.name(),
            InferredType::Blank => "BLANK",
            InferredType::Table => "Table",
            InferredType::Unknown => "unknown",
        }
    }

    pub const fn data(self) -> Option<DataType> {
        match self {
            InferredType::Data(data_type) => Some(data_type),
            InferredType::Blank | InferredType::Table | InferredType::Unknown => None,
        }
    }
}

impl From<DataType> for InferredType {
    fn from(value: DataType) -> Self {
        InferredType::Data(value)
    }
}

impl From<PseudoType> for InferredType {
    fn from(value: PseudoType) -> Self {
        match value {
            PseudoType::Blank => InferredType::Blank,
            PseudoType::Table => InferredType::Table,
        }
    }
}

/// A static type error: operators applied to operands with no documented result type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeError {
    /// The binary operator has no result for these operands, or was used with the wrong kind
    /// of API (for example `Eq` passed to [`arithmetic_type`]).
    IncompatibleOperands { op: BinaryOp, left: InferredType, right: InferredType },
    /// Unary `+`, `-` or `NOT` cannot be applied to this operand.
    IncompatibleUnaryOperand { op: UnaryOp, operand: InferredType },
    /// Two values have no documented common comparison type (used by
    /// [`comparison_coercion`], which takes no operator).
    IncompatibleTypes { left: InferredType, right: InferredType },
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeError::IncompatibleOperands { op, left, right } => {
                write!(f, "operator `{}` cannot combine {} and {}", op.symbol(), left.name(), right.name())
            }
            TypeError::IncompatibleUnaryOperand { op, operand } => {
                write!(f, "unary operator `{}` cannot be applied to {}", op.symbol(), operand.name())
            }
            TypeError::IncompatibleTypes { left, right } => {
                write!(f, "types {} and {} have no common comparison type", left.name(), right.name())
            }
        }
    }
}

impl std::error::Error for TypeError {}

/// Classifies a literal. Every literal form maps to one of the eight scalar types; BLANK has
/// no literal form.
pub const fn literal_type(literal: &Literal) -> DataType {
    match literal {
        Literal::Integer(_) => DataType::Integer,
        Literal::Decimal(_) => DataType::Decimal,
        Literal::DateTime(_) => DataType::DateTime,
        Literal::String(_) => DataType::String,
        Literal::Boolean(_) => DataType::Boolean,
    }
}

/// Infers the static type of an expression.
///
/// Name references are [`InferredType::Unknown`] (they need the model), as are calls other than
/// `TRUE()`, `FALSE()` and `BLANK()`. Incompatible known operands produce a [`TypeError`].
pub fn infer_type(expr: &Expr) -> Result<InferredType, TypeError> {
    match &expr.kind {
        ExprKind::Literal(literal) => Ok(InferredType::Data(literal_type(literal))),
        ExprKind::Name(_) => Ok(InferredType::Unknown),
        ExprKind::Call(call) => {
            // The call itself may be unknown without a signature catalog, but every argument
            // still has to type-check.
            for arg in &call.args {
                infer_type(arg)?;
            }
            Ok(call_type(call))
        }
        ExprKind::Unary { op, expr } => {
            let operand = infer_type(expr)?;
            unary_type(*op, operand)
        }
        ExprKind::Binary { op, left, right } => {
            // `(a, b) IN { (a, b), ... }` uses a row constructor on the left, not a table
            // expression. Validate its elements but let the operator comparison handle the
            // row value.
            let left = match (&left.kind, op) {
                (ExprKind::Tuple(items), BinaryOp::In) => {
                    for item in items {
                        infer_type(item)?;
                    }
                    InferredType::Unknown
                }
                _ => infer_type(left)?,
            };
            let right = infer_type(right)?;
            binary_type(*op, left, right)
        }
        ExprKind::Table(items) | ExprKind::Tuple(items) => {
            // Containers are tables, but their elements still have to type-check.
            for item in items {
                infer_type(item)?;
            }
            Ok(InferredType::Table)
        }
    }
}

/// `TRUE()`, `FALSE()` and `BLANK()` are inferable without a function catalog. Everything else
/// returns [`InferredType::Unknown`]; function signatures are owned by the functions PR.
fn call_type(call: &Call) -> InferredType {
    if call.args.is_empty() {
        if call.name.text.eq_ignore_ascii_case("TRUE") || call.name.text.eq_ignore_ascii_case("FALSE") {
            return InferredType::Data(DataType::Boolean);
        }
        if call.name.text.eq_ignore_ascii_case("BLANK") {
            return InferredType::Blank;
        }
    }
    InferredType::Unknown
}

/// Infers the result type of a unary operator. `+` and `-` preserve the numeric type (DateTime
/// keeps its serial type); `NOT` always yields Boolean. A BLANK operand stays polymorphic
/// instead of becoming Integer.
pub fn unary_type(op: UnaryOp, operand: InferredType) -> Result<InferredType, TypeError> {
    match op {
        UnaryOp::Not => match operand {
            InferredType::Table | InferredType::Data(DataType::Binary) => Err(TypeError::IncompatibleUnaryOperand { op, operand }),
            _ => Ok(InferredType::Data(DataType::Boolean)),
        },
        UnaryOp::Plus | UnaryOp::Neg => match operand {
            InferredType::Data(DataType::Integer) => Ok(InferredType::Data(DataType::Integer)),
            InferredType::Data(DataType::Currency) => Ok(InferredType::Data(DataType::Currency)),
            InferredType::Data(DataType::Decimal) => Ok(InferredType::Data(DataType::Decimal)),
            // TRUE/FALSE are 1/0 when a numeric operator needs a number.
            InferredType::Data(DataType::Boolean) => Ok(InferredType::Data(DataType::Integer)),
            InferredType::Data(DataType::Variant) => Ok(InferredType::Data(DataType::Variant)),
            // DAX documents unary `-` as preserving the operand type, and DateTime is a serial.
            InferredType::Data(DataType::DateTime) => Ok(InferredType::Data(DataType::DateTime)),
            // A text operand may parse as a number at runtime.
            InferredType::Data(DataType::String) | InferredType::Unknown => Ok(InferredType::Unknown),
            InferredType::Blank => Ok(InferredType::Blank),
            InferredType::Data(DataType::Binary) | InferredType::Table => Err(TypeError::IncompatibleUnaryOperand { op, operand }),
        },
    }
}

/// Infers the result type of a binary operator using the documented DAX tables.
pub fn binary_type(op: BinaryOp, left: InferredType, right: InferredType) -> Result<InferredType, TypeError> {
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Pow => arithmetic_type(op, left, right),
        BinaryOp::Concat => {
            reject_non_scalar(op, left, right)?;
            Ok(InferredType::Data(DataType::String))
        }
        BinaryOp::And | BinaryOp::Or => {
            reject_non_scalar(op, left, right)?;
            Ok(InferredType::Data(DataType::Boolean))
        }
        BinaryOp::Eq | BinaryOp::StrictEq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
            comparison_type(op, left, right).map(InferredType::Data)
        }
        BinaryOp::In => in_type(op, left, right),
    }
}

/// `IN` always yields Boolean. Its right operand is a table expression (`{ ... }`, a table
/// reference or `VALUES(...)`), not a scalar.
fn in_type(op: BinaryOp, left: InferredType, right: InferredType) -> Result<InferredType, TypeError> {
    if matches!(left, InferredType::Table | InferredType::Data(DataType::Binary)) {
        return Err(TypeError::IncompatibleOperands { op, left, right });
    }
    match right {
        InferredType::Table | InferredType::Unknown | InferredType::Data(DataType::Variant) => Ok(InferredType::Data(DataType::Boolean)),
        InferredType::Data(_) | InferredType::Blank => Err(TypeError::IncompatibleOperands { op, left, right }),
    }
}

/// Infers the result of `+`, `-`, `*`, `/` or `^`.
///
/// Unknown operands stay unknown. Text operands stay unknown because only the evaluator can
/// decide whether the text parses as a number. Variant propagates. BLANK is polymorphic and
/// follows the documented special cases instead of being treated as Integer.
pub fn arithmetic_type(op: BinaryOp, left: InferredType, right: InferredType) -> Result<InferredType, TypeError> {
    if !matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Pow) {
        return Err(TypeError::IncompatibleOperands { op, left, right });
    }
    reject_non_scalar(op, left, right)?;

    if left == InferredType::Blank || right == InferredType::Blank {
        return blank_arithmetic(op, left, right);
    }
    if matches!(left, InferredType::Data(DataType::Variant)) || matches!(right, InferredType::Data(DataType::Variant)) {
        return Ok(InferredType::Data(DataType::Variant));
    }
    if left == InferredType::Unknown || right == InferredType::Unknown {
        return Ok(InferredType::Unknown);
    }
    if matches!(left, InferredType::Data(DataType::String)) || matches!(right, InferredType::Data(DataType::String)) {
        // `"22" + 22` succeeds only if the text parses; that is evaluator work.
        return Ok(InferredType::Unknown);
    }

    let converted_left = coerce_boolean(left);
    let converted_right = coerce_boolean(right);
    let (Some(left_data), Some(right_data)) = (converted_left.data(), converted_right.data()) else {
        return Err(TypeError::IncompatibleOperands { op, left, right });
    };

    let result = match op {
        BinaryOp::Add => numeric_cell(&ADDITION, left_data, right_data),
        BinaryOp::Sub => numeric_cell(&SUBTRACTION, left_data, right_data),
        BinaryOp::Mul => multiplication_cell(left_data, right_data),
        BinaryOp::Div => numeric_cell(&DIVISION, left_data, right_data),
        BinaryOp::Pow => match (left_data, right_data) {
            // `^` is not covered by the conversion tables; the engine computes a double.
            (DataType::DateTime, _) | (_, DataType::DateTime) => None,
            _ => Some(DataType::Decimal),
        },
        _ => None,
    };
    result.map(InferredType::Data).ok_or(TypeError::IncompatibleOperands { op, left, right })
}

/// BLANK arithmetic follows the documented special cases:
/// `BLANK()+5 = 5`, `BLANK()*5 = BLANK`, `5/BLANK() = Infinity` and
/// `BLANK()/BLANK() = BLANK`. The runtime values are evaluator work; only the result types are
/// recorded here.
fn blank_arithmetic(op: BinaryOp, left: InferredType, right: InferredType) -> Result<InferredType, TypeError> {
    let left_blank = left == InferredType::Blank;
    let right_blank = right == InferredType::Blank;
    match op {
        BinaryOp::Add | BinaryOp::Sub => {
            if left_blank && right_blank {
                Ok(InferredType::Blank)
            } else {
                let other = if left_blank { right } else { left };
                Ok(neutral_arithmetic(other))
            }
        }
        // BLANK is absorbing in multiplication.
        BinaryOp::Mul => Ok(InferredType::Blank),
        BinaryOp::Div => {
            if left_blank {
                if right_blank {
                    Ok(InferredType::Blank)
                } else if right == InferredType::Unknown {
                    Ok(InferredType::Unknown)
                } else {
                    Ok(InferredType::Blank)
                }
            } else if right_blank {
                if left == InferredType::Unknown {
                    Ok(InferredType::Unknown)
                } else {
                    // 5/BLANK() is Infinity and 0/BLANK() is NaN: both are Decimal.
                    Ok(InferredType::Data(DataType::Decimal))
                }
            } else {
                Ok(InferredType::Unknown)
            }
        }
        BinaryOp::Pow => Ok(InferredType::Blank),
        _ => Err(TypeError::IncompatibleOperands { op, left, right }),
    }
}

/// The result of `BLANK + x` / `BLANK - x`: BLANK is neutral, so `x`'s numeric type survives.
fn neutral_arithmetic(other: InferredType) -> InferredType {
    match other {
        InferredType::Data(DataType::Boolean) => InferredType::Data(DataType::Integer),
        InferredType::Data(DataType::String) => InferredType::Unknown,
        InferredType::Data(DataType::Variant) => InferredType::Data(DataType::Variant),
        InferredType::Data(DataType::Binary) | InferredType::Table => InferredType::Unknown,
        other => other,
    }
}

const fn coerce_boolean(value: InferredType) -> InferredType {
    match value {
        InferredType::Data(DataType::Boolean) => InferredType::Data(DataType::Integer),
        other => other,
    }
}

/// Resolves the common type for a non-strict comparison (`=`, `<>`, `<`, `>`, `<=`, `>=`).
///
/// BLANK is coerced to the other operand's type (`0`, `""` or `FALSE` according to that type):
/// `BLANK() = 0` is TRUE. Text is never compared implicitly with a number or a Boolean; those
/// combinations are a [`TypeError`]. `==` does not use this function, because strict equality
/// does not coerce BLANK.
pub fn comparison_coercion(left: InferredType, right: InferredType) -> Result<InferredType, TypeError> {
    if matches!(left, InferredType::Table | InferredType::Data(DataType::Binary))
        || matches!(right, InferredType::Table | InferredType::Data(DataType::Binary))
    {
        return Err(TypeError::IncompatibleTypes { left, right });
    }
    if left == InferredType::Blank && right == InferredType::Blank {
        return Ok(InferredType::Blank);
    }
    if left == InferredType::Blank {
        return Ok(right);
    }
    if right == InferredType::Blank {
        return Ok(left);
    }
    match (left, right) {
        (InferredType::Unknown, _) | (_, InferredType::Unknown) => Ok(InferredType::Unknown),
        (InferredType::Data(DataType::Variant), _) | (_, InferredType::Data(DataType::Variant)) => Ok(InferredType::Data(DataType::Variant)),
        (InferredType::Data(DataType::Boolean), InferredType::Data(DataType::Boolean)) => Ok(InferredType::Data(DataType::Boolean)),
        (InferredType::Data(DataType::String), InferredType::Data(DataType::String)) => Ok(InferredType::Data(DataType::String)),
        (InferredType::Data(left_data), InferredType::Data(right_data)) => {
            numeric_cell(&COMPARISON, left_data, right_data).map(InferredType::Data).ok_or(TypeError::IncompatibleTypes { left, right })
        }
        (InferredType::Table, _) | (_, InferredType::Table) => Err(TypeError::IncompatibleTypes { left, right }),
        // BLANK returns above; this arm keeps the match exhaustive without a panic.
        (InferredType::Blank, _) | (_, InferredType::Blank) => Ok(InferredType::Blank),
    }
}

/// Checks whether a comparison operator accepts the two operands.
///
/// Non-strict operators use [`comparison_coercion`]. `==` is strict: BLANK is never replaced
/// with `0`, `""` or FALSE, though a BLANK operand is still comparable. Numeric/DateTime
/// pairs keep the documented comparison matrix; String/Boolean pairs are same-type only.
pub fn comparison_compatible(op: BinaryOp, left: InferredType, right: InferredType) -> Result<(), TypeError> {
    if !op.is_comparison() {
        return Err(TypeError::IncompatibleOperands { op, left, right });
    }
    if matches!(left, InferredType::Table | InferredType::Data(DataType::Binary))
        || matches!(right, InferredType::Table | InferredType::Data(DataType::Binary))
    {
        return Err(TypeError::IncompatibleOperands { op, left, right });
    }
    if op == BinaryOp::StrictEq {
        if left == InferredType::Blank || right == InferredType::Blank {
            return Ok(());
        }
        let compatible = matches!(
            (left, right),
            (InferredType::Unknown, _)
                | (_, InferredType::Unknown)
                | (InferredType::Data(DataType::Variant), _)
                | (_, InferredType::Data(DataType::Variant))
                | (InferredType::Data(DataType::Boolean), InferredType::Data(DataType::Boolean))
                | (InferredType::Data(DataType::String), InferredType::Data(DataType::String))
        ) || matches!(
            (left.data(), right.data()),
            (Some(left_data), Some(right_data))
                if numeric_index(left_data).is_some() && numeric_index(right_data).is_some()
        );
        return if compatible { Ok(()) } else { Err(TypeError::IncompatibleOperands { op, left, right }) };
    }
    comparison_coercion(left, right).map(|_| ()).map_err(|_| TypeError::IncompatibleOperands { op, left, right })
}

/// The result of any comparison is Boolean. This also checks operand compatibility.
pub fn comparison_type(op: BinaryOp, left: InferredType, right: InferredType) -> Result<DataType, TypeError> {
    comparison_compatible(op, left, right)?;
    Ok(DataType::Boolean)
}

fn reject_non_scalar(op: BinaryOp, left: InferredType, right: InferredType) -> Result<(), TypeError> {
    match (left, right) {
        (InferredType::Table, _)
        | (_, InferredType::Table)
        | (InferredType::Data(DataType::Binary), _)
        | (_, InferredType::Data(DataType::Binary)) => Err(TypeError::IncompatibleOperands { op, left, right }),
        _ => Ok(()),
    }
}

// Documented operator result tables (rows = left operand, columns = right operand) for
// Integer, Currency, Decimal ("Real") and DateTime. They are transcribed cell for cell.

const NUMERIC_ORDER: [DataType; 4] = [DataType::Integer, DataType::Currency, DataType::Decimal, DataType::DateTime];

const ADDITION: [[DataType; 4]; 4] = [
    [DataType::Integer, DataType::Currency, DataType::Decimal, DataType::DateTime],
    [DataType::Currency, DataType::Currency, DataType::Decimal, DataType::DateTime],
    [DataType::Decimal, DataType::Decimal, DataType::Decimal, DataType::DateTime],
    [DataType::DateTime, DataType::DateTime, DataType::DateTime, DataType::DateTime],
];

const SUBTRACTION: [[DataType; 4]; 4] = [
    [DataType::Integer, DataType::Currency, DataType::Decimal, DataType::Decimal],
    [DataType::Currency, DataType::Currency, DataType::Decimal, DataType::Decimal],
    [DataType::Decimal, DataType::Decimal, DataType::Decimal, DataType::Decimal],
    [DataType::DateTime, DataType::DateTime, DataType::DateTime, DataType::DateTime],
];

const MULTIPLICATION: [[Option<DataType>; 4]; 4] = [
    [Some(DataType::Integer), Some(DataType::Currency), Some(DataType::Decimal), Some(DataType::Integer)],
    [Some(DataType::Currency), Some(DataType::Decimal), Some(DataType::Currency), Some(DataType::Currency)],
    [Some(DataType::Decimal), Some(DataType::Currency), Some(DataType::Decimal), Some(DataType::Decimal)],
    [None, None, None, None],
];

const DIVISION: [[DataType; 4]; 4] = [
    [DataType::Decimal, DataType::Currency, DataType::Decimal, DataType::Decimal],
    [DataType::Currency, DataType::Decimal, DataType::Currency, DataType::Decimal],
    [DataType::Decimal, DataType::Decimal, DataType::Decimal, DataType::Decimal],
    [DataType::Decimal, DataType::Decimal, DataType::Decimal, DataType::Decimal],
];

const COMPARISON: [[DataType; 4]; 4] = [
    [DataType::Integer, DataType::Currency, DataType::Decimal, DataType::Decimal],
    [DataType::Currency, DataType::Currency, DataType::Decimal, DataType::Decimal],
    [DataType::Decimal, DataType::Decimal, DataType::Decimal, DataType::Decimal],
    [DataType::Decimal, DataType::Decimal, DataType::Decimal, DataType::DateTime],
];

const fn numeric_index(data_type: DataType) -> Option<usize> {
    match data_type {
        DataType::Integer => Some(0),
        DataType::Currency => Some(1),
        DataType::Decimal => Some(2),
        DataType::DateTime => Some(3),
        DataType::Binary | DataType::Boolean | DataType::String | DataType::Variant => None,
    }
}

fn numeric_cell(matrix: &[[DataType; 4]; 4], left: DataType, right: DataType) -> Option<DataType> {
    let row = numeric_index(left)?;
    let column = numeric_index(right)?;
    matrix.get(row)?.get(column).copied()
}

fn multiplication_cell(left: DataType, right: DataType) -> Option<DataType> {
    let row = numeric_index(left)?;
    let column = numeric_index(right)?;
    MULTIPLICATION.get(row)?.get(column).copied().flatten()
}

/// The row/column order of the operator tables, exposed for tests and tooling that wants to
/// verify a particular documented cell.
pub const fn numeric_matrix_order() -> [DataType; 4] {
    NUMERIC_ORDER
}
