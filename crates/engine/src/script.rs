//! The workbook side of the Office Scripts bridge: every call a script makes becomes a normal
//! engine command, so it is journaled, undoable and bounded exactly like the same action from the
//! ribbon. Errors come back as [`BridgeError`]s for the runtime to throw at the failing line.
//!
//! Getters go through the bridge-support queries (`script.rangeInfo`, `script.sheets`,
//! `script.selection`, `script.tables`, `script.charts`, `script.validation`, `script.workbook`,
//! `formulas.nameManager`, `cell.get`); setters and structural edits are the ordinary commands.
//! Address arithmetic (`getCell`, `getOffsetRange`, `getEntireRow`, …) is done on the parsed
//! range, because it is a fact about the address, not a workbook read.
//!
//! A member without a real counterpart is refused with a typed `notSupported` error naming the
//! class and member. `docs/office-scripts-coverage.md` lists every member and its status.

use serde_json::{Map, Value as Json, json};

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::ChartKind;
use gridcraft_script::BridgeError;

use crate::{Session, cmd};

/// Seconds a scripting call may take before the batch of formats/commands is too large.
const MAX_CELLS: u64 = 1_000_000;

/// Runs one bridge call as a command on the active workbook.
// The dispatch is one large table: each arm is a thin adapter, which is clearer than a dozen
// helper traits for this interface.
#[allow(clippy::cognitive_complexity)]
pub(crate) fn execute_call(s: &mut Session, method: &str, params: &Json) -> Result<Json, BridgeError> {
    match method {
        // ---------------------------------------------------------------- Workbook
        "Workbook.getActiveWorksheet" => {
            let info = command(s, "script.sheets", json!({}))?;
            let name = info.get("active").and_then(Json::as_str).ok_or_else(|| BridgeError::engine("no active worksheet"))?;
            Ok(json!({"sheet": name}))
        }
        "Workbook.getWorksheets" => {
            let info = command(s, "script.sheets", json!({}))?;
            Ok(info.get("sheets").cloned().unwrap_or_else(|| Json::Array(Vec::new())))
        }
        "Workbook.getSelectedRange" => {
            let info = command(s, "script.selection", json!({}))?;
            Ok(json!({"sheet": info["sheet"], "address": info["address"]}))
        }
        "Workbook.getSelectedRangeAreas" | "Workbook.getSelectedRanges" => {
            let info = command(s, "script.selection", json!({}))?;
            Ok(json!({"sheet": info["sheet"], "address": info["address"], "areas": info["areas"]}))
        }
        "Workbook.getActiveCell" => {
            let info = command(s, "script.selection", json!({}))?;
            Ok(json!({"sheet": info["sheet"], "address": info["active"]}))
        }
        "Workbook.getName" => Ok(Json::String(command(s, "script.workbook", json!({}))?["name"].as_str().unwrap_or("").to_string())),
        "Workbook.getIsDirty" => Ok(command(s, "script.workbook", json!({}))?["dirty"].clone()),
        "Workbook.getApplication" => {
            let info = command(s, "script.workbook", json!({}))?;
            Ok(json!({"name": info["app"], "version": info["version"]}))
        }
        "Workbook.addWorksheet" => {
            let mut p = Map::new();
            if let Some(name) = params.get("name").and_then(Json::as_str) {
                p.insert("name".into(), Json::String(name.to_string()));
            }
            if params.get("position").is_some_and(|v| !v.is_null()) {
                return Err(BridgeError::not_supported("Workbook.addWorksheet(position)"));
            }
            let result = command(s, "home.insertSheet", Json::Object(p))?;
            let name = result.get("name").cloned().unwrap_or(Json::Null);
            Ok(json!({"sheet": name}))
        }
        "Workbook.getNames" => command(s, "formulas.nameManager", json!({})),
        "Workbook.getNamedItem" => {
            let name = member_str(params, "name")?;
            let names = command(s, "formulas.nameManager", json!({}))?;
            Ok(names
                .as_array()
                .and_then(|a| a.iter().find(|n| n["name"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(&name))))
                .cloned()
                .unwrap_or(Json::Null))
        }

        // ---------------------------------------------------------------- NamedItem
        "NamedItem.getName" => Ok(Json::String(member_str(params, "name")?)),
        "NamedItem.getRefersTo"
        | "NamedItem.getFormula"
        | "NamedItem.getScope"
        | "NamedItem.getComment"
        | "NamedItem.getVisible"
        | "NamedItem.getType" => {
            let name = member_str(params, "name")?;
            let names = command(s, "formulas.nameManager", json!({}))?;
            let entry = names
                .as_array()
                .and_then(|a| a.iter().find(|n| n["name"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(&name))))
                .cloned()
                .ok_or_else(|| BridgeError::invalid_argument(format!("there is no name called `{name}`")))?;
            Ok(match method {
                "NamedItem.getRefersTo" | "NamedItem.getFormula" => entry["refersTo"].clone(),
                "NamedItem.getScope" => entry["scope"].clone(),
                "NamedItem.getComment" => entry["comment"].clone(),
                "NamedItem.getVisible" => Json::Bool(true),
                _ => Json::String("Range".into()),
            })
        }

        // ---------------------------------------------------------------- Worksheet
        "Worksheet.getRange" => {
            let (sheet, range) = checked_range(s, params)?;
            Ok(json!({"sheet": sheet, "address": range.a1()}))
        }
        "Range.getBoundingRect" | "Range.getRangeBetween" => {
            let first = parse_range(params)?;
            let text =
                params.get("otherRange").and_then(Json::as_str).ok_or_else(|| BridgeError::invalid_argument("this call needs `otherRange`"))?;
            let other = RangeRef::parse(text).ok_or_else(|| BridgeError::invalid_argument(format!("`{text}` is not a range")))?;
            let sheet = sheet_name(s, params)?;
            Ok(json!({"sheet": sheet, "address": first.union(&other).a1()}))
        }
        "Worksheet.getCell" => {
            let (sheet, cell) = checked_cell(s, params, "cell")?;
            Ok(json!({"sheet": sheet, "address": cell.a1()}))
        }
        "Worksheet.getUsedRange" => {
            let sheets = command(s, "script.sheets", json!({}))?;
            let sheet = sheet_name(s, params)?;
            let used = sheets["sheets"]
                .as_array()
                .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(sheet.as_str())))
                .and_then(|x| x["usedRange"].as_str())
                .unwrap_or("A1");
            Ok(json!({"sheet": sheet, "address": used}))
        }
        "Worksheet.getName" => Ok(Json::String(sheet_name(s, params)?)),
        "Worksheet.setName" => {
            let name = member_str(params, "name")?;
            let sheet = sheet_name(s, params)?;
            command(s, "sheet.rename", json!({"sheet": sheet, "name": name}))?;
            Ok(json!({"sheet": name}))
        }
        "Worksheet.activate" => {
            let sheet = sheet_name(s, params)?;
            command(s, "sheet.activate", json!({"sheet": sheet}))?;
            Ok(Json::Null)
        }
        "Worksheet.delete" => {
            let sheet = sheet_name(s, params)?;
            command(s, "home.deleteSheet", json!({"sheet": sheet}))?;
            Ok(Json::Null)
        }
        "Worksheet.getStandardHeight" | "Worksheet.getStandardWidth" => {
            let sheets = command(s, "script.sheets", json!({}))?;
            let sheet = sheet_name(s, params)?;
            let entry = sheets["sheets"]
                .as_array()
                .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(sheet.as_str())))
                .cloned()
                .unwrap_or(Json::Null);
            let key = if method.ends_with("Height") { "standardHeight" } else { "standardWidth" };
            Ok(entry[key].clone())
        }
        "Worksheet.getTables" => {
            let sheet = sheet_name(s, params)?;
            let tables = command(s, "script.tables", json!({"sheet": sheet}))?;
            Ok(Json::Array(tables.as_array().map(|a| a.iter().map(|t| json!({"name": t["name"]})).collect()).unwrap_or_default()))
        }
        "Worksheet.getCharts" => {
            let sheet = sheet_name(s, params)?;
            let charts = command(s, "script.charts", json!({"sheet": sheet}))?;
            Ok(Json::Array(charts.as_array().map(|a| a.iter().map(|c| json!({"id": c["id"], "name": c["name"]})).collect()).unwrap_or_default()))
        }
        "Worksheet.getIndex" => {
            let sheets = command(s, "script.sheets", json!({}))?;
            let sheet = sheet_name(s, params)?;
            Ok(sheets["sheets"]
                .as_array()
                .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(sheet.as_str())))
                .and_then(|x| x.get("index").cloned())
                .unwrap_or(Json::Null))
        }
        "Worksheet.getVisibility" => {
            let sheets = command(s, "script.sheets", json!({}))?;
            let sheet = sheet_name(s, params)?;
            let visible = sheets["sheets"]
                .as_array()
                .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(sheet.as_str())))
                .and_then(|x| x.get("visible").and_then(Json::as_bool))
                .unwrap_or(true);
            Ok(Json::String(if visible { "visible".into() } else { "hidden".into() }))
        }
        "Worksheet.setVisibility" => {
            let visibility = member_str(params, "visibility")?.to_ascii_lowercase();
            let sheet = sheet_name(s, params)?;
            if visibility == "visible" {
                command(s, "sheet.unhide", json!({"sheet": sheet}))?;
            } else {
                command(s, "sheet.hide", json!({"sheet": sheet}))?;
            }
            Ok(Json::Null)
        }

        // ---------------------------------------------------------------- Range getters
        "Range.getValue" => {
            let info = range_info(s, params)?;
            Ok(cell_value(&info, "values"))
        }
        "Range.getValues" => {
            let info = range_info(s, params)?;
            Ok(text_or_empty(info.get("values").cloned().unwrap_or_else(|| Json::Array(Vec::new()))))
        }
        "Range.getText" => {
            let info = range_info(s, params)?;
            Ok(cell_value(&info, "texts"))
        }
        "Range.getTexts" => {
            let info = range_info(s, params)?;
            Ok(info.get("texts").cloned().unwrap_or_else(|| Json::Array(Vec::new())))
        }
        "Range.getFormula" => {
            let info = range_info(s, params)?;
            let f = cell_value(&info, "formulas");
            Ok(if f.is_null() { Json::String(String::new()) } else { f })
        }
        "Range.getFormulas" => {
            let info = range_info(s, params)?;
            Ok(text_or_empty(info.get("formulas").cloned().unwrap_or_else(|| Json::Array(Vec::new()))))
        }
        "Range.getNumberFormat" => {
            let info = range_info(s, params)?;
            Ok(cell_value(&info, "numberFormats"))
        }
        "Range.getNumberFormats" => {
            let info = range_info(s, params)?;
            Ok(info.get("numberFormats").cloned().unwrap_or_else(|| Json::Array(Vec::new())))
        }
        "Range.getAddress" | "Range.getAddressLocal" => Ok(Json::String(parse_range(params)?.a1())),
        "Range.getRowCount" => Ok(json!(parse_range(params)?.height())),
        "Range.getColumnCount" => Ok(json!(parse_range(params)?.width())),
        "Range.getRowIndex" => Ok(json!(parse_range(params)?.start.row)),
        "Range.getColumnIndex" => Ok(json!(parse_range(params)?.start.col)),
        "Range.getCell" => {
            let range = parse_range(params)?;
            let row = member_u32(params, "row")?;
            let col = member_u32(params, "column")?;
            let at = range.start.row.checked_add(row).zip(range.start.col.checked_add(col)).map(|(r, c)| CellRef::new(r, c));
            let Some(at) = at.filter(|c| c.row < gridcraft_core::MAX_ROWS && c.col < gridcraft_core::MAX_COLS) else {
                return Err(BridgeError::invalid_argument("getCell is outside the worksheet"));
            };
            let sheet = sheet_name(s, params)?;
            Ok(json!({"sheet": sheet, "address": at.a1()}))
        }
        "Range.getOffsetRange" => {
            let range = parse_range(params)?;
            let dr =
                params.get("rowOffset").and_then(Json::as_i64).ok_or_else(|| BridgeError::invalid_argument("getOffsetRange needs `rowOffset`"))?;
            let dc = params
                .get("columnOffset")
                .and_then(Json::as_i64)
                .ok_or_else(|| BridgeError::invalid_argument("getOffsetRange needs `columnOffset`"))?;
            let move_by = |v: u32, d: i64| -> Option<u32> {
                let n = i64::from(v).checked_add(d)?;
                if n < 0 { None } else { u32::try_from(n).ok() }
            };
            let (r0, c0) = (move_by(range.start.row, dr), move_by(range.start.col, dc));
            let (r1, c1) = (move_by(range.end.row, dr), move_by(range.end.col, dc));
            let (Some(r0), Some(c0), Some(r1), Some(c1)) = (r0, c0, r1, c1) else {
                return Err(BridgeError::invalid_argument("getOffsetRange is outside the worksheet"));
            };
            let moved = RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1));
            if moved.end.row >= gridcraft_core::MAX_ROWS || moved.end.col >= gridcraft_core::MAX_COLS {
                return Err(BridgeError::invalid_argument("getOffsetRange is outside the worksheet"));
            }
            let sheet = sheet_name(s, params)?;
            Ok(json!({"sheet": sheet, "address": moved.a1()}))
        }
        "Range.getEntireRow" => {
            let range = parse_range(params)?;
            let sheet = sheet_name(s, params)?;
            Ok(json!({"sheet": sheet, "address": RangeRef::rows(range.start.row, range.end.row).a1()}))
        }
        "Range.getEntireColumn" => {
            let range = parse_range(params)?;
            let sheet = sheet_name(s, params)?;
            Ok(json!({"sheet": sheet, "address": RangeRef::cols(range.start.col, range.end.col).a1()}))
        }
        "Range.getWorksheet" => Ok(json!({"sheet": sheet_name(s, params)?})),
        "Range.getTables" => {
            let range = parse_range(params)?;
            let sheet = sheet_name(s, params)?;
            let tables = command(s, "script.tables", json!({"sheet": sheet}))?;
            let mut out = Vec::new();
            for t in tables.as_array().into_iter().flatten() {
                if let Some(address) = t["range"].as_str()
                    && let Some(tr) = RangeRef::parse(address)
                    && tr.intersects(&range)
                {
                    out.push(json!({"name": t["name"]}));
                }
            }
            Ok(Json::Array(out))
        }
        "Range.getDataValidation" => {
            let (sheet, cell) = checked_cell(s, params, "range")?;
            let rule = command(s, "script.validation", json!({"sheet": sheet, "cell": cell.a1()}))?;
            Ok(if rule.is_null() { Json::Null } else { json!({"sheet": sheet, "cell": cell.a1()}) })
        }
        "Range.getComment" => {
            let (sheet, cell) = checked_cell(s, params, "range")?;
            let info = command(s, "cell.get", json!({"sheet": sheet, "cell": cell.a1()}))?;
            Ok(if info.get("comment").is_some_and(|c| !c.is_null()) { json!({"sheet": sheet, "cell": cell.a1()}) } else { Json::Null })
        }

        // ---------------------------------------------------------------- Range setters
        "Range.setValue" | "Range.setValues" | "Range.setFormula" | "Range.setFormulas" | "Range.setText" | "Range.setTexts" => {
            range_set(s, method, params)
        }
        "Range.setNumberFormat" => {
            let range = parse_range(params)?;
            let code = member_str(params, "format")?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.numberFormat", json!({"sheet": sheet, "range": range.a1(), "code": code}))?;
            Ok(Json::Null)
        }
        "Range.setNumberFormats" => {
            let range = parse_range(params)?;
            let rows = params
                .get("formats")
                .and_then(Json::as_array)
                .ok_or_else(|| BridgeError::invalid_argument("setNumberFormats takes rows of formats"))?;
            if range.count() > MAX_CELLS {
                return Err(BridgeError::invalid_argument("setNumberFormats range is too large"));
            }
            let sheet = sheet_name(s, params)?;
            for (ri, row) in rows.iter().enumerate() {
                let Some(row) = row.as_array() else { return Err(BridgeError::invalid_argument("setNumberFormats takes rows of formats")) };
                for (ci, code) in row.iter().enumerate() {
                    let Some(code) = code.as_str() else { return Err(BridgeError::invalid_argument("a number format is text")) };
                    let at = CellRef::new(
                        range.start.row.saturating_add(u32::try_from(ri).unwrap_or(u32::MAX)),
                        range.start.col.saturating_add(u32::try_from(ci).unwrap_or(u32::MAX)),
                    );
                    if at.row > range.end.row || at.col > range.end.col {
                        continue;
                    }
                    command(s, "home.numberFormat", json!({"sheet": sheet, "range": at.a1(), "code": code}))?;
                }
            }
            Ok(Json::Null)
        }
        "Range.insert" => {
            let range = parse_range(params)?;
            let shift = params.get("shift").and_then(Json::as_str).unwrap_or("down").to_ascii_lowercase();
            let sheet = sheet_name(s, params)?;
            command(s, "home.insertCells", json!({"sheet": sheet, "range": range.a1(), "shift": shift}))?;
            Ok(Json::Null)
        }
        "Range.delete" => {
            let range = parse_range(params)?;
            let shift = params.get("shift").and_then(Json::as_str).unwrap_or("up").to_ascii_lowercase();
            let sheet = sheet_name(s, params)?;
            command(s, "home.deleteCells", json!({"sheet": sheet, "range": range.a1(), "shift": shift}))?;
            Ok(Json::Null)
        }
        "Range.clear" => {
            let range = parse_range(params)?;
            let apply = params.get("applyTo").and_then(Json::as_str).unwrap_or("all").to_ascii_lowercase();
            let id = match apply.as_str() {
                "contents" | "resercontents" | "resetcontents" => "edit.clearContents",
                "formats" => "edit.clearFormats",
                _ => "edit.clearAll",
            };
            let sheet = sheet_name(s, params)?;
            command(s, id, json!({"sheet": sheet, "range": range.a1()}))?;
            Ok(Json::Null)
        }
        "Range.merge" => {
            let range = parse_range(params)?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.mergeCells", json!({"sheet": sheet, "range": range.a1()}))?;
            Ok(Json::Null)
        }
        "Range.unmerge" => {
            let range = parse_range(params)?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.unmergeCells", json!({"sheet": sheet, "range": range.a1()}))?;
            Ok(Json::Null)
        }
        "Range.select" => {
            let range = parse_range(params)?;
            let sheet = sheet_name(s, params)?;
            command(s, "selection.set", json!({"sheet": sheet, "range": range.a1()}))?;
            Ok(Json::Null)
        }

        // ---------------------------------------------------------------- Format / Font / Fill / Borders
        "Range.getFormat" => Ok(json!({"kind": "format", "sheet": sheet_name(s, params)?, "range": parse_range(params)?.a1()})),
        "RangeFormat.getFont" => Ok(json!({"kind": "font", "sheet": sheet_name(s, params)?, "range": parse_range(params)?.a1()})),
        "RangeFormat.getFill" => Ok(json!({"kind": "fill", "sheet": sheet_name(s, params)?, "range": parse_range(params)?.a1()})),
        "RangeFormat.getBorders" => Ok(json!({"kind": "borders", "sheet": sheet_name(s, params)?, "range": parse_range(params)?.a1()})),
        "RangeFormat.getNumberFormat" => {
            let info = range_info(s, params)?;
            Ok(cell_value(&info, "numberFormats"))
        }
        "RangeFormat.setNumberFormat" => {
            let code = member_str(params, "format")?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.numberFormat", json!({"sheet": sheet, "range": parse_range(params)?.a1(), "code": code}))?;
            Ok(Json::Null)
        }
        "RangeFont.getBold"
        | "RangeFont.getItalic"
        | "RangeFont.getUnderline"
        | "RangeFont.getSize"
        | "RangeFont.getName"
        | "RangeFont.getColor"
        | "RangeFont.getStrikethrough"
        | "RangeFormat.getHorizontalAlignment"
        | "RangeFormat.getVerticalAlignment" => font_get(s, method, params),
        "RangeFont.setBold" => {
            let on = member_bool(params, "bold")?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.bold", json!({"sheet": sheet, "range": parse_range(params)?.a1(), "on": on}))?;
            Ok(Json::Null)
        }
        "RangeFont.setItalic" => {
            let on = member_bool(params, "italic")?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.italic", json!({"sheet": sheet, "range": parse_range(params)?.a1(), "on": on}))?;
            Ok(Json::Null)
        }
        "RangeFont.setUnderline" => {
            let style = member_str(params, "underline")?.to_ascii_lowercase();
            let (id, on) = match style.as_str() {
                "none" => ("home.underline", false),
                "double" | "doubleaccountant" => ("home.doubleUnderline", true),
                _ => ("home.underline", true),
            };
            let sheet = sheet_name(s, params)?;
            command(s, id, json!({"sheet": sheet, "range": parse_range(params)?.a1(), "on": on}))?;
            Ok(Json::Null)
        }
        "RangeFont.setSize" => {
            let size = params.get("size").and_then(Json::as_f64).ok_or_else(|| BridgeError::invalid_argument("setSize needs a number"))?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.fontSize", json!({"sheet": sheet, "range": parse_range(params)?.a1(), "size": size}))?;
            Ok(Json::Null)
        }
        "RangeFont.setName" => {
            let name = member_str(params, "name")?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.fontName", json!({"sheet": sheet, "range": parse_range(params)?.a1(), "name": name}))?;
            Ok(Json::Null)
        }
        "RangeFont.setColor" => {
            let color = member_str(params, "color")?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.fontColor", json!({"sheet": sheet, "range": parse_range(params)?.a1(), "color": color}))?;
            Ok(Json::Null)
        }
        "RangeFill.getColor" => {
            let info = range_info(s, params)?;
            Ok(style_field(&info, &["fill", "color"]))
        }
        "RangeFill.setColor" => {
            let color = member_str(params, "color")?;
            let sheet = sheet_name(s, params)?;
            command(s, "home.fillColor", json!({"sheet": sheet, "range": parse_range(params)?.a1(), "color": color}))?;
            Ok(Json::Null)
        }
        "RangeFormat.getRangeBorder" => {
            let side = member_str(params, "side")?.to_ascii_lowercase();
            let sheet = sheet_name(s, params)?;
            Ok(json!({"kind": "border", "sheet": sheet, "range": parse_range(params)?.a1(), "side": side}))
        }
        "RangeBorder.getColor" | "RangeBorder.getStyle" | "RangeBorder.getWeight" | "RangeBorder.getSideIndex" => border_get(s, method, params),
        "RangeBorder.setColor" | "RangeBorder.setStyle" | "RangeBorder.setWeight" => {
            let color = params.get("color").and_then(Json::as_str);
            let style = params.get("style").and_then(Json::as_str);
            let weight = params.get("weight").and_then(Json::as_str);
            let side = member_str(params, "side")?;
            let mut p = Map::new();
            p.insert("sheet".into(), Json::String(sheet_name(s, params)?));
            p.insert("range".into(), Json::String(parse_range(params)?.a1()));
            p.insert("edge".into(), Json::String(border_edge(side)?));
            if let Some(c) = color {
                p.insert("color".into(), Json::String(c.to_string()));
            }
            if let Some(s) = style {
                p.insert("style".into(), Json::String(script_border_style(s)));
            }
            if let Some(w) = weight {
                p.insert("style".into(), Json::String(weight_style(w, style)));
            }
            command(s, "home.drawBorder", Json::Object(p))?;
            Ok(Json::Null)
        }

        // ---------------------------------------------------------------- Table
        "Table.getRange" | "Table.getHeaderRowRange" | "Table.getDataBodyRange" | "Table.getTotalsRowRange" | "Table.getTotalRowRange" => {
            let table = table_info(s, params)?;
            let range = match method {
                "Table.getRange" => table["range"].clone(),
                "Table.getHeaderRowRange" => tables_row_range(&table, true),
                "Table.getTotalsRowRange" | "Table.getTotalRowRange" => tables_row_range(&table, false),
                _ => data_body_range(&table),
            };
            Ok(json!({"sheet": table["worksheet"], "address": range}))
        }
        "Table.getName" => Ok(table_info(s, params)?["name"].clone()),
        "Table.setName" => {
            let table = member_str(params, "table")?;
            let name = member_str(params, "name")?;
            command(s, "table.rename", json!({"table": table, "name": name}))?;
            Ok(json!({"table": name}))
        }
        "Table.getColumns" => {
            let table = table_info(s, params)?;
            Ok(Json::Array(
                table["columns"]
                    .as_array()
                    .map(|a| a.iter().map(|c| json!({"name": c["name"], "index": c["index"], "table": table["name"]})).collect())
                    .unwrap_or_default(),
            ))
        }
        "Table.getColumn" => {
            let table = table_info(s, params)?;
            let name = member_str(params, "name")?;
            let col = table["columns"].as_array().and_then(|a| a.iter().find(|c| c["name"].as_str() == Some(name.as_str()))).cloned();
            Ok(col.map(|c| json!({"name": c["name"], "index": c["index"], "table": table["name"]})).unwrap_or(Json::Null))
        }
        "Table.getStyle" | "Table.getPredefinedTableStyle" => Ok(table_info(s, params)?["style"].clone()),
        "Table.setStyle" | "Table.setPredefinedTableStyle" => {
            let table = member_str(params, "table")?;
            let style = member_str(params, "style")?;
            command(s, "table.style", json!({"table": table, "style": style}))?;
            Ok(Json::Null)
        }
        "Table.getShowHeaders" | "Table.getShowTotals" => {
            let table = table_info(s, params)?;
            Ok(table[if method.ends_with("Headers") { "headerRow" } else { "totalsRow" }].clone())
        }
        "Table.setShowHeaders" | "Table.setShowTotals" => {
            let table = member_str(params, "table")?;
            let on = member_bool(params, "on")?;
            let id = if method.ends_with("Headers") { "table.headerRow" } else { "table.totalRow" };
            command(s, id, json!({"table": table, "on": on}))?;
            Ok(Json::Null)
        }
        "Table.convertToRange" => {
            let table = member_str(params, "table")?;
            command(s, "table.convertToRange", json!({"table": table}))?;
            Ok(Json::Null)
        }
        "Table.delete" => {
            let table = table_info(s, params)?;
            // Conversion removes the table object and keeps its cells, which is what a script
            // deleting a table means in GridCraft's model (rows stay where the person put them).
            command(s, "table.convertToRange", json!({"table": table["name"]}))?;
            Ok(Json::Null)
        }
        "TableColumn.getRange"
        | "TableColumn.getHeaderRowRange"
        | "TableColumn.getDataBodyRange"
        | "TableColumn.getTotalsRowRange"
        | "TableColumn.getTotalRowRange"
        | "TableColumn.getRangeBetweenHeaderAndTotal" => {
            let (table, column) = table_column_info(s, params)?;
            let key = match method {
                "TableColumn.getRange" => "range",
                "TableColumn.getHeaderRowRange" => "headerRowRange",
                "TableColumn.getDataBodyRange" | "TableColumn.getRangeBetweenHeaderAndTotal" => "dataBodyRange",
                _ => "totalsRowRange",
            };
            Ok(json!({"sheet": table["worksheet"], "address": column[key]}))
        }
        "TableColumn.getName" => Ok(table_column_info(s, params)?.1["name"].clone()),
        "TableColumn.getIndex" => Ok(table_column_info(s, params)?.1["index"].clone()),

        // ---------------------------------------------------------------- Chart
        "Chart.getChartType" => Ok(chart_info(s, params)?["type"].clone()),
        "Chart.setChartType" => {
            let name = member_str(params, "type")?;
            let (kind, subtype) = chart_kind_from_name(&name);
            if !chart_name_known(&name) {
                return Err(BridgeError::not_supported(format!("Chart.setChartType({name})")));
            }
            let chart = chart_id(s, params)?;
            command(s, "chart.changeType", json!({"chart": chart, "type": kind, "subtype": subtype}))?;
            Ok(Json::Null)
        }
        "Chart.getRange" => Ok(chart_info(s, params)?["source"].clone()),
        "Chart.getName" => Ok(chart_info(s, params)?["name"].clone()),
        "Chart.setName" => {
            let chart = chart_id(s, params)?;
            let name = member_str(params, "name")?;
            command(s, "chart.set", json!({"chart": chart, "title": name}))?;
            Ok(Json::Null)
        }
        "Chart.getPosition" => {
            let info = chart_info(s, params)?;
            Ok(json!({"left": info["left"], "top": info["top"], "width": info["width"], "height": info["height"]}))
        }
        "Chart.setPosition" => {
            let chart = chart_id(s, params)?;
            let mut p = Map::new();
            p.insert("chart".into(), json!(chart));
            if let Some(at) = params.get("at") {
                p.insert("at".into(), at.clone());
            }
            if let Some(left) = params.get("left").and_then(Json::as_f64) {
                p.insert("dx".into(), json!(left));
            }
            if let Some(top) = params.get("top").and_then(Json::as_f64) {
                p.insert("dy".into(), json!(top));
            }
            for key in ["width", "height"] {
                if let Some(v) = params.get(key) {
                    p.insert(key.into(), v.clone());
                }
            }
            command(s, "chart.set", Json::Object(p))?;
            Ok(Json::Null)
        }
        "Chart.getLeft" | "Chart.getTop" | "Chart.getWidth" | "Chart.getHeight" => {
            let info = chart_info(s, params)?;
            let key = match method {
                "Chart.getLeft" => "left",
                "Chart.getTop" => "top",
                "Chart.getWidth" => "width",
                _ => "height",
            };
            Ok(info[key].clone())
        }
        "Chart.setLeft" | "Chart.setTop" | "Chart.setWidth" | "Chart.setHeight" => {
            let chart = chart_id(s, params)?;
            let value = params.get("value").and_then(Json::as_f64).ok_or_else(|| BridgeError::invalid_argument("this setter takes a number"))?;
            let key = match method {
                "Chart.setLeft" => "dx",
                "Chart.setTop" => "dy",
                "Chart.setWidth" => "width",
                _ => "height",
            };
            command(s, "chart.set", json!({"chart": chart, key: value}))?;
            Ok(Json::Null)
        }
        "Chart.delete" => {
            let chart = chart_id(s, params)?;
            command(s, "chart.delete", json!({"chart": chart}))?;
            Ok(Json::Null)
        }

        // ---------------------------------------------------------------- Comment
        "Comment.getContent" | "Comment.getAuthorName" | "Comment.getVisible" => comment_get(s, method, params),
        "Comment.setContent" => {
            let (sheet, cell) = checked_cell(s, params, "cell")?;
            let text = member_str(params, "text")?;
            command(s, "review.newComment", json!({"sheet": sheet, "cell": cell.a1(), "text": text}))?;
            Ok(Json::Null)
        }
        "Comment.delete" => {
            let (sheet, cell) = checked_cell(s, params, "cell")?;
            command(s, "review.deleteComment", json!({"sheet": sheet, "cell": cell.a1()}))?;
            Ok(Json::Null)
        }

        // ---------------------------------------------------------------- DataValidation
        "DataValidation.getRule"
        | "DataValidation.getType"
        | "DataValidation.getErrorAlert"
        | "DataValidation.getPrompt"
        | "DataValidation.getIgnoreBlanks" => {
            let (sheet, cell) = checked_cell(s, params, "cell")?;
            let rule = command(s, "script.validation", json!({"sheet": sheet, "cell": cell.a1()}))?;
            if rule.is_null() {
                return Ok(Json::Null);
            }
            Ok(match method {
                "DataValidation.getType" => rule["type"].clone(),
                "DataValidation.getIgnoreBlanks" => rule["allowBlank"].clone(),
                "DataValidation.getErrorAlert" => json!({
                    "message": rule["errorMessage"],
                    "style": rule["errorStyle"],
                    "title": rule["errorTitle"],
                }),
                "DataValidation.getPrompt" => json!({
                    "message": rule["inputMessage"],
                    "showPrompt": rule["showInput"],
                    "title": rule["inputTitle"],
                }),
                _ => json!({
                    "type": rule["type"],
                    "operator": rule["operator"],
                    "formula1": rule["formula1"],
                    "formula2": rule["formula2"],
                    "allowBlank": rule["allowBlank"],
                    "errorStyle": rule["errorStyle"],
                    "errorMessage": rule["errorMessage"],
                }),
            })
        }
        "DataValidation.setErrorAlert" => {
            let alert = params.get("alert").and_then(Json::as_object).cloned().unwrap_or_default();
            let (sheet, cell) = checked_cell(s, params, "cell")?;
            let mut p = validation_rule(s, sheet, cell)?;
            for (from, to) in [("message", "errorMessage"), ("title", "errorTitle"), ("style", "errorStyle")] {
                if let Some(v) = alert.get(from) {
                    p.insert(to.into(), v.clone());
                }
            }
            command(s, "data.validation", Json::Object(p))?;
            Ok(Json::Null)
        }
        "DataValidation.setPrompt" => {
            let prompt = params.get("prompt").and_then(Json::as_object).cloned().unwrap_or_default();
            let (sheet, cell) = checked_cell(s, params, "cell")?;
            let mut p = validation_rule(s, sheet, cell)?;
            for (from, to) in [("message", "inputMessage"), ("title", "inputTitle")] {
                if let Some(v) = prompt.get(from) {
                    p.insert(to.into(), v.clone());
                }
            }
            command(s, "data.validation", Json::Object(p))?;
            Ok(Json::Null)
        }
        "DataValidation.setIgnoreBlanks" => {
            let (sheet, cell) = checked_cell(s, params, "cell")?;
            let mut p = validation_rule(s, sheet, cell)?;
            p.insert("allowBlank".into(), params.get("ignoreBlanks").cloned().unwrap_or(Json::Bool(true)));
            command(s, "data.validation", Json::Object(p))?;
            Ok(Json::Null)
        }
        "DataValidation.setRule" => validation_set(s, params),
        "DataValidation.delete" | "DataValidation.clear" => {
            let (sheet, cell) = checked_cell(s, params, "cell")?;
            command(s, "data.validation", json!({"sheet": sheet, "cell": cell.a1(), "clear": true}))?;
            Ok(Json::Null)
        }

        // ---------------------------------------------------------------- RangeAreas
        "RangeAreas.getAreas" => {
            let areas = member_array(params, "areas")?;
            let sheet = sheet_name(s, params)?;
            Ok(Json::Array(areas.iter().map(|a| json!({"sheet": sheet, "address": a})).collect()))
        }
        "RangeAreas.getAddress" => Ok(Json::String(member_array(params, "areas")?.join(","))),
        "RangeAreas.getFormat" => {
            let first = member_array(params, "areas")?.first().cloned().unwrap_or_else(|| "A1".into());
            let sheet = sheet_name(s, params)?;
            Ok(json!({"kind": "format", "sheet": sheet, "range": first}))
        }

        // ---------------------------------------------------------------- console
        "console.log" => Ok(Json::Null),
        other => Err(BridgeError::not_supported(other)),
    }
}

fn command(s: &mut Session, id: &str, params: Json) -> Result<Json, BridgeError> {
    s.execute(id, params).map_err(|e| BridgeError::engine(e.to_string()))
}

fn member_str(params: &Json, key: &str) -> Result<String, BridgeError> {
    params.get(key).and_then(Json::as_str).map(str::to_string).ok_or_else(|| BridgeError::invalid_argument(format!("missing `{key}`")))
}

fn member_bool(params: &Json, key: &str) -> Result<bool, BridgeError> {
    params.get(key).and_then(Json::as_bool).ok_or_else(|| BridgeError::invalid_argument(format!("`{key}` must be true or false")))
}

fn member_u32(params: &Json, key: &str) -> Result<u32, BridgeError> {
    params
        .get(key)
        .and_then(Json::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| BridgeError::invalid_argument(format!("`{key}` must be a whole number from 0")))
}

fn member_array(params: &Json, key: &str) -> Result<Vec<String>, BridgeError> {
    let items = params.get(key).and_then(Json::as_array).ok_or_else(|| BridgeError::invalid_argument(format!("`{key}` must be a list")))?;
    items.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| BridgeError::invalid_argument(format!("`{key}` holds text")))).collect()
}

/// The range in the call, parsed (the address is separate from the worksheet).
fn parse_range(params: &Json) -> Result<RangeRef, BridgeError> {
    let text = params.get("range").and_then(Json::as_str).ok_or_else(|| BridgeError::invalid_argument("this call needs a `range`"))?;
    RangeRef::parse(text).ok_or_else(|| BridgeError::invalid_argument(format!("`{text}` is not a range")))
}

/// The call's range checked against the workbook (the sheet exists, the address parses there).
fn checked_range(s: &mut Session, params: &Json) -> Result<(String, RangeRef), BridgeError> {
    let mut p = Map::new();
    if let Some(sheet) = params.get("sheet") {
        p.insert("sheet".into(), sheet.clone());
    }
    p.insert("range".into(), Json::String(parse_range(params)?.a1()));
    let range = cmd::target_range(s, &Json::Object(p)).map_err(|e| BridgeError::engine(e.to_string()))?;
    Ok((sheet_name(s, params)?, range))
}

fn checked_cell(s: &mut Session, params: &Json, key: &str) -> Result<(String, CellRef), BridgeError> {
    let sheet = sheet_name(s, params)?;
    let text = params.get(key).and_then(Json::as_str).ok_or_else(|| BridgeError::invalid_argument(format!("this call needs a `{key}`")))?;
    let cell = RangeRef::parse(text).map(|r| r.start).ok_or_else(|| BridgeError::invalid_argument(format!("`{text}` is not a cell")))?;
    let _ = s;
    Ok((sheet, cell))
}

/// The worksheet the call names, or the active one.
fn sheet_name(s: &mut Session, params: &Json) -> Result<String, BridgeError> {
    let mut p = Map::new();
    if let Some(sheet) = params.get("sheet") {
        p.insert("sheet".into(), sheet.clone());
    }
    let index = cmd::target_sheet(s, &Json::Object(p)).map_err(|e| BridgeError::engine(e.to_string()))?;
    let d = s.doc().map_err(|e| BridgeError::engine(e.to_string()))?;
    d.wb.sheet(index).map(|sh| sh.name.clone()).ok_or_else(|| BridgeError::engine("no such worksheet"))
}

fn range_info(s: &mut Session, params: &Json) -> Result<Json, BridgeError> {
    let mut p = Map::new();
    if let Some(sheet) = params.get("sheet") {
        p.insert("sheet".into(), sheet.clone());
    }
    p.insert("range".into(), Json::String(parse_range(params)?.a1()));
    command(s, "script.rangeInfo", Json::Object(p))
}

/// The top-left cell's value or text from a `script.rangeInfo` reply, empty cells as `""`
/// (ExcelScript never returns null for a cell).
fn cell_value(info: &Json, key: &str) -> Json {
    text_or_empty(info.get(key).and_then(|a| a.get(0)).and_then(|r| r.get(0)).cloned().unwrap_or(Json::Null))
}

fn text_or_empty(value: Json) -> Json {
    match value {
        Json::Null => Json::String(String::new()),
        Json::Array(rows) => Json::Array(rows.into_iter().map(text_or_empty).collect()),
        other => other,
    }
}

fn style_field(info: &Json, path: &[&str]) -> Json {
    let mut v = info;
    for key in path {
        v = match v.get(*key) {
            Some(next) => next,
            None => return Json::Null,
        };
    }
    v.clone()
}

fn font_get(s: &mut Session, method: &str, params: &Json) -> Result<Json, BridgeError> {
    let info = range_info(s, params)?;
    let key = match method {
        "RangeFont.getBold" => &["style", "font", "bold"][..],
        "RangeFont.getItalic" => &["style", "font", "italic"][..],
        "RangeFont.getUnderline" => &["style", "font", "underline"][..],
        "RangeFont.getSize" => &["style", "font", "size"][..],
        "RangeFont.getName" => &["style", "font", "name"][..],
        "RangeFont.getColor" => &["style", "font", "color"][..],
        "RangeFont.getStrikethrough" => &["style", "font", "strikethrough"][..],
        "RangeFormat.getHorizontalAlignment" => &["style", "horizontalAlignment"][..],
        _ => &["style", "verticalAlignment"][..],
    };
    Ok(style_field(&info, key))
}

fn border_get(s: &mut Session, method: &str, params: &Json) -> Result<Json, BridgeError> {
    let info = range_info(s, params)?;
    let side = member_str(params, "side")?.to_ascii_lowercase();
    let side = border_edge(side)?;
    let key = match method {
        "RangeBorder.getColor" => "color",
        "RangeBorder.getStyle" => "style",
        "RangeBorder.getWeight" => "weight",
        _ => "side",
    };
    if key == "side" {
        return Ok(Json::String(side));
    }
    Ok(style_field(&info, &["style", "borders", side.as_str(), key]))
}

fn border_edge(side: String) -> Result<String, BridgeError> {
    Ok(match side.as_str() {
        "top" | "edgetop" => "top",
        "bottom" | "edgebottom" => "bottom",
        "left" | "edgeleft" => "left",
        "right" | "edgeright" => "right",
        "diagonaldown" => "diagonalDown",
        "diagonalup" => "diagonalUp",
        other => return Err(BridgeError::not_supported(format!("RangeBorder side `{other}`"))),
    }
    .to_string())
}

/// Maps the ExcelScript border line style names to the engine's.
fn script_border_style(style: &str) -> String {
    match style.to_ascii_lowercase().as_str() {
        "continuous" => "thin",
        "dash" => "dashed",
        "dot" => "dotted",
        "dashdot" => "dashDot",
        "dashdotdot" => "dashDotDot",
        "slantdashdot" => "slantDashDot",
        "double" => "double",
        "none" => "none",
        other => other,
    }
    .to_string()
}

fn weight_style(weight: &str, style: Option<&str>) -> String {
    let already = style.map(script_border_style).unwrap_or_else(|| "thin".into());
    match weight.to_ascii_lowercase().as_str() {
        "hairline" => "hair".into(),
        "medium" => "medium".into(),
        "thick" => "thick".into(),
        "thin" => "thin".into(),
        _ => already,
    }
}

fn range_set(s: &mut Session, method: &str, params: &Json) -> Result<Json, BridgeError> {
    let sheet = sheet_name(s, params)?;
    let range = parse_range(params)?.a1();
    match method {
        "Range.setValue" | "Range.setValues" => {
            let key = if method == "Range.setValue" { "value" } else { "values" };
            let value = params.get(key).cloned().ok_or_else(|| BridgeError::invalid_argument(format!("missing `{key}`")))?;
            let values = if method == "Range.setValue" { json!([[value]]) } else { value };
            if !values.is_array() {
                return Err(BridgeError::invalid_argument("setValues takes rows of values"));
            }
            command(s, "range.setValues", json!({"sheet": sheet, "range": range, "values": values, "parse": false}))?;
        }
        "Range.setFormula" | "Range.setFormulas" => {
            let key = if method == "Range.setFormula" { "formula" } else { "formulas" };
            let value = params.get(key).cloned().ok_or_else(|| BridgeError::invalid_argument(format!("missing `{key}`")))?;
            let values = if method == "Range.setFormula" {
                json!([[formula(value.as_str().ok_or_else(|| BridgeError::invalid_argument("setFormula takes text"))?)]])
            } else {
                let rows = value.as_array().ok_or_else(|| BridgeError::invalid_argument("setFormulas takes rows of formulas"))?;
                let mut out = Vec::new();
                for row in rows {
                    let row = row.as_array().ok_or_else(|| BridgeError::invalid_argument("setFormulas takes rows of formulas"))?;
                    out.push(Json::Array(
                        row.iter()
                            .map(|v| v.as_str().map(formula).map(Json::String).ok_or_else(|| BridgeError::invalid_argument("a formula must be text")))
                            .collect::<Result<Vec<_>, _>>()?,
                    ));
                }
                Json::Array(out)
            };
            command(s, "range.setValues", json!({"sheet": sheet, "range": range, "values": values, "parse": false}))?;
        }
        _ => {
            // setText / setTexts: the text exactly as given.
            let key = if method == "Range.setText" { "text" } else { "texts" };
            let value = params.get(key).cloned().ok_or_else(|| BridgeError::invalid_argument(format!("missing `{key}`")))?;
            let values =
                if method == "Range.setText" { json!([[value.as_str().map_or(value.clone(), |s| Json::String(s.to_string()))]]) } else { value };
            if !values.is_array() {
                return Err(BridgeError::invalid_argument("setTexts takes rows of text"));
            }
            command(s, "range.setValues", json!({"sheet": sheet, "range": range, "values": values, "parse": false}))?;
        }
    }
    Ok(Json::Null)
}

fn formula(text: &str) -> String {
    if text.starts_with('=') { text.to_string() } else { format!("={text}") }
}

fn table_info(s: &mut Session, params: &Json) -> Result<Json, BridgeError> {
    let name = member_str(params, "table")?;
    let tables = command(s, "script.tables", json!({}))?;
    tables
        .as_array()
        .and_then(|a| a.iter().find(|t| t["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&name))))
        .cloned()
        .ok_or_else(|| BridgeError::invalid_argument(format!("there is no table called `{name}`")))
}

fn table_column_info(s: &mut Session, params: &Json) -> Result<(Json, Json), BridgeError> {
    let table = table_info(s, params)?;
    let name = member_str(params, "column")?;
    let column = table["columns"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&name))))
        .cloned()
        .ok_or_else(|| BridgeError::invalid_argument(format!("the table has no column called `{name}`")))?;
    Ok((table, column))
}

fn tables_row_range(table: &Json, header: bool) -> Json {
    let key = if header { "headerRow" } else { "totalsRow" };
    if table[key].as_bool() != Some(true) {
        return Json::Null;
    }
    let Some(range) = table["range"].as_str().and_then(RangeRef::parse) else { return Json::Null };
    let row = if header { range.start.row } else { range.end.row };
    Json::String(RangeRef::rows(row, row).a1())
}

fn data_body_range(table: &Json) -> Json {
    let Some(range) = table["range"].as_str().and_then(RangeRef::parse) else { return Json::Null };
    let start = range.start.row.saturating_add(u32::from(table["headerRow"].as_bool() == Some(true)));
    let end = range.end.row.saturating_sub(u32::from(table["totalsRow"].as_bool() == Some(true)));
    if start > end {
        return Json::Null;
    }
    Json::String(RangeRef::rows(start, end).a1())
}

/// ExcelScript `DataValidationType` → the engine's type name.
fn validation_type(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "wholenumber" => "whole".into(),
        "none" => "any".into(),
        "textlength" => "textLength".into(),
        "custom" => "custom".into(),
        "date" => "date".into(),
        "time" => "time".into(),
        "decimal" => "decimal".into(),
        "list" => "list".into(),
        other => other.to_string(),
    }
}

/// ExcelScript `DataValidationOperator` → the engine's operator name.
fn validation_operator(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "equalto" => "equal".into(),
        "notequalto" => "notEqual".into(),
        "greaterthan" => "greater".into(),
        "lessthan" => "less".into(),
        "greaterthanorequalto" => "greaterOrEqual".into(),
        "lessthanorequalto" => "lessOrEqual".into(),
        "notbetween" => "notBetween".into(),
        "between" => "between".into(),
        other => other.to_string(),
    }
}

/// Whether the name is a ChartType the engine knows (so an invented one is refused).
fn chart_name_known(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "columnclustered"
            | "columnstacked"
            | "columnstacked100"
            | "barclustered"
            | "barstacked"
            | "barstacked100"
            | "line"
            | "linemarkers"
            | "linestacked"
            | "linemarkersstacked"
            | "linestacked100"
            | "linemarkersstacked100"
            | "pie"
            | "pieexploded"
            | "pieofpie"
            | "barofpie"
            | "doughnut"
            | "doughnutexploded"
            | "area"
            | "areastacked"
            | "areastacked100"
            | "scatter"
            | "scatterlines"
            | "bubble"
            | "bubble3deffect"
            | "radar"
            | "histogram"
            | "pareto"
            | "waterfall"
            | "funnel"
            | "treemap"
            | "sunburst"
            | "boxwhisker"
            | "stock"
    )
}

fn chart_info(s: &mut Session, params: &Json) -> Result<Json, BridgeError> {
    let id = member_u32(params, "id")?;
    let charts = command(s, "script.charts", json!({}))?;
    charts
        .as_array()
        .and_then(|a| a.iter().find(|c| c["id"].as_u64() == Some(u64::from(id))))
        .cloned()
        .ok_or_else(|| BridgeError::invalid_argument(format!("there is no chart with id {id}")))
}

fn chart_id(s: &mut Session, params: &Json) -> Result<u32, BridgeError> {
    Ok(chart_info(s, params)?["id"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0))
}

fn comment_get(s: &mut Session, method: &str, params: &Json) -> Result<Json, BridgeError> {
    let (sheet, cell) = checked_cell(s, params, "cell")?;
    let info = command(s, "cell.get", json!({"sheet": sheet, "cell": cell.a1()}))?;
    let comment = info.get("comment").cloned().unwrap_or(Json::Null);
    if comment.is_null() {
        return Err(BridgeError::invalid_argument("there is no comment on that cell"));
    }
    Ok(match method {
        "Comment.getContent" => comment.get("text").cloned().unwrap_or(Json::Null),
        "Comment.getAuthorName" => comment.get("author").cloned().unwrap_or(Json::Null),
        _ => Json::Bool(comment.get("visible").and_then(Json::as_bool).unwrap_or(false)),
    })
}

/// The current rule of a cell as `data.validation` parameters, for a setter that changes one part.
fn validation_rule(s: &mut Session, sheet: String, cell: CellRef) -> Result<Map<String, Json>, BridgeError> {
    let rule = command(s, "script.validation", json!({"sheet": sheet, "cell": cell.a1()}))?;
    let mut p = Map::new();
    p.insert("sheet".into(), Json::String(sheet));
    p.insert("range".into(), Json::String(cell.a1()));
    if let Json::Object(fields) = rule {
        for key in [
            "type",
            "operator",
            "formula1",
            "formula2",
            "allowBlank",
            "errorStyle",
            "errorMessage",
            "errorTitle",
            "inputMessage",
            "inputTitle",
            "showError",
            "showInput",
            "inCellDropdown",
        ] {
            if let Some(v) = fields.get(key) {
                p.insert(key.into(), v.clone());
            }
        }
    }
    Ok(p)
}

fn validation_set(s: &mut Session, params: &Json) -> Result<Json, BridgeError> {
    let (sheet, cell) = checked_cell(s, params, "cell")?;
    let rule = params.get("rule").and_then(Json::as_object).ok_or_else(|| BridgeError::invalid_argument("setRule takes a rule object"))?;
    let mut p = Map::new();
    p.insert("sheet".into(), Json::String(sheet));
    p.insert("range".into(), Json::String(cell.a1()));
    for (key, value) in rule {
        let value = match key.as_str() {
            "type" => value.as_str().map(validation_type).map(Json::String).unwrap_or_else(|| value.clone()),
            "operator" => value.as_str().map(validation_operator).map(Json::String).unwrap_or_else(|| value.clone()),
            "errorStyle" => value.as_str().map(|s| Json::String(s.to_ascii_lowercase())).unwrap_or_else(|| value.clone()),
            _ => value.clone(),
        };
        p.insert(key.clone(), value);
    }
    command(s, "data.validation", Json::Object(p))?;
    Ok(Json::Null)
}

/// The ExcelScript `ChartType` name for a model kind (names only; the reference is the source of
/// the names).
pub(crate) fn chart_type_name(kind: ChartKind) -> &'static str {
    match kind {
        ChartKind::ColumnClustered => "columnClustered",
        ChartKind::ColumnStacked => "columnStacked",
        ChartKind::ColumnStacked100 => "columnStacked100",
        ChartKind::BarClustered => "barClustered",
        ChartKind::BarStacked => "barStacked",
        ChartKind::BarStacked100 => "barStacked100",
        ChartKind::Line => "line",
        ChartKind::LineMarkers => "lineMarkers",
        ChartKind::LineStacked => "lineStacked",
        ChartKind::Pie => "pie",
        ChartKind::Doughnut => "doughnut",
        ChartKind::Area => "area",
        ChartKind::AreaStacked => "areaStacked",
        ChartKind::AreaStacked100 => "areaStacked100",
        ChartKind::Scatter => "scatter",
        ChartKind::ScatterLines => "scatterLines",
        ChartKind::Bubble => "bubble",
        ChartKind::Radar => "radar",
        ChartKind::Histogram => "histogram",
        ChartKind::Waterfall => "waterfall",
        ChartKind::Funnel => "funnel",
        ChartKind::Treemap => "treemap",
        ChartKind::Sunburst => "sunburst",
        ChartKind::BoxWhisker => "boxwhisker",
        ChartKind::Stock => "stock",
        // ExcelScript has no combo chart type; the column form is the closest single kind.
        ChartKind::Combo => "columnClustered",
    }
}

/// The engine's `type`/`subtype` for an ExcelScript `ChartType` name.
fn chart_kind_from_name(name: &str) -> (&'static str, &'static str) {
    match name.to_ascii_lowercase().as_str() {
        "columnclustered" => ("column", ""),
        "columnstacked" => ("column", "stacked"),
        "columnstacked100" => ("column", "stacked100"),
        "barclustered" => ("bar", ""),
        "barstacked" => ("bar", "stacked"),
        "barstacked100" => ("bar", "stacked100"),
        "line" => ("line", ""),
        "linemarkers" => ("line", "markers"),
        "linestacked" | "linemarkersstacked" => ("line", "stacked"),
        "lineStacked100" | "linemarkersstacked100" => ("line", "stacked100"),
        "pie" | "pieexploded" | "pieofpie" | "barofpie" | "doughnutexploded" => ("pie", ""),
        "doughnut" => ("doughnut", ""),
        "area" => ("area", ""),
        "areastacked" => ("area", "stacked"),
        "areastacked100" => ("area", "stacked100"),
        "scatter" => ("scatter", ""),
        "scatterlines" => ("scatter", "lines"),
        "bubble" | "bubble3deffect" => ("bubble", ""),
        "radar" => ("radar", ""),
        "histogram" | "pareto" => ("histogram", ""),
        "waterfall" => ("waterfall", ""),
        "funnel" => ("funnel", ""),
        "treemap" => ("treemap", ""),
        "sunburst" => ("sunburst", ""),
        "boxwhisker" => ("boxWhisker", ""),
        "stock" => ("stock", ""),
        _ => ("column", ""),
    }
}

/// The bridge executor for one run: it holds the session the run may edit.
pub(crate) struct SessionBridge<'a> {
    pub session: &'a mut Session,
}

impl gridcraft_script::BridgeExecutor for SessionBridge<'_> {
    fn execute(&mut self, call: &gridcraft_script::BridgeCall) -> Result<Json, BridgeError> {
        execute_call(self.session, &call.method, &call.params)
    }
}

/// The parameters a recorded journal entry should carry to the recorder: a numeric `sheet` is
/// replaced by the sheet's name, because a script cannot know a sheet's index at replay time.
/// An index the workbook no longer has is dropped, leaving the active sheet, as the command did.
pub(crate) fn resolve_recorded_sheet(s: &Session, params: &Json) -> Json {
    let Some(index) = params.get("sheet").and_then(Json::as_u64) else {
        return params.clone();
    };
    let mut out = params.clone();
    if let Json::Object(map) = &mut out {
        let name = s.doc().ok().and_then(|d| d.wb.sheets.get(index as usize)).map(|sh| sh.name.clone());
        match name {
            Some(name) => {
                map.insert("sheet".into(), Json::String(name));
            }
            None => {
                map.remove("sheet");
            }
        }
    }
    out
}

/// `console.log` lines the runtime collected, capped to keep a runaway script from filling the
/// pane. The host caps bytes already; this caps the number of lines the result carries.
pub(crate) fn capped_output(mut output: Vec<String>) -> Vec<String> {
    const MAX_LINES: usize = 200;
    output.truncate(MAX_LINES);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_get_one_equals_sign() {
        assert_eq!(formula("SUM(A1:A2)"), "=SUM(A1:A2)");
        assert_eq!(formula("=SUM(A1:A2)"), "=SUM(A1:A2)");
    }

    #[test]
    fn table_row_ranges_follow_the_flags() {
        let table = json!({"range": "A1:C5", "headerRow": true, "totalsRow": true});
        assert_eq!(tables_row_range(&table, true), Json::String("1:1".into()));
        assert_eq!(tables_row_range(&table, false), Json::String("5:5".into()));
        assert_eq!(data_body_range(&table), Json::String("2:4".into()));
        let plain = json!({"range": "A1:C5", "headerRow": false, "totalsRow": false});
        assert_eq!(data_body_range(&plain), Json::String("1:5".into()));
    }

    #[test]
    fn chart_names_round_trip_for_the_core_kinds() {
        for kind in [ChartKind::ColumnClustered, ChartKind::LineMarkers, ChartKind::Pie, ChartKind::Doughnut, ChartKind::ScatterLines] {
            let name = chart_type_name(kind);
            let (t, sub) = chart_kind_from_name(name);
            assert_eq!(crate::cmd::insert::chart_kind(t, sub), kind, "{name}");
        }
    }
}
