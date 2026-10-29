//! Calculation benchmarks on fixed workbooks built in code (release build). Run through
//! `cargo xtask bench`, which builds this in release and compares against a saved baseline, or
//! directly: `cargo run --release -p gridcraft-engine --example bench -- [--quick] [--rows N]
//! [--reps N] [--filter NAME] [--json PATH]`.
//!
//! Every scenario builds its workbook untimed, runs one warm-up, then times `reps` runs of one
//! operation through `Session::execute` (so a timing includes what an edit costs the user: undo
//! history, graph update and recalculation). Results are checked so a scenario that silently
//! stopped recalculating fails instead of getting fast.

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it

use std::time::{Duration, Instant};

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use serde_json::{Value as Json, json};

struct Cfg {
    rows: usize,
    reps: usize,
    filter: Option<String>,
}

struct Outcome {
    name: &'static str,
    what: String,
    times: Vec<Duration>,
    cells: usize,
}

type R<T> = Result<T, String>;
/// A scenario: its name and the function that runs it.
type Scenario = (&'static str, fn(&Cfg) -> R<Outcome>);

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bench: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> R<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let quick = args.iter().any(|a| a == "--quick");
    let cfg = Cfg {
        rows: count(flag("--rows"), "--rows")?.unwrap_or(if quick { 10_000 } else { 100_000 }).max(200),
        reps: count(flag("--reps"), "--reps")?.unwrap_or(if quick { 3 } else { 7 }).max(1),
        filter: flag("--filter"),
    };
    let scenarios: [Scenario; 10] = [
        ("edit.chain", edit_chain),
        ("edit.fanout", edit_fanout),
        ("edit.sliding", edit_sliding),
        ("edit.sumifs", edit_sumifs),
        ("edit.lookup", edit_lookup),
        ("enter.formulas", enter_formulas),
        ("recalc.full", recalc_full),
        ("data.sort", data_sort),
        ("xlsx.save", xlsx_save),
        ("xlsx.open", xlsx_open),
    ];
    println!("GridCraft calc bench: {} rows, {} reps (median / min)\n", cfg.rows, cfg.reps);
    println!("{:<16} {:>10} {:>10} {:>9}  workload", "scenario", "median", "min", "cells");
    let mut results = Vec::new();
    // A failed scenario is reported and the others still run, so one regression doesn't hide
    // the numbers for everything else.
    let mut failed = Vec::new();
    let selected: Vec<_> = scenarios.into_iter().filter(|(name, _)| cfg.filter.as_deref().is_none_or(|p| name.contains(p))).collect();
    if selected.is_empty() {
        // An empty run would otherwise save as a baseline with nothing in it.
        return Err(format!("--filter {} matches no scenario", cfg.filter.as_deref().unwrap_or("")));
    }
    for (name, f) in selected {
        let o = match f(&cfg) {
            Ok(o) => o,
            Err(e) => {
                println!("{name:<16} {:>10}  {e}", "FAILED");
                failed.push(name);
                continue;
            }
        };
        let (med, min) = stats(&o.times);
        let cells = if o.cells == 0 { "-".to_string() } else { o.cells.to_string() };
        println!("{:<16} {:>10} {:>10} {:>9}  {}", o.name, ms(med), ms(min), cells, o.what);
        results.push(json!({"name": o.name, "what": o.what, "median_ms": med, "min_ms": min, "cells": o.cells}));
    }
    if let Some(path) = flag("--json") {
        let out = json!({"rows": cfg.rows, "reps": cfg.reps, "results": results, "failed": failed});
        let text = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
    }
    if failed.is_empty() { Ok(()) } else { Err(format!("{} scenario(s) failed: {}", failed.len(), failed.join(", "))) }
}

fn stats(times: &[Duration]) -> (f64, f64) {
    let mut v: Vec<f64> = times.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    v.sort_by(f64::total_cmp);
    // An even count averages the two middle samples.
    let mid = v.len() / 2;
    let med = match (v.len() % 2, v.get(mid.wrapping_sub(1)), v.get(mid)) {
        (0, Some(a), Some(b)) => (a + b) / 2.0,
        (_, _, Some(m)) => *m,
        _ => 0.0,
    };
    (med, v.first().copied().unwrap_or(0.0))
}

fn ms(v: f64) -> String {
    if v >= 100.0 { format!("{v:.0} ms") } else { format!("{v:.2} ms") }
}

fn exec(s: &mut Session, cmd: &str, p: Json) -> R<Json> {
    s.execute(cmd, p).map_err(|e| format!("{cmd}: {e}"))
}

/// A count flag's value; one that isn't a number is an error, not the default.
fn count(v: Option<String>, name: &str) -> R<Option<usize>> {
    v.map(|s| s.parse().map_err(|_| format!("{name}: `{s}` is not a count"))).transpose()
}

fn session() -> R<Session> {
    let mut s = Session::new();
    s.new_workbook();
    Ok(s)
}

/// Writes `rows` rows starting at `col` row 1, `make(i)` giving row i (0-based), in chunks.
fn fill(s: &mut Session, col: &str, rows: usize, make: impl Fn(usize) -> Json) -> R<()> {
    let chunk = 10_000;
    let mut r = 0;
    while r < rows {
        let n = chunk.min(rows - r);
        let vals: Vec<Json> = (r..r + n).map(&make).collect();
        exec(s, "range.setValues", json!({"range": format!("{col}{}", r + 1), "values": vals}))?;
        r += n;
    }
    Ok(())
}

fn num(s: &Session, at: &str) -> R<f64> {
    let c = CellRef::parse(at).ok_or_else(|| format!("bad ref {at}"))?;
    let d = s.doc().map_err(|e| e.to_string())?;
    match d.wb.sheet(0).map(|sh| sh.value(c)) {
        Some(Value::Number(n)) => Ok(n),
        other => Err(format!("{at} = {other:?}, expected a number")),
    }
}

fn expect(s: &Session, at: &str, want: f64) -> R<()> {
    let got = num(s, at)?;
    if (got - want).abs() <= 1e-6 * want.abs().max(1.0) { Ok(()) } else { Err(format!("{at} = {got}, expected {want}")) }
}

fn recalc_cells(s: &Session) -> usize {
    s.doc().map(|d| d.calc.last_recalc_cells).unwrap_or(0)
}

/// Runs a warm-up then `reps` timed calls of `op(rep)`, then `check(rep)` (untimed) after each.
fn time(
    cfg: &Cfg,
    s: &mut Session,
    op: impl FnMut(&mut Session, usize) -> R<()>,
    check: impl Fn(&Session, usize) -> R<()>,
) -> R<(Vec<Duration>, usize)> {
    time_prepared(cfg, s, |_| Ok(()), op, check)
}

/// [`time`] with `prep` run (untimed) before each call, to put back the state `op` starts from.
fn time_prepared(
    cfg: &Cfg,
    s: &mut Session,
    mut prep: impl FnMut(&mut Session) -> R<()>,
    mut op: impl FnMut(&mut Session, usize) -> R<()>,
    check: impl Fn(&Session, usize) -> R<()>,
) -> R<(Vec<Duration>, usize)> {
    let mut times = Vec::with_capacity(cfg.reps);
    let mut cells = 0;
    for rep in 0..=cfg.reps {
        prep(s)?;
        let t = Instant::now();
        op(s, rep)?;
        let dt = t.elapsed();
        check(s, rep)?;
        cells = recalc_cells(s);
        if rep > 0 {
            times.push(dt);
        }
    }
    Ok((times, cells))
}

/// Value typed into A1 on rep `r`: alternates so every rep changes something.
fn a1(r: usize) -> f64 {
    (r % 2 + 2) as f64
}

/// A running sum `B[n] = B[n-1] + A[n]`: one edit at the top recalculates a dependency chain
/// `rows` deep.
fn edit_chain(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = session()?;
    fill(&mut s, "A", n, |_| json!([1]))?;
    fill(&mut s, "B", n, |i| json!([if i == 0 { "=A1".to_string() } else { format!("=B{i}+A{}", i + 1) }]))?;
    let (times, cells) = time(
        cfg,
        &mut s,
        |s, r| exec(s, "cell.set", json!({"cell": "A1", "input": a1(r).to_string()})).map(drop),
        |s, r| expect(s, &format!("B{n}"), (n - 1) as f64 + a1(r)),
    )?;
    Ok(Outcome { name: "edit.chain", what: format!("edit A1 under a {n}-deep running sum"), times, cells })
}

/// `rows` formulas that each read `$A$1`: one edit, a wide and shallow dirty set.
fn edit_fanout(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = session()?;
    exec(&mut s, "cell.set", json!({"cell": "A1", "input": "1"}))?;
    fill(&mut s, "B", n, |i| json!([format!("=$A$1*{}", i + 1)]))?;
    let (times, cells) = time(
        cfg,
        &mut s,
        |s, r| exec(s, "cell.set", json!({"cell": "A1", "input": a1(r).to_string()})).map(drop),
        |s, r| expect(s, &format!("B{n}"), a1(r) * n as f64),
    )?;
    Ok(Outcome { name: "edit.fanout", what: format!("edit $A$1 read by {n} formulas"), times, cells })
}

/// `rows` overlapping 100-row window sums; an edit in the middle dirties ~100 of them, so this
/// measures the range index lookup, not evaluation.
fn edit_sliding(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = session()?;
    fill(&mut s, "A", n + 99, |_| json!([1]))?;
    fill(&mut s, "B", n, |i| json!([format!("=SUM(A{}:A{})", i + 1, i + 100)]))?;
    let mid = n / 2;
    let (times, cells) = time(
        cfg,
        &mut s,
        |s, r| exec(s, "cell.set", json!({"cell": format!("A{mid}"), "input": a1(r).to_string()})).map(drop),
        |s, r| expect(s, &format!("B{mid}"), 99.0 + a1(r)),
    )?;
    Ok(Outcome { name: "edit.sliding", what: format!("edit one cell under {n} 100-row SUM windows"), times, cells })
}

/// 100 whole-column SUMIFS over `rows` rows of data, all dirtied by one data edit.
fn edit_sumifs(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = session()?;
    fill(&mut s, "A", n, |i| json!([1, (i % 100) as f64]))?;
    fill(&mut s, "D", 100, |k| json!([format!("=SUMIFS(A:A,B:B,{k})")]))?;
    // Rows whose category is 0 (i % 100 == 0), A1 among them.
    let per = n.div_ceil(100) as f64;
    let (times, cells) = time(
        cfg,
        &mut s,
        |s, r| exec(s, "cell.set", json!({"cell": "A1", "input": a1(r).to_string()})).map(drop),
        |s, r| expect(s, "D1", per - 1.0 + a1(r)),
    )?;
    Ok(Outcome { name: "edit.sumifs", what: format!("edit data under 100 SUMIFS(A:A,B:B,k) over {n} rows"), times, cells })
}

/// 100 exact-match XLOOKUPs over whole columns of `rows` keys, all dirtied by one key edit.
fn edit_lookup(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = session()?;
    fill(&mut s, "A", n, |i| json!([(i + 1) as f64, (2 * (i + 1)) as f64]))?;
    let step = n / 100;
    fill(&mut s, "D", 100, |k| json!([format!("=XLOOKUP({},A:A,B:B,0)", (k + 1) * step)]))?;
    let last = (2 * 100 * step) as f64;
    let (times, cells) = time(
        cfg,
        &mut s,
        // D1 looks up the key in A{step}: removing it (-step) makes D1 fall back to 0, putting it
        // back finds it again. Every lookup reads A:A, so all 100 recalculate either way.
        |s, r| {
            let key = if r % 2 == 0 { format!("-{step}") } else { step.to_string() };
            exec(s, "cell.set", json!({"cell": format!("A{step}"), "input": key})).map(drop)
        },
        |s, r| {
            expect(s, "D1", if r % 2 == 0 { 0.0 } else { (2 * step) as f64 })?;
            expect(s, "D100", last)
        },
    )?;
    Ok(Outcome { name: "edit.lookup", what: format!("edit a key under 100 XLOOKUPs over {n} rows"), times, cells })
}

/// Entering `rows` formulas in one command: parse, graph insertion and first evaluation.
fn enter_formulas(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = session()?;
    fill(&mut s, "A", n, |i| json!([(i % 97) as f64]))?;
    let vals: Vec<Json> = (0..n).map(|i| json!([format!("=A{}*2+1", i + 1)])).collect();
    let (times, cells) = time_prepared(
        cfg,
        &mut s,
        // Each run enters the formulas into empty cells, not over the previous run's.
        |s| exec(s, "edit.clearAll", json!({"range": format!("B1:B{n}")})).map(drop),
        |s, _| exec(s, "range.setValues", json!({"range": "B1", "values": vals})).map(drop),
        |s, _| expect(s, &format!("B{n}"), ((n - 1) % 97) as f64 * 2.0 + 1.0),
    )?;
    Ok(Outcome { name: "enter.formulas", what: format!("enter {n} formulas =An*2+1 in one paste"), times, cells })
}

/// A model with 3 formulas per row (arithmetic and a row SUM), recalculated in full.
fn model(n: usize) -> R<Session> {
    let mut s = session()?;
    fill(&mut s, "A", n, |i| json!([(i % 97) as f64, format!("=A{}*2", i + 1), format!("=B{0}+A{0}", i + 1), format!("=SUM(A{0}:C{0})", i + 1)]))?;
    Ok(s)
}

fn recalc_full(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = model(n)?;
    // Manual calculation, and A[n] changed (untimed) before each run, so D[n] is stale until
    // Calculate Now recalculates: a Calculate Now that did nothing fails the check.
    exec(&mut s, "formulas.calculationOptions", json!({"mode": "manual"}))?;
    let mut rep = 0;
    let (times, cells) = time_prepared(
        cfg,
        &mut s,
        |s| {
            rep += 1;
            exec(s, "cell.set", json!({"cell": format!("A{n}"), "input": (100 + rep).to_string()})).map(drop)
        },
        |s, _| exec(s, "formulas.calculateNow", json!({})).map(drop),
        |s, r| expect(s, &format!("D{n}"), (101 + r) as f64 * 6.0),
    )?;
    Ok(Outcome { name: "recalc.full", what: format!("Calculate Now, {} formulas (3 per row)", 3 * n), times, cells })
}

fn data_sort(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = session()?;
    fill(&mut s, "A", n, |i| json!([format!("Item {i}"), (i % 97) as f64, ((i * 7919) % 1000) as f64]))?;
    let (times, _) = time(
        cfg,
        &mut s,
        |s, r| {
            let cmd = if r % 2 == 0 { "data.sortDescending" } else { "data.sortAscending" };
            exec(s, cmd, json!({"range": format!("A1:C{n}"), "column": "B"})).map(drop)
        },
        |s, r| expect(s, "B1", if r % 2 == 0 { 96.0 } else { 0.0 }),
    )?;
    // A sort with no formulas recalculates nothing; 0 shows as "-".
    Ok(Outcome { name: "data.sort", what: format!("sort {n} rows x 3 columns by a numeric key"), times, cells: 0 })
}

fn save_bytes(s: &mut Session) -> R<String> {
    let r = exec(s, "file.saveBytes", json!({"format": "xlsx"}))?;
    r.get("base64").and_then(Json::as_str).map(str::to_string).ok_or_else(|| "file.saveBytes: no base64".to_string())
}

fn xlsx_save(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = model(n)?;
    let (times, _) = time(cfg, &mut s, |s, _| save_bytes(s).map(drop), |_, _| Ok(()))?;
    Ok(Outcome { name: "xlsx.save", what: format!("save the {n}-row model as XLSX"), times, cells: 0 })
}

fn xlsx_open(cfg: &Cfg) -> R<Outcome> {
    let n = cfg.rows;
    let mut s = model(n)?;
    let b64 = save_bytes(&mut s)?;
    let last = ((n - 1) % 97) as f64 * 6.0;
    let (times, cells) = time(
        cfg,
        &mut s,
        |s, _| exec(s, "file.open", json!({"name": "bench.xlsx", "base64": b64})).map(drop),
        |s, _| expect(s, &format!("D{n}"), last),
    )?;
    Ok(Outcome { name: "xlsx.open", what: format!("open the {n}-row model from XLSX (with full recalc)"), times, cells })
}
