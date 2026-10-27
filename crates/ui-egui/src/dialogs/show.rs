use super::about::about;
use super::analysis::{analysis_toolpak, goal_seek, solver, solver_results};
use super::charts::{recommended_charts, select_data};
use super::command_search::command_search;
use super::cond_format_rules::manage_rules;
use super::data::{advanced_filter_form, data_validation_form, sort};
use super::find::{find, go_to};
use super::format_cells::format_cells;
use super::insert_function::insert_function;
use super::macros::macros;
use super::names::{name_manager, paste_name};
use super::paste::{paste_special, paste_special_shortcuts};
use super::spelling::spelling;
use super::zoom::zoom;
use super::*;

/// Builds the command params from form values for dialogs that need translation.
/// The parameters OK sends to the dialog's command: its field values, reshaped per dialog.
pub(crate) fn build_params(d: &Dialog) -> Json {
    let mut v = Json::Object(d.values.clone());
    match d.name.as_str() {
        "advancedFilter" => {
            if let Some(m) = v.as_object_mut() {
                m.remove("action");
                if d.values.get("action").and_then(Json::as_str) != Some("copy") {
                    m.remove("copyTo");
                }
                if m.get("criteria").and_then(Json::as_str).is_some_and(|s| s.trim().is_empty()) {
                    m.remove("criteria");
                }
            }
        }
        "dataValidation" => {
            let kind = d.values.get("type").and_then(Json::as_str).unwrap_or("any");
            let operator = d.values.get("operator").and_then(Json::as_str).unwrap_or("between");
            if let Some(m) = v.as_object_mut() {
                if !matches!(operator, "between" | "notBetween") || matches!(kind, "any" | "list" | "custom") {
                    m.remove("formula2");
                }
                if matches!(kind, "any" | "list" | "custom") {
                    m.remove("operator");
                }
                if kind == "any" {
                    m.remove("formula1");
                }
                if kind != "list" {
                    m.remove("dropdown");
                }
            }
        }
        "textToColumns" => {
            let mut delims = Vec::new();
            for (k, c) in [("tab", "\t"), ("semicolon", ";"), ("comma", ","), ("space", " ")] {
                if d.values.get(k).and_then(Json::as_bool) == Some(true) {
                    delims.push(json!(c));
                }
            }
            v["delimiters"] = Json::Array(delims);
        }
        "removeDuplicates" | "subtotal" => {
            if let Some(t) = d.values.get("columnsText").and_then(Json::as_str).filter(|t| !t.trim().is_empty()) {
                v["columns"] = Json::Array(t.split(',').map(|c| json!(c.trim().to_ascii_uppercase())).collect());
            }
        }
        "allowEditRanges" => {
            if let Some(m) = v.as_object_mut() {
                // Blank fields mean "unchanged"; the check box clears a password.
                m.retain(|_, x| x.as_str().is_none_or(|s| !s.trim().is_empty()));
                if m.remove("removePassword").and_then(|x| x.as_bool()) == Some(true) {
                    m.insert("password".into(), json!(""));
                }
            }
        }
        "cfQuick" => {
            let mut rule = Map::new();
            for (k, val) in &d.values {
                rule.insert(k.clone(), val.clone());
            }
            v = json!({"rule": Json::Object(rule)});
            if let Some(pick) = d.pick.as_ref().filter(|pick| !pick.applies_to.is_empty() && !pick.sheet.is_empty()) {
                v["sheet"] = json!(pick.sheet.as_str());
                v["range"] = json!(pick.applies_to);
            }
        }
        "series" => {
            if let Some(s) = d.values.get("stop").and_then(Json::as_str).and_then(|s| s.parse::<f64>().ok()) {
                v["stop"] = json!(s);
            } else if let Some(m) = v.as_object_mut() {
                m.remove("stop");
            }
        }
        "defineName" => {
            if let Some(m) = v.as_object_mut() {
                for key in ["scope", "oldScope"] {
                    if m.get(key).and_then(Json::as_str) == Some("Workbook") {
                        m.remove(key);
                    }
                }
            }
        }
        _ => {}
    }
    v
}

/// Takes and draws the active dialog, dispatches its body by name, consumes picture-inbox results, runs OK/confirm commands, handles Esc/close and restores the back-dialog stack.
pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else { return };
    let home = app.ui_window();
    if d.name == "pasteSpecial" {
        paste_special_shortcuts(&mut d, ctx);
    }
    let mut open = true;
    let mut win_open = true;
    let mut confirm = false;
    let width = match d.name.as_str() {
        "formatCells" => 560.0,
        "about" => 660.0,
        "insertFunction" | "commandSearch" | "nameManager" | "manageRules" | "journal" | "agents" | "macros" => 520.0,
        "recommendedCharts" => 600.0,
        "analysisToolPak" => 420.0,
        "solver" => 460.0,
        "zoom" => 220.0,
        "pasteSpecial" => 480.0,
        _ => 380.0,
    };
    let window = egui::Window::new(app.ui.language.tr(&d.title))
        .id(egui::Id::unique("dialog").with(&d.name))
        .collapsible(false)
        .resizable(false)
        .default_width(width)
        // A default position, not an anchor: egui anchors can't be dragged, and dialogs move by their title bar.
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center() + vec2(0.0, -40.0))
        .open(&mut win_open);
    let window = match d.name.as_str() {
        "insertPicture" => window.default_height(240.0),
        "advancedFilter" => window.default_height(0.0),
        _ => window,
    };
    window.show(ctx, |ui| {
            ui.set_min_width(width - 20.0);
            match d.name.as_str() {
                "formatCells" => format_cells(app, ui, &mut d, &mut confirm),
                "dataValidation" => data_validation_form(ui, &mut d, &mut confirm, &mut open),
                "advancedFilter" => advanced_filter_form(ui, &mut d, &mut confirm, &mut open),
                "insertFunction" => insert_function(app, ui, &mut d, &mut confirm),
                "find" => find(app, ui, &mut d),
                "goTo" => go_to(app, ui, &mut d, &mut confirm),
                "zoom" => zoom(ui, &mut d, &mut confirm, &mut open),
                "sort" => sort(app, ui, &mut d, &mut confirm),
                "nameManager" => name_manager(app, ui, &mut d, &mut open),
                "pasteName" => paste_name(app, ui, &mut d, &mut confirm, &mut open),
                "manageRules" => manage_rules(app, ui),
                "recommendedCharts" => recommended_charts(app, ui, &mut d, &mut confirm, &mut open),
                "selectData" => select_data(app, ui, &mut d, &mut confirm, &mut open),
                "commandSearch" => command_search(app, ui, &mut d, &mut confirm),
                "goalSeek" => goal_seek(app, ui, &mut d, &mut confirm),
                "pasteSpecial" => paste_special(app, ui, &mut d, &mut confirm, &mut open),
                "analysisToolPak" => analysis_toolpak(app, ui, &mut d, &mut confirm, &mut open),
                "solver" => solver(ui, &mut d, &mut confirm, &mut open),
                "solverResults" => solver_results(app, ui, &mut d, &mut open),
                "spelling" => spelling(app, ui, &mut d, &mut open),
                "agents" => {
                    ui.label("GridCraft is fully drivable by agents. Every menu item, button and gesture is a command:");
                    ui.add_space(4.0);
                    ui.monospace("gridcraft --control 7979        # JSON-lines control channel\ngridcraft-cli mcp --connect 7979   # MCP bridged to this window\ngridcraft-cli mcp                   # headless MCP server");
                    ui.add_space(4.0);
                    ui.label(format!("{} engine commands are available. See docs/mcp.md and docs/control-protocol.md.", gridcraft_engine::command_specs().len()));
                }
                "about" => about(ui, &mut d),
                "journal" => {
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (id, p) in app.session.journal.iter().rev().take(500) {
                            ui.monospace(format!("{id} {p}"));
                        }
                    });
                }
                "macros" => macros(app, ui, &mut d),
                "statistics" | "accessibility" | "errorChecking" | "evaluateFormula" | "comments" => {
                    let r = if d.name == "comments" {
                        app.session.active().and_then(|doc| doc.wb.active().map(|s| json!(s.comments.iter().map(|(c, m)| json!({"cell": c.a1(), "author": m.author, "text": m.text})).collect::<Vec<_>>())))
                    } else {
                        d.result.clone()
                    };
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        ui.monospace(serde_json::to_string_pretty(&r.unwrap_or(Json::Null)).unwrap_or_default());
                    });
                }
                "insertPicture" => {
                    // A private inbox dies with this dialog; late browser results
                    // cannot affect a later picture choice or another workbook.
                    let arrived = d.picture_inbox.as_ref().and_then(|inbox| {
                        inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).pop()
                    });
                    if let Some((name, bytes)) = arrived {
                        d.picture_inbox = None;
                        if bytes.len() > 16 * 1024 * 1024 {
                            app.message = Some((d.title.clone(), "Choose a PNG or JPEG no larger than 16 MB.".into()));
                        } else {
                            d.values.remove("path");
                            d.values.insert("alt".into(), json!(name));
                            d.values.insert("base64".into(), json!(gridcraft_engine::io::base64_encode(&bytes)));
                        }
                    }
                    if app.services.pick_picture_async.is_some() {
                        let name = d.values.get("alt").and_then(Json::as_str).unwrap_or("No picture selected");
                        ui.label(name);
                    } else {
                        ui.label("Picture file path (PNG or JPEG):");
                        let mut path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
                        if ui.text_edit_singleline(&mut path).changed() {
                            d.values.remove("base64");
                            d.values.insert("path".into(), json!(path));
                        }
                    }
                    if (app.services.pick_picture.is_some() || app.services.pick_picture_async.is_some()) && ui.button("Browse…").clicked() {
                        if let Some(start) = &app.services.pick_picture_async {
                            let inbox = crate::Inbox::default();
                            start(inbox.clone());
                            d.picture_inbox = Some(inbox);
                        } else if let Some(pick) = &app.services.pick_picture
                            && let Some(path) = pick()
                        {
                            d.values.remove("base64");
                            let alt = std::path::Path::new(&path).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("");
                            d.values.insert("alt".into(), json!(alt));
                            d.values.insert("path".into(), json!(path));
                        }
                    }
                    ui.label("Description (optional):");
                    let mut alt = d.values.get("alt").and_then(Json::as_str).unwrap_or("").to_string();
                    if ui.text_edit_singleline(&mut alt).changed() {
                        d.values.insert("alt".into(), json!(alt));
                    }
                    if d.values.get("placement").and_then(Json::as_str) == Some("cell") {
                        ui.label(egui::RichText::new("Fits inside the selected cell and moves with its content.").small());
                    }
                    ok_cancel(ui, &mut confirm, &mut open);
                }
                "pickList" => {
                    let items: Vec<String> = app
                        .session
                        .active()
                        .and_then(|doc| {
                            let sh = doc.wb.active()?;
                            let a = doc.selection.active;
                            let mut seen = std::collections::BTreeSet::new();
                            let mut r = a.row;
                            while r > 0 && !sh.value(gridcraft_engine::core::CellRef::new(r - 1, a.col)).is_empty() {
                                r -= 1;
                                seen.insert(sh.value(gridcraft_engine::core::CellRef::new(r, a.col)).display());
                            }
                            Some(seen.into_iter().collect())
                        })
                        .unwrap_or_default();
                    for it in items {
                        if ui.button(&it).clicked() {
                            app.run_or_alert("cell.set", json!({"input": it}));
                            open = false;
                        }
                    }
                }
                "saveCopy" => {
                    let fmt = d.values.get("format").and_then(Json::as_str).unwrap_or("xlsx").to_string();
                    ui.label(format!("Save a copy as .{fmt}"));
                    let mut path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
                    ui.text_edit_singleline(&mut path);
                    d.values.insert("path".into(), json!(path.clone()));
                    ok_cancel(ui, &mut confirm, &mut open);
                    if confirm {
                        // A bare name has no folder: written as typed it would land in the
                        // process's working directory (`/` from the Finder, the install folder on
                        // Windows), never where the user looks. The picker chooses the folder.
                        let pick = app.services.pick_save.as_ref();
                        let target = if path.is_empty() {
                            pick.and_then(|f| f(&format!("Copy.{fmt}")))
                        } else if let Some(f) = pick.filter(|_| std::path::Path::new(&path).is_relative()) {
                            f(&if path.ends_with(&format!(".{fmt}")) { path } else { format!("{path}.{fmt}") })
                        } else {
                            Some(path)
                        };
                        if let Some(t) = target {
                            let id = match fmt.as_str() {
                                "csv" => "file.exportCsv",
                                "html" => "file.exportHtml",
                                _ => "file.saveAs",
                            };
                            app.run_or_alert(id, json!({"path": t}));
                        }
                    }
                }
                "saveChanges" => {
                    ui.label(format!("Do you want to save the changes you made to {}?", d.values.get("title").and_then(Json::as_str).unwrap_or("this workbook")));
                    let quitting = d.values.get("quit").and_then(Json::as_bool) == Some(true);
                    ui.horizontal(|ui| {
                        for (label, answer) in
                            [("Don't Save", crate::SaveAnswer::DontSave), ("Cancel", crate::SaveAnswer::Cancel), ("Save", crate::SaveAnswer::Save)]
                        {
                            if ui.button(label).clicked() && app.answer_save_changes(answer, quitting, ui.ctx()) {
                                open = false;
                            }
                        }
                    });
                }
                "start" => {
                    ui.heading("Start a new workbook");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Blank workbook").clicked() {
                            app.run_or_alert("file.new", json!({}));
                            open = false;
                        }
                        for (n, t) in gridcraft_engine::sample::SAMPLES {
                            if ui.button(*t).clicked() {
                                app.run_or_alert("file.new", json!({"sample": n}));
                                open = false;
                            }
                        }
                    });
                    ui.separator();
                    ui.label(egui::RichText::new("Recent").strong());
                    for path in app.ui.recent.clone() {
                        if ui.link(&path).clicked() {
                            app.open_path(&path);
                            open = false;
                        }
                    }
                    if ui.button("Open…").clicked() {
                        app.open_dialog("open", json!({}));
                        open = false;
                    }
                }
                _ => form(ui, &mut d, &mut confirm, &mut open),
            }
        });
    // Hyperlinks inside dialogs (About ▸ Contributors) open through the host's `open_url`, since
    // the native shell has no browser integration of its own.
    if let Some(open_url) = &app.services.open_url {
        let urls: Vec<String> = ctx.output_mut(|o| {
            let mut urls = vec![];
            o.commands.retain(|c| match c {
                egui::OutputCommand::OpenUrl(u) => {
                    urls.push(u.url.clone());
                    false
                }
                _ => true,
            });
            urls
        });
        for u in urls {
            open_url(&u);
        }
    }
    if ctx.input(|i| i.key_pressed(Key::Escape)) || !win_open {
        open = false;
    }
    if confirm && d.name == "insertPicture" && d.picture_target != app.view_key() {
        app.message = Some((d.title.clone(), "Return to the original worksheet to insert this picture, or cancel and choose another cell.".into()));
        confirm = false;
    }
    if confirm && let Some(cmd) = d.command {
        let params = build_params(&d);
        match app.run(cmd, params) {
            Ok(r) => {
                open = false;
                if d.name == "solver" {
                    app.open_dialog("solverResults", r);
                } else if d.name == "openPassword" {
                    let path = d.values.get("path").and_then(Json::as_str).map(str::to_string);
                    app.after_open(path.as_deref(), &r);
                }
            }
            // Data Analysis asks before replacing data in its output range.
            Err(e) if d.name == "analysisToolPak" && e.contains(gridcraft_engine::cmd::analysis::OVERWRITE_ERROR) => {
                d.values.insert("askOverwrite".into(), json!(true));
            }
            // A wrong password: say so, clear the field and stay open for another try.
            Err(e) if d.name == "openPassword" => {
                d.values.insert("password".into(), json!(""));
                app.message = Some((d.title.clone(), crate::clean_error(&e)));
            }
            Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
        }
    } else if confirm {
        open = false;
    }
    if app.ui_window() != home {
        // A command run from the dialog switched windows: whatever it opened shows in the new
        // window, and this dialog (or the one it returns to) stays in its own.
        let keep = if open { Some(d) } else { d.back.take().map(|b| *b) };
        if let Some(keep) = keep {
            app.return_dialog(home, keep);
        }
        return;
    }
    if let Some(child) = app.dialog.as_mut() {
        // The body opened another dialog (Name Manager › New…). It takes over; this one waits
        // behind it and comes back when it closes, as Excel returns to the Name Manager. Go To
        // hands over to Go To Special for good, as in Excel.
        if open && d.name != "goTo" {
            child.back = Some(Box::new(d));
        } else if child.back.is_none() {
            child.back = d.back.take();
        }
        return;
    }
    if open {
        app.dialog = Some(d);
    } else if let Some(back) = d.back.take() {
        app.dialog = Some(*back);
    }
}

pub(super) fn ok_cancel(ui: &mut egui::Ui, confirm: &mut bool, open: &mut bool) {
    ui.add_space(8.0);
    // A row as tall as the buttons: a right-to-left layout straight in the window takes all the
    // height it is offered, which stretched every form dialog to the screen's height.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let lang = crate::i18n::current(ui.ctx());
            let t = crate::theme::Tokens::get(ui.ctx());
            let ok = ui.add(egui::Button::new(egui::RichText::new(format!("   {}   ", lang.tr("OK"))).color(t.on_accent)).fill(t.accent));
            if ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                *confirm = true;
            }
            if ui.button(format!(" {} ", lang.tr("Cancel"))).clicked() {
                *open = false;
            }
        })
    });
}

fn form(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    form_fields(ui, d);
    ok_cancel(ui, confirm, open);
}

/// Gives keyboard focus to the first text or number field the first time a form is drawn. A
/// focused number field is in typing mode with its value selected, so Alt+H, O, W then 12 and
/// Enter sets the width, as in Excel.
/// Keeps asking until the field really has focus: a new window's first frame is an invisible
/// sizing pass where a focus request is ignored.
fn take_first_focus(d: &mut Dialog, r: &egui::Response) {
    if !d.focus_first_field {
        return;
    }
    if !r.has_focus() {
        r.request_focus();
        return;
    }
    // Focus has landed, and the field is now a text box: select its whole value so typing
    // replaces it (the field's own select-on-focus fired before it was a text box).
    let mut state = egui::text_edit::TextEditState::load(&r.ctx, r.id).unwrap_or_default();
    let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(usize::MAX / 2));
    state.cursor.set_char_range(Some(all));
    state.store(&r.ctx, r.id);
    d.focus_first_field = false;
}

/// Keeps a picking formula box's caret, and gives it the keyboard back after a grid click put a
/// reference in it (the click took the focus), its caret after the reference.
fn track_pick(pick: &mut RefPick, r: &egui::Response, typed: bool) {
    if typed {
        pick.point = None;
    }
    if pick.refocus {
        let mut state = egui::text_edit::TextEditState::load(&r.ctx, r.id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(pick.caret))));
        state.store(&r.ctx, r.id);
        r.request_focus();
        pick.refocus = false;
    } else if r.has_focus()
        && let Some(range) = egui::text_edit::TextEditState::load(&r.ctx, r.id).and_then(|state| state.cursor.char_range())
    {
        pick.caret = range.primary.index.into();
    }
}

/// Draws a dialog's `fields`, editing its values.
pub(super) fn form_fields(ui: &mut egui::Ui, d: &mut Dialog) {
    // Field labels, choices and notes are English source strings, translated as they are drawn.
    let lang = crate::i18n::current(ui.ctx());
    egui::Grid::new("form").num_columns(1).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        for f in d.fields.clone() {
            match f {
                Field::Text { key, label } => {
                    ui.vertical(|ui| {
                        let label = ui.label(lang.tr(label));
                        let mut s = d
                            .values
                            .get(key)
                            .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| if v.is_null() { String::new() } else { v.to_string() }))
                            .unwrap_or_default();
                        let multiline = key == "text";
                        let r = if multiline {
                            ui.add(egui::TextEdit::multiline(&mut s).desired_rows(4).desired_width(320.0)).labelled_by(label.id)
                        } else {
                            ui.add(egui::TextEdit::singleline(&mut s).desired_width(320.0)).labelled_by(label.id)
                        };
                        take_first_focus(d, &r);
                        if r.changed() {
                            d.values.insert(key.into(), json!(s));
                        }
                        if let Some(pick) = d.pick.as_mut().filter(|pick| pick.key == key) {
                            track_pick(pick, &r, r.changed());
                        }
                    });
                }
                Field::Password { key, label } => {
                    // Masked as typed (Excel shows dots); "Show password" reveals it on purpose.
                    ui.vertical(|ui| {
                        let label = ui.label(lang.tr(label));
                        let mut s = d.values.get(key).and_then(Json::as_str).unwrap_or_default().to_string();
                        if ui.add(egui::TextEdit::singleline(&mut s).password(!d.show_password).desired_width(320.0)).labelled_by(label.id).changed()
                        {
                            d.values.insert(key.into(), json!(s));
                        }
                        ui.checkbox(&mut d.show_password, lang.tr("Show password"));
                    });
                }
                Field::Number { key, label } => {
                    ui.horizontal(|ui| {
                        let label = ui.label(lang.tr(label));
                        let mut n = d.values.get(key).and_then(Json::as_f64).unwrap_or(0.0);
                        // Typed values count straight away: otherwise Enter or OK in the same frame
                        // confirms the old value, because a number field only commits when it loses focus.
                        let r = ui.add(egui::DragValue::new(&mut n).speed(0.5).update_while_editing(true)).labelled_by(label.id);
                        take_first_focus(d, &r);
                        if r.changed() {
                            d.values.insert(key.into(), json!(n));
                        }
                    });
                }
                Field::Choice { key, label, options } => {
                    ui.vertical(|ui| {
                        let field_label = ui.label(egui::RichText::new(lang.tr(label)).strong());
                        let cur = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
                        if d.name == "dataValidation" {
                            let selected = options.iter().find(|(val, _)| *val == cur).map(|(_, text)| lang.tr(text)).unwrap_or("");
                            egui::ComboBox::from_id_salt(("dataValidation", key))
                                .selected_text(selected)
                                .width(300.0)
                                .show_ui(ui, |ui| {
                                    for (val, text) in options {
                                        if ui.selectable_label(cur == val, lang.tr(text)).clicked() {
                                            d.set_value(key, json!(val));
                                        }
                                    }
                                })
                                .response
                                .labelled_by(field_label.id);
                        } else {
                            for (val, text) in options {
                                if ui.radio(cur == val, lang.tr(text)).clicked() {
                                    d.set_value(key, json!(val));
                                }
                            }
                        }
                    });
                }
                Field::Check { key, label } => {
                    let mut b = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
                    if ui.checkbox(&mut b, lang.tr(label)).changed() {
                        d.values.insert(key.into(), json!(b));
                    }
                }
                Field::Note(t) => {
                    // A note can carry user data (a URL, a file or sheet name); `tr` reads a `|` as a
                    // context separator, so such a note is shown as it is.
                    let text = if t.contains('|') { t.as_str() } else { lang.tr(&t) };
                    ui.label(egui::RichText::new(text).small());
                }
            }
            ui.end_row();
        }
    });
}
