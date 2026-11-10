//! Statistical functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples. The order matches
//! <https://dax.guide/functions/statistical/>.
//!
//! `LINEST`, `LINESTX` and `SAMPLE` have open-ended signatures: their repeated column/expression
//! groups (and the trailing `Const`/`Order` argument) are modelled with a variadic overload and
//! [`usize::MAX`] as the upper arity bound. All other functions have a closed arity range.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::Statistical, include_str!("statistical.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `statistical.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; a failure here means the checked-in data
    // is corrupt, which is a programmer error, not a recoverable runtime one.
    #[allow(clippy::panic)]
    let ready = match &*TABLE {
        Ok(functions) => functions,
        Err(error) => panic!("invalid catalog table statistical.toml: {error}"),
    };
    ready
}
