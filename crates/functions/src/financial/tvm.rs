use super::{Arg, CellError, Ctx, R, Value, array_numbers, as_array, flatten, num, num_val, numbers, opt_num, type_arg};

// ---------------------------------------------------------------------------------------------
// Time value of money
// ---------------------------------------------------------------------------------------------

/// The payment per period. `(1 + rate)^nper` comes from `nper * ln_1p(rate)` and `p - 1` from
/// `exp_m1`: rounding `1 + rate` first throws away the low bits of a small rate, which left PMT up to
/// about 1000 ULPs from Excel (observed over 400 synthetic probes; this form is within 4). A loan
/// schedule's last `IF(p - SUM(principal) > 0, ...)` turns on those bits. Excel gives #NUM! for a
/// rate of -1 or less, and -pv*rate (no divide) once `(1 + rate)^nper` overflows.
fn pmt_calc(rate: f64, nper: f64, pv: f64, fv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        return -(pv + fv) / nper;
    }
    if rate <= -1.0 {
        return f64::NAN;
    }
    let l = nper * rate.ln_1p();
    let m = l.exp_m1();
    let k = 1.0 + rate * typ;
    if m > 0.0 {
        // (fv + pv*p) / (p - 1) = pv + (pv + fv) / (p - 1), without the cancellation in p - 1.
        // rate / m (about 1 / nper for a tiny rate) is taken first so (pv + fv) / m can't overflow.
        -(pv * rate + (pv + fv) * (rate / m)) / k
    } else if m > -0.5 {
        // p near 1 from below: fv + pv*p = (pv + fv) + pv*m keeps the small terms that rounding p would drop.
        -(rate * (pv + fv + pv * m)) / (k * m)
    } else {
        // p well below 1: m nears -1 and pv*m would swallow pv + fv, so use p itself.
        -(rate * (fv + pv * l.exp())) / (k * m)
    }
}

fn fv_calc(rate: f64, nper: f64, pmt: f64, pv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        return -(pv + pmt * nper);
    }
    let p = (1.0 + rate).powf(nper);
    -(pv * p + pmt * (1.0 + rate * typ) * (p - 1.0) / rate)
}

fn pv_calc(rate: f64, nper: f64, pmt: f64, fv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        return -(fv + pmt * nper);
    }
    let p = (1.0 + rate).powf(nper);
    -(fv + pmt * (1.0 + rate * typ) * (p - 1.0) / rate) / p
}

pub(super) fn pmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let (fv, typ) = (opt_num(a, 3, 0.0)?, type_arg(a, 4)?);
    if nper == 0.0 {
        return Err(CellError::Num);
    }
    num_val(pmt_calc(rate, nper, pv, fv, typ))
}

pub(super) fn fv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pmt) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    num_val(fv_calc(rate, nper, pmt, opt_num(a, 3, 0.0)?, type_arg(a, 4)?))
}

pub(super) fn pv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pmt) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    num_val(pv_calc(rate, nper, pmt, opt_num(a, 3, 0.0)?, type_arg(a, 4)?))
}

pub(super) fn nper(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, pmt, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let (fv, typ) = (opt_num(a, 3, 0.0)?, type_arg(a, 4)?);
    if rate == 0.0 {
        if pmt == 0.0 {
            return Err(CellError::Num);
        }
        return num_val(-(pv + fv) / pmt);
    }
    let numer = pmt * (1.0 + rate * typ) - fv * rate;
    let denom = pv * rate + pmt * (1.0 + rate * typ);
    if denom == 0.0 {
        return Err(CellError::Num);
    }
    let q = numer / denom;
    if q <= 0.0 || rate <= -1.0 {
        return Err(CellError::Num);
    }
    num_val(q.ln() / (1.0 + rate).ln())
}

fn ipmt_calc(rate: f64, per: f64, nper: f64, pv: f64, fv: f64, typ: f64) -> f64 {
    let p = pmt_calc(rate, nper, pv, fv, typ);
    if typ == 1.0 && per == 1.0 {
        return 0.0;
    }
    let bal = fv_calc(rate, per - 1.0, p, pv, typ);
    let i = bal * rate;
    if typ == 1.0 { i / (1.0 + rate) } else { i }
}

pub(super) fn ipmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, per, np, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?);
    let (fv, typ) = (opt_num(a, 4, 0.0)?, type_arg(a, 5)?);
    if per < 1.0 || per > np || np == 0.0 {
        return Err(CellError::Num);
    }
    num_val(ipmt_calc(rate, per, np, pv, fv, typ))
}

pub(super) fn ppmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, per, np, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?);
    let (fv, typ) = (opt_num(a, 4, 0.0)?, type_arg(a, 5)?);
    if per < 1.0 || per > np || np == 0.0 {
        return Err(CellError::Num);
    }
    num_val(pmt_calc(rate, np, pv, fv, typ) - ipmt_calc(rate, per, np, pv, fv, typ))
}

fn cumulative(a: &[Arg], principal: bool) -> R<Value> {
    let (rate, np, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let (start, end, typ) = (num(a, 3)?.trunc(), num(a, 4)?.trunc(), num(a, 5)?);
    let np_i = np.trunc();
    if rate <= 0.0 || np_i <= 0.0 || pv <= 0.0 || start < 1.0 || end < start || end > np_i || (typ != 0.0 && typ != 1.0) {
        return Err(CellError::Num);
    }
    if end - start > 1e7 || np_i > 1e15 {
        return Err(CellError::Num);
    }
    let p = pmt_calc(rate, np_i, pv, 0.0, typ);
    let mut sum = 0.0;
    let mut per = start;
    while per <= end {
        let i = ipmt_calc(rate, per, np_i, pv, 0.0, typ);
        sum += if principal { p - i } else { i };
        per += 1.0;
    }
    num_val(sum)
}

pub(super) fn cumipmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    cumulative(a, false)
}

pub(super) fn cumprinc(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    cumulative(a, true)
}

/// Newton iteration with a numeric derivative, keeping the rate above -1.
fn newton(f: impl Fn(f64) -> f64, guess: f64) -> Option<f64> {
    let mut x = guess;
    for _ in 0..200 {
        let y = f(x);
        if !y.is_finite() {
            return None;
        }
        let h = 1e-6 * x.abs().max(1e-3);
        let d = (f(x + h) - f(x - h)) / (2.0 * h);
        if d == 0.0 || !d.is_finite() {
            return None;
        }
        let mut nx = x - y / d;
        if nx <= -1.0 {
            nx = (x - 1.0) / 2.0;
            if nx <= -1.0 {
                nx = -0.999_999_999;
            }
        }
        if (nx - x).abs() < 1e-12 * nx.abs().max(1.0) {
            return Some(nx);
        }
        x = nx;
    }
    None
}

/// Tries Newton from the guess and a few fallback starting points; then bisection over a
/// bracket if one can be found.
fn solve_rate(f: impl Fn(f64) -> f64, guess: f64) -> R<f64> {
    let check = |r: f64| f(r).is_finite() && f(r).abs() < 1e-7 * (1.0 + f(guess).abs().min(1e12));
    for g in [guess, 0.1, 0.01, 0.5, -0.5, 1.0, 0.001, 5.0, -0.9, 10.0] {
        if let Some(r) = newton(&f, g)
            && r > -1.0
            && check(r)
        {
            return Ok(r);
        }
    }
    // Scan for a sign change and bisect.
    let grid: Vec<f64> = [-0.999_999, -0.99, -0.9, -0.5, -0.2, 0.0, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 100.0, 1000.0].to_vec();
    for w in grid.windows(2) {
        let (mut lo, mut hi) = match w {
            [a, b] => (*a, *b),
            _ => continue,
        };
        let (flo, fhi) = (f(lo), f(hi));
        if !(flo.is_finite() && fhi.is_finite()) || flo * fhi > 0.0 {
            continue;
        }
        let lo_neg = flo < 0.0;
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            let fm = f(mid);
            if (fm < 0.0) == lo_neg {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        return Ok(0.5 * (lo + hi));
    }
    Err(CellError::Num)
}

pub(super) fn rate(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (np, pmt, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let (fv, typ, guess) = (opt_num(a, 3, 0.0)?, type_arg(a, 4)?, opt_num(a, 5, 0.1)?);
    if np <= 0.0 {
        return Err(CellError::Num);
    }
    let f = |r: f64| {
        if r.abs() < 1e-12 {
            pv + pmt * np + fv
        } else {
            let p = (1.0 + r).powf(np);
            pv * p + pmt * (1.0 + r * typ) * (p - 1.0) / r + fv
        }
    };
    num_val(solve_rate(f, guess)?)
}

pub(super) fn npv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let rate = num(a, 0)?;
    if rate == -1.0 {
        return Err(CellError::Div0);
    }
    let vals = numbers(a.get(1..).unwrap_or(&[]))?;
    let mut sum = 0.0;
    let mut d = 1.0;
    for v in vals {
        d *= 1.0 + rate;
        sum += v / d;
    }
    num_val(sum)
}

/// Values and dates for XNPV/XIRR: both must be numeric and of equal length. XNPV rejects a
/// date before the first; XIRR (`lenient`) takes it, as Excel does.
///
/// Blank cells follow what Excel returns (observed on synthetic workbooks). With `lenient` (XIRR)
/// a blank value counts as 0 and a blank date is ignored when its value is blank or 0; a nonzero
/// value without a date is `#NUM!`, except in the first row, where the date is read as day 0.
/// XNPV is strict: a blank (or text) value or a blank date is `#NUM!`, except in the first row,
/// where it is `#VALUE!`.
fn dated_flows(values: &Value, dates: &Value, lenient: bool) -> R<(Vec<f64>, Vec<f64>)> {
    let va = as_array(values);
    let da = as_array(dates);
    if va.data.len() != da.data.len() {
        return Err(CellError::Num);
    }
    let mut v = Vec::with_capacity(va.data.len());
    let mut d = Vec::with_capacity(da.data.len());
    for (i, (x, y)) in va.data.iter().zip(da.data.iter()).enumerate() {
        // Excel (XNPV): a blank or text value or a blank date is #NUM!, except in the first row,
        // where it is #VALUE!.
        let strict_err = if i == 0 { CellError::Value } else { CellError::Num };
        let value = match x {
            Value::Number(n) => *n,
            Value::Empty if lenient => 0.0,
            Value::Empty => return Err(strict_err),
            Value::Error(e) => return Err(*e),
            _ if lenient => return Err(CellError::Value),
            _ => return Err(strict_err),
        };
        match y {
            Value::Number(n) => d.push(n.trunc()),
            Value::Error(e) => return Err(*e),
            Value::Text(_) => d.push(y.to_number()?.trunc()),
            Value::Empty if !lenient => return Err(strict_err),
            // XIRR skips a blank date next to a 0 flow and reads one in the first row as day 0.
            Value::Empty if value == 0.0 => continue,
            Value::Empty if i == 0 => d.push(0.0),
            Value::Empty => return Err(CellError::Num),
            _ => return Err(CellError::Value),
        }
        v.push(value);
    }
    let d0 = *d.first().ok_or(CellError::Num)?;
    if d.iter().any(|&x| (x < d0 && !lenient) || x < 0.0) {
        return Err(CellError::Num);
    }
    Ok((v, d))
}

fn xnpv_calc(rate: f64, v: &[f64], d: &[f64]) -> f64 {
    let d0 = d.first().copied().unwrap_or(0.0);
    v.iter().zip(d).map(|(x, t)| x / (1.0 + rate).powf((t - d0) / 365.0)).sum()
}

pub(super) fn xnpv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let rate = num(a, 0)?;
    if rate <= -1.0 {
        return Err(CellError::Num);
    }
    let (v, d) = dated_flows(&a.get(1).ok_or(CellError::Value)?.value, &a.get(2).ok_or(CellError::Value)?.value, false)?;
    num_val(xnpv_calc(rate, &v, &d))
}

/// Excel's XIRR solver, reconstructed by observing Excel with synthetic inputs (not from its
/// code; its documentation says only "iteration", 100 tries and 0.000001 percent). It looks
/// only at the sign of XNPV until the end, so its answer is the root to about 1e-8, not to the
/// last digit:
///
/// - Bracket the root from the guess (0 counts as 1e-5): double the rate while XNPV keeps the
///   sign it has at the guess and that sign differs from the first nonzero value (the root is higher).
///   Otherwise the root is lower: bisect towards 0 if XNPV changes sign there; halve the rate
///   towards 0 if XNPV is larger in size at 0 than at the guess; otherwise try minus the guess
///   and then step halfway to -100% each time. A negative guess only moves towards 0 (root
///   higher) or towards -100% (root lower).
/// - Bisect the bracket, taking the midpoint from the upper end, until half the bracket is
///   under 1e-8 * (|rate| + 0.5) and |XNPV| is under 2e-8 * |first nonzero value|; return that
///   midpoint. A zero XNPV counts as the upper side.
/// - Halving towards 0 that gets within that tolerance of 0 without a sign change goes on with
///   the negative side; at most 100 XNPV evaluations in all; a rate of -100% or an XNPV that
///   can't be computed (a negative base for fractional years) is #NUM!.
struct XirrSolver<'a> {
    values: &'a [f64],
    /// First nonzero value: sets the search direction and the residual scale.
    first: f64,
    years: Vec<f64>,
    evals: usize,
    /// XNPV must be within this of 0 at the answer.
    residual: f64,
}

impl XirrSolver<'_> {
    const MAX_EVALS: usize = 100;

    fn tol(rate: f64) -> f64 {
        1e-8 * (rate.abs() + 0.5)
    }

    fn same_sign(a: f64, b: f64) -> bool {
        a * b > 0.0
    }

    fn f(&mut self, rate: f64) -> R<f64> {
        self.evals += 1;
        if self.evals > Self::MAX_EVALS {
            return Err(CellError::Num);
        }
        let base = 1.0 + rate;
        if base == 0.0 {
            return Err(CellError::Num);
        }
        let mut sum = 0.0;
        for (x, t) in self.values.iter().zip(&self.years) {
            sum += x / base.powf(*t);
        }
        if sum.is_finite() { Ok(sum) } else { Err(CellError::Num) }
    }

    fn solve(&mut self, guess: f64) -> R<f64> {
        let g = if guess == 0.0 { 1e-5 } else { guess };
        let fg = self.f(g)?;
        let higher = fg * self.first < 0.0;
        if g > 0.0 {
            if higher {
                return self.double(g, fg);
            }
            let f0 = self.f(0.0)?;
            if !Self::same_sign(f0, fg) {
                return self.bisect(g, fg, 0.0, f0);
            }
            if f0.abs() > fg.abs() {
                return self.halve(g, fg, g, fg);
            }
            return self.negative_side(g, fg);
        }
        if higher { self.halve(g, fg, g, fg) } else { self.towards_minus_one(g, fg) }
    }

    fn double(&mut self, mut x: f64, mut fx: f64) -> R<f64> {
        loop {
            let next = 2.0 * x;
            let fnext = self.f(next)?;
            if !Self::same_sign(fnext, fx) {
                return self.bisect(x, fx, next, fnext);
            }
            (x, fx) = (next, fnext);
        }
    }

    fn halve(&mut self, mut x: f64, mut fx: f64, g: f64, fg: f64) -> R<f64> {
        loop {
            let step = -x / 2.0;
            let next = x + step;
            let fnext = self.f(next)?;
            if !Self::same_sign(fnext, fx) {
                return self.bisect(x, fx, next, fnext);
            }
            if step.abs() < Self::tol(next) {
                if next < 0.0 {
                    return Err(CellError::Num);
                }
                return self.negative_side(g, fg);
            }
            (x, fx) = (next, fnext);
        }
    }

    fn negative_side(&mut self, g: f64, fg: f64) -> R<f64> {
        let fm = self.f(-g)?;
        if !Self::same_sign(fm, fg) {
            return self.bisect(g, fg, -g, fm);
        }
        self.towards_minus_one(-g, fm)
    }

    fn towards_minus_one(&mut self, mut x: f64, mut fx: f64) -> R<f64> {
        loop {
            let next = x + (-1.0 - x) / 2.0;
            let fnext = self.f(next)?;
            if !Self::same_sign(fnext, fx) {
                return self.bisect(x, fx, next, fnext);
            }
            (x, fx) = (next, fnext);
        }
    }

    fn bisect(&mut self, a: f64, fa: f64, b: f64, fb: f64) -> R<f64> {
        let (mut lo, mut flo, mut hi) = if a < b { (a, fa, b) } else { (b, fb, a) };
        loop {
            let mid = hi - (hi - lo) / 2.0;
            let fm = self.f(mid)?;
            if (hi - lo) / 2.0 < Self::tol(mid) && fm.abs() < self.residual {
                return Ok(mid);
            }
            if Self::same_sign(fm, flo) {
                (lo, flo) = (mid, fm);
            } else {
                hi = mid;
            }
        }
    }
}

pub(super) fn xirr(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (v, d) = dated_flows(&a.first().ok_or(CellError::Value)?.value, &a.get(1).ok_or(CellError::Value)?.value, true)?;
    let guess = opt_num(a, 2, 0.1)?;
    if guess <= -1.0 || !v.iter().any(|x| *x > 0.0) || !v.iter().any(|x| *x < 0.0) {
        return Err(CellError::Num);
    }
    if v.first() == Some(&0.0) {
        // Excel with a 0 (or blank) first flow: the residual scale is 0, so the search just
        // halves the guess towards 0 until the step is under the tolerance; a negative guess is #NUM!.
        if guess < 0.0 {
            return Err(CellError::Num);
        }
        let mut x = if guess == 0.0 { 1e-5 } else { guess };
        // Bounded: a huge guess halves to the tolerance within about 1100 steps.
        for _ in 0..1100 {
            if x / 2.0 < XirrSolver::tol(x / 2.0) {
                break;
            }
            x /= 2.0;
        }
        return num_val(x / 2.0);
    }
    let d0 = d.first().copied().unwrap_or(0.0);
    let first = v.iter().copied().find(|x| *x != 0.0).unwrap_or(0.0);
    let mut solver = XirrSolver { values: &v, first, years: d.iter().map(|t| (t - d0) / 365.0).collect(), evals: 0, residual: 2e-8 * first.abs() };
    num_val(solver.solve(guess)?)
}

/// Excel's IRR solver, approximated from observation (Excel's documentation says 20 tries and
/// 0.00001 percent): Newton's method on the discount factor 1/(1+rate), where the cash flows
/// are a polynomial, from 1/(1+guess) until a step moves less than 1e-8, for at most 100 steps.
/// Excel's answers agree with it to about 1e-12 on ordinary flows (the last digits differ) and
/// on which flows have no answer, apart from a few far from any guess.
fn irr_solve(v: &[f64], guess: f64) -> R<f64> {
    let mut x = 1.0 / (1.0 + guess);
    for _ in 0..100 {
        let (mut s, mut ds) = (0.0, 0.0);
        for c in v.iter().rev() {
            ds = ds * x + s;
            s = s * x + c;
        }
        if ds == 0.0 {
            // An exact root with a zero slope (a repeated root such as {1,-2,1} at guess 0) is
            // still the answer: Excel returns 0 there.
            return if s == 0.0 && x > 0.0 { Ok(1.0 / x - 1.0) } else { Err(CellError::Num) };
        }
        let next = x - s / ds;
        if !next.is_finite() {
            return Err(CellError::Num);
        }
        if (next - x).abs() < 1e-8 {
            return if next > 0.0 { Ok(1.0 / next - 1.0) } else { Err(CellError::Num) };
        }
        x = next;
    }
    Err(CellError::Num)
}

pub(super) fn irr(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = array_numbers(&a.first().ok_or(CellError::Value)?.value)?;
    let guess = opt_num(a, 1, 0.1)?;
    // Excel: a guess at or below -100% is #VALUE! for IRR (#NUM! for XIRR).
    if guess <= -1.0 {
        return Err(CellError::Value);
    }
    if !v.iter().any(|x| *x > 0.0) || !v.iter().any(|x| *x < 0.0) {
        return Err(CellError::Num);
    }
    num_val(irr_solve(&v, guess)?)
}

pub(super) fn mirr(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = array_numbers(&a.first().ok_or(CellError::Value)?.value)?;
    let (fr, rr) = (num(a, 1)?, num(a, 2)?);
    let n = v.len() as f64;
    if v.len() < 2 {
        return Err(CellError::Div0);
    }
    let (mut pos, mut neg) = (0.0, 0.0);
    for (i, x) in v.iter().enumerate() {
        if *x > 0.0 {
            pos += x / (1.0 + rr).powi(i as i32);
        } else if *x < 0.0 {
            neg += x / (1.0 + fr).powi(i as i32);
        }
    }
    // No positive flow is not an error in Excel: the formula below gives -1.
    if neg == 0.0 {
        return Err(CellError::Div0);
    }
    let r = (-pos * (1.0 + rr).powf(n - 1.0) / neg).powf(1.0 / (n - 1.0)) - 1.0;
    num_val(r)
}

pub(super) fn fvschedule(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let mut p = num(a, 0)?;
    for v in flatten(a.get(1..2).unwrap_or(&[])) {
        let r = match v {
            Value::Number(n) => n,
            Value::Empty => 0.0,
            Value::Error(e) => return Err(e),
            _ => return Err(CellError::Value),
        };
        p *= 1.0 + r;
    }
    num_val(p)
}

pub(super) fn effect(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (nom, n) = (num(a, 0)?, num(a, 1)?.trunc());
    if nom <= 0.0 || n < 1.0 {
        return Err(CellError::Num);
    }
    num_val((1.0 + nom / n).powf(n) - 1.0)
}

pub(super) fn nominal(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (eff, n) = (num(a, 0)?, num(a, 1)?.trunc());
    if eff <= 0.0 || n < 1.0 {
        return Err(CellError::Num);
    }
    num_val(n * ((1.0 + eff).powf(1.0 / n) - 1.0))
}

pub(super) fn ispmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, per, np, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?);
    if np == 0.0 {
        return Err(CellError::Div0);
    }
    num_val(pv * rate * (per / np - 1.0))
}

pub(super) fn pduration(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, pv, fv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    if rate <= 0.0 || pv <= 0.0 || fv <= 0.0 {
        return Err(CellError::Num);
    }
    num_val((fv.ln() - pv.ln()) / (1.0 + rate).ln())
}

pub(super) fn rri(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (np, pv, fv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    if np <= 0.0 || pv == 0.0 {
        return Err(CellError::Num);
    }
    num_val((fv / pv).powf(1.0 / np) - 1.0)
}

fn dollar_digits(fraction: f64) -> R<f64> {
    if fraction < 0.0 {
        return Err(CellError::Num);
    }
    if fraction < 1.0 {
        return Err(CellError::Div0);
    }
    Ok(10f64.powf(fraction.log10().ceil()))
}

pub(super) fn dollarde(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, f) = (num(a, 0)?, num(a, 1)?.trunc());
    let p = dollar_digits(f)?;
    let int = x.trunc();
    num_val(int + (x - int) * p / f)
}

pub(super) fn dollarfr(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, f) = (num(a, 0)?, num(a, 1)?.trunc());
    let p = dollar_digits(f)?;
    let int = x.trunc();
    num_val(int + (x - int) * f / p)
}
