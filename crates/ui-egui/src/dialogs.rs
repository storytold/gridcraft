//! Dialogs. Simple dialogs are forms (fields → a command with params); Format Cells, Insert
//! Function, Find & Replace, Sort, Name Manager and the command palette are custom.
//!
//! Every dialog is also reachable programmatically: `ui.dialog {name}` opens one,
//! `ui.dialog.set {field, value}` edits a field and `ui.dialog.confirm` presses OK.

use egui::{Color32, Key, Sense, Stroke, StrokeKind, vec2};

use crate::icons::{self, Icon};
use gridcraft_engine::core::{CellRef, RangeRef};
use serde_json::{Map, Value as Json, json};

use crate::SheetApp;
use crate::theme;

mod about;
mod analysis;
mod charts;
mod command_search;
mod cond_format_rules;
mod data;
mod find;
mod format_cells;
mod insert_function;
mod names;
mod paste;
mod show;
mod spelling;
mod zoom;

use analysis::toolpak_defaults;
use charts::select_chart;
use data::{abs, data_validation_fields, remove_duplicates_range, validation_defaults};
use find::go_to_entries;
pub(crate) use names::insert_name;
use names::pasteable_names;
pub(crate) use show::build_params;
pub use show::show;
use zoom::zoom_preset;

#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Text { key: &'static str, label: &'static str },
    Password { key: &'static str, label: &'static str },
    Number { key: &'static str, label: &'static str },
    Choice { key: &'static str, label: &'static str, options: Vec<(&'static str, &'static str)> },
    Check { key: &'static str, label: &'static str },
    Note(String),
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub name: String,
    pub title: String,
    pub fields: Vec<Field>,
    pub values: Map<String, Json>,
    /// Command to run on OK (params = values, transformed by `build`).
    pub command: Option<&'static str>,
    pub tab: String,
    pub search: String,
    pub list_index: usize,
    pub result: Option<Json>,
    /// "Show password" is ticked: password fields show their text.
    pub show_password: bool,
    /// The first text or number field still has to take keyboard focus (set when a form opens,
    /// used once): like Excel, Column Width opens ready to type, its value selected.
    focus_first_field: bool,
    picture_inbox: Option<crate::Inbox>,
    picture_target: Option<(u64, usize)>,
    /// The dialog this one was opened from (Name Manager › New…): it comes back when this closes.
    back: Option<Box<Dialog>>,
    focus_initialized: bool,
    /// The formula box that takes references clicked or dragged on the grid (New Formatting Rule).
    pick: Option<RefPick>,
    /// The linked cell's initial display text; unchanged text must not replace a number or formula.
    original_link_text: Option<String>,
}

/// A formula box in point mode, as in Excel: a click on the grid writes the cell's absolute
/// reference at the caret instead of selecting it, and a drag grows it into a range.
#[derive(Clone, Debug)]
struct RefPick {
    key: &'static str,
    /// The sheet the dialog was opened on, by name: its index changes when sheets are inserted or
    /// moved while the dialog is open. References to another sheet carry that sheet's name.
    sheet: String,
    /// Caret (char index), mirrored from the text box while it has the keyboard.
    caret: usize,
    /// The reference last inserted (byte span), replaced by the next click or the drag.
    point: Option<(usize, usize)>,
    anchor: CellRef,
    /// Give the box the keyboard back, caret after the reference, on the next frame.
    refocus: bool,
    /// The selection the rule applies to, fixed when the dialog opened as Excel's "Applies to":
    /// pointing on another sheet must not move the rule there.
    applies_to: String,
}

impl Dialog {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets a form value. In Zoom, a Magnification choice is also the percentage `view.zoom`
    /// stores, a typed percentage selects Custom, and Fit selection makes OK run
    /// `view.zoomToSelection` (its view size was captured when the dialog opened).
    pub(crate) fn set_value(&mut self, key: &str, value: Json) {
        if self.name == "zoom" {
            match key {
                "preset" => match value.as_str() {
                    Some("fit") => self.command = Some("view.zoomToSelection"),
                    Some(s) => {
                        self.command = Some("view.zoom");
                        if let Ok(p) = s.parse::<f64>() {
                            self.values.insert("percent".into(), json!(p));
                        }
                    }
                    None => {}
                },
                "percent" => {
                    self.command = Some("view.zoom");
                    self.values.insert("preset".into(), json!(zoom_preset(value.as_f64().unwrap_or(0.0))));
                }
                _ => {}
            }
        }
        self.values.insert(key.into(), value);
    }

    /// Whether a click on the grid goes to a formula box of this dialog rather than the selection.
    pub fn picks_references(&self) -> bool {
        self.pick.is_some()
    }

    /// A click (`extend` false) or a drag reaching `cell`: writes the absolute reference at the
    /// formula box's caret, replacing the reference the press inserted, as in the formula bar's
    /// point mode. An empty box starts with `=`. After an operand there is no place for a
    /// reference, so nothing changes. `active_sheet` names the sheet shown when it is not the
    /// dialog's own.
    pub fn pick_reference(&mut self, cell: CellRef, active_sheet: &str, extend: bool) {
        let Some(pick) = self.pick.as_mut() else { return };
        let text = self.values.get(pick.key).and_then(Json::as_str).unwrap_or("").to_string();
        let mut ed = crate::editor::EditState::new(0, 0, cell, text, true, false);
        ed.caret = pick.caret.min(ed.text.chars().count());
        ed.point = pick.point;
        if ed.text.trim().is_empty() {
            ed.text = "=".into();
            ed.caret = 1;
            ed.point = None;
        }
        if !ed.can_point() {
            return;
        }
        if !extend {
            pick.anchor = cell;
        }
        let range = abs(&RangeRef::new(pick.anchor, cell));
        let reference = if active_sheet.eq_ignore_ascii_case(&pick.sheet) {
            range
        } else {
            format!("{}!{range}", gridcraft_engine::formula::quote_sheet(active_sheet))
        };
        ed.insert_ref(&reference);
        pick.caret = ed.caret;
        pick.point = ed.point;
        pick.refocus = true;
        self.values.insert(pick.key.into(), json!(ed.text));
    }

    /// Creates a command-backed form with its initial field values.
    fn form(name: &str, title: &str, command: &'static str, fields: Vec<Field>, defaults: Json) -> Dialog {
        Dialog {
            name: name.into(),
            title: title.into(),
            fields,
            values: defaults.as_object().cloned().unwrap_or_default(),
            command: Some(command),
            tab: String::new(),
            search: String::new(),
            list_index: 0,
            result: None,
            show_password: false,
            focus_first_field: true,
            picture_inbox: None,
            picture_target: None,
            back: None,
            focus_initialized: false,
            pick: None,
            original_link_text: None,
        }
    }

    /// Creates a dialog whose body handles its own controls instead of a standard form.
    pub(crate) fn custom(name: &str, title: &str, defaults: Json) -> Dialog {
        Dialog {
            name: name.into(),
            title: title.into(),
            fields: vec![],
            values: defaults.as_object().cloned().unwrap_or_default(),
            command: None,
            tab: String::new(),
            search: String::new(),
            list_index: 0,
            result: None,
            show_password: false,
            focus_first_field: false,
            picture_inbox: None,
            picture_target: None,
            back: None,
            focus_initialized: false,
            pick: None,
            original_link_text: None,
        }
    }

    /// Opens the named dialog with defaults from the active workbook, including a cell's stored hyperlink.
    pub fn open(app: &mut SheetApp, name: &str, params: Json) -> Option<Dialog> {
        let sel = app.session.active().map(|d| d.selection.current().a1()).unwrap_or_default();
        let active = app.session.active().map(|d| d.selection.active.a1()).unwrap_or_default();
        let p = |k: &str| params.get(k).cloned().unwrap_or(Json::Null);
        use Field::*;
        let d = match name {
            "insertCells" => Dialog::form(
                "insertCells",
                "Insert",
                "home.insertCells",
                vec![Choice {
                    key: "shift",
                    label: "Insert",
                    options: vec![("right", "Shift cells right"), ("down", "Shift cells down"), ("row", "Entire row"), ("column", "Entire column")],
                }],
                json!({"shift": "down"}),
            ),
            "arrangeAll" => Dialog::form(
                "arrangeAll",
                "Arrange Windows",
                "view.arrangeAll",
                vec![
                    Choice {
                        key: "arrange",
                        label: "Arrange",
                        options: vec![("tiled", "Tiled"), ("horizontal", "Horizontal"), ("vertical", "Vertical"), ("cascade", "Cascade")],
                    },
                    Check { key: "activeWorkbook", label: "Windows of active workbook" },
                ],
                json!({"arrange": "tiled", "activeWorkbook": false}),
            ),
            "insertPaste" => Dialog::form(
                "insertPaste",
                "Insert Paste",
                "home.insertCopiedCells",
                vec![Choice { key: "shift", label: "Insert", options: vec![("right", "Shift cells right"), ("down", "Shift cells down")] }],
                json!({"shift": "down"}),
            ),
            "deleteCells" => Dialog::form(
                "deleteCells",
                "Delete",
                "home.deleteCells",
                vec![Choice {
                    key: "shift",
                    label: "Delete",
                    options: vec![("left", "Shift cells left"), ("up", "Shift cells up"), ("row", "Entire row"), ("column", "Entire column")],
                }],
                json!({"shift": "up"}),
            ),
            "rowHeight" => Dialog::form(
                "rowHeight",
                "Row Height",
                "home.rowHeight",
                vec![Number { key: "points", label: "Row height (points):" }],
                json!({"points": app.session.active().and_then(|d| d.wb.active().map(|s| s.row_height(d.selection.active.row) as f64 * 0.75)).unwrap_or(15.0)}),
            ),
            "columnWidth" => Dialog::form(
                "columnWidth",
                "Column Width",
                "home.columnWidth",
                vec![Number { key: "chars", label: "Column width (characters):" }],
                json!({"chars": app.session.active().and_then(|d| d.wb.active().map(|s| gridcraft_engine::cmd::format::points_to_chars(s.col_width(d.selection.active.col) as f64))).unwrap_or(8.43)}),
            ),
            "defaultWidth" => Dialog::form(
                "defaultWidth",
                "Standard Width",
                "home.defaultWidth",
                vec![Number { key: "width", label: "Standard column width (pixels):" }],
                json!({"width": 64}),
            ),
            "renameSheet" => Dialog::form(
                "renameSheet",
                "Rename Sheet",
                "sheet.rename",
                vec![Text { key: "name", label: "Name:" }],
                json!({"name": app.session.active().and_then(|d| d.wb.active().map(|s| s.name.clone()))}),
            ),
            "moveSheet" => {
                let sheets: Vec<String> = app.session.active().map(|d| d.wb.sheets.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
                let mut d = Dialog::form(
                    "moveSheet",
                    "Move or Copy",
                    "sheet.move",
                    vec![
                        Number { key: "to", label: "Before sheet (position, 0 = first):" },
                        Check { key: "copy", label: "Create a copy" },
                        Note(format!("Sheets: {}", sheets.join(", "))),
                    ],
                    json!({"to": 0, "copy": false}),
                );
                if let Some(s) = params.get("sheet") {
                    d.values.insert("sheet".into(), s.clone());
                }
                d
            }
            "protectSheet" => Dialog::form(
                "protectSheet",
                "Protect Sheet",
                "review.protectSheet",
                vec![
                    Password { key: "password", label: "Password (optional):" },
                    Note("Allow all users of this sheet to:".into()),
                    Check { key: "selectLockedCells", label: "Select locked cells" },
                    Check { key: "selectUnlockedCells", label: "Select unlocked cells" },
                    Check { key: "formatCells", label: "Format cells" },
                    Check { key: "formatColumns", label: "Format columns" },
                    Check { key: "formatRows", label: "Format rows" },
                    Check { key: "insertColumns", label: "Insert columns" },
                    Check { key: "insertRows", label: "Insert rows" },
                    Check { key: "deleteColumns", label: "Delete columns" },
                    Check { key: "deleteRows", label: "Delete rows" },
                    Check { key: "sort", label: "Sort" },
                    Check { key: "autofilter", label: "Use AutoFilter" },
                ],
                json!({"selectLockedCells": true, "selectUnlockedCells": true}),
            ),
            "allowEditRanges" => {
                let list = app.session.run("review.allowEditRanges", json!({})).unwrap_or(Json::Null);
                let rows: Vec<String> = list
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|r| {
                                let lock = match (r["password"].as_bool(), r["unlocked"].as_bool()) {
                                    (Some(true), Some(true)) => " (password, unlocked)",
                                    (Some(true), _) => " (password)",
                                    _ => "",
                                };
                                format!("{}: {}{lock}", r["title"].as_str().unwrap_or(""), r["range"].as_str().unwrap_or(""))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let summary = if rows.is_empty() { "No ranges yet.".to_string() } else { rows.join("\n") };
                Dialog::form(
                    "allowEditRanges",
                    "Allow Edit Ranges",
                    "review.allowEditRanges",
                    vec![
                        Note(format!("Ranges that stay editable when the sheet is protected:\n{summary}")),
                        Choice {
                            key: "action",
                            label: "Action",
                            options: vec![("add", "New"), ("modify", "Modify"), ("delete", "Delete"), ("unlock", "Unlock with password")],
                        },
                        Text { key: "title", label: "Title:" },
                        Text { key: "range", label: "Refers to cells:" },
                        Text { key: "password", label: "Range password:" },
                        Text { key: "newTitle", label: "New title (Modify):" },
                        Check { key: "removePassword", label: "Remove the password (Modify)" },
                    ],
                    json!({"action": "add", "range": sel}),
                )
            }
            "protectWorkbook" => Dialog::form(
                "protectWorkbook",
                "Protect Structure and Windows",
                "review.protectWorkbook",
                vec![
                    Password { key: "password", label: "Password (optional):" },
                    Note("Protect workbook for: Structure (sheets can't be added, moved, renamed, hidden or deleted).".into()),
                ],
                json!({"on": true}),
            ),
            "unprotectWorkbook" => Dialog::form(
                "unprotectWorkbook",
                "Unprotect Workbook",
                "review.protectWorkbook",
                vec![Password { key: "password", label: "Password:" }],
                json!({"on": false}),
            ),
            "openPassword" => {
                // The file rides along in the values (`path` on the desktop, `name` and `base64`
                // on the web) and goes back to `file.open` with the password.
                let source = p("path").as_str().or(p("name").as_str()).map(str::to_string).unwrap_or_default();
                let file = std::path::Path::new(&source).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("This workbook").to_string();
                // An empty password from the start, so OK on the untouched field says "incorrect"
                // rather than asking for the password again.
                let mut values = params.clone();
                values["password"] = json!("");
                Dialog::form(
                    "openPassword",
                    "Password",
                    "file.open",
                    vec![Note(format!("'{file}' is protected.")), Password { key: "password", label: "Password:" }],
                    values,
                )
            }
            "unprotectSheet" => Dialog::form(
                "unprotectSheet",
                "Unprotect Sheet",
                "review.unprotectSheet",
                vec![Password { key: "password", label: "Password:" }],
                json!({}),
            ),
            "unhideSheet" => {
                let opts: Vec<String> =
                    p("sheets").as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                let first = opts.first().cloned().unwrap_or_default();
                let mut d = Dialog::form(
                    "unhideSheet",
                    "Unhide",
                    "sheet.unhide",
                    vec![Text { key: "sheet", label: "Unhide sheet:" }, Note(format!("Hidden: {}", opts.join(", ")))],
                    json!({"sheet": first}),
                );
                d.values.insert("sheet".into(), json!(first));
                d
            }
            "pasteSpecial" => Dialog::form(
                "pasteSpecial",
                "Paste Special",
                "edit.pasteSpecial",
                vec![
                    Choice {
                        key: "what",
                        label: "Paste",
                        options: vec![
                            ("all", "All"),
                            ("formulas", "Formulas (F)"),
                            ("values", "Values (V)"),
                            ("formats", "Formats (T)"),
                            ("comments", "Comments and notes (C)"),
                            ("validation", "Validation"),
                            ("allExceptBorders", "All except borders"),
                            ("columnWidths", "Column widths"),
                            ("formulasAndNumberFormats", "Formulas and number formats"),
                            ("valuesAndNumberFormats", "Values and number formats"),
                        ],
                    },
                    Choice {
                        key: "operation",
                        label: "Operation",
                        options: vec![("none", "None"), ("add", "Add"), ("subtract", "Subtract"), ("multiply", "Multiply"), ("divide", "Divide")],
                    },
                    Check { key: "skipBlanks", label: "Skip blanks" },
                    Check { key: "transpose", label: "Transpose (E)" },
                    Check { key: "link", label: "Paste Link" },
                ],
                json!({"what": "all", "operation": "none"}),
            ),
            "series" => Dialog::form(
                "series",
                "Series",
                "edit.fillSeries",
                vec![
                    Choice { key: "direction", label: "Series in", options: vec![("columns", "Columns"), ("rows", "Rows")] },
                    Choice { key: "type", label: "Type", options: vec![("linear", "Linear"), ("growth", "Growth"), ("date", "Date")] },
                    Choice {
                        key: "dateUnit",
                        label: "Date unit",
                        options: vec![("day", "Day"), ("weekday", "Weekday"), ("month", "Month"), ("year", "Year")],
                    },
                    Number { key: "step", label: "Step value:" },
                    Text { key: "stop", label: "Stop value:" },
                ],
                json!({"direction": if p("direction") == "rows" { "rows" } else { "columns" }, "type": "linear", "dateUnit": "day", "step": 1}),
            ),
            "comment" => {
                let threaded = p("threaded").as_bool().unwrap_or(true);
                let existing = app
                    .session
                    .active()
                    .and_then(|d| d.wb.active().and_then(|s| s.comments.get(&d.selection.active).map(|c| c.text.clone())))
                    .unwrap_or_default();
                Dialog::form(
                    if threaded { "comment" } else { "note" },
                    if threaded { "New Comment" } else { "New Note" },
                    if threaded { "review.newComment" } else { "review.newNote" },
                    vec![Text { key: "text", label: if threaded { "Start a conversation:" } else { "Note:" } }],
                    json!({"text": existing}),
                )
            }
            // Asks before a web or e-mail link leaves the workbook; OK runs `link.open`, which
            // refuses anything but http, https and mailto.
            "openLink" => {
                let url = params.get("url").and_then(Json::as_str).unwrap_or_default().to_string();
                Dialog::form(
                    "openLink",
                    "Open Hyperlink",
                    "link.open",
                    vec![Note(format!(
                        "This workbook asks to open the address below. Links can lead to harmful sites or start other programs: open it only if you trust the workbook.\n\n{url}"
                    ))],
                    json!({"url": url}),
                )
            }
            "insertLink" => {
                let mut d = Dialog::form(
                    "insertLink",
                    "Insert Hyperlink",
                    "insert.link",
                    vec![
                        Text { key: "text", label: "Display:" },
                        Text { key: "target", label: "Link to (web address, e-mail or Sheet!A1):" },
                        Text { key: "tooltip", label: "ScreenTip:" },
                    ],
                    json!({"target": "https://"}),
                );
                if let Some(doc) = app.session.active()
                    && let Some(sheet) = doc.wb.active()
                    && let Some(link) = sheet.hyperlinks.get(&doc.selection.active)
                {
                    let text = gridcraft_engine::display::cell_text(&doc.wb, sheet, doc.selection.active);
                    d.values.insert("target".into(), json!(link.target));
                    d.values.insert("text".into(), json!(text));
                    d.values.insert("tooltip".into(), json!(link.tooltip));
                    d.original_link_text = Some(text);
                }
                d
            }
            "sparkline" => Dialog::form(
                "sparkline",
                "Create Sparklines",
                "insert.sparkline",
                vec![
                    Text { key: "range", label: "Data Range:" },
                    Text { key: "location", label: "Location Range:" },
                    Check { key: "markers", label: "Markers" },
                ],
                json!({"range": sel, "location": active, "type": p("type")}),
            ),
            "headerFooter" => Dialog::form(
                "headerFooter",
                "Header & Footer",
                "pageLayout.headerFooter",
                vec![
                    Text { key: "header", label: "Header (&L left, &C center, &R right; &P page, &N pages, &D date, &A sheet):" },
                    Text { key: "footer", label: "Footer:" },
                ],
                {
                    let sheet = app.session.active().and_then(|d| d.wb.active());
                    let (header, footer) = sheet.map(|s| (s.print.header.clone(), s.print.footer.clone())).unwrap_or_default();
                    json!({"header": header, "footer": footer})
                },
            ),
            "pageSetup" => {
                // Opens on the active sheet's saved margins, so OK without edits rewrites them unchanged.
                let m = app.session.active().and_then(|d| d.wb.active()).map(|s| s.print.margins).unwrap_or([0.7, 0.7, 0.75, 0.75, 0.3, 0.3]);
                // Round off f32 noise so the form shows 0.1, not 0.10000000149.
                let [left, right, top, bottom, header, footer] = m.map(|v| (f64::from(v) * 1e4).round() / 1e4);
                Dialog::form(
                    "pageSetup",
                    "Page Setup",
                    "pageLayout.margins",
                    vec![
                        Number { key: "left", label: "Left margin (in):" },
                        Number { key: "right", label: "Right margin (in):" },
                        Number { key: "top", label: "Top margin (in):" },
                        Number { key: "bottom", label: "Bottom margin (in):" },
                        Number { key: "header", label: "Header (in):" },
                        Number { key: "footer", label: "Footer (in):" },
                    ],
                    json!({"left": left, "right": right, "top": top, "bottom": bottom, "header": header, "footer": footer}),
                )
            }
            "textToColumns" => Dialog::form(
                "textToColumns",
                "Convert Text to Columns",
                "data.textToColumns",
                vec![
                    Check { key: "tab", label: "Tab" },
                    Check { key: "semicolon", label: "Semicolon" },
                    Check { key: "comma", label: "Comma" },
                    Check { key: "space", label: "Space" },
                    Text { key: "other", label: "Other:" },
                    Check { key: "treatConsecutive", label: "Treat consecutive delimiters as one" },
                    Text { key: "destination", label: "Destination:" },
                ],
                json!({"tab": true, "comma": true, "destination": active}),
            ),
            "removeDuplicates" => Dialog::form(
                "removeDuplicates",
                "Remove Duplicates",
                "data.removeDuplicates",
                vec![
                    Text { key: "range", label: "Range:" },
                    Check { key: "header", label: "My data has headers" },
                    Text { key: "columnsText", label: "Columns (blank = all, e.g. A,C):" },
                ],
                json!({"range": remove_duplicates_range(app, &sel), "header": true}),
            ),
            "advancedFilter" => {
                let list = app
                    .session
                    .active()
                    .and_then(|d| d.wb.active().map(|sh| gridcraft_engine::selection::current_region(sh, d.selection.active).a1()))
                    .unwrap_or(sel.clone());
                let mut d = Dialog::form(
                    "advancedFilter",
                    "Advanced Filter",
                    "data.advancedFilter",
                    vec![],
                    json!({"action": "inPlace", "range": list, "criteria": "", "copyTo": "", "unique": false}),
                );
                for key in ["action", "range", "criteria", "copyTo", "unique"] {
                    if let Some(value) = params.get(key) {
                        d.set_value(key, value.clone());
                    }
                }
                d
            }
            "goalSeek" => Dialog::custom("goalSeek", "Goal Seek", json!({"set": active, "to": "0", "changing": ""})),
            "forecastSheet" => Dialog::form(
                "forecastSheet",
                "Create Forecast Worksheet",
                "data.forecastSheet",
                vec![
                    Text { key: "timeline", label: "Timeline range (blank: first column of the selection):" },
                    Text { key: "values", label: "Values range (blank: second column of the selection):" },
                    Text { key: "end", label: "Forecast end (date or number; blank: a third of the history):" },
                    Text { key: "forecastStart", label: "Forecast start (a date or number in the history; blank: after the last point):" },
                    Check { key: "confidenceInterval", label: "Confidence interval" },
                    Text { key: "confidence", label: "Confidence level (%):" },
                    Text { key: "seasonality", label: "Seasonality (auto, 0 for none, or a season length):" },
                    Choice {
                        key: "fillMissing",
                        label: "Fill missing points using",
                        options: vec![("interpolate", "Interpolation"), ("zeros", "Zeros")],
                    },
                    Choice {
                        key: "aggregate",
                        label: "Aggregate duplicates using",
                        options: vec![
                            ("average", "Average"),
                            ("count", "Count"),
                            ("counta", "CountA"),
                            ("max", "Max"),
                            ("median", "Median"),
                            ("min", "Min"),
                            ("sum", "Sum"),
                        ],
                    },
                    Check { key: "statistics", label: "Include forecast statistics" },
                    Choice { key: "chart", label: "Chart", options: vec![("line", "Line chart"), ("column", "Column chart")] },
                ],
                json!({"confidenceInterval": true, "confidence": "95", "seasonality": "auto", "fillMissing": "interpolate", "aggregate": "average", "chart": "line"}),
            ),
            "analysisToolPak" => {
                // Opened with a known `tool`, it goes straight to that tool's form.
                let tool = params.get("tool").and_then(Json::as_str).unwrap_or("");
                let mut d = Dialog::custom("analysisToolPak", "Data Analysis", json!({"tool": "", "pick": tool}));
                if gridcraft_engine::cmd::analysis::TOOLS.iter().any(|(id, _)| *id == tool) {
                    d.values = toolpak_defaults(&sel, tool);
                }
                d.command = Some("data.analysisToolPak");
                d
            }
            "solver" => {
                // Starts from the sheet's saved model, else the active cell as objective.
                let saved = app.session.active().and_then(|doc| gridcraft_engine::cmd::solver::saved_model(&doc.wb, doc.wb.active_sheet()));
                let mut values = json!({"objective": active, "goal": "max", "value": "0", "variables": "", "constraints": [], "nonNegative": true, "method": "grg"});
                if let (Some(m), Some(v)) = (saved.as_ref().and_then(Json::as_object), values.as_object_mut()) {
                    for (k, x) in m {
                        v.insert(k.clone(), if k == "value" { json!(x.to_string()) } else { x.clone() });
                    }
                }
                if let Some(v) = values.as_object_mut() {
                    v.insert("newOp".into(), json!("<="));
                }
                let mut d = Dialog::custom("solver", "Solver Parameters", values);
                d.command = Some("data.solver");
                d
            }
            "solverResults" => Dialog::custom("solverResults", "Solver Results", params.clone()),
            "subtotal" => Dialog::form(
                "subtotal",
                "Subtotal",
                "data.subtotal",
                vec![
                    Text { key: "groupBy", label: "At each change in (column letter):" },
                    Choice {
                        key: "function",
                        label: "Use function",
                        options: vec![
                            ("sum", "Sum"),
                            ("count", "Count"),
                            ("average", "Average"),
                            ("max", "Max"),
                            ("min", "Min"),
                            ("product", "Product"),
                        ],
                    },
                    Text { key: "columnsText", label: "Add subtotal to (columns, e.g. C,D):" },
                ],
                json!({"groupBy": "A", "function": "sum"}),
            ),
            "outlineSettings" => {
                let (below, right) =
                    app.session.active().and_then(|d| d.wb.active().map(|s| (s.summary_below, s.summary_right))).unwrap_or((true, true));
                Dialog::form(
                    "outlineSettings",
                    "Settings",
                    "data.outlineSettings",
                    vec![
                        Check { key: "summaryBelow", label: "Summary rows below detail" },
                        Check { key: "summaryRight", label: "Summary columns to right of detail" },
                    ],
                    json!({"summaryBelow": below, "summaryRight": right}),
                )
            }
            "zoom" => {
                // The current zoom is pre-selected (Custom when it matches no preset). Fit
                // selection needs the grid's size, so it is captured here for `view.zoomToSelection`.
                let zoom = app.session.active().and_then(|d| d.wb.active().map(|s| s.zoom)).unwrap_or(100);
                let view = app.grid.cells_rect.unwrap_or(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1200.0, 700.0)));
                Dialog::form(
                    "zoom",
                    "Zoom",
                    "view.zoom",
                    vec![],
                    json!({"percent": zoom, "preset": zoom_preset(zoom as f64), "viewWidth": view.width(), "viewHeight": view.height()}),
                )
            }
            "defineName" => {
                // Name Manager › Edit… passes the name being edited (`oldName`, `oldScope`) and
                // its current fields; New… and Define Name start from the selection.
                let editing = params.get("oldName").and_then(Json::as_str).is_some();
                let selection = app.session.active().and_then(|d| {
                    d.wb.active().map(|s| format!("={}!{}", gridcraft_engine::formula::quote_sheet(&s.name), abs(&d.selection.current())))
                });
                let mut defaults = json!({"scope": "Workbook", "refersTo": selection});
                for key in ["name", "scope", "comment", "refersTo", "oldName", "oldScope"] {
                    if let Some(v) = params.get(key).filter(|v| v.is_string()) {
                        defaults[key] = v.clone();
                    }
                }
                Dialog::form(
                    "defineName",
                    if editing { "Edit Name" } else { "New Name" },
                    "formulas.defineName",
                    vec![
                        Text { key: "name", label: "Name:" },
                        Text { key: "scope", label: "Scope (Workbook or sheet name):" },
                        Text { key: "comment", label: "Comment:" },
                        Text { key: "refersTo", label: "Refers to:" },
                    ],
                    defaults,
                )
            }
            "tableResize" => {
                let (table, range) = app
                    .session
                    .active()
                    .and_then(|d| {
                        let sh = d.wb.active()?;
                        let t = sh.table_at(d.selection.active)?;
                        Some((t.name.clone(), t.range.a1()))
                    })
                    .unwrap_or_default();
                Dialog::form(
                    "tableResize",
                    "Resize Table",
                    "table.resize",
                    vec![Text { key: "range", label: "New range for the table:" }],
                    json!({"table": table, "range": range}),
                )
            }
            "dataValidation" => Dialog::form(
                "dataValidation",
                "Data Validation",
                "data.validation",
                data_validation_fields("any", "between"),
                validation_defaults(app),
            ),
            "cfQuick" => {
                let ty = p("type").as_str().unwrap_or("cellIs").to_string();
                let title = p("title").as_str().unwrap_or("Conditional Formatting").to_string();
                let mut fields = vec![];
                match ty.as_str() {
                    "cellIs" => {
                        fields.push(Text { key: "value", label: "Format cells that are (value):" });
                        if p("operator").as_str() == Some("between") {
                            fields.push(Text { key: "value2", label: "and:" });
                        }
                    }
                    "containsText" => fields.push(Text { key: "text", label: "Format cells that contain the text:" }),
                    "timePeriod" => fields.push(Choice {
                        key: "period",
                        label: "Format cells that contain a date occurring:",
                        options: vec![
                            ("yesterday", "Yesterday"),
                            ("today", "Today"),
                            ("tomorrow", "Tomorrow"),
                            ("last7Days", "In the last 7 days"),
                            ("lastWeek", "Last week"),
                            ("thisWeek", "This week"),
                            ("nextWeek", "Next week"),
                            ("lastMonth", "Last month"),
                            ("thisMonth", "This month"),
                            ("nextMonth", "Next month"),
                        ],
                    }),
                    _ => fields.push(Text { key: "formula", label: "Format values where this formula is true:" }),
                }
                fields.push(Choice {
                    key: "preset",
                    label: "with",
                    options: vec![
                        ("lightRedFillDarkRedText", "Light Red Fill with Dark Red Text"),
                        ("yellowFillDarkYellowText", "Yellow Fill with Dark Yellow Text"),
                        ("greenFillDarkGreenText", "Green Fill with Dark Green Text"),
                        ("lightRedFill", "Light Red Fill"),
                        ("redText", "Red Text"),
                        ("redBorder", "Red Border"),
                    ],
                });
                let mut d = Dialog::form(
                    "cfQuick",
                    &title,
                    "home.conditionalFormat",
                    fields,
                    json!({"preset": "lightRedFillDarkRedText", "period": "today"}),
                );
                if ty == "expression" {
                    let (sheet, applies_to) = app
                        .session
                        .active()
                        .map(|doc| {
                            (
                                doc.wb.active().map(|sh| sh.name.clone()).unwrap_or_default(),
                                doc.selection.ranges.iter().map(RangeRef::a1).collect::<Vec<_>>().join(","),
                            )
                        })
                        .unwrap_or_default();
                    d.pick = Some(RefPick { key: "formula", sheet, caret: 0, point: None, anchor: CellRef::new(0, 0), refocus: false, applies_to });
                }
                d.values.insert("type".into(), json!(ty));
                d.values.insert("operator".into(), p("operator"));
                d
            }
            "formatCells" => {
                let mut d = Dialog::custom("formatCells", "Format Cells", json!({}));
                d.tab = p("tab").as_str().unwrap_or("Number").to_string();
                if let Ok(st) = app.session.run("cell.get", json!({})) {
                    d.values.insert("style".into(), st["style"].clone());
                }
                d
            }
            "insertFunction" => Dialog::custom("insertFunction", "Formula Builder", json!({"category": "Most Recently Used"})),
            "find" => {
                let mut d = Dialog::custom("find", "Find and Replace", json!({"what": "", "with": "", "within": "sheet", "lookIn": "formulas"}));
                d.tab = if p("replace").as_bool() == Some(true) { "Replace".into() } else { "Find".into() };
                d
            }
            "goTo" => {
                let mut dialog = Dialog::custom("goTo", "Go To", json!({"reference": ""}));
                // Listed once, when the dialog opens.
                dialog.result = Some(json!(go_to_entries(app)));
                dialog
            }
            "pivotCalculatedField" => Dialog::form(
                "pivotCalculatedField",
                "Insert Calculated Field",
                "pivot.calculatedField",
                vec![Text { key: "name", label: "Name:" }, Text { key: "formula", label: "Formula:" }],
                json!({"pivot": p("pivot"), "name": p("name"), "formula": p("formula")}),
            ),
            "goToSpecial" => Dialog::form(
                "goToSpecial",
                "Go To Special",
                "edit.goToSpecial",
                vec![Choice {
                    key: "kind",
                    label: "Select",
                    options: vec![
                        ("blanks", "Blanks"),
                        ("constants", "Constants"),
                        ("formulas", "Formulas"),
                        ("notes", "Notes"),
                        ("errors", "Errors"),
                        ("numbers", "Numbers"),
                        ("text", "Text"),
                        ("currentRegion", "Current region"),
                        ("lastCell", "Last cell"),
                        ("visible", "Visible cells only"),
                        ("conditionalFormats", "Conditional formats"),
                        ("dataValidation", "Data validation"),
                    ],
                }],
                json!({"kind": "blanks"}),
            ),
            "sort" => {
                // Seed "Sort by" from the column the user is actually in (#1906).
                let col = app.session.active().map(|d| gridcraft_engine::core::col_to_letters(d.selection.active.col)).unwrap_or_else(|| "A".into());
                Dialog::custom("sort", "Sort", json!({"header": true, "levels": [{"column": col, "order": "asc"}]}))
            }
            "nameManager" => Dialog::custom("nameManager", "Name Manager", json!({})),
            "pasteName" => {
                // F3 opens it only when the workbook has names to paste, as in Excel.
                let names = pasteable_names(app);
                if p("shortcut") == json!(true) && names.is_empty() {
                    return None;
                }
                // OK runs `formulas.pasteName` with the selected `name`, so `ui.dialog.confirm` does too.
                let mut dialog = Dialog::custom("pasteName", "Paste Name", json!({"name": names.first()}));
                dialog.command = Some("formulas.pasteName");
                // Listed once, when the dialog opens.
                dialog.result = Some(json!(names));
                dialog
            }
            "manageRules" => Dialog::custom("manageRules", "Conditional Formatting Rules Manager", json!({})),
            "commandSearch" => Dialog::custom("commandSearch", "Search Commands", json!({})),
            "agents" => Dialog::custom("agents", "Agent Control", json!({})),
            "about" => {
                let mut d = Dialog::custom("about", "About GridCraft", json!({}));
                d.tab = match p("tab").as_str() {
                    Some(t @ ("Contributors" | "Models")) => t.to_string(),
                    _ => "About".into(),
                };
                d
            }
            "recommendedCharts" => {
                // Insert › Recommended Charts: chart types for the selected data, with previews.
                let recs = match app.session.run("insert.chartRecommendations", json!({})) {
                    Ok(r) => r,
                    Err(e) => {
                        app.message = Some(("Insert Chart".into(), crate::clean_error(&e)));
                        return None;
                    }
                };
                let mut d = Dialog::custom("recommendedCharts", "Insert Chart", json!({}));
                d.tab = if p("tab").as_str() == Some("All Charts") { "All Charts".into() } else { "Recommended Charts".into() };
                d.result = Some(recs);
                select_chart(&mut d, 0);
                d
            }
            "selectData" => {
                // Chart Design › Select Data: data range, series to keep, category axis labels.
                let id = p("chart").as_u64();
                let found = app.session.active().and_then(|doc| {
                    let si = doc.wb.active_sheet();
                    let sh = doc.wb.sheet(si)?;
                    let ch = sh.charts.iter().find(|c| Some(u64::from(c.id)) == id)?;
                    let data = gridcraft_chart::resolve(&doc.wb, si, ch);
                    let names: Vec<String> = data.series.iter().map(|x| x.name.clone()).collect();
                    let cats = ch.series.first().and_then(|x| x.categories.clone()).unwrap_or_default();
                    Some((ch.source.clone().unwrap_or_default(), cats, names))
                });
                let Some((range, cats, names)) = found else {
                    app.message = Some(("Select Data".into(), "Select a chart first.".into()));
                    return None;
                };
                let eq = |t: &str| if t.is_empty() { String::new() } else { format!("={t}") };
                Dialog::custom(
                    "selectData",
                    "Select Data Source",
                    json!({"chart": id, "range": eq(&range), "range0": eq(&range), "categories": eq(&cats), "categories0": eq(&cats), "names": names, "remove": []}),
                )
            }
            "journal" => Dialog::custom("journal", "Action Journal", json!({})),
            "macros" => crate::macro_ui::open_dialog(app, &params),
            "recordMacro" => return crate::macro_ui::open_record(app, &params),
            "macroEditor" => return crate::macro_editor::open(app, &params),
            "statistics" => {
                let mut d = Dialog::custom("statistics", "Workbook Statistics", json!({}));
                d.result = app.session.run("review.workbookStatistics", json!({})).ok();
                d
            }
            "accessibility" => {
                let mut d = Dialog::custom("accessibility", "Accessibility", json!({}));
                d.result = app.session.run("review.checkAccessibility", json!({})).ok();
                d
            }
            "errorChecking" => {
                let mut d = Dialog::custom("errorChecking", "Error Checking", json!({}));
                d.result = app.session.run("formulas.errorChecking", json!({})).ok();
                d
            }
            "evaluateFormula" => {
                let mut d = Dialog::custom("evaluateFormula", "Evaluate Formula", json!({}));
                d.result = app.session.run("formulas.evaluateFormula", json!({})).ok();
                d
            }
            "spelling" => {
                let mut d = Dialog::custom("spelling", "Spelling", json!({}));
                d.result = Some(app.session.run("review.spelling", json!({})).unwrap_or_else(|e| json!({"error": e})));
                d
            }
            "chartTitle" => Dialog::form(
                "chartTitle",
                "Chart Title",
                "chart.set",
                vec![
                    Text { key: "title", label: "Title:" },
                    Text { key: "xTitle", label: "Horizontal axis title:" },
                    Text { key: "yTitle", label: "Vertical axis title:" },
                ],
                json!({"chart": p("chart")}),
            ),
            "insertPicture" => {
                let placement = params.get("placement").and_then(Json::as_str).unwrap_or("overCells");
                let mut dialog = Dialog::form(
                    "insertPicture",
                    if placement == "cell" { "Place Picture in Cell" } else { "Place Picture over Cells" },
                    "insert.picture",
                    vec![],
                    json!({"path": "", "placement": placement, "at": active}),
                );
                dialog.picture_target = app.view_key();
                dialog
            }
            "pickList" => Dialog::custom("pickList", "Pick From List", json!({})),
            "saveCopy" => Dialog::custom("saveCopy", "Save a Copy", json!({"format": p("format")})),
            "saveChanges" => Dialog::custom("saveChanges", "Save Changes?", json!({"title": p("title"), "quit": p("quit")})),
            "start" => Dialog::custom("start", "GridCraft", json!({})),
            "comments" => {
                app.grid.pane = None;
                app.run_or_alert("review.showComments", json!({"on": true}));
                return None;
            }
            "watch" | "selectionPane" | "formatChart" => {
                app.grid.pane = Some(
                    match name {
                        "watch" => "watch",
                        "selectionPane" => "selection",
                        _ => "formatChart",
                    }
                    .into(),
                );
                return None;
            }
            _ => return None,
        };
        Some(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_duplicates_prefills_the_selection_and_submits_the_edited_range() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        session.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
        let mut app = SheetApp::new(session, Default::default());
        let mut dialog = Dialog::open(&mut app, "removeDuplicates", json!({})).unwrap();
        assert_eq!(dialog.values.get("range"), Some(&json!("B2:C5")));
        assert!(dialog.fields.iter().any(|field| matches!(field, Field::Text { key: "range", .. })));
        assert_eq!(build_params(&dialog)["range"], "B2:C5");
        dialog.set_value("range", json!("D2:E6"));
        assert_eq!(build_params(&dialog)["range"], "D2:E6");
    }

    #[test]
    fn remove_duplicates_with_only_the_cursor_prefills_the_current_region() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        session.execute("range.setValues", json!({"range": "B2:C4", "values": [["a", 1], ["a", 1], ["b", 2]]})).unwrap();
        session.execute("selection.set", json!({"range": "C3"})).unwrap();
        let mut app = SheetApp::new(session, Default::default());
        let dialog = Dialog::open(&mut app, "removeDuplicates", json!({})).unwrap();
        assert_eq!(dialog.values.get("range"), Some(&json!("B2:C4")));
    }

    #[test]
    fn data_validation_opens_on_the_rule_of_the_active_cell() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        app.run("data.validation", json!({"range": "A1", "type": "whole", "operator": "greater", "formula1": "5"})).unwrap();
        let d = Dialog::open(&mut app, "dataValidation", json!({})).unwrap();
        assert_eq!(d.values["type"], "whole");
        assert_eq!(d.values["operator"], "greater");
        assert_eq!(d.values["formula1"], "5");
        // OK without edits keeps the rule: 6 is accepted, 4 is rejected.
        app.run(d.command.unwrap(), build_params(&d)).unwrap();
        let doc = app.session.active().unwrap();
        let at = gridcraft_engine::core::CellRef::new(0, 0);
        assert!(gridcraft_engine::cmd::data::check_validation(&doc.wb, 0, at, "6").is_none());
        assert!(gridcraft_engine::cmd::data::check_validation(&doc.wb, 0, at, "4").is_some());
    }

    #[test]
    fn data_validation_without_a_rule_opens_on_a_blank_list() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        let d = Dialog::open(&mut app, "dataValidation", json!({})).unwrap();
        assert_eq!(d.values["type"], "list");
        assert!(d.values.get("formula1").is_none());
    }
}
