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

fn run_with_closed_stdout(args: &[&str]) -> Output {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    bin().args(args).stdout(writer).output().unwrap()
}

#[test]
fn broken_stdout_still_runs_later_commands_and_saves_workbook() {
    let dir = tmpdir("broken-stdout-save");
    let book = dir.join("out.xlsx");
    let out = run_with_closed_stdout(&[
        "run",
        "--cmd",
        r#"cell.set={"cell":"A1","input":"first"}"#,
        "--cmd",
        r#"cell.set={"cell":"A2","input":"saved"}"#,
        "--out",
        book.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(book.exists(), "workbook was not saved");
    assert_eq!(ok(&["cat", book.to_str().unwrap(), "--range", "A1:A2"]).split_whitespace().collect::<Vec<_>>(), ["first", "saved"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn stdout_closing_after_a_written_line_still_finishes_the_save() {
    use std::io::{BufRead, BufReader};

    let dir = tmpdir("stdout-closes-after-line");
    let script = dir.join("steps.txt");
    let book = dir.join("out.xlsx");
    let mut steps = format!("{}\n", r#"cell.set={"cell":"A1","input":"first"}"#).repeat(8192);
    steps.push_str(r#"cell.set={"cell":"A2","input":"saved"}"#);
    std::fs::write(&script, steps).unwrap();
    let (reader, writer) = std::io::pipe().unwrap();
    let child = bin()
        .args(["run", "--script", script.to_str().unwrap(), "--out", book.to_str().unwrap()])
        .stdout(writer)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(reader);
    let mut first = String::new();
    assert!(reader.read_line(&mut first).unwrap() > 0);
    drop(reader);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(ok(&["cat", book.to_str().unwrap(), "--range", "A2"]).trim(), "saved");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn broken_stdout_does_not_hide_later_work_or_save_errors() {
    let dir = tmpdir("broken-stdout-errors");
    let book = dir.join("missing").join("out.xlsx");
    let fail_save = run_with_closed_stdout(&["run", "--cmd", r#"cell.set={"cell":"A1","input":"first"}"#, "--out", book.to_str().unwrap()]);
    assert_eq!(fail_save.status.code(), Some(1), "save failure must return 1 without panicking");
    assert!(!fail_save.stderr.is_empty(), "save failure must include a diagnostic");
    let fail_command = run_with_closed_stdout(&["run", "--cmd", r#"cell.set={"cell":"A1","input":"first"}"#, "--cmd", "no.such.command"]);
    assert_eq!(fail_command.status.code(), Some(1), "later command failure must return 1 without panicking");
    assert!(String::from_utf8_lossy(&fail_command.stderr).contains("no.such.command"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn closed_stdout_and_stderr_do_not_hide_successful_save() {
    let dir = tmpdir("closed-both-save");
    let book = dir.join("out.xlsx");
    let (reader, writer) = std::io::pipe().unwrap();
    let error_writer = writer.try_clone().unwrap();
    drop(reader);
    let status = bin()
        .args(["run", "--cmd", r#"cell.set={"cell":"A1","input":"saved"}"#, "--out", book.to_str().unwrap()])
        .stdout(Stdio::from(writer))
        .stderr(Stdio::from(error_writer))
        .status()
        .unwrap();
    assert!(status.success(), "successful save must not fail after the reader closes: {status}");
    assert_eq!(ok(&["cat", book.to_str().unwrap(), "--range", "A1"]).trim(), "saved");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn closed_stdout_and_stderr_still_report_work_failure() {
    let (reader, writer) = std::io::pipe().unwrap();
    let error_writer = writer.try_clone().unwrap();
    drop(reader);
    let status = bin()
        .args(["run", "--cmd", r#"cell.set={"cell":"A1","input":"first"}"#, "--cmd", "no.such.command"])
        .stdout(Stdio::from(writer))
        .stderr(Stdio::from(error_writer))
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1), "failed work should exit 1 without panicking");
}

#[test]
fn stdout_errors_other_than_broken_pipe_fail() {
    let read_only = std::fs::File::open(env!("CARGO_BIN_EXE_gridcraft-cli")).unwrap();
    let out = bin().arg("version").stdout(Stdio::from(read_only)).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("can't write to stdout"));
}

#[test]
fn closed_stdout_preserves_read_only_completion_and_check_failure() {
    for command in ["commands", "functions"] {
        let out = run_with_closed_stdout(&[command]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stderr).contains(command));
    }
    let dir = tmpdir("closed-stdout-check");
    let book = dir.join("good.xlsx");
    let bad = dir.join("missing.xlsx");
    ok(&["run", "--cmd", r#"cell.set={"cell":"A1","input":"1"}"#, "--out", book.to_str().unwrap()]);
    assert!(!bad.exists());
    let good = run_with_closed_stdout(&["check", book.to_str().unwrap()]);
    assert_eq!(good.status.code(), Some(0), "{}", String::from_utf8_lossy(&good.stderr));
    let out = run_with_closed_stdout(&["check", book.to_str().unwrap(), bad.to_str().unwrap(), "--max-report", "0"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("can't write to stdout"));
    let _ = std::fs::remove_dir_all(dir);
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
fn check_rejects_an_empty_scan() {
    let dir = tmpdir("check-empty");
    std::fs::write(dir.join("notes.txt"), "not a workbook").unwrap();
    std::fs::write(dir.join(".hidden.xlsx"), "ignored").unwrap();
    std::fs::write(dir.join("~$locked.xlsx"), "ignored").unwrap();
    let out = run(&["check", dir.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["files"][0]["error"].as_str().unwrap().contains("no workbook files"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn check_reports_a_depth_limited_scan_while_checking_readable_files() {
    let dir = tmpdir("check-depth");
    let book = dir.join("readable.xlsx");
    ok(&["run", "--out", book.to_str().unwrap(), "--quiet"]);
    let mut deep = dir.clone();
    for _ in 0..34 {
        deep.push("d");
    }
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("skipped.xlsx"), "not reached").unwrap();
    let out = run(&["check", dir.to_str().unwrap(), "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["totals"]["opened"], 1);
    assert!(report["files"].as_array().unwrap().iter().any(|r| r["error"].as_str().is_some_and(|e| e.contains("depth limit"))));
    let _ = std::fs::remove_dir_all(dir);
}

/// A script line whose parameters are under a key `run` does not read used to run the command
/// with no parameters at all, and the run still exited 0: three mistaken lines made Sheet2,
/// Sheet3 and Sheet4 instead of the named sheets, and nothing said so (#2256).
#[test]
fn a_script_line_with_an_unknown_key_stops_the_run() {
    let dir = tmpdir("script-keys");
    let script = dir.join("s.jsonl");
    let out = dir.join("t.xlsx");
    let write = |lines: &[&str]| std::fs::write(&script, lines.join("\n")).unwrap();

    write(&[r#"{"command":"file.new"}"#, r#"{"command":"home.insertSheet","args":{"name":"Report"}}"#]);
    let o = run(&["run", "--script", script.to_str().unwrap(), "--out", out.to_str().unwrap()]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "the run must stop: {err}");
    assert!(err.contains("s.jsonl:2"), "it names the line: {err}");
    assert!(err.contains("unknown key `args`"), "and the key: {err}");
    assert!(!out.exists(), "and writes no workbook: {err}");

    // The forms that are read still work, in one script: `params`, `id=json`, and a comment.
    write(&[
        r#"{"command":"file.new"}"#,
        r#"{"command":"home.insertSheet","params":{"name":"Report"}}"#,
        "# a comment",
        r#"home.insertSheet={"name":"Report2"}"#,
    ]);
    let o = run(&["run", "--script", script.to_str().unwrap(), "--out", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let info = ok(&["info", out.to_str().unwrap()]);
    assert!(info.contains("Report:") && info.contains("Report2:"), "the sheets are named: {info}");
    assert!(!info.contains("Sheet2"), "and no sheet was added with a default name: {info}");

    // The usage text says what a line looks like, which is how this was found.
    let usage = String::from_utf8_lossy(&run(&[]).stderr).into_owned() + &String::from_utf8_lossy(&run(&["--help"]).stdout);
    assert!(usage.contains(r#"{"command": "id", "params": {…}}"#), "{usage}");

    let _ = std::fs::remove_dir_all(&dir);
}
