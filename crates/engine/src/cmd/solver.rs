//! Solver (Data › Analysis › Solver): finds values for variable cells that maximise, minimise
//! or hit a value in an objective cell, subject to constraints.
//!
//! The workbook is the model: every evaluation sets the variable cells on a copy of the
//! workbook and recalculates their dependents ([`Trial`]), so any formula works. Two methods:
//!
//! - **Simplex LP**: the model is linearised from `n + 1` evaluations and checked for linearity
//!   at one more point; then a dense two-phase simplex (Dantzig's rule, Bland's rule when it
//!   stalls) solves it exactly. Integer and binary constraints use depth-first branch and bound.
//! - **GRG Nonlinear** (the name Excel's dialog uses): GridCraft's method is *not* a generalised
//!   reduced gradient. It is an augmented-Lagrangian method (Powell–Hestenes–Rockafellar) whose
//!   inner problems are solved by projected gradient descent with Barzilai–Borwein steps and an
//!   Armijo line search, on central-difference gradients. Variable bounds are kept by
//!   projection. It finds a local optimum of a smooth model; integer constraints use a small
//!   branch and bound on top.
//!
//! - **Evolutionary**: a genetic search for models that aren't smooth (IF, lookups, rounding),
//!   with tournament selection, blend crossover and Gaussian mutation inside the variable bounds,
//!   feasibility-first ranking and integer rounding, followed by a compass (pattern) search around
//!   the best member. It stops when the population has converged, when the best member stops
//!   improving, or at the evaluation budget.
//!
//! Work is bounded by an evaluation budget (no clock: `Instant` traps on wasm). The model is
//! saved as hidden sheet-scoped names (`solver_opt`, `solver_adj`, `solver_lhs1`…), so it
//! travels with the workbook through XLSX.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Cell, DefinedName, Sheet, Workbook};
use serde_json::{Value as Json, json};

use super::check_arrays;
use super::whatif::Trial;
use super::{CommandSpec, Ctx, DocState, EngineError, Result, Session, bad, bool_param, cmd, edit, f64_param, has_doc, str_param};
use gridcraft_core::split_sheet;

mod evolutionary;
mod lp;
mod model;
mod nlp;
mod problem;
mod reports;

use evolutionary::{EvoEnd, evolutionary};
use lp::{Outcome, Sense, branch_and_bound, linearize, solve_lp};
pub use model::saved_model;
use model::{Goal, Method, Model, model_params, parse_model, save_model};
use nlp::{nlp, project};
use problem::{Evaluator, as_num, build_problem};
use reports::{ReportInput, insert_report_selections, prepare_reports, report, report_kinds, write_reports};

const CMD: &str = "data.solver";
const REPORT_CMD: &str = "data.solverReport";
/// Excel's limit on decision variables.
const MAX_VARS: usize = 200;
/// Most scalar constraints (after expanding ranges).
const MAX_CONSTRAINTS: usize = 1000;
const DEFAULT_EVALS: u64 = 20_000;
const MAX_EVALS: u64 = 200_000;
/// Most simplex pivots × tableau cells per solve.
const LP_WORK: u64 = 2_000_000_000;
const MAX_NODES_LP: usize = 10_000;
const MAX_NODES_NLP: usize = 200;
/// Evolutionary population sizes.
const MIN_POPULATION: usize = 10;
const MAX_POPULATION: usize = 1000;
/// Constraint precision (Excel's default 0.000001).
const PRECISION: f64 = 1e-6;
const EPS: f64 = 1e-9;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "data.solver",
            "Solver…",
            ["Data", "Analysis"],
            None,
            "{objective: \"B10\" (formula cell), goal?: max|min|value (default max), value?: number (for goal value), variables: \"B2:B4,D1\" (≤200 constant cells, same sheet), constraints?: [\"B5 <= 10\", \"B2:B4 >= C2:C4\", \"B2 int\", \"B3 bin\", {lhs: \"B6\", op: \"=\", rhs: 3 | \"C6\"}], nonNegative?: bool (default true: variables without a lower bound get 0), method?: grg|simplex|evolutionary (default grg), populationSize?: 100, mutationRate?: 0.075, randomSeed?: 0 (a fixed seed, so runs repeat), convergence?: 0.0001, generationsWithoutImprovement?: 50, requireBounds?: bool (default true: every variable needs both bounds; false searches within ±100 of the start) (Evolutionary options), keep?: bool (default true; false restores the original values), saveModel?: bool (default true: hidden solver_* names), maxEvaluations?: 20000, integerOptimality?: percent (default 0), saved?: bool (solve the sheet's saved model; other params override), reports?: [answer|limits] (written on new sheets when a solution is found)} → {status: optimal|converged|cannotImprove|iterationLimit|infeasible|unbounded|notLinear, message, objective, values: {cell: value}, original: {cell: value}, model: the model as params, evaluations, changed, reports?: [sheet names]}. No `objective` → {model: the saved model} (the menu opens the dialog).",
            has_doc,
            run
        ),
        cmd!(
            "data.solverReport",
            "Solver Reports",
            [],
            None,
            "{reports: [answer|limits], values?: {cell: number} (the solution; default the variable cells' current values), original?: {cell: number} (the values before solving; default `values`), message?: text (the result line), model?: data.solver params (default the sheet's saved model, with any data.solver params given here over it)} → {sheets: [names]}. Writes Answer and Limits reports on new sheets before the model's sheet; Limits needs Simplex LP or GRG Nonlinear and no integer constraints. data.solver's result can be passed as is.",
            has_doc,
            report
        ),
    ]
}

/// What a solve ended with, and why Simplex LP or Evolutionary stopped.
struct Solve {
    outcome: Outcome,
    not_linear: bool,
    evo_end: Option<EvoEnd>,
}

/// Runs the model's method for `goal` within `lo..hi` from `x0`.
#[allow(clippy::too_many_arguments)]
fn optimise(ev: &mut Evaluator, m: &Model, goal: Goal, x0: &[f64], lo: &[f64], hi: &[f64], integer: &[bool], gap: f64) -> Solve {
    let any_int = integer.iter().any(|b| *b);
    let mut not_linear = false;
    let mut evo_end = None;
    let outcome = if lo.iter().zip(hi).any(|(l, h)| l > h) {
        Outcome::Infeasible
    } else {
        match m.method {
            Method::Simplex => match linearize(ev, x0) {
                Err("nonlinear") | Err("error") => {
                    not_linear = true;
                    Outcome::Infeasible
                }
                Err(_) => Outcome::Limit,
                Ok(((cf, f_const), lin)) => {
                    let mut rows: Vec<(Vec<f64>, Sense, f64)> = lin.iter().zip(&ev.rows).map(|((a, k), r)| (a.clone(), r.sense, -k)).collect();
                    let c: Vec<f64> = match goal {
                        Goal::Max => cf.iter().map(|v| -v).collect(),
                        Goal::Min => cf.clone(),
                        Goal::Value(v) => {
                            rows.push((cf.clone(), Sense::Eq, v - f_const));
                            vec![0.0; cf.len()]
                        }
                    };
                    let size = (rows.len() as u64 + 2 * x0.len() as u64 + 1) * (4 * x0.len() as u64 + 2 * rows.len() as u64 + 1);
                    let mut work = LP_WORK.max(size);
                    let mut relax = |l: &[f64], h: &[f64]| solve_lp(&c, &rows, l, h, &mut work);
                    if any_int { branch_and_bound(lo, hi, integer, MAX_NODES_LP, gap, &mut relax).0 } else { relax(lo, hi) }
                }
            },
            Method::Grg => {
                if any_int {
                    let mut relax = |l: &[f64], h: &[f64]| {
                        let mut start = x0.to_vec();
                        project(&mut start, l, h);
                        nlp(ev, &start, l, h, goal)
                    };
                    branch_and_bound(lo, hi, integer, MAX_NODES_NLP, gap, &mut relax).0
                } else {
                    nlp(ev, x0, lo, hi, goal)
                }
            }
            Method::Evolutionary => {
                let (out, end) = evolutionary(ev, x0, lo, hi, integer, goal, &m.evo);
                evo_end = Some(end);
                out
            }
        }
    };
    Solve { outcome, not_linear, evo_end }
}

// ---------------------------------------------------------------- command

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let active = d.wb.active_sheet();
    let mut params = p.clone();
    if bool_param(p, "saved").unwrap_or(false) {
        let saved = saved_model(&d.wb, active).ok_or_else(|| EngineError::Other("This sheet has no saved Solver model.".into()))?;
        if let (Some(base), Some(over)) = (saved.as_object(), p.as_object()) {
            let mut merged = base.clone();
            for (k, v) in over {
                merged.insert(k.clone(), v.clone());
            }
            params = Json::Object(merged);
        }
    }
    let has_objective = ["objective", "setObjective", "set"].iter().any(|k| str_param(&params, k).is_some_and(|t| !t.trim().is_empty()));
    if !has_objective {
        let model = saved_model(&d.wb, active);
        s.ui_requests.push(crate::UiRequest::Dialog("solver".into(), json!({})));
        return Ok(json!({"model": model}));
    }
    let model = parse_model(s, &params)?;
    let budget = params
        .get("maxEvaluations")
        .and_then(Json::as_f64)
        .filter(|v| v.is_finite() && *v >= 1.0)
        .map_or(DEFAULT_EVALS, |v| (v as u64).min(MAX_EVALS));
    let gap = params.get("integerOptimality").and_then(Json::as_f64).filter(|v| v.is_finite()).map_or(0.0, |v| v.clamp(0.0, 100.0) / 100.0);
    let keep = bool_param(&params, "keep").unwrap_or(true);
    let save = bool_param(&params, "saveModel").unwrap_or(true);
    let reports = report_kinds(CMD, &params)?;
    let prob = build_problem(&model);
    let d = s.doc()?;
    let sh = d.wb.sheet(model.sheet).ok_or(EngineError::NoDocument)?;
    let x0: Vec<f64> = model.vars.iter().map(|c| as_num(&sh.value(*c))).map(|v| if v.is_finite() { v } else { 0.0 }).collect();
    let mut ev = Evaluator {
        trial: Trial::new(d),
        sheet: model.sheet,
        vars: model.vars.clone(),
        objective: model.objective,
        rows: prob.rows.clone(),
        evals: 0,
        budget,
        target: None,
    };
    if model.method == Method::Evolutionary && model.evo.require_bounds && prob.lo.iter().chain(&prob.hi).any(|b| !b.is_finite()) {
        return Err(EngineError::Other(
            "The Evolutionary method needs a lower and an upper bound on every variable cell (constraints such as \"A1:A3 <= 10\"), or requireBounds: false."
                .into(),
        ));
    }
    let Solve { outcome, not_linear, evo_end } = optimise(&mut ev, &model, model.goal, &x0, &prob.lo, &prob.hi, &prob.integer, gap);
    let (status, message, solution) = match outcome {
        _ if not_linear => ("notLinear", "The model isn't linear in the variable cells, so Simplex LP can't solve it. Try GRG Nonlinear.", None),
        Outcome::Solved(x, _, true) if model.method == Method::Evolutionary => match evo_end {
            Some(EvoEnd::Stalled) => {
                ("cannotImprove", "Solver stopped when the best solution stopped improving. All constraints are satisfied.", Some(x))
            }
            _ => ("converged", "The population converged. The best solution found satisfies all constraints.", Some(x)),
        },
        Outcome::Solved(x, _, true) => (
            if model.method == Method::Simplex { "optimal" } else { "converged" },
            if model.method == Method::Simplex {
                "Solver found an optimal solution. All constraints are satisfied."
            } else {
                "Solver converged to a locally optimal solution. All constraints are satisfied."
            },
            Some(x),
        ),
        Outcome::Solved(x, _, false) => {
            ("iterationLimit", "Solver stopped at its evaluation limit; the best solution found satisfies the constraints.", Some(x))
        }
        Outcome::Limit => ("iterationLimit", "Solver stopped at its evaluation limit without finding a feasible solution.", None),
        Outcome::Infeasible => ("infeasible", "Solver could not find a solution that satisfies all constraints.", None),
        Outcome::Unbounded => ("unbounded", "The objective can grow without limit: the problem is unbounded.", None),
    };
    let evaluations = ev.evals;
    // The objective's value at the solution.
    let objective = solution.as_ref().and_then(|x| {
        ev.budget = ev.budget.saturating_add(1);
        ev.eval(x).map(|p| p.f)
    });
    let mut values = serde_json::Map::new();
    if let Some(x) = &solution {
        for (c, v) in model.vars.iter().zip(x) {
            values.insert(c.a1(), json!(v));
        }
    }
    let original: serde_json::Map<String, Json> = model.vars.iter().zip(&x0).map(|(c, v)| (c.a1(), json!(v))).collect();
    let changed = keep && solution.is_some();
    let built = match (&reports, &solution) {
        (Some(kinds), Some(x)) => {
            let inp = ReportInput { model: &model, prob: &prob, original: x0, solution: x.clone(), message: message.to_string(), budget };
            prepare_reports(s.doc()?, kinds, &inp)?
        }
        _ => Vec::new(),
    };
    let count = built.len();
    let before = s.doc()?.wb.clone();
    let mut out = json!({
        "status": status,
        "message": message,
        "objective": objective,
        "values": values,
        "original": original,
        "model": model_params(&before, &model),
        "evaluations": evaluations,
        "changed": changed,
    });
    let names = if changed || save || count > 0 {
        let sheet = model.sheet;
        edit(s, |cx| {
            if changed && let Some(x) = &solution {
                let sh = cx.sheet_mut(sheet)?;
                for (c, v) in model.vars.iter().zip(x) {
                    let style = sh.style_id(*c);
                    sh.set_cell(*c, Cell { value: Value::number(*v), formula: None, style });
                }
                for c in &model.vars {
                    cx.touch(sheet + count, *c);
                }
            }
            if save {
                save_model(&mut cx.wb, &model);
            }
            if count > 0 {
                // Inserting sheets makes this structural, so retain the variable edit's array check.
                check_arrays(&before, &cx.wb)?;
            }
            Ok(write_reports(cx, sheet, built))
        })?
    } else {
        Vec::new()
    };
    insert_report_selections(s, model.sheet, count);
    if reports.is_some() && solution.is_some() {
        out["reports"] = json!(names);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
