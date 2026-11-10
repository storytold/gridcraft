//! Table manipulation functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The category holds four kinds of functions:
//!
//! - table constructors and projections (`DATATABLE`, `ROW`, `ADDCOLUMNS`, `SELECTCOLUMNS`,
//!   `SUMMARIZE`, `SUMMARIZECOLUMNS`, `GROUPBY`);
//! - relational operations (`CROSSJOIN`, `UNION`, `EXCEPT`, `INTERSECT`, the natural joins and
//!   `SUBSTITUTEWITHINDEX`);
//! - companion functions that are only valid as arguments of those operations (`CURRENTGROUP`,
//!   `IGNORE`, `NONVISUAL`, `NATURALJOINUSAGE`, `ROLLUP`, `ROLLUPGROUP`,
//!   `ROLLUPADDISSUBTOTAL`, `ROLLUPISSUBTOTAL`);
//! - row selection and value helpers (`DISTINCT`, `VALUES`, `FILTERS`, `TOPN`, `TOPNSKIP`,
//!   `GENERATESERIES`, `DETAILROWS`, `TREATAS`, `SAMPLEAXISWITHLOCALMINMAX`,
//!   `TOPNPERLEVEL`).
//!
//! The companion functions are catalogued because the parser accepts them, but several do not
//! return a value on their own: they are syntax markers inside `SUMMARIZE`,
//! `SUMMARIZECOLUMNS` or `ADDMISSINGITEMS`, so their return type is [`ReturnType::Blank`].

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::TableManipulation, include_str!("table_manipulation.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `table_manipulation.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; a failure here means the checked-in data
    // is corrupt, which is a programmer error, not a recoverable runtime one.
    #[allow(clippy::panic)]
    let ready = match &*TABLE {
        Ok(functions) => functions,
        Err(error) => panic!("invalid catalog table table_manipulation.toml: {error}"),
    };
    ready
}
