//! Office Scripts: saved TypeScript programs that drive the workbook through the ExcelScript
//! object model, and the recorder that writes one from the command journal.
//!
//! The scripts live outside the workbook, in the application's settings folder
//! ([`gridcraft_script::ScriptStore`]), so opening a workbook never carries someone else's code.
//! A run is one undo step: the bridge turns each call into a command (see [`crate::script`]),
//! failures and timeouts roll the whole run back, and `script.run` itself is not journaled — the
//! commands it issued are.
//!
//! Recording reads the existing journal: `script.record {on: true}` remembers its position and
//! `{on: false}` turns what arrived since into a script. Nothing about the running session is
//! recorded twice.

use serde_json::{Value as Json, json};

use gridcraft_script::host::Limits;
use gridcraft_script::recorder::JournalStep;
use gridcraft_script::{ScriptStore, StoreError};

use super::{CommandSpec, EngineError, Result, Session, bad, bool_param, cmd, str_param};

/// What the pane says when there is no runtime at all (wasm, or a host that never installed one).
const NO_RUNTIME: &str = "Office Scripts need Bun, a JavaScript/TypeScript runtime, which is downloaded separately. \
                          Install Bun, then point GridCraft at it with GRIDCRAFT_BUN or put it in the runtimes folder \
                          (Automate ▸ Office Scripts shows both). This build has no script runtime.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "script.list",
            "List Scripts",
            ["Automate", "Scripting Tools"],
            None,
            "{} → [{id, name, code, created, modified}] (created and modified in milliseconds since 1970-01-01 UTC)",
            super::always,
            list
        ),
        cmd!(
            "script.save",
            "Save Script",
            ["Automate", "Scripting Tools"],
            None,
            "{name, code} saves the script under the name (replacing a script with that name, ignoring case) → {id, name, code, created, modified}; the code is checked for balanced strings, comments and brackets, and the run reports the real error",
            super::always,
            save
        ),
        cmd!("script.delete", "Delete Script", ["Automate", "Scripting Tools"], None, "{id}", super::always, delete),
        cmd!(
            query "script.runtime",
            "Script Runtime",
            ["Automate", "Scripting Tools"],
            None,
            "{} → {found, message, path?, expectedPath, downloadUrl, version?}: where the Office Scripts runtime is, or what to do about it",
            super::always,
            runtime
        ),
        cmd!(
            query "script.run",
            "Run Script",
            ["Automate", "Scripting Tools"],
            None,
            "{id | code, timeoutMs? (default 30000, at most 600000)} runs the script as one undo step; a syntax error, a runtime error or a timeout leaves the workbook as it was → {id?, name?, calls, output: [console.log lines]}",
            super::has_doc,
            run
        ),
        cmd!(query "script.workbook", "Script Workbook", [], None, "{} → {name, app, version, activeSheet, dirty}: the workbook facts the Office Scripts bridge reads", super::always, workbook_info),
        cmd!(
            query "script.rangeInfo",
            "Script Range Info",
            [],
            None,
            "{range?, sheet?} → {sheet, address, rowIndex, columnIndex, rowCount, columnCount, values, texts, formulas, numberFormats} (the used range when `range` is omitted); bridge support for Office Scripts getters",
            super::has_doc,
            range_info
        ),
        cmd!(query "script.sheets", "Script Sheets", [], None, "{} → {workbook, active, sheets: [{name, index, usedRange, visible, standardHeight, standardWidth, tables, charts}]}: bridge support for the Worksheet collection", super::has_doc, sheets_info),
        cmd!(
            query "script.selection",
            "Script Selection",
            [],
            None,
            "{} → {sheet, address, active, areas: [{sheet, address}]}: the selection as Office Scripts sees it",
            super::has_doc,
            selection_info
        ),
        cmd!(
            query "script.tables",
            "Script Tables",
            [],
            None,
            "{sheet?} → [{name, range, headerRow, totalsRow, style, columns: [{name, index, range, headerRowRange, dataBodyRange, totalsRowRange}]}]: bridge support for Table",
            super::has_doc,
            tables_info
        ),
        cmd!(
            query "script.charts",
            "Script Charts",
            [],
            None,
            "{sheet?} → [{id, index, name, type, source, left, top, width, height, worksheet}]: bridge support for Chart",
            super::has_doc,
            charts_info
        ),
        cmd!(
            query "script.validation",
            "Script Validation",
            [],
            None,
            "{cell?, sheet?} → {type, operator, formula1, formula2, allowBlank, errorStyle, errorTitle, errorMessage, inputTitle, inputMessage} or null: bridge support for DataValidation",
            super::has_doc,
            validation_info
        ),
        cmd!(
            query "script.folder",
            "Script Folder",
            ["Automate", "Scripting Tools"],
            None,
            "{path?: a folder GridCraft keeps scripts in, e.g. a synced OneDrive/Documents/Office Scripts; path: \"\" resets to the app's own folder} → {folder, default, custom}",
            super::always,
            folder
        ),
        cmd!(
            query "script.osts",
            "Script File Format",
            [],
            None,
            "{id?} → {formatVersion, apiInfo, honoured, bodyCrLf, unknownFields}: whether GridCraft can honour the file's Office Scripts API variant",
            super::always,
            osts_info
        ),
        cmd!(
            "script.import",
            "Import Script",
            ["Automate", "Scripting Tools"],
            None,
            "{path, name?} imports an Office Scripts .osts file (version 0.x, CRLF body, unknown fields kept) → {id, name, code, created, modified}",
            super::always,
            import
        ),
        cmd!(
            "script.export",
            "Export Script",
            ["Automate", "Scripting Tools"],
            None,
            "{id, path} writes the script as an Office Scripts .osts file, with the fields it was imported with → {path, bytes}",
            super::always,
            export
        ),
        cmd!(
            noundo "script.record",
            "Record Script",
            ["Automate", "Scripting Tools"],
            None,
            "{on?: bool, name?} toggles recording from the command journal; on stopping, the commands since it started become a script (Recorded Script, or `name`) → {recording} or {recording: false, id, name, code, steps, skipped: [{id, reason}]}",
            super::always,
            record
        ),
    ]
}

fn store(s: &Session) -> Result<ScriptStore> {
    s.scripts_dir
        .clone()
        .map(ScriptStore::new)
        .ok_or_else(|| EngineError::Other("Office Scripts are unavailable: this session has no scripts folder to keep them in.".into()))
}

fn store_error(cmd: &str, e: StoreError) -> EngineError {
    bad(cmd, e.to_string())
}

fn list(s: &mut Session, _: &Json) -> Result<Json> {
    let records = store(s)?.list().map_err(|e| store_error("script.list", e))?;
    serde_json::to_value(records).map_err(|e| EngineError::Other(e.to_string()))
}

fn save(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("script.save", "missing `name`"))?;
    let code = str_param(p, "code").ok_or_else(|| bad("script.save", "missing `code`"))?;
    let record = store(s)?.save(name, code).map_err(|e| store_error("script.save", e))?;
    serde_json::to_value(record).map_err(|e| EngineError::Other(e.to_string()))
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let id = str_param(p, "id").ok_or_else(|| bad("script.delete", "missing `id`"))?;
    store(s)?.delete(id).map_err(|e| store_error("script.delete", e))?;
    Ok(json!({"deleted": id}))
}

fn runtime(s: &mut Session, _: &Json) -> Result<Json> {
    let info = match &s.script_host {
        Some(host) => host.runtime(),
        None => gridcraft_script::RuntimeInfo::missing("", NO_RUNTIME),
    };
    serde_json::to_value(info).map_err(|e| EngineError::Other(e.to_string()))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let (id, name, code) = match str_param(p, "code") {
        Some(code) => (None, None, code.to_string()),
        None => {
            let id = str_param(p, "id").ok_or_else(|| bad("script.run", "missing `code` or `id`"))?;
            let record = store(s)?.load(id).map_err(|e| store_error("script.run", e))?;
            (Some(record.id), Some(record.name), record.code)
        }
    };
    // The real grammar is the runtime's job; this catches the damage that would make it fail
    // before the first statement, and it does so before the workbook can be touched.
    gridcraft_script::syntax::check(&code).map_err(|e| bad("script.run", e.to_string()))?;
    let timeout_ms = match p.get("timeoutMs") {
        None | Some(Json::Null) => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| bad("script.run", "`timeoutMs` must be a whole number of milliseconds"))?),
    };
    let limits = Limits::default().with_timeout(timeout_ms);
    let host = s.script_host.clone().ok_or_else(|| EngineError::Other(NO_RUNTIME.into()))?;
    let label = match &name {
        Some(name) => format!("Run Script: {name}"),
        None => "Run Script".to_string(),
    };
    let report = s.grouped(&label, |s| {
        let mut exec = crate::script::SessionBridge { session: s };
        host.run(&code, &limits, &mut exec).map_err(|e| EngineError::Other(e.to_string()))
    })?;
    let mut out = json!({
        "calls": report.calls,
        "output": crate::script::capped_output(report.output),
    });
    if let Some(id) = id {
        out["id"] = Json::String(id);
    }
    if let Some(name) = name {
        out["name"] = Json::String(name);
    }
    Ok(out)
}

fn record(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or_else(|| s.script_recording.is_none());
    if on {
        if s.script_recording.is_none() {
            s.script_recording = Some(s.journal.len());
        }
        return Ok(json!({"recording": true}));
    }
    let Some(start) = s.script_recording.take() else {
        return Ok(json!({"recording": false}));
    };

    let store = store(s)?;
    // A journal trimmed past the start loses the beginning of the recording, never the workbook.
    let start = start.min(s.journal.len());
    let steps: Vec<JournalStep> = s.journal[start..]
        .iter()
        .enumerate()
        .filter(|(_, (id, _))| id != "script.record")
        .map(|(offset, (id, params))| {
            let name = s.sheet_names.get(start + offset).cloned().flatten();
            let mut params = crate::script::resolve_recorded_sheet_named(params, name.as_deref());
            if id == "range.setValues"
                && let Some(values) = s.write_values.get(start + offset).cloned().flatten()
                && let Json::Object(map) = &mut params
            {
                // The values the command stored (typed input and the cell's format applied).
                map.insert("values".into(), values);
                map.remove("parse");
            }
            JournalStep::new(id.clone(), params)
        })
        .collect();
    let recording = gridcraft_script::recorder::record(&steps);
    let base = str_param(p, "name").unwrap_or("Recorded Script");
    let name = store.free_name(base).map_err(|e| store_error("script.record", e))?;
    let saved = store.save(&name, &recording.code).map_err(|e| store_error("script.record", e))?;
    let skipped: Vec<Json> = recording.skipped.iter().map(|s| json!({"id": s.id, "reason": s.reason})).collect();
    Ok(json!({
        "recording": false,
        "id": saved.id,
        "name": saved.name,
        "code": saved.code,
        "steps": recording.calls.len(),
        "skipped": skipped,
    }))
}

// ---------------------------------------------------------------------------
// Bridge support queries. They are commands like every other query, so the CLI, the control
// channel and MCP can inspect the same facts the Office Scripts object model reads.
// ---------------------------------------------------------------------------

/// The types a range needs to answer `getValues` and friends in one round trip.
fn range_info(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = super::target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let range = match super::str_param(p, "range") {
        Some(_) => super::target_range(s, p)?,
        None => sh.used_range().unwrap_or_else(|| gridcraft_core::RangeRef::cell(d.selection.active)),
    };
    if range.count() > 1_000_000 {
        return Err(bad("script.rangeInfo", "range larger than 1,000,000 cells; read it in parts"));
    }
    let mut values = Vec::new();
    let mut texts = Vec::new();
    let mut formulas = Vec::new();
    let mut number_formats = Vec::new();
    for row in range.start.row..=range.end.row {
        let mut v = Vec::new();
        let mut t = Vec::new();
        let mut f = Vec::new();
        let mut n = Vec::new();
        for col in range.start.col..=range.end.col {
            let at = gridcraft_core::CellRef::new(row, col);
            v.push(super::super::cmd::inspect::value_json(&sh.value(at)));
            t.push(Json::String(crate::display::cell_text(&d.wb, sh, at)));
            f.push(sh.cell(at).and_then(|c| c.formula.as_ref()).map(|f| Json::String(format!("={}", f.text))).unwrap_or(Json::Null));
            let style = d.wb.styles.get(sh.style_id(at));
            n.push(Json::String(style.num_fmt.as_str().to_string()));
        }
        values.push(Json::Array(v));
        texts.push(Json::Array(t));
        formulas.push(Json::Array(f));
        number_formats.push(Json::Array(n));
    }
    Ok(json!({
        "sheet": sh.name,
        "address": range.a1(),
        "rowIndex": range.start.row,
        "columnIndex": range.start.col,
        "rowCount": range.height(),
        "columnCount": range.width(),
        "values": values,
        "texts": texts,
        "formulas": formulas,
        "numberFormats": number_formats,
        "style": style_json(&d.wb, sh, range.start),
    }))
}

/// The style facts the Office Scripts `Format`, `Font`, `Fill` and `Borders` getters read. It is
/// the top-left cell's style, as ExcelScript's `RangeFormat` is.
fn style_json(wb: &gridcraft_model::Workbook, sh: &gridcraft_model::Sheet, at: gridcraft_core::CellRef) -> Json {
    let st = wb.styles.get(sh.style_id(at));
    let border = |line: &gridcraft_model::BorderLine| {
        json!({
            "style": border_style_name(line.style),
            "color": line.color.hex(&wb.theme),
            "weight": border_weight(line.style),
        })
    };
    let fill = if st.fill.pattern == gridcraft_model::PatternType::Solid { st.fill.fg.hex(&wb.theme) } else { None };
    json!({
        "font": {
            "name": st.font.name,
            "size": st.font.size,
            "bold": st.font.bold,
            "italic": st.font.italic,
            "underline": underline_name(st.font.underline),
            "strikethrough": st.font.strike,
            "color": st.font.color.hex(&wb.theme),
        },
        "fill": { "color": fill },
        "borders": {
            "top": border(&st.borders.top),
            "bottom": border(&st.borders.bottom),
            "left": border(&st.borders.left),
            "right": border(&st.borders.right),
            // The diagonal borders `BorderIndex.diagonalDown`/`diagonalUp` name.
            "diagonalDown": border(&st.borders.diag_down),
            "diagonalUp": border(&st.borders.diag_up),
        },
        "numberFormat": st.num_fmt.as_str(),
        "horizontalAlignment": format!("{:?}", st.align.h),
        "verticalAlignment": format!("{:?}", st.align.v),
    })
}

fn underline_name(u: gridcraft_model::Underline) -> &'static str {
    match u {
        gridcraft_model::Underline::None => "none",
        gridcraft_model::Underline::Single => "single",
        gridcraft_model::Underline::Double => "double",
        gridcraft_model::Underline::SingleAccounting => "singleAccountant",
        gridcraft_model::Underline::DoubleAccounting => "doubleAccountant",
    }
}

fn border_style_name(s: gridcraft_model::BorderStyle) -> &'static str {
    match s {
        // The PascalCase values `ExcelScriptNs` defines, so a comparison with the enum matches.
        gridcraft_model::BorderStyle::None => "None",
        gridcraft_model::BorderStyle::Dashed | gridcraft_model::BorderStyle::MediumDashed => "Dash",
        gridcraft_model::BorderStyle::Dotted => "Dot",
        gridcraft_model::BorderStyle::DashDot | gridcraft_model::BorderStyle::MediumDashDot => "DashDot",
        gridcraft_model::BorderStyle::DashDotDot | gridcraft_model::BorderStyle::MediumDashDotDot => "DashDotDot",
        gridcraft_model::BorderStyle::SlantDashDot => "SlantDashDot",
        gridcraft_model::BorderStyle::Double => "Double",
        _ => "Continuous",
    }
}

fn border_weight(s: gridcraft_model::BorderStyle) -> &'static str {
    match s {
        gridcraft_model::BorderStyle::Hair => "Hairline",
        gridcraft_model::BorderStyle::Medium
        | gridcraft_model::BorderStyle::MediumDashed
        | gridcraft_model::BorderStyle::MediumDashDot
        | gridcraft_model::BorderStyle::MediumDashDotDot => "Medium",
        gridcraft_model::BorderStyle::Thick => "Thick",
        _ => "Thin",
    }
}

fn workbook_info(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(json!({
        "name": d.title,
        "app": "GridCraft",
        "version": env!("CARGO_PKG_VERSION"),
        "activeSheet": d.wb.active().map(|sh| sh.name.clone()),
        "dirty": !std::sync::Arc::ptr_eq(&d.wb, &d.saved),
    }))
}

fn sheets_info(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sheets: Vec<Json> =
        d.wb.sheets
            .iter()
            .enumerate()
            .map(|(index, sh)| {
                json!({
                    "name": sh.name,
                    "index": index,
                    "usedRange": sh.used_range().map(|r| r.a1()),
                    "visible": sh.visibility == gridcraft_model::Visibility::Visible,
                    "standardHeight": sh.default_row_height,
                    "standardWidth": sh.default_col_width,
                    "tables": sh.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
                    "charts": sh.charts.iter().map(|c| c.id).collect::<Vec<_>>(),
                })
            })
            .collect();
    Ok(json!({
        "workbook": d.title,
        "active": d.wb.active().map(|sh| sh.name.clone()),
        "sheets": sheets,
    }))
}

fn selection_info(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let areas: Vec<Json> = d.selection.ranges.iter().map(|r| json!({"sheet": sh.name, "address": r.a1()})).collect();
    Ok(json!({
        "sheet": sh.name,
        "address": d.selection.current().a1(),
        "active": d.selection.active.a1(),
        "areas": areas,
    }))
}

fn tables_info(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = super::target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let tables: Vec<Json> = sh
        .tables
        .iter()
        .map(|t| {
            let columns: Vec<Json> = t
                .columns
                .iter()
                .enumerate()
                .map(|(index, col)| {
                    let c = t.range.start.col.saturating_add(index as u32).min(t.range.end.col);
                    let header = t.header_row.then(|| {
                        // One cell: the column's own header, as the totals row and body ranges are.
                        let cell = gridcraft_core::CellRef::new(t.range.start.row, c);
                        gridcraft_core::RangeRef::new(cell, cell)
                    });
                    let body_start = t.range.start.row.saturating_add(u32::from(t.header_row));
                    let body_end = t.range.end.row.saturating_sub(u32::from(t.totals_row));
                    let body = (body_start <= body_end).then(|| {
                        gridcraft_core::RangeRef::new(gridcraft_core::CellRef::new(body_start, c), gridcraft_core::CellRef::new(body_end, c))
                    });
                    let totals = t.totals_row.then(|| {
                        gridcraft_core::RangeRef::new(
                            gridcraft_core::CellRef::new(t.range.end.row, c),
                            gridcraft_core::CellRef::new(t.range.end.row, c),
                        )
                    });
                    json!({
                        "name": col.name,
                        "index": index,
                        "range": gridcraft_core::RangeRef::new(
                            gridcraft_core::CellRef::new(t.range.start.row, c),
                            gridcraft_core::CellRef::new(t.range.end.row, c),
                        ).a1(),
                        "headerRowRange": header.map(|r| r.a1()),
                        "dataBodyRange": body.map(|r| r.a1()),
                        "totalsRowRange": totals.map(|r| r.a1()),
                    })
                })
                .collect();
            json!({
                "name": t.name,
                "range": t.range.a1(),
                "headerRow": t.header_row,
                "totalsRow": t.totals_row,
                "style": t.style,
                "columns": columns,
                "worksheet": sh.name,
            })
        })
        .collect();
    Ok(Json::Array(tables))
}

fn charts_info(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = super::target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let charts: Vec<Json> = sh
        .charts
        .iter()
        .enumerate()
        .map(|(index, c)| {
            json!({
                "id": c.id,
                "index": index,
                "name": c.title.clone().unwrap_or_else(|| format!("Chart{}", c.id)),
                "type": crate::script::chart_type_name(c.kind),
                "source": c.source,
                "left": c.anchor.dx,
                "top": c.anchor.dy,
                "width": c.anchor.width,
                "height": c.anchor.height,
                "worksheet": sh.name,
            })
        })
        .collect();
    Ok(Json::Array(charts))
}

fn validation_info(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = super::target_sheet(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let at = super::cell_param_on(s, p, "cell", sheet)?.unwrap_or(d.selection.active);
    let Some(dv) = sh.validations.iter().find(|v| v.ranges.iter().any(|r| r.contains(at))) else { return Ok(Json::Null) };
    Ok(json!({
        "type": format!("{:?}", dv.kind),
        "operator": format!("{:?}", dv.op),
        "formula1": dv.f1,
        "formula2": dv.f2,
        "allowBlank": dv.allow_blank,
        "inCellDropdown": dv.in_cell_dropdown,
        "errorStyle": format!("{:?}", dv.error_style),
        "errorTitle": dv.error_title,
        "errorMessage": dv.error_message,
        "inputTitle": dv.input_title,
        "inputMessage": dv.input_message,
        "showError": dv.show_error,
        "showInput": dv.show_input,
    }))
}

fn folder(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(path) = super::str_param(p, "path") else {
        return Ok(json!({
            "folder": s.scripts_dir,
            "default": s.scripts_dir_default,
            "custom": s.prefs.scripts_dir.is_some(),
        }));
    };
    if path.trim().is_empty() {
        // Reset to the app's own folder.
        s.prefs.scripts_dir = None;
        s.scripts_dir = s.scripts_dir_default.clone();
        return Ok(json!({"folder": s.scripts_dir, "default": s.scripts_dir_default, "custom": false}));
    }
    let path = std::path::Path::new(path);
    if !path.is_absolute() {
        return Err(bad("script.folder", "give an absolute folder path, e.g. a synced OneDrive folder"));
    }
    std::fs::create_dir_all(path).map_err(|e| bad("script.folder", format!("the folder cannot be made: {e}")))?;
    // Writability: a folder a script cannot be saved in is not a scripts folder. The probe is
    // removed before returning, and a script's own save is the real write.
    let probe = path.join(".gridcraft-write-test");
    std::fs::write(&probe, b"gridcraft").map_err(|e| bad("script.folder", format!("the folder is not writable: {e}")))?;
    let _ = std::fs::remove_file(&probe);
    let text = path.display().to_string();
    s.scripts_dir = Some(text.clone());
    s.prefs.scripts_dir = Some(text.clone());
    Ok(json!({"folder": text, "default": s.scripts_dir_default, "custom": true}))
}

fn osts_info(s: &mut Session, p: &Json) -> Result<Json> {
    let meta = match super::str_param(p, "id") {
        Some(id) => {
            let record = store(s)?.load(id).map_err(|e| store_error("script.osts", e))?;
            record.osts.unwrap_or_default()
        }
        None => gridcraft_script::OstsMeta::default(),
    };
    let file = meta.file("");
    let (api, honoured) = file.api();
    let mut unknown: Vec<String> = meta.extra.keys().cloned().collect();
    unknown.sort();
    Ok(json!({
        "formatVersion": gridcraft_script::osts::FORMAT_VERSION,
        "apiInfo": api,
        "honoured": honoured,
        "bodyCrLf": true,
        "unknownFields": unknown,
    }))
}

fn import(s: &mut Session, p: &Json) -> Result<Json> {
    let path = super::str_param(p, "path").ok_or_else(|| bad("script.import", "missing `path`"))?;
    let path = std::path::Path::new(path);
    // Open the file itself (nonblocking and no-follow on Unix, regular files only) and read through
    // that handle, bounded: a FIFO cannot block the import and a file that grows cannot be read
    // whole. `MAX_OSTS_BYTES` is the same limit the parser enforces.
    let mut bytes = Vec::new();
    let limit = gridcraft_script::osts::MAX_OSTS_BYTES as u64 + 1;
    gridcraft_script::store::open_regular(path)
        .and_then(|file| std::io::Read::read_to_end(&mut std::io::Read::take(file, limit), &mut bytes))
        .map_err(|e| bad("script.import", format!("{e}")))?;
    if bytes.len() > gridcraft_script::osts::MAX_OSTS_BYTES {
        return Err(bad("script.import", format!("the file is larger than {} bytes", gridcraft_script::osts::MAX_OSTS_BYTES)));
    }
    let text = String::from_utf8(bytes).map_err(|_| bad("script.import", "the file is not UTF-8 text".to_string()))?;
    let file = gridcraft_script::OstsFile::parse(&text).map_err(|e| bad("script.import", e.to_string()))?;
    let stem = path.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Script");
    let name = super::str_param(p, "name")
        .map(str::to_string)
        .unwrap_or_else(|| if file.description.trim().is_empty() { stem.to_string() } else { file.description.clone() });
    let name = gridcraft_script::osts::check_description(&name).map_err(|e| bad("script.import", e.to_string()))?;
    let store = store(s)?;
    let name = store.free_name(&name).map_err(|e| store_error("script.import", e))?;
    let record =
        store.save_imported(&name, &file.code(), gridcraft_script::OstsMeta::from_file(&file)).map_err(|e| store_error("script.import", e))?;
    serde_json::to_value(record).map_err(|e| EngineError::Other(e.to_string()))
}

fn export(s: &mut Session, p: &Json) -> Result<Json> {
    let id = super::str_param(p, "id").ok_or_else(|| bad("script.export", "missing `id`"))?;
    let path = super::str_param(p, "path").ok_or_else(|| bad("script.export", "missing `path`"))?;
    let record = store(s)?.load(id).map_err(|e| store_error("script.export", e))?;
    let file = match &record.osts {
        Some(meta) => meta.file(&record.code),
        None => gridcraft_script::OstsFile::for_script(&record.name, &record.code),
    };
    let text = file.without_record().to_json();
    if let Some(parent) = std::path::Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| bad("script.export", format!("{e}")))?;
    }
    // Write beside the target and rename: a sync client or a concurrent `script.list` must never
    // see a half-written `.osts`, and a failed write must not lose the file that was there.
    let target = std::path::Path::new(path);
    let (temp, mut file) = export_temp(target).map_err(|e| bad("script.export", e))?;
    let written = std::io::Write::write_all(&mut file, text.as_bytes()).and_then(|()| std::io::Write::flush(&mut file));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(bad("script.export", format!("{e}")));
    }
    drop(file);
    if let Err(e) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(bad("script.export", format!("{e}")));
    }
    Ok(json!({"path": path, "bytes": text.len()}))
}

/// A sibling temporary file for an export, created exclusively: two exports in one folder never
/// share a name, so one can never rename the other's content into its own destination.
fn export_temp(target: &std::path::Path) -> std::result::Result<(std::path::PathBuf, std::fs::File), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let stem = target.file_stem().and_then(|s| s.to_str()).unwrap_or("script");
    for _ in 0..8 {
        let name = format!("{stem}.{}.{}.osts.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
        let temp = target.with_file_name(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{e}")),
        }
    }
    Err("could not make a temporary file beside the export".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use gridcraft_core::{CellRef, Value};
    use gridcraft_script::bridge::FailureKind;
    use gridcraft_script::host::FakeHost;
    use gridcraft_script::recorder::{self, JournalStep};
    use gridcraft_script::{BridgeCall, RuntimeInfo, ScriptError, ScriptHost};

    fn session(tag: &str) -> Session {
        let mut s = Session::new();
        let dir = std::env::temp_dir().join(format!("gridcraft-engine-script-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        s.scripts_dir = Some(dir.display().to_string());
        s.execute("file.new", json!({})).unwrap();
        s
    }

    fn calls(s: &mut Session, calls: Vec<BridgeCall>) {
        s.script_host = Some(Arc::new(FakeHost::new(calls)) as Arc<dyn ScriptHost>);
    }

    fn value(s: &Session, sheet: usize, a1: &str) -> Value {
        let at = CellRef::parse(a1).unwrap();
        s.doc().unwrap().wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default()
    }

    fn set_call(range: &str, value: Json) -> BridgeCall {
        BridgeCall::new("Range.setValue", json!({"range": range, "value": value}))
    }

    const SCRIPT: &str = "function main(workbook: ExcelScript.Workbook) { workbook.getActiveWorksheet().getRange(\"A1\").setValue(42); }";

    #[test]
    fn a_border_setter_accepts_the_excel_border_index_spelling() {
        // `getRangeBorder(ExcelScript.BorderIndex.edgeTop).setColor(…)` sends "EdgeTop": the
        // setter path must lowercase it like the getter does, or the call is refused.
        let mut s = session("border-index");
        let mut bridge = crate::script::SessionBridge { session: &mut s };
        let call = BridgeCall::new("RangeBorder.setColor", json!({"range": "A1:B2", "side": "EdgeTop", "color": "#ff0000"}));
        gridcraft_script::BridgeExecutor::execute(&mut bridge, &call).unwrap();
        let lower = BridgeCall::new("RangeBorder.setColor", json!({"range": "A1:B2", "side": "edgebottom", "color": "#00ff00"}));
        gridcraft_script::BridgeExecutor::execute(&mut bridge, &lower).unwrap();
    }

    #[test]
    fn a_script_call_lands_as_a_number_and_one_undo_step() {
        let mut s = session("number");
        calls(&mut s, vec![set_call("A1", json!(42))]);
        let before = s.doc().unwrap().undo.len();
        let result = s.execute("script.run", json!({"code": SCRIPT})).unwrap();
        assert_eq!(result["calls"], 1);
        assert_eq!(value(&s, 0, "A1"), Value::Number(42.0));
        assert_eq!(s.doc().unwrap().undo.len(), before + 1, "the whole run is one step");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(value(&s, 0, "A1"), Value::Empty);
    }

    #[test]
    fn a_failing_script_changes_nothing() {
        let mut s = session("failing");
        let host = FakeHost::failing_after(1, vec![set_call("A1", json!(1)), set_call("A2", json!(2))], ScriptError::runtime("boom"));
        s.script_host = Some(Arc::new(host) as Arc<dyn ScriptHost>);
        let error = s.execute("script.run", json!({"code": SCRIPT})).unwrap_err().to_string();
        assert!(error.contains("boom"), "{error}");
        assert_eq!(value(&s, 0, "A1"), Value::Empty);
        assert_eq!(value(&s, 0, "A2"), Value::Empty);
        assert_eq!(s.doc().unwrap().undo.len(), 0);
    }

    #[test]
    fn a_timeout_rolls_the_run_back() {
        let mut s = session("timeout");
        let host = FakeHost::failing_after(1, vec![set_call("A1", json!(1)), set_call("A2", json!(2))], ScriptError::timeout(50));
        s.script_host = Some(Arc::new(host) as Arc<dyn ScriptHost>);
        let error = s.execute("script.run", json!({"code": SCRIPT, "timeoutMs": 50})).unwrap_err().to_string();
        assert!(error.contains("did not finish"), "{error}");
        assert_eq!(value(&s, 0, "A1"), Value::Empty);
        assert_eq!(value(&s, 0, "A2"), Value::Empty);
    }

    #[test]
    fn a_syntax_error_touches_nothing() {
        let mut s = session("syntax");
        calls(&mut s, vec![set_call("A1", json!(1))]);
        let error = s.execute("script.run", json!({"code": "function main() { "})).unwrap_err().to_string();
        assert!(error.contains("never closed"), "{error}");
        assert_eq!(value(&s, 0, "A1"), Value::Empty);
    }

    #[test]
    fn without_a_runtime_the_commands_say_what_to_do() {
        let mut s = session("none");
        let error = s.execute("script.run", json!({"code": SCRIPT})).unwrap_err().to_string();
        assert!(error.contains("Bun"), "{error}");
        let info = s.execute("script.runtime", json!({})).unwrap();
        assert_eq!(info["found"], false);
        assert!(info["message"].as_str().unwrap_or_default().contains("Bun"));
    }

    #[test]
    fn the_runtime_query_reports_a_found_runtime() {
        let mut s = session("found");
        s.script_host = Some(
            Arc::new(FakeHost::with_runtime(RuntimeInfo::found("C:/bun/bun.exe", "C:/bun/bun.exe", Some("1.2.3".into())))) as Arc<dyn ScriptHost>
        );
        let info = s.execute("script.runtime", json!({})).unwrap();
        assert_eq!(info["found"], true);
        assert_eq!(info["version"], "1.2.3");
    }

    #[test]
    fn scripts_are_saved_listed_run_and_deleted() {
        let mut s = session("store");
        let saved = s.execute("script.save", json!({"name": "Setter", "code": SCRIPT})).unwrap();
        assert_eq!(saved["id"], "setter");
        let list = s.execute("script.list", json!({})).unwrap();
        assert_eq!(list.as_array().map(Vec::len), Some(1));
        calls(&mut s, vec![set_call("A1", json!(42))]);
        let ran = s.execute("script.run", json!({"id": "setter"})).unwrap();
        assert_eq!(ran["name"], "Setter");
        assert_eq!(value(&s, 0, "A1"), Value::Number(42.0));
        s.execute("script.delete", json!({"id": "setter"})).unwrap();
        assert_eq!(s.execute("script.list", json!({})).unwrap().as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn a_script_id_cannot_escape_the_scripts_folder() {
        let mut s = session("traversal");
        for id in ["../x", "..", "a/b"] {
            assert!(s.execute("script.delete", json!({"id": id})).is_err(), "{id}");
            assert!(s.execute("script.run", json!({"id": id})).is_err(), "{id}");
        }
    }

    #[test]
    fn the_recorder_writes_a_script_that_replays_through_the_bridge() {
        let mut s = session("record");
        let start = s.journal.len();
        s.execute("script.record", json!({"on": true})).unwrap();
        s.execute("cell.set", json!({"cell": "A1", "input": 1})).unwrap();
        s.execute("range.setValues", json!({"range": "A2:B2", "values": [[2, "x"]]})).unwrap();
        s.execute("home.bold", json!({"range": "A1", "on": true})).unwrap();
        s.execute("home.italic", json!({"range": "A1"})).unwrap();
        s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
        s.execute("cell.set", json!({"sheet": 1, "cell": "A1", "input": 5})).unwrap();
        let stopped = s.execute("script.record", json!({"on": false})).unwrap();
        let code = stopped["code"].as_str().unwrap_or_default().to_string();
        assert!(code.contains("getRange(\"A1\").setValue(1);"), "{code}");
        assert!(code.contains("getRange(\"A2:B2\").setValues([[2, \"x\"]]);"), "{code}");
        assert!(code.contains("getRange(\"A1\").getFormat().getFont().setBold(true);"), "{code}");
        assert!(code.contains("// Skipped: home.italic (not supported by the Office Scripts bridge)."), "{code}");
        assert!(code.contains("workbook.getWorksheet(\"Data\").getRange(\"A1\").setValue(5);"), "{code}");
        assert_eq!(stopped["steps"], 5);
        assert_eq!(stopped["skipped"][0]["id"], "home.italic");

        // The same journal slice, through the same recorder, makes the same calls the fake
        // bridge then runs: the recorded script reproduces the recorded work.
        let steps: Vec<JournalStep> = s.journal[start..]
            .iter()
            .enumerate()
            .filter(|(_, (id, _))| id != "script.record")
            .map(|(offset, (id, params))| {
                let name = s.sheet_names.get(start + offset).cloned().flatten();
                JournalStep::new(id.clone(), crate::script::resolve_recorded_sheet_named(params, name.as_deref()))
            })
            .collect();
        let rec = recorder::record(&steps);
        assert_eq!(rec.code, code);

        let mut replay = session("record-replay");
        replay.script_host = Some(Arc::new(FakeHost::new(rec.calls)) as Arc<dyn ScriptHost>);
        replay.execute("script.run", json!({"code": code})).unwrap();
        assert_eq!(value(&replay, 0, "A1"), Value::Number(1.0));
        assert_eq!(value(&replay, 0, "A2"), Value::Number(2.0));
        assert_eq!(value(&replay, 0, "B2"), Value::Text("x".into()));
        assert_eq!(value(&replay, 1, "A1"), Value::Number(5.0));
    }

    #[test]
    fn recording_stops_without_a_recording() {
        let mut s = session("no-record");
        let stopped = s.execute("script.record", json!({"on": false})).unwrap();
        assert_eq!(stopped["recording"], false);
        assert!(stopped.get("id").is_none());
    }

    #[test]
    fn a_recorded_range_write_holds_the_value_the_cell_format_gave_it() {
        let mut s = session("record-format");
        s.execute("home.numberFormat", json!({"range": "A1:B1", "code": "0%"})).unwrap();
        s.execute("home.numberFormat", json!({"range": "C1", "code": "@"})).unwrap();
        s.execute("script.record", json!({"on": true})).unwrap();
        s.execute("range.setValues", json!({"range": "A1:C1", "values": [["2", "0.5", "2"]]})).unwrap();
        let stopped = s.execute("script.record", json!({"on": false})).unwrap();
        let code = stopped["code"].as_str().unwrap_or_default();
        // The format decides the stored value: a percent cell holds 0.02 for "2", a text cell "2".
        assert!(code.contains("setValues([[0.02, 0.5, \"2\"]]);"), "{code}");
    }

    #[test]
    fn a_recorded_range_write_holds_the_value_the_command_parsed() {
        let mut s = session("record-parse");
        s.execute("script.record", json!({"on": true})).unwrap();
        s.execute("range.setValues", json!({"range": "A1:B1", "values": [["2.500", "text"]]})).unwrap();
        let stopped = s.execute("script.record", json!({"on": false})).unwrap();
        let code = stopped["code"].as_str().unwrap_or_default();
        // Typed input read "2.500" as 2.5; the script must hold that, not the text.
        assert!(code.contains("setValues([[2.5, \"text\"]]);"), "{code}");
    }

    #[test]
    fn a_very_hidden_request_is_refused() {
        let mut s = session("very-hidden");
        s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
        let mut bridge = crate::script::SessionBridge { session: &mut s };
        let call = BridgeCall::new("Worksheet.setVisibility", json!({"sheet": "Data", "visibility": "VeryHidden"}));
        let error = gridcraft_script::BridgeExecutor::execute(&mut bridge, &call).unwrap_err();
        assert!(error.message.contains("veryHidden"), "{}", error.message);
        assert_eq!(error.code, gridcraft_script::ErrorCode::NotSupported);
    }

    #[test]
    fn a_parsed_set_value_replays_typed_input_as_a_number() {
        let mut s = session("parse-set");
        {
            let mut bridge = crate::script::SessionBridge { session: &mut s };
            let parsed = BridgeCall::new("Range.setValue", json!({"range": "A1", "value": "42", "parse": true}));
            gridcraft_script::BridgeExecutor::execute(&mut bridge, &parsed).unwrap();
            let verbatim = BridgeCall::new("Range.setValue", json!({"range": "B1", "value": "42"}));
            gridcraft_script::BridgeExecutor::execute(&mut bridge, &verbatim).unwrap();
        }
        assert_eq!(value(&s, 0, "A1"), Value::Number(42.0), "typed input stays a number");
        assert_eq!(value(&s, 0, "B1"), Value::Text("42".into()), "a verbatim write stays text");
    }

    #[test]
    fn a_table_is_found_on_another_worksheet() {
        let mut s = session("table-other-sheet");
        s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
        s.execute("cell.set", json!({"cell": "A1", "input": "Name"})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": "x"})).unwrap();
        s.execute("insert.table", json!({"range": "A1:A2", "header": true})).unwrap();
        s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
        let mut bridge = crate::script::SessionBridge { session: &mut s };
        let call = BridgeCall::new("Table.getHeaderRowRange", json!({"table": "Table1"}));
        let range = gridcraft_script::BridgeExecutor::execute(&mut bridge, &call).unwrap();
        assert_eq!(range, json!({"address": "A1", "sheet": "Data"}), "the header cell, found on the sheet that owns the table");
    }

    #[test]
    fn a_table_columns_header_range_is_one_cell() {
        let mut s = session("table-header-range");
        for (cell, text) in [("A1", "Name"), ("B1", "Qty"), ("A2", "x"), ("B2", "1")] {
            s.execute("cell.set", json!({"cell": cell, "input": text})).unwrap();
        }
        s.execute("insert.table", json!({"range": "A1:B2", "header": true})).unwrap();
        let tables = s.execute("script.tables", json!({})).unwrap();
        let columns = tables[0]["columns"].as_array().cloned().unwrap_or_default();
        assert_eq!(columns.len(), 2, "{tables}");
        // The header range is the column's own header cell, not every header up to it.
        assert_eq!(columns[0]["headerRowRange"], json!("A1"), "{tables}");
        assert_eq!(columns[1]["headerRowRange"], json!("B1"), "{tables}");
    }

    #[test]
    fn a_recording_keeps_the_sheet_a_command_edited_when_the_order_changes() {
        // Record starts with Sheet1, Two, Three. The command edits index 1 (Two), then a sheet
        // before it is deleted, which moves every later index. The recording must still name Two,
        // not the sheet that took index 1 afterwards.
        let mut s = session("record-sheet-order");
        s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
        s.execute("home.insertSheet", json!({"name": "Three"})).unwrap();
        s.execute("script.record", json!({"on": true})).unwrap();
        s.execute("cell.set", json!({"sheet": 1, "cell": "A1", "input": "kept"})).unwrap();
        s.execute("home.deleteSheet", json!({"sheet": 0})).unwrap();
        let stopped = s.execute("script.record", json!({"on": false})).unwrap();
        let code = stopped["code"].as_str().unwrap_or_default();
        assert!(code.contains("workbook.getWorksheet(\"Two\").getRange(\"A1\").setValue(\"kept\");"), "{code}");
        assert_eq!(stopped["steps"], 2, "the cell and the delete");
    }

    #[test]
    fn a_journal_trim_moves_the_recording_start_with_it() {
        // Record starts at the end of a full journal; the `script.record` command itself then
        // evicts the oldest half. The start must move with the eviction, or the recording is
        // empty (the start sits past the end of the journal).
        let mut s = session("record-trim");
        while s.journal.len() < crate::JOURNAL_MAX_ENTRIES {
            s.execute("selection.set", json!({"range": "B2"})).unwrap();
        }
        assert_eq!(s.journal.len(), crate::JOURNAL_MAX_ENTRIES);
        // The `script.record` entry itself is the one that pushes the journal past its cap.
        s.execute("script.record", json!({"on": true})).unwrap();
        assert!(s.journal.len() < crate::JOURNAL_MAX_ENTRIES, "the record command trimmed the journal");
        s.execute("cell.set", json!({"cell": "A1", "input": 1})).unwrap();
        s.execute("cell.set", json!({"cell": "A2", "input": 2})).unwrap();
        let stopped = s.execute("script.record", json!({"on": false})).unwrap();
        let code = stopped["code"].as_str().unwrap_or_default();
        assert!(code.contains("getRange(\"A1\").setValue(1);"), "{code}");
        assert!(code.contains("getRange(\"A2\").setValue(2);"), "{code}");
        assert_eq!(stopped["steps"], 2);
    }

    #[test]
    fn a_failed_recording_save_leaves_the_recording_off() {
        let mut s = Session::new();
        s.scripts_dir = None;
        assert!(s.execute("script.record", json!({"on": true})).is_ok());
        let error = s.execute("script.record", json!({"on": false})).unwrap_err().to_string();
        assert!(error.contains("scripts folder"), "{error}");
        assert!(s.script_recording.is_none());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn an_osts_file_round_trips_through_import_and_export() {
        let mut s = session("osts");
        let dir = std::env::temp_dir().join(format!(
            "gridcraft-osts-{}-{}",
            std::process::id(),
            web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let imported = dir.join("Excel Script.osts");
        let body = "function main(workbook: ExcelScript.Workbook) {\r\n  workbook.getActiveWorksheet().getRange(\"A1\").setValue(7);\r\n}\r\n";
        std::fs::write(
            &imported,
            serde_json::json!({
                "version": "0.3.0",
                "body": body,
                "description": "From Excel",
                "noCodeMetadata": "",
                "parameterInfo": "{\"version\":1}",
                "apiInfo": "{\"variant\":\"synchronous\",\"variantVersion\":2}",
                "futureField": {"kept": true}
            })
            .to_string(),
        )
        .unwrap();
        let record = s.execute("script.import", json!({"path": imported.display().to_string()})).unwrap();
        let id = record["id"].as_str().unwrap().to_string();
        assert_eq!(record["name"], "From Excel");
        let exported = dir.join("out.osts");
        s.execute("script.export", json!({"id": id, "path": exported.display().to_string()})).unwrap();
        let text = std::fs::read_to_string(&exported).unwrap();
        assert!(text.contains("futureField"), "{text}");
        assert!(text.contains("\\r\\n"), "{text}");
        assert!(text.contains("synchronous"), "{text}");
        let policy = s.execute("script.osts", json!({"id": id})).unwrap();
        assert_eq!(policy["honoured"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_scripts_folder_is_set_validated_and_reset() {
        let mut s = session("folder");
        let default = s.scripts_dir.clone();
        s.scripts_dir_default = default.clone();
        let dir = std::env::temp_dir().join(format!("gridcraft-scripts-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let set = s.execute("script.folder", json!({"path": dir.display().to_string()})).unwrap();
        assert_eq!(set["custom"], true);
        assert_eq!(s.scripts_dir.as_deref(), Some(dir.display().to_string().as_str()));
        assert!(s.execute("script.folder", json!({"path": "relative/folder"})).is_err());
        let reset = s.execute("script.folder", json!({"path": ""})).unwrap();
        assert_eq!(reset["custom"], false);
        assert_eq!(s.scripts_dir, default);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failures_are_kind_tagged_for_the_pane() {
        let error = ScriptError::protocol("bad line");
        assert_eq!(error.kind, FailureKind::Protocol);
    }
}
