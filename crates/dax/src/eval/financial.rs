//! Financial functions.
#![allow(unused_imports)]

mod loans;
mod securities;

use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    securities::call(ev, name, args, ctx).or_else(|| loans::call(ev, name, args, ctx))
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    securities::call_table(ev, name, args, ctx).or_else(|| loans::call_table(ev, name, args, ctx))
}
