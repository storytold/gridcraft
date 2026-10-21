//! Release timing of one edit that recalculates 100_000 dependents.
//!
//! `cargo xtask bench` runs this. Both medians must stay under 100 ms. The serial chain is
//! recorded and is not a pass condition. Debug `cargo test` does not run this.

#![allow(clippy::disallowed_methods)]

use std::time::Instant;

use gridcraft_calc::Calc;
use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Cell, Formula, Workbook};

const N: u32 = 100_000;
const RUNS: usize = 5;

fn main() {
    if let Err(msg) = run() {
        eprintln!("{msg}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    println!("cpu: {}", cpu_model());
    let cap = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 8);
    println!("worker-cap: {cap}");

    let independent = median_ms(&time_edits(independent_book()?, 2.0, |wb| {
        let v = wb.sheet(0).map(|s| s.value(CellRef::new(N - 1, 1)));
        v == Some(Value::Number(f64::from(2 + N - 1)))
    })?);
    let same_row = median_ms(&time_edits(same_row_book()?, 3.0, |wb| {
        let v = wb.sheet(0).map(|s| s.value(CellRef::new(N - 1, 10)));
        v == Some(Value::Number(9.0))
    })?);
    let chain = chain_ms()?;

    println!("median-independent-ms: {independent:.3}");
    println!("median-same-row-sum-ms: {same_row:.3}");
    println!("serial-chain-ms: {chain:.3}");
    if independent >= 100.0 || same_row >= 100.0 {
        return Err(format!("median not under 100 ms (independent {independent:.3}, same-row sum {same_row:.3})"));
    }
    Ok(())
}

fn cpu_model() -> String {
    if let Some(name) = proc_cpu_model() {
        return name;
    }
    sysctl_brand()
}

/// `model name` from `/proc/cpuinfo`. `None` when the file or the line is missing.
fn proc_cpu_model() -> Option<String> {
    let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("model name") else { continue };
        let Some((_, name)) = rest.split_once(':') else { continue };
        let name = name.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// macOS brand string. Called only when `/proc/cpuinfo` did not yield a name.
fn sysctl_brand() -> String {
    let Ok(out) = std::process::Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output() else {
        return "unknown".into();
    };
    if !out.status.success() {
        return "unknown".into();
    }
    let Ok(text) = std::str::from_utf8(&out.stdout) else {
        return "unknown".into();
    };
    let name = text.trim();
    if name.is_empty() { "unknown".into() } else { name.to_string() }
}

fn median_ms(samples: &[std::time::Duration]) -> f64 {
    let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    ms.sort_by(f64::total_cmp);
    ms.get(samples.len() / 2).copied().unwrap_or(f64::MAX)
}

/// One edit of A1, five times, after a warmup. `check` sees the workbook after the last edit.
fn time_edits(mut wb: Workbook, last: f64, check: impl Fn(&Workbook) -> bool) -> Result<Vec<std::time::Duration>, String> {
    let mut calc = Calc::new();
    calc.recalc_all(&mut wb);
    let a1 = (0, CellRef::new(0, 0));
    // Warmup uses a different value so the timed edits are not a no-op cache hit.
    set_a1(&mut wb, 1.0);
    calc.cells_changed(&mut wb, &[a1]);
    let mut samples = Vec::with_capacity(RUNS);
    for i in 0..RUNS {
        set_a1(&mut wb, last + f64::from(i as u32));
        let t = Instant::now();
        calc.cells_changed(&mut wb, &[a1]);
        samples.push(t.elapsed());
    }
    set_a1(&mut wb, last);
    calc.cells_changed(&mut wb, &[a1]);
    if !check(&wb) {
        return Err("recalc stored the wrong value".into());
    }
    Ok(samples)
}

fn set_a1(wb: &mut Workbook, n: f64) {
    if let Some(sh) = wb.sheet_mut(0) {
        sh.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(n)));
    }
}

/// Column of formulas that read A1 and do not read each other.
fn independent_book() -> Result<Workbook, String> {
    let mut wb = Workbook::new();
    let Some(sh) = wb.sheet_mut(0) else { return Err("no sheet".into()) };
    sh.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(1.0)));
    for r in 0..N {
        sh.cells.set(CellRef::new(r, 1), Cell::formula(Formula::new(&format!("=$A$1+{r}"))));
    }
    Ok(wb)
}

/// 100_000 rows of `=SUM(E{r}:J{r})+$A$1` (issue #66's shape). The formulas do not read each other.
fn same_row_book() -> Result<Workbook, String> {
    let mut wb = Workbook::new();
    let Some(sh) = wb.sheet_mut(0) else { return Err("no sheet".into()) };
    sh.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(1.0)));
    for r in 0..N {
        for c in 4..10 {
            sh.cells.set(CellRef::new(r, c), Cell::value(Value::Number(1.0)));
        }
        let row = r + 1;
        sh.cells.set(CellRef::new(r, 10), Cell::formula(Formula::new(&format!("=SUM(E{row}:J{row})+$A$1"))));
    }
    Ok(wb)
}

/// 100_000-step chain. Recorded only; a slow chain does not fail the bench.
fn chain_ms() -> Result<f64, String> {
    let mut wb = Workbook::new();
    let Some(sh) = wb.sheet_mut(0) else { return Err("no sheet".into()) };
    sh.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(1.0)));
    sh.cells.set(CellRef::new(0, 1), Cell::formula(Formula::new("=A1+1")));
    for r in 1..N {
        let prev = r;
        sh.cells.set(CellRef::new(r, 1), Cell::formula(Formula::new(&format!("=B{prev}+1"))));
    }
    let mut calc = Calc::new();
    calc.recalc_all(&mut wb);
    set_a1(&mut wb, 2.0);
    let t = Instant::now();
    calc.cells_changed(&mut wb, &[(0, CellRef::new(0, 0))]);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let last = wb.sheet(0).and_then(|s| s.value(CellRef::new(N - 1, 1)).as_f64());
    if last != Some(f64::from(2 + N)) {
        return Err(format!("serial chain stored {last:?}"));
    }
    Ok(ms)
}
