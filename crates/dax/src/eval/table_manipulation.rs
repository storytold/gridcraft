//! Table-manipulation functions.
#![allow(unused_imports)]

mod joins;
mod summarizes;

use super::*;

pub(crate) use summarizes::subtotal_columns;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    summarizes::call(ev, name, args, ctx).or_else(|| joins::call(ev, name, args, ctx))
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    summarizes::call_table(ev, name, args, ctx).or_else(|| joins::call_table(ev, name, args, ctx))
}
