//! Renders the whole GridCraft window offscreen (wgpu, no window) to a PNG.
//!
//! `cargo run --release -p gridcraft-ui-egui --example snapshot -- [--sample sales] [--in file.xlsx]
//!  [--size 1440x900] [--scale 2] [--cmd 'id={json}']... [--tab Insert] [--dark] [--language en|es|zh|ja|ko|ru|pt|de|cs|fr|tr]
//!  [--pane name] [--object id] [--dialog 'name={json}'] [--refedit key=A1:B2] [--hf-edit header|footer] out.png`
//!
//! `--refedit key=range` collapses the open dialog to its reference box `key`, then selects
//! `range` on the grid, as a person picking a range for that box does.
//!
//! `GRIDCRAFT_I18N_TRACE=path` adds the labels this run fell back to English for to `path` (see
//! `i18n::untranslated`): rendering a tab with `--language zh` then measures a localization pass
//! against the real interface instead of against a grep.

use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut sample = None;
    let mut input = None;
    let mut size = (1440.0f32, 900.0f32);
    let mut scale = 2.0f32;
    let mut cmds: Vec<String> = vec![];
    let mut tab = None;
    let mut dark = false;
    let mut pane: Option<String> = None;
    let mut chart: Option<u32> = None;
    let mut dialog: Option<String> = None;
    let mut language: Option<String> = None;
    let mut hf_edit: Option<String> = None;
    let mut refedit: Option<String> = None;
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
                app
            },
        );
    // The workbook's installed fonts bind once the font scan is done; frames past the first few
    // apply them, so the snapshot shows them as the app does.
    harness.run_steps(14);
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
    // `GRIDCRAFT_I18N_TRACE=/tmp/…` collects the labels this run could not translate, so a
    // localization pass can be measured instead of grepped. Each key is one JSON string per
    // line: `tr` keys can carry newlines (`Resize\nTable`), and one raw key per line would
    // split it into two records for the next run.
    if let Some(path) = std::env::var_os("GRIDCRAFT_I18N_TRACE") {
        // A line that doesn't parse (a corrupted record, or a plaintext key from before the JSON
        // format) leaves the file alone: rewriting would drop that record for good.
        let mut merged: std::collections::BTreeSet<String> = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut records = std::collections::BTreeSet::new();
                for line in text.lines().filter(|line| !line.is_empty()) {
                    match serde_json::from_str::<String>(line) {
                        Ok(key) => {
                            records.insert(key);
                        }
                        Err(e) => {
                            eprintln!("i18n trace: {}: {line:?} is not a JSON record ({e}); not rewriting the trace", path.to_string_lossy());
                            return;
                        }
                    }
                }
                records
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
            // A file that exists but can't be read keeps its contents: nothing is written on top of it.
            Err(e) => {
                eprintln!("i18n trace: can't read {}: {e}", path.to_string_lossy());
                return;
            }
        };
        merged.extend(gridcraft_ui_egui::i18n::untranslated());
        let mut text = merged.into_iter().filter_map(|k| serde_json::to_string(&k).ok()).collect::<Vec<_>>().join("\n");
        text.push('\n');
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("i18n trace: can't write {}: {e}", path.to_string_lossy());
            std::process::exit(1);
        }
    }
}
