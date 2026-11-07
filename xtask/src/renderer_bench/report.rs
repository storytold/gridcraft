//! Bounded report loading, readable tables and comparisons without historical revision labels.
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use serde_json::Value;

pub fn read(path: &Path) -> Result<Value, String> {
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|e| e.to_string())?.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("report exceeds 8 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn configs(report: &Value) -> Result<&Vec<Value>, String> {
    if report["schema"] != 1 || report["verified_source"] != true {
        return Err("expected a source-verified schema1 report".into());
    }
    report["measurements"]["configurations"].as_array().ok_or_else(|| "missing configurations".into())
}

fn ns(row: &Value, key: &str) -> Result<f64, String> {
    row[key].as_u64().map(|n| n as f64 / 1_000_000.0).ok_or_else(|| format!("missing/noninteger {key}"))
}

pub fn markdown(report: &Value) -> Result<String, String> {
    let mut text = format!(
        "# Grid renderer measurements\n\nRecorded UTC: {}\n\nSource: {}\n\nProfile: {}\n\n",
        report["recorded_at_utc"], report["source"], report["profile"]
    );
    text.push_str("| Zoom / DPI | Cold first-pass p50 / p95 ms | Warm p50 / p95 ms |\n|---|---:|---:|\n");
    for row in configs(report)? {
        text.push_str(&format!(
            "| {} / {} | {:.4} / {:.4} | {:.4} / {:.4} |\n",
            row["zoom_percent"],
            row["dpi"],
            ns(row, "cold_p50_ns")?,
            ns(row, "cold_p95_ns")?,
            ns(row, "warm_p50_ns")?,
            ns(row, "warm_p95_ns")?
        ));
    }
    text.push_str("\nCPU/UI only; GPU rendering, encoding, workbook construction and evidence checks are excluded. Warmup checks output stability, not adapter completion or text-layout correctness. Raw samples and the source manifest accompany this report.\n");
    Ok(text)
}

pub fn compare(args: &[&str]) -> Result<(), String> {
    let [base_path, head_path, output] = args else {
        return Err("compare requires BASE.json HEAD.json OUTPUT.md".into());
    };
    let base = read(Path::new(base_path))?;
    let head = read(Path::new(head_path))?;
    compatible(&base, &head)?;
    let mut text = String::from(
        "# Renderer comparison\n\nObserved timings; no statistical-significance or GPU/presentation claim. Run both revisions on the same idle host and alternate run order.\n\n| Zoom / DPI | Base warm p50/p95 ms | Head warm p50/p95 ms |\n|---|---:|---:|---:|\n",
    );
    for (a, b) in configs(&base)?.iter().zip(configs(&head)?) {
        text.push_str(&format!(
            "| {} / {} | {:.4} / {:.4} | {:.4} / {:.4} |\n",
            a["zoom_percent"],
            a["dpi"],
            ns(a, "warm_p50_ns")?,
            ns(a, "warm_p95_ns")?,
            ns(b, "warm_p50_ns")?,
            ns(b, "warm_p95_ns")?
        ));
    }
    text.push_str(&format!("\nBase source: {}\n\nHead source: {}\n", base["source"], head["source"]));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| e.to_string())?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    println!("Comparison: {output}");
    Ok(())
}

fn compatible(base: &Value, head: &Value) -> Result<(), String> {
    for key in ["profile", "rustc", "cargo", "os", "arch", "rustflags", "build_environment"] {
        if base.get(key).is_none() || base[key] != head[key] {
            return Err(format!("comparison requires matching {key}"));
        }
    }
    for key in ["script", "font", "workload_sha256", "cell_scalars"] {
        if base["measurements"].get(key).is_none() || base["measurements"][key] != head["measurements"][key] {
            return Err(format!("comparison requires matching workload {key}"));
        }
    }
    let a = configs(base)?;
    let b = configs(head)?;
    if a.is_empty() || a.len() != b.len() {
        return Err("configuration sets must match".into());
    }
    for (a, b) in a.iter().zip(b) {
        for key in ["zoom_percent", "dpi", "visible_cells", "cell_geometry_sha256"] {
            if a.get(key).is_none() || a[key] != b[key] {
                return Err(format!("configuration {key} differs"));
            }
        }
        for key in ["cold_first_pass_ns", "warm_pass_ns"] {
            let a = a[key].as_array().filter(|s| !s.is_empty()).map(Vec::len);
            let b = b[key].as_array().filter(|s| !s.is_empty()).map(Vec::len);
            if a.is_none() || a != b {
                return Err("comparison requires matching nonempty sample counts".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn report() -> Value {
        json!({"schema":1,"verified_source":true,"profile":"release","rustc":"test","cargo":"test","os":"test","arch":"test","rustflags":"","build_environment":{},
            "measurements":{"script":"latin","font":{},"workload_sha256":"fixture","cell_scalars":80,
                "configurations":[{"zoom_percent":100,"dpi":1,"visible_cells":500,"cell_geometry_sha256":"fixture","cold_first_pass_ns":[1],"warm_pass_ns":[1,2]}]}})
    }
    #[test]
    fn refuses_comparing_incompatible_measurements() {
        let a = report();
        assert!(compatible(&a, &a).is_ok());
        for key in ["profile", "rustc", "os", "arch", "rustflags", "build_environment"] {
            let mut b = a.clone();
            b[key] = json!("different");
            assert!(compatible(&a, &b).is_err());
        }
        let mut b = a.clone();
        b["measurements"]["font"] = json!({"sha256":"different"});
        assert!(compatible(&a, &b).is_err());
        b = a.clone();
        b["verified_source"] = json!(false);
        assert!(compatible(&a, &b).is_err());
        b = a.clone();
        b["measurements"]["configurations"][0]["warm_pass_ns"] = json!([]);
        assert!(compatible(&a, &b).is_err());
        b = a.clone();
        b["measurements"]["configurations"][0]["cell_geometry_sha256"] = json!("different");
        assert!(compatible(&a, &b).is_err());
    }
}
