//! Dialogs. Simple dialogs are forms (fields → a command with params); Format Cells, Insert
//! Function, Find & Replace, Sort, Name Manager and the command palette are custom.
//!
//! Every dialog is also reachable programmatically: `ui.dialog {name}` opens one,
//! `ui.dialog.set {field, value}` edits a field and `ui.dialog.confirm` presses OK.

use egui::{Color32, Key, vec2};
use serde_json::{Map, Value as Json, json};

use crate::SheetApp;
use crate::theme;

#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Text { key: &'static str, label: &'static str },
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
    picture_inbox: Option<crate::Inbox>,
    picture_target: Option<(u64, usize)>,
}

impl Dialog {
    pub fn name(&self) -> &str {
        &self.name
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
            picture_inbox: None,
            picture_target: None,
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
            picture_inbox: None,
            picture_target: None,
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
                    Text { key: "password", label: "Password (optional):" },
                    Note("Allow all users of this sheet to:".into()),
                    Check { key: "formatCells", label: "Format cells" },
                    Check { key: "formatColumns", label: "Format columns" },
                    Check { key: "formatRows", label: "Format rows" },
                    Check { key: "insertRows", label: "Insert rows" },
                    Check { key: "deleteRows", label: "Delete rows" },
                    Check { key: "sort", label: "Sort" },
                    Check { key: "autofilter", label: "Use AutoFilter" },
                ],
                json!({}),
            ),
            "unprotectSheet" => Dialog::form(
                "unprotectSheet",
                "Unprotect Sheet",
                "review.unprotectSheet",
                vec![Text { key: "password", label: "Password:" }],
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
                json!({"direction": "columns", "type": "linear", "dateUnit": "day", "step": 1}),
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
                json!({"header": "", "footer": "&CPage &P of &N"}),
            ),
            "pageSetup" => Dialog::form(
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
                json!({"left": 0.7, "right": 0.7, "top": 0.75, "bottom": 0.75, "header": 0.3, "footer": 0.3}),
            ),
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
                vec![Check { key: "header", label: "My data has headers" }, Text { key: "columnsText", label: "Columns (blank = all, e.g. A,C):" }],
                json!({"header": true}),
            ),
            "goalSeek" => Dialog::custom("goalSeek", "Goal Seek", json!({"set": active, "to": "0", "changing": ""})),
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
            "zoom" => Dialog::form(
                "zoom",
                "Zoom",
                "view.zoom",
                vec![
                    Choice {
                        key: "preset",
                        label: "Magnification",
                        options: vec![("200", "200%"), ("100", "100%"), ("75", "75%"), ("50", "50%"), ("25", "25%")],
                    },
                    Number { key: "percent", label: "Custom (%):" },
                ],
                json!({"percent": app.session.active().and_then(|d| d.wb.active().map(|s| s.zoom)).unwrap_or(100)}),
            ),
            "defineName" => Dialog::form(
                "defineName",
                "New Name",
                "formulas.defineName",
                vec![
                    Text { key: "name", label: "Name:" },
                    Text { key: "scope", label: "Scope (Workbook or sheet name):" },
                    Text { key: "comment", label: "Comment:" },
                    Text { key: "refersTo", label: "Refers to:" },
                ],
                json!({"scope": "Workbook", "refersTo": app.session.active().and_then(|d| d.wb.active().map(|s| format!("={}!{}", gridcraft_engine::formula::quote_sheet(&s.name), abs(&d.selection.current()))))}),
            ),
            "dataValidation" => Dialog::form(
                "dataValidation",
                "Data Validation",
                "data.validation",
                vec![
                    Choice {
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
                    },
                    Choice {
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
                    },
                    Text { key: "formula1", label: "Minimum / Source / Formula:" },
                    Text { key: "formula2", label: "Maximum:" },
                    Check { key: "ignoreBlank", label: "Ignore blank" },
                    Check { key: "dropdown", label: "In-cell dropdown" },
                    Text { key: "inputMessage", label: "Input message:" },
                    Choice {
                        key: "errorStyle",
                        label: "Error style",
                        options: vec![("stop", "Stop"), ("warning", "Warning"), ("information", "Information")],
                    },
                    Text { key: "errorMessage", label: "Error message:" },
                ],
                json!({"type": "list", "operator": "between", "ignoreBlank": true, "dropdown": true, "errorStyle": "stop"}),
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
            "sort" => Dialog::custom("sort", "Sort", json!({"header": true, "levels": [{"column": "A", "order": "asc"}]})),
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
            "journal" => Dialog::custom("journal", "Action Journal", json!({})),
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
            "saveChanges" => Dialog::custom("saveChanges", "Save Changes?", json!({"title": p("title")})),
            "start" => Dialog::custom("start", "GridCraft", json!({})),
            "comments" => {
                app.grid.pane = Some("comments".into());
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

fn abs(r: &gridcraft_engine::core::RangeRef) -> String {
    let a = |c: gridcraft_engine::core::CellRef| format!("${}${}", gridcraft_engine::core::col_to_letters(c.col), c.row + 1);
    if r.is_single() { a(r.start) } else { format!("{}:{}", a(r.start), a(r.end)) }
}

/// Builds the command params from form values for dialogs that need translation.
fn build_params(d: &Dialog) -> Json {
    let mut v = Json::Object(d.values.clone());
    match d.name.as_str() {
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
        "zoom" => {
            if let Some(p) = d.values.get("preset").and_then(Json::as_str)
                && d.values.get("percent").is_none()
            {
                v["percent"] = json!(p.parse::<f64>().unwrap_or(100.0));
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
            if d.values.get("scope").and_then(Json::as_str) == Some("Workbook")
                && let Some(m) = v.as_object_mut()
            {
                m.remove("scope");
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
        "insertFunction" | "commandSearch" | "nameManager" | "manageRules" | "journal" | "agents" => 520.0,
        _ => 380.0,
    };
    let window = egui::Window::new(d.title.clone())
        .id(egui::Id::new("dialog").with(&d.name))
        .collapsible(false)
        .resizable(false)
        .default_width(width)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0))
        .open(&mut win_open);
    let window = if d.name == "insertPicture" { window.default_height(240.0) } else { window };
    window.show(ctx, |ui| {
            ui.set_min_width(width - 20.0);
            match d.name.as_str() {
                "formatCells" => format_cells(app, ui, &mut d, &mut confirm),
                "insertFunction" => insert_function(app, ui, &mut d, &mut confirm),
                "find" => find(app, ui, &mut d),
                "goTo" => go_to(app, ui, &mut d, &mut confirm),
                "sort" => sort(app, ui, &mut d, &mut confirm),
                "nameManager" => name_manager(app, ui),
                "manageRules" => manage_rules(app, ui),
                "commandSearch" => command_search(app, ui, &mut d, &mut confirm),
                "goalSeek" => goal_seek(app, ui, &mut d, &mut confirm),
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
                        let target = if path.is_empty() { app.services.pick_save.as_ref().and_then(|f| f(&format!("Copy.{fmt}"))) } else { Some(path) };
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
                    ui.horizontal(|ui| {
                        if ui.button("Don't Save").clicked() {
                            app.run_or_alert("file.close", json!({"force": true}));
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
            Ok(_) => open = false,
            Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
        }
    } else if confirm {
        open = false;
    }
    if open {
        app.dialog = Some(d);
    }
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
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let ok = ui.add(egui::Button::new(egui::RichText::new("   OK   ").color(Color32::WHITE)).fill(crate::theme::Tokens::get(ui.ctx()).accent));
        if ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            *confirm = true;
        }
        if ui.button(" Cancel ").clicked() {
            *open = false;
        }
    });
}

fn form(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    egui::Grid::new("form").num_columns(1).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        for f in d.fields.clone() {
            match f {
                Field::Text { key, label } => {
                    ui.vertical(|ui| {
                        ui.label(label);
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
                        if r.changed() {
                            d.values.insert(key.into(), json!(s));
                        }
                    });
                }
                Field::Number { key, label } => {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let mut n = d.values.get(key).and_then(Json::as_f64).unwrap_or(0.0);
                        if ui.add(egui::DragValue::new(&mut n).speed(0.5)).changed() {
                            d.values.insert(key.into(), json!(n));
                        }
                    });
                }
                Field::Choice { key, label, options } => {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(label).strong());
                        let cur = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
                        for (val, text) in options {
                            if ui.radio(cur == val, text).clicked() {
                                d.values.insert(key.into(), json!(val));
                            }
                        }
                    });
                }
                Field::Check { key, label } => {
                    let mut b = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
                    if ui.checkbox(&mut b, label).changed() {
                        d.values.insert(key.into(), json!(b));
                    }
                }
                Field::Note(t) => {
                    ui.label(egui::RichText::new(t).small());
                }
            }
            ui.end_row();
        }
    });
    ok_cancel(ui, confirm, open);
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

fn format_cells(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    ui.horizontal(|ui| {
        for tab in ["Number", "Alignment", "Font", "Border", "Fill", "Protection"] {
            if ui.selectable_label(d.tab == tab, tab).clicked() {
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
                cols[0].label(egui::RichText::new("Category:").strong());
                for name in
                    ["General", "Number", "Currency", "Accounting", "Short Date", "Long Date", "Time", "Percentage", "Fraction", "Scientific", "Text"]
                {
                    let c = gridcraft_engine::cmd::format::format_code_for(name).to_string();
                    if cols[0].selectable_label(code == c, name).clicked() {
                        code = c;
                    }
                }
                cols[1].label(egui::RichText::new("Sample").strong());
                let preview = app.session.active().map(|doc| gridcraft_engine::display::format(&sample, &code, &doc.wb).text).unwrap_or_default();
                cols[1].label(egui::RichText::new(preview).font(theme::ui_font(15.0)));
                cols[1].add_space(8.0);
                cols[1].label("Type (custom format code):");
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
            ui.label(egui::RichText::new("Horizontal").strong());
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
                    ui.radio_value(&mut st.align.h, h, n);
                }
            });
            ui.label(egui::RichText::new("Vertical").strong());
            ui.horizontal_wrapped(|ui| {
                for (v, n) in [
                    (VAlign::Top, "Top"),
                    (VAlign::Center, "Center"),
                    (VAlign::Bottom, "Bottom"),
                    (VAlign::Justify, "Justify"),
                    (VAlign::Distributed, "Distributed"),
                ] {
                    ui.radio_value(&mut st.align.v, v, n);
                }
            });
            let mut indent = st.align.indent as f32;
            ui.add(egui::Slider::new(&mut indent, 0.0..=15.0).text("Indent"));
            st.align.indent = indent as u8;
            let mut rot = if st.align.rotation == 255 { 0.0 } else { st.align.rotation as f32 };
            ui.add(egui::Slider::new(&mut rot, -90.0..=90.0).text("Orientation (degrees)"));
            st.align.rotation = rot as i16;
            ui.checkbox(&mut st.align.wrap, "Wrap text");
            ui.checkbox(&mut st.align.shrink, "Shrink to fit");
        }
        "Font" => {
            egui::ComboBox::from_label("Font").selected_text(st.font.name.clone()).show_ui(ui, |ui| {
                for f in crate::ribbon::FONTS {
                    ui.selectable_value(&mut st.font.name, f.to_string(), *f);
                }
            });
            ui.add(egui::Slider::new(&mut st.font.size, 6.0..=72.0).text("Size"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.font.bold, "Bold");
                ui.checkbox(&mut st.font.italic, "Italic");
                ui.checkbox(&mut st.font.strike, "Strikethrough");
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
                    ui.radio_value(&mut st.font.underline, u, n);
                }
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.font.vert, VertAlign::Baseline, "Normal");
                ui.radio_value(&mut st.font.vert, VertAlign::Superscript, "Superscript");
                ui.radio_value(&mut st.font.vert, VertAlign::Subscript, "Subscript");
            });
            ui.label("Color:");
            let colors = app.session.active().map(|doc| doc.wb.theme.colors).unwrap_or_default();
            if let Some(c) = crate::widgets::color_palette(ui, &colors, "Automatic") {
                st.font.color =
                    if c == "none" { gridcraft_engine::model::Color::Auto } else { gridcraft_engine::model::Color::from_hex(&c).unwrap_or_default() };
            }
        }
        "Border" => {
            use gridcraft_engine::model::{BorderLine, BorderStyle};
            let line = BorderLine { style: BorderStyle::Thin, color: gridcraft_engine::model::Color::Auto };
            ui.horizontal(|ui| {
                if ui.button("None").clicked() {
                    st.borders = Default::default();
                }
                if ui.button("Outline").clicked() {
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
            let colors = app.session.active().map(|doc| doc.wb.theme.colors).unwrap_or_default();
            ui.label("Background Color:");
            if let Some(c) = crate::widgets::color_palette(ui, &colors, "No Color") {
                st.fill = if c == "none" {
                    Default::default()
                } else {
                    gridcraft_engine::model::Fill::solid(gridcraft_engine::model::Color::from_hex(&c).unwrap_or_default())
                };
            }
        }
        "Protection" => {
            ui.checkbox(&mut st.protection.locked, "Locked");
            ui.checkbox(&mut st.protection.hidden, "Hidden");
            ui.label(
                egui::RichText::new("Locking cells or hiding formulas has no effect until you protect the worksheet (Review › Protect Sheet).")
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

fn insert_function(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    ui.horizontal(|ui| {
        ui.label("Search:");
        let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(300.0).hint_text("e.g. lookup, average, date"));
        r.request_focus();
    });
    let locale = app.ui.language.formula_locale();
    let list = gridcraft_engine::cmd::formulas::function_list();
    let q = d.search.to_ascii_lowercase();
    let hits: Vec<&Json> = list
        .iter()
        .filter(|f| {
            q.is_empty()
                || f["name"].as_str().is_some_and(|n| locale.function_name(n).to_ascii_lowercase().contains(&q))
                || f["description"].as_str().is_some_and(|n| n.to_ascii_lowercase().contains(&q))
        })
        .take(300)
        .collect();
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (i, f) in hits.iter().enumerate() {
            let name = locale.function_name(f["name"].as_str().unwrap_or(""));
            let r = ui.add(
                egui::Button::selectable(i == d.list_index, format!("{name:<18} {}", f["category"].as_str().unwrap_or("")))
                    .min_size(vec2(480.0, 20.0)),
            );
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
        ui.separator();
        ui.label(egui::RichText::new(crate::formula_locale::signature(f["name"].as_str().unwrap_or(""), locale).unwrap_or_default()).strong());
        ui.label(crate::formula_locale::description(f["name"].as_str().unwrap_or(""), locale).unwrap_or_default());
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
        ui.add(egui::TextEdit::singleline(&mut what).desired_width(260.0));
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
    let mut remove = None;
    for (i, lv) in levels.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(if i == 0 { "Sort by" } else { "Then by" });
            let mut col = lv["column"].as_str().unwrap_or("A").to_string();
            ui.add(egui::TextEdit::singleline(&mut col).desired_width(50.0));
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
        levels.push(json!({"column": "B", "order": "asc"}));
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

fn name_manager(app: &mut SheetApp, ui: &mut egui::Ui) {
    let list = app.session.run("formulas.nameManager", json!({})).unwrap_or(Json::Null);
    egui::Grid::new("names").striped(true).num_columns(4).show(ui, |ui| {
        ui.strong("Name");
        ui.strong("Value");
        ui.strong("Refers To");
        ui.strong("Scope");
        ui.end_row();
        for n in list.as_array().cloned().unwrap_or_default() {
            ui.label(n["name"].as_str().unwrap_or(""));
            ui.label(n["value"].to_string().chars().take(30).collect::<String>());
            ui.label(n["refersTo"].as_str().unwrap_or(""));
            ui.horizontal(|ui| {
                ui.label(n["scope"].as_str().unwrap_or(""));
                if ui.small_button("Delete").clicked() {
                    app.run_or_alert("formulas.deleteName", json!({"name": n["name"], "scope": n["scope"]}));
                }
            });
            ui.end_row();
        }
    });
    if ui.button("New…").clicked() {
        app.open_dialog("defineName", json!({}));
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
    let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(480.0).hint_text("Type a command, e.g. freeze, chart, bold…"));
    r.request_focus();
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
    if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
        d.list_index = (d.list_index + 1).min(cmds.len().saturating_sub(1));
    }
    if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
        d.list_index = d.list_index.saturating_sub(1);
    }
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        for (i, (id, label, menu)) in cmds.iter().enumerate() {
            let r = ui.add(egui::Button::selectable(i == d.list_index, format!("{label}    {menu}")).min_size(vec2(480.0, 20.0)));
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

fn spelling(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let r = d.result.clone().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(crate::clean_error(e));
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
