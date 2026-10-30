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
    match app.run("data.databaseColumns", p) {
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

pub(super) fn from_database(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
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
        if ui.button(lang.tr("Connect")).on_hover_text(lang.tr("Connect and list the tables")).clicked() {
            match app.run("data.databaseTables", db_conn_params(d)) {
                Ok(t) => {
                    d.values.insert("tables".into(), t);
                    d.values.insert("tablesFor".into(), json!(db_list_key(d)));
                    d.values.remove("error");
                }
                Err(e) => {
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
            let mut prof = db_profile(d);
            prof.name = name.trim().to_string();
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
