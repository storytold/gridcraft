//! `gridcraft-cli sandbox-check [--read PATH]... [--exec PATH]... [-- PROGRAM ARGS...]`: whether
//! this system confines programs, for a person or a reviewer checking a machine.
//!
//! Without a program it starts a confined copy of itself that tries what the sandbox must deny
//! (reading outside the policy, writing, the network, starting a process) and what it must still
//! allow (a thread, reading an allowed file), and prints one line per attempt. With a program it
//! runs that program confined, with the given read and executable paths and the system's shared
//! libraries, and passes its output and exit status through; its own lines then go to standard
//! error. Every path is absolute. Exit status: 0 when every attempt is
//! as required (or the program succeeded), 1 otherwise (a refused launch included), 2 when GridCraft
//! has no sandbox for this kind of system.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use gridcraft_sandbox::{Canary, Policy, REFUSED_EXIT_CODE, REPORT_FLAG, Support};

pub fn main(args: &[String]) -> ExitCode {
    match run(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("gridcraft-cli: sandbox-check: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut policy = Policy::new().with_system_libraries();
    let mut it = args.iter();
    let mut program: Option<(PathBuf, Vec<OsString>)> = None;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--read" => policy = policy.read(it.next().ok_or("--read needs a path")?),
            "--exec" => policy = policy.exec(it.next().ok_or("--exec needs a path")?),
            "--" => {
                let p = it.next().ok_or("a program follows `--`")?;
                program = Some((PathBuf::from(p), it.by_ref().map(OsString::from).collect()));
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    // With a program, its standard output is passed through untouched, so this command's own
    // lines go to standard error; the self-test's report is the output, on standard output.
    let report = program.is_none();
    let say = |line: String| {
        if report {
            outln!("{line}");
        } else {
            eprintln!("{line}");
        }
    };
    match gridcraft_sandbox::support() {
        Support::Available(how) => say(format!("sandbox: available ({how})")),
        Support::Unsupported(why) => {
            say(format!("sandbox: unsupported ({why})"));
            return Ok(ExitCode::from(2));
        }
    }
    let canary = Canary::create().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let (program, program_args) = match program {
        Some((p, a)) => (p, a),
        // A confined copy of this program, reporting each attempt; it reads its own executable as
        // the allowed file.
        None => {
            let home = std::env::var_os("HOME").unwrap_or_default();
            (exe.clone(), vec![REPORT_FLAG.into(), canary.file().into(), exe.clone().into(), home])
        }
    };
    let policy = policy.exec(program.clone());
    let mut cmd = gridcraft_sandbox::launch_command(&policy, &canary, &program, &program_args).map_err(|e| e.to_string())?;
    let status = cmd.status().map_err(|e| format!("couldn't start the launcher: {e}"))?;
    drop(canary);
    match status.code() {
        Some(0) => {
            say(if report { "result: every attempt is as the sandbox requires" } else { "result: the program ran confined and succeeded" }.into());
            Ok(ExitCode::SUCCESS)
        }
        Some(REFUSED_EXIT_CODE) => {
            // The launcher's refusal and a program's own exit share the code; the launcher's reason,
            // when it refused, is the `gridcraft-sandbox: refused:` line above.
            say(format!(
                "result: exit code {REFUSED_EXIT_CODE}: the launcher refused to run the program (its reason is above), or the program itself exited with {REFUSED_EXIT_CODE}"
            ));
            Ok(ExitCode::FAILURE)
        }
        other => {
            say(format!(
                "result: {} ({})",
                if report { "some attempts are not as the sandbox requires" } else { "the program failed" },
                other.map_or("no exit code".into(), |c| format!("exit code {c}"))
            ));
            Ok(ExitCode::FAILURE)
        }
    }
}
