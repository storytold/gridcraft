//! Parent-child functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! These five functions flatten a parent-child hierarchy into a delimited path. The delimiter
//! is always the vertical bar (`|`); `PATH` produces the string, `PATHLENGTH` counts its items,
//! `PATHCONTAINS` searches it, and `PATHITEM`/`PATHITEMREVERSE` extract one item from the left
//! or the right end.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{self, CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::ParentChild, include_str!("parent_child.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `parent_child.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; corrupt checked-in data is recorded for
    // that test and served as an empty table, never a panic (CodeRabbit, #3412).
    catalog::shared(&TABLE)
}
