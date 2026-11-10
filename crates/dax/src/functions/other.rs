//! Other functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The category collects the bit-shift operators, the row-context helpers, the window ranking
//! functions, the metadata table function and the remaining special functions that dax.guide
//! lists under "Other". `INFO.STORAGETABLES` keeps the exact dotted spelling and shares the
//! optional restriction-pair syntax of the other `INFO.*` functions.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> = LazyLock::new(|| load(FunctionCategory::Other, include_str!("other.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `other.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; a failure here means the checked-in data
    // is corrupt, which is a programmer error, not a recoverable runtime one.
    #[allow(clippy::panic)]
    let ready = match &*TABLE {
        Ok(functions) => functions,
        Err(error) => panic!("invalid catalog table other.toml: {error}"),
    };
    ready
}
