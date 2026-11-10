//! Visual calculation functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! All functions in this category are Power BI visual calculation functions: they can only be
//! used in the visual calculation editor and are rejected in measures, calculated columns and
//! calculated tables. The `axis` parameter is defined by the VISUAL SHAPE query syntax emitted
//! by the visual calculation feature, so `ROWS` and `COLUMNS` appear in examples as bare names.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::VisualCalculations, include_str!("visual_calculations.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `visual_calculations.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; a failure here means the checked-in data
    // is corrupt, which is a programmer error, not a recoverable runtime one.
    #[allow(clippy::panic)]
    let ready = match &*TABLE {
        Ok(functions) => functions,
        Err(error) => panic!("invalid catalog table visual_calculations.toml: {error}"),
    };
    ready
}
