//! Statistical functions.
#![allow(unused_imports)]

mod aggregates;
mod distributions;

use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    aggregates::call(ev, name, args, ctx).or_else(|| distributions::call(ev, name, args, ctx))
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    aggregates::call_table(ev, name, args, ctx).or_else(|| distributions::call_table(ev, name, args, ctx))
}
