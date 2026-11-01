//! File menu: new, open, save, close, export, properties, options.

use serde_json::{Value as Json, json};

use super::{CommandSpec, Result, Session, always, bad, bool_param, cmd, edit, has_doc, ok, str_param, target_sheet, u32_param};
use crate::DocState;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.new", "New Workbook", ["File"], Some("Cmd+N"), "{sample?: \"budget\"|\"sales\"|\"grades\"}", always, new_workbook),
        cmd!(
            noundo "file.open",
            "Open…",
            ["File"],
            Some("Cmd+O"),
            "{path} | {name, base64}, plus {password} for a password-protected workbook. Text files (csv, tsv, txt, prn), the Text Import Wizard's settings: \
             {delimiter?: \"auto\" (detect the delimiter, a title before the table and the decimal separator) | \";\" | \"||\" (any string), \
             delimiters?: [\",\", \"\\t\"] ([] for none: each line in one cell), other?: \"|\", treatConsecutive?: bool, fixedWidths?: [10, 20] (where fields start), startRow?: 1, \
             decimal?: \"auto\" (default: from the values) | \".\" | \",\", textQualifier?: \"\\\"\" | \"'\" | \"\" (none), csvAsText?: bool (every field as text)}; \
             without them .csv splits at the one of , ; or tab that fits, .txt/.tsv split at tabs and .prn keeps each line in one cell → {index, title, sheets, warnings, \
             text?: {split, delimiters?, fixedWidths?, detected, startRow, decimal, decimalFrom, mixedColumns, ambiguous, fields, rows, completeRows, otherRows, splitNumbers, uncertain}} \
             (xlsx, xlsm, xls import, xlsb and ods data import, csv, tsv, txt, prn, json)",
            always,
            open
        ),
        cmd!(
            query "file.previewText",
            "Text Import Preview",
            [],
            None,
            "{path} | {name, base64}, and file.open's text settings (delimiter, delimiters, other, treatConsecutive, fixedWidths, startRow, decimal, textQualifier), \
             rows?: 20 (at most 200) → {text: as file.open reports it, preview: [[field, …], …] (fields as the text they hold), partial: bool (only the first MiB was read)}",
            always,
            preview_text
        ),
        cmd!(noundo "file.save", "Save", ["File"], Some("Cmd+S"), "{path?} (xlsx by default; .csv/.tsv/.prn/.json/.html/.pdf by extension)", has_doc, save),
        cmd!(noundo "file.saveAs", "Save As…", ["File"], Some("Cmd+Shift+S"), "{path} (.xlsx, .csv, .tsv, .prn: Formatted Text (Space delimited), .json, .html, or .pdf: a PDF of the active sheet)", has_doc, save_as),
        cmd!(query "file.saveBytes", "Encode Workbook", [], None, "{format?: xlsx|csv|tsv|prn|json|html} → {base64}", has_doc, save_bytes),
        cmd!(noundo "file.close", "Close", ["File"], Some("Cmd+W"), "{force?: bool, document?: bool} (closes the active window, or with `document` the workbook and all its windows; the workbook closes with its last window)", has_doc, close),
        cmd!(noundo "file.exportCsv", "Export as CSV", ["File", "Export"], None, "{path, sheet?, values?: bool} → {path, bytes}: writes the cells as displayed, like Excel's Save As CSV; `values: true` writes raw values instead (full-precision numbers, ISO 8601 dates and times, TRUE/FALSE)", has_doc, |s, p| export(s, p, "csv")),
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
        cmd!(noundo "window.activate", "Switch Window", ["Window"], None, "{window} | {doc} | {index} (a window id, a workbook id, or the workbook's position in the list)", has_doc, activate_window),
        cmd!(noundo "window.close", "Close Window", ["Window"], None, "{window?, doc?, force?: bool} → {closed, document}: closes a window (the active one by default); its workbook closes with its last window, asking first when it has unsaved changes unless `force`", has_doc, close_window),
        cmd!(query "window.list", "Windows", ["Window"], None, "{} → [{window, doc, index, title, dirty, active, hidden}]", always, |s, _| Ok(s.window_list())),
    ]
}

fn new_workbook(s: &mut Session, p: &Json) -> Result<Json> {
    let i = match str_param(p, "sample") {
        Some(name) => {
            let wb = crate::sample::build(name).ok_or_else(|| bad("file.new", format!("unknown sample `{name}`")))?;
            let title = crate::sample::title(name);
            s.add_document(DocState::new(wb, None, title, crate::Trust::Local))
        }
        None => s.new_workbook(),
    };
    Ok(json!({"index": i, "title": s.doc()?.display_title()}))
}

fn open(s: &mut Session, p: &Json) -> Result<Json> {
    let (name, bytes) = if let Some(b) = str_param(p, "base64") {
        (str_param(p, "name").unwrap_or("Book.xlsx").to_string(), crate::io::base64_decode(b).ok_or_else(|| bad("file.open", "invalid base64"))?)
    } else if let Some(path) = str_param(p, "path") {
        // Already open? Switch to it.
        if let Some(i) = s.documents().iter().position(|d| d.path.as_deref() == Some(path)) {
            s.set_active(i);
            return Ok(json!({"index": i, "alreadyOpen": true}));
        }
        (path.to_string(), crate::io::read_file(path)?)
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("open".into(), json!({})));
        return ok();
    };
    let encrypted = gridcraft_xlsx::sniff(&bytes) == gridcraft_xlsx::Format::Encrypted;
    let (mut wb, warnings, text) = crate::io::open_bytes_text(&name, &bytes, str_param(p, "password"), &text_options(p)?)?;
    wb.source_dir = str_param(p, "path").and_then(crate::io::source_dir);
    // What `CELL("filename")` reports (#1489). A workbook opened from bytes knows its name but
    // never a folder, so it reports `[name]Sheet` with none; an import reports the file it came
    // from, as Excel does, even though Save must still ask where to put the .xlsx.
    wb.source_path = match str_param(p, "path") {
        Some(path) => crate::io::source_path(path),
        None => Some(std::path::PathBuf::from(&name)),
    };
    let imported =
        matches!(crate::io::FileKind::from_path(&name), Some(crate::io::FileKind::Ods | crate::io::FileKind::Xlsb | crate::io::FileKind::Xls))
            || matches!(gridcraft_xlsx::sniff(&bytes), gridcraft_xlsx::Format::Ods | gridcraft_xlsx::Format::Xlsb | gridcraft_xlsx::Format::Xls);
    let title = if imported {
        format!("{} (imported).xlsx", std::path::Path::new(&name).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book"))
    } else {
        std::path::Path::new(&name).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("Book").to_string()
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
    // Someone else may have made the file: its active content asks before it runs.
    let mut doc = DocState::new(wb, path, title, crate::Trust::FromFile);
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
    let mut out = json!({"index": i, "title": d.display_title(), "sheets": d.wb.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), "warnings": warnings});
    if let Some(report) = &text {
        out["text"] = text_report(report);
    }
    Ok(out)
}

/// The Text Import Wizard's preview: the first rows as the settings split them, and what
/// reading found, without opening the file (#2553).
fn preview_text(_: &mut Session, p: &Json) -> Result<Json> {
    // Only the preview's window is read: the dialog asks again on every change. One byte more
    // says whether the file goes on.
    let head = crate::io::PREVIEW_BYTES + 1;
    let (name, bytes) = if let Some(b) = str_param(p, "base64") {
        (
            str_param(p, "name").unwrap_or("Book.txt").to_string(),
            crate::io::base64_decode_head(b, head).ok_or_else(|| bad("file.previewText", "invalid base64"))?,
        )
    } else if let Some(path) = str_param(p, "path") {
        (path.to_string(), crate::io::read_file_head(path, head as u64)?)
    } else {
        return Err(bad("file.previewText", "missing `path` or `name` and `base64`"));
    };
    let rows = u32_param(p, "rows").map_or(20, |n| n.min(200)) as usize;
    let (preview, report, partial) = crate::io::preview_text(&name, &bytes, &text_options(p)?, rows)?;
    Ok(json!({"text": text_report(&report), "preview": preview, "partial": partial}))
}

/// The Text Import Wizard's settings from `file.open`'s params (#2553).
fn text_options(p: &Json) -> Result<crate::io::TextOpen> {
    use crate::io::{Qualifier, SplitChoice};
    use gridcraft_xlsx::text_layout::{Decimal, Split};
    // A setting of the wrong kind is an error, never a quiet fall back to the extension's split.
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let text = |k: &str| given(k).map(|v| v.as_str().ok_or_else(|| bad("file.open", format!("`{k}` is a string")))).transpose();
    let flag = |k: &str| given(k).map(|v| v.as_bool().ok_or_else(|| bad("file.open", format!("`{k}` is true or false")))).transpose();
    let mut t = crate::io::TextOpen { as_text: flag("csvAsText")?.unwrap_or(false), ..Default::default() };
    let consecutive = flag("treatConsecutive")?.unwrap_or(false);
    let delimiter = text("delimiter")?;
    let other = text("other")?.filter(|o| !o.is_empty());
    let list = given("delimiters")
        .map(|v| {
            let strings = v.as_array().map(|a| a.iter().map(Json::as_str).collect::<Option<Vec<&str>>>());
            strings.flatten().ok_or_else(|| bad("file.open", "`delimiters` is a list of strings, such as [\";\", \"\\t\"]"))
        })
        .transpose()?;
    let fixed = given("fixedWidths")
        .map(|v| {
            let starts =
                v.as_array().map(|a| a.iter().map(|n| n.as_u64().map(|n| usize::try_from(n).unwrap_or(usize::MAX))).collect::<Option<Vec<usize>>>());
            starts.flatten().ok_or_else(|| bad("file.open", "`fixedWidths` is a list of character positions, such as [10, 20]"))
        })
        .transpose()?;
    let chosen = [delimiter.is_some(), list.is_some() || other.is_some(), fixed.is_some()];
    if chosen.iter().filter(|g| **g).count() > 1 {
        return Err(bad("file.open", "give one of `delimiter`, `delimiters`/`other` or `fixedWidths`"));
    }
    // A line end ends the record before any delimiter is looked for.
    if delimiter.into_iter().chain(other).chain(list.iter().flatten().copied()).any(|d| d.contains(['\r', '\n'])) {
        return Err(bad("file.open", "a delimiter can't hold a line end"));
    }
    if let Some(d) = delimiter {
        t.split = match d {
            "" => return Err(bad("file.open", "`delimiter` is empty")),
            "auto" => SplitChoice::Detect,
            d => SplitChoice::Given(Split::Delimited { delimiters: vec![d.to_string()], consecutive }),
        };
    } else if list.is_some() || other.is_some() {
        let mut delimiters: Vec<String> = list.into_iter().flatten().filter(|d| !d.is_empty()).map(str::to_string).collect();
        delimiters.extend(other.map(str::to_string));
        // None ticked, as the wizard allows: each line in one cell.
        t.split = SplitChoice::Given(if delimiters.is_empty() { Split::Lines } else { Split::Delimited { delimiters, consecutive } });
    } else if let Some(breaks) = fixed {
        t.split = SplitChoice::Given(Split::FixedWidth(breaks));
    }
    if let Some(row) = given("startRow") {
        let row = row.as_u64().filter(|r| *r >= 1).ok_or_else(|| bad("file.open", "`startRow` is a row number from 1"))?;
        t.start_row = Some(usize::try_from(row - 1).unwrap_or(usize::MAX));
    }
    t.decimal = match text("decimal")? {
        None | Some("auto") => None,
        Some("." | "point" | "dot") => Some(Decimal::Point),
        Some("," | "comma") => Some(Decimal::Comma),
        Some(other) => return Err(bad("file.open", format!("unknown `decimal` `{other}`: use \"auto\", \".\" or \",\""))),
    };
    t.qualifier = match text("textQualifier")? {
        None | Some("\"") => Qualifier::Double,
        Some("'") => Qualifier::Single,
        Some("" | "none") => Qualifier::None,
        Some(other) => return Err(bad("file.open", format!("unknown `textQualifier` `{other}`: use \"\\\"\", \"'\" or \"\""))),
    };
    Ok(t)
}

/// What reading a text file found, for `file.open`'s result.
fn text_report(r: &gridcraft_xlsx::text_layout::TextReport) -> Json {
    use gridcraft_xlsx::text_layout::{Decimal, DecimalBasis, Split};
    let column = |c: usize| {
        let a1 = gridcraft_core::CellRef::new(0, u32::try_from(c).unwrap_or(u32::MAX)).to_string();
        a1.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string()
    };
    let mut out = json!({
        "detected": r.detected,
        "startRow": r.start_row.saturating_add(1),
        "decimal": match r.numbers.decimal { Decimal::Point => ".", Decimal::Comma => "," },
        "decimalFrom": match r.numbers.basis {
            DecimalBasis::Given => "given",
            DecimalBasis::Values => "values",
            DecimalBasis::Delimiter => "delimiter",
            DecimalBasis::Default => "default",
        },
        "mixedColumns": r.numbers.mixed.iter().map(|c| column(*c)).collect::<Vec<_>>(),
        "ambiguous": r.numbers.ambiguous,
        "fields": r.fields,
        "rows": r.rows,
        "completeRows": r.complete,
        "otherRows": r.other_rows.iter().map(|(row, fields)| json!({"row": row, "fields": fields})).collect::<Vec<_>>(),
        "splitNumbers": r.split_numbers.iter().map(|(row, number)| json!({"row": row, "number": number})).collect::<Vec<_>>(),
        "uncertain": r.uncertain,
    });
    match &r.split {
        Split::Delimited { delimiters, consecutive } => {
            out["split"] = json!("delimited");
            out["delimiters"] = json!(delimiters);
            out["treatConsecutive"] = json!(consecutive);
        }
        Split::FixedWidth(breaks) => {
            out["split"] = json!("fixedWidth");
            out["fixedWidths"] = json!(breaks);
        }
        Split::Lines => out["split"] = json!("lines"),
    }
    out
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
    if std::path::Path::new(&path).extension().and_then(std::ffi::OsStr::to_str).is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
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
        // The new file is the workbook's own now, so `CELL("filename")` reports it (#1489). Set
        // before the `saved` snapshot, or the workbook would read as dirty the moment it was
        // saved; and only when it changes, so an ordinary Save does not clone the workbook.
        let file = crate::io::source_path(&path);
        if d.wb.source_path != file {
            std::sync::Arc::make_mut(&mut d.wb).source_path = file;
        }
        d.saved = d.wb.clone();
        d.path = Some(path.clone());
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
    let name = format!("{}.{fmt}", std::path::Path::new(&d.display_title()).file_stem().and_then(|x| x.to_str()).unwrap_or("Book"));
    // The file name goes into a PivotChart's link to its PivotTable.
    let bytes = crate::io::save_bytes(&d.wb, &name)?;
    Ok(json!({"name": name, "base64": crate::io::base64_encode(&bytes), "bytes": bytes.len()}))
}

/// File › Close closes the active window; the workbook closes with its last window.
fn close(s: &mut Session, p: &Json) -> Result<Json> {
    if !bool_param(p, "document").unwrap_or(false)
        && let Some(id) = s.active_window()
        && let Some(doc) = s.window(id).map(|w| w.doc_uid)
        && s.windows().iter().filter(|w| w.doc_uid == doc).count() > 1
    {
        s.close_window(id);
        return Ok(json!({"closed": true, "document": false}));
    }
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
    let wb = (*d.wb).clone();
    if p.get("sheet").is_some() {
        wb.set_active_sheet(target_sheet(s, p)?);
    }
    let target = if path.ends_with(&format!(".{ext}")) { path.clone() } else { format!("{path}.{ext}") };
    let bytes = if ext == "csv" && bool_param(p, "values").unwrap_or(false) {
        crate::io::csv_values_bytes(&wb)?
    } else {
        crate::io::save_bytes(&wb, &target)?
    };
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
    let id = match u32_param(p, "index").filter(|_| p.get("window").is_none_or(Json::is_null) && p.get("doc").is_none_or(Json::is_null)) {
        Some(i) => {
            let uid = s.documents().get(i as usize).map(|d| d.uid).ok_or_else(|| bad("window.activate", "no such window"))?;
            s.doc_window(uid).ok_or_else(|| bad("window.activate", "no such window"))?
        }
        None if p.get("window").is_none_or(Json::is_null) && p.get("doc").is_none_or(Json::is_null) => {
            return Err(bad("window.activate", "missing `window`"));
        }
        None => super::extra::window_param(s, "window.activate", p)?,
    };
    s.unhide_window(id);
    ok()
}

fn close_window(s: &mut Session, p: &Json) -> Result<Json> {
    let id = super::extra::window_param(s, "window.close", p)?;
    let doc = s.window(id).map(|w| w.doc_uid).unwrap_or_default();
    if s.windows().iter().filter(|w| w.doc_uid == doc).count() > 1 {
        s.close_window(id);
        return Ok(json!({"closed": true, "document": false}));
    }
    let force = bool_param(p, "force").unwrap_or(false);
    let d = s.documents().iter().find(|d| d.uid == doc).ok_or(crate::EngineError::NoDocument)?;
    if d.is_dirty() && !force {
        let title = d.display_title();
        s.activate_window(id);
        s.ui_requests.push(crate::UiRequest::Dialog("saveChanges".into(), json!({"title": title})));
        return Ok(json!({"closed": false, "dirty": true}));
    }
    s.close_window(id);
    Ok(json!({"closed": true, "document": true}))
}
