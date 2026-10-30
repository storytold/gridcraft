//! Life contingencies and term-structure valuation. Tables contain annual qx values starting
//! at the life currently being valued; no age, mortality table or terminal death is invented.

use gridcraft_core::{CellError, Value};

use crate::util::{R, S, arg, array_val, has, num, num_val};
use crate::{Arg, Ctx, FnSpec};

const MAX_POINTS: usize = 10_000;

/// Neumaier sum: preserve residual cash flows when large opposite signs cancel.
#[derive(Default)]
struct Sum {
    total: f64,
    correction: f64,
}
impl Sum {
    /// Add a finite term with Neumaier compensation; reject a nonfinite term or intermediate sum.
    fn add(&mut self, x: f64) -> R<()> {
        if !x.is_finite() {
            return Err(CellError::Num);
        }
        let t = self.total + x;
        if !t.is_finite() {
            return Err(CellError::Num);
        }
        self.correction += if self.total.abs() >= x.abs() { (self.total - t) + x } else { (x - t) + self.total };
        self.total = t;
        Ok(())
    }
    /// Return the accumulated value including the compensation for cancellation.
    fn finish(self) -> f64 {
        self.total + self.correction
    }
}

/// Require a finite scalar, returning `#NUM!` for NaN or infinity.
fn finite(x: f64) -> R<f64> {
    if x.is_finite() { Ok(x) } else { Err(CellError::Num) }
}
/// Read a worksheet scalar using standard coercion and reject nonfinite values.
fn scalar(a: &[Arg], i: usize) -> R<f64> {
    finite(num(a, i)?)
}
/// Use the documented default for absent slots and empty literals, preserving blank-reference coercion.
fn optional(a: &[Arg], i: usize, default: f64) -> R<f64> {
    finite(super::optional_num(a, i, default)?)
}
/// Validate an exact nonnegative horizon bounded by the mortality-table resource limit.
fn integer(x: f64) -> R<usize> {
    if x.is_finite() && x >= 0.0 && x <= MAX_POINTS as f64 && x.fract() == 0.0 { Ok(x as usize) } else { Err(CellError::Num) }
}
/// Read an optional convention flag, accepting only the numerical values zero and one.
fn flag(a: &[Arg], i: usize, default: bool) -> R<bool> {
    match optional(a, i, if default { 1.0 } else { 0.0 })? {
        0.0 => Ok(false),
        1.0 => Ok(true),
        _ => Err(CellError::Num),
    }
}
/// Convert a finite effective rate greater than minus one to its stable log accumulation factor.
fn rate(x: f64) -> R<f64> {
    if x.is_finite() && x > -1.0 { Ok(x.ln_1p()) } else { Err(CellError::Num) }
}

/// Strict vectors: blanks, text and booleans must not shift mortality ages or cash-flow dates.
fn vector(a: &[Arg], i: usize) -> R<Vec<f64>> {
    let v = &arg(a, i)?.value;
    let vals = match v {
        Value::Array(arr) => {
            if arr.rows > 1 && arr.cols > 1 {
                return Err(CellError::Value);
            }
            if arr.data.is_empty() || arr.data.len() > MAX_POINTS {
                return Err(CellError::Num);
            }
            arr.data.as_slice()
        }
        other => std::slice::from_ref(other),
    };
    vals.iter()
        .map(|v| match v {
            Value::Number(x) => finite(*x),
            Value::Error(e) => Err(*e),
            _ => Err(CellError::Value),
        })
        .collect()
}
/// Read an aligned annual mortality vector and require every death probability to lie in `[0,1]`.
fn table(a: &[Arg], i: usize) -> R<Vec<f64>> {
    let q = vector(a, i)?;
    if q.iter().any(|x| !(0.0..=1.0).contains(x)) { Err(CellError::Num) } else { Ok(q) }
}
/// Select a mortality interval with checked bounds; never extrapolate missing ages.
fn window(q: &[f64], start: usize, term: usize) -> R<&[f64]> {
    let end = start.checked_add(term).ok_or(CellError::Num)?;
    q.get(start..end).ok_or(CellError::Num)
}
/// Resolve an explicit term or a supplied whole-life horizon ending in terminal mortality one.
fn whole_term(q: &[f64], a: &[Arg], i: usize, defer: usize) -> R<usize> {
    if has(a, i) {
        integer(scalar(a, i)?)
    } else if q.last() == Some(&1.0) {
        q.len().checked_sub(defer).ok_or(CellError::Num)
    } else {
        // Omitting term means whole life, which requires a supplied terminal qx = 1.
        Err(CellError::Num)
    }
}
/// Sum log survival probabilities, preserving terminal death as negative infinity.
fn log_survival(q: &[f64]) -> f64 {
    q.iter().map(|x| (-x).ln_1p()).sum()
}
/// Combine log survival and discount/growth exponents, returning zero for impossible survival.
fn weighted(log_probability: f64, exponent: f64) -> R<f64> {
    if log_probability == f64::NEG_INFINITY { Ok(0.0) } else { finite((log_probability + exponent).exp()) }
}
/// Value one unit at a nonnegative time using an effective per-period rate greater than minus one.
fn discount(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let lr = rate(scalar(a, 0)?)?;
    let t = scalar(a, 1)?;
    if t < 0.0 {
        return Err(CellError::Num);
    }
    num_val((-t * lr).exp())
}

/// Compute curve-discounted PV or its normalized time/convexity numerator.
/// Orders one and two require nonzero PV; cash flows, times and spot rates must remain aligned.
fn curve(a: &[Arg], order: usize) -> R<Value> {
    let cash = vector(a, 0)?;
    let times = vector(a, 1)?;
    let rates = vector(a, 2)?;
    if cash.len() != times.len() || !(rates.len() == 1 || rates.len() == cash.len()) {
        return Err(CellError::Value);
    }
    let mut pv = Sum::default();
    let mut numerator = Sum::default();
    for (j, (&c, &t)) in cash.iter().zip(&times).enumerate() {
        if t < 0.0 {
            return Err(CellError::Num);
        }
        let r = *rates.get(if rates.len() == 1 { 0 } else { j }).ok_or(CellError::Value)?;
        let lr = rate(r)?;
        let p = if c == 0.0 { 0.0 } else { c * (-t * lr).exp() };
        pv.add(p)?;
        if order == 1 {
            numerator.add(t * p)?;
        } else if order == 2 {
            // Parallel additive shift of annual effective spot rates.
            let inv = (-lr).exp();
            numerator.add(t * (t + 1.0) * inv * inv * p)?;
        }
    }
    let pv = pv.finish();
    if order == 0 {
        num_val(pv)
    } else if pv == 0.0 {
        Err(CellError::Div0)
    } else {
        num_val(numerator.finish() / pv)
    }
}
/// Return the compensated present value of signed cash flows discounted by supplied spot rates.
fn curve_pv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    curve(a, 0)
}
/// Return PV-weighted payment time; a zero net present value makes duration undefined.
fn curve_duration(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    curve(a, 1)
}
/// Return normalized convexity under parallel additive shifts of annual effective spot rates.
fn curve_convexity(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    curve(a, 2)
}

/// Compute conditional survival or its death complement over a checked mortality window.
fn probability(a: &[Arg], death: bool) -> R<Value> {
    let q = table(a, 0)?;
    let term = integer(scalar(a, 1)?)?;
    let start = integer(optional(a, 2, 0.0)?)?;
    let lp = log_survival(window(&q, start, term)?);
    num_val(if death { -lp.exp_m1() } else { lp.exp() })
}
/// Return conditional survival across the requested annual mortality window.
fn survival(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    probability(a, false)
}
/// Return death probability across the requested window, using `expm1` to retain small complements.
fn death(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    probability(a, true)
}
/// Compute restricted curtate life expectancy or its piecewise-linear UDD survival integral.
/// The supplied horizon is honored without inventing a terminal mortality observation.
fn expectancy(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let q = table(a, 0)?;
    let udd = flag(a, 1, false)?;
    let term = integer(optional(a, 2, q.len() as f64)?)?;
    let mut lp = 0.0_f64;
    let mut out = Sum::default();
    for &qx in window(&q, 0, term)? {
        let p0 = lp.exp();
        lp += (-qx).ln_1p();
        let p1 = lp.exp();
        out.add(if udd { (p0 + p1) * 0.5 } else { p1 })?;
    }
    num_val(out.finish())
}

/// EPV of term payments, relative to the beginning of the supplied table. Growth starts with
/// exponent zero at the first scheduled payment, regardless of due/immediate convention.
fn annuity_value(q: &[f64], lr: f64, term: usize, due: bool, defer: usize, lg: f64) -> R<f64> {
    window(q, defer, term)?;
    let mut lp = log_survival(window(q, 0, defer)?);
    let mut out = Sum::default();
    for (j, &qx) in window(q, defer, term)?.iter().enumerate() {
        let t = defer + j + usize::from(!due);
        if !due {
            lp += (-qx).ln_1p();
        }
        out.add(weighted(lp, -(t as f64) * lr + j as f64 * lg)?)?;
        if due {
            lp += (-qx).ln_1p();
        }
    }
    finite(out.finish())
}
/// Value temporary/deferred life payments with explicit advance/arrears timing and annual growth.
fn annuity(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let q = table(a, 0)?;
    let lr = rate(scalar(a, 1)?)?;
    let due = flag(a, 3, true)?;
    let defer = integer(optional(a, 4, 0.0)?)?;
    let term = whole_term(&q, a, 2, defer)?;
    let lg = rate(optional(a, 5, 0.0)?)?;
    num_val(annuity_value(&q, lr, term, due, defer, lg)?)
}

/// Value unit end-of-death-year benefits and an optional survival endowment over a supplied term.
fn insurance_value(q: &[f64], lr: f64, term: usize, endowment: bool, defer: usize) -> R<f64> {
    window(q, defer, term)?;
    let mut lp = log_survival(window(q, 0, defer)?);
    let mut out = Sum::default();
    for (j, &qx) in window(q, defer, term)?.iter().enumerate() {
        if qx > 0.0 {
            out.add(weighted(lp, qx.ln() - (defer + j + 1) as f64 * lr)?)?;
        }
        lp += (-qx).ln_1p();
    }
    if endowment {
        out.add(weighted(lp, -((defer + term) as f64) * lr)?)?;
    }
    finite(out.finish())
}
/// Return unit insurance EPV, including survival and discounting through any deferment period.
fn insurance(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let q = table(a, 0)?;
    let lr = rate(scalar(a, 1)?)?;
    let endowment = flag(a, 3, false)?;
    let defer = integer(optional(a, 4, 0.0)?)?;
    let term = whole_term(&q, a, 2, defer)?;
    num_val(insurance_value(&q, lr, term, endowment, defer)?)
}
/// Compute an equivalence net premium from insurance EPV and advance premium-annuity EPV.
/// A zero premium-annuity denominator returns `#DIV/0!` rather than a fabricated premium.
fn premium(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let q = table(a, 0)?;
    let lr = rate(scalar(a, 1)?)?;
    let term = integer(scalar(a, 2)?)?;
    let premium_term = integer(optional(a, 3, term as f64)?)?;
    let endowment = flag(a, 4, false)?;
    if premium_term == 0 || premium_term > term {
        return Err(CellError::Num);
    }
    let denominator = annuity_value(&q, lr, premium_term, true, 0, 0.0)?;
    if denominator == 0.0 {
        return Err(CellError::Div0);
    }
    num_val(insurance_value(&q, lr, term, endowment, 0)? / denominator)
}
/// Compute a prospective net reserve conditional on survival at elapsed duration.
/// Valuation occurs before the current premium or maturity payment; the supplied premium stays fixed.
fn reserve(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let q = table(a, 0)?;
    let lr = rate(scalar(a, 1)?)?;
    let term = integer(scalar(a, 2)?)?;
    let elapsed = integer(scalar(a, 3)?)?;
    let premium = scalar(a, 4)?;
    let endowment = flag(a, 5, false)?;
    let premium_term = integer(optional(a, 6, term as f64)?)?;
    if elapsed > term || premium_term > term || premium < 0.0 {
        return Err(CellError::Num);
    }
    window(&q, 0, term)?;
    let remaining = window(&q, elapsed, term - elapsed)?;
    let b = insurance_value(remaining, lr, remaining.len(), endowment, 0)?;
    let p = annuity_value(remaining, lr, premium_term.saturating_sub(elapsed), true, 0, 0.0)?;
    num_val(b - premium * p)
}
/// Value independent joint-life or last-survivor annuities with explicit payment timing.
fn joint_annuity(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let q1 = table(a, 0)?;
    let q2 = table(a, 1)?;
    let lr = rate(scalar(a, 2)?)?;
    let term = integer(scalar(a, 3)?)?;
    let last = flag(a, 4, false)?;
    let due = flag(a, 5, true)?;
    let defer = integer(optional(a, 6, 0.0)?)?;
    let w1 = window(&q1, defer, term)?;
    let w2 = window(&q2, defer, term)?;
    let mut lp1 = log_survival(window(&q1, 0, defer)?);
    let mut lp2 = log_survival(window(&q2, 0, defer)?);
    let mut out = Sum::default();
    for (j, (&x, &y)) in w1.iter().zip(w2).enumerate() {
        if !due {
            lp1 += (-x).ln_1p();
            lp2 += (-y).ln_1p();
        }
        let lp = if last {
            // p1 + (1-p1)*p2 avoids cancellation near 1 and preserves small tails.
            let p1 = lp1.exp();
            let p2 = lp2.exp();
            (p1 + (-lp1.exp_m1()) * p2).ln()
        } else {
            lp1 + lp2
        };
        out.add(weighted(lp, -((defer + j + usize::from(!due)) as f64) * lr)?)?;
        if due {
            lp1 += (-x).ln_1p();
            lp2 += (-y).ln_1p();
        }
    }
    num_val(out.finish())
}
/// Spill mortality probabilities after multiplicative improvement, without resetting terminal mortality.
fn improve(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let q = table(a, 0)?;
    let improvement = scalar(a, 1)?;
    let years = integer(scalar(a, 2)?)?;
    if !(0.0..=1.0).contains(&improvement) {
        return Err(CellError::Num);
    }
    let factor = if years == 0 { 1.0 } else { ((-improvement).ln_1p() * years as f64).exp() };
    array_val(q.len(), 1, q.into_iter().map(|x| Value::Number(x * factor)).collect())
}
/// Evaluate analytic Gompertz–Makeham survival, including the constant-force limiting case.
fn gompertz(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let aa = scalar(a, 0)?;
    let b = scalar(a, 1)?;
    let c = scalar(a, 2)?;
    let age = scalar(a, 3)?;
    let term = scalar(a, 4)?;
    if aa < 0.0 || b < 0.0 || c < 1.0 || age < 0.0 || term < 0.0 {
        return Err(CellError::Num);
    }
    if term == 0.0 {
        return num_val(1.0);
    }
    let h = if b == 0.0 {
        aa * term
    } else if c == 1.0 {
        (aa + b) * term
    } else {
        let lc = c.ln();
        let z = lc * term;
        // Evaluate the integrated Gompertz term on a log scale; overflow of a positive
        // integrated hazard has the well-defined survival limit zero.
        let log_ratio = if z > 50.0 { z + (-(-z).exp()).ln_1p() - lc.ln() } else { z.exp_m1().ln() - lc.ln() };
        aa * term + (b.ln() + age * lc + log_ratio).exp()
    };
    num_val((-h).exp())
}
/// Value an accrued-service pension using projected salary and a deferred life annuity.
/// No future-service accrual, disability model or accounting-compliance assumption is introduced.
fn pension(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let q = table(a, 0)?;
    let lr = rate(scalar(a, 1)?)?;
    let defer = integer(scalar(a, 2)?)?;
    let salary = scalar(a, 3)?;
    let accrual = scalar(a, 4)?;
    let service = scalar(a, 5)?;
    let salary_growth = rate(optional(a, 6, 0.0)?)?;
    let pension_growth = rate(optional(a, 7, 0.0)?)?;
    let due = flag(a, 8, true)?;
    if q.last() != Some(&1.0) || salary < 0.0 || accrual < 0.0 || service < 0.0 || defer > q.len() {
        return Err(CellError::Num);
    }
    // Accrued service only; no benefit is accrued for future service. No withdrawals,
    // disability or salary/mortality dependence is assumed.
    if salary == 0.0 || accrual == 0.0 || service == 0.0 {
        return num_val(0.0);
    }
    let log_benefit = salary.ln() + accrual.ln() + service.ln() + defer as f64 * salary_growth;
    let mut lp = log_survival(window(&q, 0, defer)?);
    let mut out = Sum::default();
    for (j, &qx) in window(&q, defer, q.len() - defer)?.iter().enumerate() {
        if !due {
            lp += (-qx).ln_1p();
        }
        let t = defer + j + usize::from(!due);
        out.add(weighted(lp, log_benefit - t as f64 * lr + j as f64 * pension_growth)?)?;
        if due {
            lp += (-qx).ln_1p();
        }
    }
    num_val(out.finish())
}

/// Register deterministic life, pension and term-structure worksheet functions with scalar/range masks.
pub(super) fn specs() -> Vec<FnSpec> {
    vec![
        act!(
            "ACT.DISCOUNT",
            2,
            2,
            Financial,
            S,
            "ACT.DISCOUNT(rate, time)",
            "Discount factor using an annual effective rate and nonnegative time.",
            discount
        ),
        act!(
            "ACT.CURVEPV",
            3,
            3,
            Financial,
            &[false, false, false],
            "ACT.CURVEPV(cashflows, times, spot_rates)",
            "Present value on annual effective spot rates; one rate or a matching vector.",
            curve_pv
        ),
        act!(
            "ACT.CURVEDURATION",
            3,
            3,
            Financial,
            &[false, false, false],
            "ACT.CURVEDURATION(cashflows, times, spot_rates)",
            "Present-value-weighted cash-flow time (Macaulay duration), including signed flows.",
            curve_duration
        ),
        act!(
            "ACT.CURVECONVEXITY",
            3,
            3,
            Financial,
            &[false, false, false],
            "ACT.CURVECONVEXITY(cashflows, times, spot_rates)",
            "PV-normalized second derivative for a parallel additive effective spot-rate shift.",
            curve_convexity
        ),
        act!(
            "ACT.SURVIVAL",
            2,
            3,
            Financial,
            &[false, true, true],
            "ACT.SURVIVAL(qx, term, [start])",
            "Conditional term survival from supplied annual mortality probabilities at start.",
            survival
        ),
        act!(
            "ACT.DEATH",
            2,
            3,
            Financial,
            &[false, true, true],
            "ACT.DEATH(qx, term, [start])",
            "Conditional probability of death during term years from start.",
            death
        ),
        act!(
            "ACT.LIFEEXPECTANCY",
            1,
            3,
            Financial,
            &[false, true, true],
            "ACT.LIFEEXPECTANCY(qx, [udd], [term])",
            "Restricted curtate expectancy, or complete expectancy under UDD; truncated at term.",
            expectancy
        ),
        act!(
            "ACT.ANNUITY",
            2,
            6,
            Financial,
            &[false, true, true, true, true, true],
            "ACT.ANNUITY(qx, rate, [term], [due], [defer], [growth])",
            "Life annuity EPV; due defaults true; omitted term requires terminal qx=1.",
            annuity
        ),
        act!(
            "ACT.INSURANCE",
            2,
            5,
            Financial,
            &[false, true, true, true, true],
            "ACT.INSURANCE(qx, rate, [term], [endowment], [defer])",
            "Unit end-of-death-year insurance EPV, with optional survival endowment.",
            insurance
        ),
        act!(
            "ACT.PREMIUM",
            3,
            5,
            Financial,
            &[false, true, true, true, true],
            "ACT.PREMIUM(qx, rate, term, [premium_term], [endowment])",
            "Level annual net premium by equivalence; premiums payable in advance while alive.",
            premium
        ),
        act!(
            "ACT.RESERVE",
            5,
            7,
            Financial,
            &[false, true, true, true, true, true, true],
            "ACT.RESERVE(qx, rate, term, elapsed, premium, [endowment], [premium_term])",
            "Prospective unit net reserve just before the premium or maturity payment at elapsed.",
            reserve
        ),
        act!(
            "ACT.JOINTANNUITY",
            4,
            7,
            Financial,
            &[false, false, true, true, true, true, true],
            "ACT.JOINTANNUITY(qx1, qx2, rate, term, [last_survivor], [due], [defer])",
            "Independent joint-life or last-survivor unit annuity EPV.",
            joint_annuity
        ),
        act!(
            "ACT.MORTALITYIMPROVE",
            3,
            3,
            Financial,
            &[false, true, true],
            "ACT.MORTALITYIMPROVE(qx, improvement, years)",
            "Project every qx by a proportional annual improvement, including the terminal entry.",
            improve
        ),
        act!(
            "ACT.GOMPERTZSURVIVAL",
            5,
            5,
            Financial,
            S,
            "ACT.GOMPERTZSURVIVAL(makeham, gompertz, base, age, term)",
            "Survival under nonnegative Makeham plus Gompertz force of mortality; base >= 1.",
            gompertz
        ),
        act!(
            "ACT.PENSION",
            6,
            9,
            Financial,
            &[false, true, true, true, true, true, true, true, true],
            "ACT.PENSION(qx, rate, defer, salary, accrual, service, [salary_growth], [pension_growth], [due])",
            "Accrued-service projected pension EPV including survival to retirement.",
            pension
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::*;

    #[test]
    fn discount_and_curves_have_analytic_values() {
        close(ev("ACT.DISCOUNT", vec![n(0.1), n(2.0)]), 1.0 / 1.21);
        close(ev("ACT.DISCOUNT", vec![n(-0.5), n(2.0)]), 4.0);
        close(ev("ACT.CURVEPV", vec![av(col(&[100.0, 121.0])), av(col(&[0.0, 2.0])), n(0.1)]), 200.0);
        close(ev("ACT.CURVEDURATION", vec![n(121.0), n(2.0), n(0.1)]), 2.0);
        close(ev("ACT.CURVECONVEXITY", vec![n(121.0), n(2.0), n(0.1)]), 6.0 / 1.21);
        close(ev("ACT.CURVEPV", vec![av(col(&[1e16, 1.0, -1e16])), av(col(&[0.0, 0.0, 0.0])), n(0.0)]), 1.0);
    }

    #[test]
    fn mortality_probability_and_restricted_expectancy() {
        let q = || av(col(&[0.2, 0.5, 1.0]));
        close(ev("ACT.SURVIVAL", vec![q(), n(2.0)]), 0.4);
        close(ev("ACT.SURVIVAL", vec![q(), n(1.0), n(1.0)]), 0.5);
        close(ev("ACT.DEATH", vec![q(), n(2.0)]), 0.6);
        close(ev("ACT.SURVIVAL", vec![q(), n(0.0)]), 1.0);
        close(ev("ACT.LIFEEXPECTANCY", vec![q()]), 1.2);
        close(ev("ACT.LIFEEXPECTANCY", vec![q(), n(1.0)]), 1.7);
        close(ev("ACT.LIFEEXPECTANCY", vec![av(col(&[0.0, 0.0])), n(1.0)]), 2.0);
        close(ev("ACT.DEATH", vec![n(1e-16), n(1.0)]), 1e-16);
        close(ev("ACT.LIFEEXPECTANCY", vec![n(1.0)]), 0.0);
        close(ev("ACT.LIFEEXPECTANCY", vec![n(1.0), n(1.0)]), 0.5);
    }

    #[test]
    fn annuity_insurance_twin_and_deferment() {
        let q = || av(col(&[0.2, 0.5, 1.0]));
        let aa = ev("ACT.ANNUITY", vec![q(), n(0.1)]).to_number().unwrap();
        let insurance = ev("ACT.INSURANCE", vec![q(), n(0.1)]).to_number().unwrap();
        assert!((insurance + (0.1 / 1.1) * aa - 1.0).abs() < 1e-12);
        close(ev("ACT.ANNUITY", vec![q(), n(0.1), n(2.0), n(0.0)]), 0.8 / 1.1 + 0.4 / 1.21);
        close(ev("ACT.ANNUITY", vec![q(), n(0.1), n(1.0), n(1.0), n(1.0)]), 0.8 / 1.1);
        close(ev("ACT.INSURANCE", vec![q(), n(0.1), n(1.0), n(0.0), n(1.0)]), 0.4 / 1.21);
        close(ev("ACT.INSURANCE", vec![av(col(&[0.0, 0.0])), n(0.0), n(2.0), n(1.0)]), 1.0);
        close(ev("ACT.ANNUITY", vec![n(1.0), n(0.0)]), 1.0);
        close(ev("ACT.ANNUITY", vec![n(1.0), n(0.0), n(1.0), n(0.0)]), 0.0);
        close(ev("ACT.ANNUITY", vec![av(col(&[0.0, 0.0])), n(0.0), n(2.0), n(1.0), n(0.0), n(0.1)]), 2.1);
    }

    #[test]
    fn skipped_optional_literals_use_contract_defaults_without_hiding_explicit_zero() {
        let q = || av(col(&[0.2, 0.5, 1.0]));
        let immediate = 0.8 / 1.1 + 0.4 / 1.21;
        close(ev("ACT.ANNUITY", vec![q(), n(0.1), empty(), n(0.0)]), immediate);
        close(ev("ACT.ANNUITY", vec![q(), n(0.1), n(2.0), empty(), n(1.0)]), immediate);
        let insurance = 0.2 / 1.1 + 0.4 / 1.21 + 0.4 / 1.331;
        close(ev("ACT.INSURANCE", vec![q(), n(0.1), empty(), n(1.0)]), insurance);
        close(ev("ACT.PREMIUM", vec![q(), n(0.1), n(3.0), empty(), n(1.0)]), insurance / (1.0 + immediate));
        close(ev("ACT.ANNUITY", vec![q(), n(0.1), n(0.0)]), 0.0);
        close(ev("ACT.ANNUITY", vec![q(), n(0.1), Arg::reference(Value::Empty), n(0.0)]), 0.0);
        close(ev("ACT.ANNUITY", vec![q(), n(0.1), n(3.0), Arg::reference(Value::Empty)]), immediate);
        assert_eq!(ev("ACT.ANNUITY", vec![av(col(&[0.1, 0.2])), n(0.1), empty(), n(0.0)]), Value::Error(CellError::Num),);
    }

    #[test]
    fn equivalence_premium_and_reserves_satisfy_recursion() {
        let q = || av(col(&[0.2, 0.5, 1.0]));
        let prem = ev("ACT.PREMIUM", vec![q(), n(0.1), n(3.0)]).to_number().unwrap();
        close(ev("ACT.RESERVE", vec![q(), n(0.1), n(3.0), n(0.0), n(prem)]), 0.0);
        let v1 = ev("ACT.RESERVE", vec![q(), n(0.1), n(3.0), n(1.0), n(prem)]).to_number().unwrap();
        assert!((prem * 1.1 - (0.2 + 0.8 * v1)).abs() < 1e-12);
        close(ev("ACT.RESERVE", vec![q(), n(0.1), n(3.0), n(3.0), n(prem)]), 0.0);
        close(ev("ACT.RESERVE", vec![q(), n(0.1), n(3.0), n(3.0), n(prem), n(1.0)]), 1.0);
    }

    #[test]
    fn independent_joint_and_last_survivor_identity() {
        let q = || av(col(&[0.2, 0.5, 1.0]));
        let aa = ev("ACT.ANNUITY", vec![q(), n(0.1)]).to_number().unwrap();
        let joint = ev("ACT.JOINTANNUITY", vec![q(), q(), n(0.1), n(3.0)]).to_number().unwrap();
        let last = ev("ACT.JOINTANNUITY", vec![q(), q(), n(0.1), n(3.0), n(1.0)]).to_number().unwrap();
        assert!((joint + last - 2.0 * aa).abs() < 1e-12);
        close(ev("ACT.JOINTANNUITY", vec![n(1.0), n(1.0), n(0.0), n(1.0), n(1.0), n(0.0)]), 0.0);
    }

    #[test]
    fn mortality_improvement_and_gompertz_limits() {
        let v = ev("ACT.MORTALITYIMPROVE", vec![av(col(&[0.2, 1.0])), n(0.1), n(2.0)]);
        let expected = col(&[0.162, 0.81]);
        if let (Value::Array(x), Value::Array(y)) = (v, expected) {
            for (a, b) in x.data.iter().zip(&y.data) {
                close(a.clone(), b.to_number().unwrap());
            }
        } else {
            panic!("expected mortality column");
        }
        close(ev("ACT.GOMPERTZSURVIVAL", vec![n(0.02), n(0.03), n(1.0), n(50.0), n(10.0)]), (-0.5_f64).exp());
        close(ev("ACT.GOMPERTZSURVIVAL", vec![n(0.0), n(1.0), n(2.0), n(10000.0), n(1.0)]), 0.0);
        close(ev("ACT.GOMPERTZSURVIVAL", vec![n(0.0), n(0.0), n(2.0), n(10000.0), n(1.0)]), 1.0);
    }

    #[test]
    fn accrued_service_pension_has_no_future_service() {
        // Certain survival through retirement, then one due payment before certain death.
        close(ev("ACT.PENSION", vec![av(col(&[0.0, 1.0])), n(0.1), n(1.0), n(100.0), n(0.02), n(10.0), n(0.1)]), 20.0);
        // Death before retirement prevents every pension payment, even at negative rates.
        close(ev("ACT.PENSION", vec![av(col(&[1.0, 1.0])), n(-0.5), n(1.0), n(100.0), n(0.02), n(10.0)]), 0.0);
    }

    #[test]
    fn hostile_inputs_fail_without_coercing_table_holes() {
        for q in [av(col(&[-0.1])), av(col(&[1.1])), n(f64::NAN), n(f64::INFINITY)] {
            assert_eq!(ev("ACT.SURVIVAL", vec![q, n(1.0)]), Value::Error(CellError::Num));
        }
        for q in [av(arr(vec![vec![Value::Empty]])), av(arr(vec![vec![Value::Bool(false)]])), av(arr(vec![vec![tv("0.1")]]))] {
            assert_eq!(ev("ACT.SURVIVAL", vec![q, n(1.0)]), Value::Error(CellError::Value));
        }
        assert_eq!(ev("ACT.SURVIVAL", vec![n(0.1), n(1.5)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.SURVIVAL", vec![n(0.1), n(2.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.SURVIVAL", vec![av(col(&vec![0.1; MAX_POINTS + 1])), n(1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.ANNUITY", vec![n(0.1), n(0.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.ANNUITY", vec![n(1.0), n(-1.0)]), Value::Error(CellError::Num));
        assert_eq!(ev("ACT.CURVEPV", vec![av(col(&[1.0, 2.0])), n(1.0), n(0.0)]), Value::Error(CellError::Value));
        assert_eq!(ev("ACT.CURVEDURATION", vec![n(0.0), n(1.0), n(0.0)]), Value::Error(CellError::Div0));
        assert_eq!(ev("ACT.ANNUITY", vec![n(1.0), n(0.0), n(1.0), n(2.0)]), Value::Error(CellError::Num));
    }
}
