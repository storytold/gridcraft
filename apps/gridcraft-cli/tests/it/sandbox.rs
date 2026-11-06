//! The confined launcher, through the real binary: what a confined program can't do, what it
//! still can, and that a launch whose check fails runs nothing. Linux and macOS confine; other
//! systems report that they can't.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Whether this machine confines programs: a supported system whose self-test passes. On a Linux
/// or macOS machine that reports no sandbox (another architecture, no `sandbox-exec`) or whose
/// kernel doesn't enforce Landlock, the confinement tests have nothing to check and pass with a
/// note. On CI (`CI` set) the machine must confine, so a failure there is never skipped.
fn confines() -> bool {
    static CONFINES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CONFINES.get_or_init(|| {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return false;
        }
        let o = cli(&["sandbox-check"]);
        let out = text(&o);
        let missing = out.contains("sandbox: unsupported") || out.contains("Landlock is not enforced");
        if missing && std::env::var_os("CI").is_none() {
            eprintln!("this machine doesn't confine programs; skipping:\n{out}");
            return false;
        }
        true
    })
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gridcraft-cli")).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// A directory of the test's own in the temporary directory, with one file.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("gridcraft-sandbox-it-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("allowed.txt"), "allowed text\n").unwrap();
    dir
}

/// `cat` as this system names it.
fn cat() -> &'static str {
    if Path::new("/bin/cat").exists() { "/bin/cat" } else { "/usr/bin/cat" }
}

#[test]
fn the_self_test_finds_every_denial() {
    // A home directory of the test's own, so the home-directory attempt always runs.
    let home = scratch("home");
    let o = Command::new(env!("CARGO_BIN_EXE_gridcraft-cli")).arg("sandbox-check").env("HOME", &home).output().unwrap();
    let out = text(&o);
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        assert_eq!(o.status.code(), Some(2), "{out}");
        assert!(out.contains("sandbox: unsupported"), "{out}");
        return;
    }
    if !confines() {
        return;
    }
    assert!(o.status.success(), "{out}");
    for line in [
        "denied: read the canary",
        "denied: write next to the canary",
        "denied: write in the temporary directory",
        "denied: TCP connection",
        "denied: start a process",
        "denied: UDP socket",
        "denied: list the home directory",
        "works: start a thread",
        "works: read an allowed file",
    ] {
        assert!(out.contains(line), "missing `{line}` in:\n{out}");
    }
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn a_confined_program_reads_only_what_it_is_given() {
    if !confines() {
        return;
    }
    let dir = scratch("read");
    let outside = scratch("outside");
    let d = dir.to_string_lossy().into_owned();
    let allowed = dir.join("allowed.txt").to_string_lossy().into_owned();
    let o = cli(&["sandbox-check", "--read", &d, "--", cat(), &allowed]);
    assert!(o.status.success(), "{}", text(&o));
    // The program's output is passed through untouched; this command's own lines go to stderr.
    assert_eq!(String::from_utf8_lossy(&o.stdout), "allowed text\n", "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("result: the program ran confined"), "{}", text(&o));
    // The same program, a file outside the policy: the read is refused and nothing is printed.
    let other = outside.join("allowed.txt").to_string_lossy().into_owned();
    let o = cli(&["sandbox-check", "--read", &d, "--", cat(), &other]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(!String::from_utf8_lossy(&o.stdout).contains("allowed text"), "{}", text(&o));
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn a_launch_whose_check_fails_runs_nothing() {
    // A run that says it is already confined and isn't finds the canary readable and must refuse.
    // On macOS that is the second stage started without `sandbox-exec`; on Linux the launcher never
    // uses that flag, and the same refusal must hold.
    let canary = scratch("canary");
    let echo = if Path::new("/bin/echo").exists() { "/bin/echo" } else { "/usr/bin/echo" };
    let probe = canary.join("allowed.txt").to_string_lossy().into_owned();
    // A dynamically linked `echo` needs the system's shared libraries and its loader.
    let mut launch =
        vec!["--sandbox-confined".to_string(), "--exec".into(), echo.into(), "--exec".into(), env!("CARGO_BIN_EXE_gridcraft-cli").into()];
    for lib in ["/lib", "/lib64", "/usr/lib", "/usr/lib64"].into_iter().filter(|d| Path::new(d).exists()) {
        launch.extend(["--exec".to_string(), lib.into()]);
    }
    launch.extend(["--probe".to_string(), probe.clone(), "--".into(), echo.into(), "the program ran".into()]);
    let o = cli(&launch.iter().map(String::as_str).collect::<Vec<_>>());
    let out = text(&o);
    assert_eq!(o.status.code(), Some(97), "{out}");
    assert!(!out.contains("the program ran"), "{out}");
    assert!(out.contains("gridcraft-sandbox: refused"), "{out}");
    // The normal path: the launcher confines itself first, and the program runs.
    if confines() {
        launch[0] = "--sandbox-launch".into();
        let o = cli(&launch.iter().map(String::as_str).collect::<Vec<_>>());
        let out = text(&o);
        assert!(o.status.success() && out.contains("the program ran"), "{out}");
    }
    // A canary inside an allowed path is refused before anything runs, everywhere.
    let dir = canary.to_string_lossy().into_owned();
    let o = cli(&["--sandbox-launch", "--read", &dir, "--exec", echo, "--probe", &probe, "--", echo, "the program ran"]);
    assert_eq!(o.status.code(), Some(97), "{}", text(&o));
    assert!(!text(&o).contains("the program ran"));
    std::fs::remove_dir_all(canary).unwrap();
}

/// A Python found on this system starts and imports its standard library while confined, with
/// read access to its installation only. Skipped when there is no `python3`.
#[test]
fn python_starts_confined() {
    if !confines() {
        return;
    }
    // On macOS `/usr/bin/python3` is a stub that starts Xcode's Python, outside any read path given
    // here; an installed Python comes first.
    let candidates = if cfg!(target_os = "macos") {
        ["/opt/homebrew/bin/python3", "/usr/local/bin/python3"]
    } else {
        ["/usr/bin/python3", "/usr/local/bin/python3"]
    };
    let Some(python) = candidates.into_iter().find(|p| Path::new(p).exists()) else {
        return;
    };
    let prefix = Command::new(python).args(["-c", "import sys; print(sys.base_prefix)"]).output().unwrap();
    let prefix = String::from_utf8_lossy(&prefix.stdout).trim().to_string();
    // A framework build's `bin/python3` only starts `Python.app`'s interpreter as a new process,
    // which the sandbox refuses; the interpreter itself is run instead.
    let framework = Path::new(&prefix).join("Resources/Python.app/Contents/MacOS/Python");
    let real = if framework.exists() { framework } else { std::fs::canonicalize(python).unwrap() };
    let real = real.to_string_lossy().into_owned();
    let o =
        cli(&["sandbox-check", "--read", &prefix, "--", &real, "-I", "-c", "import json, math, datetime, decimal; print(json.dumps([math.pi > 3]))"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("[true]"), "{}", text(&o));
    // Confined Python can't open the network or a file outside its installation.
    let o = cli(&["sandbox-check", "--read", &prefix, "--", &real, "-I", "-c", "import socket; socket.create_connection(('127.0.0.1', 9), 2)"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("PermissionError") || text(&o).contains("Operation not permitted"), "{}", text(&o));
}

#[test]
fn a_confined_program_inherits_no_secrets() {
    if !confines() {
        return;
    }
    let env = if Path::new("/usr/bin/env").exists() { "/usr/bin/env" } else { "/bin/env" };
    let o = Command::new(env!("CARGO_BIN_EXE_gridcraft-cli"))
        .args(["sandbox-check", "--", env])
        .env("GRIDCRAFT_CONTROL_TOKEN", "secret-token")
        .env("AWS_SECRET_ACCESS_KEY", "secret-key")
        .env("LANG", "en_US.UTF-8")
        .output()
        .unwrap();
    let out = text(&o);
    assert!(o.status.success(), "{out}");
    assert!(!out.contains("secret-"), "the environment reached the program:\n{out}");
    assert!(out.contains("LANG=en_US.UTF-8"), "the locale is kept:\n{out}");
    assert!(out.contains("PATH=/usr/bin:/bin"), "{out}");
}

/// A descriptor the parent leaves open (here fd 9, opened by a shell on a file outside any policy)
/// would survive `exec` with access the sandbox doesn't revoke, so the launcher closes it: the
/// program runs, and can't read through it. (A desktop app's system libraries leave such files open,
/// so refusing to run would block Python there.)
#[test]
fn an_inherited_descriptor_never_reaches_the_program() {
    if !confines() {
        return;
    }
    let outside = scratch("leak");
    let leaked = outside.join("allowed.txt").to_string_lossy().into_owned();
    let cli = env!("CARGO_BIN_EXE_gridcraft-cli");
    // The shell opens fd 9 (not close-on-exec) and then becomes the command, which inherits it. The
    // confined shell then tries to read through it, with a built-in: starting `cat` is refused. Not
    // fd 3: on macOS the system's libraries hold a socket of their own there under the sandbox.
    let bash = if cfg!(target_os = "macos") { "--exec /bin/bash" } else { "" };
    let program = "read line <&9 && echo \"leaked: $line\"; echo finished";
    let script = format!("exec 9<'{leaked}'; exec '{cli}' sandbox-check {bash} -- /bin/sh -c '{program}'");
    let o = Command::new("/bin/sh").args(["-c", &script]).output().unwrap();
    let out = text(&o);
    assert!(out.contains("finished"), "the program ran:\n{out}");
    assert!(!out.contains("leaked:"), "the program read through the descriptor:\n{out}");
    std::fs::remove_dir_all(outside).unwrap();
}
