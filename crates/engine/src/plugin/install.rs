//! Installing plug-ins: from a `.craftplugin` file, the bundle itself (base64), a GitHub
//! repository's release or a catalog's package; reviewing a bundle before it is installed (the
//! install confirmation), updates, removing, and the catalogs.

use std::sync::Arc;

use craft_plugin::fetch::{self, Fetch};
use craft_plugin::{Manifest, Source, bundle, catalog, github};
use serde_json::{Map, Value, json};

use super::{APP, InstalledPlugin};
use crate::cmd::bad;
use crate::{Result, Session};

/// Where a bundle to install comes from (`plugin.install`'s parameters).
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// A `.craftplugin` file on this computer.
    Path(String),
    /// The bundle itself (base64 in the parameters), with a name to remember it by.
    Data { bytes: Vec<u8>, name: String },
    /// A GitHub repository's release: `owner/repo` or `owner/repo@tag`.
    Repo(String),
    /// A catalog's package (its newest release for GridCraft, or `version`).
    Catalog { url: String, id: String, version: Option<String> },
}

impl Request {
    /// The source `p` names: `path`, `data` (base64), `repo` or `catalog` + `id`.
    pub fn parse(cmd: &str, p: &Value) -> Result<Request> {
        let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
        if let Some(path) = text("path") {
            return Ok(Request::Path(path.into()));
        }
        if let Some(data) = text("data") {
            if data.len() > bundle::MAX_BUNDLE_BYTES / 3 * 4 + 4 {
                return Err(bad(cmd, format!("the bundle is larger than {} MB", bundle::MAX_BUNDLE_BYTES >> 20)));
            }
            let bytes = crate::io::base64_decode(data).ok_or_else(|| bad(cmd, "`data` isn't base64"))?;
            return Ok(Request::Data { bytes, name: text("name").unwrap_or("data").chars().take(200).collect() });
        }
        if let Some(spec) = text("repo") {
            return Ok(Request::Repo(spec.into()));
        }
        if let Some(url) = text("catalog") {
            let id = text("id").ok_or_else(|| bad(cmd, "`id` required with `catalog` (a package of the catalog)"))?;
            return Ok(Request::Catalog { url: url.into(), id: id.into(), version: text("version").map(str::to_string) });
        }
        Err(bad(cmd, "pass `path`, `data` (base64), `repo` (owner/repo or owner/repo@tag) or `catalog` with `id`"))
    }

    /// Whether getting the bundle goes over the network (a repository, or a catalog that isn't a
    /// local `file://` one).
    pub fn needs_network(&self) -> bool {
        match self {
            Request::Repo(_) => true,
            Request::Catalog { url, .. } => !is_file_url(url),
            Request::Path(_) | Request::Data { .. } => false,
        }
    }
}

fn is_file_url(url: &str) -> bool {
    url.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("file://"))
}

/// A bundle obtained for installing: its bytes, where it came from and, for a repository, whether
/// a catalog in use lists it.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub bytes: Vec<u8>,
    pub source: Source,
    /// `Some(true)` from a catalog; for a repository, whether a catalog in use lists it.
    pub in_catalog: Option<bool>,
}

/// A bundle checked and shown for review (`plugin.install {review: true}`), kept so the install
/// that follows doesn't download it again.
pub(crate) struct Pending {
    sha256: String,
    /// The `plugin.install` parameters that named it.
    request: Value,
    candidate: Candidate,
}

/// Read the bundle `req` names, downloading over `f` (catalogs in use: `catalogs`, to tell
/// whether a repository is listed in one). Needs no session, so a front end can run it on a
/// worker thread and hand the result to `plugin.install` through [`hold`].
pub fn obtain_with(req: &Request, f: &mut dyn Fetch, catalogs: &[String]) -> std::result::Result<Candidate, String> {
    match req {
        Request::Path(path) => {
            let bytes = read_bundle_file(path).map_err(|e| format!("{path}: {e}"))?;
            Ok(Candidate { bytes, source: Source::File { path: path.clone() }, in_catalog: None })
        }
        Request::Data { bytes, name } => Ok(Candidate { bytes: bytes.clone(), source: Source::File { path: name.clone() }, in_catalog: None }),
        Request::Repo(spec) => {
            let repo = github::parse_repo_spec(spec)?;
            let plan = fetch::resolve_repo(f, &repo)?;
            let bytes = fetch::download_repo(f, &plan)?;
            let source = Source::Github { repo: plan.repo.slug(), tag: plan.repo.tag.clone().unwrap_or_default(), asset: plan.asset.clone() };
            let in_catalog = catalogs.iter().any(|url| {
                fetch::fetch_catalog(f, url)
                    .is_ok_and(|(cat, _)| cat.packages.iter().any(|pkg| pkg.repo_ref().is_some_and(|r| r.same_repo(&repo.owner, &repo.name))))
            });
            Ok(Candidate { bytes, source, in_catalog: Some(in_catalog) })
        }
        Request::Catalog { url, id, version } => {
            let (cat, _) = fetch::fetch_catalog(f, url)?;
            let package = cat.package(id).ok_or_else(|| format!("{url} has no package `{id}`"))?;
            let release = match version {
                Some(v) => package.release(v).ok_or_else(|| format!("{id} has no version {v} in {url}"))?,
                None => package.latest_for(&APP).ok_or_else(|| format!("{id} has no release for GridCraft {}", APP.version))?,
            };
            release.target(&package.kind, &APP)?;
            let bytes = fetch::download_release(f, url, package, release)?;
            let (_, manifest) = bundle::open(&bytes)?;
            package.check_manifest(release, &manifest)?;
            Ok(Candidate { bytes, source: Source::Catalog { catalog: url.clone(), package: id.clone() }, in_catalog: Some(true) })
        }
    }
}

/// [`obtain_with`] from a command, over the session's client.
pub(crate) fn obtain(s: &mut Session, cmd: &str, req: &Request) -> Result<Candidate> {
    let catalogs = s.prefs.plugin_catalogs.clone();
    with_fetch(s, |f| obtain_with(req, f, &catalogs)).map_err(|e| bad(cmd, e))
}

/// Longest part of a URL an error repeats.
const SHOWN_URL_CHARS: usize = 120;

/// The fetcher of sessions without network access: local files and `file://` catalogs still read.
struct NoNetwork;

impl Fetch for NoNetwork {
    fn get(&mut self, url: &str, _max: usize) -> std::result::Result<Option<Vec<u8>>, String> {
        let shown: String = url.chars().take(SHOWN_URL_CHARS).collect();
        Err(format!("there is no network access here to read {shown}: install the plug-in from a file, or in the GridCraft app"))
    }
}

/// Run `f` with the session's HTTPS client (or [`NoNetwork`]).
fn with_fetch<T>(s: &mut Session, f: impl FnOnce(&mut dyn Fetch) -> T) -> T {
    match s.plugins.fetch.as_deref_mut() {
        Some(http) => f(http),
        None => f(&mut NoNetwork),
    }
}

/// The bundle file at `path`, refused past [`bundle::MAX_BUNDLE_BYTES`] before it is read whole,
/// and refused unless it is a regular file (opening a FIFO nobody writes to would block).
#[cfg(not(target_arch = "wasm32"))]
fn read_bundle_file(path: &str) -> std::result::Result<Vec<u8>, String> {
    use std::io::Read;
    let max = bundle::MAX_BUNDLE_BYTES as u64;
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("only regular files can be installed".into());
    }
    if meta.len() > max {
        return Err(format!("larger than {} MB", bundle::MAX_BUNDLE_BYTES >> 20));
    }
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    f.take(max + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err(format!("larger than {} MB", bundle::MAX_BUNDLE_BYTES >> 20));
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
fn read_bundle_file(_path: &str) -> std::result::Result<Vec<u8>, String> {
    Err("the browser has no file paths: pass the bundle as `data` (base64)".into())
}

/// What an install confirmation shows about `c`: the manifest's permissions, kind and commands,
/// the source, the SHA-256, whether a catalog lists it and what it replaces. An error when the
/// bundle isn't a plug-in GridCraft can run.
pub(crate) fn review(s: &Session, cmd: &str, c: &Candidate) -> Result<(Manifest, Value)> {
    let (_, manifest) = bundle::open(&c.bytes).map_err(|e| bad(cmd, e))?;
    manifest.target(&APP).map_err(|e| bad(cmd, format!("{} can't be installed: {e}", manifest.name)))?;
    c.source.check().map_err(|e| bad(cmd, e))?;
    let old = s.plugins.get(&manifest.id);
    let mut v = serde_json::to_value(&manifest).unwrap_or_else(|_| json!({"id": manifest.id}));
    v["kind"] = json!(manifest.kind);
    v["sha256"] = json!(bundle::sha256_hex(&c.bytes));
    v["source"] = serde_json::to_value(&c.source).unwrap_or(Value::Null);
    v["inCatalog"] = json!(c.in_catalog);
    v["replaces"] = json!(old.map(|o| &o.manifest.version));
    v["permissionsChanged"] = json!(old.is_some_and(|o| craft_plugin::permissions::changed(&o.manifest, &manifest)));
    v["stored"] = json!(s.plugins.has_store());
    Ok((manifest, v))
}

/// Install `c` (already reviewed): into the user's folder when the session has one, else for the
/// session. Replaces an installed plug-in with the same id; its settings stay.
pub(crate) fn install(s: &mut Session, cmd: &str, c: Candidate) -> Result<InstalledPlugin> {
    let (manifest, _) = review(s, cmd, &c)?;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(store) = &s.plugins.store {
        let (i, _) = store.install_for(&APP, &c.bytes, c.source).map_err(|e| bad(cmd, e))?;
        let plugin =
            InstalledPlugin { manifest: i.manifest, sha256: i.record.sha256, source: i.record.source, stored: true, wasm: Arc::new(i.bundle.wasm) };
        return Ok(s.plugins.keep(plugin));
    }
    let sha256 = bundle::sha256_hex(&c.bytes);
    let (b, _) = bundle::open(&c.bytes).map_err(|e| bad(cmd, e))?;
    Ok(s.plugins.keep(InstalledPlugin { manifest, sha256, source: c.source, stored: false, wasm: Arc::new(b.wasm) }))
}

/// Keep `c`, named by the `plugin.install` parameters `request` (without `review` and `sha256`),
/// for the install that follows its review (one at a time). Returns its SHA-256, which that
/// install passes. A front end that downloaded `c` itself ([`obtain_with`]) hands it over here.
pub fn hold(s: &mut Session, request: &Value, c: Candidate) -> String {
    let sha256 = bundle::sha256_hex(&c.bytes);
    s.plugins.pending = Some(Pending { sha256: sha256.clone(), request: install_request(request), candidate: c });
    sha256
}

/// `plugin.install`'s parameters as [`hold`] and the install after a review compare them.
pub fn install_request(p: &Value) -> Value {
    let mut request: Map<String, Value> = p.as_object().cloned().unwrap_or_default();
    request.remove("review");
    request.remove("sha256");
    Value::Object(request)
}

/// The bundle reviewed last, when it is `sha256` and `request` named it.
pub(crate) fn take_held(s: &mut Session, sha256: &str, request: &Value) -> Option<Candidate> {
    let held = s.plugins.pending.take_if(|h| h.sha256.eq_ignore_ascii_case(sha256) && h.request == *request)?;
    Some(held.candidate)
}

/// Remove plug-in `id` and its settings; whether it was installed.
pub(crate) fn remove(s: &mut Session, cmd: &str, id: &str) -> Result<bool> {
    let Some(p) = s.plugins.get(id).cloned() else { return Ok(false) };
    #[cfg(not(target_arch = "wasm32"))]
    if p.stored
        && let Some(store) = &s.plugins.store
    {
        store.remove(&p.manifest.id).map_err(|e| bad(cmd, e))?;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = cmd;
    s.plugins.forget(&p.manifest.id);
    s.prefs.plugin_settings.retain(|k, _| !k.eq_ignore_ascii_case(&p.manifest.id));
    Ok(true)
}

/// The catalog `url`'s packages GridCraft can run, with their newest release for it (and the
/// version installed here).
pub(crate) fn catalog_packages(s: &mut Session, url: &str) -> std::result::Result<(Vec<Value>, Vec<String>), String> {
    let (mut packages, warnings) = with_fetch(s, |f| catalog_packages_with(f, url))?;
    for pkg in &mut packages {
        let installed = pkg["id"].as_str().and_then(|id| s.plugins.get(id)).map(|p| p.manifest.version.clone());
        pkg["installed"] = json!(installed);
    }
    Ok((packages, warnings))
}

/// [`catalog_packages`] without a session (`installed` is left out), for a front end's worker
/// thread.
pub fn catalog_packages_with(f: &mut dyn Fetch, url: &str) -> std::result::Result<(Vec<Value>, Vec<String>), String> {
    let (cat, warnings) = fetch::fetch_catalog(f, url)?;
    let packages = cat
        .packages
        .iter()
        .filter_map(|pkg| {
            let release = pkg.latest_for(&APP)?;
            Some(json!({
                "catalog": url, "id": pkg.id, "name": pkg.name, "version": release.version, "author": pkg.author,
                "description": pkg.description, "license": pkg.license, "repo": pkg.repo, "kind": pkg.kind,
                "sha256": release.sha256,
            }))
        })
        .collect();
    Ok((packages, warnings))
}

/// Whether `url` is a catalog address GridCraft reads (`https://` on GitHub's hosts, or
/// `file://`), or why not.
pub(crate) fn check_catalog_url(url: &str) -> std::result::Result<(), String> {
    catalog::check_url(url, None).map(|_| ())
}

/// Where plug-in `p` has a newer release: the install parameters that get it and its version (a
/// tag, for a repository). `None` when it is up to date or came from a file.
pub(crate) fn update_for(f: &mut dyn Fetch, p: &InstalledPlugin) -> std::result::Result<Option<(Value, String)>, String> {
    match &p.source {
        Source::Catalog { catalog: url, package: id } => {
            let (cat, _) = fetch::fetch_catalog(f, url)?;
            let release = cat.package(id).and_then(|pkg| pkg.latest_for(&APP));
            Ok(release
                .filter(|r| bundle::newer(&r.version, &p.manifest.version))
                .map(|r| (json!({"catalog": url, "id": id, "version": r.version}), r.version.clone())))
        }
        Source::Github { repo, tag: installed, .. } => {
            let mut repo = github::parse_repo_spec(repo)?;
            repo.tag = None;
            let plan = fetch::resolve_repo(f, &repo)?;
            let tag = plan.repo.tag.clone().unwrap_or_default();
            Ok((tag != *installed).then(|| (json!({"repo": format!("{}@{tag}", plan.repo.slug())}), tag)))
        }
        Source::File { .. } | Source::Other { .. } => Ok(None),
    }
}

/// [`update_for`] over the session's client.
pub(crate) fn update_for_session(s: &mut Session, p: &InstalledPlugin) -> std::result::Result<Option<(Value, String)>, String> {
    with_fetch(s, |f| update_for(f, p))
}

/// Which of `plugins` have a newer release (as `plugin.update` lists them, without installing),
/// for a front end's worker thread.
pub fn check_updates(f: &mut dyn Fetch, plugins: &[InstalledPlugin]) -> Value {
    let (mut updates, mut errors) = (vec![], vec![]);
    for p in plugins {
        match update_for(f, p) {
            Ok(Some((install, available))) => {
                updates.push(json!({"id": p.manifest.id, "installed": p.manifest.version, "available": available, "install": install}));
            }
            Ok(None) => {}
            Err(e) => errors.push(json!({"id": p.manifest.id, "error": e})),
        }
    }
    json!({"updates": updates, "updated": [], "errors": errors})
}
