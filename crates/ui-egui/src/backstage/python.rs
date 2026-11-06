use super::{SheetApp, Ui, dim, section, vec2};
use crate::python_ui::{self, MEMORY_LIMIT_MB, TIME_LIMIT_SECS};
use gridcraft_engine::PythonState;
use gridcraft_engine::python::{Interpreter, STARTER_PACKAGES, UV_INSTALL, uv_setup_commands};

/// File › Options › Python: the on/off switch, the interpreter, the limits and the sandbox's status.
/// These are preferences a person writes here and nothing else can (see [`crate::python_ui`]), so
/// all of it stays disabled for frames with programmatic input.
pub(super) fn python_options(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let ctx = ui.ctx().clone();
    let locked = python_ui::locked(app, &ctx);
    let blocked = python_ui::system_blocked(app);
    let mut on = app.ui.python.enabled;
    let enable = ui.add_enabled(!locked && blocked.is_none(), egui::Checkbox::new(&mut on, lang.tr("Enable Python in cells")));
    let enable = match &blocked {
        Some(why) => enable.on_disabled_hover_text(why),
        None => enable,
    };
    if enable.changed() {
        python_ui::write_settings(app, &ctx, |p| p.enabled = on);
    }
    dim(
        ui,
        lang.tr(
            "Off by default. Only you can turn this on, here: no file, macro or agent can. A workbook opened from a file also needs its own Enable.",
        ),
    );
    // These mean nothing where Python can't run.
    let off = locked || blocked.is_some();
    ui.add_space(18.0);
    interpreter(app, ui, off);
    ui.add_space(18.0);
    section(ui, lang, "Limits");
    limits(app, ui, off);
    ui.add_space(18.0);
    section(ui, lang, "Sandbox");
    python_ui::sandbox_lines(app, ui);
}

/// Automatic or a chosen path, what was found, and how to set up Python when nothing usable was.
fn interpreter(app: &mut SheetApp, ui: &mut Ui, locked: bool) {
    let lang = app.ui.language;
    let ctx = ui.ctx().clone();
    python_ui::ensure_search(app);
    ui.horizontal(|ui| {
        let label = ui.label(egui::RichText::new(lang.tr("Interpreter")).font(crate::theme::ui_bold(15.0)));
        let chosen = app.ui.python.interpreter.clone().unwrap_or_else(|| lang.tr("Automatic").to_string());
        // In a narrow window the list gives way, so Browse… stays in view.
        let width = (ui.available_width() - 90.0).clamp(120.0, 380.0);
        ui.add_enabled_ui(!locked, |ui| {
            craft_ui::ComboBox::from_id_salt("backstage_python_interpreter")
                .width(width)
                .truncate()
                .selected_text(chosen)
                .show_ui(ui, |ui| python_ui::interpreter_items(app, ui))
                .response
                .labelled_by(label.id);
        });
        let browse = ui.add_enabled(!locked && app.services.pick_interpreter.is_some(), egui::Button::new(lang.tr("Browse…")));
        if browse.clicked() {
            python_ui::browse(app, &ctx);
        }
    });
    ui.add_space(6.0);
    found(app, ui);
    let report = app.session.python_report();
    let shown = python_ui::state(app);
    let no_python = match shown {
        PythonState::NotSetUp => report.interpreter.is_none(),
        PythonState::Off => app.python.found().is_some_and(|s| s.found.is_empty()),
        _ => false,
    };
    let interp = report.interpreter.clone();
    if no_python {
        set_up(app, ui);
    }
    ui.add_space(6.0);
    other_managers(app, ui, interp.as_ref());
}

/// What Automatic, or the chosen path, came to.
fn found(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let report = app.session.python_report().clone();
    let shown = python_ui::state(app);
    if let Some(i) = &report.interpreter {
        dim(ui, lang.tr(if app.ui.python.interpreter.is_some() { "Chosen by you:" } else { "Automatic found:" }));
        ui.add(egui::Label::new(egui::RichText::new(i.path.display().to_string()).monospace()).wrap());
        ui.label(format!("Python {}, {}", i.version, python_ui::kind_label(lang, i.kind)));
    } else if python_ui::checking(app) {
        dim(ui, lang.tr("GridCraft is checking Python."));
    } else if shown == PythonState::Off {
        dim(ui, lang.tr("While Python is off, GridCraft starts nothing, so it can't yet say which version it would use."));
        if let Some(first) = app.python.found().and_then(|s| s.found.first()) {
            dim(ui, lang.tr("Automatic would try first:"));
            ui.add(egui::Label::new(egui::RichText::new(first.path.display().to_string()).monospace()).wrap());
        }
    } else if let Some(d) = python_ui::reason(app).filter(|_| shown != PythonState::Blocked) {
        ui.label(python_ui::sentence(lang, &d));
    }
}

/// No usable Python: the commands that make one with uv. GridCraft copies them; it never runs them.
fn set_up(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    ui.add_space(8.0);
    ui.label(egui::RichText::new(lang.tr("Set up Python with uv")).strong());
    dim(ui, lang.tr("Run these in a terminal, in order. GridCraft looks in this environment first, so there is nothing more to choose. It only copies the commands; it never runs them."));
    let uv = app.session.python_report().uv.clone().or_else(|| app.python.found().and_then(|s| s.uv.clone()));
    if uv.is_none() {
        python_ui::command_row(app, ui, UV_INSTALL);
    }
    for command in uv_setup_commands(&STARTER_PACKAGES) {
        python_ui::command_row(app, ui, &command);
    }
    if ui.button(lang.tr("Check Again")).clicked() {
        python_ui::check_again(app);
    }
}

/// For people who don't use uv, collapsed: any interpreter by path, and the pip command.
fn other_managers(app: &mut SheetApp, ui: &mut Ui, interpreter: Option<&Interpreter>) {
    let lang = app.ui.language;
    egui::CollapsingHeader::new(lang.tr("Other package managers")).default_open(false).show(ui, |ui| {
        dim(ui, lang.tr("Any Python 3.11 or newer can be chosen by path with Browse…. To add packages to it with pip:"));
        let command = python_ui::pip_command(interpreter, &STARTER_PACKAGES);
        python_ui::command_row(app, ui, &command);
    });
}

/// A number field for a limit: what is being typed or dragged stays a draft, and goes to the
/// preference once when that ends, since a new limit restarts Python.
fn limit_field(ui: &mut Ui, label: &str, id: &str, value: u64, range: std::ops::RangeInclusive<u64>, suffix: &str) -> Option<u64> {
    let text = ui.label(label);
    let draft = egui::Id::unique(id);
    let mut current = ui.data_mut(|d| d.get_temp::<u64>(draft)).unwrap_or(value);
    let r = ui.add(egui::DragValue::new(&mut current).range(range).suffix(suffix)).labelled_by(text.id);
    if r.drag_stopped() || r.lost_focus() {
        ui.data_mut(|d| d.remove::<u64>(draft));
        return (!ui.input(|i| i.key_pressed(egui::Key::Escape)) && current != value).then_some(current);
    }
    if r.dragged() || r.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft, current));
    } else {
        ui.data_mut(|d| d.remove::<u64>(draft));
    }
    None
}

/// The time and memory limits, through [`gridcraft_engine::python::Limits`].
fn limits(app: &mut SheetApp, ui: &mut Ui, locked: bool) {
    let lang = app.ui.language;
    let ctx = ui.ctx().clone();
    let (time, memory) = (app.ui.python.time_limit_secs, app.ui.python.memory_limit_mb);
    let mut new_time = None;
    let mut new_memory = None;
    ui.add_enabled_ui(!locked, |ui| {
        egui::Grid::new("backstage_python_limits").num_columns(2).spacing(vec2(18.0, 8.0)).show(ui, |ui| {
            new_time = limit_field(ui, lang.tr("Time limit for one cell"), "python_time_limit", time, TIME_LIMIT_SECS, lang.tr(" seconds"));
            ui.end_row();
            new_memory = limit_field(ui, lang.tr("Memory limit"), "python_memory_limit", memory, MEMORY_LIMIT_MB, lang.tr(" MB"));
            ui.end_row();
        });
    });
    if new_time.is_some() || new_memory.is_some() {
        python_ui::write_settings(app, &ctx, |p| {
            p.time_limit_secs = new_time.unwrap_or(p.time_limit_secs);
            p.memory_limit_mb = new_memory.unwrap_or(p.memory_limit_mb);
        });
    }
    dim(ui, lang.tr("At a limit Python is restarted and its variables are cleared; the next run starts fresh."));
    if cfg!(target_os = "macos") {
        dim(ui, lang.tr("On macOS the memory limit is a check twice a second, not a hard cap."));
    }
}
