//! ODBC forms consume engine jobs; driver access and workbook edits stay in the engine.
use crate::{
    SheetApp,
    dialogs::{Dialog, Field},
    theme::Tokens,
};
use egui::Ui;
use serde_json::{Value as Json, json};

#[derive(Default)]
pub(crate) struct State {
    job: Option<Job>,
    sources: Vec<String>,
    preview: Option<Json>,
    message: String,
    error: Option<String>,
}
struct Job {
    id: String,
    action: String,
    cancel: bool,
}

pub(crate) fn dialog(app: &mut SheetApp, params: Json) -> Dialog {
    if app.odbc.job.is_none() {
        app.odbc.preview = None;
        app.odbc.message.clear();
        app.odbc.error = None;
    }
    let mut values = json!({"mode":"dsn", "dsn":"", "connectionString":"", "user":"", "password":"", "sql":"SELECT 1 AS value", "maxRows":100000, "replace":false});
    if let Some(table) = params.get("table").and_then(Json::as_str) {
        if let Ok(queries) = app.run("data.queriesConnections", json!({}))
            && let Some(query) = queries.as_array().into_iter().flatten().find(|q| q["name"] == table).and_then(|q| q.get("query"))
        {
            values["dsn"] = query.get("dsn").filter(|d| d.is_string()).cloned().unwrap_or(json!(""));
            values["sql"] = query.get("sql").cloned().unwrap_or(json!(""));
            values["maxRows"] = query.get("maxRows").cloned().unwrap_or(json!(100000));
        }
        values["table"] = json!(table);
    }
    if let (Some(values), Some(params)) = (values.as_object_mut(), params.as_object()) {
        values.extend(params.iter().filter(|(k, _)| k.as_str() != "name").map(|(k, v)| (k.clone(), v.clone())));
    }
    let mut dialog = Dialog::custom("odbc", "From ODBC", values);
    dialog.fields = vec![
        Field::Text { key: "dsn", label: "Data source (DSN)" },
        Field::Password { key: "connectionString", label: "Connection string" },
        Field::Text { key: "user", label: "User name" },
        Field::Password { key: "password", label: "Password" },
        Field::Text { key: "sql", label: "SQL query" },
    ];
    dialog
}

pub(crate) fn start(app: &mut SheetApp, action: &str, params: Json) -> Result<(), String> {
    if app.odbc.job.is_some() {
        return Err("Finish or cancel the current query first.".into());
    }
    let result = app.run(action, params)?;
    if result.get("jobId").and_then(Json::as_str).is_none() {
        app.odbc.error = None;
        app.odbc.preview = None;
        app.odbc.message = "Refresh completed".into();
    }
    Ok(())
}

pub(crate) fn track_job(app: &mut SheetApp, action: &str, result: &Json) {
    let Some(id) = result.get("jobId").and_then(Json::as_str) else { return };
    app.odbc.error = None;
    app.odbc.preview = None;
    app.odbc.message.clear();
    app.odbc.job = Some(Job { id: id.into(), action: action.into(), cancel: false });
}

pub(crate) fn cancel(app: &mut SheetApp) {
    if let Some(job) = &mut app.odbc.job {
        job.cancel = true;
        let id = job.id.clone();
        let _ = app.run("data.odbcCancel", json!({"jobId":id}));
    }
}

pub(crate) fn poll(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(job) = app.odbc.job.take() else { return };
    let status = app.run("data.odbcStatus", json!({"jobId":job.id}));
    match status {
        Ok(status) => match status["state"].as_str().unwrap_or("") {
            "running" => {
                app.odbc.job = Some(job);
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            "ready" if !job.cancel => match app.run("data.odbcApply", json!({"jobId":job.id})) {
                Ok(result) if job.action == "data.odbcPreview" => app.odbc.preview = Some(result),
                Ok(result) => {
                    app.odbc.message = "Query loaded".into();
                    if matches!(job.action.as_str(), "data.fromOdbc" | "data.refreshQuery") {
                        app.dialog = None;
                    }
                    if let Some(errors) = result.get("errors").and_then(Json::as_array).filter(|e| !e.is_empty()) {
                        app.odbc.error = Some(serde_json::to_string_pretty(errors).unwrap_or_default());
                    }
                    if result["truncated"] == true {
                        app.message = Some((
                            "ODBC".into(),
                            app.ui.language.tr("The row limit was reached. Increase the limit or restrict the SQL query.").into(),
                        ));
                    }
                }
                Err(error) => {
                    let _ = app.run("data.odbcCancel", json!({"jobId":job.id}));
                    app.odbc.error = Some(crate::clean_error(&error));
                }
            },
            "completed" => app.odbc.message = "Connection succeeded".into(),
            "cancelled" | "ready" => {
                let _ = app.run("data.odbcCancel", json!({"jobId":job.id}));
                app.odbc.message = "Query cancelled".into();
            }
            _ => app.odbc.error = Some(status["error"].as_str().unwrap_or("ODBC operation failed.").into()),
        },
        Err(error) => app.odbc.error = Some(crate::clean_error(&error)),
    }
    if app.odbc.error.is_some() && app.dialog.is_none() {
        app.message = Some(("ODBC".into(), app.odbc.error.clone().unwrap_or_default()));
    }
}

fn text(ui: &mut Ui, dialog: &mut Dialog, key: &str, label: &str, password: bool, multiline: bool) {
    let language = crate::i18n::current(ui.ctx());
    let label = ui.label(language.tr(label));
    let mut value = dialog.values.get(key).and_then(Json::as_str).unwrap_or_default().to_string();
    let edit = if multiline {
        egui::TextEdit::multiline(&mut value).desired_rows(4).code_editor()
    } else {
        egui::TextEdit::singleline(&mut value).password(password)
    };
    let response = ui.add(edit.id(egui::Id::new(key)).desired_width(f32::INFINITY).char_limit(if key == "sql" { 1_000_000 } else { 16_384 }));
    response.labelled_by(label.id);
    dialog.values.insert(key.into(), json!(value));
}

fn parameters(dialog: &Dialog) -> Json {
    let mut params = dialog.values.clone();
    let string_mode = params.get("mode").and_then(Json::as_str) == Some("connectionString");
    params.remove("mode");
    params.remove(if string_mode { "dsn" } else { "connectionString" });
    if params.contains_key("table") {
        params.retain(|key, value| {
            !matches!(key.as_str(), "dsn" | "connectionString" | "user" | "password") || value.as_str().is_some_and(|s| !s.is_empty())
        });
    }
    Json::Object(params)
}

pub(crate) fn show(app: &mut SheetApp, ui: &mut Ui, dialog: &mut Dialog, open: &mut bool) {
    let language = app.ui.language;
    let busy = app.odbc.job.is_some();
    let refresh = dialog.values.contains_key("table");
    let available = cfg!(target_os = "windows");
    if !available {
        ui.label(language.tr("ODBC connections require the Windows desktop app."));
    }
    ui.add_enabled_ui(!busy && available, |ui| {
        let mut mode = dialog.values.get("mode").and_then(Json::as_str).unwrap_or("dsn").to_string();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut mode, "dsn".into(), language.tr("Data source (DSN)"));
            ui.selectable_value(&mut mode, "connectionString".into(), language.tr("Connection string"));
        });
        dialog.values.insert("mode".into(), json!(mode));
        if mode == "dsn" {
            text(ui, dialog, "dsn", "Data source (DSN)", false, false);
            ui.horizontal(|ui| {
                if ui.button(language.tr("Find data sources")).clicked() {
                    match app.run("data.odbcSources", json!({})) {
                        Ok(result) => {
                            app.odbc.sources =
                                result["dataSources"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect();
                            if app.odbc.sources.is_empty() {
                                app.odbc.message = "No data sources found. Configure a DSN or use a connection string.".into();
                            }
                        }
                        Err(error) => app.odbc.error = Some(crate::clean_error(&error)),
                    }
                }
                if !app.odbc.sources.is_empty() {
                    egui::ComboBox::from_id_salt("odbc-dsn-list").selected_text(language.tr("Choose a data source")).show_ui(ui, |ui| {
                        for dsn in &app.odbc.sources {
                            if ui.selectable_label(false, dsn).clicked() {
                                dialog.values.insert("dsn".into(), json!(dsn));
                            }
                        }
                    });
                }
            });
        } else {
            text(ui, dialog, "connectionString", "Connection string", true, false);
        }
        text(ui, dialog, "user", "User name", false, false);
        text(ui, dialog, "password", "Password", true, false);
        ui.add_enabled_ui(!refresh, |ui| text(ui, dialog, "sql", "SQL query", false, true));
        ui.horizontal(|ui| {
            ui.label(language.tr("Maximum rows"));
            let mut rows = dialog.values.get("maxRows").and_then(Json::as_u64).unwrap_or(100000);
            ui.add_enabled(!refresh, egui::DragValue::new(&mut rows).range(1..=100000));
            dialog.values.insert("maxRows".into(), json!(rows));
        });
        if refresh {
            let mut replace = dialog.values.get("replace").and_then(Json::as_bool).unwrap_or(false);
            ui.checkbox(&mut replace, language.tr("Replace locally edited query data"));
            dialog.values.insert("replace".into(), json!(replace));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (label, action) in [
                ("Test Connection", "data.odbcTest"),
                ("Preview (100 rows)", "data.odbcPreview"),
                (if refresh { "Refresh Query" } else { "Load as Table" }, if refresh { "data.refreshQuery" } else { "data.fromOdbc" }),
            ] {
                if ui.button(language.tr(label)).clicked()
                    && let Err(error) = start(app, action, parameters(dialog))
                {
                    app.odbc.error = Some(crate::clean_error(&error));
                }
            }
        });
    });
    if busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(language.tr(if app.odbc.job.as_ref().is_some_and(|j| j.cancel) { "Cancelling query…" } else { "Query running…" }));
        });
    }
    if !app.odbc.message.is_empty() {
        ui.label(language.tr(&app.odbc.message));
    }
    if let Some(error) = &app.odbc.error {
        ui.colored_label(Tokens::get(ui.ctx()).danger, error);
    }
    if let Some(preview) = &app.odbc.preview {
        preview_grid(ui, preview);
    }
    ui.label(
        egui::RichText::new(
            language.tr("PostgreSQL, SQL Server, MySQL, MariaDB, Oracle and SQLite. Install a matching ODBC driver and use a read-only database account. Credentials stay in this session."),
        )
        .small(),
    );
    if ui.button(language.tr(if busy { "Cancel Query" } else { "Close" })).clicked() {
        if busy {
            cancel(app);
        } else {
            *open = false;
        }
    }
}

fn preview_grid(ui: &mut Ui, data: &Json) {
    egui::ScrollArea::both().max_height(220.0).show(ui, |ui| {
        egui::Grid::new("odbc-preview").striped(true).show(ui, |ui| {
            for column in data["columns"].as_array().into_iter().flatten() {
                ui.strong(column["name"].as_str().unwrap_or(""));
            }
            ui.end_row();
            for row in data["rows"].as_array().into_iter().flatten().take(100) {
                for value in row.as_array().into_iter().flatten() {
                    let value = serde_json::from_value::<gridcraft_engine::core::Value>(value.clone()).unwrap_or_default().display();
                    ui.label(value.chars().take(120).collect::<String>());
                }
                ui.end_row();
            }
        });
    });
    if data["truncated"] == true {
        ui.label(crate::i18n::current(ui.ctx()).tr("Preview was limited to 100 rows."));
    }
}

pub(crate) fn connections(app: &mut SheetApp, ui: &mut Ui) {
    let language = app.ui.language;
    let queries = app.run("data.queriesConnections", json!({})).unwrap_or(json!([]));
    let mut count = 0;
    for query in queries.as_array().into_iter().flatten().filter(|q| q["query"].is_object()) {
        count += 1;
        ui.horizontal(|ui| {
            ui.label(format!("{} — {} ({})", query["name"].as_str().unwrap_or(""), query["sheet"].as_str().unwrap_or(""), query["rows"]));
            if ui.add_enabled(app.odbc.job.is_none() && cfg!(target_os = "windows"), egui::Button::new(language.tr("Refresh Query"))).clicked() {
                app.open_dialog("odbc", json!({"table": query["name"]}));
            }
        });
    }
    if count == 0 {
        ui.label(language.tr("No ODBC queries in this workbook."));
    }
    if ui.add_enabled(app.odbc.job.is_none(), egui::Button::new(language.tr("Refresh All"))).clicked()
        && let Err(error) = start(app, "data.refreshAll", json!({}))
    {
        app.odbc.error = Some(crate::clean_error(&error));
    }
    if app.odbc.job.is_some() {
        ui.spinner();
        if ui.button(language.tr("Cancel Query")).clicked() {
            cancel(app);
        }
    }
    if let Some(error) = &app.odbc.error {
        ui.colored_label(Tokens::get(ui.ctx()).danger, error);
    }
}
