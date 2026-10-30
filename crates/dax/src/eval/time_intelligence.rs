//! Time-intelligence functions.
#![allow(unused_imports)]

mod ranges;
mod totals;

use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    totals::call(ev, name, args, ctx).or_else(|| ranges::call(ev, name, args, ctx))
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    totals::call_table(ev, name, args, ctx).or_else(|| ranges::call_table(ev, name, args, ctx))
}
