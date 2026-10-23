//! Get & Transform Data windows: the Power Query Editor, the Navigator, From Web, From ODBC, From
//! SQL Server Database, Import Data (Load To), Query Properties, Data Source Settings, Query
//! Options, Recent Sources, Existing Connections, Merge and Append, and the Queries &
//! Connections task pane.
//!
//! They read engine state and act through commands (`data.*`, `query.*`). The editor works on
//! unsaved drafts of the workbook's queries (previewed with `query.preview`) and saves them with
//! `query.set` on Close & Load, or discards them, as Excel's editor does.

use egui::{Color32, RichText, Ui, vec2};
use serde_json::{Map, Value as Json, json};

use crate::SheetApp;
use crate::dialogs::Dialog;
use crate::i18n::Language;
use crate::theme::{self, Tokens};

/// Rows the editor previews.
const PREVIEW_ROWS: usize = 200;
/// Rows the Navigator previews.
const NAV_ROWS: usize = 50;
/// Distinct values offered in a column's filter list.
const FILTER_VALUES: usize = 500;

fn s<'a>(v: &'a Json, k: &str) -> &'a str {
    v.get(k).and_then(Json::as_str).unwrap_or("")
}

fn strings(v: &Json) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// A cell of a preview as text: `null` for nulls, `Error` for errors.
fn cell_text(v: &Json) -> (String, bool) {
    match v {
        Json::Null => ("null".into(), true),
        Json::String(s) => (s.clone(), false),
        Json::Number(n) => (n.as_f64().map(gridcraft_engine::core::number_to_text).unwrap_or_default(), false),
        Json::Bool(b) => ((if *b { "TRUE" } else { "FALSE" }).into(), false),
        Json::Object(o) if o.contains_key("error") => ("Error".into(), true),
        other => (other.to_string(), false),
    }
}

/// A cell of a date, date/time or time column as text (ISO order, so it reads the same in every
/// language); other columns as `cell_text`.
fn typed_cell_text(v: &Json, ty: &str) -> (String, bool) {
    let Some(serial) = v.as_f64().filter(|_| matches!(ty, "date" | "dateTime" | "time")) else { return cell_text(v) };
    let Some(dt) = gridcraft_engine::core::date::datetime_from_serial(gridcraft_engine::core::DateSystem::D1900, serial) else { return cell_text(v) };
    let date = format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day);
    let time = format!("{:02}:{:02}:{:02}", dt.hour, dt.minute, dt.second);
    let text = match ty {
        "date" => date,
        "time" => time,
        _ => format!("{date} {time}"),
    };
    (text, false)
}

/// The badge Power Query shows for a column type.
fn type_badge(t: &str) -> &'static str {
    match t {
        "text" => "ABC",
        "number" => "1.2",
        "whole" => "123",
        "currency" => "$",
        "percentage" => "%",
        "date" => "Date",
        "dateTime" => "D/T",
        "time" => "Time",
        "logical" => "T/F",
        _ => "ABC123",
    }
}

/// What a column header of the editor's preview asked for.
#[derive(Clone, Debug)]
enum HeaderAction {
    /// Select the column (`true`: add to the selection, Ctrl/Cmd held).
    Select(String, bool),
    Rename(String),
    Sort(String, bool),
    RemoveEmpty(String),
    /// A text, number or date filter with this operator preset.
    Filter(String, &'static str),
    /// Filter by the column's values.
    Values(String),
}

/// The column filter menus by type, as Power Query names them: (menu, [(item, operator)]).
fn filter_menu(ty: &str) -> (&'static str, &'static [(&'static str, &'static str)]) {
    match ty {
        "number" | "whole" | "currency" | "percentage" => (
            "Number Filters",
            &[
                ("Equals…", "equals"),
                ("Does Not Equal…", "notEquals"),
                ("Greater Than…", "greaterThan"),
                ("Greater Than Or Equal To…", "greaterOrEqual"),
                ("Less Than…", "lessThan"),
                ("Less Than Or Equal To…", "lessOrEqual"),
            ],
        ),
        "date" | "dateTime" | "time" => ("Date Filters", &[("Equals…", "equals"), ("Before…", "lessThan"), ("After…", "greaterThan")]),
        _ => (
            "Text Filters",
            &[
                ("Equals…", "equals"),
                ("Does Not Equal…", "notEquals"),
                ("Begins With…", "beginsWith"),
                ("Does Not Begin With…", "notBeginsWith"),
                ("Ends With…", "endsWith"),
                ("Does Not End With…", "notEndsWith"),
                ("Contains…", "contains"),
                ("Does Not Contain…", "notContains"),
            ],
        ),
    }
}

/// A preview table: header with type badges, rows of values. With `menus`, each header has the
/// column menu (sort, filters by type, filter by values); returns what the headers asked for.
fn preview_grid(ui: &mut Ui, id: &str, p: &Json, selected: &[String], lang: Language, menus: bool) -> Vec<HeaderAction> {
    let t = Tokens::get(ui.ctx());
    let columns = strings(&p["columns"]);
    let types = strings(&p["types"]);
    let mut actions = Vec::new();
    if let Some(e) = p.get("error").and_then(Json::as_str) {
        ui.colored_label(Color32::from_rgb(0xC0, 0x30, 0x30), format!("{}: {e}", lang.tr("Expression.Error")));
        return actions;
    }
    egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new(id).striped(true).min_col_width(60.0).spacing(vec2(10.0, 3.0)).show(ui, |ui| {
            ui.label("");
            for (i, c) in columns.iter().enumerate() {
                let ty = types.get(i).map(String::as_str).unwrap_or("");
                let text = RichText::new(format!("{}  {c}", type_badge(ty))).strong();
                ui.horizontal(|ui| {
                    let r = ui.selectable_label(selected.contains(c), text);
                    if r.clicked() {
                        actions.push(HeaderAction::Select(c.clone(), ui.input(|i| i.modifiers.command || i.modifiers.ctrl)));
                    }
                    if r.double_clicked() {
                        actions.push(HeaderAction::Rename(c.clone()));
                    }
                    if menus {
                        ui.menu_button("▾", |ui| {
                            if ui.button(lang.tr("Sort Ascending")).clicked() {
                                actions.push(HeaderAction::Sort(c.clone(), false));
                                ui.close();
                            }
                            if ui.button(lang.tr("Sort Descending")).clicked() {
                                actions.push(HeaderAction::Sort(c.clone(), true));
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(lang.tr("Remove Empty")).clicked() {
                                actions.push(HeaderAction::RemoveEmpty(c.clone()));
                                ui.close();
                            }
                            let (menu, items) = filter_menu(ty);
                            ui.menu_button(lang.tr(menu), |ui| {
                                for (label, op) in items {
                                    if ui.button(lang.tr(label)).clicked() {
                                        actions.push(HeaderAction::Filter(c.clone(), op));
                                        ui.close();
                                    }
                                }
                            });
                            if ui.button(lang.tr("Filter by Values…")).clicked() {
                                actions.push(HeaderAction::Values(c.clone()));
                                ui.close();
                            }
                        });
                    }
                });
            }
            ui.end_row();
            for (n, row) in p["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
                ui.label(RichText::new((n + 1).to_string()).color(t.text_dim));
                for (i, v) in row.as_array().map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
                    let (text, dim) = typed_cell_text(v, types.get(i).map(String::as_str).unwrap_or(""));
                    let shown: String = text.chars().take(80).collect();
                    if dim {
                        ui.label(RichText::new(shown).italics().color(t.text_dim));
                    } else {
                        ui.label(shown);
                    }
                }
                ui.end_row();
            }
        });
    });
    actions
}

// ---------------------------------------------------------------- opening

/// Get Data › From File items and From Text/CSV: the desktop picks the file, then the Navigator
/// (or the delimited-text preview) opens.
pub fn import_file(app: &mut SheetApp, params: &Json) {
    let kind = s(params, "kind").to_string();
    let kind = if kind.is_empty() { "csv".to_string() } else { kind };
    let picked = if kind == "folder" {
        match &app.services.pick_folder {
            Some(pick) => pick(),
            None => return unavailable(app),
        }
    } else {
        match &app.services.pick_data_file {
            Some(pick) => pick(&kind),
            None => return unavailable(app),
        }
    };
    let Some(path) = picked else { return };
    app.open_dialog("navigator", json!({"kind": kind, "path": path}));
}

fn unavailable(app: &mut SheetApp) {
    let lang = app.ui.language;
    app.message = Some((lang.tr("Get Data").into(), lang.tr("Choosing files needs the GridCraft desktop app.").into()));
}

/// The command that creates a query of a Navigator kind.
fn nav_command(kind: &str) -> &'static str {
    match kind {
        "csv" => "data.fromTextCsv",
        "excel" => "data.fromExcelWorkbook",
        "xml" => "data.fromXml",
        "json" => "data.fromJson",
        "folder" => "data.fromFolder",
        "web" => "data.fromWeb",
        "odata" => "data.fromODataFeed",
        "sqlserver" => "data.fromSqlServer",
        "access" => "data.fromAccess",
        _ => "data.fromOdbc",
    }
}

/// The location params of a Navigator (path, url, connection, server, database, delimiter…).
fn nav_params(d: &Dialog) -> Map<String, Json> {
    let mut p = Map::new();
    for k in ["path", "url", "connection", "server", "database", "delimiter", "encoding", "sql"] {
        if let Some(v) = d.values.get(k).filter(|v| v.as_str().is_some_and(|x| !x.is_empty())) {
            p.insert(k.into(), v.clone());
        }
    }
    p
}

/// What the Navigator lists on its left (`None`: a single table, no list).
fn nav_source(d: &Dialog) -> Option<Json> {
    let v = |k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    Some(match v("kind").as_str() {
        "excel" => json!({"kind": "workbook", "path": v("path"), "item": ""}),
        "xml" => json!({"kind": "xml", "path": v("path")}),
        "json" => json!({"kind": "json", "path": v("path")}),
        "web" => json!({"kind": "web", "url": v("url")}),
        "odata" => json!({"kind": "odata", "url": v("url")}),
        "odbc" => json!({"kind": "odbc", "connection": v("connection")}),
        "sqlserver" => json!({"kind": "odbc", "connection": gridcraft_engine::cmd::query::sql_server_connection(&v("server"), Some(&v("database")))}),
        "access" => json!({"kind": "odbc", "connection": gridcraft_engine::cmd::query::access_connection(&v("path"))}),
        _ => return None,
    })
}

/// The params for creating the query of the selected Navigator item.
fn nav_create_params(d: &Dialog) -> Map<String, Json> {
    let mut p = nav_params(d);
    if let Some(item) = d.values.get("item").and_then(Json::as_str).filter(|i| !i.is_empty()) {
        let kind = d.values.get("kind").and_then(Json::as_str).unwrap_or("");
        p.insert(if matches!(kind, "sqlserver" | "access" | "odbc") { "table" } else { "item" }.into(), json!(item));
        if let Some(k) = d.values.get("itemKind") {
            p.insert("itemKind".into(), k.clone());
        }
    }
    if d.values.get("combine").and_then(Json::as_bool) == Some(true) {
        p.insert("combine".into(), json!(true));
    }
    p
}

// ---------------------------------------------------------------- dialogs

/// Draws the Get & Transform dialogs (called from `dialogs::show`).
pub fn dialog(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let name = d.name().to_string();
    match name.as_str() {
        "navigator" => navigator(app, ui, d, open),
        "fromWeb" => from_web(app, ui, d, open),
        "fromOdbc" => from_odbc(app, ui, d, open),
        "fromSqlServer" => from_sql_server(app, ui, d, open),
        "loadTo" => load_to(app, ui, d, open),
        "queryProperties" => query_properties(app, ui, d, open),
        "dataSourceSettings" => data_source_settings(app, ui, d, open),
        "queryOptions" => query_options(app, ui, open),
        "recentSources" => recent_sources(app, ui, d, open),
        "existingConnections" => existing_connections(app, ui, d, open),
        "mergeQueries" => merge_queries(app, ui, d, open),
        "appendQueries" => append_queries(app, ui, d, open),
        _ => {}
    }
}

fn buttons(ui: &mut Ui, lang: Language, ok_label: &str) -> (bool, bool) {
    let mut ok = false;
    let mut cancel = false;
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        cancel = ui.button(lang.tr("Cancel")).clicked();
        ok = ui.button(lang.tr(ok_label)).clicked();
    });
    (ok, cancel)
}

fn text_field(ui: &mut Ui, d: &mut Dialog, key: &str, width: f32) -> egui::Response {
    let mut v = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
    let r = ui.add(egui::TextEdit::singleline(&mut v).desired_width(width));
    if r.changed() {
        d.values.insert(key.into(), json!(v));
    }
    r
}

fn check(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) -> bool {
    let mut b = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
    let changed = ui.checkbox(&mut b, label).changed();
    if changed {
        d.values.insert(key.into(), json!(b));
    }
    changed
}

fn error_line(ui: &mut Ui, d: &Dialog) {
    if let Some(e) = d.values.get("error").and_then(Json::as_str) {
        ui.colored_label(Color32::from_rgb(0xC0, 0x30, 0x30), crate::clean_error(e));
    }
}

fn navigator(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    let kind = d.values.get("kind").and_then(Json::as_str).unwrap_or("csv").to_string();
    // The items, once.
    if !d.values.contains_key("items")
        && let Some(src) = nav_source(d)
    {
        match app.session.run("query.navigator", json!({"source": src})) {
            Ok(r) => {
                let items = r.get("items").cloned().unwrap_or(json!([]));
                if let Some(first) = items.as_array().and_then(|a| a.first()) {
                    d.values.insert("item".into(), first.get("name").cloned().unwrap_or(json!("")));
                    d.values.insert("itemKind".into(), first.get("kind").cloned().unwrap_or(json!("")));
                }
                d.values.insert("items".into(), items);
            }
            Err(e) => {
                d.values.insert("items".into(), json!([]));
                d.values.insert("error".into(), json!(e));
            }
        }
    }
    // The preview of the selected item (with the steps Power Query adds), when it changes.
    let mut params = nav_create_params(d);
    params.insert("load".into(), json!("none"));
    let key = Json::Object(params.clone()).to_string();
    if d.values.get("previewKey").and_then(Json::as_str) != Some(key.as_str()) && !d.values.contains_key("error") {
        match app.session.run(nav_command(&kind), Json::Object(params.clone())) {
            Ok(r) => {
                d.values.insert(
                    "preview".into(),
                    r.get("preview").cloned().unwrap_or_else(|| json!({"error": r.get("error").cloned().unwrap_or(json!(""))})),
                );
                d.values.insert("draft".into(), r.get("query").cloned().unwrap_or(Json::Null));
            }
            Err(e) => {
                d.values.insert("preview".into(), json!({"error": crate::clean_error(&e), "columns": [], "rows": []}));
            }
        }
        d.values.insert("previewKey".into(), json!(key));
    }
    let location = ["path", "url", "connection", "server"].iter().find_map(|k| d.values.get(*k).and_then(Json::as_str)).unwrap_or("").to_string();
    ui.label(RichText::new(location).strong());
    error_line(ui, d);
    if kind == "csv" {
        ui.horizontal(|ui| {
            ui.label(lang.tr("File Origin"));
            let enc = d.values.get("encoding").and_then(Json::as_str).unwrap_or("").to_string();
            egui::ComboBox::from_id_salt("pq_origin").selected_text(lang.tr(origin_label(&enc))).show_ui(ui, |ui| {
                for (v, label) in [
                    ("", "Detect automatically"),
                    ("utf-8", "65001: Unicode (UTF-8)"),
                    ("windows-1252", "1252: Western European (Windows)"),
                    ("utf-16", "1200: Unicode"),
                ] {
                    if ui.selectable_label(enc == v, lang.tr(label)).clicked() {
                        d.values.insert("encoding".into(), json!(v));
                    }
                }
            });
            ui.label(lang.tr("Delimiter"));
            let delim = d.values.get("delimiter").and_then(Json::as_str).unwrap_or("").to_string();
            egui::ComboBox::from_id_salt("pq_delim").selected_text(lang.tr(delimiter_label(&delim))).show_ui(ui, |ui| {
                for (v, label) in [("", "Detect automatically"), (",", "Comma"), (";", "Semicolon"), ("tab", "Tab"), (" ", "Space"), ("|", "Pipe")] {
                    if ui.selectable_label(delim == v, lang.tr(label)).clicked() {
                        d.values.insert("delimiter".into(), json!(v));
                    }
                }
            });
            ui.label(RichText::new(lang.tr("Data Type Detection: Based on first 200 rows")).small());
        });
    }
    let items: Vec<Json> = d.values.get("items").and_then(Json::as_array).cloned().unwrap_or_default();
    let preview = d.values.get("preview").cloned().unwrap_or(Json::Null);
    ui.horizontal_top(|ui| {
        if !items.is_empty() {
            ui.vertical(|ui| {
                ui.set_width(220.0);
                ui.label(RichText::new(lang.tr("Display Options")).small());
                egui::ScrollArea::vertical().id_salt("pq_nav_items").max_height(360.0).show(ui, |ui| {
                    let current = d.values.get("item").and_then(Json::as_str).unwrap_or("").to_string();
                    for it in &items {
                        let name = s(it, "name");
                        let shown = if name.is_empty() { lang.tr("Document").to_string() } else { name.to_string() };
                        let r = ui.selectable_label(current == name, shown);
                        let r = if s(it, "detail").is_empty() { r } else { r.on_hover_text(s(it, "detail")) };
                        if r.clicked() {
                            d.values.insert("item".into(), json!(name));
                            d.values.insert("itemKind".into(), it.get("kind").cloned().unwrap_or(Json::Null));
                        }
                    }
                });
            });
            ui.separator();
        }
        ui.vertical(|ui| {
            ui.set_min_size(vec2(480.0, 360.0));
            ui.set_max_height(360.0);
            let trimmed = trim_rows(&preview, NAV_ROWS);
            preview_grid(ui, "pq_nav_preview", &trimmed, &[], lang, false);
        });
    });
    ui.add_space(6.0);
    let mut action = None;
    ui.horizontal(|ui| {
        if kind == "folder" {
            if ui.button(lang.tr("Combine & Load")).clicked() {
                d.values.insert("combine".into(), json!(true));
                action = Some("load");
            }
            if ui.button(lang.tr("Combine & Transform Data")).clicked() {
                d.values.insert("combine".into(), json!(true));
                action = Some("transform");
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(lang.tr("Cancel")).clicked() {
                *open = false;
            }
            if ui.button(lang.tr("Transform Data")).clicked() {
                action = Some("transform");
            }
            // Load is a split button: the arrow offers Load To….
            ui.menu_button("▾", |ui| {
                if ui.button(lang.tr("Load")).clicked() {
                    action = Some("load");
                    ui.close();
                }
                if ui.button(lang.tr("Load To…")).clicked() {
                    action = Some("loadTo");
                    ui.close();
                }
            });
            if ui.button(lang.tr("Load")).clicked() {
                action = Some("load");
            }
        });
    });
    let command = nav_command(&kind);
    match action {
        Some("load") => {
            let p = nav_create_params(d);
            match app.run(command, Json::Object(p)) {
                Ok(_) => {
                    *open = false;
                    app.after_engine();
                }
                Err(e) => {
                    d.values.insert("error".into(), json!(e));
                }
            }
        }
        Some("loadTo") => {
            *open = false;
            let p = nav_create_params(d);
            app.open_dialog("loadTo", json!({"command": command, "params": p}));
        }
        Some("transform") => {
            let mut p = nav_create_params(d);
            p.insert("load".into(), json!("none"));
            match app.run(command, Json::Object(p)) {
                Ok(r) => {
                    *open = false;
                    if let Some(q) = r.get("query").filter(|q| q.is_object()) {
                        open_editor(app, None, Some(q.clone()));
                    }
                }
                Err(e) => {
                    d.values.insert("error".into(), json!(e));
                }
            }
        }
        _ => {}
    }
}

fn origin_label(e: &str) -> &'static str {
    match e {
        "utf-8" => "65001: Unicode (UTF-8)",
        "windows-1252" => "1252: Western European (Windows)",
        "utf-16" => "1200: Unicode",
        _ => "Detect automatically",
    }
}

fn delimiter_label(d: &str) -> &'static str {
    match d {
        "," => "Comma",
        ";" => "Semicolon",
        "tab" => "Tab",
        " " => "Space",
        "|" => "Pipe",
        _ => "Detect automatically",
    }
}

fn trim_rows(p: &Json, n: usize) -> Json {
    let mut p = p.clone();
    if let Some(rows) = p.get_mut("rows").and_then(Json::as_array_mut) {
        rows.truncate(n);
    }
    p
}

fn from_web(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    let odata = d.values.get("odata").and_then(Json::as_bool) == Some(true);
    ui.label(lang.tr("URL"));
    let r = text_field(ui, d, "url", 460.0);
    if !d.values.contains_key("focused") {
        r.request_focus();
        d.values.insert("focused".into(), json!(true));
    }
    error_line(ui, d);
    let (ok, cancel) = buttons(ui, lang, "OK");
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if cancel {
        *open = false;
    }
    if ok || enter {
        let url = d.values.get("url").and_then(Json::as_str).unwrap_or("").trim().to_string();
        match gridcraft_engine::query::check_url(&url) {
            Ok(()) => {
                *open = false;
                app.open_dialog("navigator", json!({"kind": if odata { "odata" } else { "web" }, "url": url}));
            }
            Err(e) => {
                d.values.insert("error".into(), json!(e));
            }
        }
    }
}

fn from_odbc(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    ui.label(lang.tr("Connection string (DSN=… or Driver={…};…)"));
    text_field(ui, d, "connection", 460.0);
    ui.collapsing(lang.tr("Advanced options"), |ui| {
        ui.label(lang.tr("SQL statement (optional)"));
        let mut v = d.values.get("sql").and_then(Json::as_str).unwrap_or("").to_string();
        if ui.add(egui::TextEdit::multiline(&mut v).desired_rows(4).desired_width(460.0)).changed() {
            d.values.insert("sql".into(), json!(v));
        }
    });
    error_line(ui, d);
    let (ok, cancel) = buttons(ui, lang, "OK");
    if cancel {
        *open = false;
    }
    if ok {
        let conn = s(&Json::Object(d.values.clone()), "connection").trim().to_string();
        let sql = s(&Json::Object(d.values.clone()), "sql").trim().to_string();
        if !sql.is_empty() {
            match app.run("data.fromOdbc", json!({"connection": conn, "sql": sql})) {
                Ok(_) => {
                    *open = false;
                    app.after_engine();
                }
                Err(e) => {
                    d.values.insert("error".into(), json!(e));
                }
            }
        } else {
            *open = false;
            app.open_dialog("navigator", json!({"kind": "odbc", "connection": conn}));
        }
    }
}

fn from_sql_server(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    egui::Grid::new("pq_sql").num_columns(2).show(ui, |ui| {
        ui.label(lang.tr("Server"));
        text_field(ui, d, "server", 300.0);
        ui.end_row();
        ui.label(lang.tr("Database (optional)"));
        text_field(ui, d, "database", 300.0);
        ui.end_row();
    });
    ui.collapsing(lang.tr("Advanced options"), |ui| {
        ui.label(lang.tr("SQL statement (optional, requires database)"));
        let mut v = d.values.get("sql").and_then(Json::as_str).unwrap_or("").to_string();
        if ui.add(egui::TextEdit::multiline(&mut v).desired_rows(4).desired_width(420.0)).changed() {
            d.values.insert("sql".into(), json!(v));
        }
    });
    ui.label(RichText::new(lang.tr("GridCraft connects with Windows authentication through the SQL Server ODBC driver.")).small());
    error_line(ui, d);
    let (ok, cancel) = buttons(ui, lang, "OK");
    if cancel {
        *open = false;
    }
    if ok {
        let v = Json::Object(d.values.clone());
        let (server, database, sql) = (s(&v, "server").trim(), s(&v, "database").trim(), s(&v, "sql").trim());
        if server.is_empty() {
            d.values.insert("error".into(), json!("Enter a server name."));
        } else if !sql.is_empty() {
            match app.run("data.fromSqlServer", json!({"server": server, "database": database, "sql": sql})) {
                Ok(_) => {
                    *open = false;
                    app.after_engine();
                }
                Err(e) => {
                    d.values.insert("error".into(), json!(e));
                }
            }
        } else {
            let (server, database) = (server.to_string(), database.to_string());
            *open = false;
            app.open_dialog("navigator", json!({"kind": "sqlserver", "server": server, "database": database}));
        }
    }
}

/// Import Data (Load To…): how the data shows and where it goes. Returns the choice when OK is
/// pressed: `{to, sheet?, cell?, newSheet?}`.
fn import_data_ui(app: &SheetApp, ui: &mut Ui, values: &mut Map<String, Json>) -> (Option<Json>, bool) {
    let lang = app.ui.language;
    let to = values.get("to").and_then(Json::as_str).unwrap_or("table").to_string();
    ui.label(lang.tr("Select how you want to view this data in your workbook."));
    for (v, label) in [("table", "Table"), ("pivot", "PivotTable Report"), ("pivotChart", "PivotChart"), ("connectionOnly", "Only Create Connection")]
    {
        let enabled = matches!(v, "table" | "connectionOnly");
        let r = ui.add_enabled(enabled, egui::RadioButton::new(to == v, lang.tr(label)));
        let r = r.on_disabled_hover_text(lang.tr("Load the query as a table, then insert a PivotTable from it."));
        if r.clicked() {
            values.insert("to".into(), json!(v));
        }
    }
    ui.add_space(6.0);
    ui.label(lang.tr("Where do you want to put the data?"));
    let table = to == "table";
    let where_ = values.get("where").and_then(Json::as_str).unwrap_or("new").to_string();
    ui.add_enabled_ui(table, |ui| {
        if ui.radio(where_ == "existing", lang.tr("Existing worksheet:")).clicked() {
            values.insert("where".into(), json!("existing"));
        }
        let mut cell = values.get("cell").and_then(Json::as_str).unwrap_or("").to_string();
        if ui.add_enabled(where_ == "existing", egui::TextEdit::singleline(&mut cell).desired_width(200.0)).changed() {
            values.insert("cell".into(), json!(cell));
        }
        if ui.radio(where_ == "new", lang.tr("New worksheet")).clicked() {
            values.insert("where".into(), json!("new"));
        }
    });
    let (ok, cancel) = buttons(ui, lang, "OK");
    if !ok {
        return (None, cancel);
    }
    let mut out = json!({"to": if table { "table" } else { "connectionOnly" }});
    if table && where_ == "existing" {
        let raw = values.get("cell").and_then(Json::as_str).unwrap_or("").trim().trim_start_matches('=').to_string();
        let (sheet, cell) = match raw.rsplit_once('!') {
            Some((sh, c)) => (sh.trim_matches('\'').replace("''", "'"), c.replace('$', "")),
            None => (app.session.active().and_then(|d| d.wb.active()).map(|sh| sh.name.clone()).unwrap_or_default(), raw.replace('$', "")),
        };
        out["sheet"] = json!(sheet);
        out["cell"] = json!(if cell.is_empty() { "A1".to_string() } else { cell });
    } else if table {
        out["newSheet"] = json!(true);
    }
    (Some(out), cancel)
}

/// The Existing worksheet field's start value: the active cell.
fn default_cell(app: &SheetApp) -> String {
    app.session
        .active()
        .and_then(|d| {
            let sh = d.wb.active()?;
            let c = d.selection.active;
            Some(format!("={}!${}${}", gridcraft_engine::formula::quote_sheet(&sh.name), gridcraft_engine::core::col_to_letters(c.col), c.row + 1))
        })
        .unwrap_or_else(|| "=$A$1".into())
}

fn load_to(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    if !d.values.contains_key("cell") {
        d.values.insert("cell".into(), json!(default_cell(app)));
    }
    let (choice, cancel) = import_data_ui(app, ui, &mut d.values);
    error_line(ui, d);
    if cancel {
        *open = false;
    }
    let Some(choice) = choice else { return };
    let r = if let Some(cmd) = d.values.get("command").and_then(Json::as_str).map(str::to_string) {
        let mut p = d.values.get("params").and_then(Json::as_object).cloned().unwrap_or_default();
        p.insert("load".into(), choice["to"].clone());
        if let Some(sh) = choice.get("sheet") {
            p.insert("sheet".into(), sh.clone());
            p.insert("cell".into(), choice["cell"].clone());
        }
        app.run(&cmd, Json::Object(p))
    } else {
        let name = d.values.get("query").and_then(Json::as_str).unwrap_or("").to_string();
        let mut p = json!({"name": name, "to": choice["to"]});
        for k in ["sheet", "cell", "newSheet"] {
            if let Some(v) = choice.get(k) {
                p[k] = v.clone();
            }
        }
        app.run("query.load", p)
    };
    match r {
        Ok(_) => {
            *open = false;
            app.after_engine();
        }
        Err(e) => {
            d.values.insert("error".into(), json!(e));
        }
    }
}

fn query_properties(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    let query = d.values.get("query").and_then(Json::as_str).unwrap_or("").to_string();
    if !d.values.contains_key("loaded") {
        if let Ok(p) = app.session.run("data.connectionProperties", json!({"query": query})) {
            d.values.insert("newName".into(), json!(query));
            for k in ["description", "refreshOnOpen", "background", "inRefreshAll"] {
                d.values.insert(k.into(), p[k].clone());
            }
            let every = p["everyMinutes"].as_u64().unwrap_or(0);
            d.values.insert("every".into(), json!(every > 0));
            d.values.insert("minutes".into(), json!(if every > 0 { every.to_string() } else { "60".into() }));
        }
        d.values.insert("loaded".into(), json!(true));
    }
    egui::Grid::new("pq_props").num_columns(2).show(ui, |ui| {
        ui.label(lang.tr("Query name:"));
        text_field(ui, d, "newName", 280.0);
        ui.end_row();
        ui.label(lang.tr("Description:"));
        text_field(ui, d, "description", 280.0);
        ui.end_row();
    });
    ui.separator();
    ui.label(RichText::new(lang.tr("Usage")).strong());
    ui.label(lang.tr("Refresh control"));
    check(ui, d, "background", lang.tr("Enable background refresh"));
    ui.horizontal(|ui| {
        check(ui, d, "every", lang.tr("Refresh every"));
        text_field(ui, d, "minutes", 50.0);
        ui.label(lang.tr("minutes"));
    });
    check(ui, d, "refreshOnOpen", lang.tr("Refresh data when opening the file"));
    check(ui, d, "inRefreshAll", lang.tr("Refresh this connection on Refresh All"));
    error_line(ui, d);
    let (ok, cancel) = buttons(ui, lang, "OK");
    if cancel {
        *open = false;
    }
    if ok {
        let v = Json::Object(d.values.clone());
        let minutes = if v["every"].as_bool() == Some(true) { s(&v, "minutes").trim().parse::<u32>().unwrap_or(0) } else { 0 };
        let mut p = json!({
            "query": query,
            "description": v["description"],
            "background": v["background"].as_bool().unwrap_or(true),
            "everyMinutes": minutes,
            "refreshOnOpen": v["refreshOnOpen"].as_bool().unwrap_or(false),
            "inRefreshAll": v["inRefreshAll"].as_bool().unwrap_or(true),
        });
        let new = s(&v, "newName").trim();
        if !new.is_empty() && new != query {
            p["newName"] = json!(new);
        }
        match app.run("data.connectionProperties", p) {
            Ok(_) => *open = false,
            Err(e) => {
                d.values.insert("error".into(), json!(e));
            }
        }
    }
}

fn data_source_settings(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    ui.label(lang.tr("Manage settings for data sources."));
    ui.label(RichText::new(lang.tr("Data sources in current workbook")).strong());
    let list = app.session.run("data.dataSourceSettings", json!({"list": true})).ok().and_then(|r| r.get("sources").cloned()).unwrap_or(json!([]));
    let selected = d.values.get("selected").and_then(Json::as_str).unwrap_or("").to_string();
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        for src in list.as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let loc = s(src, "location");
            let users = strings(&src["queries"]).join(", ");
            if ui.selectable_label(selected == loc, format!("{loc}    ({users})")).clicked() {
                d.values.insert("selected".into(), json!(loc));
                d.values.insert("to".into(), json!(loc));
            }
        }
        if list.as_array().is_none_or(Vec::is_empty) {
            ui.label(RichText::new(lang.tr("No data sources are used in this workbook.")).italics());
        }
    });
    ui.add_enabled_ui(!selected.is_empty(), |ui| {
        ui.horizontal(|ui| {
            ui.label(lang.tr("Change Source…"));
            text_field(ui, d, "to", 300.0);
            if ui.button(lang.tr("Apply")).clicked() {
                let to = d.values.get("to").and_then(Json::as_str).unwrap_or("").to_string();
                match app.run("data.dataSourceSettings", json!({"from": selected, "to": to})) {
                    Ok(_) => {
                        d.values.insert("selected".into(), json!(to));
                        d.values.remove("error");
                    }
                    Err(e) => {
                        d.values.insert("error".into(), json!(e));
                    }
                }
            }
        });
    });
    ui.add_enabled(false, egui::Button::new(lang.tr("Clear Permissions")))
        .on_disabled_hover_text(lang.tr("GridCraft stores no credentials or privacy levels."));
    error_line(ui, d);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(lang.tr("Close")).clicked() {
            *open = false;
        }
    });
}

fn query_options(app: &mut SheetApp, ui: &mut Ui, open: &mut bool) {
    let lang = app.ui.language;
    ui.label(RichText::new(lang.tr("Data Load")).strong());
    ui.label(lang.tr("Default Query Load Settings"));
    let load = app.session.prefs.load_to_worksheet;
    if ui.radio(load, lang.tr("Load to worksheet")).clicked() {
        app.run_or_alert("data.queryOptions", json!({"loadToWorksheet": true}));
    }
    if ui.radio(!load, lang.tr("Only create connection")).clicked() {
        app.run_or_alert("data.queryOptions", json!({"loadToWorksheet": false}));
    }
    ui.add_space(6.0);
    ui.label(RichText::new(lang.tr("Security")).strong());
    ui.label(lang.tr("External data connections follow File › Options › Trust Center."));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(lang.tr("OK")).clicked() {
            *open = false;
        }
    });
}

/// The Navigator kind a recent source opens with.
fn kind_of(location: &str) -> (&'static str, &'static str) {
    let l = location.to_ascii_lowercase();
    if l.starts_with("http://") || l.starts_with("https://") {
        return ("web", "url");
    }
    let ext = l.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "xlsx" | "xlsm" | "xls" | "xlsb" | "ods" => ("excel", "path"),
        "xml" => ("xml", "path"),
        "json" => ("json", "path"),
        "csv" | "txt" | "tsv" | "prn" => ("csv", "path"),
        "accdb" | "mdb" => ("access", "path"),
        _ => ("folder", "path"),
    }
}

fn recent_sources(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    let list = app.session.prefs.recent_sources.clone();
    let selected = d.values.get("selected").and_then(Json::as_str).unwrap_or("").to_string();
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        if list.is_empty() {
            ui.label(RichText::new(lang.tr("No recent sources yet.")).italics());
        }
        for loc in &list {
            let r = ui.selectable_label(selected == *loc, loc.as_str());
            if r.clicked() {
                d.values.insert("selected".into(), json!(loc));
            }
            if r.double_clicked() {
                d.values.insert("connect".into(), json!(loc));
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!selected.is_empty(), egui::Button::new(lang.tr("Connect"))).clicked() {
            d.values.insert("connect".into(), json!(selected));
        }
        if ui.add_enabled(!list.is_empty(), egui::Button::new(lang.tr("Clear the list"))).clicked() {
            app.run_or_alert("data.recentSources", json!({"clear": true}));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(lang.tr("Close")).clicked() {
                *open = false;
            }
        });
    });
    if let Some(loc) = d.values.remove("connect").and_then(|v| v.as_str().map(str::to_string)) {
        let (kind, key) = kind_of(&loc);
        *open = false;
        let mut p = Map::new();
        p.insert("kind".into(), json!(kind));
        p.insert(key.into(), json!(loc));
        app.open_dialog("navigator", Json::Object(p));
    }
}

fn existing_connections(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    ui.label(RichText::new(lang.tr("Connections in this Workbook")).strong());
    let list = app.session.run("data.existingConnections", json!({})).ok().and_then(|r| r.get("connections").cloned()).unwrap_or(json!([]));
    let selected = d.values.get("selected").and_then(Json::as_str).unwrap_or("").to_string();
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for c in list.as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let q = s(c, "query");
            let r = ui.selectable_label(selected == q, RichText::new(s(c, "name")).strong());
            if !s(c, "description").is_empty() {
                ui.label(RichText::new(s(c, "description")).small());
            }
            if r.clicked() {
                d.values.insert("selected".into(), json!(q));
            }
        }
        if list.as_array().is_none_or(Vec::is_empty) {
            ui.label(RichText::new(lang.tr("No connections found in this workbook.")).italics());
        }
    });
    let (ok, cancel) = buttons(ui, lang, "Open");
    if cancel {
        *open = false;
    }
    if ok && !selected.is_empty() {
        *open = false;
        app.open_dialog("loadTo", json!({"query": selected}));
    }
}

fn query_names(app: &SheetApp) -> Vec<String> {
    app.session.active().map(|d| d.wb.queries.iter().map(|q| q.name.clone()).collect()).unwrap_or_default()
}

/// Columns worked out once per key and kept in egui's memory, so an open dialog doesn't evaluate
/// a query every frame.
fn remembered_columns(ui: &Ui, key: &str, compute: impl FnOnce() -> Vec<String>) -> Vec<String> {
    let id = egui::Id::new(("pq_columns", key));
    if let Some(v) = ui.ctx().data(|d| d.get_temp::<Vec<String>>(id)) {
        return v;
    }
    let v = compute();
    ui.ctx().data_mut(|d| d.insert_temp(id, v.clone()));
    v
}

/// Columns of a stored query (from a preview of no rows), for this revision of the workbook.
fn query_columns(app: &mut SheetApp, ui: &Ui, name: &str) -> Vec<String> {
    let revision = app.session.active().map(|d| (d.uid, d.revision)).unwrap_or_default();
    remembered_columns(ui, &format!("{name}#{revision:?}"), || {
        app.session.run("query.preview", json!({"name": name, "rows": 0})).ok().map(|p| strings(&p["columns"])).unwrap_or_default()
    })
}

fn combo(ui: &mut Ui, id: &str, current: &str, options: &[String]) -> Option<String> {
    let mut picked = None;
    egui::ComboBox::from_id_salt(id).width(220.0).selected_text(current).show_ui(ui, |ui| {
        for o in options {
            if ui.selectable_label(o == current, o.as_str()).clicked() {
                picked = Some(o.clone());
            }
        }
    });
    picked
}

/// A list of columns to tick, in the order ticked.
fn column_picker(ui: &mut Ui, id: &str, columns: &[String], chosen: &mut Vec<String>) {
    egui::ScrollArea::vertical().id_salt(id).max_height(140.0).show(ui, |ui| {
        for c in columns {
            let mut on = chosen.contains(c);
            let label = match chosen.iter().position(|x| x == c) {
                Some(i) => format!("{c}  ({})", i + 1),
                None => c.clone(),
            };
            if ui.checkbox(&mut on, label).changed() {
                if on {
                    chosen.push(c.clone());
                } else {
                    chosen.retain(|x| x != c);
                }
            }
        }
    });
}

const JOIN_KINDS: [(&str, &str); 6] = [
    ("leftOuter", "Left Outer (all from first, matching from second)"),
    ("rightOuter", "Right Outer (all from second, matching from first)"),
    ("fullOuter", "Full Outer (all rows from both)"),
    ("inner", "Inner (only matching rows)"),
    ("leftAnti", "Left Anti (rows only in first)"),
    ("rightAnti", "Right Anti (rows only in second)"),
];

fn join_combo(ui: &mut Ui, lang: Language, join: &mut String) {
    let current = JOIN_KINDS.iter().find(|(k, _)| *k == join.as_str()).map(|(_, l)| *l).unwrap_or(JOIN_KINDS[0].1);
    egui::ComboBox::from_id_salt("pq_join").width(360.0).selected_text(lang.tr(current)).show_ui(ui, |ui| {
        for (k, l) in JOIN_KINDS {
            if ui.selectable_label(*join == k, lang.tr(l)).clicked() {
                *join = k.to_string();
            }
        }
    });
}

fn merge_queries(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    let names = query_names(app);
    let v = |d: &Dialog, k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let mut left_cols: Vec<String> = d.values.get("leftColumns").map(strings).unwrap_or_default();
    let mut right_cols: Vec<String> = d.values.get("rightColumns").map(strings).unwrap_or_default();
    ui.label(lang.tr("Select a table and matching columns to create a merged table."));
    let left = v(d, "left");
    if let Some(p) = combo(ui, "pq_merge_left", &left, &names) {
        d.values.insert("left".into(), json!(p));
        left_cols.clear();
    }
    let lc = if left.is_empty() { vec![] } else { query_columns(app, ui, &left) };
    column_picker(ui, "pq_merge_lc", &lc, &mut left_cols);
    let right = v(d, "right");
    if let Some(p) = combo(ui, "pq_merge_right", &right, &names) {
        d.values.insert("right".into(), json!(p));
        right_cols.clear();
    }
    let rc = if right.is_empty() { vec![] } else { query_columns(app, ui, &right) };
    column_picker(ui, "pq_merge_rc", &rc, &mut right_cols);
    ui.label(lang.tr("Join Kind"));
    let mut join = v(d, "join");
    if join.is_empty() {
        join = "leftOuter".into();
    }
    join_combo(ui, lang, &mut join);
    d.values.insert("join".into(), json!(join));
    d.values.insert("leftColumns".into(), json!(left_cols));
    d.values.insert("rightColumns".into(), json!(right_cols));
    error_line(ui, d);
    let (ok, cancel) = buttons(ui, lang, "OK");
    if cancel {
        *open = false;
    }
    if ok {
        let p = json!({"left": left, "right": right, "leftColumns": left_cols, "rightColumns": right_cols, "join": join});
        match app.run("data.mergeQueries", p) {
            Ok(_) => {
                *open = false;
                app.after_engine();
            }
            Err(e) => {
                d.values.insert("error".into(), json!(e));
            }
        }
    }
}

fn append_queries(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let lang = app.ui.language;
    let names = query_names(app);
    let mut chosen: Vec<String> = d.values.get("queries").map(strings).unwrap_or_default();
    ui.label(lang.tr("Concatenate rows from tables into a single table."));
    ui.label(lang.tr("Tables to append (in order):"));
    column_picker(ui, "pq_append", &names, &mut chosen);
    d.values.insert("queries".into(), json!(chosen));
    error_line(ui, d);
    let (ok, cancel) = buttons(ui, lang, "OK");
    if cancel {
        *open = false;
    }
    if ok {
        match app.run("data.appendQueries", json!({"queries": chosen})) {
            Ok(_) => {
                *open = false;
                app.after_engine();
            }
            Err(e) => {
                d.values.insert("error".into(), json!(e));
            }
        }
    }
}

// ---------------------------------------------------------------- Queries & Connections pane

/// The Queries & Connections task pane (`data.queriesConnections`).
pub fn pane(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());
    let queries = app.session.run("query.list", json!({})).unwrap_or(json!([]));
    let blocked = app.session.active().is_some_and(|d| d.external_blocked);
    ui.horizontal(|ui| {
        let tab = ui.ctx().data(|m| m.get_temp::<bool>(egui::Id::new("pq_pane_connections"))).unwrap_or(false);
        if ui.selectable_label(!tab, lang.tr("Queries")).clicked() {
            ui.ctx().data_mut(|m| m.insert_temp(egui::Id::new("pq_pane_connections"), false));
        }
        if ui.selectable_label(tab, lang.tr("Connections")).clicked() {
            ui.ctx().data_mut(|m| m.insert_temp(egui::Id::new("pq_pane_connections"), true));
        }
    });
    let connections = ui.ctx().data(|m| m.get_temp::<bool>(egui::Id::new("pq_pane_connections"))).unwrap_or(false);
    let list = queries.as_array().cloned().unwrap_or_default();
    let excel = app.session.run("query.listExcel", json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let count = list.len() + excel.len();
    ui.label(RichText::new(format!("{count} {}", lang.tr(if count == 1 { "query" } else { "queries" }))).small().color(t.text_dim));
    if blocked {
        ui.colored_label(Color32::from_rgb(0xB0, 0x6A, 0x00), lang.tr("External data connections are disabled. Enable Content to refresh."));
    }
    ui.separator();
    if count == 0 {
        ui.label(RichText::new(lang.tr("This workbook has no queries yet. Use Data › Get Data to add one.")).italics());
    }
    for q in &list {
        let name = s(q, "name").to_string();
        let title = if connections { format!("Query - {name}") } else { name.clone() };
        let status = match (q.get("error").and_then(Json::as_str), q.get("rows").and_then(Json::as_u64), q["load"]["to"].as_str()) {
            (Some(_), _, _) => lang.tr("Load error.").to_string(),
            (None, _, Some("connectionOnly")) => lang.tr("Connection only.").to_string(),
            (None, Some(n), _) => lang.tr("{n} rows loaded.").replace("{n}", &n.to_string()),
            _ => lang.tr("Not loaded yet.").to_string(),
        };
        let r = egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(4, 3))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    crate::icons::paint(ui.painter(), rect, crate::icons::Icon::Query, t.text);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&title).strong());
                        let line = RichText::new(&status).small();
                        let line = if q.get("error").is_some_and(|e| e.is_string()) {
                            line.color(Color32::from_rgb(0xC0, 0x30, 0x30))
                        } else {
                            line.color(t.text_dim)
                        };
                        let r = ui.label(line);
                        if let Some(e) = q.get("error").and_then(Json::as_str) {
                            r.on_hover_text(e);
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::widgets::icon_button(ui, crate::icons::Icon::Refresh, t.text, "Refresh", vec2(20.0, 20.0)).clicked() {
                            app.run_or_alert("data.refresh", json!({"query": name}));
                        }
                    });
                });
            })
            .response
            .interact(egui::Sense::click());
        if r.double_clicked() {
            open_editor(app, Some(&name), None);
        }
        r.context_menu(|ui| {
            for (label, action) in [
                ("Edit", "edit"),
                ("Delete", "delete"),
                ("Refresh", "refresh"),
                ("Load To…", "loadTo"),
                ("Duplicate", "duplicate"),
                ("Reference", "reference"),
                ("Merge…", "merge"),
                ("Append…", "append"),
                ("Properties…", "properties"),
            ] {
                if ui.button(lang.tr(label)).clicked() {
                    ui.close();
                    match action {
                        "edit" => open_editor(app, Some(&name), None),
                        "delete" => app.run_or_alert("query.delete", json!({"name": name})),
                        "refresh" => app.run_or_alert("data.refresh", json!({"query": name})),
                        "loadTo" => app.open_dialog("loadTo", json!({"query": name})),
                        "duplicate" => app.run_or_alert("query.duplicate", json!({"name": name})),
                        "reference" => app.run_or_alert("query.reference", json!({"name": name})),
                        "merge" => app.open_dialog("mergeQueries", json!({"left": name})),
                        "append" => app.open_dialog("appendQueries", json!({"queries": [name]})),
                        _ => app.open_dialog("queryProperties", json!({"query": name})),
                    }
                }
            }
        });
        ui.separator();
    }
    for q in &excel {
        excel_query_row(app, ui, &t, lang, q, connections);
        ui.separator();
    }
}

/// A Power Query query made in Excel: listed with where it loads, refreshed from its M formula,
/// which is viewable read-only (View M Formula, or double-click). What GridCraft can't run yet
/// is named.
fn excel_query_row(app: &mut SheetApp, ui: &mut Ui, t: &Tokens, lang: Language, q: &Json, connections: bool) {
    let name = s(q, "name").to_string();
    let title = match q.get("connection").and_then(Json::as_str) {
        Some(c) if connections => c.to_string(),
        _ if connections => format!("Query - {name}"),
        _ => name.clone(),
    };
    let rows = q.get("rows").and_then(Json::as_u64);
    let status = match (q.get("load").and_then(Json::as_str), q.get("table").and_then(Json::as_str)) {
        (Some("table"), Some(table)) => match rows {
            Some(n) => lang.tr("Loads to table {table} ({n} rows).").replace("{table}", table).replace("{n}", &n.to_string()),
            None => lang.tr("Loads to table {table}.").replace("{table}", table),
        },
        (Some("dataModel"), _) => lang.tr("Loads to the Data Model.").to_string(),
        _ => lang.tr("Connection only.").to_string(),
    };
    let open_id = egui::Id::new(("pq_excel_m", &name));
    let mut open = ui.ctx().data(|m| m.get_temp::<bool>(open_id)).unwrap_or(false);
    let mut toggle = false;
    let r = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(4, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                crate::icons::paint(ui.painter(), rect, crate::icons::Icon::Query, t.text_dim);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&title).strong());
                    ui.label(RichText::new(&status).small().color(t.text_dim));
                    let missing: Vec<&str> = q.get("unsupported").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
                    if let Some(e) = q.get("error").and_then(Json::as_str) {
                        ui.label(RichText::new(lang.tr("Load error.")).small().color(Color32::from_rgb(0xC0, 0x30, 0x30))).on_hover_text(e);
                    } else if q.get("syntaxError").is_some_and(Json::is_string) {
                        ui.label(RichText::new(lang.tr("Made in Excel: its M formula can't be read.")).small().color(t.text_dim))
                            .on_hover_text(s(q, "syntaxError"));
                    } else if !missing.is_empty() {
                        ui.label(RichText::new(lang.tr("Made in Excel: needs {names}, not supported yet.").replace("{names}", &missing.join(", "))).small().color(t.text_dim))
                            .on_hover_text(lang.tr("GridCraft runs this query's M formula, but not these functions yet. The data loaded last time stays in the workbook, and Excel refreshes the query as before."));
                    } else {
                        ui.label(RichText::new(lang.tr("Made in Excel: refreshed from its M formula.")).small().color(t.text_dim));
                    }
                    if ui.link(RichText::new(lang.tr(if open { "Hide M Formula" } else { "View M Formula" })).small()).clicked() {
                        toggle = true;
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::widgets::icon_button(ui, crate::icons::Icon::Refresh, t.text, "Refresh", vec2(20.0, 20.0)).clicked() {
                        app.run_or_alert("data.refresh", json!({"query": name}));
                    }
                });
            });
        })
        .response
        .interact(egui::Sense::click());
    if r.double_clicked() || toggle {
        open = !open;
    }
    let formula = s(q, "formula").to_string();
    r.context_menu(|ui| {
        if ui.button(lang.tr("Refresh")).clicked() {
            app.run_or_alert("data.refresh", json!({"query": name}));
            ui.close();
        }
        if ui.button(lang.tr(if open { "Hide M Formula" } else { "View M Formula" })).clicked() {
            open = !open;
            ui.close();
        }
        if ui.button(lang.tr("Copy M Formula")).clicked() {
            ui.ctx().copy_text(formula.clone());
            ui.close();
        }
    });
    if open {
        if let Some(d) = q.get("description").and_then(Json::as_str) {
            ui.label(RichText::new(d).small().italics());
        }
        let mut text = formula.as_str();
        let lines = text.lines().count().clamp(3, 18);
        egui::ScrollArea::vertical().id_salt(("pq_excel_m_scroll", &name)).max_height(280.0).show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(lines).desired_width(f32::INFINITY));
        });
    }
    ui.ctx().data_mut(|m| m.insert_temp(open_id, open));
}

// ---------------------------------------------------------------- the Power Query Editor

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    Transform,
    AddColumn,
    View,
}

/// One query being edited.
#[derive(Clone, Debug)]
pub struct Draft {
    pub def: Json,
    /// The workbook query it edits (`None`: new).
    pub original: Option<String>,
    pub dirty: bool,
}

/// A small dialog over the editor.
#[derive(Clone, Debug)]
enum Sub {
    Rows { op: &'static str, count: String, first: String },
    Split { delimiter: String, custom: String, mode: String },
    GroupBy { key: String, name: String, op: String, column: String },
    Replace { find: String, with: String, whole: bool },
    Rename { from: String, to: String },
    Conditional { name: String, rules: Vec<[String; 4]>, otherwise: String },
    Index { start: String, step: String },
    Filter { column: String, op: String, value: String, op2: String, value2: String, or: bool },
    Values { column: String, values: Vec<(Json, bool)> },
    Merge { query: String, left: Vec<String>, right: Vec<String>, join: String },
    Append { queries: Vec<String> },
    Locale { ty: String, locale: String },
    LoadTo { values: Map<String, Json> },
    Properties { name: String, description: String },
}

/// The Power Query Editor's state (`SheetApp::power_query`).
pub struct Editor {
    pub drafts: Vec<Draft>,
    pub current: usize,
    /// The selected applied step (0 = Source); `None` = the last.
    pub step: Option<usize>,
    pub tab: Tab,
    pub selected: Vec<String>,
    preview: Json,
    preview_key: Option<(usize, Option<usize>, u64)>,
    revision: u64,
    sub: Option<Sub>,
    advanced: bool,
    renaming: Option<(usize, String)>,
    deleted: Vec<String>,
    confirm_close: bool,
    pub message: Option<String>,
}

impl Editor {
    fn def(&self) -> Option<&Json> {
        self.drafts.get(self.current).map(|d| &d.def)
    }
    fn name(&self) -> String {
        self.def().map(|d| s(d, "name").to_string()).unwrap_or_default()
    }
    fn touch(&mut self) {
        self.revision += 1;
        if let Some(d) = self.drafts.get_mut(self.current) {
            d.dirty = true;
        }
    }
    fn steps_mut(&mut self) -> Option<&mut Vec<Json>> {
        let d = self.drafts.get_mut(self.current)?;
        if !d.def.get("steps").is_some_and(Json::is_array) {
            d.def["steps"] = json!([]);
        }
        d.def.get_mut("steps").and_then(Json::as_array_mut)
    }
    /// Inserts a step after the selected one (at the end when the last is selected).
    fn add_step(&mut self, step: Json) {
        let selected = self.step;
        let Some(steps) = self.steps_mut() else { return };
        let at = selected.map_or(steps.len(), |i| i.min(steps.len()));
        steps.insert(at, step);
        if selected.is_some() {
            self.step = Some(at + 1);
        }
        self.touch();
    }
    fn columns(&self) -> Vec<String> {
        strings(&self.preview["columns"])
    }
    /// The columns a transform applies to: the selection, else every column.
    fn targets(&self) -> Vec<String> {
        if self.selected.is_empty() { self.columns() } else { self.selected.clone() }
    }
}

/// Opens the editor on the workbook's queries, focused on `name`, or on a new draft.
pub fn open_editor(app: &mut SheetApp, name: Option<&str>, new: Option<Json>) {
    let mut drafts: Vec<Draft> = app
        .session
        .active()
        .map(|d| {
            d.wb.queries
                .iter()
                .map(|q| Draft { def: serde_json::to_value(q).unwrap_or(Json::Null), original: Some(q.name.clone()), dirty: false })
                .collect()
        })
        .unwrap_or_default();
    let mut current = name.and_then(|n| drafts.iter().position(|d| s(&d.def, "name").eq_ignore_ascii_case(n))).unwrap_or(0);
    if let Some(def) = new {
        drafts.push(Draft { def, original: None, dirty: true });
        current = drafts.len() - 1;
    }
    if drafts.is_empty() {
        drafts.push(Draft { def: json!({"name": "Query1", "source": {"kind": "blank"}}), original: None, dirty: true });
    }
    // An editor already open keeps its drafts; it switches to the query asked for.
    if let Some(ed) = app.power_query.as_mut() {
        if let Some(n) = name
            && let Some(i) = ed.drafts.iter().position(|d| s(&d.def, "name").eq_ignore_ascii_case(n))
        {
            ed.current = i;
            ed.step = None;
        }
        return;
    }
    app.power_query = Some(Editor {
        drafts,
        current,
        step: None,
        tab: Tab::Home,
        selected: vec![],
        preview: Json::Null,
        preview_key: None,
        revision: 0,
        sub: None,
        advanced: false,
        renaming: None,
        deleted: vec![],
        confirm_close: false,
        message: None,
    });
}

/// Refreshes the cached preview when the query, the step or the drafts changed.
fn refresh_preview(app: &mut SheetApp, ed: &mut Editor) {
    let key = (ed.current, ed.step, ed.revision);
    if ed.preview_key == Some(key) {
        return;
    }
    ed.preview_key = Some(key);
    let Some(def) = ed.def().cloned() else { return };
    let others: Vec<Json> = ed.drafts.iter().enumerate().filter(|(i, _)| *i != ed.current).map(|(_, d)| d.def.clone()).collect();
    let mut p = json!({"query": def, "rows": PREVIEW_ROWS, "drafts": others});
    if let Some(st) = ed.step {
        p["step"] = json!(st);
    }
    ed.preview = match app.session.run("query.preview", p) {
        Ok(r) => r,
        Err(e) => json!({"error": crate::clean_error(&e), "columns": [], "rows": [], "steps": ["Source"]}),
    };
    // Steps named by the engine (`Removed Columns1`) keep those names in the draft.
    let names = strings(&ed.preview["steps"]);
    if let Some(steps) = ed.steps_mut() {
        for (i, st) in steps.iter_mut().enumerate() {
            if st.get("name").and_then(Json::as_str).is_none_or(str::is_empty)
                && let Some(n) = names.get(i + 1)
            {
                st["name"] = json!(n);
            }
        }
    }
    ed.selected.retain(|c| strings(&ed.preview["columns"]).contains(c));
}

/// Saves every changed draft (`query.set`), deletes removed queries, then loads or refreshes the
/// queries that changed. `target` is Close & Load To's choice for the current query.
fn save(app: &mut SheetApp, ed: &mut Editor, target: Option<Json>) -> Result<(), String> {
    for name in std::mem::take(&mut ed.deleted) {
        app.run("query.delete", json!({"name": name}))?;
    }
    let current = ed.current;
    let mut changed: Vec<(String, bool)> = Vec::new();
    for (i, d) in ed.drafts.iter_mut().enumerate() {
        if !d.dirty && d.original.is_some() && !(i == current && target.is_some()) {
            continue;
        }
        let mut p = json!({"query": d.def});
        if let Some(o) = &d.original {
            p["replace"] = json!(o);
        }
        app.run("query.set", p)?;
        let name = s(&d.def, "name").to_string();
        changed.push((name.clone(), d.original.is_none()));
        d.original = Some(name);
        d.dirty = false;
    }
    let current_name = ed.drafts.get(current).map(|d| s(&d.def, "name").to_string()).unwrap_or_default();
    for (name, new) in changed {
        let result = match (&target, name == current_name) {
            (Some(t), true) => {
                let mut p = json!({"name": name});
                if let Some(o) = t.as_object() {
                    for (k, v) in o {
                        p[k] = v.clone();
                    }
                }
                app.run("query.load", p)
            }
            _ if new => {
                let to = if app.session.prefs.load_to_worksheet { "table" } else { "connectionOnly" };
                app.run("query.load", json!({"name": name, "to": to}))
            }
            _ => app.run("data.refresh", json!({"query": name})),
        };
        if let Err(e) = result {
            app.message = Some((name, crate::clean_error(&e)));
        }
    }
    app.after_engine();
    Ok(())
}

/// Draws the Power Query Editor window when it is open.
pub fn show_editor(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(mut ed) = app.power_query.take() else { return };
    let lang = app.ui.language;
    refresh_preview(app, &mut ed);
    let mut open = true;
    let mut close = false;
    let title = format!("{} - {}", ed.name(), lang.tr("Power Query Editor"));
    egui::Window::new(title)
        .id(egui::Id::new("power_query_editor"))
        .default_size(vec2(1060.0, 660.0))
        .min_size(vec2(720.0, 420.0))
        .resizable(true)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            editor_ribbon(app, ui, &mut ed, &mut close);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("fx").italics());
                let mut formula = ed.preview.get("formula").and_then(Json::as_str).unwrap_or("").to_string();
                ui.add(egui::TextEdit::singleline(&mut formula).desired_width(f32::INFINITY).interactive(false).font(egui::TextStyle::Monospace));
            });
            egui::Panel::left("pq_queries").default_size(190.0).resizable(true).show(ui, |ui| queries_list(app, ui, &mut ed));
            egui::Panel::right("pq_settings").default_size(240.0).resizable(true).show(ui, |ui| settings(app, ui, &mut ed));
            egui::Panel::bottom("pq_status").show(ui, |ui| {
                let n = ed.preview.get("rowCount").and_then(Json::as_u64).unwrap_or(0);
                let c = ed.columns().len();
                let more = if n as usize > PREVIEW_ROWS { format!(" ({} {PREVIEW_ROWS})", lang.tr("preview of the first")) } else { String::new() };
                ui.label(RichText::new(format!("{c} {}, {n} {}{more}", lang.tr("COLUMNS"), lang.tr("ROWS"))).small());
            });
            egui::CentralPanel::default().show(ui, |ui| {
                let preview = ed.preview.clone();
                for action in preview_grid(ui, "pq_grid", &preview, &ed.selected, lang, true) {
                    header_action(&mut ed, action);
                }
            });
        });
    if sub_dialog(app, ctx, &mut ed) {
        return;
    }
    if ed.advanced {
        let m = ed
            .def()
            .cloned()
            .and_then(|def| app.session.run("query.mText", json!({"query": def})).ok())
            .and_then(|r| r.get("m").and_then(Json::as_str).map(str::to_string))
            .unwrap_or_default();
        let mut shown = true;
        egui::Window::new(format!("{} - {}", ed.name(), lang.tr("Advanced Editor")))
            .id(egui::Id::new("pq_advanced"))
            .default_size(vec2(640.0, 380.0))
            .open(&mut shown)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(lang.tr("Generated from the applied steps (read-only). Change the query with the ribbon and Applied Steps."))
                        .small(),
                );
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut m.as_str()).code_editor().desired_width(f32::INFINITY));
                });
            });
        ed.advanced = shown;
    }
    if !open {
        if ed.drafts.iter().any(|d| d.dirty) || !ed.deleted.is_empty() {
            ed.confirm_close = true;
        } else {
            return;
        }
    }
    if ed.confirm_close {
        let mut keep = None;
        egui::Window::new(lang.tr("Power Query Editor")).id(egui::Id::new("pq_confirm")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(lang.tr("Do you want to keep your changes?"));
            ui.horizontal(|ui| {
                if ui.button(lang.tr("Keep")).clicked() {
                    keep = Some(true);
                }
                if ui.button(lang.tr("Discard")).clicked() {
                    keep = Some(false);
                }
                if ui.button(lang.tr("Cancel")).clicked() {
                    ed.confirm_close = false;
                }
            });
        });
        match keep {
            Some(true) => close = true,
            Some(false) => return,
            None => {}
        }
    }
    if close {
        match save(app, &mut ed, None) {
            Ok(()) => return,
            Err(e) => {
                ed.confirm_close = false;
                ed.message = Some(crate::clean_error(&e));
            }
        }
    }
    if let Some(m) = ed.message.take() {
        app.message = Some((lang.tr("Power Query Editor").into(), m));
    }
    app.power_query = Some(ed);
}

fn queries_list(app: &mut SheetApp, ui: &mut Ui, ed: &mut Editor) {
    let lang = app.ui.language;
    ui.label(RichText::new(format!("{} [{}]", lang.tr("Queries"), ed.drafts.len())).strong());
    ui.separator();
    let mut delete = None;
    egui::ScrollArea::vertical().id_salt("pq_qlist").show(ui, |ui| {
        for (i, d) in ed.drafts.iter().enumerate() {
            let label = format!("{}{}", s(&d.def, "name"), if d.dirty { " *" } else { "" });
            let r = ui.selectable_label(i == ed.current, label);
            if r.clicked() && i != ed.current {
                ed.current = i;
                ed.step = None;
                ed.selected.clear();
            }
            r.context_menu(|ui| {
                if ui.button(lang.tr("Delete")).clicked() {
                    delete = Some(i);
                    ui.close();
                }
            });
        }
    });
    if let Some(i) = delete
        && i < ed.drafts.len()
    {
        let d = ed.drafts.remove(i);
        if let Some(o) = d.original {
            ed.deleted.push(o);
        }
        if ed.drafts.is_empty() {
            ed.drafts.push(Draft { def: json!({"name": "Query1", "source": {"kind": "blank"}}), original: None, dirty: true });
        }
        ed.current = ed.current.min(ed.drafts.len() - 1);
        ed.step = None;
        ed.revision += 1;
    }
}

fn settings(app: &mut SheetApp, ui: &mut Ui, ed: &mut Editor) {
    let lang = app.ui.language;
    ui.label(RichText::new(lang.tr("Query Settings")).font(theme::ui_bold(14.0)));
    ui.label(RichText::new(lang.tr("PROPERTIES")).small().strong());
    ui.label(lang.tr("Name"));
    let mut name = ed.name();
    let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
    if r.lost_focus() && !name.trim().is_empty() && name != ed.name() {
        let old = ed.name();
        let new = name.trim().to_string();
        if ed.drafts.iter().enumerate().any(|(i, d)| i != ed.current && s(&d.def, "name").eq_ignore_ascii_case(&new)) {
            ed.message = Some(lang.tr("A query with that name already exists.").into());
        } else {
            if let Some(d) = ed.drafts.get_mut(ed.current) {
                d.def["name"] = json!(new);
            }
            // References in other drafts follow.
            for d in ed.drafts.iter_mut() {
                let mut changed = false;
                if d.def["source"]["kind"] == "query" && s(&d.def["source"], "name").eq_ignore_ascii_case(&old) {
                    d.def["source"]["name"] = json!(new);
                    changed = true;
                }
                if let Some(steps) = d.def.get_mut("steps").and_then(Json::as_array_mut) {
                    for st in steps {
                        if st["op"] == "mergeQueries" && s(st, "query").eq_ignore_ascii_case(&old) {
                            st["query"] = json!(new);
                            changed = true;
                        }
                        if let Some(list) = st.get_mut("queries").and_then(Json::as_array_mut) {
                            for x in list.iter_mut().filter(|x| x.as_str().is_some_and(|x| x.eq_ignore_ascii_case(&old))) {
                                *x = json!(new);
                                changed = true;
                            }
                        }
                    }
                }
                d.dirty |= changed;
            }
            ed.touch();
        }
    }
    if ui.link(lang.tr("All Properties")).clicked() {
        ed.sub = Some(Sub::Properties { name: ed.name(), description: ed.def().map(|d| s(d, "description").to_string()).unwrap_or_default() });
    }
    ui.add_space(8.0);
    ui.label(RichText::new(lang.tr("APPLIED STEPS")).small().strong());
    let names = strings(&ed.preview["steps"]);
    let names = if names.is_empty() { vec!["Source".to_string()] } else { names };
    let last = names.len().saturating_sub(1);
    let selected = ed.step.unwrap_or(last).min(last);
    let mut action: Option<(&str, usize)> = None;
    egui::ScrollArea::vertical().id_salt("pq_steps").show(ui, |ui| {
        for (i, n) in names.iter().enumerate() {
            if let Some((ri, buf)) = ed.renaming.as_mut()
                && *ri == i
            {
                let r = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY));
                r.request_focus();
                if r.lost_focus() {
                    action = Some(("renameDone", i));
                }
                continue;
            }
            ui.horizontal(|ui| {
                let shown = if i == 0 { lang.tr("Source").to_string() } else { n.clone() };
                let r = ui.selectable_label(i == selected, shown);
                if r.clicked() {
                    action = Some(("select", i));
                }
                if r.double_clicked() && i > 0 {
                    action = Some(("rename", i));
                }
                r.context_menu(|ui| {
                    for (label, a) in
                        [("Rename", "rename"), ("Delete", "delete"), ("Delete Until End", "deleteToEnd"), ("Move Up", "up"), ("Move Down", "down")]
                    {
                        if ui.add_enabled(i > 0, egui::Button::new(lang.tr(label))).clicked() {
                            action = Some((a, i));
                            ui.close();
                        }
                    }
                });
                if i > 0
                    && i == selected
                    && crate::widgets::icon_button(ui, crate::icons::Icon::Close, Color32::from_rgb(0xC0, 0x30, 0x30), "Delete", vec2(18.0, 18.0))
                        .clicked()
                {
                    action = Some(("delete", i));
                }
            });
        }
    });
    match action {
        Some(("select", i)) => {
            ed.step = if i >= last { None } else { Some(i) };
        }
        Some(("rename", i)) => ed.renaming = Some((i, names.get(i).cloned().unwrap_or_default())),
        Some(("renameDone", i)) => {
            if let Some((_, new)) = ed.renaming.take() {
                let new = new.trim().to_string();
                let taken = new.eq_ignore_ascii_case("Source") || names.iter().enumerate().any(|(j, n)| j != i && n.eq_ignore_ascii_case(&new));
                if !new.is_empty() && !taken {
                    if let Some(st) = ed.steps_mut().and_then(|s| s.get_mut(i - 1)) {
                        st["name"] = json!(new);
                    }
                    ed.touch();
                }
            }
        }
        Some(("delete", i)) => {
            if let Some(steps) = ed.steps_mut()
                && i >= 1
                && i - 1 < steps.len()
            {
                steps.remove(i - 1);
            }
            ed.step = None;
            ed.touch();
        }
        Some(("deleteToEnd", i)) => {
            if let Some(steps) = ed.steps_mut() {
                steps.truncate(i.saturating_sub(1));
            }
            ed.step = None;
            ed.touch();
        }
        Some(("up", i)) | Some(("down", i)) => {
            let up = matches!(action, Some(("up", _)));
            if let Some(steps) = ed.steps_mut() {
                let a = i - 1;
                let b = if up { a.checked_sub(1) } else { Some(a + 1).filter(|b| *b < steps.len()) };
                if let Some(b) = b {
                    steps.swap(a, b);
                }
            }
            ed.touch();
        }
        _ => {}
    }
}

fn type_items() -> [(&'static str, &'static str); 9] {
    [
        ("number", "Decimal Number"),
        ("currency", "Currency"),
        ("whole", "Whole Number"),
        ("percentage", "Percentage"),
        ("dateTime", "Date/Time"),
        ("date", "Date"),
        ("time", "Time"),
        ("logical", "True/False"),
        ("text", "Text"),
    ]
}

fn change_type(ed: &mut Editor, ty: &str, locale: Option<&str>) {
    let cols: Vec<Json> = ed.targets().iter().map(|c| json!({"column": c, "type": ty})).collect();
    if cols.is_empty() {
        return;
    }
    let mut st = json!({"op": "changeType", "columns": cols});
    if let Some(l) = locale.filter(|l| !l.trim().is_empty()) {
        st["locale"] = json!(l.trim());
    }
    ed.add_step(st);
}

fn editor_ribbon(app: &mut SheetApp, ui: &mut Ui, ed: &mut Editor, close: &mut bool) {
    let lang = app.ui.language;
    ui.horizontal(|ui| {
        for (tab, label) in [(Tab::Home, "Home"), (Tab::Transform, "Transform"), (Tab::AddColumn, "Add Column"), (Tab::View, "View")] {
            if ui.selectable_label(ed.tab == tab, RichText::new(lang.tr(label)).strong()).clicked() {
                ed.tab = tab;
            }
        }
    });
    let has_cols = !ed.columns().is_empty();
    let one = ed.selected.len() == 1;
    let first = ed.selected.first().cloned().unwrap_or_default();
    ui.horizontal_wrapped(|ui| match ed.tab {
        Tab::Home => {
            ui.menu_button(lang.tr("Close & Load ▾"), |ui| {
                if ui.button(lang.tr("Close & Load")).clicked() {
                    *close = true;
                    ui.close();
                }
                if ui.button(lang.tr("Close & Load To…")).clicked() {
                    ed.sub = Some(Sub::LoadTo { values: Map::new() });
                    ui.close();
                }
            });
            if ui.button(lang.tr("Refresh Preview")).clicked() {
                if let Some(d) = app.session.active_mut() {
                    d.fetch_cache.clear();
                }
                ed.preview_key = None;
            }
            if ui.button(lang.tr("Properties")).clicked() {
                ed.sub =
                    Some(Sub::Properties { name: ed.name(), description: ed.def().map(|d| s(d, "description").to_string()).unwrap_or_default() });
            }
            if ui.button(lang.tr("Advanced Editor")).clicked() {
                ed.advanced = true;
            }
            ui.separator();
            if ui.add_enabled(has_cols && !ed.selected.is_empty(), egui::Button::new(lang.tr("Choose Columns"))).clicked() {
                let cols = ed.selected.clone();
                ed.add_step(json!({"op": "chooseColumns", "columns": cols}));
            }
            if ui.add_enabled(!ed.selected.is_empty(), egui::Button::new(lang.tr("Remove Columns"))).clicked() {
                let cols = ed.selected.clone();
                ed.selected.clear();
                ed.add_step(json!({"op": "removeColumns", "columns": cols}));
            }
            ui.menu_button(lang.tr("Keep Rows ▾"), |ui| {
                for (label, op) in [("Keep Top Rows…", "keepTopRows"), ("Keep Bottom Rows…", "keepBottomRows"), ("Keep Range of Rows…", "keepRange")]
                {
                    if ui.button(lang.tr(label)).clicked() {
                        ed.sub = Some(Sub::Rows { op, count: "10".into(), first: "1".into() });
                        ui.close();
                    }
                }
            });
            ui.menu_button(lang.tr("Remove Rows ▾"), |ui| {
                for (label, op) in [("Remove Top Rows…", "removeTopRows"), ("Remove Bottom Rows…", "removeBottomRows")] {
                    if ui.button(lang.tr(label)).clicked() {
                        ed.sub = Some(Sub::Rows { op, count: "1".into(), first: String::new() });
                        ui.close();
                    }
                }
                if ui.button(lang.tr("Remove Duplicates")).clicked() {
                    let cols = ed.selected.clone();
                    ed.add_step(json!({"op": "removeDuplicates", "columns": cols}));
                    ui.close();
                }
                if ui.button(lang.tr("Remove Blank Rows")).clicked() {
                    ed.add_step(json!({"op": "removeBlankRows"}));
                    ui.close();
                }
                if ui.button(lang.tr("Remove Errors")).clicked() {
                    let cols = ed.selected.clone();
                    ed.add_step(json!({"op": "removeErrors", "columns": cols}));
                    ui.close();
                }
            });
            if ui.add_enabled(one, egui::Button::new(lang.tr("Sort Ascending"))).clicked() {
                ed.add_step(json!({"op": "sort", "keys": [{"column": first}]}));
            }
            if ui.add_enabled(one, egui::Button::new(lang.tr("Sort Descending"))).clicked() {
                ed.add_step(json!({"op": "sort", "keys": [{"column": first, "descending": true}]}));
            }
            ui.separator();
            if ui.add_enabled(one, egui::Button::new(lang.tr("Split Column…"))).clicked() {
                ed.sub = Some(Sub::Split { delimiter: ",".into(), custom: String::new(), mode: "each".into() });
            }
            if ui.add_enabled(has_cols, egui::Button::new(lang.tr("Group By…"))).clicked() {
                ed.sub = Some(Sub::GroupBy { key: first.clone(), name: "Count".into(), op: "count".into(), column: String::new() });
            }
            type_menu(ui, lang, ed);
            ui.menu_button(lang.tr("Use First Row as Headers ▾"), |ui| {
                if ui.button(lang.tr("Use First Row as Headers")).clicked() {
                    ed.add_step(json!({"op": "promoteHeaders"}));
                    ui.close();
                }
                if ui.button(lang.tr("Use Headers as First Row")).clicked() {
                    ed.add_step(json!({"op": "demoteHeaders"}));
                    ui.close();
                }
            });
            if ui.add_enabled(!ed.selected.is_empty(), egui::Button::new(lang.tr("Replace Values…"))).clicked() {
                ed.sub = Some(Sub::Replace { find: String::new(), with: String::new(), whole: false });
            }
            ui.separator();
            if ui.button(lang.tr("Merge Queries…")).clicked() {
                ed.sub = Some(Sub::Merge { query: String::new(), left: ed.selected.clone(), right: vec![], join: "leftOuter".into() });
            }
            if ui.button(lang.tr("Append Queries…")).clicked() {
                ed.sub = Some(Sub::Append { queries: vec![] });
            }
        }
        Tab::Transform => {
            if ui.add_enabled(has_cols, egui::Button::new(lang.tr("Group By…"))).clicked() {
                ed.sub = Some(Sub::GroupBy { key: first.clone(), name: "Count".into(), op: "count".into(), column: String::new() });
            }
            if ui.button(lang.tr("Use First Row as Headers")).clicked() {
                ed.add_step(json!({"op": "promoteHeaders"}));
            }
            if ui.button(lang.tr("Transpose")).clicked() {
                ed.add_step(json!({"op": "transpose"}));
            }
            type_menu(ui, lang, ed);
            if ui.add_enabled(one, egui::Button::new(lang.tr("Rename"))).clicked() {
                ed.sub = Some(Sub::Rename { from: first.clone(), to: first.clone() });
            }
            if ui.add_enabled(!ed.selected.is_empty(), egui::Button::new(lang.tr("Replace Values…"))).clicked() {
                ed.sub = Some(Sub::Replace { find: String::new(), with: String::new(), whole: false });
            }
            ui.menu_button(lang.tr("Fill ▾"), |ui| {
                for (label, op) in [("Down", "fillDown"), ("Up", "fillUp")] {
                    if ui.add_enabled(!ed.selected.is_empty(), egui::Button::new(lang.tr(label))).clicked() {
                        let cols = ed.selected.clone();
                        ed.add_step(json!({"op": op, "columns": cols}));
                        ui.close();
                    }
                }
            });
            if ui.add_enabled(!ed.selected.is_empty(), egui::Button::new(lang.tr("Unpivot Columns"))).clicked() {
                let cols = ed.selected.clone();
                ed.selected.clear();
                ed.add_step(json!({"op": "unpivot", "columns": cols}));
            }
            if ui.add_enabled(one, egui::Button::new(lang.tr("Split Column…"))).clicked() {
                ed.sub = Some(Sub::Split { delimiter: ",".into(), custom: String::new(), mode: "each".into() });
            }
            ui.menu_button(lang.tr("Format ▾"), |ui| {
                for (label, f) in
                    [("lowercase", "lower"), ("UPPERCASE", "upper"), ("Capitalize Each Word", "proper"), ("Trim", "trim"), ("Clean", "clean")]
                {
                    if ui.button(lang.tr(label)).clicked() {
                        let cols = ed.targets();
                        ed.add_step(json!({"op": "transformText", "columns": cols, "function": f}));
                        ui.close();
                    }
                }
            });
        }
        Tab::AddColumn => {
            if ui.add_enabled(has_cols, egui::Button::new(lang.tr("Conditional Column…"))).clicked() {
                ed.sub = Some(Sub::Conditional {
                    name: "Custom".into(),
                    rules: vec![[first.clone(), "equals".into(), String::new(), String::new()]],
                    otherwise: String::new(),
                });
            }
            ui.menu_button(lang.tr("Index Column ▾"), |ui| {
                if ui.button(lang.tr("From 0")).clicked() {
                    ed.add_step(json!({"op": "addIndexColumn", "start": 0, "step": 1}));
                    ui.close();
                }
                if ui.button(lang.tr("From 1")).clicked() {
                    ed.add_step(json!({"op": "addIndexColumn", "start": 1, "step": 1}));
                    ui.close();
                }
                if ui.button(lang.tr("Custom…")).clicked() {
                    ed.sub = Some(Sub::Index { start: "0".into(), step: "1".into() });
                    ui.close();
                }
            });
            if ui.add_enabled(one, egui::Button::new(lang.tr("Duplicate Column"))).clicked() {
                ed.add_step(json!({"op": "duplicateColumn", "column": first}));
            }
            ui.menu_button(lang.tr("Format ▾"), |ui| {
                for (label, f) in
                    [("lowercase", "lower"), ("UPPERCASE", "upper"), ("Capitalize Each Word", "proper"), ("Trim", "trim"), ("Clean", "clean")]
                {
                    if ui.button(lang.tr(label)).clicked() {
                        let cols = ed.targets();
                        ed.add_step(json!({"op": "transformText", "columns": cols, "function": f}));
                        ui.close();
                    }
                }
            });
        }
        Tab::View => {
            if ui.button(lang.tr("Advanced Editor")).clicked() {
                ed.advanced = true;
            }
        }
    });
}

/// Applies what a column header asked for: selection, rename, sort, filters.
fn header_action(ed: &mut Editor, action: HeaderAction) {
    match action {
        HeaderAction::Select(col, add) => {
            if !add {
                ed.selected = vec![col];
            } else if ed.selected.contains(&col) {
                ed.selected.retain(|c| *c != col);
            } else {
                ed.selected.push(col);
            }
        }
        HeaderAction::Rename(col) => ed.sub = Some(Sub::Rename { to: col.clone(), from: col }),
        HeaderAction::Sort(col, descending) => ed.add_step(json!({"op": "sort", "keys": [{"column": col, "descending": descending}]})),
        HeaderAction::RemoveEmpty(col) => {
            ed.add_step(json!({"op": "filterRows", "column": col, "conditions": [{"op": "isNotNull"}, {"op": "notEquals", "value": ""}]}))
        }
        HeaderAction::Filter(col, op) => {
            ed.sub = Some(Sub::Filter { column: col, op: op.into(), value: String::new(), op2: String::new(), value2: String::new(), or: false })
        }
        HeaderAction::Values(col) => {
            let vals = distinct_values(&ed.preview, &col);
            ed.sub = Some(Sub::Values { column: col, values: vals.into_iter().map(|v| (v, true)).collect() });
        }
    }
}

fn type_menu(ui: &mut Ui, lang: Language, ed: &mut Editor) {
    ui.menu_button(lang.tr("Data Type ▾"), |ui| {
        for (ty, label) in type_items() {
            if ui.button(lang.tr(label)).clicked() {
                change_type(ed, ty, None);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(lang.tr("Using Locale…")).clicked() {
            ed.sub = Some(Sub::Locale { ty: "number".into(), locale: "de-DE".into() });
            ui.close();
        }
    });
}

fn distinct_values(p: &Json, column: &str) -> Vec<Json> {
    let Some(i) = strings(&p["columns"]).iter().position(|c| c == column) else { return vec![] };
    let mut out: Vec<Json> = Vec::new();
    for row in p["rows"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let v = row.get(i).cloned().unwrap_or(Json::Null);
        if !out.contains(&v) {
            out.push(v);
        }
        if out.len() >= FILTER_VALUES {
            break;
        }
    }
    out
}

/// A literal from what was typed: numbers as numbers, `true`/`false`, empty as empty text.
fn literal(text: &str) -> Json {
    let t = text.trim();
    if let Ok(n) = t.parse::<f64>() {
        return json!(n);
    }
    match t.to_ascii_lowercase().as_str() {
        "true" => json!(true),
        "false" => json!(false),
        "null" => Json::Null,
        _ => json!(text),
    }
}

const FILTER_OPS: [(&str, &str); 14] = [
    ("equals", "equals"),
    ("notEquals", "does not equal"),
    ("beginsWith", "begins with"),
    ("notBeginsWith", "does not begin with"),
    ("endsWith", "ends with"),
    ("notEndsWith", "does not end with"),
    ("contains", "contains"),
    ("notContains", "does not contain"),
    ("greaterThan", "is greater than"),
    ("greaterOrEqual", "is greater than or equal to"),
    ("lessThan", "is less than"),
    ("lessOrEqual", "is less than or equal to"),
    ("isNull", "is null"),
    ("isNotNull", "is not null"),
];

fn op_combo(ui: &mut Ui, id: &str, lang: Language, op: &mut String, allow_none: bool) {
    let current = FILTER_OPS.iter().find(|(k, _)| *k == op.as_str()).map(|(_, l)| lang.tr(l)).unwrap_or("");
    egui::ComboBox::from_id_salt(id).width(190.0).selected_text(current).show_ui(ui, |ui| {
        if allow_none && ui.selectable_label(op.is_empty(), "").clicked() {
            op.clear();
        }
        for (k, l) in FILTER_OPS {
            if ui.selectable_label(*op == k, lang.tr(l)).clicked() {
                *op = k.to_string();
            }
        }
    });
}

fn col_combo(ui: &mut Ui, id: &str, columns: &[String], current: &mut String) {
    if let Some(p) = combo(ui, id, current, columns) {
        *current = p;
    }
}

/// Draws the open sub-dialog; OK adds its step. Returns true when the editor closes (Close &
/// Load To saved the queries).
fn sub_dialog(app: &mut SheetApp, ctx: &egui::Context, ed: &mut Editor) -> bool {
    let Some(mut sub) = ed.sub.take() else { return false };
    let mut close_editor = false;
    let lang = app.ui.language;
    let columns = ed.columns();
    let title = match &sub {
        Sub::Rows { op, .. } => match *op {
            "keepTopRows" => "Keep Top Rows",
            "keepBottomRows" => "Keep Bottom Rows",
            "keepRange" => "Keep Range of Rows",
            "removeTopRows" => "Remove Top Rows",
            _ => "Remove Bottom Rows",
        },
        Sub::Split { .. } => "Split Column by Delimiter",
        Sub::GroupBy { .. } => "Group By",
        Sub::Replace { .. } => "Replace Values",
        Sub::Rename { .. } => "Rename",
        Sub::Conditional { .. } => "Add Conditional Column",
        Sub::Index { .. } => "Add Index Column",
        Sub::Filter { .. } => "Filter Rows",
        Sub::Values { .. } => "Filter Rows",
        Sub::Merge { .. } => "Merge",
        Sub::Append { .. } => "Append",
        Sub::Locale { .. } => "Change Type with Locale",
        Sub::LoadTo { .. } => "Import Data",
        Sub::Properties { .. } => "Query Properties",
    };
    let mut keep = true;
    let mut step: Option<Json> = None;
    let mut done = false;
    egui::Window::new(lang.tr(title)).id(egui::Id::new("pq_sub")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.set_min_width(360.0);
        match &mut sub {
            Sub::Rows { op, count, first } => {
                if *op == "keepRange" {
                    ui.label(lang.tr("First row"));
                    ui.text_edit_singleline(first);
                }
                ui.label(lang.tr("Number of rows"));
                ui.text_edit_singleline(count);
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok {
                    let n = count.trim().parse::<u64>().unwrap_or(0);
                    step = Some(if *op == "keepRange" {
                        json!({"op": "keepRange", "first": first.trim().parse::<u64>().unwrap_or(1).max(1), "count": n})
                    } else {
                        json!({"op": *op, "count": n})
                    });
                }
            }
            Sub::Split { delimiter, custom, mode } => {
                ui.label(lang.tr("Select or enter delimiter"));
                egui::ComboBox::from_id_salt("pq_split_d").selected_text(lang.tr(split_label(delimiter))).show_ui(ui, |ui| {
                    for (v, l) in [(",", "Comma"), (";", "Semicolon"), ("\t", "Tab"), (" ", "Space"), ("=", "Equal Sign"), ("custom", "--Custom--")] {
                        if ui.selectable_label(*delimiter == v, lang.tr(l)).clicked() {
                            *delimiter = v.to_string();
                        }
                    }
                });
                if delimiter == "custom" {
                    ui.text_edit_singleline(custom);
                }
                ui.label(lang.tr("Split at"));
                for (v, l) in [("leftMost", "Left-most delimiter"), ("rightMost", "Right-most delimiter"), ("each", "Each occurrence of the delimiter")] {
                    if ui.radio(*mode == v, lang.tr(l)).clicked() {
                        *mode = v.to_string();
                    }
                }
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok && let Some(col) = ed.selected.first() {
                    let d = if delimiter == "custom" { custom.clone() } else { delimiter.clone() };
                    step = Some(json!({"op": "splitColumn", "column": col, "delimiter": d, "mode": mode}));
                }
            }
            Sub::GroupBy { key, name, op, column } => {
                ui.label(lang.tr("Specify the column to group by and the desired output."));
                col_combo(ui, "pq_gb_key", &columns, key);
                egui::Grid::new("pq_gb").num_columns(3).show(ui, |ui| {
                    ui.label(lang.tr("New column name"));
                    ui.label(lang.tr("Operation"));
                    ui.label(lang.tr("Column"));
                    ui.end_row();
                    ui.text_edit_singleline(name);
                    let ops = [("count", "Count Rows"), ("countDistinct", "Count Distinct Rows"), ("sum", "Sum"), ("average", "Average"), ("median", "Median"), ("min", "Min"), ("max", "Max")];
                    let cur = ops.iter().find(|(k, _)| *k == op.as_str()).map(|(_, l)| lang.tr(l)).unwrap_or("");
                    egui::ComboBox::from_id_salt("pq_gb_op").selected_text(cur).show_ui(ui, |ui| {
                        for (k, l) in ops {
                            if ui.selectable_label(*op == k, lang.tr(l)).clicked() {
                                *op = k.to_string();
                            }
                        }
                    });
                    ui.add_enabled_ui(op != "count", |ui| col_combo(ui, "pq_gb_col", &columns, column));
                    ui.end_row();
                });
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok {
                    let mut agg = json!({"name": name, "op": op});
                    if op != "count" {
                        agg["column"] = json!(column);
                    }
                    step = Some(json!({"op": "groupBy", "keys": [key], "aggregations": [agg]}));
                }
            }
            Sub::Replace { find, with, whole } => {
                ui.label(lang.tr("Value To Find"));
                ui.text_edit_singleline(find);
                ui.label(lang.tr("Replace With"));
                ui.text_edit_singleline(with);
                ui.checkbox(whole, lang.tr("Match entire cell contents"));
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok {
                    let f = if *whole { literal(find) } else { json!(find) };
                    let w = if *whole { literal(with) } else { json!(with) };
                    step = Some(json!({"op": "replaceValues", "columns": ed.selected, "find": f, "replace": w, "whole": whole}));
                }
            }
            Sub::Rename { from, to } => {
                ui.label(format!("{} '{from}'", lang.tr("New name for")));
                let r = ui.text_edit_singleline(to);
                r.request_focus();
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if (ok || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !to.trim().is_empty() && to != from {
                    step = Some(json!({"op": "renameColumns", "renames": [{"from": from, "to": to.trim()}]}));
                    ed.selected = vec![to.trim().to_string()];
                }
            }
            Sub::Conditional { name, rules, otherwise } => {
                ui.horizontal(|ui| {
                    ui.label(lang.tr("New column name"));
                    ui.text_edit_singleline(name);
                });
                let mut remove = None;
                egui::Grid::new("pq_cond").num_columns(6).show(ui, |ui| {
                    for (i, r) in rules.iter_mut().enumerate() {
                        ui.label(lang.tr(if i == 0 { "If" } else { "Else If" }));
                        let [col, op, value, out] = r;
                        col_combo(ui, &format!("pq_cond_c{i}"), &columns, col);
                        op_combo(ui, &format!("pq_cond_o{i}"), lang, op, false);
                        ui.add(egui::TextEdit::singleline(value).desired_width(90.0).hint_text(lang.tr("Value")));
                        ui.add(egui::TextEdit::singleline(out).desired_width(90.0).hint_text(lang.tr("Output")));
                        if i > 0 && ui.small_button("✕").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = remove
                    && rules.len() > 1
                {
                    rules.remove(i);
                }
                if ui.button(lang.tr("Add Clause")).clicked() {
                    rules.push([columns.first().cloned().unwrap_or_default(), "equals".into(), String::new(), String::new()]);
                }
                ui.horizontal(|ui| {
                    ui.label(lang.tr("Else"));
                    ui.text_edit_singleline(otherwise);
                });
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok {
                    let rs: Vec<Json> = rules.iter().map(|[c, o, v, out]| json!({"column": c, "op": o, "value": literal(v), "output": {"value": literal(out)}})).collect();
                    step = Some(json!({"op": "addConditionalColumn", "newColumn": name, "rules": rs, "otherwise": {"value": literal(otherwise)}}));
                }
            }
            Sub::Index { start, step: inc } => {
                ui.label(lang.tr("Starting Index"));
                ui.text_edit_singleline(start);
                ui.label(lang.tr("Increment"));
                ui.text_edit_singleline(inc);
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok {
                    step = Some(json!({"op": "addIndexColumn", "start": start.trim().parse::<i64>().unwrap_or(0), "step": inc.trim().parse::<i64>().unwrap_or(1)}));
                }
            }
            Sub::Filter { column, op, value, op2, value2, or } => {
                ui.label(format!("{} '{column}'", lang.tr("Keep rows where")));
                ui.horizontal(|ui| {
                    op_combo(ui, "pq_f1", lang, op, false);
                    ui.text_edit_singleline(value);
                });
                ui.horizontal(|ui| {
                    if ui.radio(!*or, lang.tr("And")).clicked() {
                        *or = false;
                    }
                    if ui.radio(*or, lang.tr("Or")).clicked() {
                        *or = true;
                    }
                });
                ui.horizontal(|ui| {
                    op_combo(ui, "pq_f2", lang, op2, true);
                    ui.text_edit_singleline(value2);
                });
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok {
                    let mut conds = vec![json!({"op": op, "value": literal(value)})];
                    if !op2.is_empty() {
                        conds.push(json!({"op": op2, "value": literal(value2)}));
                    }
                    step = Some(json!({"op": "filterRows", "column": column, "conditions": conds, "or": or}));
                }
            }
            Sub::Values { column, values } => {
                ui.label(format!("{} '{column}'", lang.tr("Keep rows where")));
                ui.horizontal(|ui| {
                    if ui.small_button(lang.tr("(Select All)")).clicked() {
                        values.iter_mut().for_each(|v| v.1 = true);
                    }
                    if ui.small_button(lang.tr("(Clear)")).clicked() {
                        values.iter_mut().for_each(|v| v.1 = false);
                    }
                });
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (v, on) in values.iter_mut() {
                        let (text, _) = cell_text(v);
                        ui.checkbox(on, if text.is_empty() { "(blank)".to_string() } else { text });
                    }
                });
                if ed.preview.get("rowCount").and_then(Json::as_u64).unwrap_or(0) as usize > PREVIEW_ROWS {
                    ui.label(RichText::new(lang.tr("The list may be incomplete: it shows values of the preview rows.")).small());
                }
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok && values.iter().any(|v| !v.1) {
                    let kept: Vec<Json> = values.iter().filter(|v| v.1).map(|v| v.0.clone()).collect();
                    step = Some(json!({"op": "filterRows", "column": column, "values": kept}));
                }
            }
            Sub::Merge { query, left, right, join } => {
                let others: Vec<String> = ed.drafts.iter().map(|d| s(&d.def, "name").to_string()).collect();
                ui.label(lang.tr("Select tables and matching columns to create a merged table."));
                ui.label(RichText::new(ed.name()).strong());
                column_picker(ui, "pq_m_l", &columns, left);
                if let Some(p) = combo(ui, "pq_m_q", query, &others) {
                    *query = p;
                    right.clear();
                }
                let rc = if query.is_empty() {
                    vec![]
                } else {
                    let def = ed.drafts.iter().find(|d| s(&d.def, "name") == query.as_str()).map(|d| d.def.clone()).unwrap_or(Json::Null);
                    remembered_columns(ui, &def.to_string(), || {
                        app.session.run("query.preview", json!({"query": def, "rows": 0})).ok().map(|p| strings(&p["columns"])).unwrap_or_default()
                    })
                };
                column_picker(ui, "pq_m_r", &rc, right);
                ui.label(lang.tr("Join Kind"));
                join_combo(ui, lang, join);
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok && !query.is_empty() {
                    step = Some(json!({"op": "mergeQueries", "query": query, "left": left, "right": right, "join": join, "prefix": query}));
                }
            }
            Sub::Append { queries } => {
                let others: Vec<String> = ed.drafts.iter().map(|d| s(&d.def, "name").to_string()).filter(|n| *n != ed.name()).collect();
                ui.label(lang.tr("Tables to append (in order):"));
                column_picker(ui, "pq_a", &others, queries);
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok && !queries.is_empty() {
                    step = Some(json!({"op": "appendQueries", "queries": queries}));
                }
            }
            Sub::Locale { ty, locale } => {
                egui::ComboBox::from_id_salt("pq_loc_t").selected_text(lang.tr(type_items().iter().find(|(k, _)| *k == ty.as_str()).map(|(_, l)| *l).unwrap_or(""))).show_ui(ui, |ui| {
                    for (k, l) in type_items() {
                        if ui.selectable_label(*ty == k, lang.tr(l)).clicked() {
                            *ty = k.to_string();
                        }
                    }
                });
                ui.label(lang.tr("Locale"));
                ui.text_edit_singleline(locale);
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok {
                    let (t, l) = (ty.clone(), locale.clone());
                    change_type(ed, &t, Some(&l));
                    done = true;
                }
            }
            Sub::LoadTo { values } => {
                if !values.contains_key("cell") {
                    values.insert("cell".into(), json!(default_cell(app)));
                }
                let (choice, cancel) = import_data_ui(app, ui, values);
                keep = !cancel;
                if let Some(target) = choice {
                    match save(app, ed, Some(target)) {
                        Ok(()) => {
                            done = true;
                            close_editor = true;
                        }
                        Err(e) => ed.message = Some(crate::clean_error(&e)),
                    }
                }
            }
            Sub::Properties { name, description } => {
                ui.label(lang.tr("Name"));
                ui.text_edit_singleline(name);
                ui.label(lang.tr("Description"));
                ui.add(egui::TextEdit::multiline(description).desired_rows(3));
                let (ok, cancel) = buttons(ui, lang, "OK");
                keep = !cancel;
                if ok {
                    if let Some(d) = ed.drafts.get_mut(ed.current) {
                        d.def["description"] = json!(description);
                        if !name.trim().is_empty() {
                            d.def["name"] = json!(name.trim());
                        }
                    }
                    ed.touch();
                    done = true;
                }
            }
        }
    });
    if let Some(st) = step {
        ed.add_step(st);
        done = true;
    }
    if keep && !done {
        ed.sub = Some(sub);
    }
    close_editor
}

fn split_label(d: &str) -> &'static str {
    match d {
        "," => "Comma",
        ";" => "Semicolon",
        "\t" => "Tab",
        " " => "Space",
        "=" => "Equal Sign",
        _ => "--Custom--",
    }
}
