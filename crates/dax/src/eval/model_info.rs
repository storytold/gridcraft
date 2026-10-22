//! `INFO.*` metadata functions.
#![allow(unused_imports)]

use super::*;

pub(crate) fn call(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    None
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}
