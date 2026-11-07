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

/// The month and day a `YearEndDate` argument names. The range functions (`DATESYTD`, ...) and the
/// total functions (`TOTALYTD`, ...) both parse that argument here, so the documented `"6/30/2008"`
/// and `"6/30"` forms behave the same in both families and an impossible day (`"2/30"`) is rejected
/// by both instead of being clamped in one and rejected in the other. A month/day-only form is
/// checked against a leap year, so `"2/29"` stays valid.
pub(super) fn year_end_parts(value: &Scalar) -> Option<(i64, i64)> {
    let (year, month, day) = match value {
        Scalar::String(text) => {
            let parts: Vec<&str> = text.trim().split(['/', '-']).map(str::trim).collect();
            // An empty component (`"6/-30"`, `"6/"`) is not a date: reject it instead of letting
            // the remaining parts parse as a valid month and day.
            if parts.iter().any(|part| part.is_empty()) {
                return None;
            }
            let numbers: Vec<i64> = parts.iter().map(|part| part.parse::<i64>().ok()).collect::<Option<Vec<i64>>>()?;
            match numbers.as_slice() {
                // `M/D`: only the month and day matter, and the year is never read back.
                [month, day] => (2000, *month, *day),
                // `Y/M/D` and `M/D/Y` are both documented shapes; a two-digit year is ignored.
                [first, second, third] if *first > 31 => (*first, *second, *third),
                [month, day, year] => (if *year > 99 { *year } else { 2000 }, *month, *day),
                _ => return None,
            }
        }
        other => {
            let (year, month, day) = super::support::date_parts(super::support::date_value(other)?);
            (year, month, day)
        }
    };
    ((1..=12).contains(&month) && day >= 1 && day <= super::support::days_in_month(year, month)).then_some((month, day))
}

/// The column a table argument contributes to the range and total functions, chosen by one rule so
/// `DATESWTD(Calendar)` and `TOTALWTD(…, Calendar)` never disagree: a column whose declared type is
/// a date type, then a column named `Date` that is not declared as another type, then — only when it
/// cannot be ambiguous — the table's single untyped column.
pub(super) fn table_date_column(table: &crate::model::ModelTable) -> Option<usize> {
    if let Some(index) = table.columns.iter().position(|column| column.data_type.as_deref().is_some_and(is_date_type)) {
        return Some(index);
    }
    if let Some(index) =
        table.columns.iter().position(|column| column.name.eq_ignore_ascii_case("Date") && is_date_column(column.data_type.as_deref()))
    {
        return Some(index);
    }
    let untyped: Vec<usize> = table.columns.iter().enumerate().filter(|(_, column)| column.data_type.is_none()).map(|(index, _)| index).collect();
    match untyped.as_slice() {
        [index] => Some(*index),
        _ => None,
    }
}

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    totals::call(ev, name, args, ctx).or_else(|| ranges::call(ev, name, args, ctx))
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    totals::call_table(ev, name, args, ctx).or_else(|| ranges::call_table(ev, name, args, ctx))
}
