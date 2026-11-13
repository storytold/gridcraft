//! Ordered occurrence excess-of-loss claims with finite aggregate reinstatement capacity.
use super::optional_num as opt_num;

use crate::util::{R, S, arg, array_val, as_array, has, num, num_val};
use crate::{Arg, Ctx, FnSpec};
use gridcraft_core::{CellError, Value};

const MAX_CLAIMS: usize = 100_000;
/// Require a finite nonnegative contractual amount or exposure.
fn nonnegative(x: f64) -> R<f64> {
    if x.is_finite() && x >= 0.0 { Ok(x) } else { Err(CellError::Num) }
}
/// Parse an ordered numeric claim/time vector without silently discarding invalid observations.
fn vector(a: &[Arg], i: usize) -> R<Vec<f64>> {
    let data = as_array(&arg(a, i)?.value);
    if (data.rows > 1 && data.cols > 1) || data.data.is_empty() || data.data.len() > MAX_CLAIMS {
        return Err(CellError::Value);
    }
    data.iter()
        .map(|v| match v {
            Value::Number(x) => nonnegative(*x),
            Value::Error(e) => Err(*e),
            _ => Err(CellError::Value),
        })
        .collect()
}
/// Apply occurrence XOL cover in claim order with finite reinstatement capacity and optional time-prorata premiums.
///
/// Each claim row reports gross, ceded, retained, reinstated, extra premium and available limit;
/// the last row totals claims and includes the base premium in its premium column.
fn reinstate(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let losses = vector(a, 0)?;
    let deductible = nonnegative(num(a, 1)?)?;
    let limit = nonnegative(num(a, 2)?)?;
    if limit == 0.0 {
        return Err(CellError::Num);
    }
    let number = nonnegative(num(a, 3)?)?;
    if number.fract() != 0.0 || number > 100.0 {
        return Err(CellError::Num);
    }
    let premium = nonnegative(num(a, 4)?)?;
    let loading = nonnegative(opt_num(a, 5, 1.0)?)?;
    let prorata = match opt_num(a, 7, 0.0)? {
        0.0 => false,
        1.0 => true,
        _ => return Err(CellError::Num),
    };
    let times = if has(a, 6) {
        vector(a, 6)?
    } else {
        if prorata {
            return Err(CellError::Value);
        }
        vec![0.0; losses.len()]
    };
    if times.len() != losses.len() || times.iter().any(|t| *t > 1.0) || times.windows(2).any(|w| w[0] > w[1]) {
        return Err(CellError::Num);
    }
    let mut capacity = nonnegative(limit * number)?;
    let mut available = limit;
    let mut data = Vec::with_capacity((losses.len() + 1) * 6);
    let (mut gross, mut ceded, mut reinstated, mut additional) = (0.0, 0.0, 0.0, 0.0);
    for (loss, time) in losses.iter().zip(times) {
        let recovery = (loss - deductible).max(0.0).min(limit).min(available);
        available -= recovery;
        let restored = recovery.min(capacity);
        capacity -= restored;
        available += restored;
        let extra = nonnegative(premium * loading * (restored / limit) * if prorata { 1.0 - time } else { 1.0 })?;
        data.extend([*loss, recovery, *loss - recovery, restored, extra, available].into_iter().map(Value::Number));
        gross = nonnegative(gross + loss)?;
        ceded = nonnegative(ceded + recovery)?;
        reinstated = nonnegative(reinstated + restored)?;
        additional = nonnegative(additional + extra)?;
    }
    let total_premium = nonnegative(premium + additional)?;
    data.extend([gross, ceded, gross - ceded, reinstated, total_premium, available].into_iter().map(Value::Number));
    array_val(losses.len() + 1, 6, data)
}
/// Calculate the ceded loss under a sum-insured surplus-share retention and treaty line capacity.
fn surplus(a: &[Arg], _: &mut dyn Ctx) -> R<Value> {
    let insured = nonnegative(num(a, 0)?)?;
    let retention = nonnegative(num(a, 1)?)?;
    let lines = nonnegative(num(a, 2)?)?;
    let loss = nonnegative(num(a, 3)?)?;
    if insured == 0.0 {
        return Err(CellError::Num);
    }
    let ceded = (insured - retention).max(0.0).min(nonnegative(retention * lines)?);
    num_val((ceded / insured) * loss)
}
/// Register occurrence reinstatement and surplus-share reinsurance worksheet contracts.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![
        act!(
            "ACT.REINSURANCE.REINSTATEMENTS",
            5,
            8,
            Financial,
            &[false, true, true, true, true, true, false, true],
            "ACT.REINSURANCE.REINSTATEMENTS(losses,deductible,limit,reinstatements,base_premium,[reinstatement_rate=1],[times],[time_prorata=0])",
            "Ordered occurrence XOL recoveries and paid reinstatements with finite aggregate capacity.",
            reinstate
        ),
        act!(
            "ACT.REINSURANCE.SURPLUS",
            4,
            4,
            Financial,
            S,
            "ACT.REINSURANCE.SURPLUS(sum_insured,retention,lines,loss)",
            "Ceded surplus-share loss using sum-insured retention and treaty line capacity.",
            surplus
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;
    fn v(xs: &[f64]) -> Arg {
        av(col(xs))
    }
    fn data(value: Value) -> Vec<f64> {
        match value {
            Value::Array(a) => a
                .data
                .iter()
                .map(|v| match v {
                    Value::Number(x) => *x,
                    _ => panic!(),
                })
                .collect(),
            _ => panic!(),
        }
    }
    #[test]
    fn capacity_reinstatement_and_total_conservation() {
        let values = data(ev("ACT.REINSURANCE.REINSTATEMENTS", vec![v(&[200.0, 200.0, 200.0]), n(50.0), n(100.0), n(1.0), n(10.0)]));
        assert_eq!(&values[0..6], &[200.0, 100.0, 100.0, 100.0, 10.0, 100.0]);
        assert_eq!(&values[6..12], &[200.0, 100.0, 100.0, 0.0, 0.0, 0.0]);
        assert_eq!(&values[12..18], &[200.0, 0.0, 200.0, 0.0, 0.0, 0.0]);
        assert_eq!(&values[18..24], &[600.0, 200.0, 400.0, 100.0, 20.0, 0.0]);
    }
    #[test]
    fn partial_amount_time_prorata_and_validation() {
        let values = data(ev("ACT.REINSURANCE.REINSTATEMENTS", vec![v(&[50.0]), n(0.0), n(100.0), n(1.0), n(20.0), n(1.0), v(&[0.25]), n(1.0)]));
        assert_eq!(values[4], 7.5);
        assert_eq!(values[10], 27.5);
        assert_eq!(ev("ACT.REINSURANCE.REINSTATEMENTS", vec![v(&[50.0]), n(0.0), n(100.0), n(0.5), n(20.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.REINSURANCE.SURPLUS", vec![n(1000.0), n(200.0), n(3.0), n(100.0)]), nv(60.0));
    }

    #[test]
    fn skipped_reinstatement_rate_defaults_to_one_with_later_time_options() {
        let args = vec![v(&[50.0]), n(0.0), n(100.0), n(1.0), n(20.0), empty(), v(&[0.25]), n(1.0)];
        let values = data(ev("ACT.REINSURANCE.REINSTATEMENTS", args.clone()));
        assert_eq!(values[4], 7.5);
        assert_eq!(values[10], 27.5);
        let mut referenced = args;
        referenced[5] = Arg::reference(Value::Empty);
        let values = data(ev("ACT.REINSURANCE.REINSTATEMENTS", referenced));
        assert_eq!(values[4], 0.0);
        assert_eq!(values[10], 20.0);
    }
}
