use super::{
    CMD, CellRef, DefinedName, EngineError, Json, MAX_CONSTRAINTS, MAX_POPULATION, MAX_VARS, MIN_POPULATION, RangeRef, Result, Session, Workbook,
    bad, bool_param, f64_param, json, split_sheet, str_param,
};

// ---------------------------------------------------------------- model

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Rel {
    Le,
    Eq,
    Ge,
    Int,
    Bin,
}

impl Rel {
    fn parse(s: &str) -> Option<Rel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "<=" | "≤" | "=<" | "le" => Some(Rel::Le),
            ">=" | "≥" | "=>" | "ge" => Some(Rel::Ge),
            "=" | "==" | "eq" => Some(Rel::Eq),
            "int" | "integer" => Some(Rel::Int),
            "bin" | "binary" => Some(Rel::Bin),
            _ => None,
        }
    }
    pub(super) fn text(self) -> &'static str {
        match self {
            Rel::Le => "<=",
            Rel::Eq => "=",
            Rel::Ge => ">=",
            Rel::Int => "int",
            Rel::Bin => "bin",
        }
    }
    /// The relation code of the saved model (`solver_relN`), as in Solver's VBA `SolverAdd`.
    fn code(self) -> u8 {
        match self {
            Rel::Le => 1,
            Rel::Eq => 2,
            Rel::Ge => 3,
            Rel::Int => 4,
            Rel::Bin => 5,
        }
    }
    fn from_code(c: &str) -> Option<Rel> {
        match c.trim() {
            "1" => Some(Rel::Le),
            "2" => Some(Rel::Eq),
            "3" => Some(Rel::Ge),
            "4" => Some(Rel::Int),
            "5" => Some(Rel::Bin),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum Rhs {
    None,
    Const(f64),
    Cells(Vec<CellRef>),
}

#[derive(Clone, Debug)]
pub(super) struct Constraint {
    pub(super) lhs: Vec<CellRef>,
    lhs_text: String,
    pub(super) rel: Rel,
    pub(super) rhs: Rhs,
    rhs_text: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Goal {
    Max,
    Min,
    Value(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Method {
    Simplex,
    Grg,
    Evolutionary,
}

/// The Evolutionary method's options (Solver Options › Evolutionary). The search keeps no
/// clock, so the time without improvement is counted in generations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EvoOptions {
    pub(super) population: usize,
    pub(super) mutation: f64,
    pub(super) seed: u64,
    pub(super) convergence: f64,
    pub(super) stall: u64,
    pub(super) require_bounds: bool,
}

impl Default for EvoOptions {
    fn default() -> EvoOptions {
        EvoOptions { population: 100, mutation: 0.075, seed: 0, convergence: 1e-4, stall: 50, require_bounds: true }
    }
}

fn evo_options(p: &Json) -> EvoOptions {
    let d = EvoOptions::default();
    let num = |k: &str| p.get(k).and_then(Json::as_f64).or_else(|| str_param(p, k).and_then(num_text)).filter(|v| v.is_finite());
    EvoOptions {
        population: num("populationSize").map_or(d.population, |v| v.clamp(MIN_POPULATION as f64, MAX_POPULATION as f64) as usize),
        mutation: num("mutationRate").map_or(d.mutation, |v| v.clamp(0.0, 1.0)),
        seed: num("randomSeed").map_or(d.seed, |v| v.clamp(0.0, f64::from(u32::MAX)) as u64),
        convergence: num("convergence").map_or(d.convergence, |v| v.clamp(0.0, 1.0)),
        stall: num("generationsWithoutImprovement").map_or(d.stall, |v| v.clamp(1.0, 100_000.0) as u64),
        require_bounds: bool_param(p, "requireBounds").unwrap_or(d.require_bounds),
    }
}

#[derive(Clone, Debug)]
pub(super) struct Model {
    pub(super) sheet: usize,
    pub(super) objective: CellRef,
    pub(super) goal: Goal,
    pub(super) vars: Vec<CellRef>,
    vars_text: String,
    pub(super) cons: Vec<Constraint>,
    pub(super) non_negative: bool,
    pub(super) method: Method,
    pub(super) evo: EvoOptions,
}

/// Cells of `"B2:B4,D1"` on one sheet (row-major per area, in order), at most `cap`.
fn cells_of(wb: &Workbook, t: &str, default_sheet: usize, cap: usize) -> Result<(usize, Vec<CellRef>)> {
    let mut sheet = None;
    let mut out = Vec::new();
    for part in t.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let (sh, body) = split_sheet(part);
        let si = match sh {
            Some(n) => wb.sheet_index(&n).ok_or_else(|| bad(CMD, format!("no sheet named `{n}`")))?,
            None => default_sheet,
        };
        if *sheet.get_or_insert(si) != si {
            return Err(bad(CMD, "all cells of a reference must be on one sheet"));
        }
        let r = RangeRef::parse(body.trim()).ok_or_else(|| bad(CMD, format!("not a reference: `{part}`")))?;
        if r.count() > cap as u64 || out.len() + r.count() as usize > cap {
            return Err(bad(CMD, format!("too many cells (at most {cap})")));
        }
        out.extend(r.iter());
    }
    if out.is_empty() {
        return Err(bad(CMD, format!("not a reference: `{t}`")));
    }
    Ok((sheet.unwrap_or(default_sheet), out))
}

fn num_text(t: &str) -> Option<f64> {
    gridcraft_core::parse::parse_number_text(t.trim()).filter(|v| v.is_finite())
}

/// Splits `"B5 <= 10"` / `"B2 int"` into (lhs, relation, rhs).
fn split_constraint(t: &str) -> Option<(String, Rel, String)> {
    for op in ["<=", ">=", "=<", "=>", "≤", "≥", "=="] {
        if let Some((a, b)) = t.split_once(op) {
            return Some((a.trim().into(), Rel::parse(op)?, b.trim().into()));
        }
    }
    if let Some((a, b)) = t.split_once('=') {
        return Some((a.trim().into(), Rel::Eq, b.trim().into()));
    }
    let lower = t.trim().to_ascii_lowercase();
    for (suffix, rel) in [("integer", Rel::Int), ("binary", Rel::Bin), ("int", Rel::Int), ("bin", Rel::Bin)] {
        if let Some(a) = lower.strip_suffix(suffix) {
            let a = a.trim();
            return Some((t.trim().get(..a.len())?.trim().to_string(), rel, String::new()));
        }
    }
    None
}

fn parse_constraint(wb: &Workbook, sheet: usize, c: &Json) -> Result<Constraint> {
    let (lhs_text, rel, rhs_json) = match c {
        Json::String(t) => {
            let (l, r, rhs) = split_constraint(t).ok_or_else(|| bad(CMD, format!("not a constraint: `{t}`")))?;
            (l, r, Json::String(rhs))
        }
        Json::Object(_) => {
            let lhs = str_param(c, "lhs").or_else(|| str_param(c, "cell")).or_else(|| str_param(c, "range")).unwrap_or("").to_string();
            let op = str_param(c, "op").or_else(|| str_param(c, "relation")).unwrap_or("");
            let rel = Rel::parse(op).ok_or_else(|| bad(CMD, format!("unknown constraint relation `{op}`")))?;
            (lhs, rel, c.get("rhs").or_else(|| c.get("value")).cloned().unwrap_or(Json::Null))
        }
        _ => return Err(bad(CMD, "a constraint is a string like \"B5 <= 10\" or {lhs, op, rhs}")),
    };
    let (lsi, lhs) = cells_of(wb, &lhs_text, sheet, MAX_CONSTRAINTS)?;
    if lsi != sheet {
        return Err(EngineError::Other("Constraint cells must be on the objective cell's sheet.".into()));
    }
    let (rhs, rhs_text) = match rel {
        Rel::Int | Rel::Bin => (Rhs::None, String::new()),
        _ => match &rhs_json {
            Json::Number(n) => {
                let v = n.as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(CMD, "the constraint's right side must be a number"))?;
                (Rhs::Const(v), v.to_string())
            }
            Json::String(t) => {
                let t = t.trim().trim_start_matches('=').trim();
                if let Some(v) = num_text(t) {
                    (Rhs::Const(v), t.to_string())
                } else {
                    let (rsi, cells) = cells_of(wb, t, sheet, MAX_CONSTRAINTS)?;
                    if rsi != sheet {
                        return Err(EngineError::Other("Constraint cells must be on the objective cell's sheet.".into()));
                    }
                    if cells.len() != 1 && cells.len() != lhs.len() {
                        return Err(EngineError::Other("The constraint's two sides must be the same size (or the right side one cell).".into()));
                    }
                    (Rhs::Cells(cells), t.to_string())
                }
            }
            _ => return Err(bad(CMD, "missing the constraint's right side")),
        },
    };
    Ok(Constraint { lhs, lhs_text, rel, rhs, rhs_text })
}

pub(super) fn parse_model(s: &Session, p: &Json) -> Result<Model> {
    let d = s.doc()?;
    let wb = &d.wb;
    let obj_text = str_param(p, "objective").or_else(|| str_param(p, "setObjective")).or_else(|| str_param(p, "set")).unwrap_or("");
    let (sheet, obj) = cells_of(wb, obj_text, wb.active_sheet(), 1).map_err(|_| EngineError::Other("The objective must be a single cell.".into()))?;
    let objective = *obj.first().ok_or_else(|| EngineError::Other("The objective must be a single cell.".into()))?;
    let sh = wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    if sh.cell(objective).is_none_or(|c| c.formula.is_none()) {
        return Err(EngineError::Other("The objective cell must contain a formula.".into()));
    }
    let goal = match str_param(p, "goal").or_else(|| str_param(p, "to")).unwrap_or("max").to_ascii_lowercase().as_str() {
        "max" | "maximize" | "maximum" => Goal::Max,
        "min" | "minimize" | "minimum" => Goal::Min,
        "value" | "valueof" | "value of" => {
            let v = f64_param(p, "value")
                .or_else(|| str_param(p, "value").and_then(num_text))
                .filter(|v| v.is_finite())
                .ok_or_else(|| bad(CMD, "goal `value` needs a number in `value`"))?;
            Goal::Value(v)
        }
        g => return Err(bad(CMD, format!("unknown goal `{g}` (max, min or value)"))),
    };
    let vars_text = str_param(p, "variables").or_else(|| str_param(p, "changing")).or_else(|| str_param(p, "byChanging")).unwrap_or("").to_string();
    let (vsi, mut vars) = cells_of(wb, &vars_text, sheet, MAX_VARS).map_err(|e| match e {
        EngineError::BadParams { msg, .. } if msg.starts_with("too many") => {
            EngineError::Other(format!("Too many variable cells (at most {MAX_VARS})."))
        }
        _ => EngineError::Other("The variable cells must be a reference like B2:B4.".into()),
    })?;
    if vsi != sheet {
        return Err(EngineError::Other("The variable cells must be on the objective cell's sheet.".into()));
    }
    let mut seen = std::collections::HashSet::new();
    vars.retain(|c| seen.insert(*c));
    if vars.iter().any(|c| sh.cell(*c).is_some_and(|c| c.formula.is_some())) {
        return Err(EngineError::Other("Variable cells must hold constants, not formulas.".into()));
    }
    let mut cons = Vec::new();
    let mut scalar = 0usize;
    if let Some(list) = p.get("constraints") {
        let list = list.as_array().ok_or_else(|| bad(CMD, "`constraints` must be an array"))?;
        if list.len() > MAX_CONSTRAINTS {
            return Err(EngineError::Other(format!("Too many constraints (at most {MAX_CONSTRAINTS}).")));
        }
        for c in list {
            let c = parse_constraint(wb, sheet, c)?;
            scalar = scalar.saturating_add(c.lhs.len());
            if scalar > MAX_CONSTRAINTS {
                return Err(EngineError::Other(format!("Too many constraints (at most {MAX_CONSTRAINTS} cells).")));
            }
            if matches!(c.rel, Rel::Int | Rel::Bin) && c.lhs.iter().any(|x| !vars.contains(x)) {
                return Err(EngineError::Other("Integer and binary constraints must be on variable cells.".into()));
            }
            cons.push(c);
        }
    }
    let non_negative = bool_param(p, "nonNegative").unwrap_or(true);
    let method = match str_param(p, "method").unwrap_or("grg").to_ascii_lowercase().replace([' ', '_', '-'], "").as_str() {
        "grg" | "grgnonlinear" | "nonlinear" => Method::Grg,
        "simplex" | "simplexlp" | "lp" | "linear" => Method::Simplex,
        "evolutionary" | "evo" | "genetic" => Method::Evolutionary,
        m => return Err(bad(CMD, format!("unknown method `{m}` (grg, simplex or evolutionary)"))),
    };
    Ok(Model { sheet, objective, goal, vars, vars_text, cons, non_negative, method, evo: evo_options(p) })
}

// ---------------------------------------------------------------- saved model

fn ref_text(sheet_name: &str, cells: &str) -> String {
    let q = crate::pivot::quote_sheet(sheet_name);
    cells
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .filter_map(|part| {
            let r = RangeRef::parse(split_sheet(part).1.trim())?;
            let abs = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row as u64 + 1);
            Some(if r.is_single() { format!("{q}!{}", abs(r.start)) } else { format!("{q}!{}:{}", abs(r.start), abs(r.end)) })
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The model as hidden names scoped to its sheet (replacing any saved before).
pub(super) fn save_model(wb: &mut Workbook, m: &Model) {
    let Some(sheet_name) = wb.sheet(m.sheet).map(|s| s.name.clone()) else { return };
    wb.names.retain(|n| !(n.scope == Some(m.sheet) && n.name.to_ascii_lowercase().starts_with("solver_")));
    let mut put = |name: String, formula: String| {
        wb.names.push(DefinedName { name, scope: Some(m.sheet), formula, comment: String::new(), hidden: true });
    };
    put("solver_opt".into(), ref_text(&sheet_name, &m.objective.a1()));
    let (typ, val) = match m.goal {
        Goal::Max => (1, 0.0),
        Goal::Min => (2, 0.0),
        Goal::Value(v) => (3, v),
    };
    put("solver_typ".into(), typ.to_string());
    put("solver_val".into(), val.to_string());
    put("solver_adj".into(), ref_text(&sheet_name, &m.vars_text));
    for (i, c) in m.cons.iter().enumerate() {
        let k = i + 1;
        put(format!("solver_lhs{k}"), ref_text(&sheet_name, &c.lhs_text));
        put(format!("solver_rel{k}"), c.rel.code().to_string());
        let rhs = match &c.rhs {
            Rhs::None => {
                if c.rel == Rel::Bin {
                    "\"binary\"".to_string()
                } else {
                    "\"integer\"".to_string()
                }
            }
            Rhs::Const(v) => v.to_string(),
            Rhs::Cells(_) => ref_text(&sheet_name, &c.rhs_text),
        };
        put(format!("solver_rhs{k}"), rhs);
    }
    put("solver_num".into(), m.cons.len().to_string());
    put("solver_neg".into(), if m.non_negative { "1" } else { "2" }.into());
    let eng = match m.method {
        Method::Grg => "1",
        Method::Simplex => "2",
        Method::Evolutionary => "3",
    };
    put("solver_eng".into(), eng.into());
    if m.method == Method::Evolutionary {
        put("solver_ssz".into(), m.evo.population.to_string());
        put("solver_mrt".into(), m.evo.mutation.to_string());
        put("solver_rsd".into(), m.evo.seed.to_string());
        put("solver_rbv".into(), if m.evo.require_bounds { "1" } else { "2" }.into());
    }
    put("solver_ver".into(), "3".into());
}

/// The saved model of a sheet as `data.solver` params, if any.
pub fn saved_model(wb: &Workbook, sheet: usize) -> Option<Json> {
    let get = |n: &str| {
        wb.names
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(n) && (d.scope == Some(sheet) || d.scope.is_none()))
            .map(|d| d.formula.trim().trim_start_matches('=').to_string())
    };
    let local = |t: String| -> String { t.split(',').map(|p| split_sheet(p.trim()).1.replace('$', "")).collect::<Vec<_>>().join(",") };
    let objective = local(get("solver_opt")?);
    if objective.is_empty() {
        return None;
    }
    let goal = match get("solver_typ").as_deref().map(str::trim) {
        Some("2") => "min",
        Some("3") => "value",
        _ => "max",
    };
    let value = get("solver_val").and_then(|v| num_text(&v)).unwrap_or(0.0);
    let count = get("solver_num").and_then(|v| num_text(&v)).unwrap_or(0.0).clamp(0.0, MAX_CONSTRAINTS as f64) as usize;
    let mut constraints = Vec::new();
    for k in 1..=count {
        let (Some(lhs), Some(rel)) = (get(&format!("solver_lhs{k}")), get(&format!("solver_rel{k}")).and_then(|r| Rel::from_code(&r))) else {
            continue;
        };
        let lhs = local(lhs);
        constraints.push(match rel {
            Rel::Int | Rel::Bin => json!(format!("{lhs} {}", rel.text())),
            _ => {
                let rhs = get(&format!("solver_rhs{k}")).unwrap_or_default();
                let rhs = if num_text(&rhs).is_some() { rhs } else { local(rhs) };
                json!(format!("{lhs} {} {rhs}", rel.text()))
            }
        });
    }
    let method = match get("solver_eng").as_deref().map(str::trim) {
        Some("2") => "simplex",
        Some("3") => "evolutionary",
        _ => "grg",
    };
    let mut model = json!({
        "objective": objective,
        "goal": goal,
        "value": value,
        "variables": get("solver_adj").map(local).unwrap_or_default(),
        "constraints": constraints,
        "nonNegative": get("solver_neg").as_deref().map(str::trim) != Some("2"),
        "method": method,
    });
    for (name, key) in [("solver_ssz", "populationSize"), ("solver_mrt", "mutationRate"), ("solver_rsd", "randomSeed")] {
        if let Some(v) = get(name).and_then(|v| num_text(&v)) {
            model[key] = json!(v);
        }
    }
    if let Some(v) = get("solver_rbv") {
        model["requireBounds"] = json!(v.trim() != "2");
    }
    Some(model)
}

/// The model as `data.solver` params, in the form [`saved_model`] returns (the objective
/// qualified by its sheet).
pub(super) fn model_params(wb: &Workbook, m: &Model) -> Json {
    let sheet = wb.sheet(m.sheet).map(|s| crate::pivot::quote_sheet(&s.name)).unwrap_or_default();
    let (goal, value) = match m.goal {
        Goal::Max => ("max", 0.0),
        Goal::Min => ("min", 0.0),
        Goal::Value(v) => ("value", v),
    };
    let constraints: Vec<Json> = m
        .cons
        .iter()
        .map(|c| match c.rel {
            Rel::Int | Rel::Bin => json!(format!("{} {}", c.lhs_text, c.rel.text())),
            _ => json!(format!("{} {} {}", c.lhs_text, c.rel.text(), c.rhs_text)),
        })
        .collect();
    let method = match m.method {
        Method::Simplex => "simplex",
        Method::Grg => "grg",
        Method::Evolutionary => "evolutionary",
    };
    let mut out = json!({
        "objective": format!("{sheet}!{}", m.objective.a1()),
        "goal": goal,
        "value": value,
        "variables": m.vars_text,
        "constraints": constraints,
        "nonNegative": m.non_negative,
        "method": method,
    });
    if m.method == Method::Evolutionary {
        let e = &m.evo;
        out["populationSize"] = json!(e.population);
        out["mutationRate"] = json!(e.mutation);
        out["randomSeed"] = json!(e.seed);
        out["convergence"] = json!(e.convergence);
        out["generationsWithoutImprovement"] = json!(e.stall);
        out["requireBounds"] = json!(e.require_bounds);
    }
    out
}
