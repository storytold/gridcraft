//! `check`: measures calculation correctness and XLSX fidelity on any set of workbooks.
//!
//! **Calc oracle.** A workbook saved by Excel (or another calculating producer) carries the
//! producer's own result next to every formula (`<v>` beside `<f>`). The file is read keeping
//! those cached values, recalculated from scratch by GridCraft, and every formula cell's new value
//! is compared with the cached one. Formulas without a cached value (files from writers that don't
//! calculate) count as *no oracle*; volatile formulas (`RAND`, `NOW`…) and the formulas that read
//! them are skipped.
//!
//! **Round trip.** The recalculated workbook is written with GridCraft's XLSX writer, the package is
//! validated (content types cover every part, every internal relationship target exists, no kind of
//! relationship the source had is missing) and read back, and the two workbooks are compared feature
//! by feature.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gridcraft_engine::calc::{Calc, Key};
use gridcraft_engine::cmd::inspect::value_json;
use gridcraft_engine::core::{CellError, CellRef, Region, Value};
use gridcraft_engine::formula::{Expr, quote_sheet};
use gridcraft_engine::model::{Cell, Sheet, Workbook};
use serde_json::{Value as Json, json};

use super::{Args, USAGE, number_text, pretty};

mod calc;
mod fidelity;
mod package;
mod report;

pub use calc::oracle;
#[cfg(test)]
use calc::volatile_names;
#[cfg(test)]
use calc::{bool_cached_as_number, values_match};
#[cfg(test)]
use fidelity::diff_workbooks;
pub use fidelity::round_trip;
use package::full_calc_on_load;
#[cfg(test)]
use package::validate_package;
use report::print_human;
pub use report::report_json;

/// Version tag of the `--json` output; bumped when a field changes meaning or disappears.
pub const SCHEMA: &str = "gridcraft-check/1";
/// Directory recursion limit (symlink loops end here).
const MAX_DEPTH: usize = 32;
/// Most files collected from the inputs.
const MAX_FILES: usize = 100_000;

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Relative tolerance for numbers (absolute below magnitude 1).
    pub tolerance: f64,
    /// Most examples listed per file and category.
    pub max_report: usize,
    /// The runtime regional format, applied before formula recalculation and round-trip comparison.
    pub region: &'static Region,
}

impl Default for Options {
    /// Use the standard comparison tolerance, report limit and canonical formula region.
    fn default() -> Self {
        Options { tolerance: 1e-9, max_report: 20, region: gridcraft_engine::core::region::default_region() }
    }
}

// ---------------------------------------------------------------- reports

/// Per-function counts: formula cells checked that call the function, how many of them mismatch,
/// and how many mismatch while none of their precedents do (*primary*: the likely culprit).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FnStat {
    pub checked: usize,
    pub mismatched: usize,
    pub primary: usize,
}

impl FnStat {
    fn add(&mut self, o: &FnStat) {
        self.checked += o.checked;
        self.mismatched += o.mismatched;
        self.primary += o.primary;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch {
    /// `Sheet1!B2`.
    pub cell: String,
    /// `=SUM(A1:A3)`, `{=…}` for a legacy array formula.
    pub formula: String,
    pub expected: Value,
    pub actual: Value,
    /// `numeric`, `text`, `bool`, `error`, `blank` (GridCraft produced no value) or `type`.
    pub kind: &'static str,
    pub functions: Vec<String>,
    pub primary: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalcReport {
    pub formula_cells: usize,
    pub checked: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub no_oracle: usize,
    /// The producer asked for a full recalculation on load (`fullCalcOnLoad`): its cached values
    /// aren't results, so every formula counts as no oracle.
    pub stale_cache: bool,
    pub volatile: usize,
    /// Matched cells whose producer saved GridCraft's TRUE or FALSE as the number 1 or 0 (no
    /// `t="b"`, as Business Central's Excel exports do); counted in `matched` too.
    pub bool_as_number: usize,
    /// The first `max_report` mismatches, primary ones first.
    pub mismatches: Vec<Mismatch>,
    pub functions: BTreeMap<String, FnStat>,
    /// Mismatch counts by `expected type → actual type` (an error counts as its code), e.g.
    /// `#VALUE! → #SPILL!`: one signature often covers a single root cause across functions.
    pub signatures: BTreeMap<String, usize>,
}

/// One round-trip comparison: how many differences, and the first few.
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub name: &'static str,
    pub diffs: usize,
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoundTrip {
    /// Writing or reading back failed (the categories are then empty).
    pub error: Option<String>,
    pub categories: Vec<Category>,
}

impl RoundTrip {
    pub fn differing(&self) -> Vec<&'static str> {
        self.categories.iter().filter(|c| c.diffs > 0).map(|c| c.name).collect()
    }
    fn is_clean(&self) -> bool {
        self.error.is_none() && self.categories.iter().all(|c| c.diffs == 0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileReport {
    pub path: String,
    /// Opening the file or discovering the requested inputs failed: its message.
    pub error: Option<String>,
    /// Import warnings, in the order the importer produced them.
    pub warnings: Vec<String>,
    pub calc: CalcReport,
    pub round_trip: RoundTrip,
}

impl FileReport {
    fn is_clean(&self) -> bool {
        self.error.is_none() && self.calc.mismatched == 0 && self.round_trip.is_clean()
    }
}

/// Collects the first `max` examples while counting all differences.
struct Diffs {
    name: &'static str,
    count: usize,
    max: usize,
    examples: Vec<String>,
}

impl Diffs {
    fn new(name: &'static str, max: usize) -> Diffs {
        Diffs { name, count: 0, max, examples: vec![] }
    }
    fn push(&mut self, f: impl FnOnce() -> String) {
        self.count = self.count.saturating_add(1);
        if self.examples.len() < self.max {
            self.examples.push(f());
        }
    }
    fn done(self) -> Category {
        Category { name: self.name, diffs: self.count, examples: self.examples }
    }
}

// ---------------------------------------------------------------- entry point

/// `check <file-or-dir>... [--json] [--tolerance X] [--max-report N] [--region TAG]`.
/// Exit 0 when every file opens,
/// every checked formula matches and every round trip is clean; 1 otherwise; 2 on a usage error.
pub fn main(args: &[String]) -> ExitCode {
    let usage = |e: String| {
        eprintln!("gridcraft-cli: {e}\n{USAGE}");
        ExitCode::from(2)
    };
    let a = match Args::parse(args, &["tolerance", "max-report", "region"], &["json"]) {
        Ok(a) => a,
        Err(e) => return usage(e),
    };
    if a.positional.is_empty() {
        return usage("missing <file-or-dir>".into());
    }
    let mut opts = Options::default();
    match a.region() {
        Ok(Some(region)) => opts.region = region,
        Ok(None) => {}
        Err(e) => return usage(e),
    }
    if let Some(t) = a.opt("tolerance") {
        match t.parse::<f64>() {
            Ok(t) if t.is_finite() && t >= 0.0 => opts.tolerance = t,
            _ => return usage(format!("--tolerance needs a number ≥ 0, not `{t}`")),
        }
    }
    if let Some(n) = a.opt("max-report") {
        match n.parse::<usize>() {
            Ok(n) => opts.max_report = n,
            Err(_) => return usage(format!("--max-report needs a whole number, not `{n}`")),
        }
    }
    let Discovery { files, failures: mut reports } = collect_inputs(&a.positional);
    reports.extend(files.iter().map(|p| {
        let path = p.to_string_lossy().into_owned();
        match gridcraft_engine::io::read_file(&path) {
            Ok(bytes) => check_bytes(&path, &bytes, &opts),
            Err(e) => FileReport { path, error: Some(e.to_string()), ..Default::default() },
        }
    }));
    if a.has("json") {
        outln!("{}", pretty(&report_json(&reports, &opts)));
    } else {
        print_human(&reports, &opts);
    }
    if !files.is_empty() && reports.iter().all(FileReport::is_clean) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

#[derive(Default)]
struct Discovery {
    files: Vec<PathBuf>,
    failures: Vec<FileReport>,
}

impl Discovery {
    fn fail(&mut self, path: &Path, error: String) {
        self.failures.push(FileReport { path: path.to_string_lossy().into_owned(), error: Some(error), ..Default::default() });
    }

    /// False means the file budget is exhausted: the remaining scan is incomplete.
    fn add_file(&mut self, path: PathBuf) -> bool {
        if self.files.len() >= MAX_FILES {
            self.fail(&path, format!("file limit of {MAX_FILES} reached; this and remaining inputs were not checked"));
            return false;
        }
        self.files.push(path);
        true
    }
}

/// Files named on the command line are checked whatever their extension; directories contribute
/// their `.xlsx`/`.xlsm` files, recursively, in name order. Hidden files and Excel lock files
/// (`~$…`) are skipped.
fn collect_inputs(inputs: &[String]) -> Discovery {
    let mut out = Discovery::default();
    let mut seen = std::collections::HashSet::new();
    for i in inputs {
        let p = PathBuf::from(i);
        if p.is_dir() {
            if !walk(&p, 0, &mut out, &mut seen) {
                break;
            }
        } else if !out.add_file(p) {
            break;
        }
    }
    if out.files.is_empty()
        && out.failures.is_empty()
        && let Some(input) = inputs.first()
    {
        out.fail(Path::new(input), "no workbook files were found in the requested inputs".into());
    }
    out
}

/// Returns false only when the global file budget stops traversal.
fn walk(dir: &Path, depth: usize, out: &mut Discovery, seen: &mut std::collections::HashSet<u64>) -> bool {
    // A directory is visited once by identity (dev+inode on unix): several links to the same
    // directory are aliases, not exponential branches of the scan. Other platforms keep the
    // depth cap only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if let Ok(m) = std::fs::metadata(dir)
            && let Some(id) = m.dev().checked_mul(1 << 60).and_then(|d| d.checked_add(m.ino()))
            && !seen.insert(id)
        {
            return true;
        }
    }
    if depth > MAX_DEPTH {
        out.fail(dir, format!("directory depth limit of {MAX_DEPTH} exceeded; this subtree was not checked"));
        return true;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            out.fail(dir, format!("cannot read directory: {e}"));
            return true;
        }
    };
    let mut entries = Vec::new();
    for entry in rd {
        match entry {
            Ok(entry) => entries.push(entry.path()),
            Err(e) => out.fail(dir, format!("cannot read directory entry: {e}")),
        }
    }
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || name.starts_with("~$") {
            continue;
        }
        let metadata = match p.metadata() {
            Ok(metadata) => metadata,
            Err(e) => {
                out.fail(&p, format!("cannot inspect input: {e}"));
                continue;
            }
        };
        if metadata.is_dir() {
            if !walk(&p, depth + 1, out, seen) {
                return false;
            }
        } else if p.extension().and_then(std::ffi::OsStr::to_str).is_some_and(|e| e.eq_ignore_ascii_case("xlsx") || e.eq_ignore_ascii_case("xlsm"))
            && !out.add_file(p)
        {
            return false;
        }
    }
    true
}

/// Checks one file's bytes. Never fails: problems are recorded in the report. A panic escaping
/// GridCraft's reader, calculator or writer is caught and reported as an internal error, so one
/// bad file doesn't end a corpus run.
pub fn check_bytes(path: &str, bytes: &[u8], opts: &Options) -> FileReport {
    let guarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_inner(path, bytes, opts)));
    guarded.unwrap_or_else(|p| {
        let msg = p.downcast_ref::<&str>().map(ToString::to_string).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
        FileReport { path: path.to_string(), error: Some(format!("internal error while checking: {msg}")), ..Default::default() }
    })
}

/// Open and recalculate a workbook in the selected region before comparing caches and round trips.
fn check_inner(path: &str, bytes: &[u8], opts: &Options) -> FileReport {
    let mut report = FileReport { path: path.to_string(), ..Default::default() };
    let (mut cached, warnings) = match gridcraft_engine::io::open_bytes(path, bytes) {
        Ok(x) => x,
        Err(e) => {
            report.error = Some(e.to_string());
            return report;
        }
    };
    cached.region = opts.region;
    report.warnings = warnings;
    let mut fresh = cached.clone();
    let mut calc = Calc::new();
    calc.recalc_all(&mut fresh);
    let trust_cache = !full_calc_on_load(bytes);
    report.calc = oracle(&cached, &fresh, &calc, trust_cache, opts);
    report.round_trip = round_trip(&fresh, bytes, opts);
    report
}

#[cfg(test)]
mod tests;
