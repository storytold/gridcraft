//! Filter functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! This category covers table expressions that inspect or remove filter context (`ALL`,
//! `ALLSELECTED`, ...), the two functions that build a filter context (`CALCULATE`,
//! `CALCULATETABLE`), and the window/apply clauses (`INDEX`, `OFFSET`, `WINDOW`, `ORDERBY`,
//! `PARTITIONBY`, `MATCHBY`) that are only meaningful inside one of those table expressions.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{self, CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::Filter, include_str!("filter.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `filter.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; corrupt checked-in data is recorded for
    // that test and served as an empty table, never a panic (CodeRabbit, #3412).
    catalog::shared(&TABLE)
}
