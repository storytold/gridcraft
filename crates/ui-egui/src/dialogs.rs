//! Dialogs. Simple dialogs are forms (fields → a command with params); Format Cells, Insert
//! Function, Find & Replace, Sort, Name Manager and the command palette are custom.
//!
//! Every dialog is also reachable programmatically: `ui.dialog {name}` opens one,
//! `ui.dialog.set {field, value}` edits a field and `ui.dialog.confirm` presses OK.

use egui::{Color32, Key, vec2};
use serde_json::{Map, Value as Json, json};

use crate::SheetApp;
use crate::fnlist;
use crate::l10n::{Arg, Localizer, Tr, msg};
use crate::theme;

#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Text {
        key: &'static str,
        label: &'static str,
    },
    Number {
        key: &'static str,
        label: &'static str,
    },
    Choice {
        key: &'static str,
        label: &'static str,
        options: Vec<(&'static str, &'static str)>,
    },
    /// A drop-down over `(value, shown text)` pairs known only when the dialog opens.
    Combo {
        key: &'static str,
        label: &'static str,
        options: Vec<(String, String)>,
    },
    Check {
        key: &'static str,
        label: &'static str,
    },
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
        let l = app.l10n;
        let sel = app.session.active().map(|d| d.selection.current().a1()).unwrap_or_default();
        let active = app.session.active().map(|d| d.selection.active.a1()).unwrap_or_default();
        let p = |k: &str| params.get(k).cloned().unwrap_or(Json::Null);
        use Field::*;
        let d = match name {
            "insertCells" => Dialog::form(
                "insertCells",
                msg!("Insert"),
                "home.insertCells",
                vec![Choice {
                    key: "shift",
                    label: msg!("Insert"),
                    options: vec![
                        ("right", msg!("Shift cells right")),
                        ("down", msg!("Shift cells down")),
                        ("row", msg!("Entire row")),
                        ("column", msg!("Entire column")),
                    ],
                }],
                json!({"shift": "down"}),
            ),
            "deleteCells" => Dialog::form(
                "deleteCells",
                msg!("Delete"),
                "home.deleteCells",
                vec![Choice {
                    key: "shift",
                    label: msg!("Delete"),
                    options: vec![
                        ("left", msg!("Shift cells left")),
                        ("up", msg!("Shift cells up")),
                        ("row", msg!("Entire row")),
                        ("column", msg!("Entire column")),
                    ],
                }],
                json!({"shift": "up"}),
            ),
            "rowHeight" => Dialog::form(
                "rowHeight",
                msg!("Row Height"),
                "home.rowHeight",
                vec![Number { key: "height", label: msg!("Row height (pixels):") }],
                json!({"height": app.session.active().and_then(|d| d.wb.active().map(|s| s.row_height(d.selection.active.row))).unwrap_or(20.0)}),
            ),
            "columnWidth" => Dialog::form(
                "columnWidth",
                msg!("Column Width"),
                "home.columnWidth",
                vec![Number { key: "chars", label: msg!("Column width (characters):") }],
                json!({"chars": app.session.active().and_then(|d| d.wb.active().map(|s| gridcraft_engine::cmd::format::points_to_chars(s.col_width(d.selection.active.col) as f64))).unwrap_or(8.43)}),
            ),
            "defaultWidth" => Dialog::form(
                "defaultWidth",
                msg!("Standard Width"),
                "home.defaultWidth",
                vec![Number { key: "width", label: msg!("Standard column width (pixels):") }],
                json!({"width": 64}),
            ),
            "renameSheet" => Dialog::form(
                "renameSheet",
                msg!("Rename Sheet"),
                "sheet.rename",
                vec![Text { key: "name", label: "ui-dialogs-name-label" }],
                json!({"name": app.session.active().and_then(|d| d.wb.active().map(|s| s.name.clone()))}),
            ),
            "moveSheet" => {
                let sheets: Vec<String> = app.session.active().map(|d| d.wb.sheets.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
                let list = sheets.join(", ");
                let mut d = Dialog::form(
                    "moveSheet",
                    msg!("Move or Copy"),
                    "sheet.move",
                    vec![
                        Number { key: "to", label: msg!("Before sheet (position, 0 = first):") },
                        Check { key: "copy", label: msg!("Create a copy") },
                        Note(l.text("ui-dialogs-sheets-note", &[("list", Arg::from(list.as_str()))]).into_owned()),
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
                msg!("Protect Sheet"),
                "review.protectSheet",
                vec![
                    Text { key: "password", label: msg!("Password (optional):") },
                    Note(l.tr("Allow all users of this sheet to:").into_owned()),
                    Check { key: "formatCells", label: "ui-dialogs-check-format-cells" },
                    Check { key: "formatColumns", label: msg!("Format columns") },
                    Check { key: "formatRows", label: msg!("Format rows") },
                    Check { key: "insertRows", label: msg!("Insert rows") },
                    Check { key: "deleteRows", label: msg!("Delete rows") },
                    Check { key: "sort", label: msg!("Sort") },
                    Check { key: "autofilter", label: msg!("Use AutoFilter") },
                ],
                json!({}),
            ),
            "unprotectSheet" => Dialog::form(
                "unprotectSheet",
                msg!("Unprotect Sheet"),
                "review.unprotectSheet",
                vec![Text { key: "password", label: msg!("Password:") }],
                json!({}),
            ),
            "unhideSheet" => {
                let opts: Vec<String> =
                    p("sheets").as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                let first = opts.first().cloned().unwrap_or_default();
                let list = opts.join(", ");
                let mut d = Dialog::form(
                    "unhideSheet",
                    msg!("Unhide"),
                    "sheet.unhide",
                    vec![
                        Text { key: "sheet", label: msg!("Unhide sheet:") },
                        Note(l.text("ui-dialogs-hidden-note", &[("list", Arg::from(list.as_str()))]).into_owned()),
                    ],
                    json!({"sheet": first}),
                );
                d.values.insert("sheet".into(), json!(first));
                d
            }
            "pasteSpecial" => Dialog::form(
                "pasteSpecial",
                msg!("Paste Special"),
                "edit.pasteSpecial",
                vec![
                    Choice {
                        key: "what",
                        label: msg!("Paste"),
                        options: vec![
                            ("all", msg!("All")),
                            ("formulas", msg!("Formulas")),
                            ("values", msg!("Values")),
                            ("formats", msg!("Formats")),
                            ("comments", msg!("Comments and notes")),
                            ("validation", msg!("Validation")),
                            ("allExceptBorders", msg!("All except borders")),
                            ("columnWidths", msg!("Column widths")),
                            ("formulasAndNumberFormats", msg!("Formulas and number formats")),
                            ("valuesAndNumberFormats", msg!("Values and number formats")),
                        ],
                    },
                    Choice {
                        key: "operation",
                        label: msg!("Operation"),
                        options: vec![
                            ("none", msg!("None")),
                            ("add", msg!("Add")),
                            ("subtract", msg!("Subtract")),
                            ("multiply", msg!("Multiply")),
                            ("divide", msg!("Divide")),
                        ],
                    },
                    Check { key: "skipBlanks", label: msg!("Skip blanks") },
                    Check { key: "transpose", label: msg!("Transpose") },
                    Check { key: "link", label: msg!("Paste Link") },
                ],
                json!({"what": "all", "operation": "none"}),
            ),
            "series" => Dialog::form(
                "series",
                msg!("Series"),
                "edit.fillSeries",
                vec![
                    Choice { key: "direction", label: msg!("Series in"), options: vec![("columns", msg!("Columns")), ("rows", msg!("Rows"))] },
                    Choice {
                        key: "type",
                        label: msg!("Type"),
                        options: vec![("linear", msg!("Linear")), ("growth", msg!("Growth")), ("date", msg!("Date"))],
                    },
                    Choice {
                        key: "dateUnit",
                        label: msg!("Date unit"),
                        options: vec![("day", msg!("Day")), ("weekday", msg!("Weekday")), ("month", msg!("Month")), ("year", msg!("Year"))],
                    },
                    Number { key: "step", label: msg!("Step value:") },
                    Text { key: "stop", label: msg!("Stop value:") },
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
                    if threaded { msg!("New Comment") } else { msg!("New Note") },
                    if threaded { "review.newComment" } else { "review.newNote" },
                    vec![Text { key: "text", label: if threaded { msg!("Start a conversation:") } else { msg!("Note:") } }],
                    json!({"text": existing}),
                )
            }
            "insertLink" => Dialog::form(
                "insertLink",
                msg!("Insert Hyperlink"),
                "insert.link",
                vec![
                    Text { key: "text", label: msg!("Display:") },
                    Text { key: "target", label: msg!("Link to (web address, e-mail or Sheet!A1):") },
                    Text { key: "tooltip", label: msg!("ScreenTip:") },
                ],
                json!({"target": "https://"}),
            ),
            "sparkline" => Dialog::form(
                "sparkline",
                msg!("Create Sparklines"),
                "insert.sparkline",
                vec![
                    Text { key: "range", label: msg!("Data Range:") },
                    Text { key: "location", label: msg!("Location Range:") },
                    Check { key: "markers", label: msg!("Markers") },
                ],
                json!({"range": sel, "location": active, "type": p("type")}),
            ),
            "headerFooter" => Dialog::form(
                "headerFooter",
                msg!("Header & Footer"),
                "pageLayout.headerFooter",
                vec![
                    Text { key: "header", label: msg!("Header (&L left, &C center, &R right; &P page, &N pages, &D date, &A sheet):") },
                    Text { key: "footer", label: msg!("Footer:") },
                ],
                json!({"header": "", "footer": l.text("ui-dialogs-default-footer", &[])}),
            ),
            "pageSetup" => Dialog::form(
                "pageSetup",
                msg!("Page Setup"),
                "pageLayout.margins",
                vec![
                    Number { key: "left", label: msg!("Left margin (in):") },
                    Number { key: "right", label: msg!("Right margin (in):") },
                    Number { key: "top", label: msg!("Top margin (in):") },
                    Number { key: "bottom", label: msg!("Bottom margin (in):") },
                    Number { key: "header", label: msg!("Header (in):") },
                    Number { key: "footer", label: msg!("Footer (in):") },
                ],
                json!({"left": 0.7, "right": 0.7, "top": 0.75, "bottom": 0.75, "header": 0.3, "footer": 0.3}),
            ),
            "textToColumns" => Dialog::form(
                "textToColumns",
                msg!("Convert Text to Columns"),
                "data.textToColumns",
                vec![
                    Check { key: "tab", label: msg!("Tab") },
                    Check { key: "semicolon", label: msg!("Semicolon") },
                    Check { key: "comma", label: msg!("Comma") },
                    Check { key: "space", label: msg!("Space") },
                    Text { key: "other", label: msg!("Other:") },
                    Check { key: "treatConsecutive", label: msg!("Treat consecutive delimiters as one") },
                    Text { key: "destination", label: msg!("Destination:") },
                ],
                json!({"tab": true, "comma": true, "destination": active}),
            ),
            "removeDuplicates" => Dialog::form(
                "removeDuplicates",
                msg!("Remove Duplicates"),
                "data.removeDuplicates",
                vec![
                    Check { key: "header", label: msg!("My data has headers") },
                    Text { key: "columnsText", label: msg!("Columns (blank = all, e.g. A,C):") },
                ],
                json!({"header": true}),
            ),
            "goalSeek" => Dialog::custom("goalSeek", msg!("Goal Seek"), json!({"set": active, "to": "0", "changing": ""})),
            "subtotal" => Dialog::form(
                "subtotal",
                msg!("Subtotal"),
                "data.subtotal",
                vec![
                    Text { key: "groupBy", label: msg!("At each change in (column letter):") },
                    Choice {
                        key: "function",
                        label: msg!("Use function"),
                        options: vec![
                            ("sum", msg!("Sum")),
                            ("count", msg!("Count")),
                            ("average", msg!("Average")),
                            ("max", msg!("Max")),
                            ("min", msg!("Min")),
                            ("product", msg!("Product")),
                        ],
                    },
                    Text { key: "columnsText", label: msg!("Add subtotal to (columns, e.g. C,D):") },
                ],
                json!({"groupBy": "A", "function": "sum"}),
            ),
            "zoom" => Dialog::form(
                "zoom",
                msg!("Zoom"),
                "view.zoom",
                vec![
                    Choice {
                        key: "preset",
                        label: msg!("Magnification"),
                        options: vec![("200", "200%"), ("100", "100%"), ("75", "75%"), ("50", "50%"), ("25", "25%")],
                    },
                    Number { key: "percent", label: "ui-dialogs-custom-percent" },
                ],
                json!({"percent": app.session.active().and_then(|d| d.wb.active().map(|s| s.zoom)).unwrap_or(100)}),
            ),
            "defineName" => Dialog::form(
                "defineName",
                msg!("New Name"),
                "formulas.defineName",
                vec![
                    Text { key: "name", label: "ui-dialogs-name-label" },
                    Combo { key: "scope", label: msg!("Scope (Workbook or sheet name):"), options: scope_options(app) },
                    Text { key: "comment", label: msg!("Comment:") },
                    Text { key: "refersTo", label: msg!("Refers to:") },
                ],
                json!({"scope": "Workbook", "refersTo": app.session.active().and_then(|d| d.wb.active().map(|s| app.local_formula(&format!("={}!{}", gridcraft_engine::formula::quote_sheet(&s.name), abs(&d.selection.current())))))}),
            ),
            "dataValidation" => Dialog::form(
                "dataValidation",
                msg!("Data Validation"),
                "data.validation",
                vec![
                    Choice {
                        key: "type",
                        label: msg!("Allow"),
                        options: vec![
                            ("any", msg!("Any value")),
                            ("whole", msg!("Whole number")),
                            ("decimal", msg!("Decimal")),
                            ("list", msg!("List")),
                            ("date", msg!("Date")),
                            ("time", msg!("Time")),
                            ("textLength", msg!("Text length")),
                            ("custom", msg!("Custom")),
                        ],
                    },
                    Choice {
                        key: "operator",
                        label: msg!("Data"),
                        options: vec![
                            ("between", msg!("between")),
                            ("notBetween", msg!("not between")),
                            ("equal", msg!("equal to")),
                            ("notEqual", msg!("not equal to")),
                            ("greater", msg!("greater than")),
                            ("less", msg!("less than")),
                            ("greaterOrEqual", msg!("greater than or equal to")),
                            ("lessOrEqual", msg!("less than or equal to")),
                        ],
                    },
                    Text { key: "formula1", label: msg!("Minimum / Source / Formula:") },
                    Text { key: "formula2", label: msg!("Maximum:") },
                    Check { key: "ignoreBlank", label: msg!("Ignore blank") },
                    Check { key: "dropdown", label: msg!("In-cell dropdown") },
                    Text { key: "inputMessage", label: msg!("Input message:") },
                    Choice {
                        key: "errorStyle",
                        label: msg!("Error style"),
                        options: vec![("stop", msg!("Stop")), ("warning", msg!("Warning")), ("information", msg!("Information"))],
                    },
                    Text { key: "errorMessage", label: msg!("Error message:") },
                ],
                json!({"type": "list", "operator": "between", "ignoreBlank": true, "dropdown": true, "errorStyle": "stop"}),
            ),
            "cfQuick" => {
                let ty = p("type").as_str().unwrap_or("cellIs").to_string();
                // The ribbon passes its English menu text as the title; it is localized when shown.
                let title = p("title").as_str().unwrap_or(msg!("Conditional Formatting")).to_string();
                let mut fields = vec![];
                match ty.as_str() {
                    "cellIs" => {
                        fields.push(Text { key: "value", label: msg!("Format cells that are (value):") });
                        if p("operator").as_str() == Some("between") {
                            fields.push(Text { key: "value2", label: msg!("and:") });
                        }
                    }
                    "containsText" => fields.push(Text { key: "text", label: msg!("Format cells that contain the text:") }),
                    "timePeriod" => fields.push(Choice {
                        key: "period",
                        label: msg!("Format cells that contain a date occurring:"),
                        options: vec![
                            ("yesterday", msg!("Yesterday")),
                            ("today", msg!("Today")),
                            ("tomorrow", msg!("Tomorrow")),
                            ("last7Days", msg!("In the last 7 days")),
                            ("lastWeek", msg!("Last week")),
                            ("thisWeek", msg!("This week")),
                            ("nextWeek", msg!("Next week")),
                            ("lastMonth", msg!("Last month")),
                            ("thisMonth", msg!("This month")),
                            ("nextMonth", msg!("Next month")),
                        ],
                    }),
                    _ => fields.push(Text { key: "formula", label: msg!("Format values where this formula is true:") }),
                }
                fields.push(Choice {
                    key: "preset",
                    label: msg!("with"),
                    options: vec![
                        ("lightRedFillDarkRedText", msg!("Light Red Fill with Dark Red Text")),
                        ("yellowFillDarkYellowText", msg!("Yellow Fill with Dark Yellow Text")),
                        ("greenFillDarkGreenText", msg!("Green Fill with Dark Green Text")),
                        ("lightRedFill", msg!("Light Red Fill")),
                        ("redText", msg!("Red Text")),
                        ("redBorder", msg!("Red Border")),
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
                let mut d = Dialog::custom("formatCells", msg!("Format Cells"), json!({}));
                d.tab = p("tab").as_str().unwrap_or("Number").to_string();
                if let Ok(st) = app.session.run("cell.get", json!({})) {
                    d.values.insert("style".into(), st["style"].clone());
                }
                d
            }
            "insertFunction" => Dialog::custom("insertFunction", msg!("Formula Builder"), json!({"category": "All"})),
            "find" => {
                let mut d =
                    Dialog::custom("find", msg!("Find and Replace"), json!({"what": "", "with": "", "within": "sheet", "lookIn": "formulas"}));
                d.tab = if p("replace").as_bool() == Some(true) { "Replace".into() } else { "Find".into() };
                d
            }
            "goTo" => Dialog::custom("goTo", msg!("Go To"), json!({"reference": ""})),
            "goToSpecial" => Dialog::form(
                "goToSpecial",
                msg!("Go To Special"),
                "edit.goToSpecial",
                vec![Choice {
                    key: "kind",
                    label: msg!("Select"),
                    options: vec![
                        ("blanks", msg!("Blanks")),
                        ("constants", msg!("Constants")),
                        ("formulas", msg!("Formulas")),
                        ("notes", msg!("Notes")),
                        ("errors", msg!("Errors")),
                        ("numbers", msg!("Numbers")),
                        ("text", msg!("Text")),
                        ("currentRegion", msg!("Current region")),
                        ("lastCell", msg!("Last cell")),
                        ("visible", msg!("Visible cells only")),
                        ("conditionalFormats", msg!("Conditional formats")),
                        ("dataValidation", "ui-dialogs-goto-data-validation"),
                    ],
                }],
                json!({"kind": "blanks"}),
            ),
            "sort" => Dialog::custom("sort", msg!("Sort"), json!({"header": true, "levels": [{"column": "A", "order": "asc"}]})),
            "nameManager" => Dialog::custom("nameManager", msg!("Name Manager"), json!({})),
            "manageRules" => Dialog::custom("manageRules", msg!("Conditional Formatting Rules Manager"), json!({})),
            "commandSearch" => Dialog::custom("commandSearch", msg!("Search Commands"), json!({})),
            "agents" => Dialog::custom("agents", msg!("Agent Control"), json!({})),
            "options" => Dialog::custom("options", msg!("Options"), crate::options::defaults(app)),
            "about" => {
                let mut d = Dialog::custom("about", msg!("About GridCraft"), json!({}));
                d.tab = match p("tab").as_str() {
                    Some(t @ ("Contributors" | "Models")) => t.to_string(),
                    _ => "About".into(),
                };
                d
            }
            "journal" => Dialog::custom("journal", msg!("Action Journal"), json!({})),
            "statistics" => {
                let mut d = Dialog::custom("statistics", msg!("Workbook Statistics"), json!({}));
                d.result = app.session.run("review.workbookStatistics", json!({})).ok();
                d
            }
            "accessibility" => {
                let mut d = Dialog::custom("accessibility", msg!("Accessibility"), json!({}));
                d.result = app.session.run("review.checkAccessibility", json!({})).ok();
                d
            }
            "errorChecking" => {
                let mut d = Dialog::custom("errorChecking", msg!("Error Checking"), json!({}));
                d.result = app.session.run("formulas.errorChecking", json!({})).ok();
                d
            }
            "evaluateFormula" => {
                let mut d = Dialog::custom("evaluateFormula", msg!("Evaluate Formula"), json!({}));
                d.result = app.session.run("formulas.evaluateFormula", json!({})).ok();
                d
            }
            "spelling" => {
                let mut d = Dialog::custom("spelling", msg!("Spelling"), json!({}));
                d.result = Some(app.session.execute("review.spelling", json!({})).unwrap_or_else(|e| json!({"error": app.error_text(&e)})));
                d
            }
            "chartTitle" => Dialog::form(
                "chartTitle",
                msg!("Chart Title"),
                "chart.set",
                vec![
                    Text { key: "title", label: msg!("Title:") },
                    Text { key: "xTitle", label: msg!("Horizontal axis title:") },
                    Text { key: "yTitle", label: msg!("Vertical axis title:") },
                ],
                json!({"chart": p("chart")}),
            ),
            "insertPicture" => {
                let placement = params.get("placement").and_then(Json::as_str).unwrap_or("overCells");
                let mut dialog = Dialog::form(
                    "insertPicture",
                    if placement == "cell" { msg!("Place Picture in Cell") } else { msg!("Place Picture over Cells") },
                    "insert.picture",
                    vec![],
                    json!({"path": "", "placement": placement, "at": active}),
                );
                dialog.picture_target = app.view_key();
                dialog
            }
            "pickList" => Dialog::custom("pickList", msg!("Pick From List"), json!({})),
            "saveCopy" => Dialog::custom("saveCopy", msg!("Save a Copy"), json!({"format": p("format")})),
            "saveChanges" => Dialog::custom("saveChanges", msg!("Save Changes?"), json!({"title": p("title")})),
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

/// The distinct entries of the contiguous cells above the active cell, spelled as they are typed
/// in the session's language and region (what `cell.set` reads as `inputLocal`).
pub fn pick_list_items(app: &SheetApp) -> Vec<String> {
    let Some(doc) = app.session.active() else { return vec![] };
    let Some(sh) = doc.wb.active() else { return vec![] };
    let a = doc.selection.active;
    let mut seen = std::collections::BTreeSet::new();
    let mut r = a.row;
    while r > 0 && !sh.value(gridcraft_engine::core::CellRef::new(r - 1, a.col)).is_empty() {
        r -= 1;
        seen.insert(gridcraft_engine::locale::value_local(&sh.value(gridcraft_engine::core::CellRef::new(r, a.col)), &doc.wb.locale));
    }
    seen.into_iter().collect()
}

fn abs(r: &gridcraft_engine::core::RangeRef) -> String {
    let a = |c: gridcraft_engine::core::CellRef| format!("${}${}", gridcraft_engine::core::col_to_letters(c.col), c.row + 1);
    if r.is_single() { a(r.start) } else { format!("{}:{}", a(r.start), a(r.end)) }
}

/// The scopes a new name can have: the whole workbook (value `Workbook`, the engine's spelling,
/// shown in the interface language) or one of the sheets.
fn scope_options(app: &SheetApp) -> Vec<(String, String)> {
    let mut options = vec![("Workbook".to_string(), app.l10n.tr("Workbook").into_owned())];
    if let Some(d) = app.session.active() {
        options.extend(d.wb.sheets.iter().map(|s| (s.name.clone(), s.name.clone())));
    }
    options
}

/// A report as the user reads it: wherever the engine gives a `<key>Local` twin next to `<key>`
/// (the text spelled in the formula language and region), the twin replaces `<key>`.
pub fn local_view(v: &Json) -> Json {
    match v {
        Json::Array(items) => Json::Array(items.iter().map(local_view).collect()),
        Json::Object(map) => Json::Object(
            map.iter()
                .filter(|(key, _)| !key.strip_suffix("Local").is_some_and(|base| map.contains_key(base)))
                .map(|(key, value)| (key.clone(), map.get(&format!("{key}Local")).map_or_else(|| local_view(value), local_view)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Moves the text fields `keys` to their `…Local` twins, which the engine reads in the formula
/// language and region the user typed them in. Empty values are dropped.
fn to_local_keys(v: &mut Json, keys: &[&str]) {
    let Some(m) = v.as_object_mut() else { return };
    for key in keys {
        if let Some(value) = m.remove(*key)
            && value.as_str().is_none_or(|s| !s.trim().is_empty())
        {
            m.insert(format!("{key}Local"), value);
        }
    }
}

/// Builds the command params from form values for dialogs that need translation. What the user
/// typed as formulas and numbers is sent as `…Local` text; a number field that is not a number
/// in the region's spelling is an error message.
pub fn build_params(d: &Dialog, app: &SheetApp) -> Result<Json, String> {
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
            let mut rule = Json::Object(d.values.clone());
            to_local_keys(&mut rule, &["value", "value2", "formula"]);
            v = json!({"rule": rule});
        }
        "dataValidation" => to_local_keys(&mut v, &["formula1", "formula2"]),
        "series" => {
            let stop = d.values.get("stop").and_then(Json::as_str).map(str::trim).filter(|s| !s.is_empty());
            match stop {
                Some(text) => {
                    let n =
                        app.parse_number(text).ok_or_else(|| app.l10n.text("ui-dialogs-not-a-number", &[("text", Arg::from(text))]).into_owned())?;
                    v["stop"] = json!(n);
                }
                None => {
                    if let Some(m) = v.as_object_mut() {
                        m.remove("stop");
                    }
                }
            }
        }
        "defineName" => {
            to_local_keys(&mut v, &["refersTo"]);
            if d.values.get("scope").and_then(Json::as_str) == Some("Workbook")
                && let Some(m) = v.as_object_mut()
            {
                m.remove("scope");
            }
        }
        _ => {}
    }
    Ok(v)
}

pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else { return };
    let l = app.l10n;
    if d.name == "pasteSpecial" {
        paste_special_shortcuts(&mut d, ctx);
    }
    let mut open = true;
    let mut win_open = true;
    let mut confirm = false;
    let width = match d.name.as_str() {
        "formatCells" => 560.0,
        "about" => 660.0,
        "options" => 460.0,
        "insertFunction" | "commandSearch" | "nameManager" | "manageRules" | "journal" | "agents" => 520.0,
        _ => 380.0,
    };
    let window = egui::Window::new(l.tr(&d.title).into_owned())
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
                    ui.label(&*l.tr("GridCraft is fully drivable by agents. Every menu item, button and gesture is a command:"));
                    ui.add_space(4.0);
                    ui.monospace("gridcraft --control 7979        # JSON-lines control channel\ngridcraft-cli mcp --connect 7979   # MCP bridged to this window\ngridcraft-cli mcp                   # headless MCP server");
                    ui.add_space(4.0);
                    let count = gridcraft_engine::command_specs().len();
                    ui.label(&*l.text("ui-dialogs-agents-commands", &[("count", Arg::from(count.to_string().as_str()))]));
                }
                "about" => about(ui, l, &mut d),
                "options" => {
                    crate::options::show(l, ui, &mut d);
                    let mut ok = false;
                    let mut stay_open = true;
                    ok_cancel(ui, l, &mut ok, &mut stay_open);
                    if ok {
                        match crate::options::apply(app, &d) {
                            Ok(()) => confirm = true,
                            Err(text) => app.message = Some((l.tr("Options").into_owned(), text)),
                        }
                    }
                    // Cancel closes the dialog without applying anything.
                    if !stay_open {
                        confirm = true;
                    }
                }
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
                        d.result.as_ref().map(local_view)
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
                            app.message = Some((l.tr(&d.title).into_owned(), l.tr("Choose a PNG or JPEG no larger than 16 MB.").into_owned()));
                        } else {
                            d.values.remove("path");
                            d.values.insert("alt".into(), json!(name));
                            d.values.insert("base64".into(), json!(gridcraft_engine::io::base64_encode(&bytes)));
                        }
                    }
                    if app.services.pick_picture_async.is_some() {
                        match d.values.get("alt").and_then(Json::as_str) {
                            Some(name) => ui.label(name),
                            None => ui.label(&*l.tr("No picture selected")),
                        };
                    } else {
                        ui.label(&*l.tr("Picture file path (PNG or JPEG):"));
                        let mut path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
                        if ui.text_edit_singleline(&mut path).changed() {
                            d.values.remove("base64");
                            d.values.insert("path".into(), json!(path));
                        }
                    }
                    if (app.services.pick_picture.is_some() || app.services.pick_picture_async.is_some()) && ui.button(&*l.tr("Browse…")).clicked() {
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
                    ui.label(&*l.tr("Description (optional):"));
                    let mut alt = d.values.get("alt").and_then(Json::as_str).unwrap_or("").to_string();
                    if ui.text_edit_singleline(&mut alt).changed() {
                        d.values.insert("alt".into(), json!(alt));
                    }
                    if d.values.get("placement").and_then(Json::as_str) == Some("cell") {
                        ui.label(egui::RichText::new(l.tr("Fits inside the selected cell and moves with its content.")).small());
                    }
                    ok_cancel(ui, l, &mut confirm, &mut open);
                }
                "pickList" => {
                    for it in pick_list_items(app) {
                        if ui.button(&it).clicked() {
                            app.run_or_alert("cell.set", json!({"inputLocal": it}));
                            open = false;
                        }
                    }
                }
                "saveCopy" => {
                    let fmt = d.values.get("format").and_then(Json::as_str).unwrap_or("xlsx").to_string();
                    ui.label(&*l.text("ui-dialogs-save-copy-as", &[("fmt", Arg::from(fmt.as_str()))]));
                    let mut path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
                    ui.text_edit_singleline(&mut path);
                    d.values.insert("path".into(), json!(path.clone()));
                    ok_cancel(ui, l, &mut confirm, &mut open);
                    if confirm {
                        let target = if path.is_empty() {
                            app.services.pick_save.as_ref().and_then(|f| f(&l.text("ui-dialogs-copy-file-name", &[("fmt", Arg::from(fmt.as_str()))])))
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
                    let question = match d.values.get("title").and_then(Json::as_str) {
                        Some(t) => l.text("ui-dialogs-save-changes-question", &[("title", Arg::from(t))]),
                        None => l.text("ui-dialogs-save-changes-question-untitled", &[]),
                    };
                    ui.label(&*question);
                    ui.horizontal(|ui| {
                        if ui.button(&*l.tr("Don't Save")).clicked() {
                            app.run_or_alert("file.close", json!({"force": true}));
                            open = false;
                        }
                        if ui.button(&*l.tr("Cancel")).clicked() {
                            open = false;
                        }
                        if ui.button(&*l.tr("Save")).clicked() {
                            app.run_or_alert("file.save", json!({}));
                            if app.session.active().is_some_and(|x| !x.is_dirty()) {
                                app.run_or_alert("file.close", json!({"force": true}));
                            }
                            open = false;
                        }
                    });
                }
                "start" => {
                    ui.heading(&*l.tr("Start a new workbook"));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(&*l.tr("Blank workbook")).clicked() {
                            app.run_or_alert("file.new", json!({}));
                            open = false;
                        }
                        for (n, english) in gridcraft_engine::sample::SAMPLES {
                            let name = l.get(&format!("ui-sample-{n}"), &[]).map_or_else(|| (*english).to_string(), |t| t.into_owned());
                            if ui.button(name).clicked() {
                                app.run_or_alert("file.new", json!({"sample": n}));
                                open = false;
                            }
                        }
                    });
                    ui.separator();
                    ui.label(egui::RichText::new(l.tr("Recent")).strong());
                    for path in app.ui.recent.clone() {
                        if ui.link(&path).clicked() {
                            app.open_path(&path);
                            open = false;
                        }
                    }
                    if ui.button(&*l.tr("Open…")).clicked() {
                        app.open_dialog("open", json!({}));
                        open = false;
                    }
                }
                _ => form(ui, l, app.number_style(), &mut d, &mut confirm, &mut open),
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
        let text = l.tr("Return to the original worksheet to insert this picture, or cancel and choose another cell.").into_owned();
        app.message = Some((l.tr(&d.title).into_owned(), text));
        confirm = false;
    }
    if confirm && let Some(cmd) = d.command {
        let result = build_params(&d, app).and_then(|p| app.run_typed(cmd, p).map_err(|e| app.error_text(&e)));
        match result {
            Ok(_) => open = false,
            Err(text) => app.message = Some((l.tr(&d.title).into_owned(), text)),
        }
    } else if confirm {
        open = false;
    }
    if open {
        app.dialog = Some(d);
    }
}

/// Paste Special's legacy mnemonics select options; Enter still performs the paste.
/// Keep the English keytip letters in every language, matching the ribbon access sequences.
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

fn ok_cancel(ui: &mut egui::Ui, l: Localizer, confirm: &mut bool, open: &mut bool) {
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let ok_text = format!("   {}   ", l.tr("OK"));
        let ok = ui.add(egui::Button::new(egui::RichText::new(ok_text).color(Color32::WHITE)).fill(crate::theme::Tokens::get(ui.ctx()).accent));
        if ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            *confirm = true;
        }
        if ui.button(format!(" {} ", l.tr("Cancel"))).clicked() {
            *open = false;
        }
    });
}

/// Text of a form field: a literal `ui-dialogs-…` is an explicit message key (used where two English
/// texts would share the key derived from their wording), anything else is an English literal.
fn shown(l: Localizer, text: &str) -> std::borrow::Cow<'static, str> {
    if text.starts_with("ui-dialogs-") { l.text(text, &[]) } else { l.tr(text) }
}

fn form(ui: &mut egui::Ui, l: Localizer, numbers: crate::widgets::NumberStyle, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    egui::Grid::new("form").num_columns(1).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        for f in d.fields.clone() {
            match f {
                Field::Text { key, label } => {
                    ui.vertical(|ui| {
                        ui.label(&*shown(l, label));
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
                        ui.label(&*shown(l, label));
                        let mut n = d.values.get(key).and_then(Json::as_f64).unwrap_or(0.0);
                        if ui.add(numbers.drag(&mut n).speed(0.5)).changed() {
                            d.values.insert(key.into(), json!(n));
                        }
                    });
                }
                Field::Combo { key, label, options } => {
                    ui.vertical(|ui| {
                        ui.label(&*shown(l, label));
                        let cur = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
                        let selected = options.iter().find(|(v, _)| *v == cur).map_or(cur.as_str(), |(_, text)| text.as_str()).to_string();
                        egui::ComboBox::from_id_salt(("form_combo", key)).selected_text(selected).width(320.0).show_ui(ui, |ui| {
                            for (val, text) in &options {
                                if ui.selectable_label(cur == *val, text).clicked() {
                                    d.values.insert(key.into(), json!(val));
                                }
                            }
                        });
                    });
                }
                Field::Choice { key, label, options } => {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(shown(l, label)).strong());
                        let cur = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
                        for (val, text) in options {
                            let text = shown(l, text);
                            let text = if d.name == "pasteSpecial" && key == "what" {
                                match val {
                                    "formulas" => std::borrow::Cow::Owned(format!("{text} (F)")),
                                    "values" => std::borrow::Cow::Owned(format!("{text} (V)")),
                                    "formats" => std::borrow::Cow::Owned(format!("{text} (T)")),
                                    "comments" => std::borrow::Cow::Owned(format!("{text} (C)")),
                                    _ => text,
                                }
                            } else {
                                text
                            };
                            if ui.radio(cur == val, text).clicked() {
                                d.values.insert(key.into(), json!(val));
                            }
                        }
                    });
                }
                Field::Check { key, label } => {
                    let mut b = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
                    let label = shown(l, label);
                    let label = if d.name == "pasteSpecial" && key == "transpose" { std::borrow::Cow::Owned(format!("{label} (E)")) } else { label };
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
    ok_cancel(ui, l, confirm, open);
}

/// Help ▸ About GridCraft: the app, its contributors and the AI models that helped. The credits
/// are compiled in (`crate::credits`, docs/contributors.md).
fn about(ui: &mut egui::Ui, l: Localizer, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for tab in [msg!("About"), msg!("Contributors"), msg!("Models")] {
            if ui.selectable_label(d.tab == tab, &*l.tr(tab)).clicked() {
                d.tab = tab.to_string();
            }
        }
    });
    ui.separator();
    match d.tab.as_str() {
        "Contributors" => crate::credits::contributors_ui(ui, l),
        "Models" => crate::credits::models_ui(ui, l),
        _ => {
            ui.heading("GridCraft");
            ui.label(&*l.text("ui-dialogs-version", &[("version", Arg::from(env!("CARGO_PKG_VERSION")))]));
            ui.add_space(4.0);
            ui.label(&*l.tr("A clean-room, open-source, Rust-native spreadsheet. Part of ArtCraft."));
            ui.hyperlink_to("getartcraft.com/apps/gridcraft", "https://getartcraft.com/apps/gridcraft");
            ui.hyperlink_to(&*l.tr("Join the ArtCraft Discord"), "https://discord.gg/artcraft");
        }
    }
}

fn format_cells(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let l = app.l10n;
    ui.horizontal(|ui| {
        for tab in [msg!("Number"), msg!("Alignment"), msg!("Font"), msg!("Border"), msg!("Fill"), msg!("Protection")] {
            if ui.selectable_label(d.tab == tab, &*l.tr(tab)).clicked() {
                d.tab = tab.to_string();
            }
        }
    });
    ui.separator();
    let mut st: gridcraft_engine::model::Style = d.values.get("style").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let sample = app.session.active().and_then(|doc| doc.wb.active().map(|s| s.value(doc.selection.active))).unwrap_or_default();
    match d.tab.as_str() {
        "Number" => {
            let loc = app.session.locale();
            let dialect = loc.dialect();
            let mut code = st.num_fmt.as_str().to_string();
            // The code is edited in the formula language (`#.##0,00`, `aaaa`); the buffer is kept
            // while it still means the same canonical code, so half-typed text is not rewritten.
            let mut local = d
                .values
                .get("codeLocal")
                .and_then(Json::as_str)
                .filter(|s| gridcraft_engine::locale::from_local_format(s, &dialect) == code)
                .map_or_else(|| gridcraft_engine::locale::to_local_format(&code, &dialect), str::to_string);
            ui.columns(2, |cols| {
                cols[0].label(egui::RichText::new(l.tr("Category:")).strong());
                for name in [
                    msg!("General"),
                    msg!("Number"),
                    msg!("Currency"),
                    msg!("Accounting"),
                    msg!("Short Date"),
                    msg!("Long Date"),
                    msg!("Time"),
                    msg!("Percentage"),
                    msg!("Fraction"),
                    msg!("Scientific"),
                    msg!("Text"),
                ] {
                    let c = gridcraft_engine::cmd::format::format_code_for(name).to_string();
                    if cols[0].selectable_label(code == c, &*l.tr(name)).clicked() {
                        local = gridcraft_engine::locale::to_local_format(&c, &dialect);
                        code = c;
                    }
                }
                cols[1].label(egui::RichText::new(l.tr("Sample")).strong());
                let preview = app.session.active().map(|doc| gridcraft_engine::display::format(&sample, &code, &doc.wb).text).unwrap_or_default();
                cols[1].label(egui::RichText::new(preview).font(theme::ui_font(15.0)));
                cols[1].add_space(8.0);
                cols[1].label(&*l.tr("Type (custom format code):"));
                if cols[1].text_edit_singleline(&mut local).changed() {
                    code = gridcraft_engine::locale::from_local_format(&local, &dialect);
                }
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
                    if cols[1].small_button(gridcraft_engine::locale::to_local_format(c, &dialect)).clicked() {
                        code = c.to_string();
                        local = gridcraft_engine::locale::to_local_format(c, &dialect);
                    }
                }
            });
            d.values.insert("codeLocal".into(), json!(local));
            st.num_fmt = gridcraft_engine::model::NumFmt::new(&code);
        }
        "Alignment" => {
            use gridcraft_engine::model::{HAlign, VAlign};
            ui.label(egui::RichText::new(l.tr("Horizontal")).strong());
            ui.horizontal_wrapped(|ui| {
                for (h, n) in [
                    (HAlign::General, msg!("General")),
                    (HAlign::Left, msg!("Left")),
                    (HAlign::Center, msg!("Center")),
                    (HAlign::Right, msg!("Right")),
                    (HAlign::Fill, msg!("Fill")),
                    (HAlign::Justify, msg!("Justify")),
                    (HAlign::CenterAcross, msg!("Center Across Selection")),
                    (HAlign::Distributed, msg!("Distributed")),
                ] {
                    ui.radio_value(&mut st.align.h, h, &*l.tr(n));
                }
            });
            ui.label(egui::RichText::new(l.tr("Vertical")).strong());
            ui.horizontal_wrapped(|ui| {
                for (v, n) in [
                    (VAlign::Top, msg!("Top")),
                    (VAlign::Center, msg!("Center")),
                    (VAlign::Bottom, msg!("Bottom")),
                    (VAlign::Justify, msg!("Justify")),
                    (VAlign::Distributed, msg!("Distributed")),
                ] {
                    ui.radio_value(&mut st.align.v, v, &*l.tr(n));
                }
            });
            let mut indent = st.align.indent as f32;
            ui.add(egui::Slider::new(&mut indent, 0.0..=15.0).text(&*l.tr("Indent")));
            st.align.indent = indent as u8;
            let mut rot = if st.align.rotation == 255 { 0.0 } else { st.align.rotation as f32 };
            ui.add(egui::Slider::new(&mut rot, -90.0..=90.0).text(&*l.tr("Orientation (degrees)")));
            st.align.rotation = rot as i16;
            ui.checkbox(&mut st.align.wrap, &*l.tr("Wrap text"));
            ui.checkbox(&mut st.align.shrink, &*l.tr("Shrink to fit"));
        }
        "Font" => {
            egui::ComboBox::from_label(&*l.tr("Font")).selected_text(st.font.name.clone()).show_ui(ui, |ui| {
                for f in crate::ribbon::FONTS {
                    ui.selectable_value(&mut st.font.name, f.to_string(), *f);
                }
            });
            ui.add(app.number_style().slider(&mut st.font.size, 6.0..=72.0).text(&*l.tr("Size")));
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.font.bold, &*l.tr("Bold"));
                ui.checkbox(&mut st.font.italic, &*l.tr("Italic"));
                ui.checkbox(&mut st.font.strike, &*l.tr("Strikethrough"));
            });
            use gridcraft_engine::model::{Underline, VertAlign};
            ui.horizontal(|ui| {
                for (u, n) in [
                    (Underline::None, msg!("None")),
                    (Underline::Single, msg!("Single")),
                    (Underline::Double, msg!("Double")),
                    (Underline::SingleAccounting, msg!("Single Accounting")),
                    (Underline::DoubleAccounting, msg!("Double Accounting")),
                ] {
                    ui.radio_value(&mut st.font.underline, u, &*l.tr(n));
                }
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.font.vert, VertAlign::Baseline, &*l.tr("Normal"));
                ui.radio_value(&mut st.font.vert, VertAlign::Superscript, &*l.tr("Superscript"));
                ui.radio_value(&mut st.font.vert, VertAlign::Subscript, &*l.tr("Subscript"));
            });
            ui.label(&*l.tr("Color:"));
            let colors = app.session.active().map(|doc| doc.wb.theme.colors).unwrap_or_default();
            if let Some(c) = crate::widgets::color_palette(ui, l, &colors, &l.tr("Automatic")) {
                st.font.color =
                    if c == "none" { gridcraft_engine::model::Color::Auto } else { gridcraft_engine::model::Color::from_hex(&c).unwrap_or_default() };
            }
        }
        "Border" => {
            use gridcraft_engine::model::{BorderLine, BorderStyle};
            let line = BorderLine { style: BorderStyle::Thin, color: gridcraft_engine::model::Color::Auto };
            ui.horizontal(|ui| {
                if ui.button(&*l.tr("None")).clicked() {
                    st.borders = Default::default();
                }
                if ui.button(&*l.tr("Outline")).clicked() {
                    st.borders.top = line;
                    st.borders.bottom = line;
                    st.borders.left = line;
                    st.borders.right = line;
                }
            });
            for (n, slot) in [
                (l.tr("Top"), &mut st.borders.top),
                (l.tr("Bottom"), &mut st.borders.bottom),
                (l.tr("Left"), &mut st.borders.left),
                (l.tr("Right"), &mut st.borders.right),
                (l.text("ui-dialogs-diagonal-down", &[]), &mut st.borders.diag_down),
                (l.text("ui-dialogs-diagonal-up", &[]), &mut st.borders.diag_up),
            ] {
                ui.horizontal(|ui| {
                    ui.label(format!("{n}:"));
                    for (s, label) in [
                        (BorderStyle::None, l.text("ui-dialogs-border-style-none", &[])),
                        (BorderStyle::Thin, l.text("ui-dialogs-border-style-thin", &[])),
                        (BorderStyle::Medium, l.text("ui-dialogs-border-style-medium", &[])),
                        (BorderStyle::Thick, l.text("ui-dialogs-border-style-thick", &[])),
                        (BorderStyle::Dashed, l.text("ui-dialogs-border-style-dashed", &[])),
                        (BorderStyle::Dotted, l.text("ui-dialogs-border-style-dotted", &[])),
                        (BorderStyle::Double, l.text("ui-dialogs-border-style-double", &[])),
                    ] {
                        let response = ui.selectable_label(slot.style == s, &*label);
                        response
                            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::SelectableLabel, ui.is_enabled(), format!("{n} {label}")));
                        if response.clicked() {
                            slot.style = s;
                        }
                    }
                });
            }
            let theme = app.session.active().map(|doc| &doc.wb.theme).cloned().unwrap_or_default();
            crate::border_preview::sample(ui, l, &st.borders, &theme);
        }
        "Fill" => {
            let colors = app.session.active().map(|doc| doc.wb.theme.colors).unwrap_or_default();
            ui.label(&*l.tr("Background Color:"));
            if let Some(c) = crate::widgets::color_palette(ui, l, &colors, &l.tr("No Color")) {
                st.fill = if c == "none" {
                    Default::default()
                } else {
                    gridcraft_engine::model::Fill::solid(gridcraft_engine::model::Color::from_hex(&c).unwrap_or_default())
                };
            }
        }
        "Protection" => {
            ui.checkbox(&mut st.protection.locked, &*l.tr("Locked"));
            ui.checkbox(&mut st.protection.hidden, &*l.tr("Hidden"));
            ui.label(
                egui::RichText::new(l.tr("Locking cells or hiding formulas has no effect until you protect the worksheet (Review › Protect Sheet)."))
                    .small(),
            );
        }
        _ => {}
    }
    if let Ok(v) = serde_json::to_value(&st) {
        d.values.insert("style".into(), v);
    }
    let mut open = true;
    ok_cancel(ui, l, confirm, &mut open);
    if *confirm && let Some(style) = d.values.get("style").cloned() {
        app.run_or_alert("home.formatCells", json!({"style": style}));
    }
    if !open {
        app.dialog = None;
        *confirm = true; // closes
    }
}

fn insert_function(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let l = app.l10n;
    let loc = app.session.locale();
    let mut category = d.values.get("category").and_then(Json::as_str).unwrap_or("All").to_string();
    let before = (d.search.clone(), category.clone());
    ui.horizontal(|ui| {
        ui.label(&*l.tr("Search:"));
        let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(220.0).hint_text(&*l.tr("e.g. lookup, average, date")));
        r.request_focus();
        let shown = if category == "All" { fnlist::all_categories(&l) } else { fnlist::category_label(&l, &category) };
        egui::ComboBox::from_id_salt("fn_category").selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut category, "All".to_string(), fnlist::all_categories(&l));
            for c in fnlist::CATEGORIES {
                ui.selectable_value(&mut category, (*c).to_string(), fnlist::category_label(&l, c));
            }
        });
    });
    if before != (d.search.clone(), category.clone()) {
        d.list_index = 0;
    }
    d.values.insert("category".into(), json!(category));
    let q = d.search.to_lowercase();
    let hits: Vec<(&fnlist::FunctionInfo, String)> = fnlist::all()
        .iter()
        .filter(|f| category == "All" || f.category == category)
        .map(|f| (f, fnlist::local_name(&loc, f)))
        .filter(|(f, name)| q.is_empty() || name.to_lowercase().contains(&q) || fnlist::description(&l, f).to_lowercase().contains(&q))
        .take(300)
        .collect();
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (i, (f, name)) in hits.iter().enumerate() {
            let r = ui.add(
                egui::Button::selectable(i == d.list_index, format!("{name:<18} {}", fnlist::category_label(&l, &f.category)))
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
    if let Some((f, _)) = hits.get(d.list_index) {
        ui.separator();
        ui.label(egui::RichText::new(fnlist::signature(&l, &loc, f)).strong());
        ui.label(fnlist::description(&l, f));
    }
    let mut open = true;
    ok_cancel(ui, l, confirm, &mut open);
    if *confirm && let Some((_, n)) = hits.get(d.list_index) {
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
    let l = app.l10n;
    ui.horizontal(|ui| {
        for tab in [msg!("Find"), msg!("Replace")] {
            if ui.selectable_label(d.tab == tab, &*l.tr(tab)).clicked() {
                d.tab = tab.into();
            }
        }
    });
    let mut what = d.values.get("what").and_then(Json::as_str).unwrap_or("").to_string();
    let mut with = d.values.get("with").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label(&*l.tr("Find what:"));
        ui.add(egui::TextEdit::singleline(&mut what).desired_width(260.0));
    });
    if d.tab == "Replace" {
        ui.horizontal(|ui| {
            ui.label(&*l.tr("Replace with:"));
            ui.add(egui::TextEdit::singleline(&mut with).desired_width(244.0));
        });
    }
    let mut case = d.values.get("matchCase").and_then(Json::as_bool).unwrap_or(false);
    let mut whole = d.values.get("wholeCell").and_then(Json::as_bool).unwrap_or(false);
    let mut wb = d.values.get("within").and_then(Json::as_str) == Some("workbook");
    ui.horizontal(|ui| {
        ui.checkbox(&mut case, &*l.tr("Match case"));
        ui.checkbox(&mut whole, &*l.tr("Match entire cell contents"));
        ui.checkbox(&mut wb, &*l.tr("Within workbook"));
    });
    d.values.insert("what".into(), json!(what));
    d.values.insert("with".into(), json!(with));
    d.values.insert("matchCase".into(), json!(case));
    d.values.insert("wholeCell".into(), json!(whole));
    d.values.insert("within".into(), json!(if wb { "workbook" } else { "sheet" }));
    let base = json!({"what": what, "matchCase": case, "wholeCell": whole, "within": if wb { "workbook" } else { "sheet" }});
    ui.horizontal(|ui| {
        if d.tab == "Replace" {
            if ui.button(&*l.tr("Replace All")).clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(true);
                match app.run_typed("edit.replace", p) {
                    Ok(r) => {
                        let msg = l.text("ui-dialogs-replacements-done", &[("count", Arg::from(r["replaced"].to_string().as_str()))]);
                        app.toast = Some((msg.into_owned(), crate::now_ms()));
                    }
                    Err(e) => app.message = Some((l.tr("Find and Replace").into_owned(), app.error_text(&e))),
                }
            }
            if ui.button(&*l.tr("Replace")).clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(false);
                let _ = app.run("edit.replace", p);
                let _ = app.run("edit.find", base.clone());
            }
        }
        if ui.button(&*l.tr("Find All")).clicked() {
            let mut p = base.clone();
            p["all"] = json!(true);
            d.result = app.run("edit.find", p).ok();
        }
        if ui.button(&*l.tr("Find Next")).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Err(e) = app.run_typed("edit.find", base.clone()) {
                app.message = Some((l.tr("Find and Replace").into_owned(), app.error_text(&e)));
            }
            app.grid.ensure_visible = true;
        }
    });
    if let Some(r) = &d.result
        && let Some(list) = r["results"].as_array()
    {
        ui.separator();
        ui.label(&*l.text("ui-dialogs-cells-found", &[("count", Arg::from(list.len().to_string().as_str()))]));
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
    let l = app.l10n;
    let names: Vec<String> = app.session.active().map(|doc| doc.wb.names.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
    for n in names {
        if ui.selectable_label(false, &n).clicked() {
            d.values.insert("reference".into(), json!(n));
        }
    }
    let mut r = d.values.get("reference").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label(&*l.tr("Reference:"));
        ui.text_edit_singleline(&mut r).request_focus();
    });
    d.values.insert("reference".into(), json!(r.clone()));
    if ui.button(&*l.tr("Special…")).clicked() {
        app.open_dialog("goToSpecial", json!({}));
        return;
    }
    let mut open = true;
    ok_cancel(ui, l, confirm, &mut open);
    if *confirm {
        app.run_or_alert("edit.goTo", json!({"reference": r}));
        app.grid.ensure_visible = true;
    }
    if !open {
        *confirm = true;
    }
}

fn sort(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let l = app.l10n;
    let mut header = d.values.get("header").and_then(Json::as_bool).unwrap_or(true);
    ui.checkbox(&mut header, &*l.tr("My list has headers"));
    d.values.insert("header".into(), json!(header));
    let mut levels: Vec<Json> = d.values.get("levels").and_then(Json::as_array).cloned().unwrap_or_default();
    let mut remove = None;
    for (i, lv) in levels.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(&*if i == 0 { l.tr("Sort by") } else { l.tr("Then by") });
            let mut col = lv["column"].as_str().unwrap_or("A").to_string();
            ui.add(egui::TextEdit::singleline(&mut col).desired_width(50.0));
            lv["column"] = json!(col.to_ascii_uppercase());
            let mut by = lv["by"].as_str().unwrap_or("values").to_string();
            egui::ComboBox::from_id_salt(("by", i))
                .selected_text(&*l.tr(match by.as_str() {
                    "cellColor" => msg!("Cell Color"),
                    "fontColor" => msg!("Font Color"),
                    _ => msg!("Cell Values"),
                }))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut by, "values".into(), &*l.tr("Cell Values"));
                    ui.selectable_value(&mut by, "cellColor".into(), &*l.tr("Cell Color"));
                    ui.selectable_value(&mut by, "fontColor".into(), &*l.tr("Font Color"));
                });
            lv["by"] = json!(by);
            let mut desc = lv["order"].as_str() == Some("desc");
            ui.radio_value(&mut desc, false, &*l.tr("A to Z"));
            ui.radio_value(&mut desc, true, &*l.tr("Z to A"));
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
    if ui.button(&*l.tr("+ Add Level")).clicked() {
        levels.push(json!({"column": "B", "order": "asc"}));
    }
    d.values.insert("levels".into(), Json::Array(levels.clone()));
    let mut open = true;
    ok_cancel(ui, l, confirm, &mut open);
    if *confirm {
        app.run_or_alert("data.sort", json!({"header": header, "keys": levels}));
    }
    if !open {
        *confirm = true;
    }
}

/// An engine value (`inspect::value_json`) spelled as the user types it: text without quotes,
/// numbers with the region's decimal separator, booleans and errors in the formula language,
/// arrays as `{1;2}` constants with the region's separators.
pub fn value_text(loc: &gridcraft_locale::Locale, v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::String(s) => s.clone(),
        Json::Bool(b) => (if *b { loc.formula.bool_true } else { loc.formula.bool_false }).to_string(),
        Json::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |f| gridcraft_engine::core::number_to_text_in(f, &loc.regional)),
        Json::Object(o) => o.get("error").and_then(Json::as_str).map_or_else(String::new, |e| loc.formula.local_error(e).to_string()),
        Json::Array(rows) => {
            let row = |r: &Json| match r {
                Json::Array(cells) => cells.iter().map(|c| value_text(loc, c)).collect::<Vec<_>>().join(&loc.regional.array_col.to_string()),
                other => value_text(loc, other),
            };
            format!("{{{}}}", rows.iter().map(row).collect::<Vec<_>>().join(&loc.regional.array_row.to_string()))
        }
    }
}

fn name_manager(app: &mut SheetApp, ui: &mut egui::Ui) {
    let l = app.l10n;
    let list = app.session.run("formulas.nameManager", json!({})).unwrap_or(Json::Null);
    egui::Grid::new("names").striped(true).num_columns(4).show(ui, |ui| {
        ui.strong(&*l.tr("Name"));
        ui.strong(&*l.tr("Value"));
        ui.strong(&*l.text("ui-dialogs-column-refers-to", &[]));
        ui.strong(&*l.tr("Scope"));
        ui.end_row();
        for n in list.as_array().cloned().unwrap_or_default() {
            ui.label(n["name"].as_str().unwrap_or(""));
            let value = n["valueLocal"].as_str().map_or_else(|| value_text(&app.session.locale(), &n["value"]), str::to_string);
            ui.label(value.chars().take(30).collect::<String>());
            ui.label(n["refersToLocal"].as_str().or_else(|| n["refersTo"].as_str()).unwrap_or(""));
            ui.horizontal(|ui| {
                let scope = n["scope"].as_str().unwrap_or("");
                ui.label(if scope == "Workbook" { l.tr("Workbook").into_owned() } else { scope.to_string() });
                if ui.small_button(&*l.tr("Delete")).clicked() {
                    app.run_or_alert("formulas.deleteName", json!({"name": n["name"], "scope": n["scope"]}));
                }
            });
            ui.end_row();
        }
    });
    if ui.button(&*l.tr("New…")).clicked() {
        app.open_dialog("defineName", json!({}));
    }
}

fn manage_rules(app: &mut SheetApp, ui: &mut egui::Ui) {
    let l = app.l10n;
    let list = app.session.run("home.manageRules", json!({})).unwrap_or(Json::Null);
    let rules = list["rules"].as_array().cloned().unwrap_or_default();
    if rules.is_empty() {
        ui.label(&*l.tr("There are no conditional formatting rules on this sheet."));
    }
    for (i, r) in rules.iter().enumerate() {
        ui.horizontal(|ui| {
            let kind = r["rule"].as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_default();
            let ranges = r["ranges"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default();
            ui.label(&*l.text(
                "ui-dialogs-rule-applies-to",
                &[("n", Arg::from((i + 1).to_string().as_str())), ("kind", Arg::from(kind.as_str())), ("ranges", Arg::from(ranges.as_str()))],
            ));
            if ui.small_button("▲").clicked() {
                app.run_or_alert("home.manageRules", json!({"moveUp": i}));
            }
            if ui.small_button("▼").clicked() {
                app.run_or_alert("home.manageRules", json!({"moveDown": i}));
            }
            if ui.small_button(&*l.tr("Delete")).clicked() {
                app.run_or_alert("home.manageRules", json!({"delete": i}));
            }
        });
    }
}

/// Ribbon path of a command (`Home › Cells`) in the active language; names without a translation stay English.
fn menu_path(l: Localizer, menu: &[&str]) -> String {
    let parts: Vec<String> = menu
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let local = if i == 0 { l.ribbon_tab(seg) } else { l.ribbon_group(seg) };
            local.map_or_else(|| (*seg).to_string(), |s| s.into_owned())
        })
        .collect();
    parts.join(" › ")
}

fn command_search(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let l = app.l10n;
    let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(480.0).hint_text(&*l.tr("Type a command, e.g. freeze, chart, bold…")));
    r.request_focus();
    let q = d.search.to_lowercase();
    let cmds: Vec<(String, String, String)> = app
        .session
        .commands()
        .into_iter()
        .filter(|c| c.enabled)
        .map(|c| {
            let shown = l.command_label(c.id).map_or_else(|| c.label.to_string(), |s| s.into_owned());
            (c, shown)
        })
        .filter(|(c, shown)| {
            q.is_empty() || shown.to_lowercase().contains(&q) || c.label.to_lowercase().contains(&q) || c.id.to_lowercase().contains(&q)
        })
        .map(|(c, shown)| (c.id.to_string(), shown, menu_path(l, &c.menu)))
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
    let l = app.l10n;
    let get = |k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let (mut set, mut to, mut changing) = (get("set"), get("to"), get("changing"));
    egui::Grid::new("gs").show(ui, |ui| {
        ui.label(&*l.tr("Set cell:"));
        ui.text_edit_singleline(&mut set);
        ui.end_row();
        ui.label(&*l.tr("To value:"));
        ui.text_edit_singleline(&mut to);
        ui.end_row();
        ui.label(&*l.tr("By changing cell:"));
        ui.text_edit_singleline(&mut changing);
        ui.end_row();
    });
    d.values.insert("set".into(), json!(set.clone()));
    d.values.insert("to".into(), json!(to.clone()));
    d.values.insert("changing".into(), json!(changing.clone()));
    let mut open = true;
    ok_cancel(ui, l, confirm, &mut open);
    if *confirm {
        match app.parse_number(&to) {
            None => {
                // Keep the dialog open so the typed value can be corrected.
                *confirm = false;
                let text = l.text("ui-dialogs-not-a-number", &[("text", Arg::from(to.trim()))]).into_owned();
                app.message = Some((l.tr("Goal Seek").into_owned(), text));
            }
            Some(to_value) => match app.run_typed("data.goalSeek", json!({"set": set, "to": to_value, "changing": changing})) {
                Ok(r) => {
                    let value = r["value"].as_f64().map_or_else(|| r["value"].to_string(), |n| app.number_text(n));
                    let msg = l.text("ui-dialogs-goal-seek-found", &[("value", Arg::from(value.as_str()))]);
                    app.toast = Some((msg.into_owned(), crate::now_ms()));
                }
                Err(e) => app.message = Some((l.tr("Goal Seek").into_owned(), app.error_text(&e))),
            },
        }
    }
    if !open {
        *confirm = true;
    }
}

fn spelling(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let l = app.l10n;
    let r = d.result.clone().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(e);
        return;
    }
    let issues = r["issues"].as_array().cloned().unwrap_or_default();
    let Some(it) = issues.get(d.list_index) else {
        ui.label(&*l.tr("The spelling check is complete for the entire sheet."));
        if ui.button(&*l.tr("OK")).clicked() {
            *open = false;
        }
        return;
    };
    let word = it["word"].as_str().unwrap_or("").to_string();
    let cell = it["cell"].as_str().unwrap_or("").to_string();
    ui.label(&*l.text("ui-dialogs-not-in-dictionary", &[("cell", Arg::from(cell.as_str()))]));
    ui.label(egui::RichText::new(&word).strong().color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
    let sugg: Vec<String> =
        it["suggestions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    ui.label(&*l.tr("Suggestions:"));
    let mut pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
    for s in &sugg {
        if ui.selectable_label(pick == *s, s).clicked() {
            pick = s.clone();
        }
    }
    if sugg.is_empty() {
        ui.label(egui::RichText::new(l.tr("(No suggestions)")).italics());
    }
    d.values.insert("pick".into(), json!(pick.clone()));
    let _ = app.session.run("selection.set", json!({"cell": cell}));
    ui.horizontal(|ui| {
        if ui.button(&*l.tr("Ignore Once")).clicked() {
            d.list_index += 1;
        }
        if ui.button(&*l.tr("Add to Dictionary")).clicked() {
            let _ = app.session.run("review.addToDictionary", json!({"word": word}));
            d.list_index += 1;
        }
        if !pick.is_empty() {
            if ui.button(&*l.tr("Change")).clicked() {
                app.run_or_alert("review.changeSpelling", json!({"cell": cell, "word": word, "to": pick}));
                d.list_index += 1;
            }
            if ui.button(&*l.tr("Change All")).clicked() {
                app.run_or_alert("review.changeSpelling", json!({"word": word, "to": pick, "all": true}));
                d.list_index += 1;
            }
        }
    });
}
