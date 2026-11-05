//! Time-intelligence functions.
#![allow(unused_imports)]

mod ranges;
mod totals;

use super::*;

/// Whether `data_type` names one of the two date types. `CALENDARAUTO`, the range functions and
/// the total functions share this test, so a column the model declares as `Date` counts as a date
/// everywhere instead of only in some of them.
pub(super) fn is_date_type(data_type: &str) -> bool {
    data_type.eq_ignore_ascii_case("DateTime") || data_type.eq_ignore_ascii_case("Date")
}

/// [`is_date_type`] for a column's optional declared type: an undeclared type is not judged.
pub(super) fn is_date_column(data_type: Option<&str>) -> bool {
    data_type.is_none_or(is_date_type)
}

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    totals::call(ev, name, args, ctx).or_else(|| ranges::call(ev, name, args, ctx))
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    totals::call_table(ev, name, args, ctx).or_else(|| ranges::call_table(ev, name, args, ctx))
}
