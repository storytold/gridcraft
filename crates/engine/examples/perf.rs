//! Performance scenarios on large workbooks (release build):
//! `cargo run --release -p gridcraft-engine --example perf -- [rows] [--only NAME[,NAME…]]`
//!
//! Every scenario goes through `Session::execute`, the path the UI, CLI and MCP use, and checks
//! its answer: a fast wrong result is not a result. Scenarios: `basic` (fill, formulas, running
//! sum, aggregates, edit, sort, save, open, undo), `lookup` (exact VLOOKUP / XLOOKUP / MATCH over a
//! table, then an edit to the table), `colsum` (many `SUM(A:A)` over one column, then an edit),
//! `chain` (a long dependency chain in both directions).

#![allow(clippy::disallowed_methods)] // a native-only benchmark: wasm never runs it

use std::time::{Duration, Instant};

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use serde_json::json;

struct Bench {
    rows: usize,
    failures: Vec<String>,
}

impl Bench {
    fn time<R>(&self, label: &str, f: impl FnOnce() -> R) -> R {
        let t = Instant::now();
        let r = f();
        println!("  {label:<58} {:>10}", ms(t.elapsed()));
        r
    }

    fn check(&mut self, what: &str, s: &Session, a1: &str, want: f64) {
        let got = value(s, a1);
        if got != Value::Number(want) {
            println!("  WRONG {what}: {a1} = {got:?}, expected {want}");
            self.failures.push(format!("{what}: {a1} = {got:?}, expected {want}"));
        }
    }
}

fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

fn value(s: &Session, a1: &str) -> Value {
    let Some(c) = CellRef::parse(a1) else { return Value::Empty };
    s.active().and_then(|d| d.wb.active()).map(|sh| sh.value(c)).unwrap_or_default()
}

fn run(s: &mut Session, id: &str, p: serde_json::Value) {
    if let Err(e) = s.execute(id, p) {
        println!("  ERROR {id}: {e}");
    }
}

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

/// Writes `rows` rows starting at `top` (1-based) in column `col`, in chunks.
fn fill(s: &mut Session, col: &str, top: usize, rows: usize, mut row: impl FnMut(usize) -> serde_json::Value) {
    let chunk = 10_000;
    let mut r = 0;
    while r < rows {
        let n = chunk.min(rows - r);
        let vals: Vec<serde_json::Value> = (r..r + n).map(&mut row).collect();
        run(s, "range.setValues", json!({"range": format!("{col}{}", top + r), "values": vals}));
        r += n;
    }
}

fn basic(b: &mut Bench) {
    let rows = b.rows;
    let mut s = session();
    b.time(&format!("fill {rows}x6 values"), || {
        fill(&mut s, "A", 1, rows, |i| json!([format!("Item {i}"), i as f64, (i % 97) as f64, (i % 13) as f64 * 1.5, 1.0, 2.0]))
    });
    b.time(&format!("fill {rows} formulas G=B*C+D"), || {
        run(&mut s, "cell.set", json!({"cell": "G1", "input": "=B1*C1+D1"}));
        run(&mut s, "edit.autoFill", json!({"source": "G1", "target": format!("G1:G{rows}")}));
    });
    b.time(&format!("fill {rows} chained running-sum formulas"), || {
        run(&mut s, "cell.set", json!({"cell": "H1", "input": "=G1"}));
        run(&mut s, "cell.set", json!({"cell": "H2", "input": "=H1+G2"}));
        run(&mut s, "edit.autoFill", json!({"source": "H2", "target": format!("H2:H{rows}")}));
    });
    b.time("aggregate formulas (SUM, SUMIFS, XLOOKUP)", || {
        run(&mut s, "cell.set", json!({"cell": "J1", "input": format!("=SUM(G1:G{rows})")}));
        run(&mut s, "cell.set", json!({"cell": "J2", "input": "=SUMIFS(B:B,C:C,\">50\")"}));
        run(&mut s, "cell.set", json!({"cell": "J3", "input": "=XLOOKUP(\"Item 7777\",A:A,B:B)"}));
    });
    if rows > 7777 {
        b.check("XLOOKUP over a column", &s, "J3", 7777.0);
    }
    b.time("edit B1: recalc everything below", || run(&mut s, "cell.set", json!({"cell": "B1", "input": "5"})));
    let total: f64 = (0..rows).map(|i| (if i == 0 { 5.0 } else { i as f64 }) * (i % 97) as f64 + (i % 13) as f64 * 1.5).sum();
    b.check("running sum after edit", &s, &format!("H{rows}"), total);
    b.time(&format!("sort {rows} rows"), || run(&mut s, "data.sortDescending", json!({"range": format!("A1:F{rows}"), "column": "C"})));
    b.time("undo the sort", || run(&mut s, "edit.undo", json!({})));
    let bytes = b.time("save xlsx", || s.execute("file.saveBytes", json!({"format": "xlsx"})).ok());
    let b64 = bytes.as_ref().and_then(|r| r["base64"].as_str()).unwrap_or("").to_string();
    b.time("open xlsx + full recalc", || run(&mut s, "file.open", json!({"name": "big.xlsx", "base64": b64})));
}

/// Exact-match lookups: `n` formulas over an `n`-row table, the shape that is O(n²) when every
/// lookup copies and scans the table.
fn lookup(b: &mut Bench) {
    let n = (b.rows / 5).max(1);
    let mut s = session();
    fill(&mut s, "A", 1, n, |i| json!([format!("K{i}"), (i * 2) as f64]));
    b.time(&format!("{n} exact VLOOKUPs over a {n}-row table"), || {
        fill(&mut s, "D", 1, n, |i| json!([format!("=VLOOKUP(\"K{}\",$A$1:$B${n},2,FALSE)", n - 1 - i)]))
    });
    b.check("VLOOKUP", &s, "D1", ((n - 1) * 2) as f64);
    b.time(&format!("{n} XLOOKUPs"), || fill(&mut s, "E", 1, n, |i| json!([format!("=XLOOKUP(\"K{i}\",$A$1:$A${n},$B$1:$B${n})")])));
    b.check("XLOOKUP", &s, &format!("E{n}"), ((n - 1) * 2) as f64);
    b.time(&format!("{n} MATCHes"), || fill(&mut s, "F", 1, n, |i| json!([format!("=MATCH(\"K{i}\",$A$1:$A${n},0)")])));
    b.check("MATCH", &s, &format!("F{n}"), n as f64);
    b.time(&format!("edit B{n}: {} lookups depend on the table", 3 * n), || run(&mut s, "cell.set", json!({"cell": format!("B{n}"), "input": "-1"})));
    b.check("VLOOKUP after edit", &s, "D1", -1.0);
}

/// Many whole-column aggregates over one large column.
fn colsum(b: &mut Bench) {
    let rows = b.rows;
    let formulas = 1000;
    let mut s = session();
    fill(&mut s, "A", 1, rows, |i| json!([(i % 10) as f64]));
    b.time(&format!("{formulas} x =SUM(A:A) over {rows} rows"), || fill(&mut s, "C", 1, formulas, |_| json!(["=SUM(A:A)"])));
    let sum: f64 = (0..rows).map(|i| (i % 10) as f64).sum();
    b.check("SUM(A:A)", &s, &format!("C{formulas}"), sum);
    b.time(&format!("edit A1: {formulas} SUMs recalc"), || run(&mut s, "cell.set", json!({"cell": "A1", "input": "100"})));
    b.check("SUM(A:A) after edit", &s, "C1", sum + 100.0);
}

/// A dependency chain in row order and in reverse order (each cell reads the one below it): both
/// are valid, neither is circular.
fn chain(b: &mut Bench) {
    let n = b.rows.clamp(1, 20_000);
    let mut s = session();
    b.time(&format!("{n}-cell chain, top to bottom"), || {
        run(&mut s, "cell.set", json!({"cell": "A1", "input": "1"}));
        fill(&mut s, "A", 2, n - 1, |i| json!([format!("=A{}+1", i + 1)]));
    });
    b.check("chain", &s, &format!("A{n}"), n as f64);
    b.time(&format!("{n}-cell chain, bottom to top"), || {
        fill(&mut s, "C", 1, n, |i| if i + 1 == n { json!([1]) } else { json!([format!("=C{}+1", i + 2)]) })
    });
    b.check("reverse chain", &s, "C1", n as f64);
    b.time("edit the chain's start: recalc the whole chain", || run(&mut s, "cell.set", json!({"cell": "A1", "input": "5"})));
    b.check("chain after edit", &s, &format!("A{n}"), (n + 4) as f64);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rows = args.iter().find_map(|a| a.parse().ok()).unwrap_or(100_000usize).max(10);
    let only: Option<Vec<String>> =
        args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).map(|v| v.split(',').map(str::to_string).collect());
    let scenarios: [(&str, fn(&mut Bench)); 4] = [("basic", basic), ("lookup", lookup), ("colsum", colsum), ("chain", chain)];
    let mut b = Bench { rows, failures: Vec::new() };
    let t = Instant::now();
    for (name, f) in scenarios {
        if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == name)) {
            continue;
        }
        println!("{name}:");
        f(&mut b);
    }
    println!("total {}", ms(t.elapsed()));
    if !b.failures.is_empty() {
        println!("{} wrong results", b.failures.len());
        std::process::exit(1);
    }
}
