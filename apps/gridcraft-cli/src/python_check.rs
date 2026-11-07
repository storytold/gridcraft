//! `gridcraft-cli python-check [--python PATH] [--code CODE]...`: which Python cells would use on
//! this machine, and what it can do, for a person or a reviewer checking a machine.
//!
//! It prints the interpreter found (uv's first), what runs and what the sandbox lets it read, and
//! the packages it can import, listed from inside the sandbox. Each `--code` runs through the same
//! worker and sandbox as a Python cell, in one shared set of variables, and prints its result. The
//! code comes from this command line, never from a workbook: the CLI doesn't run Python cells.
//! Exit status: 0 when Python started and all the code ran, 1 otherwise, 2 when GridCraft has no
//! sandbox for this kind of system.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use gridcraft_python::describe::{self, Interpreter};
use gridcraft_python::diagnostics;
use gridcraft_python::interp::{self, Detection, SearchEnv};
use gridcraft_python::process::ProcessTransport;
use gridcraft_python::{Client, Limits, Outcome, Output};
use gridcraft_sandbox::Support;

pub fn main(args: &[String]) -> ExitCode {
    match run(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("gridcraft-cli: python-check: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut python: Option<PathBuf> = None;
    let mut code: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--python" => python = Some(PathBuf::from(it.next().ok_or("--python needs a path")?)),
            "--code" => code.push(it.next().ok_or("--code needs code")?.clone()),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    match gridcraft_sandbox::support() {
        Support::Available(how) => outln!("sandbox: available ({how})"),
        Support::Unsupported(why) => {
            outln!("sandbox: unsupported ({why})");
            return Ok(ExitCode::from(2));
        }
    }
    let env = SearchEnv::from_process();
    outln!("uv: {}", interp::find_uv(&env).map_or("not found".to_string(), |p| p.display().to_string()));
    let interpreter = match interp::detect(&env, python.as_deref(), interp::PROBE_TIMEOUT) {
        Detection::Found { path, .. } => {
            describe::describe(&path, interp::PROBE_TIMEOUT).map_err(|e| format!("couldn't ask {} what it is: {e:?}", path.display()))?
        }
        Detection::NoneFound => {
            outln!("python: none found (Python {} or newer is needed)", interp::MIN_VERSION);
            setup_hint();
            return Ok(ExitCode::FAILURE);
        }
        Detection::TooOld { path, version } => {
            outln!("python: {} is {version}; Python {} or newer is needed", path.display(), interp::MIN_VERSION);
            setup_hint();
            return Ok(ExitCode::FAILURE);
        }
        Detection::Stub { path } => {
            outln!("python: {} is Apple's stub, which GridCraft doesn't use", path.display());
            setup_hint();
            return Ok(ExitCode::FAILURE);
        }
    };
    report(&interpreter);
    let mut client =
        Client::new(Box::new(ProcessTransport::confined(&interpreter, None)), Limits { timeout: Duration::from_secs(60), ..Limits::default() });
    let mut ok = list_packages(&mut client);
    for c in &code {
        ok &= run_code(&mut client, c);
    }
    Ok(if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn setup_hint() {
    outln!("to set up Python with uv:");
    for c in describe::uv_setup_commands(&describe::STARTER_PACKAGES) {
        outln!("  {c}");
    }
}

fn report(i: &Interpreter) {
    outln!("python: {} ({}, {})", i.path.display(), i.version, i.kind.label());
    outln!("runs as: {}", i.run_as.display());
    for d in &i.site_dirs {
        outln!("package folder: {}", d.display());
    }
    for d in &i.read_paths {
        outln!("can read: {}", d.display());
    }
}

fn list_packages(client: &mut Client) -> bool {
    match client.run(interp::LIST_PACKAGES, BTreeMap::new()) {
        Ok(Outcome::Ok { output: Output::Value(gridcraft_core::Value::Array(rows)), .. }) => {
            let names: Vec<String> = (0..rows.rows)
                .map(|r| {
                    let cell = |c| match rows.get(r, c) {
                        Some(gridcraft_core::Value::Text(t)) => t.to_string(),
                        _ => String::new(),
                    };
                    format!("{} {}", cell(0), cell(1))
                })
                .collect();
            outln!("packages ({}): {}", names.len(), names.join(", "));
            true
        }
        Ok(Outcome::Ok { .. }) => {
            outln!("packages: none");
            true
        }
        Ok(other) => {
            outln!("python: didn't start: {}", describe_outcome(&other));
            false
        }
        Err(e) => {
            outln!("python: couldn't ask: {e:?}");
            false
        }
    }
}

fn run_code(client: &mut Client, code: &str) -> bool {
    outln!("code: {code}");
    match client.run(code, BTreeMap::new()) {
        Ok(Outcome::Ok { output, stdout }) => {
            if !stdout.is_empty() {
                outln!("printed: {}", stdout.trim_end());
            }
            match output {
                Output::Value(v) => outln!("result: {v:?}"),
                Output::Dated(d) => outln!("result: {:?}, with {:?}", d.value, d.stamps),
                Output::Object { type_name, preview, .. } => outln!("result: {type_name}: {preview}"),
            }
            true
        }
        Ok(other) => {
            let kind = diagnostics::for_outcome(&other).map_or("", |d| d.kind.id);
            outln!("failed ({kind}): {}", describe_outcome(&other));
            false
        }
        Err(e) => {
            outln!("failed: couldn't ask: {e:?}");
            false
        }
    }
}

fn describe_outcome(o: &Outcome) -> String {
    match o {
        Outcome::Ok { .. } => "no error".into(),
        Outcome::PythonError { error, .. } => error.message.clone(),
        Outcome::TimedOut => "ran past its time limit".into(),
        Outcome::OverMemory { rss_bytes } => format!("went over its memory limit ({rss_bytes} bytes)"),
        Outcome::OutputTooLarge => "wrote a line over the cap".into(),
        Outcome::ProtocolViolation(m) | Outcome::Crashed(m) | Outcome::FailedToStart(m) => m.clone(),
    }
}
