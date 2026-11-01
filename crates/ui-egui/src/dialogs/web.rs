use super::show::ok_cancel;
use super::*;

/// Starts the preview fetch off the UI thread (native); the wasm build runs it synchronously,
/// straight into the dialog being drawn.
fn start_preview_fetch(app: &mut SheetApp, d: &mut Dialog, url: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = d;
        app.start_fetch(&[url], ("data.webPreview", json!({"url": url})));
    }
    #[cfg(target_arch = "wasm32")]
    match app.run("data.webPreview", json!({"url": url})) {
        Ok(res) => {
            d.values.insert("tables".into(), res["tables"].clone());
        }
        Err(e) => app.message = Some(("From Web".into(), crate::clean_error(&e))),
    }
}

/// Applies a finished background preview fetch: the tables land in the dialog the caller is
/// drawing (`app.dialog` is taken while a dialog renders, so the result must go to `d`). A
/// still-running fetch keeps the dialog as it was and asks for repaints; the caller shows a
/// spinner. A result for a URL the user has since edited away is dropped.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn poll_preview_fetch(app: &mut SheetApp, d: &mut Dialog, url: &str, open: &mut bool) -> bool {
    if app.fetching() {
        return true;
    }
    // Only this dialog's own fetches; a foreign command (a ribbon refresh, say) is left to
    // its own poller, so it never lands in the dialog's table list.
    let Some((id, fetch_url)) = app.fetch.as_ref().map(|f| (f.command.0.clone(), f.command.1.get("url").and_then(Json::as_str).map(str::to_string))) else {
        return false;
    };
    if id != "data.webPreview" && id != "data.fromWeb" {
        return false;
    }
    // An address edited since invalidates the fetch: drop it without running its command, so
    // an old page never lands under the new URL.
    if fetch_url.as_deref().is_some_and(|u| u != url.trim()) {
        app.discard_fetch();
        return false;
    }
    if let Some((pages, r)) = app.poll_fetch() {
        match r {
            Ok(res) => {
                // The full rows are kept for Load (the preview's own list is truncated to the
                // pane); a `data.fromWeb` fetch (Load before Go) imports straight away.
                if let Some(page) = pages.get(url.trim()) {
                    d.values.insert("full_tables".into(), page["tables"].clone());
                }
                if res.get("sheet").is_some() {
                    // Load before Go landed: the import ran, the dialog closes with its toast.
                    app.toast = Some((format!("Imported {} rows into {}", res["rows"], res["sheet"]), crate::now_ms()));
                    *open = false;
                } else {
                    d.values.insert("tables".into(), res["tables"].clone());
                }
            }
            Err(e) => {
                // The fetch is gone (the workbook changed under it, say): a stale snapshot must
                // not survive either, or Load would import the old preview into the new workbook.
                d.values.remove("full_tables");
                app.message = Some(("From Web".into(), crate::clean_error(&e)));
                d.values.insert("tables".into(), Json::Null);
            }
        }
    }
    false
}

/// Reports one connection's Sync result: a toast on success, the per-connection error else.
fn report_sync(app: &mut SheetApp, name: &str, r: &Json) {
    let refreshed = r["refreshed"].as_u64().unwrap_or(0);
    let errors = r["errors"].as_array().map(Vec::len).unwrap_or(0);
    if refreshed > 0 && errors == 0 {
        app.toast = Some((format!("Synced {name}"), crate::now_ms()));
    } else if let Some(e) = r["errors"].as_array().and_then(|a| a.first()) {
        let msg = e["error"].as_str().unwrap_or_default();
        app.message = Some(("Queries & Connections".into(), format!("{name}: {msg}")));
    }
}

/// Reports a Refresh All result: a toast when everything synced, the joined errors else.
pub(crate) fn report_refresh_all(app: &mut SheetApp, r: &Json) {
    let refreshed = r["refreshed"].as_u64().unwrap_or(0);
    let errors = r["errors"].as_array().cloned().unwrap_or_default();
    if errors.is_empty() {
        app.toast = Some((format!("Synced {refreshed} connection(s)"), crate::now_ms()));
    } else {
        let msg = errors.iter().filter_map(|e| e["error"].as_str()).collect::<Vec<_>>().join("; ");
        app.message = Some(("Queries & Connections".into(), msg));
    }
}

/// Applies a finished background Sync/Refresh All and reports it like the synchronous path:
/// a single connection's Sync reports by name, a batch reports as Refresh All.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn poll_refresh_fetch(app: &mut SheetApp) -> bool {
    // Only this dialog's own refreshes: a preview or import fetch that is still running when
    // the dialog opens stays for the From Web dialog or the app-level fallback, never
    // reported here as a refresh.
    if app.fetching() || app.fetch.as_ref().is_none_or(|f| f.command.0 != "data.webRefresh") {
        return app.fetching();
    }
    let sync_name = app.fetch.as_ref().and_then(|f| f.command.1.get("name").and_then(Json::as_str)).map(str::to_string);
    if let Some((_, r)) = app.poll_fetch() {
        match r {
            Ok(res) => match &sync_name {
                // A single connection's Sync reports by its name, like the synchronous path.
                Some(name) => report_sync(app, name, &res),
                None => report_refresh_all(app, &res),
            },
            Err(e) => app.message = Some(("Queries & Connections".into(), crate::clean_error(&e))),
        }
    }
    false
}

/// Data ▸ Queries & Connections: the workbook's saved From Web connections, each with a
/// one-click Sync, its last-synced time, and the refresh errors and unavailable-source
/// messages in the same window (Excel's Queries & Connections pane).
pub(super) fn queries_connections(app: &mut SheetApp, ui: &mut egui::Ui, _d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    // A background Sync/Refresh All lands here as soon as it finishes.
    #[cfg(not(target_arch = "wasm32"))]
    {
        if poll_refresh_fetch(app) {
            ui.ctx().request_repaint();
            ui.spinner();
        }
    }
    let queries = app.run("data.queriesConnections", json!({})).ok().and_then(|r| r.as_array().cloned()).unwrap_or_default();
    if queries.is_empty() {
        ui.label("No saved connections. Import a table with Data ▸ From Web to create one.");
    } else {
        egui::ScrollArea::vertical().max_height(280.0).id_salt("qc_list").show(ui, |ui| {
            for q in &queries {
                let name = q["name"].as_str().unwrap_or_default();
                let sheet = q["sheet"].as_str().unwrap_or_default();
                egui::CollapsingHeader::new(name).default_open(true).show(ui, |ui| {
                    ui.label(format!("Sheet: {sheet}"));
                    if let Some(url) = q.get("url").and_then(Json::as_str) {
                        ui.label(format!("Source: {}", crate::clean_url_for_display(url)));
                    }
                    let synced = q.get("last_synced").and_then(Json::as_u64).filter(|t| *t > 0);
                    match synced {
                        Some(ms) => ui.weak(format!("Last synced: {}", crate::format_sync_time(ms))),
                        None => ui.weak("Last synced: never"),
                    };
                    let is_web = q.get("web").and_then(Json::as_bool).unwrap_or(false);
                    ui.horizontal(|ui| {
                        if is_web && ui.button("Sync").clicked() {
                            let urls: Vec<String> = q.get("url").and_then(Json::as_str).map(str::to_string).into_iter().collect();
                            #[cfg(not(target_arch = "wasm32"))]
                            {
                                let refs: Vec<&str> = urls.iter().map(String::as_str).collect();
                                app.start_fetch(&refs, ("data.webRefresh", json!({"name": name})));
                            }
                            #[cfg(target_arch = "wasm32")]
                            match app.run("data.webRefresh", json!({"name": name})) {
                                Ok(r) => report_sync(app, name, &r),
                                Err(e) => app.message = Some(("Queries & Connections".into(), crate::clean_error(&e))),
                            }
                        }
                        // Excel's connection property: "Refresh data when opening the file".
                        let roo = q.get("refresh_on_open").and_then(Json::as_bool).unwrap_or(false);
                        if ui.checkbox(&mut roo.clone(), "Refresh on open").changed() {
                            let _ = app.run("data.webQuerySet", json!({"name": name, "refreshOnOpen": roo}));
                        }
                        if ui.button("Go to table").clicked() {
                            let _ = app.run("sheet.activate", json!({"sheet": sheet}));
                            *open = false;
                        }
                    });
                });
                ui.separator();
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let any_web = queries.iter().any(|q| q.get("web").and_then(Json::as_bool).unwrap_or(false));
            if ui.button("Refresh All").clicked() && any_web {
                let urls: Vec<String> = queries
                    .iter()
                    .filter(|q| q.get("web").and_then(Json::as_bool).unwrap_or(false))
                    .filter_map(|q| q.get("url").and_then(Json::as_str).map(str::to_string))
                    .collect();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let refs: Vec<&str> = urls.iter().map(String::as_str).collect();
                    app.start_fetch(&refs, ("data.webRefresh", json!({})));
                }
                #[cfg(target_arch = "wasm32")]
                match app.run("data.webRefresh", json!({})) {
                    Ok(r) => report_refresh_all(app, &r),
                    Err(e) => app.message = Some(("Queries & Connections".into(), crate::clean_error(&e))),
                }
            }
        });
    }
    let mut load = true;
    ok_cancel(ui, confirm, &mut load);
    if *confirm || !load {
        *open = false;
    }
}

/// Data ▸ From Web: Excel's Navigator. Enter a URL, the page's detected tables are previewed
/// on the right, and Load imports the selected one into a new sheet.
pub(super) fn from_web(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let get = |k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let mut url = get("url");
    egui::Grid::new("fw").show(ui, |ui| {
        ui.label("URL:");
        ui.add(egui::TextEdit::singleline(&mut url).desired_width(360.0).hint_text("https://example.com/table.html"));
        // An edited address invalidates the preview: don't let Load pair the old page's
        // table names with the new URL.
        if d.values.get("preview_url").and_then(Json::as_str).is_some_and(|u| u != url.trim()) {
            d.values.remove("preview_url");
            d.values.remove("tables");
            d.values.remove("full_tables");
            d.values.remove("pick");
        }
        if ui.button("Go").clicked() && !url.trim().is_empty() {
            d.values.insert("url".into(), json!(url.trim()));
            d.values.insert("preview_url".into(), json!(url.trim()));
            d.values.insert("tables".into(), Json::Null);
            d.values.insert("pick".into(), json!(0));
            start_preview_fetch(app, d, url.trim());
        }
        ui.end_row();
    });
    // A background preview fetch lands here as soon as it finishes.
    #[cfg(not(target_arch = "wasm32"))]
    {
        if poll_preview_fetch(app, d, &url, open) {
            ui.ctx().request_repaint();
            ui.spinner();
        }
    }
    let tables: Vec<Json> = d.values.get("tables").and_then(Json::as_array).cloned().unwrap_or_default();
    if !tables.is_empty() {
        ui.separator();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.strong("Suggested Tables");
                let mut pick = d.values.get("pick").and_then(Json::as_u64).unwrap_or(0) as usize;
                for (i, t) in tables.iter().enumerate() {
                    let name = t["name"].as_str().unwrap_or("Table");
                    let rows = t["rows"].as_u64().unwrap_or(0);
                    if ui.selectable_label(i == pick, format!("{name}  ({rows} rows)")).clicked() {
                        pick = i;
                        d.values.insert("pick".into(), json!(i));
                    }
                }
            });
            ui.separator();
            let pick = d.values.get("pick").and_then(Json::as_u64).unwrap_or(0) as usize;
            if let Some(t) = tables.get(pick) {
                ui.vertical(|ui| {
                    ui.strong("Preview");
                    egui::ScrollArea::vertical().max_height(200.0).id_salt("fw_preview").show(ui, |ui| {
                        egui::Grid::new("fw_preview_grid").striped(true).show(ui, |ui| {
                            if let Some(rows) = t["preview"].as_array() {
                                for row in rows {
                                    for cell in row.as_array().into_iter().flatten() {
                                        ui.label(cell.as_str().unwrap_or_default());
                                    }
                                    ui.end_row();
                                }
                            }
                        });
                    });
                });
            }
        });
    } else if d.values.get("tables").is_some_and(|t| t.is_null()) {
        ui.label("No tables were found on this page.");
    } else {
        ui.label("Enter an address and choose Go to detect the page's tables.");
    }
    d.values.insert("url".into(), json!(url.clone()));
    let mut load = true;
    ok_cancel(ui, confirm, &mut load);
    if *confirm && url.trim().is_empty() {
        // OK with no address: stay open and say so, instead of closing with nothing.
        app.message = Some(("From Web".into(), "Enter an https:// address first.".into()));
        *confirm = false;
    }
    if *confirm && !url.trim().is_empty() && d.values.get("full_tables").is_none() {
        // Load without a downloaded page (no Go yet, or the fetch still running): fetch in the
        // background and import when it lands, instead of blocking the interface on the fetch.
        #[cfg(not(target_arch = "wasm32"))]
        {
            app.start_fetch(&[url.trim()], ("data.fromWeb", json!({"url": url.trim()})));
            *confirm = false;
        }
    }
    if *confirm && !url.trim().is_empty() {
        let pick = d.values.get("pick").and_then(Json::as_u64).unwrap_or(0) as usize;
        let table = tables.get(pick).and_then(|t| t["name"].as_str()).map(String::from);
        let mut params = match table {
            Some(t) => {
                // A page can hold duplicate table names: which one was picked is which one to
                // import, and the connection saves it so refreshes keep importing the same.
                let occurrence = tables.iter().take(pick).filter(|o| o["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&t))).count();
                json!({"url": url.trim(), "table": t, "occurrence": occurrence})
            }
            None => json!({"url": url.trim()}),
        };
        // The page is already downloaded whenever the preview fetched it — also when it found
        // no tables: hand the full tables over so Load never re-fetches on the UI thread.
        if let Some(full) = d.values.get("full_tables") {
            if let Some(o) = params.as_object_mut() {
                o.entry("prefetched").or_insert_with(|| full.clone());
            }
        }
        match app.run("data.fromWeb", params) {
            Ok(r) => app.toast = Some((format!("Imported {} rows into {}", r["rows"], r["sheet"]), crate::now_ms())),
            Err(e) => {
                app.message = Some(("From Web".into(), crate::clean_error(&e)));
                *confirm = false;
            }
        }
    }
    if !load {
        *confirm = true;
        *open = false;
    }
}
