//! File menu: new, open, save, close, export, properties, options.

use serde_json::{Value as Json, json};

use super::*;
use crate::DocState;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.new", "New Workbook", ["File"], Some("Cmd+N"), "{sample?: \"budget\"|\"sales\"|\"grades\"}", always, new_workbook),
        cmd!(noundo "file.open", "Open…", ["File"], Some("Cmd+O"), "{path} | {name, base64}, plus {password} for a password-protected workbook (xlsx, xlsm, xls import, xlsb and ods data import, csv, tsv, txt, json)", always, open),
        cmd!(noundo "file.reload", "Reload Workbook", [], None, "{path?} | {name?, base64?}, plus {force?: bool} to discard unsaved changes: replaces an open workbook's contents with the file's current contents, keeping its window, active sheet, selection and scroll (opens it when it isn't open; without a path or bytes, the active workbook's own file). For a file another program rewrote; the Save-versus-disk prompt runs it with force", always, reload),
        cmd!(noundo "file.save", "Save", ["File"], Some("Cmd+S"), "{path?} (xlsx by default; .csv/.tsv/.json/.html/.pdf by extension)", has_doc, save),
        cmd!(noundo "file.saveAs", "Save As…", ["File"], Some("Cmd+Shift+S"), "{path} (.xlsx, .csv, .tsv, .json, .html, or .pdf: a PDF of the active sheet)", has_doc, save_as),
        cmd!(query "file.saveBytes", "Encode Workbook", [], None, "{format?: xlsx|csv|tsv|json|html} → {base64}", has_doc, save_bytes),
        cmd!(
            query "file.externalChanges",
            "Changed on Disk",
            [],
            None,
            "{} → {changed, documents: [{index, uid, title, path, dirty}]}: the open workbooks whose file no longer holds what GridCraft read or wrote (a Git checkout, another editor). Always empty in the browser",
            always,
            external_changes
        ),
        cmd!(
            noundo "file.ignoreExternalChange",
            "Keep This Version",
            [],
            None,
            "{path?} (else the active workbook): keeps the version in GridCraft and accepts the file on disk as it is now, so it isn't reported again until the file changes once more",
            always,
            ignore_external_change
        ),
        cmd!(noundo "file.close", "Close", ["File"], Some("Cmd+W"), "{force?: bool}", has_doc, close),
        cmd!(noundo "file.exportCsv", "Export as CSV", ["File", "Export"], None, "{path, sheet?}", has_doc, |s, p| export(s, p, "csv")),
        cmd!(noundo "file.exportHtml", "Save as Web Page", ["File", "Export"], None, "{path}", has_doc, |s, p| export(s, p, "html")),
        cmd!("file.properties", "Properties", ["File"], None, "{title?, subject?, author?, company?, keywords?, description?}", has_doc, properties),
        cmd!(
            query "file.options",
            "Options",
            ["File"],
            None,
            "{} → {calculation: {mode, iterative, maxIterations, maxChange}} for the active workbook (the interface opens File › Options; change settings with their own commands, e.g. formulas.calculationOptions, view.theme, app.language.set)",
            always,
            options
        ),
        cmd!(noundo "window.activate", "Switch Window", ["Window"], None, "{index}", has_doc, activate_window),
    ]
}

fn new_workbook(s: &mut Session, p: &Json) -> Result<Json> {
    let i = match str_param(p, "sample") {
        Some(name) => {
            let wb = crate::sample::build(name).ok_or_else(|| bad("file.new", format!("unknown sample `{name}`")))?;
            let title = crate::sample::title(name);
            s.add_document(DocState::new(wb, None, title))
        }
        None => s.new_workbook(),
    };
    Ok(json!({"index": i, "title": s.doc()?.display_title()}))
}

fn open(s: &mut Session, p: &Json) -> Result<Json> {
    let mut stamp = None;
    let (name, bytes) = if let Some(b) = str_param(p, "base64") {
        (str_param(p, "name").unwrap_or("Book.xlsx").to_string(), crate::io::base64_decode(b).ok_or_else(|| bad("file.open", "invalid base64"))?)
    } else if let Some(path) = str_param(p, "path") {
        // Already open? Switch to it.
        if let Some(i) = s.documents().iter().position(|d| d.path.as_deref() == Some(path)) {
            s.set_active(i);
            return Ok(json!({"index": i, "alreadyOpen": true}));
        }
        // Taken before the read: a file that changes while we read it is reported afterwards (and
        // the user answers for it), rather than silently recorded as the version we're holding.
        stamp = crate::io::file_stamp(path);
        (path.to_string(), crate::io::read_file(path)?)
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("open".into(), json!({})));
        return ok();
    };
    let encrypted = gridcraft_xlsx::sniff(&bytes) == gridcraft_xlsx::Format::Encrypted;
    let (mut wb, warnings) = crate::io::open_bytes_with_password(&name, &bytes, str_param(p, "password"))?;
    wb.source_dir = str_param(p, "path").and_then(crate::io::source_dir);
    let imported =
        matches!(crate::io::FileKind::from_path(&name), Some(crate::io::FileKind::Ods | crate::io::FileKind::Xlsb | crate::io::FileKind::Xls))
            || matches!(gridcraft_xlsx::sniff(&bytes), gridcraft_xlsx::Format::Ods | gridcraft_xlsx::Format::Xlsb | gridcraft_xlsx::Format::Xls);
    let title = if imported {
        format!("{} (imported).xlsx", std::path::Path::new(&name).file_stem().and_then(|n| n.to_str()).unwrap_or("Book"))
    } else {
        std::path::Path::new(&name).file_name().and_then(|n| n.to_str()).unwrap_or("Book").to_string()
    };
    // An import has no save target: ordinary Save must ask for a new destination. Neither does a
    // workbook decrypted with its password, so Save never replaces the encrypted original with
    // an unprotected copy.
    let path = str_param(p, "path").filter(|_| !imported && !encrypted).map(str::to_string);
    // Replace an untouched blank Book1 like Excel does.
    if s.documents().len() == 1
        && s.active().is_some_and(|d| d.path.is_none() && !d.is_dirty() && d.undo.is_empty() && d.wb.sheets.iter().all(|sh| sh.cells.is_empty()))
    {
        s.close_document(0);
    }
    let mut doc = DocState::new(wb, path, title);
    if doc.path.is_some() {
        doc.file_stamp = stamp;
    }
    if crate::io::FileKind::from_path(&name).is_none() && gridcraft_xlsx::sniff(&bytes) == gridcraft_xlsx::Format::Xlsx {
        doc.content_kind = Some(crate::io::FileKind::Xlsx);
    }
    let i = s.add_document(doc);
    // The decrypted-workbook notice is for the info bar, not a message box.
    let shown: Vec<&str> = warnings.iter().map(String::as_str).filter(|w| *w != crate::io::DECRYPTED_NOTICE).collect();
    if !shown.is_empty() {
        let mut message = shown.iter().take(5).copied().collect::<Vec<_>>().join("\n\n");
        if shown.len() > 5 {
            message.push_str(&format!("\n\n{} additional import warnings.", shown.len() - 5));
        }
        s.ui_requests.push(crate::UiRequest::Message(message));
    }
    let d = s.doc()?;
    Ok(
        json!({"index": i, "title": d.display_title(), "sheets": d.wb.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), "warnings": warnings}),
    )
}

/// The open workbooks whose file no longer holds what GridCraft read or wrote. The interface asks
/// about them once a second and offers the two answers: [`reload`] (the file on disk) or
/// [`ignore_external_change`] (the version in GridCraft).
fn external_changes(s: &mut Session, _: &Json) -> Result<Json> {
    let documents: Vec<Json> = s
        .documents()
        .iter()
        .enumerate()
        .filter_map(|(index, d)| {
            let path = d.path.as_deref()?;
            // A file that can't be read now (gone, no permission) is left to the next check: a
            // prompt can only offer the two versions while both exist.
            let stamp = crate::io::file_stamp(path)?;
            (Some(stamp) != d.file_stamp).then(|| {
                json!({
                    "index": index,
                    "uid": d.uid,
                    "title": d.display_title(),
                    "path": path,
                    "dirty": d.is_dirty(),
                })
            })
        })
        .collect();
    Ok(json!({"changed": !documents.is_empty(), "documents": documents}))
}

/// Replaces a workbook with the file's current contents (the "disk version" answer). The document
/// keeps its window, uid, active sheet, selection and scroll; the undo history starts over, since
/// the base it referred to is gone. Unsaved changes are kept unless `force`.
fn reload(s: &mut Session, p: &Json) -> Result<Json> {
    let requested = str_param(p, "path").map(str::to_string);
    let name = str_param(p, "name").map(str::to_string);
    let given = match str_param(p, "base64") {
        Some(b) => Some(crate::io::base64_decode(b).ok_or_else(|| bad("file.reload", "invalid base64"))?),
        None => None,
    };
    let file = name.as_deref().map(file_name);
    // Which open workbook does this call name? A path identifies it exactly; a name is the title
    // the workbook was opened under; neither means the active one.
    let index = match (&requested, &file) {
        (Some(path), _) => s.documents().iter().position(|d| d.path.as_deref() == Some(path.as_str())),
        (None, Some(file)) => s.documents().iter().position(|d| d.display_title() == *file || d.title == *file),
        (None, None) => Some(s.active_index()).filter(|i| s.documents().get(*i).is_some()),
    };
    let Some(i) = index else {
        // Not open: reloading it is an open, with the file rules that go with one (`file.open`).
        if requested.is_none() && given.is_none() {
            return Err(bad("file.reload", "missing `path` or `base64`, and the active workbook has no file"));
        }
        let mut r = open(s, p)?;
        if let Some(o) = r.as_object_mut() {
            o.insert("reloaded".into(), json!(false));
        }
        return Ok(r);
    };
    // Everything the new document inherits from the old one, so no borrow of `s` outlives the swap.
    let old = OldDoc::from(s, i)?;
    if old.dirty && !bool_param(p, "force").unwrap_or(false) {
        return Err(bad("file.reload", format!("`{}` has unsaved changes; pass force: true to discard them", old.title)));
    }
    // Read the workbook's own file, unless the caller handed over the bytes (the browser's relay
    // reads them). Stat first: a change that lands while we read is reported afterwards, rather
    // than recorded as the version we hold.
    let source = requested.clone().or_else(|| old.path.clone());
    let (bytes, stamp) = match given {
        Some(b) => (b, source.as_deref().and_then(crate::io::file_stamp)),
        None => {
            let path = requested.clone().or_else(|| old.path.clone()).ok_or_else(|| bad("file.reload", "this workbook has no file; pass `path`"))?;
            let stamp = crate::io::file_stamp(&path);
            (crate::io::read_file(&path)?, stamp)
        }
    };
    let open_name = file.or_else(|| source.as_deref().map(file_name)).unwrap_or_else(|| "Book.xlsx".into());
    let (mut wb, warnings) = crate::io::open_bytes_with_password(&open_name, &bytes, str_param(p, "password"))?;
    wb.source_dir = source.as_deref().and_then(crate::io::source_dir);
    let sheets: Vec<String> = wb.sheets.iter().map(|sh| sh.name.clone()).collect();
    // The sheets that are still there keep their place and their selection memory.
    let active = old.active_sheet.as_deref().and_then(|n| wb.sheet_index(n));
    if let Some(a) = active {
        wb.active_sheet = a;
    }
    let mut doc = DocState::new(wb, old.path.clone(), old.title.clone());
    doc.uid = old.uid;
    doc.revision = old.revision.saturating_add(1);
    doc.content_kind = old.content_kind;
    doc.file_stamp = stamp;
    if active.is_some() {
        doc.selection = old.selection.clone();
    }
    if sheets == old.sheet_names {
        doc.sheet_selections = old.sheet_selections.clone();
    }
    let title = doc.display_title();
    if let Some(slot) = s.docs.get_mut(i) {
        *slot = doc;
    }
    s.set_active(i);
    Ok(json!({"index": i, "title": title, "sheets": sheets, "warnings": warnings, "reloaded": true}))
}

/// The parts of the workbook a reload carries over, taken before `s` is mutated.
struct OldDoc {
    path: Option<String>,
    title: String,
    uid: u64,
    revision: u64,
    content_kind: Option<crate::io::FileKind>,
    dirty: bool,
    selection: Selection,
    sheet_selections: Vec<Selection>,
    sheet_names: Vec<String>,
    active_sheet: Option<String>,
}

impl OldDoc {
    fn from(s: &Session, i: usize) -> Result<OldDoc> {
        let d = s.documents().get(i).ok_or(EngineError::NoDocument)?;
        Ok(OldDoc {
            path: d.path.clone(),
            title: d.title.clone(),
            uid: d.uid,
            revision: d.revision,
            content_kind: d.content_kind,
            dirty: d.is_dirty(),
            selection: d.selection.clone(),
            sheet_selections: d.sheet_selections.clone(),
            sheet_names: d.wb.sheets.iter().map(|sh| sh.name.clone()).collect(),
            active_sheet: d.wb.active().map(|sh| sh.name.clone()),
        })
    }
}

/// The "keep the version in GridCraft" answer: records the file's stamp as it is now, so the
/// change is not reported again until the file changes once more (the next Save overwrites it).
fn ignore_external_change(s: &mut Session, p: &Json) -> Result<Json> {
    let i = document_index(s, p, "file.ignoreExternalChange")?;
    let path = s.documents().get(i).and_then(|d| d.path.clone()).ok_or_else(|| bad("file.ignoreExternalChange", "this workbook has no file"))?;
    let stamp = crate::io::file_stamp(&path);
    if let Some(d) = s.docs.get_mut(i) {
        d.file_stamp = stamp;
    }
    Ok(json!({"index": i, "path": path, "kept": true}))
}

/// The open workbook a file command names: the one at `path`, else the active one.
fn document_index(s: &Session, p: &Json, cmd: &str) -> Result<usize> {
    if let Some(path) = str_param(p, "path") {
        return s.documents().iter().position(|d| d.path.as_deref() == Some(path)).ok_or_else(|| bad(cmd, format!("`{path}` is not open")));
    }
    let i = s.active_index();
    s.documents().get(i).map(|_| i).ok_or(EngineError::NoDocument)
}

/// The file name of a path, for matching an open workbook and for the format by extension.
fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string()
}

fn save(s: &mut Session, p: &Json) -> Result<Json> {
    let path = match str_param(p, "path").map(str::to_string).or_else(|| s.active().and_then(|d| d.path.clone())) {
        Some(p) => p,
        None => {
            s.ui_requests.push(crate::UiRequest::Dialog("saveAs".into(), json!({})));
            return ok();
        }
    };
    // A PDF is a print of the sheet (Page Setup applies), not a copy of the workbook: the
    // document keeps its file and its unsaved changes.
    if std::path::Path::new(&path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return super::print::export_pdf(s, &json!({"path": path}));
    }
    let d = s.doc_mut()?;
    // The document's own file keeps the format its content had when opened; any other path
    // goes by its extension. Relative paths to linked workbooks are rewritten for the new folder.
    let kind = crate::io::FileKind::from_path(&path).or(d.content_kind.filter(|_| d.path.as_deref() == Some(path.as_str())));
    let rebased = crate::io::source_dir(&path).and_then(|f| gridcraft_xlsx::rebase_links(&d.wb, &f));
    let wb = rebased.as_ref().unwrap_or(&d.wb);
    let bytes = match kind {
        Some(kind) => crate::io::save_bytes_as(wb, &path, kind)?,
        None => crate::io::save_bytes(wb, &path)?,
    };
    crate::io::write_file(&path, &bytes)?;
    // Only a full-fidelity format clears the dirty flag and becomes the document's file.
    if matches!(kind, Some(crate::io::FileKind::Xlsx | crate::io::FileKind::Json)) {
        d.saved = d.wb.clone();
        d.path = Some(path.clone());
    }
    // Our own write is not an external change: record what we just put there.
    if d.path.as_deref() == Some(path.as_str()) {
        d.file_stamp = crate::io::file_stamp(&path);
    }
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

fn save_as(s: &mut Session, p: &Json) -> Result<Json> {
    if str_param(p, "path").is_none() {
        s.ui_requests.push(crate::UiRequest::Dialog("saveAs".into(), json!({})));
        return ok();
    }
    save(s, p)
}

fn save_bytes(s: &mut Session, p: &Json) -> Result<Json> {
    let fmt = str_param(p, "format").unwrap_or("xlsx");
    let d = s.doc()?;
    let bytes = crate::io::save_bytes(&d.wb, &format!("x.{fmt}"))?;
    let name = format!("{}.{fmt}", std::path::Path::new(&d.display_title()).file_stem().and_then(|x| x.to_str()).unwrap_or("Book"));
    Ok(json!({"name": name, "base64": crate::io::base64_encode(&bytes), "bytes": bytes.len()}))
}

fn close(s: &mut Session, p: &Json) -> Result<Json> {
    let force = bool_param(p, "force").unwrap_or(false);
    let d = s.doc()?;
    if d.is_dirty() && !force {
        s.ui_requests.push(crate::UiRequest::Dialog("saveChanges".into(), json!({"title": d.display_title()})));
        return Ok(json!({"closed": false, "dirty": true}));
    }
    let i = s.active_index();
    s.close_document(i);
    Ok(json!({"closed": true}))
}

fn export(s: &mut Session, p: &Json, ext: &str) -> Result<Json> {
    let path = str_param(p, "path").ok_or_else(|| bad("file.export", "missing `path`"))?.to_string();
    let d = s.doc()?;
    let mut wb = (*d.wb).clone();
    if p.get("sheet").is_some() {
        wb.active_sheet = target_sheet(s, p)?;
    }
    let target = if path.ends_with(&format!(".{ext}")) { path.clone() } else { format!("{path}.{ext}") };
    let bytes = crate::io::save_bytes(&wb, &target)?;
    crate::io::write_file(&target, &bytes)?;
    Ok(json!({"path": target, "bytes": bytes.len()}))
}

fn properties(s: &mut Session, p: &Json) -> Result<Json> {
    edit(s, |cx| {
        let pr = &mut cx.wb.props;
        for (k, slot) in [
            ("title", &mut pr.title),
            ("subject", &mut pr.subject),
            ("author", &mut pr.author),
            ("company", &mut pr.company),
            ("keywords", &mut pr.keywords),
            ("description", &mut pr.description),
        ] {
            if let Some(v) = str_param(p, k) {
                *slot = v.to_string();
            }
        }
        Ok(serde_json::to_value(&cx.wb.props).unwrap_or_default())
    })
}

/// File › Options: reports the workbook-level settings and asks the interface to show its Options
/// page. Nothing changes here; every setting has its own command.
fn options(s: &mut Session, _: &Json) -> Result<Json> {
    s.ui_requests.push(crate::UiRequest::Dialog("options".into(), json!({})));
    let calculation = s.active().map(|d| {
        let c = &d.wb.calc;
        let mode = match c.mode {
            gridcraft_model::CalcMode::Automatic => "automatic",
            gridcraft_model::CalcMode::AutomaticExceptTables => "automaticExceptTables",
            gridcraft_model::CalcMode::Manual => "manual",
        };
        json!({"mode": mode, "iterative": c.iterative, "maxIterations": c.max_iterations, "maxChange": c.max_change})
    });
    Ok(json!({"calculation": calculation}))
}

fn activate_window(s: &mut Session, p: &Json) -> Result<Json> {
    let i = u32_param(p, "index").ok_or_else(|| bad("window.activate", "missing `index`"))? as usize;
    if i >= s.documents().len() {
        return Err(bad("window.activate", "no such window"));
    }
    s.set_active(i);
    ok()
}
