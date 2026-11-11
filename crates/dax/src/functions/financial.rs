//! Financial functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! All financial functions are Excel-compatible scalar calculations. Functions that take a
//! `Basis` argument use it as the day-count convention: 0 or omitted is US (NASD) 30/360,
//! 1 is actual/actual, 2 is actual/360, 3 is actual/365 and 4 is European 30/360. Functions
//! that take `Type` use 0 or omitted for payments at the end of the period and 1 for payments
//! at the beginning. Settlement must precede maturity, and frequency must be 1, 2 or 4 where a
//! coupon frequency is accepted. These restrictions are documented rather than modelled as
//! parameter kinds, because they depend on values, not on expression shape.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{self, CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::Financial, include_str!("financial.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `financial.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; corrupt checked-in data is recorded for
    // that test and served as an empty table, never a panic (CodeRabbit, #3412).
    catalog::shared(&TABLE)
}
