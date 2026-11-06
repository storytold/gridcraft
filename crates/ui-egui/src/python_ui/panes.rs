//! The Python panes: Environment, Packages, Problems, Run Log and What Python Can Do. Each is a task
//! pane like the others (`panes.rs`) and opens by a UI-only command, so the control channel reaches
//! it; only the switch in What Python Can Do is a person's. The lists draw only the rows in view.

use egui::{Align, Layout, RichText, ScrollArea, Sense, Ui, UiBuilder, vec2};
use gridcraft_engine::PythonState;
use gridcraft_engine::python::{Diagnostic, STARTER_PACKAGES, UV_INSTALL};
use serde_json::json;

use super::{ENVIRONMENT, Guard, PACKAGES, PERMISSIONS, PROBLEMS, RUN_LOG, dim};
use crate::SheetApp;
use crate::i18n::Language;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

const PACKAGE_ROW: f32 = 20.0;
const PROBLEM_ROW: f32 = 46.0;
const RUN_ROW: f32 = 38.0;
/// The most characters of one line a row lays out, whatever a run printed or a package is called:
/// what comes from Python is capped per field, far above what fits in a pane.
const ROW_CHARS: usize = 200;
/// The most characters of an explanation from Python or the system the Environment pane shows.
const DETAIL_CHARS: usize = 2000;

/// One row of the Problems list.
#[derive(Clone)]
enum ProblemRow {
    /// A kind, with how many entries it has.
    Kind(&'static str, usize),
    /// An entry: its place, as text and as the sheet and cell to go to, or whether it is about the
    /// initialization code, where a click opens that.
    Entry { place: Option<(String, usize, String)>, initialization: bool, diagnostic: Diagnostic },
}

/// The list as it was built, and what it was built from: reading every cell's state each frame would
/// cost more than the pane draws.
struct ProblemsCache {
    key: ProblemsKey,
    rows: std::sync::Arc<Vec<ProblemRow>>,
}

/// What the list depends on. A run or a change of state moves one of these.
type ProblemsKey = (u64, u64, usize, PythonState, bool, bool);

/// A run, known again after others have come and gone.
type RunKey = (String, String, std::time::Duration);

/// What the panes keep between frames.
#[derive(Default)]
pub struct PaneState {
    package_filter: String,
    package_name: String,
    selected_run: Option<RunKey>,
    problems: Option<ProblemsCache>,
    /// Rows the last list drew, for tests that it stays cheap.
    rows_drawn: usize,
}

impl super::PythonUi {
    /// How many rows the last list drew.
    #[doc(hidden)]
    pub fn rows_drawn(&self) -> usize {
        self.panes.rows_drawn
    }
}

/// A pane's title, as a translation key; `None` for a pane that isn't Python's.
pub fn title(pane: &str) -> Option<&'static str> {
    Some(match pane {
        ENVIRONMENT => "Python Environment",
        PACKAGES => "Python Packages",
        PROBLEMS => "Python Problems",
        RUN_LOG => "Python Run Log",
        PERMISSIONS => "What Python Can Do",
        _ => return None,
    })
}

/// Draws a Python pane's body, with its own scrolling.
pub fn body(app: &mut SheetApp, ui: &mut Ui, pane: &str) {
    match pane {
        ENVIRONMENT => {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| environment(app, ui));
        }
        PACKAGES => packages(app, ui),
        PROBLEMS => problems(app, ui),
        RUN_LOG => run_log(app, ui),
        PERMISSIONS => {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| permissions(app, ui));
        }
        _ => {}
    }
}

/// The first line of `text`, cut to what a row can show.
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().chars().take(ROW_CHARS).collect()
}

fn heading(ui: &mut Ui, lang: Language, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(lang.tr(text)).strong());
    ui.add_space(2.0);
}

fn path_list(ui: &mut Ui, paths: &[std::path::PathBuf]) {
    for p in paths {
        ui.add(egui::Label::new(RichText::new(p.display().to_string()).monospace()).wrap());
    }
}

/// State and reason with its action; the interpreter; the sandbox; the folders; uv.
fn environment(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());
    let shown = super::state(app);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
        icons::paint(ui.painter(), r, super::state_icon(shown), t.text);
        ui.label(RichText::new(lang.tr(super::state_label(shown))).font(theme::ui_bold(14.0)));
    });
    if super::checking(app) {
        dim(ui, lang.tr("GridCraft is checking Python."));
    }
    if let Some(d) = super::reason(app) {
        ui.label(lang.tr(d.kind.message));
        if !d.detail.is_empty() {
            let detail: String = d.detail.chars().take(DETAIL_CHARS).collect();
            ui.add(egui::Label::new(RichText::new(detail).monospace()).wrap());
        }
        super::action_button(app, ui, &d, None);
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button(lang.tr("Check Again")).clicked() {
            super::check_again(app);
        }
        if ui.button(lang.tr("Open Python Settings")).clicked() {
            super::open_options(app);
        }
    });
    heading(ui, lang, "Interpreter");
    let report = app.session.python_report().clone();
    match &report.interpreter {
        Some(i) => {
            let chosen = if app.ui.python.interpreter.is_some() { "Chosen by you:" } else { "Automatic found:" };
            dim(ui, lang.tr(chosen));
            ui.add(egui::Label::new(RichText::new(i.path.display().to_string()).monospace()).wrap());
            ui.label(format!("Python {}, {}", i.version, super::kind_label(lang, i.kind)));
            dim(ui, lang.tr("What runs:"));
            ui.add(egui::Label::new(RichText::new(i.run_as.display().to_string()).monospace()).wrap());
        }
        None => dim(ui, lang.tr("None found yet.")),
    }
    heading(ui, lang, "Sandbox");
    super::sandbox_lines(app, ui);
    if !report.sandbox.is_empty() {
        ui.add(egui::Label::new(RichText::new(&report.sandbox).monospace()).wrap());
    }
    if let Some(i) = &report.interpreter {
        heading(ui, lang, "Package folders");
        path_list(ui, &i.site_dirs);
        heading(ui, lang, "What Python can read");
        path_list(ui, &i.read_paths);
    }
    heading(ui, lang, "uv");
    match &report.uv {
        Some(uv) => {
            ui.add(egui::Label::new(RichText::new(uv.display().to_string()).monospace()).wrap());
        }
        None => {
            dim(ui, lang.tr("uv manages Python versions and packages. To get it, run:"));
            super::command_row(app, ui, UV_INSTALL);
        }
    }
}

/// A list that draws only the rows in view: the filter, the install command, the packages.
fn packages(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    if let Some(d) = super::reason(app).filter(|d| d.kind.id != "packages.noPandas") {
        ui.label(lang.tr(d.kind.message));
        ui.separator();
    }
    dim(ui, lang.tr("Install a package with uv. GridCraft only copies the command; you run it in a terminal, then choose Check Again."));
    let mut name = std::mem::take(&mut app.python.panes.package_name);
    named_field(ui, &mut name, lang.tr("Package name"));
    let names: Vec<&str> = name.split_whitespace().collect();
    let valid = !names.is_empty() && names.iter().all(|n| package_spec(n));
    if ui.add_enabled(valid, egui::Button::new(lang.tr("Copy Install Command"))).clicked() {
        let command = super::install_command(app.session.python_report().interpreter.as_ref(), &names);
        super::copy_command(app, &ctx, &command);
    }
    egui::CollapsingHeader::new(lang.tr("Other package managers")).default_open(false).show(ui, |ui| {
        dim(ui, lang.tr("For people who manage Python another way: choose any interpreter by path in File › Options › Python. With pip:"));
        let shown: &[&str] = if valid { &names } else { &STARTER_PACKAGES };
        let command = super::pip_command(app.session.python_report().interpreter.as_ref(), shown);
        super::command_row(app, ui, &command);
    });
    app.python.panes.package_name = name;
    ui.separator();
    named_field(ui, &mut app.python.panes.package_filter, lang.tr("Filter"));
    let filter = app.python.panes.package_filter.to_lowercase();
    let list = &app.session.python_report().packages;
    let shown: Vec<usize> =
        list.iter().enumerate().filter(|(_, (name, _))| filter.is_empty() || name.to_lowercase().contains(&filter)).map(|(i, _)| i).collect();
    dim(ui, &lang.tr("Packages: {count}").replace("{count}", &shown.len().to_string()));
    let mut drawn = 0;
    ScrollArea::vertical().id_salt("python_packages").auto_shrink([false, false]).show_rows(ui, PACKAGE_ROW, shown.len(), |ui, range| {
        for row in range {
            let Some((name, version)) = shown.get(row).and_then(|i| list.get(*i)) else { continue };
            drawn += 1;
            ui.allocate_ui_with_layout(vec2(ui.available_width(), PACKAGE_ROW), Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(first_line(name)).truncate());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(first_line(version)).color(t.text_dim));
                });
            });
        }
    });
    app.python.panes.rows_drawn = drawn;
}

/// A text field whose hint is also its name for accessibility.
fn named_field(ui: &mut Ui, text: &mut String, name: &str) {
    let field = ui.add(egui::TextEdit::singleline(text).hint_text(name).desired_width(f32::INFINITY));
    ui.ctx().accesskit_node_builder(field.id, |node| node.set_label(name));
}

/// Whether this is what a package can be called in an install command: a name, with extras or a
/// version. Everything else stays out of what a person pastes into a terminal.
fn package_spec(text: &str) -> bool {
    !text.is_empty()
        && text.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && text.chars().all(|c| c.is_ascii_alphanumeric() || "._-[],=<>!~".contains(c))
}

/// The diagnostics in rows, grouped by kind, each kind in the order it first comes.
fn problem_rows(app: &SheetApp) -> Vec<ProblemRow> {
    let d = app.session.active();
    let entries = app.session.python_diagnostics();
    let mut kinds: Vec<&'static str> = Vec::new();
    for e in &entries {
        if !kinds.contains(&e.diagnostic.kind.id) {
            kinds.push(e.diagnostic.kind.id);
        }
    }
    let mut rows = Vec::with_capacity(entries.len() + kinds.len());
    for kind in kinds {
        let of_kind: Vec<_> = entries.iter().filter(|e| e.diagnostic.kind.id == kind).collect();
        rows.push(ProblemRow::Kind(kind, of_kind.len()));
        for e in of_kind {
            let place = e.cell.and_then(|(sheet, at)| {
                let name = d?.wb.sheet(sheet)?.name.as_str();
                Some((format!("{}!{}", gridcraft_engine::formula::quote_sheet(name), at.a1()), sheet, at.a1()))
            });
            rows.push(ProblemRow::Entry { place, initialization: e.initialization, diagnostic: e.diagnostic.clone() });
        }
    }
    rows
}

fn problems_key(app: &SheetApp) -> ProblemsKey {
    let (uid, revision) = app.session.active().map_or((0, 0), |d| (d.uid, d.revision));
    (uid, revision, app.session.python_run_log().count(), super::state(app), app.session.python_workbook_enabled(), app.session.python_busy())
}

/// Every diagnostic, grouped by kind, with its cell, message and detail. A click goes to the cell.
fn problems(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());
    let key = problems_key(app);
    let fresh = app.python.panes.problems.as_ref().filter(|c| c.key == key).map(|c| c.rows.clone());
    let rows = fresh.unwrap_or_else(|| {
        let rows = std::sync::Arc::new(problem_rows(app));
        app.python.panes.problems = Some(ProblemsCache { key, rows: rows.clone() });
        rows
    });
    if rows.is_empty() {
        ui.label(RichText::new(lang.tr("No problems.")).italics());
        app.python.panes.rows_drawn = 0;
        return;
    }
    let mut go: Option<(usize, String)> = None;
    let mut copy: Option<Vec<String>> = None;
    let mut open_init = false;
    let mut drawn = 0;
    ScrollArea::vertical().id_salt("python_problems").auto_shrink([false, false]).show_rows(ui, PROBLEM_ROW, rows.len(), |ui, range| {
        for row in range {
            let Some(row) = rows.get(row) else { continue };
            drawn += 1;
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(width, PROBLEM_ROW), Sense::hover());
            let mut ui = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
            ui.spacing_mut().item_spacing.y = 1.0;
            match row {
                ProblemRow::Kind(kind, count) => {
                    ui.add_space(12.0);
                    ui.add(egui::Label::new(RichText::new(format!("{kind} ({count})")).font(theme::ui_bold(12.5))).truncate());
                }
                ProblemRow::Entry { place, initialization, diagnostic } => {
                    ui.horizontal(|ui| {
                        match place {
                            Some((label, sheet, cell)) => {
                                if ui.link(label).clicked() {
                                    go = Some((*sheet, cell.clone()));
                                }
                            }
                            None if *initialization => {
                                open_init |= ui.link(lang.tr(crate::python_window::INITIALIZATION)).clicked();
                            }
                            None => {
                                ui.label(RichText::new(lang.tr("This workbook")).color(t.text_dim));
                            }
                        }
                        ui.add(egui::Label::new(lang.tr(diagnostic.kind.message)).truncate());
                    });
                    ui.horizontal(|ui| {
                        let packages = super::packages_for(diagnostic);
                        if !packages.is_empty() && ui.small_button(lang.tr("Copy Install Command")).clicked() {
                            copy = Some(packages);
                        }
                        if let Some(line) = diagnostic.line.filter(|_| *initialization) {
                            let text = lang.tr("Line {line}").replace("{line}", &line.to_string());
                            ui.add(egui::Label::new(RichText::new(text).font(theme::ui_font(11.5)).color(t.text_dim)));
                        }
                        if !diagnostic.detail.is_empty() {
                            let line = first_line(&diagnostic.detail);
                            ui.add(egui::Label::new(RichText::new(line).font(theme::ui_font(11.5)).color(t.text_dim)).truncate());
                        }
                    });
                }
            }
        }
    });
    app.python.panes.rows_drawn = drawn;
    if let Some(packages) = copy {
        let names: Vec<&str> = packages.iter().map(String::as_str).collect();
        let command = super::install_command(app.session.python_report().interpreter.as_ref(), &names);
        super::copy_command(app, ui.ctx(), &command);
    }
    if let Some((sheet, cell)) = go {
        go_to(app, sheet, &cell);
    }
    if open_init {
        crate::python_window::open_initialization(app);
    }
}

/// Goes to a cell, through the command, as the circular references menu does.
fn go_to(app: &mut SheetApp, sheet: impl Into<serde_json::Value>, cell: &str) {
    app.run_or_alert("selection.set", json!({"range": cell, "sheet": sheet.into()}));
    app.grid.ensure_visible = true;
}

/// Each run's cell, duration, diagnostic and printed output, newest first. A click on a run shows
/// all of what it printed.
fn run_log(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());
    if ui.button(lang.tr("Clear")).clicked() {
        app.run_or_alert("python.clearRunLog", json!({}));
        app.python.panes.selected_run = None;
    }
    let selected = app.python.panes.selected_run.clone();
    let runs: Vec<&gridcraft_engine::PythonRun> = app.session.python_run_log().collect();
    if runs.is_empty() {
        ui.label(RichText::new(lang.tr("No runs yet.")).italics());
        app.python.panes.rows_drawn = 0;
        return;
    }
    let key_of = |r: &gridcraft_engine::PythonRun| (r.sheet.clone(), r.cell.a1(), r.duration);
    let place_of = |r: &gridcraft_engine::PythonRun| {
        if r.initialization {
            lang.tr(crate::python_window::INITIALIZATION).to_string()
        } else {
            format!("{}!{}", gridcraft_engine::formula::quote_sheet(&r.sheet), r.cell.a1())
        }
    };
    let count = runs.len();
    let mut pick = None;
    let mut go = None;
    let mut open_init = false;
    let mut drawn = 0;
    let detail = selected.as_ref().and_then(|s| runs.iter().find(|r| key_of(r) == *s));
    let list_height = if detail.is_some() { (ui.available_height() * 0.5).max(RUN_ROW * 3.0) } else { f32::INFINITY };
    ScrollArea::vertical().id_salt("python_run_log").max_height(list_height).auto_shrink([false, false]).show_rows(
        ui,
        RUN_ROW,
        count,
        |ui, range| {
            for row in range {
                // Newest first.
                let Some(run) = count.checked_sub(row + 1).and_then(|i| runs.get(i)) else { continue };
                drawn += 1;
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), RUN_ROW), Sense::click());
                let place = place_of(run);
                let is_selected = selected.as_ref().is_some_and(|s| *s == key_of(run));
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, is_selected, &place));
                if is_selected {
                    ui.painter().rect_filled(rect, 3.0, t.pressed);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 3.0, t.hover);
                }
                if resp.clicked() {
                    pick = Some(key_of(run));
                }
                let mut ui = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(3.0, 1.0))).layout(Layout::top_down(Align::Min)));
                ui.spacing_mut().item_spacing.y = 1.0;
                let duration = format!("{} ms", run.duration.as_millis());
                let id = run.diagnostic.unwrap_or_default();
                ui.add(
                    egui::Label::new(RichText::new(format!("{place}   {duration}   {id}")).font(theme::ui_font(12.5))).truncate().selectable(false),
                );
                ui.add(egui::Label::new(RichText::new(first_line(&run.stdout)).monospace().color(t.text_dim)).truncate().selectable(false));
            }
        },
    );
    if let Some(run) = detail {
        ui.separator();
        if run.initialization {
            open_init = ui.button(lang.tr("Open Initialization")).clicked();
        } else if ui.button(lang.tr("Go To Cell")).clicked() {
            go = Some((run.sheet.clone(), run.cell.a1()));
        }
        ScrollArea::vertical().id_salt("python_run_output").auto_shrink([false, false]).show(ui, |ui| {
            if run.stdout.is_empty() {
                ui.label(RichText::new(lang.tr("This run printed nothing.")).italics());
            } else {
                ui.add(egui::Label::new(RichText::new(&run.stdout).monospace()).wrap());
            }
            if run.stdout_cut {
                ui.label(RichText::new(lang.tr("The printed output was cut off at its limit.")).italics());
            }
        });
    }
    app.python.panes.rows_drawn = drawn;
    if let Some(pick) = pick {
        app.python.panes.selected_run = Some(pick);
    }
    if let Some((sheet, cell)) = go {
        go_to(app, sheet, &cell);
    }
    if open_init {
        crate::python_window::open_initialization(app);
    }
}

/// One line of the permissions lists: the mark, the words, and what enforces it here.
fn permission(ui: &mut Ui, lang: Language, can: bool, text: &str, by: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), r, if can { Icon::Check } else { Icon::Close }, if can { t.text } else { t.danger });
        ui.vertical(|ui| {
            ui.label(lang.tr(text));
            ui.label(RichText::new(lang.tr(by)).font(theme::ui_font(11.5)).color(t.text_dim));
        });
    });
}

/// What Python can and can't do, each with what enforces it on this system, and the workbook's
/// switch.
fn permissions(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    if let Some(why) = super::system_blocked(app) {
        ui.label(why);
        ui.separator();
    }
    heading(ui, lang, "Python cells can");
    let files = super::enforcer(Guard::Files);
    let system = super::enforcer(Guard::System);
    let environment = super::enforcer(Guard::Environment);
    permission(ui, lang, true, "Read cells through xl()", "GridCraft sends only the cells that xl() names");
    permission(ui, lang, true, "Import installed packages", "The sandbox lets Python read its own installation");
    permission(ui, lang, true, "Compute", "Within the time and memory limits in File › Options › Python");
    permission(ui, lang, true, "Print", "Printed output is kept in the Run Log");
    permission(ui, lang, true, "Use threads", "Threads stay inside the one Python process");
    heading(ui, lang, "Python cells cannot");
    permission(ui, lang, false, "Read your files", files);
    permission(ui, lang, false, "Write files", files);
    permission(ui, lang, false, "Use the network", system);
    permission(ui, lang, false, "Start programs", system);
    permission(ui, lang, false, "See environment variables", environment);
    permission(ui, lang, false, "Open windows", system);
    permission(ui, lang, false, "Draw plots. The data goes to a sheet and a chart is built from it.", files);
    ui.add_space(10.0);
    workbook_switch(app, ui);
}

/// "This Workbook": the switch the Enable bar also sets, for a person only.
pub(crate) fn workbook_switch(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let ctx = ui.ctx().clone();
    let mut on = app.session.python_workbook_enabled();
    let label = lang.tr("Run this workbook's Python cells");
    let locked = super::locked(app, &ctx);
    if ui.add_enabled(!locked && app.python.no_sandbox().is_none(), egui::Checkbox::new(&mut on, label)).changed() {
        super::set_workbook(app, &ctx, on);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_a_package_can_be_called_goes_into_a_command() {
        for good in ["pandas", "scikit-learn", "numpy==2.1", "ruamel.yaml", "pandas[excel]", "a>=1,<2"] {
            assert!(package_spec(good), "{good}");
        }
        for bad in ["", "; rm -rf x", "a;b", "$(x)", "a|b", "`x`", "../x", "-e", "--index-url", "a b", "a&b", "'a'"] {
            assert!(!package_spec(bad), "{bad}");
        }
    }
}
