//! PDF-specific UI state; extraction is a cancelable worker, loading is an engine command.
use crate::SheetApp;
#[cfg(any(not(target_arch = "wasm32"), test))]
use gridcraft_engine::pdf_import as engine;
use gridcraft_engine::pdf_import::PageTables;
use serde_json::{Value as Json, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
struct Loaded {
    bytes: Arc<Vec<u8>>,
    preview: PageTables,
}
struct Job {
    rx: mpsc::Receiver<Result<Loaded, String>>,
    stop: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
pub(crate) struct State {
    path: String,
    destination: u64,
    bytes: Option<Arc<Vec<u8>>>,
    preview: Option<PageTables>,
    job: Option<Job>,
    error: Option<String>,
    page: u32,
    pages: usize,
    selected: usize,
    as_table: bool,
    headers: bool,
    numeric: Vec<bool>,
    type_column: usize,
    started: bool,
}
impl State {
    fn new(path: String, destination: u64) -> Self {
        Self {
            path,
            destination,
            bytes: None,
            preview: None,
            job: None,
            error: None,
            page: 1,
            pages: 0,
            selected: 0,
            as_table: true,
            headers: true,
            numeric: Vec::new(),
            type_column: 0,
            started: false,
        }
    }
    fn poll(&mut self) {
        let result = self.job.as_ref().map(|j| j.rx.try_recv());
        match result {
            Some(Ok(result)) => {
                self.job = None;
                match result {
                    Ok(loaded) => {
                        self.bytes = Some(loaded.bytes);
                        self.pages = loaded.preview.pages;
                        self.preview = Some(loaded.preview);
                        self.selected = 0;
                        self.numeric.clear();
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.job = None;
                self.error = Some("PDF import worker stopped unexpectedly.".into());
            }
            _ => {}
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn start(&mut self, ctx: &egui::Context) {
        self.job = None;
        self.preview = None;
        self.error = None;
        self.started = true;
        let path = self.path.clone();
        let bytes = self.bytes.clone();
        let page = self.page;
        let ctx = ctx.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("gridcraft-pdf-import".into()).spawn(move || {
            let result = (|| {
                let bytes = match bytes {
                    Some(bytes) => bytes,
                    None => engine::read(std::path::Path::new(&path)).map_err(|e| e.to_string())?,
                };
                let preview = engine::inspect(bytes.clone(), page, stopped).map_err(|e| e.to_string())?;
                Ok(Loaded { bytes, preview })
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.job = Some(Job { rx, stop }),
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn start(&mut self, _: &egui::Context) {
        self.started = true;
        self.error = Some("PDF table import currently requires the desktop application.".into());
    }
}
pub(crate) fn open(app: &mut SheetApp, params: &Json) {
    let Some(destination) = app.session.active().map(|d| d.uid) else {
        return;
    };
    let path = params.get("path").and_then(Json::as_str).map(str::to_owned).or_else(|| app.services.pick_pdf.as_ref().and_then(|pick| pick()));
    if let Some(path) = path {
        app.pdf_import = Some(State::new(path, destination));
    } else if app.services.pick_pdf.is_none() {
        app.message = Some(("From PDF".into(), "PDF table import requires a desktop file picker.".into()));
    }
}
// Bound layout work too: a valid cell can contain far more text than a preview needs.
fn preview_text(text: &str) -> String {
    let mut out = String::new();
    let mut lines = 1;
    for (i, c) in text.chars().enumerate() {
        if i == 512 || (c == '\n' && lines == 8) {
            out.push('…');
            break;
        }
        if c == '\n' {
            lines += 1;
        }
        out.push(c);
    }
    out
}
fn table_picker(ui: &mut egui::Ui, state: &mut State) {
    let lang = crate::i18n::current(ui.ctx());
    let Some(preview) = &state.preview else {
        return;
    };
    if preview.tables.is_empty() {
        ui.label(lang.tr("No supported bordered tables were found on this page. Scanned PDFs require OCR."));
        return;
    }
    let old = state.selected;
    let table_label = ui.label(lang.tr("Table"));
    craft_ui::ComboBox::from_id_salt("pdf.table")
        .selected_text(format!("{} {}", lang.tr("Table"), state.selected + 1))
        .show_ui(ui, |ui| {
            for (index, table) in preview.tables.iter().enumerate() {
                ui.selectable_value(
                    &mut state.selected,
                    index,
                    format!("{} {} — {} × {}", lang.tr("Table"), index + 1, table.rows.len(), table.rows.first().map_or(0, Vec::len)),
                );
            }
        })
        .response
        .labelled_by(table_label.id);
    let Some(table) = preview.tables.get(state.selected) else {
        return;
    };
    let width = table.rows.first().map_or(0, Vec::len);
    if old != state.selected || state.numeric.len() != width {
        state.numeric = vec![false; width];
        state.type_column = 0;
    }
    ui.label(lang.tr("Choose Number only for columns containing plain decimal numbers. Text preserves IDs and leading zeros."));
    ui.horizontal(|ui| {
        let column_label = ui.label(lang.tr("Column"));
        ui.add(
            egui::DragValue::new(&mut state.type_column)
                .range(0..=width.saturating_sub(1))
                .custom_formatter(|n, _| format!("{}", n as usize + 1))
                .custom_parser(|s| s.parse::<usize>().ok()?.checked_sub(1).map(|n| n as f64)),
        )
        .labelled_by(column_label.id);
        let type_label = ui.label(lang.tr("Column type"));
        if let Some(number) = state.numeric.get_mut(state.type_column) {
            craft_ui::ComboBox::from_id_salt("pdf.selected.type")
                .selected_text(lang.tr(if *number { "Number" } else { "Text" }))
                .show_ui(ui, |ui| {
                    ui.selectable_value(number, false, lang.tr("Text"));
                    ui.selectable_value(number, true, lang.tr("Number"));
                })
                .response
                .labelled_by(type_label.id);
        }
    });
    egui::ScrollArea::both().id_salt("pdf.preview").max_height(320.0).show(ui, |ui| {
        egui::Grid::new("pdf.preview.cells").striped(true).show(ui, |ui| {
            for row in table.rows.iter().take(20) {
                for text in row.iter().take(20) {
                    ui.vertical(|ui| {
                        ui.set_max_width(220.0);
                        ui.label(preview_text(text));
                    });
                }
                ui.end_row();
            }
        });
    });
    ui.label(lang.tr("Long cell contents are shortened in the preview only."));
    ui.label(format!("{}: {} × {}", lang.tr("Table size"), table.rows.len(), width));
    if table.rows.len() > 20 || width > 20 {
        ui.label(lang.tr("Preview shows the first 20 rows and columns; Load imports the complete selected table."));
    }
}
pub(crate) fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(mut state) = app.pdf_import.take() else {
        return;
    };
    state.poll();
    if !state.started {
        state.start(ctx);
    }
    let mut close = false;
    let mut load = false;
    // The message box is drawn above this modal and owns Escape until it closes.
    let message_open = app.message.is_some();
    egui::Modal::new(egui::Id::unique("pdf.import")).show(ctx, |ui| {
        let lang = crate::i18n::current(ctx);
        ui.set_width((ctx.content_rect().width() - 40.0).clamp(240.0, 650.0));
        // Scroll the entire dialog, not just its cells, so the footer remains reachable
        // at the minimum window size and at larger interface zoom factors.
        egui::ScrollArea::vertical().id_salt("pdf.import.body").max_height((ctx.content_rect().height() - 32.0).max(1.0)).show(ui, |ui| {
            ui.heading(lang.tr("Import PDF Table"));
            ui.label(std::path::Path::new(&state.path).file_name().and_then(|s| s.to_str()).unwrap_or(&state.path));
            ui.label(lang.tr("Bordered tables with selectable text. Data is loaded into a new worksheet."));
            ui.horizontal(|ui| {
                let page_label = ui.label(lang.tr("Page"));
                let max = if state.pages == 0 { u32::MAX } else { u32::try_from(state.pages).unwrap_or(u32::MAX).max(1) };
                if ui.add_enabled(state.job.is_none(), egui::DragValue::new(&mut state.page).range(1..=max)).labelled_by(page_label.id).changed() {
                    state.preview = None;
                }
                if ui.add_enabled(state.job.is_none(), egui::Button::new(lang.tr("Preview page"))).clicked() {
                    state.start(ctx);
                }
                ui.label(if state.pages == 0 { "/ ?".into() } else { format!("/ {}", state.pages) });
            });
            if state.job.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(lang.tr("Reading PDF tables…"));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            if let Some(error) = &state.error {
                ui.colored_label(crate::theme::Tokens::get(ctx).warning, lang.tr_error(error));
            }
            table_picker(ui, &mut state);
            ui.checkbox(&mut state.as_table, lang.tr("Format as table"));
            ui.checkbox(&mut state.headers, lang.tr("First row contains headers"));
            ui.horizontal(|ui| {
                let ready =
                    state.job.is_none() && state.preview.as_ref().is_some_and(|p| p.page == state.page && p.tables.get(state.selected).is_some());
                load = ui.add_enabled(ready, egui::Button::new(lang.tr("Load"))).clicked();
                close = ui.button(lang.tr("Cancel")).clicked() || (!message_open && ui.input(|i| i.key_pressed(egui::Key::Escape)));
            });
        });
    });
    if load {
        if app.session.active().map(|d| d.uid) != Some(state.destination) {
            state.error = Some("The destination workbook changed. Reopen PDF import in the intended workbook.".into());
        } else if let Some(table) = state.preview.as_ref().and_then(|p| p.tables.get(state.selected)) {
            let numeric: Vec<_> = state.numeric.iter().enumerate().filter_map(|(i, n)| n.then_some(i)).collect();
            match app.run("data.fromPdf", json!({"rows":table.rows,"asTable":state.as_table,"headers":state.headers,"numericColumns":numeric})) {
                Ok(_) => close = true,
                Err(e) => state.error = Some(e),
            }
        }
    }
    if !close {
        app.pdf_import = Some(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable as _;
    type Harness = egui_kittest::Harness<'static, SheetApp>;
    fn harness(dark: bool) -> Harness {
        harness_with_size(dark, egui::vec2(1100., 800.), 100)
    }
    fn harness_with_size(dark: bool, size: egui::Vec2, percent: u16) -> Harness {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        app.use_prepared_fonts();
        app.ui.dark = dark;
        app.ui.system_theme = false;
        app.run("view.interfaceScale", json!({"percent":percent})).unwrap();
        let mut state = State::new("original-sample.pdf".into(), app.session.doc().unwrap().uid);
        state.started = true;
        state.pages = 2;
        state.preview = Some(PageTables {
            page: 1,
            pages: 2,
            tables: vec![engine::PdfTable {
                bounds: [0., 0., 100., 100.],
                rows: vec![vec!["Code".into(), "Amount".into()], vec!["00123".into(), "12.50".into()], vec!["=1+1".into(), "".into()]],
            }],
        });
        app.pdf_import = Some(state);
        let mut h = egui_kittest::Harness::builder()
            .with_size(size)
            .with_context_setup(move |ctx| {
                ctx.set_fonts(crate::theme::pinned_font_definitions());
                crate::theme::apply(ctx, dark);
            })
            .build_ui_state(
                |ui, app: &mut SheetApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
        h.run_steps(4);
        h
    }
    #[test]
    fn confirmed_preview_loads_literals_in_one_undo() {
        let mut h = harness(false);
        if let Ok(path) = std::env::var("GRIDCRAFT_PDF_PREVIEW_SNAPSHOT") {
            h.render().unwrap().save(path).unwrap();
        }
        h.get_by_label("Load").click();
        h.run_steps(3);
        assert!(h.state().pdf_import.is_none());
        let sh = h.state().session.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.value(gridcraft_engine::core::CellRef::new(1, 0)), gridcraft_engine::core::Value::text("00123"));
        assert_eq!(sh.value(gridcraft_engine::core::CellRef::new(2, 0)), gridcraft_engine::core::Value::text("=1+1"));
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().wb.sheets.len(), 1);
    }
    #[test]
    fn cancel_does_not_mutate_workbook_and_modal_owns_typing() {
        let mut h = harness(true);
        if let Ok(path) = std::env::var("GRIDCRAFT_PDF_PREVIEW_SNAPSHOT") {
            h.render().unwrap().save(std::path::Path::new(&path).with_file_name("production-preview-dark.png")).unwrap();
        }
        h.input_mut().events.push(egui::Event::Text("Unwanted grid edit".into()));
        h.run_steps(2);
        assert!(h.state().editor.is_none());
        assert!(h.state().session.doc().unwrap().wb.active().unwrap().cells.is_empty());
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").click();
        h.run_steps(2);
        assert!(h.state().pdf_import.is_none());
        assert_eq!(h.state().session.doc().unwrap().wb.sheets.len(), 1);
    }
    #[test]
    fn closing_a_pending_preview_requests_cancellation() {
        let mut h = harness(false);
        let stop = Arc::new(AtomicBool::new(false));
        let (_tx, rx) = mpsc::channel();
        let state = h.state_mut().pdf_import.as_mut().unwrap();
        state.job = Some(Job { rx, stop: stop.clone() });
        h.run_steps(2);
        // The progress row can move the footer below the dialog's scroll viewport.
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").scroll_to_me();
        h.run_steps(30);
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").click();
        h.run_steps(2);
        assert!(stop.load(Ordering::Relaxed));
        assert!(h.state().pdf_import.is_none());
    }
    #[test]
    fn number_type_controls_reach_columns_outside_the_preview() {
        let mut h = harness(false);
        let state = h.state_mut().pdf_import.as_mut().unwrap();
        let table = &mut state.preview.as_mut().unwrap().tables[0];
        table.rows = vec![(0..22).map(|i| format!("Field {}", i + 1)).collect(), vec!["12.50".into(); 22]];
        h.run_steps(2);
        h.state_mut().pdf_import.as_mut().unwrap().type_column = 21;
        h.run_steps(2);
        h.get_by_label("Column type").click();
        h.run_steps(2);
        h.get_by_label("Number").click();
        h.run_steps(2);
        h.get_by_label("Load").click();
        h.run_steps(2);
        let sh = h.state().session.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.value(gridcraft_engine::core::CellRef::new(1, 21)), gridcraft_engine::core::Value::Number(12.5));
        assert_eq!(sh.value(gridcraft_engine::core::CellRef::new(1, 0)), gridcraft_engine::core::Value::text("12.50"));
    }
    #[test]
    fn cell_preview_bounds_unicode_and_multiline_layout() {
        assert_eq!(preview_text("00123"), "00123");
        assert_eq!(preview_text(&"é".repeat(513)).chars().count(), 513);
        assert!(preview_text(&"line\n".repeat(100)).lines().count() <= 8);
    }
    fn escape(h: &mut Harness) {
        for pressed in [true, false] {
            h.input_mut().events.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run_steps(2);
    }
    #[test]
    fn escape_dismisses_the_message_before_the_import_and_keeps_column_choices() {
        let mut h = harness(false);
        h.state_mut().pdf_import.as_mut().unwrap().numeric[1] = true;
        h.state_mut().message = Some(("PDF import warning".into(), "An editor draft needs attention.".into()));
        h.run_steps(3);
        escape(&mut h);
        assert!(h.state().message.is_none());
        assert!(h.state().pdf_import.as_ref().unwrap().numeric[1]);
        escape(&mut h);
        assert!(h.state().pdf_import.is_none());
        assert_eq!(h.state().session.doc().unwrap().wb.sheets.len(), 1);
    }
    #[test]
    fn import_controls_remain_reachable_in_small_scaled_windows() {
        for percent in [100, 150, 200] {
            let mut h = harness_with_size(false, egui::vec2(640., 420.), percent);
            let state = h.state_mut().pdf_import.as_mut().unwrap();
            state.preview.as_mut().unwrap().tables[0].rows = vec![vec!["Code".into(), "Description".into()]];
            state.preview.as_mut().unwrap().tables[0]
                .rows
                .extend((1..20).map(|i| vec![format!("Item {i}"), "First line\nSecond line\nThird line\nFourth line".into()]));
            h.run_steps(4);
            for label in ["Format as table", "First row contains headers", "Load"] {
                h.get_by_label(label).scroll_to_me();
                h.run_steps(30);
                let rect = h.get_by_label(label).rect();
                assert!(h.ctx.content_rect().contains_rect(rect), "{label} is clipped at {percent}%: {rect:?}");
            }
            if let Ok(dir) = std::env::var("GRIDCRAFT_PDF_REVIEW_SNAPSHOT_DIR") {
                h.render().unwrap().save(std::path::Path::new(&dir).join(format!("review-small-{percent}.png"))).unwrap();
            }
            h.get_by_label("Load").click();
            h.run_steps(3);
            assert!(h.state().pdf_import.is_none(), "Load must work at {percent}%");
            assert_eq!(h.state().session.doc().unwrap().wb.sheets.len(), 2);
        }
    }
}
