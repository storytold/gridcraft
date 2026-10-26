//! `cargo xtask bench`: runs the calculation benchmarks (`crates/engine/examples/bench.rs`) in a
//! release build, writes the results to `<target>/bench/latest.json` and compares them with a
//! baseline (`<target>/bench/baseline.json` unless `--baseline PATH`). `--save` makes this run
//! the baseline. Timings are machine-specific, so baselines stay in the target directory; the
//! reference numbers in docs/target-app-parity.md name the machine they came from.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Flags passed through to the bench binary (the others are xtask's own).
const PASS: [&str; 3] = ["--rows", "--reps", "--filter"];

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    // Cargo resolves a relative CARGO_TARGET_DIR against the directory it runs in, which for the
    // bench is the workspace root; resolve it the same way here.
    let target = std::env::var_os("CARGO_TARGET_DIR").map(|d| root.join(d)).unwrap_or_else(|| root.join("target"));
    let dir = target.join("bench");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let latest = dir.join("latest.json");
    let mut baseline = dir.join("baseline.json");
    let mut save = false;
    let mut pass: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "--save" => save = true,
            "--quick" => pass.push(a.into()),
            "--baseline" => baseline = it.next().map(PathBuf::from).ok_or("--baseline needs a path")?,
            _ if PASS.contains(&a) => {
                let v = it.next().ok_or_else(|| format!("{a} needs a value"))?;
                pass.extend([a.to_string(), v.to_string()]);
            }
            other => return Err(format!("bench: unknown flag `{other}`")),
        }
    }
    let mut cmd = crate::cargo();
    cmd.args(["run", "--release", "--quiet", "-p", "gridcraft-engine", "--example", "bench", "--"]).args(&pass).arg("--json").arg(&latest);
    // A stale file from an earlier run must not pass for this one's results.
    let _ = std::fs::remove_file(&latest);
    // The bench exits non-zero when a scenario fails but still writes the others' results:
    // compare those, then report the failure.
    let status = crate::run(cmd, "cargo run --release -p gridcraft-engine --example bench");
    let now = match read(&latest) {
        Ok(v) => v,
        Err(e) => return status.and(Err(e)),
    };
    if save && status.is_err() {
        return status.map_err(|e| format!("{e}\nnot saved as the baseline: a scenario failed"));
    }
    if save {
        std::fs::copy(&latest, &baseline).map_err(|e| format!("{}: {e}", baseline.display()))?;
        println!("\nSaved as the baseline: {}", baseline.display());
        return Ok(());
    }
    if baseline.exists() {
        // A baseline that is there but unreadable is an error, not a missing one.
        compare(&read(&baseline)?, &now);
    } else {
        println!("\nNo baseline at {} (run `cargo xtask bench --save` to make one).", baseline.display());
    }
    status
}

fn read(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn results(v: &Value) -> Vec<(String, f64)> {
    v.get("results")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|r| Some((r.get("name")?.as_str()?.to_string(), r.get("median_ms")?.as_f64()?))).collect())
        .unwrap_or_default()
}

/// Prints the median change per scenario; a different row count makes the numbers incomparable.
fn compare(base: &Value, now: &Value) {
    if base.get("rows") != now.get("rows") {
        println!("\nBaseline was run with {} rows, this run with {}: not compared.", base["rows"], now["rows"]);
        return;
    }
    let old = results(base);
    println!("\nAgainst the baseline (median):");
    for (name, ms) in results(now) {
        match old.iter().find(|(n, _)| *n == name) {
            Some((_, was)) if *was > 0.0 => {
                let pct = (ms - was) / was * 100.0;
                println!("  {name:<16} {was:>10.2} ms -> {ms:>10.2} ms  {pct:+6.1}%");
            }
            _ => println!("  {name:<16} {:>13} -> {ms:>10.2} ms", "new"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_results() {
        let v = json!({"rows": 10, "results": [{"name": "edit.chain", "median_ms": 1.5}, {"name": "bad"}]});
        assert_eq!(results(&v), vec![("edit.chain".to_string(), 1.5)]);
        assert!(results(&json!({})).is_empty());
    }
}
