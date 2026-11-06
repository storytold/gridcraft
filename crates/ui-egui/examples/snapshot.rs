//! Renders the whole GridCraft window offscreen (wgpu, no window) to a PNG.
//!
//! `cargo run --release -p gridcraft-ui-egui --example snapshot -- [--sample sales] [--in file.xlsx]
//!  [--size 1440x900] [--scale 2] [--cmd 'id={json}']... [--tab Insert] [--dark] [--language en|es|zh|ja|ko|ru|pt|de|cs|fr|tr]
//!  [--pane name] [--object id] [--dialog 'name={json}'] [--refedit key=A1:B2] [--hf-edit header|footer]
//!  [--python off|on|shortLimit|fromFile|notSetUp|noPython|blocked|noSandbox] [--ui 'id={json}']... [--ui-settled 'id={json}']... out.png`
//!
//! `--refedit key=range` collapses the open dialog to its reference box `key`, then selects
//! `range` on the grid, as a person picking a range for that box does.
//!
//! `--cmd` runs an engine command on the session before the app starts; `--ui` runs a command
//! through the app (UI commands too, such as `ui.pythonMode`, `ui.pythonCard`, `ui.pythonEditor` or
//! `ui.pythonInitialization`) after it starts.
//!
//! `--python` shows Python in that state with a fake worker and a made-up report, so no Python runs:
//! a cell's code is `ret 1`, `print text`, `fail` (or any code with `raise` in it, failing on its
//! second line), `denied`, `missing`, `hang`, `frame ROWS COLS` (a DataFrame that big, whose table
//! is cut as the real worker cuts it), `wide` (a table with very long cells), `dict` or `lines` (a
//! text of three lines). `shortLimit` has a time limit of a second.

use gridcraft_engine::python::{Interpreter, InterpreterKind, PythonSearch, Version};
use gridcraft_engine::{FoundPython, PythonEnvironment, PythonState, Session};
use gridcraft_python::fake::FakeTransport;
use gridcraft_python::protocol::{MAX_TABLE_COLS, MAX_TABLE_ROWS, Output, PyError, Reply, Request, Table};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

/// A made-up DataFrame, for pictures of the card.
fn demo_frame(rows: u64, cols: u64) -> Output {
    use gridcraft_engine::core::Value;
    const NAMES: [&str; 5] = ["Region", "Units", "Price", "Margin", "Owner"];
    const REGIONS: [&str; 4] = ["North", "East", "South", "West"];
    let (shown_rows, shown_cols) = ((rows as usize).min(MAX_TABLE_ROWS), (cols as usize).min(MAX_TABLE_COLS));
    let columns = (0..shown_cols).map(|c| NAMES.get(c).map_or_else(|| format!("Column {}", c + 1), |n| n.to_string())).collect();
    let mut data = Vec::with_capacity(shown_rows * shown_cols);
    for r in 0..shown_rows {
        for c in 0..shown_cols {
            data.push(match c {
                0 => Value::text(REGIONS.get(r % 4).copied().unwrap_or_default()),
                1 => Value::Number((r * 37 % 500) as f64),
                2 => Value::Number(9.5 + r as f64 * 1.25),
                3 => Value::Number(-0.05 + (r % 7) as f64 / 20.0),
                4 => Value::text(if r % 3 == 0 { "Asha" } else { "Ben" }),
                _ => Value::Number((r * c) as f64),
            });
        }
    }
    let table = Table { columns, rows: shown_rows, cols: shown_cols, data, total_rows: rows, total_cols: cols, ..Table::default() };
    Output::Object { type_name: "DataFrame".into(), preview: "   Region  Units".into(), size: vec![rows, cols], table: Some(table), picture: None }
}

/// A made-up table with cells far wider than their columns.
fn demo_wide() -> Output {
    use gridcraft_engine::core::Value;
    let long = "A note that goes on for much longer than any column in a card could ever show, ".repeat(8);
    let data = vec![
        Value::text(long.as_str()),
        Value::Number(1.0),
        Value::text("line one\nline two"),
        Value::Bool(true),
        Value::Error(gridcraft_engine::core::CellError::NA),
        Value::Empty,
    ];
    let table = Table {
        columns: vec!["Note".into(), long.clone()],
        rows: 3,
        cols: 2,
        data,
        total_rows: 3,
        total_cols: 2,
        text_cut: true,
        ..Table::default()
    };
    Output::Object { type_name: "DataFrame".into(), preview: String::new(), size: vec![3, 2], table: Some(table), picture: None }
}

/// A made-up plot: axes, and a line through them.
fn demo_plot(wide: bool) -> Output {
    let (w, h) = if wide { (1200u32, 500u32) } else { (800, 800) };
    let mut image = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]));
    let (left, bottom) = (90, h - 70);
    for x in left..w - 30 {
        image.put_pixel(x, bottom, image::Rgba([0, 0, 0, 255]));
        let t = f64::from(x - left) / f64::from(w - 30 - left);
        let y = f64::from(bottom) - (0.5 + 0.4 * (t * 9.0).sin()) * f64::from(bottom - 40);
        for dy in 0..4 {
            image.put_pixel(x, y as u32 + dy, image::Rgba([31, 119, 180, 255]));
        }
    }
    for y in 40..bottom {
        image.put_pixel(left, y, image::Rgba([0, 0, 0, 255]));
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).ok();
    match gridcraft_python::Picture::from_bytes(bytes.into_inner(), 2) {
        Ok(picture) => Output::picture(picture),
        Err(e) => Output::object("Error", e),
    }
}

/// The fake worker's cells.
fn fake_worker() -> FakeTransport {
    FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let Request::Run { id, code, .. } = req else {
                if let Request::Reset { id } = req {
                    w.done(id, Ok(Output::Value(gridcraft_engine::core::Value::Empty)));
                }
                continue;
            };
            let number = code.strip_prefix("ret ").and_then(|n| n.parse().ok()).unwrap_or(1.0);
            let value = Ok(Output::Value(gridcraft_engine::core::Value::Number(number)));
            let error = |kind: &str, message: &str, subject: Option<&str>, line| PyError {
                message: message.into(),
                kind: Some(kind.into()),
                subject: subject.map(str::to_string),
                line,
            };
            let frame = code.strip_prefix("frame ").and_then(|size| size.split_once(' ')).and_then(|(r, c)| Some((r.parse().ok()?, c.parse().ok()?)));
            let sent = if code.contains("spill") {
                use gridcraft_engine::core::{Array, Value};
                let cells = vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)];
                w.done(id, Ok(Output::Value(Value::Array(std::sync::Arc::new(Array { rows: 3, cols: 1, data: cells })))))
            } else if code.contains("plot") {
                w.done(id, Ok(demo_plot(code.contains("wide"))))
            } else if code == "hang" {
                w.stall();
                false
            } else if let Some((rows, cols)) = frame {
                w.done(id, Ok(demo_frame(rows, cols)))
            } else if code == "wide" {
                w.done(id, Ok(demo_wide()))
            } else if code == "dict" {
                w.done(
                    id,
                    Ok(Output::Object {
                        type_name: "dict".into(),
                        preview: "{'a': 1, 'b': 2, 'c': 3}".into(),
                        size: vec![3],
                        table: None,
                        picture: None,
                    }),
                )
            } else if code == "lines" {
                w.done(id, Ok(Output::Value(gridcraft_engine::core::Value::text("First line\nSecond line\nThird line"))))
            } else if code == "fail" || code.contains("raise") {
                w.done(id, Err(PyError { message: "ValueError: the input has no rows".into(), line: Some(2), ..PyError::default() }))
            } else if code == "denied" {
                w.done(
                    id,
                    Err(error("denied.file", "[Errno 1] Operation not permitted: '/Users/you/data.csv'", Some("/Users/you/data.csv"), Some(1))),
                )
            } else if code == "missing" {
                w.done(id, Err(error("module.missing", "No module named 'sklearn'", Some("sklearn"), Some(1))))
            } else if let Some(text) = code.strip_prefix("print ") {
                w.send(&Reply::Done { id, result: value, stdout: format!("{text}\n"), peak: None, touched: None })
            } else {
                w.done(id, value)
            };
            if !sent {
                return;
            }
        }
    })
}

/// A made-up Python, for pictures of the panes.
fn demo_interpreter() -> Interpreter {
    let env = PathBuf::from("/Users/you/.gridcraft/python");
    Interpreter {
        path: env.join("bin/python"),
        run_as: PathBuf::from("/Users/you/.local/share/uv/python/cpython-3.12.7/bin/python3.12"),
        version: Version { major: 3, minor: 12, patch: 7 },
        kind: InterpreterKind::UvEnvironment,
        site_dirs: vec![env.join("lib/python3.12/site-packages")],
        read_paths: vec![env, PathBuf::from("/Users/you/.local/share/uv/python/cpython-3.12.7")],
    }
}

fn demo_report(state: PythonState) -> PythonEnvironment {
    let mut packages = vec![
        ("numpy".to_string(), "2.1.3".to_string()),
        ("pandas".to_string(), "2.2.3".to_string()),
        ("statsmodels".to_string(), "0.14.4".to_string()),
    ];
    packages.extend((0..1000).map(|i| (format!("package-{i:04}"), format!("1.{i}.0"))));
    PythonEnvironment {
        state,
        diagnostic: None,
        sandbox: "sandbox-exec with a deny-by-default profile (checked again at each launch)".into(),
        uv: Some(PathBuf::from("/Users/you/.local/bin/uv")),
        interpreter: Some(demo_interpreter()),
        packages,
    }
}

/// Puts the app's Python in the state `--python` names.
fn demo_python(app: &mut SheetApp, state: &str) {
    let found = |version, kind| FoundPython { path: demo_interpreter().path, version, kind };
    let version = Some(Version { major: 3, minor: 12, patch: 7 });
    let mut search =
        PythonSearch { found: vec![found(version, Some(InterpreterKind::UvEnvironment))], uv: Some(PathBuf::from("/Users/you/.local/bin/uv")) };
    app.python.set_sandbox(Ok("sandbox-exec with a deny-by-default profile".into()));
    app.ui.python.enabled = !matches!(state, "off" | "noSandbox");
    let t = fake_worker();
    let mut report = demo_report(PythonState::Ready);
    match state {
        "noPython" => {
            report = PythonEnvironment {
                state: PythonState::NotSetUp,
                diagnostic: Some(gridcraft_engine::python::Diagnostic::new("notSetUp.noInterpreter", "", None)),
                interpreter: None,
                uv: None,
                packages: Vec::new(),
                ..report
            };
            search = PythonSearch::default();
        }
        "notSetUp" => {
            report = PythonEnvironment {
                state: PythonState::NotSetUp,
                diagnostic: Some(gridcraft_engine::python::Diagnostic::new("connect.failed", "Python stopped before it said it was ready", None)),
                ..report
            };
        }
        "blocked" => {
            report = PythonEnvironment {
                state: PythonState::Blocked,
                diagnostic: Some(gridcraft_engine::python::Diagnostic::new(
                    "blocked.sandboxCheck",
                    "the sandbox is not enforced: read the canary",
                    None,
                )),
                ..report
            };
        }
        "noSandbox" => app.python.set_sandbox(Err("GridCraft has no sandbox for Windows".into())),
        "shortLimit" => app.ui.python.time_limit_secs = 1,
        "fromFile" => {
            if let Some(d) = app.session.active_mut() {
                d.trust = gridcraft_engine::Trust::FromFile;
            }
        }
        _ => {}
    }
    app.python.report = Some(report);
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    app.python.search_answer = Some(search);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut sample = None;
    let mut input = None;
    let mut size = (1440.0f32, 900.0f32);
    let mut scale = 2.0f32;
    let mut cmds: Vec<String> = vec![];
    let mut ui_cmds: Vec<String> = vec![];
    let mut settled_cmds: Vec<String> = vec![];
    let mut tab = None;
    let mut dark = false;
    let mut pane: Option<String> = None;
    let mut chart: Option<u32> = None;
    let mut dialog: Option<String> = None;
    let mut language: Option<String> = None;
    let mut hf_edit: Option<String> = None;
    let mut refedit: Option<String> = None;
    let mut python: Option<String> = None;
    let mut out = "snapshot.png".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--sample" => {
                i += 1;
                sample = args.get(i).cloned();
            }
            "--in" => {
                i += 1;
                input = args.get(i).cloned();
            }
            "--size" => {
                i += 1;
                if let Some((w, h)) = args.get(i).and_then(|s| s.split_once('x')) {
                    size = (w.parse().unwrap_or(1440.0), h.parse().unwrap_or(900.0));
                }
            }
            "--scale" => {
                i += 1;
                scale = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(2.0);
            }
            "--cmd" => {
                i += 1;
                if let Some(c) = args.get(i) {
                    cmds.push(c.clone());
                }
            }
            "--tab" => {
                i += 1;
                tab = args.get(i).cloned();
            }
            "--dark" => dark = true,
            // A command that needs Python's results: it runs once the fake worker has answered.
            "--ui-settled" => {
                i += 1;
                if let Some(c) = args.get(i) {
                    settled_cmds.push(c.clone());
                }
            }
            "--ui" => {
                i += 1;
                if let Some(c) = args.get(i) {
                    ui_cmds.push(c.clone());
                }
            }
            "--pane" => {
                i += 1;
                pane = args.get(i).cloned();
            }
            // Selects a floating object of any kind by id (`--chart` is the older spelling).
            "--object" | "--chart" => {
                i += 1;
                chart = args.get(i).and_then(|s| s.parse().ok());
            }
            "--dialog" => {
                i += 1;
                dialog = args.get(i).cloned();
            }
            "--refedit" => {
                i += 1;
                refedit = args.get(i).cloned();
            }
            "--hf-edit" => {
                i += 1;
                hf_edit = args.get(i).cloned();
            }
            "--python" => {
                i += 1;
                python = args.get(i).cloned();
            }
            "--language" => {
                i += 1;
                language = args.get(i).cloned();
            }
            other => out = other.to_string(),
        }
        i += 1;
    }
    // Catalog the installed fonts first (the app scans in the background from its first frame), so
    // the workbook's fonts are drawn in their own faces, not a substitute.
    let _ = gridcraft_ui_egui::fonts::catalog();
    let mut session = Session::new();
    if let Some(p) = &input {
        session.execute("file.open", json!({"path": p})).expect("open");
    } else if let Some(s) = &sample {
        session.execute("file.new", json!({"sample": s})).expect("sample");
    } else {
        session.new_workbook();
    }
    for c in &cmds {
        let (id, p) = c.split_once('=').map(|(a, b)| (a.to_string(), serde_json::from_str(b).unwrap_or(json!({})))).unwrap_or((c.clone(), json!({})));
        if let Err(e) = session.execute(&id, p) {
            eprintln!("{id}: {e}");
        }
    }
    let mut harness =
        egui_kittest::Harness::builder().with_size(egui::vec2(size.0, size.1)).with_pixels_per_point(scale).with_max_steps(12).wgpu().build_ui_state(
            move |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            {
                let mut app = SheetApp::new(session, Default::default());
                app.ui.dark = dark;
                if let Some(state) = &python {
                    demo_python(&mut app, state);
                }
                if let Some(code) = &language
                    && let Some(l) = gridcraft_ui_egui::i18n::Language::parse(code)
                {
                    app.ui.language = l;
                }
                if let Some(t) = tab {
                    app.ui.ribbon_tab = t;
                }
                app.grid.pane = pane;
                app.select_object_id(chart);
                if let Some(d) = &dialog {
                    let (name, p) =
                        d.split_once('=').map(|(a, b)| (a, serde_json::from_str(b).unwrap_or(json!({})))).unwrap_or((d.as_str(), json!({})));
                    app.open_dialog(name, p);
                }
                // Page Layout view: edit the first page's centre header (or footer) section.
                let active = app.session.active().and_then(|d| d.wb.active().cloned().map(|s| (d.wb.active_sheet(), s)));
                if let (Some(part), Some((si, sh))) = (&hf_edit, active) {
                    let zone = gridcraft_engine::paper::HfZone { footer: part == "footer", section: 1 };
                    gridcraft_ui_egui::paper_view::begin(&mut app, si, &sh, (0, 0), zone);
                }
                for c in &ui_cmds {
                    let (id, p) = c
                        .split_once('=')
                        .map(|(a, b)| (a.to_string(), serde_json::from_str(b).unwrap_or(json!({}))))
                        .unwrap_or((c.clone(), json!({})));
                    if let Err(e) = app.run(&id, p) {
                        eprintln!("{id}: {e}");
                    }
                }
                app
            },
        );
    // The workbook's installed fonts bind once the font scan is done; frames past the first few
    // apply them, so the snapshot shows them as the app does.
    harness.run_steps(14);
    // A fake worker answers on its own thread: results come in without input, as in the app. A cell
    // that hangs keeps Python running, so the wait is bounded.
    if let Some(state) = &python {
        let wait_it_out = state == "shortLimit";
        for _ in 0..if wait_it_out { 400 } else { 60 } {
            let session = &harness.state().session;
            if (session.python_running().is_some() && !wait_it_out) || !session.python_busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            harness.step();
        }
        harness.run_steps(4);
        for c in &settled_cmds {
            let (id, p) =
                c.split_once('=').map(|(a, b)| (a.to_string(), serde_json::from_str(b).unwrap_or(json!({})))).unwrap_or((c.clone(), json!({})));
            if let Err(e) = harness.state_mut().run(&id, p) {
                eprintln!("{id}: {e}");
            }
        }
        harness.run_steps(4);
    }
    if let Some((key, range)) = refedit.as_deref().and_then(|r| r.split_once('=')) {
        let app = harness.state_mut();
        if let Some(mut d) = app.dialog.take() {
            d.collapse_to(app, key);
            app.dialog = Some(d);
        }
        app.session.execute("selection.set", json!({"range": range})).expect("select");
        harness.run_steps(8);
    }
    let img = harness.render().expect("render");
    img.save(&out).expect("save png");
    eprintln!("wrote {out} ({}x{})", img.width(), img.height());
}
