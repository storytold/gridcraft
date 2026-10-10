//! Statistical functions: aggregates, order statistics, regression and probability
//! distributions (with their inverses).

use gridcraft_core::{CellError, Value};

use crate::special::{erf, gamma, invert, ln_beta, ln_gamma, norm_cdf, norm_inv, norm_pdf, reg_gamma_p, reg_gamma_q, reg_inc_beta};
use crate::util::{A, R, S, arg, array_numbers, array_val, as_array, boolean, has, num, num_val, numbers, numbers_a, opt_num};
use crate::{Arg, Ctx, FnSpec, VAR};

type C<'a> = &'a mut dyn Ctx;

// ---------------------------------------------------------------------------------------------
// Basic helpers over number lists
// ---------------------------------------------------------------------------------------------

fn mean(x: &[f64]) -> R<f64> {
    if x.is_empty() {
        return Err(CellError::Div0);
    }
    Ok(x.iter().sum::<f64>() / x.len() as f64)
}

fn devsq(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    let m = x.iter().sum::<f64>() / x.len() as f64;
    x.iter().map(|v| (v - m) * (v - m)).sum()
}

fn var_s(x: &[f64]) -> R<f64> {
    if x.len() < 2 {
        return Err(CellError::Div0);
    }
    Ok(devsq(x) / (x.len() - 1) as f64)
}

fn var_p(x: &[f64]) -> R<f64> {
    if x.is_empty() {
        return Err(CellError::Div0);
    }
    Ok(devsq(x) / x.len() as f64)
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.total_cmp(b));
    v
}

fn median(v: Vec<f64>) -> R<f64> {
    if v.is_empty() {
        return Err(CellError::Num);
    }
    let s = sorted(v);
    let n = s.len();
    let hi = s.get(n / 2).copied().unwrap_or(0.0);
    if n % 2 == 1 { Ok(hi) } else { Ok((s.get(n / 2 - 1).copied().unwrap_or(0.0) + hi) / 2.0) }
}

/// Values occurring at least twice, ordered by first occurrence, with their (equal, maximal) count.
fn modes(v: &[f64]) -> Vec<f64> {
    let mut counts: Vec<(f64, usize)> = Vec::new();
    for &x in v {
        if let Some(e) = counts.iter_mut().find(|e| e.0 == x) {
            e.1 += 1;
        } else {
            counts.push((x, 1));
        }
    }
    let best = counts.iter().map(|e| e.1).max().unwrap_or(0);
    if best < 2 {
        return Vec::new();
    }
    counts.into_iter().filter(|e| e.1 == best).map(|e| e.0).collect()
}

fn mode_sngl(v: &[f64]) -> R<f64> {
    modes(v).first().copied().ok_or(CellError::NA)
}

fn product(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().product() }
}

fn min_of(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().copied().fold(f64::INFINITY, f64::min) }
}

fn max_of(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().copied().fold(f64::NEG_INFINITY, f64::max) }
}

/// k-th largest (k is rounded up like Excel).
fn kth(v: Vec<f64>, k: f64, largest: bool) -> R<f64> {
    let k = k.ceil();
    if k.is_nan() || k < 1.0 || k > v.len() as f64 {
        return Err(CellError::Num);
    }
    let s = sorted(v);
    let k = k as usize;
    let idx = if largest { s.len() - k } else { k - 1 };
    s.get(idx).copied().ok_or(CellError::Num)
}

fn interp(s: &[f64], pos: f64) -> R<f64> {
    let i = pos.floor();
    let frac = pos - i;
    let i = i as usize;
    let lo = *s.get(i).ok_or(CellError::Num)?;
    match s.get(i + 1) {
        Some(hi) if frac > 0.0 => Ok(lo + frac * (hi - lo)),
        _ => Ok(lo),
    }
}

fn percentile_inc(v: Vec<f64>, k: f64) -> R<f64> {
    if v.is_empty() || !(0.0..=1.0).contains(&k) {
        return Err(CellError::Num);
    }
    let s = sorted(v);
    interp(&s, k * (s.len() - 1) as f64)
}

fn percentile_exc(v: Vec<f64>, k: f64) -> R<f64> {
    let n = v.len() as f64;
    if v.is_empty() || !(k > 0.0 && k < 1.0) {
        return Err(CellError::Num);
    }
    let pos = k * (n + 1.0);
    if pos < 1.0 || pos > n {
        return Err(CellError::Num);
    }
    let s = sorted(v);
    interp(&s, pos - 1.0)
}

fn quartile_inc(v: Vec<f64>, q: f64) -> R<f64> {
    let q = q.trunc();
    if !(0.0..=4.0).contains(&q) {
        return Err(CellError::Num);
    }
    percentile_inc(v, q / 4.0)
}

fn quartile_exc(v: Vec<f64>, q: f64) -> R<f64> {
    let q = q.trunc();
    if !(1.0..=3.0).contains(&q) {
        return Err(CellError::Num);
    }
    percentile_exc(v, q / 4.0)
}

fn arg_nums(a: &[Arg], i: usize) -> R<Vec<f64>> {
    array_numbers(&arg(a, i)?.value)
}

/// Pairs of numbers at matching positions of two arrays (pairs with a non-number skipped).
/// Different sizes give `#N/A`; errors propagate.
fn pairs(x: &Value, y: &Value) -> R<(Vec<f64>, Vec<f64>)> {
    let xa = as_array(x);
    let ya = as_array(y);
    if xa.data.len() != ya.data.len() {
        return Err(CellError::NA);
    }
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for (a, b) in xa.data.iter().zip(ya.data.iter()) {
        if let Value::Error(e) = a {
            return Err(*e);
        }
        if let Value::Error(e) = b {
            return Err(*e);
        }
        if let (Value::Number(p), Value::Number(q)) = (a, b) {
            xs.push(*p);
            ys.push(*q);
        }
    }
    Ok((xs, ys))
}

fn arg_pairs(a: &[Arg], i: usize, j: usize) -> R<(Vec<f64>, Vec<f64>)> {
    pairs(&arg(a, i)?.value, &arg(a, j)?.value)
}

/// (sum of cross deviations, devsq x, devsq y, mean x, mean y).
fn moments(x: &[f64], y: &[f64]) -> R<(f64, f64, f64, f64, f64)> {
    let mx = mean(x)?;
    let my = mean(y)?;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx) * (a - mx);
        syy += (b - my) * (b - my);
    }
    Ok((sxy, sxx, syy, mx, my))
}

fn correl(x: &[f64], y: &[f64]) -> R<f64> {
    if x.len() < 2 {
        return Err(CellError::Div0);
    }
    let (sxy, sxx, syy, _, _) = moments(x, y)?;
    if sxx == 0.0 || syy == 0.0 {
        return Err(CellError::Div0);
    }
    Ok(sxy / (sxx * syy).sqrt())
}

fn slope(y: &[f64], x: &[f64]) -> R<f64> {
    let (sxy, sxx, _, _, _) = moments(x, y)?;
    if sxx == 0.0 {
        return Err(CellError::Div0);
    }
    Ok(sxy / sxx)
}

fn intercept(y: &[f64], x: &[f64]) -> R<f64> {
    let (sxy, sxx, _, mx, my) = moments(x, y)?;
    if sxx == 0.0 {
        return Err(CellError::Div0);
    }
    Ok(my - sxy / sxx * mx)
}

fn trunc_digits(r: f64, sig: f64) -> f64 {
    let f = 10f64.powi(sig as i32);
    ((r * f) + 1e-9).floor() / f
}

fn col_value(v: Vec<f64>) -> R<Value> {
    let n = v.len();
    array_val(n, 1, v.into_iter().map(Value::Number).collect())
}

// ---------------------------------------------------------------------------------------------
// Aggregates
// ---------------------------------------------------------------------------------------------

fn count_impl(a: &[Arg]) -> f64 {
    let mut c = 0usize;
    for x in a {
        match &x.value {
            Value::Array(arr) => c += arr.iter().filter(|v| v.is_number()).count(),
            Value::Number(_) => c += 1,
            _ if x.from_ref => {}
            Value::Bool(_) => c += 1,
            Value::Text(t) if gridcraft_core::parse::parse_number_text(t).is_some() => c += 1,
            _ => {}
        }
    }
    c as f64
}

fn counta_impl(a: &[Arg]) -> f64 {
    let mut c = 0usize;
    for x in a {
        match &x.value {
            Value::Array(arr) => c += arr.iter().filter(|v| !v.is_empty()).count(),
            Value::Empty if x.from_ref => {}
            _ => c += 1,
        }
    }
    c as f64
}

fn countblank(a: &[Arg], _: C) -> R<Value> {
    let v = &arg(a, 0)?.value;
    let blank = |x: &Value| matches!(x, Value::Empty) || matches!(x, Value::Text(t) if t.is_empty());
    let c = match v {
        Value::Array(arr) => arr.iter().filter(|x| blank(x)).count(),
        x => usize::from(blank(x)),
    };
    num_val(c as f64)
}

fn mode_mult(a: &[Arg], _: C) -> R<Value> {
    let v = numbers(a)?;
    let m = modes(&v);
    if m.is_empty() {
        return Err(CellError::NA);
    }
    col_value(m)
}

fn large(a: &[Arg], _: C) -> R<Value> {
    num_val(kth(arg_nums(a, 0)?, num(a, 1)?, true)?)
}

fn small(a: &[Arg], _: C) -> R<Value> {
    num_val(kth(arg_nums(a, 0)?, num(a, 1)?, false)?)
}

fn rank_impl(a: &[Arg], avg: bool) -> R<Value> {
    let x = num(a, 0)?;
    let v = arg_nums(a, 1)?;
    let asc = opt_num(a, 2, 0.0)? != 0.0;
    let eq = v.iter().filter(|&&y| y == x).count();
    if eq == 0 {
        return Err(CellError::NA);
    }
    let before = v.iter().filter(|&&y| if asc { y < x } else { y > x }).count();
    let r = before as f64 + 1.0;
    num_val(if avg { r + (eq as f64 - 1.0) / 2.0 } else { r })
}

fn percentrank_impl(a: &[Arg], exc: bool) -> R<Value> {
    let s = sorted(arg_nums(a, 0)?);
    let x = num(a, 1)?;
    let sig = opt_num(a, 2, 3.0)?.trunc();
    if sig < 1.0 || s.is_empty() {
        return Err(CellError::Num);
    }
    let (first, last) = (s.first().copied().unwrap_or(0.0), s.last().copied().unwrap_or(0.0));
    if x < first || x > last {
        return Err(CellError::NA);
    }
    let n = s.len() as f64;
    let rank_of = |v: f64| -> f64 {
        let less = s.iter().filter(|&&y| y < v).count() as f64;
        if exc {
            (less + 1.0) / (n + 1.0)
        } else if n <= 1.0 {
            1.0
        } else {
            less / (n - 1.0)
        }
    };
    let r = if s.contains(&x) {
        rank_of(x)
    } else {
        let lo = s.iter().copied().filter(|&y| y < x).fold(f64::NEG_INFINITY, f64::max);
        let hi = s.iter().copied().filter(|&y| y > x).fold(f64::INFINITY, f64::min);
        let (rl, rh) = (rank_of(lo), rank_of(hi));
        rl + (x - lo) / (hi - lo) * (rh - rl)
    };
    num_val(trunc_digits(r, sig))
}

fn trimmean(a: &[Arg], _: C) -> R<Value> {
    let v = arg_nums(a, 0)?;
    let p = num(a, 1)?;
    if !(0.0..1.0).contains(&p) || v.is_empty() {
        return Err(CellError::Num);
    }
    let n = v.len();
    let per_side = ((n as f64 * p) / 2.0).floor() as usize;
    let s = sorted(v);
    let kept = s.get(per_side..n - per_side).unwrap_or(&[]);
    num_val(mean(kept)?)
}

fn kurt(v: &[f64]) -> R<f64> {
    let n = v.len() as f64;
    if v.len() < 4 {
        return Err(CellError::Div0);
    }
    let m = mean(v)?;
    let s = var_s(v)?.sqrt();
    if s == 0.0 {
        return Err(CellError::Div0);
    }
    let sum4: f64 = v.iter().map(|x| ((x - m) / s).powi(4)).sum();
    Ok(n * (n + 1.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0)) * sum4 - 3.0 * (n - 1.0).powi(2) / ((n - 2.0) * (n - 3.0)))
}

fn skew(v: &[f64]) -> R<f64> {
    let n = v.len() as f64;
    if v.len() < 3 {
        return Err(CellError::Div0);
    }
    let m = mean(v)?;
    let s = var_s(v)?.sqrt();
    if s == 0.0 {
        return Err(CellError::Div0);
    }
    let sum3: f64 = v.iter().map(|x| ((x - m) / s).powi(3)).sum();
    Ok(n / ((n - 1.0) * (n - 2.0)) * sum3)
}

fn skew_p(v: &[f64]) -> R<f64> {
    let m = mean(v)?;
    let s = var_p(v)?.sqrt();
    if s == 0.0 {
        return Err(CellError::Div0);
    }
    Ok(v.iter().map(|x| ((x - m) / s).powi(3)).sum::<f64>() / v.len() as f64)
}

fn geomean(v: &[f64]) -> R<f64> {
    if v.is_empty() || v.iter().any(|&x| x <= 0.0) {
        return Err(CellError::Num);
    }
    Ok((v.iter().map(|x| x.ln()).sum::<f64>() / v.len() as f64).exp())
}

fn harmean(v: &[f64]) -> R<f64> {
    if v.is_empty() || v.iter().any(|&x| x <= 0.0) {
        return Err(CellError::Num);
    }
    Ok(v.len() as f64 / v.iter().map(|x| 1.0 / x).sum::<f64>())
}

fn avedev(v: &[f64]) -> R<f64> {
    if v.is_empty() {
        return Err(CellError::Num);
    }
    let m = mean(v)?;
    Ok(v.iter().map(|x| (x - m).abs()).sum::<f64>() / v.len() as f64)
}

fn frequency(a: &[Arg], _: C) -> R<Value> {
    let data = arg_nums(a, 0)?;
    let bins = arg_nums(a, 1)?;
    if bins.is_empty() {
        return col_value(vec![data.len() as f64]);
    }
    let mut order: Vec<usize> = (0..bins.len()).collect();
    order.sort_by(|&i, &j| bins.get(i).copied().unwrap_or(0.0).total_cmp(&bins.get(j).copied().unwrap_or(0.0)));
    let mut counts = vec![0.0; bins.len() + 1];
    for x in data {
        let slot = order.iter().find(|&&i| x <= bins.get(i).copied().unwrap_or(f64::INFINITY)).copied().unwrap_or(bins.len());
        if let Some(c) = counts.get_mut(slot) {
            *c += 1.0;
        }
    }
    col_value(counts)
}

fn prob(a: &[Arg], _: C) -> R<Value> {
    let (xs, ps) = arg_pairs(a, 0, 1)?;
    let lo = num(a, 2)?;
    let hi = if has(a, 3) { num(a, 3)? } else { lo };
    if ps.iter().any(|&p| !(0.0..=1.0).contains(&p)) {
        return Err(CellError::Num);
    }
    let total: f64 = ps.iter().sum();
    if (total - 1.0).abs() > 1e-7 {
        return Err(CellError::Num);
    }
    num_val(xs.iter().zip(&ps).filter(|(x, _)| **x >= lo && **x <= hi).map(|(_, p)| p).sum())
}

fn standardize(a: &[Arg], _: C) -> R<Value> {
    let (x, m, sd) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    if sd <= 0.0 {
        return Err(CellError::Num);
    }
    num_val((x - m) / sd)
}

fn forecast(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let (ys, xs) = arg_pairs(a, 1, 2)?;
    if ys.is_empty() {
        return Err(CellError::Div0);
    }
    num_val(intercept(&ys, &xs)? + slope(&ys, &xs)? * x)
}

fn steyx(a: &[Arg], _: C) -> R<Value> {
    let (ys, xs) = arg_pairs(a, 0, 1)?;
    let n = ys.len() as f64;
    if ys.len() < 3 {
        return Err(CellError::Div0);
    }
    let (sxy, sxx, syy, _, _) = moments(&xs, &ys)?;
    if sxx == 0.0 {
        return Err(CellError::Div0);
    }
    num_val(((syy - sxy * sxy / sxx) / (n - 2.0)).max(0.0).sqrt())
}

fn covariance(a: &[Arg], sample: bool) -> R<Value> {
    let (xs, ys) = arg_pairs(a, 0, 1)?;
    let n = xs.len();
    if n == 0 || (sample && n < 2) {
        return Err(CellError::Div0);
    }
    let (sxy, ..) = moments(&xs, &ys)?;
    num_val(sxy / if sample { (n - 1) as f64 } else { n as f64 })
}

// ---------------------------------------------------------------------------------------------
// Regression (LINEST, LOGEST, TREND, GROWTH)
// ---------------------------------------------------------------------------------------------

struct Xy {
    y: Vec<f64>,
    /// n rows of k predictor values.
    x: Vec<Vec<f64>>,
    k: usize,
    y_is_row: bool,
    y_rows: usize,
    y_cols: usize,
}

fn strict_num(v: &Value) -> R<f64> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Error(e) => Err(*e),
        _ => Err(CellError::Value),
    }
}

fn parse_xy(y: &Value, x: Option<&Value>) -> R<Xy> {
    let ya = as_array(y);
    let n = ya.data.len();
    let y_is_row = ya.rows == 1 && ya.cols > 1;
    let yv: Vec<f64> = ya.data.iter().map(strict_num).collect::<R<_>>()?;
    let (xs, k) = match x {
        None => ((1..=n).map(|i| vec![i as f64]).collect(), 1),
        Some(xv) => {
            let xa = as_array(xv);
            if xa.rows == ya.rows && xa.cols == ya.cols {
                (xa.data.iter().map(|v| strict_num(v).map(|f| vec![f])).collect::<R<Vec<_>>>()?, 1)
            } else if !y_is_row && xa.rows == n {
                let mut rows = Vec::with_capacity(n);
                for r in 0..n {
                    rows.push((0..xa.cols).map(|c| strict_num(xa.get(r, c).unwrap_or(&Value::Empty))).collect::<R<Vec<_>>>()?);
                }
                (rows, xa.cols)
            } else if y_is_row && xa.cols == n {
                let mut rows = Vec::with_capacity(n);
                for c in 0..n {
                    rows.push((0..xa.rows).map(|r| strict_num(xa.get(r, c).unwrap_or(&Value::Empty))).collect::<R<Vec<_>>>()?);
                }
                (rows, xa.rows)
            } else {
                return Err(CellError::Ref);
            }
        }
    };
    Ok(Xy { y: yv, x: xs, k, y_is_row, y_rows: ya.rows, y_cols: ya.cols })
}

struct Fit {
    coef: Vec<f64>,
    b: f64,
    se: Vec<f64>,
    se_b: Option<f64>,
    r2: R<f64>,
    sey: R<f64>,
    f: R<f64>,
    df: f64,
    ssreg: f64,
    ssresid: f64,
}

/// Inverts a symmetric positive semi-definite matrix by Gauss–Jordan on the diagonal, dropping
/// (zeroing) collinear variables. Returns the inverse and the dropped flags.
fn sym_inverse(m: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<bool>) {
    let p = m.len();
    let mut a: Vec<Vec<f64>> = m.to_vec();
    let mut inv: Vec<Vec<f64>> = (0..p).map(|i| (0..p).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    let mut dropped = vec![false; p];
    let diag: Vec<f64> = (0..p).map(|i| m.get(i).and_then(|r| r.get(i)).copied().unwrap_or(0.0).abs()).collect();
    for j in 0..p {
        let piv = a.get(j).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
        let scale = diag.get(j).copied().unwrap_or(0.0);
        if piv.abs() <= 1e-12 * scale || piv.abs() < 1e-300 {
            if let Some(d) = dropped.get_mut(j) {
                *d = true;
            }
            for row in a.iter_mut() {
                if let Some(v) = row.get_mut(j) {
                    *v = 0.0;
                }
            }
            if let Some(r) = a.get_mut(j) {
                r.iter_mut().for_each(|v| *v = 0.0);
            }
            if let Some(r) = inv.get_mut(j) {
                r.iter_mut().for_each(|v| *v = 0.0);
            }
            continue;
        }
        let arow: Vec<f64> = a.get(j).map(|r| r.iter().map(|v| v / piv).collect()).unwrap_or_default();
        let irow: Vec<f64> = inv.get(j).map(|r| r.iter().map(|v| v / piv).collect()).unwrap_or_default();
        for i in 0..p {
            if i == j {
                continue;
            }
            let f = a.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
            if f == 0.0 {
                continue;
            }
            if let Some(r) = a.get_mut(i) {
                r.iter_mut().zip(&arow).for_each(|(v, w)| *v -= f * w);
            }
            if let Some(r) = inv.get_mut(i) {
                r.iter_mut().zip(&irow).for_each(|(v, w)| *v -= f * w);
            }
        }
        if let Some(r) = a.get_mut(j) {
            *r = arow;
        }
        if let Some(r) = inv.get_mut(j) {
            *r = irow;
        }
    }
    (inv, dropped)
}

fn regress(y: &[f64], x: &[Vec<f64>], k: usize, constant: bool) -> R<Fit> {
    let n = y.len();
    if n == 0 || x.len() != n {
        return Err(CellError::Value);
    }
    let nf = n as f64;
    let (xm, ym) = if constant {
        let xm: Vec<f64> = (0..k).map(|j| x.iter().map(|r| r.get(j).copied().unwrap_or(0.0)).sum::<f64>() / nf).collect();
        (xm, y.iter().sum::<f64>() / nf)
    } else {
        (vec![0.0; k], 0.0)
    };
    // Normal equations on (centred) data.
    let mut xtx = vec![vec![0.0; k]; k];
    let mut xty = vec![0.0; k];
    for (row, yv) in x.iter().zip(y) {
        let d: Vec<f64> = (0..k).map(|j| row.get(j).copied().unwrap_or(0.0) - xm.get(j).copied().unwrap_or(0.0)).collect();
        let yd = yv - ym;
        for (i, di) in d.iter().enumerate() {
            if let Some(t) = xty.get_mut(i) {
                *t += di * yd;
            }
            if let Some(r) = xtx.get_mut(i) {
                for (j, dj) in d.iter().enumerate() {
                    if let Some(v) = r.get_mut(j) {
                        *v += di * dj;
                    }
                }
            }
        }
    }
    let (inv, dropped) = sym_inverse(&xtx);
    let coef: Vec<f64> = (0..k).map(|i| inv.get(i).map(|r| r.iter().zip(&xty).map(|(a, b)| a * b).sum()).unwrap_or(0.0)).collect();
    let b = if constant { ym - coef.iter().zip(&xm).map(|(c, m)| c * m).sum::<f64>() } else { 0.0 };
    let mut ssresid = 0.0;
    let mut sstot = 0.0;
    for (row, yv) in x.iter().zip(y) {
        let pred = b + coef.iter().zip(row).map(|(c, v)| c * v).sum::<f64>();
        ssresid += (yv - pred).powi(2);
        sstot += if constant { (yv - ym).powi(2) } else { yv * yv };
    }
    let used = dropped.iter().filter(|d| !**d).count() + usize::from(constant);
    let df = nf - used as f64;
    let ssreg = sstot - ssresid;
    let r2 = if sstot == 0.0 { Err(CellError::Num) } else { Ok(ssreg / sstot) };
    let sey2 = if df > 0.0 { Ok(ssresid / df) } else { Err(CellError::Num) };
    let sey = sey2.map(f64::sqrt);
    let v1 = if constant { nf - df - 1.0 } else { nf - df };
    let f = match sey2 {
        Ok(s2) if s2 > 0.0 && v1 > 0.0 => Ok((ssreg / v1) / s2),
        _ => Err(CellError::Num),
    };
    let s2 = sey2.unwrap_or(f64::NAN);
    let se: Vec<f64> = (0..k)
        .map(|i| if dropped.get(i).copied().unwrap_or(true) { 0.0 } else { (inv.get(i).and_then(|r| r.get(i)).copied().unwrap_or(0.0) * s2).sqrt() })
        .collect();
    let se_b = if constant {
        let mut q = 0.0;
        for (i, mi) in xm.iter().enumerate() {
            for (j, mj) in xm.iter().enumerate() {
                q += mi * mj * inv.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
            }
        }
        Some((s2 * (1.0 / nf + q)).sqrt())
    } else {
        None
    };
    Ok(Fit { coef, b, se, se_b, r2, sey, f, df, ssreg, ssresid })
}

fn const_arg(a: &[Arg], i: usize) -> R<bool> {
    if has(a, i) { boolean(a, i) } else { Ok(true) }
}

fn opt_value(a: &[Arg], i: usize) -> Option<&Value> {
    if has(a, i) { a.get(i).map(|x| &x.value) } else { None }
}

fn linest_impl(a: &[Arg], log: bool) -> R<Value> {
    let mut xy = parse_xy(&arg(a, 0)?.value, opt_value(a, 1))?;
    if log {
        if xy.y.iter().any(|&v| v <= 0.0) {
            return Err(CellError::Num);
        }
        xy.y.iter_mut().for_each(|v| *v = v.ln());
    }
    let constant = const_arg(a, 2)?;
    let stats = if has(a, 3) { boolean(a, 3)? } else { false };
    let fit = regress(&xy.y, &xy.x, xy.k, constant)?;
    let tr = |v: f64| if log { v.exp() } else { v };
    let cols = xy.k + 1;
    let mut data: Vec<Value> = Vec::new();
    for c in fit.coef.iter().rev() {
        data.push(Value::number(tr(*c)));
    }
    data.push(Value::number(tr(fit.b)));
    if stats {
        let rv = |r: R<f64>| r.map(Value::number).unwrap_or_else(Value::Error);
        let na = Value::Error(CellError::NA);
        for s in fit.se.iter().rev() {
            data.push(Value::number(*s));
        }
        data.push(fit.se_b.map(Value::number).unwrap_or(na.clone()));
        let rows: [[Value; 2]; 3] =
            [[rv(fit.r2), rv(fit.sey)], [rv(fit.f), Value::number(fit.df)], [Value::number(fit.ssreg), Value::number(fit.ssresid)]];
        for [p, q] in rows {
            data.push(p);
            if cols > 1 {
                data.push(q);
            }
            for _ in 2..cols {
                data.push(na.clone());
            }
        }
        return array_val(5, cols, data);
    }
    array_val(1, cols, data)
}

fn trend_impl(a: &[Arg], log: bool) -> R<Value> {
    let mut xy = parse_xy(&arg(a, 0)?.value, opt_value(a, 1))?;
    if log {
        if xy.y.iter().any(|&v| v <= 0.0) {
            return Err(CellError::Num);
        }
        xy.y.iter_mut().for_each(|v| *v = v.ln());
    }
    let constant = const_arg(a, 3)?;
    let fit = regress(&xy.y, &xy.x, xy.k, constant)?;
    let predict = |row: &[f64]| -> Value {
        let v = fit.b + fit.coef.iter().zip(row).map(|(c, x)| c * x).sum::<f64>();
        Value::number(if log { v.exp() } else { v })
    };
    match opt_value(a, 2) {
        None => {
            let data: Vec<Value> = xy.x.iter().map(|r| predict(r)).collect();
            array_val(xy.y_rows, xy.y_cols, data)
        }
        Some(nv) => {
            let na = as_array(nv);
            if xy.k == 1 {
                let data = na.data.iter().map(|v| strict_num(v).map(|x| predict(&[x]))).collect::<R<Vec<_>>>()?;
                array_val(na.rows, na.cols, data)
            } else if !xy.y_is_row && na.cols == xy.k {
                let mut data = Vec::with_capacity(na.rows);
                for r in 0..na.rows {
                    let row = (0..na.cols).map(|c| strict_num(na.get(r, c).unwrap_or(&Value::Empty))).collect::<R<Vec<_>>>()?;
                    data.push(predict(&row));
                }
                array_val(na.rows, 1, data)
            } else if xy.y_is_row && na.rows == xy.k {
                let mut data = Vec::with_capacity(na.cols);
                for c in 0..na.cols {
                    let row = (0..na.rows).map(|r| strict_num(na.get(r, c).unwrap_or(&Value::Empty))).collect::<R<Vec<_>>>()?;
                    data.push(predict(&row));
                }
                array_val(1, na.cols, data)
            } else {
                Err(CellError::Ref)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Distributions
// ---------------------------------------------------------------------------------------------

fn ln_comb(n: f64, k: f64) -> f64 {
    ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0)
}

fn t_upper(t: f64, v: f64) -> f64 {
    // P(T > t) for t ≥ 0.
    0.5 * reg_inc_beta(v / (v + t * t), v / 2.0, 0.5)
}

fn t_cdf(t: f64, v: f64) -> f64 {
    if t >= 0.0 { 1.0 - t_upper(t, v) } else { t_upper(-t, v) }
}

fn t_pdf(t: f64, v: f64) -> f64 {
    (ln_gamma((v + 1.0) / 2.0) - ln_gamma(v / 2.0) - 0.5 * (v * std::f64::consts::PI).ln() - (v + 1.0) / 2.0 * (1.0 + t * t / v).ln()).exp()
}

/// t ≥ 0 with P(T > t) = q, q in (0, 0.5].
fn t_inv_upper(q: f64, v: f64) -> f64 {
    if q >= 0.5 {
        return 0.0;
    }
    invert(|t| t_upper(t, v), q, 0.0, 1.0, 1e60)
}

fn chi_inv(p: f64, v: f64) -> f64 {
    if p <= 0.0 {
        return 0.0;
    }
    invert(|x| reg_gamma_p(v / 2.0, x / 2.0), p, 0.0, v.max(1.0), 1e60)
}

fn chi_inv_rt(q: f64, v: f64) -> f64 {
    if q >= 1.0 {
        return 0.0;
    }
    invert(|x| reg_gamma_q(v / 2.0, x / 2.0), q, 0.0, v.max(1.0), 1e60)
}

fn f_cdf(x: f64, d1: f64, d2: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    reg_inc_beta(d1 * x / (d1 * x + d2), d1 / 2.0, d2 / 2.0)
}

fn f_upper(x: f64, d1: f64, d2: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    reg_inc_beta(d2 / (d2 + d1 * x), d2 / 2.0, d1 / 2.0)
}

fn f_pdf(x: f64, d1: f64, d2: f64) -> f64 {
    if x == 0.0 {
        return if d1 < 2.0 {
            f64::INFINITY
        } else if d1 == 2.0 {
            1.0
        } else {
            0.0
        };
    }
    ((d1 / 2.0) * (d1 / d2).ln() + (d1 / 2.0 - 1.0) * x.ln() - ((d1 + d2) / 2.0) * (1.0 + d1 * x / d2).ln() - ln_beta(d1 / 2.0, d2 / 2.0)).exp()
}

fn binom_pmf(k: f64, n: f64, p: f64) -> f64 {
    if k < 0.0 || k > n {
        return 0.0;
    }
    if p == 0.0 {
        return if k == 0.0 { 1.0 } else { 0.0 };
    }
    if p == 1.0 {
        return if k == n { 1.0 } else { 0.0 };
    }
    (ln_comb(n, k) + k * p.ln() + (n - k) * (1.0 - p).ln()).exp()
}

fn binom_cdf(k: f64, n: f64, p: f64) -> f64 {
    if k < 0.0 {
        return 0.0;
    }
    if k >= n {
        return 1.0;
    }
    if n <= 10_000.0 {
        let mut s = 0.0;
        let mut i = 0.0;
        while i <= k {
            s += binom_pmf(i, n, p);
            i += 1.0;
        }
        return s.min(1.0);
    }
    if p == 0.0 {
        return 1.0;
    }
    if p == 1.0 {
        return 0.0;
    }
    reg_inc_beta(1.0 - p, n - k, k + 1.0)
}

fn poisson_pmf(x: f64, m: f64) -> f64 {
    if m == 0.0 {
        return if x == 0.0 { 1.0 } else { 0.0 };
    }
    (-m + x * m.ln() - ln_gamma(x + 1.0)).exp()
}

fn poisson_cdf(x: f64, m: f64) -> f64 {
    if m == 0.0 {
        return 1.0;
    }
    if x <= 1000.0 {
        let mut s = 0.0;
        let mut i = 0.0;
        while i <= x {
            s += poisson_pmf(i, m);
            i += 1.0;
        }
        return s.min(1.0);
    }
    reg_gamma_q(x + 1.0, m)
}

fn norm_dist(a: &[Arg], _: C) -> R<Value> {
    let (x, m, sd, cum) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, boolean(a, 3)?);
    if sd <= 0.0 {
        return Err(CellError::Num);
    }
    let z = (x - m) / sd;
    num_val(if cum { norm_cdf(z) } else { norm_pdf(z) / sd })
}

fn norm_inv_fn(a: &[Arg], _: C) -> R<Value> {
    let (p, m, sd) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    if !(p > 0.0 && p < 1.0) || sd <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(m + sd * norm_inv(p))
}

fn norm_s_dist(a: &[Arg], _: C) -> R<Value> {
    let z = num(a, 0)?;
    let cum = if a.len() > 1 { boolean(a, 1)? } else { true };
    num_val(if cum { norm_cdf(z) } else { norm_pdf(z) })
}

fn norm_s_inv(a: &[Arg], _: C) -> R<Value> {
    let p = num(a, 0)?;
    if !(p > 0.0 && p < 1.0) {
        return Err(CellError::Num);
    }
    num_val(norm_inv(p))
}

fn df_arg(a: &[Arg], i: usize) -> R<f64> {
    let v = num(a, i)?.trunc();
    if v < 1.0 {
        return Err(CellError::Num);
    }
    Ok(v)
}

fn t_dist(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let v = df_arg(a, 1)?;
    let cum = boolean(a, 2)?;
    num_val(if cum { t_cdf(x, v) } else { t_pdf(x, v) })
}

fn t_dist_2t(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let v = df_arg(a, 1)?;
    if x < 0.0 {
        return Err(CellError::Num);
    }
    num_val(2.0 * t_upper(x, v))
}

fn t_dist_rt(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let v = df_arg(a, 1)?;
    num_val(1.0 - t_cdf(x, v))
}

fn tdist_legacy(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let v = df_arg(a, 1)?;
    let tails = num(a, 2)?.trunc();
    if x < 0.0 || !(tails == 1.0 || tails == 2.0) {
        return Err(CellError::Num);
    }
    num_val(tails * t_upper(x, v))
}

fn t_inv(a: &[Arg], _: C) -> R<Value> {
    let p = num(a, 0)?;
    let v = df_arg(a, 1)?;
    if !(p > 0.0 && p < 1.0) {
        return Err(CellError::Num);
    }
    num_val(if p < 0.5 { -t_inv_upper(p, v) } else { t_inv_upper(1.0 - p, v) })
}

fn t_inv_2t(a: &[Arg], _: C) -> R<Value> {
    let p = num(a, 0)?;
    let v = df_arg(a, 1)?;
    if !(p > 0.0 && p <= 1.0) {
        return Err(CellError::Num);
    }
    num_val(t_inv_upper(p / 2.0, v))
}

fn t_test(a: &[Arg], _: C) -> R<Value> {
    let tails = num(a, 2)?.trunc();
    let ty = num(a, 3)?.trunc();
    if !(tails == 1.0 || tails == 2.0) || !(1.0..=3.0).contains(&ty) {
        return Err(CellError::Num);
    }
    let (t, v) = if ty == 1.0 {
        let (x, y) = arg_pairs(a, 0, 1)?;
        let d: Vec<f64> = x.iter().zip(&y).map(|(p, q)| p - q).collect();
        let n = d.len() as f64;
        let sd = var_s(&d)?.sqrt();
        if sd == 0.0 {
            return Err(CellError::Div0);
        }
        (mean(&d)? / (sd / n.sqrt()), n - 1.0)
    } else {
        let x = arg_nums(a, 0)?;
        let y = arg_nums(a, 1)?;
        let (n1, n2) = (x.len() as f64, y.len() as f64);
        let (v1, v2) = (var_s(&x)?, var_s(&y)?);
        let diff = mean(&x)? - mean(&y)?;
        if ty == 2.0 {
            let df = n1 + n2 - 2.0;
            let sp = ((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / df;
            let se = (sp * (1.0 / n1 + 1.0 / n2)).sqrt();
            if se == 0.0 {
                return Err(CellError::Div0);
            }
            (diff / se, df)
        } else {
            let (a1, a2) = (v1 / n1, v2 / n2);
            let se = (a1 + a2).sqrt();
            if se == 0.0 {
                return Err(CellError::Div0);
            }
            let df = (a1 + a2).powi(2) / (a1 * a1 / (n1 - 1.0) + a2 * a2 / (n2 - 1.0));
            (diff / se, df)
        }
    };
    num_val(tails * t_upper(t.abs(), v))
}

fn chisq_dist(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let v = df_arg(a, 1)?;
    let cum = boolean(a, 2)?;
    if x < 0.0 || v > 1e10 {
        return Err(CellError::Num);
    }
    if cum {
        return num_val(reg_gamma_p(v / 2.0, x / 2.0));
    }
    if x == 0.0 {
        return if v == 2.0 {
            num_val(0.5)
        } else if v < 2.0 {
            Err(CellError::Num)
        } else {
            num_val(0.0)
        };
    }
    num_val(((v / 2.0 - 1.0) * x.ln() - x / 2.0 - (v / 2.0) * 2f64.ln() - ln_gamma(v / 2.0)).exp())
}

fn chisq_dist_rt(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let v = df_arg(a, 1)?;
    if x < 0.0 || v > 1e10 {
        return Err(CellError::Num);
    }
    num_val(reg_gamma_q(v / 2.0, x / 2.0))
}

fn chisq_inv(a: &[Arg], _: C) -> R<Value> {
    let p = num(a, 0)?;
    let v = df_arg(a, 1)?;
    if !(0.0..1.0).contains(&p) || v > 1e10 {
        return Err(CellError::Num);
    }
    num_val(chi_inv(p, v))
}

fn chisq_inv_rt(a: &[Arg], _: C) -> R<Value> {
    let p = num(a, 0)?;
    let v = df_arg(a, 1)?;
    if !(p > 0.0 && p <= 1.0) || v > 1e10 {
        return Err(CellError::Num);
    }
    num_val(chi_inv_rt(p, v))
}

fn chisq_test(a: &[Arg], _: C) -> R<Value> {
    let act = as_array(&arg(a, 0)?.value).into_owned();
    let exp = as_array(&arg(a, 1)?.value).into_owned();
    if act.rows != exp.rows || act.cols != exp.cols {
        return Err(CellError::NA);
    }
    let mut x2 = 0.0;
    for (o, e) in act.data.iter().zip(&exp.data) {
        if let Value::Error(err) = o {
            return Err(*err);
        }
        if let Value::Error(err) = e {
            return Err(*err);
        }
        if let (Value::Number(o), Value::Number(e)) = (o, e) {
            if *e == 0.0 {
                return Err(CellError::Div0);
            }
            x2 += (o - e).powi(2) / e;
        }
    }
    let df = if act.rows > 1 && act.cols > 1 { ((act.rows - 1) * (act.cols - 1)) as f64 } else { (act.data.len().saturating_sub(1)) as f64 };
    if df < 1.0 {
        return Err(CellError::NA);
    }
    num_val(reg_gamma_q(df / 2.0, x2 / 2.0))
}

fn f_args(a: &[Arg]) -> R<(f64, f64, f64)> {
    let x = num(a, 0)?;
    let d1 = num(a, 1)?.trunc();
    let d2 = num(a, 2)?.trunc();
    if d1 < 1.0 || d2 < 1.0 || d1 >= 1e10 || d2 >= 1e10 {
        return Err(CellError::Num);
    }
    Ok((x, d1, d2))
}

fn f_dist(a: &[Arg], _: C) -> R<Value> {
    let (x, d1, d2) = f_args(a)?;
    let cum = boolean(a, 3)?;
    if x < 0.0 {
        return Err(CellError::Num);
    }
    num_val(if cum { f_cdf(x, d1, d2) } else { f_pdf(x, d1, d2) })
}

fn f_dist_rt(a: &[Arg], _: C) -> R<Value> {
    let (x, d1, d2) = f_args(a)?;
    if x < 0.0 {
        return Err(CellError::Num);
    }
    num_val(f_upper(x, d1, d2))
}

fn f_inv(a: &[Arg], _: C) -> R<Value> {
    let (p, d1, d2) = f_args(a)?;
    if !(0.0..1.0).contains(&p) {
        return Err(CellError::Num);
    }
    if p == 0.0 {
        return num_val(0.0);
    }
    num_val(invert(|x| f_cdf(x, d1, d2), p, 0.0, 1.0, 1e60))
}

fn f_inv_rt(a: &[Arg], _: C) -> R<Value> {
    let (p, d1, d2) = f_args(a)?;
    if !(p > 0.0 && p <= 1.0) {
        return Err(CellError::Num);
    }
    if p == 1.0 {
        return num_val(0.0);
    }
    num_val(invert(|x| f_upper(x, d1, d2), p, 0.0, 1.0, 1e60))
}

fn f_test(a: &[Arg], _: C) -> R<Value> {
    let x = arg_nums(a, 0)?;
    let y = arg_nums(a, 1)?;
    let (v1, v2) = (var_s(&x)?, var_s(&y)?);
    if v1 == 0.0 || v2 == 0.0 {
        return Err(CellError::Div0);
    }
    let f = v1 / v2;
    let (d1, d2) = ((x.len() - 1) as f64, (y.len() - 1) as f64);
    let c = f_cdf(f, d1, d2);
    num_val((2.0 * c.min(1.0 - c)).min(1.0))
}

fn binom_dist(a: &[Arg], _: C) -> R<Value> {
    let k = num(a, 0)?.trunc();
    let n = num(a, 1)?.trunc();
    let p = num(a, 2)?;
    let cum = boolean(a, 3)?;
    if k < 0.0 || k > n || !(0.0..=1.0).contains(&p) {
        return Err(CellError::Num);
    }
    num_val(if cum { binom_cdf(k, n, p) } else { binom_pmf(k, n, p) })
}

fn binom_dist_range(a: &[Arg], _: C) -> R<Value> {
    let n = num(a, 0)?.trunc();
    let p = num(a, 1)?;
    let s1 = num(a, 2)?.trunc();
    let s2 = if has(a, 3) { num(a, 3)?.trunc() } else { s1 };
    if !(0.0..=1e15).contains(&n) || !(0.0..=1.0).contains(&p) || s1 < 0.0 || s1 > n || s2 < s1 || s2 > n {
        return Err(CellError::Num);
    }
    if s2 - s1 > 10_000.0 {
        return num_val(binom_cdf(s2, n, p) - binom_cdf(s1 - 1.0, n, p));
    }
    let mut s = 0.0;
    let mut k = s1;
    while k <= s2 {
        s += binom_pmf(k, n, p);
        k += 1.0;
    }
    num_val(s)
}

fn binom_inv(a: &[Arg], _: C) -> R<Value> {
    let n = num(a, 0)?.trunc();
    let p = num(a, 1)?;
    let alpha = num(a, 2)?;
    if n < 0.0 || !(0.0..=1.0).contains(&p) || !(0.0..=1.0).contains(&alpha) {
        return Err(CellError::Num);
    }
    // Smallest k with cdf(k) ≥ alpha, by bisection on the integer k.
    let (mut lo, mut hi) = (0.0f64, n);
    if binom_cdf(0.0, n, p) >= alpha {
        return num_val(0.0);
    }
    while hi - lo > 1.0 {
        let mid = ((lo + hi) / 2.0).floor();
        if binom_cdf(mid, n, p) >= alpha {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    num_val(hi)
}

fn poisson_dist(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?.trunc();
    let m = num(a, 1)?;
    let cum = boolean(a, 2)?;
    if x < 0.0 || m < 0.0 {
        return Err(CellError::Num);
    }
    num_val(if cum { poisson_cdf(x, m) } else { poisson_pmf(x, m) })
}

fn expon_dist(a: &[Arg], _: C) -> R<Value> {
    let (x, l, cum) = (num(a, 0)?, num(a, 1)?, boolean(a, 2)?);
    if x < 0.0 || l <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(if cum { -(-l * x).exp_m1() } else { l * (-l * x).exp() })
}

fn gamma_fn(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let g = gamma(x);
    if !g.is_finite() {
        return Err(CellError::Num);
    }
    num_val(g)
}

fn gamma_dist(a: &[Arg], _: C) -> R<Value> {
    let (x, al, be, cum) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, boolean(a, 3)?);
    if x < 0.0 || al <= 0.0 || be <= 0.0 {
        return Err(CellError::Num);
    }
    if cum {
        return num_val(reg_gamma_p(al, x / be));
    }
    if x == 0.0 {
        return if al < 1.0 {
            Err(CellError::Num)
        } else if al == 1.0 {
            num_val(1.0 / be)
        } else {
            num_val(0.0)
        };
    }
    num_val(((al - 1.0) * x.ln() - x / be - al * be.ln() - ln_gamma(al)).exp())
}

fn gamma_inv(a: &[Arg], _: C) -> R<Value> {
    let (p, al, be) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    if !(0.0..1.0).contains(&p) || al <= 0.0 || be <= 0.0 {
        return Err(CellError::Num);
    }
    if p == 0.0 {
        return num_val(0.0);
    }
    num_val(invert(|x| reg_gamma_p(al, x / be), p, 0.0, (al * be).max(1e-3), 1e60))
}

fn gammaln(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    if x <= 0.0 {
        return Err(CellError::Num);
    }
    num_val(ln_gamma(x))
}

fn beta_bounds(a: &[Arg], i: usize) -> R<(f64, f64)> {
    let lo = if has(a, i) { num(a, i)? } else { 0.0 };
    let hi = if has(a, i + 1) { num(a, i + 1)? } else { 1.0 };
    if lo >= hi {
        return Err(CellError::Num);
    }
    Ok((lo, hi))
}

fn beta_dist(a: &[Arg], _: C) -> R<Value> {
    let (x, al, be, cum) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, boolean(a, 3)?);
    let (lo, hi) = beta_bounds(a, 4)?;
    if al <= 0.0 || be <= 0.0 || x < lo || x > hi {
        return Err(CellError::Num);
    }
    let z = (x - lo) / (hi - lo);
    if cum {
        return num_val(reg_inc_beta(z, al, be));
    }
    if (z == 0.0 && al < 1.0) || (z == 1.0 && be < 1.0) {
        return Err(CellError::Num);
    }
    let lp = if z == 0.0 || z == 1.0 {
        let base = if z == 0.0 { al } else { be };
        if base == 1.0 { -ln_beta(al, be) } else { f64::NEG_INFINITY }
    } else {
        (al - 1.0) * z.ln() + (be - 1.0) * (1.0 - z).ln() - ln_beta(al, be)
    };
    num_val(lp.exp() / (hi - lo))
}

fn betadist_legacy(a: &[Arg], _: C) -> R<Value> {
    let (x, al, be) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let (lo, hi) = beta_bounds(a, 3)?;
    if al <= 0.0 || be <= 0.0 || x < lo || x > hi {
        return Err(CellError::Num);
    }
    num_val(reg_inc_beta((x - lo) / (hi - lo), al, be))
}

fn beta_inv(a: &[Arg], _: C) -> R<Value> {
    let (p, al, be) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let (lo, hi) = beta_bounds(a, 3)?;
    if al <= 0.0 || be <= 0.0 || !(0.0..=1.0).contains(&p) {
        return Err(CellError::Num);
    }
    let z = if p == 0.0 {
        0.0
    } else if p == 1.0 {
        1.0
    } else {
        invert(|x| reg_inc_beta(x, al, be), p, 0.0, 1.0, 1.0)
    };
    num_val(lo + z * (hi - lo))
}

fn lognorm_dist(a: &[Arg], _: C) -> R<Value> {
    let (x, m, sd) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    let cum = if a.len() > 3 { boolean(a, 3)? } else { true };
    if x <= 0.0 || sd <= 0.0 {
        return Err(CellError::Num);
    }
    let z = (x.ln() - m) / sd;
    num_val(if cum { norm_cdf(z) } else { norm_pdf(z) / (x * sd) })
}

fn lognorm_inv(a: &[Arg], _: C) -> R<Value> {
    let (p, m, sd) = (num(a, 0)?, num(a, 1)?, num(a, 2)?);
    if !(p > 0.0 && p < 1.0) || sd <= 0.0 {
        return Err(CellError::Num);
    }
    num_val((m + sd * norm_inv(p)).exp())
}

fn weibull_dist(a: &[Arg], _: C) -> R<Value> {
    let (x, al, be, cum) = (num(a, 0)?, num(a, 1)?, num(a, 2)?, boolean(a, 3)?);
    if x < 0.0 || al <= 0.0 || be <= 0.0 {
        return Err(CellError::Num);
    }
    let t = (x / be).powf(al);
    num_val(if cum { -(-t).exp_m1() } else { al / be.powf(al) * x.powf(al - 1.0) * (-t).exp() })
}

fn hypgeom_dist(a: &[Arg], _: C) -> R<Value> {
    let k = num(a, 0)?.trunc();
    let n = num(a, 1)?.trunc();
    let kk = num(a, 2)?.trunc();
    let nn = num(a, 3)?.trunc();
    let cum = if a.len() > 4 { boolean(a, 4)? } else { false };
    if k < 0.0 || n <= 0.0 || kk <= 0.0 || nn <= 0.0 || n > nn || kk > nn || k > n.min(kk) || k < (n - nn + kk).max(0.0) {
        return Err(CellError::Num);
    }
    let pmf = |i: f64| (ln_comb(kk, i) + ln_comb(nn - kk, n - i) - ln_comb(nn, n)).exp();
    if !cum {
        return num_val(pmf(k));
    }
    let mut s = 0.0;
    let mut i = (n - nn + kk).max(0.0);
    if k - i > 1e6 || i + 1.0 == i {
        return Err(CellError::Num);
    }
    while i <= k {
        s += pmf(i);
        i += 1.0;
    }
    num_val(s.min(1.0))
}

fn negbinom_dist(a: &[Arg], _: C) -> R<Value> {
    let f = num(a, 0)?.trunc();
    let s = num(a, 1)?.trunc();
    let p = num(a, 2)?;
    let cum = if a.len() > 3 { boolean(a, 3)? } else { false };
    if f < 0.0 || s < 1.0 || !(0.0..=1.0).contains(&p) {
        return Err(CellError::Num);
    }
    if cum {
        if p == 0.0 {
            return num_val(0.0);
        }
        return num_val(reg_inc_beta(p, s, f + 1.0));
    }
    if p == 0.0 || p == 1.0 {
        return num_val(if p == 1.0 && f == 0.0 { 1.0 } else { 0.0 });
    }
    num_val((ln_comb(f + s - 1.0, s - 1.0) + s * p.ln() + f * (1.0 - p).ln()).exp())
}

fn confidence_norm(a: &[Arg], _: C) -> R<Value> {
    let (al, sd, n) = (num(a, 0)?, num(a, 1)?, num(a, 2)?.trunc());
    if !(al > 0.0 && al < 1.0) || sd <= 0.0 || n < 1.0 {
        return Err(CellError::Num);
    }
    num_val(norm_inv(1.0 - al / 2.0) * sd / n.sqrt())
}

fn confidence_t(a: &[Arg], _: C) -> R<Value> {
    let (al, sd, n) = (num(a, 0)?, num(a, 1)?, num(a, 2)?.trunc());
    if !(al > 0.0 && al < 1.0) || sd <= 0.0 || n < 1.0 {
        return Err(CellError::Num);
    }
    if n == 1.0 {
        return Err(CellError::Div0);
    }
    num_val(t_inv_upper(al / 2.0, n - 1.0) * sd / n.sqrt())
}

fn z_test(a: &[Arg], _: C) -> R<Value> {
    let v = arg_nums(a, 0)?;
    let x = num(a, 1)?;
    if v.is_empty() {
        return Err(CellError::NA);
    }
    let sd = if has(a, 2) { num(a, 2)? } else { var_s(&v)?.sqrt() };
    if sd <= 0.0 {
        return Err(if has(a, 2) { CellError::Num } else { CellError::Div0 });
    }
    let z = (mean(&v)? - x) / (sd / (v.len() as f64).sqrt());
    num_val(1.0 - norm_cdf(z))
}

fn fisher(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    if x <= -1.0 || x >= 1.0 {
        return Err(CellError::Num);
    }
    num_val(0.5 * ((1.0 + x) / (1.0 - x)).ln())
}

fn gauss(a: &[Arg], _: C) -> R<Value> {
    let z = num(a, 0)?;
    // Φ(z) − 0.5 = erf(z/√2)/2, accurate near zero.
    num_val(0.5 * erf(z / std::f64::consts::SQRT_2))
}

// ---------------------------------------------------------------------------------------------
// SUBTOTAL / AGGREGATE support
// ---------------------------------------------------------------------------------------------

/// Evaluates SUBTOTAL/AGGREGATE function `function_num` over already-filtered cell values.
///
/// Codes 1–11 (and 101–111) are AVERAGE, COUNT, COUNTA, MAX, MIN, PRODUCT, STDEV.S, STDEV.P,
/// SUM, VAR.S, VAR.P; 12 = MEDIAN, 13 = MODE.SNGL. Codes 14–19 need `k`: use
/// [`aggregate_values_k`] (they give `#VALUE!` here). Values that are not numbers are ignored
/// (COUNTA counts non-empty values); the first error value present is returned.
pub fn aggregate_values(function_num: u32, values: &[Value]) -> Value {
    crate::util::finish(aggregate_impl(function_num, values, None))
}

/// AGGREGATE codes 14 = LARGE, 15 = SMALL, 16 = PERCENTILE.INC, 17 = QUARTILE.INC,
/// 18 = PERCENTILE.EXC, 19 = QUARTILE.EXC with their `k` argument; codes 1–13 delegate to
/// [`aggregate_values`].
pub fn aggregate_values_k(function_num: u32, values: &[Value], k: &Value) -> Value {
    crate::util::finish(aggregate_impl(function_num, values, Some(k)))
}

fn aggregate_impl(function_num: u32, values: &[Value], k: Option<&Value>) -> R<Value> {
    let code = if (101..=111).contains(&function_num) { function_num - 100 } else { function_num };
    let mut xs = Vec::new();
    let mut nonempty = 0usize;
    let mut push = |v: &Value| -> R<()> {
        match v {
            Value::Number(n) => xs.push(*n),
            Value::Error(e) => return Err(*e),
            _ => {}
        }
        if !v.is_empty() {
            nonempty += 1;
        }
        Ok(())
    };
    for v in values {
        match v {
            Value::Array(arr) => arr.iter().try_for_each(&mut push)?,
            other => push(other)?,
        }
    }
    let k = match (code, k) {
        (14..=19, Some(k)) => k.scalar().to_number()?,
        (14..=19, None) => return Err(CellError::Value),
        _ => 0.0,
    };
    let r = match code {
        1 => mean(&xs)?,
        2 => xs.len() as f64,
        3 => nonempty as f64,
        4 => max_of(&xs),
        5 => min_of(&xs),
        6 => product(&xs),
        7 => var_s(&xs)?.sqrt(),
        8 => var_p(&xs)?.sqrt(),
        9 => xs.iter().sum(),
        10 => var_s(&xs)?,
        11 => var_p(&xs)?,
        12 => median(xs)?,
        13 => mode_sngl(&xs)?,
        14 => kth(xs, k, true)?,
        15 => kth(xs, k, false)?,
        16 => percentile_inc(xs, k)?,
        17 => quartile_inc(xs, k)?,
        18 => percentile_exc(xs, k)?,
        19 => quartile_exc(xs, k)?,
        _ => return Err(CellError::Value),
    };
    num_val(r)
}

// ---------------------------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------------------------

const S_ARR_K: &[bool] = &[false, true];
const S_RANK: &[bool] = &[true, false, true];
const S_PRANK: &[bool] = &[false, true, true];
const S_FORECAST: &[bool] = &[true, false, false];
const S_PROB: &[bool] = &[false, false, true, true];
const S_TTEST: &[bool] = &[false, false, true, true];

pub(crate) fn specs() -> Vec<FnSpec> {
    vec![
        f!("AVERAGE", 1, VAR, Statistical, A, "AVERAGE(number1, [number2], ...)", "Arithmetic mean of the numbers.", |a: &[Arg], _c: C| num_val(
            mean(&numbers(a)?)?
        )),
        f!(
            "AVERAGEA",
            1,
            VAR,
            Statistical,
            A,
            "AVERAGEA(value1, [value2], ...)",
            "Mean counting text as 0 and logical values as 1 or 0.",
            |a: &[Arg], _c: C| num_val(mean(&numbers_a(a)?)?)
        ),
        f!("MEDIAN", 1, VAR, Statistical, A, "MEDIAN(number1, [number2], ...)", "Middle value of the numbers.", |a: &[Arg], _c: C| num_val(median(
            numbers(a)?
        )?)),
        f!("MODE", 1, VAR, Compatibility, A, "MODE(number1, [number2], ...)", "Most frequently occurring number.", |a: &[Arg], _c: C| num_val(
            mode_sngl(&numbers(a)?)?
        )),
        f!("MODE.SNGL", 1, VAR, Statistical, A, "MODE.SNGL(number1, [number2], ...)", "Most frequently occurring number.", |a: &[Arg], _c: C| {
            num_val(mode_sngl(&numbers(a)?)?)
        }),
        f!("MODE.MULT", 1, VAR, Statistical, A, "MODE.MULT(number1, [number2], ...)", "Vertical array of all the most frequent numbers.", mode_mult),
        f!("STDEV", 1, VAR, Compatibility, A, "STDEV(number1, [number2], ...)", "Sample standard deviation.", |a: &[Arg], _c: C| num_val(
            var_s(&numbers(a)?)?.sqrt()
        )),
        f!("STDEV.S", 1, VAR, Statistical, A, "STDEV.S(number1, [number2], ...)", "Sample standard deviation.", |a: &[Arg], _c: C| num_val(
            var_s(&numbers(a)?)?.sqrt()
        )),
        f!("STDEV.P", 1, VAR, Statistical, A, "STDEV.P(number1, [number2], ...)", "Population standard deviation.", |a: &[Arg], _c: C| num_val(
            var_p(&numbers(a)?)?.sqrt()
        )),
        f!("STDEVP", 1, VAR, Compatibility, A, "STDEVP(number1, [number2], ...)", "Population standard deviation.", |a: &[Arg], _c: C| num_val(
            var_p(&numbers(a)?)?.sqrt()
        )),
        f!(
            "STDEVA",
            1,
            VAR,
            Statistical,
            A,
            "STDEVA(value1, [value2], ...)",
            "Sample standard deviation counting text as 0 and logicals as 1/0.",
            |a: &[Arg], _c: C| { num_val(var_s(&numbers_a(a)?)?.sqrt()) }
        ),
        f!(
            "STDEVPA",
            1,
            VAR,
            Statistical,
            A,
            "STDEVPA(value1, [value2], ...)",
            "Population standard deviation counting text as 0 and logicals as 1/0.",
            |a: &[Arg], _c: C| { num_val(var_p(&numbers_a(a)?)?.sqrt()) }
        ),
        f!("VAR", 1, VAR, Compatibility, A, "VAR(number1, [number2], ...)", "Sample variance.", |a: &[Arg], _c: C| num_val(var_s(&numbers(a)?)?)),
        f!("VAR.S", 1, VAR, Statistical, A, "VAR.S(number1, [number2], ...)", "Sample variance.", |a: &[Arg], _c: C| num_val(var_s(&numbers(a)?)?)),
        f!("VAR.P", 1, VAR, Statistical, A, "VAR.P(number1, [number2], ...)", "Population variance.", |a: &[Arg], _c: C| num_val(var_p(&numbers(
            a
        )?)?)),
        f!("VARP", 1, VAR, Compatibility, A, "VARP(number1, [number2], ...)", "Population variance.", |a: &[Arg], _c: C| num_val(var_p(&numbers(
            a
        )?)?)),
        f!(
            "VARA",
            1,
            VAR,
            Statistical,
            A,
            "VARA(value1, [value2], ...)",
            "Sample variance counting text as 0 and logicals as 1/0.",
            |a: &[Arg], _c: C| num_val(var_s(&numbers_a(a)?)?)
        ),
        f!(
            "VARPA",
            1,
            VAR,
            Statistical,
            A,
            "VARPA(value1, [value2], ...)",
            "Population variance counting text as 0 and logicals as 1/0.",
            |a: &[Arg], _c: C| num_val(var_p(&numbers_a(a)?)?)
        ),
        f!("MIN", 1, VAR, Statistical, A, "MIN(number1, [number2], ...)", "Smallest number (0 when there are none).", |a: &[Arg], _c: C| num_val(
            min_of(&numbers(a)?)
        )),
        f!(
            "MINA",
            1,
            VAR,
            Statistical,
            A,
            "MINA(value1, [value2], ...)",
            "Smallest value counting text as 0 and logicals as 1/0.",
            |a: &[Arg], _c: C| num_val(min_of(&numbers_a(a)?))
        ),
        f!("MAX", 1, VAR, Statistical, A, "MAX(number1, [number2], ...)", "Largest number (0 when there are none).", |a: &[Arg], _c: C| num_val(
            max_of(&numbers(a)?)
        )),
        f!(
            "MAXA",
            1,
            VAR,
            Statistical,
            A,
            "MAXA(value1, [value2], ...)",
            "Largest value counting text as 0 and logicals as 1/0.",
            |a: &[Arg], _c: C| num_val(max_of(&numbers_a(a)?))
        ),
        f!("LARGE", 2, 2, Statistical, S_ARR_K, "LARGE(array, k)", "The k-th largest number in a data set.", large),
        f!("SMALL", 2, 2, Statistical, S_ARR_K, "SMALL(array, k)", "The k-th smallest number in a data set.", small),
        f!("RANK", 2, 3, Compatibility, S_RANK, "RANK(number, ref, [order])", "Rank of a number within a list.", |a: &[Arg], _c: C| rank_impl(
            a, false
        )),
        f!(
            "RANK.EQ",
            2,
            3,
            Statistical,
            S_RANK,
            "RANK.EQ(number, ref, [order])",
            "Rank of a number within a list; ties share the top rank.",
            |a: &[Arg], _c: C| rank_impl(a, false)
        ),
        f!(
            "RANK.AVG",
            2,
            3,
            Statistical,
            S_RANK,
            "RANK.AVG(number, ref, [order])",
            "Rank of a number within a list; ties get the average rank.",
            |a: &[Arg], _c: C| { rank_impl(a, true) }
        ),
        f!("PERCENTILE", 2, 2, Compatibility, S_ARR_K, "PERCENTILE(array, k)", "k-th percentile, k from 0 to 1 inclusive.", |a: &[Arg], _c: C| {
            num_val(percentile_inc(arg_nums(a, 0)?, num(a, 1)?)?)
        }),
        f!(
            "PERCENTILE.INC",
            2,
            2,
            Statistical,
            S_ARR_K,
            "PERCENTILE.INC(array, k)",
            "k-th percentile, k from 0 to 1 inclusive.",
            |a: &[Arg], _c: C| num_val(percentile_inc(arg_nums(a, 0)?, num(a, 1)?)?)
        ),
        f!(
            "PERCENTILE.EXC",
            2,
            2,
            Statistical,
            S_ARR_K,
            "PERCENTILE.EXC(array, k)",
            "k-th percentile, k strictly between 0 and 1.",
            |a: &[Arg], _c: C| num_val(percentile_exc(arg_nums(a, 0)?, num(a, 1)?)?)
        ),
        f!("QUARTILE", 2, 2, Compatibility, S_ARR_K, "QUARTILE(array, quart)", "Quartile (0–4) of a data set.", |a: &[Arg], _c: C| num_val(
            quartile_inc(arg_nums(a, 0)?, num(a, 1)?)?
        )),
        f!(
            "QUARTILE.INC",
            2,
            2,
            Statistical,
            S_ARR_K,
            "QUARTILE.INC(array, quart)",
            "Quartile (0–4) of a data set, inclusive method.",
            |a: &[Arg], _c: C| num_val(quartile_inc(arg_nums(a, 0)?, num(a, 1)?)?)
        ),
        f!(
            "QUARTILE.EXC",
            2,
            2,
            Statistical,
            S_ARR_K,
            "QUARTILE.EXC(array, quart)",
            "Quartile (1–3) of a data set, exclusive method.",
            |a: &[Arg], _c: C| num_val(quartile_exc(arg_nums(a, 0)?, num(a, 1)?)?)
        ),
        f!(
            "PERCENTRANK",
            2,
            3,
            Compatibility,
            S_PRANK,
            "PERCENTRANK(array, x, [significance])",
            "Relative standing of a value as a fraction of the data set.",
            |a: &[Arg], _c: C| { percentrank_impl(a, false) }
        ),
        f!(
            "PERCENTRANK.INC",
            2,
            3,
            Statistical,
            S_PRANK,
            "PERCENTRANK.INC(array, x, [significance])",
            "Relative standing of a value from 0 to 1 inclusive.",
            |a: &[Arg], _c: C| percentrank_impl(a, false)
        ),
        f!(
            "PERCENTRANK.EXC",
            2,
            3,
            Statistical,
            S_PRANK,
            "PERCENTRANK.EXC(array, x, [significance])",
            "Relative standing of a value strictly between 0 and 1.",
            |a: &[Arg], _c: C| percentrank_impl(a, true)
        ),
        f!("COUNT", 1, VAR, Statistical, A, "COUNT(value1, [value2], ...)", "Counts the numbers.", |a: &[Arg], _c: C| num_val(count_impl(a))),
        f!("COUNTA", 1, VAR, Statistical, A, "COUNTA(value1, [value2], ...)", "Counts the non-empty values.", |a: &[Arg], _c: C| num_val(
            counta_impl(a)
        )),
        f!("COUNTBLANK", 1, 1, Statistical, A, "COUNTBLANK(range)", "Counts empty cells and empty text in a range.", countblank),
        f!("CORREL", 2, 2, Statistical, A, "CORREL(array1, array2)", "Correlation coefficient of two data sets.", |a: &[Arg], _c: C| {
            let (x, y) = arg_pairs(a, 0, 1)?;
            num_val(correl(&x, &y)?)
        }),
        f!("PEARSON", 2, 2, Statistical, A, "PEARSON(array1, array2)", "Pearson product-moment correlation coefficient.", |a: &[Arg], _c: C| {
            let (x, y) = arg_pairs(a, 0, 1)?;
            num_val(correl(&x, &y)?)
        }),
        f!("RSQ", 2, 2, Statistical, A, "RSQ(known_ys, known_xs)", "Square of the correlation coefficient.", |a: &[Arg], _c: C| {
            let (y, x) = arg_pairs(a, 0, 1)?;
            num_val(correl(&x, &y)?.powi(2))
        }),
        f!("SLOPE", 2, 2, Statistical, A, "SLOPE(known_ys, known_xs)", "Slope of the least-squares regression line.", |a: &[Arg], _c: C| {
            let (y, x) = arg_pairs(a, 0, 1)?;
            if y.is_empty() {
                return Err(CellError::Div0);
            }
            num_val(slope(&y, &x)?)
        }),
        f!(
            "INTERCEPT",
            2,
            2,
            Statistical,
            A,
            "INTERCEPT(known_ys, known_xs)",
            "Y-intercept of the least-squares regression line.",
            |a: &[Arg], _c: C| {
                let (y, x) = arg_pairs(a, 0, 1)?;
                if y.is_empty() {
                    return Err(CellError::Div0);
                }
                num_val(intercept(&y, &x)?)
            }
        ),
        f!("STEYX", 2, 2, Statistical, A, "STEYX(known_ys, known_xs)", "Standard error of predicted y values in a regression.", steyx),
        f!("FORECAST", 3, 3, Compatibility, S_FORECAST, "FORECAST(x, known_ys, known_xs)", "Predicts a value along a linear trend.", forecast),
        f!(
            "FORECAST.LINEAR",
            3,
            3,
            Statistical,
            S_FORECAST,
            "FORECAST.LINEAR(x, known_ys, known_xs)",
            "Predicts a value along a linear trend.",
            forecast
        ),
        f!(
            "TREND",
            1,
            4,
            Statistical,
            A,
            "TREND(known_ys, [known_xs], [new_xs], [const])",
            "Values along a least-squares linear fit.",
            |a: &[Arg], _c: C| trend_impl(a, false)
        ),
        f!(
            "GROWTH",
            1,
            4,
            Statistical,
            A,
            "GROWTH(known_ys, [known_xs], [new_xs], [const])",
            "Values along a fitted exponential growth curve.",
            |a: &[Arg], _c: C| { trend_impl(a, true) }
        ),
        f!(
            "LINEST",
            1,
            4,
            Statistical,
            A,
            "LINEST(known_ys, [known_xs], [const], [stats])",
            "Least-squares line coefficients and optional regression statistics.",
            |a: &[Arg], _c: C| { linest_impl(a, false) }
        ),
        f!(
            "LOGEST",
            1,
            4,
            Statistical,
            A,
            "LOGEST(known_ys, [known_xs], [const], [stats])",
            "Exponential curve coefficients and optional regression statistics.",
            |a: &[Arg], _c: C| { linest_impl(a, true) }
        ),
        f!("COVARIANCE.P", 2, 2, Statistical, A, "COVARIANCE.P(array1, array2)", "Population covariance of paired values.", |a: &[Arg], _c: C| {
            covariance(a, false)
        }),
        f!("COVARIANCE.S", 2, 2, Statistical, A, "COVARIANCE.S(array1, array2)", "Sample covariance of paired values.", |a: &[Arg], _c: C| {
            covariance(a, true)
        }),
        f!("COVAR", 2, 2, Compatibility, A, "COVAR(array1, array2)", "Population covariance of paired values.", |a: &[Arg], _c: C| covariance(
            a, false
        )),
        f!("DEVSQ", 1, VAR, Statistical, A, "DEVSQ(number1, [number2], ...)", "Sum of squared deviations from the mean.", |a: &[Arg], _c: C| {
            let v = numbers(a)?;
            if v.is_empty() {
                return Err(CellError::Num);
            }
            num_val(devsq(&v))
        }),
        f!("AVEDEV", 1, VAR, Statistical, A, "AVEDEV(number1, [number2], ...)", "Average absolute deviation from the mean.", |a: &[Arg], _c: C| {
            num_val(avedev(&numbers(a)?)?)
        }),
        f!("GEOMEAN", 1, VAR, Statistical, A, "GEOMEAN(number1, [number2], ...)", "Geometric mean of positive numbers.", |a: &[Arg], _c: C| num_val(
            geomean(&numbers(a)?)?
        )),
        f!("HARMEAN", 1, VAR, Statistical, A, "HARMEAN(number1, [number2], ...)", "Harmonic mean of positive numbers.", |a: &[Arg], _c: C| num_val(
            harmean(&numbers(a)?)?
        )),
        f!("TRIMMEAN", 2, 2, Statistical, S_ARR_K, "TRIMMEAN(array, percent)", "Mean after trimming a fraction of points from both ends.", trimmean),
        f!("KURT", 1, VAR, Statistical, A, "KURT(number1, [number2], ...)", "Excess kurtosis of a sample.", |a: &[Arg], _c: C| num_val(kurt(
            &numbers(a)?
        )?)),
        f!("SKEW", 1, VAR, Statistical, A, "SKEW(number1, [number2], ...)", "Sample skewness.", |a: &[Arg], _c: C| num_val(skew(&numbers(a)?)?)),
        f!("SKEW.P", 1, VAR, Statistical, A, "SKEW.P(number1, [number2], ...)", "Population skewness.", |a: &[Arg], _c: C| num_val(skew_p(
            &numbers(a)?
        )?)),
        f!("STANDARDIZE", 3, 3, Statistical, S, "STANDARDIZE(x, mean, standard_dev)", "Z-score of a value.", standardize),
        f!(
            "FREQUENCY",
            2,
            2,
            Statistical,
            A,
            "FREQUENCY(data_array, bins_array)",
            "Counts how many values fall into each bin, as a vertical array.",
            frequency
        ),
        f!(
            "PROB",
            3,
            4,
            Statistical,
            S_PROB,
            "PROB(x_range, prob_range, lower_limit, [upper_limit])",
            "Probability that values fall between two limits.",
            prob
        ),
        f!(
            "NORM.DIST",
            4,
            4,
            Statistical,
            S,
            "NORM.DIST(x, mean, standard_dev, cumulative)",
            "Normal distribution density or cumulative probability.",
            norm_dist
        ),
        f!(
            "NORMDIST",
            4,
            4,
            Compatibility,
            S,
            "NORMDIST(x, mean, standard_dev, cumulative)",
            "Normal distribution density or cumulative probability.",
            norm_dist
        ),
        f!(
            "NORM.INV",
            3,
            3,
            Statistical,
            S,
            "NORM.INV(probability, mean, standard_dev)",
            "Inverse of the normal cumulative distribution.",
            norm_inv_fn
        ),
        f!(
            "NORMINV",
            3,
            3,
            Compatibility,
            S,
            "NORMINV(probability, mean, standard_dev)",
            "Inverse of the normal cumulative distribution.",
            norm_inv_fn
        ),
        f!("NORM.S.DIST", 2, 2, Statistical, S, "NORM.S.DIST(z, cumulative)", "Standard normal density or cumulative probability.", norm_s_dist),
        f!("NORMSDIST", 1, 1, Compatibility, S, "NORMSDIST(z)", "Standard normal cumulative probability.", norm_s_dist),
        f!("NORM.S.INV", 1, 1, Statistical, S, "NORM.S.INV(probability)", "Inverse of the standard normal cumulative distribution.", norm_s_inv),
        f!("NORMSINV", 1, 1, Compatibility, S, "NORMSINV(probability)", "Inverse of the standard normal cumulative distribution.", norm_s_inv),
        f!("T.DIST", 3, 3, Statistical, S, "T.DIST(x, deg_freedom, cumulative)", "Student's t density or left-tailed probability.", t_dist),
        f!("T.DIST.2T", 2, 2, Statistical, S, "T.DIST.2T(x, deg_freedom)", "Two-tailed Student's t probability.", t_dist_2t),
        f!("T.DIST.RT", 2, 2, Statistical, S, "T.DIST.RT(x, deg_freedom)", "Right-tailed Student's t probability.", t_dist_rt),
        f!("TDIST", 3, 3, Compatibility, S, "TDIST(x, deg_freedom, tails)", "One- or two-tailed Student's t probability.", tdist_legacy),
        f!("T.INV", 2, 2, Statistical, S, "T.INV(probability, deg_freedom)", "Left-tailed inverse of Student's t distribution.", t_inv),
        f!("T.INV.2T", 2, 2, Statistical, S, "T.INV.2T(probability, deg_freedom)", "Two-tailed inverse of Student's t distribution.", t_inv_2t),
        f!("TINV", 2, 2, Compatibility, S, "TINV(probability, deg_freedom)", "Two-tailed inverse of Student's t distribution.", t_inv_2t),
        f!("T.TEST", 4, 4, Statistical, S_TTEST, "T.TEST(array1, array2, tails, type)", "Probability from Student's t-test.", t_test),
        f!("TTEST", 4, 4, Compatibility, S_TTEST, "TTEST(array1, array2, tails, type)", "Probability from Student's t-test.", t_test),
        f!(
            "CHISQ.DIST",
            3,
            3,
            Statistical,
            S,
            "CHISQ.DIST(x, deg_freedom, cumulative)",
            "Chi-squared density or left-tailed probability.",
            chisq_dist
        ),
        f!("CHISQ.DIST.RT", 2, 2, Statistical, S, "CHISQ.DIST.RT(x, deg_freedom)", "Right-tailed chi-squared probability.", chisq_dist_rt),
        f!("CHIDIST", 2, 2, Compatibility, S, "CHIDIST(x, deg_freedom)", "Right-tailed chi-squared probability.", chisq_dist_rt),
        f!(
            "CHISQ.INV",
            2,
            2,
            Statistical,
            S,
            "CHISQ.INV(probability, deg_freedom)",
            "Inverse of the left-tailed chi-squared distribution.",
            chisq_inv
        ),
        f!(
            "CHISQ.INV.RT",
            2,
            2,
            Statistical,
            S,
            "CHISQ.INV.RT(probability, deg_freedom)",
            "Inverse of the right-tailed chi-squared distribution.",
            chisq_inv_rt
        ),
        f!(
            "CHIINV",
            2,
            2,
            Compatibility,
            S,
            "CHIINV(probability, deg_freedom)",
            "Inverse of the right-tailed chi-squared distribution.",
            chisq_inv_rt
        ),
        f!("CHISQ.TEST", 2, 2, Statistical, A, "CHISQ.TEST(actual_range, expected_range)", "Chi-squared test of independence.", chisq_test),
        f!("CHITEST", 2, 2, Compatibility, A, "CHITEST(actual_range, expected_range)", "Chi-squared test of independence.", chisq_test),
        f!(
            "F.DIST",
            4,
            4,
            Statistical,
            S,
            "F.DIST(x, deg_freedom1, deg_freedom2, cumulative)",
            "F distribution density or left-tailed probability.",
            f_dist
        ),
        f!("F.DIST.RT", 3, 3, Statistical, S, "F.DIST.RT(x, deg_freedom1, deg_freedom2)", "Right-tailed F probability.", f_dist_rt),
        f!("FDIST", 3, 3, Compatibility, S, "FDIST(x, deg_freedom1, deg_freedom2)", "Right-tailed F probability.", f_dist_rt),
        f!("F.INV", 3, 3, Statistical, S, "F.INV(probability, deg_freedom1, deg_freedom2)", "Inverse of the left-tailed F distribution.", f_inv),
        f!(
            "F.INV.RT",
            3,
            3,
            Statistical,
            S,
            "F.INV.RT(probability, deg_freedom1, deg_freedom2)",
            "Inverse of the right-tailed F distribution.",
            f_inv_rt
        ),
        f!("FINV", 3, 3, Compatibility, S, "FINV(probability, deg_freedom1, deg_freedom2)", "Inverse of the right-tailed F distribution.", f_inv_rt),
        f!("F.TEST", 2, 2, Statistical, A, "F.TEST(array1, array2)", "Two-tailed probability that two variances do not differ.", f_test),
        f!("FTEST", 2, 2, Compatibility, A, "FTEST(array1, array2)", "Two-tailed probability that two variances do not differ.", f_test),
        f!("BINOM.DIST", 4, 4, Statistical, S, "BINOM.DIST(number_s, trials, probability_s, cumulative)", "Binomial probability.", binom_dist),
        f!("BINOMDIST", 4, 4, Compatibility, S, "BINOMDIST(number_s, trials, probability_s, cumulative)", "Binomial probability.", binom_dist),
        f!(
            "BINOM.DIST.RANGE",
            3,
            4,
            Statistical,
            S,
            "BINOM.DIST.RANGE(trials, probability_s, number_s, [number_s2])",
            "Probability of a number of successes falling in a range.",
            binom_dist_range
        ),
        f!(
            "BINOM.INV",
            3,
            3,
            Statistical,
            S,
            "BINOM.INV(trials, probability_s, alpha)",
            "Smallest success count whose cumulative binomial probability reaches alpha.",
            binom_inv
        ),
        f!(
            "CRITBINOM",
            3,
            3,
            Compatibility,
            S,
            "CRITBINOM(trials, probability_s, alpha)",
            "Smallest success count whose cumulative binomial probability reaches alpha.",
            binom_inv
        ),
        f!("POISSON.DIST", 3, 3, Statistical, S, "POISSON.DIST(x, mean, cumulative)", "Poisson probability.", poisson_dist),
        f!("POISSON", 3, 3, Compatibility, S, "POISSON(x, mean, cumulative)", "Poisson probability.", poisson_dist),
        f!("EXPON.DIST", 3, 3, Statistical, S, "EXPON.DIST(x, lambda, cumulative)", "Exponential distribution density or probability.", expon_dist),
        f!("EXPONDIST", 3, 3, Compatibility, S, "EXPONDIST(x, lambda, cumulative)", "Exponential distribution density or probability.", expon_dist),
        f!("GAMMA", 1, 1, Statistical, S, "GAMMA(number)", "The gamma function.", gamma_fn),
        f!("GAMMA.DIST", 4, 4, Statistical, S, "GAMMA.DIST(x, alpha, beta, cumulative)", "Gamma distribution density or probability.", gamma_dist),
        f!("GAMMADIST", 4, 4, Compatibility, S, "GAMMADIST(x, alpha, beta, cumulative)", "Gamma distribution density or probability.", gamma_dist),
        f!("GAMMA.INV", 3, 3, Statistical, S, "GAMMA.INV(probability, alpha, beta)", "Inverse of the gamma cumulative distribution.", gamma_inv),
        f!("GAMMAINV", 3, 3, Compatibility, S, "GAMMAINV(probability, alpha, beta)", "Inverse of the gamma cumulative distribution.", gamma_inv),
        f!("GAMMALN", 1, 1, Statistical, S, "GAMMALN(x)", "Natural log of the gamma function.", gammaln),
        f!("GAMMALN.PRECISE", 1, 1, Statistical, S, "GAMMALN.PRECISE(x)", "Natural log of the gamma function.", gammaln),
        f!(
            "BETA.DIST",
            4,
            6,
            Statistical,
            S,
            "BETA.DIST(x, alpha, beta, cumulative, [A], [B])",
            "Beta distribution density or probability.",
            beta_dist
        ),
        f!("BETADIST", 3, 5, Compatibility, S, "BETADIST(x, alpha, beta, [A], [B])", "Cumulative beta probability.", betadist_legacy),
        f!(
            "BETA.INV",
            3,
            5,
            Statistical,
            S,
            "BETA.INV(probability, alpha, beta, [A], [B])",
            "Inverse of the cumulative beta distribution.",
            beta_inv
        ),
        f!(
            "BETAINV",
            3,
            5,
            Compatibility,
            S,
            "BETAINV(probability, alpha, beta, [A], [B])",
            "Inverse of the cumulative beta distribution.",
            beta_inv
        ),
        f!(
            "LOGNORM.DIST",
            4,
            4,
            Statistical,
            S,
            "LOGNORM.DIST(x, mean, standard_dev, cumulative)",
            "Lognormal density or probability.",
            lognorm_dist
        ),
        f!("LOGNORMDIST", 3, 3, Compatibility, S, "LOGNORMDIST(x, mean, standard_dev)", "Cumulative lognormal probability.", lognorm_dist),
        f!(
            "LOGNORM.INV",
            3,
            3,
            Statistical,
            S,
            "LOGNORM.INV(probability, mean, standard_dev)",
            "Inverse of the cumulative lognormal distribution.",
            lognorm_inv
        ),
        f!(
            "LOGINV",
            3,
            3,
            Compatibility,
            S,
            "LOGINV(probability, mean, standard_dev)",
            "Inverse of the cumulative lognormal distribution.",
            lognorm_inv
        ),
        f!("WEIBULL.DIST", 4, 4, Statistical, S, "WEIBULL.DIST(x, alpha, beta, cumulative)", "Weibull density or probability.", weibull_dist),
        f!("WEIBULL", 4, 4, Compatibility, S, "WEIBULL(x, alpha, beta, cumulative)", "Weibull density or probability.", weibull_dist),
        f!(
            "HYPGEOM.DIST",
            5,
            5,
            Statistical,
            S,
            "HYPGEOM.DIST(sample_s, number_sample, population_s, number_pop, cumulative)",
            "Hypergeometric probability.",
            hypgeom_dist
        ),
        f!(
            "HYPGEOMDIST",
            4,
            4,
            Compatibility,
            S,
            "HYPGEOMDIST(sample_s, number_sample, population_s, number_pop)",
            "Hypergeometric probability of an exact count.",
            hypgeom_dist
        ),
        f!(
            "NEGBINOM.DIST",
            4,
            4,
            Statistical,
            S,
            "NEGBINOM.DIST(number_f, number_s, probability_s, cumulative)",
            "Negative binomial probability.",
            negbinom_dist
        ),
        f!(
            "NEGBINOMDIST",
            3,
            3,
            Compatibility,
            S,
            "NEGBINOMDIST(number_f, number_s, probability_s)",
            "Negative binomial probability.",
            negbinom_dist
        ),
        f!(
            "CONFIDENCE",
            3,
            3,
            Compatibility,
            S,
            "CONFIDENCE(alpha, standard_dev, size)",
            "Confidence interval half-width using the normal distribution.",
            confidence_norm
        ),
        f!(
            "CONFIDENCE.NORM",
            3,
            3,
            Statistical,
            S,
            "CONFIDENCE.NORM(alpha, standard_dev, size)",
            "Confidence interval half-width using the normal distribution.",
            confidence_norm
        ),
        f!(
            "CONFIDENCE.T",
            3,
            3,
            Statistical,
            S,
            "CONFIDENCE.T(alpha, standard_dev, size)",
            "Confidence interval half-width using Student's t distribution.",
            confidence_t
        ),
        f!("Z.TEST", 2, 3, Statistical, S_PRANK, "Z.TEST(array, x, [sigma])", "One-tailed probability of a z-test.", z_test),
        f!("ZTEST", 2, 3, Compatibility, S_PRANK, "ZTEST(array, x, [sigma])", "One-tailed probability of a z-test.", z_test),
        f!("FISHER", 1, 1, Statistical, S, "FISHER(x)", "Fisher transformation.", fisher),
        f!("FISHERINV", 1, 1, Statistical, S, "FISHERINV(y)", "Inverse of the Fisher transformation.", |a: &[Arg], _c: C| num_val(num(a, 0)?.tanh())),
        f!("PHI", 1, 1, Statistical, S, "PHI(x)", "Standard normal density.", |a: &[Arg], _c: C| num_val(norm_pdf(num(a, 0)?))),
        f!("GAUSS", 1, 1, Statistical, S, "GAUSS(z)", "Probability that a standard normal value falls between the mean and z.", gauss),
    ]
}

#[cfg(test)]
mod tests {
    use super::{aggregate_values, aggregate_values_k};
    use crate::util::testutil::*;
    use gridcraft_core::{CellError, Value};

    const D7: f64 = 1e-6;

    fn c(xs: &[f64]) -> crate::Arg {
        av(col(xs))
    }

    #[test]
    fn averages_and_counts() {
        close(ev("AVERAGE", vec![n(1.0), n(2.0), n(6.0)]), 3.0);
        is_err(ev("AVERAGE", vec![rf(arr(vec![vec![tv("a")]]))]), CellError::Div0);
        let mixed = arr(vec![vec![nv(1.0), tv("x"), Value::Bool(true), Value::Empty, nv(3.0)]]);
        close(ev("AVERAGE", vec![rf(mixed.clone())]), 2.0);
        close(ev("AVERAGEA", vec![rf(mixed.clone())]), 1.25);
        close(ev("AVERAGE", vec![t("4"), b(true)]), 2.5);
        is_err(ev("AVERAGE", vec![t("abc")]), CellError::Value);
        is_err(ev("AVERAGE", vec![rf(arr(vec![vec![nv(1.0), Value::Error(CellError::NA)]]))]), CellError::NA);
        close(ev("COUNT", vec![rf(mixed.clone()), n(4.0), t("5"), t("x")]), 4.0);
        close(ev("COUNT", vec![e(CellError::Div0)]), 0.0);
        close(ev("COUNTA", vec![rf(mixed.clone())]), 4.0);
        close(ev("COUNTA", vec![n(1.0), t("")]), 2.0);
        let blanks = arr(vec![vec![Value::Empty, tv(""), nv(0.0), tv(" ")]]);
        close(ev("COUNTBLANK", vec![rf(blanks)]), 2.0);
        close(ev("MEDIAN", vec![c(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])]), 3.5);
        close(ev("MEDIAN", vec![c(&[3.0, 1.0, 2.0])]), 2.0);
        is_err(ev("MEDIAN", vec![rf(arr(vec![vec![tv("a")]]))]), CellError::Num);
        close(ev("MODE.SNGL", vec![c(&[5.6, 4.0, 4.0, 3.0, 2.0, 4.0])]), 4.0);
        close(ev("MODE", vec![c(&[1.0, 2.0, 2.0, 1.0])]), 1.0);
        is_err(ev("MODE", vec![c(&[1.0, 2.0, 3.0])]), CellError::NA);
        assert_eq!(rows_of(&ev("MODE.MULT", vec![c(&[1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 2.0, 3.0])])), vec![vec![nv(2.0)], vec![nv(3.0)]]);
    }

    #[test]
    fn spread() {
        let d = [1345.0, 1301.0, 1368.0, 1322.0, 1310.0, 1370.0, 1318.0, 1350.0, 1303.0, 1299.0];
        close_tol(ev("STDEV.S", vec![c(&d)]), 27.46391572, 1e-9);
        close_tol(ev("STDEV", vec![c(&d)]), 27.46391572, 1e-9);
        close_tol(ev("STDEV.P", vec![c(&d)]), 26.05455814, 1e-9);
        close_tol(ev("STDEVP", vec![c(&d)]), 26.05455814, 1e-9);
        close_tol(ev("VAR.S", vec![c(&d)]), 754.2666667, 1e-9);
        close(ev("VAR.P", vec![c(&d)]), 678.84);
        close(ev("VARP", vec![c(&d)]), 678.84);
        is_err(ev("STDEV.S", vec![n(1.0)]), CellError::Div0);
        close(ev("STDEV.P", vec![n(1.0)]), 0.0);
        let mixed = arr(vec![vec![nv(2.0), Value::Bool(true), tv("x")]]);
        close(ev("VARA", vec![rf(mixed.clone())]), 1.0);
        close(ev("VARPA", vec![rf(mixed.clone())]), 2.0 / 3.0);
        close(ev("STDEVA", vec![rf(mixed.clone())]), 1.0);
        close(ev("STDEVPA", vec![rf(mixed)]), (2.0f64 / 3.0).sqrt());
        let s = [4.0, 5.0, 6.0, 7.0, 5.0, 4.0, 3.0];
        close(ev("DEVSQ", vec![c(&[4.0, 5.0, 8.0, 7.0, 11.0, 4.0, 3.0])]), 48.0);
        close_tol(ev("AVEDEV", vec![c(&s)]), 1.020408163, 1e-9);
        let g = [4.0, 5.0, 8.0, 7.0, 11.0, 4.0, 3.0];
        close_tol(ev("GEOMEAN", vec![c(&g)]), 5.476986969, 1e-9);
        close_tol(ev("HARMEAN", vec![c(&g)]), 5.028375962, 1e-9);
        is_err(ev("GEOMEAN", vec![c(&[1.0, 0.0])]), CellError::Num);
        close_tol(ev("TRIMMEAN", vec![c(&[4.0, 5.0, 6.0, 7.0, 2.0, 3.0, 4.0, 5.0, 1.0, 2.0, 3.0]), n(0.2)]), 3.777777778, 1e-9);
        is_err(ev("TRIMMEAN", vec![c(&[1.0]), n(1.0)]), CellError::Num);
        let k = [3.0, 4.0, 5.0, 2.0, 3.0, 4.0, 5.0, 6.0, 4.0, 7.0];
        close_tol(ev("KURT", vec![c(&k)]), -0.151799637, 1e-8);
        close_tol(ev("SKEW", vec![c(&k)]), 0.359543071, 1e-8);
        close_tol(ev("SKEW.P", vec![c(&k)]), 0.303193339, 1e-8);
        is_err(ev("KURT", vec![c(&[1.0, 2.0, 3.0])]), CellError::Div0);
        close_tol(ev("STANDARDIZE", vec![n(42.0), n(40.0), n(1.5)]), 1.333333333, 1e-9);
        is_err(ev("STANDARDIZE", vec![n(42.0), n(40.0), n(0.0)]), CellError::Num);
    }

    #[test]
    fn min_max_order() {
        close(ev("MIN", vec![c(&[3.0, -1.0, 2.0]), n(0.0)]), -1.0);
        close(ev("MAX", vec![c(&[3.0, -1.0, 2.0]), n(10.0)]), 10.0);
        close(ev("MAX", vec![rf(arr(vec![vec![tv("a")]]))]), 0.0);
        close(ev("MAXA", vec![rf(arr(vec![vec![nv(-1.0), Value::Bool(true)]]))]), 1.0);
        close(ev("MINA", vec![rf(arr(vec![vec![nv(1.0), tv("x")]]))]), 0.0);
        let d = c(&[3.0, 5.0, 3.0, 5.0, 4.0, 4.0, 2.0, 4.0, 6.0, 7.0]);
        close(ev("LARGE", vec![d.clone(), n(3.0)]), 5.0);
        close(ev("SMALL", vec![d.clone(), n(4.0)]), 4.0);
        is_err(ev("LARGE", vec![d.clone(), n(0.0)]), CellError::Num);
        is_err(ev("SMALL", vec![d.clone(), n(11.0)]), CellError::Num);
        assert_eq!(rows_of(&ev("LARGE", vec![d, av(row(&[1.0, 2.0]))])), vec![vec![nv(7.0), nv(6.0)]]);
        let r = c(&[89.0, 88.0, 92.0, 101.0, 94.0, 97.0, 95.0]);
        close(ev("RANK.AVG", vec![n(94.0), r.clone()]), 4.0);
        close(ev("RANK.EQ", vec![n(101.0), r.clone()]), 1.0);
        is_err(ev("RANK", vec![n(1.0), r]), CellError::NA);
        let r2 = c(&[7.0, 3.5, 3.5, 1.0, 2.0]);
        close(ev("RANK.EQ", vec![n(7.0), r2.clone(), n(1.0)]), 5.0);
        close(ev("RANK.EQ", vec![n(3.5), r2.clone(), n(1.0)]), 3.0);
        close(ev("RANK.AVG", vec![n(3.5), r2, n(1.0)]), 3.5);
    }

    #[test]
    fn percentiles() {
        close(ev("PERCENTILE.INC", vec![c(&[1.0, 3.0, 2.0, 4.0]), n(0.3)]), 1.9);
        close(ev("PERCENTILE", vec![c(&[1.0, 3.0, 2.0, 4.0]), n(1.0)]), 4.0);
        is_err(ev("PERCENTILE.INC", vec![c(&[1.0]), n(1.5)]), CellError::Num);
        let e9 = c(&[1.0, 2.0, 3.0, 6.0, 6.0, 6.0, 7.0, 8.0, 9.0]);
        close(ev("PERCENTILE.EXC", vec![e9.clone(), n(0.25)]), 2.5);
        is_err(ev("PERCENTILE.EXC", vec![e9.clone(), n(0.0)]), CellError::Num);
        is_err(ev("PERCENTILE.EXC", vec![e9.clone(), n(0.01)]), CellError::Num);
        close(ev("QUARTILE.INC", vec![c(&[1.0, 2.0, 4.0, 7.0, 8.0, 9.0, 10.0, 12.0]), n(1.0)]), 3.5);
        close(ev("QUARTILE", vec![c(&[1.0, 2.0, 4.0, 7.0, 8.0, 9.0, 10.0, 12.0]), n(4.0)]), 12.0);
        let q = c(&[6.0, 7.0, 15.0, 36.0, 39.0, 40.0, 41.0, 42.0, 43.0, 47.0, 49.0]);
        close(ev("QUARTILE.EXC", vec![q.clone(), n(1.0)]), 15.0);
        close(ev("QUARTILE.EXC", vec![q.clone(), n(3.0)]), 43.0);
        is_err(ev("QUARTILE.EXC", vec![q, n(4.0)]), CellError::Num);
        let p = c(&[13.0, 12.0, 11.0, 8.0, 4.0, 3.0, 2.0, 1.0, 1.0, 1.0]);
        close(ev("PERCENTRANK.INC", vec![p.clone(), n(2.0)]), 0.333);
        close(ev("PERCENTRANK.INC", vec![p.clone(), n(4.0)]), 0.555);
        close(ev("PERCENTRANK.INC", vec![p.clone(), n(8.0)]), 0.666);
        close(ev("PERCENTRANK", vec![p.clone(), n(5.0)]), 0.583);
        close(ev("PERCENTRANK.INC", vec![p.clone(), n(5.0), n(1.0)]), 0.5);
        is_err(ev("PERCENTRANK.INC", vec![p.clone(), n(0.5)]), CellError::NA);
        is_err(ev("PERCENTRANK.INC", vec![p, n(5.0), n(0.0)]), CellError::Num);
        close(ev("PERCENTRANK.EXC", vec![e9.clone(), n(7.0)]), 0.7);
        close(ev("PERCENTRANK.EXC", vec![e9.clone(), n(5.43)]), 0.381);
        close(ev("PERCENTRANK.EXC", vec![e9, n(5.43), n(1.0)]), 0.3);
    }

    #[test]
    fn pairs_and_regression() {
        close_tol(ev("CORREL", vec![c(&[3.0, 2.0, 4.0, 5.0, 6.0]), c(&[9.0, 7.0, 12.0, 15.0, 17.0])]), 0.997054486, 1e-9);
        close_tol(ev("PEARSON", vec![c(&[9.0, 7.0, 5.0, 3.0, 1.0]), c(&[10.0, 6.0, 1.0, 5.0, 3.0])]), 0.699379, D7);
        is_err(ev("CORREL", vec![c(&[1.0, 2.0]), c(&[1.0, 2.0, 3.0])]), CellError::NA);
        is_err(ev("CORREL", vec![c(&[1.0, 1.0]), c(&[1.0, 2.0])]), CellError::Div0);
        let y = c(&[2.0, 3.0, 9.0, 1.0, 8.0, 7.0, 5.0]);
        let x = c(&[6.0, 5.0, 11.0, 7.0, 5.0, 4.0, 4.0]);
        close_tol(ev("SLOPE", vec![y.clone(), x.clone()]), 0.305555556, 1e-8);
        close_tol(ev("RSQ", vec![y.clone(), x.clone()]), 0.057950192, 1e-8);
        close_tol(ev("STEYX", vec![y.clone(), x.clone()]), 3.305718950, 1e-8);
        close_tol(ev("INTERCEPT", vec![c(&[2.0, 3.0, 9.0, 1.0, 8.0]), c(&[6.0, 5.0, 11.0, 7.0, 5.0])]), 0.048387097, 1e-8);
        close_tol(ev("FORECAST", vec![n(30.0), c(&[6.0, 7.0, 9.0, 15.0, 21.0]), c(&[20.0, 28.0, 31.0, 38.0, 40.0])]), 10.607253, D7);
        close_tol(ev("FORECAST.LINEAR", vec![n(30.0), c(&[6.0, 7.0, 9.0, 15.0, 21.0]), c(&[20.0, 28.0, 31.0, 38.0, 40.0])]), 10.607253, D7);
        close(ev("COVARIANCE.P", vec![c(&[3.0, 2.0, 4.0, 5.0, 6.0]), c(&[9.0, 7.0, 12.0, 15.0, 17.0])]), 5.2);
        close(ev("COVAR", vec![c(&[3.0, 2.0, 4.0, 5.0, 6.0]), c(&[9.0, 7.0, 12.0, 15.0, 17.0])]), 5.2);
        close(ev("COVARIANCE.S", vec![c(&[2.0, 4.0, 8.0]), c(&[5.0, 11.0, 12.0])]), 29.0 / 3.0);
        // Pairs with text are skipped.
        let xs = arr(vec![vec![nv(1.0)], vec![tv("x")], vec![nv(3.0)]]);
        close(ev("SLOPE", vec![c(&[2.0, 100.0, 6.0]), rf(xs)]), 2.0);
    }

    #[test]
    fn linest_family() {
        let r = rows_of(&ev("LINEST", vec![c(&[1.0, 9.0, 5.0, 7.0]), c(&[0.0, 4.0, 2.0, 3.0])]));
        assert_eq!(r.len(), 1);
        close(r[0][0].clone(), 2.0);
        close(r[0][1].clone(), 1.0);
        // Statistics for a simple fit: y = 1,2,4 on x = 1,2,3.
        let s = rows_of(&ev("LINEST", vec![c(&[1.0, 2.0, 4.0]), c(&[1.0, 2.0, 3.0]), b(true), b(true)]));
        assert_eq!((s.len(), s[0].len()), (5, 2));
        close(s[0][0].clone(), 1.5);
        close(s[0][1].clone(), -2.0 / 3.0);
        close(s[2][0].clone(), 0.9642857142857143); // r²
        close(s[3][1].clone(), 1.0); // df
        close(s[4][0].clone(), 4.5); // ssreg
        close(s[4][1].clone(), 1.0 / 6.0); // ssresid
        close(s[2][1].clone(), (1.0f64 / 6.0).sqrt()); // sey
        close(s[3][0].clone(), 27.0); // F
        close(s[1][0].clone(), (1.0f64 / 6.0 / 2.0).sqrt()); // se slope
        // No constant.
        let z = rows_of(&ev("LINEST", vec![c(&[2.0, 4.0, 6.0]), c(&[1.0, 2.0, 3.0]), b(false)]));
        close(z[0][0].clone(), 2.0);
        close(z[0][1].clone(), 0.0);
        // Two predictors: y = 1 + 2a + 3b.
        let xs = arr(vec![vec![nv(1.0), nv(0.0)], vec![nv(0.0), nv(1.0)], vec![nv(1.0), nv(1.0)], vec![nv(2.0), nv(1.0)], vec![nv(3.0), nv(5.0)]]);
        let ys: Vec<f64> = [(1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (3.0, 5.0)].iter().map(|(a, b)| 1.0 + 2.0 * a + 3.0 * b).collect();
        let m = rows_of(&ev("LINEST", vec![c(&ys), av(xs)]));
        close(m[0][0].clone(), 3.0);
        close(m[0][1].clone(), 2.0);
        close(m[0][2].clone(), 1.0);
        let gy = [33100.0, 47300.0, 69000.0, 102000.0, 150000.0, 220000.0];
        let gx = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0];
        let l = rows_of(&ev("LOGEST", vec![c(&gy), c(&gx)]));
        close_tol(l[0][0].clone(), 1.463275628, 1e-9);
        close_tol(l[0][1].clone(), 495.3047702, 1e-9);
        let g = rows_of(&ev("GROWTH", vec![c(&gy), c(&gx), c(&[17.0, 18.0])]));
        close_tol(g[0][0].clone(), 320196.7184, 1e-9);
        close_tol(g[1][0].clone(), 468536.0539, 1e-9);
        let ty = [133890.0, 135000.0, 135790.0, 137300.0, 138130.0, 139100.0, 139900.0, 141120.0, 141890.0, 143230.0, 144000.0, 145290.0];
        let tx: Vec<f64> = (1..=12).map(f64::from).collect();
        let t1 = ev("TREND", vec![c(&ty), c(&tx), n(13.0)]);
        close_tol(Value::Number(rows_of(&t1)[0][0].as_f64().unwrap().round()), 146172.0, 1e-9);
        let fitted = rows_of(&ev("TREND", vec![c(&[1.0, 9.0, 5.0, 7.0]), c(&[0.0, 4.0, 2.0, 3.0])]));
        close(fitted[1][0].clone(), 9.0);
        is_err(ev("LOGEST", vec![c(&[1.0, -2.0])]), CellError::Num);
        is_err(ev("LINEST", vec![c(&[1.0, 2.0]), c(&[1.0, 2.0, 3.0])]), CellError::Ref);
    }

    #[test]
    fn frequency_prob() {
        let f = ev("FREQUENCY", vec![c(&[79.0, 85.0, 78.0, 85.0, 50.0, 81.0, 95.0, 88.0, 97.0]), c(&[70.0, 79.0, 89.0])]);
        assert_eq!(rows_of(&f), vec![vec![nv(1.0)], vec![nv(2.0)], vec![nv(4.0)], vec![nv(2.0)]]);
        let x = c(&[0.0, 1.0, 2.0, 3.0]);
        let p = c(&[0.2, 0.3, 0.1, 0.4]);
        close(ev("PROB", vec![x.clone(), p.clone(), n(2.0)]), 0.1);
        close(ev("PROB", vec![x.clone(), p, n(1.0), n(3.0)]), 0.8);
        is_err(ev("PROB", vec![x, c(&[0.5, 0.5, 0.5, 0.5]), n(1.0)]), CellError::Num);
    }

    #[test]
    fn normal() {
        close_tol(ev("NORM.DIST", vec![n(42.0), n(40.0), n(1.5), b(true)]), 0.9087888, D7);
        close_tol(ev("NORM.DIST", vec![n(42.0), n(40.0), n(1.5), b(false)]), 0.10934005, D7);
        close_tol(ev("NORMDIST", vec![n(42.0), n(40.0), n(1.5), b(true)]), 0.9087888, D7);
        is_err(ev("NORM.DIST", vec![n(42.0), n(40.0), n(0.0), b(true)]), CellError::Num);
        close_tol(ev("NORM.INV", vec![n(0.908789), n(40.0), n(1.5)]), 42.000002, D7);
        close_tol(ev("NORMINV", vec![n(0.908789), n(40.0), n(1.5)]), 42.000002, D7);
        is_err(ev("NORM.INV", vec![n(1.0), n(0.0), n(1.0)]), CellError::Num);
        close_tol(ev("NORM.S.DIST", vec![n(1.333333), b(true)]), 0.908788726, 1e-8);
        close_tol(ev("NORM.S.DIST", vec![n(0.0), b(false)]), 0.398942280, 1e-8);
        close_tol(ev("NORMSDIST", vec![n(1.333333)]), 0.908788726, 1e-8);
        close_tol(ev("NORM.S.INV", vec![n(0.908789)]), 1.3333347, D7);
        close_tol(ev("NORMSINV", vec![n(0.975)]), 1.959963985, 1e-9);
        close_tol(ev("PHI", vec![n(0.75)]), 0.301137432, 1e-8);
        close_tol(ev("GAUSS", vec![n(2.0)]), 0.477249868, 1e-8);
        close_tol(ev("FISHER", vec![n(0.75)]), 0.972955075, 1e-8);
        close_tol(ev("FISHERINV", vec![n(0.972955)]), 0.75, D7);
        is_err(ev("FISHER", vec![n(1.0)]), CellError::Num);
        close_tol(ev("CONFIDENCE.NORM", vec![n(0.05), n(2.5), n(50.0)]), 0.692951912, 1e-8);
        close_tol(ev("CONFIDENCE", vec![n(0.05), n(2.5), n(50.0)]), 0.692951912, 1e-8);
        close_tol(ev("CONFIDENCE.T", vec![n(0.05), n(1.0), n(50.0)]), 0.284196855, 1e-8);
        is_err(ev("CONFIDENCE.T", vec![n(0.05), n(1.0), n(1.0)]), CellError::Div0);
        let z = c(&[3.0, 6.0, 7.0, 8.0, 6.0, 5.0, 4.0, 2.0, 1.0, 9.0]);
        close_tol(ev("Z.TEST", vec![z.clone(), n(4.0)]), 0.090574, 1e-5);
        close_tol(ev("ZTEST", vec![z, n(6.0)]), 0.863043, 1e-5);
        close_tol(ev("LOGNORM.DIST", vec![n(4.0), n(3.5), n(1.2), b(true)]), 0.0390836, D7);
        close_tol(ev("LOGNORM.DIST", vec![n(4.0), n(3.5), n(1.2), b(false)]), 0.0176176, D7);
        close_tol(ev("LOGNORMDIST", vec![n(4.0), n(3.5), n(1.2)]), 0.0390836, D7);
        close_tol(ev("LOGNORM.INV", vec![n(0.039084), n(3.5), n(1.2)]), 4.0000252, D7);
        close_tol(ev("LOGINV", vec![n(0.039084), n(3.5), n(1.2)]), 4.0000252, D7);
    }

    #[test]
    fn t_chi_f() {
        close_tol(ev("T.DIST", vec![n(60.0), n(1.0), b(true)]), 0.99469533, D7);
        close_tol(ev("T.DIST", vec![n(8.0), n(3.0), b(false)]), 0.00073691, 1e-5);
        close_tol(ev("T.DIST.2T", vec![n(1.959999998), n(60.0)]), 0.054644930, D7);
        close_tol(ev("T.DIST.RT", vec![n(1.959999998), n(60.0)]), 0.027322465, D7);
        close_tol(ev("TDIST", vec![n(1.959999998), n(60.0), n(2.0)]), 0.054644930, D7);
        is_err(ev("TDIST", vec![n(-1.0), n(60.0), n(2.0)]), CellError::Num);
        is_err(ev("T.DIST", vec![n(1.0), n(0.0), b(true)]), CellError::Num);
        close_tol(ev("T.INV", vec![n(0.75), n(2.0)]), 0.8164966, D7);
        close_tol(ev("T.INV", vec![n(0.25), n(2.0)]), -0.8164966, D7);
        close_tol(ev("T.INV.2T", vec![n(0.546449), n(60.0)]), 0.606533, D7);
        close_tol(ev("T.INV.2T", vec![n(0.05), n(10.0)]), 2.228138852, 1e-9);
        close_tol(ev("TINV", vec![n(0.05), n(10.0)]), 2.228138852, 1e-9);
        let a1 = c(&[3.0, 4.0, 5.0, 8.0, 9.0, 1.0, 2.0, 4.0, 5.0]);
        let a2 = c(&[6.0, 19.0, 3.0, 2.0, 14.0, 4.0, 5.0, 17.0, 1.0]);
        close_tol(ev("T.TEST", vec![a1.clone(), a2.clone(), n(2.0), n(1.0)]), 0.196016, 1e-5);
        close_tol(ev("TTEST", vec![a1.clone(), a2.clone(), n(2.0), n(1.0)]), 0.196016, 1e-5);
        close_tol(ev("T.TEST", vec![a1.clone(), a2.clone(), n(2.0), n(2.0)]), 0.191995887, 1e-6);
        close_tol(ev("T.TEST", vec![a1.clone(), a2.clone(), n(1.0), n(3.0)]), 0.1012, 1e-3);
        is_err(ev("T.TEST", vec![a1, a2, n(3.0), n(1.0)]), CellError::Num);
        close_tol(ev("CHISQ.DIST", vec![n(0.5), n(1.0), b(true)]), 0.52049988, D7);
        close_tol(ev("CHISQ.DIST", vec![n(2.0), n(3.0), b(false)]), 0.20755375, D7);
        close_tol(ev("CHISQ.DIST.RT", vec![n(18.307), n(10.0)]), 0.0500006, 1e-5);
        close_tol(ev("CHIDIST", vec![n(18.307), n(10.0)]), 0.0500006, 1e-5);
        close_tol(ev("CHISQ.INV", vec![n(0.93), n(1.0)]), 3.283020287, 1e-8);
        close_tol(ev("CHISQ.INV", vec![n(0.6), n(2.0)]), 1.832581464, 1e-8);
        close_tol(ev("CHISQ.INV.RT", vec![n(0.050001), n(10.0)]), 18.30697, D7);
        close_tol(ev("CHIINV", vec![n(0.050001), n(10.0)]), 18.30697, D7);
        is_err(ev("CHISQ.DIST", vec![n(-1.0), n(1.0), b(true)]), CellError::Num);
        let act = arr(vec![vec![nv(58.0), nv(35.0)], vec![nv(11.0), nv(25.0)], vec![nv(10.0), nv(23.0)]]);
        let exp = arr(vec![vec![nv(45.35), nv(47.65)], vec![nv(17.56), nv(18.44)], vec![nv(16.09), nv(16.91)]]);
        close_tol(ev("CHISQ.TEST", vec![av(act.clone()), av(exp.clone())]), 0.0003082, 1e-3);
        close_tol(ev("CHITEST", vec![av(act), av(exp)]), 0.0003082, 1e-3);
        close_tol(ev("F.DIST", vec![n(15.2069), n(6.0), n(4.0), b(true)]), 0.99, 1e-6);
        close_tol(ev("F.DIST", vec![n(15.2069), n(6.0), n(4.0), b(false)]), 0.0012238, 1e-5);
        close_tol(ev("F.DIST.RT", vec![n(15.2069), n(6.0), n(4.0)]), 0.01, 1e-6);
        close_tol(ev("FDIST", vec![n(15.2069), n(6.0), n(4.0)]), 0.01, 1e-6);
        close_tol(ev("F.INV", vec![n(0.01), n(6.0), n(4.0)]), 0.10930991, D7);
        close_tol(ev("F.INV.RT", vec![n(0.01), n(6.0), n(4.0)]), 15.20686, D7);
        close_tol(ev("FINV", vec![n(0.01), n(6.0), n(4.0)]), 15.20686, D7);
        close_tol(ev("F.TEST", vec![c(&[6.0, 7.0, 9.0, 15.0, 21.0]), c(&[20.0, 28.0, 31.0, 38.0, 40.0])]), 0.64831785, D7);
        close_tol(ev("FTEST", vec![c(&[6.0, 7.0, 9.0, 15.0, 21.0]), c(&[20.0, 28.0, 31.0, 38.0, 40.0])]), 0.64831785, D7);
        is_err(ev("F.DIST", vec![n(1.0), n(0.0), n(4.0), b(true)]), CellError::Num);
    }

    #[test]
    fn discrete() {
        close_tol(ev("BINOM.DIST", vec![n(6.0), n(10.0), n(0.5), b(false)]), 0.205078125, 1e-12);
        close_tol(ev("BINOM.DIST", vec![n(6.0), n(10.0), n(0.5), b(true)]), 0.828125, 1e-12);
        close_tol(ev("BINOMDIST", vec![n(6.0), n(10.0), n(0.5), b(false)]), 0.205078125, 1e-12);
        is_err(ev("BINOM.DIST", vec![n(11.0), n(10.0), n(0.5), b(false)]), CellError::Num);
        close_tol(ev("BINOM.DIST.RANGE", vec![n(60.0), n(0.75), n(48.0)]), 0.083974967, 1e-6);
        close_tol(ev("BINOM.DIST.RANGE", vec![n(60.0), n(0.75), n(45.0), n(50.0)]), 0.523629793, 1e-6);
        close(ev("BINOM.INV", vec![n(6.0), n(0.5), n(0.75)]), 4.0);
        close(ev("CRITBINOM", vec![n(6.0), n(0.5), n(0.75)]), 4.0);
        close(ev("BINOM.INV", vec![n(1e9), n(0.5), n(0.5)]), 5e8);
        close_tol(ev("POISSON.DIST", vec![n(2.0), n(5.0), b(true)]), 0.124652, 1e-5);
        close_tol(ev("POISSON.DIST", vec![n(2.0), n(5.0), b(false)]), 0.084224, 1e-5);
        close_tol(ev("POISSON", vec![n(2.0), n(5.0), b(false)]), 0.084224, 1e-5);
        is_err(ev("POISSON.DIST", vec![n(-1.0), n(5.0), b(false)]), CellError::Num);
        close_tol(ev("HYPGEOM.DIST", vec![n(1.0), n(4.0), n(8.0), n(20.0), b(true)]), 0.4654, 1e-4);
        close_tol(ev("HYPGEOM.DIST", vec![n(1.0), n(4.0), n(8.0), n(20.0), b(false)]), 0.3633, 1e-4);
        close_tol(ev("HYPGEOMDIST", vec![n(1.0), n(4.0), n(8.0), n(20.0)]), 0.363261094, 1e-8);
        is_err(ev("HYPGEOMDIST", vec![n(5.0), n(4.0), n(8.0), n(20.0)]), CellError::Num);
        close_tol(ev("NEGBINOM.DIST", vec![n(10.0), n(5.0), n(0.25), b(true)]), 0.3135141, D7);
        close_tol(ev("NEGBINOM.DIST", vec![n(10.0), n(5.0), n(0.25), b(false)]), 0.0550487, D7);
        close_tol(ev("NEGBINOMDIST", vec![n(10.0), n(5.0), n(0.25)]), 0.0550487, D7);
    }

    #[test]
    fn continuous() {
        close_tol(ev("EXPON.DIST", vec![n(0.2), n(10.0), b(true)]), 0.86466472, D7);
        close_tol(ev("EXPON.DIST", vec![n(0.2), n(10.0), b(false)]), 1.35335283, D7);
        close_tol(ev("EXPONDIST", vec![n(0.2), n(10.0), b(true)]), 0.86466472, D7);
        is_err(ev("EXPON.DIST", vec![n(0.2), n(0.0), b(true)]), CellError::Num);
        close_tol(ev("GAMMA.DIST", vec![n(10.00001131), n(9.0), n(2.0), b(false)]), 0.032639, 1e-5);
        close_tol(ev("GAMMA.DIST", vec![n(10.00001131), n(9.0), n(2.0), b(true)]), 0.068094, 1e-5);
        close_tol(ev("GAMMADIST", vec![n(10.00001131), n(9.0), n(2.0), b(true)]), 0.068094, 1e-5);
        close_tol(ev("GAMMA.INV", vec![n(0.068094), n(9.0), n(2.0)]), 10.0000112, 1e-6);
        close_tol(ev("GAMMAINV", vec![n(0.068094), n(9.0), n(2.0)]), 10.0000112, 1e-6);
        close_tol(ev("GAMMALN", vec![n(4.0)]), 1.791759469, 1e-9);
        close_tol(ev("GAMMALN.PRECISE", vec![n(4.5)]), 2.453736571, 1e-9);
        is_err(ev("GAMMALN", vec![n(0.0)]), CellError::Num);
        close_tol(ev("GAMMA", vec![n(2.5)]), 1.329340388, 1e-9);
        close_tol(ev("GAMMA", vec![n(-0.75)]), -4.834146544, 1e-8);
        is_err(ev("GAMMA", vec![n(-1.0)]), CellError::Num);
        is_err(ev("GAMMA", vec![n(0.0)]), CellError::Num);
        close_tol(ev("BETA.DIST", vec![n(2.0), n(8.0), n(10.0), b(true), n(1.0), n(3.0)]), 0.6854706, D7);
        close_tol(ev("BETA.DIST", vec![n(2.0), n(8.0), n(10.0), b(false), n(1.0), n(3.0)]), 1.4837646, D7);
        close_tol(ev("BETADIST", vec![n(2.0), n(8.0), n(10.0), n(1.0), n(3.0)]), 0.6854706, D7);
        close_tol(ev("BETA.INV", vec![n(0.685470581), n(8.0), n(10.0), n(1.0), n(3.0)]), 2.0, D7);
        close_tol(ev("BETAINV", vec![n(0.685470581), n(8.0), n(10.0), n(1.0), n(3.0)]), 2.0, D7);
        is_err(ev("BETA.DIST", vec![n(4.0), n(8.0), n(10.0), b(true), n(1.0), n(3.0)]), CellError::Num);
        close_tol(ev("WEIBULL.DIST", vec![n(105.0), n(20.0), n(100.0), b(true)]), 0.929581, 1e-5);
        close_tol(ev("WEIBULL.DIST", vec![n(105.0), n(20.0), n(100.0), b(false)]), 0.035589, 1e-4);
        close_tol(ev("WEIBULL", vec![n(105.0), n(20.0), n(100.0), b(true)]), 0.929581, 1e-5);
        // Lifting over an array of x values.
        let r = rows_of(&ev("NORM.S.DIST", vec![av(row(&[0.0, 1.0])), b(true)]));
        close(r[0][0].clone(), 0.5);
        close_tol(r[0][1].clone(), 0.841344746, 1e-8);
    }

    #[test]
    fn aggregate_helper() {
        let vals = vec![nv(1.0), nv(2.0), tv("x"), Value::Empty, Value::Bool(true), nv(6.0)];
        close(aggregate_values(1, &vals), 3.0);
        close(aggregate_values(2, &vals), 3.0);
        close(aggregate_values(3, &vals), 5.0);
        close(aggregate_values(4, &vals), 6.0);
        close(aggregate_values(105, &vals), 1.0);
        close(aggregate_values(6, &vals), 12.0);
        close(aggregate_values(9, &vals), 9.0);
        close(aggregate_values(109, &vals), 9.0);
        close(aggregate_values(11, &vals), 14.0 / 3.0);
        close(aggregate_values(10, &vals), 7.0);
        close(aggregate_values(12, &vals), 2.0);
        is_err(aggregate_values(13, &vals), CellError::NA);
        is_err(aggregate_values(14, &vals), CellError::Value);
        is_err(aggregate_values(99, &vals), CellError::Value);
        is_err(aggregate_values(9, &[nv(1.0), Value::Error(CellError::Div0)]), CellError::Div0);
        close(aggregate_values_k(14, &vals, &nv(1.0)), 6.0);
        close(aggregate_values_k(15, &vals, &nv(2.0)), 2.0);
        close(aggregate_values_k(16, &vals, &nv(0.5)), 2.0);
        close(aggregate_values_k(17, &vals, &nv(4.0)), 6.0);
        close(aggregate_values_k(18, &vals, &nv(0.5)), 2.0);
        close(aggregate_values_k(19, &vals, &nv(2.0)), 2.0);
        close(aggregate_values_k(9, &vals, &nv(2.0)), 9.0);
        is_err(aggregate_values_k(14, &vals, &nv(10.0)), CellError::Num);
    }
}
