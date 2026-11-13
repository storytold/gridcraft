//! GridCraft's calculation engine: the evaluator (references, names, tables, LET/LAMBDA,
//! dynamic arrays) and dependency-driven recalculation with spills and cycle detection.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod eval;
mod precision;
pub mod recalc;
pub mod simulation;
mod spill_index;
mod trimrange;
pub mod work;

pub use eval::{Area, SPECIAL_FUNCTIONS, is_known_function};
pub use precision::DisplayedPrecision;
pub use recalc::{Calc, Key, evaluate, now_serial};
pub use simulation::{SimulationHook, SimulationRequest, SimulationSite};
pub use work::WorkHook;

#[cfg(test)]
mod tests;
