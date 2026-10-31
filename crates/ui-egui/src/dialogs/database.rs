//! Data › From Database: connection fields, saved connections, table and column picker or an
//! SQL query, and the password asked for on Refresh.

use egui::vec2;

use super::show::ok_cancel;
use super::{Dialog, Json, SheetApp, json};

fn db_str(d: &Dialog, k: &str) -> String {
    d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string()
}

/// The connection fields of the From Database dialog.
fn db_profile(d: &Dialog) -> gridcraft_engine::datasource::ConnectionProfile {
    let edit = d.values.get("editConnection").and_then(Json::as_bool).unwrap_or(false);
    gridcraft_engine::datasource::ConnectionProfile {
        name: db_str(d, "saveName"),
        driver: db_str(d, "driver"),
        host: db_str(d, "host"),
        port: db_str(d, "port"),
        database: db_str(d, "database"),
        file: db_str(d, "file"),
        dsn: db_str(d, "dsn"),
        user: db_str(d, "user"),
        custom: if edit { db_str(d, "connection") } else { String::new() },
    }
}

/// `p` with only the fields its driver uses, so a value left in another driver's field (a DSN
/// typed before switching to PostgreSQL) isn't saved with it.
fn saved_profile(mut p: gridcraft_engine::datasource::ConnectionProfile) -> gridcraft_engine::datasource::ConnectionProfile {
    let network = matches!(p.driver.as_str(), "postgres" | "mysql");
    if !network {
        p.host.clear();
        p.port.clear();
        p.database.clear();
    }
    if p.driver != "sqlite" {
        p.file.clear();
    }
    if p.driver != "odbc" {
        p.dsn.clear();
    }
    p
}

/// Driver, connection, user and password, as the database commands take them.
fn db_conn_params(d: &Dialog) -> Json {
    let prof = db_profile(d);
    let mut v = json!({"driver": prof.driver, "connection": prof.connection()});
    for k in ["user", "password"] {
        let x = db_str(d, k);
        if !x.trim().is_empty() {
            v[k] = json!(if k == "user" { x.trim().to_string() } else { x });
        }
    }
    v
}

/// What the connection list was loaded for: a changed connection hides the stale list.
fn db_list_key(d: &Dialog) -> String {
    let c = db_conn_params(d);
    format!("{}\n{}\n{}", c["driver"], c["connection"], c["user"])
}

/// The `data.fromDatabase` params of the dialog.
pub(super) fn db_import_params(d: &Dialog) -> Json {
    let mut v = db_conn_params(d);
    let sheet = db_str(d, "sheet");
    if !sheet.trim().is_empty() {
        v["sheet"] = json!(sheet.trim());
    }
    if db_str(d, "mode") == "sql" {
        v["query"] = json!(db_str(d, "query"));
        return v;
    }
    let table = db_str(d, "table");
    v["table"] = json!(table);
    let schema = db_str(d, "schema");
    if !schema.is_empty() {
        v["schema"] = json!(schema);
    }
    // Every column ticked = all of them, so columns added to the table later come in on Refresh.
    let all = d.values.get("columns").and_then(Json::as_array).map_or(0, Vec::len);
    let selected: Vec<Json> = d.values.get("selected").and_then(Json::as_array).cloned().unwrap_or_default();
    if selected.len() < all {
        v["columns"] = Json::Array(selected);
    }
    v
}

/// Loads the columns of the chosen table and ticks them all.
fn db_load_columns(app: &mut SheetApp, d: &mut Dialog) {
    let mut p = db_conn_params(d);
    p["table"] = json!(db_str(d, "table"));
    p["schema"] = json!(db_str(d, "schema"));
    let Some(r) = app.run_db("data.databaseColumns", p) else {
        d.values.insert("busy".into(), json!("columns"));
        return;
    };
    d.values.remove("busy");
    match r {
        Ok(cols) => {
            let names: Vec<Json> = cols.as_array().map(|a| a.iter().map(|c| c["name"].clone()).collect()).unwrap_or_default();
            d.values.insert("columns".into(), cols);
            d.values.insert("selected".into(), Json::Array(names));
            d.values.remove("error");
        }
        Err(e) => {
            d.values.insert("columns".into(), json!([]));
            d.values.insert("selected".into(), json!([]));
            d.values.insert("error".into(), json!(crate::clean_error(&e)));
        }
    }
}

/// While a database request of the dialog runs in the background: a spinner and Cancel.
pub(super) fn wait_row(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    if db_str(d, "busy").is_empty() {
        return;
    }
    let lang = crate::i18n::current(ui.ctx());
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(lang.tr("Waiting for the database…"));
        if ui.button(lang.tr("Cancel")).clicked() {
            app.cancel_db();
            d.values.remove("busy");
        }
    });
}

/// Runs the command of a confirmed dialog: `None` when there is nothing to run, or when its
/// database requests run in the background. Then OK is pressed again for the user once the
/// answers are in, and not before.
pub(super) fn run_confirmed(app: &mut SheetApp, d: &mut Dialog, confirm: &mut bool) -> Option<Result<Json, String>> {
    match db_str(d, "busy").as_str() {
        "" => {}
        "ok" => *confirm = !app.db_running(),
        _ => *confirm = false,
    }
    let cmd = d.command.filter(|_| *confirm)?;
    if !crate::db_jobs::is_db_command(cmd) {
        return Some(app.run(cmd, super::show::build_params(d)));
    }
    let Some(r) = app.run_db(cmd, super::show::build_params(d)) else {
        d.values.insert("busy".into(), json!("ok"));
        return None;
    };
    d.values.remove("busy");
    Some(r)
}

/// A dialog closed while waiting for the database: its answers are no longer wanted.
pub(super) fn stop_waiting(app: &mut SheetApp, d: &Dialog) {
    if !db_str(d, "busy").is_empty() {
        app.cancel_db();
    }
}

/// What the dialog was waiting for, once the background requests are in ("" = nothing).
fn resumed(app: &SheetApp, d: &Dialog) -> String {
    if app.db_running() { String::new() } else { db_str(d, "busy") }
}

pub(super) fn from_database(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let waiting = !db_str(d, "busy").is_empty();
    let resume = resumed(app, d);
    if resume == "columns" {
        db_load_columns(app, d);
    }
    let lang = crate::i18n::current(ui.ctx());
    let text_row = |ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str, hint: &str| {
        ui.label(label);
        let mut v = db_str(d, key);
        if ui.add(egui::TextEdit::singleline(&mut v).hint_text(hint).desired_width(300.0)).changed() {
            d.set_value(key, json!(v));
        }
        ui.end_row();
    };
    // Saved connections (preferences; never a password).
    let profiles = app.session.prefs.db_connections.clone();
    ui.horizontal(|ui| {
        ui.label(lang.tr("Saved connection:"));
        let current = db_str(d, "profile");
        craft_ui::ComboBox::from_id_salt("dbProfile")
            .selected_text(if current.is_empty() { lang.tr("(new)").to_string() } else { current.clone() })
            .width(220.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_empty(), lang.tr("(new)")).clicked() {
                    d.values.insert("profile".into(), json!(""));
                }
                for pr in &profiles {
                    if ui.selectable_label(current == pr.name, &pr.name).clicked() {
                        for (k, v) in [
                            ("driver", &pr.driver),
                            ("host", &pr.host),
                            ("port", &pr.port),
                            ("database", &pr.database),
                            ("file", &pr.file),
                            ("dsn", &pr.dsn),
                            ("user", &pr.user),
                            ("connection", &pr.custom),
                            ("profile", &pr.name),
                            ("saveName", &pr.name),
                        ] {
                            d.values.insert(k.into(), json!(v));
                        }
                        d.values.insert("editConnection".into(), json!(!pr.custom.is_empty()));
                        d.values.insert("password".into(), json!(""));
                    }
                }
            });
        if !current.is_empty() && ui.button(lang.tr("Delete")).on_hover_text(lang.tr("Remove this saved connection")).clicked() {
            app.session.prefs.db_connections.retain(|p| p.name != current);
            d.values.insert("profile".into(), json!(""));
        }
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new(lang.tr("Database:")).strong());
    ui.horizontal(|ui| {
        let cur = db_str(d, "driver");
        for (val, text) in [("postgres", "PostgreSQL"), ("mysql", "MySQL / MariaDB"), ("sqlite", "SQLite file"), ("odbc", "ODBC")] {
            if ui.radio(cur == val, lang.tr(text)).clicked() {
                d.set_value("driver", json!(val));
            }
        }
    });
    let driver = db_str(d, "driver");
    egui::Grid::new("dbConn").num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        match driver.as_str() {
            "sqlite" => text_row(ui, d, "file", lang.tr("Database file:"), "/path/data.db"),
            "odbc" => {
                text_row(ui, d, "dsn", lang.tr("Data source:"), lang.tr("DSN name, or Driver={…};Server=…"));
                text_row(ui, d, "user", lang.tr("User name:"), lang.tr("optional"));
            }
            _ => {
                text_row(ui, d, "host", lang.tr("Server:"), "localhost");
                text_row(ui, d, "port", lang.tr("Port:"), if driver == "mysql" { "3306" } else { "5432" });
                text_row(ui, d, "database", lang.tr("Database:"), "");
                text_row(ui, d, "user", lang.tr("User name:"), "");
            }
        }
        if driver != "sqlite" {
            ui.label(lang.tr("Password:"));
            let mut pw = db_str(d, "password");
            if ui.add(egui::TextEdit::singleline(&mut pw).password(!d.show_password).desired_width(300.0)).changed() {
                d.values.insert("password".into(), json!(pw));
            }
            ui.end_row();
        }
    });
    // The composed connection string, editable on request.
    let mut edit = d.values.get("editConnection").and_then(Json::as_bool).unwrap_or(false);
    ui.horizontal(|ui| {
        if ui.checkbox(&mut edit, lang.tr("Edit connection string")).changed() {
            if edit && db_str(d, "connection").is_empty() {
                let composed = db_profile(d).connection();
                d.values.insert("connection".into(), json!(composed));
            }
            d.values.insert("editConnection".into(), json!(edit));
        }
        if driver != "sqlite" {
            ui.checkbox(&mut d.show_password, lang.tr("Show password"));
        }
    });
    if edit {
        let mut c = db_str(d, "connection");
        if ui.add(egui::TextEdit::singleline(&mut c).desired_width(f32::INFINITY)).changed() {
            d.set_value("connection", json!(c));
        }
    } else {
        ui.label(egui::RichText::new(db_profile(d).connection()).monospace().weak());
    }
    ui.horizontal(|ui| {
        let connect = ui.add_enabled(!waiting, egui::Button::new(lang.tr("Connect"))).on_hover_text(lang.tr("Connect and list the tables")).clicked();
        if connect || resume == "tables" {
            match app.run_db("data.databaseTables", db_conn_params(d)) {
                None => {
                    d.values.insert("busy".into(), json!("tables"));
                }
                Some(Ok(t)) => {
                    d.values.remove("busy");
                    d.values.insert("tables".into(), t);
                    d.values.insert("tablesFor".into(), json!(db_list_key(d)));
                    d.values.remove("error");
                }
                Some(Err(e)) => {
                    d.values.remove("busy");
                    d.values.insert("tables".into(), json!([]));
                    d.values.insert("error".into(), json!(crate::clean_error(&e)));
                }
            }
        }
        ui.separator();
        let mut name = db_str(d, "saveName");
        ui.add(egui::TextEdit::singleline(&mut name).hint_text(lang.tr("connection name")).desired_width(150.0));
        d.values.insert("saveName".into(), json!(name.clone()));
        if ui
            .add_enabled(!name.trim().is_empty(), egui::Button::new(lang.tr("Save connection")))
            .on_hover_text(lang.tr("Keep these settings (not the password) for next time"))
            .clicked()
        {
            let mut prof = saved_profile(db_profile(d));
            prof.name = name.trim().to_string();
            if [&prof.custom, &prof.dsn, &prof.host, &prof.database, &prof.file].iter().any(|f| gridcraft_engine::datasource::redact(f) != **f) {
                d.values.insert(
                    "error".into(),
                    json!(lang.tr("Remove the password from the connection before saving it: it goes in the Password field, which is never saved.")),
                );
                return;
            }
            let list = &mut app.session.prefs.db_connections;
            match list.iter_mut().find(|p| p.name == prof.name) {
                Some(p) => *p = prof.clone(),
                None => list.push(prof.clone()),
            }
            d.values.insert("profile".into(), json!(prof.name));
            app.toast = Some((format!("{} '{}'", lang.tr("Connection saved:"), prof.name), crate::now_ms()));
        }
    });
    if let Some(e) = d.values.get("error").and_then(Json::as_str) {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    ui.separator();
    // What to import: a table (and its columns) or an SQL query.
    let mode = db_str(d, "mode");
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(lang.tr("Import:")).strong());
        if ui.radio(mode != "sql", lang.tr("Table")).clicked() {
            d.values.insert("mode".into(), json!("table"));
        }
        if ui.radio(mode == "sql", lang.tr("SQL query")).clicked() {
            d.values.insert("mode".into(), json!("sql"));
        }
    });
    if mode == "sql" {
        let mut q = db_str(d, "query");
        if ui.add(egui::TextEdit::multiline(&mut q).code_editor().desired_rows(5).desired_width(f32::INFINITY)).changed() {
            d.values.insert("query".into(), json!(q));
        }
    } else if db_str(d, "tablesFor") != db_list_key(d) {
        ui.label(egui::RichText::new(lang.tr("Press Connect to list the tables.")).weak());
    } else {
        db_table_picker(app, ui, d);
    }
    ui.horizontal(|ui| {
        ui.label(lang.tr("New sheet name:"));
        let mut sheet = db_str(d, "sheet");
        let hint = if mode == "sql" { "Query".to_string() } else { db_str(d, "table") };
        if ui.add(egui::TextEdit::singleline(&mut sheet).hint_text(hint).desired_width(220.0)).changed() {
            d.values.insert("sheet".into(), json!(sheet));
        }
    });
    ui.label(egui::RichText::new(lang.tr("The table remembers its query: Data › Refresh brings it up to date.")).small().weak());
    ok_cancel(ui, confirm, open);
    // OK needs something to import.
    if *confirm && mode != "sql" && db_str(d, "table").is_empty() {
        *confirm = false;
        d.values.insert("error".into(), json!(lang.tr("Connect and choose a table, or switch to SQL query.")));
    }
}

/// The table grid (filterable) and, below, the chosen table's columns as checkboxes.
fn db_table_picker(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = crate::i18n::current(ui.ctx());
    let tables: Vec<Json> = d.values.get("tables").and_then(Json::as_array).cloned().unwrap_or_default();
    let mut filter = db_str(d, "filter");
    ui.horizontal(|ui| {
        ui.label(format!("{} {}", tables.len(), lang.tr("tables and views")));
        if ui.add(egui::TextEdit::singleline(&mut filter).hint_text(lang.tr("Search")).desired_width(180.0)).changed() {
            d.values.insert("filter".into(), json!(filter.clone()));
        }
    });
    let needle = filter.to_lowercase();
    let (cur_table, cur_schema) = (db_str(d, "table"), db_str(d, "schema"));
    let mut picked: Option<(String, String)> = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("dbTables").max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("dbTablesGrid").num_columns(3).striped(true).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
                ui.label(egui::RichText::new(lang.tr("Name")).strong());
                ui.label(egui::RichText::new(lang.tr("Schema")).strong());
                ui.label(egui::RichText::new(lang.tr("Type")).strong());
                ui.end_row();
                for t in &tables {
                    let (name, schema, kind) =
                        (t["name"].as_str().unwrap_or(""), t["schema"].as_str().unwrap_or(""), t["type"].as_str().unwrap_or(""));
                    if !needle.is_empty() && !name.to_lowercase().contains(&needle) && !schema.to_lowercase().contains(&needle) {
                        continue;
                    }
                    let on = name == cur_table && schema == cur_schema;
                    if ui.selectable_label(on, name).clicked() && !on {
                        picked = Some((name.to_string(), schema.to_string()));
                    }
                    ui.label(schema);
                    ui.label(egui::RichText::new(kind.to_lowercase()).weak());
                    ui.end_row();
                }
            });
        });
    });
    if let Some((name, schema)) = picked {
        d.values.insert("table".into(), json!(name));
        d.values.insert("schema".into(), json!(schema));
        db_load_columns(app, d);
    }
    let columns: Vec<Json> = d.values.get("columns").and_then(Json::as_array).cloned().unwrap_or_default();
    if cur_table.is_empty() || columns.is_empty() {
        return;
    }
    let mut selected: Vec<String> = d
        .values
        .get("selected")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{} {cur_table}:", lang.tr("Columns of"))).strong());
        if ui.small_button(lang.tr("All")).clicked() {
            selected = columns.iter().filter_map(|c| c["name"].as_str().map(str::to_string)).collect();
        }
        if ui.small_button(lang.tr("None")).clicked() {
            selected.clear();
        }
    });
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("dbColumns").max_height(130.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("dbColumnsGrid").num_columns(2).spacing(vec2(12.0, 0.0)).show(ui, |ui| {
                for c in &columns {
                    let name = c["name"].as_str().unwrap_or("");
                    let mut on = selected.iter().any(|s| s == name);
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            selected.push(name.to_string());
                        } else {
                            selected.retain(|s| s != name);
                        }
                    }
                    ui.label(egui::RichText::new(c["type"].as_str().unwrap_or("")).weak());
                    ui.end_row();
                }
            });
        });
    });
    // Keep the database's column order.
    let ordered: Vec<Json> =
        columns.iter().filter_map(|c| c["name"].as_str()).filter(|n| selected.iter().any(|s| s == n)).map(|n| json!(n)).collect();
    d.values.insert("selected".into(), Json::Array(ordered));
    let driver = db_str(d, "driver");
    let cols: Vec<String> = if selected.len() == columns.len() { vec![] } else { selected };
    let sql = gridcraft_engine::datasource::select_sql(&driver, Some(&cur_schema), &cur_table, &cols);
    ui.label(egui::RichText::new(sql).monospace().small().weak());
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::datasource::ConnectionProfile;

    #[test]
    fn saved_profile_keeps_only_the_drivers_fields() {
        let p = ConnectionProfile {
            driver: "postgres".into(),
            host: "db".into(),
            dsn: "DSN=x;PWD=secret".into(),
            file: "/x.db".into(),
            ..Default::default()
        };
        let s = saved_profile(p);
        assert_eq!((s.host.as_str(), s.dsn.as_str(), s.file.as_str()), ("db", "", ""));
        let s = saved_profile(ConnectionProfile { driver: "odbc".into(), host: "db".into(), dsn: "DSN=x".into(), ..Default::default() });
        assert_eq!((s.host.as_str(), s.dsn.as_str()), ("", "DSN=x"));
    }

    #[test]
    fn ok_waits_for_the_database_and_then_imports() {
        use std::sync::Arc;
        use std::time::Duration;

        use gridcraft_engine::datasource::{DbRequest, DbTable, DbValue};

        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        session.data_source = Some(Arc::new(|_: &DbRequest| {
            std::thread::sleep(Duration::from_millis(150));
            Ok(DbTable { columns: vec!["a".into()], rows: vec![vec![DbValue::Int(1)]], truncated: false })
        }));
        let mut app = SheetApp::new(session, crate::Services::default());
        app.install_db_jobs(&egui::Context::default());
        app.open_dialog("fromDatabase", json!({"driver": "sqlite", "file": "/x.db", "mode": "sql", "query": "SELECT 1"}));
        let mut d = app.dialog.take().expect("dialog");
        let sheets = app.session.active().map(|doc| doc.wb.sheets.len());
        let mut confirm = true;
        assert!(run_confirmed(&mut app, &mut d, &mut confirm).is_none(), "OK must not wait for the server");
        assert_eq!(db_str(&d, "busy"), "ok");
        assert_eq!(app.session.active().map(|doc| doc.wb.sheets.len()), sheets);
        // Frames go by: OK is held back while the request runs, then pressed again.
        let mut result = None;
        for _ in 0..200 {
            let mut confirm = false;
            if let Some(r) = run_confirmed(&mut app, &mut d, &mut confirm) {
                result = Some(r);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(result.is_some_and(|r| r.is_ok()));
        assert_eq!(app.session.active().map(|doc| doc.wb.sheets.len()), sheets.map(|n| n + 1));
        assert!(db_str(&d, "busy").is_empty());
    }
}
