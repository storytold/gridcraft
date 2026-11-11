//! Information functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The `INFO.*` records take no required arguments: dax.guide documents an optional, repeatable
//! `RestrictionName` / `RestrictionValue` pair for every one of them, so their arity is open and
//! the descriptive signature keeps both positions optional. The `INFO.VIEW.*` functions take no
//! arguments at all.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{self, CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::Information, include_str!("information.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `information.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; corrupt checked-in data is recorded for
    // that test and served as an empty table, never a panic (CodeRabbit, #3412).
    catalog::shared(&TABLE)
}
