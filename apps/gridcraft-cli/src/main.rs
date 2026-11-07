//! Headless GridCraft.
//!
//! ```text
//! gridcraft-cli info <file> [--json]
//! gridcraft-cli convert <in> <out> [--sheet NAME] [--allow-lossy]
//! gridcraft-cli eval <formula> [--in FILE] [--sheet S] [--cell A1] [--json]
//! gridcraft-cli cat <file> [--range A1:D20] [--sheet S] [--formulas] [--csv]
//! gridcraft-cli check <file-or-dir>... [--json] [--tolerance 1e-9] [--max-report N]
//! gridcraft-cli run [--in FILE | --sample NAME] [--cmd 'id={json}']... [--script FILE.jsonl] [--out FILE] [--print RANGE] [--quiet] [--allow-lossy]
//! gridcraft-cli commands [--json] [--search X]
//! gridcraft-cli functions [--json] [--search X] [--category C]
//! gridcraft-cli mcp [--connect PORT] [--in FILE | --sample NAME]
//! gridcraft-cli send <port> <method> [json]
//! gridcraft-cli version | --version
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::process::ExitCode;

/// `println!` that ends the program quietly when stdout is closed (`gridcraft-cli commands |
/// head`) instead of panicking on a broken pipe.
macro_rules! outln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
            $crate::stdout_failed(e);
        }
    }};
}

mod check;

use gridcraft_engine::Session;
use serde_json::{Value, json};

const USAGE: &str = "usage:
  gridcraft-cli info <file> [--json]                      sheets, used ranges, tables, charts, names
  gridcraft-cli convert <in> <out> [--sheet NAME] [--allow-lossy]
                     xlsx/csv/tsv/json/html/pdf by extension; exit 1 if the import
                     dropped or altered content, unless --allow-lossy
  gridcraft-cli eval <formula> [--in FILE] [--sheet S] [--cell A1] [--json]
  gridcraft-cli cat <file> [--range A1:D20] [--sheet S] [--formulas] [--csv]
  gridcraft-cli check <file-or-dir>... [--json] [--tolerance 1e-9] [--max-report N]
                     cached-value calc oracle + XLSX round-trip fidelity (exit 1 on any difference)
  gridcraft-cli run [--in FILE | --sample NAME] [--cmd 'id={json}']... [--script FILE.jsonl]
                     [--out FILE] [--print RANGE] [--quiet] [--allow-lossy]
                     with --out, exit 1 if any counted import (the --in file, or
                     one opened by a file.open command) dropped or altered
                     content, unless --allow-lossy
                     a script line is id={json} or {\"command\": \"id\", \"params\": {…}};
                     blank lines and lines starting with # or // are skipped
  gridcraft-cli commands [--json] [--search X]
  gridcraft-cli functions [--json] [--search X] [--category C]
  gridcraft-cli mcp [--connect PORT] [--in FILE | --sample NAME]
  gridcraft-cli send <port> <method> [json]
  gridcraft-cli version";

/// stdout went away. A reader that stopped early (a closed pipe) ends the program quietly; any
/// other write error is reported.
fn stdout_failed(e: std::io::Error) -> ! {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        std::process::exit(0);
    }
    eprintln!("gridcraft-cli: can't write to stdout: {e}");
    std::process::exit(1);
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let r = match args.first().map(String::as_str) {
        Some("--version" | "-V" | "version") => {
            outln!("gridcraft-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h" | "help") => {
            outln!("{USAGE}");
            Ok(())
        }
        Some("info") => info(rest),
        Some("convert") => convert(rest),
        Some("eval") => eval(rest),
        Some("cat") => cat(rest),
        Some("run") => run(rest),
        Some("check") => return check::main(rest),
        Some("commands") => commands(rest),
        Some("functions") => functions(rest),
        Some("mcp") => mcp(rest),
        Some("send") => send(rest),
        Some(other) => Err(format!("unknown subcommand `{other}`\n{USAGE}")),
        None => Err(USAGE.to_string()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gridcraft-cli: {e}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------- argument parsing

/// Parsed arguments: positionals, `--flag value` options and boolean `--switch`es.
struct Args {
    positional: Vec<String>,
    opts: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Args {
    /// `with_value` lists the options that take a value; `switches` the boolean ones. Anything
    /// else starting with `--` is an error.
    fn parse(args: &[String], with_value: &[&str], switches: &[&str]) -> Result<Args, String> {
        let mut out = Args { positional: vec![], opts: vec![], switches: vec![] };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--").filter(|n| !n.is_empty()) {
                let (name, inline) = match name.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (name, None),
                };
                if with_value.contains(&name) {
                    let v = match inline {
                        Some(v) => v,
                        None => it.next().cloned().ok_or_else(|| format!("--{name} needs a value"))?,
                    };
                    out.opts.push((name.to_string(), v));
                } else if switches.contains(&name) && inline.is_none() {
                    out.switches.push(name.to_string());
                } else {
                    return Err(format!("unknown option --{name}"));
                }
            } else {
                out.positional.push(a.clone());
            }
        }
        Ok(out)
    }
    fn opt(&self, name: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
    fn has(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
    fn pos(&self, i: usize, what: &str) -> Result<&str, String> {
        self.positional.get(i).map(String::as_str).ok_or_else(|| format!("missing {what}\n{USAGE}"))
    }
    fn no_extra(&self, n: usize) -> Result<(), String> {
        match self.positional.get(n) {
            Some(x) => Err(format!("unexpected argument `{x}`")),
            None => Ok(()),
        }
    }
}

// ---------------------------------------------------------------- helpers

fn exec(s: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    let r = s.execute_headless(id, params).map_err(|e| e.to_string());
    if id == "file.open"
        && let Ok(value) = &r
        && let Some(warnings) = value.get("warnings").and_then(Value::as_array)
    {
        for warning in warnings.iter().filter_map(Value::as_str) {
            eprintln!("Warning: {warning}");
        }
    }
    r
}

/// Opens `path` and returns the session with the number of import warnings that report lost or
/// altered content: every `file.open` warning except the purely informational
/// [`gridcraft_engine::io::DECRYPTED_NOTICE`] (the rule `file.open` shows warnings by).
fn open(path: &str) -> Result<(Session, usize), String> {
    let mut s = Session::new();
    let r = exec(&mut s, "file.open", json!({"path": path}))?;
    Ok((s, lossy_warnings(&r)))
}

/// The lossy warnings in a `file.open` result: all of them except the informational
/// [`gridcraft_engine::io::DECRYPTED_NOTICE`].
fn lossy_warnings(r: &Value) -> usize {
    r.get("warnings").and_then(Value::as_array).map_or(0, |w| w.iter().filter(|w| w.as_str() != Some(gridcraft_engine::io::DECRYPTED_NOTICE)).count())
}

/// A lossy import fails the command unless `--allow-lossy` accepts it. The output is already
/// written; only the exit status says it may be incomplete (#3353).
fn lossy_check(input: &str, lossy: usize, allow: bool) -> Result<(), String> {
    if lossy > 0 && !allow {
        return Err(format!(
            "{input}: the import dropped or altered content ({lossy} import warning(s) above); the output may be incomplete — re-run with --allow-lossy to accept it"
        ));
    }
    Ok(())
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn activate_sheet(s: &mut Session, sheet: Option<&str>) -> Result<(), String> {
    if let Some(name) = sheet {
        let p = match name.parse::<u64>() {
            // A number that isn't also a sheet name is an index.
            Ok(i) if s.doc().is_ok_and(|d| d.wb.sheet_index(name).is_none()) => json!({"sheet": i}),
            _ => json!({"sheet": name}),
        };
        exec(s, "sheet.activate", p)?;
    }
    Ok(())
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// A number as a person would type it: `6`, `0.5`, `1e+300`.
fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else if n.is_finite() && (n.abs() >= 1e15 || n.abs() < 1e-5) {
        // Rust's `Display` never uses exponent form; `{:e}` omits the `+` on positive exponents.
        let s = format!("{n:e}");
        match s.split_once('e') {
            Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
            _ => s,
        }
    } else {
        format!("{n}")
    }
}

/// A computed value (as returned by `formulas.evaluate` / `sheet.read`) as plain text.
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Number(n) => n.as_f64().map(number_text).unwrap_or_else(|| n.to_string()),
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("error").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| v.to_string()),
        Value::Array(rows) => rows
            .iter()
            .map(|r| match r {
                Value::Array(cells) => cells.iter().map(value_text).collect::<Vec<_>>().join("\t"),
                other => value_text(other),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn csv_field(s: &str, delim: char) -> String {
    if s.contains(delim) || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------- subcommands

fn info(args: &[String]) -> Result<(), String> {
    let a = Args::parse(args, &[], &["json"])?;
    let path = a.pos(0, "<file>")?;
    a.no_extra(1)?;
    let (mut s, _) = open(path)?;
    let v = exec(&mut s, "document.inspect", json!({}))?;
    if a.has("json") {
        outln!("{}", pretty(&v));
        return Ok(());
    }
    outln!("{}", v["title"].as_str().unwrap_or(path));
    for (i, sh) in v["sheets"].as_array().into_iter().flatten().enumerate() {
        let active = if v["activeSheet"].as_u64() == Some(i as u64) { "*" } else { " " };
        let vis = sh["visibility"].as_str().filter(|x| *x != "Visible").map(|x| format!(" ({x})")).unwrap_or_default();
        outln!(
            "{active} {}{vis}: used {}, {} cells",
            sh["name"].as_str().unwrap_or("?"),
            sh["usedRange"].as_str().unwrap_or("(empty)"),
            sh["cells"].as_u64().unwrap_or(0)
        );
        for t in sh["tables"].as_array().into_iter().flatten() {
            outln!("    table {} {} ({})", t["name"].as_str().unwrap_or("?"), t["range"].as_str().unwrap_or("?"), t["style"].as_str().unwrap_or(""));
        }
        for c in sh["charts"].as_array().into_iter().flatten() {
            let title = c["title"].as_str().map(|t| format!(" \"{t}\"")).unwrap_or_default();
            outln!("    chart {}{title} at {}, {} series", c["kind"].as_str().unwrap_or("?"), c["at"].as_str().unwrap_or("?"), c["series"]);
        }
        if let Some(f) = sh["autofilter"].as_str() {
            outln!("    filter {f}");
        }
    }
    let names = v["names"].as_array().cloned().unwrap_or_default();
    if !names.is_empty() {
        outln!("names:");
        for n in names {
            outln!("  {} {}", n["name"].as_str().unwrap_or("?"), n["refersTo"].as_str().unwrap_or(""));
        }
    }
    Ok(())
}

fn convert(args: &[String]) -> Result<(), String> {
    let a = Args::parse(args, &["sheet"], &["allow-lossy"])?;
    let input = a.pos(0, "<in>")?;
    let out = a.pos(1, "<out>")?;
    a.no_extra(2)?;
    let pdf = out.to_ascii_lowercase().ends_with(".pdf");
    if !pdf && gridcraft_engine::io::FileKind::from_path(out).is_none() {
        return Err(format!("{out}: unknown output format (use .xlsx, .csv, .tsv, .json, .html or .pdf)"));
    }
    let (mut s, lossy) = open(input)?;
    activate_sheet(&mut s, a.opt("sheet"))?;
    if pdf {
        let result = exec(&mut s, "file.exportPdf", json!({"path": out}))?;
        eprintln!("wrote {out} ({} bytes, {} pages)", result["bytes"], result["pages"]);
    } else {
        let d = s.doc().map_err(|e| e.to_string())?;
        let bytes = gridcraft_engine::io::save_file_bytes(&d.wb, out).map_err(|e| e.to_string())?;
        gridcraft_engine::io::write_file(out, &bytes).map_err(|e| e.to_string())?;
        eprintln!("wrote {out} ({} bytes)", bytes.len());
    }
    lossy_check(input, lossy, a.has("allow-lossy"))
}

fn eval(args: &[String]) -> Result<(), String> {
    let a = Args::parse(args, &["in", "sheet", "cell"], &["json"])?;
    let formula = a.pos(0, "<formula>")?;
    a.no_extra(1)?;
    let mut s = match a.opt("in") {
        Some(p) => open(p)?.0,
        None => blank(),
    };
    activate_sheet(&mut s, a.opt("sheet"))?;
    let formula = if formula.starts_with('=') { formula.to_string() } else { format!("={formula}") };
    let mut p = json!({"formula": formula});
    if let Some(c) = a.opt("cell") {
        p["cell"] = json!(c);
    }
    let v = exec(&mut s, "formulas.evaluate", p)?;
    if a.has("json") {
        outln!("{}", serde_json::to_string(&v).unwrap_or_default());
    } else {
        outln!("{}", value_text(&v));
    }
    Ok(())
}

/// Prints `range` of the active (or `sheet`) sheet as an aligned table or CSV.
fn print_range(s: &mut Session, range: Option<&str>, sheet: Option<&str>, formulas: bool, csv: bool) -> Result<(), String> {
    let mut p = json!({});
    if let Some(r) = range {
        p["range"] = json!(r);
    }
    if let Some(sh) = sheet {
        p["sheet"] = json!(sh);
    }
    let raw = exec(s, "sheet.read", p.clone())?;
    p["formatted"] = json!(true);
    p["formulas"] = json!(formulas);
    let shown = exec(s, "sheet.read", p)?;
    let rows: Vec<Vec<String>> = shown["values"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| r.as_array().into_iter().flatten().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| value_text(c))).collect())
        .collect();
    if csv {
        for r in &rows {
            outln!("{}", r.iter().map(|c| csv_field(c, ',')).collect::<Vec<_>>().join(","));
        }
        return Ok(());
    }
    let numeric = |ri: usize, ci: usize| raw["values"][ri][ci].is_number() && !formulas;
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..ncols).map(|c| rows.iter().filter_map(|r| r.get(c)).map(|t| t.chars().count()).max().unwrap_or(0).min(60)).collect();
    for (ri, r) in rows.iter().enumerate() {
        let mut line = String::new();
        for (ci, cell) in r.iter().enumerate() {
            let w = widths.get(ci).copied().unwrap_or(0);
            let cell: String = cell.chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).collect();
            if ci > 0 {
                line.push_str("  ");
            }
            if numeric(ri, ci) {
                line.push_str(&format!("{cell:>w$}"));
            } else {
                line.push_str(&format!("{cell:<w$}"));
            }
        }
        outln!("{}", line.trim_end());
    }
    Ok(())
}

fn cat(args: &[String]) -> Result<(), String> {
    let a = Args::parse(args, &["range", "sheet"], &["formulas", "csv"])?;
    let path = a.pos(0, "<file>")?;
    a.no_extra(1)?;
    let (mut s, _) = open(path)?;
    print_range(&mut s, a.opt("range"), a.opt("sheet"), a.has("formulas"), a.has("csv"))
}

/// One command from `id={json}`, `id {json}`, `id` or `{"command": id, "params": {..}}`.
fn parse_command(text: &str) -> Result<Option<(String, Value)>, String> {
    let t = text.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
        return Ok(None);
    }
    if t.starts_with('{') {
        let v: Value = serde_json::from_str(t).map_err(|e| format!("bad JSON: {e}"))?;
        // A key this does not read is a mistake, not a comment: `{"command": …, "args": {…}}` and
        // `{"command": …, "name": "Report"}` ran the command with no parameters at all and the
        // script still exited 0, so a typo made a valid, wrong workbook (#2256).
        if let Some(o) = v.as_object() {
            let mut unknown: Vec<&str> = o.keys().map(String::as_str).filter(|k| !matches!(*k, "command" | "id" | "params")).collect();
            if !unknown.is_empty() {
                unknown.sort_unstable();
                let (plural, list) = (if unknown.len() == 1 { "key" } else { "keys" }, unknown.join("`, `"));
                return Err(format!("unknown {plural} `{list}`: a line is {{\"command\": \"id\", \"params\": {{…}}}}"));
            }
        }
        let id = v.get("command").or_else(|| v.get("id")).and_then(Value::as_str).ok_or("JSON command needs a `command` field")?;
        return Ok(Some((id.to_string(), v.get("params").cloned().unwrap_or_else(|| json!({})))));
    }
    let split = t.find(['=', ' ', '\t']);
    let (id, rest) = match split {
        Some(i) => (t.get(..i).unwrap_or(t), t.get(i + 1..).unwrap_or("").trim()),
        None => (t, ""),
    };
    let params = if rest.is_empty() { json!({}) } else { serde_json::from_str(rest).map_err(|e| format!("{id}: bad JSON params: {e}"))? };
    Ok(Some((id.to_string(), params)))
}

fn run(args: &[String]) -> Result<(), String> {
    let a = Args::parse(args, &["in", "sample", "cmd", "script", "out", "print", "sheet"], &["quiet", "csv", "formulas", "allow-lossy"])?;
    a.no_extra(0)?;
    let (mut s, mut lossy) = match (a.opt("in"), a.opt("sample")) {
        (Some(_), Some(_)) => return Err("give --in or --sample, not both".into()),
        (Some(p), None) => open(p)?,
        (None, Some(name)) => {
            let mut s = Session::new();
            exec(&mut s, "file.new", json!({"sample": name}))?;
            (s, 0)
        }
        (None, None) => (blank(), 0),
    };
    let mut lossy_from = a.opt("in").unwrap_or_default().to_string();
    let quiet = a.has("quiet");
    let mut step = 0usize;
    for (name, value) in &a.opts {
        let cmds: Vec<(String, Value, String)> = match name.as_str() {
            "cmd" => parse_command(value)?.map(|(id, p)| vec![(id, p, "--cmd".to_string())]).unwrap_or_default(),
            "script" => {
                let text = std::fs::read_to_string(value).map_err(|e| format!("{value}: {e}"))?;
                let mut v = Vec::new();
                for (i, line) in text.lines().enumerate() {
                    if let Some((id, p)) = parse_command(line).map_err(|e| format!("{value}:{}: {e}", i + 1))? {
                        v.push((id, p, format!("{value}:{}", i + 1)));
                    }
                }
                v
            }
            _ => continue,
        };
        for (id, p, origin) in cmds {
            step += 1;
            let open_path = if id == "file.open" { p.get("path").and_then(Value::as_str).map(str::to_string) } else { None };
            let r = exec(&mut s, &id, p).map_err(|e| format!("step {step} ({origin}, {id}): {e}"))?;
            if open_path.is_some() || id == "file.open" {
                let counted = lossy_warnings(&r);
                lossy += counted;
                // Only a counted warning moves the blame: a later clean open must not be
                // named as the import that dropped content.
                if counted > 0
                    && let Some(path) = open_path
                {
                    lossy_from = path;
                }
            }
            if !quiet && !r.is_null() {
                outln!("{}", serde_json::to_string(&json!({"command": id, "result": r})).unwrap_or_default());
            }
        }
    }
    // A saved workbook from a lossy import fails the run; the `--print`s still run first, so the
    // failure only changes the exit status.
    let mut lossy_out = Ok(());
    if let Some(out) = a.opt("out") {
        let r = exec(&mut s, "file.save", json!({"path": out}))?;
        eprintln!("wrote {out} ({} bytes)", r["bytes"]);
        lossy_out = lossy_check(&lossy_from, lossy, a.has("allow-lossy"));
    }
    for (name, range) in &a.opts {
        if name == "print" {
            let range = if range.is_empty() || range == "used" { None } else { Some(range.as_str()) };
            print_range(&mut s, range, a.opt("sheet"), a.has("formulas"), a.has("csv"))?;
        }
    }
    lossy_out
}

fn commands(args: &[String]) -> Result<(), String> {
    let a = Args::parse(args, &["search"], &["json"])?;
    let q = a.opt("search").or(a.positional.first().map(String::as_str)).map(str::to_ascii_lowercase);
    let s = Session::new();
    let list: Vec<_> = s
        .commands()
        .into_iter()
        .filter(|c| {
            q.as_ref().is_none_or(|q| {
                c.id.to_ascii_lowercase().contains(q.as_str())
                    || c.label.to_ascii_lowercase().contains(q.as_str())
                    || c.menu.iter().any(|m| m.to_ascii_lowercase().contains(q.as_str()))
            })
        })
        .collect();
    if a.has("json") {
        // Enablement depends on a live workbook; not meaningful here.
        let v: Vec<Value> =
            list.iter().map(|c| json!({"id": c.id, "label": c.label, "menu": c.menu, "shortcut": c.shortcut, "params": c.params})).collect();
        outln!("{}", pretty(&Value::Array(v)));
        return Ok(());
    }
    let w = list.iter().map(|c| c.id.len()).max().unwrap_or(0);
    for c in &list {
        let sc = c.shortcut.map(|s| format!(" [{s}]")).unwrap_or_default();
        let menu = if c.menu.is_empty() { String::new() } else { format!(" ({})", c.menu.join(" > ")) };
        outln!("{:<w$}  {}{menu}{sc}", c.id, c.label.trim_end_matches('…'));
        outln!("{:<w$}    {}", "", c.params);
    }
    eprintln!("{} commands", list.len());
    Ok(())
}

fn functions(args: &[String]) -> Result<(), String> {
    let a = Args::parse(args, &["search", "category"], &["json"])?;
    let mut p = json!({});
    if let Some(q) = a.opt("search").or(a.positional.first().map(String::as_str)) {
        p["search"] = json!(q);
    }
    if let Some(c) = a.opt("category") {
        p["category"] = json!(c);
    }
    let mut s = Session::new();
    let v = exec(&mut s, "formulas.functions", p)?;
    if a.has("json") {
        outln!("{}", pretty(&v));
        return Ok(());
    }
    let list = v.as_array().cloned().unwrap_or_default();
    for f in &list {
        let sig = f["signature"].as_str().or(f["name"].as_str()).unwrap_or("?");
        outln!("{sig}");
        let cat = f["category"].as_str().unwrap_or("");
        let desc = f["description"].as_str().unwrap_or("");
        outln!("    {cat}: {desc}");
    }
    eprintln!("{} functions", list.len());
    Ok(())
}

/// `mcp` (headless, in-process engine) or `mcp --connect PORT|HOST:PORT` (drive a running app
/// started with `gridcraft --control PORT`). JSON-RPC on stdin/stdout; logs on stderr.
fn mcp(args: &[String]) -> Result<(), String> {
    use gridcraft_mcp::{Backend, Headless, Remote, Server, control_addr};
    let a = Args::parse(args, &["connect", "in", "sample"], &[])?;
    a.no_extra(0)?;
    let backend: Box<dyn Backend> = match a.opt("connect") {
        Some(c) => {
            let addr = control_addr(c);
            Box::new(
                Remote::connect(&addr)
                    .map_err(|e| format!("cannot connect to the GridCraft app at {addr}: {e} (start it with `gridcraft --control PORT`)"))?,
            )
        }
        None => {
            let session = match (a.opt("in"), a.opt("sample")) {
                (Some(p), _) => open(p)?.0,
                (None, Some(name)) => {
                    let mut s = Session::new();
                    exec(&mut s, "file.new", json!({"sample": name}))?;
                    s
                }
                (None, None) => blank(),
            };
            Box::new(Headless::with_session(session))
        }
    };
    eprintln!("gridcraft-cli: MCP server on stdio ({})", backend.describe());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(|e| e.to_string())
}

/// `send PORT METHOD [JSON]`: one control-channel request to a running app.
fn send(args: &[String]) -> Result<(), String> {
    use gridcraft_mcp::{Backend, Remote, control_addr};
    let port = args.first().ok_or_else(|| format!("missing <port>\n{USAGE}"))?;
    let method = args.get(1).ok_or_else(|| format!("missing <method>\n{USAGE}"))?;
    let params: Value = match args.get(2) {
        Some(t) => serde_json::from_str(t).map_err(|e| format!("bad JSON params: {e}"))?,
        None => json!({}),
    };
    if let Some(x) = args.get(3) {
        return Err(format!("unexpected argument `{x}`"));
    }
    let addr = control_addr(port);
    let mut r = Remote::connect(&addr)
        .map_err(|e| format!("cannot connect to the GridCraft app at {addr}: {e} (start it with `gridcraft --control {port}`)"))?;
    let v = r.call(method, params)?;
    outln!("{}", pretty(&v));
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("home.bold={\"range\":\"A1\"}").unwrap(), Some(("home.bold".into(), json!({"range": "A1"}))));
        assert_eq!(parse_command("edit.undo").unwrap(), Some(("edit.undo".into(), json!({}))));
        assert_eq!(
            parse_command("cell.set {\"cell\":\"A1\",\"input\":\"=1\"}").unwrap(),
            Some(("cell.set".into(), json!({"cell": "A1", "input": "=1"})))
        );
        assert_eq!(parse_command("{\"command\":\"edit.redo\"}").unwrap(), Some(("edit.redo".into(), json!({}))));
        assert_eq!(parse_command("  # comment").unwrap(), None);
        assert!(parse_command("x={bad").is_err());
        // A key this does not read is a mistake: the command used to run with no parameters and
        // the script still exited 0, so a typo made a valid, wrong workbook (#2256).
        for (line, named) in [
            (r#"{"command":"home.insertSheet","args":{"name":"R"}}"#, "`args`"),
            (r#"{"command":"home.insertSheet","name":"R"}"#, "`name`"),
            (r#"{"command":"home.insertSheet","params":{},"note":"why"}"#, "`note`"),
        ] {
            let e = parse_command(line).expect_err("refused");
            assert!(e.contains("unknown key") && e.contains(named), "{line}: {e}");
            assert!(e.contains(r#"{"command": "id", "params": {…}}"#), "{line}: {e}");
        }
        // Several at once are listed in one message, sorted.
        let e = parse_command(r#"{"command":"x","name":"R","args":{}}"#).expect_err("refused");
        assert!(e.contains("unknown keys `args`, `name`"), "{e}");
        // `id` is still the accepted alternative to `command`, and `params` still carries them.
        assert_eq!(parse_command(r#"{"id":"edit.undo"}"#).unwrap(), Some(("edit.undo".into(), json!({}))));
        assert_eq!(
            parse_command(r#"{"command":"home.insertSheet","params":{"name":"R"}}"#).unwrap(),
            Some(("home.insertSheet".into(), json!({"name": "R"})))
        );
    }

    #[test]
    fn value_texts() {
        assert_eq!(value_text(&json!(6.0)), "6");
        assert_eq!(value_text(&json!(0.25)), "0.25");
        assert_eq!(value_text(&json!(1e300)), "1e+300");
        assert_eq!(value_text(&json!(-1.5e300)), "-1.5e+300");
        assert_eq!(value_text(&json!(1e-20)), "1e-20");
        assert_eq!(value_text(&json!(1e-300)), "1e-300");
        assert_eq!(value_text(&json!(0.0001)), "0.0001");
        assert_eq!(value_text(&json!(true)), "TRUE");
        assert_eq!(value_text(&json!({"error": "#DIV/0!"})), "#DIV/0!");
        assert_eq!(value_text(&json!([[1.0, 2.0], [3.0, "x"]])), "1\t2\n3\tx");
    }

    #[test]
    fn args_parsing() {
        let v: Vec<String> = ["a", "--range", "A1", "--csv", "--sheet=S"].iter().map(|s| s.to_string()).collect();
        let a = Args::parse(&v, &["range", "sheet"], &["csv"]).unwrap();
        assert_eq!(a.positional, ["a"]);
        assert_eq!(a.opt("range"), Some("A1"));
        assert_eq!(a.opt("sheet"), Some("S"));
        assert!(a.has("csv"));
        assert!(Args::parse(&["--nope".to_string()], &[], &[]).is_err());
        assert!(Args::parse(&["--range".to_string()], &["range"], &[]).is_err());
    }

    #[test]
    fn convert_xlsx_to_pdf_keeps_links() {
        let dir = std::env::temp_dir().join(format!("gridcraft-cli-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("links.xlsx");
        let output = dir.join("links.pdf");
        let mut s = blank();
        s.execute("insert.link", json!({"cell": "A1", "target": "https://example.org"})).unwrap();
        let bytes = gridcraft_engine::io::save_file_bytes(&s.doc().unwrap().wb, input.to_str().unwrap()).unwrap();
        gridcraft_engine::io::write_file(input.to_str().unwrap(), &bytes).unwrap();
        convert(&[input.to_string_lossy().into_owned(), output.to_string_lossy().into_owned()]).unwrap();
        let pdf = std::fs::read(&output).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.windows(b"/URI (https://example.org)".len()).any(|w| w == b"/URI (https://example.org)"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A fresh per-test temp dir holding a CSV whose every date is ambiguous (day or month
    /// first), so the import guesses the order and warns about it.
    fn warned_csv(test: &str) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("gridcraft-cli-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("dates.csv");
        std::fs::write(&input, "when,amount\n03/04/2026,1\n05/06/2026,2\n07/08/2026,3\n").unwrap();
        let input = input.to_string_lossy().into_owned();
        (dir, input)
    }

    fn path(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn convert_fails_when_the_import_warns() {
        // A lossy import used to write the output and exit 0, so a script shipped a cut-off
        // workbook as a success (#3353).
        let (dir, input) = warned_csv("lossy");
        let (_, lossy) = open(&input).unwrap();
        assert!(lossy > 0, "the fixture must produce an import warning");
        let output = dir.join("dates.xlsx");
        let e = convert(&[input.clone(), path(&output)]).expect_err("a lossy import fails");
        assert!(e.contains(&input) && e.contains("--allow-lossy") && e.contains(&format!("({lossy} import warning(s) above)")), "{e}");
        // The output is still written; only the exit status changes.
        assert!(std::fs::read(&output).unwrap().starts_with(b"PK"));
        // The PDF path, which returns separately, is covered too.
        let pdf = dir.join("dates.pdf");
        assert!(convert(&[input.clone(), path(&pdf)]).is_err());
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn convert_allow_lossy_accepts_a_warned_import() {
        let (dir, input) = warned_csv("allow-lossy");
        let output = dir.join("dates.xlsx");
        convert(&[input.clone(), path(&output), "--allow-lossy".into()]).unwrap();
        assert!(std::fs::read(&output).unwrap().starts_with(b"PK"));
        let pdf = dir.join("dates.pdf");
        convert(&[input, path(&pdf), "--allow-lossy".into()]).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn convert_succeeds_on_a_clean_import() {
        let dir = std::env::temp_dir().join(format!("gridcraft-cli-clean-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("clean.xlsx");
        let output = dir.join("copy.xlsx");
        let mut s = blank();
        s.execute("cell.set", json!({"cell": "A1", "input": "42"})).unwrap();
        let bytes = gridcraft_engine::io::save_file_bytes(&s.doc().unwrap().wb, &path(&input)).unwrap();
        gridcraft_engine::io::write_file(&path(&input), &bytes).unwrap();
        assert_eq!(open(&path(&input)).unwrap().1, 0);
        convert(&[path(&input), path(&output)]).unwrap();
        let (mut s, lossy) = open(&path(&output)).unwrap();
        assert_eq!(lossy, 0);
        assert_eq!(value_text(&exec(&mut s, "sheet.read", json!({"range": "A1"})).unwrap()["values"]), "42");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn run_fails_when_a_command_opens_a_warned_import() {
        // A file.open issued as a command replaces the workbook; its warnings count too, or
        // the save that follows ships the lossy content with a success status.
        let (dir, input) = warned_csv("run-cmd-open");
        let output = dir.join("dates.xlsx");
        let open_cmd = format!("file.open={}", json!({"path": input}));
        let e = run(&vec!["--quiet".into(), "--cmd".into(), open_cmd.clone(), "--out".into(), path(&output)])
            .expect_err("a warned import opened by a command fails the run");
        assert!(e.contains("--allow-lossy"), "{e}");
        assert!(std::fs::read(&output).unwrap().starts_with(b"PK"));
        run(&vec!["--quiet".into(), "--cmd".into(), open_cmd, "--out".into(), path(&output), "--allow-lossy".into()]).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn run_blames_the_lossy_import_not_a_later_clean_open() {
        // A clean file.open after a lossy import must not take the blame: the failure
        // names the import that actually dropped content.
        let (dir, input) = warned_csv("run-blame");
        let clean = dir.join("clean.xlsx");
        let mut s = blank();
        s.execute("cell.set", json!({"cell": "A1", "input": "42"})).unwrap();
        let bytes = gridcraft_engine::io::save_file_bytes(&s.doc().unwrap().wb, &path(&clean)).unwrap();
        gridcraft_engine::io::write_file(&path(&clean), &bytes).unwrap();
        assert_eq!(open(&path(&clean)).unwrap().1, 0, "the second open must be clean");
        let output = dir.join("dates.xlsx");
        let open_cmd = format!("file.open={}", json!({"path": path(&clean)}));
        let e = run(&vec!["--in".into(), input.clone(), "--quiet".into(), "--cmd".into(), open_cmd, "--out".into(), path(&output)])
            .expect_err("the lossy --in import still fails the run");
        assert!(e.contains(&input), "{e}");
        assert!(!e.contains(&path(&clean)), "{e}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn run_fails_when_the_import_warns_and_writes_out() {
        let (dir, input) = warned_csv("run-lossy");
        let output = dir.join("dates.xlsx");
        let args = |extra: &[&str]| -> Vec<String> {
            let mut v: Vec<String> = vec!["--in".into(), input.clone(), "--quiet".into()];
            v.extend(extra.iter().map(|s| s.to_string()));
            v
        };
        let e = run(&args(&["--out", &path(&output)])).expect_err("a lossy import saved fails");
        assert!(e.contains("--allow-lossy"), "{e}");
        assert!(std::fs::read(&output).unwrap().starts_with(b"PK"));
        run(&args(&["--out", &path(&output), "--allow-lossy"])).unwrap();
        // Without --out nothing is shipped, so the run does not fail for the warning.
        run(&args(&["--cmd", r#"cell.set={"cell":"C1","input":"1"}"#])).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
