//! What the command line does with its arguments: RFC-0018's contract, through the binary. A usage error exits 2 on
//! stderr; help and the version go to stdout with exit 0; an unknown or repeated flag after a subcommand is refused.

use std::process::Command;

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

fn run(args: &[&str]) -> Ran {
    let o = Command::new(env!("CARGO_BIN_EXE_gridcraft-cli")).args(args).output().unwrap();
    Ran { code: o.status.code(), out: String::from_utf8_lossy(&o.stdout).into_owned(), err: String::from_utf8_lossy(&o.stderr).into_owned() }
}

const SUBCOMMANDS: [&str; 11] = ["info", "convert", "eval", "cat", "check", "run", "commands", "functions", "mcp", "send", "sandbox-check"];

/// A usage error: exit 2, nothing on stdout, `error:` and the offending argument on stderr.
fn usage_error(args: &[&str], names: &str) {
    let r = run(args);
    assert_eq!(r.code, Some(2), "{args:?}: {:?} / {:?}", r.out, r.err);
    assert_eq!(r.out, "", "{args:?}");
    assert!(r.err.starts_with("error:") && r.err.contains(names), "{args:?}: {:?}", r.err);
}

#[test]
fn every_subcommand_answers_help_and_version_on_stdout() {
    for sub in SUBCOMMANDS {
        for flag in ["--help", "-h"] {
            let r = run(&[sub, flag]);
            assert_eq!((r.code, r.err.as_str()), (Some(0), ""), "{sub} {flag}");
            assert!(r.out.contains(&format!("Usage: gridcraft-cli {sub}")), "{sub} {flag}: {:?}", r.out);
            assert!(r.out.contains("Exit status:"), "{sub} {flag}: {:?}", r.out);
        }
        let r = run(&[sub, "--version"]);
        assert!(r.out.starts_with(&format!("gridcraft-cli-{sub} ")), "{sub}: {:?}", r.out);
    }
    let r = run(&["help", "run"]);
    assert_eq!(r.code, Some(0));
}

#[test]
fn an_unknown_option_is_refused_by_name_in_every_subcommand() {
    for sub in ["info", "convert", "eval", "cat", "check", "run", "commands", "functions", "mcp", "send", "sandbox-check"] {
        usage_error(&[sub, "--no-such-option"], "--no-such-option");
    }
    // An option of another subcommand is not this one's.
    usage_error(&["commands", "--sample", "x"], "--sample");
    usage_error(&["info", "a.xlsx", "--csv"], "--csv");
    usage_error(&["cat", "a.xlsx", "--json"], "--json");
}

#[test]
fn a_missing_or_extra_argument_is_a_usage_error() {
    usage_error(&["info"], "FILE");
    usage_error(&["info", "a", "b"], "b");
    usage_error(&["convert", "a.xlsx"], "OUTPUT");
    usage_error(&["eval"], "FORMULA");
    usage_error(&["check"], "FILE_OR_DIR");
    usage_error(&["send", "7979"], "METHOD");
    usage_error(&["run", "--in"], "--in");
    usage_error(&["run", "--cmd"], "--cmd");
    usage_error(&["commands", "a", "b"], "b");
}

#[test]
fn a_repeated_single_value_flag_is_refused() {
    usage_error(&["convert", "a.xlsx", "b.csv", "--sheet", "1", "--sheet", "2"], "--sheet");
    usage_error(&["run", "--in", "a.xlsx", "--in", "b.xlsx"], "--in");
    usage_error(&["run", "--out", "a.xlsx", "--out", "b.xlsx"], "--out");
    usage_error(&["check", "x.xlsx", "--tolerance", "1", "--tolerance", "2"], "--tolerance");
}

#[test]
fn conflicting_flags_are_refused_before_anything_starts() {
    usage_error(&["run", "--in", "a.xlsx", "--sample", "sales"], "--sample");
    usage_error(&["mcp", "--connect", "7979", "--in", "a.xlsx"], "--connect");
    usage_error(&["mcp", "--connect", "7979", "--sample", "sales"], "--connect");
    usage_error(&["mcp", "--in", "a.xlsx", "--sample", "sales"], "--sample");
    usage_error(&["commands", "x", "--search", "y"], "--search");
    // `--print` carries the options that shape it.
    usage_error(&["run", "--csv"], "--print");
    usage_error(&["run", "--sheet", "1"], "--print");
}

#[test]
fn a_value_that_cannot_mean_anything_is_a_usage_error() {
    usage_error(&["check", "x.xlsx", "--tolerance", "-1"], "--tolerance");
    usage_error(&["check", "x.xlsx", "--tolerance", "nan"], "--tolerance");
    usage_error(&["check", "x.xlsx", "--max-report", "many"], "--max-report");
    usage_error(&["run", "--cmd", "x={bad"], "--cmd");
    usage_error(&["send", "7979", "m", "{bad"], "JSON");
}

/// `eval` takes its formula as the first thing, even one that starts with a dash; `--` ends the options.
#[test]
fn a_formula_may_start_with_a_dash() {
    let r = run(&["eval", "-1+3"]);
    assert_eq!((r.code, r.out.trim()), (Some(0), "2"), "{:?}", r.err);
    let r = run(&["eval", "--json", "-1+3"]);
    assert_eq!((r.code, r.out.trim()), (Some(0), "2.0"), "{:?}", r.err);
    // A mistyped option is not a formula; `=` makes it one.
    usage_error(&["eval", "--no-such-option"], "--no-such-option");
    assert_eq!(run(&["eval", "=--1"]).code, Some(0));
}

/// A word after the options (or after `--`) is a filter; `--search` is the same filter as an option.
#[test]
fn commands_and_functions_filter_by_a_word_or_search() {
    let a = run(&["commands", "--json", "home.bold"]);
    let b = run(&["commands", "--json", "--search", "home.bold"]);
    assert_eq!((a.code, b.code), (Some(0), Some(0)));
    assert_eq!(a.out, b.out);
    assert!(a.out.contains("home.bold"));
    let r = run(&["functions", "--json", "--category", "Math and trigonometry", "SUM"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
}

/// A failed command is exit 1 with `error:`; connection failures are command failures, not usage errors.
#[test]
fn a_failed_command_exits_1_with_an_error_line() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    drop(listener);
    for args in [&["send", &port, "x"][..], &["mcp", "--connect", &port], &["info", "/no/such/file.xlsx"], &["convert", "/no/such.xlsx", "o.csv"]] {
        let r = run(args);
        assert_eq!(r.code, Some(1), "{args:?}: {:?}", r.err);
        assert_eq!(r.out, "");
        assert!(r.err.starts_with("error:"), "{args:?}: {:?}", r.err);
    }
}

/// The nightly profile protocol belongs to the desktop app: exit 2 before any subcommand parses it.
#[test]
fn the_launcher_profile_arguments_are_refused() {
    for args in [&["--launch-profile", "nightly-v1", "info", "a.xlsx"][..], &["info", "--profile-root=/tmp/x", "a.xlsx"]] {
        let r = run(args);
        assert_eq!((r.code, r.out.as_str()), (Some(2), ""), "{args:?}");
        assert!(r.err.contains("desktop app"), "{:?}", r.err);
    }
}
