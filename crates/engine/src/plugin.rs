//! Plug-ins: WebAssembly modules that work on the open workbook through the same commands as the
//! ribbon, the CLI, the control channel and MCP, inside the sandbox of the shared `craft-plugin`
//! crate (RFC-0046). This module is GridCraft's side of it: which commands a plug-in may call
//! ([`ALLOWED`], [`policy`], [`refusal`]), how much one run may change ([`MAX_RUN_EDITS`],
//! [`MAX_CALL_CELLS`], [`MAX_RUN_CELLS`]), one undo step per run (rolled back when the run fails),
//! read-only access checked after every call, each plug-in's settings (kept with the
//! preferences) and the installed plug-ins (the user's `Plug-ins` folder, or this session only).
//! The `plugin.*` commands are in `cmd::plugins`. See `docs/plugins.md`.
//!
//! A run works on the active workbook. It never gets a file, a path, another workbook or window,
//! the clipboard, the undo history, the preferences, macros, scripts or Python, or an external
//! data connection: everything but [`ALLOWED`] is refused, so a command added to GridCraft later
//! is refused until someone decides it is safe here. Its calls also run inside the scripts' file
//! sandbox ([`crate::io::ScriptSandbox`]), so no command reaches the file system from a plug-in
//! whatever its parameters are called; the control channel's token, which lives in a file and the
//! app's environment, is out of reach that way too.

mod install;
mod runtime;

use std::sync::OnceLock;

pub use craft_plugin;
use craft_plugin::{API_VERSION, AppInfo, Instance, KIND_COMMAND, Limits, Manifest, Policy, RunOutput, UiChange};
use serde_json::{Map, Value, json};

use crate::cmd::bad;
use crate::{EngineError, Result, Session, UiRequest};
pub use install::hold;
pub use install::{Candidate, Request, catalog_packages_with, check_updates, install_request, obtain_with};
pub(crate) use install::{catalog_packages, check_catalog_url, install, obtain, remove, review, take_held, update_for_session};
pub use runtime::{InstalledPlugin, Runtime};

/// GridCraft as manifests name it: command plug-ins of every shared ABI version, and only
/// manifests that name `gridcraft` in `apps`.
pub const APP: AppInfo<'static> = AppInfo {
    id: "gridcraft",
    version: env!("CARGO_PKG_VERSION"),
    api: 1..=API_VERSION,
    kinds: &[KIND_COMMAND],
    accept_legacy: false,
    kind_api: &[],
};

/// What one plug-in may keep in its settings (keys and JSON values, in bytes), and its longest key.
pub const MAX_SETTINGS_BYTES: usize = 64 * 1024;
pub const MAX_SETTING_KEY: usize = 128;
/// Catalogs a person may add.
pub const MAX_CATALOGS: usize = 32;
/// Calls that edit the workbook one run may make. A run is one undo step, but each edit copies
/// the workbook while it runs; the workbook's undo history keeps 100 steps, and a run stays well
/// inside that.
pub const MAX_RUN_EDITS: usize = 50;
/// Cells one call may write values into (`cell.set`, `range.setValues`, `range.fill`, the fills,
/// …), and all of a run's calls together: a whole column is about a million cells, so no call
/// writes one, and no run fills one with a few calls.
pub const MAX_CALL_CELLS: u64 = 100_000;
pub const MAX_RUN_CELLS: u64 = 500_000;
/// Messages a run may show (`notify`), and their length in characters.
const MAX_NOTES: usize = 20;
const MAX_NOTE_CHARS: usize = 1000;
/// Least instruction budget `plugin.run {fuel}` may ask for.
const MIN_FUEL: u64 = 1000;

/// The only commands a plug-in may call (everything else is refused: `*` in [`policy`]).
///
/// What is on it reads the active workbook, moves the selection and active sheet, and edits the
/// workbook through undoable commands: cells and ranges, formatting, rows, columns and sheets,
/// names, sorting, filtering, outlines, validation, what-if tools, tables, charts, PivotTables,
/// slicers, objects, comments and notes, page layout and sheet view settings. What is not, and why:
/// - files and paths: `file.*` (open, save, close, export, print, properties), Insert › Picture and
///   the sheet background (they read a file), the Solver's model load and save;
/// - external data and its connections: Get Data, From Text/CSV, From Table/Range, Refresh All,
///   Queries & Connections;
/// - code that runs past this list: macros (`macro.*`), Automate scripts and Record Actions
///   (`automate.*`), Python cells (`python.*`), form controls (they run macros);
/// - other workbooks and windows: `window.*`, New/Hide/Unhide/Switch Windows, Arrange All, custom
///   views, Move or Copy Sheet (it can copy to another workbook);
/// - the undo history (`edit.undo`, `edit.redo`, `history.list`): a run is already one step;
/// - the clipboard and Format Painter: `edit.copy`, `edit.cut`, `edit.paste*`, Insert Copied Cells;
/// - the person's preferences and the app: `app.*`, `file.options`, Add to Dictionary, the formula
///   and status bars, the ruler, the calculation mode, the drawing tool, the add-ins list;
/// - links that leave the app (`link.open`) and the cell editor (`edit.beginEdit`);
/// - protection with a password the person doesn't know (Protect Sheet and Workbook, Allow Edit
///   Ranges, Unprotect), and the slow solvers (Solver, Data Analysis).
pub const ALLOWED: &[&str] = &[
    // reading
    "cell.get",
    "sheet.read",
    "selection.stats",
    "formulas.nameManager",
    "formulas.evaluate",
    "formulas.evaluateFormula",
    "formulas.functions",
    "formulas.errorChecking",
    "formulas.traceError",
    "data.validate",
    "data.scenarioManager",
    "pivot.list",
    "pivot.fields",
    "pivot.fieldList",
    "insert.chartRecommendations",
    "review.workbookStatistics",
    "review.checkAccessibility",
    "review.spelling",
    "review.thesaurus",
    "arrange.selectionPane",
    // the selection and the active sheet
    "selection.set",
    "selection.move",
    "selection.next",
    "selection.nextCorner",
    "selection.row",
    "selection.column",
    "selection.currentRegion",
    "edit.selectAll",
    "edit.find",
    "edit.goTo",
    "edit.goToSpecial",
    "sheet.activate",
    "sheet.next",
    "sheet.previous",
    "formulas.selectPrecedents",
    "formulas.selectDependents",
    // cells and ranges
    "cell.set",
    "cell.toggleCheckbox",
    "range.setValues",
    "range.fill",
    "range.move",
    "edit.clearAll",
    "edit.clearContents",
    "edit.clearFormats",
    "edit.clearComments",
    "edit.clearHyperlinks",
    "edit.fillDown",
    "edit.fillUp",
    "edit.fillLeft",
    "edit.fillRight",
    "edit.fillSeries",
    "edit.autoFill",
    "edit.flashFill",
    "edit.replace",
    "formulas.autoSum",
    "formulas.convertToNumber",
    "formulas.calculateNow",
    "formulas.calculateSheet",
    "formulas.showFormulas",
    "formulas.removeArrows",
    // names
    "formulas.defineName",
    "formulas.deleteName",
    "formulas.createFromSelection",
    "formulas.pasteName",
    // formatting, rows, columns and sheets (Home)
    "home.accounting",
    "home.alignBottom",
    "home.alignCenter",
    "home.alignLeft",
    "home.alignMiddle",
    "home.alignRight",
    "home.alignTop",
    "home.autofitColumnWidth",
    "home.autofitRowHeight",
    "home.bold",
    "home.borders",
    "home.cellStyle",
    "home.clearRules",
    "home.columnWidth",
    "home.comma",
    "home.conditionalFormat",
    "home.decreaseDecimal",
    "home.decreaseFontSize",
    "home.decreaseIndent",
    "home.defaultWidth",
    "home.deleteCells",
    "home.deleteColumns",
    "home.deleteRows",
    "home.deleteSheet",
    "home.doubleUnderline",
    "home.drawBorder",
    "home.fillColor",
    "home.fontColor",
    "home.fontName",
    "home.fontSize",
    "home.formatAsTable",
    "home.formatCells",
    "home.hideColumns",
    "home.hideRows",
    "home.increaseDecimal",
    "home.increaseFontSize",
    "home.increaseIndent",
    "home.insertCells",
    "home.insertColumns",
    "home.insertRows",
    "home.insertSheet",
    "home.italic",
    "home.justify",
    "home.lockCell",
    "home.manageRules",
    "home.mergeAcross",
    "home.mergeCells",
    "home.mergeCenter",
    "home.newCellStyle",
    "home.numberFormat",
    "home.orientation",
    "home.percent",
    "home.rowHeight",
    "home.shrinkToFit",
    "home.strikethrough",
    "home.subscript",
    "home.superscript",
    "home.underline",
    "home.unhideColumns",
    "home.unhideRows",
    "home.unmergeCells",
    "home.wrapText",
    "sheet.rename",
    "sheet.tabColor",
    "sheet.hide",
    "sheet.unhide",
    "sheet.moveColumns",
    // data
    "data.sort",
    "data.sortAscending",
    "data.sortDescending",
    "data.filter",
    "data.filterBy",
    "data.clearFilter",
    "data.reapply",
    "data.advancedFilter",
    "data.removeDuplicates",
    "data.textToColumns",
    "data.flashFill",
    "data.subtotal",
    "data.group",
    "data.ungroup",
    "data.showDetail",
    "data.hideDetail",
    "data.showLevels",
    "data.outlineSettings",
    "data.validation",
    "data.circleInvalid",
    "data.consolidate",
    "data.dataTable",
    "data.goalSeek",
    "data.scenarioAdd",
    "data.scenarioDelete",
    "data.scenarioShow",
    "data.scenarioSummary",
    "data.forecastSheet",
    // tables
    "insert.table",
    "table.bandedColumns",
    "table.bandedRows",
    "table.convertToRange",
    "table.firstColumn",
    "table.headerRow",
    "table.insertSlicer",
    "table.lastColumn",
    "table.removeDuplicates",
    "table.rename",
    "table.resize",
    "table.style",
    "table.summarizePivot",
    "table.totalFunction",
    "table.totalRow",
    // charts, sparklines and objects
    "insert.chart",
    "insert.recommendedCharts",
    "insert.sparkline",
    "insert.checkbox",
    "insert.shape",
    "insert.textBox",
    "insert.symbol",
    "insert.equation",
    "insert.wordArt",
    "insert.icons",
    "insert.link",
    "chart.addElement",
    "chart.changeColors",
    "chart.changeType",
    "chart.delete",
    "chart.formatSelection",
    "chart.move",
    "chart.quickLayout",
    "chart.selectData",
    "chart.set",
    "chart.shapeFill",
    "chart.shapeOutline",
    "chart.switchRowColumn",
    "object.delete",
    "object.move",
    "object.setAnchorMode",
    "arrange.align",
    "arrange.bringForward",
    "arrange.group",
    "arrange.rotate",
    "arrange.sendBackward",
    "shape.setText",
    "wordArt.set",
    "equation.edit",
    "draw.stroke",
    "draw.inkToShape",
    // PivotTables and slicers
    "insert.pivotTable",
    "insert.pivotChart",
    "insert.recommendedPivotTables",
    "insert.slicer",
    "pivot.addField",
    "pivot.calculatedField",
    "pivot.changeSource",
    "pivot.collapse",
    "pivot.delete",
    "pivot.filter",
    "pivot.grandTotals",
    "pivot.group",
    "pivot.layout",
    "pivot.moveField",
    "pivot.options",
    "pivot.refresh",
    "pivot.removeField",
    "pivot.reportLayout",
    "pivot.showDetails",
    "pivot.sort",
    "pivot.style",
    "pivot.subtotals",
    "pivot.valueSettings",
    "slicer.clear",
    "slicer.delete",
    "slicer.selectAll",
    "slicer.toggle",
    // comments, notes and spelling
    "review.newComment",
    "review.replyComment",
    "review.deleteComment",
    "review.resolveComment",
    "review.newNote",
    "review.showNote",
    "review.convertToComments",
    "review.changeSpelling",
    // page layout
    "pageLayout.breaks",
    "pageLayout.headerFooter",
    "pageLayout.margins",
    "pageLayout.orientation",
    "pageLayout.pageSetup",
    "pageLayout.printArea",
    "pageLayout.printGridlines",
    "pageLayout.printHeadings",
    "pageLayout.printTitles",
    "pageLayout.scaleToFit",
    "pageLayout.size",
    "pageLayout.theme",
    "pageLayout.themeColors",
    "pageLayout.themeEffects",
    "pageLayout.themeFonts",
    // the sheet's view settings (saved with the workbook)
    "view.freezeFirstColumn",
    "view.freezePanes",
    "view.freezeTopRow",
    "view.unfreezePanes",
    "view.gridlines",
    "view.headings",
    "view.rightToLeft",
    "view.showHorizontalScrollBar",
    "view.showVerticalScrollBar",
    "view.showObjects",
    "view.showOutlineSymbols",
    "view.showSheetTabs",
    "view.showZeros",
    "view.zoom",
    "view.zoom100",
    "view.zoomToSelection",
    "view.normal",
    "view.pageLayout",
    "view.pageBreakPreview",
    "view.split",
];

/// Allowed commands that write values into cells, and so count against [`MAX_CALL_CELLS`] and
/// [`MAX_RUN_CELLS`] by the cells they write: their `range` / `cell` (or the selection), their
/// `target`, or, for `range.setValues`, the values given.
const VALUE_WRITERS: &[&str] = &[
    "cell.set",
    "cell.toggleCheckbox",
    "range.setValues",
    "range.fill",
    "edit.fillDown",
    "edit.fillUp",
    "edit.fillLeft",
    "edit.fillRight",
    "edit.fillSeries",
    "edit.autoFill",
    "edit.flashFill",
    "data.flashFill",
    "data.dataTable",
    "formulas.autoSum",
    "insert.checkbox",
];

/// Parameter names that name a file or folder on disk, at any depth. No allowed command needs one.
const FILE_KEYS: &[&str] = &["path", "paths", "file", "files", "dir", "folder", "destination"];

/// The rules every plug-in run follows: [`craft_plugin::ALWAYS_DENIED`], and every command but
/// [`ALLOWED`].
pub fn policy() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| Policy::new(&["*"], ALLOWED))
}

/// Why plug-in `m` may not call `command` with `params`, before it runs: a refused command, one
/// outside its manifest's `permissions.commands`, or parameters that name a file.
pub fn refusal(m: &Manifest, command: &str, params: &Value) -> Option<String> {
    if let Err(why) = craft_plugin::check_call(m, policy(), command) {
        return Some(why);
    }
    if file_key_inside(params, 0) {
        return Some(format!(
            "plug-ins can't name files or folders (`{command}` was given one of {}): work on the workbook's cells",
            FILE_KEYS.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    None
}

/// Whether `v` holds one of [`FILE_KEYS`] at any depth.
fn file_key_inside(v: &Value, depth: usize) -> bool {
    // serde_json refuses nesting past 128, so nothing deeper reaches here.
    depth < 128
        && match v {
            Value::Object(m) => m.iter().any(|(k, v)| FILE_KEYS.contains(&k.as_str()) || file_key_inside(v, depth + 1)),
            Value::Array(a) => a.iter().any(|v| file_key_inside(v, depth + 1)),
            _ => false,
        }
}

/// The cells `command` with `params` writes values into ([`VALUE_WRITERS`]; 0 for the others).
fn cells_written(s: &Session, command: &str, params: &Value) -> u64 {
    if !VALUE_WRITERS.contains(&command) {
        return 0;
    }
    if command == "range.setValues" {
        let rows = params.get("values").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        return rows.iter().map(|r| r.as_array().map_or(1, Vec::len) as u64).fold(0, u64::saturating_add);
    }
    if command == "cell.set" {
        // One cell, or a legacy array formula over the selection.
        let array = params.get("array").and_then(Value::as_bool).unwrap_or(false);
        return if array { s.active().map_or(1, |d| d.selection.current().count()) } else { 1 };
    }
    let area = |r: &str| {
        let (_, body) = crate::cmd::split_sheet(r);
        body.split(',').filter_map(gridcraft_core::RangeRef::parse).map(|r| r.count()).fold(0, u64::saturating_add)
    };
    let mut n = crate::cmd::target_ranges(s, params).map_or(0, |rs| rs.iter().map(gridcraft_core::RangeRef::count).fold(0, u64::saturating_add));
    if let Some(t) = params.get("target").and_then(Value::as_str) {
        n = n.saturating_add(area(t));
    }
    n
}

/// A plug-in error as the engine reports it.
fn failed(name: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("{name}: {e}"))
}

/// Store (`Some`) or forget (`None`) plug-in `id`'s setting `key` in `all`, within
/// [`MAX_SETTINGS_BYTES`] and [`MAX_SETTING_KEY`].
fn set_setting(
    all: &mut std::collections::BTreeMap<String, Map<String, Value>>,
    id: &str,
    key: &str,
    value: Option<&str>,
) -> std::result::Result<(), String> {
    let Some(text) = value else {
        if let Some(mine) = all.get_mut(id) {
            mine.remove(key);
            if mine.is_empty() {
                all.remove(id);
            }
        }
        return Ok(());
    };
    if key.is_empty() || key.chars().count() > MAX_SETTING_KEY || key.chars().any(char::is_control) {
        return Err(format!("a setting's key must be 1–{MAX_SETTING_KEY} characters without control characters"));
    }
    let too_big = || format!("a plug-in's settings are limited to {} KB", MAX_SETTINGS_BYTES / 1024);
    if key.len().saturating_add(text.len()) > MAX_SETTINGS_BYTES {
        return Err(too_big());
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("a setting must be JSON: {e}"))?;
    let others: usize = all.get(id).map_or(0, |mine| {
        mine.iter().filter(|(k, _)| k.as_str() != key).map(|(k, v)| k.len().saturating_add(v.to_string().len())).fold(0, usize::saturating_add)
    });
    if others.saturating_add(key.len()).saturating_add(v.to_string().len()) > MAX_SETTINGS_BYTES {
        return Err(too_big());
    }
    all.entry(id.to_string()).or_default().insert(key.to_string(), v);
    Ok(())
}

// ---------- running ----------

/// The selection as a plug-in sees it: its areas in A1 notation (`["A1:B3", "D4"]`).
fn selection_json(s: &Session) -> Value {
    json!(s.active().map(|d| d.selection.ranges.iter().map(gridcraft_core::RangeRef::a1).collect::<Vec<_>>()).unwrap_or_default())
}

/// The sandbox's view of the session: every call is checked by [`refusal`] and the run's caps,
/// and runs as its own undo group inside the run's, so one that opens a dialog, changes the
/// active workbook or, from a read-only plug-in, edits it is rolled back and refused.
struct EngineHost<'s> {
    s: &'s mut Session,
    manifest: &'s Manifest,
    notes: Vec<String>,
    /// Calls that edited the workbook, and the cells they wrote values into.
    edits: usize,
    cells: u64,
}

impl EngineHost<'_> {
    /// Why `command` (which `edits`, writing `cells` values) would take the run past its caps.
    fn over_cap(&self, command: &str, edits: bool, cells: u64) -> Option<String> {
        if edits && self.edits >= MAX_RUN_EDITS {
            return Some(format!(
                "a plug-in run may edit the workbook at most {MAX_RUN_EDITS} times (`{command}`): write a range at once with range.setValues"
            ));
        }
        if cells > MAX_CALL_CELLS {
            return Some(format!("`{command}` would write {cells} cells, and a plug-in call may write at most {MAX_CALL_CELLS}"));
        }
        if self.cells.saturating_add(cells) > MAX_RUN_CELLS {
            return Some(format!("a plug-in run may write at most {MAX_RUN_CELLS} cells (`{command}`)"));
        }
        None
    }
}

impl craft_plugin::Host for EngineHost<'_> {
    fn call(&mut self, command: &str, params: &str) -> std::result::Result<String, String> {
        let params: Value = serde_json::from_str(params).map_err(|e| format!("the params for `{command}` aren't JSON: {e}"))?;
        if let Some(why) = refusal(self.manifest, command, &params) {
            return Err(why);
        }
        let undoable = crate::find_command(command).is_some_and(|c| c.undoable);
        let cells = cells_written(self.s, command, &params);
        if let Some(why) = self.over_cap(command, undoable, cells) {
            return Err(why);
        }
        let s = &mut *self.s;
        let before = s.active().map(|d| (d.uid, d.wb.clone()));
        let requests = s.ui_requests.len();
        // Its own undo group inside the run's: a refusal below puts back the workbook, selection
        // and history as the call found them.
        let token = s.group_begin(command).ok();
        let r = s.execute(command, params);
        // A dialog, the cell editor or a link would wait for the person or leave the app.
        let asks = s.ui_requests.get(requests..).is_some_and(|new| new.iter().any(|q| !matches!(q, UiRequest::Message(_))));
        let after = s.active().map(|d| (d.uid, d.wb.clone()));
        let moved = before.as_ref().map(|b| b.0) != after.as_ref().map(|a| a.0);
        let changed = match (&before, &after) {
            (Some((_, a)), Some((_, b))) => !std::sync::Arc::ptr_eq(a, b),
            _ => false,
        };
        let edited = undoable && changed;
        let refused = if asks {
            Some(format!("`{command}` opens a dialog or leaves the workbook, which plug-ins can't: pass its parameters"))
        } else if moved {
            Some(format!("`{command}` switched to another workbook, which plug-ins can't"))
        } else if edited {
            craft_plugin::check_read_only(self.manifest, command, true).err()
        } else {
            None
        };
        if let Some(t) = token {
            // (A token ends only once; an error here means the group already went with its
            // workbook, and there is nothing left to put back.)
            let _ = if refused.is_some() || r.is_err() { s.group_rollback(t) } else { s.group_end(t).map(|_| ()) };
        }
        if moved && let Some(i) = before.and_then(|(uid, _)| s.document_index(uid)) {
            s.set_active(i);
        }
        // What the call queued for the UI was for a person at a menu, not for this run.
        s.ui_requests.truncate(requests);
        if let Some(why) = refused {
            return Err(why);
        }
        if r.is_ok() && edited {
            self.edits = self.edits.saturating_add(1);
            self.cells = self.cells.saturating_add(cells);
        }
        r.map(|v| v.to_string()).map_err(|e| e.to_string())
    }

    fn selection(&mut self) -> String {
        selection_json(self.s).to_string()
    }

    fn notify(&mut self, message: &str) {
        if self.notes.len() < MAX_NOTES {
            self.notes.push(message.chars().take(MAX_NOTE_CHARS).collect());
        }
    }

    fn log(&mut self, line: &str) {
        log::info!("plug-in {}: {line}", self.manifest.id);
    }

    fn setting(&mut self, key: &str) -> Option<String> {
        Some(self.s.prefs.plugin_settings.get(&self.manifest.id)?.get(key)?.to_string())
    }

    fn set_setting(&mut self, key: &str, value: Option<&str>) -> std::result::Result<(), String> {
        set_setting(&mut self.s.prefs.plugin_settings, &self.manifest.id, key, value)
    }
}

/// Run plug-in `id`'s `command` (default: its first) with `params` on the active workbook, as
/// one undo step named after the plug-in. A run that fails (a trap, an exhausted budget, a
/// refusal it passes on, a non-zero status) leaves the workbook, the selection, the undo history
/// and the plug-in's settings as they were.
pub fn run(s: &mut Session, id: &str, command: Option<&str>, params: &Value, fuel: Option<u64>) -> Result<Value> {
    let cmd = "plugin.run";
    let p = s.plugins.get(id).cloned().ok_or_else(|| bad(cmd, format!("no plug-in `{id}` is installed (see plugin.list)")))?;
    let m = &p.manifest;
    let api = m.target(&APP).map_err(|e| bad(cmd, format!("{} can't run here: {e}", m.name)))?.api();
    let c = m.command(command).ok_or_else(|| bad(cmd, format!("{} has no command `{}`", m.name, command.unwrap_or_default())))?;
    let params = c.resolve_params(params).map_err(|e| bad(cmd, e))?;
    let mut limits = Limits::default();
    if let Some(f) = fuel {
        limits.fuel = f.clamp(MIN_FUEL, limits.fuel);
    }
    let mut instance = Instance::new(&p.wasm, &limits).map_err(|e| failed(&m.name, e))?;
    let (sheet, active) =
        s.active().map(|d| (d.wb.active().map(|sh| sh.name.clone()).unwrap_or_default(), d.selection.active.a1())).ok_or(EngineError::NoDocument)?;
    let input = json!({"api": api, "command": c.id, "params": params, "selection": selection_json(s), "sheet": sheet, "active": active}).to_string();
    s.plugins.last = Some((m.id.clone(), c.id.clone(), params));
    let label = if m.commands.len() > 1 { format!("{}: {}", m.name, c.label.trim_end_matches(['…', '.'])) } else { m.name.clone() };

    // Only `plugin.run` itself is journaled (replaying it runs the plug-in again), not the
    // commands it ran; and no command it runs reaches a file.
    let settings = s.prefs.plugin_settings.clone();
    s.journal_paused = s.journal_paused.saturating_add(1);
    let sandbox = crate::io::ScriptSandbox::enter();
    let ran = s.grouped(&label, |s| {
        let mut host = EngineHost { s, manifest: m, notes: Vec::new(), edits: 0, cells: 0 };
        let ran: std::result::Result<RunOutput, craft_plugin::PluginError> = instance.call(&input, &mut host);
        let notes = std::mem::take(&mut host.notes);
        ran.map(|out| (out, notes)).map_err(|e| failed(&m.name, e))
    });
    drop(sandbox);
    s.journal_paused = s.journal_paused.saturating_sub(1);
    let (out, notes) = match ran {
        Ok(done) => done,
        Err(e) => {
            s.prefs.plugin_settings = settings;
            return Err(e);
        }
    };
    let output = serde_json::from_str::<Value>(&out.output).unwrap_or(Value::String(out.output));
    // Panels (`ui_show`) aren't shown in GridCraft yet: say so rather than drop it silently.
    let panel = matches!(out.ui, Some(UiChange::Show(_))).then_some("GridCraft doesn't show plug-in panels yet");
    Ok(
        json!({"plugin": m.id, "command": c.id, "output": output, "notes": notes, "logs": out.logs, "calls": out.calls, "fuel": out.fuel, "panel": panel}),
    )
}

#[cfg(test)]
mod tests;
