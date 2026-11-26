//! `gridcraft-cli`: headless GridCraft. Thin wiring over [`gridcraft_cli::run`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    // An exit code is 0 to 2 here; 128 + signal is the shell's, for a process a signal ended.
    ExitCode::from(u8::try_from(gridcraft_cli::run(std::env::args_os().skip(1), &mut out, &mut err)).unwrap_or(1))
}
