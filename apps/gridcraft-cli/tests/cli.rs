#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_gridcraft-cli"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("run gridcraft-cli")
}

fn ok(args: &[&str]) -> String {
    let o = run(args);
    assert!(o.status.success(), "gridcraft-cli {args:?} failed: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gridcraft-cli-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn version_and_usage() {
    assert_eq!(ok(&["--version"]).trim(), format!("gridcraft-cli {}", env!("CARGO_PKG_VERSION")));
    assert_eq!(ok(&["version"]).trim(), format!("gridcraft-cli {}", env!("CARGO_PKG_VERSION")));
    let o = run(&["frobnicate"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("unknown subcommand"));
    let o = run(&[]);
    assert!(!o.status.success());
}

#[test]
fn eval_formulas() {
    assert_eq!(ok(&["eval", "=SUM(1,2,3)"]).trim(), "6");
    assert_eq!(ok(&["eval", "1/4"]).trim(), "0.25");
    assert_eq!(ok(&["eval", "=UPPER(\"abc\")"]).trim(), "ABC");
    assert_eq!(ok(&["eval", "=1/0"]).trim(), "#DIV/0!");
    assert_eq!(ok(&["eval", "=2>1", "--json"]).trim(), "true");
    let o = run(&["eval", "=1", "--in", "/no/such/file.xlsx"]);
    assert!(!o.status.success());
    assert!(!String::from_utf8_lossy(&o.stderr).is_empty());
}

#[test]
fn run_then_cat_and_info_on_sample() {
    let dir = tmpdir("run");
    let book = dir.join("budget.xlsx");
    let book_s = book.to_str().unwrap();
    let script = dir.join("steps.jsonl");
    std::fs::write(
        &script,
        "# steps\n{\"command\":\"cell.set\",\"params\":{\"cell\":\"K2\",\"input\":\"=K1*2\"}}\nhome.bold {\"range\":\"K1:K2\"}\n",
    )
    .unwrap();
    let out = ok(&[
        "run",
        "--sample",
        "budget",
        "--cmd",
        r#"cell.set={"cell":"K1","input":"21"}"#,
        "--script",
        script.to_str().unwrap(),
        "--out",
        book_s,
        "--print",
        "K1:K2",
        "--quiet",
    ]);
    assert_eq!(out.split_whitespace().collect::<Vec<_>>(), ["21", "42"], "{out}");
    assert!(book.exists());

    let table = ok(&["cat", book_s, "--range", "K1:K2"]);
    assert_eq!(table.split_whitespace().collect::<Vec<_>>(), ["21", "42"]);
    let formulas = ok(&["cat", book_s, "--range", "K2", "--formulas"]);
    assert_eq!(formulas.trim(), "=K1*2");
    let whole = ok(&["cat", book_s]);
    assert!(whole.lines().count() > 3, "{whole}");

    let info = ok(&["info", book_s]);
    assert!(info.contains("Budget"), "{info}");
    let json: serde_json::Value = serde_json::from_str(&ok(&["info", book_s, "--json"])).unwrap();
    assert!(json["sheets"][0]["cells"].as_u64().unwrap() > 10);

    // A failing command gives a non-zero exit and names the step.
    let o = run(&["run", "--cmd", "no.such.command"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no.such.command"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn convert_xlsx_csv_roundtrip() {
    let dir = tmpdir("convert");
    let xlsx = dir.join("in.xlsx");
    let csv = dir.join("out.csv");
    let back = dir.join("back.xlsx");
    ok(&[
        "run",
        "--cmd",
        r#"range.setValues={"range":"A1","values":[["Name","Qty"],["apple",3],["pear, green",4],["total","=SUM(B2:B3)"]]}"#,
        "--out",
        xlsx.to_str().unwrap(),
        "--quiet",
    ]);
    ok(&["convert", xlsx.to_str().unwrap(), csv.to_str().unwrap()]);
    let text = std::fs::read_to_string(&csv).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines, ["Name,Qty", "apple,3", "\"pear, green\",4", "total,7"], "{text}");
    ok(&["convert", csv.to_str().unwrap(), back.to_str().unwrap()]);
    let again = ok(&["cat", back.to_str().unwrap(), "--csv"]);
    assert_eq!(again.lines().collect::<Vec<_>>(), lines);
    let o = run(&["convert", xlsx.to_str().unwrap(), dir.join("x.unknown").to_str().unwrap()]);
    assert!(!o.status.success());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn commands_and_functions() {
    let out = ok(&["commands", "--search", "bold"]);
    assert!(out.contains("home.bold"));
    let v: serde_json::Value = serde_json::from_str(&ok(&["commands", "--json"])).unwrap();
    assert!(v.as_array().unwrap().len() > 100);
    assert!(ok(&["functions", "--search", "vlookup"]).contains("VLOOKUP"));
}

#[test]
fn mcp_over_stdio() {
    let mut child = bin().arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#).unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"evaluate_formula","arguments":{{"formula":"=6*7"}}}}}}"#
        )
        .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "gridcraft");
    assert!(lines[1]["result"]["content"][0]["text"].as_str().unwrap().contains("42"));
}

#[test]
fn check_exit_codes_and_json() {
    let dir = tmpdir("check");
    let book = dir.join("calc.xlsx");
    let book_s = book.to_str().unwrap();
    ok(&["run", "--cmd", r#"cell.set={"cell":"A1","input":"2"}"#, "--cmd", r#"cell.set={"cell":"B1","input":"=A1*3"}"#, "--out", book_s, "--quiet"]);
    // GridCraft's own file: its cached values are its own results, and it round-trips.
    let o = run(&["check", dir.to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("1 checked: 1 matched, 0 mismatched"), "{out}");
    let v: serde_json::Value = serde_json::from_str(&ok(&["check", book_s, "--json"])).unwrap();
    assert_eq!(v["schema"], "gridcraft-check/1");
    assert_eq!(v["totals"]["matched"], 1);
    // A file that can't be opened is reported, not fatal, and fails the check.
    let o = run(&["check", book_s, "/no/such/file.xlsx"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stdout).contains("1 failed to open"));
    // Usage errors exit 2.
    assert_eq!(run(&["check"]).status.code(), Some(2));
    assert_eq!(run(&["check", book_s, "--tolerance", "x"]).status.code(), Some(2));
    assert_eq!(run(&["check", book_s, "--bogus"]).status.code(), Some(2));
}

#[test]
fn dax_format_parse_and_errors() {
    assert_eq!(ok(&["dax", "format", "1 + 2 * 3"]).trim(), "1 + 2 * 3");
    assert_eq!(ok(&["dax", "format", "(1 + 2) * 3"]).trim(), "(1 + 2) * 3");
    assert_eq!(ok(&["dax", "format", "-2 ^ 2"]).trim(), "-2 ^ 2");
    assert!(ok(&["dax", "parse", "1 + 2 * 3"]).contains("Binary"));

    assert_eq!(ok(&["dax", "format", "'Sales'[Amount] + 1"]).trim(), "'Sales'[Amount] + 1");
    assert!(ok(&["dax", "parse", "'It''s'[A]]B]"]).contains("It's"));

    let query = ok(&["dax", "format", "EVALUATE SUM('Sales'[Amount])"]);
    assert_eq!(query.trim(), "EVALUATE SUM('Sales'[Amount])");
    assert!(ok(&["dax", "parse", "EVALUATE 1"]).contains("Query"));

    let o = run(&["dax", "parse", "1 +"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("expected expression"));
    let o = run(&["dax", "format", "EVALUATE 1 +"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("end of input"));

    let dir = tmpdir("dax");
    let file = dir.join("expr.dax");
    std::fs::write(&file, "1 + 2 * 3").unwrap();
    assert_eq!(ok(&["dax", "format", "--file", file.to_str().unwrap()]).trim(), "1 + 2 * 3");
    let _ = std::fs::remove_dir_all(dir);

    let mut child = bin().args(["dax", "format"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.as_mut().unwrap().write_all(b"1 + 2 * 3").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "1 + 2 * 3");
}
