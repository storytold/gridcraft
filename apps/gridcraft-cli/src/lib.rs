//! Headless GridCraft (`gridcraft-cli`). [`run`] is the whole program, so it can be tested in-process as well as
//! through the binary; `gridcraft-cli --help` lists the subcommands. The conventions are RFC-0018's: exit 0, 1 (a
//! command failed) or 2 (a usage error), results on stdout, `error: <message>` on stderr.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod args;
mod check;
mod commands;
mod error;
mod sandbox_check;
mod script;

use std::ffi::OsString;
use std::io::Write;

use error::CliError;

pub use args::command;

/// Where a command writes: results to `out`, diagnostics and progress to `err`.
pub(crate) struct Io<'a> {
    pub(crate) out: &'a mut dyn Write,
    pub(crate) err: &'a mut dyn Write,
}

/// Writes a line of results; fails with [`CliError::Stdout`], which `run` turns into a quiet end for a closed pipe
/// (`gridcraft-cli commands | head`) and a message for any other error.
macro_rules! outln {
    ($io:expr, $($arg:tt)*) => {
        writeln!($io.out, $($arg)*).map_err($crate::error::CliError::Stdout)?
    };
}

/// Writes a line of progress or a warning; a closed stderr is not worth an error.
macro_rules! errln {
    ($io:expr, $($arg:tt)*) => {
        drop(writeln!($io.err, $($arg)*))
    };
}

pub(crate) use {errln, outln};

/// The first Launcher-profile argument in `args`, in either its separate or `=` form. Only the tokens a shell would see
/// before `--` count: after it they are file names.
fn launch_profile_argument(args: &[OsString]) -> Option<String> {
    for arg in args {
        if arg == "--" {
            return None;
        }
        let text = arg.to_string_lossy();
        for flag in ["--launch-profile", "--profile-root"] {
            if text == flag || text.starts_with(&format!("{flag}=")) {
                return Some(flag.to_string());
            }
        }
    }
    None
}

/// Run the CLI on its arguments (without the program name), writing results to `out` and diagnostics to `err`.
/// Returns the process exit code: 0 on success, 1 when a command failed, 2 for a usage error. Help and the version go
/// to `out`, exit 0.
pub fn run<I, T>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    // A confined launcher run (and its probes) comes first, before anything else starts.
    if let Some(code) = gridcraft_sandbox::handle_args(&args) {
        return code;
    }
    // The nightly profile protocol belongs to the desktop app: the Launcher's selector is not a CLI option, and a CLI
    // that silently ignored it would act on the wrong files.
    if let Some(flag) = launch_profile_argument(&args) {
        let _ = writeln!(err, "error: {flag} belongs to the GridCraft desktop app, not this CLI");
        return 2;
    }
    // Help and the version (`help`, `-h`, `<subcommand> --help`, `-V`) come back as errors too.
    let cli = match args::parse(&args) {
        Ok(cli) => cli,
        Err(e) => {
            let to: &mut dyn Write = if e.use_stderr() { err } else { out };
            let _ = write!(to, "{}", e.render());
            return e.exit_code();
        }
    };
    let mut io = Io { out, err };
    match commands::dispatch(cli.command, &mut io) {
        Ok(code) => code,
        // A reader that stopped early (a closed pipe) ends the program quietly, as ripgrep does.
        Err(CliError::Stdout(e)) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            errln!(io, "error: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Launcher's nightly selector belongs to the desktop app; the CLI says so and exits 2 rather than acting on
    /// whatever it thinks the file list is.
    #[test]
    fn launcher_profile_arguments_are_refused_in_both_forms_and_only_before_the_delimiter() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(launch_profile_argument(&args(&["--launch-profile", "nightly-v1"])).as_deref(), Some("--launch-profile"));
        assert_eq!(launch_profile_argument(&args(&["--profile-root", "/tmp/p", "info", "a.xlsx"])).as_deref(), Some("--profile-root"));
        assert_eq!(launch_profile_argument(&args(&["--profile-root=/tmp/p"])).as_deref(), Some("--profile-root"));
        assert_eq!(launch_profile_argument(&args(&["info", "--launch-profile=nightly-v1"])).as_deref(), Some("--launch-profile"));
        // After `--` they are file names, exactly as the desktop app parses them.
        assert_eq!(launch_profile_argument(&args(&["--", "--launch-profile", "nightly-v1"])), None);
        assert_eq!(launch_profile_argument(&args(&["--", "--profile-root=/tmp/p"])), None);
        assert_eq!(launch_profile_argument(&args(&["info", "a.xlsx", "--json"])), None);
        assert_eq!(launch_profile_argument(&[]), None);
    }
}
