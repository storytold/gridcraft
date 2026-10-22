//! The DAX language: tokenizer, syntax tree, Pratt parser, canonical printer and static types.
//!
//! This crate covers the DAX expression grammar, the aggregation function catalog with static
//! call validation, the static type surface (literal classification, inference and the documented
//! operator result tables) and the DAX query statements: `DEFINE` (`MEASURE`, `COLUMN`, `TABLE`,
//! `VAR`, `FUNCTION`, `MPARAMETER`), `EVALUATE`, `ORDER BY`, `START AT`, expression-level
//! `VAR ... RETURN`, and `WITH VISUAL SHAPE` (`AXIS`, `GROUP`, `TOTAL`, `DENSIFY`). It does not
//! evaluate expressions.
//!
//! ```
//! use gridcraft_dax::{parse_expression, print_expression};
//!
//! let expr = parse_expression("1 + 2 * 3")?;
//! assert_eq!(print_expression(&expr), "1 + 2 * 3");
//! # Ok::<(), gridcraft_dax::ParseError>(())
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod ast;
pub mod functions;
pub mod lexer;
pub mod parser;
pub mod printer;
pub mod types;
pub mod validate;

pub use ast::{
    Axis, AxisGroup, AxisKind, AxisOrderItem, BinaryOp, Call, ColumnDef, DateTimeLiteral, Define, Definition, Evaluate, Expr, ExprKind, FunctionDef,
    FunctionParam, Literal, MParameterDef, MeasureDef, Name, NameRef, NameStyle, OrderItem, ParameterSubtype, ParameterType, PassingMode, Query,
    SortDirection, Span, StartValue, TableDef, UnaryOp, VarDecl, VarDef, VisualShape,
};
pub use functions::{
    BlankPolicy, CATALOG, FunctionCategory, FunctionFlags, FunctionSpec, Overload, ParamKind, Parameter, ReturnType, catalog, lookup,
};
pub use lexer::{LexError, MAX_INPUT_LEN, Token, TokenKind, tokenize};
pub use parser::{MAX_DEPTH, ParseError, parse_expression, parse_query};
pub use printer::{print_expression, print_query};
pub use validate::{ValidationError, ValidationErrorKind, validate_call, validate_expression};

#[cfg(test)]
mod tests;
