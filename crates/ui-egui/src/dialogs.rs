//! Dialogs. Simple dialogs are forms (fields → a command with params); Format Cells, Insert
//! Function, Find & Replace, Sort, Name Manager and the command palette are custom.
//!
//! Every dialog is also reachable programmatically: `ui.dialog {name}` opens one,
//! `ui.dialog.set {field, value}` edits a field and `ui.dialog.confirm` presses OK.

use egui::{Color32, Key, Sense, Stroke, StrokeKind, vec2};
use gridcraft_engine::model::{Color as CellColor, Fill, GradientFill, GradientKind, GradientStop, PatternType, Theme};

use crate::icons::{self, Icon};
use serde_json::{Map, Value as Json, json};

use crate::SheetApp;
use crate::theme;

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
        }
    }

    fn custom(name: &str, title: &str, defaults: Json) -> Dialog {
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
        }
    }

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
                vec![Number { key: "height", label: "Row height (pixels):" }],
                json!({"height": app.session.active().and_then(|d| d.wb.active().map(|s| s.row_height(d.selection.active.row))).unwrap_or(20.0)}),
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
                let file = std::path::Path::new(&source).file_name().and_then(|n| n.to_str()).unwrap_or("This workbook").to_string();
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
            "insertLink" => Dialog::form(
                "insertLink",
                "Insert Hyperlink",
                "insert.link",
                vec![
                    Text { key: "text", label: "Display:" },
                    Text { key: "target", label: "Link to (web address, e-mail or Sheet!A1):" },
                    Text { key: "tooltip", label: "ScreenTip:" },
                ],
                json!({"target": "https://"}),
            ),
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
                let saved = app.session.active().and_then(|doc| gridcraft_engine::cmd::solver::saved_model(&doc.wb, doc.wb.active_sheet));
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
            "goTo" => Dialog::custom("goTo", "Go To", json!({"reference": ""})),
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
                    let si = doc.wb.active_sheet;
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
            "macros" => {
                let mut d = Dialog::custom("macros", "Macros", json!({}));
                d.result = app.session.run("view.macros", json!({})).ok();
                d
            }
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

/// Initial values of the Data Validation form: the rule stored on the active cell, so OK without
/// edits keeps it; a plain list rule when the cell has none.
fn validation_defaults(app: &SheetApp) -> Json {
    let mut v = json!({"type": "list", "operator": "between", "ignoreBlank": true, "dropdown": true, "errorStyle": "stop"});
    let Some(doc) = app.session.active() else { return v };
    let Some(dv) = doc.wb.active().and_then(|s| s.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(doc.selection.active)))) else {
        return v;
    };
    // The enums serialize as their variant names; the command takes them in camelCase.
    let camel = |x: Json| {
        let name = x.as_str().unwrap_or_default();
        let mut chars = name.chars();
        chars.next().map(|c| c.to_lowercase().chain(chars).collect::<String>()).unwrap_or_default()
    };
    v["type"] = json!(camel(json!(dv.kind)));
    v["operator"] = json!(camel(json!(dv.op)));
    v["errorStyle"] = json!(camel(json!(dv.error_style)));
    v["formula1"] = json!(dv.f1);
    if let Some(f2) = &dv.f2 {
        v["formula2"] = json!(f2);
    }
    v["ignoreBlank"] = json!(dv.allow_blank);
    v["dropdown"] = json!(dv.in_cell_dropdown);
    v["inputTitle"] = json!(dv.input_title);
    v["inputMessage"] = json!(dv.input_message);
    v["errorTitle"] = json!(dv.error_title);
    v["errorMessage"] = json!(dv.error_message);
    v
}

/// The range Remove Duplicates starts with. A block selection is used as given; with only the
/// cursor placed, the table's data rows or the current region, as the command picks when it gets
/// no range (a single-cell range would remove nothing).
fn remove_duplicates_range(app: &SheetApp, sel: &str) -> String {
    let Some(d) = app.session.active() else { return sel.to_string() };
    let Some(sh) = d.wb.active() else { return sel.to_string() };
    if !d.selection.current().is_single() {
        return sel.to_string();
    }
    let range = match sh.table_at(d.selection.active) {
        Some(t) => t.data_range().map(|dr| gridcraft_engine::core::RangeRef::new(t.range.start, dr.end)).unwrap_or(t.range),
        None => gridcraft_engine::selection::current_region(sh, d.selection.active),
    };
    range.a1()
}

fn abs(r: &gridcraft_engine::core::RangeRef) -> String {
    let a = |c: gridcraft_engine::core::CellRef| format!("${}${}", gridcraft_engine::core::col_to_letters(c.col), c.row + 1);
    if r.is_single() { a(r.start) } else { format!("{}:{}", a(r.start), a(r.end)) }
}

/// The criterion controls in Excel's Settings tab depend on both Allow and Data.
/// `formula1` is the sole source for a list or custom rule; `formula2` is a
/// second bound only for Between and Not between.
fn data_validation_fields(kind: &str, operator: &str) -> Vec<Field> {
    use Field::*;
    let mut fields = vec![Choice {
        key: "type",
        label: "Allow",
        options: vec![
            ("any", "Any value"),
            ("whole", "Whole number"),
            ("decimal", "Decimal"),
            ("list", "List"),
            ("date", "Date"),
            ("time", "Time"),
            ("textLength", "Text length"),
            ("custom", "Custom"),
        ],
    }];
    let two_bounds = matches!(operator, "between" | "notBetween");
    match kind {
        "list" => {
            fields.push(Text { key: "formula1", label: "Source:" });
            fields.push(Check { key: "ignoreBlank", label: "Ignore blank" });
            fields.push(Check { key: "dropdown", label: "In-cell dropdown" });
        }
        "custom" => {
            fields.push(Text { key: "formula1", label: "Formula:" });
            fields.push(Check { key: "ignoreBlank", label: "Ignore blank" });
        }
        "whole" | "decimal" | "date" | "time" | "textLength" => {
            fields.push(Choice {
                key: "operator",
                label: "Data",
                options: vec![
                    ("between", "between"),
                    ("notBetween", "not between"),
                    ("equal", "equal to"),
                    ("notEqual", "not equal to"),
                    ("greater", "greater than"),
                    ("less", "less than"),
                    ("greaterOrEqual", "greater than or equal to"),
                    ("lessOrEqual", "less than or equal to"),
                ],
            });
            let lower = matches!(operator, "greater" | "greaterOrEqual");
            let (first, second) = match kind {
                "date" => (
                    if two_bounds || lower {
                        "Start date:"
                    } else if matches!(operator, "less" | "lessOrEqual") {
                        "End date:"
                    } else {
                        "Date:"
                    },
                    "End date:",
                ),
                "time" => (
                    if two_bounds || lower {
                        "Start time:"
                    } else if matches!(operator, "less" | "lessOrEqual") {
                        "End time:"
                    } else {
                        "Time:"
                    },
                    "End time:",
                ),
                _ => (
                    if two_bounds || lower {
                        "Minimum:"
                    } else if matches!(operator, "less" | "lessOrEqual") {
                        "Maximum:"
                    } else {
                        "Value:"
                    },
                    "Maximum:",
                ),
            };
            fields.push(Text { key: "formula1", label: first });
            if two_bounds {
                fields.push(Text { key: "formula2", label: second });
            }
            fields.push(Check { key: "ignoreBlank", label: "Ignore blank" });
        }
        _ => {}
    }
    fields.push(Text { key: "inputMessage", label: "Input message:" });
    fields.push(Choice {
        key: "errorStyle",
        label: "Error style",
        options: vec![("stop", "Stop"), ("warning", "Warning"), ("information", "Information")],
    });
    fields.push(Text { key: "errorMessage", label: "Error message:" });
    fields
}

fn data_validation_form(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let kind = d.values.get("type").and_then(Json::as_str).unwrap_or("any");
    let operator = d.values.get("operator").and_then(Json::as_str).unwrap_or("between");
    d.fields = data_validation_fields(kind, operator);
    form_fields(ui, d);
    ok_cancel(ui, confirm, open);
}

fn advanced_filter_form(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    use Field::*;
    let copy = d.values.get("action").and_then(Json::as_str) == Some("copy");
    d.fields = vec![
        Choice {
            key: "action",
            label: "Action",
            options: vec![("inPlace", "Show matching rows in place"), ("copy", "Copy matching rows elsewhere")],
        },
        Text { key: "range", label: "List range:" },
        Text { key: "criteria", label: "Criteria range (optional):" },
    ];
    if copy {
        d.fields.push(Text { key: "copyTo", label: "Copy to:" });
    }
    d.fields.push(Check { key: "unique", label: "Unique rows only" });
    form_fields(ui, d);
    // Constrain the button row: the shared right-to-left layout otherwise
    // consumes the window's remaining height instead of fitting the buttons.
    ui.allocate_ui(vec2(ui.available_width(), ui.spacing().interact_size.y + 8.0), |ui| ok_cancel(ui, confirm, open));
}

/// Builds the command params from form values for dialogs that need translation.
/// The parameters OK sends to the dialog's command: its field values, reshaped per dialog.
pub(crate) fn build_params(d: &Dialog) -> Json {
    let mut v = Json::Object(d.values.clone());
    match d.name.as_str() {
        "advancedFilter" => {
            if let Some(m) = v.as_object_mut() {
                m.remove("action");
                if d.values.get("action").and_then(Json::as_str) != Some("copy") {
                    m.remove("copyTo");
                }
                if m.get("criteria").and_then(Json::as_str).is_some_and(|s| s.trim().is_empty()) {
                    m.remove("criteria");
                }
            }
        }
        "dataValidation" => {
            let kind = d.values.get("type").and_then(Json::as_str).unwrap_or("any");
            let operator = d.values.get("operator").and_then(Json::as_str).unwrap_or("between");
            if let Some(m) = v.as_object_mut() {
                if !matches!(operator, "between" | "notBetween") || matches!(kind, "any" | "list" | "custom") {
                    m.remove("formula2");
                }
                if matches!(kind, "any" | "list" | "custom") {
                    m.remove("operator");
                }
                if kind == "any" {
                    m.remove("formula1");
                }
                if kind != "list" {
                    m.remove("dropdown");
                }
            }
        }
        "textToColumns" => {
            let mut delims = Vec::new();
            for (k, c) in [("tab", "\t"), ("semicolon", ";"), ("comma", ","), ("space", " ")] {
                if d.values.get(k).and_then(Json::as_bool) == Some(true) {
                    delims.push(json!(c));
                }
            }
            v["delimiters"] = Json::Array(delims);
        }
        "removeDuplicates" | "subtotal" => {
            if let Some(t) = d.values.get("columnsText").and_then(Json::as_str).filter(|t| !t.trim().is_empty()) {
                v["columns"] = Json::Array(t.split(',').map(|c| json!(c.trim().to_ascii_uppercase())).collect());
            }
        }
        "allowEditRanges" => {
            if let Some(m) = v.as_object_mut() {
                // Blank fields mean "unchanged"; the check box clears a password.
                m.retain(|_, x| x.as_str().is_none_or(|s| !s.trim().is_empty()));
                if m.remove("removePassword").and_then(|x| x.as_bool()) == Some(true) {
                    m.insert("password".into(), json!(""));
                }
            }
        }
        "cfQuick" => {
            let mut rule = Map::new();
            for (k, val) in &d.values {
                rule.insert(k.clone(), val.clone());
            }
            v = json!({"rule": Json::Object(rule)});
        }
        "series" => {
            if let Some(s) = d.values.get("stop").and_then(Json::as_str).and_then(|s| s.parse::<f64>().ok()) {
                v["stop"] = json!(s);
            } else if let Some(m) = v.as_object_mut() {
                m.remove("stop");
            }
        }
        "defineName" => {
            if let Some(m) = v.as_object_mut() {
                for key in ["scope", "oldScope"] {
                    if m.get(key).and_then(Json::as_str) == Some("Workbook") {
                        m.remove(key);
                    }
                }
            }
        }
        _ => {}
    }
    v
}

pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else { return };
    if d.name == "pasteSpecial" {
        paste_special_shortcuts(&mut d, ctx);
    }
    let mut open = true;
    let mut win_open = true;
    let mut confirm = false;
    let width = match d.name.as_str() {
        "formatCells" => 560.0,
        "about" => 660.0,
        "insertFunction" | "commandSearch" | "nameManager" | "manageRules" | "journal" | "agents" | "macros" => 520.0,
        "recommendedCharts" => 600.0,
        "analysisToolPak" => 420.0,
        "solver" => 460.0,
        "zoom" => 220.0,
        "pasteSpecial" => 480.0,
        _ => 380.0,
    };
    let window = egui::Window::new(app.ui.language.tr(&d.title))
        .id(egui::Id::new("dialog").with(&d.name))
        .collapsible(false)
        .resizable(false)
        .default_width(width)
        // A default position, not an anchor: egui anchors can't be dragged, and dialogs move by their title bar.
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center() + vec2(0.0, -40.0))
        .open(&mut win_open);
    let window = match d.name.as_str() {
        "insertPicture" => window.default_height(240.0),
        "advancedFilter" => window.default_height(0.0),
        _ => window,
    };
    window.show(ctx, |ui| {
            ui.set_min_width(width - 20.0);
            match d.name.as_str() {
                "formatCells" => format_cells(app, ui, &mut d, &mut confirm),
                "dataValidation" => data_validation_form(ui, &mut d, &mut confirm, &mut open),
                "advancedFilter" => advanced_filter_form(ui, &mut d, &mut confirm, &mut open),
                "insertFunction" => insert_function(app, ui, &mut d, &mut confirm),
                "find" => find(app, ui, &mut d),
                "goTo" => go_to(app, ui, &mut d, &mut confirm),
                "zoom" => zoom(ui, &mut d, &mut confirm, &mut open),
                "sort" => sort(app, ui, &mut d, &mut confirm),
                "nameManager" => name_manager(app, ui, &mut d, &mut open),
                "manageRules" => manage_rules(app, ui),
                "recommendedCharts" => recommended_charts(app, ui, &mut d, &mut confirm, &mut open),
                "selectData" => select_data(app, ui, &mut d, &mut confirm, &mut open),
                "commandSearch" => command_search(app, ui, &mut d, &mut confirm),
                "goalSeek" => goal_seek(app, ui, &mut d, &mut confirm),
                "pasteSpecial" => paste_special(app, ui, &mut d, &mut confirm, &mut open),
                "analysisToolPak" => analysis_toolpak(app, ui, &mut d, &mut confirm, &mut open),
                "solver" => solver(ui, &mut d, &mut confirm, &mut open),
                "solverResults" => solver_results(app, ui, &mut d, &mut open),
                "spelling" => spelling(app, ui, &mut d, &mut open),
                "agents" => {
                    ui.label("GridCraft is fully drivable by agents. Every menu item, button and gesture is a command:");
                    ui.add_space(4.0);
                    ui.monospace("gridcraft --control 7979        # JSON-lines control channel\ngridcraft-cli mcp --connect 7979   # MCP bridged to this window\ngridcraft-cli mcp                   # headless MCP server");
                    ui.add_space(4.0);
                    ui.label(format!("{} engine commands are available. See docs/mcp.md and docs/control-protocol.md.", gridcraft_engine::command_specs().len()));
                }
                "about" => about(ui, &mut d),
                "journal" => {
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (id, p) in app.session.journal.iter().rev().take(500) {
                            ui.monospace(format!("{id} {p}"));
                        }
                    });
                }
                "macros" => macros(app, ui, &mut d),
                "statistics" | "accessibility" | "errorChecking" | "evaluateFormula" | "comments" => {
                    let r = if d.name == "comments" {
                        app.session.active().and_then(|doc| doc.wb.active().map(|s| json!(s.comments.iter().map(|(c, m)| json!({"cell": c.a1(), "author": m.author, "text": m.text})).collect::<Vec<_>>())))
                    } else {
                        d.result.clone()
                    };
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        ui.monospace(serde_json::to_string_pretty(&r.unwrap_or(Json::Null)).unwrap_or_default());
                    });
                }
                "insertPicture" => {
                    // A private inbox dies with this dialog; late browser results
                    // cannot affect a later picture choice or another workbook.
                    let arrived = d.picture_inbox.as_ref().and_then(|inbox| {
                        inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).pop()
                    });
                    if let Some((name, bytes)) = arrived {
                        d.picture_inbox = None;
                        if bytes.len() > 16 * 1024 * 1024 {
                            app.message = Some((d.title.clone(), "Choose a PNG or JPEG no larger than 16 MB.".into()));
                        } else {
                            d.values.remove("path");
                            d.values.insert("alt".into(), json!(name));
                            d.values.insert("base64".into(), json!(gridcraft_engine::io::base64_encode(&bytes)));
                        }
                    }
                    if app.services.pick_picture_async.is_some() {
                        let name = d.values.get("alt").and_then(Json::as_str).unwrap_or("No picture selected");
                        ui.label(name);
                    } else {
                        ui.label("Picture file path (PNG or JPEG):");
                        let mut path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
                        if ui.text_edit_singleline(&mut path).changed() {
                            d.values.remove("base64");
                            d.values.insert("path".into(), json!(path));
                        }
                    }
                    if (app.services.pick_picture.is_some() || app.services.pick_picture_async.is_some()) && ui.button("Browse…").clicked() {
                        if let Some(start) = &app.services.pick_picture_async {
                            let inbox = crate::Inbox::default();
                            start(inbox.clone());
                            d.picture_inbox = Some(inbox);
                        } else if let Some(pick) = &app.services.pick_picture
                            && let Some(path) = pick()
                        {
                            d.values.remove("base64");
                            let alt = std::path::Path::new(&path).file_name().and_then(|name| name.to_str()).unwrap_or("");
                            d.values.insert("alt".into(), json!(alt));
                            d.values.insert("path".into(), json!(path));
                        }
                    }
                    ui.label("Description (optional):");
                    let mut alt = d.values.get("alt").and_then(Json::as_str).unwrap_or("").to_string();
                    if ui.text_edit_singleline(&mut alt).changed() {
                        d.values.insert("alt".into(), json!(alt));
                    }
                    if d.values.get("placement").and_then(Json::as_str) == Some("cell") {
                        ui.label(egui::RichText::new("Fits inside the selected cell and moves with its content.").small());
                    }
                    ok_cancel(ui, &mut confirm, &mut open);
                }
                "pickList" => {
                    let items: Vec<String> = app
                        .session
                        .active()
                        .and_then(|doc| {
                            let sh = doc.wb.active()?;
                            let a = doc.selection.active;
                            let mut seen = std::collections::BTreeSet::new();
                            let mut r = a.row;
                            while r > 0 && !sh.value(gridcraft_engine::core::CellRef::new(r - 1, a.col)).is_empty() {
                                r -= 1;
                                seen.insert(sh.value(gridcraft_engine::core::CellRef::new(r, a.col)).display());
                            }
                            Some(seen.into_iter().collect())
                        })
                        .unwrap_or_default();
                    for it in items {
                        if ui.button(&it).clicked() {
                            app.run_or_alert("cell.set", json!({"input": it}));
                            open = false;
                        }
                    }
                }
                "saveCopy" => {
                    let fmt = d.values.get("format").and_then(Json::as_str).unwrap_or("xlsx").to_string();
                    ui.label(format!("Save a copy as .{fmt}"));
                    let mut path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
                    ui.text_edit_singleline(&mut path);
                    d.values.insert("path".into(), json!(path.clone()));
                    ok_cancel(ui, &mut confirm, &mut open);
                    if confirm {
                        // A bare name has no folder: written as typed it would land in the
                        // process's working directory (`/` from the Finder, the install folder on
                        // Windows), never where the user looks. The picker chooses the folder.
                        let pick = app.services.pick_save.as_ref();
                        let target = if path.is_empty() {
                            pick.and_then(|f| f(&format!("Copy.{fmt}")))
                        } else if let Some(f) = pick.filter(|_| std::path::Path::new(&path).is_relative()) {
                            f(&if path.ends_with(&format!(".{fmt}")) { path } else { format!("{path}.{fmt}") })
                        } else {
                            Some(path)
                        };
                        if let Some(t) = target {
                            let id = match fmt.as_str() {
                                "csv" => "file.exportCsv",
                                "html" => "file.exportHtml",
                                _ => "file.saveAs",
                            };
                            app.run_or_alert(id, json!({"path": t}));
                        }
                    }
                }
                "saveChanges" => {
                    ui.label(format!("Do you want to save the changes you made to {}?", d.values.get("title").and_then(Json::as_str).unwrap_or("this workbook")));
                    // Asked while quitting: once answered, the next unsaved workbook is asked about.
                    let quitting = d.values.get("quit").and_then(Json::as_bool) == Some(true);
                    ui.horizontal(|ui| {
                        if ui.button("Don't Save").clicked() {
                            app.run_or_alert("file.close", json!({"force": true}));
                            if quitting {
                                app.continue_quit(ui.ctx());
                            }
                            open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            open = false;
                        }
                        // File operations must include the cell the user is still editing (as the File > Save menu item already does).
                        if ui.button("Save").clicked() && app.commit_edit(0, 0, false, false) {
                            app.run_or_alert("file.save", json!({}));
                            if app.session.active().is_some_and(|x| !x.is_dirty()) {
                                app.run_or_alert("file.close", json!({"force": true}));
                                if quitting {
                                    app.continue_quit(ui.ctx());
                                }
                            }
                            open = false;
                        }
                    });
                }
                "start" => {
                    ui.heading("Start a new workbook");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Blank workbook").clicked() {
                            app.run_or_alert("file.new", json!({}));
                            open = false;
                        }
                        for (n, t) in gridcraft_engine::sample::SAMPLES {
                            if ui.button(*t).clicked() {
                                app.run_or_alert("file.new", json!({"sample": n}));
                                open = false;
                            }
                        }
                    });
                    ui.separator();
                    ui.label(egui::RichText::new("Recent").strong());
                    for path in app.ui.recent.clone() {
                        if ui.link(&path).clicked() {
                            app.open_path(&path);
                            open = false;
                        }
                    }
                    if ui.button("Open…").clicked() {
                        app.open_dialog("open", json!({}));
                        open = false;
                    }
                }
                _ => form(ui, &mut d, &mut confirm, &mut open),
            }
        });
    // Hyperlinks inside dialogs (About ▸ Contributors) open through the host's `open_url`, since
    // the native shell has no browser integration of its own.
    if let Some(open_url) = &app.services.open_url {
        let urls: Vec<String> = ctx.output_mut(|o| {
            let mut urls = vec![];
            o.commands.retain(|c| match c {
                egui::OutputCommand::OpenUrl(u) => {
                    urls.push(u.url.clone());
                    false
                }
                _ => true,
            });
            urls
        });
        for u in urls {
            open_url(&u);
        }
    }
    if ctx.input(|i| i.key_pressed(Key::Escape)) || !win_open {
        open = false;
    }
    if confirm && d.name == "insertPicture" && d.picture_target != app.view_key() {
        app.message = Some((d.title.clone(), "Return to the original worksheet to insert this picture, or cancel and choose another cell.".into()));
        confirm = false;
    }
    if confirm && let Some(cmd) = d.command {
        let params = build_params(&d);
        match app.run(cmd, params) {
            Ok(r) => {
                open = false;
                if d.name == "solver" {
                    app.open_dialog("solverResults", r);
                } else if d.name == "openPassword" {
                    let path = d.values.get("path").and_then(Json::as_str).map(str::to_string);
                    app.after_open(path.as_deref(), &r);
                }
            }
            // Data Analysis asks before replacing data in its output range.
            Err(e) if d.name == "analysisToolPak" && e.contains(gridcraft_engine::cmd::analysis::OVERWRITE_ERROR) => {
                d.values.insert("askOverwrite".into(), json!(true));
            }
            // A wrong password: say so, clear the field and stay open for another try.
            Err(e) if d.name == "openPassword" => {
                d.values.insert("password".into(), json!(""));
                app.message = Some((d.title.clone(), crate::clean_error(&e)));
            }
            Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
        }
    } else if confirm {
        open = false;
    }
    if let Some(child) = app.dialog.as_mut() {
        // The body opened another dialog (Name Manager › New…). It takes over; this one waits
        // behind it and comes back when it closes, as Excel returns to the Name Manager. Go To
        // hands over to Go To Special for good, as in Excel.
        if open && d.name != "goTo" {
            child.back = Some(Box::new(d));
        } else if child.back.is_none() {
            child.back = d.back.take();
        }
        return;
    }
    if open {
        app.dialog = Some(d);
    } else if let Some(back) = d.back.take() {
        app.dialog = Some(*back);
    }
}

/// Paste Special as Excel lays it out: Paste and Operation each in two columns of options, Skip
/// blanks and Transpose side by side, and Paste Link as a button that pastes at once. The options
/// come from the dialog's `fields`, so the letter shortcuts and the control channel see the same.
fn paste_special(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = app.ui.language;
    let t = theme::Tokens::get(ui.ctx());
    let choices: Vec<(&'static str, &'static str, Vec<(&'static str, &'static str)>)> = d
        .fields
        .iter()
        .filter_map(|f| match f {
            Field::Choice { key, label, options } => Some((*key, *label, options.clone())),
            _ => None,
        })
        .collect();
    for (key, label, options) in choices {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(lang.tr(label)).strong());
        egui::Frame::new().stroke(Stroke::new(1.0, t.separator)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let cur = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
            // Column by column, as Excel fills them.
            let rows = options.len().div_ceil(2);
            egui::Grid::new(("paste_special", key)).num_columns(2).min_col_width(200.0).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                for r in 0..rows {
                    for c in 0..2 {
                        if let Some((value, text)) = options.get(c * rows + r) {
                            if ui.radio(cur == *value, lang.tr(text)).clicked() {
                                d.set_value(key, json!(value));
                            }
                        } else {
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        });
    }
    ui.add_space(8.0);
    // In the same columns as the options above.
    egui::Frame::new().inner_margin(egui::Margin::symmetric(11, 0)).show(ui, |ui| {
        egui::Grid::new("paste_special_checks").num_columns(2).min_col_width(200.0).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
            for (key, label) in [("skipBlanks", "Skip blanks"), ("transpose", "Transpose (E)")] {
                let mut b = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
                if ui.checkbox(&mut b, lang.tr(label)).changed() {
                    d.values.insert(key.into(), json!(b));
                }
            }
            ui.end_row();
        });
    });
    ui.add_space(8.0);
    ui.separator();
    ui.horizontal(|ui| {
        // Excel's Paste Link pastes references to the copied cells straight away.
        if ui.button(lang.tr("Paste Link")).on_hover_text(lang.tr("Paste formulas that refer to the copied cells")).clicked() {
            d.values.insert("what".into(), json!("all"));
            d.values.insert("operation".into(), json!("none"));
            d.values.insert("link".into(), json!(true));
            *confirm = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = ui.add(egui::Button::new(egui::RichText::new(format!("   {}   ", lang.tr("OK"))).color(Color32::WHITE)).fill(t.accent));
            if ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                *confirm = true;
            }
            if ui.button(format!(" {} ", lang.tr("Cancel"))).clicked() {
                *open = false;
            }
        });
    });
}

/// Paste Special's legacy mnemonics select options; Enter still performs the paste.
fn paste_special_shortcuts(d: &mut Dialog, ctx: &egui::Context) {
    if ctx.text_edit_focused() {
        return;
    }
    let right_alt = ctx.input(|i| {
        i.key_down(Key::AltRight)
            || i.events.iter().any(|event| {
                matches!(event, egui::Event::Key { key: Key::AltRight, .. })
                    || matches!(event, egui::Event::Key { physical_key: Some(Key::AltRight), .. })
            })
    });
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    let allowed = |m: egui::Modifiers| !m.ctrl && !m.command && !m.mac_cmd && !(m.alt && (mac || right_alt));
    let mnemonic = |key| match key {
        Key::T => Some('t'),
        Key::V => Some('v'),
        Key::F => Some('f'),
        Key::C => Some('c'),
        Key::E => Some('e'),
        _ => None,
    };
    let mut text_to_consume = Vec::new();
    for event in ctx.input(|i| i.events.clone()) {
        let egui::Event::Key { key, pressed: true, repeat, modifiers, .. } = event else { continue };
        if !allowed(modifiers) {
            continue;
        }
        let Some(letter) = mnemonic(key) else { continue };
        text_to_consume.push(letter);
        if repeat {
            continue;
        }
        if key == Key::E {
            let transpose = d.values.get("transpose").and_then(Json::as_bool).unwrap_or(false);
            d.values.insert("transpose".into(), json!(!transpose));
        } else {
            let what = match key {
                Key::T => "formats",
                Key::V => "values",
                Key::F => "formulas",
                _ => "comments",
            };
            d.values.insert("what".into(), json!(what));
        }
    }
    // Browsers can deliver Text before or after Key. Act only on Key, then remove
    // its matching text so one physical press cannot toggle Transpose twice.
    ctx.input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::Key { key, modifiers, .. } => !(allowed(*modifiers) && mnemonic(*key).is_some()),
            egui::Event::Text(text) if text.len() == 1 => {
                let letter = text.chars().next().unwrap_or_default().to_ascii_lowercase();
                if let Some(index) = text_to_consume.iter().position(|&c| c == letter) {
                    text_to_consume.remove(index);
                    false
                } else {
                    true
                }
            }
            _ => true,
        });
    });
}

fn ok_cancel(ui: &mut egui::Ui, confirm: &mut bool, open: &mut bool) {
    ui.add_space(8.0);
    // A row as tall as the buttons: a right-to-left layout straight in the window takes all the
    // height it is offered, which stretched every form dialog to the screen's height.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let lang = crate::i18n::current(ui.ctx());
            let ok = ui.add(
                egui::Button::new(egui::RichText::new(format!("   {}   ", lang.tr("OK"))).color(Color32::WHITE))
                    .fill(crate::theme::Tokens::get(ui.ctx()).accent),
            );
            if ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                *confirm = true;
            }
            if ui.button(format!(" {} ", lang.tr("Cancel"))).clicked() {
                *open = false;
            }
        })
    });
}

fn form(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    form_fields(ui, d);
    ok_cancel(ui, confirm, open);
}

/// Gives keyboard focus to the first text or number field the first time a form is drawn. A
/// focused number field is in typing mode with its value selected, so Alt+H, O, W then 12 and
/// Enter sets the width, as in Excel.
/// Keeps asking until the field really has focus: a new window's first frame is an invisible
/// sizing pass where a focus request is ignored.
fn take_first_focus(d: &mut Dialog, r: &egui::Response) {
    if !d.focus_first_field {
        return;
    }
    if !r.has_focus() {
        r.request_focus();
        return;
    }
    // Focus has landed, and the field is now a text box: select its whole value so typing
    // replaces it (the field's own select-on-focus fired before it was a text box).
    let mut state = egui::text_edit::TextEditState::load(&r.ctx, r.id).unwrap_or_default();
    let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(usize::MAX / 2));
    state.cursor.set_char_range(Some(all));
    state.store(&r.ctx, r.id);
    d.focus_first_field = false;
}

/// Draws a dialog's `fields`, editing its values.
fn form_fields(ui: &mut egui::Ui, d: &mut Dialog) {
    // Field labels, choices and notes are English source strings, translated as they are drawn.
    let lang = crate::i18n::current(ui.ctx());
    egui::Grid::new("form").num_columns(1).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        for f in d.fields.clone() {
            match f {
                Field::Text { key, label } => {
                    ui.vertical(|ui| {
                        ui.label(lang.tr(label));
                        let mut s = d
                            .values
                            .get(key)
                            .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| if v.is_null() { String::new() } else { v.to_string() }))
                            .unwrap_or_default();
                        let multiline = key == "text";
                        let r = if multiline {
                            ui.add(egui::TextEdit::multiline(&mut s).desired_rows(4).desired_width(320.0))
                        } else {
                            ui.add(egui::TextEdit::singleline(&mut s).desired_width(320.0))
                        };
                        take_first_focus(d, &r);
                        if r.changed() {
                            d.values.insert(key.into(), json!(s));
                        }
                    });
                }
                Field::Password { key, label } => {
                    // Masked as typed (Excel shows dots); "Show password" reveals it on purpose.
                    ui.vertical(|ui| {
                        ui.label(lang.tr(label));
                        let mut s = d.values.get(key).and_then(Json::as_str).unwrap_or_default().to_string();
                        if ui.add(egui::TextEdit::singleline(&mut s).password(!d.show_password).desired_width(320.0)).changed() {
                            d.values.insert(key.into(), json!(s));
                        }
                        ui.checkbox(&mut d.show_password, lang.tr("Show password"));
                    });
                }
                Field::Number { key, label } => {
                    ui.horizontal(|ui| {
                        ui.label(lang.tr(label));
                        let mut n = d.values.get(key).and_then(Json::as_f64).unwrap_or(0.0);
                        // Typed values count straight away: otherwise Enter or OK in the same frame
                        // confirms the old value, because a number field only commits when it loses focus.
                        let r = ui.add(egui::DragValue::new(&mut n).speed(0.5).update_while_editing(true));
                        take_first_focus(d, &r);
                        if r.changed() {
                            d.values.insert(key.into(), json!(n));
                        }
                    });
                }
                Field::Choice { key, label, options } => {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(lang.tr(label)).strong());
                        let cur = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
                        if d.name == "dataValidation" {
                            let selected = options.iter().find(|(val, _)| *val == cur).map(|(_, text)| lang.tr(text)).unwrap_or("");
                            egui::ComboBox::from_id_salt(("dataValidation", key)).selected_text(selected).width(300.0).show_ui(ui, |ui| {
                                for (val, text) in options {
                                    if ui.selectable_label(cur == val, lang.tr(text)).clicked() {
                                        d.set_value(key, json!(val));
                                    }
                                }
                            });
                        } else {
                            for (val, text) in options {
                                if ui.radio(cur == val, lang.tr(text)).clicked() {
                                    d.set_value(key, json!(val));
                                }
                            }
                        }
                    });
                }
                Field::Check { key, label } => {
                    let mut b = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
                    if ui.checkbox(&mut b, lang.tr(label)).changed() {
                        d.values.insert(key.into(), json!(b));
                    }
                }
                Field::Note(t) => {
                    // A note can carry user data (a URL, a file or sheet name); `tr` reads a `|` as a
                    // context separator, so such a note is shown as it is.
                    let text = if t.contains('|') { t.as_str() } else { lang.tr(&t) };
                    ui.label(egui::RichText::new(text).small());
                }
            }
            ui.end_row();
        }
    });
}

/// Help ▸ About GridCraft: the app, its contributors and the AI models that helped. The credits
/// are compiled in (`crate::credits`, docs/contributors.md).
fn about(ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for tab in ["About", "Contributors", "Models"] {
            if ui.selectable_label(d.tab == tab, tab).clicked() {
                d.tab = tab.to_string();
            }
        }
    });
    ui.separator();
    match d.tab.as_str() {
        "Contributors" => crate::credits::contributors_ui(ui),
        "Models" => crate::credits::models_ui(ui),
        _ => {
            ui.heading("GridCraft");
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(4.0);
            ui.label("A clean-room, open-source, Rust-native spreadsheet. Part of ArtCraft.");
            ui.hyperlink_to("getartcraft.com/apps/gridcraft", "https://getartcraft.com/apps/gridcraft");
            ui.hyperlink_to("Join the ArtCraft Discord", "https://discord.gg/artcraft");
        }
    }
}

/// The Macros dialog: lists GridCraft's own recorded scripts and (if the file carries one) the
/// preserved VBA project. Mirrors Excel's Developer ▸ Macros list, but is honest that VBA is
/// preserved and not executed.
fn macros(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.label("Automation in this workbook. GridCraft runs its own scripts; a file's VBA project is preserved but not executed.");
    ui.add_space(6.0);
    let items: Vec<Json> = d.result.as_ref().and_then(Json::as_array).cloned().unwrap_or_default();
    if items.is_empty() {
        ui.weak("This workbook has no scripts or macros.");
        return;
    }
    // Scripts that came with a file stay disabled until the user trusts the workbook (#2178).
    if items.iter().any(|it| it.get("trusted").and_then(Json::as_bool) == Some(false)) {
        ui.horizontal(|ui| {
            ui.weak("Scripts in this workbook came from a file and are disabled.");
            if ui.button("Trust Scripts").on_hover_text("Let this workbook's scripts run for the rest of this session").clicked() {
                let _ = app.run("automate.trustScripts", json!({"trusted": true}));
                d.result = app.session.run("view.macros", json!({})).ok();
            }
        });
        ui.add_space(6.0);
    }
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        for it in &items {
            let name = it.get("name").and_then(Json::as_str).unwrap_or("(unnamed)");
            let is_vba = it.get("kind").and_then(Json::as_str) == Some("vba");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).strong());
                if is_vba {
                    ui.weak("— VBA project (preserved, not run)");
                } else {
                    let steps = it.get("steps").and_then(Json::as_u64).unwrap_or(0);
                    ui.weak(format!("— {steps} step{}", if steps == 1 { "" } else { "s" }));
                }
            });
        }
    });
}

fn gradient_color_button(ui: &mut egui::Ui, color: &mut CellColor, theme: &Theme) {
    let mut rgb = color.resolve(theme).unwrap_or([0, 0, 0]);
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        *color = CellColor::rgb(rgb[0], rgb[1], rgb[2]);
    }
}

fn fill_tab(ui: &mut egui::Ui, fill: &mut Fill, theme: &Theme, lang: crate::i18n::Language) {
    let was = if fill.is_gradient() {
        2
    } else if fill.pattern != PatternType::None {
        1
    } else {
        0
    };
    let mut mode = was;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut mode, 0, lang.tr("No Fill"));
        ui.selectable_value(&mut mode, 1, lang.tr("Solid"));
        ui.selectable_value(&mut mode, 2, lang.tr("Gradient"));
    });
    if mode != was {
        match mode {
            0 => *fill = Fill::default(),
            1 => {
                let c = fill
                    .gradient
                    .as_ref()
                    .and_then(|g| g.stops.first())
                    .map(|s| s.color)
                    .unwrap_or_else(|| if fill.fg == CellColor::Auto { CellColor::Theme(4, 0) } else { fill.fg });
                *fill = Fill::solid(c);
            }
            _ => {
                let start = if fill.pattern == PatternType::Solid && fill.fg != CellColor::Auto { fill.fg } else { CellColor::Theme(4, 0) };
                *fill = Fill::gradient(
                    GradientKind::Linear { degree: 90.0 },
                    vec![GradientStop { position: 0.0, color: start }, GradientStop { position: 1.0, color: CellColor::Theme(0, 0) }],
                );
            }
        }
    }
    ui.add_space(4.0);
    match mode {
        0 => {
            ui.weak(lang.tr("No fill"));
        }
        1 => {
            ui.label(lang.tr("Background Color:"));
            if let Some(c) = crate::widgets::color_palette(ui, &theme.colors, lang.tr("No Color")) {
                *fill = if c == "none" { Fill::default() } else { Fill::solid(CellColor::from_hex(&c).unwrap_or_default()) };
            }
        }
        _ => {
            let Some(mut g) = fill.gradient.clone() else { return };
            let mut linear = g.is_linear();
            ui.horizontal(|ui| {
                ui.radio_value(&mut linear, true, lang.tr("Linear"));
                ui.radio_value(&mut linear, false, lang.tr("Path"));
            });
            if linear != g.is_linear() {
                g.kind =
                    if linear { GradientKind::Linear { degree: 90.0 } } else { GradientKind::Path { left: 0.5, right: 0.5, top: 0.5, bottom: 0.5 } };
            }
            match &mut g.kind {
                GradientKind::Linear { degree } => {
                    ui.add(egui::Slider::new(degree, 0.0..=360.0).text(lang.tr("Angle")));
                    ui.horizontal(|ui| {
                        for (label, value) in [("Vertical", 90.0), ("Diagonal up", 315.0), ("Horizontal", 0.0), ("Diagonal down", 45.0)] {
                            if ui.small_button(lang.tr(label)).clicked() {
                                *degree = value;
                            }
                        }
                    });
                }
                GradientKind::Path { left, right, top, bottom } => {
                    ui.horizontal(|ui| {
                        if ui.small_button(lang.tr("From center")).clicked() {
                            *left = 0.5;
                            *right = 0.5;
                            *top = 0.5;
                            *bottom = 0.5;
                        }
                        if ui.small_button(lang.tr("From corner")).clicked() {
                            *left = 0.0;
                            *right = 0.0;
                            *top = 0.0;
                            *bottom = 0.0;
                        }
                    });
                    ui.add(egui::Slider::new(left, 0.0..=1.0).text(lang.tr("Left")));
                    ui.add(egui::Slider::new(right, 0.0..=1.0).text(lang.tr("Right")));
                    ui.add(egui::Slider::new(top, 0.0..=1.0).text(lang.tr("Top")));
                    ui.add(egui::Slider::new(bottom, 0.0..=1.0).text(lang.tr("Bottom")));
                }
            }
            ui.separator();
            let count = g.stops.len();
            let mut remove = None;
            let mut editing = false;
            for i in 0..count {
                ui.horizontal(|ui| {
                    ui.label(format!("{}{}", lang.tr("Gradient|Stop"), i + 1));
                    gradient_color_button(ui, &mut g.stops[i].color, theme);
                    let mut pct = g.stops[i].position * 100.0;
                    let resp = ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%"));
                    if resp.changed() {
                        g.stops[i].position = pct / 100.0;
                    }
                    // Keyboard changes set `changed` but not `dragged`; keep the row bound to
                    // the same stop while its slider still owns focus as well.
                    editing |= resp.dragged() || resp.has_focus();
                    if count > 2 && ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                g.stops.remove(i);
            }
            if ui.small_button(lang.tr("Add Stop")).clicked() {
                let color = g.stops.last().map(|s| s.color).unwrap_or(CellColor::Auto);
                g.stops.push(GradientStop { position: 0.5, color });
            }
            // Sorting while a stop is being dragged or keyboard-edited would move a different
            // stop under that slider. Keep the row order until the edit ends; only the preview
            // is sorted.
            let preview = GradientFill::new(g.kind, g.stops.clone());
            let (rect, _) = ui.allocate_exact_size(vec2(240.0, 54.0), Sense::hover());
            crate::gradient::paint_rect(ui.painter(), rect, &preview, theme);
            ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(190)), StrokeKind::Inside);
            *fill = if editing {
                Fill { pattern: PatternType::None, fg: CellColor::Auto, bg: CellColor::Auto, gradient: Some(g) }
            } else {
                Fill::gradient(preview.kind, preview.stops)
            };
        }
    }
}

fn format_cells(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    // `d.tab` stays the English name (it is what `open_dialog` and tests pass); only the drawn
    // labels are translated. The Fill tab is keyed by context: the Alignment tab's "Fill" is a
    // different word in some languages.
    let lang = app.ui.language;
    ui.horizontal(|ui| {
        for (tab, key) in [
            ("Number", "Number"),
            ("Alignment", "Alignment"),
            ("Font", "Font"),
            ("Border", "Border"),
            ("Fill", "Format Cells|Fill"),
            ("Protection", "Protection"),
        ] {
            if ui.selectable_label(d.tab == tab, lang.tr(key)).clicked() {
                d.tab = tab.to_string();
            }
        }
    });
    ui.separator();
    let mut st: gridcraft_engine::model::Style = d.values.get("style").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let sample = app.session.active().and_then(|doc| doc.wb.active().map(|s| s.value(doc.selection.active))).unwrap_or_default();
    match d.tab.as_str() {
        "Number" => {
            let mut code = st.num_fmt.as_str().to_string();
            ui.columns(2, |cols| {
                cols[0].label(egui::RichText::new(lang.tr("Category:")).strong());
                for name in
                    ["General", "Number", "Currency", "Accounting", "Short Date", "Long Date", "Time", "Percentage", "Fraction", "Scientific", "Text"]
                {
                    let c = gridcraft_engine::cmd::format::format_code_for(name).to_string();
                    if cols[0].selectable_label(code == c, lang.tr(name)).clicked() {
                        code = c;
                    }
                }
                cols[1].label(egui::RichText::new(lang.tr("Sample")).strong());
                let preview = app.session.active().map(|doc| gridcraft_engine::display::format(&sample, &code, &doc.wb).text).unwrap_or_default();
                cols[1].label(egui::RichText::new(preview).font(theme::ui_font(15.0)));
                cols[1].add_space(8.0);
                cols[1].label(lang.tr("Type (custom format code):"));
                cols[1].text_edit_singleline(&mut code);
                for c in [
                    "0",
                    "0.00",
                    "#,##0",
                    "#,##0.00",
                    "#,##0;[Red]-#,##0",
                    "0%",
                    "0.00%",
                    "0.00E+00",
                    "# ?/?",
                    "m/d/yyyy",
                    "d-mmm-yy",
                    "mmmm d, yyyy",
                    "h:mm AM/PM",
                    "h:mm:ss",
                    "[h]:mm:ss",
                    "@",
                ] {
                    if cols[1].small_button(c).clicked() {
                        code = c.to_string();
                    }
                }
            });
            st.num_fmt = gridcraft_engine::model::NumFmt::new(&code);
        }
        "Alignment" => {
            use gridcraft_engine::model::{HAlign, VAlign};
            ui.label(egui::RichText::new(lang.tr("Horizontal")).strong());
            ui.horizontal_wrapped(|ui| {
                for (h, n) in [
                    (HAlign::General, "General"),
                    (HAlign::Left, "Left"),
                    (HAlign::Center, "Center"),
                    (HAlign::Right, "Right"),
                    (HAlign::Fill, "Fill"),
                    (HAlign::Justify, "Justify"),
                    (HAlign::CenterAcross, "Center Across Selection"),
                    (HAlign::Distributed, "Distributed"),
                ] {
                    ui.radio_value(&mut st.align.h, h, lang.tr(n));
                }
            });
            ui.label(egui::RichText::new(lang.tr("Vertical")).strong());
            ui.horizontal_wrapped(|ui| {
                for (v, n) in [
                    (VAlign::Top, "Top"),
                    (VAlign::Center, "Center"),
                    (VAlign::Bottom, "Bottom"),
                    (VAlign::Justify, "Justify"),
                    (VAlign::Distributed, "Distributed"),
                ] {
                    ui.radio_value(&mut st.align.v, v, lang.tr(n));
                }
            });
            let mut indent = st.align.indent as f32;
            ui.add(egui::Slider::new(&mut indent, 0.0..=15.0).text(lang.tr("Indent")));
            st.align.indent = indent as u8;
            let mut rot = if st.align.rotation == 255 { 0.0 } else { st.align.rotation as f32 };
            ui.add(egui::Slider::new(&mut rot, -90.0..=90.0).text(lang.tr("Orientation (degrees)")));
            st.align.rotation = rot as i16;
            ui.checkbox(&mut st.align.wrap, lang.tr("Wrap text"));
            ui.checkbox(&mut st.align.shrink, lang.tr("Shrink to fit"));
        }
        "Font" => {
            egui::ComboBox::from_label(lang.tr("Font")).selected_text(st.font.name.clone()).show_ui(ui, |ui| {
                for f in crate::fonts::families() {
                    ui.selectable_value(&mut st.font.name, f.clone(), f.clone());
                }
            });
            ui.add(egui::Slider::new(&mut st.font.size, 6.0..=72.0).text(lang.tr("Size")));
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.font.bold, lang.tr("Bold"));
                ui.checkbox(&mut st.font.italic, lang.tr("Italic"));
                ui.checkbox(&mut st.font.strike, lang.tr("Strikethrough"));
            });
            use gridcraft_engine::model::{Underline, VertAlign};
            ui.horizontal(|ui| {
                for (u, n) in [
                    (Underline::None, "None"),
                    (Underline::Single, "Single"),
                    (Underline::Double, "Double"),
                    (Underline::SingleAccounting, "Single Accounting"),
                    (Underline::DoubleAccounting, "Double Accounting"),
                ] {
                    ui.radio_value(&mut st.font.underline, u, lang.tr(n));
                }
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.font.vert, VertAlign::Baseline, lang.tr("Normal"));
                ui.radio_value(&mut st.font.vert, VertAlign::Superscript, lang.tr("Superscript"));
                ui.radio_value(&mut st.font.vert, VertAlign::Subscript, lang.tr("Subscript"));
            });
            ui.label(lang.tr("Color:"));
            let colors = app.session.active().map(|doc| doc.wb.theme.colors).unwrap_or_default();
            if let Some(c) = crate::widgets::color_palette(ui, &colors, lang.tr("Automatic")) {
                st.font.color =
                    if c == "none" { gridcraft_engine::model::Color::Auto } else { gridcraft_engine::model::Color::from_hex(&c).unwrap_or_default() };
            }
        }
        "Border" => {
            use gridcraft_engine::model::{BorderLine, BorderStyle};
            let line = BorderLine { style: BorderStyle::Thin, color: gridcraft_engine::model::Color::Auto };
            ui.horizontal(|ui| {
                if ui.button(lang.tr("None")).clicked() {
                    st.borders = Default::default();
                }
                if ui.button(lang.tr("Outline")).clicked() {
                    st.borders.top = line;
                    st.borders.bottom = line;
                    st.borders.left = line;
                    st.borders.right = line;
                }
            });
            for (n, slot) in [
                ("Top", &mut st.borders.top),
                ("Bottom", &mut st.borders.bottom),
                ("Left", &mut st.borders.left),
                ("Right", &mut st.borders.right),
                ("Diagonal ↘", &mut st.borders.diag_down),
                ("Diagonal ↗", &mut st.borders.diag_up),
            ] {
                let n = lang.tr(n);
                ui.horizontal(|ui| {
                    ui.label(format!("{n}:"));
                    for (s, label) in [
                        (BorderStyle::None, "none"),
                        (BorderStyle::Thin, "thin"),
                        (BorderStyle::Medium, "medium"),
                        (BorderStyle::Thick, "thick"),
                        (BorderStyle::Dashed, "dashed"),
                        (BorderStyle::Dotted, "dotted"),
                        (BorderStyle::Double, "double"),
                    ] {
                        let label = lang.tr(label);
                        let response = ui.selectable_label(slot.style == s, label);
                        response
                            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::SelectableLabel, ui.is_enabled(), format!("{n} {label}")));
                        if response.clicked() {
                            slot.style = s;
                        }
                    }
                });
            }
            let theme = app.session.active().map(|doc| &doc.wb.theme).cloned().unwrap_or_default();
            crate::border_preview::sample(ui, &st.borders, &theme);
        }
        "Fill" => {
            let theme = app.session.active().map(|doc| doc.wb.theme.clone()).unwrap_or_default();
            fill_tab(ui, &mut st.fill, &theme, lang);
        }
        "Protection" => {
            ui.checkbox(&mut st.protection.locked, lang.tr("Locked"));
            ui.checkbox(&mut st.protection.hidden, lang.tr("Hidden"));
            ui.label(
                egui::RichText::new(
                    lang.tr("Locking cells or hiding formulas has no effect until you protect the worksheet (Review › Protect Sheet)."),
                )
                .small(),
            );
        }
        _ => {}
    }
    if let Ok(v) = serde_json::to_value(&st) {
        d.values.insert("style".into(), v);
    }
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm && let Some(style) = d.values.get("style").cloned() {
        app.run_or_alert("home.formatCells", json!({"style": style}));
    }
    if !open {
        app.dialog = None;
        *confirm = true; // closes
    }
}

/// Insert Function: searches names (as the formula language spells them) and descriptions, shows
/// each hit's category and description, and starts editing `=NAME(` (or appends to the formula).
fn insert_function(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let language = app.ui.language;
    ui.horizontal(|ui| {
        ui.label(language.tr("Search:"));
        let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(300.0).hint_text(language.tr("e.g. lookup, average, date")));
        r.request_focus();
    });
    let locale = language.formula_locale();
    let list = gridcraft_engine::cmd::formulas::function_list();
    // The names and descriptions searched are the ones shown, so in the interface language.
    let q = d.search.to_lowercase();
    let hits: Vec<&Json> = list
        .iter()
        .filter(|f| {
            q.is_empty()
                || f["name"].as_str().is_some_and(|n| locale.function_name(n).to_lowercase().contains(&q))
                || f["description"].as_str().is_some_and(|n| language.function_description(n).unwrap_or(n).to_lowercase().contains(&q))
        })
        .take(300)
        .collect();
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (i, f) in hits.iter().enumerate() {
            let name = locale.function_name(f["name"].as_str().unwrap_or(""));
            let category = language.tr(function_category_label(f["category"].as_str().unwrap_or("")));
            let r = ui.add(egui::Button::selectable(i == d.list_index, format!("{name:<18} {category}")).min_size(vec2(480.0, 20.0)));
            if r.clicked() {
                d.list_index = i;
            }
            if r.double_clicked() {
                d.list_index = i;
                *confirm = true;
            }
        }
    });
    if let Some(f) = hits.get(d.list_index) {
        let name = f["name"].as_str().unwrap_or("");
        ui.separator();
        ui.label(egui::RichText::new(crate::formula_locale::signature(name, locale, language).unwrap_or_default()).strong());
        ui.label(crate::formula_locale::description(name, locale, language).unwrap_or_default());
    }
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm && let Some(n) = hits.get(d.list_index).and_then(|f| f["name"].as_str()) {
        let n = app.session.active().map(|doc| crate::formula_locale::completion_name(n, locale, &doc.wb, doc.wb.active_sheet)).unwrap_or(n);
        let cur = app.editor.as_ref().map(|e| e.text.clone());
        let text = match cur {
            Some(t) if t.starts_with('=') => format!("{t}{n}("),
            _ => format!("={n}("),
        };
        app.begin_edit(Some(text), false);
    }
    if !open {
        *confirm = true;
    }
}

/// Excel's name for a function category, given the catalog's id (`MathTrig`).
fn function_category_label(id: &str) -> &str {
    match id {
        "DateTime" => "Date & Time",
        "Lookup" => "Lookup & Reference",
        "MathTrig" => "Math & Trig",
        "AddIn" => "Add-in and Automation",
        // Financial, Logical, Text, Statistical, Engineering, Database, Information, …
        other => other,
    }
}

fn find(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for tab in ["Find", "Replace"] {
            if ui.selectable_label(d.tab == tab, tab).clicked() {
                d.tab = tab.into();
            }
        }
    });
    let mut what = d.values.get("what").and_then(Json::as_str).unwrap_or("").to_string();
    let mut with = d.values.get("with").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label("Find what:");
        let r = ui.add(egui::TextEdit::singleline(&mut what).desired_width(260.0));
        // Like Excel, the cursor starts in "Find what" - asked for once, so clicking another field still works.
        if !d.values.contains_key("focused") {
            r.request_focus();
            d.values.insert("focused".into(), json!(true));
        }
    });
    if d.tab == "Replace" {
        ui.horizontal(|ui| {
            ui.label("Replace with:");
            ui.add(egui::TextEdit::singleline(&mut with).desired_width(244.0));
        });
    }
    let mut case = d.values.get("matchCase").and_then(Json::as_bool).unwrap_or(false);
    let mut whole = d.values.get("wholeCell").and_then(Json::as_bool).unwrap_or(false);
    let mut wb = d.values.get("within").and_then(Json::as_str) == Some("workbook");
    ui.horizontal(|ui| {
        ui.checkbox(&mut case, "Match case");
        ui.checkbox(&mut whole, "Match entire cell contents");
        ui.checkbox(&mut wb, "Within workbook");
    });
    d.values.insert("what".into(), json!(what));
    d.values.insert("with".into(), json!(with));
    d.values.insert("matchCase".into(), json!(case));
    d.values.insert("wholeCell".into(), json!(whole));
    d.values.insert("within".into(), json!(if wb { "workbook" } else { "sheet" }));
    let base = json!({"what": what, "matchCase": case, "wholeCell": whole, "within": if wb { "workbook" } else { "sheet" }});
    ui.horizontal(|ui| {
        if d.tab == "Replace" {
            if ui.button("Replace All").clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(true);
                match app.run("edit.replace", p) {
                    Ok(r) => app.toast = Some((format!("All done. We made {} replacements.", r["replaced"]), crate::now_ms())),
                    Err(e) => app.message = Some(("Find and Replace".into(), crate::clean_error(&e))),
                }
            }
            if ui.button("Replace").clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(false);
                let _ = app.run("edit.replace", p);
                let _ = app.run("edit.find", base.clone());
            }
        }
        if ui.button("Find All").clicked() {
            let mut p = base.clone();
            p["all"] = json!(true);
            d.result = app.run("edit.find", p).ok();
        }
        if ui.button("Find Next").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Err(e) = app.run("edit.find", base.clone()) {
                app.message = Some(("Find and Replace".into(), crate::clean_error(&e)));
            }
            app.grid.ensure_visible = true;
        }
    });
    if let Some(r) = &d.result
        && let Some(list) = r["results"].as_array()
    {
        ui.separator();
        ui.label(format!("{} cell(s) found", list.len()));
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for item in list {
                let label = format!(
                    "{}  {}  {}",
                    item["sheet"].as_str().unwrap_or(""),
                    item["cell"].as_str().unwrap_or(""),
                    item["value"].as_str().unwrap_or("")
                );
                if ui.selectable_label(false, label).clicked() {
                    if let Some(s) = item["sheet"].as_str() {
                        let _ = app.run("sheet.activate", json!({"sheet": s}));
                    }
                    let _ = app.run("selection.set", json!({"cell": item["cell"]}));
                    app.grid.ensure_visible = true;
                }
            }
        });
    }
}

/// Zoom's Magnification presets, as `preset` values and labels, in Excel's order.
const ZOOM_PRESETS: [(&str, &str); 5] = [("200", "200%"), ("100", "100%"), ("75", "75%"), ("50", "50%"), ("25", "25%")];

/// The `preset` that shows a zoom percentage: its Magnification entry, or Custom.
fn zoom_preset(percent: f64) -> &'static str {
    ZOOM_PRESETS.iter().map(|(val, _)| *val).find(|val| val.parse::<f64>().ok() == Some(percent)).unwrap_or("custom")
}

/// View › Zoom, laid out as Excel's: one Magnification column of 200% to 25%, Fit selection and
/// Custom with its percentage box. The current zoom is pre-selected; typing a percentage
/// selects Custom; Fit selection zooms so the selected cells fill the grid.
fn zoom(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let cur = d.values.get("preset").and_then(Json::as_str).unwrap_or("custom").to_string();
    ui.label(egui::RichText::new(lang.tr("Magnification")).strong());
    for (val, text) in ZOOM_PRESETS {
        if ui.radio(cur == val, text).clicked() {
            d.set_value("preset", json!(val));
        }
    }
    if ui.radio(cur == "fit", lang.tr("Fit selection")).clicked() {
        d.set_value("preset", json!("fit"));
    }
    ui.horizontal(|ui| {
        if ui.radio(cur == "custom", format!("{}:", lang.tr("Custom"))).clicked() {
            d.set_value("preset", json!("custom"));
        }
        // Whole percentages, as Excel shows them; the engine clamps to 10..400 as well.
        let mut percent = d.values.get("percent").and_then(Json::as_f64).unwrap_or(100.0).round() as i64;
        if ui.add(egui::DragValue::new(&mut percent).range(10..=400).speed(1.0)).changed() {
            d.set_value("percent", json!(percent));
        }
        ui.label("%");
    });
    ok_cancel(ui, confirm, open);
}

fn go_to(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let names: Vec<String> = app.session.active().map(|doc| doc.wb.names.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
    for n in names {
        if ui.selectable_label(false, &n).clicked() {
            d.values.insert("reference".into(), json!(n));
        }
    }
    let mut r = d.values.get("reference").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label("Reference:");
        ui.text_edit_singleline(&mut r).request_focus();
    });
    d.values.insert("reference".into(), json!(r.clone()));
    if ui.button("Special…").clicked() {
        app.open_dialog("goToSpecial", json!({}));
        return;
    }
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        app.run_or_alert("edit.goTo", json!({"reference": r}));
        app.grid.ensure_visible = true;
    }
    if !open {
        *confirm = true;
    }
}

fn sort(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let mut header = d.values.get("header").and_then(Json::as_bool).unwrap_or(true);
    ui.checkbox(&mut header, "My list has headers");
    d.values.insert("header".into(), json!(header));
    let mut levels: Vec<Json> = d.values.get("levels").and_then(Json::as_array).cloned().unwrap_or_default();
    // Columns of the region around the active cell, labelled with their header (or first value).
    let columns: Vec<(String, String)> = app
        .session
        .active()
        .and_then(|d| {
            let sh = d.wb.active()?;
            let region = gridcraft_engine::selection::current_region(sh, d.selection.active);
            Some(
                (region.start.col..=region.end.col)
                    .map(|c| {
                        let letter = gridcraft_engine::core::col_to_letters(c);
                        // The region starts at the header row when one is present, so the
                        // label always comes from start.row: header text with headers on,
                        // first data value with them off.
                        let label_row = region.start.row;
                        let text = gridcraft_engine::display::cell_text(&d.wb, sh, gridcraft_engine::core::CellRef::new(label_row, c));
                        let text: String = text.chars().take(24).collect();
                        (letter.clone(), if text.is_empty() { letter } else { format!("{letter}: {text}") })
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    let mut remove = None;
    for (i, lv) in levels.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(if i == 0 { "Sort by" } else { "Then by" });
            let mut col = lv["column"].as_str().unwrap_or("A").to_string();
            if columns.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut col).desired_width(50.0));
            } else {
                egui::ComboBox::from_id_salt(("sort_col", i)).selected_text(col.clone()).show_ui(ui, |ui| {
                    for (letter, label) in &columns {
                        ui.selectable_value(&mut col, letter.clone(), label.as_str());
                    }
                });
            }
            lv["column"] = json!(col.to_ascii_uppercase());
            let mut by = lv["by"].as_str().unwrap_or("values").to_string();
            egui::ComboBox::from_id_salt(("by", i))
                .selected_text(match by.as_str() {
                    "cellColor" => "Cell Color",
                    "fontColor" => "Font Color",
                    _ => "Cell Values",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut by, "values".into(), "Cell Values");
                    ui.selectable_value(&mut by, "cellColor".into(), "Cell Color");
                    ui.selectable_value(&mut by, "fontColor".into(), "Font Color");
                });
            lv["by"] = json!(by);
            let mut desc = lv["order"].as_str() == Some("desc");
            ui.radio_value(&mut desc, false, "A to Z");
            ui.radio_value(&mut desc, true, "Z to A");
            lv["order"] = json!(if desc { "desc" } else { "asc" });
            if ui.small_button("✕").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove
        && levels.len() > 1
    {
        levels.remove(i);
    }
    if ui.button("+ Add Level").clicked() {
        let used: Vec<String> = levels.iter().filter_map(|l| l.get("column").and_then(Json::as_str).map(str::to_string)).collect();
        let next = columns
            .iter()
            .map(|(l, _)| l)
            .find(|l| !used.contains(l))
            .cloned()
            .or_else(|| columns.last().map(|(l, _)| l.clone()))
            .unwrap_or_else(|| "B".into());
        levels.push(json!({"column": next, "order": "asc"}));
    }
    d.values.insert("levels".into(), Json::Array(levels.clone()));
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        app.run_or_alert("data.sort", json!({"header": header, "keys": levels}));
    }
    if !open {
        *confirm = true;
    }
}

fn name_manager(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let list = app.session.run("formulas.nameManager", json!({})).unwrap_or(Json::Null);
    let names = list.as_array().cloned().unwrap_or_default();
    let selected = names.get(d.list_index).cloned();
    let field = |n: &Json, k: &str| n.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        if ui.button("New…").clicked() {
            app.open_dialog("defineName", json!({}));
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new("Edit…")).clicked()
            && let Some(n) = &selected
        {
            app.open_dialog(
                "defineName",
                json!({
                    "name": field(n, "name"), "scope": field(n, "scope"), "comment": field(n, "comment"),
                    "refersTo": field(n, "refersTo"), "oldName": field(n, "name"), "oldScope": field(n, "scope"),
                }),
            );
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new("Delete")).clicked()
            && let Some(n) = &selected
        {
            app.run_or_alert("formulas.deleteName", json!({"name": field(n, "name"), "scope": field(n, "scope")}));
        }
    });
    ui.add_space(4.0);
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        egui::Grid::new("names").striped(true).num_columns(4).show(ui, |ui| {
            ui.strong("Name");
            ui.strong("Value");
            ui.strong("Refers To");
            ui.strong("Scope");
            ui.end_row();
            if names.is_empty() {
                ui.label(egui::RichText::new("No names defined").italics());
                ui.end_row();
            }
            for (i, n) in names.iter().enumerate() {
                if ui.selectable_label(i == d.list_index, field(n, "name")).clicked() {
                    d.list_index = i;
                }
                ui.label(n.get("value").map(|v| v.to_string()).unwrap_or_default().chars().take(30).collect::<String>());
                ui.label(field(n, "refersTo"));
                ui.label(field(n, "scope"));
                ui.end_row();
            }
        });
    });
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(" Close ").clicked() {
            *open = false;
        }
    });
}

/// Every chart type the All Charts tab offers: `(type, subtype, label)` for `insert.chart`.
const ALL_CHARTS: [(&str, &str, &str); 20] = [
    ("column", "", "Clustered Column"),
    ("column", "stacked", "Stacked Column"),
    ("column", "stacked100", "100% Stacked Column"),
    ("bar", "", "Clustered Bar"),
    ("bar", "stacked", "Stacked Bar"),
    ("bar", "stacked100", "100% Stacked Bar"),
    ("line", "", "Line"),
    ("line", "markers", "Line with Markers"),
    ("line", "stacked", "Stacked Line"),
    ("pie", "", "Pie"),
    ("doughnut", "", "Doughnut"),
    ("area", "", "Area"),
    ("area", "stacked", "Stacked Area"),
    ("scatter", "", "Scatter"),
    ("scatter", "lines", "Scatter with Lines"),
    ("radar", "", "Radar"),
    ("histogram", "", "Histogram"),
    ("waterfall", "", "Waterfall"),
    ("funnel", "", "Funnel"),
    ("combo", "", "Combo"),
];

/// The chart types listed on the dialog's current tab.
fn chart_choices(d: &Dialog) -> Vec<(String, String, String)> {
    if d.tab == "All Charts" {
        return ALL_CHARTS.iter().map(|(t, s, l)| (t.to_string(), s.to_string(), l.to_string())).collect();
    }
    let field = |x: &Json, k: &str| x.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    d.result
        .as_ref()
        .and_then(Json::as_array)
        .map(|a| a.iter().map(|x| (field(x, "type"), field(x, "subtype"), field(x, "label"))).collect())
        .unwrap_or_default()
}

/// Selects entry `i` of the current tab: OK inserts it.
fn select_chart(d: &mut Dialog, i: usize) {
    let Some((t, sub, _)) = chart_choices(d).into_iter().nth(i) else { return };
    d.list_index = i;
    d.values.insert("type".into(), json!(t));
    d.values.insert("subtype".into(), json!(sub));
}

fn chart_icon(t: &str) -> Icon {
    match t {
        "column" | "bar" | "histogram" | "waterfall" | "funnel" | "combo" => Icon::ChartBar,
        "line" | "area" | "radar" => Icon::ChartLine,
        "pie" | "doughnut" => Icon::ChartPie,
        "scatter" | "bubble" => Icon::ChartScatter,
        _ => Icon::Chart,
    }
}

fn recommended_charts(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    ui.horizontal(|ui| {
        for tab in ["Recommended Charts", "All Charts"] {
            if ui.selectable_label(d.tab == tab, tab).clicked() && d.tab != tab {
                d.tab = tab.to_string();
                select_chart(d, 0);
            }
        }
    });
    ui.separator();
    let choices = chart_choices(d);
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(190.0);
            egui::ScrollArea::vertical().id_salt("chart_choices").max_height(300.0).show(ui, |ui| {
                for (i, (kind, _, label)) in choices.iter().enumerate() {
                    let resp = ui
                        .horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                            icons::paint(ui.painter(), r.shrink(2.0), chart_icon(kind), t.text);
                            ui.selectable_label(i == d.list_index, label.as_str())
                        })
                        .inner;
                    if resp.clicked() {
                        select_chart(d, i);
                    }
                    if resp.double_clicked() {
                        select_chart(d, i);
                        *confirm = true;
                    }
                }
            });
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            let label = choices.get(d.list_index).map(|c| c.2.clone()).unwrap_or_default();
            ui.strong(label);
            let (rect, _) = ui.allocate_exact_size(vec2(360.0, 216.0), Sense::hover());
            ui.painter().rect_filled(rect, 2.0, Color32::WHITE);
            ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
            let params = json!({"type": d.values.get("type").cloned().unwrap_or(Json::Null), "subtype": d.values.get("subtype").cloned().unwrap_or(Json::Null)});
            if let Some(doc) = app.session.active()
                && let Ok((sheet, chart)) = gridcraft_engine::cmd::insert::build_chart(&app.session, &params)
            {
                crate::chartview::paint_chart(ui.painter(), rect, &doc.wb, sheet, &chart);
            }
        });
    });
    ok_cancel(ui, confirm, open);
    if *confirm {
        *confirm = false;
        let params =
            json!({"type": d.values.get("type").cloned().unwrap_or(Json::Null), "subtype": d.values.get("subtype").cloned().unwrap_or(Json::Null)});
        match app.run("insert.chart", params) {
            Ok(r) => {
                // The new chart is selected, as after inserting any object.
                app.selected_chart = r.get("chart").and_then(Json::as_u64).and_then(|id| u32::try_from(id).ok());
                *open = false;
            }
            Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
        }
    }
}

fn select_data(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let text = |d: &Dialog, k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let edit = |ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str| {
        ui.label(label);
        let mut v = text(d, key);
        if ui.add(egui::TextEdit::singleline(&mut v).desired_width(420.0)).changed() {
            d.values.insert(key.into(), json!(v));
        }
    };
    edit(ui, d, "range", "Chart data range:");
    ui.add_space(6.0);
    ui.strong("Legend Entries (Series)");
    let names: Vec<String> =
        d.values.get("names").and_then(Json::as_array).map(|a| a.iter().map(|n| n.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let mut remove: Vec<u64> =
        d.values.get("remove").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_u64).collect()).unwrap_or_default();
    let kept = names.len().saturating_sub(remove.len());
    egui::ScrollArea::vertical().id_salt("series").max_height(160.0).show(ui, |ui| {
        for (i, name) in names.iter().enumerate() {
            let i = i as u64;
            let removed = remove.contains(&i);
            ui.horizontal(|ui| {
                let label = if name.is_empty() { format!("Series{}", i + 1) } else { name.clone() };
                ui.label(if removed { egui::RichText::new(label).strikethrough().weak() } else { egui::RichText::new(label) });
                if removed {
                    if ui.small_button("Keep").clicked() {
                        remove.retain(|x| *x != i);
                    }
                } else if ui.add_enabled(kept > 1, egui::Button::new("Remove").small()).clicked() {
                    remove.push(i);
                }
            });
        }
    });
    d.values.insert("remove".into(), json!(remove));
    ui.add_space(6.0);
    edit(ui, d, "categories", "Horizontal (Category) Axis Labels:");
    ok_cancel(ui, confirm, open);
    if *confirm {
        *confirm = false;
        let mut params = serde_json::Map::new();
        params.insert("chart".into(), d.values.get("chart").cloned().unwrap_or(Json::Null));
        let range_changed = text(d, "range") != text(d, "range0");
        if range_changed {
            params.insert("range".into(), json!(text(d, "range")));
        } else if !remove.is_empty() {
            // A new range rebuilds the series; removals only apply to the current ones.
            params.insert("removeSeries".into(), json!(remove));
        }
        if text(d, "categories") != text(d, "categories0") || (range_changed && !text(d, "categories").is_empty()) {
            params.insert("categories".into(), json!(text(d, "categories")));
        }
        if params.len() == 1 {
            *open = false;
            return;
        }
        match app.run("chart.selectData", Json::Object(params)) {
            Ok(_) => *open = false,
            Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
        }
    }
}

fn manage_rules(app: &mut SheetApp, ui: &mut egui::Ui) {
    let list = app.session.run("home.manageRules", json!({})).unwrap_or(Json::Null);
    let rules = list["rules"].as_array().cloned().unwrap_or_default();
    if rules.is_empty() {
        ui.label("There are no conditional formatting rules on this sheet.");
    }
    for (i, r) in rules.iter().enumerate() {
        ui.horizontal(|ui| {
            let kind = r["rule"].as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_default();
            ui.label(format!(
                "{}. {kind}  applies to {}",
                i + 1,
                r["ranges"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default()
            ));
            if ui.small_button("▲").clicked() {
                app.run_or_alert("home.manageRules", json!({"moveUp": i}));
            }
            if ui.small_button("▼").clicked() {
                app.run_or_alert("home.manageRules", json!({"moveDown": i}));
            }
            if ui.small_button("Delete").clicked() {
                app.run_or_alert("home.manageRules", json!({"delete": i}));
            }
        });
    }
}

fn command_search(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let (down, up, home, end, pgup, pgdn) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::CTRL, Key::Home),
            i.consume_key(egui::Modifiers::CTRL, Key::End),
            i.consume_key(egui::Modifiers::NONE, Key::PageUp),
            i.consume_key(egui::Modifiers::NONE, Key::PageDown),
        )
    });
    let width = ui.available_width();
    let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(width).hint_text("Type a command, e.g. freeze, chart, bold…"));

    if !d.focus_initialized {
        r.request_focus();
        d.focus_initialized = true;
    }

    let q = d.search.to_ascii_lowercase();
    let cmds: Vec<(String, String, String)> = app
        .session
        .commands()
        .into_iter()
        .filter(|c| c.enabled)
        .filter(|c| q.is_empty() || c.label.to_ascii_lowercase().contains(&q) || c.id.to_ascii_lowercase().contains(&q))
        .map(|c| (c.id.to_string(), c.label.to_string(), c.menu.join(" › ")))
        .take(40)
        .collect();

    let last = cmds.len().saturating_sub(1);

    let row_h = ui.spacing().interact_size.y.max(20.0);
    let row_step = row_h + ui.spacing().item_spacing.y;

    const LIST_HEIGHT: f32 = 320.0;

    let page_size = ((LIST_HEIGHT / row_step).floor() as usize).saturating_sub(1).max(1);

    if down {
        d.list_index = (d.list_index + 1).min(last);
    }
    if up {
        d.list_index = d.list_index.saturating_sub(1);
    }
    if pgdn {
        d.list_index = (d.list_index + page_size).min(last);
    }
    if pgup {
        d.list_index = d.list_index.saturating_sub(page_size);
    }
    if home {
        d.list_index = 0;
    }
    if end {
        d.list_index = last;
    }

    d.list_index = d.list_index.min(last);

    egui::ScrollArea::vertical().max_height(LIST_HEIGHT).auto_shrink([true, false]).show(ui, |ui| {
        let scroll_area_width = ui.available_width();
        for (i, (id, label, menu)) in cmds.iter().enumerate() {
            let r = ui.add(egui::Button::selectable(i == d.list_index, format!("{label}    {menu}")).min_size(vec2(scroll_area_width, 20.0)));

            if (down || up || home || end || pgup || pgdn) && i == d.list_index && !ui.clip_rect().contains_rect(r.rect) {
                r.scroll_to_me(Some(if down || end || pgdn { egui::Align::Max } else { egui::Align::Min }));
            }

            if r.clicked() {
                d.list_index = i;
                *confirm = true;
            }

            r.on_hover_text(id);
        }
    });
    if ui.input(|i| i.key_pressed(Key::Enter)) {
        *confirm = true;
    }
    if *confirm && let Some((id, ..)) = cmds.get(d.list_index) {
        let id = id.clone();
        app.dialog = None;
        app.run_or_alert(&id, json!({}));
    }
}

fn goal_seek(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let get = |k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let (mut set, mut to, mut changing) = (get("set"), get("to"), get("changing"));
    egui::Grid::new("gs").show(ui, |ui| {
        ui.label("Set cell:");
        ui.text_edit_singleline(&mut set);
        ui.end_row();
        ui.label("To value:");
        ui.text_edit_singleline(&mut to);
        ui.end_row();
        ui.label("By changing cell:");
        ui.text_edit_singleline(&mut changing);
        ui.end_row();
    });
    d.values.insert("set".into(), json!(set.clone()));
    d.values.insert("to".into(), json!(to.clone()));
    d.values.insert("changing".into(), json!(changing.clone()));
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        match app.run("data.goalSeek", json!({"set": set, "to": to.parse::<f64>().unwrap_or(0.0), "changing": changing})) {
            Ok(r) => app.toast = Some((format!("Goal Seeking found a solution: {}", r["value"]), crate::now_ms())),
            Err(e) => app.message = Some(("Goal Seek".into(), crate::clean_error(&e))),
        }
    }
    if !open {
        *confirm = true;
    }
}

/// The fields of one Analysis ToolPak tool's form (input and options; output is drawn apart).
fn toolpak_fields(tool: &str) -> Vec<Field> {
    use Field::*;
    let grouped = || Choice { key: "groupedBy", label: "Grouped By:", options: vec![("columns", "Columns"), ("rows", "Rows")] };
    let labels = || Check { key: "labels", label: "Labels in first row" };
    let range = || Text { key: "range", label: "Input Range:" };
    let two = || vec![Text { key: "range1", label: "Variable 1 Range:" }, Text { key: "range2", label: "Variable 2 Range:" }];
    match tool {
        "descriptive" => vec![
            range(),
            grouped(),
            labels(),
            Check { key: "summary", label: "Summary statistics" },
            Text { key: "confidence", label: "Confidence Level for Mean (%), blank for none:" },
            Text { key: "kthLargest", label: "Kth Largest (blank for none):" },
            Text { key: "kthSmallest", label: "Kth Smallest (blank for none):" },
        ],
        "histogram" => vec![
            range(),
            Text { key: "bins", label: "Bin Range (blank for automatic bins):" },
            labels(),
            Check { key: "pareto", label: "Pareto (sorted histogram)" },
            Check { key: "cumulative", label: "Cumulative Percentage" },
        ],
        "correlation" | "covariance" | "rankPercentile" => vec![range(), grouped(), labels()],
        "anovaSingle" => vec![range(), grouped(), labels(), Text { key: "alpha", label: "Alpha:" }],
        "regression" => vec![
            Text { key: "inputY", label: "Input Y Range:" },
            Text { key: "inputX", label: "Input X Range:" },
            labels(),
            Check { key: "constantZero", label: "Constant is Zero" },
            Text { key: "confidence", label: "Confidence Level (%):" },
            Check { key: "residuals", label: "Residuals" },
            Check { key: "standardizedResiduals", label: "Standardized Residuals" },
            Check { key: "normalProbability", label: "Normal Probability" },
        ],
        "movingAverage" => {
            vec![range(), labels(), Text { key: "interval", label: "Interval:" }, Check { key: "standardErrors", label: "Standard Errors" }]
        }
        "exponentialSmoothing" => {
            vec![range(), labels(), Text { key: "damping", label: "Damping factor:" }, Check { key: "standardErrors", label: "Standard Errors" }]
        }
        "fTest" => {
            let mut v = two();
            v.extend([labels(), Text { key: "alpha", label: "Alpha:" }]);
            v
        }
        _ => {
            let mut v = two();
            v.extend([
                Text { key: "hypothesizedDifference", label: "Hypothesized Mean Difference:" },
                labels(),
                Text { key: "alpha", label: "Alpha:" },
            ]);
            v
        }
    }
}

/// The starting values of a tool's form: the selection as its input, a new sheet as output.
fn toolpak_defaults(sel: &str, tool: &str) -> Map<String, Json> {
    let mut v = json!({
        "tool": tool, "range": sel, "inputY": "", "inputX": "", "range1": sel, "range2": "", "bins": "",
        "groupedBy": "columns", "labels": false, "summary": true, "confidence": "", "kthLargest": "", "kthSmallest": "",
        "interval": "3", "damping": "0.3", "alpha": "0.05", "hypothesizedDifference": "0",
        "outputTo": "newSheet", "output": "", "newSheet": "",
    });
    if tool == "regression" {
        v["confidence"] = json!("95");
        v["inputY"] = json!(sel);
    }
    v.as_object().cloned().unwrap_or_default()
}

/// Data › Data Analysis: pick a tool, then fill in its form. OK runs `data.analysisToolPak`
/// (through the dialog's command); a report that would replace data asks first.
fn analysis_toolpak(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    use gridcraft_engine::cmd::analysis::TOOLS;
    if d.values.get("askOverwrite").and_then(Json::as_bool) == Some(true) {
        ui.label("The output range already holds data. Replace it with the report?");
        ui.horizontal(|ui| {
            if ui.button("Replace").clicked() {
                d.values.remove("askOverwrite");
                d.values.insert("overwrite".into(), json!(true));
                *confirm = true;
            }
            if ui.button("Cancel").clicked() {
                d.values.remove("askOverwrite");
            }
        });
        return;
    }
    let tool = d.values.get("tool").and_then(Json::as_str).unwrap_or("").to_string();
    let Some((_, label)) = TOOLS.iter().find(|(id, _)| *id == tool) else {
        ui.label(egui::RichText::new("Analysis Tools").strong());
        let pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
        let mut chosen = false;
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (id, label) in TOOLS {
                let r = ui.selectable_label(pick == *id, *label);
                if r.clicked() {
                    d.values.insert("pick".into(), json!(id));
                }
                if r.double_clicked() {
                    chosen = true;
                }
            }
        });
        let mut ok = false;
        ok_cancel(ui, &mut ok, open);
        let pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
        if (ok || chosen) && TOOLS.iter().any(|(id, _)| *id == pick) {
            let sel = app.session.active().map(|doc| doc.selection.current().a1()).unwrap_or_default();
            d.values = toolpak_defaults(&sel, &pick);
        }
        return;
    };
    d.title = label.to_string();
    d.fields = toolpak_fields(&tool);
    form_fields(ui, d);
    ui.separator();
    ui.label(egui::RichText::new("Output options").strong());
    let to = d.values.get("outputTo").and_then(Json::as_str).unwrap_or("newSheet").to_string();
    for (key, choice, text_label) in [("output", "range", "Output Range:"), ("newSheet", "newSheet", "New Worksheet Ply:")] {
        ui.horizontal(|ui| {
            if ui.radio(to == choice, text_label).clicked() {
                d.values.insert("outputTo".into(), json!(choice));
            }
            let mut t = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
            if ui.add(egui::TextEdit::singleline(&mut t).desired_width(160.0)).changed() {
                d.values.insert(key.into(), json!(t));
                d.values.insert("outputTo".into(), json!(choice));
            }
        });
    }
    ok_cancel(ui, confirm, open);
}

/// Data › Solver: the Solver Parameters form. Solve runs `data.solver` through the dialog's
/// command; the result opens Solver Results.
fn solver(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let text = |d: &Dialog, k: &str| d.values.get(k).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or_default();
    let edit = |ui: &mut egui::Ui, d: &mut Dialog, k: &str, width: f32| {
        let mut t = text(d, k);
        if ui.add(egui::TextEdit::singleline(&mut t).desired_width(width)).changed() {
            d.values.insert(k.into(), json!(t));
        }
    };
    ui.horizontal(|ui| {
        ui.label("Set Objective:");
        edit(ui, d, "objective", 200.0);
    });
    let goal = text(d, "goal");
    ui.horizontal(|ui| {
        ui.label("To:");
        for (g, label) in [("max", "Max"), ("min", "Min"), ("value", "Value Of:")] {
            if ui.radio(goal == g, label).clicked() {
                d.values.insert("goal".into(), json!(g));
            }
        }
        edit(ui, d, "value", 80.0);
    });
    ui.label("By Changing Variable Cells:");
    edit(ui, d, "variables", 420.0);
    ui.label("Subject to the Constraints:");
    let list: Vec<String> = d
        .values
        .get("constraints")
        .and_then(Json::as_array)
        .map(|a| a.iter().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string())).collect())
        .unwrap_or_default();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_size(vec2(420.0, 90.0));
        egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
            for (i, c) in list.iter().enumerate() {
                if ui.selectable_label(d.list_index == i, c).clicked() {
                    d.list_index = i;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        edit(ui, d, "newLhs", 110.0);
        let op = text(d, "newOp");
        egui::ComboBox::from_id_salt("solverOp").selected_text(op.clone()).width(60.0).show_ui(ui, |ui| {
            for o in ["<=", "=", ">=", "int", "bin"] {
                if ui.selectable_label(op == o, o).clicked() {
                    d.values.insert("newOp".into(), json!(o));
                }
            }
        });
        if !matches!(op.as_str(), "int" | "bin") {
            edit(ui, d, "newRhs", 110.0);
        }
        if ui.button("Add").clicked() {
            let lhs = text(d, "newLhs");
            if !lhs.trim().is_empty() {
                let rhs = if matches!(op.as_str(), "int" | "bin") { String::new() } else { format!(" {}", text(d, "newRhs").trim()) };
                let mut list = list.clone();
                list.push(format!("{} {op}{rhs}", lhs.trim()));
                d.values.insert("constraints".into(), json!(list));
                d.values.insert("newLhs".into(), json!(""));
                d.values.insert("newRhs".into(), json!(""));
            }
        }
        if ui.add_enabled(d.list_index < list.len(), egui::Button::new("Delete")).clicked() {
            let mut list = list.clone();
            if d.list_index < list.len() {
                list.remove(d.list_index);
            }
            d.values.insert("constraints".into(), json!(list));
        }
    });
    let mut nonneg = d.values.get("nonNegative").and_then(Json::as_bool).unwrap_or(true);
    if ui.checkbox(&mut nonneg, "Make Unconstrained Variables Non-Negative").changed() {
        d.values.insert("nonNegative".into(), json!(nonneg));
    }
    let method = text(d, "method");
    ui.horizontal(|ui| {
        ui.label("Select a Solving Method:");
        let label = |m: &str| match m {
            "simplex" => "Simplex LP",
            "evolutionary" => "Evolutionary",
            _ => "GRG Nonlinear",
        };
        egui::ComboBox::from_id_salt("solverMethod").selected_text(label(&method)).show_ui(ui, |ui| {
            for m in ["grg", "simplex", "evolutionary"] {
                if ui.selectable_label(method == m, label(m)).clicked() {
                    d.values.insert("method".into(), json!(m));
                }
            }
        });
    });
    ui.label(
        egui::RichText::new(match method.as_str() {
            "simplex" => "Simplex LP: for linear models; integer and binary constraints by branch and bound.",
            "evolutionary" => "Evolutionary: a genetic search for models that aren't smooth; every variable cell needs bounds.",
            _ => "GRG Nonlinear: a local optimum of a smooth model, from the current values.",
        })
        .small(),
    );
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let solve =
            ui.add(egui::Button::new(egui::RichText::new("  Solve  ").color(Color32::WHITE)).fill(crate::theme::Tokens::get(ui.ctx()).accent));
        if solve.clicked() && !text(d, "objective").trim().is_empty() {
            *confirm = true;
        }
        if ui.button(" Close ").clicked() {
            *open = false;
        }
    });
}

/// Solver Results: the outcome, and keeping or restoring the variable cells.
fn solver_results(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let message = d.values.get("message").and_then(Json::as_str).unwrap_or("").to_string();
    ui.label(message);
    let changed = d.values.get("changed").and_then(Json::as_bool).unwrap_or(false);
    let restore = d.values.get("restore").and_then(Json::as_bool).unwrap_or(false);
    if changed {
        ui.add_space(4.0);
        if ui.radio(!restore, "Keep Solver Solution").clicked() {
            d.values.insert("restore".into(), json!(false));
        }
        if ui.radio(restore, "Restore Original Values").clicked() {
            d.values.insert("restore".into(), json!(true));
        }
    }
    // Reports describe the solution whether it is kept or not.
    let solved = d.values.get("values").and_then(Json::as_object).is_some_and(|v| !v.is_empty());
    if solved {
        ui.add_space(4.0);
        ui.label("Reports:");
        for (key, label) in [("answerReport", "Answer"), ("limitsReport", "Limits")] {
            let mut on = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
            if ui.checkbox(&mut on, label).changed() {
                d.values.insert(key.into(), json!(on));
            }
        }
    }
    let mut confirm = false;
    ok_cancel(ui, &mut confirm, open);
    if confirm {
        // The solution (and the saved model) is one undo step.
        if changed && restore {
            app.run_or_alert("edit.undo", json!({}));
        }
        let reports: Vec<&str> = [("answerReport", "answer"), ("limitsReport", "limits")]
            .into_iter()
            .filter(|(key, _)| d.values.get(*key).and_then(Json::as_bool) == Some(true))
            .map(|(_, kind)| kind)
            .collect();
        if solved && !reports.is_empty() {
            let field = |k: &str| d.values.get(k).cloned().unwrap_or(Json::Null);
            let params = json!({"reports": reports, "model": field("model"), "values": field("values"), "original": field("original"), "message": field("message")});
            app.run_or_alert("data.solverReport", params);
        }
        *open = false;
    }
}

fn spelling(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let r = d.result.clone().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(app.ui.language.tr_error(e));
        return;
    }
    let issues = r["issues"].as_array().cloned().unwrap_or_default();
    let Some(it) = issues.get(d.list_index) else {
        ui.label("The spelling check is complete for the entire sheet.");
        if ui.button("OK").clicked() {
            *open = false;
        }
        return;
    };
    let word = it["word"].as_str().unwrap_or("").to_string();
    let cell = it["cell"].as_str().unwrap_or("").to_string();
    ui.label(format!("Not in Dictionary ({cell}):"));
    ui.label(egui::RichText::new(&word).strong().color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
    let sugg: Vec<String> =
        it["suggestions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    ui.label("Suggestions:");
    let mut pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
    for s in &sugg {
        if ui.selectable_label(pick == *s, s).clicked() {
            pick = s.clone();
        }
    }
    if sugg.is_empty() {
        ui.label(egui::RichText::new("(No suggestions)").italics());
    }
    d.values.insert("pick".into(), json!(pick.clone()));
    let _ = app.session.run("selection.set", json!({"cell": cell}));
    ui.horizontal(|ui| {
        if ui.button("Ignore Once").clicked() {
            d.list_index += 1;
        }
        if ui.button("Add to Dictionary").clicked() {
            let _ = app.session.run("review.addToDictionary", json!({"word": word}));
            d.list_index += 1;
        }
        if !pick.is_empty() {
            if ui.button("Change").clicked() {
                app.run_or_alert("review.changeSpelling", json!({"cell": cell, "word": word, "to": pick}));
                d.list_index += 1;
            }
            if ui.button("Change All").clicked() {
                app.run_or_alert("review.changeSpelling", json!({"word": word, "to": pick, "all": true}));
                d.list_index += 1;
            }
        }
    });
}

#[cfg(test)]
mod data_validation_tests {
    use super::*;

    #[test]
    fn list_uses_one_source_and_does_not_submit_a_second_list() {
        let fields = data_validation_fields("list", "between");
        assert!(fields.iter().any(|f| matches!(f, Field::Text { key: "formula1", label: "Source:" })));
        assert!(!fields.iter().any(|f| matches!(f, Field::Text { key: "formula2", .. } | Field::Choice { key: "operator", .. })));

        let dialog = Dialog::form(
            "dataValidation",
            "Data Validation",
            "data.validation",
            fields,
            json!({"type": "list", "operator": "between", "formula1": "ABC,DEF", "formula2": "GHI,JKL", "dropdown": true}),
        );
        let params = build_params(&dialog);
        assert_eq!(params["formula1"], "ABC,DEF");
        assert!(params.get("formula2").is_none());
        assert!(params.get("operator").is_none());
    }

    #[test]
    fn only_between_operators_show_and_submit_two_bounds() {
        let two = data_validation_fields("whole", "between");
        assert!(two.iter().any(|f| matches!(f, Field::Text { key: "formula2", label: "Maximum:" })));
        let one = data_validation_fields("date", "lessOrEqual");
        assert!(one.iter().any(|f| matches!(f, Field::Text { key: "formula1", label: "End date:" })));
        assert!(!one.iter().any(|f| matches!(f, Field::Text { key: "formula2", .. })));

        let dialog = Dialog::form(
            "dataValidation",
            "Data Validation",
            "data.validation",
            one,
            json!({"type": "date", "operator": "lessOrEqual", "formula1": "2026-10-12", "formula2": "2030-01-01"}),
        );
        let params = build_params(&dialog);
        assert_eq!(params["operator"], "lessOrEqual");
        assert_eq!(params["formula1"], "2026-10-12");
        assert!(params.get("formula2").is_none());
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
