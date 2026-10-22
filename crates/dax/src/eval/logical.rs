//! Logical functions.
//!
//! This module owns the engine-only bitwise integer operations and `IF.EAGER`. `IF`, `AND`, `OR`,
//! `NOT`, `COALESCE` and `IFERROR` live in the core evaluator because they need lazy argument
//! evaluation; `IF.EAGER` is the documented opposite: it forces both branches to be evaluated
//! before the condition selects one.

use super::support::{args as arg_values, eval_all};
use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let name = name.to_ascii_uppercase();
    match name.as_str() {
        "BITAND" | "BITOR" | "BITXOR" => Some(bitwise(&name, ev, args, ctx)),
        "IF.EAGER" => Some(if_eager(ev, args, ctx)),
        _ => None,
    }
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

fn bitwise(name: &str, ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = arg_values(name, &values);
    let left = bit_operand(a.number(0)?, name)?;
    let right = bit_operand(a.number(1)?, name)?;
    let result = match name {
        "BITAND" => left & right,
        "BITOR" => left | right,
        "BITXOR" => left ^ right,
        _ => return Err(EvalError::new(format!("{name} is not a bitwise function"))),
    };
    Ok(Scalar::Integer(result))
}

/// Bit operations use the truncated two's-complement `i64` value; DAX documents a signed 64-bit
/// result, so negative operands participate through their two's-complement representation.
fn bit_operand(value: f64, name: &str) -> Result<i64, EvalError> {
    if !value.is_finite() || value.trunc() < -9_223_372_036_854_775_808.0 || value.trunc() >= 9_223_372_036_854_775_808.0 {
        return Err(EvalError::new(format!("{name} argument is outside the 64-bit integer range")));
    }
    Ok(value.trunc() as i64)
}

/// `IF.EAGER` evaluates the condition and both branch expressions before choosing a result, so an
/// error in the branch that would not be selected still propagates.
fn if_eager(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = arg_values("IF.EAGER", &values);
    let condition = a.boolean(0)?;
    let when_true = a.arg(1)?;
    if condition {
        return Ok(when_true.clone());
    }
    Ok(a.optional(2).cloned().unwrap_or(Scalar::Blank))
}
