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
use gridcraft_engine::core::{CellError, CellRef, Value};
use gridcraft_engine::formula::{Expr, quote_sheet};
use gridcraft_engine::model::{Cell, Comment, Sheet, Workbook};
use serde_json::{Value as Json, json};

use super::{Args, USAGE, number_text, pretty};

/// Version tag of the `--json` output; bumped when a field changes meaning or disappears.
pub const SCHEMA: &str = "gridcraft-check/1";

/// Functions whose result depends on the clock, a random generator or the environment.
const VOLATILE: [&str; 6] = ["RAND", "RANDARRAY", "RANDBETWEEN", "NOW", "TODAY", "INFO"];
/// Directory recursion limit (symlink loops end here).
const MAX_DEPTH: usize = 32;
/// Most files collected from the inputs.
const MAX_FILES: usize = 100_000;
/// Largest package part read while validating a written package.
const MAX_XML_PART: u64 = 64 * 1024 * 1024;
/// Most zip entries looked at while validating a written package.
const MAX_ENTRIES: usize = 200_000;
/// Relationship kinds (lower case) whose absence after a save isn't a loss: the calculation chain is
/// a cache Excel rebuilds, and GridCraft never writes it.
const REBUILT_KINDS: [&str; 1] = ["calcchain"];

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Relative tolerance for numbers (absolute below magnitude 1).
    pub tolerance: f64,
    /// Most examples listed per file and category.
    pub max_report: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { tolerance: 1e-9, max_report: 20 }
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
    /// GridCraft couldn't open the file: its message.
    pub error: Option<String>,
    pub warnings: usize,
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

/// `check <file-or-dir>... [--json] [--tolerance X] [--max-report N]`. Exit 0 when every file opens,
/// every checked formula matches and every round trip is clean; 1 otherwise; 2 on a usage error.
pub fn main(args: &[String]) -> ExitCode {
    let usage = |e: String| {
        eprintln!("gridcraft-cli: {e}\n{USAGE}");
        ExitCode::from(2)
    };
    let a = match Args::parse(args, &["tolerance", "max-report"], &["json"]) {
        Ok(a) => a,
        Err(e) => return usage(e),
    };
    if a.positional.is_empty() {
        return usage("missing <file-or-dir>".into());
    }
    let mut opts = Options::default();
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
    let files = collect_inputs(&a.positional);
    let reports: Vec<FileReport> = files
        .iter()
        .map(|p| {
            let path = p.to_string_lossy().into_owned();
            match gridcraft_engine::io::read_file(&path) {
                Ok(bytes) => check_bytes(&path, &bytes, &opts),
                Err(e) => FileReport { path, error: Some(e.to_string()), ..Default::default() },
            }
        })
        .collect();
    if a.has("json") {
        outln!("{}", pretty(&report_json(&reports, &opts)));
    } else {
        print_human(&reports, &opts);
    }
    if reports.iter().all(FileReport::is_clean) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Files named on the command line are checked whatever their extension; directories contribute
/// their `.xlsx`/`.xlsm` files, recursively, in name order. Hidden files and Excel lock files
/// (`~$…`) are skipped.
fn collect_inputs(inputs: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for i in inputs {
        let p = PathBuf::from(i);
        if p.is_dir() {
            walk(&p, 0, &mut out);
        } else {
            // A missing path is reported as a file that failed to open.
            out.push(p);
        }
        if out.len() >= MAX_FILES {
            out.truncate(MAX_FILES);
            break;
        }
    }
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || name.starts_with("~$") {
            continue;
        }
        if p.is_dir() {
            walk(&p, depth + 1, out);
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("xlsx") || e.eq_ignore_ascii_case("xlsm")) {
            out.push(p);
        }
        if out.len() >= MAX_FILES {
            return;
        }
    }
}

/// Checks one file's bytes. Never fails: problems are recorded in the report. A panic escaping
/// GridCraft's reader, calculator or writer is caught and reported as an internal error, so one
/// bad file doesn't end a corpus run.
pub fn check_bytes(path: &str, bytes: &[u8], opts: &Options) -> FileReport {
    let guarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_inner(path, bytes, opts)));
    guarded.unwrap_or_else(|p| {
        let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
        FileReport { path: path.to_string(), error: Some(format!("internal error while checking: {msg}")), ..Default::default() }
    })
}

fn check_inner(path: &str, bytes: &[u8], opts: &Options) -> FileReport {
    let mut report = FileReport { path: path.to_string(), ..Default::default() };
    let (cached, warnings) = match gridcraft_engine::io::open_bytes(path, bytes) {
        Ok(x) => x,
        Err(e) => {
            report.error = Some(e.to_string());
            return report;
        }
    };
    report.warnings = warnings.len();
    let mut fresh = cached.clone();
    let mut calc = Calc::new();
    calc.recalc_all(&mut fresh);
    let trust_cache = !full_calc_on_load(bytes);
    report.calc = oracle(&cached, &fresh, &calc, trust_cache, opts);
    report.round_trip = round_trip(&fresh, bytes, opts);
    report
}

// ---------------------------------------------------------------- calc oracle

fn functions_of(e: &Expr) -> Vec<String> {
    let mut set = BTreeSet::new();
    e.walk(&mut |x| {
        if let Expr::Call(n, _) = x {
            set.insert(n.clone());
        }
    });
    set.into_iter().collect()
}

fn is_volatile(e: &Expr, volatile_names: &HashSet<String>) -> bool {
    let mut v = false;
    e.walk(&mut |x| match x {
        Expr::Call(n, args) => {
            let filename = n == "CELL" && matches!(args.first(), Some(Expr::Text(t)) if t.eq_ignore_ascii_case("filename"));
            if filename || VOLATILE.contains(&n.as_str()) {
                v = true;
            }
        }
        Expr::Name(n) if volatile_names.contains(&n.to_ascii_uppercase()) => v = true,
        _ => {}
    });
    v
}

/// Upper-cased defined names whose formula is volatile, directly or through other names.
fn volatile_names(wb: &Workbook) -> HashSet<String> {
    let parsed: Vec<(String, Option<Expr>)> =
        wb.names.iter().map(|n| (n.name.to_ascii_uppercase(), gridcraft_engine::formula::parse(&n.formula).ok())).collect();
    let mut set = HashSet::new();
    // Each round adds at least one name or stops, so `len + 1` rounds reach the fixed point.
    for _ in 0..=parsed.len() {
        let before = set.len();
        for (name, e) in &parsed {
            if !set.contains(name) && e.as_ref().is_some_and(|e| is_volatile(e, &set)) {
                set.insert(name.clone());
            }
        }
        if set.len() == before {
            break;
        }
    }
    set
}

/// The formula as the formula bar shows it.
fn formula_text(cell: &Cell) -> String {
    cell.bar_text()
}

fn cell_name(wb: &Workbook, (sheet, c): Key) -> String {
    let name = wb.sheet(sheet).map(|s| s.name.as_str()).unwrap_or("?");
    format!("{}!{}", quote_sheet(name), c.a1())
}

/// Whether the producer cached GridCraft's logical result as the number 1 or 0. Excel types a
/// logical result `t="b"`; a writer that leaves the type out (`<c><f>TRUE()</f><v>1</v></c>`, as
/// Business Central's "Open in Excel" exports do) stores the same value, read back as a number.
pub fn bool_cached_as_number(expected: &Value, actual: &Value) -> bool {
    matches!((expected, actual), (Value::Number(n), Value::Bool(b)) if *n == if *b { 1.0 } else { 0.0 })
}

/// Whether GridCraft's `actual` agrees with the producer's `expected`.
pub fn values_match(expected: &Value, actual: &Value, tolerance: f64) -> bool {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => (a - b).abs() <= tolerance * a.abs().max(b.abs()).max(1.0),
        (Value::Text(a), Value::Text(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Error(a), Value::Error(b)) => a == b,
        _ => false,
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Empty => "blank",
        Value::Number(_) => "number",
        Value::Text(_) => "text",
        Value::Bool(_) => "bool",
        Value::Error(e) => e.as_str(),
        Value::Array(_) => "array",
    }
}

fn mismatch_kind(expected: &Value, actual: &Value) -> &'static str {
    match (expected, actual) {
        (_, Value::Empty) => "blank",
        (Value::Number(_), Value::Number(_)) => "numeric",
        (Value::Text(_), Value::Text(_)) => "text",
        (Value::Bool(_), Value::Bool(_)) => "bool",
        (Value::Error(_), _) | (_, Value::Error(_)) => "error",
        _ => "type",
    }
}

/// Mismatched cells indexed by (sheet, column) → rows, for precedent-area queries.
struct MismatchIndex {
    cols: HashMap<(usize, u32), BTreeSet<u32>>,
    /// Columns with mismatches per sheet (for very wide areas).
    sheet_cols: HashMap<usize, BTreeSet<u32>>,
}

impl MismatchIndex {
    fn new(keys: &[Key]) -> MismatchIndex {
        let mut cols: HashMap<(usize, u32), BTreeSet<u32>> = HashMap::new();
        let mut sheet_cols: HashMap<usize, BTreeSet<u32>> = HashMap::new();
        for &(s, c) in keys {
            cols.entry((s, c.col)).or_default().insert(c.row);
            sheet_cols.entry(s).or_default().insert(c.col);
        }
        MismatchIndex { cols, sheet_cols }
    }
    /// Whether a mismatched cell other than `own` lies in the area.
    fn any_in(&self, sheet: usize, r0: u32, r1: u32, c0: u32, c1: u32, own: Key) -> bool {
        let Some(cols) = self.sheet_cols.get(&sheet) else { return false };
        cols.range(c0..=c1)
            .any(|&col| self.cols.get(&(sheet, col)).is_some_and(|rows| rows.range(r0..=r1).any(|&row| (sheet, CellRef::new(row, col)) != own)))
    }
}

/// Compares every formula cell of `cached` (as the producer saved it) with `fresh` (recalculated
/// by GridCraft). Without `trust_cache` nothing is compared (see [`CalcReport::stale_cache`]).
pub fn oracle(cached: &Workbook, fresh: &Workbook, calc: &Calc, trust_cache: bool, opts: &Options) -> CalcReport {
    let mut r = CalcReport { stale_cache: !trust_cache, ..Default::default() };
    let vnames = volatile_names(cached);
    let mut volatile: HashSet<Key> = HashSet::new();
    for (si, sh) in cached.sheets.iter().enumerate() {
        for (c, cell) in sh.cells.iter() {
            if let Some(f) = &cell.formula {
                r.formula_cells += 1;
                if f.expr_arc().is_some_and(|e| is_volatile(&e, &vnames)) {
                    volatile.insert((si, c));
                }
            }
        }
    }
    // Everything downstream of a volatile formula is volatile too; each key is pushed once.
    let mut stack: Vec<Key> = volatile.iter().copied().collect();
    let mut deps = Vec::new();
    while let Some((s, c)) = stack.pop() {
        deps.clear();
        calc.graph.dependents(s, c, &mut deps);
        if let Some(range) = fresh.sheet(s).and_then(|sh| sh.spill_ranges.get(&c)) {
            calc.graph.dependents_of_range(s, *range, &mut deps);
        }
        for k in deps.drain(..) {
            if volatile.insert(k) {
                stack.push(k);
            }
        }
    }

    let mut bad: Vec<(Key, Vec<String>)> = Vec::new();
    for (si, sh) in cached.sheets.iter().enumerate() {
        for (c, cell) in sh.cells.iter() {
            let Some(f) = &cell.formula else { continue };
            if volatile.contains(&(si, c)) {
                r.volatile += 1;
                continue;
            }
            if cell.value.is_empty() || !trust_cache {
                r.no_oracle += 1;
                continue;
            }
            r.checked += 1;
            let actual = fresh.sheet(si).and_then(|s| s.cell(c)).map(|x| x.value.scalar()).unwrap_or_default();
            let funcs = match f.expr_arc() {
                Some(e) => {
                    let names = functions_of(&e);
                    if names.is_empty() { vec!["(no function)".to_string()] } else { names }
                }
                None => vec!["(unparsed)".to_string()],
            };
            let bool_as_number = bool_cached_as_number(&cell.value, &actual);
            let ok = bool_as_number || values_match(&cell.value, &actual, opts.tolerance);
            if bool_as_number {
                r.bool_as_number += 1;
            }
            for n in &funcs {
                let st = r.functions.entry(n.clone()).or_default();
                st.checked += 1;
                if !ok {
                    st.mismatched += 1;
                }
            }
            if ok {
                r.matched += 1;
            } else {
                r.mismatched += 1;
                *r.signatures.entry(format!("{} → {}", type_name(&cell.value), type_name(&actual))).or_default() += 1;
                bad.push(((si, c), funcs));
            }
        }
    }

    // A mismatch is primary when none of its precedent cells mismatch.
    let keys: Vec<Key> = bad.iter().map(|(k, _)| *k).collect();
    let index = MismatchIndex::new(&keys);
    let mut listed: Vec<(bool, Key, Vec<String>)> = Vec::with_capacity(bad.len());
    for (k, funcs) in bad {
        let upstream = calc
            .graph
            .precedents_of(k)
            .iter()
            .any(|a| index.any_in(a.sheet, a.range.start.row, a.range.end.row, a.range.start.col, a.range.end.col, k));
        if !upstream {
            for n in &funcs {
                r.functions.entry(n.clone()).or_default().primary += 1;
            }
        }
        listed.push((!upstream, k, funcs));
    }
    // Primary mismatches first, each group in sheet then row-major order (already sorted).
    listed.sort_by_key(|(primary, _, _)| !*primary);
    for (primary, k, functions) in listed.into_iter().take(opts.max_report) {
        let cell = cached.sheet(k.0).and_then(|s| s.cell(k.1));
        let expected = cell.map(|c| c.value.clone()).unwrap_or_default();
        let actual = fresh.sheet(k.0).and_then(|s| s.cell(k.1)).map(|x| x.value.scalar()).unwrap_or_default();
        r.mismatches.push(Mismatch {
            cell: cell_name(cached, k),
            formula: cell.map(formula_text).unwrap_or_default(),
            kind: mismatch_kind(&expected, &actual),
            expected,
            actual,
            functions,
            primary,
        });
    }
    r
}

// ---------------------------------------------------------------- round trip

/// Writes `wb` with GridCraft's XLSX writer, validates the package against itself and against
/// `source` (the file `wb` was read from), reads it back and compares.
pub fn round_trip(wb: &Workbook, source: &[u8], opts: &Options) -> RoundTrip {
    let bytes = match gridcraft_engine::io::save_bytes(wb, "roundtrip.xlsx") {
        Ok(b) => b,
        Err(e) => return RoundTrip { error: Some(format!("write failed: {e}")), categories: vec![] },
    };
    let mut package = Diffs::new("package", opts.max_report);
    match validate_package(&bytes) {
        Ok(issues) => issues.into_iter().for_each(|i| package.push(|| i)),
        Err(e) => package.push(|| e),
    }
    if let (Ok(before), Ok(after)) = (relationship_kinds(source), relationship_kinds(&bytes)) {
        for (key, kind) in &before {
            if !after.contains_key(key) && !REBUILT_KINDS.contains(&key.as_str()) {
                package.push(|| format!("lost relationship {kind}"));
            }
        }
    }
    let back = match gridcraft_engine::io::open_bytes("roundtrip.xlsx", &bytes) {
        Ok((b, _)) => b,
        Err(e) => return RoundTrip { error: Some(format!("reading the written file failed: {e}")), categories: vec![package.done()] },
    };
    let mut categories = diff_workbooks(wb, &back, opts);
    categories.push(package.done());
    RoundTrip { error: None, categories }
}

fn value_show(v: &Value) -> String {
    match v {
        Value::Empty => "(blank)".to_string(),
        Value::Text(t) => serde_json::to_string(t.as_ref()).unwrap_or_default(),
        Value::Number(n) => number_text(*n),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Error(e) => e.as_str().to_string(),
        Value::Array(_) => v.scalar().display(),
    }
}

fn opt_formula(c: Option<&Cell>) -> String {
    c.filter(|c| c.formula.is_some()).map(formula_text).unwrap_or_default()
}

/// Per-sheet comparison of two lists, reporting what one side has and the other lacks.
fn diff_sets(d: &mut Diffs, sheet: &str, a: &BTreeSet<String>, b: &BTreeSet<String>) {
    for x in a.difference(b) {
        d.push(|| format!("{sheet}: lost {x}"));
    }
    for x in b.difference(a) {
        d.push(|| format!("{sheet}: gained {x}"));
    }
}

/// A sheet's conditional-format rules in precedence order, as comparable text. The priority number
/// itself is left out, so renumbering without changing the order isn't a difference.
fn cond_format_list(s: &Sheet) -> Vec<String> {
    let mut cfs: Vec<_> = s.cond_formats.iter().collect();
    cfs.sort_by_key(|c| c.priority);
    cfs.iter().map(|c| format!("{:?} {:?} stopIfTrue={}", c.ranges, c.rule, c.stop_if_true)).collect()
}

fn diff_count(d: &mut Diffs, sheet: &str, a: usize, b: usize) {
    if a != b {
        d.push(|| format!("{sheet}: {a} → {b}"));
    }
}

fn canonical_comment(c: &Comment) -> Comment {
    let mut c = c.clone();
    // Missing timestamps and trailing unknown timestamps both read back as an absent suffix.
    // Interior gaps identify the reply a later timestamp belongs to, so retain those slots.
    while c.dates.last().is_some_and(String::is_empty) {
        c.dates.pop();
    }
    c
}

fn diff_comments(d: &mut Diffs, sheet: &str, a: &Sheet, b: &Sheet) {
    let mut compare = |cell: &CellRef| {
        let (before, after) = (a.comments.get(cell), b.comments.get(cell));
        if before == after {
            return;
        }
        if let (Some(x), Some(y)) = (before, after)
            && canonical_comment(x) == canonical_comment(y)
        {
            return;
        }
        d.push(|| format!("{sheet}!{}: {before:?} → {after:?}", cell.a1()));
    };
    for cell in a.comments.keys() {
        compare(cell);
    }
    for cell in b.comments.keys().filter(|cell| !a.comments.contains_key(cell)) {
        compare(cell);
    }
}

/// A value as the file format can hold it: `#SPILL!`/`#CALC!` are cached as `#VALUE!` and an
/// unresolved circular reference as 0 (what Excel writes), so those aren't round-trip losses.
fn as_cached(v: Value) -> Value {
    match v {
        Value::Error(CellError::Spill | CellError::Calc) => Value::Error(CellError::Value),
        Value::Error(CellError::Circ) => Value::Number(0.0),
        v => v,
    }
}

fn num_fmt<'a>(wb: &'a Workbook, c: Option<&Cell>) -> &'a str {
    wb.styles.get(c.map(|c| c.style).unwrap_or_default()).num_fmt.as_str()
}

/// Feature-by-feature comparison of a workbook (`a`) with what reading it back gave (`b`). Sheets
/// are paired by position.
pub fn diff_workbooks(a: &Workbook, b: &Workbook, opts: &Options) -> Vec<Category> {
    let m = opts.max_report;
    let mut sheets = Diffs::new("sheets", m);
    let (an, bn): (Vec<&str>, Vec<&str>) = (a.sheets.iter().map(|s| s.name.as_str()).collect(), b.sheets.iter().map(|s| s.name.as_str()).collect());
    if an != bn {
        sheets.push(|| format!("{an:?} → {bn:?}"));
    }
    let mut values = Diffs::new("values", m);
    let mut formulas = Diffs::new("formulas", m);
    let mut num_fmts = Diffs::new("numFmts", m);
    let mut merges = Diffs::new("merges", m);
    let mut tables = Diffs::new("tables", m);
    let mut cond_formats = Diffs::new("condFormats", m);
    let mut validations = Diffs::new("validations", m);
    let mut charts = Diffs::new("charts", m);
    let mut comments = Diffs::new("comments", m);
    let mut hyperlinks = Diffs::new("hyperlinks", m);
    let mut pivots = Diffs::new("pivots", m);
    for (sa, sb) in a.sheets.iter().zip(b.sheets.iter()) {
        let name = quote_sheet(&sa.name);
        let mut cell = |c: CellRef, ca: Option<&Cell>, cb: Option<&Cell>| {
            let cached = |x: Option<&Cell>| as_cached(x.map(|x| x.value.scalar()).unwrap_or_default());
            let (va, vb) = (cached(ca), cached(cb));
            if va != vb {
                values.push(|| format!("{name}!{}: {} → {}", c.a1(), value_show(&va), value_show(&vb)));
            }
            let (fa, fb) = (opt_formula(ca), opt_formula(cb));
            if fa != fb {
                formulas.push(|| format!("{name}!{}: {fa} → {fb}", c.a1()));
            }
            let (na, nb) = (num_fmt(a, ca), num_fmt(b, cb));
            if na != nb {
                num_fmts.push(|| format!("{name}!{}: {na} → {nb}", c.a1()));
            }
        };
        for (c, ca) in sa.cells.iter() {
            cell(c, Some(ca), sb.cell(c));
        }
        for (c, cb) in sb.cells.iter() {
            if sa.cell(c).is_none() {
                cell(c, None, Some(cb));
            }
        }
        let set = |f: &dyn Fn(&Sheet) -> Vec<String>, s: &Sheet| f(s).into_iter().collect::<BTreeSet<String>>();
        let merge_list = |s: &Sheet| -> Vec<String> { s.merges.iter().map(|r| r.a1()).collect() };
        diff_sets(&mut merges, &name, &set(&merge_list, sa), &set(&merge_list, sb));
        let table_list = |s: &Sheet| -> Vec<String> {
            s.tables
                .iter()
                .map(|t| format!("{} {} [{}]", t.name, t.range.a1(), t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")))
                .collect()
        };
        diff_sets(&mut tables, &name, &set(&table_list, sa), &set(&table_list, sb));
        let (cfa, cfb) = (cond_format_list(sa), cond_format_list(sb));
        if cfa.len() != cfb.len() {
            cond_formats.push(|| format!("{name}: {} → {}", cfa.len(), cfb.len()));
        } else if let Some((x, y)) = cfa.iter().zip(&cfb).find(|(x, y)| x != y) {
            cond_formats.push(|| format!("{name}: {x} → {y}"));
        }
        diff_count(&mut validations, &name, sa.validations.len(), sb.validations.len());
        let chart_list = |s: &Sheet| s.charts.iter().map(|c| format!("{:?} ({} series)", c.kind, c.series.len())).collect::<Vec<_>>();
        let (ca, cb) = (chart_list(sa), chart_list(sb));
        if ca != cb {
            charts.push(|| format!("{name}: {ca:?} → {cb:?}"));
        }
        diff_comments(&mut comments, &name, sa, sb);
        diff_count(&mut hyperlinks, &name, sa.hyperlinks.len(), sb.hyperlinks.len());
        diff_count(&mut pivots, &name, sa.pivots.len(), sb.pivots.len());
    }
    let mut names = Diffs::new("names", m);
    let name_list = |wb: &Workbook| -> BTreeSet<String> {
        wb.names
            .iter()
            .map(|n| {
                let scope = n.scope.map(|i| format!("{}!", quote_sheet(wb.sheet(i).map(|s| s.name.as_str()).unwrap_or("?")))).unwrap_or_default();
                format!("{scope}{}={}", n.name, n.formula)
            })
            .collect()
    };
    diff_sets(&mut names, "workbook", &name_list(a), &name_list(b));
    vec![
        sheets.done(),
        values.done(),
        formulas.done(),
        num_fmts.done(),
        merges.done(),
        names.done(),
        tables.done(),
        cond_formats.done(),
        validations.done(),
        charts.done(),
        comments.done(),
        hyperlinks.done(),
        pivots.done(),
    ]
}

// ---------------------------------------------------------------- package validation

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => name.get(i + 1..).unwrap_or(name),
        None => name,
    }
}

/// `(element local name, attributes)` for every start/empty element of an XML part.
fn xml_elements(xml: &[u8]) -> Result<Vec<(String, Vec<(String, String)>)>, String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let attrs = e
                    .attributes()
                    .with_checks(false)
                    .flatten()
                    .map(|a| {
                        let v = match a.normalized_value(quick_xml::XmlVersion::Explicit1_0) {
                            Ok(v) => v.into_owned(),
                            Err(_) => String::from_utf8_lossy(&a.value).into_owned(),
                        };
                        (String::from_utf8_lossy(local_name(a.key.as_ref())).into_owned(), v)
                    })
                    .collect();
                out.push((String::from_utf8_lossy(local_name(e.name().as_ref())).into_owned(), attrs));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("invalid XML: {e}")),
        }
        buf.clear();
    }
    Ok(out)
}

fn attr<'a>(attrs: &'a [(String, String)], k: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

/// Lower-cased part name without a leading slash: OPC part names compare case-insensitively.
fn part_key(name: &str) -> String {
    name.trim_start_matches('/').to_ascii_lowercase()
}

/// `%XX` escapes in a relationship target.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let hex = |j: usize| b.get(j).and_then(|h| (*h as char).to_digit(16));
        match (c, hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolves a relationship target against the source part's directory.
fn resolve_target(source_dir: &str, target: &str) -> String {
    let target = percent_decode(target.split('#').next().unwrap_or(target));
    let joined = match target.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None if source_dir.is_empty() => target,
        None => format!("{source_dir}/{target}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

type Zip<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

fn zip_names(zip: &mut Zip<'_>) -> Vec<String> {
    (0..zip.len().min(MAX_ENTRIES)).filter_map(|i| zip.by_index_raw(i).ok().filter(|f| !f.is_dir()).map(|f| f.name().to_string())).collect()
}

fn read_zip_part(zip: &mut Zip<'_>, name: &str) -> Result<Vec<u8>, String> {
    let f = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut buf = Vec::new();
    f.take(MAX_XML_PART).read_to_end(&mut buf).map_err(|e| format!("{name}: {e}"))?;
    Ok(buf)
}

/// Whether the producer flagged its cached results as stale with `<calcPr fullCalcOnLoad="1">`:
/// libraries that don't calculate write it, and store 0 or nothing as every formula's value.
fn full_calc_on_load(bytes: &[u8]) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else { return false };
    let names = zip_names(&mut zip);
    let find = |key: &str| names.iter().find(|n| part_key(n) == key).cloned();
    let mut workbook = "xl/workbook.xml".to_string();
    if let Some(rels) = find("_rels/.rels")
        && let Ok(xml) = read_zip_part(&mut zip, &rels)
        && let Ok(els) = xml_elements(&xml)
        && let Some(target) = els
            .iter()
            .find(|(e, a)| e == "Relationship" && attr(a, "Type").is_some_and(|t| t.ends_with("/officeDocument")))
            .and_then(|(_, a)| attr(a, "Target"))
    {
        workbook = resolve_target("", target);
    }
    let Some(name) = find(&part_key(&workbook)) else { return false };
    let Ok(xml) = read_zip_part(&mut zip, &name) else { return false };
    xml_elements(&xml)
        .ok()
        .into_iter()
        .flatten()
        .any(|(e, a)| e == "calcPr" && attr(&a, "fullCalcOnLoad").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
}

/// Problems Excel would repair in a written package: parts without a content type, content-type
/// overrides for missing parts, relationship files for missing parts, duplicate relationship ids
/// and internal relationships pointing at missing parts. `Err` when the package can't be read.
pub fn validate_package(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let names = zip_names(&mut zip);
    let parts: HashSet<String> = names.iter().map(|n| part_key(n)).collect();
    let mut read_part = |name: &str| read_zip_part(&mut zip, name);
    let mut issues = Vec::new();
    let ct_name = names.iter().find(|n| part_key(n) == "[content_types].xml").cloned();
    let (mut defaults, mut overrides) = (HashSet::new(), HashSet::new());
    match ct_name {
        None => issues.push("no [Content_Types].xml".to_string()),
        Some(n) => {
            for (el, attrs) in xml_elements(&read_part(&n)?).map_err(|e| format!("[Content_Types].xml: {e}"))? {
                match el.as_str() {
                    "Default" => {
                        if let Some(x) = attr(&attrs, "Extension") {
                            defaults.insert(x.to_ascii_lowercase());
                        }
                    }
                    "Override" => {
                        if let Some(p) = attr(&attrs, "PartName") {
                            overrides.insert(part_key(p));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let mut sorted: Vec<&String> = names.iter().collect();
    sorted.sort();
    for n in &sorted {
        let key = part_key(n);
        if key == "[content_types].xml" {
            continue;
        }
        let ext = key.rsplit('/').next().and_then(|f| f.rsplit_once('.')).map(|(_, e)| e.to_string());
        if !overrides.contains(&key) && !ext.is_some_and(|e| defaults.contains(&e)) {
            issues.push(format!("part {n} has no content type"));
        }
    }
    let mut missing_overrides: Vec<&String> = overrides.iter().filter(|p| !parts.contains(*p)).collect();
    missing_overrides.sort();
    for p in missing_overrides {
        issues.push(format!("content type override for missing part /{p}"));
    }
    for n in sorted {
        let key = part_key(n);
        let Some(rels_file) = key.strip_suffix(".rels") else { continue };
        // `dir/_rels/name.rels` describes `dir/name`; `_rels/.rels` the package itself.
        let (dir, file) = match rels_file.rsplit_once('/') {
            Some((d, f)) => (d, f),
            None => continue,
        };
        let Some(src_dir) = dir.strip_suffix("_rels").map(|d| d.trim_end_matches('/')) else { continue };
        let source = if src_dir.is_empty() { file.to_string() } else { format!("{src_dir}/{file}") };
        if !source.is_empty() && !parts.contains(&source) {
            issues.push(format!("{n} describes missing part {source}"));
        }
        let mut ids = HashSet::new();
        for (el, attrs) in xml_elements(&read_part(n)?).map_err(|e| format!("{n}: {e}"))? {
            if el != "Relationship" {
                continue;
            }
            let id = attr(&attrs, "Id").unwrap_or("");
            if !ids.insert(id.to_string()) {
                issues.push(format!("{n}: duplicate relationship id {id}"));
            }
            if attr(&attrs, "TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External")) {
                continue;
            }
            let target = attr(&attrs, "Target").unwrap_or("");
            let resolved = resolve_target(src_dir, target);
            if !parts.contains(&resolved.to_ascii_lowercase()) {
                let kind = attr(&attrs, "Type").and_then(|t| t.rsplit('/').next()).unwrap_or("?");
                issues.push(format!("{n}: {id} ({kind}) targets missing part {resolved}"));
            }
        }
    }
    Ok(issues)
}

/// Every relationship kind (last segment of the type, the same in the transitional and strict
/// namespaces) in the package's relationship parts, keyed in lower case. Relationships that carry
/// nothing are left out, as a save may rightly drop them: a link to a place in the workbook
/// (`#Sheet1!A1`, written as a cell's `location`) and a target part with nothing in it.
fn relationship_kinds(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let names = zip_names(&mut zip);
    let by_key: HashMap<String, &String> = names.iter().map(|n| (part_key(n), n)).collect();
    let mut kinds = BTreeMap::new();
    for n in names.iter().filter(|n| part_key(n).ends_with(".rels")) {
        let Some(src_dir) = rels_source_dir(&part_key(n)) else { continue };
        let Ok(els) = read_zip_part(&mut zip, n).and_then(|xml| xml_elements(&xml)) else { continue };
        for (_, attrs) in els.iter().filter(|(el, _)| el == "Relationship") {
            let Some(kind) = attr(attrs, "Type").and_then(|t| t.rsplit('/').next()) else { continue };
            let target = attr(attrs, "Target").unwrap_or("");
            if kind.eq_ignore_ascii_case("hyperlink") && target.starts_with('#') {
                continue;
            }
            let external = attr(attrs, "TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"));
            if !external
                && let Some(part) = by_key.get(&resolve_target(&src_dir, target).to_ascii_lowercase())
                && !holds_something(&mut zip, part, kind)
            {
                continue;
            }
            kinds.entry(kind.to_ascii_lowercase()).or_insert_with(|| kind.to_string());
        }
    }
    Ok(kinds)
}

/// The directory of the part a rels part describes: `xl` for `xl/_rels/workbook.xml.rels`, the
/// empty string for the package's `_rels/.rels`.
fn rels_source_dir(key: &str) -> Option<String> {
    let (dir, _) = key.strip_suffix(".rels")?.rsplit_once('/')?;
    Some(dir.strip_suffix("_rels")?.trim_end_matches('/').to_string())
}

/// Whether a part has content: an element under its root, or for comments (whose authors list
/// comes first) a comment, for VML a shape. Read only as far as needed; an unreadable part counts.
fn holds_something(zip: &mut Zip<'_>, name: &str, kind: &str) -> bool {
    use quick_xml::events::Event;
    let Ok(f) = zip.by_name(name) else { return true };
    let mut reader = quick_xml::Reader::from_reader(std::io::BufReader::new(f.take(MAX_XML_PART)));
    let wanted: Option<&[u8]> = match kind.to_ascii_lowercase().as_str() {
        "comments" => Some(b"comment"),
        "vmldrawing" => Some(b"shape"),
        _ => None,
    };
    let mut buf = Vec::new();
    let mut in_root = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let found = match wanted {
                    Some(w) => local_name(e.name().as_ref()) == w,
                    None => in_root,
                };
                if found {
                    return true;
                }
                in_root = true;
            }
            Ok(Event::Eof) => return false,
            Ok(_) => {}
            Err(_) => return true,
        }
        buf.clear();
    }
}

// ---------------------------------------------------------------- output

#[derive(Default)]
struct Totals {
    files: usize,
    opened: usize,
    failed: usize,
    formula_cells: usize,
    checked: usize,
    matched: usize,
    mismatched: usize,
    no_oracle: usize,
    volatile: usize,
    bool_as_number: usize,
    round_trip_failed: usize,
    round_trip_differing: usize,
    categories: BTreeMap<&'static str, usize>,
    functions: BTreeMap<String, FnStat>,
    signatures: BTreeMap<String, usize>,
}

fn totals(reports: &[FileReport]) -> Totals {
    let mut t = Totals { files: reports.len(), ..Default::default() };
    for r in reports {
        if r.error.is_some() {
            t.failed += 1;
            continue;
        }
        t.opened += 1;
        t.formula_cells += r.calc.formula_cells;
        t.checked += r.calc.checked;
        t.matched += r.calc.matched;
        t.mismatched += r.calc.mismatched;
        t.no_oracle += r.calc.no_oracle;
        t.volatile += r.calc.volatile;
        t.bool_as_number += r.calc.bool_as_number;
        if r.round_trip.error.is_some() {
            t.round_trip_failed += 1;
        }
        if !r.round_trip.is_clean() {
            t.round_trip_differing += 1;
        }
        for c in r.round_trip.differing() {
            *t.categories.entry(c).or_default() += 1;
        }
        for (n, s) in &r.calc.functions {
            t.functions.entry(n.clone()).or_default().add(s);
        }
        for (sig, n) in &r.calc.signatures {
            *t.signatures.entry(sig.clone()).or_default() += n;
        }
    }
    t
}

/// Functions with mismatches, worst first: most primary mismatches, then most mismatches.
fn ranked(functions: &BTreeMap<String, FnStat>) -> Vec<(&String, &FnStat)> {
    let mut v: Vec<(&String, &FnStat)> = functions.iter().filter(|(_, s)| s.mismatched > 0).collect();
    v.sort_by(|a, b| b.1.primary.cmp(&a.1.primary).then(b.1.mismatched.cmp(&a.1.mismatched)).then(a.0.cmp(b.0)));
    v
}

fn functions_json(functions: &BTreeMap<String, FnStat>) -> Json {
    let mut v: Vec<(&String, &FnStat)> = functions.iter().collect();
    v.sort_by(|a, b| b.1.primary.cmp(&a.1.primary).then(b.1.mismatched.cmp(&a.1.mismatched)).then(a.0.cmp(b.0)));
    Json::Array(v.into_iter().map(|(n, s)| json!({"name": n, "checked": s.checked, "mismatched": s.mismatched, "primary": s.primary})).collect())
}

pub fn report_json(reports: &[FileReport], opts: &Options) -> Json {
    let files: Vec<Json> = reports
        .iter()
        .map(|r| {
            let c = &r.calc;
            let mismatches: Vec<Json> = c
                .mismatches
                .iter()
                .map(|m| {
                    json!({
                        "cell": m.cell, "formula": m.formula, "expected": value_json(&m.expected), "actual": value_json(&m.actual),
                        "kind": m.kind, "functions": m.functions, "primary": m.primary,
                    })
                })
                .collect();
            let categories: serde_json::Map<String, Json> = r
                .round_trip
                .categories
                .iter()
                .map(|cat| (cat.name.to_string(), json!({"equal": cat.diffs == 0, "diffs": cat.diffs, "examples": cat.examples})))
                .collect();
            json!({
                "path": r.path,
                "opened": r.error.is_none(),
                "error": r.error,
                "warnings": r.warnings,
                "calc": {
                    "formulaCells": c.formula_cells, "checked": c.checked, "matched": c.matched, "mismatched": c.mismatched,
                    "noOracle": c.no_oracle, "staleCache": c.stale_cache, "volatile": c.volatile, "boolAsNumber": c.bool_as_number,
                    "mismatches": mismatches, "functions": functions_json(&c.functions),
                    "signatures": c.signatures,
                },
                "roundTrip": {"error": r.round_trip.error, "differing": r.round_trip.differing(), "categories": categories},
            })
        })
        .collect();
    let t = totals(reports);
    json!({
        "schema": SCHEMA,
        "tolerance": opts.tolerance,
        "files": files,
        "totals": {
            "files": t.files, "opened": t.opened, "failedToOpen": t.failed, "formulaCells": t.formula_cells, "checked": t.checked,
            "matched": t.matched, "mismatched": t.mismatched, "noOracle": t.no_oracle, "volatile": t.volatile, "boolAsNumber": t.bool_as_number,
            "roundTripFailed": t.round_trip_failed, "roundTripDiffering": t.round_trip_differing, "categoriesDiffering": t.categories,
        },
        "functions": functions_json(&t.functions),
        "signatures": t.signatures,
    })
}

/// One report line: line breaks and tabs shown escaped so that they stay visible as differences.
fn one_line(s: &str) -> String {
    s.replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

/// `…tail` of a long label.
fn fit(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w { s.to_string() } else { format!("…{}", s.chars().skip(n + 1 - w).collect::<String>()) }
}

fn print_human(reports: &[FileReport], opts: &Options) {
    for r in reports {
        outln!("== {}", r.path);
        if let Some(e) = &r.error {
            outln!("  can't open: {e}");
            continue;
        }
        let c = &r.calc;
        outln!(
            "  formulas {}: {} checked, {} matched, {} mismatched, {} no oracle, {} volatile",
            c.formula_cells,
            c.checked,
            c.matched,
            c.mismatched,
            c.no_oracle,
            c.volatile
        );
        if c.stale_cache {
            outln!("  (the producer asked for a full recalculation on load: its cached values aren't results)");
        }
        if c.bool_as_number > 0 {
            outln!("  ({} matched TRUE/FALSE results were saved by the producer as 1/0, without t=\"b\")", c.bool_as_number);
        }
        for m in &c.mismatches {
            let tag = if m.primary { "mismatch" } else { "  (downstream)" };
            outln!(
                "  {tag} {}  {}  expected {}  got {}  [{}]",
                m.cell,
                one_line(&m.formula),
                value_show(&m.expected),
                value_show(&m.actual),
                m.functions.join(" ")
            );
        }
        if c.mismatched > c.mismatches.len() {
            outln!("  … {} more mismatches", c.mismatched - c.mismatches.len());
        }
        let rt = &r.round_trip;
        if let Some(e) = &rt.error {
            outln!("  round trip: {e}");
        }
        let differing: Vec<&Category> = rt.categories.iter().filter(|c| c.diffs > 0).collect();
        if differing.is_empty() && rt.error.is_none() {
            outln!("  round trip: all equal");
        }
        for cat in differing {
            outln!("  round trip {}: {} difference(s)", cat.name, cat.diffs);
            for e in &cat.examples {
                outln!("    {}", one_line(e));
            }
            if cat.diffs > cat.examples.len() {
                outln!("    … {} more", cat.diffs - cat.examples.len());
            }
        }
    }
    outln!("");
    let w = reports.iter().map(|r| r.path.chars().count()).max().unwrap_or(4).clamp(4, 50);
    outln!(
        "{:<w$}  {:>8}  {:>8}  {:>8}  {:>8}  {:>9}  {:>8}  round trip",
        "file",
        "formulas",
        "checked",
        "matched",
        "mismatch",
        "no-oracle",
        "volatile"
    );
    for r in reports {
        let label = fit(&r.path, w);
        if let Some(e) = &r.error {
            outln!("{label:<w$}  can't open: {}", one_line(&fit(e, 100)));
            continue;
        }
        let c = &r.calc;
        let rt = match (&r.round_trip.error, r.round_trip.differing()) {
            (Some(_), _) => "FAILED".to_string(),
            (None, d) if d.is_empty() => "ok".to_string(),
            (None, d) => d.join(","),
        };
        outln!(
            "{label:<w$}  {:>8}  {:>8}  {:>8}  {:>8}  {:>9}  {:>8}  {rt}",
            c.formula_cells,
            c.checked,
            c.matched,
            c.mismatched,
            c.no_oracle,
            c.volatile
        );
    }
    let t = totals(reports);
    let top = ranked(&t.functions);
    if !top.is_empty() {
        outln!("");
        outln!("top mismatching functions (primary = no mismatching precedent):");
        outln!("  {:<20}  {:>8}  {:>10}  {:>8}", "function", "primary", "mismatched", "checked");
        for (n, s) in top.iter().take(opts.max_report.max(1)) {
            outln!("  {:<20}  {:>8}  {:>10}  {:>8}", n, s.primary, s.mismatched, s.checked);
        }
    }
    let mut sigs: Vec<(&String, &usize)> = t.signatures.iter().collect();
    sigs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    if !sigs.is_empty() {
        outln!("");
        outln!("mismatch signatures (expected → GridCraft):");
        for (sig, n) in sigs.iter().take(opts.max_report.max(1)) {
            outln!("  {n:>8}  {sig}");
        }
    }
    outln!("");
    outln!("files: {} ({} opened, {} failed to open)", t.files, t.opened, t.failed);
    outln!(
        "formula cells: {} ({} checked: {} matched, {} mismatched; {} no oracle; {} volatile)",
        t.formula_cells,
        t.checked,
        t.matched,
        t.mismatched,
        t.no_oracle,
        t.volatile
    );
    if t.bool_as_number > 0 {
        outln!("booleans saved as 1/0 by the producer: {} (counted as matched)", t.bool_as_number);
    }
    let cats: Vec<String> = t.categories.iter().map(|(c, n)| format!("{c} {n}")).collect();
    outln!(
        "round trip: {} of {} files differ{}{}",
        t.round_trip_differing,
        t.opened,
        if t.round_trip_failed > 0 { format!(" ({} failed)", t.round_trip_failed) } else { String::new() },
        if cats.is_empty() { String::new() } else { format!("; categories (files): {}", cats.join(", ")) }
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::io::Write as _;
    use std::sync::Arc;

    use gridcraft_engine::model::Formula;

    use super::*;

    fn at(a1: &str) -> CellRef {
        CellRef::parse(a1).unwrap()
    }

    /// A workbook whose cells are `(A1, input, cached value)`: an input starting with `=` is a
    /// formula with that cached value, anything else a constant.
    fn book(cells: &[(&str, &str, Value)]) -> Workbook {
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        for (a1, input, cached) in cells {
            let cell = if input.starts_with('=') {
                Cell { value: cached.clone(), formula: Some(Arc::new(Formula::new(input))), ..Default::default() }
            } else {
                Cell::value(cached.clone())
            };
            sh.set_cell(at(a1), cell);
        }
        wb
    }

    fn check(wb: &Workbook, opts: &Options) -> FileReport {
        let bytes = gridcraft_engine::io::save_bytes(wb, "t.xlsx").unwrap();
        check_bytes("t.xlsx", &bytes, opts)
    }

    fn n(x: f64) -> Value {
        Value::number(x)
    }

    #[test]
    fn matching_cached_values_pass() {
        let wb = book(&[("A1", "2", n(2.0)), ("B1", "=A1*3", n(6.0)), ("C1", "=A1&\"x\"", Value::text("2x")), ("D1", "=A1>1", Value::Bool(true))]);
        let r = check(&wb, &Options::default());
        assert_eq!(r.error, None);
        assert_eq!((r.calc.formula_cells, r.calc.checked, r.calc.matched, r.calc.mismatched), (3, 3, 3, 0));
        assert!(r.is_clean(), "{r:?}");
    }

    #[test]
    fn wrong_cached_value_is_a_mismatch_and_ranks_its_function() {
        // Excel "said" SUM gives 7; GridCraft computes 6. C1 reads B1 and is consistent with
        // Excel's 7, so it mismatches too, but only as a downstream effect.
        let wb = book(&[("A1", "2", n(2.0)), ("A2", "4", n(4.0)), ("B1", "=SUM(A1:A2)", n(7.0)), ("C1", "=B1+1", n(8.0))]);
        let r = check(&wb, &Options::default());
        assert_eq!((r.calc.checked, r.calc.mismatched), (2, 2));
        let first = &r.calc.mismatches[0];
        assert_eq!(first.cell, "Sheet1!B1");
        assert_eq!(first.formula, "=SUM(A1:A2)");
        assert_eq!((first.expected.clone(), first.actual.clone(), first.kind, first.primary), (n(7.0), n(6.0), "numeric", true));
        assert!(!r.calc.mismatches[1].primary);
        assert_eq!(r.calc.functions["SUM"], FnStat { checked: 1, mismatched: 1, primary: 1 });
        assert_eq!(r.calc.functions["(no function)"], FnStat { checked: 1, mismatched: 1, primary: 0 });
        assert!(!r.is_clean());
    }

    #[test]
    fn errors_compare_by_kind() {
        let wb = book(&[("A1", "=1/0", Value::Error(CellError::Div0)), ("A2", "=1/0", Value::Error(CellError::NA)), ("A3", "=1/0", n(0.0))]);
        let r = check(&wb, &Options::default());
        assert_eq!((r.calc.matched, r.calc.mismatched), (1, 2));
        assert!(r.calc.mismatches.iter().all(|m| m.kind == "error"));
        assert_eq!(r.calc.signatures.get("#N/A → #DIV/0!"), Some(&1));
        assert_eq!(r.calc.signatures.get("number → #DIV/0!"), Some(&1));
        assert!(values_match(&Value::Error(CellError::NA), &Value::Error(CellError::NA), 0.0));
        assert!(!values_match(&Value::Error(CellError::NA), &Value::Error(CellError::Value), 0.0));
    }

    #[test]
    fn numbers_compare_within_tolerance() {
        assert!(values_match(&n(3.0000000001), &n(3.0), 1e-9));
        assert!(!values_match(&n(3.0000000001), &n(3.0), 1e-12));
        // Absolute near zero.
        assert!(values_match(&n(1e-12), &n(0.0), 1e-9));
        assert!(values_match(&n(1e12 + 1.0), &n(1e12), 1e-9));
        assert!(!values_match(&n(0.0), &Value::Empty, 1.0));
        assert!(!values_match(&Value::text("1"), &n(1.0), 1.0));
        assert!(values_match(&Value::text("a"), &Value::text("a"), 0.0));
        assert!(!values_match(&Value::text("a"), &Value::text("A"), 0.0));
        assert!(!values_match(&Value::Bool(true), &n(1.0), 1.0));
        let wb = book(&[("A1", "=1+2", n(3.0000000001))]);
        assert_eq!(check(&wb, &Options::default()).calc.matched, 1);
        assert_eq!(check(&wb, &Options { tolerance: 1e-12, ..Default::default() }).calc.mismatched, 1);
    }

    /// Business Central writes TRUE and FALSE as `<c><f>TRUE()</f><v>1</v></c>`, without `t="b"`:
    /// the cache holds the right value under a number type, so it matches and is counted apart.
    #[test]
    fn booleans_cached_as_numbers_match_and_are_counted_apart() {
        let wb = book(&[
            ("A1", "=TRUE()", n(1.0)),
            ("B1", "=FALSE()", n(0.0)),
            ("C1", "=TRUE()", Value::Bool(true)),
            ("D1", "=1=1", n(0.0)),
            ("E1", "=TRUE()+0", n(1.0)),
        ]);
        let r = check(&wb, &Options::default());
        assert_eq!((r.calc.checked, r.calc.matched, r.calc.mismatched), (5, 4, 1));
        // A cached 0 for TRUE is still a mismatch, and a number result is compared as a number.
        assert_eq!(r.calc.mismatches[0].cell, "Sheet1!D1");
        assert_eq!(r.calc.signatures.get("number → bool"), Some(&1));
        assert_eq!(r.calc.bool_as_number, 2);
        let v = report_json(std::slice::from_ref(&r), &Options::default());
        assert_eq!((v["files"][0]["calc"]["boolAsNumber"].clone(), v["totals"]["boolAsNumber"].clone()), (json!(2), json!(2)));
        assert!(bool_cached_as_number(&n(1.0), &Value::Bool(true)));
        assert!(!bool_cached_as_number(&n(1.0), &Value::Bool(false)));
        assert!(!bool_cached_as_number(&n(2.0), &Value::Bool(true)));
        assert!(!bool_cached_as_number(&Value::Bool(true), &n(1.0)));
    }

    #[test]
    fn missing_cached_values_and_volatile_formulas_are_skipped() {
        let wb =
            book(&[("A1", "=1+1", Value::Empty), ("B1", "=NOW()", n(1.0)), ("B2", "=B1+1", n(2.0)), ("B3", "=CELL(\"filename\")", Value::text("x"))]);
        let r = check(&wb, &Options::default());
        assert_eq!((r.calc.formula_cells, r.calc.checked, r.calc.no_oracle, r.calc.volatile), (4, 0, 1, 3));
    }

    #[test]
    fn full_calc_on_load_means_no_oracle() {
        // A formula without a cached value makes GridCraft's writer flag the file
        // `fullCalcOnLoad`, like libraries that store 0 for every formula: B1's wrong "cached"
        // value must not count as a mismatch then.
        let wb = book(&[("A1", "=1+1", Value::Empty), ("B1", "=2+2", n(0.0))]);
        let bytes = gridcraft_engine::io::save_bytes(&wb, "t.xlsx").unwrap();
        assert!(full_calc_on_load(&bytes));
        let r = check_bytes("t.xlsx", &bytes, &Options::default());
        assert!(r.calc.stale_cache);
        assert_eq!((r.calc.checked, r.calc.no_oracle, r.calc.mismatched), (0, 2, 0));
        let fresh = gridcraft_engine::io::save_bytes(&book(&[("B1", "=2+2", n(0.0))]), "t.xlsx").unwrap();
        assert!(!full_calc_on_load(&fresh));
        assert!(!full_calc_on_load(b"not a zip"));
    }

    #[test]
    fn volatile_names_propagate() {
        let mut wb = book(&[("A1", "=Stamp+1", n(1.0))]);
        wb.names.push(gridcraft_engine::model::DefinedName {
            name: "Stamp".into(),
            scope: None,
            formula: "TODAY()".into(),
            comment: String::new(),
            hidden: false,
        });
        assert!(volatile_names(&wb).contains("STAMP"));
        assert_eq!(check(&wb, &Options::default()).calc.volatile, 1);
    }

    #[test]
    fn round_trip_of_a_simple_book_is_clean() {
        let mut wb = book(&[("A1", "2", n(2.0)), ("B1", "=A1*3", n(6.0)), ("A2", "x", Value::text("x"))]);
        wb.sheet_mut(0).unwrap().merges.push(gridcraft_engine::core::RangeRef::parse("C1:D2").unwrap());
        let source = gridcraft_engine::io::save_bytes(&wb, "t.xlsx").unwrap();
        let rt = round_trip(&wb, &source, &Options::default());
        assert_eq!(rt.error, None);
        assert!(rt.is_clean(), "{rt:?}");
        assert!(rt.categories.iter().any(|c| c.name == "package"));
    }

    #[test]
    fn diff_detects_lost_features() {
        let mut a = book(&[("A1", "2", n(2.0)), ("B1", "=A1*3", n(6.0))]);
        a.sheet_mut(0).unwrap().merges.push(gridcraft_engine::core::RangeRef::parse("C1:D2").unwrap());
        let mut b = a.clone();
        let sh = b.sheet_mut(0).unwrap();
        sh.merges.clear();
        sh.set_cell(at("A1"), Cell::value(n(3.0)));
        sh.set_cell(at("B1"), Cell::value(n(6.0)));
        b.sheets.push(Arc::new(Sheet::new("Extra")));
        let cats = diff_workbooks(&a, &b, &Options { max_report: 1, ..Default::default() });
        let get = |name: &str| cats.iter().find(|c| c.name == name).unwrap();
        assert_eq!(get("values").diffs, 1);
        assert_eq!(get("values").examples, ["Sheet1!A1: 2 → 3"]);
        assert_eq!(get("formulas").examples, ["Sheet1!B1: =A1*3 → "]);
        assert_eq!(get("merges").examples, ["Sheet1: lost C1:D2"]);
        assert_eq!(get("sheets").diffs, 1);
        assert_eq!(get("tables").diffs, 0);
    }

    #[test]
    fn diff_detects_changed_cond_format_rule() {
        use gridcraft_engine::model::{CfRule, CfValueKind, Color, CondFormat};
        let bar = |gradient: bool, priority: u32| CondFormat {
            ranges: vec![gridcraft_engine::core::RangeRef::parse("A1").unwrap()],
            rule: CfRule::DataBar { min: CfValueKind::Min, max: CfValueKind::Max, color: Color::Rgb(0x638EC6), gradient, show_value: true },
            priority,
            stop_if_true: false,
        };
        let with = |cf: CondFormat| {
            let mut wb = book(&[("A1", "2", n(2.0))]);
            wb.sheet_mut(0).unwrap().cond_formats.push(cf);
            wb
        };
        let diffs =
            |a: &Workbook, b: &Workbook| diff_workbooks(a, b, &Options::default()).into_iter().find(|c| c.name == "condFormats").unwrap().diffs;
        assert_eq!(diffs(&with(bar(false, 1)), &with(bar(true, 1))), 1);
        assert_eq!(diffs(&with(bar(true, 1)), &with(bar(true, 1))), 0);
        assert_eq!(diffs(&with(bar(true, 1)), &with(bar(true, 5))), 0);
    }

    fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn package_validation_finds_what_excel_would_repair() {
        let bytes = zip_of(&[
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="r"/><Override PartName="/xl/workbook.xml" ContentType="w"/><Override PartName="/xl/gone.xml" ContentType="g"/></Types>"#,
            ),
            ("_rels/.rels", r#"<Relationships><Relationship Id="rId1" Type="x/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
            ("xl/workbook.xml", "<workbook/>"),
            ("xl/styles.xml", "<styleSheet/>"),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Type="x/styles" Target="styles.xml"/><Relationship Id="rId2" Type="x/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="x/hyperlink" Target="https://example.com" TargetMode="External"/><Relationship Id="rId3" Type="x/theme" Target="/xl/../xl/styles.xml"/></Relationships>"#,
            ),
        ]);
        let issues = validate_package(&bytes).unwrap();
        assert_eq!(
            issues,
            [
                "part xl/styles.xml has no content type",
                "content type override for missing part /xl/gone.xml",
                "xl/_rels/workbook.xml.rels: rId2 (worksheet) targets missing part xl/worksheets/sheet1.xml",
                "xl/_rels/workbook.xml.rels: duplicate relationship id rId2",
            ]
        );
        assert!(validate_package(b"not a zip").is_err());
        let own = gridcraft_engine::io::save_bytes(&book(&[("A1", "=1", n(1.0))]), "t.xlsx").unwrap();
        assert_eq!(validate_package(&own).unwrap(), Vec::<String>::new());
    }

    const WB_RELS: &str = "xl/_rels/workbook.xml.rels";
    const SHEET_RELS: &str = "xl/worksheets/_rels/sheet1.xml.rels";
    const MADE_UP: &str = r#"<Relationship Id="rId90" Type="http://example.com/relationships/madeUp" Target="madeUp.xml"/>"#;
    /// Not empty: an empty part isn't a loss.
    const MADE_UP_PART: (&str, &str) = ("xl/madeUp.xml", "<madeUp><item/></madeUp>");

    /// `bytes` with relationships added to rels parts (created when missing) and `parts` added.
    fn with_rels(bytes: &[u8], rels: &[(&str, &str)], parts: &[(&str, &str)]) -> Vec<u8> {
        let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut pending: Vec<(&str, &str)> = rels.to_vec();
        for i in 0..src.len() {
            let mut f = src.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut body = String::new();
            f.read_to_string(&mut body).unwrap();
            if let Some(at) = pending.iter().position(|(r, _)| *r == name) {
                body = body.replace("</Relationships>", &format!("{}</Relationships>", pending.remove(at).1));
            }
            out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(body.as_bytes()).unwrap();
        }
        let created = pending.iter().map(|(r, extra)| {
            (*r, format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{extra}</Relationships>"#))
        });
        for (name, body) in created.collect::<Vec<_>>().iter().map(|(n, b)| (*n, b.as_str())).chain(parts.iter().copied()) {
            out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(body.as_bytes()).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    /// The `package` examples of checking `source`.
    fn package_of(source: &[u8]) -> Vec<String> {
        let r = check_bytes("t.xlsx", source, &Options::default());
        r.round_trip.categories.iter().find(|c| c.name == "package").unwrap().examples.clone()
    }

    fn plain() -> Vec<u8> {
        gridcraft_engine::io::save_bytes(&book(&[("A1", "1", n(1.0))]), "t.xlsx").unwrap()
    }

    #[test]
    fn package_reports_relationship_kinds_the_output_lost() {
        // A kind GridCraft doesn't know is dropped on save; a calculation chain is never written
        // (Excel rebuilds it), so its absence isn't a loss.
        let calc_chain = r#"<Relationship Id="rId91" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/>"#;
        let source = with_rels(&plain(), &[(WB_RELS, &format!("{MADE_UP}{calc_chain}"))], &[MADE_UP_PART, ("xl/calcChain.xml", "<calcChain/>")]);
        assert_eq!(package_of(&source), ["lost relationship madeUp"]);
        assert!(check_bytes("t.xlsx", &plain(), &Options::default()).round_trip.is_clean());
    }

    // In the three tests below the made-up kind stays lost, so the comparison is known to run.

    #[test]
    fn an_internal_link_written_as_a_relationship_is_not_a_loss() {
        // openpyxl writes `#Sheet1!B2` as an external relationship; GridCraft writes `location`.
        let link = r##"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="#Sheet1!B2" TargetMode="External"/>"##;
        let source = with_rels(&plain(), &[(WB_RELS, MADE_UP), (SHEET_RELS, link)], &[MADE_UP_PART]);
        assert_eq!(package_of(&source), ["lost relationship madeUp"]);
    }

    #[test]
    fn comments_without_a_comment_are_not_a_loss() {
        let rels = r#"<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../comments1.xml"/><Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/>"#;
        let comments = r#"<comments xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><authors><author>A</author></authors><commentList/></comments>"#;
        let vml = r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office"><o:shapelayout><o:idmap data="1"/></o:shapelayout><v:shapetype id="_x0000_t202" path="m,l,21600r21600,l21600,xe"><v:path gradientshapeok="t"/></v:shapetype></xml>"#;
        let source = with_rels(
            &plain(),
            &[(WB_RELS, MADE_UP), (SHEET_RELS, rels)],
            &[MADE_UP_PART, ("xl/comments1.xml", comments), ("xl/drawings/vmlDrawing1.vml", vml)],
        );
        assert_eq!(package_of(&source), ["lost relationship madeUp"]);
    }

    #[test]
    fn empty_drawings_and_person_lists_are_not_a_loss() {
        let drawing = r#"<Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/>"#;
        let person =
            r#"<Relationship Id="rId92" Type="http://schemas.microsoft.com/office/2017/10/relationships/person" Target="persons/person.xml"/>"#;
        let source = with_rels(
            &plain(),
            &[(WB_RELS, &format!("{MADE_UP}{person}")), (SHEET_RELS, drawing)],
            &[
                MADE_UP_PART,
                ("xl/drawings/drawing1.xml", r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"/>"#),
                ("xl/persons/person.xml", r#"<personList xmlns="http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments"/>"#),
            ],
        );
        assert_eq!(package_of(&source), ["lost relationship madeUp"]);
    }

    #[test]
    fn unopenable_files_are_reported() {
        let r = check_bytes("broken.xlsx", b"PK\x03\x04 not really", &Options::default());
        assert!(r.error.is_some());
        assert!(!r.is_clean());
    }

    #[test]
    fn json_schema_keys() {
        let wb = book(&[("A1", "=1+1", n(3.0))]);
        let reports = vec![check(&wb, &Options::default()), check_bytes("bad.xlsx", b"", &Options::default())];
        let v = report_json(&reports, &Options::default());
        let keys = |v: &Json| v.as_object().unwrap().keys().cloned().collect::<BTreeSet<String>>();
        let set = |k: &[&str]| k.iter().map(|s| s.to_string()).collect::<BTreeSet<String>>();
        assert_eq!(keys(&v), set(&["schema", "tolerance", "files", "totals", "functions", "signatures"]));
        assert_eq!(v["schema"], SCHEMA);
        let f = &v["files"][0];
        assert_eq!(keys(f), set(&["path", "opened", "error", "warnings", "calc", "roundTrip"]));
        assert_eq!(
            keys(&f["calc"]),
            set(&[
                "formulaCells",
                "checked",
                "matched",
                "mismatched",
                "noOracle",
                "staleCache",
                "volatile",
                "boolAsNumber",
                "mismatches",
                "functions",
                "signatures"
            ])
        );
        assert_eq!(f["calc"]["signatures"], json!({"number → number": 1}));
        assert_eq!(keys(&f["calc"]["mismatches"][0]), set(&["cell", "formula", "expected", "actual", "kind", "functions", "primary"]));
        assert_eq!(f["calc"]["mismatches"][0]["expected"], json!(3.0));
        assert_eq!(keys(&f["roundTrip"]), set(&["error", "differing", "categories"]));
        assert_eq!(
            keys(&f["roundTrip"]["categories"]),
            set(&[
                "sheets",
                "values",
                "formulas",
                "numFmts",
                "merges",
                "names",
                "tables",
                "condFormats",
                "validations",
                "charts",
                "comments",
                "hyperlinks",
                "pivots",
                "package"
            ])
        );
        assert_eq!(keys(&f["roundTrip"]["categories"]["values"]), set(&["equal", "diffs", "examples"]));
        assert_eq!(v["files"][1]["opened"], json!(false));
        assert_eq!(
            keys(&v["totals"]),
            set(&[
                "files",
                "opened",
                "failedToOpen",
                "formulaCells",
                "checked",
                "matched",
                "mismatched",
                "noOracle",
                "volatile",
                "boolAsNumber",
                "roundTripFailed",
                "roundTripDiffering",
                "categoriesDiffering"
            ])
        );
        assert_eq!(keys(&v["functions"][0]), set(&["name", "checked", "mismatched", "primary"]));
    }
}

#[cfg(test)]
mod comment_tests;
