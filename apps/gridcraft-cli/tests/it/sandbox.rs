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
        "denied: change the canary's permissions",
        "denied: truncate the canary",
        "denied: write next to the canary",
        "denied: write in the temporary directory",
        "denied: TCP connection",
        "denied: start a process",
        "denied: signal another process",
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
    // The second stage takes the process the first looked at: here, the parent of what `cli` starts.
    let mut launch = vec![
        "--sandbox-confined".to_string(),
        std::process::id().to_string(),
        "--exec".into(),
        echo.into(),
        "--exec".into(),
        env!("CARGO_BIN_EXE_gridcraft-cli").into(),
    ];
    for lib in ["/lib", "/lib64", "/usr/lib", "/usr/lib64"].into_iter().filter(|d| Path::new(d).exists()) {
        launch.extend(["--exec".to_string(), lib.into()]);
    }
    launch.extend(["--probe".to_string(), probe.clone(), "--".into(), echo.into(), "the program ran".into()]);
    let o = cli(&launch.iter().map(String::as_str).collect::<Vec<_>>());
    let out = text(&o);
    assert_eq!(o.status.code(), Some(97), "{out}");
    assert!(!out.contains("the program ran"), "{out}");
    assert!(out.contains("gridcraft-sandbox: refused"), "{out}");
    assert!(out.contains("read the canary"), "the check refused, not the arguments: {out}");
    // Without that process, the second stage has nothing to try a signal on.
    let without: Vec<&str> = std::iter::once("--sandbox-confined").chain(launch[2..].iter().map(String::as_str)).collect();
    let o = cli(&without);
    assert_eq!(o.status.code(), Some(97), "{}", text(&o));
    assert!(text(&o).contains("needs the process"), "{}", text(&o));
    // The normal path: the launcher confines itself first, and the program runs.
    if confines() {
        launch.splice(0..2, ["--sandbox-launch".to_string()]);
        // The launcher may be under a folder in the list, on either system.
        let mut by_folder = launch.clone();
        by_folder[4] = Path::new(env!("CARGO_BIN_EXE_gridcraft-cli")).parent().unwrap().to_string_lossy().into_owned();
        let o = cli(&by_folder.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(o.status.success(), "{}", text(&o));
        assert!(text(&o).contains("the program ran"), "{}", text(&o));
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

/// A Python found on this system: the interpreter to run confined, and its installation, the path it
/// needs to read. `None` when there is no `python3`.
fn system_python() -> Option<(String, String)> {
    // On macOS `/usr/bin/python3` is a stub that starts Xcode's Python, outside any read path given
    // here; an installed Python comes first.
    let candidates = if cfg!(target_os = "macos") {
        ["/opt/homebrew/bin/python3", "/usr/local/bin/python3"]
    } else {
        ["/usr/bin/python3", "/usr/local/bin/python3"]
    };
    let python = candidates.into_iter().find(|p| Path::new(p).exists())?;
    let prefix = Command::new(python).args(["-c", "import sys; print(sys.base_prefix)"]).output().unwrap();
    let prefix = String::from_utf8_lossy(&prefix.stdout).trim().to_string();
    // A framework build's `bin/python3` only starts `Python.app`'s interpreter as a new process,
    // which the sandbox refuses; the interpreter itself is run instead.
    let framework = Path::new(&prefix).join("Resources/Python.app/Contents/MacOS/Python");
    let real = if framework.exists() { framework } else { std::fs::canonicalize(python).unwrap() };
    Some((real.to_string_lossy().into_owned(), prefix))
}

/// Confined, a program can't signal another process (this test, signal 0 delivering nothing), but can
/// still signal itself and its own threads. On Linux, where the filter names them, it also can't
/// change another process's limits or priority, or type into a terminal: no call here reaches a real
/// terminal, since its standard input isn't one.
#[test]
fn a_confined_program_leaves_other_processes_alone() {
    if !confines() {
        return;
    }
    let Some((python, prefix)) = system_python() else {
        return;
    };
    let code = r#"
import os, signal, sys, threading
other = int(sys.argv[1])
def refused(call, *args):
    try:
        call(*args)
    except PermissionError:
        return True
    except OSError as e:
        print("other error", call, args, e)
    return False
assert refused(os.kill, other, 0), "a signal to another process"
os.kill(os.getpid(), 0)
seen = []
signal.signal(signal.SIGUSR1, lambda *_: seen.append(1))
signal.raise_signal(signal.SIGUSR1)
worker = threading.Thread(target=lambda: signal.pthread_kill(threading.main_thread().ident, signal.SIGUSR1))
worker.start()
worker.join()
assert len(seen) >= 1, "a signal to itself"
if sys.platform == "linux":
    import ctypes, errno, fcntl, resource, termios
    libc = ctypes.CDLL(None, use_errno=True)
    assert libc.shmget(0, 4096, 0o600) == -1 and ctypes.get_errno() == errno.EPERM, "System V shared memory"
    assert libc.msgget(0, 0o600) == -1 and ctypes.get_errno() == errno.EPERM, "a System V message queue"
    assert libc.semget(0, 1, 0o600) == -1 and ctypes.get_errno() == errno.EPERM, "a System V semaphore"
    if hasattr(libc, "mq_open"):
        assert libc.mq_open(b"/gridcraft-check", os.O_RDONLY) == -1 and ctypes.get_errno() == errno.EPERM, "a POSIX message queue"
    # macOS answers these for the processes the profile lets it signal, which are only itself.
    assert refused(os.kill, -1, 0), "a signal to every process"
    assert refused(os.kill, 0, 0), "a signal to the process group"
    assert refused(resource.prlimit, other, resource.RLIMIT_NOFILE), "another process's limits"
    assert refused(os.setpriority, os.PRIO_PROCESS, other, 0), "another process's priority"
    assert refused(os.setpriority, os.PRIO_USER, 0, 0), "every process's priority"
    assert refused(os.sched_setaffinity, other, {0}), "another process's processors"
    assert refused(os.pidfd_open, other), "a handle on another process"
    resource.prlimit(0, resource.RLIMIT_NOFILE)
    resource.prlimit(os.getpid(), resource.RLIMIT_NOFILE)
    os.setpriority(os.PRIO_PROCESS, 0, os.getpriority(os.PRIO_PROCESS, 0))
    assert refused(fcntl.ioctl, 0, termios.TIOCSTI, b"x"), "typing into a terminal"
    assert refused(fcntl.ioctl, 0, termios.TIOCSTI | (1 << 32), b"x"), "the same with high bits set"
    assert refused(fcntl.ioctl, 0, 0x541C, b"x"), "TIOCLINUX"
print("left alone")
"#;
    let me = std::process::id().to_string();
    let o = cli(&["sandbox-check", "--read", &prefix, "--", &python, "-I", "-c", code, &me]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("left alone"), "{}", text(&o));
}

/// Confined, a program changes no file, whether the policy lets it read that file or not, by path or
/// by a descriptor: not its contents, permissions, owner, extended attributes, times or flags.
/// Skipped when there is no `python3`.
#[test]
fn a_confined_program_changes_no_file() {
    if !confines() {
        return;
    }
    let Some((python, prefix)) = system_python() else {
        return;
    };
    let code = r#"
import errno, fcntl, os, struct, sys
readable, outside = sys.argv[1:3]
def refused(what, call, *args):
    try:
        call(*args)
    except PermissionError:
        return
    except OSError as e:
        sys.exit(f"{what}: another error, not a refusal: {e}")
    sys.exit(f"{what}: it worked")
fd = os.open(readable, os.O_RDONLY)
for target in (readable, outside, fd):
    refused(f"permissions of {target}", os.chmod, target, 0)
    refused(f"owner of {target}", os.chown, target, os.getuid(), os.getgid())
    refused(f"times of {target}", os.utime, target, (0, 0))
    if sys.platform == "linux":
        refused(f"setting an attribute of {target}", os.setxattr, target, "user.gridcraft", b"1")
        refused(f"removing an attribute of {target}", os.removexattr, target, "user.gridcraft")
for path in (readable, outside):
    refused(f"truncate {path}", os.truncate, path, 0)
    refused(f"open {path} with O_TRUNC", os.open, path, os.O_RDONLY | os.O_TRUNC)
    refused(f"open {path} to write", os.open, path, os.O_WRONLY)
# A descriptor opened to read can't be truncated either way; the file's contents are checked after.
try:
    os.ftruncate(fd, 0)
except OSError:
    pass
if sys.platform == "linux":
    import ctypes
    libc = ctypes.CDLL(None, use_errno=True)
    # Newer calls, which the kernel running this may not have: the filter answers before it would.
    for name, number in (("fchmodat2", 452), ("setxattrat", 463), ("removexattrat", 466), ("file_setattr", 469)):
        ctypes.set_errno(0)
        result = libc.syscall(number, -100, readable.encode(), 0, 0)
        assert result == -1 and ctypes.get_errno() == errno.EPERM, f"{name}: {result}, {ctypes.get_errno()}"
    # On a descriptor opened only to read, these change the file's flags and attributes.
    refused("FS_IOC_SETFLAGS", fcntl.ioctl, fd, 0x40086602, struct.pack("l", 0))
    refused("FS_IOC_FSSETXATTR", fcntl.ioctl, fd, 0x401C5820, bytes(28))
    refused("FS_IOC_SET_ENCRYPTION_POLICY", fcntl.ioctl, fd, 0x800C6613, bytes(12))
    # What a program still needs: its own descriptors' flags, and a file of its own in memory.
    os.set_inheritable(fd, True)
    os.set_inheritable(fd, False)
    os.set_blocking(fd, False)
    own = os.memfd_create("gridcraft-check")
    os.ftruncate(own, 16)
    assert os.fstat(own).st_size == 16
print("unchanged")
"#;
    let inside = scratch("unchanged-inside");
    let elsewhere = scratch("unchanged-outside");
    let (readable, outside) = (inside.join("allowed.txt"), elsewhere.join("allowed.txt"));
    let state = |file: &Path| {
        let m = std::fs::metadata(file).unwrap();
        (std::fs::read(file).unwrap(), m.permissions(), m.modified().unwrap())
    };
    let before = (state(&readable), state(&outside));
    let d = inside.to_string_lossy().into_owned();
    let (r, x) = (readable.to_string_lossy().into_owned(), outside.to_string_lossy().into_owned());
    let o = cli(&["sandbox-check", "--read", &prefix, "--read", &d, "--", &python, "-I", "-c", code, &r, &x]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("unchanged"), "{}", text(&o));
    assert_eq!((state(&readable), state(&outside)), before);
    std::fs::remove_dir_all(inside).unwrap();
    std::fs::remove_dir_all(elsewhere).unwrap();
}

/// A Python found on this system starts and imports its standard library while confined, with
/// read access to its installation only. Skipped when there is no `python3`.
#[test]
fn python_starts_confined() {
    if !confines() {
        return;
    }
    let Some((real, prefix)) = system_python() else {
        return;
    };
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
