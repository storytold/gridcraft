//! Statistical distributions, their inverses, and the LINEST/LINESTX regressions.
//!
//! The numerics are self-contained: a Lanczos log-gamma, the regularized incomplete gamma and
//! beta functions (series/continued fractions), `erf`/`erfc`, the normal CDF and its inverse
//! (Acklam rational approximation refined with Halley steps) and monotone inversion by bisection
//! with a bounded iteration count. Every loop is bounded and every domain check mirrors the
//! documented Excel behavior; invalid inputs return [`EvalError`], never NaN or infinity.
//!
//! `LINEST`/`LINESTX` return the documented single-row table with `Slope1..SlopeN`, `Intercept`
//! and the regression statistics columns. A statistic that is undefined for the data (for
//! example the intercept standard error when `const` is FALSE) is returned as BLANK, which is the
//! closest representable value: the evaluator's scalar model has no error-cell variant.

#![allow(unused_imports)]

use std::f64::consts::{PI, SQRT_2};

use crate::ast::Literal;

use super::super::support::{args as make_args, eval_all, frame_for};
use super::*;

// ---------------------------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------------------------

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let upper = name.to_ascii_uppercase();
    let result = match upper.as_str() {
        "BETA.DIST" => beta_dist(ev, args, ctx),
        "BETA.INV" => beta_inv(ev, args, ctx),
        "CHISQ.DIST" => chisq_dist(ev, args, ctx),
        "CHISQ.DIST.RT" => chisq_dist_rt(ev, args, ctx),
        "CHISQ.INV" => chisq_inv(ev, args, ctx),
        "CHISQ.INV.RT" => chisq_inv_rt(ev, args, ctx),
        "CONFIDENCE.NORM" => confidence_norm(ev, args, ctx),
        "CONFIDENCE.T" => confidence_t(ev, args, ctx),
        "EXPON.DIST" => expon_dist(ev, args, ctx),
        "NORM.DIST" => norm_dist(ev, args, ctx),
        "NORM.INV" => norm_inv_fn(ev, args, ctx),
        "NORM.S.DIST" => norm_s_dist(ev, args, ctx),
        "NORM.S.INV" => norm_s_inv(ev, args, ctx),
        "POISSON.DIST" => poisson_dist(ev, args, ctx),
        "T.DIST" => t_dist(ev, args, ctx),
        "T.DIST.2T" => t_dist_2t(ev, args, ctx),
        "T.DIST.RT" => t_dist_rt(ev, args, ctx),
        "T.INV" => t_inv(ev, args, ctx),
        "T.INV.2T" => t_inv_2t(ev, args, ctx),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    let upper = name.to_ascii_uppercase();
    let result = match upper.as_str() {
        "LINEST" => linest(ev, args, ctx),
        "LINESTX" => linestx(ev, args, ctx),
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------------------------

fn domain(name: &str, message: &str) -> EvalError {
    EvalError::new(format!("{name}: {message}"))
}

fn finite_number(a: &super::super::support::Args<'_>, index: usize) -> Result<f64, EvalError> {
    let value = a.number(index)?;
    if value.is_finite() { Ok(value) } else { Err(EvalError::new(format!("{} argument {} must be a finite number", a.name, index + 1))) }
}

/// An optional bound (`A`/`B` in the beta functions). A BLANK bound is an error: the catalog
/// documents that omitting the argument and passing BLANK are different.
fn optional_bound(a: &super::super::support::Args<'_>, index: usize, default: f64) -> Result<f64, EvalError> {
    match a.optional(index) {
        None => Ok(default),
        Some(value) if value.is_blank() => Err(EvalError::new(format!("{} argument {} cannot be BLANK", a.name, index + 1))),
        Some(value) => {
            let number = value.to_number().ok_or_else(|| EvalError::new(format!("{} argument {} must be numeric", a.name, index + 1)))?;
            if number.is_finite() { Ok(number) } else { Err(EvalError::new(format!("{} argument {} must be a finite number", a.name, index + 1))) }
        }
    }
}

fn decimal(name: &str, value: f64) -> Result<Scalar, EvalError> {
    if value.is_finite() { Ok(Scalar::Decimal(value)) } else { Err(EvalError::new(format!("{name} produced a non-finite result"))) }
}

/// Recognizes the optional `const` argument of `LINEST`/`LINESTX` (`TRUE`, `FALSE`, `TRUE()`,
/// `FALSE()`). Anything else is an X expression/column.
fn const_flag(expr: &BoundExpr) -> Option<bool> {
    match expr {
        BoundExpr::Literal(Literal::Boolean(value)) => Some(*value),
        BoundExpr::Call { spec, args } if args.is_empty() => match spec.name.to_ascii_uppercase().as_str() {
            "TRUE" => Some(true),
            "FALSE" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// A numeric model/row value, or `None` when the row must be dropped from a regression.
fn numeric(value: &Scalar) -> Option<f64> {
    if value.is_blank() {
        return None;
    }
    value.to_number().filter(|number| number.is_finite())
}

// ---------------------------------------------------------------------------------------------
// Distributions
// ---------------------------------------------------------------------------------------------

fn beta_dist(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("BETA.DIST", &values);
    let x = finite_number(&a, 0)?;
    let alpha = finite_number(&a, 1)?;
    let beta = finite_number(&a, 2)?;
    let cumulative = a.boolean(3)?;
    let lo = optional_bound(&a, 4, 0.0)?;
    let hi = optional_bound(&a, 5, 1.0)?;
    if alpha <= 0.0 || beta <= 0.0 {
        return Err(domain("BETA.DIST", "Alpha and Beta must be positive"));
    }
    if lo >= hi {
        return Err(domain("BETA.DIST", "A must be less than B"));
    }
    if x < lo || x > hi {
        return Err(domain("BETA.DIST", "X must lie between A and B"));
    }
    let z = (x - lo) / (hi - lo);
    let value = if cumulative {
        reg_inc_beta(z, alpha, beta)
    } else if z <= 0.0 {
        if alpha < 1.0 {
            return Err(domain("BETA.DIST", "the density is unbounded at A"));
        }
        if alpha > 1.0 { 0.0 } else { (-ln_beta(alpha, beta)).exp() / (hi - lo) }
    } else if z >= 1.0 {
        if beta < 1.0 {
            return Err(domain("BETA.DIST", "the density is unbounded at B"));
        }
        if beta > 1.0 { 0.0 } else { (-ln_beta(alpha, beta)).exp() / (hi - lo) }
    } else {
        ((alpha - 1.0) * z.ln() + (beta - 1.0) * (1.0 - z).ln() - ln_beta(alpha, beta)).exp() / (hi - lo)
    };
    decimal("BETA.DIST", value)
}

fn beta_inv(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("BETA.INV", &values);
    let probability = finite_number(&a, 0)?;
    let alpha = finite_number(&a, 1)?;
    let beta = finite_number(&a, 2)?;
    let lo = optional_bound(&a, 3, 0.0)?;
    let hi = optional_bound(&a, 4, 1.0)?;
    if alpha <= 0.0 || beta <= 0.0 {
        return Err(domain("BETA.INV", "Alpha and Beta must be positive"));
    }
    if probability <= 0.0 || probability > 1.0 {
        return Err(domain("BETA.INV", "Probability must be greater than 0 and at most 1"));
    }
    if lo >= hi {
        return Err(domain("BETA.INV", "A must be less than B"));
    }
    let z = if probability >= 1.0 {
        1.0
    } else {
        let z = invert(|value| reg_inc_beta(value, alpha, beta), probability, 0.0, 1.0, 1.0);
        if !z.is_finite() {
            return Err(domain("BETA.INV", "the inverse did not converge"));
        }
        z
    };
    decimal("BETA.INV", lo + z * (hi - lo))
}

fn chisq_dist(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("CHISQ.DIST", &values);
    let x = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.trunc();
    let cumulative = a.boolean(2)?;
    if x < 0.0 {
        return Err(domain("CHISQ.DIST", "X must be non-negative"));
    }
    if !(1.0..=1e10).contains(&freedom) {
        return Err(domain("CHISQ.DIST", "Deg_freedom must be between 1 and 1E10"));
    }
    let value = if cumulative {
        reg_gamma_p(freedom / 2.0, x / 2.0)
    } else if x == 0.0 {
        if freedom < 2.0 {
            return Err(domain("CHISQ.DIST", "the density is unbounded at 0"));
        }
        if freedom == 2.0 { 0.5 } else { 0.0 }
    } else {
        ((freedom / 2.0 - 1.0) * x.ln() - x / 2.0 - (freedom / 2.0) * 2.0f64.ln() - ln_gamma(freedom / 2.0)).exp()
    };
    decimal("CHISQ.DIST", value)
}

fn chisq_dist_rt(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("CHISQ.DIST.RT", &values);
    let x = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.trunc();
    if x < 0.0 {
        return Err(domain("CHISQ.DIST.RT", "X must be non-negative"));
    }
    if !(1.0..=1e10).contains(&freedom) {
        return Err(domain("CHISQ.DIST.RT", "Deg_freedom must be between 1 and 1E10"));
    }
    decimal("CHISQ.DIST.RT", reg_gamma_q(freedom / 2.0, x / 2.0))
}

fn chisq_inv(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("CHISQ.INV", &values);
    let probability = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.round();
    if !(1.0..=1e10).contains(&freedom) {
        return Err(domain("CHISQ.INV", "Deg_freedom must be between 1 and 1E10"));
    }
    if probability < 0.0 || probability >= 1.0 {
        return Err(domain("CHISQ.INV", "Probability must be at least 0 and less than 1"));
    }
    if probability == 0.0 {
        return Ok(Scalar::Decimal(0.0));
    }
    let value = invert(|x| reg_gamma_p(freedom / 2.0, x / 2.0), probability, 0.0, freedom.max(1.0), 1e60);
    if !value.is_finite() {
        return Err(domain("CHISQ.INV", "the inverse did not converge"));
    }
    decimal("CHISQ.INV", value)
}

fn chisq_inv_rt(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("CHISQ.INV.RT", &values);
    let probability = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.round();
    if !(1.0..=1e10).contains(&freedom) {
        return Err(domain("CHISQ.INV.RT", "Deg_freedom must be between 1 and 1E10"));
    }
    if probability <= 0.0 || probability > 1.0 {
        return Err(domain("CHISQ.INV.RT", "Probability must be greater than 0 and at most 1"));
    }
    if probability == 1.0 {
        return Ok(Scalar::Decimal(0.0));
    }
    let value = invert(|x| reg_gamma_q(freedom / 2.0, x / 2.0), probability, 0.0, freedom.max(1.0), 1e60);
    if !value.is_finite() {
        return Err(domain("CHISQ.INV.RT", "the inverse did not converge"));
    }
    decimal("CHISQ.INV.RT", value)
}

fn confidence_norm(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("CONFIDENCE.NORM", &values);
    let alpha = finite_number(&a, 0)?;
    let standard_deviation = finite_number(&a, 1)?;
    let size = finite_number(&a, 2)?.trunc();
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(domain("CONFIDENCE.NORM", "Alpha must be greater than 0 and less than 1"));
    }
    if standard_deviation <= 0.0 {
        return Err(domain("CONFIDENCE.NORM", "Standard_dev must be positive"));
    }
    if size < 1.0 {
        return Err(domain("CONFIDENCE.NORM", "Size must be at least 1"));
    }
    decimal("CONFIDENCE.NORM", norm_inv(1.0 - alpha / 2.0) * standard_deviation / size.sqrt())
}

fn confidence_t(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("CONFIDENCE.T", &values);
    let alpha = finite_number(&a, 0)?;
    let standard_deviation = finite_number(&a, 1)?;
    let size = finite_number(&a, 2)?.trunc();
    if alpha <= 0.0 || alpha >= 1.0 {
        return Err(domain("CONFIDENCE.T", "Alpha must be greater than 0 and less than 1"));
    }
    if standard_deviation <= 0.0 {
        return Err(domain("CONFIDENCE.T", "Standard_dev must be positive"));
    }
    if size < 1.0 {
        return Err(domain("CONFIDENCE.T", "Size must be at least 1"));
    }
    if size == 1.0 {
        return Err(domain("CONFIDENCE.T", "Size must be greater than 1"));
    }
    decimal("CONFIDENCE.T", t_inv_upper(alpha / 2.0, size - 1.0) * standard_deviation / size.sqrt())
}

fn expon_dist(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("EXPON.DIST", &values);
    let x = finite_number(&a, 0)?;
    let lambda = finite_number(&a, 1)?;
    let cumulative = a.boolean(2)?;
    if x < 0.0 {
        return Err(domain("EXPON.DIST", "X must be non-negative"));
    }
    if lambda <= 0.0 {
        return Err(domain("EXPON.DIST", "Lambda must be positive"));
    }
    decimal("EXPON.DIST", if cumulative { -(-lambda * x).exp_m1() } else { lambda * (-lambda * x).exp() })
}

fn norm_dist(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("NORM.DIST", &values);
    let x = finite_number(&a, 0)?;
    let mean = finite_number(&a, 1)?;
    let standard_deviation = finite_number(&a, 2)?;
    let cumulative = a.boolean(3)?;
    if standard_deviation <= 0.0 {
        return Err(domain("NORM.DIST", "Standard_dev must be positive"));
    }
    let z = (x - mean) / standard_deviation;
    decimal("NORM.DIST", if cumulative { norm_cdf(z) } else { norm_pdf(z) / standard_deviation })
}

fn norm_inv_fn(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("NORM.INV", &values);
    let probability = finite_number(&a, 0)?;
    let mean = finite_number(&a, 1)?;
    let standard_deviation = finite_number(&a, 2)?;
    if standard_deviation <= 0.0 {
        return Err(domain("NORM.INV", "Standard_dev must be positive"));
    }
    if probability <= 0.0 || probability >= 1.0 {
        return Err(domain("NORM.INV", "Probability must be greater than 0 and less than 1"));
    }
    decimal("NORM.INV", mean + standard_deviation * norm_inv(probability))
}

fn norm_s_dist(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("NORM.S.DIST", &values);
    let z = finite_number(&a, 0)?;
    let cumulative = a.boolean(1)?;
    decimal("NORM.S.DIST", if cumulative { norm_cdf(z) } else { norm_pdf(z) })
}

fn norm_s_inv(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("NORM.S.INV", &values);
    let probability = finite_number(&a, 0)?;
    if probability <= 0.0 || probability >= 1.0 {
        return Err(domain("NORM.S.INV", "Probability must be greater than 0 and less than 1"));
    }
    decimal("NORM.S.INV", norm_inv(probability))
}

fn poisson_dist(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("POISSON.DIST", &values);
    let x = finite_number(&a, 0)?.trunc();
    let mean = finite_number(&a, 1)?;
    let cumulative = a.boolean(2)?;
    if x < 0.0 {
        return Err(domain("POISSON.DIST", "X must be non-negative"));
    }
    if mean < 0.0 {
        return Err(domain("POISSON.DIST", "Mean must be non-negative"));
    }
    let value = if cumulative {
        if mean == 0.0 { 1.0 } else { reg_gamma_q(x + 1.0, mean) }
    } else if mean == 0.0 {
        if x == 0.0 { 1.0 } else { 0.0 }
    } else {
        let density = (-mean + x * mean.ln() - ln_gamma(x + 1.0)).exp();
        if density.is_finite() { density } else { 0.0 }
    };
    decimal("POISSON.DIST", value)
}

fn t_dist(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("T.DIST", &values);
    let x = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.trunc();
    let cumulative = a.boolean(2)?;
    if freedom < 1.0 {
        return Err(domain("T.DIST", "Deg_freedom must be at least 1"));
    }
    decimal("T.DIST", if cumulative { t_cdf(x, freedom) } else { t_pdf(x, freedom) })
}

fn t_dist_2t(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("T.DIST.2T", &values);
    let x = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.trunc();
    if x < 0.0 {
        return Err(domain("T.DIST.2T", "X must be non-negative"));
    }
    if freedom < 1.0 {
        return Err(domain("T.DIST.2T", "Deg_freedom must be at least 1"));
    }
    decimal("T.DIST.2T", (2.0 * t_upper(x, freedom)).clamp(0.0, 1.0))
}

fn t_dist_rt(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("T.DIST.RT", &values);
    let x = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.trunc();
    if freedom < 1.0 {
        return Err(domain("T.DIST.RT", "Deg_freedom must be at least 1"));
    }
    let tail = if x >= 0.0 { t_upper(x, freedom) } else { 1.0 - t_upper(-x, freedom) };
    decimal("T.DIST.RT", tail.clamp(0.0, 1.0))
}

fn t_inv(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("T.INV", &values);
    let probability = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.trunc();
    if freedom < 1.0 {
        return Err(domain("T.INV", "Deg_freedom must be at least 1"));
    }
    if probability <= 0.0 || probability >= 1.0 {
        return Err(domain("T.INV", "Probability must be greater than 0 and less than 1"));
    }
    let value = if probability == 0.5 {
        0.0
    } else if probability < 0.5 {
        -t_inv_upper(probability, freedom)
    } else {
        t_inv_upper(1.0 - probability, freedom)
    };
    if !value.is_finite() {
        return Err(domain("T.INV", "the inverse did not converge"));
    }
    decimal("T.INV", value)
}

fn t_inv_2t(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = make_args("T.INV.2T", &values);
    let probability = finite_number(&a, 0)?;
    let freedom = finite_number(&a, 1)?.trunc();
    if freedom < 1.0 {
        return Err(domain("T.INV.2T", "Deg_freedom must be at least 1"));
    }
    if probability <= 0.0 || probability > 1.0 {
        return Err(domain("T.INV.2T", "Probability must be greater than 0 and at most 1"));
    }
    if probability == 1.0 {
        return Ok(Scalar::Decimal(0.0));
    }
    let value = t_inv_upper(probability / 2.0, freedom);
    if !value.is_finite() {
        return Err(domain("T.INV.2T", "the inverse did not converge"));
    }
    decimal("T.INV.2T", value)
}

// ---------------------------------------------------------------------------------------------
// LINEST / LINESTX
// ---------------------------------------------------------------------------------------------

struct Fit {
    slopes: Vec<f64>,
    intercept: f64,
    slope_se: Vec<f64>,
    intercept_se: Option<f64>,
    r2: Option<f64>,
    standard_error: f64,
    f: Option<f64>,
    degrees: f64,
    ssreg: f64,
    ssresid: f64,
}

fn linest(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let name = "LINEST";
    if args.len() < 2 {
        return Err(domain(name, "at least one Y column and one X column are required"));
    }
    let mut end = args.len();
    let mut constant = true;
    if args.len() > 2
        && let Some(value) = args.last().and_then(const_flag)
    {
        constant = value;
        end -= 1;
    }
    let mut table_index = None;
    let mut columns = Vec::with_capacity(end);
    for arg in &args[..end] {
        let BoundExpr::Column { table, column } = arg else {
            return Err(domain(name, "every argument must be a column reference"));
        };
        match table_index {
            None => table_index = Some(*table),
            Some(existing) if existing == *table => {}
            Some(_) => return Err(domain(name, "all column references must come from the same table")),
        }
        columns.push(*column);
    }
    if columns.len() < 2 {
        return Err(domain(name, "at least one X column is required"));
    }
    let table = table_index.ok_or_else(|| domain(name, "no table was found"))?;
    let source = ev.table(table, &ctx.filters);
    let mut y = Vec::new();
    let mut xs = vec![Vec::new(); columns.len() - 1];
    for row in &source.rows {
        let Some(y_value) = row.get(columns[0]).and_then(numeric) else { continue };
        let mut x_values = Vec::with_capacity(columns.len() - 1);
        let mut complete = true;
        for column in &columns[1..] {
            match row.get(*column).and_then(numeric) {
                Some(value) => x_values.push(value),
                None => {
                    complete = false;
                    break;
                }
            }
        }
        if !complete {
            continue;
        }
        y.push(y_value);
        for (target, value) in xs.iter_mut().zip(x_values) {
            target.push(value);
        }
    }
    let fit = regress(&y, &xs, constant)?;
    Ok(linest_table(&fit))
}

fn linestx(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let name = "LINESTX";
    if args.len() < 3 {
        return Err(domain(name, "a table, a Y expression and at least one X expression are required"));
    }
    let mut end = args.len();
    let mut constant = true;
    if args.len() > 3
        && let Some(value) = args.last().and_then(const_flag)
    {
        constant = value;
        end -= 1;
    }
    if end < 3 {
        return Err(domain(name, "at least one X expression is required"));
    }
    let table = ev.eval_table(&args[0], ctx)?;
    let y_expr = &args[1];
    let x_exprs = &args[2..end];
    let mut y = Vec::new();
    let mut xs = vec![Vec::new(); x_exprs.len()];
    for row in &table.rows {
        ctx.rows.push(frame_for(ev, &table, row));
        let mut evaluated = Vec::with_capacity(1 + x_exprs.len());
        let mut failure = None;
        for expr in std::iter::once(y_expr).chain(x_exprs.iter()) {
            match ev.eval(expr, ctx) {
                Ok(value) => evaluated.push(value),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        ctx.rows.pop();
        if let Some(error) = failure {
            return Err(error);
        }
        let mut values = Vec::with_capacity(evaluated.len());
        let mut complete = true;
        for value in &evaluated {
            match numeric(value) {
                Some(number) => values.push(number),
                None => {
                    complete = false;
                    break;
                }
            }
        }
        if !complete {
            continue;
        }
        y.push(values[0]);
        for (target, value) in xs.iter_mut().zip(&values[1..]) {
            target.push(*value);
        }
    }
    let fit = regress(&y, &xs, constant)?;
    Ok(linest_table(&fit))
}

fn linest_table(fit: &Fit) -> TableValue {
    let mut columns = Vec::new();
    let mut row = Vec::new();
    for (index, slope) in fit.slopes.iter().enumerate() {
        columns.push(TableColumn { name: format!("Slope{}", index + 1) });
        row.push(Scalar::Decimal(*slope));
    }
    columns.push(TableColumn { name: "Intercept".into() });
    row.push(Scalar::Decimal(fit.intercept));
    for (index, error) in fit.slope_se.iter().enumerate() {
        columns.push(TableColumn { name: format!("StandardErrorSlope{}", index + 1) });
        row.push(Scalar::Decimal(*error));
    }
    columns.push(TableColumn { name: "StandardErrorIntercept".into() });
    row.push(fit.intercept_se.map(Scalar::Decimal).unwrap_or(Scalar::Blank));
    columns.push(TableColumn { name: "CoefficientOfDetermination".into() });
    row.push(fit.r2.map(Scalar::Decimal).unwrap_or(Scalar::Blank));
    columns.push(TableColumn { name: "StandardError".into() });
    row.push(Scalar::Decimal(fit.standard_error));
    columns.push(TableColumn { name: "FStatistic".into() });
    row.push(fit.f.map(Scalar::Decimal).unwrap_or(Scalar::Blank));
    columns.push(TableColumn { name: "DegreesOfFreedom".into() });
    row.push(Scalar::Integer(fit.degrees as i64));
    columns.push(TableColumn { name: "RegressionSumOfSquares".into() });
    row.push(Scalar::Decimal(fit.ssreg));
    columns.push(TableColumn { name: "ResidualSumOfSquares".into() });
    row.push(Scalar::Decimal(fit.ssresid));
    TableValue::new(columns, vec![row])
}

/// Least squares fit with Excel's regression statistics. Collinear X columns are dropped
/// (their slope is 0) so a singular normal-equation matrix still produces a fit.
fn regress(y: &[f64], xs: &[Vec<f64>], constant: bool) -> Result<Fit, EvalError> {
    let k = xs.len();
    let n = y.len();
    if k == 0 || n == 0 {
        return Err(domain("LINEST", "at least one complete observation is required"));
    }
    if xs.iter().any(|column| column.len() != n) {
        return Err(domain("LINEST", "the X and Y values have different lengths"));
    }
    let nf = n as f64;
    let x_bar: Vec<f64> = if constant { xs.iter().map(|column| column.iter().sum::<f64>() / nf).collect() } else { vec![0.0; k] };
    let y_bar = if constant { y.iter().sum::<f64>() / nf } else { 0.0 };
    let mut xtx = vec![vec![0.0; k]; k];
    let mut xty = vec![0.0; k];
    for (index, y_value) in y.iter().enumerate() {
        let centered: Vec<f64> = xs
            .iter()
            .enumerate()
            .map(|(column, values)| values.get(index).copied().unwrap_or(0.0) - x_bar.get(column).copied().unwrap_or(0.0))
            .collect();
        let y_centered = y_value - y_bar;
        for (i, di) in centered.iter().enumerate() {
            if let Some(total) = xty.get_mut(i) {
                *total += di * y_centered;
            }
            for (j, dj) in centered.iter().enumerate() {
                if let Some(row) = xtx.get_mut(i)
                    && let Some(value) = row.get_mut(j)
                {
                    *value += di * dj;
                }
            }
        }
    }
    let (inverse, dropped) = inverse_symmetric(&xtx);
    let slopes: Vec<f64> = (0..k)
        .map(|i| {
            let row = inverse.get(i);
            (0..k).map(|j| row.and_then(|values| values.get(j)).copied().unwrap_or(0.0) * xty.get(j).copied().unwrap_or(0.0)).sum()
        })
        .collect();
    let intercept = if constant { y_bar - slopes.iter().zip(&x_bar).map(|(slope, mean)| slope * mean).sum::<f64>() } else { 0.0 };
    let mut ssresid = 0.0;
    let mut sstot = 0.0;
    for (index, y_value) in y.iter().enumerate() {
        let predicted = intercept + xs.iter().zip(&slopes).map(|(column, slope)| slope * column.get(index).copied().unwrap_or(0.0)).sum::<f64>();
        ssresid += (y_value - predicted).powi(2);
        sstot += if constant { (y_value - y_bar).powi(2) } else { y_value * y_value };
    }
    let used = dropped.iter().filter(|dropped| !**dropped).count() + usize::from(constant);
    let degrees = nf - used as f64;
    if degrees < 1.0 {
        return Err(domain("LINEST", "not enough observations for the number of X columns"));
    }
    let ssreg = sstot - ssresid;
    let variance = ssresid / degrees;
    let r2 = if sstot > 0.0 {
        let value = ssreg / sstot;
        value.is_finite().then_some(value)
    } else {
        None
    };
    let f = if variance > 0.0 {
        let value = (ssreg / k as f64) / variance;
        value.is_finite().then_some(value)
    } else {
        None
    };
    let slope_se: Vec<f64> = (0..k)
        .map(|i| {
            if dropped.get(i).copied().unwrap_or(true) {
                0.0
            } else {
                let diagonal = inverse.get(i).and_then(|row| row.get(i)).copied().unwrap_or(0.0);
                (diagonal * variance).max(0.0).sqrt()
            }
        })
        .collect();
    let intercept_se = if constant {
        let mut quadratic = 0.0;
        for (i, left) in x_bar.iter().enumerate() {
            for (j, right) in x_bar.iter().enumerate() {
                quadratic += left * right * inverse.get(i).and_then(|row| row.get(j)).copied().unwrap_or(0.0);
            }
        }
        Some((variance * (1.0 / nf + quadratic)).max(0.0).sqrt())
    } else {
        None
    };
    let finite = slopes.iter().all(|value| value.is_finite())
        && intercept.is_finite()
        && ssreg.is_finite()
        && ssresid.is_finite()
        && variance.is_finite()
        && slope_se.iter().all(|value| value.is_finite())
        && intercept_se.is_none_or(|value| value.is_finite());
    if !finite {
        return Err(domain("LINEST", "the regression produced a non-finite statistic"));
    }
    Ok(Fit { slopes, intercept, slope_se, intercept_se, r2, standard_error: variance.max(0.0).sqrt(), f, degrees, ssreg, ssresid })
}

/// Gauss-Jordan inverse of a symmetric positive semi-definite matrix, zeroing collinear columns.
#[allow(clippy::needless_range_loop)]
fn inverse_symmetric(matrix: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<bool>) {
    let size = matrix.len();
    let mut work = matrix.to_vec();
    let mut inverse = vec![vec![0.0; size]; size];
    for (index, row) in inverse.iter_mut().enumerate() {
        if let Some(value) = row.get_mut(index) {
            *value = 1.0;
        }
    }
    let mut dropped = vec![false; size];
    for pivot_index in 0..size {
        let pivot = work.get(pivot_index).and_then(|row| row.get(pivot_index)).copied().unwrap_or(0.0);
        let scale = matrix.get(pivot_index).and_then(|row| row.get(pivot_index)).copied().unwrap_or(0.0).abs();
        if pivot.abs() <= 1e-12 * scale || pivot.abs() < 1e-300 {
            if let Some(flag) = dropped.get_mut(pivot_index) {
                *flag = true;
            }
            for row in &mut work {
                if let Some(value) = row.get_mut(pivot_index) {
                    *value = 0.0;
                }
            }
            if let Some(row) = work.get_mut(pivot_index) {
                row.iter_mut().for_each(|value| *value = 0.0);
            }
            if let Some(row) = inverse.get_mut(pivot_index) {
                row.iter_mut().for_each(|value| *value = 0.0);
            }
            continue;
        }
        let pivot_row: Vec<f64> = work.get(pivot_index).map(|row| row.iter().map(|value| value / pivot).collect()).unwrap_or_default();
        let inverse_row: Vec<f64> = inverse.get(pivot_index).map(|row| row.iter().map(|value| value / pivot).collect()).unwrap_or_default();
        for i in 0..size {
            if i == pivot_index {
                continue;
            }
            let factor = work.get(i).and_then(|row| row.get(pivot_index)).copied().unwrap_or(0.0);
            if factor == 0.0 {
                continue;
            }
            if let Some(row) = work.get_mut(i) {
                for (value, pivot_value) in row.iter_mut().zip(&pivot_row) {
                    *value -= factor * pivot_value;
                }
            }
            if let Some(row) = inverse.get_mut(i) {
                for (value, inverse_value) in row.iter_mut().zip(&inverse_row) {
                    *value -= factor * inverse_value;
                }
            }
        }
        if let Some(row) = work.get_mut(pivot_index) {
            *row = pivot_row;
        }
        if let Some(row) = inverse.get_mut(pivot_index) {
            *row = inverse_row;
        }
    }
    (inverse, dropped)
}

// ---------------------------------------------------------------------------------------------
// Special functions
// ---------------------------------------------------------------------------------------------

const LANCZOS_G: f64 = 7.0;
const LANCZOS: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

/// ln |Γ(x)|. NaN at non-positive integers.
fn ln_gamma(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if x <= 0.0 && x == x.floor() {
        return f64::NAN;
    }
    if x < 0.5 {
        let sine = (PI * x).sin().abs();
        return (PI / sine).ln() - ln_gamma(1.0 - x);
    }
    if x < 15.0 && x == x.floor() {
        let mut factorial = 1.0;
        let mut factor = 2.0;
        while factor < x {
            factorial *= factor;
            factor += 1.0;
        }
        return factorial.ln();
    }
    let shifted = x - 1.0;
    let mut series = LANCZOS[0];
    let t = shifted + LANCZOS_G + 0.5;
    for (index, coefficient) in LANCZOS.iter().enumerate().skip(1) {
        series += coefficient / (shifted + index as f64);
    }
    0.5 * (2.0 * PI).ln() + (shifted + 0.5) * t.ln() - t + series.ln()
}

fn ln_beta(a: f64, b: f64) -> f64 {
    ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b)
}

/// Regularized lower incomplete gamma P(a, x).
fn reg_gamma_p(a: f64, x: f64) -> f64 {
    if !a.is_finite() || a <= 0.0 || x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x.is_infinite() {
        return 1.0;
    }
    if x < a + 1.0 { gamma_series(a, x) } else { 1.0 - gamma_cf(a, x) }
}

/// Regularized upper incomplete gamma Q(a, x) = 1 − P(a, x).
fn reg_gamma_q(a: f64, x: f64) -> f64 {
    if !a.is_finite() || a <= 0.0 || x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 1.0;
    }
    if x.is_infinite() {
        return 0.0;
    }
    if x < a + 1.0 { 1.0 - gamma_series(a, x) } else { gamma_cf(a, x) }
}

fn gamma_series(a: f64, x: f64) -> f64 {
    let mut ap = a;
    let mut sum = 1.0 / a;
    let mut delta = sum;
    for _ in 0..10_000 {
        ap += 1.0;
        delta *= x / ap;
        sum += delta;
        if delta.abs() < sum.abs() * 1e-17 {
            break;
        }
    }
    sum * (-x + a * x.ln() - ln_gamma(a)).exp()
}

fn gamma_cf(a: f64, x: f64) -> f64 {
    let tiny = 1e-300;
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / tiny;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..10_000 {
        let index = i as f64;
        let an = -index * (index - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < tiny {
            d = tiny;
        }
        c = b + an / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-17 {
            break;
        }
    }
    (-x + a * x.ln() - ln_gamma(a)).exp() * h
}

/// Error function.
fn erf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x.abs() < 0.5 {
        let mut sum = x;
        let mut term = x;
        let square = x * x;
        for n in 1..60 {
            term *= -square / n as f64;
            let add = term / (2 * n + 1) as f64;
            sum += add;
            if add.abs() < 1e-18 * sum.abs() {
                break;
            }
        }
        return sum * 2.0 / PI.sqrt();
    }
    let tail = reg_gamma_p(0.5, x * x);
    if x < 0.0 { -tail } else { tail }
}

/// Complementary error function, accurate in the tails.
fn erfc(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.abs() < 0.5 {
        return 1.0 - erf(x);
    }
    if x > 0.0 { reg_gamma_q(0.5, x * x) } else { 1.0 + reg_gamma_p(0.5, x * x) }
}

/// Regularized incomplete beta I_x(a, b).
fn reg_inc_beta(x: f64, a: f64, b: f64) -> f64 {
    if !a.is_finite() || a <= 0.0 || !b.is_finite() || b <= 0.0 || !(0.0..=1.0).contains(&x) {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return 1.0;
    }
    let front = (-ln_beta(a, b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) { front * beta_cf(a, b, x) / a } else { 1.0 - front * beta_cf(b, a, 1.0 - x) / b }
}

fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    let tiny = 1e-300;
    let sum = a + b;
    let a_plus = a + 1.0;
    let a_minus = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - sum * x / a_plus;
    if d.abs() < tiny {
        d = tiny;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..10_000 {
        let step = m as f64;
        let doubled = 2.0 * step;
        let first = step * (b - step) * x / ((a_minus + doubled) * (a + doubled));
        d = 1.0 + first * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + first / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        h *= d * c;
        let second = -(a + step) * (sum + step) * x / ((a + doubled) * (a_plus + doubled));
        d = 1.0 + second * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + second / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-17 {
            break;
        }
    }
    h
}

/// Standard normal density.
fn norm_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * PI).sqrt()
}

/// Standard normal cumulative distribution.
fn norm_cdf(z: f64) -> f64 {
    0.5 * erfc(-z / SQRT_2)
}

/// Inverse of the standard normal CDF.
fn norm_inv(p: f64) -> f64 {
    if !(p > 0.0 && p < 1.0) {
        return f64::NAN;
    }
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] =
        [-5.447_609_879_822_406e1, 1.615_858_368_580_409e2, -1.556_989_798_598_866e2, 6.680_131_188_771_972e1, -1.328_068_155_288_572e1];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [7.784_695_709_041_462e-3, 3.224_671_290_700_398e-1, 2.445_134_137_142_996, 3.754_408_661_907_416];
    let low = 0.02425;
    let mut x = if p < low {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5]) / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - low {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5]) / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    for _ in 0..2 {
        let error = norm_cdf(x) - p;
        let u = error * (2.0 * PI).sqrt() * (x * x / 2.0).exp();
        let next = x - u / (1.0 + x * u / 2.0);
        if !next.is_finite() {
            break;
        }
        x = next;
    }
    x
}

/// Finds `x` in `[lo, hi]` with `f(x) = target` for a monotone `f`. `hi` is expanded up to
/// `hi_max` while the root is not bracketed. Returns NaN when no root is bracketed.
fn invert(f: impl Fn(f64) -> f64, target: f64, lo: f64, hi: f64, hi_max: f64) -> f64 {
    let (mut lo, mut hi) = (lo, hi);
    let mut f_lo = f(lo) - target;
    let mut f_hi = f(hi) - target;
    let mut expansions = 0;
    while f_lo * f_hi > 0.0 && hi < hi_max && expansions < 200 {
        lo = hi;
        hi = (hi * 2.0).min(hi_max);
        f_lo = f(lo) - target;
        f_hi = f(hi) - target;
        expansions += 1;
    }
    if f_lo == 0.0 {
        return lo;
    }
    if f_hi == 0.0 {
        return hi;
    }
    if f_lo * f_hi > 0.0 || f_lo.is_nan() || f_hi.is_nan() {
        return f64::NAN;
    }
    let lo_negative = f_lo < 0.0;
    for _ in 0..300 {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi {
            break;
        }
        let f_mid = f(mid) - target;
        if f_mid == 0.0 {
            return mid;
        }
        if (f_mid < 0.0) == lo_negative {
            lo = mid;
        } else {
            hi = mid;
        }
        if (hi - lo).abs() <= 1e-15 * mid.abs().max(1e-300) {
            break;
        }
    }
    0.5 * (lo + hi)
}

/// P(T > t) for t ≥ 0.
fn t_upper(t: f64, freedom: f64) -> f64 {
    0.5 * reg_inc_beta(freedom / (freedom + t * t), freedom / 2.0, 0.5)
}

fn t_cdf(t: f64, freedom: f64) -> f64 {
    if t >= 0.0 { 1.0 - t_upper(t, freedom) } else { t_upper(-t, freedom) }
}

fn t_pdf(t: f64, freedom: f64) -> f64 {
    (ln_gamma((freedom + 1.0) / 2.0) - ln_gamma(freedom / 2.0) - 0.5 * (freedom * PI).ln() - (freedom + 1.0) / 2.0 * (1.0 + t * t / freedom).ln())
        .exp()
}

/// The non-negative t with P(T > t) = q, for q in (0, 0.5].
fn t_inv_upper(q: f64, freedom: f64) -> f64 {
    if q >= 0.5 {
        return 0.0;
    }
    invert(|t| t_upper(t, freedom), q, 0.0, 1.0, 1e60)
}
