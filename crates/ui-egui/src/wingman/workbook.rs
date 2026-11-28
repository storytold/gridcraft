//! Workbook capabilities of the desktop assistant. Only entries in this table can become
//! engine commands. Never infer permissions from a provider's tool label, command prefixes,
//! or the engine's `query` flag (some UI queries toggle a pane).

#[path = "workbook_analysis.rs"]
mod analysis;
#[path = "workbook_appearance.rs"]
mod appearance;
#[path = "workbook_buttons.rs"]
mod buttons;
#[path = "workbook_context.rs"]
mod context;
#[path = "workbook_data.rs"]
mod data;
#[path = "workbook_formula.rs"]
mod formula;
#[path = "workbook_layout.rs"]
mod layout;
#[path = "workbook_menu.rs"]
pub(super) mod menu;
#[path = "workbook_profile.rs"]
mod profile;
#[path = "workbook_ribbon.rs"]
pub(super) mod ribbon;
#[path = "workbook_search.rs"]
mod search;

use crate::SheetApp;
use gridcraft_engine::core::{CellRef, RangeRef};
use serde_json::{Value as Json, json};

pub(super) use context::{MENTIONS, mention};
pub(super) use search::Matcher;

const MAX_ARGS: usize = 64 * 1024;
const MAX_RESULT: usize = 32 * 1024;
const MAX_CELLS: u64 = 1024;
const PATTERN_CHARS: usize = 512;
const CHARTS: &[&str] = &[
    "column",
    "bar",
    "line",
    "pie",
    "doughnut",
    "area",
    "scatter",
    "bubble",
    "radar",
    "combo",
    "histogram",
    "waterfall",
    "funnel",
    "treemap",
    "sunburst",
    "boxWhisker",
    "stock",
];

struct Tool {
    name: &'static str,
    help: &'static str,
    /// None is a strictly read-only inspection, not a dispatch through a model-supplied ID.
    command: Option<&'static str>,
    required: &'static [&'static str],
    optional: &'static [&'static str],
}

macro_rules! tool {
    ($name:literal, $help:literal, $cmd:expr, [$($r:literal),*], [$($o:literal),*]) => {
        Tool { name: $name, help: $help, command: $cmd, required: &[$($r),*], optional: &[$($o),*] }
    };
}

const COMMENTS_TO_COLUMN: Tool = tool!(
    "comments_to_column",
    "Extract full notes and comment threads as literal text. By default rows stay aligned; destinationStartRow maps startRow (default 1) to a different starting row, preserving gaps. Copy keeps originals (default); use move only when asked to remove them. Optional headerText is centered (headerRow defaults to 1). Rejects occupied destinations rather than overwriting. One undo step; do not reconstruct text from inspection previews.",
    Some("review.commentsToColumn"),
    ["sheet", "sourceColumn", "destinationColumn"],
    ["mode", "startRow", "endRow", "destinationStartRow", "headerText", "headerRow"]
);

pub(super) fn comment_transfer_params() -> Json {
    COMMENTS_TO_COLUMN.params()
}

const TOOLS: &[Tool] = &[
    tool!(
        "inspect_visual",
        "Send a picture or a native rendered chart to the vision model. Omit arguments for the selected floating picture/chart or single in-cell picture. Or choose a chart on a visible sheet by numeric chart ID or exact chartTitle; use list_objects kind charts to find it. One visual per request. Never reads files or captures the screen.",
        None,
        [],
        ["chart", "chartTitle", "sheet"]
    ),
    tool!(
        "profile_range",
        "Count stored/cached types per column (including blanks, formula errors and numeric-looking text), with example cell addresses and row pagination. Does not recalculate or change data.",
        None,
        ["range"],
        ["sheet", "offset"]
    ),
    tool!(
        "select_range",
        "Select a visible rectangle in the actual grid. Following edits on the same sheet may use @selection as their range or fill source. Selection by itself is navigation.",
        Some("selection.set"),
        ["sheet", "range"],
        []
    ),
    tool!(
        "number_format",
        "Apply a Home number format such as Currency or Percentage to a range. Supply either a ribbon format or a custom code.",
        Some("home.numberFormat"),
        ["sheet", "range"],
        ["format", "code"]
    ),
    tool!(
        "drag_fill",
        "Drag the native fill handle along one axis. target includes the source (or @selection) plus new rows or columns. To fill a block from one cell, fill across then down. Copy mode shifts relative formula references; $ anchors stay fixed.",
        Some("edit.autoFill"),
        ["sheet", "source", "target"],
        ["fillMode"]
    ),
    tool!(
        "transfer_range",
        "Copy, move, or transpose a worksheet block to at. Copies normally adjust relative references; use keepReferences to copy cell and validation formulas verbatim, even when transposing. Whole columns or rows trim to their occupied span. Overwrite must be explicit.",
        Some("range.transfer"),
        ["sheet", "source", "at"],
        ["destinationSheet", "operation", "overwrite", "keepReferences"]
    ),
    tool!(
        "reshape_delimited",
        "Split delimited text locally into literal fields. rows: one output row per input cell; columns: one column per cell; row: flatten all fields into one row.",
        Some("range.reshapeDelimited"),
        ["sheet", "source", "at", "delimiter"],
        ["destinationSheet", "layout", "overwrite"]
    ),
    tool!(
        "set_borders",
        "Apply a native border preset, optionally with a line style and RGB color.",
        Some("home.borders"),
        ["sheet", "range", "preset"],
        ["lineStyle", "color"]
    ),
    tool!("merge_cells", "Merge a rectangle without discarding occupied cells.", Some("home.mergeCells"), ["sheet", "range"], []),
    tool!("unmerge_cells", "Unmerge whole merged areas, keeping contents at their anchors.", Some("home.unmergeCells"), ["sheet", "range"], []),
    tool!("inspect_format", "Inspect actual range styles, borders, fills, alignment and merges.", None, ["range"], ["sheet", "offset"]),
    tool!("recommend_charts", "Rank native chart types suitable for the range.", None, ["sheet", "range"], []),
    tool!("inspect_chart", "Read back chart settings and series by chart ID.", None, ["sheet", "chart"], ["offset"]),
    tool!("workbook_info", "Inspect visible sheets and the active selection; no local file paths.", None, [], ["offset"]),
    tool!(
        "grep",
        "Find matching cells without knowing addresses. Search all visible sheets by default, or pass sheet/scope=sheet to narrow it. Literal substring by default; regex=true enables regex. Use searchIn=value for cell text, formula for expressions, comment for notes/threads. Do not enumerate cells. Continue with identical filters and nextCursor; null ends the search. Report skippedOversizeFields if nonzero.",
        None,
        ["pattern"],
        ["sheet", "scope", "range", "searchIn", "regex", "caseSensitive", "limit", "cursor"]
    ),
    tool!(
        "inspect_formula",
        "Explain a cell using its value, formula and direct static precedents or dependents. Ranges stay ranges; honor the reported index state, truncation and dynamic references. Use page.nextOffset for subsequent pages.",
        None,
        ["cell"],
        ["sheet", "direction", "offset"]
    ),
    tool!(
        "formula_diagnostics",
        "Find cached spreadsheet errors such as #REF! across visible sheets or within a worksheet/range. Text that only looks like an error is excluded. This does not recalculate. Use nextCursor for further pages and inspect_formula on a reported address.",
        None,
        [],
        ["sheet", "scope", "range", "limit", "cursor"]
    ),
    tool!(
        "check_formula",
        "Lint a proposed =formula before editing: native parser, function signatures and worksheet context. Does not evaluate or write a cell. Optionally give its intended sheet and cell.",
        None,
        ["formula"],
        ["sheet", "cell"]
    ),
    tool!(
        "read_range",
        "Read values and formulas in an explicit A1 range. Continue at nextOffset for large results.",
        None,
        ["range"],
        ["sheet", "offset"]
    ),
    tool!(
        "inspect_cell",
        "Inspect a known address for its value, formula, style and comment thread. For an unknown address or to locate text, use grep instead of trying individual cells. Truncation flags mark incomplete comment previews.",
        None,
        ["cell"],
        ["sheet"]
    ),
    tool!(
        "list_objects",
        "Find charts by title or ID, tables or defined names by name (optional literal search). For comments, list by column, e.g. AB, or use grep to search their text. Pass page.nextOffset with the same filter; null means finished.",
        None,
        ["kind"],
        ["sheet", "column", "search", "offset"]
    ),
    tool!(
        "find_commands",
        "Look up GridCraft commands, ribbon paths and parameter hints. Documentation only; a listed command is not permission to run it.",
        None,
        [],
        ["search", "section", "group", "offset"]
    ),
    tool!("find_functions", "Find supported spreadsheet formulas, signatures and descriptions by name or keyword.", None, ["search"], ["offset"]),
    tool!("list_scripts", "List workbook-native Automate scripts and imported VBA project metadata, without execution.", None, [], ["offset"]),
    tool!(
        "read_script",
        "Read the native steps of an Automate script. Arguments outside Wingman's workbook capabilities are omitted.",
        None,
        ["name"],
        ["offset"]
    ),
    tool!(
        "set_cell",
        "Set one cell as typed. An initial = writes a formula. An empty input explicitly clears the cell.",
        Some("cell.set"),
        ["sheet", "cell", "input"],
        []
    ),
    tool!(
        "set_values",
        "Write a rectangular matrix at a top-left A1 address, as one undo step. Strings starting with = are formulas. Set parse=false for literal non-formula strings.",
        Some("range.setValues"),
        ["sheet", "range", "values"],
        ["parse"]
    ),
    tool!(
        "format_range",
        "Apply only the supplied font, alignment or number-format properties.",
        Some("home.formatCells"),
        ["sheet", "range", "style"],
        []
    ),
    tool!("set_fill", "Set the range's fill to a #RRGGBB color or none.", Some("home.fillColor"), ["sheet", "range", "color"], []),
    tool!("set_ink", "Set the range's font color to #RRGGBB or auto.", Some("home.fontColor"), ["sheet", "range", "color"], []),
    tool!(
        "clear_contents",
        "Clear cell values and formulas within this rectangle without removing formatting. Matches the workbook's Clear Contents command.",
        Some("edit.clearContents"),
        ["sheet", "range"],
        []
    ),
    tool!(
        "fill_down",
        "Copy the rectangle's first row into its following rows, adjusting relative formula references.",
        Some("edit.fillDown"),
        ["sheet", "range"],
        []
    ),
    tool!(
        "fill_right",
        "Copy the rectangle's first column into its following columns, adjusting relative formula references.",
        Some("edit.fillRight"),
        ["sheet", "range"],
        []
    ),
    tool!(
        "size_rows",
        "Set the height in points of each whole row touched by the given A1 rectangle.",
        Some("home.rowHeight"),
        ["sheet", "range", "points"],
        []
    ),
    tool!(
        "size_columns",
        "Set the width in characters of each whole column touched by the given A1 rectangle.",
        Some("home.columnWidth"),
        ["sheet", "range", "chars"],
        []
    ),
    tool!(
        "sort_rows",
        "Sort the rows of the exact rectangle, using absolute worksheet column letters in priority order. header=true preserves the first row.",
        Some("data.sort"),
        ["sheet", "range", "keys", "header"],
        ["matchCase"]
    ),
    tool!(
        "enable_filter",
        "Install a worksheet AutoFilter on an unfiltered, non-table rectangle, starting with its header row. Existing filters are never toggled off.",
        Some("data.filter"),
        ["sheet", "range"],
        []
    ),
    tool!(
        "filter_column",
        "Filter a column of the active sheet's existing AutoFilter. Supply its exact range and absolute column letter. Pass matches (including an empty string for blanks) OR clear=true.",
        Some("data.filterBy"),
        ["sheet", "range", "column"],
        ["matches", "clear"]
    ),
    tool!(
        "data_bars",
        "Add conditional-format data bars in a #RRGGBB color to the given rectangle; numerical values stay visible.",
        Some("home.conditionalFormat"),
        ["sheet", "range", "color"],
        ["gradient"]
    ),
    tool!("set_comment", "Create or edit a threaded cell comment.", Some("review.newComment"), ["sheet", "cell", "text"], []),
    tool!("set_note", "Create or edit a classic cell note.", Some("review.newNote"), ["sheet", "cell", "text"], []),
    COMMENTS_TO_COLUMN,
    tool!("reply_comment", "Reply to an existing threaded cell comment.", Some("review.replyComment"), ["sheet", "cell", "text"], []),
    tool!(
        "resolve_comment",
        "Resolve or reopen an existing threaded cell comment.",
        Some("review.resolveComment"),
        ["sheet", "cell", "resolved"],
        []
    ),
    tool!(
        "create_chart",
        "Create a chart from an explicit data rectangle; type auto uses GridCraft's native recommendation. at is its optional top-left anchor.",
        Some("insert.chart"),
        ["sheet", "range", "type"],
        ["title", "at", "subtype", "width", "height"]
    ),
    tool!(
        "update_chart",
        "Change the identified chart on the active sheet; chart is its numeric ID from list_objects.",
        Some("chart.set"),
        ["sheet", "chart"],
        ["type", "title", "xTitle", "yTitle", "legend", "dataLabels", "gridlines", "at", "subtype", "width", "height"]
    ),
    tool!(
        "create_table",
        "Create a named spreadsheet table over an explicit rectangle.",
        Some("insert.table"),
        ["sheet", "range", "name", "header"],
        []
    ),
    tool!("add_sheet", "Insert and activate a worksheet with the given name.", Some("home.insertSheet"), ["name"], []),
    tool!(
        "rename_table",
        "Rename the named table on the given sheet; structured references follow the rename.",
        Some("table.rename"),
        ["sheet", "table", "name"],
        []
    ),
    tool!(
        "define_range",
        "Create a fresh workbook name for an absolute range on a visible worksheet; existing names and tables are never overwritten.",
        Some("formulas.defineName"),
        ["sheet", "range", "name"],
        []
    ),
    tool!("rename_sheet", "Rename a visible worksheet, preserving references through the engine.", Some("sheet.rename"), ["sheet", "name"], []),
    tool!(
        "save_script",
        "Store (never run) a native Automate script. Each step has a tool (set_cell, set_values, format_range, set_fill, set_ink or comments_to_column) and args including its target sheet. Set replace=true to overwrite.",
        Some("automate.newScript"),
        ["name", "steps"],
        ["replace"]
    ),
];

fn tools() -> impl Iterator<Item = &'static Tool> {
    TOOLS.iter().chain(buttons::TOOLS).chain(layout::tools())
}
fn lookup(name: &str) -> Result<&'static Tool, String> {
    tools().find(|t| t.name == name).ok_or_else(|| format!("Unknown Wingman tool: {name}"))
}

pub(super) fn resolve_visual(app: &SheetApp, args: &Json) -> Result<super::vision::Resolved, String> {
    validate(&lookup("inspect_visual")?.params(), args, "args")?;
    let doc = app.session.active().ok_or("No open workbook.")?;
    let sh = context::sheet(doc, args)?;
    super::vision::resolve(app, doc, sh, args)
}

pub(super) fn inspect_visual(app: &SheetApp, args: &Json, ctx: &egui::Context) -> Result<super::vision::Pending, String> {
    super::vision::start(resolve_visual(app, args)?, ctx)
}

fn string(max: usize) -> Json {
    json!({"type":"string","maxLength":max})
}

fn object(properties: Json, required: &[&str]) -> Json {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn field(k: &str) -> Json {
    let boolean = || json!({"type":"boolean"});
    match k {
        "tab" => json!({"type":"string","enum":ribbon::SECTIONS}),
        "chartTitle" => string(255),
        "section" | "group" => string(120),
        "actions" => {
            json!({"type":"array","minItems":1,"maxItems":16,"items":object(json!({"tool":string(64),"args":{"type":"object"}}), &["tool","args"])})
        }
        "operation" => json!({"type":"string","enum":["copy","move","transpose"]}),
        "layout" => json!({"type":"string","enum":["rows","columns","row"]}),
        "source" | "target" => string(42),
        "delimiter" => string(8),
        "overwrite" | "keepReferences" => boolean(),
        "fillMode" => {
            json!({"type":"string","enum":["copy","series","formats","values","extend","days","weekdays","months","years","linearTrend","growthTrend"]})
        }
        "format" => {
            json!({"type":"string","enum":["General","Number","Currency","Accounting","Short Date","Long Date","Time","Percentage","Fraction","Scientific","Text","Comma"]})
        }
        "code" => string(255),
        "width" | "height" => json!({"type":"number","minimum":32,"maximum":4096}),
        "subtype" => json!({"type":"string","enum":["clustered","stacked","stacked100","markers","stacked100Markers","lines"]}),
        "pattern" => string(PATTERN_CHARS),
        "scope" => json!({"type":"string","enum":["sheet","workbook"]}),
        "searchIn" => json!({"type":"string","enum":["all","value","display","formula","comment","error"]}),
        "regex" | "caseSensitive" => boolean(),
        "limit" => json!({"type":"integer","minimum":1,"maximum":50}),
        "cursor" => string(128),
        "direction" => json!({"type":"string","enum":["precedents","dependents"]}),
        "formula" => string(gridcraft_engine::formula::MAX_FORMULA_LEN + 1),
        "sourceColumn" | "destinationColumn" => json!({"type":"string","maxLength":3,"description":"Excel column letters, A through XFD"}),
        "mode" => json!({"type":"string","enum":["copy","move"],"default":"copy"}),
        "startRow" | "endRow" | "destinationStartRow" | "headerRow" => {
            json!({"type":"integer","minimum":1,"maximum":gridcraft_engine::core::MAX_ROWS})
        }
        "headerText" => string(32767),
        "cell" | "at" => {
            json!({"type":"string","description":"Unqualified A1 cell address. @selection requires one selected cell on the target sheet.","maxLength":20})
        }
        "range" => {
            json!({"type":"string","description":"One unqualified A1 address, rectangular A1:B5 range, or @selection on the target sheet. Whole axes only where the tool supports them.","maxLength":42})
        }
        "type" => json!({"type":"string","enum":CHARTS}),
        "kind" => json!({"type":"string","enum":["charts","comments","tables","names"]}),
        "legend" => json!({"type":"string","enum":["none","top","right","left","bottom"]}),
        "chart" | "offset" => json!({"type":"integer","minimum":0,"maximum":u32::MAX}),
        "header" | "gridlines" | "dataLabels" | "resolved" | "parse" | "replace" | "matchCase" | "clear" | "gradient" => boolean(),
        "column" => json!({"type":"string","maxLength":3,"description":"Absolute worksheet column letters, e.g. B or AA"}),
        "points" => json!({"type":"number","minimum":0.1,"maximum":409.5}),
        "chars" => json!({"type":"number","minimum":0.1,"maximum":255}),
        "keys" => {
            json!({"type":"array","minItems":1,"maxItems":8,"items":object(json!({"column":field("column"),"order":{"type":"string","enum":["asc","desc"]}}), &["column","order"])})
        }
        "matches" => json!({"type":"array","minItems":1,"maxItems":128,"items":string(255)}),
        "style" | "preset" | "lineStyle" => appearance::field(k),
        "values" => {
            json!({"type":"array","minItems":1,"maxItems":MAX_CELLS,"items":{"type":"array","minItems":1,"maxItems":MAX_CELLS,"items":{"type":["string","number","boolean","null"]}}})
        }
        "steps" => {
            json!({"type":"array","minItems":1,"maxItems":64,"items":object(json!({"tool":string(64),"args":{"type":"object"}}), &["tool","args"])})
        }
        "input" | "text" => string(32767),
        "title" | "xTitle" | "yTitle" => string(1024),
        _ => buttons::field(k).or_else(|| layout::field(k)).unwrap_or_else(|| string(255)),
    }
}

impl Tool {
    fn params(&self) -> Json {
        let props = self
            .required
            .iter()
            .chain(self.optional)
            .map(|&k| {
                let schema = if (self.name, k) == ("create_chart", "type") {
                    json!({"type":"string","enum":std::iter::once("auto").chain(CHARTS.iter().copied()).collect::<Vec<_>>()})
                } else if (self.name, k) == ("inspect_formula", "cell") {
                    json!({"type":"string","description":"A1 cell address, optionally sheet-qualified, e.g. 'Quarter 1'!B2","maxLength":320})
                } else if matches!(self.name, "grep" | "formula_diagnostics") && k == "range" {
                    json!({"type":"string","description":"Optional A1 range on each searched sheet, e.g. A1:D100 or AB:AB","maxLength":42})
                } else {
                    field(k)
                };
                (k.into(), schema)
            })
            .collect();
        object(Json::Object(props), self.required)
    }
}

pub(super) fn schemas(allow_write: bool, ribbon: &ribbon::Ribbon, editor: bool) -> Vec<Json> {
    let mut out: Vec<_> = tools()
        .filter(|t| ribbon.includes(t.name) && (t.command.is_none() || allow_write && !editor))
        .map(|t| json!({"type":"function","function":{"name":t.name,"description":t.help,"parameters":t.params()}}))
        .collect();
    if editor && ribbon.includes("read_macro_draft") {
        out.extend(super::script_editor::schemas(allow_write));
    }
    out.push(json!({"type":"function","function":{"name":"change_tab","description":"Switch your tool menu, not the user's ribbon. Open a tab to see its groups. Optionally narrow it to one exact group. Home: format/move/fill. Insert: charts/pivots. Formulas: lint/dependencies. Data: filter/sort/split. Review: comments. Automate: native scripts. External: connected MCP.","parameters":ribbon::switch_schema()}}));
    if allow_write && !editor {
        out.push(json!({"type":"function","function":{"name":"apply_edits","description":"Apply 1–16 ordered spreadsheet actions with one user approval and one Undo. Use worksheet tool names and args from tabs opened this turn. No scripts, external tools, or nested plans. If a step fails, roll back the plan.","parameters":object(json!({"actions":field("actions")}),&["actions"])}}));
    }
    out
}

pub(super) fn is_write(name: &str) -> Result<bool, String> {
    if name == "change_tab" {
        return Ok(false);
    }
    if name == "apply_edits" {
        return Ok(true);
    }
    if let Some(write) = super::script_editor::is_write(name) {
        return Ok(write);
    }
    Ok(lookup(name)?.command.is_some())
}

// Validate the same small subset of JSON Schema we advertise. Providers may ignore schemas
// entirely. The engine accepts soft defaults for the ribbon; they are inappropriate for tools.
pub(super) fn valid_field(key: &str, value: &Json) -> bool {
    validate(&field(key), value, key).is_ok()
}

fn validate(spec: &Json, value: &Json, path: &str) -> Result<(), String> {
    let fail = || format!("Invalid {path}; expected {}", spec["type"]);
    let kind = spec["type"].as_str().or_else(|| {
        spec["type"].as_array()?.iter().filter_map(Json::as_str).find(|t| match *t {
            "boolean" => value.is_boolean(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.is_i64(),
            "null" => value.is_null(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => false,
        })
    });
    match kind {
        Some("object") => {
            let fields = value.as_object().ok_or_else(fail)?;
            if let Some(keys) = spec["required"].as_array() {
                for key in keys.iter().filter_map(Json::as_str) {
                    if !fields.contains_key(key) {
                        return Err(format!("Missing {path}.{key}"));
                    }
                }
            }
            if let Some(props) = spec["properties"].as_object() {
                for (k, v) in fields {
                    let child = props.get(k).ok_or_else(|| format!("Unknown property {path}.{k}"))?;
                    validate(child, v, &format!("{path}.{k}"))?;
                }
            }
        }
        Some("array") => {
            let items = value.as_array().ok_or_else(fail)?;
            if (items.len() as u64) < spec["minItems"].as_u64().unwrap_or(0) || items.len() as u64 > spec["maxItems"].as_u64().unwrap_or(MAX_CELLS) {
                return Err(format!("Invalid number of entries at {path}"));
            }
            for item in items {
                validate(&spec["items"], item, path)?;
            }
        }
        Some("string") => {
            let s = value.as_str().ok_or_else(fail)?;
            if s.chars().count() as u64 > spec["maxLength"].as_u64().unwrap_or(32767) {
                return Err(format!("{path} is too long"));
            }
            if spec["enum"].as_array().is_some_and(|allowed| !allowed.contains(value)) {
                return Err(format!("Unsupported value at {path}"));
            }
        }
        Some("boolean") if value.is_boolean() => {}
        Some("null") if value.is_null() => {}
        Some("integer")
            if value
                .as_i64()
                .is_some_and(|n| n >= spec["minimum"].as_i64().unwrap_or(0) && n <= spec["maximum"].as_i64().unwrap_or(i64::from(u32::MAX))) => {}
        Some("number")
            if value.as_f64().is_some_and(|n| {
                n.is_finite() && n >= spec["minimum"].as_f64().unwrap_or(f64::MIN) && n <= spec["maximum"].as_f64().unwrap_or(f64::MAX)
            }) => {}
        _ => return Err(fail()),
    }
    Ok(())
}

pub(super) fn execute(app: &mut SheetApp, name: &str, args: &Json, allow_write: bool) -> Result<Json, String> {
    execute_with_matcher(app, name, args, allow_write, &mut None)
}

pub(super) fn execute_with_matcher(
    app: &mut SheetApp,
    name: &str,
    args: &Json,
    allow_write: bool,
    matcher: &mut Option<Matcher>,
) -> Result<Json, String> {
    if is_write(name)? && !allow_write {
        return Err("This profile cannot change the workbook.".into());
    }
    if args.to_string().len() > MAX_ARGS {
        return Err("Arguments exceed 64 KiB; split the request.".into());
    }
    if super::script_editor::is_write(name).is_some() {
        return super::script_editor::execute(app, name, args, allow_write);
    }
    let tool = lookup(name)?;
    validate(&tool.params(), args, name)?;
    let resolved;
    let args = if tool.command.is_none() {
        resolved = ribbon::resolve_selection(app.session.active().ok_or("No open workbook")?, args)?;
        &*resolved
    } else {
        args
    };
    let result = if let Some(command) = tool.command {
        let d = app.session.active().ok_or("No open workbook")?;
        let p = write_args(d, tool, args)?;
        if name == "save_script" && args["replace"] != true {
            let scripts = app.run("automate.allScripts", json!({}))?;
            if scripts.as_array().is_some_and(|a| a.iter().any(|s| s["name"] == args["name"])) {
                return Err("That script already exists; replace must be explicit.".into());
            }
        }
        let (value, undo_label) = if name == "select_range" {
            super::history::transaction(app, "Wingman: Select range", |s| s.execute(command, p))?
        } else {
            super::history::edit(app, command, p)?
        };
        // Some commands return null or a potentially large report of auto-converted entries.
        // Never tell the model an already applied edit failed merely because its receipt is large.
        let receipt = json!({"applied":true,"undoLabel":undo_label,"result":value});
        if receipt.to_string().len() > MAX_RESULT { json!({"applied":true,"undoLabel":undo_label,"detailsOmitted":true}) } else { receipt }
    } else {
        match name {
            "grep" => search::grep_slice(app.session.active().ok_or("No open workbook")?, args, matcher)?,
            "formula_diagnostics" => {
                let mut query = args.clone();
                query["searchIn"] = json!("error");
                query["pattern"] = json!("#");
                if query.get("sheet").is_none() && query.get("scope").is_none() {
                    query["scope"] = json!("workbook");
                }
                search::grep_slice(app.session.active().ok_or("No open workbook")?, &query, matcher)?
            }
            "inspect_formula" => formula::query(app, args)?,
            "inspect_format" => appearance::inspect(app.session.active().ok_or("No open workbook")?, args)?,
            "profile_range" => profile::inspect(app.session.active().ok_or("No open workbook")?, args)?,
            "check_formula" | "recommend_charts" | "inspect_chart" => analysis::query(app, name, args)?,
            "inspect_table" | "inspect_rules" => buttons::read(app.session.active().ok_or("No open workbook")?, name, args)?,
            "inspect_layout" | "inspect_pivots" | "inspect_shapes" | "preview_goal_seek" => {
                let d = app.session.active().ok_or("No open workbook")?;
                layout::query(d, context::sheet(d, args)?, name, args)?
            }
            "find_commands" => menu::find(args)?,
            "find_functions" | "list_scripts" | "read_script" => script_or_function_page(app, name, args)?,
            _ => context::query(app, name, args)?,
        }
    };
    if result.to_string().len() > MAX_RESULT {
        return Err("Result exceeds 32 KiB. Read a smaller range or a single item.".into());
    }
    Ok(result)
}

fn script_or_function_page(app: &mut SheetApp, name: &str, args: &Json) -> Result<Json, String> {
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let command = match name {
        "read_script" => "automate.getScript",
        "list_scripts" => "automate.allScripts",
        _ => "formulas.functions",
    };
    let result = app.run(command, args.clone())?;
    if name == "read_script" {
        let steps = result["commands"].as_array().ok_or("Invalid native script")?;
        return Ok(context::page(
            steps.iter().skip(offset).map(|step| {
                let id = step["command"].as_str().unwrap_or("");
                // Stored scripts can contain filenames or credentials (for example protection
                // passwords). Only project the explicit fields of our in-workbook tool schemas.
                let Some(tool) = TOOLS.iter().find(|t| t.command == Some(id) && t.name != "save_script" && !data::translates(t.name)) else {
                    return json!({"command":context::clip(id,128),"parametersOmitted":true});
                };
                let mut p = serde_json::Map::new();
                for &k in tool.required.iter().chain(tool.optional) {
                    if let Some(v) = step["params"].get(k)
                        && valid_field(k, v)
                    {
                        p.insert(k.to_owned(), v.clone());
                    }
                }
                json!({"command":id,"tool":tool.name,"args":p})
            }),
            offset,
            16,
        ));
    }
    Ok(context::page(result.as_array().ok_or("Expected list")?.iter().skip(offset).cloned(), offset, 32))
}

fn write_args(d: &gridcraft_engine::DocState, t: &Tool, supplied: &Json) -> Result<Json, String> {
    let resolved = ribbon::resolve_selection(d, supplied)?;
    let args = &*resolved;
    let mut p = args.clone();
    let sh = context::sheet(d, args)?;
    if t.name == "select_range" {
        // Navigation still needs the write profile and a visible target. Native
        // selection.set decides whether a protected sheet permits this selection.
        args["range"].as_str().and_then(RangeRef::parse).ok_or("Specify one literal A1 range.")?;
        return Ok(p);
    }
    if d.read_only_source.is_some() {
        return Err("This workbook was opened read-only.".into());
    }
    // Cells and styles go through the corresponding protection-aware engine paths. The object
    // and comment commands assume the ribbon checked protection, so enforce it here as well.
    if sh.is_protected()
        && !matches!(
            t.name,
            "set_cell"
                | "set_values"
                | "format_range"
                | "set_fill"
                | "set_ink"
                | "set_borders"
                | "number_format"
                | "add_sheet"
                | "rename_sheet"
                | "size_rows"
                | "size_columns"
                | "sort_rows"
                | "filter_column"
                | "comments_to_column"
                | "clear_contents"
                | "fill_right"
                | "fill_down"
        )
    {
        return Err("The sheet is protected.".into());
    }
    if let Some(n) = args["name"].as_str()
        && n.trim().is_empty()
    {
        return Err("Name must not be blank.".into());
    }
    for key in ["cell", "at"] {
        if let Some(s) = args[key].as_str()
            && CellRef::parse(s).is_none()
        {
            return Err(format!("Invalid cell address in {key}"));
        }
    }
    if let Some(range) = args["range"].as_str() {
        match t.name {
            "filter_column" => {
                RangeRef::parse(range).ok_or("Specify one literal A1 range.")?;
            }
            "set_fill" | "set_ink" | "number_format" => {
                appearance::style_range(sh, args)?;
            }
            "format_range" | "set_borders" | "merge_cells" | "unmerge_cells" | "create_chart" | "clear_filter" | "reapply_filter"
            | "conditional_format" | "clear_format_rules" | "set_validation" | "clear_validation" | "remove_duplicates" => {} // Each native adapter validates its own work bound.
            _ => {
                limited_range(range)?;
            }
        }
    }
    match t.name {
        name if buttons::TOOLS.iter().any(|v| v.name == name) => buttons::prepare(d, sh, name, &mut p)?,
        name if layout::tools().iter().any(|v| v.name == name) => layout::prepare(d, sh, name, &mut p)?,
        "set_values" => matrix(args)?,
        "format_range" | "set_borders" | "merge_cells" | "unmerge_cells" => appearance::prepare(d, sh, t.name, &mut p)?,
        "number_format" => {
            if args.get("format").is_some() == args.get("code").is_some() || args["code"].as_str().is_some_and(|s| s.trim().is_empty()) {
                return Err("Specify exactly one ribbon format or nonempty custom number format code.".into());
            }
        }
        "drag_fill" => {
            let area = |k: &str| {
                args[k]
                    .as_str()
                    .and_then(RangeRef::parse)
                    .filter(|r| r.count() <= 100_000)
                    .ok_or("Specify source and target rectangles of at most 100,000 cells.")
            };
            let source = area("source")?;
            let target = area("target")?;
            if source.width() != target.width() && source.height() != target.height() {
                return Err("One drag fills one axis. Fill a row first, then drag that row down (these can share a plan).".into());
            }
            if source == target || !gridcraft_engine::fill::holds_source_in_corner(source, target) {
                return Err("The target must extend the source and contain it in a corner.".into());
            }
            if !d.wb.active().is_some_and(|a| a.uid == sh.uid) {
                return Err("Select a range on the requested sheet before dragging its handle.".into());
            }
            p = json!({"sheet":sh.name,"source":source.a1(),"target":target.a1(),"mode":args["fillMode"].as_str().unwrap_or("copy")});
        }
        "set_fill" | "set_ink" => {
            let color = args["color"].as_str().unwrap_or("");
            if !(color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit()))
                && color != if t.name == "set_fill" { "none" } else { "auto" }
            {
                return Err("Supply a #RRGGBB color, none (fill) or auto (ink).".into());
            }
        }
        "create_chart" => analysis::prepare_chart(d, sh, t.name, &mut p)?,
        "update_chart" => {
            if d.wb.active().map(|active| active.uid.get()) != Some(sh.uid.get()) {
                return Err("Chart updates apply only to the active sheet.".into());
            }
            if !t.optional.iter().any(|&k| args.get(k).is_some()) {
                return Err("Supply at least one chart property to change.".into());
            }
            analysis::prepare_chart(d, sh, t.name, &mut p)?;
        }
        "set_comment" | "set_note" | "reply_comment" | "resolve_comment" => prepare_comment(sh, t.name, &mut p)?,
        "save_script" => return script_steps_for(d, args),
        _ => data::prepare(d, sh, t.name, &mut p)?,
    }
    Ok(p)
}

fn matrix(args: &Json) -> Result<(), String> {
    let rows = args["values"].as_array().ok_or("Expected a rectangle of values")?;
    let cols = rows.first().and_then(Json::as_array).map_or(0, Vec::len);
    let r = limited_range(args["range"].as_str().ok_or("Missing destination range")?)?;
    if cols == 0 || rows.len().saturating_mul(cols) > MAX_CELLS as usize || rows.iter().any(|v| v.as_array().is_none_or(|v| v.len() != cols)) {
        return Err("Values must be a nonempty rectangular matrix of at most 1024 cells.".into());
    }
    if r.start.offset(rows.len() as i64 - 1, cols as i64 - 1).is_none()
        || !r.is_single() && (r.height() as usize != rows.len() || r.width() as usize != cols)
    {
        return Err("Matrix does not fit the destination; specify a top-left cell or its exact rectangle.".into());
    }
    Ok(())
}

fn prepare_comment(sh: &gridcraft_engine::model::Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    let at = p["cell"].as_str().and_then(CellRef::parse).ok_or("Invalid comment cell")?;
    let existing = sh.comments.get(&at);
    if name == "reply_comment" || name == "resolve_comment" {
        if !existing.is_some_and(|c| c.threaded) {
            return Err("This cell has no threaded comment.".into());
        }
    } else if existing.is_some_and(|c| c.threaded != (name == "set_comment")) {
        return Err("Changing a note to a thread (or vice versa) would discard the existing discussion.".into());
    }
    if p.get("text").is_some_and(|v| v.as_str().is_none_or(|s| s.trim().is_empty())) {
        return Err("Comment text must not be blank.".into());
    }
    if existing.is_none() || name == "reply_comment" {
        p["author"] = json!("Wingman");
    }
    Ok(())
}

pub(super) fn script_steps(app: &SheetApp, args: &Json) -> Result<Json, String> {
    script_steps_for(app.session.active().ok_or("No open workbook")?, args)
}

fn script_steps_for(d: &gridcraft_engine::DocState, args: &Json) -> Result<Json, String> {
    validate(&field("steps"), &args["steps"], "steps")?;
    let mut commands = Vec::new();
    for step in args["steps"].as_array().ok_or("Expected script steps")? {
        let tool = lookup(step["tool"].as_str().ok_or("Expected tool name")?)?;
        // When the user later runs this script outside Wingman, every saved command
        // must still honor its explicit sheet and recheck protection in the engine.
        if !matches!(tool.name, "set_cell" | "set_values" | "format_range" | "set_fill" | "set_ink" | "comments_to_column") {
            return Err("Only sheet-qualified cell, matrix, style and comment transfer steps can be stored by Wingman.".into());
        }
        let id = tool.command.ok_or("Only editable tools can be saved as script steps")?;
        validate(&tool.params(), &step["args"], tool.name)?;
        commands.push(json!({"command":id, "params":write_args(d, tool, &step["args"])?}));
    }
    Ok(json!({"name":args["name"],"commands":commands}))
}

fn limited_range(s: &str) -> Result<RangeRef, String> {
    let r = RangeRef::parse(s).ok_or("Expected an A1 address or rectangular range")?;
    if r.count() > MAX_CELLS {
        return Err("Range exceeds 1024 cells; split it into rectangles.".into());
    }
    Ok(r)
}
