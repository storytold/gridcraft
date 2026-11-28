//! The subcommands: each takes its typed arguments and the streams, and returns what to exit with.

use gridcraft_engine::Session;
use serde_json::{Value, json};

use crate::args::{CatArgs, Command, ConvertArgs, EvalArgs, FunctionsArgs, InfoArgs, ListArgs, McpArgs, RunArgs, RunStep, SendArgs};
use crate::error::{CliError, unicode};
use crate::script::parse_command;
use crate::{Io, check, errln, outln, sandbox_check};

/// Run a parsed command line; gives the exit code (0, or 1 when `check` found a difference and said so itself).
pub(crate) fn dispatch(command: Command, io: &mut Io) -> Result<i32, CliError> {
    match command {
        Command::Info(a) => info(&a, io),
        Command::Convert(a) => convert(&a, io),
        Command::Eval(a) => eval(&a, io),
        Command::Cat(a) => cat(&a, io),
        Command::Run(a) => run(&a, io),
        Command::Commands(a) => commands(&a, io),
        Command::Functions(a) => functions(&a, io),
        Command::Mcp(a) => mcp(&a, io),
        Command::Send(a) => send(&a, io),
        Command::Check(a) => return check::run(&a, io),
        Command::SandboxCheck(a) => return sandbox_check::run(&a, io),
    }
    .map(|()| 0)
}

// ---------------------------------------------------------------- helpers

fn exec(io: &mut Io, s: &mut Session, id: &str, params: Value) -> Result<Value, CliError> {
    let r = s.execute_headless(id, params).map_err(CliError::engine);
    if id == "file.open"
        && let Ok(value) = &r
        && let Some(warnings) = value.get("warnings").and_then(Value::as_array)
    {
        for warning in warnings.iter().filter_map(Value::as_str) {
            errln!(io, "Warning: {warning}");
        }
    }
    r
}

fn open(io: &mut Io, path: &std::path::Path) -> Result<Session, CliError> {
    let mut s = Session::new();
    exec(io, &mut s, "file.open", json!({"path": unicode(path)?}))?;
    Ok(s)
}

fn sample(io: &mut Io, name: &str) -> Result<Session, CliError> {
    let mut s = Session::new();
    exec(io, &mut s, "file.new", json!({"sample": name}))?;
    Ok(s)
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn activate_sheet(io: &mut Io, s: &mut Session, sheet: Option<&str>) -> Result<(), CliError> {
    if let Some(name) = sheet {
        let p = match name.parse::<u64>() {
            // A number that isn't also a sheet name is an index.
            Ok(i) if s.doc().is_ok_and(|d| d.wb.sheet_index(name).is_none()) => json!({"sheet": i}),
            _ => json!({"sheet": name}),
        };
        exec(io, s, "sheet.activate", p)?;
    }
    Ok(())
}

pub(crate) fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// A number as a person would type it: `6`, `0.5`, `1e+300`.
pub(crate) fn number_text(n: f64) -> String {
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

fn info(a: &InfoArgs, io: &mut Io) -> Result<(), CliError> {
    let path = unicode(&a.file)?;
    let mut s = open(io, &a.file)?;
    let v = exec(io, &mut s, "document.inspect", json!({}))?;
    if a.json {
        outln!(io, "{}", pretty(&v));
        return Ok(());
    }
    outln!(io, "{}", v["title"].as_str().unwrap_or(path));
    for (i, sh) in v["sheets"].as_array().into_iter().flatten().enumerate() {
        let active = if v["activeSheet"].as_u64() == Some(i as u64) { "*" } else { " " };
        let vis = sh["visibility"].as_str().filter(|x| *x != "Visible").map(|x| format!(" ({x})")).unwrap_or_default();
        outln!(
            io,
            "{active} {}{vis}: used {}, {} cells",
            sh["name"].as_str().unwrap_or("?"),
            sh["usedRange"].as_str().unwrap_or("(empty)"),
            sh["cells"].as_u64().unwrap_or(0)
        );
        for t in sh["tables"].as_array().into_iter().flatten() {
            outln!(
                io,
                "    table {} {} ({})",
                t["name"].as_str().unwrap_or("?"),
                t["range"].as_str().unwrap_or("?"),
                t["style"].as_str().unwrap_or("")
            );
        }
        for c in sh["charts"].as_array().into_iter().flatten() {
            let title = c["title"].as_str().map(|t| format!(" \"{t}\"")).unwrap_or_default();
            outln!(io, "    chart {}{title} at {}, {} series", c["kind"].as_str().unwrap_or("?"), c["at"].as_str().unwrap_or("?"), c["series"]);
        }
        if let Some(f) = sh["autofilter"].as_str() {
            outln!(io, "    filter {f}");
        }
    }
    let names = v["names"].as_array().cloned().unwrap_or_default();
    if !names.is_empty() {
        outln!(io, "names:");
        for n in names {
            outln!(io, "  {} {}", n["name"].as_str().unwrap_or("?"), n["refersTo"].as_str().unwrap_or(""));
        }
    }
    Ok(())
}

fn convert(a: &ConvertArgs, io: &mut Io) -> Result<(), CliError> {
    let out = unicode(&a.output)?;
    let pdf = out.to_ascii_lowercase().ends_with(".pdf");
    if !pdf && gridcraft_engine::io::FileKind::from_path(out).is_none() {
        return Err(CliError::OutputFormat { out: out.to_owned() });
    }
    let mut s = open(io, &a.input)?;
    activate_sheet(io, &mut s, a.sheet.as_deref())?;
    if pdf {
        let result = exec(io, &mut s, "file.exportPdf", json!({"path": out}))?;
        errln!(io, "wrote {out} ({} bytes, {} pages)", result["bytes"], result["pages"]);
        return Ok(());
    }
    let d = s.doc().map_err(CliError::engine)?;
    let bytes = gridcraft_engine::io::save_file_bytes(&d.wb, out).map_err(CliError::engine)?;
    gridcraft_engine::io::write_file(out, &bytes).map_err(CliError::engine)?;
    errln!(io, "wrote {out} ({} bytes)", bytes.len());
    Ok(())
}

fn eval(a: &EvalArgs, io: &mut Io) -> Result<(), CliError> {
    let mut s = match &a.input {
        Some(p) => open(io, p)?,
        None => blank(),
    };
    activate_sheet(io, &mut s, a.sheet.as_deref())?;
    let formula = if a.formula.starts_with('=') { a.formula.clone() } else { format!("={}", a.formula) };
    let mut p = json!({"formula": formula});
    if let Some(c) = &a.cell {
        p["cell"] = json!(c);
    }
    let v = exec(io, &mut s, "formulas.evaluate", p)?;
    if a.json {
        outln!(io, "{}", serde_json::to_string(&v).unwrap_or_default());
    } else {
        outln!(io, "{}", value_text(&v));
    }
    Ok(())
}

/// Prints `range` of the active (or `sheet`) sheet as an aligned table or CSV.
fn print_range(io: &mut Io, s: &mut Session, range: Option<&str>, sheet: Option<&str>, formulas: bool, csv: bool) -> Result<(), CliError> {
    activate_sheet(io, s, sheet)?;
    let mut p = json!({});
    if let Some(r) = range {
        p["range"] = json!(r);
    }
    if let Some(sh) = sheet {
        p["sheet"] = json!(sh);
    }
    let raw = exec(io, s, "sheet.read", p.clone())?;
    p["formatted"] = json!(true);
    p["formulas"] = json!(formulas);
    let shown = exec(io, s, "sheet.read", p)?;
    let rows: Vec<Vec<String>> = shown["values"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| r.as_array().into_iter().flatten().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| value_text(c))).collect())
        .collect();
    if csv {
        for r in &rows {
            outln!(io, "{}", r.iter().map(|c| csv_field(c, ',')).collect::<Vec<_>>().join(","));
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
        outln!(io, "{}", line.trim_end());
    }
    Ok(())
}

fn cat(a: &CatArgs, io: &mut Io) -> Result<(), CliError> {
    let mut s = open(io, &a.file)?;
    print_range(io, &mut s, a.range.as_deref(), a.sheet.as_deref(), a.formulas, a.csv)
}

fn run(a: &RunArgs, io: &mut Io) -> Result<(), CliError> {
    let mut s = match (&a.input, &a.sample) {
        (Some(p), _) => open(io, p)?,
        (None, Some(name)) => sample(io, name)?,
        (None, None) => blank(),
    };
    let mut step = 0usize;
    for item in &a.steps {
        let cmds: Vec<(String, Value, String)> = match item {
            RunStep::Cmd(c) => c.iter().map(|(id, p)| (id.clone(), p.clone(), "--cmd".to_string())).collect(),
            RunStep::Script(path) => {
                let value = path.display().to_string();
                let text = std::fs::read_to_string(path).map_err(|source| CliError::File { path: path.clone(), source })?;
                let mut v = Vec::new();
                for (i, line) in text.lines().enumerate() {
                    let origin = format!("{value}:{}", i + 1);
                    if let Some((id, p)) = parse_command(line).map_err(|source| CliError::Script { origin: origin.clone(), source })? {
                        v.push((id, p, origin));
                    }
                }
                v
            }
        };
        for (id, p, origin) in cmds {
            step += 1;
            let r = exec(io, &mut s, &id, p).map_err(|e| CliError::Step { step, origin, id: id.clone(), message: e.to_string() })?;
            if !a.quiet && !r.is_null() {
                outln!(io, "{}", serde_json::to_string(&json!({"command": id, "result": r})).unwrap_or_default());
            }
        }
    }
    if let Some(out) = &a.out {
        let out = unicode(out)?;
        let r = exec(io, &mut s, "file.save", json!({"path": out}))?;
        errln!(io, "wrote {out} ({} bytes)", r["bytes"]);
    }
    for range in &a.print {
        let range = if range.is_empty() || range == "used" { None } else { Some(range.as_str()) };
        print_range(io, &mut s, range, a.sheet.as_deref(), a.formulas, a.csv)?;
    }
    Ok(())
}

fn commands(a: &ListArgs, io: &mut Io) -> Result<(), CliError> {
    let q = a.search.as_ref().or(a.word.as_ref()).map(|q| q.to_ascii_lowercase());
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
    if a.json {
        // Enablement depends on a live workbook; not meaningful here.
        let v: Vec<Value> =
            list.iter().map(|c| json!({"id": c.id, "label": c.label, "menu": c.menu, "shortcut": c.shortcut, "params": c.params})).collect();
        outln!(io, "{}", pretty(&Value::Array(v)));
        return Ok(());
    }
    let w = list.iter().map(|c| c.id.len()).max().unwrap_or(0);
    for c in &list {
        let sc = c.shortcut.map(|s| format!(" [{s}]")).unwrap_or_default();
        let menu = if c.menu.is_empty() { String::new() } else { format!(" ({})", c.menu.join(" > ")) };
        outln!(io, "{:<w$}  {}{menu}{sc}", c.id, c.label.trim_end_matches('…'));
        outln!(io, "{:<w$}    {}", "", c.params);
    }
    errln!(io, "{} commands", list.len());
    Ok(())
}

fn functions(a: &FunctionsArgs, io: &mut Io) -> Result<(), CliError> {
    let mut p = json!({});
    if let Some(q) = a.search.as_ref().or(a.word.as_ref()) {
        p["search"] = json!(q);
    }
    if let Some(c) = &a.category {
        p["category"] = json!(c);
    }
    let mut s = Session::new();
    let v = exec(io, &mut s, "formulas.functions", p)?;
    if a.json {
        outln!(io, "{}", pretty(&v));
        return Ok(());
    }
    let list = v.as_array().cloned().unwrap_or_default();
    for f in &list {
        let sig = f["signature"].as_str().or(f["name"].as_str()).unwrap_or("?");
        outln!(io, "{sig}");
        let cat = f["category"].as_str().unwrap_or("");
        let desc = f["description"].as_str().unwrap_or("");
        outln!(io, "    {cat}: {desc}");
    }
    errln!(io, "{} functions", list.len());
    Ok(())
}

fn connect(port: &str) -> Result<gridcraft_mcp::Remote, CliError> {
    use gridcraft_mcp::control_addr;
    let addr = control_addr(port);
    gridcraft_mcp::Remote::connect(&addr).map_err(|e| CliError::Connect { addr, port: port.to_owned(), message: e.to_string() })
}

/// `mcp` (headless, in-process engine) or `mcp --connect PORT|HOST:PORT` (drive a running app started with
/// `gridcraft --control PORT`). JSON-RPC on stdin/stdout; logs on stderr.
fn mcp(a: &McpArgs, io: &mut Io) -> Result<(), CliError> {
    use gridcraft_mcp::{Backend, Headless, Server};
    let backend: Box<dyn Backend> = match &a.connect {
        Some(c) => Box::new(connect(c)?),
        None => {
            let session = match (&a.input, &a.sample) {
                (Some(p), _) => open(io, p)?,
                (None, Some(name)) => sample(io, name)?,
                (None, None) => blank(),
            };
            Box::new(Headless::with_session(session))
        }
    };
    errln!(io, "gridcraft-cli: MCP server on stdio ({})", backend.describe());
    // The server speaks on the process's own stdin and stdout: that is the protocol, not this program's output.
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    Server::new(backend).serve(stdin.lock(), stdout.lock()).map_err(CliError::engine)
}

/// `send PORT METHOD [JSON]`: one control-channel request to a running app.
fn send(a: &SendArgs, io: &mut Io) -> Result<(), CliError> {
    use gridcraft_mcp::Backend;
    let params = a.params.clone().unwrap_or_else(|| json!({}));
    let mut r = connect(&a.port)?;
    let v = r.call(&a.method, params).map_err(CliError::engine)?;
    outln!(io, "{}", pretty(&v));
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

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
    fn convert_xlsx_to_pdf_keeps_links() {
        let dir = std::env::temp_dir().join(format!("gridcraft-cli-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("links.xlsx");
        let output = dir.join("links.pdf");
        let mut s = blank();
        s.execute("insert.link", json!({"cell": "A1", "target": "https://example.org"})).unwrap();
        let bytes = gridcraft_engine::io::save_file_bytes(&s.doc().unwrap().wb, input.to_str().unwrap()).unwrap();
        gridcraft_engine::io::write_file(input.to_str().unwrap(), &bytes).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::run(["convert", input.to_str().unwrap(), output.to_str().unwrap()], &mut out, &mut err);
        assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
        let pdf = std::fs::read(&output).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.windows(b"/URI (https://example.org)".len()).any(|w| w == b"/URI (https://example.org)"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
