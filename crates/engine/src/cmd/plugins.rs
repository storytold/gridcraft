//! Automate › Plug-ins: the `plugin.*` commands. List, install (a file, base64 data, a GitHub
//! repository or a catalog's package), update, remove and run plug-ins, and the catalogs they come
//! from. The runtime and the rules a run follows are in [`crate::plugin`] (RFC-0046).

use serde_json::{Value as Json, json};

use super::{CommandSpec, always, bad, bool_param, has_doc, str_param};
use crate::plugin;
use crate::{Result, Session};

/// Managing plug-ins changes no workbook, so these are neither journaled nor undoable.
fn manage(id: &'static str, label: &'static str, params: &'static str, run: super::Run) -> CommandSpec {
    CommandSpec { id, label, menu: &[], shortcut: None, shortcut_mac: None, params, enabled: always, run, journal: false, undoable: false }
}

fn list(s: &mut Session, _p: &Json) -> Result<Json> {
    let plugins: Vec<Json> = s.plugins.list().iter().map(plugin::InstalledPlugin::describe).collect();
    let last = s.plugins.last.as_ref().map(|(id, command, _)| json!({"id": id, "command": command}));
    Ok(json!({
        "plugins": plugins,
        "problems": s.plugins.problems(),
        "catalogs": s.prefs.plugin_catalogs,
        "stored": s.plugins.has_store(),
        "last": last,
    }))
}

fn install(s: &mut Session, p: &Json) -> Result<Json> {
    let cmd = "plugin.install";
    let request = plugin::install_request(p);
    let wanted = str_param(p, "sha256").map(str::to_string);
    let c = match wanted.as_deref().and_then(|h| plugin::take_held(s, h, &request)) {
        Some(c) => c,
        None => {
            let req = plugin::Request::parse(cmd, p)?;
            plugin::obtain(s, cmd, &req)?
        }
    };
    let (_, mut info) = plugin::review(s, cmd, &c)?;
    let sha256 = info["sha256"].as_str().unwrap_or_default().to_string();
    if let Some(w) = &wanted
        && !w.eq_ignore_ascii_case(&sha256)
    {
        return Err(bad(cmd, format!("the bundle's SHA-256 is {sha256}, not the {w} that was reviewed: review it again")));
    }
    if bool_param(p, "review").unwrap_or(false) {
        plugin::hold(s, &request, c);
        info["installed"] = json!(false);
        return Ok(info);
    }
    plugin::install(s, cmd, c)?;
    info["installed"] = json!(true);
    Ok(info)
}

fn update(s: &mut Session, p: &Json) -> Result<Json> {
    let cmd = "plugin.update";
    let only = str_param(p, "id");
    if let Some(id) = only
        && s.plugins.get(id).is_none()
    {
        return Err(bad(cmd, format!("no plug-in `{id}` is installed")));
    }
    let apply = bool_param(p, "apply").unwrap_or(false);
    let targets: Vec<plugin::InstalledPlugin> =
        s.plugins.list().iter().filter(|x| only.is_none_or(|id| x.manifest.id.eq_ignore_ascii_case(id))).cloned().collect();
    let (mut updates, mut updated, mut errors) = (vec![], vec![], vec![]);
    for old in targets {
        let id = old.manifest.id.clone();
        let (params, available) = match plugin::update_for_session(s, &old) {
            Ok(Some(found)) => found,
            Ok(None) => continue,
            Err(e) => {
                errors.push(json!({"id": id, "error": e}));
                continue;
            }
        };
        let mut entry = json!({"id": id, "installed": old.manifest.version, "available": available, "install": params});
        if apply {
            // An update that asks for other permissions waits for its confirmation (`plugin.install`).
            let done = plugin::Request::parse(cmd, &params).and_then(|req| plugin::obtain(s, cmd, &req)).and_then(|c| {
                let (_, info) = plugin::review(s, cmd, &c)?;
                if info["permissionsChanged"].as_bool().unwrap_or(true) {
                    return Ok(false);
                }
                plugin::install(s, cmd, c).map(|_| true)
            });
            match done {
                Ok(true) => updated.push(json!(id)),
                Ok(false) => entry["permissionsChanged"] = json!(true),
                Err(e) => errors.push(json!({"id": id, "error": e.to_string()})),
            }
        }
        updates.push(entry);
    }
    Ok(json!({"updates": updates, "updated": updated, "errors": errors}))
}

fn remove(s: &mut Session, p: &Json) -> Result<Json> {
    let cmd = "plugin.remove";
    let id = str_param(p, "id").ok_or_else(|| bad(cmd, "`id` required (see plugin.list)"))?.to_string();
    if !plugin::remove(s, cmd, &id)? {
        return Err(bad(cmd, format!("no plug-in `{id}` is installed")));
    }
    Ok(json!({"removed": id}))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let id = str_param(p, "id").ok_or_else(|| bad("plugin.run", "`id` required (see plugin.list)"))?.to_string();
    let command = str_param(p, "command").map(str::to_string);
    let fuel = p.get("fuel").and_then(Json::as_f64).filter(|f| f.is_finite() && *f >= 0.0).map(|f| f as u64);
    plugin::run(s, &id, command.as_deref(), p.get("params").unwrap_or(&Json::Null), fuel)
}

fn catalog_list(s: &mut Session, p: &Json) -> Result<Json> {
    let urls = match str_param(p, "url") {
        Some(url) => vec![url.to_string()],
        None => s.prefs.plugin_catalogs.clone(),
    };
    let (mut packages, mut warnings) = (vec![], vec![]);
    for url in &urls {
        match plugin::catalog_packages(s, url) {
            Ok((found, w)) => {
                packages.extend(found);
                warnings.extend(w.into_iter().map(|w| format!("{url}: {w}")));
            }
            Err(e) => warnings.push(format!("{url}: {e}")),
        }
    }
    Ok(json!({"catalogs": urls, "packages": packages, "warnings": warnings}))
}

fn catalog_add(s: &mut Session, p: &Json) -> Result<Json> {
    let cmd = "plugin.catalog.add";
    let url = str_param(p, "url").map(str::trim).filter(|u| !u.is_empty()).ok_or_else(|| bad(cmd, "`url` required"))?.to_string();
    let list = &mut s.prefs.plugin_catalogs;
    if bool_param(p, "remove").unwrap_or(false) {
        list.retain(|u| *u != url);
    } else if !list.contains(&url) {
        plugin::check_catalog_url(&url).map_err(|e| bad(cmd, e))?;
        if list.len() >= plugin::MAX_CATALOGS {
            return Err(bad(cmd, format!("at most {} catalogs", plugin::MAX_CATALOGS)));
        }
        list.push(url);
    }
    Ok(json!({"catalogs": list}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        manage(
            "plugin.list",
            "Plug-ins",
            "{} → {plugins: [{id, name, version, kind, permissions, commands, source, sha256, stored}], problems, catalogs, stored (kept in the Plug-ins folder; else this session only), last}",
            list,
        ),
        manage(
            "plugin.install",
            "Install Plug-in",
            "{path, review?, sha256?} | {data: base64 .craftplugin, name?, review?, sha256?} | {repo: owner/repo or owner/repo@tag (its release's .craftplugin, checked against SHA256SUMS), review?, sha256?} | {catalog: index.json URL, id, version? (default: the newest for GridCraft), review?, sha256?} — review: check it and describe it without installing (the confirmation); sha256: install only that bundle → {id, name, version, kind, permissions, commands, source, sha256, inCatalog, replaces, permissionsChanged, stored, installed}",
            install,
        ),
        manage(
            "plugin.update",
            "Update Plug-ins",
            "{id?, apply?: bool (install the updates that ask for no other permissions)} — checks each plug-in's catalog or repository → {updates: [{id, installed, available, install (plugin.install params), permissionsChanged?}], updated, errors}",
            update,
        ),
        manage("plugin.remove", "Remove Plug-in", "{id} — removes it and its settings", remove),
        // Journaled (replaying the journal runs the plug-in again; the commands it ran aren't
        // journaled) and undoable: the run is one undo step named after the plug-in.
        CommandSpec {
            id: "plugin.run",
            label: "Run Plug-in",
            menu: &[],
            shortcut: None,
            shortcut_mac: None,
            params: "{id, command? (default: the first), params? (the command's parameters), fuel? (a lower instruction budget)} — one undo step on the active workbook, rolled back when it fails → {plugin, command, output, notes, logs, calls, fuel, panel}",
            enabled: has_doc,
            run,
            journal: true,
            undoable: true,
        },
        manage(
            "plugin.catalog.list",
            "Plug-in Catalogs",
            "{url?} — the packages GridCraft can run in that catalog, or in every catalog added → {catalogs, packages: [{catalog, id, name, version, author, description, license, repo, kind, sha256, installed}], warnings}",
            catalog_list,
        ),
        manage(
            "plugin.catalog.add",
            "Add Plug-in Catalog",
            "{url: https:// on GitHub or file:// index.json, remove?: bool} → {catalogs}",
            catalog_add,
        ),
    ]
}
