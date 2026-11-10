//! Time intelligence functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! Every function in this category assumes a well-formed date table: a contiguous range of
//! dates, one unique date/date-time column marked as the date table, and a relationship to the
//! fact table based on that column (or the table explicitly marked as a date table). The
//! optional `YearEndDate` arguments control fiscal years; the week-based functions and the
//! calendar-only arguments introduced with model calendars require a calendar reference rather
//! than a date column and are not available to Excel's embedded engine.

use std::sync::LazyLock;

use super::FunctionCategory;
use super::catalog::{CatalogError, load};

static TABLE: LazyLock<Result<Vec<super::FunctionSpec>, CatalogError>> =
    LazyLock::new(|| load(FunctionCategory::TimeIntelligence, include_str!("time_intelligence.toml")));

/// Every function in this category, in dax.guide index order. Rows live in
/// `time_intelligence.toml`; see [`crate::functions::catalog`] for the schema.
pub fn functions() -> &'static [super::FunctionSpec] {
    // The TOML tables are validated by `catalog::load` and pinned by the
    // field-by-field fidelity test; a failure here means the checked-in data
    // is corrupt, which is a programmer error, not a recoverable runtime one.
    #[allow(clippy::panic)]
    let ready = match &*TABLE {
        Ok(functions) => functions,
        Err(error) => panic!("invalid catalog table time_intelligence.toml: {error}"),
    };
    ready
}
