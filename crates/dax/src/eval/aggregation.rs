//! Aggregation functions.
//!
//! The core evaluator switch owns `SUM`, `AVERAGE`, `COUNT`, `COUNTA`, `COUNTBLANK`, `MIN`,
//! `MAX`, `PRODUCT` and the `*X` iterators. This module covers the aggregations the core switch
//! leaves out: the `*A` variants, whose text/Boolean coercions differ from their plain
//! counterparts, and `DISTINCTCOUNTNOBLANK`.
//!
//! All four read a column under the current filter context and delegate the arithmetic to
//! [`support::aggregate_values`]. `AVERAGEA` uses every row in the denominator and coerces
//! BLANK to 0 (Microsoft's DAX example averages 1, 20, "n/a", BLANK, TRUE as 22/5), while
//! `DISTINCTCOUNTNOBLANK` drops BLANK before counting distinct values.
#![allow(unused_imports)]

use super::*;

/// Scalar aggregation functions owned by this module.
pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    match name.to_ascii_uppercase().as_str() {
        "AVERAGEA" | "DISTINCTCOUNTNOBLANK" | "MAXA" | "MINA" => Some(aggregate_column(ev, name, args, ctx)),
        _ => None,
    }
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

/// Evaluates the single column argument and aggregates it. The catalog gives every function in
/// this module exactly one `Column` parameter, so a missing argument is a programming error the
/// binder already rejected; report it as an ordinary evaluation error all the same.
fn aggregate_column(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let column = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a column")))?;
    let values = ev.column_values(column, ctx)?;
    super::support::aggregate_values(name, &values)
}
