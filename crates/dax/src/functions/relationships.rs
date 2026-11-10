//! Relationship management functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The four functions mirror <https://dax.guide/functions/relationships-management/> in index
//! order. `CROSSFILTER` and `USERELATIONSHIP` are filter modifiers: they return no value and
//! only alter filter propagation while the filter context that contains them is evaluated.
//! `RELATED` and `RELATEDTABLE` traverse an existing relationship from a row context.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::RelationshipManagement, include_str!("relationships.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `relationships.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; a failure here means the checked-in data
    // is corrupt, which is a programmer error, not a recoverable runtime one.
    #[allow(clippy::panic)]
    let ready = match &*TABLE {
        Ok(functions) => functions,
        Err(error) => panic!("invalid catalog table relationships.toml: {error}"),
    };
    ready
}
