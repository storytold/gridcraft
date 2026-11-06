//! The Python tab, contextual: it is up while the person is on or working with a Python cell. Code,
//! Run, Environment, Permissions and Diagnostics. Its buttons run `python.*` commands or open panes,
//! which any caller may do. The workbook's switch and the interpreter list
//! are widget code only, so they have no key tips: a key tip is something the control channel can
//! press.

use super::controls::{act, control_keytip, invoke, popup_command, sep};
use super::{Icon, SheetApp, Ui, big_button, json, small_button};
use crate::python_ui::{self, ENVIRONMENT, PACKAGES, PERMISSIONS, PROBLEMS, RUN_LOG};
use crate::widgets::small_toggle_button;
use gridcraft_engine::PythonState;
use gridcraft_engine::python::PythonCalculation;

/// A control turned off for `why`, which its tooltip gives.
fn gated(ui: &mut Ui, why: Option<&str>, add: impl FnOnce(&mut Ui) -> egui::Response) -> egui::Response {
    let response = ui.add_enabled_ui(why.is_none(), add).inner;
    match why {
        Some(why) => response.on_disabled_hover_text(why),
        None => response,
    }
}

/// The Python tab's groups, left to right. Room is left for groups to come after Code.
pub(super) fn python(app: &mut SheetApp, ui: &mut Ui) {
    code(app, ui);
    sep(ui);
    run(app, ui);
    sep(ui);
    environment(app, ui);
    sep(ui);
    permissions(app, ui);
    sep(ui);
    diagnostics(app, ui);
}

/// Initialization: opens the editor on the workbook's initialization code, asking first about unsaved
/// code in it. The Python tab and Formulas › Python have one each.
fn initialization_button(app: &mut SheetApp, ui: &mut Ui, label: &str, tip: &str) {
    let r = small_button(ui, Icon::PythonInit, label, tip, false);
    control_keytip(app, ui, &r, "python.initializationEditor");
    if r.clicked() | invoke(app, "python.initializationEditor") {
        crate::python_window::open_initialization(app);
    }
}

/// Code: Insert Python, Editor, Initialization, Output As.
fn code(app: &mut SheetApp, ui: &mut Ui) {
    let insert = big_button(ui, Icon::Python, "Insert\nPython", "Insert Python (Ctrl+Alt+Shift+P)", false);
    control_keytip(app, ui, &insert, "python.insert");
    if insert.clicked() | invoke(app, "python.insert") {
        app.insert_python();
    }
    ui.vertical(|ui| {
        let editor = small_button(ui, Icon::PythonEditor, "Editor", "Python Editor", false);
        control_keytip(app, ui, &editor, "python.editor");
        if editor.clicked() | invoke(app, "python.editor") {
            act(app, "ui.pythonEditor", json!({"on": true}));
        }
        initialization_button(app, ui, crate::python_window::INITIALIZATION, crate::python_window::INITIALIZATION_TIP);
        output_as(app, ui);
    });
    ui.vertical(|ui| display_over_cells(app, ui));
}

/// Display Plot over Cells: greyed, with the reason, on anything but a cell that gave a picture.
fn display_over_cells(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let has_picture = crate::python_card::active_python_cell(app).is_some_and(|(_, _, _, py)| py.image().is_some());
    let why = (!has_picture).then(|| lang.tr("Select a Python cell that gave a picture first."));
    let label = crate::python_plots::DISPLAY_OVER_CELLS;
    let r = gated(ui, why, |ui| small_button(ui, Icon::Picture, label, "Display Plot over Cells (Ctrl+Alt+Shift+C)", false));
    control_keytip(app, ui, &r, "python.displayOverCells");
    if has_picture && (r.clicked() | invoke(app, "python.displayOverCells")) {
        crate::python_plots::display_over_cells(app);
    }
}

/// Output As: a Python cell's return type, through `python.output`.
fn output_as(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let cell = crate::python_card::active_python_cell(app).map(|(_, sheet, at, py)| (sheet, at.a1(), py.shows_object()));
    let why = cell.is_none().then(|| lang.tr("Select a Python cell first."));
    let anchor = gated(ui, why, |ui| small_button(ui, Icon::PythonOutput, "Output As", "Output As", true));
    control_keytip(app, ui, &anchor, "python.outputAs");
    let forced = invoke(app, "python.outputAs") && cell.is_some();
    let Some((sheet, at, object)) = cell else { return };
    egui::Popup::menu(&anchor).open_memory(popup_command(forced, anchor.clicked())).show(|ui| {
        for (label, mode, on) in [(crate::python_card::PYTHON_OBJECT, "object", object), (crate::python_card::EXCEL_VALUE, "value", !object)] {
            if ui.radio(on, lang.tr(label)).clicked() {
                act(app, "python.output", json!({"cell": at, "sheet": sheet, "mode": mode}));
                ui.close();
            }
        }
    });
}

/// Run: Run Cell, Run All, Stop, Reset, Python Calculation.
fn run(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let blocked = python_ui::run_blocked(app);
    let cell = crate::python_card::active_python_cell(app).map(|(_, sheet, at, _)| (sheet, at.a1()));
    let run_cell_why = blocked.clone().or_else(|| cell.is_none().then(|| lang.tr("Select a Python cell first.").to_string()));
    let r = gated(ui, run_cell_why.as_deref(), |ui| big_button(ui, Icon::RunCell, "Run\nCell", "Run the selected Python cell", false));
    control_keytip(app, ui, &r, "python.run");
    if r.clicked()
        && let Some((sheet, at)) = &cell
    {
        act(app, "python.run", json!({"cell": at, "sheet": sheet}));
    }
    let r = gated(ui, blocked.as_deref(), |ui| big_button(ui, Icon::RunAll, "Run\nAll", "Run every Python cell in the workbook, in order", false));
    control_keytip(app, ui, &r, "python.runAll");
    if r.clicked() {
        act(app, "python.runAll", json!({}));
    }
    let system = python_ui::system_blocked(app);
    ui.vertical(|ui| {
        let stop_why = system.clone().or_else(|| (!app.session.python_busy()).then(|| lang.tr("Nothing is running.").to_string()));
        let r = gated(ui, stop_why.as_deref(), |ui| small_button(ui, Icon::StopSquare, "Stop", "Stop the running cell", false));
        control_keytip(app, ui, &r, "python.stop");
        if r.clicked() {
            act(app, "python.stop", json!({}));
        }
        reset_button(app, ui, blocked.as_deref());
        calculation(app, ui, system.as_deref());
    });
}

/// Reset: clears the variables and runs every cell.
fn reset_button(app: &mut SheetApp, ui: &mut Ui, blocked: Option<&str>) {
    let r = gated(ui, blocked, |ui| small_button(ui, Icon::PythonReset, "Reset", "Clear Python's variables and run every cell again", false));
    control_keytip(app, ui, &r, "python.reset");
    if r.clicked() {
        act(app, "python.reset", json!({}));
    }
}

/// Python Calculation: when this workbook's cells run, through `python.calculation`.
fn calculation(app: &mut SheetApp, ui: &mut Ui, blocked: Option<&str>) {
    let lang = app.ui.language;
    let mode = app.session.python_calculation();
    let anchor = gated(ui, blocked, |ui| small_button(ui, Icon::Calc, "Python Calculation", "When this workbook's Python cells run", true));
    control_keytip(app, ui, &anchor, "python.calculationMenu");
    let forced = invoke(app, "python.calculationMenu") && blocked.is_none();
    egui::Popup::menu(&anchor).open_memory(popup_command(forced, anchor.clicked())).show(|ui| {
        for (label, id, value) in [("Automatic", "automatic", PythonCalculation::Automatic), ("Manual", "manual", PythonCalculation::Manual)] {
            if ui.radio(mode == value, lang.tr(label)).clicked() {
                act(app, "python.calculation", json!({"mode": id}));
                ui.close();
            }
        }
    });
}

/// Environment: the state button, Interpreter, Packages, Check Sandbox.
fn environment(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let shown = python_ui::state(app);
    let tip = match python_ui::reason(app) {
        Some(d) => python_ui::sentence(lang, &d),
        None if python_ui::checking(app) => lang.tr("GridCraft is checking Python.").to_string(),
        None if shown == PythonState::Running => lang.tr("A Python cell is running.").to_string(),
        None => lang.tr("Python is ready.").to_string(),
    };
    let r = big_button(ui, python_ui::state_icon(shown), python_ui::state_label(shown), &tip, false);
    control_keytip(app, ui, &r, "python.state");
    if r.clicked() | invoke(app, "python.state") {
        python_ui::toggle_pane(app, ENVIRONMENT);
    }
    let system = python_ui::system_blocked(app);
    ui.vertical(|ui| {
        let anchor = gated(ui, system.as_deref(), |ui| small_button(ui, Icon::PythonInterpreter, "Interpreter", "Which Python runs cells", true));
        control_keytip(app, ui, &anchor, "python.interpreter");
        let forced = invoke(app, "python.interpreter") && system.is_none();
        egui::Popup::menu(&anchor).open_memory(popup_command(forced, anchor.clicked())).show(|ui| {
            ui.set_min_width(320.0);
            python_ui::interpreter_items(app, ui);
        });
        let r = gated(ui, system.as_deref(), |ui| small_button(ui, Icon::PythonPackages, "Packages", "What this Python has installed", false));
        control_keytip(app, ui, &r, "python.packages");
        if r.clicked() | (invoke(app, "python.packages") && system.is_none()) {
            python_ui::toggle_pane(app, PACKAGES);
        }
        let r = small_button(ui, Icon::PythonShieldCheck, "Check Sandbox", "Check Python and the sandbox again, and show the report", false);
        control_keytip(app, ui, &r, "python.checkSandbox");
        if r.clicked() | invoke(app, "python.checkSandbox") {
            python_ui::check_again(app);
            python_ui::open_pane(app, ENVIRONMENT);
        }
    });
}

/// Permissions: What Python Can Do, and this workbook's switch, which only a person can use.
fn permissions(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let ctx = ui.ctx().clone();
    let r = big_button(ui, Icon::PythonShield, "What Python\nCan Do", "What Python cells can and can't do here", false);
    control_keytip(app, ui, &r, "python.permissions");
    if r.clicked() | invoke(app, "python.permissions") {
        python_ui::toggle_pane(app, PERMISSIONS);
    }
    let on = app.session.python_workbook_enabled();
    let label = if on { "This Workbook: Enabled" } else { "This Workbook: Disabled" };
    let locked = python_ui::locked(app, &ctx);
    let why = python_ui::system_blocked(app).or_else(|| locked.then(|| lang.tr("Only a person can change this.").to_string()));
    let r =
        ui.vertical(|ui| gated(ui, why.as_deref(), |ui| small_toggle_button(ui, Icon::Lock, label, "Run this workbook's Python cells", on))).inner;
    if r.clicked() {
        python_ui::set_workbook(app, &ctx, !on);
    }
}

/// Diagnostics: Problems, Run Log, Clear.
fn diagnostics(app: &mut SheetApp, ui: &mut Ui) {
    ui.vertical(|ui| {
        problems_button(app, ui);
        let r = small_button(ui, Icon::PythonRunLog, "Run Log", "Each run's cell, time and printed output", false);
        control_keytip(app, ui, &r, "python.runLogPane");
        if r.clicked() | invoke(app, "python.runLogPane") {
            python_ui::toggle_pane(app, RUN_LOG);
        }
        let r = small_button(ui, Icon::Eraser, "Clear", "Clear the Run Log", false);
        control_keytip(app, ui, &r, "python.clearRunLog");
        if r.clicked() {
            act(app, "python.clearRunLog", json!({}));
        }
    });
}

/// Problems, the pane Formulas › Python's Diagnostics also opens.
fn problems_button(app: &mut SheetApp, ui: &mut Ui) {
    let r = small_button(ui, Icon::PythonProblems, "Problems", "Every Python cell with something to say", false);
    control_keytip(app, ui, &r, "python.problems");
    if r.clicked() | invoke(app, "python.problems") {
        python_ui::toggle_pane(app, PROBLEMS);
    }
}

/// Formulas › Python's Reset, Initialization and Diagnostics, beside Insert Python, as in Excel. They
/// keep their labels at any width: a window too narrow for the tab scrolls the ribbon.
pub(super) fn formulas_group(app: &mut SheetApp, ui: &mut Ui) {
    let blocked = python_ui::run_blocked(app);
    ui.vertical(|ui| {
        let r = gated(ui, blocked.as_deref(), |ui| small_button(ui, Icon::PythonReset, "Reset", "Reset", false));
        control_keytip(app, ui, &r, "python.reset");
        if r.clicked() {
            act(app, "python.reset", json!({}));
        }
        initialization_button(app, ui, crate::python_window::INITIALIZATION, crate::python_window::INITIALIZATION);
        let r = small_button(ui, Icon::PythonProblems, "Diagnostics", "Diagnostics", false);
        control_keytip(app, ui, &r, "python.problems");
        if r.clicked() | invoke(app, "python.problems") {
            python_ui::toggle_pane(app, PROBLEMS);
        }
    });
}
