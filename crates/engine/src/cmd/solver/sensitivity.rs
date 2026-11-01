//! The Sensitivity report. Simplex LP: each variable's reduced cost and the range of its
//! objective coefficient over which the basis stays optimal, and each constraint's shadow price
//! and the range of its right side over which the shadow price holds, from the final tableau of
//! the LP linearised at the solution. GRG Nonlinear: the reduced gradients and the Lagrange
//! multipliers of the binding constraints at the solution, from difference gradients.

use super::lp::{FinalTableau, LpRow, Outcome, Row, Sense, SimplexProgram, simplex_program, solve_lp_tableau};
use super::model::{Goal, Method, Model};
use super::problem::{Evaluator, Point, Problem, as_num};
use super::reports::{Lines, ReportInput, ReportKind, abs_ref, cell_name, table, title};
use super::{CellRef, DocState, EPS, EngineError, Result, Sheet, Trial, Value};

/// Excel prints an unlimited allowable change as 1E+30.
const NO_LIMIT: f64 = 1e30;

/// Most arithmetic the Lagrange multipliers of a nonlinear model may take (about a second): a
/// pass over k binding rows and f free variables costs about 2·k²·f, and each pass drops at
/// most one row, so a model at the constraint limit could otherwise take minutes.
#[cfg(not(test))]
const MULTIPLIER_WORK: u64 = 1_000_000_000;
/// A low limit in tests, so the refusal is reachable with a small model.
#[cfg(test)]
const MULTIPLIER_WORK: u64 = 20_000;

/// Why a model has no Sensitivity report (`None` when it has one).
pub(super) fn sensitivity_refusal(m: &Model, prob: &Problem) -> Option<&'static str> {
    if prob.integer.iter().any(|b| *b) {
        Some("The Sensitivity report doesn't apply to models with integer or binary constraints.")
    } else if m.method == Method::Evolutionary {
        Some("The Sensitivity report is available for Simplex LP and GRG Nonlinear.")
    } else if matches!(m.goal, Goal::Value(_)) {
        Some("The Sensitivity report needs an objective to maximise or minimise.")
    } else {
        None
    }
}

/// A sensitivity figure: values within `noise` of zero (rounding) print as 0, unlimited ones
/// as 1E+30. The noise level is relative to the coefficients or gradients the figure is made
/// of, never to the objective's value: adding a constant to the objective changes no price,
/// multiplier or gradient, so it must not change what the report shows.
fn sens_num(v: f64, noise: f64) -> Value {
    if !v.is_finite() || v >= NO_LIMIT {
        Value::number(NO_LIMIT)
    } else if v.abs() <= noise {
        Value::number(0.0)
    } else {
        Value::number(v)
    }
}

fn num(v: f64) -> Value {
    if v.is_finite() { Value::number(v) } else { Value::Error(gridcraft_core::CellError::Value) }
}

/// The smallest ratio `num / den` over the `den` above zero (the step to the first bound).
fn min_ratio(pairs: impl Iterator<Item = (f64, f64)>) -> f64 {
    pairs.filter(|(_, den)| *den > EPS).map(|(num, den)| num.max(0.0) / den).fold(f64::INFINITY, f64::min)
}

/// Solves the square system `a·x = b` by Gaussian elimination with partial pivoting; unknowns
/// of a singular system that have no pivot are 0.
fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Vec<f64> {
    let n = b.len();
    let scale = a.iter().flatten().fold(0.0f64, |m, v| m.max(v.abs())).max(f64::MIN_POSITIVE);
    let mut pivots: Vec<Option<usize>> = vec![None; n];
    let mut row = 0;
    for col in 0..n {
        let Some(p) = (row..n).max_by(|i, k| {
            let v = |r: &usize| a.get(*r).and_then(|x| x.get(col)).map_or(0.0, |v| v.abs());
            v(i).total_cmp(&v(k))
        }) else {
            break;
        };
        if a.get(p).and_then(|x| x.get(col)).is_none_or(|v| v.abs() <= 1e-12 * scale) {
            continue;
        }
        a.swap(row, p);
        b.swap(row, p);
        let (Some(prow), Some(pb)) = (a.get(row).cloned(), b.get(row).copied()) else { break };
        let pv = prow.get(col).copied().unwrap_or(1.0);
        for (i, (r, bi)) in a.iter_mut().zip(b.iter_mut()).enumerate() {
            if i == row {
                continue;
            }
            let f = r.get(col).copied().unwrap_or(0.0) / pv;
            if f != 0.0 {
                for (v, p) in r.iter_mut().zip(&prow) {
                    *v -= f * p;
                }
                *bi -= f * pb;
            }
        }
        if let Some(e) = pivots.get_mut(col) {
            *e = Some(row);
        }
        row += 1;
    }
    pivots
        .iter()
        .enumerate()
        .map(|(col, p)| match p {
            Some(r) => b.get(*r).copied().unwrap_or(0.0) / a.get(*r).and_then(|x| x.get(col)).copied().unwrap_or(1.0),
            None => 0.0,
        })
        .collect()
}

fn out_of_evaluations() -> EngineError {
    EngineError::Other("Solver ran out of evaluations.".into())
}

fn not_optimal() -> EngineError {
    EngineError::Other("The Sensitivity report needs an optimal solution: these values are not optimal for the model. Solve it first.".into())
}

/// What both report bodies share: the model, the solution, its objective value and the rows'
/// final values and right sides.
struct At<'a> {
    inp: &'a ReportInput<'a>,
    sh: &'a Sheet,
    point: Point,
    finals: Vec<(f64, f64)>,
}

impl At<'_> {
    fn var_head(&self, j: usize, c: CellRef) -> Vec<Value> {
        vec![Value::text(abs_ref(c)), Value::text(cell_name(self.sh, c)), num(self.inp.solution.get(j).copied().unwrap_or(f64::NAN))]
    }
    fn row_head(&self, i: usize, r: &Row) -> Vec<Value> {
        let fin = self.finals.get(i).map_or(f64::NAN, |f| f.0);
        vec![Value::text(abs_ref(r.lhs)), Value::text(cell_name(self.sh, r.lhs)), num(fin)]
    }
}

/// The variable rows, the constraint rows and their headers.
type Body = (Vec<Vec<Value>>, Vec<Vec<Value>>, Vec<&'static str>, Vec<&'static str>);

pub(super) fn sensitivity_report(d: &DocState, sh: &Sheet, inp: &ReportInput) -> Result<Lines> {
    let m = inp.model;
    if let Some(why) = sensitivity_refusal(m, inp.prob) {
        return Err(EngineError::Other(why.into()));
    }
    let n = m.vars.len();
    let x = &inp.solution;
    let mut ev = Evaluator {
        trial: Trial::new(d),
        sheet: m.sheet,
        vars: m.vars.clone(),
        objective: m.objective,
        rows: inp.prob.rows.clone(),
        evals: 0,
        // Room for the gradients (GRG) or two linearisations (Simplex LP) at the least.
        budget: inp.budget.max(3 * n as u64 + 4),
        target: None,
    };
    let point = ev.eval(x).ok_or_else(out_of_evaluations)?;
    // The rows' final values and right sides at the solution.
    let mut cells = Vec::new();
    for r in &inp.prob.rows {
        cells.push(r.lhs);
        cells.extend(r.rhs);
    }
    let mut trial = Trial::new(d);
    let sets: Vec<(usize, CellRef, Value)> = m.vars.iter().zip(x).map(|(c, v)| (m.sheet, *c, Value::number(*v))).collect();
    let reads: Vec<(usize, CellRef)> = cells.iter().map(|c| (m.sheet, *c)).collect();
    let mut vals = trial.run(&sets, &reads).into_iter().map(|v| as_num(&v));
    let finals: Vec<(f64, f64)> = inp
        .prob
        .rows
        .iter()
        .map(|r| {
            let l = vals.next().unwrap_or(f64::NAN);
            (l, if r.rhs.is_some() { vals.next().unwrap_or(f64::NAN) } else { r.rhs_const })
        })
        .collect();
    let at = At { inp, sh, point, finals };
    let (var_rows, con_rows, var_headers, con_headers) =
        if m.method == Method::Simplex { simplex_body(&mut ev, &at)? } else { nonlinear_body(&mut ev, &at)? };
    let mut out = Lines::new();
    let r = title(&mut out, ReportKind::Sensitivity, &sh.name);
    out.push((r, 0, Value::text("Variable Cells"), true));
    let r = table(&mut out, r + 1, &var_headers, var_rows);
    out.push((r, 0, Value::text("Constraints"), true));
    table(&mut out, r + 1, &con_headers, con_rows);
    Ok(out)
}

/// Simplex LP: reduced costs, shadow prices and their ranges from the final tableau.
fn simplex_body(ev: &mut Evaluator, at: &At) -> Result<Body> {
    let (inp, m) = (at.inp, at.inp.model);
    // The program the solve built, from the same start values (data.solver's x0, which Solver
    // Results passes back as `original`), so the solve below repeats its pivots. Start values
    // the model can't be evaluated at (given only to the report) fall back to the solution.
    let start: Vec<f64> = inp.original.iter().map(|v| if v.is_finite() { *v } else { 0.0 }).collect();
    let built = match simplex_program(ev, m.goal, &start) {
        Err("nonlinear" | "error") if start != inp.solution => simplex_program(ev, m.goal, &inp.solution),
        other => other,
    };
    let SimplexProgram { cf, c, rows, mut work } = built.map_err(|e| {
        if e == "budget" {
            out_of_evaluations()
        } else {
            EngineError::Other("The model isn't linear in the variable cells, so Simplex LP can't report its sensitivity.".into())
        }
    })?;
    let (outcome, tab) = solve_lp_tableau(&c, &rows, &inp.prob.lo, &inp.prob.hi, &mut work);
    let (Outcome::Solved(vertex, best, _), Some(tab)) = (outcome, tab) else {
        return Err(EngineError::Other("Solver could not find an optimal basis for the Sensitivity report.".into()));
    };
    // The report describes the optimum, so the values it shows must be one.
    if !optimal_point(&c, &rows, (&inp.prob.lo, &inp.prob.hi), &inp.solution, best) {
        return Err(not_optimal());
    }
    // The prices and ranges come from the tableau of `vertex`, so the values shown must be that
    // vertex. Another optimum with the same objective (when the optimum is not unique) has other
    // ranges, so it is refused too, although it is optimal.
    let near = |a: f64, b: f64| (a - b).abs() <= super::PRECISION * (1.0 + a.abs().max(b.abs()));
    if vertex.len() != inp.solution.len() || !vertex.iter().zip(&inp.solution).all(|(a, b)| near(*a, *b)) {
        return Err(EngineError::Other(
            "The Sensitivity report needs the values at the Simplex LP solution: these values are another optimal solution, whose ranges differ. Solve the model with Simplex LP first."
                .into(),
        ));
    }
    // Minimising `sign · objective`.
    let sign = if m.goal == Goal::Max { -1.0 } else { 1.0 };
    // A shadow price (objective per unit of right side) and a reduced cost (objective per unit
    // of a variable) are combinations of the objective coefficients, so a figure below 1E-9 of
    // the largest coefficient is rounding noise. Allowable changes to a coefficient or a right
    // side are ratios the tableau's own pivot tolerance (`EPS`) already bounds.
    let noise = 1e-9 * cf.iter().map(|v| v.abs()).fold(0.0, f64::max).max(f64::MIN_POSITIVE);
    let shadow: Vec<f64> = (0..rows.len())
        .map(|i| {
            let col = tab.ident.get(i).copied().unwrap_or(usize::MAX);
            sign * flip(&tab, i) * -tab.obj.get(col).copied().unwrap_or(0.0)
        })
        .collect();
    let con_rows = ev
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            // The basic values move along the row's unit column as its right side changes.
            let col = tab.ident.get(i).copied().unwrap_or(usize::MAX);
            let dir: Vec<(f64, f64)> =
                (0..tab.t.len()).filter(|r| real_basic(&tab, *r)).map(|r| (entry(&tab, r, tab.width), flip(&tab, i) * entry(&tab, r, col))).collect();
            // An artificial still basic after phase one stands for a row the others make
            // redundant, and it is zero there. When this right side moves it, any change alone
            // leaves no solution, so both ranges are 0.
            let pinned = (0..tab.t.len()).any(|r| !real_basic(&tab, r) && entry(&tab, r, col).abs() > EPS);
            let (increase, decrease) =
                if pinned { (0.0, 0.0) } else { (min_ratio(dir.iter().map(|(v, d)| (*v, -d))), min_ratio(dir.iter().map(|(v, d)| (*v, *d)))) };
            let b = at.finals.get(i).map_or(f64::NAN, |f| f.1);
            let mut row = at.row_head(i, r);
            row.extend([sens_num(shadow.get(i).copied().unwrap_or(0.0), noise), num(b), sens_num(increase, EPS), sens_num(decrease, EPS)]);
            row
        })
        .collect();
    let nonbasic: Vec<usize> = (0..tab.first_art).filter(|l| !tab.basis.contains(l)).collect();
    let var_rows = m
        .vars
        .iter()
        .enumerate()
        .map(|(j, c)| {
            let coef = cf.get(j).copied().unwrap_or(0.0);
            let used: f64 = rows.iter().zip(&shadow).map(|((a, _, _), p)| a.get(j).copied().unwrap_or(0.0) * p).sum();
            // A change Δ in the coefficient changes each nonbasic reduced cost by Δ·e.
            let own = |col: usize| tab.cols.get(col).filter(|mp| mp.var == j).map_or(0.0, |mp| mp.sign);
            let moves: Vec<(f64, f64)> = nonbasic
                .iter()
                .map(|&l| {
                    let through_basis: f64 = (0..tab.t.len()).map(|r| tab.basis.get(r).map_or(0.0, |b| own(*b)) * entry(&tab, r, l)).sum();
                    (tab.obj.get(l).copied().unwrap_or(0.0), sign * (own(l) - through_basis))
                })
                .collect();
            let increase = min_ratio(moves.iter().map(|(d, e)| (*d, -e)));
            let decrease = min_ratio(moves.iter().map(|(d, e)| (*d, *e)));
            let mut row = at.var_head(j, *c);
            row.extend([sens_num(coef - used, noise), num(coef), sens_num(increase, EPS), sens_num(decrease, EPS)]);
            row
        })
        .collect();
    Ok((
        var_rows,
        con_rows,
        vec!["Cell", "Name", "Final Value", "Reduced Cost", "Objective Coefficient", "Allowable Increase", "Allowable Decrease"],
        vec!["Cell", "Name", "Final Value", "Shadow Price", "Constraint R.H. Side", "Allowable Increase", "Allowable Decrease"],
    ))
}

/// Whether `x` satisfies the LP (rows and bounds) and reaches its optimum `best` (minimising
/// `c·x`), within the constraint precision.
fn optimal_point(c: &[f64], rows: &[LpRow], (lo, hi): (&[f64], &[f64]), x: &[f64], best: f64) -> bool {
    let near = |v: f64, size: f64| v <= super::PRECISION * (1.0 + size);
    let dot = |a: &[f64]| a.iter().zip(x).fold((0.0, 0.0), |(s, m), (p, q)| (s + p * q, m + (p * q).abs()));
    let rows_hold = rows.iter().all(|(a, sense, b)| {
        let (lhs, size) = dot(a);
        let size = size + b.abs();
        match sense {
            Sense::Le => near(lhs - b, size),
            Sense::Ge => near(b - lhs, size),
            Sense::Eq => near((lhs - b).abs(), size),
        }
    });
    let bounds_hold = x.iter().enumerate().all(|(j, v)| {
        let (l, h) = (lo.get(j).copied().unwrap_or(f64::NEG_INFINITY), hi.get(j).copied().unwrap_or(f64::INFINITY));
        v.is_finite() && near(l - v, v.abs()) && near(v - h, v.abs())
    });
    let (value, size) = dot(c);
    // Two-sided: a point just outside a bound can beat the optimum by far more than the
    // precision when its costs are large.
    rows_hold && bounds_hold && near((value - best).abs(), size + best.abs())
}

fn entry(tab: &FinalTableau, r: usize, col: usize) -> f64 {
    tab.t.get(r).and_then(|row| row.get(col)).copied().unwrap_or(0.0)
}

fn real_basic(tab: &FinalTableau, r: usize) -> bool {
    tab.basis.get(r).is_some_and(|b| *b < tab.first_art)
}

/// -1 for a row the tableau negated, else 1.
fn flip(tab: &FinalTableau, i: usize) -> f64 {
    if tab.flipped.get(i).copied().unwrap_or(false) { -1.0 } else { 1.0 }
}

/// Central-difference gradients, inside the bounds, of the objective and of each row's
/// `lhs − rhs` at the solution.
fn gradients(ev: &mut Evaluator, inp: &ReportInput) -> Result<(Vec<f64>, Vec<Vec<f64>>)> {
    let x = &inp.solution;
    let n = x.len();
    let mut df = vec![0.0; n];
    let mut dg = vec![vec![0.0; n]; ev.rows.len()];
    for j in 0..n {
        let xj = x.get(j).copied().unwrap_or(0.0);
        let (l, u) = (inp.prob.lo.get(j).copied().unwrap_or(f64::NEG_INFINITY), inp.prob.hi.get(j).copied().unwrap_or(f64::INFINITY));
        let h = 1e-6 * xj.abs().max(1.0);
        let (a, b) = ((xj - h).max(l), (xj + h).min(u));
        if b <= a {
            continue;
        }
        let mut point = |v: f64| {
            let mut p = x.clone();
            if let Some(e) = p.get_mut(j) {
                *e = v;
            }
            ev.eval(&p).ok_or_else(out_of_evaluations)
        };
        let (pa, pb) = (point(a)?, point(b)?);
        if let Some(e) = df.get_mut(j) {
            *e = (pb.f - pa.f) / (b - a);
        }
        for (row, (ga, gb)) in dg.iter_mut().zip(pa.g.iter().zip(&pb.g)) {
            if let Some(e) = row.get_mut(j) {
                *e = (gb - ga) / (b - a);
            }
        }
    }
    Ok((df, dg))
}

/// The largest absolute entry of a gradient, at least the smallest positive number.
fn gradient_size(g: &[f64]) -> f64 {
    g.iter().map(|v| v.abs()).fold(0.0, f64::max).max(f64::MIN_POSITIVE)
}

/// Least squares on the free variables: ∇f = Σ λᵢ ∇gᵢ over the `active` rows. An inequality's
/// multiplier has the sign that tightening it costs (λ ≥ 0 for `lhs ≤ rhs` when maximising); a
/// row whose multiplier has the other sign isn't binding, so it leaves and the rest are solved
/// again. A multiplier is objective per unit of its row, so its sign test compares λᵢ·|∇gᵢ|,
/// the part of the objective's gradient the row explains, with the gradient's size; the
/// objective's value plays no part. The work is bounded by `budget` (see
/// [`MULTIPLIER_WORK`]); past it the report is refused.
fn multipliers(rows: &[Row], goal: Goal, mut active: Vec<usize>, free: &[usize], (df, dg): (&[f64], &[Vec<f64>]), budget: u64) -> Result<Vec<f64>> {
    let grad = |i: usize, j: usize| dg.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let row_size = |i: usize| dg.get(i).map_or(f64::MIN_POSITIVE, |r| gradient_size(r));
    let noise = 1e-6 * gradient_size(df);
    let goal_sign = if goal == Goal::Max { 1.0 } else { -1.0 };
    let mut lambda = vec![0.0; rows.len()];
    let mut work = 0u64;
    while !active.is_empty() {
        let (k, f) = (active.len() as u64, (free.len() as u64).max(1));
        work = work.saturating_add(2u64.saturating_mul(k).saturating_mul(k).saturating_mul(f));
        if work > budget {
            return Err(EngineError::Other(
                "The Sensitivity report of this nonlinear model has too many binding constraints to work out in reasonable time.".into(),
            ));
        }
        let normal: Vec<Vec<f64>> =
            active.iter().map(|&i| active.iter().map(|&k| free.iter().map(|&j| grad(i, j) * grad(k, j)).sum()).collect()).collect();
        let rhs: Vec<f64> = active.iter().map(|&i| free.iter().map(|&j| grad(i, j) * df.get(j).copied().unwrap_or(0.0)).sum()).collect();
        let solved = solve_dense(normal, rhs);
        let wrong = active
            .iter()
            .zip(&solved)
            .enumerate()
            .filter_map(|(k, (&i, l))| {
                let want = match rows.get(i).map_or(Sense::Eq, |r| r.sense) {
                    Sense::Le => goal_sign,
                    Sense::Ge => -goal_sign,
                    Sense::Eq => return None,
                };
                (want * l * row_size(i) < -noise).then_some((k, want * l))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match wrong {
            Some((k, _)) => {
                active.remove(k);
            }
            None => {
                for (i, l) in active.iter().zip(solved) {
                    if let Some(e) = lambda.get_mut(*i) {
                        *e = l;
                    }
                }
                break;
            }
        }
    }
    Ok(lambda)
}

/// Whether the solution is a Karush-Kuhn-Tucker point of the model, from the gradients the
/// report computed: every row and bound holds, and each reduced gradient (the objective's
/// gradient less what the binding rows' multipliers explain) is near zero, or, for a variable at
/// a bound, points only out of that bound in the objective's direction. A binding row whose
/// multiplier had the wrong sign was dropped by [`multipliers`], so the part of the gradient it
/// would have explained remains in the reduced gradients and the point is refused here.
fn kkt_point(at: &At, rows: &[Row], df: &[f64], reduced: &[f64]) -> bool {
    let (inp, x) = (at.inp, &at.inp.solution);
    // Feasibility with the tolerance the nonlinear method accepts a solution within (ten times
    // the constraint precision, relative to the largest constant right side).
    let rhs_scale = rows.iter().map(|r| r.rhs_const.abs()).fold(1.0, f64::max);
    let rows_hold = rows.iter().enumerate().all(|(i, r)| {
        let (g, b) = (at.point.g.get(i).copied().unwrap_or(f64::NAN), at.finals.get(i).map_or(f64::NAN, |f| f.1));
        let over = match r.sense {
            Sense::Le => g,
            Sense::Ge => -g,
            Sense::Eq => g.abs(),
        };
        over <= 10.0 * super::PRECISION * rhs_scale.max(1.0 + b.abs())
    });
    if !rows_hold || !at.point.f.is_finite() {
        return false;
    }
    // Stationarity, relative to the size of the objective's gradient (at least 1). At optima the
    // nonlinear method solves, the reduced gradients are below about 1E-6 of that size; 1E-3
    // leaves a wide margin for the method's stopping test and for rounding in the difference
    // gradients of badly scaled models, and still refuses a point whose objective improves
    // visibly along a direction the constraints allow.
    let tol = 1e-3 * df.iter().map(|v| v.abs()).fold(1.0, f64::max);
    // Moving up a variable improves a maximum by +r and a minimum by −r per unit.
    let sign = if inp.model.goal == Goal::Max { 1.0 } else { -1.0 };
    x.iter().enumerate().all(|(j, &v)| {
        let (l, h) = (inp.prob.lo.get(j).copied().unwrap_or(f64::NEG_INFINITY), inp.prob.hi.get(j).copied().unwrap_or(f64::INFINITY));
        let near = super::PRECISION * (1.0 + v.abs());
        if !v.is_finite() || v < l - near || v > h + near {
            return false;
        }
        // The same test for "at a bound" as the choice of free variables above.
        let edge = 1e-6 * v.abs().max(1.0);
        let (at_lo, at_hi) = (v <= l + edge, v >= h - edge);
        let gain = reduced.get(j).map_or(f64::NAN, |r| sign * r);
        gain.is_finite() && (at_hi || gain <= tol) && (at_lo || gain >= -tol)
    })
}

/// GRG Nonlinear: reduced gradients and the binding rows' Lagrange multipliers.
fn nonlinear_body(ev: &mut Evaluator, at: &At) -> Result<Body> {
    let (inp, m) = (at.inp, at.inp.model);
    let x = &inp.solution;
    let (df, dg) = gradients(ev, inp)?;
    // Binding rows (within the nonlinear method's accuracy) and the variables strictly inside
    // their bounds.
    let active: Vec<usize> = ev
        .rows
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            let (g, b) = (at.point.g.get(*i).copied().unwrap_or(f64::NAN), at.finals.get(*i).map_or(0.0, |f| f.1));
            r.sense == Sense::Eq || g.abs() <= 1e-4 * (1.0 + b.abs())
        })
        .map(|(i, _)| i)
        .collect();
    let free: Vec<usize> = (0..x.len())
        .filter(|&j| {
            let v = x.get(j).copied().unwrap_or(0.0);
            let tol = 1e-6 * v.abs().max(1.0);
            v > inp.prob.lo.get(j).copied().unwrap_or(f64::NEG_INFINITY) + tol && v < inp.prob.hi.get(j).copied().unwrap_or(f64::INFINITY) - tol
        })
        .collect();
    let lambda = multipliers(&ev.rows, m.goal, active, &free, (&df, &dg), MULTIPLIER_WORK)?;
    let grad = |i: usize, j: usize| dg.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let reduced: Vec<f64> =
        (0..x.len()).map(|j| df.get(j).copied().unwrap_or(0.0) - lambda.iter().enumerate().map(|(i, l)| l * grad(i, j)).sum::<f64>()).collect();
    // The report describes the optimum, so the values it shows must be one.
    if !kkt_point(at, &ev.rows, &df, &reduced) {
        return Err(not_optimal());
    }
    // Difference gradients are good to about 1E-6 of the objective gradient's size: a reduced
    // gradient (a part of that gradient) below it is rounding, and so is a Lagrange multiplier
    // (objective per unit of its row) whose λᵢ·|∇gᵢ| is.
    let noise = 1e-6 * gradient_size(&df);
    let var_rows = m
        .vars
        .iter()
        .enumerate()
        .map(|(j, c)| {
            let mut row = at.var_head(j, *c);
            row.push(sens_num(reduced.get(j).copied().unwrap_or(f64::NAN), noise));
            row
        })
        .collect();
    let con_rows = ev
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let row_size = dg.get(i).map_or(f64::MIN_POSITIVE, |g| gradient_size(g));
            let mut row = at.row_head(i, r);
            row.push(sens_num(lambda.get(i).copied().unwrap_or(0.0), noise / row_size));
            row
        })
        .collect();
    Ok((var_rows, con_rows, vec!["Cell", "Name", "Final Value", "Reduced Gradient"], vec!["Cell", "Name", "Final Value", "Lagrange Multiplier"]))
}

#[cfg(test)]
mod tests {
    use super::{Goal, MULTIPLIER_WORK, Row, Sense, multipliers};
    use gridcraft_core::CellRef;

    #[test]
    fn multipliers_at_the_constraint_limit_stop_at_the_work_limit() {
        // 1,000 binding `<=` rows over 200 free variables, each multiplier with the wrong sign:
        // without a limit every pass would drop one row and solve again, about 4·10⁸ operations
        // a pass for up to 1,000 passes. The limit (lowered in tests) refuses it at once.
        let (k, f) = (1000usize, 200usize);
        let rows: Vec<Row> = (0..k).map(|_| Row { lhs: CellRef::new(0, 0), sense: Sense::Le, rhs: None, rhs_const: 0.0 }).collect();
        let dg: Vec<Vec<f64>> = (0..k).map(|i| (0..f).map(|j| if j == i % f { 1.0 } else { 0.0 }).collect()).collect();
        let df = vec![-1.0; f];
        let free: Vec<usize> = (0..f).collect();
        let got = multipliers(&rows, Goal::Max, (0..k).collect(), &free, (&df, &dg), MULTIPLIER_WORK);
        assert!(got.is_err());
        // A small model within the limit still gets its multipliers: ∇f = (1, 2) on two
        // binding rows with unit gradients.
        let dg2 = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let got = multipliers(&rows[..2], Goal::Max, vec![0, 1], &free[..2], (&[1.0, 2.0], &dg2), MULTIPLIER_WORK).unwrap();
        assert_eq!(got, vec![1.0, 2.0]);
    }
}
