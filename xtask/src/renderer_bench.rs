//! Build/run guard for repeatable native renderer measurements.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{SecondsFormat, Utc};
use serde_json::{Value, json};
mod report;

#[path = "../../crates/ui-egui/examples/renderer_tools/provenance.rs"]
mod provenance;

const HELP: &str = "renderer-bench [--out DIR] [--profile release|dev] [--script latin|thai|mixed]\n  [--font FILE] [--font-sha256 HEX] [--config all|50x1|50x2|100x1|100x2|200x1|200x2]\n  [--cold 1..1000] [--warm 2..1000] [--snapshots]\nrenderer-bench compare BASE.json HEAD.json OUTPUT.md\nOutput must be gitignored or outside the checkout. See docs/renderer-benchmarks.md.\n";

fn version(root: &Path, program: &str) -> Result<String, String> {
    let out = Command::new(program).current_dir(root).arg("--version").output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("{program} --version failed"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn ignored_output(root: &Path, out: &Path) -> Result<(), String> {
    if let Ok(relative) = out.strip_prefix(root) {
        let status = Command::new("git").arg("-C").arg(root).args(["check-ignore", "-q", "--"]).arg(relative).status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("output inside the checkout must be gitignored (for example plan/renderer-bench)".into());
        }
    }
    Ok(())
}

fn run_directory(parent: &Path) -> Result<PathBuf, String> {
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.9fZ").to_string();
    for attempt in 0..100 {
        let path = parent.join(format!("run-{stamp}-{}-{attempt}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("could not exclusively create a fresh run directory".into())
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    if args.first() == Some(&"compare") {
        return report::compare(args.get(1..).unwrap_or_default());
    }
    if args.iter().any(|a| matches!(*a, "--help" | "-h")) {
        print!("{HELP}");
        return Ok(());
    }
    let mut profile = "release";
    let mut out = root.join("plan/renderer-bench");
    let mut forwarded = Vec::new();
    let mut iter = args.iter().copied();
    while let Some(arg) = iter.next() {
        match arg {
            "--profile" => profile = iter.next().ok_or("--profile requires release or dev")?,
            "--out" => out = PathBuf::from(iter.next().ok_or("--out requires a directory")?),
            "--snapshots" => forwarded.push(arg.to_owned()),
            "--script" | "--font" | "--font-sha256" | "--config" | "--cold" | "--warm" => {
                forwarded.push(arg.to_owned());
                forwarded.push(iter.next().ok_or_else(|| format!("missing value after {arg}"))?.to_owned());
            }
            _ => return Err(format!("unknown option {arg}\n{HELP}")),
        }
    }
    if !matches!(profile, "release" | "dev") {
        return Err("profile must be release or dev".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if out.is_relative() {
        out = std::env::current_dir().map_err(|e| e.to_string())?.join(out);
    }
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let out = out.canonicalize().map_err(|e| e.to_string())?;
    ignored_output(&root, &out)?;
    let out = run_directory(&out)?;
    let before = provenance::capture(&root)?;
    let mut build = crate::cargo();
    build
        .args(["build", "--locked", "--profile", profile, "-p", "gridcraft-ui-egui", "--example", "renderer_bench"])
        .env("GRIDCRAFT_BENCH_SOURCE_SHA256", &before.digest)
        .env("GRIDCRAFT_BENCH_SOURCE_HEAD", &before.head);
    if !build.status().map_err(|e| e.to_string())?.success() {
        return Err("renderer benchmark build failed".into());
    }
    provenance::unchanged(&before, &provenance::capture(&root)?)?;
    let meta = crate::metadata()?;
    let target = meta.get("target_directory").and_then(Value::as_str).ok_or("cargo metadata omitted target_directory")?;
    let binary = Path::new(target)
        .join(if profile == "dev" { "debug" } else { "release" })
        .join("examples")
        .join(format!("renderer_bench{}", std::env::consts::EXE_SUFFIX));
    let executable_hash = provenance::file_hash(&binary, 1024 * 1024 * 1024)?;
    let raw = out.join("samples.json");
    let status = Command::new(&binary).arg("--out").arg(&raw).args(&forwarded).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("renderer benchmark refused or failed the run; no verified report produced".into());
    }
    provenance::unchanged(&before, &provenance::capture(&root)?)?;
    if executable_hash != provenance::file_hash(&binary, 1024 * 1024 * 1024)? {
        return Err("executable changed during measurement".into());
    }
    let measurements = report::read(&raw)?;
    let environment: BTreeMap<_, _> = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.into_string().ok()?;
            let relevant = key.starts_with("CARGO_PROFILE_")
                || (key.starts_with("CARGO_TARGET_") && key.ends_with("_RUSTFLAGS"))
                || matches!(
                    key.as_str(),
                    "RUSTFLAGS" | "CARGO_ENCODED_RUSTFLAGS" | "RUSTC" | "RUSTC_WRAPPER" | "RUSTC_WORKSPACE_WRAPPER" | "CARGO_INCREMENTAL"
                );
            relevant.then(|| (key, value.to_string_lossy().into_owned()))
        })
        .collect();
    let report = json!({"schema": 1, "verified_source": true, "recorded_at_utc": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        "source": {"head": before.head, "sha256": before.digest, "dirty": before.manifest["dirty"]},
        "executable_sha256": executable_hash, "profile": profile,
        "rustc": version(&root, &std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))?,
        "cargo": version(&root, &std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))?,
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "rustflags": std::env::var("RUSTFLAGS").unwrap_or_default(),
        "build_environment": environment,
        "measurements": measurements});
    fs::write(out.join("source-manifest.json"), serde_json::to_vec_pretty(&before.manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::write(out.join("REPORT.md"), report::markdown(&report)?).map_err(|e| e.to_string())?;
    println!("Verified renderer measurements: {}", out.join("report.json").display());
    Ok(())
}
