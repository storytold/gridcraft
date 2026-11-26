//! Workspace tooling: `cargo xtask <command>` (or `cargo run -p xtask -- <command>`).
//!
//! Pure Rust (std + serde_json, plus the engine for `parity`). External tools (`cargo`, `git`)
//! are invoked through `std::process::Command`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod assets;
mod i18n_report;
mod ico;
mod keytips;
mod layers;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod parity;
mod stats;
mod version;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  ci              i18n-report, fmt --check, clippy -D warnings, test, doctest, assets, layers, wasm,
                  parity --check, keytips --check (stops at the first failure; the parity check runs
                  only if docs/parity-checklist.md exists)
  test            workspace tests (cargo nextest when installed, else cargo test), then doctests
  assets          check that every icon/image/font/sample file is attributed in ATTRIBUTION.md
  i18n-report     list the interface strings each language doesn't translate yet (always passes)
  layers          enforce the crate dependency layering (AGENTS.md \"Layering\")
  wasm            cargo check --target wasm32-unknown-unknown for the L0-L5 crates (+ gridcraft-web), one call
  parity [--check] [--out PATH]
                  write docs/parity-checklist.md from gridcraft_engine::catalog (implemented vs missing ids)
  keytips [--check]
                  regenerate crates/ui-egui/src/keytips/generated.rs from keytips.json (--check: fail if stale)
  version [set X.Y.Z[-pre]]
                  print or set the workspace version ([workspace.package] in Cargo.toml)
  stats [--exact] count tests and lines per crate (--exact: ask the test harness via `-- --list`)
  ico <out.ico> <in.png>...
                  pack square PNGs (<= 256 px) into a Windows .ico (see packaging/icons.sh)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("assets") => assets::run(&root()),
        Some("i18n-report") => i18n_report::run(&root()),
        Some("layers") => cmd_layers(),
        Some("wasm") => cmd_wasm(),
        Some("test") => cmd_test().and_then(|()| cmd_doctest()),
        Some("ci") => cmd_ci(),
        Some("stats") => stats::run(&root(), rest.contains(&"--exact")),
        Some("parity") => parity::run(&root(), &rest),
        Some("keytips") => keytips::run(&root(), &rest),
        Some("ico") => ico::run(&rest),
        Some("version") => version::run(&root(), &rest),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (AGENTS.md \"Layering\")\n");
    println!("{:<28} {:<10} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<10} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// Workspace packages that must build for wasm32: every L0–L5 crate, plus the web app once it
/// exists (`apps/gridcraft-web`).
fn wasm_set() -> Result<Vec<String>, String> {
    let crates = layers::from_metadata(&metadata()?)?;
    Ok(crates
        .into_iter()
        .filter(|c| match layers::classify(&c.name) {
            Some(layers::Class::Layer(l)) => l <= 5 || c.name == "gridcraft-web",
            _ => false,
        })
        .map(|c| c.name)
        .collect())
}

/// One `cargo check` for the whole set; `--keep-going` reports every failing crate, not just the
/// first.
fn cmd_wasm() -> Result<(), String> {
    let set = wasm_set()?;
    if set.is_empty() {
        return Err("no wasm crates found in the workspace".into());
    }
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown", "--keep-going"]);
    for p in &set {
        c.args(["-p", p]);
    }
    let r = run(c, &format!("cargo check --target wasm32-unknown-unknown --keep-going -p {}", set.join(" -p ")));
    println!("\nwasm32-unknown-unknown check ({}): {}", set.join(", "), if r.is_ok() { "ok" } else { "FAIL" });
    if r.is_err() {
        println!("(if the target is missing: rustup target add wasm32-unknown-unknown)");
    }
    r
}

/// Targets for the test build. Examples are left out so they aren't linked; clippy
/// `--all-targets` still compiles them.
const TEST_TARGETS: [&str; 4] = ["--workspace", "--lib", "--bins", "--tests"];

fn nextest_available() -> bool {
    cargo().args(["nextest", "--version"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn cmd_test() -> Result<(), String> {
    let mut c = cargo();
    let tool = if nextest_available() {
        c.args(["nextest", "run"]);
        "nextest run"
    } else {
        eprintln!("note: cargo-nextest is not installed, running cargo test (cargo install cargo-nextest --locked)");
        c.arg("test");
        "test"
    };
    c.args(TEST_TARGETS);
    run(c, &format!("cargo {tool} {}", TEST_TARGETS.join(" ")))
}

/// nextest does not run doctests.
fn cmd_doctest() -> Result<(), String> {
    let mut c = cargo();
    c.args(["test", "--workspace", "--doc"]);
    run(c, "cargo test --workspace --doc")
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        // Missing translations never fail the gate; the list goes first so every log shows it.
        ("i18n-report", Box::new(|| i18n_report::run(&root()))),
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--keep-going", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets --keep-going -- -D warnings")
            }),
        ),
        ("test", Box::new(cmd_test)),
        ("doctest", Box::new(cmd_doctest)),
        ("assets", Box::new(|| assets::run(&root()))),
        ("layers", Box::new(cmd_layers)),
        ("wasm", Box::new(cmd_wasm)),
        (
            "parity",
            Box::new(|| {
                if root().join("docs/parity-checklist.md").exists() {
                    parity::run(&root(), &["--check"])
                } else {
                    println!("parity: docs/parity-checklist.md not generated yet; skipped (run `cargo xtask parity`)");
                    Ok(())
                }
            }),
        ),
        ("keytips", Box::new(|| keytips::run(&root(), &["--check"]))),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            for (n, _) in steps.iter().skip(done.len() + 1) {
                println!("  skip  {n}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}
