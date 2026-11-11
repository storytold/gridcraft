//! Text functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The category splits into three kinds of functions:
//!
//! - string construction (`COMBINEVALUES`, `CONCATENATE`, `CONCATENATEX`, `LEFT`, `MID`,
//!   `REPLACE`, `REPT`, `RIGHT`, `SUBSTITUTE`, `TRIM`), which return text and whose result
//!   length is measured in Unicode code points;
//! - classification and conversion (`EXACT`, `FIND`, `LEN`, `LOWER`, `SEARCH`, `UNICHAR`,
//!   `UNICODE`, `UPPER`), which are case- or accent-sensitive to different degrees;
//! - formatting and serialization (`FIXED`, `FORMAT`, `TOCSV`, `TOJSON`, `VALUE`), whose output
//!   or parsing depends on the model locale.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{self, CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> = LazyLock::new(|| load(FunctionCategory::Text, include_str!("text.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `text.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; corrupt checked-in data is recorded for
    // that test and served as an empty table, never a panic (CodeRabbit, #3412).
    catalog::shared(&TABLE)
}
