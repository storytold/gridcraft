//! Parent-child functions.
//!
//! `PATH`, `PATHCONTAINS`, `PATHITEM` and `PATHLENGTH` are plain path-string operations handled
//! by the core evaluator's scalar switch. `PATHITEMREVERSE` is the mirror image of `PATHITEM`:
//! it indexes the delimited path from the last item, so position 1 is the current item and
//! position 2 its immediate parent. A position below 1 or past the end of the path is BLANK,
//! exactly like an out-of-range `PATHITEM`.
#![allow(unused_imports)]

use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    match name.to_ascii_uppercase().as_str() {
        "PATHITEMREVERSE" => Some(item_reverse(ev, name, args, ctx)),
        _ => None,
    }
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

/// `PATHITEMREVERSE ( <Path>, <Position> [, <Type> ] )`.
///
/// The path is split on the hierarchy separator (`|`) and the item is taken at
/// `len - position`. `Type` selects the result: `TEXT` (or 0), the default, returns the item as
/// a string; `INTEGER` (or 1) converts it to an integer and errors when the item is not one.
fn item_reverse(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = super::support::eval_all(ev, args, ctx)?;
    let args = super::support::args(name, &values);
    let path = args.text(0)?;
    let position = args.number(1)?;
    let integer = match args.optional(2) {
        None => false,
        Some(Scalar::Blank) => false,
        Some(Scalar::String(text)) if text.eq_ignore_ascii_case("TEXT") => false,
        Some(Scalar::String(text)) if text.eq_ignore_ascii_case("INTEGER") => true,
        Some(value) => match value.to_number() {
            Some(0.0) => false,
            Some(1.0) => true,
            _ => return Err(EvalError::new(format!("{name} Type must be TEXT (0) or INTEGER (1)"))),
        },
    };

    // A path always has at least one item, so only positions below 1 or past the last item are
    // out of range. Non-finite positions cannot index anything.
    if !position.is_finite() || position < 1.0 {
        return Ok(Scalar::Blank);
    }
    let parts: Vec<&str> = path.split('|').collect();
    let position = position.trunc() as usize;
    let Some(item) = parts.len().checked_sub(position).and_then(|index| parts.get(index)) else {
        return Ok(Scalar::Blank);
    };

    if integer {
        item.trim().parse::<i64>().map(Scalar::Integer).map_err(|_| EvalError::new(format!("{name} item `{item}` is not an integer")))
    } else {
        Ok(Scalar::String((*item).to_string()))
    }
}
