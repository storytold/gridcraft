//! Form controls in the window, apart from drawing them: placing a new control (draw it on the
//! grid, or click for the default size), pressing a button and answering its questions, the
//! control's context menu (Edit Text, Assign Action, Format Control, Delete), and the Assign
//! Action and Format Control dialogs. Everything goes through the `insert.*` and `control.*`
//! commands.

use egui::{Pos2, Rect, Stroke, StrokeKind, Ui, vec2};
use gridcraft_engine::core::CellRef;
use gridcraft_engine::macros::policy::{Policy, macro_policy};
use gridcraft_engine::model::{CONTROL_MAX, ControlAction, ControlKind, FormControl, MacroInput, ObjKind, ObjRef, Sheet};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::dialogs::Dialog;
use crate::grid::Geo;
use crate::theme::Tokens;

/// The control `id` on the active sheet.
pub fn control(app: &SheetApp, id: u32) -> Option<FormControl> {
    app.session.active()?.wb.active()?.controls.iter().find(|c| c.id == id).cloned()
}

/// Whether the control `id` on the active sheet is a button (its menu adds Edit Text and Assign
/// Action).
pub fn is_button(app: &SheetApp, id: u32) -> bool {
    app.session.active().and_then(|d| d.wb.active()).is_some_and(|sh| sh.controls.iter().any(|c| c.id == id && c.kind == ControlKind::Button))
}

fn insert_id(kind: ControlKind) -> &'static str {
    match kind {
        ControlKind::Button => "insert.button",
        ControlKind::Slider => "insert.slider",
        ControlKind::Spin => "insert.spinButton",
    }
}

/// The cell under a logical grid position and the offset into it, in sheet points.
fn cell_point(geo: &Geo, sh: &Sheet, p: Pos2) -> (CellRef, f32, f32) {
    let (col, row) = (geo.col_at(sh, p.x), geo.row_at(sh, p.y));
    let dx = ((p.x - geo.x(sh, col)) / geo.z).max(0.0);
    let dy = ((p.y - geo.y(sh, row)) / geo.z).max(0.0);
    (CellRef::new(row, col), dx, dy)
}

/// Insert › Controls placement: while a kind is armed, a drag on the grid draws the control and a
/// click places it at its default size. Escape disarms. True when it used the pointer.
pub fn place(app: &mut SheetApp, ui: &Ui, resp: &egui::Response, geo: &Geo, sh: &Sheet) -> bool {
    let Some(kind) = app.grid.placing else {
        app.grid.place_start = None;
        return false;
    };
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.grid.placing = None;
        app.grid.place_start = None;
        return true;
    }
    let pointer = ctx.input(|i| i.pointer.clone());
    // Logical positions (see `Geo::rtl`).
    let pos = pointer.interact_pos().or(pointer.hover_pos()).map(|p| geo.mirror_pos(p));
    if pos.is_some_and(|p| geo.in_cells(p)) {
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    if pointer.primary_pressed()
        && resp.contains_pointer()
        && let Some(p) = pointer.press_origin().map(|p| geo.mirror_pos(p)).or(pos).filter(|p| geo.in_cells(*p))
    {
        app.grid.place_start = Some(p);
    }
    let Some(start) = app.grid.place_start else { return resp.contains_pointer() && pointer.any_down() };
    let rect = Rect::from_two_pos(start, pos.unwrap_or(start));
    if pointer.primary_down() {
        let t = Tokens::get(&ctx);
        ui.painter().rect_stroke(geo.mirror_rect(rect), 2.0, Stroke::new(1.0, t.sheet_control_border), StrokeKind::Inside);
        return true;
    }
    app.grid.place_start = None;
    app.grid.placing = None;
    let (cell, dx, dy) = cell_point(geo, sh, rect.min);
    let mut params = json!({"at": cell.a1(), "dx": dx, "dy": dy});
    // A drag sets the size (and a slider's orientation); a click keeps the default size.
    if rect.width() >= 6.0 || rect.height() >= 6.0 {
        params["width"] = json!((rect.width() / geo.z).max(8.0));
        params["height"] = json!((rect.height() / geo.z).max(8.0));
        if kind == ControlKind::Slider {
            params["horizontal"] = json!(rect.width() >= rect.height());
        }
    }
    match app.run(insert_id(kind), params) {
        Ok(v) => {
            if let Some(id) = v["control"].as_u64().and_then(|id| u32::try_from(id).ok()) {
                app.selected_object = Some(ObjRef::new(ObjKind::Control, id));
                // A new button asks what it should run, as Excel's Assign Macro does.
                if kind == ControlKind::Button {
                    app.open_dialog("assignAction", json!({"id": id}));
                }
            }
        }
        Err(e) => app.message = Some((app.ui.language.tr("Insert Control").to_string(), crate::clean_error(&e))),
    }
    true
}

/// Opens a control's editor (double-click): a button's text, else Format Control.
pub fn edit(app: &mut SheetApp, id: u32) {
    match control(app, id).map(|c| c.kind) {
        Some(ControlKind::Button) => app.open_dialog("formatControl", json!({"id": id, "textOnly": true})),
        Some(_) => app.open_dialog("formatControl", json!({"id": id})),
        None => {}
    }
}

// ---------------------------------------------------------------- pressing a button

/// The question in an `approve: …` error, if it is one.
fn approve_text(e: &str) -> Option<String> {
    let rest = e.split_once("approve: ")?.1;
    Some(rest.split_once(" (run again with").map_or(rest, |(q, _)| q).to_string())
}

/// The question in a `confirm: …` error, if it is one.
fn confirm_text(e: &str) -> Option<String> {
    let rest = e.split_once("confirm: ")?.1;
    Some(rest.split_once(" (run again with").map_or(rest, |(q, _)| q).to_string())
}

/// What a press of the button `params` names would run, as text: the workbook, the sheet, the
/// button, its action, the content of the macro it runs, and the selection (a macro on the
/// selection, or a command without a range, works there). `None` when the active sheet has no
/// such button. A question is answered only while this is unchanged, so an answer never applies
/// to an action other than the one it asked about.
fn press_binding(app: &SheetApp, params: &Json) -> Option<String> {
    let id = params.get("id").and_then(Json::as_u64).and_then(|i| u32::try_from(i).ok())?;
    let d = app.session.active()?;
    let sheet = d.wb.active_sheet();
    let sh = d.wb.sheet(sheet)?;
    let c = sh.controls.iter().find(|c| c.id == id && c.kind == ControlKind::Button)?;
    let mac = match &c.action {
        Some(ControlAction::Macro { name, .. }) => {
            gridcraft_engine::macros::find(&d.wb, name).and_then(|i| d.wb.macros.get(i)).map(gridcraft_engine::macros::fingerprint)
        }
        _ => None,
    };
    Some(
        json!({
            "doc": d.uid,
            "sheet": sheet,
            "sheetName": sh.name,
            "control": id,
            "action": c.action,
            "macro": mac,
            "selection": d.selection,
        })
        .to_string(),
    )
}

/// Presses a button (`control.press` with `params`). A question (approve the action, or make
/// room for a macro's output) opens a prompt whose buttons press it again with the answer, bound
/// to what the question was about (see [`press_binding`]); any other error shows in a message
/// box.
pub fn press(app: &mut SheetApp, params: Json) {
    let bound = press_binding(app, &params);
    let Err(e) = app.run("control.press", params.clone()) else { return };
    let lang = app.ui.language;
    let (text, choices) = if let Some(q) = approve_text(&e) {
        (q, json!([[lang.tr("Run"), {"approve": true}]]))
    } else if let Some(q) = confirm_text(&e) {
        let what = q.split_once(" Overwrite them").map_or(q.as_str(), |(a, _)| a).trim_end_matches(['.', ' ']).to_string();
        (
            format!("{what}. Overwrite them, or move the data out of the way?"),
            json!([
                [lang.tr("Overwrite"), {"onConflict": "overwrite"}],
                [lang.tr("Shift Down"), {"onConflict": "shiftDown"}],
                [lang.tr("Shift Right"), {"onConflict": "shiftRight"}],
            ]),
        )
    } else {
        app.message = Some((lang.tr("Button").to_string(), crate::clean_error(&e)));
        return;
    };
    app.open_dialog("controlQuestion", json!({"text": text, "params": params, "choices": choices, "bound": bound}));
}

pub fn open_question(params: &Json) -> Option<Dialog> {
    let text = params.get("text").and_then(Json::as_str)?;
    let v = json!({
        "text": text,
        "params": params.get("params").cloned().unwrap_or_else(|| json!({})),
        "choices": params.get("choices").cloned().unwrap_or_else(|| json!([])),
        "bound": params.get("bound").and_then(Json::as_str),
    });
    Some(Dialog::custom("controlQuestion", "Button", v))
}

/// The prompt [`press`] opens: the question, a button per answer, and Cancel.
pub fn question_dialog(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    ui.label(val(d, "text"));
    ui.add_space(8.0);
    let choices: Vec<(String, Json)> = d
        .values
        .get("choices")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(|c| Some((c.get(0)?.as_str()?.to_string(), c.get(1)?.clone()))).collect())
        .unwrap_or_default();
    let mut answer = None;
    let lang = crate::i18n::current(ui.ctx());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(format!(" {} ", lang.tr("Cancel"))).clicked() {
            *open = false;
        }
        for (label, extra) in choices.iter().rev() {
            if ui.button(format!(" {label} ")).clicked() {
                answer = Some(extra.clone());
            }
        }
    });
    if let Some(extra) = answer {
        *open = false;
        let mut params = d.values.get("params").cloned().unwrap_or_else(|| json!({}));
        // The answer is for the action the question named: if the workbook, the sheet, the
        // button, its action, its macro or the selection changed meanwhile (an agent reassigned
        // it), nothing runs, and the next press asks about what it runs now.
        let asked = d.values.get("bound").and_then(Json::as_str);
        let now = press_binding(app, &params);
        if asked.is_none() || asked != now.as_deref() {
            let why = if now.is_none() {
                "The button is gone, so nothing ran."
            } else {
                "The button changed while the question was open, so nothing ran. Press it again to see what it runs now."
            };
            let why = lang.tr(why);
            app.message = Some((lang.tr("Button").to_string(), why.to_string()));
            return;
        }
        if let (Some(p), Some(x)) = (params.as_object_mut(), extra.as_object()) {
            for (k, v) in x {
                p.insert(k.clone(), v.clone());
            }
        }
        press(app, params);
    }
}

// ---------------------------------------------------------------- Assign Action

fn val(d: &Dialog, key: &str) -> String {
    d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string()
}

/// The workbook's macros: (name, description).
fn macros(app: &SheetApp) -> Vec<(String, String)> {
    app.session.active().map(|d| d.wb.macros.iter().map(|m| (m.name.clone(), m.description.clone())).collect()).unwrap_or_default()
}

/// Assign Action: a macro (on the selection or a fixed range, with an optional output cell) or a
/// command with its parameters.
pub fn open_assign(app: &SheetApp, params: &Json) -> Option<Dialog> {
    let id = params.get("id").and_then(Json::as_u64).and_then(|i| u32::try_from(i).ok())?;
    let c = control(app, id)?;
    let mut v =
        json!({"id": id, "mode": "macro", "macro": "", "input": "selection", "range": "", "output": "", "command": "", "params": "{}", "search": ""});
    match &c.action {
        Some(ControlAction::Macro { name, input, output }) => {
            v["macro"] = json!(name);
            if let MacroInput::Fixed(r) = input {
                v["input"] = json!("fixed");
                v["range"] = json!(r);
            }
            v["output"] = json!(output.clone().unwrap_or_default());
        }
        Some(ControlAction::Command { id, params }) => {
            v["mode"] = json!("command");
            v["command"] = json!(id);
            v["params"] = json!(serde_json::to_string_pretty(params).unwrap_or_else(|_| "{}".into()));
        }
        None => {
            // A new button suggests the workbook's first macro.
            v["macro"] = json!(macros(app).first().map(|m| m.0.clone()).unwrap_or_default());
        }
    }
    let name = c.display_name();
    Some(Dialog::custom("assignAction", &app.ui.language.tr_fill("Assign Action: {name}", &[("name", name.as_str())]), v))
}

/// The `control.assign` params the dialog describes, or why it can't yet (translated with
/// `lang`, since the caller shows it as the dialog's error line).
pub fn assign_params(d: &Dialog, lang: crate::i18n::Language) -> Result<Json, String> {
    let id = d.values.get("id").cloned().unwrap_or(Json::Null);
    if val(d, "mode") == "command" {
        let command = val(d, "command");
        if command.trim().is_empty() {
            return Err(lang.tr("Choose a command.").to_string());
        }
        let text = val(d, "params");
        let params: Json = if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&text).map_err(|e| lang.tr_fill("The parameters aren't valid JSON: {e}", &[("e", &e.to_string())]))?
        };
        if !params.is_object() {
            return Err(lang.tr(r#"The parameters must be a JSON object, such as {"range": "A1:B2"}."#).to_string());
        }
        return Ok(json!({"id": id, "command": command.trim(), "params": params}));
    }
    let name = val(d, "macro");
    if name.trim().is_empty() {
        return Err(lang.tr("Type or choose a macro name.").to_string());
    }
    let input = if val(d, "input") == "fixed" { val(d, "range") } else { "selection".into() };
    if input.trim().is_empty() {
        return Err(lang.tr("Type the range the macro always runs on.").to_string());
    }
    Ok(json!({"id": id, "macro": name.trim(), "input": input.trim(), "output": val(d, "output").trim()}))
}

/// Commands a button may run whose id or label contains `search` (every one when it is empty):
/// (id, label). File commands are left out: the ones a button may run only produce bytes.
fn command_choices(search: &str) -> Vec<(&'static str, &'static str)> {
    let q = search.trim().to_lowercase();
    gridcraft_engine::command_specs()
        .iter()
        .filter(|c| c.journal && !c.id.starts_with("file."))
        .filter(|c| !matches!(macro_policy(c.id, &json!({}), gridcraft_engine::Trust::Local), Policy::Deny(_)))
        .filter(|c| q.is_empty() || c.id.to_lowercase().contains(&q) || c.label.to_lowercase().contains(&q))
        .map(|c| (c.id, c.label))
        .take(200)
        .collect()
}

fn text_field(ui: &mut Ui, d: &mut Dialog, key: &str, width: f32) {
    let mut s = val(d, key);
    ui.add(egui::TextEdit::singleline(&mut s).desired_width(width).id_salt(("control_dialog", key.to_string())));
    d.values.insert(key.into(), json!(s));
}

pub fn assign_dialog(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current(ui.ctx());
    let mut mode = val(d, "mode");
    ui.horizontal(|ui| {
        ui.radio_value(&mut mode, "macro".to_string(), lang.tr("Run a macro"));
        ui.radio_value(&mut mode, "command".to_string(), lang.tr("Run a command"));
    });
    d.values.insert("mode".into(), json!(mode));
    ui.add_space(4.0);
    if mode == "macro" {
        let macros = macros(app);
        let mut name = val(d, "macro");
        ui.label(lang.tr("Macro name:"));
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY).id_salt("assign_macro"));
        egui::Frame::NONE.fill(t.input_bg).stroke(Stroke::new(1.0, t.input_border)).inner_margin(2.0).show(ui, |ui| {
            ui.set_min_size(vec2(ui.available_width(), 110.0));
            egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                if macros.is_empty() {
                    ui.label(egui::RichText::new(lang.tr("This workbook has no macros yet.")).italics().color(t.text_dim));
                }
                for (n, desc) in &macros {
                    let r =
                        ui.add(egui::Button::selectable(n.eq_ignore_ascii_case(name.trim()), n.as_str()).min_size(vec2(ui.available_width(), 20.0)));
                    let r = if desc.is_empty() { r } else { r.on_hover_text(desc) };
                    if r.clicked() {
                        name = n.clone();
                    }
                }
            });
        });
        d.values.insert("macro".into(), json!(name));
        ui.add_space(4.0);
        let mut input = val(d, "input");
        ui.label(lang.tr("Run it on:"));
        ui.radio_value(&mut input, "selection".to_string(), lang.tr("The selection when the button is pressed"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut input, "fixed".to_string(), lang.tr("Always this range:"));
            if input == "fixed" {
                text_field(ui, d, "range", 160.0);
            }
        });
        d.values.insert("input".into(), json!(input));
        ui.horizontal(|ui| {
            ui.label(lang.tr("Output to (optional):"));
            text_field(ui, d, "output", 120.0);
        });
    } else {
        let mut search = val(d, "search");
        let command = val(d, "command");
        ui.label(lang.tr("Command:"));
        ui.add(egui::TextEdit::singleline(&mut search).hint_text(lang.tr("Search commands")).desired_width(f32::INFINITY).id_salt("assign_search"));
        egui::Frame::NONE.fill(t.input_bg).stroke(Stroke::new(1.0, t.input_border)).inner_margin(2.0).show(ui, |ui| {
            ui.set_min_size(vec2(ui.available_width(), 130.0));
            egui::ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                for (cid, label) in command_choices(&search) {
                    let text = format!("{}  ({cid})", lang.tr(label));
                    if ui.add(egui::Button::selectable(cid == command, text).min_size(vec2(ui.available_width(), 20.0))).clicked() {
                        d.values.insert("command".into(), json!(cid));
                    }
                }
            });
        });
        d.values.insert("search".into(), json!(search));
        let doc = gridcraft_engine::find_command(val(d, "command").trim()).map(|c| c.params).unwrap_or("");
        if !doc.is_empty() {
            ui.label(egui::RichText::new(doc).small().color(t.text_dim));
        }
        ui.label(lang.tr("Parameters (JSON):"));
        let mut params = val(d, "params");
        ui.add(egui::TextEdit::multiline(&mut params).code_editor().desired_rows(3).desired_width(f32::INFINITY).id_salt("assign_params"));
        d.values.insert("params".into(), json!(params));
    }
    let problem = assign_params(d, lang).err();
    if let Some(p) = &problem {
        ui.label(egui::RichText::new(p).color(t.danger));
    }
    let (ok, cancel) = ok_cancel(ui, problem.is_none());
    if cancel || (ok && assign(app, d)) {
        *open = false;
    }
}

/// OK (offered while `enabled`) and Cancel at the bottom right, as the other dialogs place them:
/// (OK clicked, Cancel clicked).
fn ok_cancel(ui: &mut Ui, enabled: bool) -> (bool, bool) {
    let (mut ok, mut cancel) = (false, false);
    ui.add_space(8.0);
    let lang = crate::i18n::current(ui.ctx());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let t = Tokens::get(ui.ctx());
        ok = ui
            .add_enabled(enabled, egui::Button::new(egui::RichText::new(format!("   {}   ", lang.tr("OK"))).color(t.on_accent)).fill(t.accent))
            .clicked();
        cancel = ui.button(format!(" {} ", lang.tr("Cancel"))).clicked();
    });
    (ok, cancel)
}

/// Runs `control.assign` from the dialog. False (with the error shown) when it failed.
fn assign(app: &mut SheetApp, d: &Dialog) -> bool {
    let result = assign_params(d, app.ui.language).and_then(|params| app.run("control.assign", params).map_err(|e| crate::clean_error(&e)));
    match result {
        Ok(_) => true,
        Err(e) => {
            app.message = Some((app.ui.language.tr("Assign Action").to_string(), e));
            false
        }
    }
}

// ---------------------------------------------------------------- Format Control

/// Format Control: a slider's or spin button's cell link, value and limits (orientation and page
/// change for sliders); a button's text. `textOnly` shows just the text (Edit Text).
pub fn open_format(app: &SheetApp, params: &Json) -> Option<Dialog> {
    let id = params.get("id").and_then(Json::as_u64).and_then(|i| u32::try_from(i).ok())?;
    let c = control(app, id)?;
    let text_only = params.get("textOnly").and_then(Json::as_bool) == Some(true) && c.kind == ControlKind::Button;
    let v = json!({
        "id": id,
        "textOnly": text_only,
        "text": c.text,
        "name": c.name,
        "value": c.value.to_string(),
        "min": c.min.to_string(),
        "max": c.max.to_string(),
        "step": c.step.to_string(),
        "page": c.page.to_string(),
        "link": c.link.clone().unwrap_or_default(),
        "horizontal": c.horizontal,
        "print": c.print,
        "locked": c.locked,
    });
    let lang = app.ui.language;
    let name = c.display_name();
    let title = if text_only { lang.tr("Edit Text").to_string() } else { lang.tr_fill("Format Control: {name}", &[("name", name.as_str())]) };
    Some(Dialog::custom("formatControl", &title, v))
}

fn whole(text: &str) -> Option<u32> {
    text.trim().parse::<u32>().ok().filter(|v| *v <= CONTROL_MAX)
}

/// The `control.format` params the dialog describes, or the first problem with them (the same
/// rules the engine checks, so OK is only offered when it will work). Problems are translated
/// with `lang`, since the dialog shows them as its error line.
pub fn format_params(app: &SheetApp, d: &Dialog) -> Result<Json, String> {
    let lang = app.ui.language;
    let gone = || lang.tr("The control is gone.").to_string();
    let id = d.values.get("id").and_then(Json::as_u64).and_then(|i| u32::try_from(i).ok()).ok_or_else(gone)?;
    let c = control(app, id).ok_or_else(gone)?;
    let flag = |k: &str| d.values.get(k).and_then(Json::as_bool).unwrap_or(false);
    if d.values.get("textOnly").and_then(Json::as_bool) == Some(true) {
        return Ok(json!({"id": id, "text": val(d, "text")}));
    }
    let mut out = json!({"id": id, "name": val(d, "name"), "print": flag("print"), "locked": flag("locked")});
    if c.kind == ControlKind::Button {
        out["text"] = json!(val(d, "text"));
        return Ok(out);
    }
    let max = CONTROL_MAX.to_string();
    let mut draft = c.clone();
    for (key, label, slot) in [
        ("min", "Minimum value", &mut draft.min),
        ("max", "Maximum value", &mut draft.max),
        ("step", "Incremental change", &mut draft.step),
        ("page", "Page change", &mut draft.page),
    ] {
        if key == "page" && c.kind != ControlKind::Slider {
            continue;
        }
        *slot = whole(&val(d, key))
            .ok_or_else(|| lang.tr_fill("{label} must be a whole number from 0 to {max}.", &[("label", lang.tr(label)), ("max", max.as_str())]))?;
        out[key] = json!(*slot);
    }
    draft.check_numbers()?;
    let value =
        whole(&val(d, "value")).ok_or_else(|| lang.tr_fill("Current value must be a whole number from 0 to {max}.", &[("max", max.as_str())]))?;
    if value < draft.min || value > draft.max {
        return Err(lang.tr_fill(
            "Current value must be between {min} and {max}.",
            &[("min", draft.min.to_string().as_str()), ("max", draft.max.to_string().as_str())],
        ));
    }
    out["value"] = json!(value);
    out["link"] = json!(val(d, "link").trim());
    if c.kind == ControlKind::Slider {
        out["horizontal"] = json!(flag("horizontal"));
    }
    Ok(out)
}

pub fn format_dialog(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current(ui.ctx());
    let Some(c) = d.values.get("id").and_then(Json::as_u64).and_then(|i| u32::try_from(i).ok()).and_then(|id| control(app, id)) else {
        *open = false;
        return;
    };
    let text_only = d.values.get("textOnly").and_then(Json::as_bool) == Some(true);
    if c.kind == ControlKind::Button {
        let mut text = val(d, "text");
        ui.label(lang.tr("Text:"));
        ui.add(egui::TextEdit::multiline(&mut text).desired_rows(2).desired_width(f32::INFINITY).id_salt("control_text"));
        d.values.insert("text".into(), json!(text));
    } else {
        egui::Grid::new("format_control_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            let mut fields = vec![("value", "Current value:"), ("min", "Minimum value:"), ("max", "Maximum value:"), ("step", "Incremental change:")];
            if c.kind == ControlKind::Slider {
                fields.push(("page", "Page change:"));
            }
            fields.push(("link", "Cell link:"));
            for (key, label) in fields {
                ui.label(lang.tr(label));
                text_field(ui, d, key, if key == "link" { 160.0 } else { 90.0 });
                ui.end_row();
            }
        });
        if c.kind == ControlKind::Slider {
            let mut h = d.values.get("horizontal").and_then(Json::as_bool).unwrap_or(true);
            ui.horizontal(|ui| {
                ui.label(lang.tr("Orientation:"));
                ui.radio_value(&mut h, true, lang.tr("Horizontal"));
                ui.radio_value(&mut h, false, lang.tr("Vertical"));
            });
            d.values.insert("horizontal".into(), json!(h));
        }
    }
    if !text_only {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(lang.tr("Name:"));
            let mut name = val(d, "name");
            ui.add(egui::TextEdit::singleline(&mut name).hint_text(c.display_name()).desired_width(200.0).id_salt("control_name"));
            d.values.insert("name".into(), json!(name));
        });
        for (key, label) in [("locked", "Locked (when the sheet is protected)"), ("print", "Print object")] {
            let mut on = d.values.get(key).and_then(Json::as_bool).unwrap_or(true);
            ui.checkbox(&mut on, lang.tr(label));
            d.values.insert(key.into(), json!(on));
        }
    }
    let params = format_params(app, d);
    if let Err(p) = &params {
        ui.label(egui::RichText::new(p).color(t.danger));
    }
    let (ok, cancel) = ok_cancel(ui, params.is_ok());
    if cancel {
        *open = false;
    }
    if ok && let Ok(p) = params {
        match app.run("control.format", p) {
            Ok(_) => *open = false,
            Err(e) => app.message = Some((lang.tr("Format Control").to_string(), crate::clean_error(&e))),
        }
    }
}
