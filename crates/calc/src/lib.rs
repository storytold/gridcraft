//! GridCraft's calculation engine: the evaluator (references, names, tables, LET/LAMBDA,
//! dynamic arrays) and dependency-driven recalculation with spills and cycle detection.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod eval;
pub mod par;
pub mod recalc;

pub use eval::{Area, SPECIAL_FUNCTIONS, is_known_function};
pub use recalc::{Calc, CalcProgress, Key, evaluate, now_serial};

#[cfg(test)]
mod tests;
