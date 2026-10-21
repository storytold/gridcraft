//! Release timing of one `cell.set` that recalculates 100_000 cross-sheet formulas through
//! [`Session::execute`].
//!
//! `cargo xtask bench` runs this. Wrong stored values exit non-zero; a slow median does not.
//! Debug `cargo test` does not run this.

#![allow(clippy::disallowed_methods)]

use std::sync::Arc;
use std::time::Instant;

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_model::{Cell, Formula};
use serde_json::json;

const ROWS: u32 = 100_000;
const RUNS: usize = 5;

const LOOKUP_FORMULA: &str = r#"=IF(VLOOKUP(Data!$A$1,Data!$B$1:$C$2,2,FALSE)="alpha","yes"&Data!$A$1,"no")"#;

fn main() {
    if let Err(msg) = run() {
        eprintln!("{msg}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    println!("cpu: {}", cpu_model());

    let mut s = Session::new();
    s.new_workbook();
    exec(&mut s, "sheet.rename", json!({"sheet": 0, "name": "Data"}))?;
    exec(&mut s, "home.insertSheet", json!({"name": "Calc"}))?;
    fill_workbook(&mut s)?;

    exec(&mut s, "formulas.calculateNow", json!({}))?;

    let inputs = ["2", "1", "2", "1", "2"];
    let mut samples = Vec::with_capacity(RUNS);
    for input in inputs {
        let t = Instant::now();
        exec(&mut s, "cell.set", json!({"sheet": "Data", "cell": "A1", "input": input}))?;
        samples.push(t.elapsed());
    }

    let median = median_ms(&samples);
    println!("median-general-ms: {median:.3}");

    exec(&mut s, "cell.set", json!({"sheet": "Data", "cell": "A1", "input": "2"}))?;
    verify_calc_outputs(&s)?;
    Ok(())
}

fn exec(s: &mut Session, id: &str, params: serde_json::Value) -> Result<(), String> {
    s.execute(id, params).map_err(|e| e.to_string())?;
    Ok(())
}

fn fill_workbook(s: &mut Session) -> Result<(), String> {
    let d = s.doc_mut().map_err(|e| e.to_string())?;
    let wb = Arc::make_mut(&mut d.wb);
    {
        let data = wb.sheet_mut(0).ok_or_else(|| "missing Data sheet".to_string())?;
        data.cells.set(CellRef::new(0, 0), Cell::value(Value::Number(1.0)));
        data.cells.set(CellRef::new(0, 1), Cell::value(Value::Number(1.0)));
        data.cells.set(CellRef::new(0, 2), Cell::value(Value::text("alpha")));
        data.cells.set(CellRef::new(1, 1), Cell::value(Value::Number(2.0)));
        data.cells.set(CellRef::new(1, 2), Cell::value(Value::text("beta")));
    }
    {
        let calc = wb.sheet_mut(1).ok_or_else(|| "missing Calc sheet".to_string())?;
        for r in 0..ROWS {
            calc.cells.set(CellRef::new(r, 0), Cell::formula(Formula::new(LOOKUP_FORMULA)));
        }
    }
    Ok(())
}

fn verify_calc_outputs(s: &Session) -> Result<(), String> {
    let d = s.doc().map_err(|e| e.to_string())?;
    let wb = &d.wb;
    let last = ROWS.saturating_sub(1);
    for (label, row) in [("Calc!A1", 0u32), ("Calc!A100000", last)] {
        let got = wb.sheet(1).map(|sh| sh.value(CellRef::new(row, 0))).unwrap_or(Value::Empty);
        if got != Value::text("no") {
            return Err(format!("{label} expected text \"no\", got {got:?}"));
        }
    }
    Ok(())
}

fn median_ms(samples: &[std::time::Duration]) -> f64 {
    let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    ms.sort_by(f64::total_cmp);
    ms.get(samples.len() / 2).copied().unwrap_or(f64::MAX)
}

fn cpu_model() -> String {
    if let Some(name) = proc_cpu_model() {
        return name;
    }
    sysctl_brand()
}

fn proc_cpu_model() -> Option<String> {
    let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("model name") else {
            continue;
        };
        let Some((_, name)) = rest.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

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
