//! The Excel formula language: tokenizer, parser, syntax tree, printer (A1 and R1C1) and
//! reference adjustment for copy/fill and structural edits.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod adjust;
pub mod ast;
pub mod catalog;
pub mod lexer;
pub mod parser;
pub mod printer;

pub use ast::{Anchor, BinOp, Expr, RefKind, Reference, SheetSel, StructItem, StructRef, UnOp};
pub use parser::{KnownNames, ParseError, no_names, parse, parse_local, parse_local_with};
pub use printer::{print, print_local, print_local_with, print_r1c1, print_r1c1_local, quote_sheet};

#[cfg(test)]
mod local_tests;
#[cfg(test)]
mod tests;
