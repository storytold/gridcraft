//! Financial functions: time value of money, cash-flow analysis, depreciation and securities.
//!
//! Sign convention follows Excel: money paid out is negative, money received is positive.
//! Date arguments are serial numbers (truncated) in the workbook's date system.

use gridcraft_core::date::{datetime_from_serial, days_in_month, is_leap, max_serial, serial_from_ymd};
use gridcraft_core::{CellError, DateSystem, Value};

use crate::util::{R, S, array_numbers, as_array, flatten, has, num, num_val, numbers, opt_bool, opt_num, round_half_away, text, to_sig_digits};
use crate::{Arg, Ctx, FnSpec, VAR};

// ---------------------------------------------------------------------------------------------
// Time value of money
// ---------------------------------------------------------------------------------------------

fn pmt_calc(rate: f64, nper: f64, pv: f64, fv: f64, typ: f64) -> f64 {
    if rate == 0.0 {
        return -(pv + fv) / nper;
    }
    let p = (1.0 + rate).powf(nper);
    -(rate * (fv + pv * p)) / ((1.0 + rate * typ) * (p - 1.0))
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

fn type_arg(args: &[Arg], i: usize) -> R<f64> {
    Ok(if opt_num(args, i, 0.0)? != 0.0 { 1.0 } else { 0.0 })
}

fn pmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let (fv, typ) = (opt_num(a, 3, 0.0)?, type_arg(a, 4)?);
    if nper == 0.0 {
        return Err(CellError::Num);
    }
    num_val(pmt_calc(rate, nper, pv, fv, typ))
}

fn fv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pmt) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    num_val(fv_calc(rate, nper, pmt, opt_num(a, 3, 0.0)?, type_arg(a, 4)?))
}

fn pv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pmt) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    num_val(pv_calc(rate, nper, pmt, opt_num(a, 3, 0.0)?, type_arg(a, 4)?))
}

fn nper(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
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

fn ipmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, per, np, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?);
    let (fv, typ) = (opt_num(a, 4, 0.0)?, type_arg(a, 5)?);
    if per < 1.0 || per > np || np == 0.0 {
        return Err(CellError::Num);
    }
    num_val(ipmt_calc(rate, per, np, pv, fv, typ))
}

fn ppmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
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

fn cumipmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    cumulative(a, false)
}

fn cumprinc(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
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

fn rate(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
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

fn npv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
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

/// Values and dates for XNPV/XIRR: both must be numeric and of equal length.
fn dated_flows(values: &Value, dates: &Value) -> R<(Vec<f64>, Vec<f64>)> {
    let va = as_array(values);
    let da = as_array(dates);
    if va.data.len() != da.data.len() {
        return Err(CellError::Num);
    }
    let mut v = Vec::with_capacity(va.data.len());
    let mut d = Vec::with_capacity(da.data.len());
    for (x, y) in va.data.iter().zip(da.data.iter()) {
        match x {
            Value::Number(n) => v.push(*n),
            Value::Error(e) => return Err(*e),
            _ => return Err(CellError::Value),
        }
        match y {
            Value::Number(n) => d.push(n.trunc()),
            Value::Error(e) => return Err(*e),
            Value::Text(_) => d.push(y.to_number()?.trunc()),
            _ => return Err(CellError::Value),
        }
    }
    let d0 = *d.first().ok_or(CellError::Num)?;
    if d.iter().any(|&x| x < d0 || x < 0.0) {
        return Err(CellError::Num);
    }
    Ok((v, d))
}

fn xnpv_calc(rate: f64, v: &[f64], d: &[f64]) -> f64 {
    let d0 = d.first().copied().unwrap_or(0.0);
    v.iter().zip(d).map(|(x, t)| x / (1.0 + rate).powf((t - d0) / 365.0)).sum()
}

fn xnpv(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let rate = num(a, 0)?;
    if rate <= -1.0 {
        return Err(CellError::Num);
    }
    let (v, d) = dated_flows(&a.get(1).ok_or(CellError::Value)?.value, &a.get(2).ok_or(CellError::Value)?.value)?;
    num_val(xnpv_calc(rate, &v, &d))
}

fn xirr(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (v, d) = dated_flows(&a.first().ok_or(CellError::Value)?.value, &a.get(1).ok_or(CellError::Value)?.value)?;
    let guess = opt_num(a, 2, 0.1)?;
    if !v.iter().any(|x| *x > 0.0) || !v.iter().any(|x| *x < 0.0) {
        return Err(CellError::Num);
    }
    num_val(solve_rate(|r| xnpv_calc(r, &v, &d), guess)?)
}

fn irr(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let v = array_numbers(&a.first().ok_or(CellError::Value)?.value)?;
    let guess = opt_num(a, 1, 0.1)?;
    if !v.iter().any(|x| *x > 0.0) || !v.iter().any(|x| *x < 0.0) {
        return Err(CellError::Num);
    }
    let f = |r: f64| {
        let mut s = 0.0;
        let mut d = 1.0;
        for x in &v {
            s += x / d;
            d *= 1.0 + r;
        }
        s
    };
    num_val(solve_rate(f, guess)?)
}

fn mirr(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
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

fn fvschedule(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
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

fn effect(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (nom, n) = (num(a, 0)?, num(a, 1)?.trunc());
    if nom <= 0.0 || n < 1.0 {
        return Err(CellError::Num);
    }
    num_val((1.0 + nom / n).powf(n) - 1.0)
}

fn nominal(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (eff, n) = (num(a, 0)?, num(a, 1)?.trunc());
    if eff <= 0.0 || n < 1.0 {
        return Err(CellError::Num);
    }
    num_val(n * ((1.0 + eff).powf(1.0 / n) - 1.0))
}

fn ispmt(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, per, np, pv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?);
    if np == 0.0 {
        return Err(CellError::Div0);
    }
    num_val(pv * rate * (per / np - 1.0))
}

fn pduration(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (rate, pv, fv) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    if rate <= 0.0 || pv <= 0.0 || fv <= 0.0 {
        return Err(CellError::Num);
    }
    num_val((fv.ln() - pv.ln()) / (1.0 + rate).ln())
}

fn rri(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
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

fn dollarde(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, f) = (num(a, 0)?, num(a, 1)?.trunc());
    let p = dollar_digits(f)?;
    let int = x.trunc();
    num_val(int + (x - int) * p / f)
}

fn dollarfr(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (x, f) = (num(a, 0)?, num(a, 1)?.trunc());
    let p = dollar_digits(f)?;
    let int = x.trunc();
    num_val(int + (x - int) * f / p)
}

// ---------------------------------------------------------------------------------------------
// Depreciation
// ---------------------------------------------------------------------------------------------

fn sln(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    if life == 0.0 {
        return Err(CellError::Div0);
    }
    num_val((cost - salvage) / life)
}

fn syd(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life, per) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?);
    if life <= 0.0 || per <= 0.0 || per > life || salvage < 0.0 {
        return Err(CellError::Num);
    }
    num_val((cost - salvage) * (life - per + 1.0) * 2.0 / (life * (life + 1.0)))
}

fn db(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life, period) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?.trunc());
    let month = opt_num(a, 4, 12.0)?.trunc();
    if cost < 0.0 || salvage < 0.0 || life <= 0.0 || period <= 0.0 || !(1.0..=12.0).contains(&month) {
        return Err(CellError::Num);
    }
    let max_period = if month == 12.0 { life.trunc() } else { life.trunc() + 1.0 };
    if period > max_period.max(1.0) || period > 1e7 {
        return Err(CellError::Num);
    }
    if cost == 0.0 {
        return num_val(0.0);
    }
    let rate = crate::util::round_half_away(1.0 - (salvage / cost).powf(1.0 / life), 3);
    let mut total = 0.0;
    let mut dep = 0.0;
    let mut p = 1.0;
    while p <= period {
        dep = if p == 1.0 {
            cost * rate * month / 12.0
        } else if p == life.trunc() + 1.0 {
            (cost - total) * rate * (12.0 - month) / 12.0
        } else {
            (cost - total) * rate
        };
        total += dep;
        p += 1.0;
    }
    num_val(dep)
}

fn ddb(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life, period) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?);
    let factor = opt_num(a, 4, 2.0)?;
    if cost < 0.0 || salvage < 0.0 || life <= 0.0 || period <= 0.0 || factor <= 0.0 || period > life {
        return Err(CellError::Num);
    }
    let mut rate = factor / life;
    let old;
    if rate >= 1.0 {
        rate = 1.0;
        old = if period == 1.0 { cost } else { 0.0 };
    } else {
        old = cost * (1.0 - rate).powf(period - 1.0);
    }
    let new = cost * (1.0 - rate).powf(period);
    let dep = if new < salvage { old - salvage } else { old - new };
    num_val(dep.max(0.0))
}

/// Per-period declining-balance depreciation with optional switch to straight line.
fn vdb_schedule(cost: f64, salvage: f64, life: f64, factor: f64, no_switch: bool, periods: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(periods);
    let mut book = cost;
    let mut sl: Option<f64> = None;
    for i in 0..periods {
        let dep = if let Some(s) = sl {
            s.min((book - salvage).max(0.0))
        } else {
            let ddb = (book * factor / life).min((book - salvage).max(0.0));
            let remaining = life - i as f64;
            let slv = if remaining > 0.0 { (book - salvage) / remaining.max(1.0) } else { 0.0 };
            if !no_switch && slv > ddb {
                sl = Some(slv);
                slv.min((book - salvage).max(0.0))
            } else {
                ddb
            }
        };
        out.push(dep);
        book -= dep;
    }
    out
}

fn vdb(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life, start, end) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, num(a, 3)?, num(a, 4)?);
    let factor = opt_num(a, 5, 2.0)?;
    let no_switch = opt_bool(a, 6, false)?;
    if cost < 0.0 || salvage < 0.0 || life <= 0.0 || start < 0.0 || end < start || end > life || factor <= 0.0 {
        return Err(CellError::Num);
    }
    let periods = end.ceil();
    if periods > 1e7 {
        return Err(CellError::Num);
    }
    let sched = vdb_schedule(cost, salvage, life, factor, no_switch, periods as usize);
    let mut total = 0.0;
    for (i, d) in sched.iter().enumerate() {
        let (p0, p1) = (i as f64, i as f64 + 1.0);
        let overlap = (p1.min(end) - p0.max(start)).max(0.0);
        total += d * overlap;
    }
    num_val(total)
}

fn amorlinc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (cost, purchased, first, salvage) = (num(a, 0)?, date(a, 1, c.date_system())?, date(a, 2, c.date_system())?, num(a, 3)?);
    let (period, rate, basis) = (num(a, 4)?.trunc(), num(a, 5)?, basis_arg(a, 6)?);
    if cost < 0.0 || salvage < 0.0 || salvage > cost || period < 0.0 || rate <= 0.0 || first < purchased {
        return Err(CellError::Num);
    }
    let full = cost * rate;
    let limit = cost - salvage;
    let dep0 = (full * year_frac(c.date_system(), purchased, first, basis)?).min(limit);
    if period == 0.0 {
        return num_val(dep0);
    }
    let remaining = limit - dep0 - (period - 1.0) * full;
    num_val(full.min(remaining.max(0.0)))
}

fn amordegrc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (cost, purchased, first, salvage) = (num(a, 0)?, date(a, 1, c.date_system())?, date(a, 2, c.date_system())?, num(a, 3)?);
    let (period, rate, basis) = (num(a, 4)?.trunc(), num(a, 5)?, basis_arg(a, 6)?);
    if cost < 0.0 || salvage < 0.0 || salvage > cost || period < 0.0 || rate <= 0.0 || first < purchased || period > 1e6 {
        return Err(CellError::Num);
    }
    let life = 1.0 / rate;
    let coef = if life < 3.0 {
        1.0
    } else if life < 5.0 {
        1.5
    } else if life <= 6.0 {
        2.0
    } else {
        2.5
    };
    let r = rate * coef;
    let mut book = cost;
    let mut dep = (year_frac(c.date_system(), purchased, first, basis)? * r * cost).round();
    if period == 0.0 {
        return num_val(dep);
    }
    book -= dep;
    let mut rest = book - salvage;
    let mut n = 1.0;
    while n <= period {
        dep = (r * book).round();
        rest -= dep;
        if rest < 0.0 {
            // The two final periods split what remains; afterwards nothing is left.
            let left = period - n;
            return num_val(if left <= 1.0 { (book * 0.5).round() } else { 0.0 });
        }
        book -= dep;
        n += 1.0;
    }
    num_val(dep)
}

// ---------------------------------------------------------------------------------------------
// Dates and day counts
// ---------------------------------------------------------------------------------------------

fn date(a: &[Arg], i: usize, sys: DateSystem) -> R<f64> {
    let value = num(a, i)?;
    let d = value.trunc();
    if value < 0.0 || !(0.0..=max_serial(sys) as f64).contains(&d) {
        return Err(CellError::Num);
    }
    Ok(d)
}

fn ymd(sys: DateSystem, s: f64) -> R<(i32, u32, u32)> {
    // A coupon before the workbook epoch is an intermediate calendar date, not an
    // invalid input serial. Keep #1507's workbook-aware calculations while allowing
    // December 1903 coupon boundaries in a 1904 workbook.
    let calendar = match sys {
        DateSystem::D1900 => s.trunc(),
        DateSystem::D1904 => s.trunc() + 1462.0,
    };
    let dt = datetime_from_serial(DateSystem::D1900, calendar).ok_or(CellError::Num)?;
    Ok((dt.year, dt.month, dt.day.max(1)))
}

fn serial(sys: DateSystem, y: i32, m: u32, d: u32) -> R<f64> {
    serial_from_ymd(DateSystem::D1900, y as i64, m as i64, d as i64).map(|day| gridcraft_core::date::serial_in_system(sys, day)).ok_or(CellError::Num)
}

fn basis_arg(a: &[Arg], i: usize) -> R<u32> {
    let b = opt_num(a, i, 0.0)?.trunc();
    if !(0.0..=4.0).contains(&b) {
        return Err(CellError::Num);
    }
    Ok(b as u32)
}

fn freq_arg(a: &[Arg], i: usize) -> R<f64> {
    let f = num(a, i)?.trunc();
    if f != 1.0 && f != 2.0 && f != 4.0 {
        return Err(CellError::Num);
    }
    Ok(f)
}

/// 30/360 day count (US/NASD when `us`, European otherwise).
fn days360(sys: DateSystem, s1: f64, s2: f64, us: bool) -> R<f64> {
    let (y1, m1, mut d1) = ymd(sys, s1)?;
    let (y2, m2, mut d2) = ymd(sys, s2)?;
    if us {
        let feb_end1 = m1 == 2 && d1 >= days_in_month(y1, 2);
        let feb_end2 = m2 == 2 && d2 >= days_in_month(y2, 2);
        if feb_end1 && feb_end2 {
            d2 = 30;
        }
        if feb_end1 {
            d1 = 30;
        }
        if d2 == 31 && d1 >= 30 {
            d2 = 30;
        }
        if d1 == 31 {
            d1 = 30;
        }
    } else {
        d1 = d1.min(30);
        d2 = d2.min(30);
    }
    Ok((y2 - y1) as f64 * 360.0 + (m2 as f64 - m1 as f64) * 30.0 + (d2 as f64 - d1 as f64))
}

/// Days between two dates under a basis.
fn day_count(sys: DateSystem, s1: f64, s2: f64, basis: u32) -> R<f64> {
    match basis {
        0 => days360(sys, s1, s2, true),
        4 => days360(sys, s1, s2, false),
        _ => Ok(s2 - s1),
    }
}

/// Year fraction between two dates under a basis (YEARFRAC semantics).
fn year_frac(sys: DateSystem, s1: f64, s2: f64, basis: u32) -> R<f64> {
    let (s1, s2) = if s1 <= s2 { (s1, s2) } else { (s2, s1) };
    match basis {
        0 => Ok(days360(sys, s1, s2, true)? / 360.0),
        2 => Ok((s2 - s1) / 360.0),
        3 => Ok((s2 - s1) / 365.0),
        4 => Ok(days360(sys, s1, s2, false)? / 360.0),
        _ => {
            let (y1, m1, d1) = ymd(sys, s1)?;
            let (y2, m2, d2) = ymd(sys, s2)?;
            let days = s2 - s1;
            if y1 == y2 {
                return Ok(days / if is_leap(y1) { 366.0 } else { 365.0 });
            }
            let within_year = y2 == y1 + 1 && (m2 < m1 || (m2 == m1 && d2 <= d1));
            if within_year {
                let feb29_1 = is_leap(y1) && (m1 < 2 || (m1 == 2 && d1 <= 29));
                let feb29_2 = is_leap(y2) && (m2 > 2 || (m2 == 2 && d2 == 29));
                let denom = if feb29_1 || feb29_2 { 366.0 } else { 365.0 };
                return Ok(days / denom);
            }
            let start = serial(sys, y1, 1, 1)?;
            let end = serial(sys, y2 + 1, 1, 1).unwrap_or(max_serial(sys) as f64 + 1.0);
            let avg = (end - start) / (y2 - y1 + 1) as f64;
            Ok(days / avg)
        }
    }
}

/// Days in a year for discount securities.
fn year_basis(sys: DateSystem, basis: u32, s: f64) -> R<f64> {
    Ok(match basis {
        0 | 2 | 4 => 360.0,
        3 => 365.0,
        _ => {
            if is_leap(ymd(sys, s)?.0) {
                366.0
            } else {
                365.0
            }
        }
    })
}

/// Date `months` months before/after an anchor date, keeping end-of-month alignment.
fn shift_months(sys: DateSystem, anchor: (i32, u32, u32), months: i64, eom: bool) -> R<f64> {
    let (y, m, d) = anchor;
    let total = y as i64 * 12 + (m as i64 - 1) + months;
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    if !(1900..=9999).contains(&ny) {
        return Err(CellError::Num);
    }
    let dim = days_in_month(ny as i32, nm);
    let nd = if eom { dim } else { d.min(dim) };
    serial(sys, ny as i32, nm, nd)
}

struct Coupon {
    pcd: f64,
    ncd: f64,
    num: f64,
}

fn coupons(sys: DateSystem, settle: f64, mat: f64, freq: f64) -> R<Coupon> {
    if settle >= mat {
        return Err(CellError::Num);
    }
    let anchor = ymd(sys, mat)?;
    let eom = anchor.2 == days_in_month(anchor.0, anchor.1);
    let step = (12.0 / freq) as i64;
    // Estimate the count from the span, then adjust.
    let mut k = (((mat - settle) / 365.25 * freq).floor() as i64).max(1);
    while k > 1 && shift_months(sys, anchor, -(k - 1) * step, eom)? <= settle {
        k -= 1;
    }
    let mut guard = 0;
    while shift_months(sys, anchor, -k * step, eom)? > settle {
        k += 1;
        guard += 1;
        if guard > 100_000 {
            return Err(CellError::Num);
        }
    }
    Ok(Coupon { pcd: shift_months(sys, anchor, -k * step, eom)?, ncd: shift_months(sys, anchor, -(k - 1) * step, eom)?, num: k as f64 })
}

fn coup_days(c: &Coupon, freq: f64, basis: u32) -> f64 {
    match basis {
        1 => c.ncd - c.pcd,
        3 => 365.0 / freq,
        _ => 360.0 / freq,
    }
}

fn coup_daybs(sys: DateSystem, c: &Coupon, settle: f64, basis: u32) -> R<f64> {
    day_count(sys, c.pcd, settle, basis)
}

fn coup_daysnc(sys: DateSystem, c: &Coupon, settle: f64, freq: f64, basis: u32) -> R<f64> {
    match basis {
        0 | 4 => Ok(coup_days(c, freq, basis) - coup_daybs(sys, c, settle, basis)?),
        _ => Ok(c.ncd - settle),
    }
}

/// settlement, maturity, frequency, basis at positions 0..=3 for the COUP* functions.
fn coup_args(a: &[Arg], sys: DateSystem) -> R<(f64, f64, f64, u32, Coupon)> {
    let (s, m, f, b) = (date(a, 0, sys)?, date(a, 1, sys)?, freq_arg(a, 2)?, basis_arg(a, 3)?);
    let c = coupons(sys, s, m, f)?;
    Ok((s, m, f, b, c))
}

fn coupdaybs(a: &[Arg], cx: &mut dyn Ctx) -> R<Value> {
    let (s, _, _, b, c) = coup_args(a, cx.date_system())?;
    num_val(coup_daybs(cx.date_system(), &c, s, b)?)
}

fn coupdays(a: &[Arg], cx: &mut dyn Ctx) -> R<Value> {
    let (_, _, f, b, c) = coup_args(a, cx.date_system())?;
    num_val(coup_days(&c, f, b))
}

fn coupdaysnc(a: &[Arg], cx: &mut dyn Ctx) -> R<Value> {
    let (s, _, f, b, c) = coup_args(a, cx.date_system())?;
    num_val(coup_daysnc(cx.date_system(), &c, s, f, b)?)
}

fn coupncd(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(coup_args(a, c.date_system())?.4.ncd)
}

fn couppcd(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(coup_args(a, c.date_system())?.4.pcd)
}

fn coupnum(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(coup_args(a, c.date_system())?.4.num)
}

// ---------------------------------------------------------------------------------------------
// Securities
// ---------------------------------------------------------------------------------------------

fn accrint(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let sys = c.date_system();
    let (issue, first, settle, rate) = (date(a, 0, sys)?, date(a, 1, sys)?, date(a, 2, sys)?, num(a, 3)?);
    let par = if has(a, 4) { num(a, 4)? } else { 1000.0 };
    let freq = freq_arg(a, 5)?;
    let basis = basis_arg(a, 6)?;
    let calc_method = opt_bool(a, 7, true)?;
    if issue >= settle || rate <= 0.0 || par <= 0.0 {
        return Err(CellError::Num);
    }
    let anchor = ymd(sys, first)?;
    let eom = anchor.2 == days_in_month(anchor.0, anchor.1);
    let step = (12.0 / freq) as i64;
    let cd = |j: i64| shift_months(sys, anchor, j * step, eom);
    // Coupon index of the period containing a date: cd(j) <= date < cd(j+1).
    let index_of = |d: f64| -> R<i64> {
        let mut j = (((d - first) / 365.25 * freq).floor() as i64).clamp(-200_000, 200_000);
        let mut guard = 0;
        while cd(j)? > d {
            j -= 1;
            guard += 1;
            if guard > 400_000 {
                return Err(CellError::Num);
            }
        }
        while cd(j + 1)? <= d {
            j += 1;
            guard += 1;
            if guard > 400_000 {
                return Err(CellError::Num);
            }
        }
        Ok(j)
    };
    let start = if calc_method || settle <= first { issue } else { cd(index_of(settle)?)? };
    let mut j = index_of(start)?;
    let mut sum = 0.0;
    let mut guard = 0;
    loop {
        let (p0, p1) = (cd(j)?, cd(j + 1)?);
        if p0 >= settle {
            break;
        }
        let from = p0.max(start);
        let to = p1.min(settle);
        if to > from {
            let nl = match basis {
                1 => p1 - p0,
                3 => 365.0 / freq,
                _ => 360.0 / freq,
            };
            sum += day_count(sys, from, to, basis)? / nl;
        }
        j += 1;
        guard += 1;
        if guard > 100_000 {
            return Err(CellError::Num);
        }
    }
    num_val(par * rate / freq * sum)
}

fn accrintm(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (issue, settle, rate) = (date(a, 0, c.date_system())?, date(a, 1, c.date_system())?, num(a, 2)?);
    let par = if has(a, 3) { num(a, 3)? } else { 1000.0 };
    let basis = basis_arg(a, 4)?;
    if issue >= settle || rate <= 0.0 || par <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(par * rate * year_frac(c.date_system(), issue, settle, basis)?)
}

/// settlement, maturity, value, value, [basis] for discount securities.
fn disc_args(a: &[Arg], sys: DateSystem) -> R<(f64, f64, f64, f64, u32)> {
    let (s, m, x, y, b) = (date(a, 0, sys)?, date(a, 1, sys)?, num(a, 2)?, num(a, 3)?, basis_arg(a, 4)?);
    if s >= m || x <= 0.0 || y <= 0.0 {
        return Err(CellError::Num);
    }
    Ok((s, m, x, y, b))
}

fn disc_frac(sys: DateSystem, s: f64, m: f64, b: u32) -> R<f64> {
    Ok(day_count(sys, s, m, b)? / year_basis(sys, b, s)?)
}

fn disc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, pr, red, b) = disc_args(a, c.date_system())?;
    num_val((1.0 - pr / red) / disc_frac(c.date_system(), s, m, b)?)
}

fn pricedisc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, d, red, b) = disc_args(a, c.date_system())?;
    num_val(red - d * red * disc_frac(c.date_system(), s, m, b)?)
}

fn yielddisc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, pr, red, b) = disc_args(a, c.date_system())?;
    num_val((red / pr - 1.0) / disc_frac(c.date_system(), s, m, b)?)
}

fn intrate(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, inv, red, b) = disc_args(a, c.date_system())?;
    num_val((red - inv) / inv / disc_frac(c.date_system(), s, m, b)?)
}

fn received(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, inv, d, b) = disc_args(a, c.date_system())?;
    let denom = 1.0 - d * disc_frac(c.date_system(), s, m, b)?;
    if denom <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(inv / denom)
}

fn tbill_args(a: &[Arg], sys: DateSystem) -> R<(f64, f64)> {
    let (s, m, x) = (date(a, 0, sys)?, date(a, 1, sys)?, num(a, 2)?);
    if s >= m || x <= 0.0 {
        return Err(CellError::Num);
    }
    // At most one year after settlement.
    let (y, mo, d) = ymd(sys, s)?;
    let limit = shift_months(sys, (y, mo, d), 12, false)?;
    if m > limit {
        return Err(CellError::Num);
    }
    Ok((m - s, x))
}

fn tbillprice(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dsm, d) = tbill_args(a, c.date_system())?;
    let p = 100.0 * (1.0 - d * dsm / 360.0);
    if p <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(p)
}

fn tbillyield(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dsm, pr) = tbill_args(a, c.date_system())?;
    num_val((100.0 - pr) / pr * 360.0 / dsm)
}

fn tbilleq(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dsm, d) = tbill_args(a, c.date_system())?;
    if dsm <= 182.0 {
        return num_val(365.0 * d / (360.0 - d * dsm));
    }
    let price = 100.0 * (1.0 - d * dsm / 360.0);
    if price <= 0.0 {
        return Err(CellError::Num);
    }
    let t = dsm / 365.0;
    let disc = t * t - (2.0 * t - 1.0) * (1.0 - 100.0 / price);
    if disc < 0.0 {
        return Err(CellError::Num);
    }
    num_val((-t + disc.sqrt()) / (t - 0.5))
}

/// settlement, maturity, issue, rate, value, [basis].
fn mat_terms(a: &[Arg], sys: DateSystem) -> R<(f64, f64, f64, f64, f64)> {
    let (s, m, issue, rate, x, b) = (date(a, 0, sys)?, date(a, 1, sys)?, date(a, 2, sys)?, num(a, 3)?, num(a, 4)?, basis_arg(a, 5)?);
    if s >= m || issue >= s || rate < 0.0 || x < 0.0 {
        return Err(CellError::Num);
    }
    let dim = year_frac(sys, issue, m, b)?;
    let dsm = year_frac(sys, s, m, b)?;
    let ai = year_frac(sys, issue, s, b)?;
    Ok((dim, dsm, ai, rate, x))
}

fn pricemat(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dim, dsm, ai, rate, yld) = mat_terms(a, c.date_system())?;
    num_val((100.0 + dim * rate * 100.0) / (1.0 + dsm * yld) - ai * rate * 100.0)
}

fn yieldmat(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dim, dsm, ai, rate, pr) = mat_terms(a, c.date_system())?;
    if pr <= 0.0 || dsm == 0.0 {
        return Err(CellError::Num);
    }
    let base = pr / 100.0 + ai * rate;
    num_val(((1.0 + dim * rate) - base) / base / dsm)
}

struct Bond {
    n: f64,
    e: f64,
    a: f64,
    dsc: f64,
    freq: f64,
}

fn bond(sys: DateSystem, settle: f64, mat: f64, freq: f64, basis: u32) -> R<Bond> {
    let c = coupons(sys, settle, mat, freq)?;
    let e = coup_days(&c, freq, basis);
    let a = coup_daybs(sys, &c, settle, basis)?;
    let dsc = coup_daysnc(sys, &c, settle, freq, basis)?;
    if e <= 0.0 {
        return Err(CellError::Num);
    }
    Ok(Bond { n: c.num, e, a, dsc, freq })
}

fn bond_price(b: &Bond, rate: f64, yld: f64, red: f64) -> f64 {
    let f = b.freq;
    let coupon = 100.0 * rate / f;
    let accrued = coupon * b.a / b.e;
    if b.n == 1.0 {
        return (red + coupon) / (1.0 + b.dsc / b.e * yld / f) - accrued;
    }
    let base = 1.0 + yld / f;
    let frac = b.dsc / b.e;
    let mut sum = red / base.powf(b.n - 1.0 + frac);
    let mut k = 1.0;
    while k <= b.n {
        sum += coupon / base.powf(k - 1.0 + frac);
        k += 1.0;
    }
    sum - accrued
}

fn price(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, rate, yld, red) = (date(a, 0, c.date_system())?, date(a, 1, c.date_system())?, num(a, 2)?, num(a, 3)?, num(a, 4)?);
    let (f, basis) = (freq_arg(a, 5)?, basis_arg(a, 6)?);
    if rate < 0.0 || yld < 0.0 || red <= 0.0 {
        return Err(CellError::Num);
    }
    let b = bond(c.date_system(), s, m, f, basis)?;
    num_val(bond_price(&b, rate, yld, red))
}

fn yield_(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, rate, pr, red) = (date(a, 0, c.date_system())?, date(a, 1, c.date_system())?, num(a, 2)?, num(a, 3)?, num(a, 4)?);
    let (f, basis) = (freq_arg(a, 5)?, basis_arg(a, 6)?);
    if rate < 0.0 || pr <= 0.0 || red <= 0.0 {
        return Err(CellError::Num);
    }
    let b = bond(c.date_system(), s, m, f, basis)?;
    if b.n == 1.0 {
        let base = pr / 100.0 + b.a / b.e * rate / f;
        return num_val(((red / 100.0 + rate / f) - base) / base * f * b.e / b.dsc);
    }
    // Price is decreasing in yield: bisect on [0, hi].
    let g = |y: f64| bond_price(&b, rate, y, red) - pr;
    let (mut lo, mut hi) = (-0.999_999 * f, 1.0);
    let mut guard = 0;
    while g(hi) > 0.0 && guard < 60 {
        hi *= 2.0;
        guard += 1;
    }
    if !(g(lo) >= 0.0 && g(hi) <= 0.0) {
        return Err(CellError::Num);
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if g(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    num_val(0.5 * (lo + hi))
}

fn duration_calc(a: &[Arg], sys: DateSystem) -> R<(f64, f64, f64)> {
    let (s, m, coupon, yld) = (date(a, 0, sys)?, date(a, 1, sys)?, num(a, 2)?, num(a, 3)?);
    let (f, basis) = (freq_arg(a, 4)?, basis_arg(a, 5)?);
    if coupon < 0.0 || yld < 0.0 {
        return Err(CellError::Num);
    }
    let b = bond(sys, s, m, f, basis)?;
    let c = coupon * 100.0 / f;
    let y = 1.0 + yld / f;
    let frac = b.dsc / b.e;
    let (mut wsum, mut psum) = (0.0, 0.0);
    let mut k = 1.0;
    while k <= b.n {
        let t = k - 1.0 + frac;
        let cf = if k == b.n { c + 100.0 } else { c };
        let pv = cf / y.powf(t);
        wsum += t * pv;
        psum += pv;
        k += 1.0;
    }
    if psum == 0.0 {
        return Err(CellError::Num);
    }
    Ok((wsum / psum / f, yld, f))
}

fn duration(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(duration_calc(a, c.date_system())?.0)
}

fn mduration(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (d, y, f) = duration_calc(a, c.date_system())?;
    num_val(d / (1.0 + y / f))
}

// ---------------------------------------------------------------------------------------------
// Odd first and last coupon periods
// ---------------------------------------------------------------------------------------------
//
// An odd period is split into quasi-coupon periods: the regular coupon periods the security
// would have had, counted back from the first coupon (ODDF*) or forward from the last interest
// date (ODDL*). For quasi-period i, NL is its normal length, DC the days of it inside the odd
// period, A the accrued days of it before settlement and DSC the days of it from settlement on.

/// Most quasi-coupon periods an odd period may span (the date range allows ~32,400 quarters).
const MAX_QUASI: i64 = 40_000;

/// Normal length of the coupon period `[start, end]` under a basis (COUPDAYS semantics).
fn normal_length(start: f64, end: f64, freq: f64, basis: u32) -> f64 {
    match basis {
        1 => end - start,
        3 => 365.0 / freq,
        _ => 360.0 / freq,
    }
}

/// Days of `[from, to]` under a basis, or 0 when the span is empty.
fn span(sys: DateSystem, from: f64, to: f64, basis: u32) -> R<f64> {
    if to > from { day_count(sys, from, to, basis) } else { Ok(0.0) }
}

/// The odd-first-period terms that ODDFPRICE and ODDFYIELD share.
struct OddFirst {
    /// Coupons after the first coupon up to maturity.
    n: f64,
    /// Whole quasi-coupon periods between settlement and the first coupon.
    nq: f64,
    /// DSC / NL of the quasi-period holding settlement.
    frac: f64,
    /// Σ DCi / NLi over the odd period.
    dc: f64,
    /// Σ Ai / NLi over the odd period.
    acc: f64,
    freq: f64,
}

fn odd_first(sys: DateSystem, settle: f64, mat: f64, issue: f64, first: f64, freq: f64, basis: u32) -> R<OddFirst> {
    if !(mat > first && first > settle && settle > issue) {
        return Err(CellError::Num);
    }
    let anchor = ymd(sys, first)?;
    let eom = anchor.2 == days_in_month(anchor.0, anchor.1);
    let step = (12.0 / freq) as i64;
    let quasi = |k: i64| shift_months(sys, anchor, -k * step, eom);
    let (mut dc, mut acc) = (0.0, 0.0);
    let (mut nq, mut frac) = (None, 0.0);
    // Walk back from the first coupon: period k runs from quasi(k + 1) to quasi(k).
    let mut k = 0;
    loop {
        if k >= MAX_QUASI {
            return Err(CellError::Num);
        }
        let (end, start) = (quasi(k)?, quasi(k + 1)?);
        let nl = normal_length(start, end, freq, basis);
        if nl <= 0.0 {
            return Err(CellError::Num);
        }
        let from = start.max(issue);
        dc += span(sys, from, end, basis)? / nl;
        acc += span(sys, from, settle.min(end), basis)? / nl;
        if start <= settle && settle < end {
            nq = Some(k as f64);
            frac = span(sys, settle, end, basis)? / nl;
        }
        if start <= issue {
            break;
        }
        k += 1;
    }
    let nq = nq.ok_or(CellError::Num)?;
    let n = coupons(sys, first, mat, freq)?.num;
    Ok(OddFirst { n, nq, frac, dc, acc, freq })
}

fn odd_first_price(t: &OddFirst, rate: f64, yld: f64, red: f64) -> f64 {
    let coupon = 100.0 * rate / t.freq;
    let base = 1.0 + yld / t.freq;
    let lead = t.nq + t.frac;
    let mut p = red / base.powf(t.n + lead) + coupon * t.dc / base.powf(lead);
    let mut k = 1.0;
    while k <= t.n {
        p += coupon / base.powf(k + lead);
        k += 1.0;
    }
    p - coupon * t.acc
}

/// settlement, maturity, issue, first_coupon, rate, yld/pr, redemption, frequency, [basis].
fn odd_first_args(a: &[Arg], sys: DateSystem) -> R<(OddFirst, f64, f64, f64)> {
    let (s, m, issue, first) = (date(a, 0, sys)?, date(a, 1, sys)?, date(a, 2, sys)?, date(a, 3, sys)?);
    let (rate, x, red) = (num(a, 4)?, num(a, 5)?, num(a, 6)?);
    let (f, basis) = (freq_arg(a, 7)?, basis_arg(a, 8)?);
    if rate < 0.0 || red <= 0.0 {
        return Err(CellError::Num);
    }
    Ok((odd_first(sys, s, m, issue, first, f, basis)?, rate, x, red))
}

fn oddfprice(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (t, rate, yld, red) = odd_first_args(a, c.date_system())?;
    if yld < 0.0 {
        return Err(CellError::Num);
    }
    num_val(odd_first_price(&t, rate, yld, red))
}

fn oddfyield(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (t, rate, pr, red) = odd_first_args(a, c.date_system())?;
    if pr <= 0.0 {
        return Err(CellError::Num);
    }
    // The price falls as the yield rises: bracket the root, then bisect.
    let g = |y: f64| odd_first_price(&t, rate, y, red) - pr;
    let (mut lo, mut hi) = (-0.999_999 * t.freq, 1.0);
    let mut guard = 0;
    while g(hi) > 0.0 && guard < 60 {
        hi *= 2.0;
        guard += 1;
    }
    // NaN fails both comparisons.
    if !(g(lo) >= 0.0 && g(hi) <= 0.0) {
        return Err(CellError::Num);
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if g(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    num_val(0.5 * (lo + hi))
}

/// Σ DCi/NLi, Σ Ai/NLi and Σ DSCi/NLi over an odd last period.
fn odd_last(sys: DateSystem, settle: f64, mat: f64, last: f64, freq: f64, basis: u32) -> R<(f64, f64, f64)> {
    if !(mat > settle && settle > last) {
        return Err(CellError::Num);
    }
    let anchor = ymd(sys, last)?;
    let eom = anchor.2 == days_in_month(anchor.0, anchor.1);
    let step = (12.0 / freq) as i64;
    let (mut dc, mut acc, mut dsc) = (0.0, 0.0, 0.0);
    // Walk forward from the last interest date: period k runs from quasi(k - 1) to quasi(k).
    let mut start = last;
    let mut k = 1;
    loop {
        if k > MAX_QUASI {
            return Err(CellError::Num);
        }
        let end = shift_months(sys, anchor, k * step, eom)?;
        let nl = normal_length(start, end, freq, basis);
        if nl <= 0.0 {
            return Err(CellError::Num);
        }
        let stop = end.min(mat);
        dc += span(sys, start, stop, basis)? / nl;
        acc += span(sys, start, settle.min(stop), basis)? / nl;
        dsc += span(sys, settle.max(start), stop, basis)? / nl;
        if end >= mat {
            break;
        }
        start = end;
        k += 1;
    }
    Ok((dc, acc, dsc))
}

/// settlement, maturity, last_interest, rate, yld/pr, redemption, frequency, [basis].
fn odd_last_args(a: &[Arg], sys: DateSystem) -> R<((f64, f64, f64), f64, f64, f64, f64)> {
    let (s, m, last) = (date(a, 0, sys)?, date(a, 1, sys)?, date(a, 2, sys)?);
    let (rate, x, red) = (num(a, 3)?, num(a, 4)?, num(a, 5)?);
    let (f, basis) = (freq_arg(a, 6)?, basis_arg(a, 7)?);
    if rate < 0.0 || red <= 0.0 {
        return Err(CellError::Num);
    }
    Ok((odd_last(sys, s, m, last, f, basis)?, rate, x, red, f))
}

fn oddlprice(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let ((dc, acc, dsc), rate, yld, red, f) = odd_last_args(a, c.date_system())?;
    if yld < 0.0 {
        return Err(CellError::Num);
    }
    let coupon = 100.0 * rate / f;
    num_val((red + dc * coupon) / (1.0 + dsc * yld / f) - acc * coupon)
}

fn oddlyield(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let ((dc, acc, dsc), rate, pr, red, f) = odd_last_args(a, c.date_system())?;
    if pr <= 0.0 {
        return Err(CellError::Num);
    }
    let coupon = 100.0 * rate / f;
    let paid = pr + acc * coupon;
    if paid <= 0.0 || dsc <= 0.0 {
        return Err(CellError::Num);
    }
    num_val((red + dc * coupon - paid) / paid * f / dsc)
}

// ---------------------------------------------------------------------------------------------
// Euro conversion
// ---------------------------------------------------------------------------------------------

/// The currencies EUROCONVERT documents: ISO code, the irrevocable conversion rate in units per
/// euro (Council Regulation (EC) No 2866/98 as amended) and the calculation precision in decimals.
const EURO_RATES: &[(&str, f64, i32)] = &[
    ("BEF", 40.3399, 0),
    ("LUF", 40.3399, 0),
    ("DEM", 1.95583, 2),
    ("ESP", 166.386, 0),
    ("FRF", 6.55957, 2),
    ("IEP", 0.787564, 2),
    ("ITL", 1936.27, 0),
    ("NLG", 2.20371, 2),
    ("ATS", 13.7603, 2),
    ("PTE", 200.482, 0),
    ("FIM", 5.94573, 2),
    ("GRD", 340.75, 0),
    ("SIT", 239.64, 2),
    ("EUR", 1.0, 2),
];

fn euro_currency(a: &[Arg], i: usize) -> R<(&'static str, f64, i32)> {
    let code = text(a, i)?;
    let code = code.trim();
    EURO_RATES.iter().copied().find(|(c, _, _)| c.eq_ignore_ascii_case(code)).ok_or(CellError::Value)
}

/// EUROCONVERT(number, source, target, [full_precision], [triangulation_precision]).
fn euroconvert(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let x = num(a, 0)?;
    let (src, src_rate, _) = euro_currency(a, 1)?;
    let (tgt, tgt_rate, tgt_prec) = euro_currency(a, 2)?;
    let full = opt_bool(a, 3, false)?;
    let triangulation = if has(a, 4) {
        let t = num(a, 4)?.trunc();
        if t < 3.0 {
            return Err(CellError::Value);
        }
        // Beyond 17 significant digits rounding changes nothing.
        Some(t.min(17.0) as usize)
    } else {
        None
    };
    if src == tgt {
        return num_val(x);
    }
    let mut euros = x / src_rate;
    if src != "EUR"
        && let Some(t) = triangulation
    {
        euros = to_sig_digits(euros, t);
    }
    let out = euros * tgt_rate;
    num_val(if full { out } else { round_half_away(out, tgt_prec) })
}

pub(crate) fn specs() -> Vec<FnSpec> {
    vec![
        f!(
            "PMT",
            3,
            5,
            Financial,
            S,
            "PMT(rate, nper, pv, [fv], [type])",
            "Periodic payment for a loan or annuity with constant payments and rate.",
            pmt
        ),
        f!("IPMT", 4, 6, Financial, S, "IPMT(rate, per, nper, pv, [fv], [type])", "Interest part of a given period's payment.", ipmt),
        f!("PPMT", 4, 6, Financial, S, "PPMT(rate, per, nper, pv, [fv], [type])", "Principal part of a given period's payment.", ppmt),
        f!("PV", 3, 5, Financial, S, "PV(rate, nper, pmt, [fv], [type])", "Present value of a series of equal future payments.", pv),
        f!("FV", 3, 5, Financial, S, "FV(rate, nper, pmt, [pv], [type])", "Future value of an investment with constant payments and rate.", fv),
        f!("NPER", 3, 5, Financial, S, "NPER(rate, pmt, pv, [fv], [type])", "Number of periods needed to pay off or accumulate an amount.", nper),
        f!(
            "RATE",
            3,
            6,
            Financial,
            S,
            "RATE(nper, pmt, pv, [fv], [type], [guess])",
            "Interest rate per period of an annuity, found iteratively.",
            rate
        ),
        f!(
            "CUMIPMT",
            6,
            6,
            Financial,
            S,
            "CUMIPMT(rate, nper, pv, start_period, end_period, type)",
            "Total interest paid between two periods.",
            cumipmt
        ),
        f!(
            "CUMPRINC",
            6,
            6,
            Financial,
            S,
            "CUMPRINC(rate, nper, pv, start_period, end_period, type)",
            "Total principal repaid between two periods.",
            cumprinc
        ),
        f!(
            "NPV",
            2,
            VAR,
            Financial,
            &[true, false],
            "NPV(rate, value1, [value2], ...)",
            "Net present value of periodic cash flows at a discount rate.",
            npv
        ),
        f!("XNPV", 3, 3, Financial, &[true, false, false], "XNPV(rate, values, dates)", "Net present value of cash flows on specific dates.", xnpv),
        f!("IRR", 1, 2, Financial, &[false, true], "IRR(values, [guess])", "Internal rate of return of periodic cash flows.", irr),
        f!(
            "XIRR",
            2,
            3,
            Financial,
            &[false, false, true],
            "XIRR(values, dates, [guess])",
            "Internal rate of return of cash flows on specific dates.",
            xirr
        ),
        f!(
            "MIRR",
            3,
            3,
            Financial,
            &[false, true, true],
            "MIRR(values, finance_rate, reinvest_rate)",
            "Modified internal rate of return using separate borrow and reinvest rates.",
            mirr
        ),
        f!(
            "FVSCHEDULE",
            2,
            2,
            Financial,
            &[true, false],
            "FVSCHEDULE(principal, schedule)",
            "Future value of a principal after a series of compound rates.",
            fvschedule
        ),
        f!("EFFECT", 2, 2, Financial, S, "EFFECT(nominal_rate, npery)", "Effective annual rate from a nominal rate and compounding periods.", effect),
        f!(
            "NOMINAL",
            2,
            2,
            Financial,
            S,
            "NOMINAL(effect_rate, npery)",
            "Nominal annual rate from an effective rate and compounding periods.",
            nominal
        ),
        f!("ISPMT", 4, 4, Financial, S, "ISPMT(rate, per, nper, pv)", "Interest paid in a period of a loan with even principal payments.", ispmt),
        f!("PDURATION", 3, 3, Financial, S, "PDURATION(rate, pv, fv)", "Periods needed for an investment to grow to a target value.", pduration),
        f!("RRI", 3, 3, Financial, S, "RRI(nper, pv, fv)", "Equivalent interest rate for the growth of an investment.", rri),
        f!(
            "DOLLARDE",
            2,
            2,
            Financial,
            S,
            "DOLLARDE(fractional_dollar, fraction)",
            "Converts a fractional price notation to a decimal number.",
            dollarde
        ),
        f!("DOLLARFR", 2, 2, Financial, S, "DOLLARFR(decimal_dollar, fraction)", "Converts a decimal price to fractional notation.", dollarfr),
        f!("SLN", 3, 3, Financial, S, "SLN(cost, salvage, life)", "Straight-line depreciation for one period.", sln),
        f!("SYD", 4, 4, Financial, S, "SYD(cost, salvage, life, per)", "Sum-of-years'-digits depreciation for a period.", syd),
        f!("DB", 4, 5, Financial, S, "DB(cost, salvage, life, period, [month])", "Fixed-declining-balance depreciation for a period.", db),
        f!(
            "DDB",
            4,
            5,
            Financial,
            S,
            "DDB(cost, salvage, life, period, [factor])",
            "Double-declining-balance (or other factor) depreciation for a period.",
            ddb
        ),
        f!(
            "VDB",
            5,
            7,
            Financial,
            S,
            "VDB(cost, salvage, life, start_period, end_period, [factor], [no_switch])",
            "Declining-balance depreciation over any span of periods, switching to straight line.",
            vdb
        ),
        f!(
            "AMORLINC",
            6,
            7,
            Financial,
            S,
            "AMORLINC(cost, date_purchased, first_period, salvage, period, rate, [basis])",
            "French linear depreciation prorated for the first period.",
            amorlinc
        ),
        f!(
            "AMORDEGRC",
            6,
            7,
            Financial,
            S,
            "AMORDEGRC(cost, date_purchased, first_period, salvage, period, rate, [basis])",
            "French degressive depreciation with a life-based coefficient.",
            amordegrc
        ),
        f!(
            "ACCRINT",
            6,
            8,
            Financial,
            S,
            "ACCRINT(issue, first_interest, settlement, rate, par, frequency, [basis], [calc_method])",
            "Accrued interest of a security paying periodic interest.",
            accrint
        ),
        f!(
            "ACCRINTM",
            3,
            5,
            Financial,
            S,
            "ACCRINTM(issue, settlement, rate, [par], [basis])",
            "Accrued interest of a security paying interest at maturity.",
            accrintm
        ),
        f!(
            "COUPDAYBS",
            3,
            4,
            Financial,
            S,
            "COUPDAYBS(settlement, maturity, frequency, [basis])",
            "Days from the start of the coupon period to settlement.",
            coupdaybs
        ),
        f!(
            "COUPDAYS",
            3,
            4,
            Financial,
            S,
            "COUPDAYS(settlement, maturity, frequency, [basis])",
            "Days in the coupon period that contains settlement.",
            coupdays
        ),
        f!(
            "COUPDAYSNC",
            3,
            4,
            Financial,
            S,
            "COUPDAYSNC(settlement, maturity, frequency, [basis])",
            "Days from settlement to the next coupon date.",
            coupdaysnc
        ),
        f!("COUPNCD", 3, 4, Financial, S, "COUPNCD(settlement, maturity, frequency, [basis])", "Next coupon date after settlement.", coupncd),
        f!(
            "COUPPCD",
            3,
            4,
            Financial,
            S,
            "COUPPCD(settlement, maturity, frequency, [basis])",
            "Previous coupon date on or before settlement.",
            couppcd
        ),
        f!(
            "COUPNUM",
            3,
            4,
            Financial,
            S,
            "COUPNUM(settlement, maturity, frequency, [basis])",
            "Number of coupons payable between settlement and maturity.",
            coupnum
        ),
        f!("DISC", 4, 5, Financial, S, "DISC(settlement, maturity, pr, redemption, [basis])", "Discount rate of a security.", disc),
        f!(
            "PRICEDISC",
            4,
            5,
            Financial,
            S,
            "PRICEDISC(settlement, maturity, discount, redemption, [basis])",
            "Price per 100 face value of a discounted security.",
            pricedisc
        ),
        f!(
            "YIELDDISC",
            4,
            5,
            Financial,
            S,
            "YIELDDISC(settlement, maturity, pr, redemption, [basis])",
            "Annual yield of a discounted security.",
            yielddisc
        ),
        f!(
            "INTRATE",
            4,
            5,
            Financial,
            S,
            "INTRATE(settlement, maturity, investment, redemption, [basis])",
            "Interest rate of a fully invested security.",
            intrate
        ),
        f!(
            "RECEIVED",
            4,
            5,
            Financial,
            S,
            "RECEIVED(settlement, maturity, investment, discount, [basis])",
            "Amount received at maturity for a fully invested security.",
            received
        ),
        f!(
            "TBILLPRICE",
            3,
            3,
            Financial,
            S,
            "TBILLPRICE(settlement, maturity, discount)",
            "Price per 100 face value of a Treasury bill.",
            tbillprice
        ),
        f!("TBILLYIELD", 3, 3, Financial, S, "TBILLYIELD(settlement, maturity, pr)", "Yield of a Treasury bill.", tbillyield),
        f!("TBILLEQ", 3, 3, Financial, S, "TBILLEQ(settlement, maturity, discount)", "Bond-equivalent yield of a Treasury bill.", tbilleq),
        f!(
            "PRICEMAT",
            5,
            6,
            Financial,
            S,
            "PRICEMAT(settlement, maturity, issue, rate, yld, [basis])",
            "Price per 100 face value of a security paying interest at maturity.",
            pricemat
        ),
        f!(
            "YIELDMAT",
            5,
            6,
            Financial,
            S,
            "YIELDMAT(settlement, maturity, issue, rate, pr, [basis])",
            "Annual yield of a security paying interest at maturity.",
            yieldmat
        ),
        f!(
            "PRICE",
            6,
            7,
            Financial,
            S,
            "PRICE(settlement, maturity, rate, yld, redemption, frequency, [basis])",
            "Price per 100 face value of a security paying periodic interest.",
            price
        ),
        f!(
            "YIELD",
            6,
            7,
            Financial,
            S,
            "YIELD(settlement, maturity, rate, pr, redemption, frequency, [basis])",
            "Yield of a security paying periodic interest.",
            yield_
        ),
        f!(
            "DURATION",
            5,
            6,
            Financial,
            S,
            "DURATION(settlement, maturity, coupon, yld, frequency, [basis])",
            "Macaulay duration of a security with periodic interest.",
            duration
        ),
        f!(
            "MDURATION",
            5,
            6,
            Financial,
            S,
            "MDURATION(settlement, maturity, coupon, yld, frequency, [basis])",
            "Modified duration of a security with periodic interest.",
            mduration
        ),
        f!(
            "ODDFPRICE",
            8,
            9,
            Financial,
            S,
            "ODDFPRICE(settlement, maturity, issue, first_coupon, rate, yld, redemption, frequency, [basis])",
            "Price per 100 face value of a security with an odd first period.",
            oddfprice
        ),
        f!(
            "ODDFYIELD",
            8,
            9,
            Financial,
            S,
            "ODDFYIELD(settlement, maturity, issue, first_coupon, rate, pr, redemption, frequency, [basis])",
            "Yield of a security with an odd first period.",
            oddfyield
        ),
        f!(
            "ODDLPRICE",
            7,
            8,
            Financial,
            S,
            "ODDLPRICE(settlement, maturity, last_interest, rate, yld, redemption, frequency, [basis])",
            "Price per 100 face value of a security with an odd last period.",
            oddlprice
        ),
        f!(
            "ODDLYIELD",
            7,
            8,
            Financial,
            S,
            "ODDLYIELD(settlement, maturity, last_interest, rate, pr, redemption, frequency, [basis])",
            "Yield of a security with an odd last period.",
            oddlyield
        ),
        f!(
            "EUROCONVERT",
            3,
            5,
            AddIn,
            S,
            "EUROCONVERT(number, source, target, [full_precision], [triangulation_precision])",
            "Converts between the euro and the former euro member currencies at the fixed rates.",
            euroconvert
        ),
    ]
}

#[cfg(test)]
mod tests {
    use crate::Arg;
    use crate::util::testutil::*;
    use gridcraft_core::{CellError, DateSystem, Value, date::serial_from_ymd};

    fn d(y: i64, m: i64, dd: i64) -> Arg {
        n(serial_from_ymd(DateSystem::D1900, y, m, dd).unwrap())
    }

    #[test]
    fn coupon_calendar_follows_the_workbook_date_system() {
        let mut ctx = TestCtx { sys: DateSystem::D1904, ..Default::default() };
        let args = vec![n(serial_from_ymd(ctx.sys, 2026, 10, 13).unwrap()), n(serial_from_ymd(ctx.sys, 2028, 2, 29).unwrap()), n(2.0), n(1.0)];
        let spec = crate::lookup("COUPDAYBS").unwrap();
        close(crate::call(spec, &args, &mut ctx), 43.0);
        for (name, date) in [("COUPPCD", (2026, 8, 31)), ("COUPNCD", (2027, 2, 28))] {
            close(crate::call(crate::lookup(name).unwrap(), &args, &mut ctx), serial_from_ymd(ctx.sys, date.0, date.1, date.2).unwrap());
        }
    }

    #[test]
    fn calendar_financial_families_are_epoch_invariant() {
        let cases = [
            ("PRICE", vec![d(2026, 10, 13), d(2028, 2, 29), n(0.05), n(0.04), n(100.0), n(2.0), n(1.0)], 2),
            ("DISC", vec![d(2026, 10, 13), d(2028, 2, 29), n(97.0), n(100.0), n(1.0)], 2),
            ("PRICEMAT", vec![d(2026, 10, 13), d(2028, 2, 29), d(2026, 2, 28), n(0.05), n(0.04), n(1.0)], 3),
            ("ODDFPRICE", vec![d(2026, 10, 13), d(2028, 2, 29), d(2026, 2, 28), d(2027, 2, 28), n(0.05), n(0.04), n(100.0), n(2.0), n(1.0)], 4),
        ];
        for (name, args, dates) in cases {
            let expected = ev(name, args.clone());
            let mut shifted = args;
            for arg in shifted.iter_mut().take(dates) {
                if let Value::Number(day) = &mut arg.value {
                    *day -= 1462.0;
                }
            }
            let mut ctx = TestCtx { sys: DateSystem::D1904, ..Default::default() };
            close_tol(crate::call(crate::lookup(name).unwrap(), &shifted, &mut ctx), number(expected), 1e-12);
        }
        let mut ctx = TestCtx { sys: DateSystem::D1904, ..Default::default() };
        for bad in [-0.5, -1.0, 2_957_004.0, f64::NAN, f64::INFINITY] {
            is_err(crate::call(crate::lookup("COUPDAYBS").unwrap(), &[n(bad), n(45_350.0), n(2.0)], &mut ctx), CellError::Num);
        }
    }

    // Observed in Excel for Mac 16.113.1 with a synthetic date1904 workbook:
    // valid fractional inputs truncate; a previous coupon before the epoch is -1,
    // not #NUM!. Only negative input serials are invalid.
    #[test]
    fn calendar_financial_dates_keep_excel_epoch_boundary_results() {
        let mut ctx = TestCtx { sys: DateSystem::D1904, ..Default::default() };
        close(crate::call(crate::lookup("COUPDAYBS").unwrap(), &[n(10.75), n(45350.9), n(2.0)], &mut ctx), 131.0);
        close(crate::call(crate::lookup("COUPDAYBS").unwrap(), &[n(0.0), n(45350.0), n(2.0)], &mut ctx), 121.0);
        let args = [n(1.0), n(serial_from_ymd(ctx.sys, 1905, 6, 30).unwrap()), n(2.0), n(1.0)];
        close(crate::call(crate::lookup("COUPPCD").unwrap(), &args, &mut ctx), -1.0);
        close(crate::call(crate::lookup("COUPNCD").unwrap(), &args, &mut ctx), 181.0);
    }

    #[test]
    fn discount_securities_reject_dates_beyond_each_workbook_epoch_limit() {
        for sys in [DateSystem::D1900, DateSystem::D1904] {
            let mut ctx = TestCtx { sys, ..Default::default() };
            let last = gridcraft_core::date::max_serial(sys) as f64;
            for basis in [2.0, 3.0] {
                let spec = crate::lookup("DISC").unwrap();
                let valid = [n(last - 1.0), n(last + 0.75), n(97.0), n(100.0), n(basis)];
                close(crate::call(spec, &valid, &mut ctx), 0.03 * if basis == 2.0 { 360.0 } else { 365.0 });
                for (settle, maturity) in [(last, last + 1.0), (last + 1.0, last + 2.0)] {
                    let invalid = [n(settle), n(maturity), n(97.0), n(100.0), n(basis)];
                    is_err(crate::call(spec, &invalid, &mut ctx), CellError::Num);
                }
            }
        }
    }

    #[test]
    fn tvm() {
        close_tol(ev("PMT", vec![n(0.08 / 12.0), n(10.0), n(10000.0)]), -1037.032089, 1e-9);
        close_tol(ev("PMT", vec![n(0.06 / 12.0), n(216.0), n(0.0), n(50000.0)]), -129.0811609, 1e-9);
        close(ev("PMT", vec![n(0.0), n(10.0), n(1000.0)]), -100.0);
        is_err(ev("PMT", vec![n(0.1), n(0.0), n(1000.0)]), CellError::Num);
        close_tol(ev("FV", vec![n(0.06 / 12.0), n(10.0), n(-200.0), n(-500.0), n(1.0)]), 2581.403374, 1e-9);
        close_tol(ev("FV", vec![n(0.01), n(12.0), n(-1000.0)]), 12682.50301, 1e-9);
        close(ev("FV", vec![n(0.0), n(10.0), n(-100.0)]), 1000.0);
        close_tol(ev("PV", vec![n(0.08 / 12.0), n(240.0), n(500.0)]), -59777.14585, 1e-9);
        close_tol(ev("NPER", vec![n(0.01), n(-100.0), n(-1000.0), n(10000.0), n(1.0)]), 59.6738657, 1e-8);
        close_tol(ev("NPER", vec![n(0.01), n(-100.0), n(-1000.0), n(10000.0)]), 60.0821229, 1e-8);
        close_tol(ev("NPER", vec![n(0.01), n(-100.0), n(-1000.0)]), -9.57859404, 1e-8);
        close_tol(ev("IPMT", vec![n(0.1 / 12.0), n(1.0), n(36.0), n(8000.0)]), -66.66666667, 1e-9);
        close_tol(ev("IPMT", vec![n(0.1), n(3.0), n(3.0), n(8000.0)]), -292.4471299, 1e-9);
        is_err(ev("IPMT", vec![n(0.1), n(4.0), n(3.0), n(8000.0)]), CellError::Num);
        close_tol(ev("PPMT", vec![n(0.1 / 12.0), n(1.0), n(24.0), n(2000.0)]), -75.62318601, 1e-9);
        close_tol(ev("PPMT", vec![n(0.08), n(10.0), n(10.0), n(200000.0)]), -27598.05346, 1e-9);
        close_tol(ev("RATE", vec![n(48.0), n(-200.0), n(8000.0)]), 0.0077014725, 1e-7);
        close_tol(ev("CUMIPMT", vec![n(0.09 / 12.0), n(360.0), n(125000.0), n(13.0), n(24.0), n(0.0)]), -11135.23213, 1e-9);
        close_tol(ev("CUMIPMT", vec![n(0.09 / 12.0), n(360.0), n(125000.0), n(1.0), n(1.0), n(0.0)]), -937.5, 1e-9);
        close_tol(ev("CUMPRINC", vec![n(0.09 / 12.0), n(360.0), n(125000.0), n(13.0), n(24.0), n(0.0)]), -934.1071234, 1e-9);
        close_tol(ev("CUMPRINC", vec![n(0.09 / 12.0), n(360.0), n(125000.0), n(1.0), n(1.0), n(0.0)]), -68.27827118, 1e-9);
        is_err(ev("CUMIPMT", vec![n(0.0), n(360.0), n(125000.0), n(1.0), n(1.0), n(0.0)]), CellError::Num);
        is_err(ev("CUMIPMT", vec![n(0.01), n(360.0), n(125000.0), n(1.0), n(1.0), n(2.0)]), CellError::Num);
    }

    #[test]
    fn cash_flows() {
        close_tol(ev("NPV", vec![n(0.1), n(-10000.0), n(3000.0), n(4200.0), n(6800.0)]), 1188.443412, 1e-9);
        close_tol(ev("NPV", vec![n(0.08), rf(col(&[8000.0, 9200.0, 10000.0, 12000.0, 14500.0]))]), 41922.06155, 1e-9);
        is_err(ev("NPV", vec![n(-1.0), n(1.0)]), CellError::Div0);
        let flows = col(&[-70000.0, 12000.0, 15000.0, 18000.0, 21000.0]);
        close_tol(ev("IRR", vec![rf(flows)]), -0.021244848, 1e-7);
        close_tol(ev("IRR", vec![rf(col(&[-70000.0, 12000.0, 15000.0, 18000.0, 21000.0, 26000.0]))]), 0.086630948, 1e-7);
        close_tol(ev("IRR", vec![rf(col(&[-70000.0, 12000.0, 15000.0])), n(-0.1)]), -0.44350694, 1e-7);
        is_err(ev("IRR", vec![rf(col(&[1.0, 2.0]))]), CellError::Num);
        let vals = col(&[-10000.0, 2750.0, 4250.0, 3250.0, 2750.0]);
        let dates = col(&[39448.0, 39508.0, 39751.0, 39859.0, 39904.0]);
        close_tol(ev("XNPV", vec![n(0.09), rf(vals.clone()), rf(dates.clone())]), 2086.647602, 1e-9);
        close_tol(ev("XIRR", vec![rf(vals.clone()), rf(dates)]), 0.373362535, 1e-7);
        is_err(ev("XNPV", vec![n(0.09), rf(vals), rf(col(&[1.0, 2.0]))]), CellError::Num);
        let m = col(&[-120000.0, 39000.0, 30000.0, 21000.0, 37000.0, 46000.0]);
        close_tol(ev("MIRR", vec![rf(m), n(0.1), n(0.12)]), 0.1260943, 1e-6);
        close_tol(ev("MIRR", vec![rf(col(&[-120000.0, 39000.0, 30000.0, 21000.0])), n(0.1), n(0.12)]), -0.048044655, 1e-7);
        is_err(ev("MIRR", vec![rf(col(&[1.0, 2.0])), n(0.1), n(0.1)]), CellError::Div0);
        // Excel 365 gives -1 when every flow is negative: the positive flows are worth nothing.
        close_tol(ev("MIRR", vec![rf(col(&[-10000.0, -3400.0, -6500.0, -1000.0])), n(0.07), n(0.01)]), -1.0, 1e-12);
        close(ev("FVSCHEDULE", vec![n(1.0), av(row(&[0.09, 0.11, 0.1]))]), 1.33089);
        close(ev("EFFECT", vec![n(0.0525), n(4.0)]), 0.0535426673707584);
        is_err(ev("EFFECT", vec![n(-0.1), n(4.0)]), CellError::Num);
        close_tol(ev("NOMINAL", vec![n(0.053543), n(4.0)]), 0.052500319, 1e-8);
        close_tol(ev("ISPMT", vec![n(0.1 / 12.0), n(0.0), n(36.0), n(8000000.0)]), -66666.66667, 1e-9);
        is_err(ev("ISPMT", vec![n(0.1), n(1.0), n(0.0), n(1.0)]), CellError::Div0);
        close_tol(ev("PDURATION", vec![n(0.025), n(2000.0), n(2200.0)]), 3.859866163, 1e-9);
        is_err(ev("PDURATION", vec![n(0.0), n(2000.0), n(2200.0)]), CellError::Num);
        close_tol(ev("RRI", vec![n(96.0), n(10000.0), n(11000.0)]), 0.0009933, 1e-4);
        close(ev("DOLLARDE", vec![n(1.02), n(16.0)]), 1.125);
        close(ev("DOLLARDE", vec![n(1.1), n(32.0)]), 1.3125);
        close(ev("DOLLARFR", vec![n(1.125), n(16.0)]), 1.02);
        close(ev("DOLLARFR", vec![n(1.125), n(32.0)]), 1.04);
        is_err(ev("DOLLARDE", vec![n(1.02), n(0.0)]), CellError::Div0);
        is_err(ev("DOLLARDE", vec![n(1.02), n(-1.0)]), CellError::Num);
    }

    #[test]
    fn depreciation() {
        close(ev("SLN", vec![n(30000.0), n(7500.0), n(10.0)]), 2250.0);
        is_err(ev("SLN", vec![n(30000.0), n(7500.0), n(0.0)]), CellError::Div0);
        close_tol(ev("SYD", vec![n(30000.0), n(7500.0), n(10.0), n(1.0)]), 4090.909091, 1e-9);
        close_tol(ev("SYD", vec![n(30000.0), n(7500.0), n(10.0), n(10.0)]), 409.0909091, 1e-9);
        let dbv = |p: f64| ev("DB", vec![n(1000000.0), n(100000.0), n(6.0), n(p), n(7.0)]);
        close_tol(dbv(1.0), 186083.3333, 1e-9);
        close_tol(dbv(2.0), 259639.4167, 1e-9);
        close_tol(dbv(3.0), 176814.4428, 1e-9);
        close_tol(dbv(7.0), 15845.09847, 1e-9);
        is_err(dbv(8.0), CellError::Num);
        close_tol(ev("DDB", vec![n(2400.0), n(300.0), n(3650.0), n(1.0)]), 1.315068493, 1e-8);
        close(ev("DDB", vec![n(2400.0), n(300.0), n(120.0), n(1.0), n(2.0)]), 40.0);
        close(ev("DDB", vec![n(2400.0), n(300.0), n(10.0), n(1.0), n(2.0)]), 480.0);
        close_tol(ev("DDB", vec![n(2400.0), n(300.0), n(10.0), n(2.0), n(1.5)]), 306.0, 1e-12);
        close_tol(ev("DDB", vec![n(2400.0), n(300.0), n(10.0), n(10.0)]), 22.1225472, 1e-8);
        is_err(ev("DDB", vec![n(2400.0), n(300.0), n(10.0), n(11.0)]), CellError::Num);
        close_tol(ev("VDB", vec![n(2400.0), n(300.0), n(3650.0), n(0.0), n(1.0)]), 1.315068493, 1e-8);
        close(ev("VDB", vec![n(2400.0), n(300.0), n(120.0), n(0.0), n(1.0)]), 40.0);
        close(ev("VDB", vec![n(2400.0), n(300.0), n(10.0), n(0.0), n(1.0)]), 480.0);
        close_tol(ev("VDB", vec![n(2400.0), n(300.0), n(120.0), n(6.0), n(18.0)]), 396.3060533, 1e-8);
        close_tol(ev("VDB", vec![n(2400.0), n(300.0), n(120.0), n(6.0), n(18.0), n(1.5)]), 311.8089367, 1e-8);
        close(ev("VDB", vec![n(2400.0), n(300.0), n(10.0), n(0.0), n(0.875), n(1.5)]), 315.0);
        // Over the whole life the total depreciation is cost - salvage.
        close(ev("VDB", vec![n(2400.0), n(300.0), n(10.0), n(0.0), n(10.0)]), 2100.0);
        is_err(ev("VDB", vec![n(2400.0), n(300.0), n(10.0), n(5.0), n(4.0)]), CellError::Num);
        close_tol(ev("AMORLINC", vec![n(2400.0), d(2008, 8, 19), d(2008, 12, 31), n(300.0), n(1.0), n(0.15), n(1.0)]), 360.0, 1e-9);
        close(ev("AMORDEGRC", vec![n(2400.0), d(2008, 8, 19), d(2008, 12, 31), n(300.0), n(1.0), n(0.15), n(1.0)]), 776.0);
    }

    #[test]
    fn coupons() {
        let args = |f: &str| ev(f, vec![d(2011, 1, 25), d(2011, 11, 15), n(2.0), n(1.0)]);
        close(args("COUPDAYBS"), 71.0);
        close(args("COUPDAYS"), 181.0);
        close(args("COUPDAYSNC"), 110.0);
        close(args("COUPNCD"), 40678.0);
        close(args("COUPPCD"), 40497.0);
        close(args("COUPNUM"), 2.0);
        close(ev("COUPDAYS", vec![d(2011, 1, 25), d(2011, 11, 15), n(2.0), n(0.0)]), 180.0);
        close(ev("COUPDAYS", vec![d(2011, 1, 25), d(2011, 11, 15), n(4.0), n(3.0)]), 91.25);
        close(ev("COUPNUM", vec![d(2007, 1, 25), d(2008, 11, 15), n(2.0), n(1.0)]), 4.0);
        is_err(ev("COUPNUM", vec![d(2012, 1, 25), d(2011, 11, 15), n(2.0)]), CellError::Num);
        is_err(ev("COUPNUM", vec![d(2011, 1, 25), d(2011, 11, 15), n(3.0)]), CellError::Num);
        is_err(ev("COUPNUM", vec![d(2011, 1, 25), d(2011, 11, 15), n(2.0), n(5.0)]), CellError::Num);
        // End-of-month maturity keeps month-end coupon dates.
        close(ev("COUPPCD", vec![d(2011, 3, 15), d(2011, 11, 30), n(2.0)]), serial_from_ymd(DateSystem::D1900, 2010, 11, 30).unwrap());
        close(ev("COUPNCD", vec![d(2011, 3, 15), d(2011, 11, 30), n(2.0)]), serial_from_ymd(DateSystem::D1900, 2011, 5, 31).unwrap());
    }

    #[test]
    fn securities() {
        close_tol(ev("DISC", vec![d(2007, 1, 25), d(2007, 6, 15), n(97.975), n(100.0), n(1.0)]), 0.052420213, 1e-8);
        close_tol(ev("PRICEDISC", vec![d(2008, 2, 16), d(2008, 3, 1), n(0.0525), n(100.0), n(2.0)]), 99.79583333, 1e-9);
        close_tol(ev("YIELDDISC", vec![d(2008, 2, 16), d(2008, 3, 1), n(99.795), n(100.0), n(2.0)]), 0.052823, 1e-5);
        close_tol(ev("INTRATE", vec![d(2008, 2, 15), d(2008, 5, 15), n(1000000.0), n(1014420.0), n(2.0)]), 0.05768, 1e-9);
        close_tol(ev("RECEIVED", vec![d(2008, 2, 15), d(2008, 5, 15), n(1000000.0), n(0.0575), n(2.0)]), 1014584.654, 1e-9);
        is_err(ev("DISC", vec![d(2008, 2, 15), d(2008, 1, 15), n(97.0), n(100.0)]), CellError::Num);
        close_tol(ev("TBILLPRICE", vec![d(2008, 3, 31), d(2008, 6, 1), n(0.09)]), 98.45, 1e-12);
        close_tol(ev("TBILLYIELD", vec![d(2008, 3, 31), d(2008, 6, 1), n(98.45)]), 0.091417, 1e-6);
        close_tol(ev("TBILLEQ", vec![d(2008, 3, 31), d(2008, 6, 1), n(0.0914)]), 0.094151, 1e-6);
        is_err(ev("TBILLPRICE", vec![d(2008, 3, 31), d(2009, 6, 1), n(0.09)]), CellError::Num);
        close_tol(ev("ACCRINTM", vec![d(2008, 4, 1), d(2008, 6, 15), n(0.1), n(1000.0), n(3.0)]), 20.54794521, 1e-9);
        is_err(ev("ACCRINTM", vec![d(2008, 6, 15), d(2008, 4, 1), n(0.1)]), CellError::Num);
        close_tol(ev("ACCRINT", vec![d(2008, 3, 1), d(2008, 8, 31), d(2008, 5, 1), n(0.1), n(1000.0), n(2.0), n(0.0)]), 16.66666667, 1e-9);
        close_tol(ev("ACCRINT", vec![d(2008, 3, 5), d(2008, 8, 31), d(2008, 5, 1), n(0.1), n(1000.0), n(2.0), n(0.0), b(false)]), 15.55555556, 1e-9);
        close_tol(ev("PRICEMAT", vec![d(2008, 2, 15), d(2008, 4, 13), d(2007, 11, 11), n(0.061), n(0.061), n(0.0)]), 99.98449888, 1e-9);
        close_tol(ev("YIELDMAT", vec![d(2008, 3, 15), d(2008, 11, 3), d(2007, 11, 8), n(0.0625), n(100.0123), n(0.0)]), 0.060954, 1e-6);
        close_tol(ev("PRICE", vec![d(2008, 2, 15), d(2017, 11, 15), n(0.0575), n(0.065), n(100.0), n(2.0), n(0.0)]), 94.63436, 1e-7);
        close_tol(ev("YIELD", vec![d(2008, 2, 15), d(2016, 11, 15), n(0.0575), n(95.04287), n(100.0), n(2.0), n(0.0)]), 0.065, 1e-6);
        is_err(ev("PRICE", vec![d(2008, 2, 15), d(2017, 11, 15), n(-0.01), n(0.065), n(100.0), n(2.0)]), CellError::Num);
        close_tol(ev("DURATION", vec![d(2018, 7, 1), d(2048, 1, 1), n(0.08), n(0.09), n(2.0), n(1.0)]), 10.9191453, 1e-8);
        close_tol(ev("MDURATION", vec![d(2008, 1, 1), d(2016, 1, 1), n(0.08), n(0.09), n(2.0), n(1.0)]), 5.73567, 1e-6);
        // A one-coupon bond uses the closed form for yield; PRICE and YIELD invert each other.
        let p = ev("PRICE", vec![d(2008, 2, 15), d(2008, 6, 15), n(0.05), n(0.04), n(100.0), n(2.0)]);
        let pv = match p {
            Value::Number(x) => x,
            _ => panic!(),
        };
        close_tol(ev("YIELD", vec![d(2008, 2, 15), d(2008, 6, 15), n(0.05), n(pv), n(100.0), n(2.0)]), 0.04, 1e-9);
    }

    fn number(v: Value) -> f64 {
        match v {
            Value::Number(x) => x,
            other => panic!("expected a number, got {other:?}"),
        }
    }

    #[test]
    fn odd_periods() {
        // The worked examples on Microsoft's function pages (results as printed there).
        let first = |x: f64, rest: &[f64]| {
            let mut a = vec![d(2008, 11, 11), d(2021, 3, 1), d(2008, 10, 15), d(2009, 3, 1), n(x)];
            a.extend(rest.iter().map(|v| n(*v)));
            a
        };
        close_tol(ev("ODDFPRICE", first(0.0785, &[0.0625, 100.0, 2.0, 1.0])), 113.60, 0.005 / 113.6);
        close_tol(ev("ODDFYIELD", first(0.0575, &[84.50, 100.0, 2.0, 0.0])), 0.0772, 0.00005);
        let lp = vec![d(2008, 2, 7), d(2008, 6, 15), d(2007, 10, 15), n(0.0375), n(0.0405), n(100.0), n(2.0), n(0.0)];
        close_tol(ev("ODDLPRICE", lp), 99.88, 0.005 / 99.88);
        let ly = vec![d(2008, 4, 20), d(2008, 6, 15), d(2007, 12, 24), n(0.0375), n(99.875), n(100.0), n(2.0), n(0.0)];
        close_tol(ev("ODDLYIELD", ly), 0.04519, 0.000005);

        // Yield → price → yield, for a short and a long odd first period and a long odd last period.
        for (issue, basis) in [(d(2008, 10, 15), 0.0), (d(2007, 1, 20), 1.0), (d(2008, 1, 31), 3.0)] {
            let args = |x: f64| vec![d(2008, 11, 11), d(2021, 3, 1), issue.clone(), d(2009, 3, 1), n(0.06), n(x), n(100.0), n(2.0), n(basis)];
            let p = number(ev("ODDFPRICE", args(0.05)));
            close_tol(ev("ODDFYIELD", args(p)), 0.05, 1e-9);
        }
        let last = |x: f64| vec![d(2009, 2, 1), d(2009, 12, 15), d(2008, 3, 15), n(0.04), n(x), n(100.0), n(4.0), n(1.0)];
        let p = number(ev("ODDLPRICE", last(0.07)));
        close_tol(ev("ODDLYIELD", last(p)), 0.07, 1e-9);

        // Argument checks: frequency, basis, date order, negative yield, non-positive price.
        is_err(ev("ODDFPRICE", first(0.0785, &[0.0625, 100.0, 3.0, 1.0])), CellError::Num);
        is_err(ev("ODDFPRICE", first(0.0785, &[0.0625, 100.0, 2.0, 5.0])), CellError::Num);
        is_err(ev("ODDFPRICE", first(0.0785, &[-0.01, 100.0, 2.0, 1.0])), CellError::Num);
        is_err(ev("ODDFYIELD", first(0.0575, &[0.0, 100.0, 2.0, 0.0])), CellError::Num);
        is_err(
            ev("ODDFPRICE", vec![d(2009, 4, 1), d(2021, 3, 1), d(2008, 10, 15), d(2009, 3, 1), n(0.07), n(0.06), n(100.0), n(2.0)]),
            CellError::Num,
        );
        is_err(ev("ODDLPRICE", vec![d(2007, 10, 1), d(2008, 6, 15), d(2007, 10, 15), n(0.0375), n(0.0405), n(100.0), n(2.0)]), CellError::Num);
        is_err(ev("ODDLYIELD", vec![d(2008, 4, 20), d(2008, 6, 15), d(2007, 12, 24), n(0.0375), n(-1.0), n(100.0), n(2.0)]), CellError::Num);
        is_err(
            ev("ODDFPRICE", first(0.0785, &[0.0625, 100.0, 2.0, 1.0]).into_iter().take(4).chain([t("x"), n(0.06), n(100.0), n(2.0)]).collect()),
            CellError::Value,
        );
    }

    #[test]
    fn odd_periods_survive_hostile_input() {
        // Quarterly quasi-periods across the whole date range, huge rates and yields: a value or
        // an error, never a panic or a hang.
        let wide = |x: f64, y: f64| vec![n(2_957_000.0), n(2_958_465.0), n(400.0), n(2_957_900.0), n(x), n(y), n(1e300), n(4.0), n(1.0)];
        for (x, y) in [(0.05, 0.05), (1e300, 1e300), (0.0, 1e-300)] {
            let _ = ev("ODDFPRICE", wide(x, y));
            let _ = ev("ODDFYIELD", wide(x, y));
        }
        // The long walk itself works: settled early in an odd first period of ~32,000 quarters,
        // a small yield round-trips.
        let long = |y: f64| vec![n(500.0), n(2_958_465.0), n(400.0), n(2_957_900.0), n(0.05), n(y), n(100.0), n(4.0), n(1.0)];
        let p = number(ev("ODDFPRICE", long(0.0001)));
        close_tol(ev("ODDFYIELD", long(p)), 0.0001, 1e-9);
        let last = |x: f64, y: f64| vec![n(2_958_000.0), n(2_958_465.0), n(400.0), n(x), n(y), n(1e300), n(4.0), n(1.0)];
        for (x, y) in [(0.05, 0.05), (1e300, 1e300), (0.0, f64::MAX)] {
            let _ = ev("ODDLPRICE", last(x, y));
            let _ = ev("ODDLYIELD", last(x, y));
        }
        is_err(ev("ODDFPRICE", vec![n(-5.0), n(1e20), n(0.0), n(1e10), n(0.05), n(0.05), n(100.0), n(2.0)]), CellError::Num);
    }

    #[test]
    fn euroconvert() {
        // The examples on Microsoft's EUROCONVERT page.
        let ec = |x: f64, s: &str, tg: &str, rest: Vec<Arg>| {
            let mut a = vec![n(x), t(s), t(tg)];
            a.extend(rest);
            ev("EUROCONVERT", a)
        };
        close(ec(1.2, "DEM", "EUR", vec![]), 0.61);
        close(ec(1.0, "FRF", "EUR", vec![b(true), n(3.0)]), 0.152);
        close(ec(1.0, "FRF", "EUR", vec![b(false), n(3.0)]), 0.15);
        close(ec(1.0, "FRF", "DEM", vec![b(true), n(3.0)]), 0.29728616);
        // Euro to a member currency rounds to the target's calculation precision (lira: 0).
        close(ec(10.0, "eur", "ITL", vec![]), 19363.0);
        close(ec(100.0, "EUR", "IEP", vec![b(true)]), 78.7564);
        // Same currency returns the number untouched.
        close(ec(1.23456, "DEM", "DEM", vec![]), 1.23456);
        // Unknown codes and a triangulation precision below 3 are #VALUE!.
        is_err(ec(1.0, "USD", "EUR", vec![]), CellError::Value);
        is_err(ec(1.0, "FRF", "DEM", vec![b(true), n(2.0)]), CellError::Value);
        // Hostile magnitudes give a number or #NUM!, never a panic.
        is_err(ec(f64::MAX, "EUR", "ITL", vec![]), CellError::Num);
        close(ec(1.0, "FRF", "DEM", vec![b(true), n(1e300)]), 1.95583 / 6.55957);
    }

    #[test]
    fn lifting() {
        let v = ev("SLN", vec![av(row(&[100.0, 200.0])), n(0.0), n(10.0)]);
        assert_eq!(rows_of(&v), vec![vec![nv(10.0), nv(20.0)]]);
        is_err(ev("PMT", vec![t("abc"), n(1.0), n(1.0)]), CellError::Value);
        is_err(ev("PMT", vec![e(CellError::NA), n(1.0), n(1.0)]), CellError::NA);
    }

    #[test]
    fn securities_read_dates_in_the_1904_system() {
        let d4 = |y, m, dd| n(serial_from_ymd(DateSystem::D1904, y, m, dd).unwrap());
        let ev4 = |name: &str, args: Vec<Arg>| {
            let mut ctx = TestCtx { sys: DateSystem::D1904, ..TestCtx::default() };
            crate::call(crate::lookup(name).unwrap(), &args, &mut ctx)
        };
        let (s, m) = (d4(2011, 3, 15), d4(2011, 11, 30));
        close(ev4("COUPNCD", vec![s.clone(), m.clone(), n(2.0)]), 39232.0);
        close(ev4("COUPPCD", vec![s.clone(), m.clone(), n(4.0), n(0.0)]), 39140.0);
        close(ev4("COUPDAYBS", vec![s.clone(), m.clone(), n(4.0), n(0.0)]), 15.0);
        let accrint = vec![d4(2008, 3, 1), d4(2008, 8, 31), d4(2008, 5, 1), n(0.1), n(1000.0), n(2.0), n(1.0)];
        close_tol(ev4("ACCRINT", accrint), 16.576086956522, 1e-9);
        // Results that already matched Excel keep matching.
        let semi = |name| ev4(name, vec![s.clone(), m.clone(), n(2.0), n(0.0)]);
        close(semi("COUPPCD"), 39050.0);
        close(semi("COUPDAYS"), 180.0);
        close(semi("COUPDAYBS"), 105.0);
        close(semi("COUPDAYSNC"), 75.0);
        close(semi("COUPNUM"), 2.0);
        let price = vec![s.clone(), m.clone(), n(0.0575), n(0.065), n(100.0), n(2.0), n(0.0)];
        close_tol(ev4("PRICE", price), 99.47768286339101, 1e-9);
        close_tol(ev4("DURATION", vec![s, m, n(0.08), n(0.09), n(2.0), n(0.0)]), 0.6890136809022, 1e-9);
        let odd = vec![d4(2008, 11, 11), d4(2021, 3, 1), d4(2008, 10, 15), d4(2009, 3, 1), n(0.0785), n(0.0625), n(100.0), n(2.0), n(1.0)];
        close_tol(ev4("ODDFPRICE", odd), 113.597717474079, 1e-9);
    }
}
