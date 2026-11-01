//! The DAX language: tokenizer, syntax tree, Pratt parser and canonical printer.
//!
//! This crate covers the DAX expression grammar and, for now, the query form
//! `EVALUATE <expression>`. It is the foundation for the aggregation, data-type and statement
//! work: later PRs add catalogs and type coercion outside this crate and extend [`Query`] with
//! `DEFINE`/`VAR` clauses.
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
pub mod lexer;
pub mod parser;
pub mod printer;

pub use ast::{BinaryOp, Call, Expr, ExprKind, Literal, Name, NameRef, NameStyle, Query, Span, UnaryOp};
pub use lexer::{LexError, MAX_INPUT_LEN, Token, TokenKind, tokenize};
pub use parser::{MAX_DEPTH, ParseError, parse_expression, parse_query};
pub use printer::{print_expression, print_query};

#[cfg(test)]
mod tests;
