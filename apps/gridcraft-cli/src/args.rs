//! The command line as clap derive types (RFC-0018). Values arrive typed, so the commands never validate a string;
//! what clap cannot say alone is a [`UsageError`], printed and exited like clap's own.
//!
//! `run` is ordered: its `--cmd` and `--script` steps run in the order they were given, so the repeatable options are
//! kept with their positions and read back in order ([`RunArgs::steps`]).

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use serde_json::Value;

use crate::script::{ScriptError, Step, parse_command};

/// The end of every subcommand's `--help`.
const EXIT_STATUS: &str = "Exit status:\n  0  success\n  1  the command failed (for `check`, a difference was found)\n  2  usage error\n\nA process ended by a signal exits 128 + the signal number (130 for Ctrl-C), as in the shell.";

/// The top-level `--help` ends with the environment and the exit status.
const TOP_NOTES: &str = "Environment:\n  none: the CLI reads no environment variables of its own\n\nExit status:\n  0  success\n  1  the command failed (for `check`, a difference was found)\n  2  usage error\n\nA process ended by a signal exits 128 + the signal number (130 for Ctrl-C), as in the shell.";

/// What a script line looks like (the `run` help ends with it).
const SCRIPT_LINES: &str = "A script line is `id={json}`, `id {json}`, `id`, or {\"command\": \"id\", \"params\": {…}}; blank lines and lines starting with # or // are skipped.";

/// Headless GridCraft: inspect, convert, evaluate, script and serve MCP without a window
#[derive(Parser, Debug)]
#[command(name = "gridcraft-cli", version, propagate_version = true, arg_required_else_help = true, after_help = TOP_NOTES)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Sheets, used ranges, tables, charts and names of a workbook
    #[command(after_help = EXIT_STATUS)]
    Info(InfoArgs),
    /// Convert a workbook: xlsx, csv, tsv, json, html or pdf, by the extension of OUT
    #[command(after_help = EXIT_STATUS)]
    Convert(ConvertArgs),
    /// Evaluate one formula, optionally against a workbook
    #[command(after_help = EXIT_STATUS)]
    Eval(EvalArgs),
    /// Print a range as an aligned table or CSV
    #[command(after_help = EXIT_STATUS)]
    Cat(CatArgs),
    /// Cached-value calc oracle and XLSX round-trip fidelity of files or folders (exit 1 on any difference)
    #[command(after_help = EXIT_STATUS)]
    Check(CheckArgs),
    /// Run commands and scripts on a workbook, then save or print it
    #[command(after_help = format!("{SCRIPT_LINES}\n\n{EXIT_STATUS}"))]
    Run(RunArgs),
    /// List the commands
    #[command(after_help = EXIT_STATUS)]
    Commands(ListArgs),
    /// List the formula functions
    #[command(after_help = EXIT_STATUS)]
    Functions(FunctionsArgs),
    /// Serve MCP on stdio, headless or driving the running app
    #[command(after_help = EXIT_STATUS)]
    Mcp(McpArgs),
    /// Send one control-channel request to the running app
    #[command(after_help = EXIT_STATUS)]
    Send(SendArgs),
    /// Whether this system confines programs (no program: a confined self-test)
    ///
    /// With a program it runs that program confined, with the given read and executable paths and the system's shared
    /// libraries, passing its output and exit status through; its own lines then go to standard error. Every path is
    /// absolute. Exit status: 0 when every attempt is as required (or the program succeeded), 1 otherwise (a refused
    /// launch included), 2 when GridCraft has no sandbox for this kind of system.
    #[command(name = "sandbox-check", after_help = EXIT_STATUS)]
    SandboxCheck(SandboxCheckArgs),
}

#[derive(Args, Debug)]
pub(crate) struct InfoArgs {
    /// The workbook
    pub(crate) file: PathBuf,
    /// JSON alone on stdout
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ConvertArgs {
    /// The workbook to read
    pub(crate) input: PathBuf,
    /// The file to write (.xlsx, .csv, .tsv, .json, .html or .pdf)
    pub(crate) output: PathBuf,
    /// The sheet to write, by name or index
    #[arg(long, value_name = "NAME")]
    pub(crate) sheet: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct EvalArgs {
    /// The formula; a leading `=` is optional
    #[arg(allow_hyphen_values = true, value_parser = formula)]
    pub(crate) formula: String,
    /// Evaluate against this workbook
    #[arg(long = "in", value_name = "FILE")]
    pub(crate) input: Option<PathBuf>,
    /// The sheet to evaluate on, by name or index
    #[arg(long, value_name = "S")]
    pub(crate) sheet: Option<String>,
    /// The cell the formula sits in
    #[arg(long, value_name = "A1")]
    pub(crate) cell: Option<String>,
    /// JSON alone on stdout
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CatArgs {
    /// The workbook
    pub(crate) file: PathBuf,
    /// The range to print (default: the used range)
    #[arg(long, value_name = "A1:D20")]
    pub(crate) range: Option<String>,
    /// The sheet to print, by name or index
    #[arg(long, value_name = "S")]
    pub(crate) sheet: Option<String>,
    /// Show formulas instead of values
    #[arg(long)]
    pub(crate) formulas: bool,
    /// Print CSV instead of an aligned table
    #[arg(long)]
    pub(crate) csv: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CheckArgs {
    /// Files to check, or folders to scan for .xlsx and .xlsm files
    #[arg(required = true, value_name = "FILE_OR_DIR")]
    pub(crate) inputs: Vec<PathBuf>,
    /// JSON alone on stdout
    #[arg(long)]
    pub(crate) json: bool,
    /// How far a cached number may differ from the recalculated one
    #[arg(long, value_name = "T", allow_negative_numbers = true, value_parser = tolerance)]
    pub(crate) tolerance: Option<f64>,
    /// How many mismatches to list per file
    #[arg(long = "max-report", value_name = "N")]
    pub(crate) max_report: Option<usize>,
}

/// A formula may start with one dash (`-1+3`), but `--word` is a mistyped option, not a formula (`=--1` is one).
fn formula(text: &str) -> Result<String, UsageError> {
    if text.starts_with("--") { Err(UsageError::Formula(text.to_owned())) } else { Ok(text.to_owned()) }
}

fn tolerance(text: &str) -> Result<f64, UsageError> {
    text.parse().ok().filter(|t: &f64| t.is_finite() && *t >= 0.0).ok_or_else(|| UsageError::Tolerance(text.to_owned()))
}

/// The command or the script file of a `run` step.
#[derive(Clone, Debug)]
pub(crate) enum RunStep {
    /// `--cmd`; a blank or comment line is `None`.
    Cmd(Option<Step>),
    /// `--script`
    Script(PathBuf),
}

#[derive(Args, Debug)]
pub(crate) struct RunArgs {
    /// Start from this workbook
    #[arg(long = "in", value_name = "FILE", conflicts_with = "sample")]
    pub(crate) input: Option<PathBuf>,
    /// Start from a sample workbook
    #[arg(long, value_name = "NAME")]
    pub(crate) sample: Option<String>,
    /// Run one command, `ID={json}`; repeatable, in order with `--script`
    #[arg(long = "cmd", value_name = "ID={JSON}", value_parser = command_step)]
    cmds: Vec<Option<Step>>,
    /// Run a script file of command lines (.jsonl); repeatable, in order with `--cmd`
    #[arg(long = "script", value_name = "FILE")]
    scripts: Vec<PathBuf>,
    /// Save the workbook here when the steps are done (by extension)
    #[arg(long, value_name = "FILE")]
    pub(crate) out: Option<PathBuf>,
    /// Print a range when the steps are done (`used` or empty: the used range); repeatable
    #[arg(long, value_name = "RANGE")]
    pub(crate) print: Vec<String>,
    /// The sheet `--print` shows, by name or index
    #[arg(long, value_name = "S", requires = "print")]
    pub(crate) sheet: Option<String>,
    /// `--print` shows formulas instead of values
    #[arg(long, requires = "print")]
    pub(crate) formulas: bool,
    /// `--print` writes CSV instead of an aligned table
    #[arg(long, requires = "print")]
    pub(crate) csv: bool,
    /// Don't print each command's result
    #[arg(long)]
    pub(crate) quiet: bool,
    /// The steps in the order they were given.
    #[arg(skip)]
    pub(crate) steps: Vec<RunStep>,
}

/// The values of a repeatable option with the position each was given at.
fn at<T: Clone>(sub: &ArgMatches, id: &str, values: &[T]) -> Vec<(usize, T)> {
    sub.indices_of(id).into_iter().flatten().zip(values.iter().cloned()).collect()
}

impl RunArgs {
    fn check(&mut self, sub: &ArgMatches) {
        let mut steps: Vec<(usize, RunStep)> = at(sub, "cmds", &self.cmds).into_iter().map(|(i, c)| (i, RunStep::Cmd(c))).collect();
        steps.extend(at(sub, "scripts", &self.scripts).into_iter().map(|(i, p)| (i, RunStep::Script(p))));
        steps.sort_by_key(|(i, _)| *i);
        self.steps = steps.into_iter().map(|(_, s)| s).collect();
    }
}

fn command_step(text: &str) -> Result<Option<Step>, UsageError> {
    parse_command(text).map_err(UsageError::Cmd)
}

#[derive(Args, Debug)]
pub(crate) struct ListArgs {
    /// Only those whose id, label or menu contains this
    #[arg(value_name = "SEARCH", conflicts_with = "search")]
    pub(crate) word: Option<String>,
    /// The same, as an option
    #[arg(long, value_name = "X")]
    pub(crate) search: Option<String>,
    /// JSON alone on stdout
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct FunctionsArgs {
    /// Only those whose name or description contains this
    #[arg(value_name = "SEARCH", conflicts_with = "search")]
    pub(crate) word: Option<String>,
    /// The same, as an option
    #[arg(long, value_name = "X")]
    pub(crate) search: Option<String>,
    /// Only this category
    #[arg(long, value_name = "C")]
    pub(crate) category: Option<String>,
    /// JSON alone on stdout
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct McpArgs {
    /// Drive the running app (started with `gridcraft --control PORT`) instead of a headless workbook
    #[arg(long, value_name = "PORT", conflicts_with_all = ["input", "sample"])]
    pub(crate) connect: Option<String>,
    /// Serve this workbook
    #[arg(long = "in", value_name = "FILE", conflicts_with = "sample")]
    pub(crate) input: Option<PathBuf>,
    /// Serve a sample workbook
    #[arg(long, value_name = "NAME")]
    pub(crate) sample: Option<String>,
}

#[derive(Args, Debug)]
pub(crate) struct SendArgs {
    /// The control port of the running app
    pub(crate) port: String,
    /// The control-channel method
    pub(crate) method: String,
    /// Its parameters, as JSON
    #[arg(value_name = "JSON", value_parser = json_value)]
    pub(crate) params: Option<Value>,
}

fn json_value(text: &str) -> Result<Value, UsageError> {
    serde_json::from_str(text).map_err(|e| UsageError::Json(e.to_string()))
}

#[derive(Args, Debug)]
pub(crate) struct SandboxCheckArgs {
    /// Let the program read this path; repeatable
    #[arg(long, value_name = "PATH")]
    pub(crate) read: Vec<PathBuf>,
    /// Let the program execute this path; repeatable
    #[arg(long = "exec", value_name = "PATH")]
    pub(crate) exec: Vec<PathBuf>,
    /// After `--`: the program to run confined, and its arguments
    #[arg(last = true, value_name = "PROGRAM")]
    pub(crate) program: Vec<OsString>,
}

/// A mistake in the command line that clap can't see by itself.
#[derive(Debug, PartialEq, thiserror::Error)]
pub(crate) enum UsageError {
    #[error("--tolerance: expected a number ≥ 0, not `{0}`")]
    Tolerance(String),
    #[error("unexpected argument '{0}' found (start a formula that begins with `--` with `=`, as in `=--1`)")]
    Formula(String),
    #[error("--cmd: {0}")]
    Cmd(ScriptError),
    #[error("bad JSON params: {0}")]
    Json(String),
}

/// The clap definition of the whole command line (completions and man pages are generated from it).
pub fn command() -> clap::Command {
    Cli::command()
}

/// Parse the arguments (without the program name). Help and the version come back as the `Err` clap uses for them.
pub(crate) fn parse(args: &[OsString]) -> Result<Cli, clap::Error> {
    let matches = Cli::command().try_get_matches_from(std::iter::once(OsString::from("gridcraft-cli")).chain(args.iter().cloned()))?;
    let mut cli = Cli::from_arg_matches(&matches)?;
    if let (Command::Run(run), Some(("run", sub))) = (&mut cli.command, matches.subcommand()) {
        run.check(sub);
    }
    Ok(cli)
}
