use super::problem::Evaluator;
use super::*;

// ---------------------------------------------------------------- evaluation

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Sense {
    Le,
    Ge,
    Eq,
}

/// A scalar constraint `lhs (sense) rhs`.
#[derive(Clone, Debug)]
pub(super) struct Row {
    pub(super) lhs: CellRef,
    pub(super) sense: Sense,
    pub(super) rhs: Option<CellRef>,
    pub(super) rhs_const: f64,
}

/// What a relaxation (or the whole solve) ended with.
#[derive(Clone, Debug)]
pub(super) enum Outcome {
    /// x, objective to minimise, and whether the solver converged (vs. ran out of budget).
    Solved(Vec<f64>, f64, bool),
    Infeasible,
    Unbounded,
    Limit,
}

/// Depth-first branch and bound over a relaxation solver; returns the best integer point.
pub(super) fn branch_and_bound(
    lo: &[f64],
    hi: &[f64],
    integer: &[bool],
    max_nodes: usize,
    gap: f64,
    relax: &mut dyn FnMut(&[f64], &[f64]) -> Outcome,
) -> (Outcome, usize) {
    let mut stack: Vec<(Vec<f64>, Vec<f64>)> = vec![(lo.to_vec(), hi.to_vec())];
    let mut best: Option<(Vec<f64>, f64)> = None;
    let mut nodes = 0usize;
    let mut limited = false;
    let mut root = true;
    while let Some((l, h)) = stack.pop() {
        if nodes >= max_nodes {
            limited = true;
            break;
        }
        nodes += 1;
        let out = relax(&l, &h);
        let was_root = std::mem::replace(&mut root, false);
        let (x, obj, converged) = match out {
            Outcome::Solved(x, obj, c) => (x, obj, c),
            Outcome::Unbounded if was_root => return (Outcome::Unbounded, nodes),
            Outcome::Limit => {
                limited = true;
                break;
            }
            _ => continue,
        };
        if !converged {
            limited = true;
        }
        if let Some((_, b)) = &best
            && obj >= b - gap * b.abs().max(1e-12) - 1e-9 * (1.0 + b.abs())
        {
            continue;
        }
        // The most fractional integer variable.
        let frac = x
            .iter()
            .zip(integer)
            .enumerate()
            .filter(|(_, (_, int))| **int)
            .map(|(j, (v, _))| (j, (v - v.round()).abs()))
            .filter(|(_, f)| *f > 1e-6)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match frac {
            None => {
                let snapped: Vec<f64> = x.iter().zip(integer).map(|(v, int)| if *int { v.round() } else { *v }).collect();
                best = Some((snapped, obj));
            }
            Some((j, _)) => {
                let v = x.get(j).copied().unwrap_or(0.0);
                let (mut down_h, mut up_l) = (h.clone(), l.clone());
                if let Some(e) = down_h.get_mut(j) {
                    *e = e.min(v.floor());
                }
                if let Some(e) = up_l.get_mut(j) {
                    *e = e.max(v.ceil());
                }
                // Explore the nearer side first (pushed last).
                if v - v.floor() < 0.5 {
                    stack.push((up_l, h));
                    stack.push((l, down_h));
                } else {
                    stack.push((l, down_h));
                    stack.push((up_l, h));
                }
            }
        }
    }
    match best {
        Some((x, obj)) => (Outcome::Solved(x, obj, !limited), nodes),
        None if limited => (Outcome::Limit, nodes),
        None => (Outcome::Infeasible, nodes),
    }
}

// ---------------------------------------------------------------- simplex

/// Pivots the tableau on (r, j), updating the objective row.
fn pivot(t: &mut [Vec<f64>], obj: &mut [f64], r: usize, j: usize) {
    let Some(prow) = t.get_mut(r) else { return };
    let pv = prow.get(j).copied().unwrap_or(0.0);
    if pv == 0.0 {
        return;
    }
    for v in prow.iter_mut() {
        *v /= pv;
    }
    let prow = prow.clone();
    for (i, row) in t.iter_mut().enumerate() {
        if i == r {
            continue;
        }
        let f = row.get(j).copied().unwrap_or(0.0);
        if f != 0.0 {
            for (v, p) in row.iter_mut().zip(&prow) {
                *v -= f * p;
            }
        }
    }
    let f = obj.get(j).copied().unwrap_or(0.0);
    if f != 0.0 {
        for (v, p) in obj.iter_mut().zip(&prow) {
            *v -= f * p;
        }
    }
}

enum Step {
    Optimal,
    Unbounded,
    Limit,
}

/// Minimises with the reduced costs in `obj` (last entry: −objective). Columns not `allowed`
/// never enter.
fn simplex(t: &mut [Vec<f64>], basis: &mut [usize], obj: &mut [f64], allowed: &[bool], budget: &mut u64) -> Step {
    let n = obj.len().saturating_sub(1);
    let cost = (t.len() as u64 + 1).saturating_mul(n as u64 + 1).max(1);
    let mut stalled = 0u32;
    loop {
        if *budget < cost {
            return Step::Limit;
        }
        *budget -= cost;
        let bland = stalled > 50;
        let mut enter = None;
        let mut best = -EPS;
        for (j, d) in obj.iter().take(n).enumerate() {
            if !allowed.get(j).copied().unwrap_or(false) || *d >= -EPS {
                continue;
            }
            if bland {
                enter = Some(j);
                break;
            }
            if *d < best {
                best = *d;
                enter = Some(j);
            }
        }
        let Some(j) = enter else { return Step::Optimal };
        let mut leave: Option<(usize, f64)> = None;
        for (i, row) in t.iter().enumerate() {
            let a = row.get(j).copied().unwrap_or(0.0);
            if a <= EPS {
                continue;
            }
            let ratio = row.get(n).copied().unwrap_or(0.0) / a;
            let better = match leave {
                None => true,
                Some((li, lr)) => ratio < lr - 1e-12 || (ratio <= lr + 1e-12 && basis.get(i) < basis.get(li)),
            };
            if better {
                leave = Some((i, ratio));
            }
        }
        let Some((r, ratio)) = leave else { return Step::Unbounded };
        stalled = if ratio <= EPS { stalled + 1 } else { 0 };
        pivot(t, obj, r, j);
        if let Some(b) = basis.get_mut(r) {
            *b = j;
        }
    }
}

/// Minimises `c·x` subject to `rows` (coefficients, sense, rhs) and `lo ≤ x ≤ hi`.
pub(super) fn solve_lp(c: &[f64], rows: &[(Vec<f64>, Sense, f64)], lo: &[f64], hi: &[f64], budget: &mut u64) -> Outcome {
    let n = c.len();
    if lo.iter().zip(hi).any(|(l, h)| l > h) {
        return Outcome::Infeasible;
    }
    // x_j = offset + Σ sign·y_k with y ≥ 0.
    struct Map {
        var: usize,
        sign: f64,
    }
    let mut cols: Vec<Map> = Vec::new();
    let mut offset = vec![0.0; n];
    let mut extra: Vec<(Vec<(usize, f64)>, Sense, f64)> = Vec::new();
    for j in 0..n {
        let (l, h) = (lo.get(j).copied().unwrap_or(f64::NEG_INFINITY), hi.get(j).copied().unwrap_or(f64::INFINITY));
        if l.is_finite() {
            if let Some(o) = offset.get_mut(j) {
                *o = l;
            }
            cols.push(Map { var: j, sign: 1.0 });
            if h.is_finite() {
                extra.push((vec![(cols.len() - 1, 1.0)], Sense::Le, h - l));
            }
        } else if h.is_finite() {
            if let Some(o) = offset.get_mut(j) {
                *o = h;
            }
            cols.push(Map { var: j, sign: -1.0 });
        } else {
            cols.push(Map { var: j, sign: 1.0 });
            cols.push(Map { var: j, sign: -1.0 });
        }
    }
    let ny = cols.len();
    // Rows over y: (coefficients, sense, rhs).
    let mut yrows: Vec<(Vec<f64>, Sense, f64)> = Vec::with_capacity(rows.len() + extra.len());
    for (a, sense, b) in rows {
        let mut coef = vec![0.0; ny];
        for (k, m) in cols.iter().enumerate() {
            if let Some(v) = coef.get_mut(k) {
                *v = a.get(m.var).copied().unwrap_or(0.0) * m.sign;
            }
        }
        let shift: f64 = a.iter().zip(&offset).map(|(x, o)| x * o).sum();
        yrows.push((coef, *sense, b - shift));
    }
    for (entries, sense, b) in extra {
        let mut coef = vec![0.0; ny];
        for (k, v) in entries {
            if let Some(e) = coef.get_mut(k) {
                *e = v;
            }
        }
        yrows.push((coef, sense, b));
    }
    let m = yrows.len();
    let n_slack = yrows.iter().filter(|r| r.1 != Sense::Eq).count();
    // Rows with b < 0 are negated so every right side is ≥ 0.
    let normalized: Vec<(Vec<f64>, Sense, f64)> = yrows
        .into_iter()
        .map(|(a, s, b)| {
            if b < 0.0 {
                let s = match s {
                    Sense::Le => Sense::Ge,
                    Sense::Ge => Sense::Le,
                    Sense::Eq => Sense::Eq,
                };
                (a.iter().map(|v| -v).collect(), s, -b)
            } else {
                (a, s, b)
            }
        })
        .collect();
    let n_art = normalized.iter().filter(|r| r.1 != Sense::Le).count();
    let width = ny + n_slack + n_art;
    let mut t: Vec<Vec<f64>> = Vec::with_capacity(m);
    let mut basis = vec![0usize; m];
    let (mut si, mut ai) = (ny, ny + n_slack);
    for (i, (a, s, b)) in normalized.iter().enumerate() {
        let mut row = vec![0.0; width + 1];
        for (v, x) in row.iter_mut().zip(a) {
            *v = *x;
        }
        match s {
            Sense::Le => {
                if let Some(v) = row.get_mut(si) {
                    *v = 1.0;
                }
                if let Some(bi) = basis.get_mut(i) {
                    *bi = si;
                }
                si += 1;
            }
            Sense::Ge => {
                if let Some(v) = row.get_mut(si) {
                    *v = -1.0;
                }
                si += 1;
                if let Some(v) = row.get_mut(ai) {
                    *v = 1.0;
                }
                if let Some(bi) = basis.get_mut(i) {
                    *bi = ai;
                }
                ai += 1;
            }
            Sense::Eq => {
                if let Some(v) = row.get_mut(ai) {
                    *v = 1.0;
                }
                if let Some(bi) = basis.get_mut(i) {
                    *bi = ai;
                }
                ai += 1;
            }
        }
        if let Some(v) = row.get_mut(width) {
            *v = *b;
        }
        t.push(row);
    }
    let is_art = |j: usize| j >= ny + n_slack && j < width;
    // Phase 1: minimise the sum of the artificials.
    let mut obj = vec![0.0; width + 1];
    for j in (ny + n_slack)..width {
        if let Some(v) = obj.get_mut(j) {
            *v = 1.0;
        }
    }
    for (i, row) in t.iter().enumerate() {
        if basis.get(i).is_some_and(|b| is_art(*b)) {
            for (v, x) in obj.iter_mut().zip(row) {
                *v -= x;
            }
        }
    }
    let all: Vec<bool> = vec![true; width];
    if n_art > 0 {
        match simplex(&mut t, &mut basis, &mut obj, &all, budget) {
            Step::Limit => return Outcome::Limit,
            Step::Unbounded | Step::Optimal => {}
        }
        let scale = normalized.iter().map(|r| r.2.abs()).fold(1.0f64, f64::max);
        if -obj.get(width).copied().unwrap_or(0.0) > 1e-7 * scale {
            return Outcome::Infeasible;
        }
        // Drive zero-valued artificials out of the basis where possible.
        for i in 0..m {
            if !basis.get(i).is_some_and(|b| is_art(*b)) {
                continue;
            }
            let j = t.get(i).and_then(|row| (0..ny + n_slack).find(|&j| row.get(j).is_some_and(|v| v.abs() > EPS)));
            if let Some(j) = j {
                pivot(&mut t, &mut obj, i, j);
                if let Some(b) = basis.get_mut(i) {
                    *b = j;
                }
            }
        }
    }
    // Phase 2: the real costs; artificials may not re-enter.
    let mut cost = vec![0.0; width + 1];
    for (k, mp) in cols.iter().enumerate() {
        if let Some(v) = cost.get_mut(k) {
            *v = c.get(mp.var).copied().unwrap_or(0.0) * mp.sign;
        }
    }
    let mut obj = cost.clone();
    for (i, row) in t.iter().enumerate() {
        let cb = basis.get(i).and_then(|b| cost.get(*b)).copied().unwrap_or(0.0);
        if cb != 0.0 {
            for (v, x) in obj.iter_mut().zip(row) {
                *v -= cb * x;
            }
        }
    }
    let allowed: Vec<bool> = (0..width).map(|j| !is_art(j)).collect();
    match simplex(&mut t, &mut basis, &mut obj, &allowed, budget) {
        Step::Limit => return Outcome::Limit,
        Step::Unbounded => return Outcome::Unbounded,
        Step::Optimal => {}
    }
    let mut y = vec![0.0; width];
    for (i, row) in t.iter().enumerate() {
        if let (Some(b), Some(v)) = (basis.get(i), row.get(width))
            && let Some(e) = y.get_mut(*b)
        {
            *e = *v;
        }
    }
    let mut x = offset;
    for (k, mp) in cols.iter().enumerate() {
        if let Some(e) = x.get_mut(mp.var) {
            *e += mp.sign * y.get(k).copied().unwrap_or(0.0);
        }
    }
    let val: f64 = c.iter().zip(&x).map(|(a, b)| a * b).sum();
    Outcome::Solved(x, val, true)
}

/// Linearises the model at `x0` (n + 1 evaluations) and checks it at one more point. Returns the
/// objective gradient and constant, and per row the gradient and constant of `lhs − rhs`.
#[allow(clippy::type_complexity)]
pub(super) fn linearize(ev: &mut Evaluator, x0: &[f64]) -> std::result::Result<((Vec<f64>, f64), Vec<(Vec<f64>, f64)>), &'static str> {
    let n = x0.len();
    let p0 = ev.eval(x0).ok_or("budget")?;
    if !p0.f.is_finite() || p0.g.iter().any(|v| !v.is_finite()) {
        return Err("error");
    }
    let mut cf = vec![0.0; n];
    let mut ga = vec![vec![0.0; n]; p0.g.len()];
    for j in 0..n {
        let mut x = x0.to_vec();
        if let Some(v) = x.get_mut(j) {
            *v += 1.0;
        }
        let p = ev.eval(&x).ok_or("budget")?;
        if let Some(v) = cf.get_mut(j) {
            *v = p.f - p0.f;
        }
        for (row, (gi, g0)) in ga.iter_mut().zip(p.g.iter().zip(&p0.g)) {
            if let Some(v) = row.get_mut(j) {
                *v = gi - g0;
            }
        }
    }
    let dot = |a: &[f64], x: &[f64]| a.iter().zip(x).map(|(p, q)| p * q).sum::<f64>();
    let f_const = p0.f - dot(&cf, x0);
    let rows: Vec<(Vec<f64>, f64)> = ga.into_iter().zip(&p0.g).map(|(a, g0)| (a.clone(), g0 - dot(&a, x0))).collect();
    // Linearity check at a spread-out point.
    let xt: Vec<f64> =
        x0.iter().enumerate().map(|(j, v)| v + if j % 2 == 0 { 1.7 + 0.3 * (j % 5) as f64 } else { -0.6 - 0.2 * (j % 3) as f64 }).collect();
    let pt = ev.eval(&xt).ok_or("budget")?;
    let close = |actual: f64, pred: f64, terms: f64| actual.is_finite() && (actual - pred).abs() <= 1e-6 * (1.0 + actual.abs() + terms);
    let terms = |a: &[f64]| a.iter().zip(&xt).map(|(p, q)| (p * q).abs()).sum::<f64>();
    if !close(pt.f, f_const + dot(&cf, &xt), terms(&cf)) {
        return Err("nonlinear");
    }
    for ((a, k), g) in rows.iter().zip(&pt.g) {
        if !close(*g, k + dot(a, &xt), terms(a)) {
            return Err("nonlinear");
        }
    }
    Ok(((cf, f_const), rows))
}
