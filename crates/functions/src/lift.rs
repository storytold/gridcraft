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

/// Estimates immediate argument storage without traversing or allocating nested input data.
/// Units are operational estimates, not CPU-time or hard real-time guarantees.
pub(crate) fn value_work(value: &Value) -> u64 {
    match value {
        Value::Array(array) => u64::try_from(array.data.len()).unwrap_or(u64::MAX).max(1),
        Value::Text(text) => u64::try_from(text.len()).unwrap_or(u64::MAX).max(1),
        _ => 1,
    }
}

/// Charges before each kernel invocation, including every broadcast output position.
fn native_call(spec: &FnSpec, args: &[Arg], ctx: &mut dyn Ctx) -> Value {
    let units = args.iter().fold(1u64, |units, arg| units.saturating_add(value_work(&arg.value)));
    if !ctx.charge_work(units) {
        return Value::Error(CellError::Num);
    }
    let value = (spec.imp)(args, ctx);
    if ctx.charge_work(0) { value } else { Value::Error(CellError::Num) }
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
        return native_call(spec, args, ctx);
    }
    if rows.saturating_mul(cols) > MAX_CELLS {
        return Value::Error(CellError::Num);
    }
    // Reserve output and argument-copy work before either allocation. Refuse the whole result
    // on exhaustion rather than allocating a partially calculated error-filled array.
    if !ctx.charge_work((rows as u64).saturating_mul(cols as u64).saturating_add(args.len() as u64)) {
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
            let units = cell_args.iter().fold(1u64, |units, arg| units.saturating_add(value_work(&arg.value)));
            if !ctx.charge_work(units) {
                return Value::Error(CellError::Num);
            }
            let out = (spec.imp)(&cell_args, ctx);
            // Observe a host-latched refusal inside the kernel, including the final position.
            if !ctx.charge_work(0) {
                return Value::Error(CellError::Num);
            }
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

    /// A host-owned bank records real invocations without importing any engine types.
    struct BudgetCtx {
        remaining: u64,
        kernels: usize,
        rejected: bool,
    }
    impl Ctx for BudgetCtx {
        fn charge_work(&mut self, units: u64) -> bool {
            if self.rejected || units > self.remaining {
                self.rejected = true;
                return false;
            }
            self.remaining -= units;
            true
        }
        fn date_system(&self) -> gridcraft_core::DateSystem {
            gridcraft_core::DateSystem::default()
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            self.kernels += 1;
            0.0
        }
    }
    fn counted_add(a: &[Arg], ctx: &mut dyn Ctx) -> R<Value> {
        ctx.random();
        add(a, ctx)
    }

    #[test]
    fn budgets_reject_before_kernels_and_stop_partway_through_broadcasts() {
        let spec = FnSpec { imp: |a, c| crate::util::finish(counted_add(a, c)), ..spec() };
        let mut denied = BudgetCtx { remaining: 0, kernels: 0, rejected: false };
        assert_eq!(crate::call(&spec, &[n(1.0), n(2.0)], &mut denied), Value::Error(CellError::Num));
        assert_eq!(denied.kernels, 0);
        // Input work=5, output reservation=5, first kernel=3. The second is refused.
        let mut partial = BudgetCtx { remaining: 13, kernels: 0, rejected: false };
        assert_eq!(crate::call(&spec, &[av(row(&[1.0, 2.0, 3.0])), n(1.0)], &mut partial), Value::Error(CellError::Num));
        assert_eq!(partial.kernels, 1);
        assert!(partial.rejected);
        assert_eq!(crate::call(&spec, &[n(1.0), n(2.0)], &mut partial), Value::Error(CellError::Num));
        assert_eq!(partial.kernels, 1);
    }

    #[test]
    fn output_reservation_and_shared_calls_consume_the_same_host_bank() {
        let spec = FnSpec { imp: |a, c| crate::util::finish(counted_add(a, c)), ..spec() };
        let mut ctx = BudgetCtx { remaining: 6, kernels: 0, rejected: false };
        assert_eq!(crate::call(&spec, &[n(2.0), n(3.0)], &mut ctx), Value::Number(5.0));
        assert_eq!(ctx.remaining, 0);
        assert_eq!(crate::call(&spec, &[n(2.0), n(3.0)], &mut ctx), Value::Error(CellError::Num));
        assert_eq!(ctx.kernels, 1);
        let mut reserve = BudgetCtx { remaining: 5, kernels: 0, rejected: false };
        assert_eq!(crate::call(&spec, &[av(row(&[1.0, 2.0, 3.0])), n(1.0)], &mut reserve), Value::Error(CellError::Num));
        assert_eq!(reserve.kernels, 0);
    }

    #[test]
    fn refusal_inside_the_final_broadcast_kernel_discards_the_whole_result() {
        let spec = FnSpec {
            imp: |args, ctx| {
                ctx.random();
                if !ctx.charge_work(1) {
                    return Value::Error(CellError::Num);
                }
                crate::util::finish(add(args, ctx))
            },
            ..spec()
        };
        let mut ctx = BudgetCtx { remaining: 15, kernels: 0, rejected: false };
        assert_eq!(crate::call(&spec, &[av(row(&[1.0, 2.0])), n(1.0)], &mut ctx), Value::Error(CellError::Num));
        assert_eq!(ctx.kernels, 2);
        assert!(ctx.rejected);
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
