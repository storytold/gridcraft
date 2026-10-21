//! Automate › Macros in the window: recording (with "Recording · 7 steps" and Stop in the status
//! bar), running a macro and answering its questions (approve it, make room for its output), the
//! Macros dialog (macros, Automate scripts and a file's VBA project), Macro Options, the Record
//! Macro dialog and macro shortcuts (Ctrl+Shift+letter, ⌘⇧letter on a Mac). Everything goes
//! through the `macro.*` and `automate.*` commands.

use egui::{Sense, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::dialogs::Dialog;
use crate::theme::{self, Tokens};

/// Shortcut rules live in the engine, so the dialogs and `macro.record` agree.
pub use gridcraft_engine::macros::{RESERVED_SHORTCUTS, parse_shortcut, shortcut_conflict, shortcut_text};

/// How many macros the Run Macro menu keeps as recent.
const RECENT: usize = 8;

/// Macro state that belongs to the window, not the workbook.
#[derive(Clone, Debug)]
pub struct MacroUi {
    /// Use Relative References: record steps relative to the selection when recording started.
    pub relative: bool,
    /// Macros run from this window, most recent first (Run Macro's menu).
    pub recent: Vec<String>,
    /// A question from a macro command, answered with one of its buttons.
    pub question: Option<Question>,
    /// The step editor, while it is open.
    pub editor: Option<crate::macro_editor::Editor>,
    /// A debugger result from a question's button, for the editor to take in.
    pub debug_result: Option<Json>,
}

impl Default for MacroUi {
    fn default() -> Self {
        MacroUi { relative: true, recent: vec![], question: None, editor: None, debug_result: None }
    }
}

impl MacroUi {
    fn note_recent(&mut self, name: &str) {
        self.recent.retain(|n| !n.eq_ignore_ascii_case(name));
        self.recent.insert(0, name.to_string());
        self.recent.truncate(RECENT);
    }
}

/// A question from a command (approve this macro? overwrite the data in the way?). Each choice
/// runs the command again with its parameters added; Cancel drops it.
#[derive(Clone, Debug)]
pub struct Question {
    pub title: String,
    pub text: String,
    pub command: String,
    pub params: Json,
    pub choices: Vec<(String, Json)>,
}

/// The macros of the active workbook: (name, description, shortcut).
pub fn macro_list(app: &SheetApp) -> Vec<(String, String, Option<String>)> {
    app.session
        .active()
        .map(|d| d.wb.macros.iter().map(|m| (m.name.clone(), m.description.clone(), m.shortcut.clone())).collect())
        .unwrap_or_default()
}

/// How many commands the recorder has captured, while recording.
pub fn recorded_steps(app: &SheetApp) -> Option<usize> {
    app.session.recorder.as_ref().map(|r| r.steps.len().saturating_add(r.dropped))
}

/// Record Macro / Stop Recording: opens the Record Macro dialog, or stops and saves what was
/// recorded as a macro.
pub fn toggle_recording(app: &mut SheetApp) {
    if app.session.is_recording() {
        stop_recording(app);
    } else {
        app.open_dialog("recordMacro", json!({}));
    }
}

/// Stops recording and says what was saved.
pub fn stop_recording(app: &mut SheetApp) {
    match app.run("macro.record", json!({"stop": true})) {
        Ok(v) => match v["saved"].as_str() {
            Some(name) => {
                let steps = v["steps"].as_u64().unwrap_or(0);
                app.macros.note_recent(name);
                app.toast = Some((format!("Saved macro “{name}” ({steps} step{})", if steps == 1 { "" } else { "s" }), crate::now_ms()));
            }
            None => app.toast = Some(("Recording stopped".into(), crate::now_ms())),
        },
        Err(e) => app.message = Some(("Record Macro".into(), crate::clean_error(&e))),
    }
}

/// Runs macro `name` on the selection; its questions (approve, make room) become a prompt.
pub fn run_macro(app: &mut SheetApp, name: &str) {
    call(app, "Run Macro", "macro.run", json!({"name": name}));
}

/// Runs a macro command. Errors are shown; a question (approve it, overwrite or move the data in
/// the way) becomes a prompt whose buttons run it again. `Some` with the result when it ran.
pub fn call(app: &mut SheetApp, title: &str, id: &str, params: Json) -> Option<Json> {
    match app.run(id, params.clone()) {
        Ok(v) => {
            note_result(app, id, &params, &v);
            Some(v)
        }
        Err(e) => {
            if !ask(app, title, id, &params, &e) {
                app.message = Some((title.into(), crate::clean_error(&e)));
            }
            None
        }
    }
}

/// The question in an engine error that asks something: `approve: …` or `confirm: …`.
fn question_text(e: &str, prefix: &str) -> Option<String> {
    let rest = e.split_once(prefix)?.1;
    Some(rest.split_once(" (run again with").map(|(q, _)| q).unwrap_or(rest).to_string())
}

/// Turns a question from command `id` into a prompt whose buttons run it again: Run for a macro
/// that needs approval; Overwrite, Shift Down or Shift Right for a macro whose output would land
/// on data; OK (`overwrite: true`) for anything else. False when `e` is not a question.
pub fn ask(app: &mut SheetApp, title: &str, id: &str, params: &Json, e: &str) -> bool {
    let (text, choices) = if let Some(q) = question_text(e, "approve: ") {
        (q, vec![("Run".to_string(), json!({"approve": true}))])
    } else if let Some(q) = question_text(e, "confirm: ") {
        if id.starts_with("macro.") {
            // The engine's hint for programs ("run again with onConflict …") becomes the buttons.
            let what = q.split_once(" Overwrite them").map(|(a, _)| a).unwrap_or(&q).trim_end_matches(['.', ' ']);
            (
                format!("{what}. Overwrite them, or move the data out of the way?"),
                vec![
                    ("Overwrite".to_string(), json!({"onConflict": "overwrite"})),
                    ("Shift Down".to_string(), json!({"onConflict": "shiftDown"})),
                    ("Shift Right".to_string(), json!({"onConflict": "shiftRight"})),
                ],
            )
        } else {
            (q, vec![("OK".to_string(), json!({"overwrite": true}))])
        }
    } else {
        return false;
    };
    app.macros.question = Some(Question { title: title.into(), text, command: id.into(), params: params.clone(), choices });
    true
}

/// After a macro command ran (directly or from a prompt's button): remember the macro and say
/// what it did.
pub fn note_result(app: &mut SheetApp, id: &str, params: &Json, v: &Json) {
    if id == "macro.debug" {
        app.macros.debug_result = Some(v.clone());
        return;
    }
    if id != "macro.run" {
        return;
    }
    let name = v["macro"].as_str().or(params["name"].as_str()).unwrap_or_default().to_string();
    if name.is_empty() {
        return;
    }
    app.macros.note_recent(&name);
    let ran = v["ran"].as_u64().unwrap_or(0);
    let text = match v["stopped"].as_str() {
        Some(msg) => format!("Macro “{name}” stopped: {msg}"),
        None => format!("Ran macro “{name}” ({ran} step{})", if ran == 1 { "" } else { "s" }),
    };
    app.toast = Some((text, crate::now_ms()));
}

/// The prompt for [`MacroUi::question`]: its text, a button for each choice and Cancel.
pub fn question_box(app: &mut SheetApp, ctx: &egui::Context) {
    if app.message.is_some() {
        return;
    }
    let Some(q) = app.macros.question.clone() else { return };
    let mut answer: Option<Option<Json>> = None;
    egui::Modal::new(egui::Id::new("gridcraft.macro_question")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.label(egui::RichText::new(&q.title).strong());
        ui.add(egui::Label::new(&q.text).wrap());
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                answer = Some(None);
            }
            // Right to left: list the choices reversed, so the first one ends up next to Cancel
            // and Enter picks it.
            for (k, (label, extra)) in q.choices.iter().enumerate().rev() {
                if ui.add(egui::Button::new(label.as_str()).min_size(vec2(72.0, 0.0))).clicked()
                    || (k == 0 && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    answer = Some(Some(extra.clone()));
                }
            }
        });
    });
    match answer {
        Some(Some(extra)) => {
            app.macros.question = None;
            let mut params = q.params.clone();
            if let (Some(m), Some(extra)) = (params.as_object_mut(), extra.as_object()) {
                m.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            // A second question (approve, then make room) asks again.
            call(app, &q.title, &q.command, params);
        }
        Some(None) => app.macros.question = None,
        None => {}
    }
}

/// The status bar while a macro is debugged ("Debugging “Name”", with Show when the editor is
/// closed) and while recording (a red dot, "Recording · 7 steps" and Stop).
pub fn status(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    if let Some(name) = app.session.macro_debug.as_ref().map(|d| d.name.clone()) {
        ui.add_space(12.0);
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.5, t.breakpoint);
        ui.label(egui::RichText::new(format!("Debugging “{name}”")).font(theme::ui_font(12.5)).color(t.text));
        if app.macros.editor.is_none() {
            let show =
                ui.add(egui::Button::new(egui::RichText::new(app.ui.language.tr("Show")).font(theme::ui_font(12.0))).min_size(vec2(40.0, 18.0)));
            if show.on_hover_text("Open the macro in the step editor").clicked() {
                app.open_dialog("macroEditor", json!({"macro": name}));
            }
        }
    }
    let Some(n) = recorded_steps(app) else { return };
    ui.add_space(12.0);
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 4.5, t.recording);
    let text = format!("{} · {n} step{}", app.ui.language.tr("Recording"), if n == 1 { "" } else { "s" });
    ui.label(egui::RichText::new(text).font(theme::ui_font(12.5)).color(t.text));
    let stop = ui.add(egui::Button::new(egui::RichText::new(app.ui.language.tr("Stop")).font(theme::ui_font(12.0))).min_size(vec2(40.0, 18.0)));
    if stop.on_hover_text("Stop recording and save the macro").clicked() {
        stop_recording(app);
    }
}

// ---------------------------------------------------------------- shortcuts

/// The macro whose shortcut is Ctrl+Shift+`key` in the active workbook. GridCraft's own
/// shortcuts win over a macro's (a file can carry any letter).
pub fn shortcut_macro(app: &SheetApp, key: egui::Key) -> Option<String> {
    let letter = parse_shortcut(key.name())?;
    if RESERVED_SHORTCUTS.iter().any(|(c, _)| *c == letter) {
        return None;
    }
    let wb = &app.session.active()?.wb;
    wb.macros.iter().find(|m| m.shortcut.as_deref().and_then(parse_shortcut) == Some(letter)).map(|m| m.name.clone())
}

/// Ctrl+Shift+letter on the grid: runs the macro with that shortcut. True when one ran.
pub fn run_shortcut(app: &mut SheetApp, key: egui::Key) -> bool {
    let Some(name) = shortcut_macro(app, key) else { return false };
    run_macro(app, &name);
    true
}

// ---------------------------------------------------------------- Macros dialog

/// What a row of the Macros dialog is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Macro,
    /// An Automate script (`automate.*`).
    Script,
}

impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::Macro => "macro",
            Kind::Script => "script",
        }
    }
}

/// A row of the Macros dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub description: String,
    pub shortcut: Option<String>,
    pub steps: u64,
}

/// The Macros dialog's rows: the workbook's macros, then its Automate scripts (from
/// `view.macros`, kept in the dialog's result), and whether the file carries a VBA project.
pub fn entries(app: &SheetApp, d: &Dialog) -> (Vec<Entry>, bool) {
    let mut out: Vec<Entry> = app
        .session
        .active()
        .map(|doc| {
            doc.wb
                .macros
                .iter()
                .map(|m| Entry {
                    name: m.name.clone(),
                    kind: Kind::Macro,
                    description: m.description.clone(),
                    shortcut: m.shortcut.clone(),
                    steps: m.step_count() as u64,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut vba = false;
    for it in d.result.as_ref().and_then(Json::as_array).into_iter().flatten() {
        if it.get("kind").and_then(Json::as_str) == Some("vba") {
            vba = true;
            continue;
        }
        let Some(name) = it.get("name").and_then(Json::as_str) else { continue };
        let steps = it.get("steps").and_then(Json::as_u64).unwrap_or(0);
        out.push(Entry { name: name.to_string(), kind: Kind::Script, description: String::new(), shortcut: None, steps });
    }
    (out, vba)
}

/// The Macros dialog (View › Macros, Automate › Macros, Alt+F8). Its result holds `view.macros`
/// (the Automate scripts and a file's VBA project), as before macros existed.
pub fn open_dialog(app: &mut SheetApp, params: &Json) -> Dialog {
    let names = macro_list(app);
    let first = app.macros.recent.iter().find(|r| names.iter().any(|(n, ..)| n.eq_ignore_ascii_case(r))).cloned();
    let name = params["name"].as_str().map(str::to_string).or(first).or_else(|| names.first().map(|m| m.0.clone())).unwrap_or_default();
    let mut d = Dialog::custom("macros", "Macros", json!({"name": name, "kind": "macro"}));
    d.result = app.session.run("view.macros", json!({})).ok();
    d
}

/// The Macros dialog's body: the name box and the list on the left, the buttons on the right, the
/// description under them; Options… switches to the shortcut and description of one macro.
pub fn macros_dialog(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    // Scripts can change while the dialog is open (Delete): read them again when the workbook does.
    let rev = app.session.active().map(|doc| doc.revision).unwrap_or(0);
    if d.values.get("revision").and_then(Json::as_u64) != Some(rev) {
        d.result = app.session.run("view.macros", json!({})).ok();
        d.values.insert("revision".into(), json!(rev));
    }
    if d.values.get("options").and_then(Json::as_bool) == Some(true) {
        options(app, ui, d);
        return;
    }
    let (list, vba) = entries(app, d);
    let mut name = d.values.get("name").and_then(Json::as_str).unwrap_or("").to_string();
    let mut kind = if d.values.get("kind").and_then(Json::as_str) == Some("script") { Kind::Script } else { Kind::Macro };
    let t = Tokens::get(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(330.0);
            ui.label("Macro name:");
            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(330.0).id_salt("macro_name")).changed() {
                kind = Kind::Macro;
            }
            egui::Frame::NONE.fill(t.input_bg).stroke(egui::Stroke::new(1.0, t.input_border)).inner_margin(2.0).show(ui, |ui| {
                ui.set_min_size(vec2(326.0, 200.0));
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    if list.is_empty() {
                        ui.label(egui::RichText::new("This workbook has no scripts or macros.").italics().color(t.text_dim));
                    }
                    for e in &list {
                        let label = match (e.kind, &e.shortcut) {
                            (Kind::Macro, Some(k)) => format!("{}    {k}", e.name),
                            (Kind::Macro, None) => e.name.clone(),
                            (Kind::Script, _) => format!("{}    (script, {} step{})", e.name, e.steps, if e.steps == 1 { "" } else { "s" }),
                        };
                        let selected = e.kind == kind && e.name.eq_ignore_ascii_case(name.trim());
                        let r = ui.add(egui::Button::selectable(selected, label).min_size(vec2(322.0, 20.0)));
                        if r.clicked() {
                            name = e.name.clone();
                            kind = e.kind;
                        }
                        if r.double_clicked() {
                            *open = false;
                            run_entry(app, e);
                        }
                    }
                });
            });
        });
        ui.vertical(|ui| {
            ui.add_space(18.0);
            let w = vec2(100.0, 24.0);
            let target = name.trim().to_string();
            let found = list.iter().find(|e| e.kind == kind && e.name.eq_ignore_ascii_case(&target)).cloned();
            let is_macro = found.as_ref().is_some_and(|e| e.kind == Kind::Macro);
            if ui.add_enabled(found.is_some(), egui::Button::new("Run").min_size(w)).clicked()
                && let Some(e) = &found
            {
                *open = false;
                run_entry(app, e);
            }
            if ui
                .add_enabled(is_macro, egui::Button::new("Step Into").min_size(w))
                .on_hover_text("Open the macro in the step editor and stop before its first step")
                .clicked()
            {
                *open = false;
                app.open_dialog("macroEditor", json!({"macro": target, "debug": true}));
            }
            if ui.add_enabled(is_macro, egui::Button::new("Edit").min_size(w)).clicked() {
                *open = false;
                app.open_dialog("macroEditor", json!({"macro": target}));
            }
            let free = !target.is_empty() && !list.iter().any(|e| e.kind == Kind::Macro && e.name.eq_ignore_ascii_case(&target));
            if ui.add_enabled(free, egui::Button::new("Create").min_size(w)).on_hover_text("Write a new macro with this name").clicked() {
                *open = false;
                app.open_dialog("macroEditor", json!({"new": target}));
            }
            if ui.add_enabled(found.is_some(), egui::Button::new("Delete").min_size(w)).clicked()
                && let Some(e) = &found
            {
                let (title, command, what) = match e.kind {
                    Kind::Macro => ("Delete Macro", "macro.delete", "macro"),
                    Kind::Script => ("Delete Script", "automate.deleteScript", "script"),
                };
                app.macros.question = Some(Question {
                    title: title.into(),
                    text: format!("Do you want to delete {what} “{}”?", e.name),
                    command: command.into(),
                    params: json!({"name": e.name}),
                    choices: vec![("Delete".into(), json!({}))],
                });
            }
            if ui.add_enabled(is_macro, egui::Button::new("Options…").min_size(w)).clicked()
                && let Some(e) = &found
            {
                let key = e.shortcut.as_deref().and_then(parse_shortcut).map(String::from).unwrap_or_default();
                d.values.insert("options".into(), json!(true));
                d.values.insert("shortcut".into(), json!(key));
                d.values.insert("description".into(), json!(e.description));
            }
            ui.add_space(12.0);
            if ui.add(egui::Button::new("Cancel").min_size(w)).clicked() {
                *open = false;
            }
        });
    });
    ui.add_space(6.0);
    ui.label("Description");
    let desc = list.iter().find(|e| e.kind == kind && e.name.eq_ignore_ascii_case(name.trim())).map(|e| e.description.clone()).unwrap_or_default();
    ui.label(egui::RichText::new(if desc.is_empty() { " ".to_string() } else { desc }).color(t.text_dim));
    if vba {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("This workbook also has a VBA project. GridCraft keeps it when you save but does not run it.").color(t.text_dim),
        );
    }
    d.values.insert("name".into(), json!(name));
    d.values.insert("kind".into(), json!(kind.key()));
}

/// Runs a row of the Macros dialog: a macro on the selection, or an Automate script.
fn run_entry(app: &mut SheetApp, e: &Entry) {
    match e.kind {
        Kind::Macro => run_macro(app, &e.name),
        Kind::Script => {
            call(app, "Run Script", "automate.runScript", json!({"name": e.name}));
        }
    }
}

/// Macro Options: the shortcut letter and the description of the selected macro.
fn options(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog) {
    let name = d.values.get("name").and_then(Json::as_str).unwrap_or("").trim().to_string();
    ui.label(egui::RichText::new(format!("Macro name: {name}")).strong());
    ui.add_space(4.0);
    ui.label("Shortcut key:");
    let mut key = d.values.get("shortcut").and_then(Json::as_str).unwrap_or("").to_string();
    shortcut_field(ui, &mut key, "macro_shortcut");
    let letter = parse_shortcut(&key);
    let conflict = match (key.trim().is_empty(), letter, app.session.active()) {
        (true, ..) => None,
        (false, None, _) => Some("Use a letter from A to Z.".to_string()),
        (false, Some(c), Some(doc)) => shortcut_conflict(&doc.wb, &name, c),
        (false, Some(_), None) => None,
    };
    if let Some(c) = &conflict {
        ui.label(egui::RichText::new(c).color(Tokens::get(ui.ctx()).danger));
    }
    ui.add_space(4.0);
    ui.label("Description:");
    let mut desc = d.values.get("description").and_then(Json::as_str).unwrap_or("").to_string();
    ui.add(egui::TextEdit::multiline(&mut desc).desired_rows(3).desired_width(f32::INFINITY).id_salt("macro_description"));
    ui.add_space(6.0);
    let mut done = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Cancel").clicked() {
            done = true;
        }
        if ui.add_enabled(conflict.is_none(), egui::Button::new("OK")).clicked() {
            done = save_options(app, &name, letter, &desc);
        }
    });
    d.values.insert("shortcut".into(), json!(key));
    d.values.insert("description".into(), json!(desc));
    if done {
        d.values.remove("options");
    }
}

/// The shortcut letter box, after "Ctrl+Shift+" (or "⌘⇧" on a Mac). Keeps one capital letter.
fn shortcut_field(ui: &mut Ui, key: &mut String, id: &str) {
    ui.horizontal(|ui| {
        ui.label(if ui.ctx().os() == egui::os::OperatingSystem::Mac { "⌘ ⇧ +" } else { "Ctrl+Shift+" });
        ui.add(egui::TextEdit::singleline(key).char_limit(1).desired_width(24.0).id_salt(id));
    });
    *key = key.to_ascii_uppercase();
}

/// Saves a macro's shortcut and description. True when saved (otherwise the error is shown).
pub fn save_options(app: &mut SheetApp, name: &str, letter: Option<char>, description: &str) -> bool {
    let mut m = match app.run("macro.get", json!({"name": name})) {
        Ok(m) if m.is_object() => m,
        Ok(_) => return false,
        Err(e) => {
            app.message = Some(("Macro Options".into(), crate::clean_error(&e)));
            return false;
        }
    };
    if let Some(o) = m.as_object_mut() {
        match letter {
            Some(c) => o.insert("shortcut".into(), json!(shortcut_text(c))),
            None => o.remove("shortcut"),
        };
        o.insert("description".into(), json!(description));
    }
    match app.run("macro.save", json!({"macro": m})) {
        Ok(_) => true,
        Err(e) => {
            app.message = Some(("Macro Options".into(), crate::clean_error(&e)));
            false
        }
    }
}

// ---------------------------------------------------------------- Record Macro dialog

/// Record Macro: the macro's name, shortcut and description before recording starts. While
/// recording, it stops instead (View › Macros › Record Macro toggles, as in Excel).
pub fn open_record(app: &mut SheetApp, params: &Json) -> Option<Dialog> {
    if app.session.is_recording() {
        stop_recording(app);
        return None;
    }
    let suggested = app.session.active().map(|d| gridcraft_engine::macros::next_name(&d.wb)).unwrap_or_else(|| "Macro1".into());
    let name = params["name"].as_str().map(str::to_string).filter(|n| !n.trim().is_empty()).unwrap_or(suggested);
    Some(Dialog::custom("recordMacro", "Record Macro", json!({"name": name, "shortcut": "", "description": ""})))
}

/// What is wrong with the Record Macro dialog's name or shortcut, if anything.
pub fn record_problem(app: &SheetApp, name: &str, key: &str) -> Option<String> {
    let doc = app.session.active()?;
    if let Err(e) = gridcraft_engine::macros::check_new_name(&doc.wb, name) {
        return Some(e);
    }
    if key.trim().is_empty() {
        return None;
    }
    match parse_shortcut(key) {
        None => Some("Use a letter from A to Z for the shortcut.".into()),
        Some(c) => shortcut_conflict(&doc.wb, name, c),
    }
}

/// The Record Macro dialog's body. OK starts recording; it stays off while the name or the
/// shortcut is not usable, and the reason shows under the fields.
pub fn record_dialog(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let get = |d: &Dialog, k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let mut name = get(d, "name");
    let mut key = get(d, "shortcut");
    let mut desc = get(d, "description");
    ui.label("Macro name:");
    ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY).id_salt("record_name"));
    ui.add_space(4.0);
    ui.label("Shortcut key:");
    shortcut_field(ui, &mut key, "record_shortcut");
    ui.add_space(4.0);
    ui.label("Description:");
    ui.add(egui::TextEdit::multiline(&mut desc).desired_rows(3).desired_width(f32::INFINITY).id_salt("record_description"));
    ui.checkbox(&mut app.macros.relative, "Use Relative References");
    let problem = record_problem(app, &name, &key);
    if let Some(p) = &problem {
        ui.label(egui::RichText::new(p).color(Tokens::get(ui.ctx()).danger));
    }
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Cancel").clicked() {
            *open = false;
        }
        if ui.add_enabled(problem.is_none(), egui::Button::new("OK")).clicked() {
            let mut p = json!({"name": name.trim(), "description": desc, "relative": app.macros.relative});
            if !key.trim().is_empty() {
                p["shortcut"] = json!(key.trim());
            }
            match app.run("macro.record", p) {
                Ok(_) => *open = false,
                Err(e) => app.message = Some(("Record Macro".into(), crate::clean_error(&e))),
            }
        }
    });
    d.values.insert("name".into(), json!(name));
    d.values.insert("shortcut".into(), json!(key));
    d.values.insert("description".into(), json!(desc));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questions_are_read_from_the_error() {
        let e = "approve: Run macro 'M'? It came with a workbook you opened. (run again with `approve: true`)";
        assert_eq!(question_text(e, "approve: ").as_deref(), Some("Run macro 'M'? It came with a workbook you opened."));
        assert_eq!(question_text("confirm: Replace it? (run again with `overwrite: true`)", "approve: "), None);
        assert_eq!(question_text("confirm: Replace it?", "confirm: ").as_deref(), Some("Replace it?"));
    }
}
