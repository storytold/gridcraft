//! Workers started at the same moment on many threads, each with a memory watcher: a process
//! started on another thread can inherit a pipe in the moment before it is close-on-exec, and the
//! launcher closes it instead of being stopped by it.

use std::collections::BTreeMap;

use gridcraft_python::describe::describe;
use gridcraft_python::interp::{self, Detection, SearchEnv};
use gridcraft_python::process::ProcessTransport;
use gridcraft_python::{Client, Limits, Outcome};

#[test]
fn workers_started_together_are_not_refused() {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return;
    }
    // Every worker starts through the launcher, which a machine that doesn't confine refuses. On CI
    // the machine must confine, so a failure there is never skipped.
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_gridcraft-cli")).arg("sandbox-check").output().unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&check.stdout), String::from_utf8_lossy(&check.stderr));
    if (said.contains("sandbox: unsupported") || said.contains("Landlock is not enforced")) && std::env::var_os("CI").is_none() {
        println!("skipped: this machine doesn't confine programs:\n{said}");
        return;
    }
    let Detection::Found { path, .. } = interp::detect(&SearchEnv::from_process(), None, interp::PROBE_TIMEOUT) else {
        assert!(std::env::var_os("CI").is_none(), "CI has no Python 3.11 or newer");
        println!("skipped: no Python 3.11 or newer found");
        return;
    };
    let interpreter = describe(&path, interp::PROBE_TIMEOUT).unwrap();
    let launcher = std::path::PathBuf::from(env!("CARGO_BIN_EXE_gridcraft-cli"));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (interpreter, launcher) = (interpreter.clone(), launcher.clone());
            std::thread::spawn(move || {
                for _ in 0..5 {
                    let transport = ProcessTransport::confined(&interpreter, Some(launcher.clone()));
                    let mut client = Client::new(Box::new(transport), Limits::default());
                    let outcome = client.run("1 + 1", BTreeMap::new()).unwrap();
                    assert!(matches!(outcome, Outcome::Ok { .. }), "{outcome:?}");
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
}
