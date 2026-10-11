//! Financial functions: time value of money, cash-flow analysis, depreciation and securities.
//!
//! Sign convention follows Excel: money paid out is negative, money received is positive.
//! Date arguments are serial numbers (truncated) in the 1900 date system.

use gridcraft_core::date::{datetime_from_serial, days_in_month, serial_from_ymd};
use gridcraft_core::{CellError, DateSystem, Value};

use crate::util::{R, S, array_numbers, as_array, flatten, has, num, num_val, numbers, opt_bool, opt_num, to_num};
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

fn type_arg(c: &dyn Ctx, args: &[Arg], i: usize) -> R<f64> {
    Ok(if opt_num(c, args, i, 0.0)? != 0.0 { 1.0 } else { 0.0 })
}

fn pmt(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
    let (fv, typ) = (opt_num(c, a, 3, 0.0)?, type_arg(c, a, 4)?);
    if nper == 0.0 {
        return Err(CellError::Num);
    }
    num_val(pmt_calc(rate, nper, pv, fv, typ))
}

fn fv(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pmt) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
    num_val(fv_calc(rate, nper, pmt, opt_num(c, a, 3, 0.0)?, type_arg(c, a, 4)?))
}

fn pv(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (rate, nper, pmt) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
    num_val(pv_calc(rate, nper, pmt, opt_num(c, a, 3, 0.0)?, type_arg(c, a, 4)?))
}

fn nper(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (rate, pmt, pv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
    let (fv, typ) = (opt_num(c, a, 3, 0.0)?, type_arg(c, a, 4)?);
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

fn ipmt(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (rate, per, np, pv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?);
    let (fv, typ) = (opt_num(c, a, 4, 0.0)?, type_arg(c, a, 5)?);
    if per < 1.0 || per > np || np == 0.0 {
        return Err(CellError::Num);
    }
    num_val(ipmt_calc(rate, per, np, pv, fv, typ))
}

fn ppmt(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (rate, per, np, pv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?);
    let (fv, typ) = (opt_num(c, a, 4, 0.0)?, type_arg(c, a, 5)?);
    if per < 1.0 || per > np || np == 0.0 {
        return Err(CellError::Num);
    }
    num_val(pmt_calc(rate, np, pv, fv, typ) - ipmt_calc(rate, per, np, pv, fv, typ))
}

fn cumulative(c: &dyn Ctx, a: &[Arg], principal: bool) -> R<Value> {
    let (rate, np, pv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
    let (start, end, typ) = (num(c, a, 3)?.trunc(), num(c, a, 4)?.trunc(), num(c, a, 5)?);
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

fn cumipmt(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    cumulative(c, a, false)
}

fn cumprinc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    cumulative(c, a, true)
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

fn rate(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (np, pmt, pv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
    let (fv, typ, guess) = (opt_num(c, a, 3, 0.0)?, type_arg(c, a, 4)?, opt_num(c, a, 5, 0.1)?);
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

fn npv(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let rate = num(c, a, 0)?;
    if rate == -1.0 {
        return Err(CellError::Div0);
    }
    let vals = numbers(c, a.get(1..).unwrap_or(&[]))?;
    let mut sum = 0.0;
    let mut d = 1.0;
    for v in vals {
        d *= 1.0 + rate;
        sum += v / d;
    }
    num_val(sum)
}

/// Values and dates for XNPV/XIRR: both must be numeric and of equal length.
fn dated_flows(c: &dyn Ctx, values: &Value, dates: &Value) -> R<(Vec<f64>, Vec<f64>)> {
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
            Value::Text(_) => d.push(to_num(c, y)?.trunc()),
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

fn xnpv(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let rate = num(c, a, 0)?;
    if rate <= -1.0 {
        return Err(CellError::Num);
    }
    let (v, d) = dated_flows(c, &a.get(1).ok_or(CellError::Value)?.value, &a.get(2).ok_or(CellError::Value)?.value)?;
    num_val(xnpv_calc(rate, &v, &d))
}

fn xirr(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (v, d) = dated_flows(c, &a.first().ok_or(CellError::Value)?.value, &a.get(1).ok_or(CellError::Value)?.value)?;
    let guess = opt_num(c, a, 2, 0.1)?;
    if !v.iter().any(|x| *x > 0.0) || !v.iter().any(|x| *x < 0.0) {
        return Err(CellError::Num);
    }
    num_val(solve_rate(|r| xnpv_calc(r, &v, &d), guess)?)
}

fn irr(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let v = array_numbers(c, &a.first().ok_or(CellError::Value)?.value)?;
    let guess = opt_num(c, a, 1, 0.1)?;
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

fn mirr(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let v = array_numbers(c, &a.first().ok_or(CellError::Value)?.value)?;
    let (fr, rr) = (num(c, a, 1)?, num(c, a, 2)?);
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
    if pos == 0.0 || neg == 0.0 {
        return Err(CellError::Div0);
    }
    let r = (-pos * (1.0 + rr).powf(n - 1.0) / neg).powf(1.0 / (n - 1.0)) - 1.0;
    num_val(r)
}

fn fvschedule(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let mut p = num(c, a, 0)?;
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

fn effect(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (nom, n) = (num(c, a, 0)?, num(c, a, 1)?.trunc());
    if nom <= 0.0 || n < 1.0 {
        return Err(CellError::Num);
    }
    num_val((1.0 + nom / n).powf(n) - 1.0)
}

fn nominal(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (eff, n) = (num(c, a, 0)?, num(c, a, 1)?.trunc());
    if eff <= 0.0 || n < 1.0 {
        return Err(CellError::Num);
    }
    num_val(n * ((1.0 + eff).powf(1.0 / n) - 1.0))
}

fn ispmt(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (rate, per, np, pv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?);
    if np == 0.0 {
        return Err(CellError::Div0);
    }
    num_val(pv * rate * (per / np - 1.0))
}

fn pduration(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (rate, pv, fv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
    if rate <= 0.0 || pv <= 0.0 || fv <= 0.0 {
        return Err(CellError::Num);
    }
    num_val((fv.ln() - pv.ln()) / (1.0 + rate).ln())
}

fn rri(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (np, pv, fv) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
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

fn dollarde(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (x, f) = (num(c, a, 0)?, num(c, a, 1)?.trunc());
    let p = dollar_digits(f)?;
    let int = x.trunc();
    num_val(int + (x - int) * p / f)
}

fn dollarfr(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (x, f) = (num(c, a, 0)?, num(c, a, 1)?.trunc());
    let p = dollar_digits(f)?;
    let int = x.trunc();
    num_val(int + (x - int) * f / p)
}

// ---------------------------------------------------------------------------------------------
// Depreciation
// ---------------------------------------------------------------------------------------------

fn sln(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?);
    if life == 0.0 {
        return Err(CellError::Div0);
    }
    num_val((cost - salvage) / life)
}

fn syd(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life, per) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?);
    if life <= 0.0 || per <= 0.0 || per > life || salvage < 0.0 {
        return Err(CellError::Num);
    }
    num_val((cost - salvage) * (life - per + 1.0) * 2.0 / (life * (life + 1.0)))
}

fn db(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life, period) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?.trunc());
    let month = opt_num(c, a, 4, 12.0)?.trunc();
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

fn ddb(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life, period) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?);
    let factor = opt_num(c, a, 4, 2.0)?;
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

fn vdb(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (cost, salvage, life, start, end) = (num(c, a, 0)?, num(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?, num(c, a, 4)?);
    let factor = opt_num(c, a, 5, 2.0)?;
    let no_switch = opt_bool(c, a, 6, false)?;
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
    let (cost, purchased, first, salvage) = (num(c, a, 0)?, date(c, a, 1)?, date(c, a, 2)?, num(c, a, 3)?);
    let (period, rate, basis) = (num(c, a, 4)?.trunc(), num(c, a, 5)?, basis_arg(c, a, 6)?);
    let _ = c;
    if cost < 0.0 || salvage < 0.0 || salvage > cost || period < 0.0 || rate <= 0.0 || first < purchased {
        return Err(CellError::Num);
    }
    let full = cost * rate;
    let limit = cost - salvage;
    let dep0 = (full * year_frac(purchased, first, basis)?).min(limit);
    if period == 0.0 {
        return num_val(dep0);
    }
    let remaining = limit - dep0 - (period - 1.0) * full;
    num_val(full.min(remaining.max(0.0)))
}

fn amordegrc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (cost, purchased, first, salvage) = (num(c, a, 0)?, date(c, a, 1)?, date(c, a, 2)?, num(c, a, 3)?);
    let (period, rate, basis) = (num(c, a, 4)?.trunc(), num(c, a, 5)?, basis_arg(c, a, 6)?);
    let _ = c;
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
    let mut dep = (year_frac(purchased, first, basis)? * r * cost).round();
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

/// Serial offset between the workbook's date system and the 1900 system the calendar helpers in
/// this module work in. Day counts are unaffected; only calendar lookups need the shift.
fn epoch_shift(sys: DateSystem) -> f64 {
    let at = |s| serial_from_ymd(s, 1904, 1, 1).unwrap_or(0.0);
    at(DateSystem::D1900) - at(sys)
}

/// A date argument as a 1900-system serial.
fn date(c: &dyn Ctx, a: &[Arg], i: usize) -> R<f64> {
    let sys = c.date_system();
    let d = num(c, a, i)?.trunc();
    if datetime_from_serial(sys, d).is_none() {
        return Err(CellError::Num);
    }
    Ok(d + epoch_shift(sys))
}

fn ymd(s: f64) -> R<(i32, u32, u32)> {
    let dt = datetime_from_serial(DateSystem::D1900, s.trunc()).ok_or(CellError::Num)?;
    Ok((dt.year, dt.month, dt.day.max(1)))
}

fn serial(y: i32, m: u32, d: u32) -> R<f64> {
    serial_from_ymd(DateSystem::D1900, y as i64, m as i64, d as i64).ok_or(CellError::Num)
}

fn basis_arg(c: &dyn Ctx, a: &[Arg], i: usize) -> R<u32> {
    let b = opt_num(c, a, i, 0.0)?.trunc();
    if !(0.0..=4.0).contains(&b) {
        return Err(CellError::Num);
    }
    Ok(b as u32)
}

fn freq_arg(c: &dyn Ctx, a: &[Arg], i: usize) -> R<f64> {
    let f = num(c, a, i)?.trunc();
    if f != 1.0 && f != 2.0 && f != 4.0 {
        return Err(CellError::Num);
    }
    Ok(f)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 30/360 day count (US/NASD when `us`, European otherwise).
fn days360(s1: f64, s2: f64, us: bool) -> R<f64> {
    let (y1, m1, mut d1) = ymd(s1)?;
    let (y2, m2, mut d2) = ymd(s2)?;
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
fn day_count(s1: f64, s2: f64, basis: u32) -> R<f64> {
    match basis {
        0 => days360(s1, s2, true),
        4 => days360(s1, s2, false),
        _ => Ok(s2 - s1),
    }
}

/// Year fraction between two dates under a basis (YEARFRAC semantics).
fn year_frac(s1: f64, s2: f64, basis: u32) -> R<f64> {
    let (s1, s2) = if s1 <= s2 { (s1, s2) } else { (s2, s1) };
    match basis {
        0 => Ok(days360(s1, s2, true)? / 360.0),
        2 => Ok((s2 - s1) / 360.0),
        3 => Ok((s2 - s1) / 365.0),
        4 => Ok(days360(s1, s2, false)? / 360.0),
        _ => {
            let (y1, m1, d1) = ymd(s1)?;
            let (y2, m2, d2) = ymd(s2)?;
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
            let start = serial(y1, 1, 1)?;
            let end = serial(y2 + 1, 1, 1).unwrap_or(2_958_466.0);
            let avg = (end - start) / (y2 - y1 + 1) as f64;
            Ok(days / avg)
        }
    }
}

/// Days in a year for discount securities.
fn year_basis(basis: u32, s: f64) -> R<f64> {
    Ok(match basis {
        0 | 2 | 4 => 360.0,
        3 => 365.0,
        _ => {
            if is_leap(ymd(s)?.0) {
                366.0
            } else {
                365.0
            }
        }
    })
}

/// Date `months` months before/after an anchor date, keeping end-of-month alignment.
fn shift_months(anchor: (i32, u32, u32), months: i64, eom: bool) -> R<f64> {
    let (y, m, d) = anchor;
    let total = y as i64 * 12 + (m as i64 - 1) + months;
    let ny = total.div_euclid(12);
    let nm = (total.rem_euclid(12) + 1) as u32;
    if !(1900..=9999).contains(&ny) {
        return Err(CellError::Num);
    }
    let dim = days_in_month(ny as i32, nm);
    let nd = if eom { dim } else { d.min(dim) };
    serial(ny as i32, nm, nd)
}

struct Coupon {
    pcd: f64,
    ncd: f64,
    num: f64,
}

fn coupons(settle: f64, mat: f64, freq: f64) -> R<Coupon> {
    if settle >= mat {
        return Err(CellError::Num);
    }
    let anchor = ymd(mat)?;
    let eom = anchor.2 == days_in_month(anchor.0, anchor.1);
    let step = (12.0 / freq) as i64;
    // Estimate the count from the span, then adjust.
    let mut k = (((mat - settle) / 365.25 * freq).floor() as i64).max(1);
    while k > 1 && shift_months(anchor, -(k - 1) * step, eom)? <= settle {
        k -= 1;
    }
    let mut guard = 0;
    while shift_months(anchor, -k * step, eom)? > settle {
        k += 1;
        guard += 1;
        if guard > 100_000 {
            return Err(CellError::Num);
        }
    }
    Ok(Coupon { pcd: shift_months(anchor, -k * step, eom)?, ncd: shift_months(anchor, -(k - 1) * step, eom)?, num: k as f64 })
}

fn coup_days(c: &Coupon, freq: f64, basis: u32) -> f64 {
    match basis {
        1 => c.ncd - c.pcd,
        3 => 365.0 / freq,
        _ => 360.0 / freq,
    }
}

fn coup_daybs(c: &Coupon, settle: f64, basis: u32) -> R<f64> {
    day_count(c.pcd, settle, basis)
}

fn coup_daysnc(c: &Coupon, settle: f64, freq: f64, basis: u32) -> R<f64> {
    match basis {
        0 | 4 => Ok(coup_days(c, freq, basis) - coup_daybs(c, settle, basis)?),
        _ => Ok(c.ncd - settle),
    }
}

/// settlement, maturity, frequency, basis at positions 0..=3 for the COUP* functions.
fn coup_args(c: &dyn Ctx, a: &[Arg]) -> R<(f64, f64, f64, u32, Coupon)> {
    let (s, m, f, b) = (date(c, a, 0)?, date(c, a, 1)?, freq_arg(c, a, 2)?, basis_arg(c, a, 3)?);
    let c = coupons(s, m, f)?;
    Ok((s, m, f, b, c))
}

fn coupdaybs(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, _, _, b, c) = coup_args(c, a)?;
    num_val(coup_daybs(&c, s, b)?)
}

fn coupdays(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (_, _, f, b, c) = coup_args(c, a)?;
    num_val(coup_days(&c, f, b))
}

fn coupdaysnc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, _, f, b, c) = coup_args(c, a)?;
    num_val(coup_daysnc(&c, s, f, b)?)
}

fn coupncd(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let shift = epoch_shift(c.date_system());
    num_val(coup_args(c, a)?.4.ncd - shift)
}

fn couppcd(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let shift = epoch_shift(c.date_system());
    num_val(coup_args(c, a)?.4.pcd - shift)
}

fn coupnum(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(coup_args(c, a)?.4.num)
}

// ---------------------------------------------------------------------------------------------
// Securities
// ---------------------------------------------------------------------------------------------

fn accrint(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (issue, first, settle, rate) = (date(c, a, 0)?, date(c, a, 1)?, date(c, a, 2)?, num(c, a, 3)?);
    let par = if has(a, 4) { num(c, a, 4)? } else { 1000.0 };
    let freq = freq_arg(c, a, 5)?;
    let basis = basis_arg(c, a, 6)?;
    let calc_method = opt_bool(c, a, 7, true)?;
    if issue >= settle || rate <= 0.0 || par <= 0.0 {
        return Err(CellError::Num);
    }
    let anchor = ymd(first)?;
    let eom = anchor.2 == days_in_month(anchor.0, anchor.1);
    let step = (12.0 / freq) as i64;
    let cd = |j: i64| shift_months(anchor, j * step, eom);
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
            sum += day_count(from, to, basis)? / nl;
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
    let (issue, settle, rate) = (date(c, a, 0)?, date(c, a, 1)?, num(c, a, 2)?);
    let par = if has(a, 3) { num(c, a, 3)? } else { 1000.0 };
    let basis = basis_arg(c, a, 4)?;
    if issue >= settle || rate <= 0.0 || par <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(par * rate * year_frac(issue, settle, basis)?)
}

/// settlement, maturity, value, value, [basis] for discount securities.
fn disc_args(c: &dyn Ctx, a: &[Arg]) -> R<(f64, f64, f64, f64, u32)> {
    let (s, m, x, y, b) = (date(c, a, 0)?, date(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?, basis_arg(c, a, 4)?);
    if s >= m || x <= 0.0 || y <= 0.0 {
        return Err(CellError::Num);
    }
    Ok((s, m, x, y, b))
}

fn disc_frac(s: f64, m: f64, b: u32) -> R<f64> {
    Ok(day_count(s, m, b)? / year_basis(b, s)?)
}

fn disc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, pr, red, b) = disc_args(c, a)?;
    num_val((1.0 - pr / red) / disc_frac(s, m, b)?)
}

fn pricedisc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, d, red, b) = disc_args(c, a)?;
    num_val(red - d * red * disc_frac(s, m, b)?)
}

fn yielddisc(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, pr, red, b) = disc_args(c, a)?;
    num_val((red / pr - 1.0) / disc_frac(s, m, b)?)
}

fn intrate(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, inv, red, b) = disc_args(c, a)?;
    num_val((red - inv) / inv / disc_frac(s, m, b)?)
}

fn received(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, inv, d, b) = disc_args(c, a)?;
    let denom = 1.0 - d * disc_frac(s, m, b)?;
    if denom <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(inv / denom)
}

fn tbill_args(c: &dyn Ctx, a: &[Arg]) -> R<(f64, f64)> {
    let (s, m, x) = (date(c, a, 0)?, date(c, a, 1)?, num(c, a, 2)?);
    if s >= m || x <= 0.0 {
        return Err(CellError::Num);
    }
    // At most one year after settlement.
    let (y, mo, d) = ymd(s)?;
    let limit = shift_months((y, mo, d), 12, false)?;
    if m > limit {
        return Err(CellError::Num);
    }
    Ok((m - s, x))
}

fn tbillprice(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dsm, d) = tbill_args(c, a)?;
    let p = 100.0 * (1.0 - d * dsm / 360.0);
    if p <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(p)
}

fn tbillyield(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dsm, pr) = tbill_args(c, a)?;
    num_val((100.0 - pr) / pr * 360.0 / dsm)
}

fn tbilleq(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dsm, d) = tbill_args(c, a)?;
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
fn mat_terms(c: &dyn Ctx, a: &[Arg]) -> R<(f64, f64, f64, f64, f64)> {
    let (s, m, issue, rate, x, b) = (date(c, a, 0)?, date(c, a, 1)?, date(c, a, 2)?, num(c, a, 3)?, num(c, a, 4)?, basis_arg(c, a, 5)?);
    if s >= m || issue >= s || rate < 0.0 || x < 0.0 {
        return Err(CellError::Num);
    }
    let dim = year_frac(issue, m, b)?;
    let dsm = year_frac(s, m, b)?;
    let ai = year_frac(issue, s, b)?;
    Ok((dim, dsm, ai, rate, x))
}

fn pricemat(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dim, dsm, ai, rate, yld) = mat_terms(c, a)?;
    num_val((100.0 + dim * rate * 100.0) / (1.0 + dsm * yld) - ai * rate * 100.0)
}

fn yieldmat(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (dim, dsm, ai, rate, pr) = mat_terms(c, a)?;
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

fn bond(settle: f64, mat: f64, freq: f64, basis: u32) -> R<Bond> {
    let c = coupons(settle, mat, freq)?;
    let e = coup_days(&c, freq, basis);
    let a = coup_daybs(&c, settle, basis)?;
    let dsc = coup_daysnc(&c, settle, freq, basis)?;
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
    let (s, m, rate, yld, red) = (date(c, a, 0)?, date(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?, num(c, a, 4)?);
    let (f, basis) = (freq_arg(c, a, 5)?, basis_arg(c, a, 6)?);
    if rate < 0.0 || yld < 0.0 || red <= 0.0 {
        return Err(CellError::Num);
    }
    let b = bond(s, m, f, basis)?;
    num_val(bond_price(&b, rate, yld, red))
}

fn yield_(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (s, m, rate, pr, red) = (date(c, a, 0)?, date(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?, num(c, a, 4)?);
    let (f, basis) = (freq_arg(c, a, 5)?, basis_arg(c, a, 6)?);
    if rate < 0.0 || pr <= 0.0 || red <= 0.0 {
        return Err(CellError::Num);
    }
    let b = bond(s, m, f, basis)?;
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

fn duration_calc(c: &dyn Ctx, a: &[Arg]) -> R<(f64, f64, f64)> {
    let (s, m, coupon, yld) = (date(c, a, 0)?, date(c, a, 1)?, num(c, a, 2)?, num(c, a, 3)?);
    let (f, basis) = (freq_arg(c, a, 4)?, basis_arg(c, a, 5)?);
    if coupon < 0.0 || yld < 0.0 {
        return Err(CellError::Num);
    }
    let b = bond(s, m, f, basis)?;
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
    num_val(duration_calc(c, a)?.0)
}

fn mduration(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let (d, y, f) = duration_calc(c, a)?;
    num_val(d / (1.0 + y / f))
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
    fn dates_follow_the_workbook_date_system() {
        let d04 = |y, m, dd| n(serial_from_ymd(DateSystem::D1904, y, m, dd).unwrap());
        // Text dates and numeric serials agree in a 1904 workbook.
        let want = 1000.0 * 0.1 * 28.0 / 360.0;
        close_tol(ev_1904("ACCRINTM", vec![t("2026-01-31"), t("2026-02-28"), n(0.1), n(1000.0), n(0.0)]), want, 1e-12);
        close_tol(ev_1904("ACCRINTM", vec![d04(2026, 1, 31), d04(2026, 2, 28), n(0.1), n(1000.0), n(0.0)]), want, 1e-12);
        close_tol(ev("ACCRINTM", vec![t("2026-01-31"), t("2026-02-28"), n(0.1), n(1000.0), n(0.0)]), want, 1e-12);
        // Actual/actual uses the real calendar year of the serial (2024 is a leap year).
        let leap = 1000.0 * 0.1 * 182.0 / 366.0;
        close_tol(ev_1904("ACCRINTM", vec![d04(2024, 1, 1), d04(2024, 7, 1), n(0.1), n(1000.0), n(1.0)]), leap, 1e-12);
        // Coupon dates come back as serials of the same system.
        let pcd = ev_1904("COUPPCD", vec![d04(2011, 3, 15), d04(2011, 11, 30), n(2.0)]);
        close(pcd, serial_from_ymd(DateSystem::D1904, 2010, 11, 30).unwrap());
        let ncd = ev_1904("COUPNCD", vec![t("2011-03-15"), t("2011-11-30"), n(2.0)]);
        close(ncd, serial_from_ymd(DateSystem::D1904, 2011, 5, 31).unwrap());
        close(ev_1904("COUPNUM", vec![d04(2007, 1, 25), d04(2008, 11, 15), n(2.0), n(1.0)]), 4.0);
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

    #[test]
    fn lifting() {
        let v = ev("SLN", vec![av(row(&[100.0, 200.0])), n(0.0), n(10.0)]);
        assert_eq!(rows_of(&v), vec![vec![nv(10.0), nv(20.0)]]);
        is_err(ev("PMT", vec![t("abc"), n(1.0), n(1.0)]), CellError::Value);
        is_err(ev("PMT", vec![e(CellError::NA), n(1.0), n(1.0)]), CellError::NA);
    }
}
