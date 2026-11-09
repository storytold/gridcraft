use super::descriptive::{arg_pairs, intercept, moments, slope};
use super::{Arg, C, CellError, R, Value, arg, array_val, as_array, boolean, has, num, num_val};

pub(super) fn forecast(a: &[Arg], _: C) -> R<Value> {
    let x = num(a, 0)?;
    let (ys, xs) = arg_pairs(a, 1, 2)?;
    if ys.is_empty() {
        return Err(CellError::Div0);
    }
    num_val(intercept(&ys, &xs)? + slope(&ys, &xs)? * x)
}

pub(super) fn steyx(a: &[Arg], _: C) -> R<Value> {
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

pub(super) fn covariance(a: &[Arg], sample: bool) -> R<Value> {
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

/// Most predictor columns LINEST, LOGEST, TREND and GROWTH accept. The normal equations are a
/// k×k matrix inverted in O(k³), so an unbounded k (one `SEQUENCE(1,16384)` argument) allocated
/// gigabytes and could abort the process.
pub(super) const MAX_REGRESSORS: usize = 255;

/// Most multiply-adds (rows × k²) a regression may spend building its normal equations.
const MAX_REGRESSION_WORK: usize = 1_000_000_000;

/// Rejects regressions too large to fit: more than [`MAX_REGRESSORS`] predictors is `#VALUE!`,
/// more than [`MAX_REGRESSION_WORK`] multiply-adds is `#NUM!`.
pub(super) fn check_regression_size(n: usize, k: usize) -> R<()> {
    if k > MAX_REGRESSORS {
        return Err(CellError::Value);
    }
    match n.checked_mul(k).and_then(|nk| nk.checked_mul(k)) {
        Some(work) if work <= MAX_REGRESSION_WORK => Ok(()),
        _ => Err(CellError::Num),
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
                check_regression_size(n, xa.cols)?;
                let mut rows = Vec::with_capacity(n);
                for r in 0..n {
                    rows.push((0..xa.cols).map(|c| strict_num(xa.get(r, c).unwrap_or(&Value::Empty))).collect::<R<Vec<_>>>()?);
                }
                (rows, xa.cols)
            } else if y_is_row && xa.cols == n {
                check_regression_size(n, xa.rows)?;
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
    check_regression_size(n, k)?;
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

pub(super) fn linest_impl(a: &[Arg], log: bool) -> R<Value> {
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

pub(super) fn trend_impl(a: &[Arg], log: bool) -> R<Value> {
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
