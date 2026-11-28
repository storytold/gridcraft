//! `gridcraft_cli::run` called in-process: the same program the binary is, with its streams captured, so the whole of
//! RFC-0018's surface is covered without a subprocess per case.

fn run(args: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = gridcraft_cli::run(args.iter().copied(), &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

struct Dir(std::path::PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("gc-inproc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
    fn file(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_and_version_exit_0_on_stdout_and_usage_errors_exit_2_on_stderr() {
    for args in [&["--help"][..], &["-h"], &["help"], &["--version"], &["-V"], &["run", "-V"], &["check", "--help"]] {
        let (code, out, err) = run(args);
        assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
        assert!(!out.is_empty(), "{args:?}");
    }
    for args in [&[][..], &["bogus"], &["info"], &["run", "--nope"], &["info", "--bogus"]] {
        let (code, out, err) = run(args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
        assert!(err.contains("Usage: gridcraft-cli"), "{args:?}: {err}");
    }
}

#[test]
fn eval_info_cat_and_commands_write_results_to_stdout_and_progress_to_stderr() {
    let dir = Dir::new("flow");
    let book = dir.file("b.xlsx");
    let (code, out, err) = run(&["run", "--cmd", r#"cell.set={"cell":"A1","input":"=1+1"}"#, "--out", &book]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("cell.set") && err.contains("wrote"), "{out} / {err}");

    let (code, out, _) = run(&["eval", "=SUM(1,2,3)"]);
    assert_eq!((code, out.trim()), (0, "6"));
    let (code, out, err) = run(&["info", &book]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Sheet1"), "{out}");
    let (code, out, _) = run(&["info", "--json", &book]);
    assert_eq!(code, 0);
    assert!(serde_json::from_str::<serde_json::Value>(&out).is_ok(), "{out}");
    let (code, out, _) = run(&["cat", &book, "--csv"]);
    assert_eq!((code, out.trim()), (0, "2"));
    let (code, out, _) = run(&["cat", &book, "--formulas"]);
    assert_eq!((code, out.trim()), (0, "=1+1"));

    let csv = dir.file("b.csv");
    let (code, _, err) = run(&["convert", &book, &csv]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(std::fs::read_to_string(&csv).unwrap().trim(), "2");
    let (code, _, err) = run(&["convert", &book, &dir.file("b.nope")]);
    assert_eq!(code, 1);
    assert!(err.starts_with("error:") && err.contains("unknown output format"), "{err}");
}

#[test]
fn run_keeps_cmd_and_script_in_the_order_given() {
    let dir = Dir::new("order");
    let script = dir.file("s.jsonl");
    std::fs::write(&script, "cell.set={\"cell\":\"A1\",\"input\":\"2\"}\n").unwrap();
    // The script writes A1 = 2 after the first --cmd wrote 1 and before the last wrote 3: the last one wins.
    let (code, out, err) = run(&[
        "run",
        "--quiet",
        "--cmd",
        r#"cell.set={"cell":"A1","input":"1"}"#,
        "--script",
        &script,
        "--cmd",
        r#"cell.set={"cell":"B1","input":"=A1*10"}"#,
        "--print",
        "A1:B1",
        "--csv",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), "2,20", "{out}");

    let (code, _, err) = run(&["run", "--script", &dir.file("missing.jsonl")]);
    assert_eq!(code, 1);
    assert!(err.starts_with("error:"), "{err}");
    let (code, _, err) = run(&["run", "--cmd", "no.such.command"]);
    assert_eq!(code, 1);
    assert!(err.contains("step 1 (--cmd, no.such.command)"), "{err}");
}

#[test]
fn check_reports_and_exits_with_the_verdict() {
    let dir = Dir::new("check");
    let book = dir.file("b.xlsx");
    assert_eq!(run(&["run", "--quiet", "--cmd", r#"cell.set={"cell":"A1","input":"=1+1"}"#, "--out", &book]).0, 0);
    let (code, out, _) = run(&["check", &book]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("files: 1"), "{out}");
    let (code, out, _) = run(&["check", "--json", &book, "--tolerance", "1e-6", "--max-report", "3"]);
    assert_eq!(code, 0);
    assert!(serde_json::from_str::<serde_json::Value>(&out).is_ok(), "{out}");
    let (code, _, _) = run(&["check", &dir.file("missing.xlsx")]);
    assert_eq!(code, 1);
}

#[test]
fn sandbox_check_without_a_program_reports_on_stdout() {
    let (code, out, _) = run(&["sandbox-check"]);
    assert!(out.starts_with("sandbox:"), "{out}");
    assert!(code == 0 || code == 1 || code == 2, "{code}");
}

/// A closed stdout (`gridcraft-cli commands | head`) ends the program quietly; any other write error is reported.
#[test]
fn a_closed_pipe_ends_quietly_and_another_write_error_is_an_error() {
    struct Failing(std::io::ErrorKind);
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut err = Vec::new();
    assert_eq!(gridcraft_cli::run(["commands"], &mut Failing(std::io::ErrorKind::BrokenPipe), &mut err), 0);
    assert!(err.is_empty());
    assert_eq!(gridcraft_cli::run(["commands"], &mut Failing(std::io::ErrorKind::StorageFull), &mut err), 1);
    assert!(String::from_utf8(err).unwrap().starts_with("error:"));
}
