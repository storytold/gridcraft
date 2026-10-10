//! Automatic array lifting: a function given an array where it expects a scalar is applied to
//! each element, broadcasting single rows/columns like Excel's dynamic arrays.

use gridcraft_core::{Array, CellError, Value};

use crate::util::MAX_CELLS;
use crate::{Arg, Ctx, FnSpec};

pub(crate) fn call_lifted(spec: &FnSpec, args: &[Arg], ctx: &mut dyn Ctx) -> Value {
    let mut rows = 0usize;
    let mut cols = 0usize;
    let mut any = false;
    for (i, a) in args.iter().enumerate() {
        if spec.is_scalar_arg(i)
            && let Value::Array(arr) = &a.value
        {
            any = true;
            rows = rows.max(arr.rows);
            cols = cols.max(arr.cols);
        }
    }
    if !any {
        return (spec.imp)(args, ctx);
    }
    if rows.saturating_mul(cols) > MAX_CELLS {
        return Value::Error(CellError::Num);
    }
    let mut cell_args: Vec<Arg> = args.to_vec();
    let mut data = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        for c in 0..cols {
            for (i, a) in args.iter().enumerate() {
                if spec.is_scalar_arg(i)
                    && let Value::Array(arr) = &a.value
                    && let Some(slot) = cell_args.get_mut(i)
                {
                    let v = arr.get_broadcast(r, c);
                    slot.value = if let Value::Array(inner) = &v { inner.data.first().cloned().unwrap_or(Value::Empty) } else { v };
                }
            }
            let out = (spec.imp)(&cell_args, ctx);
            data.push(match out {
                Value::Array(a) => a.data.first().cloned().unwrap_or(Value::Empty),
                v => v,
            });
        }
    }
    if rows == 1 && cols == 1 {
        return data.pop().unwrap_or(Value::Empty);
    }
    Array::new(rows, cols, data).map(Value::from).unwrap_or(Value::Error(CellError::Calc))
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellError, Value};

    use crate::util::testutil::*;
    use crate::util::{R, num};
    use crate::{Arg, Ctx, FnSpec};

    fn add(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
        Ok(Value::Number(num(c, a, 0)? + num(c, a, 1)?))
    }

    fn spec() -> FnSpec {
        f!("ADD2", 2, 2, MathTrig, &[true, true], "ADD2(a, b)", "Adds two numbers.", add)
    }

    #[test]
    fn broadcasts_row_and_column() {
        let s = spec();
        let out = crate::call(&s, &[av(row(&[1.0, 2.0, 3.0])), av(col(&[10.0, 20.0]))], &mut TestCtx::default());
        assert_eq!(rows_of(&out), vec![vec![nv(11.0), nv(12.0), nv(13.0)], vec![nv(21.0), nv(22.0), nv(23.0)]]);
    }

    #[test]
    fn mismatched_sizes_pad_with_na() {
        let s = spec();
        let out = crate::call(&s, &[av(row(&[1.0, 2.0, 3.0])), av(row(&[1.0, 1.0]))], &mut TestCtx::default());
        assert_eq!(rows_of(&out), vec![vec![nv(2.0), nv(3.0), Value::Error(CellError::NA)]]);
    }

    #[test]
    fn single_cell_unwraps_and_arity() {
        let s = spec();
        let out = crate::call(&s, &[rf(col(&[4.0])), n(1.0)], &mut TestCtx::default());
        assert_eq!(out, nv(5.0));
        is_err(crate::call(&s, &[n(1.0)], &mut TestCtx::default()), CellError::Value);
        let huge = crate::call(
            &s,
            &[av(gridcraft_core::Array::filled(1, 100_000, nv(1.0)).into()), av(gridcraft_core::Array::filled(100_000, 1, nv(1.0)).into())],
            &mut TestCtx::default(),
        );
        is_err(huge, CellError::Num);
    }
}
