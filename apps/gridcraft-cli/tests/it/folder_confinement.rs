//! Real CLI startup checks; all synthetic fixtures are retained.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn base(label: &str) -> PathBuf {
    #[allow(clippy::disallowed_methods)] // Native-only filesystem fixture; retain every run.
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let p = std::env::temp_dir().join(format!("gridcraft-cli-confinement-{label}-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(p.join("allowed")).unwrap();
    std::fs::create_dir_all(p.join("outside")).unwrap();
    std::fs::write(p.join("allowed/in.csv"), "inside,7\n").unwrap();
    std::fs::write(p.join("outside/in.csv"), "outside,91\n").unwrap();
    p
}
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gridcraft-cli")).args(args).stdin(Stdio::null()).output().unwrap()
}
#[test]
fn folder_confinement_cli_checks_initial_in_before_reading() {
    let p = base("startup");
    let root = p.join("allowed");
    let outside = p.join("outside/in.csv");
    let inside = root.join("in.csv");
    let denied = run(&["mcp", "--automation-read-root", root.to_str().unwrap(), "--in", outside.to_str().unwrap()]);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("outside the read root"));
    let allowed = run(&["mcp", "--automation-read-root", root.to_str().unwrap(), "--in", inside.to_str().unwrap()]);
    assert!(allowed.status.success(), "{}", String::from_utf8_lossy(&allowed.stderr));
    let inline = format!("--automation-read-root={}", root.display());
    assert!(run(&["mcp", &inline, "--in", inside.to_str().unwrap()]).status.success());
}
#[test]
fn folder_confinement_cli_rejects_ambiguous_or_unenforceable_configuration() {
    let p = base("options");
    let root = p.join("allowed");
    let r = root.to_str().unwrap();
    for args in [
        vec!["mcp", "--automation-read-root", r, "--automation-read-root", r],
        vec!["mcp", "--automation-write-root", r, "--automation-write-root", r],
        vec!["mcp", "--connect", "7979", "--automation-read-root", r],
        vec!["mcp", "--automation-read-root", ""],
        vec!["mcp", "--automation-read-root"],
        vec!["mcp", "--automation-read-root", "--sample", "sales"],
    ] {
        let o = run(&args);
        assert!(!o.status.success(), "{args:?}");
    }
    assert!(!run(&["mcp", "--automation-read-root", p.join("missing").to_str().unwrap()]).status.success());
    assert!(!run(&["mcp", "--automation-read-root", root.join("in.csv").to_str().unwrap()]).status.success());
}
