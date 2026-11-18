//! Automate › Plug-ins: the ribbon's Plug-ins button (its menu lists the installed plug-ins'
//! commands, then Manage Plug-ins…), and three dialogs: Plug-ins (the installed plug-ins,
//! installing from a file, a GitHub repository or a catalog, updates), the install confirmation
//! (permissions, kind, source, SHA-256, and whether a catalog lists it) and a plug-in command's
//! parameters, generated from its manifest. Everything goes through the engine's `plugin.*`
//! commands (`gridcraft_engine::plugin`, RFC-0046). Downloads run on a worker thread, so the
//! window never waits for GitHub.

use gridcraft_engine::plugin::craft_plugin::{Access, Param, ParamKind};
use serde_json::{Map, Value as Json, json};

use crate::SheetApp;
use crate::dialogs::Dialog;
use crate::icons::Icon;
use crate::theme::Tokens;

/// The Plug-ins dialog (Manage Plug-ins).
pub const MANAGE: &str = "plugins";
/// The install confirmation.
pub const CONFIRM: &str = "pluginInstall";
/// A plug-in command's parameters.
pub const PARAMS: &str = "pluginParams";

/// What a download on the worker thread brings back.
#[cfg(not(target_arch = "wasm32"))]
enum Done {
    /// A bundle for `request` (the `plugin.install` parameters that named it), or why not.
    Obtained(Json, Result<gridcraft_engine::plugin::Candidate, String>),
    /// `plugin.catalog.list`'s answer, without `installed`.
    Catalogs(Json),
    /// `plugin.update`'s answer, without installing.
    Updates(Json),
}

/// The Plug-ins dialog's download in progress, if any.
#[derive(Default)]
pub struct PluginsUi {
    #[cfg(not(target_arch = "wasm32"))]
    job: Option<std::sync::mpsc::Receiver<Done>>,
}

impl PluginsUi {
    fn busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.job.is_some();
        #[cfg(target_arch = "wasm32")]
        false
    }
}

/// HTTPS for catalogs and GitHub releases, over the client GridCraft checks for updates with.
#[cfg(not(target_arch = "wasm32"))]
struct UpdateHttp(craft_update::Http);

#[cfg(not(target_arch = "wasm32"))]
impl gridcraft_engine::plugin::craft_plugin::Fetch for UpdateHttp {
    fn get(&mut self, url: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
        use craft_update::http::Fetched;
        if !url.starts_with("https://api.github.com/") {
            return self.0.get_bytes(url, max);
        }
        match self.0.get_api(url, None)? {
            Fetched::Body { text, .. } if text.len() > max => Err(format!("GitHub's answer is larger than {max} bytes")),
            Fetched::Body { text, .. } => Ok(Some(text.into_bytes())),
            Fetched::NotFound => Ok(None),
            Fetched::RateLimited { .. } => Err("GitHub's API limit for this hour is used up: try again later".into()),
            Fetched::NotModified => Err("GitHub answered 304 Not Modified to a request without a cache".into()),
        }
    }
}

/// An HTTPS client for plug-in downloads (`Session::plugins.fetch`, and each worker's own), when
/// one can be made. The web build has none: it installs from files only.
pub fn http_fetch() -> Option<Box<dyn gridcraft_engine::plugin::craft_plugin::Fetch + Send>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let agent = format!("GridCraft/{}", env!("CARGO_PKG_VERSION"));
        craft_update::Http::new(&agent).map_err(|e| log::warn!("plug-in downloads: {e}")).ok().map(|h| Box::new(UpdateHttp(h)) as _)
    }
    #[cfg(target_arch = "wasm32")]
    None
}

// ---------- the ribbon ----------

/// Automate › Plug-ins: the button opens the Plug-ins dialog; its menu runs a plug-in command.
pub(crate) fn ribbon_button(app: &mut SheetApp, ui: &mut egui::Ui) {
    let (main, arrow) = crate::widgets::big_split_button(ui, Icon::Script, "Plug-ins", "Run and manage WebAssembly plug-ins");
    if main.clicked() {
        app.open_dialog(MANAGE, json!({}));
    }
    egui::Popup::menu(&arrow).show(|ui| {
        let t = Tokens::get(ui.ctx());
        let commands: Vec<(String, String, String)> = app
            .session
            .plugins
            .list()
            .iter()
            .flat_map(|p| {
                let m = &p.manifest;
                m.commands.iter().map(move |c| {
                    // Plug-in names and command labels are the plug-in's own, not translated.
                    let label = if m.commands.len() == 1 { m.name.clone() } else { format!("{}: {}", m.name, c.label) };
                    (label, m.id.clone(), c.id.clone())
                })
            })
            .take(100)
            .collect();
        if commands.is_empty() {
            ui.label(egui::RichText::new(app.ui.language.tr("No plug-ins installed.")).italics().color(t.text_dim));
        }
        for (label, id, command) in commands {
            if ui.add(egui::Button::new(label).frame(false).min_size(egui::vec2(220.0, 22.0))).clicked() {
                run_from_menu(app, &id, &command);
            }
        }
        ui.separator();
        if ui.add(egui::Button::new(app.ui.language.tr("Manage Plug-ins…")).frame(false).min_size(egui::vec2(220.0, 22.0))).clicked() {
            app.open_dialog(MANAGE, json!({}));
        }
    });
}

/// The Plug-ins dialog, as `ui.dialog {name: "plugins"}` and the ribbon open it.
pub(crate) fn manage_dialog() -> Dialog {
    Dialog::custom(MANAGE, "Plug-ins", json!({"repo": "", "path": "", "catalogUrl": ""}))
}

/// Run a plug-in command, after its parameter dialog when it declares parameters.
pub(crate) fn run_from_menu(app: &mut SheetApp, id: &str, command: &str) {
    let Some(plugin) = app.session.plugins.get(id) else { return };
    let m = &plugin.manifest;
    let Some(c) = m.command(Some(command)) else { return };
    if c.params.is_empty() {
        let params = json!({"id": m.id, "command": c.id});
        run_plugin(app, params);
        return;
    }
    let values: Map<String, Json> = c.params.iter().filter(|p| p.is_field()).map(|p| (p.name.clone(), p.default_value())).collect();
    let title = format!("{}: {}", m.name, c.label.trim_end_matches('…'));
    let mut d = Dialog::custom(PARAMS, &title, json!({"id": m.id, "command": c.id, "params": values}));
    d.command = Some("plugin.run");
    d.result = Some(json!(c.params));
    app.dialog = Some(d);
}

/// `plugin.run`; its notes (or that it is done) in a toast, a failure in a message.
fn run_plugin(app: &mut SheetApp, params: Json) {
    match app.run("plugin.run", params) {
        Ok(r) => after_run(app, &r),
        Err(e) => app.message = Some((app.ui.language.tr("Plug-ins").into(), crate::clean_error(&e))),
    }
}

/// What a plug-in run said: its notes, or that it is done, in a toast.
pub(crate) fn after_run(app: &mut SheetApp, r: &Json) {
    let name = r["plugin"].as_str().and_then(|id| app.session.plugins.get(id)).map(|p| p.manifest.name.clone()).unwrap_or_default();
    let notes: Vec<&str> = r["notes"].as_array().map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
    let mut line = if notes.is_empty() { format!("{name}: {}", app.ui.language.tr("Done")) } else { notes.join(" · ") };
    if let Some(panel) = r["panel"].as_str() {
        line = format!("{line} ({panel})");
    }
    app.toast = Some((line, crate::now_ms()));
}

/// What OK did in one of the dialogs (`show` calls it after the dialog's command succeeded).
pub(crate) fn after_ok(app: &mut SheetApp, d: &Dialog, r: &Json) {
    match d.name.as_str() {
        PARAMS => after_run(app, r),
        CONFIRM => {
            let text = app.ui.language.tr("Installed {name}.").replace("{name}", r["name"].as_str().unwrap_or_default());
            app.toast = Some((text, crate::now_ms()));
        }
        _ => {}
    }
}

// ---------- reviewing ----------

/// Show the confirmation for the bundle `request` names, held for the install that follows:
/// checks it with `plugin.install {…, review: true}` (passing `sha256` of a bundle a worker
/// already downloaded, so it isn't downloaded again).
fn confirm_for(app: &mut SheetApp, d: &mut Dialog, request: Json, sha256: Option<String>) {
    let mut asked = request.clone();
    asked["review"] = json!(true);
    if let Some(h) = &sha256 {
        asked["sha256"] = json!(h);
    }
    match app.run("plugin.install", asked) {
        Ok(info) => {
            let mut values = request;
            values["sha256"] = info["sha256"].clone();
            let mut c = Dialog::custom(CONFIRM, "Install Plug-in", values);
            c.command = Some("plugin.install");
            c.result = Some(info);
            // Opened from the Plug-ins dialog's body: it waits behind the confirmation.
            app.dialog = Some(c);
        }
        Err(e) => status(d, crate::clean_error(&e)),
    }
}

/// Get the bundle `request` names and show its confirmation: on a worker thread when it comes
/// over the network, else at once.
fn review(app: &mut SheetApp, d: &mut Dialog, request: Json) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(req) = gridcraft_engine::plugin::Request::parse("plugin.install", &request)
        && req.needs_network()
    {
        let catalogs = app.session.prefs.plugin_catalogs.clone();
        let label = app.ui.language.tr("Downloading…").to_string();
        spawn(app, d, label, move |f| Done::Obtained(request, gridcraft_engine::plugin::obtain_with(&req, f, &catalogs)));
        return;
    }
    confirm_for(app, d, request, None);
}

/// Run `work` on a worker thread with its own HTTPS client; its result arrives in [`poll`].
#[cfg(not(target_arch = "wasm32"))]
fn spawn(
    app: &mut SheetApp,
    d: &mut Dialog,
    label: String,
    work: impl FnOnce(&mut dyn gridcraft_engine::plugin::craft_plugin::Fetch) -> Done + Send + 'static,
) {
    let Some(mut f) = http_fetch() else {
        status(d, "No HTTPS client could be made: install the plug-in from a file.".into());
        return;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let started = std::thread::Builder::new().name("plug-in download".into()).spawn(move || {
        let _ = tx.send(work(f.as_mut()));
    });
    match started {
        Ok(_) => {
            app.plugins.job = Some(rx);
            status(d, label);
        }
        Err(e) => status(d, e.to_string()),
    }
}

/// Take the worker's result, if it has arrived, into the dialog `d`.
fn poll(app: &mut SheetApp, ui: &egui::Ui, d: &mut Dialog) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(rx) = &app.plugins.job else { return };
        let done = match rx.try_recv() {
            Ok(done) => done,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                app.plugins.job = None;
                status(d, "The download stopped.".into());
                return;
            }
        };
        app.plugins.job = None;
        status(d, String::new());
        match done {
            Done::Obtained(request, Ok(c)) => {
                let sha256 = gridcraft_engine::plugin::hold(&mut app.session, &request, c);
                confirm_for(app, d, request, Some(sha256));
            }
            Done::Obtained(_, Err(e)) => status(d, e),
            Done::Catalogs(r) => catalogs_arrived(app, d, &r),
            Done::Updates(r) => updates_arrived(app, d, &r),
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ui, d);
}

fn status(d: &mut Dialog, text: String) {
    d.values.insert("status".into(), json!(text));
}

fn catalogs_arrived(app: &SheetApp, d: &mut Dialog, r: &Json) {
    let mut packages = r["packages"].as_array().cloned().unwrap_or_default();
    for pkg in &mut packages {
        let installed = pkg["id"].as_str().and_then(|id| app.session.plugins.get(id)).map(|p| p.manifest.version.clone());
        pkg["installed"] = json!(installed);
    }
    d.values.insert("packages".into(), Json::Array(packages));
    let warnings: Vec<&str> = r["warnings"].as_array().map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
    status(d, warnings.join("\n"));
}

fn updates_arrived(app: &SheetApp, d: &mut Dialog, r: &Json) {
    let none = r["updates"].as_array().is_none_or(Vec::is_empty);
    d.values.insert("updates".into(), r["updates"].clone());
    let errors: Vec<String> = r["errors"]
        .as_array()
        .map(|a| a.iter().map(|e| format!("{}: {}", e["id"].as_str().unwrap_or_default(), e["error"].as_str().unwrap_or_default())).collect())
        .unwrap_or_default();
    let text = if !errors.is_empty() {
        errors.join("\n")
    } else if none {
        app.ui.language.tr("Every plug-in is up to date.").to_string()
    } else {
        String::new()
    };
    status(d, text);
}

/// Browse the catalogs added: on a worker thread natively.
fn browse_catalogs(app: &mut SheetApp, d: &mut Dialog) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.session.prefs.plugin_catalogs.iter().any(|u| !u.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("file://"))) {
        let urls = app.session.prefs.plugin_catalogs.clone();
        let label = app.ui.language.tr("Reading the catalogs…").to_string();
        spawn(app, d, label, move |f| {
            let (mut packages, mut warnings) = (vec![], vec![]);
            for url in &urls {
                match gridcraft_engine::plugin::catalog_packages_with(f, url) {
                    Ok((found, w)) => {
                        packages.extend(found);
                        warnings.extend(w.into_iter().map(|w| format!("{url}: {w}")));
                    }
                    Err(e) => warnings.push(format!("{url}: {e}")),
                }
            }
            Done::Catalogs(json!({"packages": packages, "warnings": warnings}))
        });
        return;
    }
    match app.run("plugin.catalog.list", json!({})) {
        Ok(r) => catalogs_arrived(app, d, &r),
        Err(e) => status(d, crate::clean_error(&e)),
    }
}

/// Check every plug-in's catalog or repository for a newer release: on a worker thread natively.
fn check_updates(app: &mut SheetApp, d: &mut Dialog) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let plugins = app.session.plugins.list().to_vec();
        let label = app.ui.language.tr("Checking for updates…").to_string();
        spawn(app, d, label, move |f| Done::Updates(gridcraft_engine::plugin::check_updates(f, &plugins)));
    }
    #[cfg(target_arch = "wasm32")]
    match app.run("plugin.update", json!({})) {
        Ok(r) => updates_arrived(app, d, &r),
        Err(e) => status(d, crate::clean_error(&e)),
    }
}

// ---------- the dialogs ----------

/// The body of whichever of the three dialogs `d` is. `confirm` presses OK (the dialog's
/// command: `plugin.install` or `plugin.run`), `open` false closes it.
pub(crate) fn body(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    match d.name.as_str() {
        CONFIRM => confirm_body(app, ui, d),
        PARAMS => params_body(ui, d),
        _ => manage_body(app, ui, d),
    }
    if let Some(st) = d.values.get("status").and_then(Json::as_str).filter(|s| !s.is_empty()) {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(st).color(Tokens::get(ui.ctx()).text_dim));
    }
    buttons(app, ui, d, confirm, open);
}

fn buttons(app: &SheetApp, ui: &mut egui::Ui, d: &Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = app.ui.language;
    let tr = |s: &'static str| -> &'static str { lang.tr(s) };
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let t = Tokens::get(ui.ctx());
            let ok = match d.name.as_str() {
                CONFIRM => Some(tr("Install")),
                PARAMS => Some(tr("Run")),
                _ => None,
            };
            if let Some(ok) = ok
                && ui.add(egui::Button::new(egui::RichText::new(format!("   {ok}   ")).color(t.on_accent)).fill(t.accent)).clicked()
            {
                *confirm = true;
            }
            let close = if ok.is_some() { tr("Cancel") } else { tr("Close") };
            if ui.button(format!(" {close} ")).clicked() {
                *open = false;
            }
        })
    });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(text).strong());
}

/// A text box bound to `d.values[key]`.
fn text_field(ui: &mut egui::Ui, d: &mut Dialog, key: &str, width: f32, hint: &str) {
    let mut s = d.values.get(key).and_then(Json::as_str).unwrap_or_default().to_string();
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(width).hint_text(hint)).changed() {
        d.values.insert(key.into(), json!(s));
    }
}

fn text_of(d: &Dialog, key: &str) -> String {
    d.values.get(key).and_then(Json::as_str).unwrap_or_default().trim().to_string()
}

fn manage_body(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    poll(app, ui, d);
    let lang = app.ui.language;
    let tr = |s: &'static str| -> &'static str { lang.tr(s) };
    let dim = Tokens::get(ui.ctx()).text_dim;
    let busy = app.plugins.busy();
    heading(ui, tr("Installed"));
    let plugins = app.session.plugins.list().to_vec();
    if plugins.is_empty() {
        ui.label(egui::RichText::new(tr("No plug-ins installed.")).color(dim));
    }
    egui::ScrollArea::vertical().id_salt("plugins_installed").max_height(220.0).show(ui, |ui| {
        for p in &plugins {
            let m = &p.manifest;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{} {}", m.name, m.version)).strong());
                let access = if m.permissions.access == Access::Write { tr("edits workbooks") } else { tr("reads workbooks") };
                ui.label(egui::RichText::new(access).color(dim));
                if !p.stored {
                    ui.label(egui::RichText::new(tr("this session only")).color(dim));
                }
            });
            ui.horizontal_wrapped(|ui| {
                for c in &m.commands {
                    if ui.add_enabled(app.session.active().is_some(), egui::Button::new(c.label.as_str())).clicked() {
                        run_from_menu(app, &m.id, &c.id);
                    }
                }
                if ui.button(tr("Remove")).clicked() {
                    let r = app.run("plugin.remove", json!({"id": m.id}));
                    status(d, r.err().map(|e| crate::clean_error(&e)).unwrap_or_default());
                }
            });
        }
        for problem in app.session.plugins.problems() {
            ui.label(egui::RichText::new(problem).color(dim));
        }
    });

    heading(ui, tr("Install"));
    ui.horizontal(|ui| {
        ui.label(tr("File"));
        text_field(ui, d, "path", 220.0, "plug-in.craftplugin");
        if app.services.pick_plugin.is_some()
            && ui.button(tr("Browse…")).clicked()
            && let Some(path) = app.services.pick_plugin.as_ref().and_then(|pick| pick())
        {
            d.values.insert("path".into(), json!(path));
        }
        if ui.add_enabled(!busy && !text_of(d, "path").is_empty(), egui::Button::new(tr("Review…"))).clicked() {
            let path = text_of(d, "path");
            review(app, d, json!({"path": path}));
        }
    });
    ui.horizontal(|ui| {
        ui.label(tr("GitHub repository"));
        text_field(ui, d, "repo", 220.0, "owner/repo");
        if ui.add_enabled(!busy && !text_of(d, "repo").is_empty(), egui::Button::new(tr("Review…"))).clicked() {
            let repo = text_of(d, "repo");
            review(app, d, json!({"repo": repo}));
        }
    });

    heading(ui, tr("Catalogs"));
    for url in app.session.prefs.plugin_catalogs.clone() {
        ui.horizontal(|ui| {
            ui.label(url.as_str());
            if ui.small_button(tr("Remove")).clicked() {
                let _ = app.run("plugin.catalog.add", json!({"url": url, "remove": true}));
            }
        });
    }
    ui.horizontal(|ui| {
        text_field(ui, d, "catalogUrl", 300.0, "https://raw.githubusercontent.com/…/index.json");
        if ui.add_enabled(!text_of(d, "catalogUrl").is_empty(), egui::Button::new(tr("Add"))).clicked() {
            match app.run("plugin.catalog.add", json!({"url": text_of(d, "catalogUrl")})) {
                Ok(_) => {
                    d.values.insert("catalogUrl".into(), json!(""));
                }
                Err(e) => status(d, crate::clean_error(&e)),
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!busy && !app.session.prefs.plugin_catalogs.is_empty(), egui::Button::new(tr("Browse Catalogs"))).clicked() {
            browse_catalogs(app, d);
        }
        if ui.add_enabled(!busy && !plugins.is_empty(), egui::Button::new(tr("Check for Updates"))).clicked() {
            check_updates(app, d);
        }
    });
    let packages = d.values.get("packages").and_then(Json::as_array).cloned().unwrap_or_default();
    for pkg in packages.iter().take(200) {
        ui.horizontal(|ui| {
            let installed = pkg["installed"].as_str().map(|v| format!(" ({} {v})", tr("installed"))).unwrap_or_default();
            ui.label(format!("{} {}{installed}", pkg["name"].as_str().unwrap_or_default(), pkg["version"].as_str().unwrap_or_default()));
            if ui.add_enabled(!busy, egui::Button::new(tr("Install…")).small()).clicked() {
                review(app, d, json!({"catalog": pkg["catalog"], "id": pkg["id"]}));
            }
        });
        if let Some(text) = pkg["description"].as_str().filter(|t| !t.is_empty()) {
            ui.label(egui::RichText::new(text).color(dim));
        }
    }
    let updates = d.values.get("updates").and_then(Json::as_array).cloned().unwrap_or_default();
    for u in updates.iter().take(200) {
        ui.horizontal(|ui| {
            let s = |k: &str| u[k].as_str().unwrap_or_default().to_string();
            ui.label(format!("{} {} → {}", s("id"), s("installed"), s("available")));
            if ui.add_enabled(!busy, egui::Button::new(tr("Update…")).small()).clicked() {
                review(app, d, u["install"].clone());
            }
        });
    }
}

fn confirm_body(app: &SheetApp, ui: &mut egui::Ui, d: &Dialog) {
    let lang = app.ui.language;
    let tr = |s: &'static str| -> &'static str { lang.tr(s) };
    let info = d.result.clone().unwrap_or_default();
    let text = |k: &str| info[k].as_str().unwrap_or_default().to_string();
    ui.label(egui::RichText::new(format!("{} {}", text("name"), text("version"))).strong().size(14.0));
    for k in ["author", "description", "license", "repo"] {
        if !text(k).is_empty() {
            ui.label(text(k));
        }
    }
    egui::Grid::new("plugin_confirm").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label(tr("Kind"));
        ui.label(text("kind"));
        ui.end_row();
        ui.label(tr("Access"));
        let write = info["permissions"]["access"] == "write";
        ui.label(if write { tr("Reads and edits the open workbook") } else { tr("Reads the open workbook") });
        ui.end_row();
        ui.label(tr("Commands"));
        let allowed: Vec<&str> = info["permissions"]["commands"].as_array().map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
        ui.label(if allowed.is_empty() { tr("Any command its access allows").to_string() } else { allowed.join(", ") });
        ui.end_row();
        if info["permissions"]["saveFile"] == true {
            ui.label(tr("Files"));
            ui.label(tr("Asks to save files (GridCraft doesn't offer this yet)"));
            ui.end_row();
        }
        ui.label(tr("Source"));
        ui.label(source_text(&info["source"]));
        ui.end_row();
        ui.label("SHA-256");
        ui.add(egui::Label::new(egui::RichText::new(text("sha256")).monospace()).wrap());
        ui.end_row();
    });
    let dim = Tokens::get(ui.ctx()).text_dim;
    if info["inCatalog"] == false {
        ui.label(egui::RichText::new(tr("This plug-in isn't in any catalog you use: install it only if you trust its author.")).strong());
    }
    if let Some(old) = info["replaces"].as_str() {
        ui.label(egui::RichText::new(tr("Replaces version {version}.").replace("{version}", old)).color(dim));
    }
    if info["permissionsChanged"] == true {
        ui.label(egui::RichText::new(tr("This version asks for different permissions.")).strong());
    }
    if info["stored"] == false {
        ui.label(egui::RichText::new(tr("It is kept for this session only.")).color(dim));
    }
}

/// Where a plug-in comes from, in a line.
pub(crate) fn source_text(source: &Json) -> String {
    let t = |k: &str| source[k].as_str().unwrap_or_default();
    match t("type") {
        "github" => format!("github.com/{} {} ({})", t("repo"), t("tag"), t("asset")),
        "catalog" => format!("{} · {}", t("package"), t("catalog")),
        _ => t("path").to_string(),
    }
}

fn params_body(ui: &mut egui::Ui, d: &mut Dialog) {
    let params: Vec<Param> = d.result.clone().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut values = d.values.get("params").and_then(Json::as_object).cloned().unwrap_or_default();
    egui::Grid::new("plugin_params").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        for p in params.iter().filter(|p| p.is_field()) {
            ui.label(p.title());
            let v = values.entry(p.name.clone()).or_insert_with(|| p.default_value());
            field(ui, p, v);
            ui.end_row();
        }
    });
    d.values.insert("params".into(), Json::Object(values));
}

/// One parameter's control, editing `v`.
fn field(ui: &mut egui::Ui, p: &Param, v: &mut Json) {
    match p.kind {
        ParamKind::Number | ParamKind::Slider => {
            let mut n = v.as_f64().unwrap_or(0.0);
            let changed = match (p.kind, p.min, p.max) {
                (ParamKind::Slider, Some(lo), Some(hi)) if lo < hi => ui.add(egui::Slider::new(&mut n, lo..=hi)).changed(),
                _ => {
                    let mut drag = egui::DragValue::new(&mut n);
                    if let (Some(lo), Some(hi)) = (p.min, p.max)
                        && lo <= hi
                    {
                        drag = drag.range(lo..=hi);
                    }
                    ui.add(drag).changed()
                }
            };
            if changed {
                *v = json!(n);
            }
        }
        ParamKind::Boolean => {
            let mut b = v.as_bool().unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                *v = json!(b);
            }
        }
        ParamKind::Select | ParamKind::List => {
            let current = v.as_str().unwrap_or_default().to_string();
            craft_ui::ComboBox::from_id_salt(("plugin_param", &p.name)).selected_text(current.clone()).show_ui(ui, |ui| {
                for o in &p.options {
                    if ui.selectable_label(*o == current, o.as_str()).clicked() {
                        *v = json!(o);
                    }
                }
            });
        }
        // Text, and a colour as `#RRGGBB`.
        _ => {
            let mut s = v.as_str().unwrap_or_default().to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(240.0)).changed() {
                *v = json!(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use gridcraft_engine::plugin::craft_plugin::bundle::Bundle;
    use serde_json::json;

    use super::*;

    /// A plug-in whose `plain` command writes 7 to A1, and whose `asks` command takes a count.
    fn install(app: &mut SheetApp) {
        let wasm = wat::parse_str(
            r#"(module
              (import "craft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 0) "cell.set")
              (data (i32.const 32) "{\"cell\":\"A1\",\"input\":\"7\"}")
              (func (export "craft_run") (param i32) (result i32)
                (drop (call $call (i32.const 0) (i32.const 8) (i32.const 32) (i32.const 25))) (i32.const 0)))"#,
        )
        .unwrap();
        let manifest = json!({"id": "io.example.ui", "name": "UI Test", "version": "1.0.0", "main": "p.wasm",
            "apps": {"gridcraft": {"api": 6}}, "permissions": {"access": "write"},
            "commands": [{"id": "plain", "label": "Plain"},
                         {"id": "asks", "label": "Asks…", "params": [{"name": "count", "type": "number", "default": 2, "min": 1, "max": 9}]}]});
        let bytes = Bundle { manifest: manifest.to_string(), wasm, assets: vec![] }.encode();
        let data = gridcraft_engine::io::base64_encode(&bytes);
        // Review, then install what was reviewed, as the confirmation's Install does.
        let mut d = manage_dialog();
        confirm_for(app, &mut d, json!({"data": data}), None);
        let c = app.dialog.take().unwrap_or_else(|| panic!("{:?}", d.values.get("status")));
        assert_eq!(c.name, CONFIRM);
        assert_eq!(c.result.as_ref().unwrap()["permissions"]["access"], "write");
        let r = app.run(c.command.unwrap(), crate::dialogs::build_params(&c)).unwrap();
        assert_eq!(r["installed"], true);
    }

    fn a1(app: &SheetApp) -> gridcraft_engine::core::Value {
        app.session.doc().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::new(0, 0))
    }

    #[test]
    fn plugin_commands_run_from_the_menu_and_ask_for_parameters() {
        let mut app = SheetApp::new(gridcraft_engine::Session::new(), crate::Services::default());
        app.session.new_workbook();
        install(&mut app);
        // A command without parameters runs at once, as one undo step.
        run_from_menu(&mut app, "io.example.ui", "plain");
        assert_eq!(a1(&app), gridcraft_engine::core::Value::Number(7.0));
        assert!(app.toast.as_ref().is_some_and(|(t, _)| t.contains("UI Test")), "{:?}", app.toast);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(a1(&app), gridcraft_engine::core::Value::Empty);
        // One with parameters asks first; OK runs it with them.
        run_from_menu(&mut app, "io.example.ui", "asks");
        let d = app.dialog.take().unwrap();
        assert_eq!(d.name, PARAMS);
        assert_eq!(d.values["params"]["count"], 2.0);
        let r = app.run(d.command.unwrap(), crate::dialogs::build_params(&d)).unwrap();
        assert_eq!(r["command"], "asks");
    }

    /// One frame of the dialogs with `app.dialog` open; the text the frame drew.
    fn frame(app: &mut SheetApp) -> String {
        fn walk(shape: &egui::epaint::Shape, text: &mut String) {
            match shape {
                egui::epaint::Shape::Text(t) => {
                    text.push_str(t.galley.text());
                    text.push('\n');
                }
                egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk(s, text)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut text = String::new();
        // A window lays itself out unseen on its first pass.
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| crate::dialogs::show(app, ui.ctx()));
            out.textures_delta.clear();
            text.clear();
            out.shapes.iter().for_each(|s| walk(&s.shape, &mut text));
        }
        text
    }

    #[test]
    fn the_three_dialogs_draw_and_stay_open_until_a_choice() {
        let mut app = SheetApp::new(gridcraft_engine::Session::new(), crate::Services::default());
        app.session.new_workbook();
        install(&mut app);
        app.open_dialog(MANAGE, json!({}));
        let text = frame(&mut app);
        assert!(text.contains("UI Test 1.0.0") && text.contains("GitHub repository"), "{text}");
        assert_eq!(app.dialog.as_ref().map(|d| d.name.as_str()), Some(MANAGE));
        // A repository in no catalog you use says so before anything is installed.
        let info = json!({"name": "X", "version": "1.0.0", "kind": "command", "permissions": {"access": "read"},
            "source": {"type": "github", "repo": "example/x", "tag": "v1", "asset": "x.craftplugin"}, "sha256": "ab", "inCatalog": false});
        let mut c = Dialog::custom(CONFIRM, "Install Plug-in", json!({"repo": "example/x", "sha256": "ab"}));
        c.command = Some("plugin.install");
        c.result = Some(info);
        app.dialog = Some(c);
        let text = frame(&mut app);
        assert!(text.contains("isn't in any catalog you use") && text.contains("Reads the open workbook"), "{text}");
        assert_eq!(app.dialog.as_ref().map(|d| d.name.as_str()), Some(CONFIRM));
        run_from_menu(&mut app, "io.example.ui", "asks");
        let text = frame(&mut app);
        assert!(text.contains("count") && text.contains("Run"), "{text}");
        assert_eq!(app.dialog.as_ref().map(|d| d.name.as_str()), Some(PARAMS));
    }

    #[test]
    fn the_install_confirmation_says_where_it_comes_from() {
        let source = json!({"type": "github", "repo": "example/x", "tag": "v1", "asset": "x.craftplugin"});
        assert_eq!(source_text(&source), "github.com/example/x v1 (x.craftplugin)");
        assert_eq!(source_text(&json!({"type": "catalog", "package": "p", "catalog": "https://c"})), "p · https://c");
    }
}
