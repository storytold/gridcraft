//! Logical functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The category holds three kinds of functions:
//!
//! - Boolean combinators (`AND`, `OR`, `NOT`) and the constants `TRUE`/`FALSE`, all of which
//!   return a Boolean;
//! - bitwise integer operations (`BITAND`, `BITOR`, `BITXOR`), which are engine-only: dax.guide
//!   does not list them for any Excel version;
//! - scalar selectors (`IF`, `IF.EAGER`, `IFERROR`, `SWITCH`, `COALESCE`), which return a value
//!   whose type follows the selected branch and are therefore catalogued as
//!   [`ReturnType::Any`].

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::Logical, include_str!("logical.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `logical.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; a failure here means the checked-in data
    // is corrupt, which is a programmer error, not a recoverable runtime one.
    #[allow(clippy::panic)]
    let ready = match &*TABLE {
        Ok(functions) => functions,
        Err(error) => panic!("invalid catalog table logical.toml: {error}"),
    };
    ready
}
