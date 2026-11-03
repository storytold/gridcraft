//! Automatic array lifting: a function given an array where it expects a scalar is applied to
//! each element, broadcasting single rows/columns like Excel's dynamic arrays.

use gridcraft_core::{Array, CellError, Value};

use crate::util::MAX_CELLS;
use crate::{Arg, Ctx, FnSpec};

/// Value-parameter masks (see [`FnSpec::value_args`]), observed in Excel 365: a reference to more
/// than one cell at these positions gives `#VALUE!` in dynamic-array and legacy formulas alike (no
/// implicit intersection), while array constants and computed arrays lift. Range parameters
/// (holidays, schedules, cash flows, coefficients) and the WORKDAY.INTL/NETWORKDAYS.INTL weekend
/// (which lifts a range) are `false`.
pub(crate) const VALUE_ARGS: &[(&str, &[bool])] = &[
    // Date and time.
    ("EOMONTH EDATE YEARFRAC WEEKNUM", &[true]),
    ("WORKDAY NETWORKDAYS", &[true, true, false]),
    ("WORKDAY.INTL NETWORKDAYS.INTL", &[true, true, false, false]),
    // Financial.
    (
        "ACCRINT ACCRINTM AMORDEGRC AMORLINC COUPDAYBS COUPDAYS COUPDAYSNC COUPNCD COUPNUM COUPPCD CUMIPMT CUMPRINC DISC \
         DOLLARDE DOLLARFR DURATION EFFECT INTRATE MDURATION NOMINAL ODDFPRICE ODDFYIELD ODDLPRICE ODDLYIELD PRICE PRICEDISC \
         PRICEMAT RECEIVED TBILLEQ TBILLPRICE TBILLYIELD YIELD YIELDDISC YIELDMAT",
        &[true],
    ),
    ("FVSCHEDULE XNPV", &[true, false]),
    ("XIRR", &[false, false, true]),
    // Math and information.
    ("ISEVEN ISODD QUOTIENT MROUND FACTDOUBLE SQRTPI RANDBETWEEN", &[true]),
    ("SERIESSUM", &[true, true, true, false]),
    // Engineering (IMSUM and IMPRODUCT aggregate, so they aren't here).
    (
        "BIN2DEC BIN2HEX BIN2OCT DEC2BIN DEC2HEX DEC2OCT HEX2BIN HEX2DEC HEX2OCT OCT2BIN OCT2DEC OCT2HEX CONVERT DELTA GESTEP \
         ERF ERF.PRECISE ERFC ERFC.PRECISE BESSELI BESSELJ BESSELK BESSELY COMPLEX IMABS IMAGINARY IMARGUMENT IMCONJUGATE \
         IMCOS IMCOSH IMCOT IMCSC IMCSCH IMDIV IMEXP IMLN IMLOG10 IMLOG2 IMPOWER IMREAL IMSEC IMSECH IMSIN IMSINH IMSQRT IMSUB \
         IMTAN",
        &[true],
    ),
];

/// The value-parameter mask of `name` (empty for functions without value parameters).
pub(crate) fn value_args(name: &str) -> &'static [bool] {
    VALUE_ARGS.iter().find(|(names, _)| names.split_ascii_whitespace().any(|n| n == name)).map_or(&[], |(_, mask)| mask)
}

/// A reference to more than one cell at a value parameter becomes a scalar `#VALUE!`, so it adds
/// no shape: `EOMONTH(A1:A3, {1;2})` is a 2x1 array of `#VALUE!`, as in Excel.
fn reject_multi_cell_refs(spec: &FnSpec, args: &[Arg]) -> Option<Vec<Arg>> {
    let multi = |i: usize, a: &Arg| a.from_ref && spec.is_value_arg(i) && matches!(&a.value, Value::Array(arr) if arr.data.len() > 1);
    if !args.iter().enumerate().any(|(i, a)| multi(i, a)) {
        return None;
    }
    Some(
        args.iter()
            .enumerate()
            .map(|(i, a)| if multi(i, a) { Arg { value: Value::Error(CellError::Value), from_ref: false } } else { a.clone() })
            .collect(),
    )
}

pub(crate) fn call_lifted(spec: &FnSpec, args: &[Arg], ctx: &mut dyn Ctx) -> Value {
    let rejected = if spec.value_args.is_empty() { None } else { reject_multi_cell_refs(spec, args) };
    let args = rejected.as_deref().unwrap_or(args);
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

    fn add(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
        Ok(Value::Number(num(a, 0)? + num(a, 1)?))
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

    #[test]
    fn value_params_reject_multi_cell_references() {
        let s = FnSpec { value_args: &[true, false], ..spec() };
        let mut ctx = TestCtx::default();
        // A multi-cell reference is #VALUE!; a one-cell reference and a computed array are fine.
        is_err(crate::call(&s, &[rf(col(&[1.0, 2.0, 3.0])), n(1.0)], &mut ctx), CellError::Value);
        assert_eq!(crate::call(&s, &[rf(col(&[4.0])), n(1.0)], &mut ctx), nv(5.0));
        assert_eq!(rows_of(&crate::call(&s, &[av(col(&[1.0, 2.0])), n(1.0)], &mut ctx)), vec![vec![nv(2.0)], vec![nv(3.0)]]);
        // The rejected reference adds no shape: the other argument still lifts.
        let v = Value::Error(CellError::Value);
        assert_eq!(rows_of(&crate::call(&s, &[rf(col(&[1.0, 2.0, 3.0])), av(col(&[1.0, 2.0]))], &mut ctx)), vec![vec![v.clone()], vec![v]]);
        // A position that isn't a value parameter still lifts the reference.
        assert_eq!(rows_of(&crate::call(&s, &[n(1.0), rf(row(&[1.0, 2.0]))], &mut ctx)), vec![vec![nv(2.0), nv(3.0)]]);
    }

    #[test]
    fn value_arg_table_names_registered_scalar_positions() {
        for (names, mask) in super::VALUE_ARGS {
            assert!(!mask.is_empty());
            for name in names.split_ascii_whitespace() {
                let Some(s) = crate::lookup(name) else { panic!("{name} isn't registered") };
                assert_eq!(s.value_args, *mask, "{name}");
                for i in 0..s.max_args.min(12) {
                    assert!(!s.is_value_arg(i) || s.is_scalar_arg(i), "{name} argument {i} is a value parameter but doesn't lift");
                }
            }
        }
        assert!(crate::lookup("WORKDAY").is_some_and(|s| s.is_value_arg(1) && !s.is_value_arg(2)));
        assert!(crate::lookup("WORKDAY.INTL").is_some_and(|s| !s.is_value_arg(2) && s.is_scalar_arg(2)));
        assert!(crate::lookup("SUM").is_some_and(|s| s.value_args.is_empty()));
        assert!(crate::lookup("ISOWEEKNUM").is_some_and(|s| s.value_args.is_empty()));
    }
}
