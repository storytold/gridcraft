//! Analysis ToolPak (Data › Analysis › Data Analysis): statistical tools that compute a report
//! from input ranges and write it as a static table (values, not formulas) to an output range or
//! a new worksheet.
//!
//! Every tool is one `tool` value of `data.analysisToolPak`. Distribution values (t, F) come
//! from the worksheet functions (`T.DIST.2T`, `F.DIST.RT`, `T.INV.2T`…), so the reports agree
//! with the formulas a user would write. Layouts follow Microsoft's public description of the
//! tools (row labels and section titles); exact spacing is GridCraft's reading of it.

use std::collections::{BTreeMap, HashMap};

use gridcraft_core::{CellError, CellRef, DateSystem, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, NumFmt, Sheet};
use serde_json::{Value as Json, json};

use super::whatif::parse_range;
use super::{CommandSpec, EngineError, Result, Session, bad, cmd, edit, has_doc, str_param};

const CMD: &str = "data.analysisToolPak";
/// Most input cells a tool reads.
const MAX_INPUT_CELLS: u64 = 1_000_000;
/// Most cells a report may write.
const MAX_OUTPUT_CELLS: u64 = 4_000_000;
/// Most variables in a correlation or covariance matrix (work grows with its square).
const MAX_MATRIX_VARS: usize = 256;
/// Excel's limit on regression X columns.
const MAX_REGRESSORS: usize = 16;
/// Most histogram bins.
const MAX_BINS: usize = 100_000;

/// Part of the error a programmatic call gets when the output range already holds data; the
/// dialog recognises it and asks before running again with `overwrite: true`.
pub const OVERWRITE_ERROR: &str = "already holds data";

/// (id, label) of every tool, in the order of Excel's Analysis Tools list.
pub const TOOLS: &[(&str, &str)] = &[
    ("anovaSingle", "Anova: Single Factor"),
    ("correlation", "Correlation"),
    ("covariance", "Covariance"),
    ("descriptive", "Descriptive Statistics"),
    ("exponentialSmoothing", "Exponential Smoothing"),
    ("fTest", "F-Test Two-Sample for Variances"),
    ("histogram", "Histogram"),
    ("movingAverage", "Moving Average"),
    ("rankPercentile", "Rank and Percentile"),
    ("regression", "Regression"),
    ("tTestPaired", "t-Test: Paired Two Sample for Means"),
    ("tTestEqual", "t-Test: Two-Sample Assuming Equal Variances"),
    ("tTestUnequal", "t-Test: Two-Sample Assuming Unequal Variances"),
];

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "data.analysisToolPak",
        "Data Analysis…",
        ["Data", "Analysis"],
        None,
        "{tool: anovaSingle|correlation|covariance|descriptive|exponentialSmoothing|fTest|histogram|movingAverage|rankPercentile|regression|tTestPaired|tTestEqual|tTestUnequal (or the tool's label), output?: \"E1\" (top-left of the report; default a new sheet), newSheet?: name, outputTo?: range|newSheet (picks between the two), overwrite?: bool (else non-empty output cells are refused), labels?: bool (first row/column holds names), groupedBy?: columns|rows, …tool params} — range tools take `range`; descriptive: summary? (default true), confidence?: 95, kthLargest?, kthSmallest?; histogram: bins?: range (default evenly spaced), pareto?, cumulative?; regression: inputY, inputX, constantZero?, confidence?: 95, residuals?, standardizedResiduals?, normalProbability?; movingAverage: interval? (3), standardErrors?; exponentialSmoothing: damping? (0.3), standardErrors?; t-tests and fTest: range1, range2, alpha? (0.05), hypothesizedDifference? (0); anovaSingle: alpha?. Writes static values. No `tool` → {tools} (the menu opens the dialog).",
        has_doc,
        run
    )]
}

// ---------------------------------------------------------------- parameters

fn num(p: &Json, key: &str) -> Option<f64> {
    match p.get(key)? {
        Json::Number(n) => n.as_f64(),
        Json::String(s) => s.trim().trim_end_matches('%').trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn flag(p: &Json, key: &str) -> bool {
    match p.get(key) {
        Some(Json::Bool(b)) => *b,
        Some(Json::String(s)) => s.eq_ignore_ascii_case("true") || s == "1",
        Some(Json::Number(n)) => n.as_f64().is_some_and(|v| v != 0.0),
        _ => false,
    }
}

fn text<'a>(p: &'a Json, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| str_param(p, k)).map(str::trim).filter(|t| !t.is_empty())
}

fn norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

/// A tool id from its id or its label (case and punctuation ignored).
fn tool_id(t: &str) -> Option<&'static str> {
    let n = norm(t);
    TOOLS.iter().find(|(id, label)| norm(id) == n || norm(label) == n).map(|(id, _)| *id)
}

/// Confidence level as a percentage in (0, 100): `95`, `"95%"` or `0.95`.
fn confidence(p: &Json, key: &str) -> Result<Option<f64>> {
    match num(p, key) {
        None => Ok(None),
        Some(c) => {
            let c = if c > 0.0 && c < 1.0 { c * 100.0 } else { c };
            if c > 0.0 && c < 100.0 { Ok(Some(c)) } else { Err(bad(CMD, "the confidence level must be between 0 and 100")) }
        }
    }
}

fn alpha(p: &Json) -> Result<f64> {
    match num(p, "alpha") {
        None => Ok(0.05),
        Some(a) if a > 0.0 && a < 1.0 => Ok(a),
        Some(_) => Err(bad(CMD, "alpha must be between 0 and 1")),
    }
}

// ---------------------------------------------------------------- input

struct Var {
    name: String,
    vals: Vec<Value>,
}

fn non_numeric() -> EngineError {
    EngineError::Other("Input range contains non-numeric data.".into())
}

/// The cells of a range parameter, after checking its size.
fn input_range(s: &Session, p: &Json, keys: &[&str]) -> Result<(usize, RangeRef)> {
    let t = text(p, keys).ok_or_else(|| bad(CMD, format!("missing `{}` (the input range)", keys.first().copied().unwrap_or("range"))))?;
    let (si, r) = parse_range(&s.doc()?.wb, t, CMD)?;
    if r.count() > MAX_INPUT_CELLS {
        return Err(bad(CMD, format!("the input range is too large ({MAX_INPUT_CELLS} cells at most)")));
    }
    Ok((si, r))
}

/// Splits a range into variables (columns, or rows when `by_rows`); with `labels` the first
/// cell of each names it, else `default(i)` does.
fn variables(s: &Session, si: usize, r: RangeRef, by_rows: bool, labels: bool, default: &dyn Fn(usize) -> String) -> Result<Vec<Var>> {
    let d = s.doc()?;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let (outer, inner) = if by_rows { (r.height(), r.width()) } else { (r.width(), r.height()) };
    let mut out = Vec::with_capacity(outer as usize);
    for i in 0..outer {
        let cell = |j: u32| if by_rows { CellRef::new(r.start.row + i, r.start.col + j) } else { CellRef::new(r.start.row + j, r.start.col + i) };
        let mut vals: Vec<Value> = (0..inner).map(|j| sh.value(cell(j))).collect();
        let mut name = default(i as usize);
        if labels && !vals.is_empty() {
            let first = vals.remove(0);
            let t = first.display();
            if !t.trim().is_empty() {
                name = t;
            }
        }
        out.push(Var { name, vals });
    }
    Ok(out)
}

/// The numbers of a variable; blanks are skipped when `skip_blank`, anything else that isn't a
/// number is an error.
fn numbers(v: &[Value], skip_blank: bool) -> Result<Vec<f64>> {
    let mut out = Vec::with_capacity(v.len());
    for x in v {
        match x {
            Value::Number(n) => out.push(*n),
            Value::Empty if skip_blank => {}
            _ => return Err(non_numeric()),
        }
    }
    Ok(out)
}

fn by_rows(p: &Json) -> bool {
    str_param(p, "groupedBy").is_some_and(|g| g.to_ascii_lowercase().starts_with("row"))
}

/// One variable from a single row or column (`range1`/`range2` of the two-sample tests).
fn single(s: &Session, p: &Json, key: &str, fallback: &str) -> Result<Var> {
    let (si, r) = input_range(s, p, &[key])?;
    if r.width() > 1 && r.height() > 1 {
        return Err(EngineError::Other("Input range must be a single row or column.".into()));
    }
    let rows = r.width() > 1;
    let mut v = variables(s, si, r, rows, flag(p, "labels"), &|_| fallback.to_string())?;
    v.pop().ok_or_else(|| bad(CMD, "empty input range"))
}

// ---------------------------------------------------------------- statistics

fn mean(x: &[f64]) -> f64 {
    if x.is_empty() { f64::NAN } else { x.iter().sum::<f64>() / x.len() as f64 }
}

/// Sum of squared deviations from the mean.
fn ss(x: &[f64]) -> f64 {
    let m = mean(x);
    x.iter().map(|v| (v - m) * (v - m)).sum()
}

/// Sample variance (n − 1).
fn var_s(x: &[f64]) -> f64 {
    if x.len() < 2 { f64::NAN } else { ss(x) / (x.len() - 1) as f64 }
}

fn div0() -> Value {
    Value::Error(CellError::Div0)
}

/// `a / b`, `#DIV/0!` when `b` is zero or the inputs aren't finite.
fn ratio(a: f64, b: f64) -> Value {
    if b == 0.0 || !a.is_finite() || !b.is_finite() { div0() } else { Value::number(a / b) }
}

fn n_or_div0(x: f64) -> Value {
    if x.is_finite() { Value::number(x) } else { div0() }
}

/// Host services for calling worksheet functions directly.
struct FnHost;

impl gridcraft_functions::Ctx for FnHost {
    fn date_system(&self) -> DateSystem {
        DateSystem::D1900
    }
    fn now_serial(&self) -> f64 {
        0.0
    }
    fn random(&mut self) -> f64 {
        0.5
    }
}

/// Calls a worksheet function on numeric arguments (and `cumulative` flags as booleans).
fn call(name: &str, args: &[Value]) -> Value {
    let Some(spec) = gridcraft_functions::lookup(name) else { return Value::Error(CellError::Name) };
    if args.iter().any(|a| a.as_f64().is_some_and(|v| !v.is_finite())) {
        return Value::Error(CellError::Num);
    }
    let a: Vec<gridcraft_functions::Arg> = args.iter().cloned().map(gridcraft_functions::Arg::val).collect();
    gridcraft_functions::call(spec, &a, &mut FnHost)
}

fn callf(name: &str, args: &[f64]) -> Value {
    call(name, &args.iter().map(|v| Value::Number(*v)).collect::<Vec<_>>())
}

fn f64_of(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

// ---------------------------------------------------------------- report grid

/// A report: values by (row, col) relative to its top-left corner.
#[derive(Default)]
struct Grid {
    cells: BTreeMap<(u32, u32), Value>,
    /// Cells shown as percentages.
    pct: Vec<(u32, u32)>,
}

impl Grid {
    fn put(&mut self, r: u32, c: u32, v: Value) {
        if !v.is_empty() {
            self.cells.insert((r, c), v);
        }
    }
    fn txt(&mut self, r: u32, c: u32, t: impl AsRef<str>) {
        self.put(r, c, Value::text(t.as_ref()));
    }
    fn num(&mut self, r: u32, c: u32, x: f64) {
        self.put(r, c, Value::number(x));
    }
    /// Writes a row of labels starting at column `c`.
    fn labels(&mut self, r: u32, c: u32, items: &[&str]) {
        for (k, t) in items.iter().enumerate() {
            if !t.is_empty() {
                self.txt(r, c + k as u32, t);
            }
        }
    }
    /// (rows, cols) spanned.
    fn extent(&self) -> (u32, u32) {
        let rows = self.cells.keys().map(|(r, _)| r + 1).max().unwrap_or(0);
        let cols = self.cells.keys().map(|(_, c)| c + 1).max().unwrap_or(0);
        (rows, cols)
    }
}

fn idx(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

// ---------------------------------------------------------------- tools

fn descriptive(s: &Session, p: &Json) -> Result<Grid> {
    let (si, r) = input_range(s, p, &["range", "input"])?;
    let vars = variables(s, si, r, by_rows(p), flag(p, "labels"), &|i| format!("Column{}", i + 1))?;
    let summary = p.get("summary").map(|_| flag(p, "summary")).unwrap_or(true);
    let conf = confidence(p, "confidence")?;
    let kl = num(p, "kthLargest").filter(|k| *k >= 1.0).map(|k| k as usize);
    let ks = num(p, "kthSmallest").filter(|k| *k >= 1.0).map(|k| k as usize);
    if !summary && conf.is_none() && kl.is_none() && ks.is_none() {
        return Err(bad(CMD, "choose at least one of summary, confidence, kthLargest or kthSmallest"));
    }
    let mut g = Grid::default();
    for (i, v) in vars.iter().enumerate() {
        let x = numbers(&v.vals, true)?;
        let c = idx(i).saturating_mul(2);
        g.txt(0, c, &v.name);
        let mut row = 2u32;
        let mut line = |g: &mut Grid, label: &str, val: Value| {
            g.txt(row, c, label);
            g.put(row, c + 1, val);
            row += 1;
        };
        let n = x.len() as f64;
        let mut sorted = x.clone();
        sorted.sort_by(f64::total_cmp);
        if summary {
            let m = mean(&x);
            let sd = var_s(&x).sqrt();
            line(&mut g, "Mean", n_or_div0(m));
            line(&mut g, "Standard Error", if x.len() < 2 { div0() } else { n_or_div0(sd / n.sqrt()) });
            line(&mut g, "Median", median(&sorted));
            line(&mut g, "Mode", mode(&x));
            line(&mut g, "Standard Deviation", n_or_div0(sd));
            line(&mut g, "Sample Variance", n_or_div0(var_s(&x)));
            line(&mut g, "Kurtosis", kurtosis(&x));
            line(&mut g, "Skewness", skewness(&x));
            let (lo, hi) = (sorted.first().copied(), sorted.last().copied());
            line(&mut g, "Range", lo.zip(hi).map(|(a, b)| Value::number(b - a)).unwrap_or(Value::Number(0.0)));
            line(&mut g, "Minimum", lo.map(Value::number).unwrap_or(Value::Number(0.0)));
            line(&mut g, "Maximum", hi.map(Value::number).unwrap_or(Value::Number(0.0)));
            line(&mut g, "Sum", Value::number(x.iter().sum()));
            line(&mut g, "Count", Value::number(n));
        }
        if let Some(k) = kl {
            let v = if k <= sorted.len() { sorted.get(sorted.len() - k).copied().map(Value::number) } else { None };
            line(&mut g, &format!("Largest({k})"), v.unwrap_or(Value::Error(CellError::Num)));
        }
        if let Some(k) = ks {
            let v = sorted.get(k - 1).copied().map(Value::number);
            line(&mut g, &format!("Smallest({k})"), v.unwrap_or(Value::Error(CellError::Num)));
        }
        if let Some(cl) = conf {
            let sd = var_s(&x).sqrt();
            let val = if x.len() < 2 {
                div0()
            } else {
                let t = f64_of(&callf("T.INV.2T", &[1.0 - cl / 100.0, n - 1.0]));
                n_or_div0(t * sd / n.sqrt())
            };
            line(&mut g, &format!("Confidence Level({cl:.1}%)"), val);
        }
    }
    Ok(g)
}

fn median(sorted: &[f64]) -> Value {
    let n = sorted.len();
    if n == 0 {
        return Value::Error(CellError::Num);
    }
    let v = if n % 2 == 1 { sorted.get(n / 2).copied() } else { sorted.get(n / 2 - 1).zip(sorted.get(n / 2)).map(|(a, b)| (a + b) / 2.0) };
    v.map(Value::number).unwrap_or(Value::Error(CellError::Num))
}

/// The most frequent value (the first one to reach the top count), `#N/A` when no value repeats.
fn mode(x: &[f64]) -> Value {
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for v in x {
        *counts.entry(v.to_bits()).or_default() += 1;
    }
    let top = counts.values().copied().max().unwrap_or(0);
    if top < 2 {
        return Value::Error(CellError::NA);
    }
    x.iter().find(|v| counts.get(&v.to_bits()).copied() == Some(top)).map(|v| Value::number(*v)).unwrap_or(Value::Error(CellError::NA))
}

/// Sample excess kurtosis (Excel's KURT).
fn kurtosis(x: &[f64]) -> Value {
    let n = x.len() as f64;
    let sd = var_s(x).sqrt();
    if x.len() < 4 || sd.is_nan() || sd <= 0.0 {
        return div0();
    }
    let m = mean(x);
    let s4: f64 = x.iter().map(|v| ((v - m) / sd).powi(4)).sum();
    Value::number(n * (n + 1.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0)) * s4 - 3.0 * (n - 1.0) * (n - 1.0) / ((n - 2.0) * (n - 3.0)))
}

/// Sample skewness (Excel's SKEW).
fn skewness(x: &[f64]) -> Value {
    let n = x.len() as f64;
    let sd = var_s(x).sqrt();
    if x.len() < 3 || sd.is_nan() || sd <= 0.0 {
        return div0();
    }
    let m = mean(x);
    let s3: f64 = x.iter().map(|v| ((v - m) / sd).powi(3)).sum();
    Value::number(n / ((n - 1.0) * (n - 2.0)) * s3)
}

fn histogram(s: &Session, p: &Json) -> Result<Grid> {
    let (si, r) = input_range(s, p, &["range", "input"])?;
    let labels = flag(p, "labels");
    let d = s.doc()?;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let mut cells: Vec<Value> = r.iter().map(|c| sh.value(c)).collect();
    if labels && !cells.is_empty() {
        cells.remove(0);
    }
    let data = numbers(&cells, true)?;
    let mut bins: Vec<f64> = match text(p, &["bins", "binRange"]) {
        Some(_) => {
            let (bsi, br) = input_range(s, p, &["bins", "binRange"])?;
            let bsh = d.wb.sheet(bsi).ok_or(EngineError::NoDocument)?;
            let mut b: Vec<Value> = br.iter().map(|c| bsh.value(c)).collect();
            if labels && !b.is_empty() {
                b.remove(0);
            }
            numbers(&b, true)?
        }
        None => {
            // Evenly spaced bins from the minimum to the maximum, √n intervals.
            let lo = data.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if !lo.is_finite() || !hi.is_finite() {
                return Err(EngineError::Other("The input range holds no numbers.".into()));
            }
            let k = ((data.len() as f64).sqrt().floor() as usize).clamp(1, 1000);
            if hi == lo { vec![lo] } else { (0..=k).map(|i| lo + (hi - lo) * i as f64 / k as f64).collect() }
        }
    };
    if bins.is_empty() {
        return Err(EngineError::Other("The bin range holds no numbers.".into()));
    }
    if bins.len() > MAX_BINS {
        return Err(bad(CMD, format!("too many bins ({MAX_BINS} at most)")));
    }
    bins.sort_by(f64::total_cmp);
    let mut freq = vec![0u64; bins.len() + 1];
    for x in &data {
        let i = bins.partition_point(|b| b < x);
        if let Some(f) = freq.get_mut(i) {
            *f += 1;
        }
    }
    let rows: Vec<(Value, u64)> = bins.iter().map(|b| Value::number(*b)).chain(std::iter::once(Value::text("More"))).zip(freq).collect();
    let total = data.len() as f64;
    let cumulative = flag(p, "cumulative");
    let mut g = Grid::default();
    let write = |g: &mut Grid, c0: u32, rows: &[(Value, u64)]| {
        g.labels(0, c0, &["Bin", "Frequency"]);
        if cumulative {
            g.txt(0, c0 + 2, "Cumulative %");
        }
        let mut acc = 0u64;
        for (i, (b, f)) in rows.iter().enumerate() {
            let r = idx(i) + 1;
            g.put(r, c0, b.clone());
            g.num(r, c0 + 1, *f as f64);
            if cumulative {
                acc += f;
                g.put(r, c0 + 2, ratio(acc as f64, total));
                g.pct.push((r, c0 + 2));
            }
        }
    };
    write(&mut g, 0, &rows);
    if flag(p, "pareto") {
        let mut sorted = rows.clone();
        sorted.sort_by_key(|r| std::cmp::Reverse(r.1));
        write(&mut g, if cumulative { 3 } else { 2 }, &sorted);
    }
    Ok(g)
}

/// Variables of a range whose columns (rows) must all be numbers, for the matrix tools.
fn matrix_vars(s: &Session, p: &Json) -> Result<Vec<(String, Vec<f64>)>> {
    let (si, r) = input_range(s, p, &["range", "input"])?;
    let vars = variables(s, si, r, by_rows(p), flag(p, "labels"), &|i| format!("Column {}", i + 1))?;
    if vars.len() > MAX_MATRIX_VARS {
        return Err(bad(CMD, format!("too many variables ({MAX_MATRIX_VARS} at most)")));
    }
    vars.into_iter().map(|v| Ok((v.name, numbers(&v.vals, false)?))).collect()
}

fn matrix(s: &Session, p: &Json, f: impl Fn(&[f64], &[f64]) -> Value, diag: impl Fn(&[f64]) -> Value) -> Result<Grid> {
    let vars = matrix_vars(s, p)?;
    let mut g = Grid::default();
    for (i, (name, x)) in vars.iter().enumerate() {
        let k = idx(i) + 1;
        g.txt(0, k, name);
        g.txt(k, 0, name);
        for (j, (_, y)) in vars.iter().enumerate().take(i + 1) {
            g.put(k, idx(j) + 1, if i == j { diag(x) } else { f(x, y) });
        }
    }
    Ok(g)
}

/// Sum of cross products of deviations.
fn sxy(x: &[f64], y: &[f64]) -> f64 {
    let (mx, my) = (mean(x), mean(y));
    x.iter().zip(y).map(|(a, b)| (a - mx) * (b - my)).sum()
}

fn correlation(x: &[f64], y: &[f64]) -> Value {
    let d = (ss(x) * ss(y)).sqrt();
    if x.is_empty() || d.is_nan() || d <= 0.0 { div0() } else { Value::number(sxy(x, y) / d) }
}

fn covariance_p(x: &[f64], y: &[f64]) -> Value {
    if x.is_empty() { div0() } else { Value::number(sxy(x, y) / x.len() as f64) }
}

// ---------------------------------------------------------------- regression

/// Inverts a small symmetric matrix by Gauss–Jordan elimination with partial pivoting; `None`
/// when it is singular (collinear inputs).
fn invert(mut a: Vec<Vec<f64>>) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let scale = (0..n).filter_map(|i| a.get(i).and_then(|r| r.get(i))).fold(0.0f64, |m, v| m.max(v.abs()));
    let mut inv: Vec<Vec<f64>> = (0..n).map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()).collect();
    for col in 0..n {
        let piv = (col..n).max_by(|&x, &y| {
            let ax = a.get(x).and_then(|r| r.get(col)).map_or(0.0, |v| v.abs());
            let ay = a.get(y).and_then(|r| r.get(col)).map_or(0.0, |v| v.abs());
            ax.total_cmp(&ay)
        })?;
        let pv = *a.get(piv)?.get(col)?;
        if pv.is_nan() || pv.abs() <= 1e-12 * scale.max(f64::MIN_POSITIVE) {
            return None;
        }
        a.swap(col, piv);
        inv.swap(col, piv);
        for j in 0..n {
            *a.get_mut(col)?.get_mut(j)? /= pv;
            *inv.get_mut(col)?.get_mut(j)? /= pv;
        }
        let prow = a.get(col)?.clone();
        let pinv = inv.get(col)?.clone();
        for i in 0..n {
            if i == col {
                continue;
            }
            let f = *a.get(i)?.get(col)?;
            if f == 0.0 {
                continue;
            }
            for j in 0..n {
                *a.get_mut(i)?.get_mut(j)? -= f * prow.get(j)?;
                *inv.get_mut(i)?.get_mut(j)? -= f * pinv.get(j)?;
            }
        }
    }
    Some(inv)
}

struct Fit {
    /// Intercept first, then one coefficient per X column.
    coef: Vec<f64>,
    /// Standard errors in the same order (NaN for a forced-zero intercept).
    se: Vec<f64>,
    predicted: Vec<f64>,
    sse: f64,
    ssr: f64,
    dfr: f64,
    dfe: f64,
    /// Total degrees of freedom (n − 1, or n through the origin).
    dft: f64,
}

/// Ordinary least squares of `y` on the columns `xs`, with an intercept unless `zero`.
fn ols(y: &[f64], xs: &[Vec<f64>], zero: bool) -> Option<Fit> {
    let n = y.len();
    let k = xs.len();
    let nf = n as f64;
    let (ym, xm): (f64, Vec<f64>) = if zero { (0.0, vec![0.0; k]) } else { (mean(y), xs.iter().map(|x| mean(x)).collect()) };
    let dev = |j: usize, i: usize| xs.get(j).and_then(|x| x.get(i)).copied().unwrap_or(0.0) - xm.get(j).copied().unwrap_or(0.0);
    let mut sxx = vec![vec![0.0; k]; k];
    let mut sxy = vec![0.0; k];
    for i in 0..n {
        let yi = y.get(i).copied().unwrap_or(0.0) - ym;
        for a in 0..k {
            let da = dev(a, i);
            if let Some(v) = sxy.get_mut(a) {
                *v += da * yi;
            }
            for b in 0..k {
                if let Some(v) = sxx.get_mut(a).and_then(|r| r.get_mut(b)) {
                    *v += da * dev(b, i);
                }
            }
        }
    }
    let inv = invert(sxx)?;
    let b: Vec<f64> = (0..k)
        .map(|a| (0..k).map(|c| inv.get(a).and_then(|r| r.get(c)).copied().unwrap_or(0.0) * sxy.get(c).copied().unwrap_or(0.0)).sum())
        .collect();
    let b0 = if zero { 0.0 } else { ym - b.iter().zip(&xm).map(|(bj, mj)| bj * mj).sum::<f64>() };
    let predicted: Vec<f64> = (0..n)
        .map(|i| b0 + (0..k).map(|j| b.get(j).copied().unwrap_or(0.0) * xs.get(j).and_then(|x| x.get(i)).copied().unwrap_or(0.0)).sum::<f64>())
        .collect();
    let sse: f64 = y.iter().zip(&predicted).map(|(a, p)| (a - p) * (a - p)).sum();
    let ssr: f64 = predicted.iter().map(|p| (p - ym) * (p - ym)).sum();
    let dfr = k as f64;
    let dfe = if zero { nf - dfr } else { nf - dfr - 1.0 };
    let dft = if zero { nf } else { nf - 1.0 };
    let s2 = sse / dfe;
    let mut se = Vec::with_capacity(k + 1);
    if zero {
        se.push(f64::NAN);
    } else {
        let q: f64 = (0..k)
            .map(|a| {
                (0..k)
                    .map(|c| {
                        xm.get(a).copied().unwrap_or(0.0)
                            * inv.get(a).and_then(|r| r.get(c)).copied().unwrap_or(0.0)
                            * xm.get(c).copied().unwrap_or(0.0)
                    })
                    .sum::<f64>()
            })
            .sum();
        se.push((s2 * (1.0 / nf + q)).sqrt());
    }
    for a in 0..k {
        se.push((s2 * inv.get(a).and_then(|r| r.get(a)).copied().unwrap_or(f64::NAN)).sqrt());
    }
    let mut coef = vec![b0];
    coef.extend(b);
    Some(Fit { coef, se, predicted, sse, ssr, dfr, dfe, dft })
}

fn regression(s: &Session, p: &Json) -> Result<Grid> {
    let labels = flag(p, "labels");
    let (ysi, yr) = input_range(s, p, &["inputY", "y"])?;
    let (xsi, xr) = input_range(s, p, &["inputX", "x"])?;
    if yr.width() != 1 {
        return Err(EngineError::Other("Input Y range must be a single column.".into()));
    }
    if xr.height() != yr.height() {
        return Err(EngineError::Other("Input Y range and input X range must have the same number of rows.".into()));
    }
    if xr.width() as usize > MAX_REGRESSORS {
        return Err(EngineError::Other(format!("Input X range must have {MAX_REGRESSORS} columns or fewer.")));
    }
    let yv = variables(s, ysi, yr, false, labels, &|_| "Y".into())?;
    let xv = variables(s, xsi, xr, false, labels, &|i| format!("X Variable {}", i + 1))?;
    let yvar = yv.first().ok_or_else(|| bad(CMD, "empty input Y range"))?;
    let y = numbers(&yvar.vals, false)?;
    let xs: Vec<Vec<f64>> = xv.iter().map(|v| numbers(&v.vals, false)).collect::<Result<_>>()?;
    let zero = flag(p, "constantZero");
    let k = xs.len();
    let n = y.len();
    let params = if zero { k } else { k + 1 };
    if n <= params {
        return Err(EngineError::Other("Too few observations for this many X variables.".into()));
    }
    let fit = ols(&y, &xs, zero).ok_or_else(|| EngineError::Other("The X variables are collinear; the regression can't be computed.".into()))?;
    let conf = confidence(p, "confidence")?.unwrap_or(95.0);
    let sst = fit.ssr + fit.sse;
    let r2 = if sst > 0.0 { fit.ssr / sst } else { f64::NAN };
    let mut g = Grid::default();
    g.txt(0, 0, "SUMMARY OUTPUT");
    g.txt(2, 0, "Regression Statistics");
    let stats: [(&str, Value); 5] = [
        ("Multiple R", n_or_div0(r2.sqrt())),
        ("R Square", n_or_div0(r2)),
        ("Adjusted R Square", n_or_div0(1.0 - (fit.sse / fit.dfe) / (sst / fit.dft))),
        ("Standard Error", n_or_div0((fit.sse / fit.dfe).sqrt())),
        ("Observations", Value::number(n as f64)),
    ];
    for (i, (label, v)) in stats.into_iter().enumerate() {
        g.txt(3 + idx(i), 0, label);
        g.put(3 + idx(i), 1, v);
    }
    g.txt(9, 0, "ANOVA");
    g.labels(10, 1, &["df", "SS", "MS", "F", "Significance F"]);
    let (msr, mse) = (fit.ssr / fit.dfr, fit.sse / fit.dfe);
    let f = msr / mse;
    g.txt(11, 0, "Regression");
    g.num(11, 1, fit.dfr);
    g.num(11, 2, fit.ssr);
    g.num(11, 3, msr);
    g.put(11, 4, n_or_div0(f));
    g.put(11, 5, if f.is_finite() { callf("F.DIST.RT", &[f, fit.dfr, fit.dfe]) } else { div0() });
    g.txt(12, 0, "Residual");
    g.num(12, 1, fit.dfe);
    g.num(12, 2, fit.sse);
    g.num(12, 3, mse);
    g.txt(13, 0, "Total");
    g.num(13, 1, fit.dft);
    g.num(13, 2, sst);
    let lo2 = format!("Lower {conf:.1}%");
    let hi2 = format!("Upper {conf:.1}%");
    g.labels(15, 1, &["Coefficients", "Standard Error", "t Stat", "P-value", "Lower 95%", "Upper 95%", &lo2, &hi2]);
    let t95 = f64_of(&callf("T.INV.2T", &[0.05, fit.dfe]));
    let tc = f64_of(&callf("T.INV.2T", &[1.0 - conf / 100.0, fit.dfe]));
    for (j, (b, se)) in fit.coef.iter().zip(&fit.se).enumerate() {
        let row = 16 + idx(j);
        g.txt(row, 0, if j == 0 { "Intercept" } else { xv.get(j - 1).map_or("", |v| v.name.as_str()) });
        g.num(row, 1, *b);
        if j == 0 && zero {
            for c in 2..=8 {
                g.put(row, c, Value::Error(CellError::NA));
            }
            continue;
        }
        let t = b / se;
        g.put(row, 2, n_or_div0(*se));
        g.put(row, 3, n_or_div0(t));
        g.put(row, 4, if t.is_finite() { callf("T.DIST.2T", &[t.abs(), fit.dfe]) } else { div0() });
        g.num(row, 5, b - t95 * se);
        g.num(row, 6, b + t95 * se);
        g.num(row, 7, b - tc * se);
        g.num(row, 8, b + tc * se);
    }
    let mut row = 16 + idx(fit.coef.len()) + 3;
    let residuals = flag(p, "residuals");
    let standardized = flag(p, "standardizedResiduals");
    if residuals || standardized {
        g.txt(row, 0, "RESIDUAL OUTPUT");
        let pred = format!("Predicted {}", yvar.name);
        let mut head = vec!["Observation", pred.as_str(), "Residuals"];
        if standardized {
            head.push("Standard Residuals");
        }
        g.labels(row + 2, 0, &head);
        // Residuals scaled by their standard deviation (n − 1).
        let sd = (fit.sse / (n as f64 - 1.0)).sqrt();
        for (i, (yi, pi)) in y.iter().zip(&fit.predicted).enumerate() {
            let r = row + 3 + idx(i);
            g.num(r, 0, (i + 1) as f64);
            g.num(r, 1, *pi);
            g.num(r, 2, yi - pi);
            if standardized {
                g.put(r, 3, ratio(yi - pi, sd));
            }
        }
    }
    if flag(p, "normalProbability") {
        let c = if residuals || standardized { if standardized { 5 } else { 4 } } else { 0 };
        if !(residuals || standardized) {
            row = 16 + idx(fit.coef.len()) + 3;
        }
        g.txt(row, c, "PROBABILITY OUTPUT");
        g.labels(row + 2, c, &["Percentile", &yvar.name]);
        let mut sorted = y.clone();
        sorted.sort_by(f64::total_cmp);
        for (i, v) in sorted.iter().enumerate() {
            let r = row + 3 + idx(i);
            g.num(r, c, 100.0 * (i as f64 + 0.5) / n as f64);
            g.num(r, c + 1, *v);
        }
    }
    Ok(g)
}

// ---------------------------------------------------------------- smoothing

fn series(s: &Session, p: &Json) -> Result<Vec<f64>> {
    let (si, r) = input_range(s, p, &["range", "input"])?;
    if r.width() > 1 && r.height() > 1 {
        return Err(EngineError::Other("Input range must be a single row or column.".into()));
    }
    let v = variables(s, si, r, r.width() > 1, flag(p, "labels"), &|_| String::new())?;
    numbers(&v.first().map(|v| v.vals.clone()).unwrap_or_default(), false)
}

/// Writes the forecasts (and with `standardErrors` the standard errors over `se_window`
/// points) down two columns.
fn forecast_grid(x: &[f64], f: &[Option<f64>], se_window: usize, p: &Json) -> Grid {
    let mut g = Grid::default();
    let na = || Value::Error(CellError::NA);
    for (i, fi) in f.iter().enumerate() {
        g.put(idx(i), 0, fi.map(Value::number).unwrap_or_else(na));
    }
    if flag(p, "standardErrors") {
        // A running sum of squared residuals over the last `se_window` points: O(n), not
        // O(n·window). A point without a forecast restarts it, so every window touching it is #N/A.
        let sq = |j: usize| Some((x.get(j)? - (*f.get(j)?)?).powi(2));
        let (mut acc, mut run) = (0.0, 0usize);
        for i in 0..f.len() {
            match sq(i) {
                Some(t) => {
                    acc += t;
                    run += 1;
                }
                None => (acc, run) = (0.0, 0),
            }
            if run > se_window {
                acc -= i.checked_sub(se_window).and_then(sq).unwrap_or(0.0);
                run = se_window;
            }
            // Rounding in the subtraction can leave a tiny negative sum.
            let v = (run == se_window).then(|| (acc.max(0.0) / se_window as f64).sqrt());
            g.put(idx(i), 1, v.map(Value::number).unwrap_or_else(na));
        }
    }
    g
}

fn moving_average(s: &Session, p: &Json) -> Result<Grid> {
    let x = series(s, p)?;
    let interval = num(p, "interval").unwrap_or(3.0);
    if interval < 1.0 || interval.fract() != 0.0 || interval > x.len() as f64 {
        return Err(EngineError::Other("The interval must be a whole number from 1 to the number of data points.".into()));
    }
    let k = interval as usize;
    let mut f = Vec::with_capacity(x.len());
    let mut acc = 0.0;
    for (i, v) in x.iter().enumerate() {
        acc += v;
        if let Some(old) = i.checked_sub(k).and_then(|j| x.get(j)) {
            acc -= old;
        }
        f.push(if i + 1 >= k { Some(acc / k as f64) } else { None });
    }
    Ok(forecast_grid(&x, &f, k, p))
}

fn exponential_smoothing(s: &Session, p: &Json) -> Result<Grid> {
    let x = series(s, p)?;
    let damp = num(p, "damping").unwrap_or(0.3);
    if !(0.0..=1.0).contains(&damp) {
        return Err(EngineError::Other("The damping factor must be between 0 and 1.".into()));
    }
    // F(1) = A(0); F(t) = (1 − damping)·A(t−1) + damping·F(t−1).
    let mut f: Vec<Option<f64>> = Vec::with_capacity(x.len());
    for i in 0..x.len() {
        let v = match i {
            0 => None,
            1 => x.first().copied(),
            _ => x.get(i - 1).zip(f.get(i - 1).copied().flatten()).map(|(a, prev)| (1.0 - damp) * a + damp * prev),
        };
        f.push(v);
    }
    Ok(forecast_grid(&x, &f, 3, p))
}

// ---------------------------------------------------------------- rank and percentile

fn rank_percentile(s: &Session, p: &Json) -> Result<Grid> {
    let (si, r) = input_range(s, p, &["range", "input"])?;
    let vars = variables(s, si, r, by_rows(p), flag(p, "labels"), &|i| format!("Column{}", i + 1))?;
    let mut g = Grid::default();
    for (vi, v) in vars.iter().enumerate() {
        let c = idx(vi).saturating_mul(4);
        g.labels(0, c, &["Point", &v.name, "Rank", "Percent"]);
        let mut pts: Vec<(usize, f64)> = Vec::new();
        for (i, val) in v.vals.iter().enumerate() {
            match val {
                Value::Number(n) => pts.push((i + 1, *n)),
                Value::Empty => {}
                _ => return Err(non_numeric()),
            }
        }
        let mut asc: Vec<f64> = pts.iter().map(|p| p.1).collect();
        asc.sort_by(f64::total_cmp);
        let n = asc.len();
        pts.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        for (k, (point, x)) in pts.iter().enumerate() {
            let row = idx(k) + 1;
            let below = asc.partition_point(|v| v < x);
            let above = n - asc.partition_point(|v| v <= x);
            g.num(row, c, *point as f64);
            g.num(row, c + 1, *x);
            g.num(row, c + 2, (above + 1) as f64);
            // PERCENTRANK.INC: share of values below, truncated to three digits.
            let pr = if n <= 1 { 1.0 } else { (below as f64 / (n - 1) as f64 * 1000.0 + 1e-9).floor() / 1000.0 };
            g.num(row, c + 3, pr);
            g.pct.push((row, c + 3));
        }
    }
    Ok(g)
}

// ---------------------------------------------------------------- tests of means and variances

fn two_samples(s: &Session, p: &Json, skip_blank: bool) -> Result<(Var, Var, Vec<f64>, Vec<f64>)> {
    let a = single(s, p, "range1", "Variable 1")?;
    let b = single(s, p, "range2", "Variable 2")?;
    let x = numbers(&a.vals, skip_blank)?;
    let y = numbers(&b.vals, skip_blank)?;
    if x.len() < 2 || y.len() < 2 {
        return Err(EngineError::Other("Each input range needs at least two numbers.".into()));
    }
    Ok((a, b, x, y))
}

/// Title, blank row and the "Variable 1 / Variable 2" header; returns the first data row.
fn test_head(g: &mut Grid, title: &str, a: &str, b: &str) -> u32 {
    g.txt(0, 0, title);
    g.txt(2, 1, a);
    g.txt(2, 2, b);
    3
}

/// The t statistic rows shared by the three t-tests.
fn t_rows(g: &mut Grid, mut row: u32, t: f64, df: f64, alpha: f64) {
    let ta = t.abs();
    let lines: [(&str, Value); 5] = [
        ("t Stat", n_or_div0(t)),
        ("P(T<=t) one-tail", if t.is_finite() { callf("T.DIST.RT", &[ta, df]) } else { div0() }),
        ("t Critical one-tail", callf("T.INV", &[1.0 - alpha, df])),
        ("P(T<=t) two-tail", if t.is_finite() { callf("T.DIST.2T", &[ta, df]) } else { div0() }),
        ("t Critical two-tail", callf("T.INV.2T", &[alpha, df])),
    ];
    for (label, v) in lines {
        g.txt(row, 0, label);
        g.put(row, 1, v);
        row += 1;
    }
}

fn pair_row(g: &mut Grid, row: u32, label: &str, a: Value, b: Value) {
    g.txt(row, 0, label);
    g.put(row, 1, a);
    g.put(row, 2, b);
}

fn t_test(s: &Session, p: &Json, kind: &str) -> Result<Grid> {
    let alpha = alpha(p)?;
    let hmd = num(p, "hypothesizedDifference").or_else(|| num(p, "difference")).unwrap_or(0.0);
    let paired = kind == "tTestPaired";
    let (a, b, x, y) = two_samples(s, p, !paired)?;
    let (n1, n2) = (x.len() as f64, y.len() as f64);
    let (m1, m2, v1, v2) = (mean(&x), mean(&y), var_s(&x), var_s(&y));
    let mut g = Grid::default();
    let title = match kind {
        "tTestPaired" => "t-Test: Paired Two Sample for Means",
        "tTestEqual" => "t-Test: Two-Sample Assuming Equal Variances",
        _ => "t-Test: Two-Sample Assuming Unequal Variances",
    };
    let mut row = test_head(&mut g, title, &a.name, &b.name);
    pair_row(&mut g, row, "Mean", Value::number(m1), Value::number(m2));
    pair_row(&mut g, row + 1, "Variance", Value::number(v1), Value::number(v2));
    pair_row(&mut g, row + 2, "Observations", Value::number(n1), Value::number(n2));
    row += 3;
    let (t, df) = match kind {
        "tTestPaired" => {
            if x.len() != y.len() {
                return Err(EngineError::Other("Input ranges must have the same number of values.".into()));
            }
            pair_row(&mut g, row, "Pearson Correlation", correlation(&x, &y), Value::Empty);
            row += 1;
            let dif: Vec<f64> = x.iter().zip(&y).map(|(a, b)| a - b).collect();
            let sd = var_s(&dif).sqrt();
            ((mean(&dif) - hmd) / (sd / n1.sqrt()), n1 - 1.0)
        }
        "tTestEqual" => {
            let pooled = ((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / (n1 + n2 - 2.0);
            pair_row(&mut g, row, "Pooled Variance", Value::number(pooled), Value::Empty);
            row += 1;
            ((m1 - m2 - hmd) / (pooled * (1.0 / n1 + 1.0 / n2)).sqrt(), n1 + n2 - 2.0)
        }
        _ => {
            let (q1, q2) = (v1 / n1, v2 / n2);
            // Welch–Satterthwaite degrees of freedom, rounded to a whole number.
            let df = ((q1 + q2).powi(2) / (q1 * q1 / (n1 - 1.0) + q2 * q2 / (n2 - 1.0))).round();
            ((m1 - m2 - hmd) / (q1 + q2).sqrt(), df)
        }
    };
    pair_row(&mut g, row, "Hypothesized Mean Difference", Value::number(hmd), Value::Empty);
    pair_row(&mut g, row + 1, "df", n_or_div0(df), Value::Empty);
    t_rows(&mut g, row + 2, t, df, alpha);
    Ok(g)
}

fn f_test(s: &Session, p: &Json) -> Result<Grid> {
    let alpha = alpha(p)?;
    let (a, b, x, y) = two_samples(s, p, true)?;
    let (v1, v2) = (var_s(&x), var_s(&y));
    let (d1, d2) = (x.len() as f64 - 1.0, y.len() as f64 - 1.0);
    let mut g = Grid::default();
    let row = test_head(&mut g, "F-Test Two-Sample for Variances", &a.name, &b.name);
    pair_row(&mut g, row, "Mean", Value::number(mean(&x)), Value::number(mean(&y)));
    pair_row(&mut g, row + 1, "Variance", Value::number(v1), Value::number(v2));
    pair_row(&mut g, row + 2, "Observations", Value::number(d1 + 1.0), Value::number(d2 + 1.0));
    pair_row(&mut g, row + 3, "df", Value::number(d1), Value::number(d2));
    let f = v1 / v2;
    pair_row(&mut g, row + 4, "F", n_or_div0(f), Value::Empty);
    // The tail on the side of F: left when F < 1, right otherwise.
    let (pv, crit) = if !f.is_finite() {
        (div0(), div0())
    } else if f < 1.0 {
        (call("F.DIST", &[Value::Number(f), Value::Number(d1), Value::Number(d2), Value::Bool(true)]), callf("F.INV", &[alpha, d1, d2]))
    } else {
        (callf("F.DIST.RT", &[f, d1, d2]), callf("F.INV.RT", &[alpha, d1, d2]))
    };
    pair_row(&mut g, row + 5, "P(F<=f) one-tail", pv, Value::Empty);
    pair_row(&mut g, row + 6, "F Critical one-tail", crit, Value::Empty);
    Ok(g)
}

fn anova_single(s: &Session, p: &Json) -> Result<Grid> {
    let alpha = alpha(p)?;
    let (si, r) = input_range(s, p, &["range", "input"])?;
    let vars = variables(s, si, r, by_rows(p), flag(p, "labels"), &|i| format!("Column {}", i + 1))?;
    let groups: Vec<(String, Vec<f64>)> = vars.into_iter().map(|v| Ok((v.name, numbers(&v.vals, true)?))).collect::<Result<_>>()?;
    if groups.len() < 2 || groups.iter().any(|(_, x)| x.is_empty()) {
        return Err(EngineError::Other("Anova needs at least two groups, each with at least one number.".into()));
    }
    let mut g = Grid::default();
    g.txt(0, 0, "Anova: Single Factor");
    g.txt(2, 0, "SUMMARY");
    g.labels(3, 0, &["Groups", "Count", "Sum", "Average", "Variance"]);
    let all: Vec<f64> = groups.iter().flat_map(|(_, x)| x.iter().copied()).collect();
    let grand = mean(&all);
    let (mut ssb, mut ssw) = (0.0, 0.0);
    for (i, (name, x)) in groups.iter().enumerate() {
        let row = 4 + idx(i);
        let m = mean(x);
        g.txt(row, 0, name);
        g.num(row, 1, x.len() as f64);
        g.num(row, 2, x.iter().sum());
        g.num(row, 3, m);
        g.put(row, 4, n_or_div0(var_s(x)));
        ssb += x.len() as f64 * (m - grand) * (m - grand);
        ssw += ss(x);
    }
    let k = groups.len() as f64;
    let n = all.len() as f64;
    let (dfb, dfw) = (k - 1.0, n - k);
    let row = 4 + idx(groups.len()) + 2;
    g.txt(row, 0, "ANOVA");
    g.labels(row + 1, 0, &["Source of Variation", "SS", "df", "MS", "F", "P-value", "F crit"]);
    let (msb, msw) = (ssb / dfb, ratio(ssw, dfw));
    let f = msw.as_f64().map_or(f64::NAN, |w| msb / w);
    g.txt(row + 2, 0, "Between Groups");
    g.num(row + 2, 1, ssb);
    g.num(row + 2, 2, dfb);
    g.num(row + 2, 3, msb);
    g.put(row + 2, 4, n_or_div0(f));
    g.put(row + 2, 5, if f.is_finite() && dfw > 0.0 { callf("F.DIST.RT", &[f, dfb, dfw]) } else { div0() });
    g.put(row + 2, 6, if dfw > 0.0 { callf("F.INV.RT", &[alpha, dfb, dfw]) } else { div0() });
    g.txt(row + 3, 0, "Within Groups");
    g.num(row + 3, 1, ssw);
    g.num(row + 3, 2, dfw);
    g.put(row + 3, 3, msw);
    g.txt(row + 5, 0, "Total");
    g.num(row + 5, 1, ssb + ssw);
    g.num(row + 5, 2, n - 1.0);
    Ok(g)
}

// ---------------------------------------------------------------- output

/// Writes the report at `output` (refusing to overwrite data unless `overwrite`) or on a new
/// sheet.
fn write_report(s: &mut Session, p: &Json, tool: &str, g: Grid) -> Result<Json> {
    let (rows, cols) = g.extent();
    let (rows, cols) = (rows.max(1), cols.max(1));
    if u64::from(rows) * u64::from(cols) > MAX_OUTPUT_CELLS {
        return Err(bad(CMD, "the report is too large"));
    }
    let d = s.doc()?;
    let target = match str_param(p, "outputTo").map(str::to_ascii_lowercase).as_deref() {
        Some("newsheet") => None,
        Some("range") => Some(text(p, &["output", "outputRange"]).ok_or_else(|| bad(CMD, "missing `output` (the output range)"))?),
        _ => text(p, &["output", "outputRange"]),
    };
    let (si, at, new_sheet) = match target {
        Some(t) => {
            let (si, r) = parse_range(&d.wb, t, CMD)?;
            (si, r.start, None)
        }
        None => {
            if d.wb.protected_structure {
                return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
            }
            let name = match p.get("newSheet") {
                Some(Json::String(n)) if !n.trim().is_empty() => n.trim().to_string(),
                _ => d.wb.next_sheet_name(),
            };
            d.wb.check_sheet_name(&name, None).map_err(EngineError::Other)?;
            (d.wb.sheets.len(), CellRef::new(0, 0), Some(name))
        }
    };
    let end_row = at.row.checked_add(rows - 1).filter(|r| *r < MAX_ROWS);
    let end_col = at.col.checked_add(cols - 1).filter(|c| *c < MAX_COLS);
    let (Some(end_row), Some(end_col)) = (end_row, end_col) else {
        return Err(EngineError::Other("The report doesn't fit on the sheet below and right of the output cell.".into()));
    };
    let rect = RangeRef::new(at, CellRef::new(end_row, end_col));
    // Existing content inside the report's rectangle.
    let mut occupied: Vec<CellRef> = Vec::new();
    if new_sheet.is_none()
        && let Some(sh) = d.wb.sheet(si)
    {
        for row in rect.start.row..=rect.end.row {
            for (c, cell) in sh.cells.row(row, rect.start.col, rect.end.col) {
                if !cell.value.is_empty() || cell.formula.is_some() {
                    occupied.push(CellRef::new(row, c));
                }
            }
        }
        if sh.spill.keys().any(|c| rect.contains(*c)) && occupied.is_empty() {
            occupied.push(rect.start);
        }
    }
    if !occupied.is_empty() && !flag(p, "overwrite") {
        return Err(EngineError::Other(format!("The output range {} {OVERWRITE_ERROR}; pass overwrite: true to replace it.", rect.a1())));
    }
    let sheet_name = new_sheet.clone().or_else(|| d.wb.sheet(si).map(|sh| sh.name.clone())).unwrap_or_default();
    let new_index = edit(s, |cx| {
        let pct_fmt = NumFmt::new("0.00%");
        if let Some(name) = &new_sheet {
            cx.wb.sheets.push(std::sync::Arc::new(Sheet::new(name.clone())));
            cx.structural = true;
        }
        let pct_styles: Vec<(CellRef, gridcraft_model::StyleId)> = {
            let sh = cx.wb.sheet(si).ok_or(EngineError::NoDocument)?;
            g.pct.iter().map(|(r, c)| CellRef::new(at.row + r, at.col + c)).map(|c| (c, sh.style_id(c))).collect()
        };
        let pct_styles: HashMap<CellRef, gridcraft_model::StyleId> =
            pct_styles.into_iter().map(|(c, st)| (c, cx.wb.styles.derive(st, |s| s.num_fmt = pct_fmt.clone()))).collect();
        let sh = cx.sheet_mut(si)?;
        for c in &occupied {
            let style = sh.style_id(*c);
            sh.set_cell(*c, Cell { value: Value::Empty, formula: None, style });
        }
        let mut touched = occupied.clone();
        for ((r, c), v) in &g.cells {
            let cell = CellRef::new(at.row + r, at.col + c);
            let style = pct_styles.get(&cell).copied().unwrap_or_else(|| sh.style_id(cell));
            sh.set_cell(cell, Cell { value: v.clone(), formula: None, style });
            touched.push(cell);
        }
        for c in touched {
            cx.touch(si, c);
        }
        Ok(new_sheet.as_ref().map(|_| si))
    })?;
    if let Some(i) = new_index {
        s.doc_mut()?.wb_switch_sheet(i);
    }
    Ok(json!({"tool": tool, "sheet": sheet_name, "range": rect.a1(), "rows": rows, "cols": cols}))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(t) = text(p, &["tool"]) else {
        s.ui_requests.push(crate::UiRequest::Dialog("analysisToolPak".into(), json!({})));
        return Ok(json!({"tools": TOOLS.iter().map(|(id, label)| json!({"id": id, "label": label})).collect::<Vec<_>>()}));
    };
    let tool = tool_id(t).ok_or_else(|| bad(CMD, format!("unknown tool `{t}`")))?;
    let g = match tool {
        "descriptive" => descriptive(s, p)?,
        "histogram" => histogram(s, p)?,
        "correlation" => matrix(s, p, correlation, |_| Value::Number(1.0))?,
        "covariance" => matrix(s, p, covariance_p, |x| covariance_p(x, x))?,
        "regression" => regression(s, p)?,
        "movingAverage" => moving_average(s, p)?,
        "exponentialSmoothing" => exponential_smoothing(s, p)?,
        "rankPercentile" => rank_percentile(s, p)?,
        "tTestPaired" | "tTestEqual" | "tTestUnequal" => t_test(s, p, tool)?,
        "fTest" => f_test(s, p)?,
        "anovaSingle" => anova_single(s, p)?,
        _ => return Err(bad(CMD, format!("unknown tool `{t}`"))),
    };
    write_report(s, p, tool, g)
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellRef, Value};
    use serde_json::json;

    use crate::Session;

    fn s() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }
    fn v(s: &Session, a: &str) -> Value {
        let d = s.doc().unwrap();
        let (sh, body) = crate::cmd::split_sheet(a);
        let si = sh.map(|n| d.wb.sheet_index(&n).unwrap()).unwrap_or(d.wb.active_sheet());
        d.wb.sheet(si).unwrap().value(CellRef::parse(body).unwrap())
    }
    fn n(s: &Session, a: &str) -> f64 {
        v(s, a).as_f64().unwrap_or(f64::NAN)
    }
    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }
    /// Evaluates a formula in a scratch cell.
    fn eval(s: &mut Session, f: &str) -> f64 {
        s.execute("cell.set", json!({"cell": "Z1000", "input": f})).unwrap();
        let x = n(s, "Z1000");
        s.execute("cell.set", json!({"cell": "Z1000", "input": ""})).unwrap();
        x
    }

    #[test]
    fn forecast_standard_errors_match_the_full_window_rescan() {
        // #1043: the running sum must give what re-adding every window gave, gaps included.
        let x: [f64; 11] = [2.0, 4.0, 5.0, 4.0, 5.0, 7.0, 1.0, 3.0, 8.0, 6.0, 2.0];
        let f = [None, Some(3.0), Some(4.5), None, Some(4.0), Some(6.5), Some(2.0), Some(2.5), Some(7.0), Some(5.0), Some(4.0)];
        let rescan = |i: usize, w: usize| -> Value {
            let v = (i + 1).checked_sub(w).and_then(|s0| {
                let mut acc = 0.0;
                for j in s0..=i {
                    acc += (x.get(j)? - (*f.get(j)?)?).powi(2);
                }
                Some((acc / w as f64).sqrt())
            });
            v.map(Value::number).unwrap_or(Value::Error(gridcraft_core::CellError::NA))
        };
        for w in 1..=5 {
            let g = super::forecast_grid(&x, &f, w, &json!({"standardErrors": true}));
            for i in 0..f.len() {
                let (got, want) = (g.cells.get(&(i as u32, 1)).cloned().unwrap_or(Value::Empty), rescan(i, w));
                match (got.as_f64(), want.as_f64()) {
                    (Some(a), Some(b)) => assert!(close(a, b, 1e-12), "window {w}, row {i}: {a} vs {b}"),
                    _ => assert_eq!(got, want, "window {w}, row {i}"),
                }
            }
        }
        // Row 3 has no forecast: windows through it are #N/A, the next full window after it is not.
        let g = super::forecast_grid(&x, &f, 3, &json!({"standardErrors": true}));
        assert_eq!(g.cells.get(&(5, 1)), Some(&Value::Error(gridcraft_core::CellError::NA)));
        assert!(g.cells.get(&(6, 1)).and_then(Value::as_f64).is_some());
    }

    #[test]
    fn descriptive_statistics_of_one_to_five() {
        // 1..5: mean 3, s = √2.5, kurtosis −1.2, skewness 0 (textbook values).
        let mut s = s();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Score"], [1], [2], [3], [4], [5]]})).unwrap();
        let r = s
            .execute("data.analysisToolPak", json!({"tool": "Descriptive Statistics", "range": "A1:A6", "labels": true, "output": "C1", "confidence": 95, "kthLargest": 2, "kthSmallest": 1}))
            .unwrap();
        assert_eq!(r["range"], "C1:D18");
        assert_eq!(v(&s, "C1"), Value::text("Score"));
        assert_eq!(v(&s, "C3"), Value::text("Mean"));
        assert_eq!(n(&s, "D3"), 3.0);
        assert!(close(n(&s, "D4"), (2.5f64 / 5.0).sqrt(), 1e-12));
        assert_eq!(n(&s, "D5"), 3.0);
        assert_eq!(v(&s, "D6"), Value::Error(gridcraft_core::CellError::NA));
        assert!(close(n(&s, "D7"), 2.5f64.sqrt(), 1e-12));
        assert_eq!(n(&s, "D8"), 2.5);
        assert!(close(n(&s, "D9"), -1.2, 1e-12));
        assert!(close(n(&s, "D10"), 0.0, 1e-12));
        assert_eq!((n(&s, "D11"), n(&s, "D12"), n(&s, "D13"), n(&s, "D14"), n(&s, "D15")), (4.0, 1.0, 5.0, 15.0, 5.0));
        assert_eq!(v(&s, "C16"), Value::text("Largest(2)"));
        assert_eq!(n(&s, "D16"), 4.0);
        assert_eq!(n(&s, "D17"), 1.0);
        assert_eq!(v(&s, "C18"), Value::text("Confidence Level(95.0%)"));
        let want = eval(&mut s, "=CONFIDENCE.T(0.05,STDEV.S(A2:A6),5)");
        assert!(close(n(&s, "D18"), want, 1e-9), "{} vs {want}", n(&s, "D18"));
        // The output range now holds data: a second run is refused unless it may overwrite.
        let again = json!({"tool": "descriptive", "range": "A2:A6", "output": "C1"});
        let e = s.execute("data.analysisToolPak", again.clone()).unwrap_err().to_string();
        assert!(e.contains(super::OVERWRITE_ERROR), "{e}");
        let mut again = again;
        again["overwrite"] = json!(true);
        s.execute("data.analysisToolPak", again).unwrap();
        assert_eq!(v(&s, "C1"), Value::text("Column1"));
    }

    #[test]
    fn regression_matches_textbook_and_linest() {
        // x = 1..5, y = 2,4,5,4,5: b = 0.6, a = 2.2, R² = 0.6, F = 4.5 (textbook example).
        let mut s = s();
        s.execute("range.setValues", json!({"range": "A1", "values": [["y", "x"], [2, 1], [4, 2], [5, 3], [4, 4], [5, 5]]})).unwrap();
        s.execute(
            "data.analysisToolPak",
            json!({"tool": "regression", "inputY": "A1:A6", "inputX": "B1:B6", "labels": true, "newSheet": "Fit", "residuals": true, "standardizedResiduals": true}),
        )
        .unwrap();
        assert!(close(n(&s, "Fit!B5"), 0.6, 1e-12), "R²");
        assert!(close(n(&s, "Fit!B6"), 1.0 - 0.4 * 4.0 / 3.0, 1e-12), "adjusted R²");
        assert!(close(n(&s, "Fit!B7"), 0.8f64.sqrt(), 1e-12), "standard error");
        assert!(close(n(&s, "Fit!E12"), 4.5, 1e-12), "F");
        assert!(close(n(&s, "Fit!B17"), 2.2, 1e-12), "intercept");
        assert!(close(n(&s, "Fit!B18"), 0.6, 1e-12), "slope");
        assert_eq!(v(&s, "Fit!A18"), Value::text("x"));
        assert!(close(n(&s, "Fit!C18"), (0.8f64 / 10.0).sqrt(), 1e-12), "SE of slope");
        // Simple regression: Significance F equals the slope's p-value.
        assert!(close(n(&s, "Fit!F12"), n(&s, "Fit!E18"), 1e-9));
        assert!(n(&s, "Fit!E18") > 0.12 && n(&s, "Fit!E18") < 0.13);
        assert_eq!(v(&s, "Fit!A22"), Value::text("RESIDUAL OUTPUT"));
        assert!(close(n(&s, "Fit!B25"), 2.8, 1e-12), "first prediction");
        assert!(close(n(&s, "Fit!C25"), -0.8, 1e-12), "first residual");
        // Multiple regression agrees with LINEST.
        s.execute(
            "range.setValues",
            json!({"range": "Sheet1!D1", "values": [[1, 2, 3], [3, 1, 5], [4, 5, 2], [7, 3, 4], [6, 6, 1], [9, 4, 6], [8, 7, 3]]}),
        )
        .unwrap();
        s.execute("data.analysisToolPak", json!({"tool": "regression", "inputY": "Sheet1!D1:D7", "inputX": "Sheet1!E1:F7", "newSheet": "Multi"}))
            .unwrap();
        let slope2 = eval(&mut s, "=INDEX(LINEST(Sheet1!D1:D7,Sheet1!E1:F7,TRUE,TRUE),1,1)");
        let slope1 = eval(&mut s, "=INDEX(LINEST(Sheet1!D1:D7,Sheet1!E1:F7,TRUE,TRUE),1,2)");
        let se1 = eval(&mut s, "=INDEX(LINEST(Sheet1!D1:D7,Sheet1!E1:F7,TRUE,TRUE),2,2)");
        let icpt = eval(&mut s, "=INDEX(LINEST(Sheet1!D1:D7,Sheet1!E1:F7,TRUE,TRUE),1,3)");
        assert!(close(n(&s, "Multi!B17"), icpt, 1e-9));
        assert!(close(n(&s, "Multi!B18"), slope1, 1e-9));
        assert!(close(n(&s, "Multi!B19"), slope2, 1e-9));
        assert!(close(n(&s, "Multi!C18"), se1, 1e-9));
        assert_eq!(v(&s, "Multi!A19"), Value::text("X Variable 2"));
    }

    #[test]
    fn tests_of_means_and_variances_match_worksheet_functions() {
        let mut s = s();
        s.execute("range.setValues", json!({"range": "A1", "values": [[12, 10], [15, 14], [11, 9], [18, 15], [14, 13], [16, 12]]})).unwrap();
        s.execute("data.analysisToolPak", json!({"tool": "tTestEqual", "range1": "A1:A6", "range2": "B1:B6", "output": "D1"})).unwrap();
        assert_eq!(v(&s, "D1"), Value::text("t-Test: Two-Sample Assuming Equal Variances"));
        assert_eq!(n(&s, "E9"), 10.0, "df");
        let p2 = eval(&mut s, "=T.TEST(A1:A6,B1:B6,2,2)");
        assert!(close(n(&s, "E13"), p2, 1e-9), "two-tail p");
        s.execute("data.analysisToolPak", json!({"tool": "tTestPaired", "range1": "A1:A6", "range2": "B1:B6", "output": "H1"})).unwrap();
        let pp = eval(&mut s, "=T.TEST(A1:A6,B1:B6,2,1)");
        assert_eq!(v(&s, "H13"), Value::text("P(T<=t) two-tail"));
        assert!(close(n(&s, "I13"), pp, 1e-9), "paired two-tail p");
        s.execute("data.analysisToolPak", json!({"tool": "fTest", "range1": "A1:A6", "range2": "B1:B6", "output": "L1"})).unwrap();
        let fp = eval(&mut s, "=F.TEST(A1:A6,B1:B6)");
        assert!(close(n(&s, "M9") * 2.0, fp, 1e-9), "one-tail p is half of F.TEST");
        // Anova on {1,2,3}, {4,5,6}, {7,8,9}: SSB 54, SSW 6, F 27.
        s.execute("range.setValues", json!({"range": "A20", "values": [[1, 4, 7], [2, 5, 8], [3, 6, 9]]})).unwrap();
        s.execute("data.analysisToolPak", json!({"tool": "anovaSingle", "range": "A20:C22", "output": "E20"})).unwrap();
        assert_eq!(v(&s, "E31"), Value::text("Between Groups"));
        assert_eq!((n(&s, "F31"), n(&s, "F32"), n(&s, "I31")), (54.0, 6.0, 27.0));
        let p = eval(&mut s, "=F.DIST.RT(27,2,6)");
        assert!(close(n(&s, "J31"), p, 1e-12));
    }

    #[test]
    fn histogram_matrices_smoothing_and_ranks() {
        let mut s = s();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [2], [3], [5], [7]]})).unwrap();
        s.execute("range.setValues", json!({"range": "B1", "values": [[2], [4], [6]]})).unwrap();
        s.execute(
            "data.analysisToolPak",
            json!({"tool": "histogram", "range": "A1:A6", "bins": "B1:B3", "output": "D1", "cumulative": true, "pareto": true}),
        )
        .unwrap();
        assert_eq!((n(&s, "E2"), n(&s, "E3"), n(&s, "E4"), n(&s, "E5")), (3.0, 1.0, 1.0, 1.0));
        assert_eq!(v(&s, "D5"), Value::text("More"));
        assert_eq!(n(&s, "F5"), 1.0);
        assert_eq!((n(&s, "G2"), n(&s, "H2")), (2.0, 3.0), "Pareto puts the largest bin first");
        // Correlation and covariance against CORREL and COVARIANCE.P.
        s.execute("range.setValues", json!({"range": "J1", "values": [[1, 2], [2, 4], [3, 5], [4, 4], [5, 5]]})).unwrap();
        s.execute("data.analysisToolPak", json!({"tool": "correlation", "range": "J1:K5", "output": "M1"})).unwrap();
        assert_eq!(n(&s, "N2"), 1.0);
        assert!(close(n(&s, "N3"), eval(&mut s, "=CORREL(J1:J5,K1:K5)"), 1e-12));
        assert!(v(&s, "O2").is_empty(), "upper triangle is blank");
        s.execute("data.analysisToolPak", json!({"tool": "covariance", "range": "J1:K5", "output": "M10"})).unwrap();
        assert!(close(n(&s, "N12"), eval(&mut s, "=COVARIANCE.P(J1:J5,K1:K5)"), 1e-12));
        assert!(close(n(&s, "N11"), eval(&mut s, "=VAR.P(J1:J5)"), 1e-12));
        // Moving average of 3 and exponential smoothing with damping 0.3.
        s.execute("data.analysisToolPak", json!({"tool": "movingAverage", "range": "K1:K5", "output": "Q1", "standardErrors": true})).unwrap();
        assert_eq!(v(&s, "Q2"), Value::Error(gridcraft_core::CellError::NA));
        assert!(close(n(&s, "Q3"), 11.0 / 3.0, 1e-12));
        assert!(close(n(&s, "Q5"), 14.0 / 3.0, 1e-12));
        assert_eq!(v(&s, "R4"), Value::Error(gridcraft_core::CellError::NA));
        assert!(n(&s, "R5").is_finite());
        s.execute("data.analysisToolPak", json!({"tool": "exponentialSmoothing", "range": "K1:K5", "output": "S1"})).unwrap();
        assert_eq!(n(&s, "S2"), 2.0);
        assert!(close(n(&s, "S3"), 0.7 * 4.0 + 0.3 * 2.0, 1e-12));
        // Rank and Percentile of 2,4,5,4,5: the two 5s share rank 1 (3 of 4 others below them).
        s.execute("data.analysisToolPak", json!({"tool": "rankPercentile", "range": "K1:K5", "output": "U1"})).unwrap();
        assert_eq!((n(&s, "U2"), n(&s, "V2"), n(&s, "W2"), n(&s, "X2")), (3.0, 5.0, 1.0, 0.75));
        assert_eq!((n(&s, "U3"), n(&s, "W3")), (5.0, 1.0));
        assert_eq!((n(&s, "V6"), n(&s, "X6")), (2.0, 0.0));
        assert_eq!(n(&s, "X4"), 0.25, "4 has one value below it among five");
    }

    #[test]
    fn hostile_inputs_are_refused_without_panicking() {
        let mut s = s();
        let r = s.execute("data.analysisToolPak", json!({})).unwrap();
        assert!(r["tools"].as_array().is_some_and(|t| t.len() == super::TOOLS.len()));
        assert!(s.take_ui_requests().iter().any(|q| matches!(q, crate::UiRequest::Dialog(n, _) if n == "analysisToolPak")));
        s.execute("range.setValues", json!({"range": "A1", "values": [[1, "x"], [2, 3]]})).unwrap();
        for p in [
            json!({"tool": "nope"}),
            json!({"tool": "descriptive"}),
            json!({"tool": "descriptive", "range": "A1:XFD1048576"}),
            json!({"tool": "descriptive", "range": "A1:B2"}),
            json!({"tool": "descriptive", "range": "A1:A2", "output": "XFD1048576"}),
            json!({"tool": "descriptive", "range": "A1:A2", "newSheet": "bad[name]"}),
            json!({"tool": "regression", "inputY": "A1:A2", "inputX": "A1:A2"}),
            json!({"tool": "regression", "inputY": "A1:A2", "inputX": "A1:Q2"}),
            json!({"tool": "movingAverage", "range": "A1:A2", "interval": 1e300}),
            json!({"tool": "movingAverage", "range": "A1:A2", "interval": -3}),
            json!({"tool": "exponentialSmoothing", "range": "A1:A2", "damping": 7}),
            json!({"tool": "histogram", "range": "C1:C3"}),
            json!({"tool": "tTestPaired", "range1": "A1", "range2": "A2"}),
            json!({"tool": "anovaSingle", "range": "A1:A2"}),
            json!({"tool": "correlation", "range": "A1:B2", "alpha": "x", "groupedBy": 5}),
            json!({"tool": "descriptive", "range": "A1:A2", "confidence": 250}),
        ] {
            assert!(s.execute("data.analysisToolPak", p.clone()).is_err(), "{p}");
        }
        // Collinear X columns are an error, not a NaN report.
        s.execute("range.setValues", json!({"range": "D1", "values": [[1, 1, 2], [2, 2, 4], [3, 3, 6], [5, 4, 8]]})).unwrap();
        assert!(s.execute("data.analysisToolPak", json!({"tool": "regression", "inputY": "D1:D4", "inputX": "E1:F4"})).is_err());
        // Constant data gives #DIV/0! cells, not a crash.
        s.execute("range.setValues", json!({"range": "H1", "values": [[4], [4], [4]]})).unwrap();
        s.execute("data.analysisToolPak", json!({"tool": "descriptive", "range": "H1:H3", "output": "J1"})).unwrap();
        assert_eq!(v(&s, "K9"), Value::Error(gridcraft_core::CellError::Div0));
    }
}
